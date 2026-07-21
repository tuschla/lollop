package de.tuschla.fitnessanlage

import android.Manifest
import android.annotation.SuppressLint
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.location.Location
import android.location.LocationListener
import android.location.LocationManager
import android.os.Build
import android.os.Bundle
import android.os.IBinder
import android.os.Looper
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

    /** One fix → one route point + a refreshed notification. Shared by both engines. */
    private fun onFix(loc: Location) {
        RunSession.add(
            GpsPoint(
                lat = loc.latitude,
                lon = loc.longitude,
                observedAt = loc.time / 1000,
                accuracyM = if (loc.hasAccuracy()) loc.accuracy.toDouble() else 999.0,
            )
        )
        // Refresh the ongoing notification so a glance at the lockscreen confirms
        // GPS is still capturing. Re-notify with the same NOTIF_ID updates the
        // existing notification in place; IMPORTANCE_LOW keeps it silent every
        // fix. Elapsed + fix count only: route distance stays core-derived.
        getSystemService(NotificationManager::class.java)
            .notify(NOTIF_ID, buildNotification())
    }

    override fun onCreate() {
        super.onCreate()
        engine = if (hasPlayServices(this)) FusedEngine(this) else PlatformEngine(this)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            getSystemService(NotificationManager::class.java).createNotificationChannel(
                NotificationChannel(CHANNEL, "Run tracking", NotificationManager.IMPORTANCE_LOW)
            )
        }
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onDestroy() {
        // Safety net: if the service is torn down without an ACTION_STOP (e.g. the
        // OS stops the component under memory pressure), unregister so the GPS
        // callback and sensor don't outlive the service. Harmless no-op when
        // stopTracking() already removed it.
        engine.stop()
        RunSession.tracking.value = false
        super.onDestroy()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_START -> startTracking()
            ACTION_STOP -> stopTracking()
        }
        // NOT_STICKY: if the OS kills us mid-run it must not recreate the service
        // with a null intent, that path skips startForeground() and crashes the
        // process on API 26+. A killed run simply ends rather than silently
        // resurrecting without UI/permission context.
        return START_NOT_STICKY
    }

    @SuppressLint("MissingPermission")
    private fun startTracking() {
        // Fine location can be revoked (or downgraded to coarse-only) between the
        // UI's permission check and this service actually starting; and on API
        // 34+ startForeground for a location FGS itself throws SecurityException
        // without a location grant. Re-verify here and bail out cleanly instead
        // of crashing the process.
        if (!hasFineLocation(this)) {
            stopSelf()
            return
        }
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
            engine.start(::onFix)
            RunSession.tracking.value = true
        } catch (_: SecurityException) {
            // Permission raced away mid-start (grant revoked while the start
            // intent was in flight). Unwind whatever half-started: unregister the
            // engine (safe no-op if it never registered), drop foreground state,
            // and stop: the UI's tracking flag stays false so Start can re-check.
            engine.stop()
            RunSession.tracking.value = false
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
        }
    }

    private fun stopTracking() {
        engine.stop()
        RunSession.tracking.value = false
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    private fun buildNotification(): Notification {
        val tap = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE,
        )
        return NotificationCompat.Builder(this, CHANNEL)
            .setContentTitle("Tracking run")
            .setContentText(progressText())
            .setSmallIcon(R.drawable.ic_launcher_foreground)
            .setOngoing(true)
            .setContentIntent(tap)
            .build()
    }

    /** Elapsed time + fix count from the captured track, or a pre-first-fix hint. */
    private fun progressText(): String {
        val pts = RunSession.points.value
        if (pts.size < 2) return "Recording your route…"
        val secs = (pts.last().observedAt - pts.first().observedAt).coerceAtLeast(0L)
        return "${formatElapsed(secs)} · ${pts.size} fixes"
    }

    companion object {
        private const val ACTION_START = "de.tuschla.fitnessanlage.action.START"
        private const val ACTION_STOP = "de.tuschla.fitnessanlage.action.STOP"
        private const val CHANNEL = "run_tracking"
        private const val NOTIF_ID = 1

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
 *  main thread; [stop] unregisters and is a safe no-op if not started. */
private interface LocationEngine {
    fun start(onFix: (Location) -> Unit)
    fun stop()
}

/** Google fused provider (Play Services). Batches several fixes per callback
 *  under Doze/screen-off; every batched fix is forwarded so the route stays
 *  continuous rather than collapsing to the newest point. */
private class FusedEngine(ctx: Context) : LocationEngine {
    private val client = LocationServices.getFusedLocationProviderClient(ctx)
    private var callback: LocationCallback? = null

    @SuppressLint("MissingPermission")
    override fun start(onFix: (Location) -> Unit) {
        val cb = object : LocationCallback() {
            override fun onLocationResult(result: LocationResult) {
                for (loc in result.locations) onFix(loc)
            }
        }
        callback = cb
        val req = LocationRequest.Builder(Priority.PRIORITY_HIGH_ACCURACY, 2000L)
            .setMinUpdateIntervalMillis(1000L)
            .build()
        client.requestLocationUpdates(req, cb, Looper.getMainLooper())
    }

    override fun stop() {
        callback?.let { client.removeLocationUpdates(it) }
        callback = null
    }
}

/** Platform [LocationManager] GPS provider, raw GNSS, no Google dependency.
 *  All four [LocationListener] methods are overridden explicitly: their default
 *  implementations only exist on API 30+, so a lambda/partial impl would throw
 *  AbstractMethodError on the minSdk-24 range this app supports. */
private class PlatformEngine(private val ctx: Context) : LocationEngine {
    private val lm = ctx.getSystemService(Context.LOCATION_SERVICE) as LocationManager
    private var listener: LocationListener? = null

    @SuppressLint("MissingPermission")
    override fun start(onFix: (Location) -> Unit) {
        // GPS_PROVIDER needs FINE: a coarse-only grant would throw from inside
        // requestLocationUpdates on de-Googled devices. Throw the same exception
        // eagerly so the service's startTracking catch unwinds one way.
        if (!hasFineLocation(ctx)) {
            throw SecurityException("ACCESS_FINE_LOCATION not granted - GPS provider unavailable")
        }
        val l = object : LocationListener {
            override fun onLocationChanged(location: Location) = onFix(location)
            override fun onProviderEnabled(provider: String) {}
            override fun onProviderDisabled(provider: String) {}
            @Deprecated("Required on API < 30; no longer called on API 30+.")
            override fun onStatusChanged(provider: String?, status: Int, extras: Bundle?) {
            }
        }
        listener = l
        // 1 s / 0 m: match the fused cadence; the core decimates as needed.
        lm.requestLocationUpdates(
            LocationManager.GPS_PROVIDER, 1000L, 0f, l, Looper.getMainLooper(),
        )
    }

    override fun stop() {
        listener?.let { lm.removeUpdates(it) }
        listener = null
    }
}
