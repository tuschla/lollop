package app.milestone

import android.Manifest
import android.annotation.SuppressLint
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.location.Location
import android.location.LocationListener
import android.location.LocationManager
import android.os.Build
import android.os.Bundle
import android.os.IBinder
import android.os.Looper
import java.util.Locale
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import com.google.android.gms.common.ConnectionResult
import com.google.android.gms.common.GoogleApiAvailability
import com.google.android.gms.location.FusedLocationProviderClient
import com.google.android.gms.location.LocationCallback
import com.google.android.gms.location.LocationRequest
import com.google.android.gms.location.LocationResult
import com.google.android.gms.location.LocationServices
import com.google.android.gms.location.Priority

/**
 * Foreground service that streams location fixes into [RunSession] with an
 * ongoing notification. Because it runs as a `location`-typed foreground service
 * - not a Composable's callback: tracking continues when the screen turns off
 * or the app is backgrounded. Started/stopped from the UI while it is visible,
 * so foreground-location permission is sufficient (no background-location grant).
 *
 * The fix source is chosen at runtime ([locationEngine]): Google's fused provider
 * where Play Services is present, otherwise the platform [LocationManager] GPS
 * provider. So the app keeps tracking on de-Googled ROMs (GrapheneOS, /e/OS, no
 * GMS) instead of silently receiving no fixes from a dead fused client.
 */
class RunTrackingService : Service() {

    private lateinit var engine: LocationEngine

    // Locate-only vs recording. Opening the tracker starts the
    // engine to preview fix quality, but records NOTHING until the user taps
    // Start; no route point, no crash-durable sidecar line. `recording` flips
    // true on ACTION_START.
    private var recording = false

    // Set once the service is tearing down (stopTracking / onDestroy). A location
    // result already queued on the main looper can still fire onFix AFTER
    // stopForeground; without this guard it would re-post the ongoing notification
    // that nothing then cancels, leaving a stuck "Getting a GPS fix".
    @Volatile private var stopped = false

    // The last notification content string actually posted, so [notifyIfChanged]
    // re-posts ONLY when the displayed line changes rather than rebuilding + notify()
    // on every ~1 Hz fix. While recording it changes ~once/sec anyway (elapsed
    // ticks), but a paused/standing-still stretch, and every locate-preview fix,
    // whose notification is static, then costs nothing.
    private var lastNotifiedContent: String? = null

    // Screen-off power state: the recording GPS request drops to a coarser
    // cadence while the screen is off (see [recordIntervalMs]); still finer than the
    // core's ~2–3 s save-time decimation, so the of-record run is unaffected while
    // the idle-screen battery cost falls. Only meaningful while recording.
    @Volatile private var screenOff = false
    private var screenReceiver: BroadcastReceiver? = null

    /** The notification's current content line, recording progress, or the static
     *  locate-preview text. The throttle key for [notifyIfChanged]. */
    private fun currentNotifContent(): String =
        if (recording) progressText() else NOTIF_LOCATE_TEXT

    /** Re-post the ongoing notification only when its content actually changed.
     *  No-op once [stopped] so a late queued fix can't resurrect it. */
    private fun notifyIfChanged() {
        if (stopped) return
        val content = currentNotifContent()
        if (content == lastNotifiedContent) return
        lastNotifiedContent = content
        getSystemService(NotificationManager::class.java).notify(NOTIF_ID, buildNotification())
    }

    /** One fix → preview state always; a route point + sidecar line only while
     *  recording. Shared by both engines. */
    private fun onFix(loc: Location) {
        // Post-stop queued fix: do nothing, and never re-post the notification.
        if (stopped) return
        // Gate implausible fixes for the LIVE display + map: the fused provider's
        // first callback is often a stale cached location (many minutes old, far
        // away) that would draw a giant phantom leg and inflate live distance; a
        // very-low-accuracy fix is likewise noise. The core's QC still gates the
        // of-record figures, but these must not reach the live UX at all.
        if (!isPlausibleLiveFix(loc)) return
        val point = GpsPoint(
            lat = loc.latitude,
            lon = loc.longitude,
            observedAt = loc.time / 1000,
            accuracyM = loc.accuracy.toDouble(),
        )
        // Preview: drives the "GPS lock · ±N m" readout, the self-location dot
        // before Start, and enables the Start button. Set on EVERY accepted fix,
        // recording or not.
        RunSession.lastFix.value = point
        // Locate-only: nothing is recorded until the user taps Start. The preview
        // notification is static, so [notifyIfChanged] re-posts it at most once.
        if (!recording) {
            notifyIfChanged()
            return
        }
        // Paused (stat-sheet control): drop the fix so the paused span records
        // no route; the service stays foregrounded so the run remains live. Mark a
        // resume boundary so the NEXT accepted fix opens a new segment; a pause +
        // relocation must not bridge the displacement into distance.
        if (RunSession.paused.value) {
            RunSession.markResumeBoundary()
            return
        }
        RunSession.add(point, loc.elapsedRealtimeNanos)
        // Persist to the crash-durable sidecar the instant the fix is accepted, so
        // a process/service kill mid-run is recoverable (off the main thread).
        ActiveRunStore.append(point)
        // Refresh the ongoing notification so a glance at the lockscreen confirms
        // GPS is still capturing. Re-notify with the same NOTIF_ID updates the
        // existing notification in place; IMPORTANCE_LOW keeps it silent every fix.
        // Throttled to only re-post when the displayed line (elapsed + distance)
        // actually changed; a paused/standing-still stretch costs nothing.
        notifyIfChanged()
    }

    override fun onCreate() {
        super.onCreate()
        ActiveRunStore.init(this)
        engine = if (hasPlayServices(this)) FusedEngine(this) else PlatformEngine(this)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            getSystemService(NotificationManager::class.java).createNotificationChannel(
                NotificationChannel(CHANNEL, "Run tracking", NotificationManager.IMPORTANCE_LOW)
            )
        }
        // Screen on/off drives the GPS cadence: while the screen is off the
        // engine drops to a coarser interval; still finer than the core's save-time
        // decimation, so the recorded route is unaffected while idle battery drops.
        // SCREEN_ON/OFF are protected system broadcasts (dynamic registration only,
        // no permission). No-op unless a stream is running (the engine guards it).
        val receiver = object : BroadcastReceiver() {
            override fun onReceive(c: Context?, i: Intent?) {
                when (i?.action) {
                    Intent.ACTION_SCREEN_OFF -> { screenOff = true; engine.setBackground(true) }
                    Intent.ACTION_SCREEN_ON -> { screenOff = false; engine.setBackground(false) }
                }
            }
        }
        screenReceiver = receiver
        registerReceiver(
            receiver,
            IntentFilter().apply {
                addAction(Intent.ACTION_SCREEN_OFF)
                addAction(Intent.ACTION_SCREEN_ON)
            },
        )
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onDestroy() {
        // Safety net: if the service is torn down without an ACTION_STOP (e.g. the
        // OS stops the component under memory pressure), unregister so the GPS
        // callback and sensor don't outlive the service. Harmless no-op when
        // stopTracking() already removed it.
        stopped = true
        engine.stop()
        screenReceiver?.let { runCatching { unregisterReceiver(it) } }
        screenReceiver = null
        // Cancel the ongoing notification: stopForeground(REMOVE) may not have run
        // (OS teardown), and a queued onFix could otherwise leave it orphaned.
        getSystemService(NotificationManager::class.java).cancel(NOTIF_ID)
        RunSession.tracking.value = false
        RunSession.locating.value = false
        super.onDestroy()
    }

    override fun onTaskRemoved(rootIntent: Intent?) {
        // User swiped the app from Recents. The foreground service keeps recording,
        // but the OS may now kill the process: flush any queued sidecar writes so
        // nothing already accepted is lost, and DO NOT clear the sidecar: its
        // presence is exactly what lets the run be recovered on next launch.
        ActiveRunStore.flush()
        super.onTaskRemoved(rootIntent)
    }

    /** Reject a fix that must not reach the LIVE display: a stale cached location
     *  (fused hands one back on the first callback) or a very-low-accuracy fix.
     *  The core's own QC still gates the of-record distance/pace on save. */
    private fun isPlausibleLiveFix(loc: Location): Boolean {
        // Freshness: elapsedRealtime age is monotonic (immune to wall-clock skew).
        val ageMs = (android.os.SystemClock.elapsedRealtimeNanos() - loc.elapsedRealtimeNanos) / 1_000_000
        if (ageMs > 10_000) return false
        // No reported accuracy → the fix can't be quality-gated: the core's
        // of-record QC drops it, so recording it live would desync the live
        // map/distance from the saved run. Reject it here to keep them in step.
        if (!loc.hasAccuracy()) return false
        // Accuracy gate (lenient, 50 m keeps normal GPS, drops obvious noise).
        if (loc.accuracy > 50f) return false
        return true
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_LOCATE -> startTracking(record = false)
            ACTION_START -> startTracking(record = true)
            ACTION_STOP -> stopTracking()
        }
        // NOT_STICKY: if the OS kills us mid-run it must not recreate the service
        // with a null intent, that path skips startForeground() and crashes the
        // process on API 26+. A killed run simply ends rather than silently
        // resurrecting without UI/permission context.
        return START_NOT_STICKY
    }

    @SuppressLint("MissingPermission")
    private fun startTracking(record: Boolean) {
        // Locate-only (record == false) previews fix quality; ACTION_START flips
        // to recording. Both arrive via startForegroundService, so both must call
        // startForeground within the deadline below. A LOCATE already foregrounded
        // then re-foregrounds harmlessly on START.
        recording = record
        // A fresh LOCATE/START on a reused service instance clears any prior
        // teardown guard so onFix can post again.
        stopped = false
        // startForegroundService (the caller) arms a ~5 s deadline: this service
        // MUST call startForeground before anything, or the framework kills the
        // process with ForegroundServiceDidNotStartInTimeException. So fulfill the
        // deadline FIRST, before the permission re-check, and only then decide
        // whether to keep running. No path may bail out having armed the deadline
        // without a startForeground call.
        try {
            // API 34+ requires the typed startForeground for a location FGS; the
            // 2-arg overload throws. ServiceCompat picks the right form per API level.
            ServiceCompat.startForeground(
                this,
                NOTIF_ID,
                buildNotification(),
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                    ServiceInfo.FOREGROUND_SERVICE_TYPE_LOCATION
                } else {
                    0
                },
            )
        } catch (_: SecurityException) {
            // API 34+: startForeground for a location FGS itself throws without a
            // location grant. The framework has REJECTED the start here (the
            // deadline is satisfied by its own rejection), so stopping now is safe.
            RunSession.tracking.value = false
            RunSession.locating.value = false
            stopSelf()
            return
        }
        // The startForeground call just posted the notification; seed the throttle
        // key so the first fix doesn't needlessly re-post identical content.
        lastNotifiedContent = currentNotifContent()
        // Foregrounded: now safe to bail if fine location raced away between the
        // UI's check and here (revoking location also downgrades to coarse-only).
        if (!hasFineLocation(this)) {
            RunSession.tracking.value = false
            RunSession.locating.value = false
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
            return
        }
        try {
            // The onError callback fires for an ASYNC start failure the try/catch
            // can't see: the fused client's requestLocationUpdates returns a Task
            // that can fail (a permission race surfacing after the callback was
            // registered) with no exception thrown here: a dropped failure would
            // leave the service foregrounded delivering ZERO fixes. It runs the same
            // clean unwind as the synchronous SecurityException catch below.
            engine.start(::onFix) { unwindLocationFailure() }
            // Sync the freshly-started stream to the current screen state.
            engine.setBackground(screenOff)
            RunSession.locating.value = true
            // Only recording flips `tracking` (Start/Stop UI); locate leaves it
            // false so the Start button stays shown while GPS is acquired.
            RunSession.tracking.value = record
        } catch (_: SecurityException) {
            // Permission raced away between the check above and engine.start.
            unwindLocationFailure()
        }
    }

    /** Clean unwind for a location-start failure (synchronous SecurityException or
     *  the fused client's async Task failure): stop the half-started engine, drop
     *  the foreground state, and self-stop so the service never lingers foregrounded
     *  with no fix stream. Marks [stopped] so a late queued onFix returns without
     *  re-posting the notification, and is idempotent, a second call (e.g. an async
     *  Task failure arriving after a clean stop) short-circuits. */
    private fun unwindLocationFailure() {
        if (stopped) return
        stopped = true
        engine.stop()
        RunSession.tracking.value = false
        RunSession.locating.value = false
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    private fun stopTracking() {
        // Guard against a location result already queued on the main looper: mark
        // stopped BEFORE tearing down so any late onFix returns without re-posting
        // the notification.
        stopped = true
        engine.stop()
        recording = false
        RunSession.tracking.value = false
        RunSession.locating.value = false
        stopForeground(STOP_FOREGROUND_REMOVE)
        // Belt-and-suspenders: explicitly cancel so no "Getting a GPS fix" / "Tracking
        // run" notification can linger if a re-post slipped in before `stopped`.
        getSystemService(NotificationManager::class.java).cancel(NOTIF_ID)
        stopSelf()
    }

    private fun buildNotification(): Notification {
        // Tapping the ongoing notification reopens MainActivity ONTO the live
        // tracking screen (EXTRA_OPEN_TRACKING is consumed by CoachScreen).
        // SINGLE_TOP so an already-running Activity gets onNewIntent instead of
        // a duplicate instance; UPDATE_CURRENT so the extra survives re-notify.
        val tap = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java)
                .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP)
                .putExtra(MainActivity.EXTRA_OPEN_TRACKING, true),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        return NotificationCompat.Builder(this, CHANNEL)
            // One "acquiring" wording everywhere (the map pill + Start button both say
            // "Acquiring GPS"), so the notification no longer reads "Getting a GPS
            // fix" for the same state.
            .setContentTitle(if (recording) "Tracking run" else NOTIF_LOCATE_TITLE)
            .setContentText(if (recording) progressText() else NOTIF_LOCATE_TEXT)
            // 1-color white route mark, silhouette only (icons README table).
            .setSmallIcon(R.drawable.ic_stat_mark)
            .setOngoing(true)
            .setContentIntent(tap)
            .build()
    }

    /**
     * Elapsed time + live distance from the captured track (06-run-tracking
     * §Behavior: "a notification shows elapsed + distance"), or a
     * pre-first-fix hint. The distance is the same factual shell-side
     * haversine value the stat sheet shows while recording.
     */
    private fun progressText(): String {
        val pts = RunSession.points.value
        if (pts.size < 2) return "Recording your route…"
        // Monotonic elapsed (RunSession.elapsedSec), immune to a wall-clock/NTP step
        // mid-run; and the incrementally-accumulated distance (RunSession.distanceKm),
        // so the notification never re-haversines the whole track on a refresh.
        // Honour the user's distance unit (same as the tracking screen / history),
        // so the notification doesn't contradict the rest of the app in km/mi.
        val unit = resolveDistanceUnit(ThemeSettings.distanceUnitOverride.value)
        val dist = metersToDisplay(RunSession.distanceKm.value * 1000.0, unit)
        return "${formatElapsed(RunSession.elapsedSec.value)} · ${String.format(Locale.US, "%.2f", dist)} ${unit.distanceLabel}"
    }

    companion object {
        private const val ACTION_LOCATE = "app.milestone.action.LOCATE"
        private const val ACTION_START = "app.milestone.action.START"
        private const val ACTION_STOP = "app.milestone.action.STOP"
        private const val CHANNEL = "run_tracking"
        private const val NOTIF_ID = 1
        // Locate-preview notification copy, unified with the map pill + Start
        // button ("Acquiring GPS") so one state has one wording.
        private const val NOTIF_LOCATE_TITLE = "Acquiring GPS"
        private const val NOTIF_LOCATE_TEXT = "Tap Start when you're ready to record."

        /**
         * Best-effort last-known fix for centering the map before tracking
         * starts. Caller must already hold a location permission; silently does
         * nothing if none is cached. [onResult] fires on the main thread.
         */
        @SuppressLint("MissingPermission")
        fun lastKnownLocation(ctx: Context, onResult: (lat: Double, lon: Double) -> Unit) {
            if (hasPlayServices(ctx)) {
                LocationServices.getFusedLocationProviderClient(ctx).lastLocation
                    .addOnSuccessListener { loc -> loc?.let { onResult(it.latitude, it.longitude) } }
            } else {
                // No Play Services: read the platform GPS provider's cached fix.
                // GPS_PROVIDER is fine-only: querying it on a coarse-only grant
                // throws SecurityException, so gate on FINE explicitly.
                if (!hasFineLocation(ctx)) return
                val lm = ctx.getSystemService(Context.LOCATION_SERVICE) as LocationManager
                lm.getLastKnownLocation(LocationManager.GPS_PROVIDER)
                    ?.let { onResult(it.latitude, it.longitude) }
            }
        }

        /** Locate-only: acquire GPS to preview fix quality; records nothing until
         *  [start] (explicit-Start). */
        fun locate(ctx: Context) = ContextCompat.startForegroundService(
            ctx,
            Intent(ctx, RunTrackingService::class.java).setAction(ACTION_LOCATE),
        )

        fun start(ctx: Context) = ContextCompat.startForegroundService(
            ctx,
            Intent(ctx, RunTrackingService::class.java).setAction(ACTION_START),
        )

        fun stop(ctx: Context) {
            // The service may already have self-stopped (e.g. Back pressed after a
            // process restart); delivering ACTION_STOP to a dead service throws on
            // API 26+/31+, so swallow that: there is nothing left to stop.
            try {
                ctx.startService(
                    Intent(ctx, RunTrackingService::class.java).setAction(ACTION_STOP),
                )
            } catch (_: IllegalStateException) {
                RunSession.tracking.value = false
            }
        }
    }
}

/** True when Google Play Services is installed and usable, i.e. the fused
 *  provider is worth asking for. False on de-Googled ROMs. */
private fun hasPlayServices(ctx: Context): Boolean =
    GoogleApiAvailability.getInstance().isGooglePlayServicesAvailable(ctx) == ConnectionResult.SUCCESS

/** True only with a full ACCESS_FINE_LOCATION grant. Coarse-only is not enough
 *  for GPS tracking: the platform GPS_PROVIDER throws SecurityException without
 *  FINE, so every GPS code path gates on this, never on "any location grant". */
internal fun hasFineLocation(ctx: Context): Boolean =
    ContextCompat.checkSelfPermission(ctx, Manifest.permission.ACCESS_FINE_LOCATION) ==
        PackageManager.PERMISSION_GRANTED

/** A high-accuracy fix stream, ~1–2 s cadence. [start] delivers each fix on the
 *  main thread and invokes [onError] on an ASYNC start failure the caller's
 *  try/catch can't observe (the fused Task failing); [stop] unregisters and is a
 *  safe no-op if not started. */
private interface LocationEngine {
    fun start(onFix: (Location) -> Unit, onError: (Throwable) -> Unit)
    /** Switch the fix cadence between foreground (screen on) and background
     *  (screen off) rates. A no-op when unchanged or not started. */
    fun setBackground(background: Boolean)
    fun stop()
}

/** Foreground (screen-on) recording cadence: fused request interval / min interval
 *  (ms), and the platform GPS interval (ms). ~1–2 s, full route fidelity. */
private const val FUSED_FG_INTERVAL_MS = 2000L
private const val FUSED_FG_MIN_INTERVAL_MS = 1000L
private const val PLATFORM_FG_INTERVAL_MS = 1000L

/** Background (screen-off) cadence: coarser to save battery, but still finer than
 *  the core's ~2–3 s save-time track decimation ([TRACK_DECIMATION_CAP]), so the
 *  of-record run is unaffected; the deliberate power/fidelity tradeoff. */
private const val FUSED_BG_INTERVAL_MS = 4000L
private const val FUSED_BG_MIN_INTERVAL_MS = 3000L
private const val PLATFORM_BG_INTERVAL_MS = 4000L

/** Google fused provider (Play Services). Batches several fixes per callback
 *  under Doze/screen-off; every batched fix is forwarded so the route stays
 *  continuous rather than collapsing to the newest point. */
private class FusedEngine(ctx: Context) : LocationEngine {
    private val client = LocationServices.getFusedLocationProviderClient(ctx)
    private var callback: LocationCallback? = null
    private var background = false

    @SuppressLint("MissingPermission")
    override fun start(onFix: (Location) -> Unit, onError: (Throwable) -> Unit) {
        // Idempotent: a double ACTION_START must not leak a second callback.
        stop()
        val cb = object : LocationCallback() {
            override fun onLocationResult(result: LocationResult) {
                // Under Doze/screen-off the fused provider batches several fixes
                // per callback, not necessarily in time order: sort so the
                // polyline can't zigzag backwards. Order by elapsedRealtimeNanos
                // (the MONOTONIC boot-timebase), NOT wall-clock `time`: an NTP step
                // mid-batch can reorder fixes sorted by `time`, and that timebase is
                // exactly the one RunSession.add uses for its per-leg dt, so sorting
                // by it keeps live figures and replay honest.
                for (loc in result.locations.sortedBy { it.elapsedRealtimeNanos }) onFix(loc)
            }
        }
        callback = cb
        // The returned Task can fail asynchronously (permission race after the
        // callback registered) with no exception on this thread: observe it so the
        // service doesn't sit foregrounded with zero fixes on a dropped failure.
        request(cb).addOnFailureListener { onError(it) }
    }

    @SuppressLint("MissingPermission")
    override fun setBackground(background: Boolean) {
        if (background == this.background) return
        this.background = background
        // Re-issue with the new cadence on the SAME callback (a repeat
        // requestLocationUpdates replaces the prior request, no second stream);
        // no-op if not started. A mid-run permission revocation surfaces via the
        // service's own location receiver, so a dropped re-request here is harmless.
        val cb = callback ?: return
        runCatching { request(cb) }
    }

    @SuppressLint("MissingPermission")
    private fun request(cb: LocationCallback) =
        client.requestLocationUpdates(
            LocationRequest.Builder(
                Priority.PRIORITY_HIGH_ACCURACY,
                if (background) FUSED_BG_INTERVAL_MS else FUSED_FG_INTERVAL_MS,
            )
                .setMinUpdateIntervalMillis(
                    if (background) FUSED_BG_MIN_INTERVAL_MS else FUSED_FG_MIN_INTERVAL_MS,
                )
                // Never hand back a location older than 10 s: blocks the stale cached
                // first fix that would otherwise draw a giant phantom opening leg.
                .setMaxUpdateAgeMillis(10_000L)
                .build(),
            cb,
            Looper.getMainLooper(),
        )

    override fun stop() {
        callback?.let { client.removeLocationUpdates(it) }
        callback = null
    }
}

/** Platform [LocationManager] GPS provider, raw GNSS, no Google dependency.
 *  The [LocationListener] callbacks are overridden explicitly: their default
 *  implementations only exist on API 30+, so a lambda/partial impl would throw
 *  AbstractMethodError on the minSdk-24 range this app supports. */
private class PlatformEngine(private val ctx: Context) : LocationEngine {
    private val lm = ctx.getSystemService(Context.LOCATION_SERVICE) as LocationManager
    private var listener: LocationListener? = null
    private var background = false

    @SuppressLint("MissingPermission")
    override fun start(onFix: (Location) -> Unit, onError: (Throwable) -> Unit) {
        // Platform LocationManager has no async start Task: requestLocationUpdates
        // throws synchronously on failure (caught by the service's start try), so
        // there is nothing to route through [onError] here.
        // Idempotent: a double ACTION_START must not leak a second listener.
        stop()
        // GPS_PROVIDER needs FINE: a coarse-only grant would throw from inside
        // requestLocationUpdates on de-Googled devices. Throw the same exception
        // eagerly so the service's startTracking catch unwinds one way.
        if (!hasFineLocation(ctx)) {
            throw SecurityException("ACCESS_FINE_LOCATION not granted: GPS provider unavailable")
        }
        val l = object : LocationListener {
            override fun onLocationChanged(location: Location) = onFix(location)
            override fun onProviderEnabled(provider: String) {}
            override fun onProviderDisabled(provider: String) {}
            // Required on API < 30 (abstract there, no default impl); the
            // framework no longer calls it on API 30+, hence @Deprecated.
            @Deprecated("Required on API < 30; no longer called on API 30+.")
            override fun onStatusChanged(provider: String?, status: Int, extras: Bundle?) {
            }
        }
        listener = l
        request(l)
    }

    @SuppressLint("MissingPermission")
    override fun setBackground(background: Boolean) {
        if (background == this.background) return
        this.background = background
        val l = listener ?: return // not started → nothing to re-cadence
        // Re-register at the new interval. Guard the permission race (revoked
        // mid-run): a thrown SecurityException just means no more fixes, which the
        // service's location receiver already surfaces, never crash the process.
        runCatching {
            lm.removeUpdates(l)
            request(l)
        }
    }

    @SuppressLint("MissingPermission")
    private fun request(l: LocationListener) = lm.requestLocationUpdates(
        // 1 s (screen on) / 4 s (screen off): the core decimates as needed.
        LocationManager.GPS_PROVIDER,
        if (background) PLATFORM_BG_INTERVAL_MS else PLATFORM_FG_INTERVAL_MS,
        0f,
        l,
        Looper.getMainLooper(),
    )

    override fun stop() {
        listener?.let { lm.removeUpdates(it) }
        listener = null
    }
}
