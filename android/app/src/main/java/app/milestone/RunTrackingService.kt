package app.milestone

import android.Manifest
import android.annotation.SuppressLint
import android.app.AlarmManager
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
import androidx.annotation.StringRes
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.launch
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

    // Locate-only vs recording. Opening the tracker starts the
    // engine to preview fix quality, but records NOTHING until the user taps
    // Start; no route point, no crash-durable sidecar line. `recording` flips
    // true on ACTION_START.
    private var recording = false

    // This instance ran startTracking; distinguishes a live locate/armed/recording
    // session from an instance created BY a stray Pause/Resume intent (whose
    // onCreate registered the screen receiver + paused collector with nothing to
    // ever stop them). Set true at the top of startTracking, checked in onStartCommand.
    private var started = false

    // Armed: Start was tapped before a lock. The service is acquiring GPS (like
    // LOCATE) but will flip to recording AUTOMATICALLY at the first fix that clears
    // [armTrigger]'s gates. `armed` mirrors RunSession.armed for the notification
    // copy; both are cleared the instant recording begins (beginRecording) or on any
    // teardown ([clearArmState]).
    private var armed = false
    private var armTrigger: ArmTrigger? = null

    // Set once the service is tearing down (stopTracking / onDestroy). A location
    // result already queued on the main looper can still fire onFix AFTER
    // stopForeground; without this guard it would re-post the ongoing notification
    // that nothing then cancels, leaving a stuck "Getting a GPS fix".
    @Volatile private var stopped = false

    // The [RunSession.generation] captured when this stream started ([startTracking]).
    // A Cancel/Back/discard while armed runs `stop()` (async ACTION_STOP) then
    // `reset()`; a fix already queued on the main looper can fire onFix BEFORE
    // ACTION_STOP lands, and with `armed` still true would beginRecording() a ghost
    // session + sidecar AFTER the reset. reset() bumps the generation, so onFix
    // compares this snapshot against the live value and stands down (self-stops) the
    // instant they diverge, before touching any recording state.
    private var sessionGeneration = 0L

    // Main-thread scope observing RunSession.paused so an in-app Pause toggle
    // re-posts the ongoing notification (updated title/content + the Pause↔Resume
    // action label). The notification's own Pause/Resume action writes the SAME
    // flag, so both directions stay in sync off one source of truth. Cancelled in
    // onDestroy. Dispatchers.Main.immediate keeps every notification write on the
    // main thread, serialized with onFix's, so lastNotifiedContent needs no lock.
    private val serviceScope = CoroutineScope(Dispatchers.Main.immediate + SupervisorJob())

    // The last notification content string actually posted, so [notifyIfChanged]
    // re-posts ONLY when the displayed line changes rather than rebuilding + notify()
    // on every ~1 Hz fix. While recording it changes ~once/sec anyway (elapsed
    // ticks), but a paused/standing-still stretch, and every locate-preview fix,
    // whose notification is static, then costs nothing.
    private var lastNotifiedContent: String? = null

    // Cached notification-building state. The PendingIntents (tap, pause, resume and
    // the two Stop variants) and a per-[NotifShape] NotificationCompat.Builder are
    // built ONCE per state, so the ~1 Hz recording refresh only re-sets the content
    // line and calls build() instead of reconstructing the PendingIntents + builder +
    // actions on every fix. The shape (recording/armed/paused) fixes the title and
    // which actions show; while steady recording it never changes.
    private var tapPending: PendingIntent? = null
    private var pausePending: PendingIntent? = null
    private var resumePending: PendingIntent? = null
    private var stopPending: PendingIntent? = null
    private var stopServicePending: PendingIntent? = null
    private var cachedBuilder: NotificationCompat.Builder? = null
    private var cachedShape: NotifShape? = null

    // Screen-off power state: the GPS request drops to a coarser cadence while the
    // screen is off ([FUSED_BG_INTERVAL_MS] / [PLATFORM_BG_INTERVAL_MS]) to cut the
    // idle-screen battery cost.
    @Volatile private var screenOff = false
    private var screenReceiver: BroadcastReceiver? = null

    // Screen-off signal-loss watchdog. The in-app pill only ages the last fix while
    // the tracker is composed (visible), so a phone-in-pocket runner would never feel a
    // GPS drop; the service owns this check instead. Re-armed on every accepted
    // recording fix; when [SIGNAL_LOSS_MS] passes with no fix it vibrates once and
    // re-posts the notification with the signal-lost line. [signalLost] latches the
    // episode so it buzzes once, and clears on the next accepted fix. Timed by a
    // [WakeTimer] (a plain Handler post stalls while the CPU sleeps with the screen
    // off); everything runs on the main looper, serialized with onFix, so no lock.
    private val mainHandler = Handler(Looper.getMainLooper())
    private var signalLost = false
    private val signalWake = WakeTimer("lollop:signal-watchdog") { checkSignal() }

    // The startId of the newest delivered start command. Every self-stop passes it to
    // stopSelf(startId), so a start queued after it (a new LOCATE/ARM racing a stop)
    // keeps the service alive instead of being torn down before its startForeground.
    private var lastStartId = 0

    /**
     * One deadline that fires on time with the screen off: a main-looper post (exact
     * while the CPU is awake) plus a listener-based exact alarm, which wakes the CPU
     * and needs no exact-alarm permission. Whichever fires first runs [action] once.
     * While a deadline is pending, a request for the same or a LATER moment is a
     * no-op: the earlier one fires and [action] re-arms for the remainder, so the
     * per-fix re-arm costs no alarm traffic. An earlier request replaces it.
     */
    private inner class WakeTimer(private val tag: String, private val action: () -> Unit) {
        private var dueAt = 0L
        private val listener = AlarmManager.OnAlarmListener { fire() }
        private val runnable = Runnable { fire() }

        private fun fire() {
            cancel()
            action()
        }

        fun schedule(delayMs: Long) {
            val at = SystemClock.elapsedRealtime() + delayMs.coerceAtLeast(0L)
            if (dueAt != 0L && dueAt <= at) return
            cancel()
            dueAt = at
            mainHandler.postDelayed(runnable, delayMs.coerceAtLeast(0L))
            // Best-effort: without the alarm the main-looper post still fires once
            // the CPU wakes (the next batched fix), just later.
            runCatching {
                getSystemService(AlarmManager::class.java)
                    ?.setExact(AlarmManager.ELAPSED_REALTIME_WAKEUP, at, tag, listener, mainHandler)
            }
        }

        fun cancel() {
            if (dueAt == 0L) return
            dueAt = 0L
            mainHandler.removeCallbacks(runnable)
            runCatching { getSystemService(AlarmManager::class.java)?.cancel(listener) }
        }
    }

    /** The notification's current content line: the recording progress (labelled
     *  "Moving", the biggest figure is MOVING time, not wall-clock elapsed, matching
     *  the tracker tile's overline so the two surfaces never disagree) plus the
     *  current pace, a "Paused · " line while paused, the signal-lost line, the armed
     *  "starts when ready" copy, or the static locate-preview text. The state→resource
     *  SELECTION lives in the pure [trackingNotifContentSpec] (unit-tested against the
     *  real R.string ids, so a swapped id can't hide behind caller-supplied literals);
     *  this just resolves. The throttle key for [notifyIfChanged]: because the paused
     *  and signal-lost states fold into the line, toggling either re-posts. */
    private fun currentNotifContent(): String {
        val spec = trackingNotifContentSpec(
            recording = recording,
            armed = armed,
            paused = RunSession.paused.value,
            signalLost = signalLost,
            progress = progressText(),
            paceNow = paceNowText(),
        )
        return when (spec) {
            is NotifContentSpec.Literal -> getString(spec.res)
            is NotifContentSpec.Progress -> {
                val base = if (spec.paceNow != null) {
                    getString(R.string.track_notif_progress_pace, spec.text, spec.paceNow)
                } else {
                    getString(R.string.track_notif_progress, spec.text)
                }
                // A guided run leads with the current step ("Rep 2 of 6 · 320 m
                // left"), so a glance at the lock screen says what to do now.
                RunSession.stepProgress.value?.let { getString(R.string.track_notif_step, runStepLine(resources, it), base) } ?: base
            }
            is NotifContentSpec.PausedProgress -> getString(spec.res, spec.progress)
        }
    }

    /** (Re)schedule the signal-loss check for [SIGNAL_LOSS_MS] after the latest
     *  accepted fix. Cancels once stopped or when not recording (locate/armed previews
     *  have no run to lose). */
    private fun armSignalWatchdog() {
        if (stopped || !recording) {
            signalWake.cancel()
            return
        }
        val last = RunSession.lastFixElapsedMs.value
        val delayMs = if (last <= 0L) {
            SIGNAL_LOSS_MS
        } else {
            (last + SIGNAL_LOSS_MS + 1L - SystemClock.elapsedRealtime()).coerceAtLeast(0L)
        }
        signalWake.schedule(delayMs + SIGNAL_CHECK_SLACK_MS)
    }

    /** Watchdog body: on a due loss while recording+unpaused, vibrate once and show
     *  the signal-lost notification line. A paused run is not buzzed (no fixes are
     *  expected to matter), but is re-checked so a resume into a dead signal still
     *  warns. Not yet due (a fix landed in between) → re-arm for the remainder. */
    private fun checkSignal() {
        if (stopped || !recording) return
        if (!signalLossDue(RunSession.lastFixElapsedMs.value, SystemClock.elapsedRealtime())) {
            armSignalWatchdog()
            return
        }
        if (RunSession.paused.value) {
            signalWake.schedule(SIGNAL_LOSS_MS)
            return
        }
        if (!signalLost) {
            signalLost = true
            vibrate(SIGNAL_LOST_WAVEFORM)
            notifyIfChanged()
        }
    }

    /** An accepted fix while recording: end any signal-loss episode (re-posting the
     *  normal progress line) and push the watchdog out again. */
    private fun onSignalFix() {
        if (signalLost) {
            signalLost = false
            lastNotifiedContent = null
        }
        armSignalWatchdog()
    }

    /** Stop the watchdog and forget any loss episode (every teardown path). */
    private fun clearSignalWatchdog() {
        signalWake.cancel()
        signalLost = false
    }

    // Guided-run step ticker: once a second while recording a planned,
    // structured run, advance [RunSession]'s step machine on the monotonic clock and
    // cue each step change with a buzz + tone from the SERVICE, so a pocketed phone
    // with the screen off still says "rep 3 starts now". The 1 s main-looper tick
    // refreshes the countdown; a [WakeTimer] at the timed step's end wakes the CPU so
    // the change cue is on time with the screen off. Main looper, serialized with
    // onFix and the screen's Skip step.
    private val stepTicker = Runnable { tickSteps() }
    private val stepWake = WakeTimer("lollop:step-guide") { tickSteps() }
    private var toneGenerator: ToneGenerator? = null

    private fun tickSteps() {
        mainHandler.removeCallbacks(stepTicker)
        if (stopped || !recording || RunSession.stepProgress.value == null) {
            stepWake.cancel()
            return
        }
        if (RunSession.tickSteps(SystemClock.elapsedRealtime())) {
            // The pending deadline belonged to the step that just ended.
            stepWake.cancel()
            vibrate(STEP_CHANGE_WAVEFORM)
            playStepTone()
        }
        notifyIfChanged()
        val progress = RunSession.stepProgress.value
        if (progress != null && !progress.finished) {
            mainHandler.postDelayed(stepTicker, STEP_TICK_MS)
            // Distance steps advance on fixes, which wake the CPU anyway.
            val remainingSec = progress.remainingSec
            if (remainingSec != null) stepWake.schedule((remainingSec * 1000L).coerceAtLeast(STEP_TICK_MS)) else stepWake.cancel()
        } else {
            stepWake.cancel()
        }
    }

    /** A short beep on the notification stream for a step change. Best-effort: a
     *  device without a tone generator, or one that refuses it, stays silent (the
     *  buzz still fires). */
    private fun playStepTone() {
        runCatching {
            val tone = toneGenerator ?: ToneGenerator(AudioManager.STREAM_NOTIFICATION, STEP_TONE_VOLUME).also { toneGenerator = it }
            tone.startTone(ToneGenerator.TONE_PROP_BEEP2, STEP_TONE_MS)
        }
    }

    /** Stop the step ticker and free the tone generator (every teardown path). */
    private fun clearStepTicker() {
        mainHandler.removeCallbacks(stepTicker)
        stepWake.cancel()
        toneGenerator?.let { runCatching { it.release() } }
        toneGenerator = null
    }

    /** One haptic cue from the SERVICE (works screen-off / backgrounded, unlike the
     *  Activity's View.performHapticFeedback). Best-effort: no vibrator, or a vendor
     *  refusing it, is silently ignored. */
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
        }
    }

    /** Clear all armed state (the service flag, the trigger, and the observed flow).
     *  Called from beginRecording (armed → recording) and every teardown path so a
     *  crash/stop while merely armed never leaves the UI stuck armed. */
    private fun clearArmState() {
        armed = false
        armTrigger = null
        RunSession.armed.value = false
    }

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
    private fun onFix(loc: Location, batchSize: Int) {
        // Post-stop queued fix: do nothing, and never re-post the notification.
        if (stopped) return
        // Stale-session guard: a Cancel/Back/discard while armed/locating reset the
        // session (bumping [RunSession.generation]) but a location result queued on
        // the main looper can still reach here BEFORE ACTION_STOP tears the stream
        // down. Recording it would resurrect a ghost session + sidecar AFTER the
        // reset. Detect the divergence and self-stop instead of touching any state
        // (not even the preview lastFix, which the reset just cleared).
        if (RunSession.generation != sessionGeneration) {
            unwindLocationFailure(failed = false)
            return
        }
        // Gate implausible fixes for the LIVE display + map: the fused provider's
        // first callback is often a stale cached location (many minutes old, far
        // away) that would draw a giant phantom leg and inflate live distance; a
        // very-low-accuracy fix is likewise noise. The core's QC still gates the
        // of-record figures, but these must not reach the live UX at all.
        if (!isPlausibleLiveFix(loc, recording = recording, batchSize = batchSize)) return
        val point = GpsPoint(
            lat = loc.latitude,
            lon = loc.longitude,
            observedAt = loc.time / 1000,
            accuracyM = loc.accuracy.toDouble(),
        )
        // Preview: drives the "GPS lock · ±N m" readout, the self-location dot
        // before Start, and the fresh-lock/signal-loss pill. Set on EVERY accepted
        // fix (recording, armed, or locate-only). Uses the fix's OWN monotonic stamp
        // (elapsedRealtimeNanos), the SAME timebase [isFreshLock] ages against — a
        // ~10 s-old batched fix must not read "fresh lock" for ~20 s, as the delivery
        // time (SystemClock.elapsedRealtime) would have made it.
        RunSession.lastFix.value = point
        RunSession.lastFixElapsedMs.value = loc.elapsedRealtimeNanos / 1_000_000
        if (recording) {
            // A fix arrived: any signal-loss episode is over; push the watchdog out.
            onSignalFix()
            // Paused (stat-sheet control): drop the fix so the paused span records
            // no route; the service stays foregrounded so the run remains live. Mark a
            // resume boundary so the NEXT accepted fix opens a new segment; a pause +
            // relocation must not bridge the displacement into distance.
            if (RunSession.paused.value) {
                RunSession.markResumeBoundary()
                return
            }
            if (RunSession.add(point, loc.elapsedRealtimeNanos)) {
                // Persist to the crash-durable sidecar the instant the fix is accepted, so
                // a process/service kill mid-run is recoverable (off the main thread). A fix
                // RunSession dropped as non-monotonic is not mirrored to the sidecar.
                ActiveRunStore.append(point)
            }
            // Refresh the ongoing notification so a glance at the lockscreen confirms
            // GPS is still capturing. Re-notify with the same NOTIF_ID updates the
            // existing notification in place; IMPORTANCE_LOW keeps it silent every fix.
            // Throttled to only re-post when the displayed line (elapsed + distance)
            // actually changed; a paused/standing-still stretch costs nothing.
            notifyIfChanged()
            return
        }
        // Armed: feed the fix to the trigger. A non-null seed means the gates are
        // satisfied, begin recording FROM that seed (moving-start run-up kept);
        // otherwise stay armed (static notification, at most one re-post).
        if (armed) {
            val seed = armTrigger?.offer(point, loc.elapsedRealtimeNanos)
            if (seed != null) {
                beginRecording(seed)
            } else {
                notifyIfChanged()
            }
            return
        }
        // Locate-only: nothing is recorded until the user taps Start. The preview
        // notification is static, so [notifyIfChanged] re-posts it at most once.
        notifyIfChanged()
    }

    /** Armed → recording: the trigger's gates are satisfied. Clears the arm state,
     *  flips to recording, and flushes the [seed] run-up fixes through the session +
     *  sidecar so a moving-start's approach distance is kept. [ActiveRunStore.begin]
     *  runs HERE, not on Start: a crash while merely armed leaves no sidecar (no false
     *  recovery prompt), and the sidecar exists from the first REAL recorded fix. */
    private fun beginRecording(seed: List<Pair<GpsPoint, Long>>) {
        clearArmState()
        recording = true
        RunSession.tracking.value = true
        // Open the crash-durable sidecar now that a real run has begun.
        ActiveRunStore.begin(this)
        for ((pt, nanos) in seed) {
            if (RunSession.add(pt, nanos)) ActiveRunStore.append(pt)
        }
        // A planned, structured run starts its step guide with the recording (the
        // run-up seed counts as already covered, so the first step starts here).
        RunSession.startSteps(SystemClock.elapsedRealtime())
        tickSteps()
        // Force a re-post: the title/copy switch from armed to "Tracking run".
        lastNotifiedContent = null
        notifyIfChanged()
        // Armed → recording happens hands-free (often phone already pocketed), so
        // confirm it with a buzz from the service, which works screen-off.
        vibrate(RECORDING_STARTED_WAVEFORM)
        armSignalWatchdog()
    }

    override fun onCreate() {
        super.onCreate()
        ActiveRunStore.init(this)
        engine = if (hasPlayServices(this)) FusedEngine(this) else PlatformEngine(this)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            getSystemService(NotificationManager::class.java).createNotificationChannel(
                NotificationChannel(
                    CHANNEL,
                    getString(R.string.track_notif_channel_name),
                    NotificationManager.IMPORTANCE_LOW,
                )
            )
        }
        // Screen on/off drives the GPS cadence: while the screen is off the
        // engine drops to a coarser interval to cut idle battery drain.
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
        // Mirror the shared paused flag into the notification: toggling Pause in-app
        // (or via the notification action, which sets the SAME flag) re-posts the
        // ongoing notification so the lockscreen copy + Pause/Resume label match the
        // in-app control. notifyIfChanged is a no-op unless the displayed line changed
        // (and while not recording the paused state isn't shown at all), so a stray
        // toggle costs nothing. drop(1) skips the StateFlow's initial replay so nothing
        // posts before startForeground.
        serviceScope.launch {
            RunSession.paused.drop(1).collect { notifyIfChanged() }
        }
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onDestroy() {
        // Safety net: if the service is torn down without an ACTION_STOP (e.g. the
        // OS stops the component under memory pressure), unregister so the GPS
        // callback and sensor don't outlive the service. Harmless no-op when
        // stopTracking() already removed it.
        stopped = true
        serviceScope.cancel()
        engine.stop()
        screenReceiver?.let { runCatching { unregisterReceiver(it) } }
        screenReceiver = null
        // Cancel the ongoing notification: stopForeground(REMOVE) may not have run
        // (OS teardown), and a queued onFix could otherwise leave it orphaned.
        getSystemService(NotificationManager::class.java).cancel(NOTIF_ID)
        RunSession.tracking.value = false
        RunSession.locating.value = false
        clearArmState()
        clearSignalWatchdog()
        clearStepTicker()
        super.onDestroy()
    }

    override fun onTaskRemoved(rootIntent: Intent?) {
        if (recording || armed) {
            // A live or armed run must survive a Recents swipe: flush queued sidecar
            // writes so nothing already accepted is lost, and KEEP the sidecar so the
            // run is recoverable if the OS then kills the process.
            ActiveRunStore.flush()
        } else {
            // Locate-only preview with no run in progress: the user swiped the app away
            // without recording, so stop the service rather than leave the GPS radio on
            // indefinitely.
            stopTracking()
        }
        super.onTaskRemoved(rootIntent)
    }

    /** Reject a fix that must not reach the LIVE display: a very-low-accuracy fix,
     *  or — while merely armed/previewing — a STALE cached location the fused provider
     *  hands back on its first callback (often minutes old / far away). [recording]
     *  scopes the AGE gate: while recording, batched fixes (Doze wakeups, main-looper
     *  stalls) may all be many seconds old but MUST still reach [RunSession.add] + the
     *  sidecar — the FusedEngine forwards every batched fix and RunSession's dt<0
     *  drop-guard + segment boundaries handle ordering, so dropping one would silently
     *  amputate the route. While ARMED the same batching applies: a delivered batch
     *  ([batchSize] > 1) is genuine ongoing screen-off delivery whose fixes are exempt
     *  from the age gate too, so the [ArmTrigger] moving window can fill; only the lone
     *  cached first fix (delivered ALONE, [batchSize] == 1) is still rejected. See
     *  [armedFixPassesAgeGate]. The ACCURACY gate always applies; the core's own QC
     *  still gates the of-record distance/pace on save. */
    private fun isPlausibleLiveFix(loc: Location, recording: Boolean, batchSize: Int): Boolean {
        // Freshness (armed/preview only): elapsedRealtime age is monotonic (immune to
        // wall-clock skew). Skipped while recording so a batched fix is never dropped.
        if (!recording) {
            val ageMs = (SystemClock.elapsedRealtimeNanos() - loc.elapsedRealtimeNanos) / 1_000_000
            if (!armedFixPassesAgeGate(ageMs, batchSize)) return false
        }
        // No reported accuracy → the fix can't be quality-gated: the core's
        // of-record QC drops it, so recording it live would desync the live
        // map/distance from the saved run. Reject it here to keep them in step.
        if (!loc.hasAccuracy()) return false
        // Accuracy gate: mirror the core's `running::MAX_GPS_ACCURACY_M` (30 m) so
        // live distance/pace only count fixes the of-record pipeline keeps.
        if (loc.accuracy > MAX_GPS_ACCURACY_M) return false
        return true
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        lastStartId = startId
        when (intent?.action) {
            ACTION_LOCATE -> startTracking(record = false)
            ACTION_START -> startTracking(record = true)
            ACTION_ARM -> startTracking(record = false, arming = true, intent = intent)
            ACTION_STOP -> stopTracking()
            // Notification Pause/Resume: write the SAME flag the in-app control sets;
            // the onFix path drops fixes while paused and the paused-flow collector
            // re-posts the notification. No startForeground needed (these arrive on an
            // already-foregrounded service via startService, not startForegroundService).
            // Guarded on `recording`: a Pause tap racing teardown could deliver here to
            // a freshly (re)created, non-recording service, whose paused-flow collector
            // would then post an ongoing notification with NO foreground service behind
            // it (a stuck ghost notification). Only a live recording may toggle pause.
            // And a stray Pause/Resume that CREATED this service (onCreate registered
            // the screen receiver + paused collector with nothing to ever stop them, and
            // it never ran startTracking, so !started) would otherwise leak the instance:
            // stopSelf(startId) tears it down. A live locate/armed session (started) is
            // left untouched.
            ACTION_PAUSE -> if (recording) RunSession.paused.value = true else if (!started) stopSelf(startId)
            ACTION_RESUME -> if (recording) RunSession.paused.value = false else if (!started) stopSelf(startId)
            // "Start now" while armed: begin recording at once, skipping the start gates
            // (treadmill, a slow start, a GPS hiccup). Only the live armed stream of the
            // current session may begin; a stray intent that created this instance is
            // torn down like a stray Pause/Resume.
            ACTION_START_NOW -> when {
                armed && !recording && !stopped && RunSession.generation == sessionGeneration -> beginRecording(emptyList())
                !started -> stopSelf(startId)
            }
        }
        // NOT_STICKY: if the OS kills us mid-run it must not recreate the service
        // with a null intent, that path skips startForeground() and crashes the
        // process on API 26+. A killed run simply ends rather than silently
        // resurrecting without UI/permission context.
        return START_NOT_STICKY
    }

    @SuppressLint("MissingPermission")
    private fun startTracking(record: Boolean, arming: Boolean = false, intent: Intent? = null) {
        // Locate-only (record == false) previews fix quality; ACTION_START flips
        // to recording; ACTION_ARM (arming == true) previews too but auto-begins
        // recording at the first fix that clears the gates. All arrive via
        // startForegroundService, so all must call startForeground within the deadline
        // below. A LOCATE already foregrounded then re-foregrounds harmlessly on ARM.
        // Defense in depth: an ARM reaching a LIVE recording must not demote it.
        // recording=false here would let a later trigger-fire re-open the sidecar via
        // beginRecording() and merge two runs. Satisfy the startForegroundService
        // deadline by re-asserting the notification, undo the shell's optimistic armed
        // flag, and leave the run untouched.
        if (arming && recording) {
            RunSession.armed.value = false
            try {
                startLocationForeground()
            } catch (_: SecurityException) {
                // Already foregrounded as a recording run; the deadline is moot.
            }
            return
        }
        // This instance now owns the stream — distinguishes it from an instance a
        // stray Pause/Resume intent created (see onStartCommand's !started teardown).
        started = true
        recording = record
        // A fresh LOCATE/START on a reused service instance clears any prior
        // teardown guard so onFix can post again.
        stopped = false
        // Bind this stream to the current session token: onFix records only while the
        // live [RunSession.generation] still equals this, so a reset() (Cancel/Back/
        // discard) racing a queued fix can't record into a cleared session.
        sessionGeneration = RunSession.generation
        // Arm: build the trigger from the requested gates and publish the armed flag
        // BEFORE foregrounding, so the notification copy and the observing screen are
        // consistent from the first frame. A non-arming start clears any stale arm.
        if (arming) {
            armTrigger = ArmTrigger(
                requirePrecise = intent?.getBooleanExtra(EXTRA_REQUIRE_PRECISE, false) ?: false,
                requireMoving = intent?.getBooleanExtra(EXTRA_REQUIRE_MOVING, false) ?: false,
            )
            armed = true
            RunSession.armed.value = true
        } else {
            clearArmState()
        }
        // startForegroundService (the caller) arms a ~5 s deadline: this service
        // MUST call startForeground before anything, or the framework kills the
        // process with ForegroundServiceDidNotStartInTimeException. So fulfill the
        // deadline FIRST, before the permission re-check, and only then decide
        // whether to keep running. No path may bail out having armed the deadline
        // without a startForeground call.
        try {
            startLocationForeground()
        } catch (_: SecurityException) {
            // API 34+: startForeground for a location FGS itself throws without a
            // location grant. The framework has REJECTED the start here (the deadline
            // is satisfied by its own rejection). Run the shared clean unwind so the
            // OPTIMISTIC armed flag the shell set is cleared too — a bare stopSelf()
            // would leave RunSession.armed latched and strand the tracker armed.
            unwindLocationFailure()
            return
        }
        // The startForeground call just posted the notification; seed the throttle
        // key so the first fix doesn't needlessly re-post identical content.
        lastNotifiedContent = currentNotifContent()
        // Foregrounded: now safe to bail if fine location raced away between the
        // UI's check and here (revoking location also downgrades to coarse-only). Route
        // through the shared clean unwind so the arm state is cleared too — a bare
        // stopForeground/stopSelf here would leave RunSession.armed latched and strand
        // the tracker on "Acquiring GPS" with no engine behind it.
        // smoke: revoke precise location while armed, tap Start → the tracker returns to
        // idle (armed flag cleared, no stuck "Acquiring GPS"); not JVM-unit-testable as
        // this path needs a live foreground Service + startForeground.
        if (!hasFineLocation(this)) {
            unwindLocationFailure()
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
            // A direct recording start (crash-resume) watches for signal loss too.
            if (record) armSignalWatchdog()
        } catch (_: SecurityException) {
            // Permission raced away between the check above and engine.start.
            unwindLocationFailure()
        } catch (_: IllegalArgumentException) {
            // The platform engine found no GPS provider (a Wi-Fi-only tablet without
            // Play Services): fail the start cleanly instead of crashing.
            unwindLocationFailure()
        }
    }

    /** Clean unwind for a location-start failure (synchronous SecurityException, a
     *  missing GPS provider, or the fused client's async Task failure): stop the
     *  half-started engine, drop the foreground state, and self-stop so the service
     *  never lingers foregrounded with no fix stream. [failed] publishes
     *  [RunSession.locationFailed] so the screen stops re-launching the preview into
     *  the same failure; false for the stale-session stand-down, which is not a
     *  failure. Marks [stopped] so a late queued onFix returns without re-posting the
     *  notification, and is idempotent, a second call (e.g. an async Task failure
     *  arriving after a clean stop) short-circuits. */
    private fun unwindLocationFailure(failed: Boolean = true) {
        if (stopped) return
        stopped = true
        engine.stop()
        RunSession.tracking.value = false
        RunSession.locating.value = false
        if (failed) RunSession.locationFailed.value = true
        clearArmState()
        clearSignalWatchdog()
        clearStepTicker()
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf(lastStartId)
    }

    private fun stopTracking() {
        // Guard against a location result already queued on the main looper: mark
        // stopped BEFORE tearing down so any late onFix returns without re-posting
        // the notification.
        stopped = true
        engine.stop()
        recording = false
        armTrigger = null
        clearSignalWatchdog()
        clearStepTicker()
        RunSession.markStopped()
        stopForeground(STOP_FOREGROUND_REMOVE)
        // Belt-and-suspenders: explicitly cancel so no "Getting a GPS fix" / "Tracking
        // run" notification can linger if a re-post slipped in before `stopped`.
        getSystemService(NotificationManager::class.java).cancel(NOTIF_ID)
        stopSelf(lastStartId)
    }

    /** Foreground as a `location`-typed service. API 34+ requires the typed
     *  startForeground for a location FGS (the 2-arg overload throws); ServiceCompat
     *  picks the right form per API level. On API 34+ this throws [SecurityException]
     *  without a location grant. */
    private fun startLocationForeground() {
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
    }

    /** The notification "shape": the state fields that decide the title and which
     *  actions show, so [buildNotification] can cache one builder per shape and reuse
     *  it across every fix that keeps the same shape (only the content line changes). */
    private data class NotifShape(val recording: Boolean, val armed: Boolean, val paused: Boolean)

    private fun buildNotification(): Notification {
        val shape = NotifShape(recording, armed, RunSession.paused.value)
        // Per-fix hot path: while steady recording the shape (title + which
        // actions show) never changes, so reuse the builder cached for it and only
        // re-set the content line + build(). A state change (pause toggle, armed →
        // recording) rebuilds the builder once. Each PendingIntent is cached once —
        // they're identical every build.
        val builder = cachedBuilder?.takeIf { cachedShape == shape } ?: buildBuilder(shape).also {
            cachedBuilder = it
            cachedShape = shape
        }
        return builder.setContentText(currentNotifContent()).build()
    }

    /** Build the ongoing-notification [NotificationCompat.Builder] for one notification
     *  [shape] (the title + which actions show). Cached by [buildNotification] and
     *  reused across every fix that keeps the same shape; the content text is set by
     *  the caller per fix, not here. */
    private fun buildBuilder(shape: NotifShape): NotificationCompat.Builder {
        // One "acquiring" wording everywhere (the map pill + Start button both say
        // "Acquiring GPS"). The title/content id selection is the pure
        // [trackingNotifTitleRes]/[trackingNotifContentSpec] (unit-tested against the
        // real R.string ids); the call site only resolves.
        val builder = NotificationCompat.Builder(this, CHANNEL)
            .setContentTitle(getString(trackingNotifTitleRes(shape.recording, shape.armed, shape.paused)))
            // 1-color white route mark, silhouette only (icons README table).
            .setSmallIcon(R.drawable.ic_stat_mark)
            .setOngoing(true)
            .setContentIntent(tapPendingIntent())
            // Shown in full on a secure lock screen: the line carries only moving
            // time, distance and pace (never a location), so a runner can glance at
            // it without unlocking.
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            // The ~1 Hz progress refresh must never re-alert.
            .setOnlyAlertOnce(true)
        // Pause/Resume only while actually RECORDING. It drives the SAME
        // RunSession.paused flag the in-app Pause button toggles (one source of
        // truth), delivered as a service action (getService) handled in
        // onStartCommand. Deliberately NOT shown while merely armed: pausing a
        // not-yet-recording run would set the flag so its first real fixes get
        // dropped the instant recording begins.
        if (shape.recording) {
            builder.addAction(
                0,
                if (shape.paused) getString(R.string.track_action_resume) else getString(R.string.track_action_pause),
                if (shape.paused) resumePendingIntent() else pausePendingIntent(),
            )
        }
        // Stop, offered in every state. While recording OR armed it DEEP-LINKS into
        // the tracking screen (same EXTRA_OPEN_TRACKING as the tap) and does NOT
        // auto-save: an of-record save must go through the in-app flow (the Finish
        // review with its short-run note + the Save/Discard choice), so this lands the user on the
        // tracking screen to Stop and review the run. An auto-save action would bypass those
        // gates and could silently log a drift/accidental run.
        if (shape.recording || shape.armed) {
            builder.addAction(0, getString(R.string.track_action_stop), stopPendingIntent())
        } else {
            // Locate-only preview: there is no captured run to route through the save
            // gate, so Stop just tears the service down (ACTION_STOP), freeing the GPS
            // radio if the user opened the tracker to check a fix and walked away.
            builder.addAction(0, getString(R.string.track_action_stop), stopServicePendingIntent())
        }
        return builder
    }

    /** Tapping the ongoing notification reopens MainActivity ONTO the live tracking
     *  screen (EXTRA_OPEN_TRACKING is consumed by CoachScreen). SINGLE_TOP so an
     *  already-running Activity gets onNewIntent instead of a duplicate; UPDATE_CURRENT
     *  so the extra survives re-notify. Cached: identical every build. */
    private fun tapPendingIntent(): PendingIntent = tapPending ?: PendingIntent.getActivity(
        this, REQ_TAP, openTrackingIntent(),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    ).also { tapPending = it }

    /** Stop action target: deep-links into the tracking screen (distinct request code
     *  from the tap, whose Activity intent is otherwise identical). Cached. */
    private fun stopPendingIntent(): PendingIntent = stopPending ?: PendingIntent.getActivity(
        this, REQ_STOP, openTrackingIntent(),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    ).also { stopPending = it }

    /** Locate-only Stop: a direct ACTION_STOP to the service (getService), so tapping
     *  it tears the preview down without routing through the in-app save flow (there
     *  is nothing captured to save). Cached: identical every build. */
    private fun stopServicePendingIntent(): PendingIntent = stopServicePending ?: PendingIntent.getService(
        this, REQ_STOP_SERVICE,
        Intent(this, RunTrackingService::class.java).setAction(ACTION_STOP),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    ).also { stopServicePending = it }

    /** Pause action: sets RunSession.paused via a service intent. Cached (the pause
     *  and resume intents differ by action, so they are distinct PendingIntents). */
    private fun pausePendingIntent(): PendingIntent = pausePending ?: PendingIntent.getService(
        this, REQ_PAUSE,
        Intent(this, RunTrackingService::class.java).setAction(ACTION_PAUSE),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    ).also { pausePending = it }

    /** Resume action: clears RunSession.paused via a service intent. Cached. */
    private fun resumePendingIntent(): PendingIntent = resumePending ?: PendingIntent.getService(
        this, REQ_PAUSE,
        Intent(this, RunTrackingService::class.java).setAction(ACTION_RESUME),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    ).also { resumePending = it }

    /** The Activity intent that reopens the live tracking screen (shared by the tap
     *  contentIntent and the Stop action). SINGLE_TOP + the OPEN_TRACKING extra. */
    private fun openTrackingIntent(): Intent =
        Intent(this, MainActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP)
            .putExtra(MainActivity.EXTRA_OPEN_TRACKING, true)

    /**
     * Elapsed time + live distance from the captured track (06-run-tracking
     * §Behavior: "a notification shows elapsed + distance"), or null before a
     * two-fix track exists (the caller substitutes a state-appropriate literal).
     * The distance is the same factual shell-side haversine value the stat sheet
     * shows while recording.
     */
    private fun progressText(): String? {
        val pts = RunSession.points.value
        if (pts.size < 2) return null
        // Monotonic elapsed (RunSession.elapsedSec), immune to a wall-clock/NTP step
        // mid-run; and the incrementally-accumulated distance (RunSession.distanceKm),
        // so the notification never re-haversines the whole track on a refresh.
        // Honour the user's distance unit (same as the tracking screen / history),
        // so the notification doesn't contradict the rest of the app in km/mi.
        val unit = resolveDistanceUnit(AppSettings.distanceUnitOverride.value)
        return getString(
            R.string.track_notif_progress_figures,
            formatElapsed(RunSession.elapsedSec.value),
            runDistLabel(RunSession.distanceKm.value, unit),
        )
    }

    /** The current (rolling-window) pace in the user's unit, e.g. "5:45 /km", or null
     *  while there isn't enough recent ground to say (start, standing still, a fresh
     *  segment). Same figure as the tracker's "Pace now" readout. */
    private fun paceNowText(): String? {
        val pace = RunSession.currentPaceMinPerKm.value
        if (!pace.isFinite()) return null
        val unit = resolveDistanceUnit(AppSettings.distanceUnitOverride.value)
        return "${formatPaceMinutes(paceInUnit(pace, unit))} ${unit.paceSuffix}"
    }

    companion object {
        private const val ACTION_LOCATE = "app.milestone.action.LOCATE"
        private const val ACTION_START = "app.milestone.action.START"
        private const val ACTION_ARM = "app.milestone.action.ARM"
        private const val ACTION_STOP = "app.milestone.action.STOP"
        // Notification Pause/Resume actions (getService PendingIntents): toggle the
        // shared RunSession.paused flag from the lockscreen.
        private const val ACTION_PAUSE = "app.milestone.action.PAUSE"
        private const val ACTION_RESUME = "app.milestone.action.RESUME"
        private const val ACTION_START_NOW = "app.milestone.action.START_NOW"
        private const val EXTRA_REQUIRE_PRECISE = "app.milestone.extra.REQUIRE_PRECISE"
        private const val EXTRA_REQUIRE_MOVING = "app.milestone.extra.REQUIRE_MOVING"
        private const val CHANNEL = "run_tracking"
        private const val NOTIF_ID = 1
        // Distinct PendingIntent request codes so the tap contentIntent, the Pause/
        // Resume action, and the Stop action stay separate PendingIntents (the tap
        // and Stop share an identical Activity intent, so they must differ by code).
        private const val REQ_TAP = 0
        private const val REQ_PAUSE = 1
        private const val REQ_STOP = 2
        // Locate-only Stop: a getService ACTION_STOP intent; a distinct request code
        // so it never collides with the getActivity deep-link Stop (REQ_STOP).
        private const val REQ_STOP_SERVICE = 3

        // Service-side haptic cues (VIBRATE permission). Recording started: one firm
        // pulse. Signal lost: two pulses, so the two can be told apart in a pocket.
        private val RECORDING_STARTED_WAVEFORM = longArrayOf(0L, 300L)
        private val SIGNAL_LOST_WAVEFORM = longArrayOf(0L, 250L, 150L, 250L)

        // Small margin past the loss deadline so the watchdog never fires a hair early
        // and has to re-arm.
        private const val SIGNAL_CHECK_SLACK_MS = 250L

        // Guided-run step change: three quick pulses (distinct from the start buzz and
        // the two-pulse signal-loss warning), re-checked once a second.
        private val STEP_CHANGE_WAVEFORM = longArrayOf(0L, 150L, 100L, 150L, 100L, 150L)
        private const val STEP_TICK_MS = 1000L

        // Step-change beep: volume (percent of the notification stream's max) and
        // duration (ms) of one short cue.
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
            } catch (_: IllegalStateException) {
                // Background-start restriction (the tap comes from the visible tracker,
                // so not expected): the run stays armed and still starts on its gates.
            }
        }

        fun stop(ctx: Context) {
            // startService throws IllegalStateException when the app may not start
            // services right now (in the background, API 26+). A service that already
            // self-stopped is no problem: it is recreated and runs stopTracking().
            try {
                ctx.startService(
                    Intent(ctx, RunTrackingService::class.java).setAction(ACTION_STOP),
                )
            } catch (_: IllegalStateException) {
                // ACTION_STOP couldn't be delivered, so stopTracking() won't reset the
                // observable state machine. Do it here directly, mirroring
                // stopTracking()'s resets — clearing ONLY `tracking` would leave
                // `locating`/`armed` latched, sticking the tracker on "Acquiring GPS"
                // with no engine behind it.
                RunSession.markStopped()
            }
        }
    }
}

// Ongoing-notification copy. Top-level + internal so the state→copy mapping below
// is a pure function unit-testable without a running service: it selects R.string
// ids, and the caller (the service, which has a Context) resolves them, so the
// selection logic stays free of Android resource lookups.

/** The ongoing-notification TITLE resource id for a given tracking state. A paused
 *  recording selects the paused title so the lockscreen reflects the same paused
 *  state the in-app control shows. The state→resource selection lives HERE, in the
 *  pure layer, so a call-site id swap can't hide from a test behind the caller's
 *  own string literals (R.string constants resolve as plain ints in JVM unit
 *  tests; only getString needs a Context). The caller resolves getString(res). */
internal fun trackingNotifTitleRes(recording: Boolean, armed: Boolean, paused: Boolean): Int = when {
    recording && paused -> R.string.track_notif_paused_title
    recording -> R.string.track_notif_recording_title
    armed -> R.string.track_notif_armed_title
    else -> R.string.track_notif_locate_title
}

/** The ongoing-notification CONTENT for a given tracking state. [Literal] selects a
 *  fixed string resource; [Progress] is the raw "elapsed · dist" line while actively
 *  recording+unpaused plus the current pace ([Progress.paceNow], null when unknown),
 *  resolved through `track_notif_progress(_pace)`; [PausedProgress] is the
 *  parameterized "Paused · %1$s" resource with [PausedProgress.progress] as its
 *  format arg. Pure → unit-testable without a Context, pinning the state→resource
 *  wiring itself. */
internal sealed class NotifContentSpec {
    data class Literal(@StringRes val res: Int) : NotifContentSpec()
    data class Progress(val text: String, val paceNow: String?) : NotifContentSpec()
    data class PausedProgress(@StringRes val res: Int, val progress: String) : NotifContentSpec()
}

/** See [NotifContentSpec]. A paused recording folds [progress] into the parameterized
 *  paused line so a glance at the lockscreen shows the pause, and so the throttle key
 *  ([RunTrackingService.currentNotifContent]) changes on a pause/resume toggle and
 *  re-posts. A lost GPS signal while actively recording replaces the progress line
 *  with the signal-lost warning (the figures are frozen anyway); a pause outranks it. */
internal fun trackingNotifContentSpec(
    recording: Boolean,
    armed: Boolean,
    paused: Boolean,
    signalLost: Boolean,
    progress: String?,
    paceNow: String?,
): NotifContentSpec = when {
    recording && paused -> if (progress != null) NotifContentSpec.PausedProgress(R.string.track_notif_paused_content, progress) else NotifContentSpec.Literal(R.string.track_notif_paused_text)
    recording && signalLost -> NotifContentSpec.Literal(R.string.track_notif_signal_lost)
    recording -> if (progress != null) NotifContentSpec.Progress(progress, paceNow) else NotifContentSpec.Literal(R.string.track_notif_recording_hint)
    armed -> NotifContentSpec.Literal(R.string.track_notif_armed_text)
    else -> NotifContentSpec.Literal(R.string.track_notif_locate_text)
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

/** A high-accuracy fix stream (~1–2 s cadence screen-on, ~3–4 s screen-off).
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

/** Foreground (screen-on) recording cadence: fused request interval / min interval
 *  (ms), and the platform GPS interval (ms). ~1–2 s, full route fidelity. */
private const val FUSED_FG_INTERVAL_MS = 2000L
private const val FUSED_FG_MIN_INTERVAL_MS = 1000L
private const val PLATFORM_FG_INTERVAL_MS = 1000L

/** Background (screen-off) cadence: coarser to save battery while the phone is
 *  pocketed; the deliberate power/fidelity tradeoff. Long runs are thinned to a
 *  ~2–3 s cadence at save time anyway ([TRACK_DECIMATION_CAP]). */
private const val FUSED_BG_INTERVAL_MS = 4000L
private const val FUSED_BG_MIN_INTERVAL_MS = 3000L
private const val PLATFORM_BG_INTERVAL_MS = 4000L

/** Platform GPS minimum displacement (m) between updates: none, so every fix is
 *  delivered and standing-still wander is handled by the moving-speed floor
 *  ([MIN_MOVING_SPEED_MPS]) like the fused stream. */
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
                // No setMaxUpdateAgeMillis: the contract is that EVERY batched fix is
                // forwarded (a Doze/screen-off callback can carry several seconds-old
                // fixes that must all reach the route), so an age filter here would
                // silently drop them. The stale cached first-fix is instead rejected by
                // the armed/preview freshness gate in [isPlausibleLiveFix].
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
        // eagerly so the service's startTracking catch unwinds one way.
        if (!hasFineLocation(ctx)) {
            throw SecurityException("ACCESS_FINE_LOCATION not granted: GPS provider unavailable")
        }
        // A device without GNSS (Wi-Fi-only tablet) has no GPS provider; requesting it
        // throws IllegalArgumentException, which startTracking unwinds cleanly.
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
        val l = listener ?: return // not started → nothing to re-cadence
        // Re-register at the new interval. A runtime-permission revocation kills the
        // process, and the PROVIDERS_CHANGED receiver in RunTracking.kt covers a toggled
        // location service while the screen is composed, so a thrown SecurityException here
        // just means no more fixes, never a crash.
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
        PLATFORM_MIN_DISTANCE_M,
        l,
        Looper.getMainLooper(),
    )

    override fun stop() {
        listener?.let { lm.removeUpdates(it) }
        listener = null
    }
}
