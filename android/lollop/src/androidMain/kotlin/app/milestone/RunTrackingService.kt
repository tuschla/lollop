package app.milestone

import android.Manifest
import android.annotation.SuppressLint
import android.app.AlarmManager
import android.app.Notification
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
import android.media.AudioManager
import android.media.ToneGenerator
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.SystemClock
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import org.jetbrains.compose.resources.StringResource
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlin.math.roundToInt
import com.google.android.gms.common.ConnectionResult
import com.google.android.gms.common.GoogleApiAvailability
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

    // Everything here runs on the main looper: the location callbacks, the recorder's
    // timers and every notification write, so no state needs a lock.
    private val mainHandler = Handler(Looper.getMainLooper())

    private val recorder = RunRecorder(Host())

    private val serviceScope = CoroutineScope(Dispatchers.Main.immediate + SupervisorJob())

    // The last notification frame actually posted. Fixes arrive at ~1 Hz; re-posting
    // only on a change makes a paused/standing-still stretch and every locate-preview
    // fix (a static notification) cost nothing.
    private var lastNotifiedFrame: RunNotifFrame? = null

    // The user swiped the notification away (its delete intent, possible for a
    // foreground-service notification on Android 14+ and for a Live Update). Live
    // Update guidance: never re-post a dismissed one, so [notifyIfChanged] stands down
    // until the next explicit start ([startLocationForeground] clears it).
    private var notifDismissed = false

    private val openTrackerPending by lazy { openPendingIntent(this, OpenDest.Track) }
    private val pausePending by lazy { servicePendingIntent(ACTION_PAUSE) }
    private val resumePending by lazy { servicePendingIntent(ACTION_RESUME) }
    private val stopServicePending by lazy { servicePendingIntent(ACTION_STOP) }
    private val dismissPending by lazy { servicePendingIntent(ACTION_NOTIF_DISMISSED) }
    private var cachedBuilder: NotificationCompat.Builder? = null
    private var cachedShape: NotifShape? = null

    // Screen-off power state: the GPS request drops to a coarser cadence while the
    // screen is off ([FUSED_BG_INTERVAL_MS] / [PLATFORM_BG_INTERVAL_MS]) to cut the
    // idle-screen battery cost.
    @Volatile private var screenOff = false
    private var screenReceiver: BroadcastReceiver? = null

    // The startId of the newest delivered start command. Every self-stop passes it to
    // stopSelf(startId), so a start queued after it (a new LOCATE/ARM racing a stop)
    // keeps the service alive instead of being torn down before its startForeground.
    private var lastStartId = 0

    private var toneGenerator: ToneGenerator? = null

    private inner class Host : RecorderHost {
        // The fixes' timebase (Location.elapsedRealtimeNanos), so sensor time.
        override fun nowNanos(): Long = SystemClock.elapsedRealtimeNanos()

        override fun nowMs(): Long = SystemClock.elapsedRealtime()

        /** Every caller used startForegroundService, so startForeground runs first
         *  (its ~5 s deadline kills the process otherwise), then the fine-location
         *  re-check, then the engine. */
        @SuppressLint("MissingPermission")
        override fun startStream(): Boolean {
            try {
                startLocationForeground()
            } catch (_: SecurityException) {
                // API 34+: a location FGS without a location grant is rejected here.
                return false
            }
            // Fine location may have been revoked between the UI's check and here.
            if (!hasFineLocation(this@RunTrackingService)) return false
            return try {
                // onError covers an async start failure (the fused client's Task) that
                // throws nothing here.
                engine.start(::onFix) { recorder.onStreamFailed() }
                engine.setBackground(screenOff)
                true
            } catch (_: SecurityException) {
                // Permission raced away between the check above and engine.start.
                false
            } catch (_: IllegalArgumentException) {
                // The platform engine found no GPS provider (no Play Services, no GPS).
                false
            }
        }

        override fun stopStream() = engine.stop()

        override fun openSidecar() = ActiveRunStore.begin()

        override fun reassertSurface() {
            try {
                startLocationForeground()
            } catch (e: SecurityException) {
                // Already foregrounded as a recording run; the deadline is moot.
                PlatformLog.w(TAG, "foreground re-assert refused", e)
            }
        }

        override fun refresh() = notifyIfChanged()

        override fun invalidateSurface() {
            lastNotifiedFrame = null
        }

        override fun cue(cue: RecorderCue) {
            val pattern = cue.pattern
            vibrate(pattern.waveform())
            if (pattern.beep) playStepTone()
        }

        override fun releaseCues() {
            toneGenerator?.let { tone ->
                runCatching { tone.release() }.onFailure { PlatformLog.w(TAG, "tone generator not released", it) }
            }
            toneGenerator = null
        }

        override fun finished(userStop: Boolean) {
            stopForeground(STOP_FOREGROUND_REMOVE)
            // A stop cancels too, in case a re-post slipped in before `stopped`.
            if (userStop) getSystemService(NotificationManager::class.java).cancel(AndroidIds.RUN_NOTIFICATION)
            stopSelf(lastStartId)
        }

        override fun wakeTimer(tag: String, action: () -> Unit): RecorderTimer = WakeTimer(tag, action)

        override fun ticker(action: () -> Unit): RecorderTimer = object : RecorderTimer {
            private val runnable = Runnable { action() }

            override fun schedule(delayMs: Long) {
                mainHandler.postDelayed(runnable, delayMs)
            }

            override fun cancel() = mainHandler.removeCallbacks(runnable)
        }
    }

    /**
     * [EarliestWinsTimer] over a main-looper post (exact while the CPU is awake) plus
     * a listener-based exact alarm, which wakes the CPU and needs no exact-alarm
     * permission. Whichever fires first runs the action once.
     */
    private inner class WakeTimer(private val tag: String, action: () -> Unit) : EarliestWinsTimer(action) {
        private val listener = AlarmManager.OnAlarmListener { fire() }
        private val runnable = Runnable { fire() }

        override fun arm(delayMs: Long) {
            mainHandler.postDelayed(runnable, delayMs)
            // Best-effort: without the alarm the main-looper post still fires once
            // the CPU wakes (the next batched fix), just later.
            runCatching {
                getSystemService(AlarmManager::class.java)
                    ?.setExact(AlarmManager.ELAPSED_REALTIME_WAKEUP, SystemClock.elapsedRealtime() + delayMs, tag, listener, mainHandler)
            }.onFailure { PlatformLog.w(TAG, "wake alarm $tag not set, the main-looper post stands", it) }
        }

        override fun disarm() {
            mainHandler.removeCallbacks(runnable)
            runCatching { getSystemService(AlarmManager::class.java)?.cancel(listener) }
                .onFailure { PlatformLog.w(TAG, "wake alarm $tag not cancelled", it) }
        }
    }

    /** What one refresh shows ([runSurfaceText]; counts in Latin digits as in every
     *  notification, AppLocale.kt). Its equality is the [notifyIfChanged] throttle key. */
    private fun currentNotifFrame(): RunNotifFrame {
        val state = recorder.surfaceState()
        val text = runSurfaceText(state, appResources())
        return RunNotifFrame(content = text.line, chip = text.chip, steps = state.step?.let(::stepBar))
    }

    /** Best-effort: a device without a tone generator, or one that refuses it, stays
     *  silent (the buzz still fires). */
    private fun playStepTone() {
        runCatching {
            val tone = toneGenerator ?: ToneGenerator(AudioManager.STREAM_NOTIFICATION, STEP_TONE_VOLUME).also { toneGenerator = it }
            tone.startTone(ToneGenerator.TONE_PROP_BEEP2, STEP_TONE_MS)
        }.onFailure { PlatformLog.w(TAG, "step tone not played", it) }
    }

    /** One haptic cue from the SERVICE (works screen-off / backgrounded, unlike the
     *  Activity's View.performHapticFeedback). Best-effort: no vibrator stays still,
     *  a vendor refusing it is logged. */
    private fun vibrate(waveform: LongArray) {
        val vibrator: Vibrator? = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            getSystemService(VibratorManager::class.java)?.defaultVibrator
        } else {
            @Suppress("DEPRECATION")
            getSystemService(Context.VIBRATOR_SERVICE) as? Vibrator
        }
        if (vibrator == null || !vibrator.hasVibrator()) return
        runCatching {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                vibrator.vibrate(VibrationEffect.createWaveform(waveform, NO_REPEAT))
            } else {
                @Suppress("DEPRECATION")
                vibrator.vibrate(waveform, NO_REPEAT)
            }
        }.onFailure { PlatformLog.w(TAG, "cue vibration refused", it) }
    }

    /** Re-post the ongoing notification only when what it shows actually changed.
     *  No-op once the recorder stopped so a late queued fix can't resurrect it, and
     *  after the user dismissed it ([notifDismissed]). */
    private fun notifyIfChanged() {
        if (recorder.stopped || notifDismissed) return
        val frame = currentNotifFrame()
        if (frame == lastNotifiedFrame) return
        lastNotifiedFrame = frame
        getSystemService(NotificationManager::class.java).notify(AndroidIds.RUN_NOTIFICATION, buildNotification(frame))
    }

    private fun onFix(loc: Location, batchSize: Int) = recorder.onFix(
        LiveFix(
            lat = loc.latitude,
            lon = loc.longitude,
            wallMs = loc.time,
            monoNanos = loc.elapsedRealtimeNanos,
            accuracyM = if (loc.hasAccuracy()) loc.accuracy.toDouble() else null,
        ),
        batchSize,
    )

    override fun onCreate() {
        super.onCreate()
        AppRuntime.start(this)
        engine = if (hasPlayServices(this)) FusedEngine(this) else PlatformEngine(this)
        ensureLiveChannel(this, CHANNEL, appResources().getString(R.string.track_notif_channel_name), RETIRED_CHANNEL)
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
        // A pause toggled in-app (or by the notification's action) re-posts the
        // notification so its copy and Pause/Resume label match.
        refreshSurfaceOnPause(serviceScope, ::notifyIfChanged)
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onDestroy() {
        // Safety net for a teardown without ACTION_STOP (the OS stopping the
        // component): the GPS callback must not outlive the service, and
        // stopForeground may not have run, so cancel the notification too.
        serviceScope.cancel()
        recorder.haltStream()
        screenReceiver?.let { r ->
            runCatching { unregisterReceiver(r) }.onFailure { PlatformLog.w(TAG, "screen receiver already gone", it) }
        }
        screenReceiver = null
        getSystemService(NotificationManager::class.java).cancel(AndroidIds.RUN_NOTIFICATION)
        super.onDestroy()
    }

    override fun onTaskRemoved(rootIntent: Intent?) {
        recorder.onAppRemoved()
        super.onTaskRemoved(rootIntent)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        lastStartId = startId
        val action = intent?.action
        if (stopIfStray(action, startId)) return START_NOT_STICKY
        when (action) {
            ACTION_LOCATE -> recorder.start(record = false)
            ACTION_START -> recorder.start(record = true)
            ACTION_ARM -> recorder.start(
                record = false,
                arming = true,
                requirePrecise = intent?.getBooleanExtra(EXTRA_REQUIRE_PRECISE, false) ?: false,
                requireMoving = intent?.getBooleanExtra(EXTRA_REQUIRE_MOVING, false) ?: false,
            )
            ACTION_STOP -> recorder.stop()
            ACTION_PAUSE -> recorder.setPaused(true)
            ACTION_RESUME -> recorder.setPaused(false)
            ACTION_START_NOW -> recorder.startNow()
            ACTION_NOTIF_DISMISSED -> notifDismissed = true
        }
        // NOT_STICKY: if the OS kills us mid-run it must not recreate the service
        // with a null intent, that path skips startForeground() and crashes the
        // process on API 26+. A killed run simply ends rather than silently
        // resurrecting without UI/permission context.
        return START_NOT_STICKY
    }

    /**
     * Stop an instance that a stray intent created, true when it did. The system
     * creates the service for any intent it is sent, but only a start or a stop may
     * run on an instance that never started a stream: any other intent that created
     * it (a late Pause tap, Start now, a dismissal) would leak it.
     */
    private fun stopIfStray(action: String?, startId: Int): Boolean {
        if (recorder.started || action in STARTING_ACTIONS) return false
        stopSelf(startId)
        return true
    }

    /** Foreground as a `location`-typed service. API 34+ requires the typed
     *  startForeground for a location FGS (the 2-arg overload throws); ServiceCompat
     *  picks the right form per API level. On API 34+ this throws [SecurityException]
     *  without a location grant. Every caller is an explicit start (Locate/Arm/Start),
     *  so a previous dismissal no longer holds; the posted frame seeds the throttle
     *  key so the first fix doesn't re-post identical content. */
    private fun startLocationForeground() {
        notifDismissed = false
        val frame = currentNotifFrame()
        ServiceCompat.startForeground(
            this,
            AndroidIds.RUN_NOTIFICATION,
            buildNotification(frame),
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                ServiceInfo.FOREGROUND_SERVICE_TYPE_LOCATION
            } else {
                0
            },
        )
        lastNotifiedFrame = frame
    }

    /** What decides the title and which actions show; [buildNotification] caches one
     *  builder per shape. */
    private data class NotifShape(val title: StringResource, val canPause: Boolean, val paused: Boolean, val ownsRun: Boolean)

    private fun buildNotification(frame: RunNotifFrame): Notification {
        val phase = recorder.surfaceState().phase
        val shape = NotifShape(phase.titleRes, phase.canPause, phase == RunSurfacePhase.Paused, phase != RunSurfacePhase.Locating)
        val builder = cachedBuilder?.takeIf { cachedShape == shape } ?: buildBuilder(shape).also {
            cachedBuilder = it
            cachedShape = shape
        }
        return buildRunNotification(builder, frame)
    }

    private fun buildBuilder(shape: NotifShape): NotificationCompat.Builder {
        val res = appResources()
        val builder = runNotificationBuilder(
            this,
            CHANNEL,
            res.getString(shape.title),
            openTrackerPending,
            dismissPending,
        )
        // Pause/Resume drives the SAME RunSession.paused flag the in-app Pause button
        // toggles, delivered as a service action handled in onStartCommand.
        if (shape.canPause) {
            builder.addAction(
                0,
                if (shape.paused) res.getString(R.string.track_action_resume) else res.getString(R.string.track_action_pause),
                if (shape.paused) resumePending else pausePending,
            )
        }
        // Stop, offered in every state. While recording OR armed it opens the tracker
        // (like the tap) and does NOT auto-save: an of-record save must go through the
        // in-app flow (the Finish review with its short-run note + the Save/Discard
        // choice). An auto-save action would bypass those gates and could silently
        // log a drift/accidental run.
        if (shape.ownsRun) {
            builder.addAction(0, res.getString(R.string.track_btn_stop), openTrackerPending)
        } else {
            // Locate-only preview: there is no captured run to route through the save
            // gate, so Stop just tears the service down (ACTION_STOP), freeing the GPS
            // radio if the user opened the tracker to check a fix and walked away.
            builder.addAction(0, res.getString(R.string.track_btn_stop), stopServicePending)
        }
        return builder
    }

    /** A notification action delivered to this service's [onStartCommand]. The
     *  actions differ, so each is its own PendingIntent. */
    private fun servicePendingIntent(action: String): PendingIntent = PendingIntent.getService(
        this, AndroidIds.RUN_ACTION_REQUEST,
        Intent(this, RunTrackingService::class.java).setAction(action),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

    companion object {
        private const val ACTION_LOCATE = "app.milestone.action.LOCATE"
        private const val ACTION_START = "app.milestone.action.START"
        private const val ACTION_ARM = "app.milestone.action.ARM"
        private const val ACTION_STOP = "app.milestone.action.STOP"
        private const val ACTION_PAUSE = "app.milestone.action.PAUSE"
        private const val ACTION_RESUME = "app.milestone.action.RESUME"
        private const val ACTION_START_NOW = "app.milestone.action.START_NOW"
        private const val ACTION_NOTIF_DISMISSED = "app.milestone.action.NOTIF_DISMISSED"
        private const val EXTRA_REQUIRE_PRECISE = "app.milestone.extra.REQUIRE_PRECISE"
        private const val EXTRA_REQUIRE_MOVING = "app.milestone.extra.REQUIRE_MOVING"
        internal const val CHANNEL = "run_live"

        /** The run's channel before it moved to [ensureLiveChannel] (IMPORTANCE_LOW). */
        internal const val RETIRED_CHANNEL = "run_tracking"

        /** The actions that may create the service ([stopIfStray]). */
        private val STARTING_ACTIONS = setOf(ACTION_LOCATE, ACTION_START, ACTION_ARM, ACTION_STOP)

        // Step-change beep: volume (percent of the notification stream's max) and
        // duration (ms).
        private const val STEP_TONE_VOLUME = 90
        private const val STEP_TONE_MS = 350

        // Vibrator repeat index meaning "play the waveform once".
        private const val NO_REPEAT = -1

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
                // throws SecurityException, so gate on FINE explicitly; a device
                // without the provider at all would throw IllegalArgumentException.
                if (!hasFineLocation(ctx)) return
                val lm = ctx.getSystemService(Context.LOCATION_SERVICE) as LocationManager
                if (LocationManager.GPS_PROVIDER !in lm.allProviders) return
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

        /** Armed start: acquire GPS and begin recording AUTOMATICALLY at the first
         *  fix clearing the requested gates (see [ArmTrigger]). Records nothing until
         *  then, so a tap on a stale lock can never record an empty run. */
        fun arm(ctx: Context, requirePrecise: Boolean, requireMoving: Boolean) =
            ContextCompat.startForegroundService(
                ctx,
                Intent(ctx, RunTrackingService::class.java)
                    .setAction(ACTION_ARM)
                    .putExtra(EXTRA_REQUIRE_PRECISE, requirePrecise)
                    .putExtra(EXTRA_REQUIRE_MOVING, requireMoving),
            )

        /** Armed → recording now, skipping the start gates. The armed service is
         *  already foregrounded, so this is a plain startService like the
         *  notification's Pause/Resume. */
        fun startNow(ctx: Context) {
            try {
                ctx.startService(Intent(ctx, RunTrackingService::class.java).setAction(ACTION_START_NOW))
            } catch (e: IllegalStateException) {
                // Background-start restriction (the tap comes from the visible tracker,
                // so not expected): the run stays armed and still starts on its gates.
                PlatformLog.w(TAG, "start now not delivered, the run stays armed", e)
            }
        }

        fun stop(ctx: Context) {
            // startService throws IllegalStateException when the app may not start
            // services right now (in the background, API 26+). A service that already
            // self-stopped is no problem: it is recreated and runs ACTION_STOP.
            try {
                ctx.startService(
                    Intent(ctx, RunTrackingService::class.java).setAction(ACTION_STOP),
                )
            } catch (e: IllegalStateException) {
                // The stop can't reach a service, so the recorder's own stop state is
                // applied here; otherwise the tracker would stick on "Finding GPS…"
                // with no engine behind it.
                PlatformLog.w(TAG, "stop not delivered, resetting the run state here", e)
                RunRecorder.stopUndelivered()
            }
        }
    }
}

private const val TAG = "RunTracking"

/** The vibrator waveform of [CuePattern]: off, then each pulse with the gap between. */
private fun CuePattern.waveform(): LongArray =
    LongArray(pulseMs.size * 2) { i -> if (i == 0) 0L else if (i % 2 == 1) pulseMs[i / 2] else gapMs }

/** A guided run's step bar: one equal [STEP_BAR_UNITS] segment per step (one segment
 *  for the whole run past [STEP_BAR_MAX_SEGMENTS]) and the progress through them. */
internal data class StepBar(val segments: List<Int>, val progress: Int)

/** Length of one step on the bar. Steps are timed or measured, so the bar counts
 *  steps, not seconds or metres. */
internal const val STEP_BAR_UNITS = 100

/** `Notification.ProgressStyle` draws only its first 15 segments
 *  (`MAX_PROGRESS_SEGMENT_LIMIT` in AOSP `Notification.java`). */
internal const val STEP_BAR_MAX_SEGMENTS = 15

/** The bar for [p]: completed steps plus the done share of the current timed or
 *  measured step (an open or "as long as the last rep" step has no known length and
 *  adds nothing until it ends). Full once every step is done. */
internal fun stepBar(p: StepProgress): StepBar? {
    if (p.total <= 0) return null
    val max = p.total * STEP_BAR_UNITS
    val segments = if (p.total <= STEP_BAR_MAX_SEGMENTS) List(p.total) { STEP_BAR_UNITS } else listOf(max)
    val step = p.step ?: return StepBar(segments, max)
    val remainingSec = p.remainingSec
    val remainingM = p.remainingM
    val done = when {
        step.durationSec > 0 && remainingSec != null -> 1.0 - remainingSec.toDouble() / step.durationSec
        step.distanceM > 0 && remainingM != null -> 1.0 - remainingM.toDouble() / step.distanceM
        else -> 0.0
    }
    // Stop short of the next boundary: the step ends when the machine says so.
    val within = (done.coerceIn(0.0, 1.0) * STEP_BAR_UNITS).roundToInt().coerceAtMost(STEP_BAR_UNITS - 1)
    return StepBar(segments, (p.index * STEP_BAR_UNITS + within).coerceIn(0, max))
}

/** One refresh of the run notification. Equality is the service's re-post throttle key. */
internal data class RunNotifFrame(val content: String, val chip: String, val steps: StepBar?)

/**
 * The run notification's per-state builder: everything but the per-frame fields and
 * the actions. It requests promotion to a Live Update (Android 16+; an ignored extra
 * below, where it posts as an ordinary ongoing notification) and meets the
 * documented criteria: ongoing, a title, no custom views, not a group summary, not
 * colorized, on a channel above IMPORTANCE_MIN ([ensureLiveChannel]). [dismiss] reports a user swipe so the
 * service stops re-posting.
 */
internal fun runNotificationBuilder(
    ctx: Context,
    channel: String,
    title: String,
    tap: PendingIntent,
    dismiss: PendingIntent,
): NotificationCompat.Builder = NotificationCompat.Builder(ctx, channel)
    .setContentTitle(title)
    // 1-color white route mark, silhouette only (icons README table). Also the
    // chip's icon.
    .setSmallIcon(R.drawable.ic_stat_mark)
    // The theme accent on the icon and app name, as every lollop notification.
    .setColor(notificationAccent(ctx))
    .setCategory(NotificationCompat.CATEGORY_WORKOUT)
    .setOngoing(true)
    .setRequestPromotedOngoing(true)
    .setContentIntent(tap)
    .setDeleteIntent(dismiss)
    // Shown in full on a secure lock screen: the line carries only moving time,
    // distance and pace (never a location), so a runner can glance at it without
    // unlocking.
    .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
    // The ~1 Hz progress refresh must never re-alert.
    .setOnlyAlertOnce(true)

/**
 * Apply one [frame] to a cached [builder] and build. A guided run gets a
 * `ProgressStyle` of its steps (the current step name leads the content line);
 * androidx falls back to a plain progress bar below Android 16. Otherwise
 * `BigTextStyle`, so the always-expanded Live Update card wraps the line (moving
 * time, distance, current pace) instead of cutting it. The chip text is applied by
 * androidx on Android 16+ only.
 */
internal fun buildRunNotification(builder: NotificationCompat.Builder, frame: RunNotifFrame): Notification {
    val bar = frame.steps
    val style = if (bar != null) {
        NotificationCompat.ProgressStyle()
            .setProgressSegments(bar.segments.map { NotificationCompat.ProgressStyle.Segment(it) })
            .setProgress(bar.progress)
    } else {
        NotificationCompat.BigTextStyle().bigText(frame.content)
    }
    return builder
        .setContentText(frame.content)
        .setShortCriticalText(frame.chip)
        .setStyle(style)
        .build()
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

/** A high-accuracy fix stream.
 *  [start] delivers each fix on the main thread and invokes [onError] on an ASYNC
 *  start failure the caller's try/catch can't observe (the fused Task failing);
 *  [stop] unregisters and is a safe no-op if not started. */
private interface LocationEngine {
    fun start(onFix: (Location, Int) -> Unit, onError: (Throwable) -> Unit)
    /** Switch the fix cadence between foreground (screen on) and background
     *  (screen off) rates. A no-op when unchanged or not started. */
    fun setBackground(background: Boolean)
    fun stop()
}

/** Foreground (screen-on) recording cadence for full route fidelity: fused request
 *  interval / min interval (ms), and the platform GPS interval (ms). */
private const val FUSED_FG_INTERVAL_MS = 2000L
private const val FUSED_FG_MIN_INTERVAL_MS = 1000L
private const val PLATFORM_FG_INTERVAL_MS = 1000L

/** Background (screen-off) cadence: coarser to save battery while the phone is
 *  pocketed; the deliberate power/fidelity tradeoff. Long runs are thinned to a
 *  ~2–3 s cadence at save time anyway ([CoreConstants.TRACK_DECIMATION_CAP]). */
private const val FUSED_BG_INTERVAL_MS = 4000L
private const val FUSED_BG_MIN_INTERVAL_MS = 3000L
private const val PLATFORM_BG_INTERVAL_MS = 4000L

/** Platform GPS minimum displacement (m) between updates. Every fix is delivered;
 *  standing-still wander is handled by the moving-speed floor
 *  ([CoreConstants.MIN_MOVING_SPEED_MPS]) like the fused stream. */
private const val PLATFORM_MIN_DISTANCE_M = 0f

/** Google fused provider (Play Services). Batches several fixes per callback
 *  under Doze/screen-off; every batched fix is forwarded so the route stays
 *  continuous rather than collapsing to the newest point. */
private class FusedEngine(ctx: Context) : LocationEngine {
    private val client = LocationServices.getFusedLocationProviderClient(ctx)
    private var callback: LocationCallback? = null
    private var background = false

    @SuppressLint("MissingPermission")
    override fun start(onFix: (Location, Int) -> Unit, onError: (Throwable) -> Unit) {
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
                val batch = result.locations.sortedBy { it.elapsedRealtimeNanos }
                for (loc in batch) onFix(loc, batch.size)
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
        // no-op if not started. A runtime-permission revocation kills the process, and the
        // PROVIDERS_CHANGED receiver in RunTracking.kt covers a toggled location service
        // while the screen is composed, so a dropped re-request here is harmless.
        val cb = callback ?: return
        runCatching { request(cb) }.onFailure { PlatformLog.w(TAG, "fused cadence not changed", it) }
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
                // No setMaxUpdateAgeMillis: every batched fix must reach the route; the
                // stale cached first fix is rejected by [armedFixPassesAgeGate].
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
    override fun start(onFix: (Location, Int) -> Unit, onError: (Throwable) -> Unit) {
        // Platform LocationManager has no async start Task: requestLocationUpdates
        // throws synchronously on failure (caught by the service's start try), so
        // there is nothing to route through [onError] here.
        // Idempotent: a double ACTION_START must not leak a second listener.
        stop()
        // GPS_PROVIDER needs FINE: a coarse-only grant would throw from inside
        // requestLocationUpdates on de-Googled devices. Throw the same exception
        // eagerly so the service's startStream catch unwinds one way.
        if (!hasFineLocation(ctx)) {
            throw SecurityException("ACCESS_FINE_LOCATION not granted: GPS provider unavailable")
        }
        // A device without GNSS (Wi-Fi-only tablet) has no GPS provider; requesting it
        // throws IllegalArgumentException, which startStream unwinds cleanly.
        if (LocationManager.GPS_PROVIDER !in lm.allProviders) {
            throw IllegalArgumentException("no GPS provider on this device")
        }
        val l = object : LocationListener {
            override fun onLocationChanged(location: Location) = onFix(location, 1)
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
        val l = listener ?: return
        // Re-register at the new interval. A runtime-permission revocation kills the
        // process, and the PROVIDERS_CHANGED receiver in RunTracking.kt covers a toggled
        // location service while the screen is composed, so a thrown SecurityException here
        // just means no more fixes, never a crash.
        runCatching {
            lm.removeUpdates(l)
            request(l)
        }.onFailure { PlatformLog.w(TAG, "GPS cadence not changed", it) }
    }

    @SuppressLint("MissingPermission")
    private fun request(l: LocationListener) = lm.requestLocationUpdates(
        // The shell thins at save time (`RunSession.decimatedTrackForCore`).
        LocationManager.GPS_PROVIDER,
        if (background) PLATFORM_BG_INTERVAL_MS else PLATFORM_FG_INTERVAL_MS,
        PLATFORM_MIN_DISTANCE_M,
        l,
        Looper.getMainLooper(),
    )

    override fun stop() {
        listener?.let { lm.removeUpdates(it) }
        listener = null
    }
}
