package app.milestone

import kotlin.concurrent.Volatile

/**
 * One location fix as the platform delivered it. [wallMs]: the fix's wall-clock
 * time (the saved `observed_at`). [monoNanos]: its own stamp on the [monotonicMillis]
 * clock, in nanoseconds (Android `Location.elapsedRealtimeNanos`; iOS the CLLocation
 * timestamp moved onto that clock at delivery). [accuracyM]: the horizontal accuracy
 * radius, null when the fix carries none.
 */
internal class LiveFix(
    val lat: Double,
    val lon: Double,
    val wallMs: Long,
    val monoNanos: Long,
    val accuracyM: Double?,
)

/** Whether a fix may reach the live display: accuracy within [CoreConstants.MAX_GPS_ACCURACY_M]
 *  (the core's of-record bound) and, unless [recording], fresh enough per
 *  [armedFixPassesAgeGate] ([ageMs] on the monotonic clock). */
internal fun isPlausibleLiveFix(accuracyM: Double?, ageMs: Long, recording: Boolean, batchSize: Int): Boolean {
    if (!recording && !armedFixPassesAgeGate(ageMs, batchSize)) return false
    return accuracyM != null && accuracyM <= CoreConstants.MAX_GPS_ACCURACY_M
}

/** A one-shot deadline on the main thread. */
internal interface RecorderTimer {
    fun schedule(delayMs: Long)

    fun cancel()
}

/** A cue the recorder gives a runner who may not be looking (screen off, pocket). */
internal enum class RecorderCue {
    /** Armed → recording, hands-free. */
    RecordingStarted,

    /** No accepted fix for [SIGNAL_LOSS_MS] while recording. */
    SignalLost,

    /** A guided run's step changed. */
    StepChange,
}

/**
 * What [RunRecorder] needs from the platform: the location stream, the run's
 * system surface (Android: the ongoing notification; iOS: the Live Activity hook),
 * the cues and the main-thread timers. Every call is on the main thread.
 */
internal interface RecorderHost {
    /** Now on the [monotonicMillis] clock, in nanoseconds and in milliseconds. */
    fun nowNanos(): Long

    fun nowMs(): Long

    /** Put the run's surface up and start the location stream into
     *  [RunRecorder.onFix]. False when it can't run (no precise grant, no GPS
     *  provider); a failure after it started goes to [RunRecorder.onStreamFailed]. */
    fun startStream(): Boolean

    /** Stop the location stream; a safe no-op when it isn't running. */
    fun stopStream()

    /** Start a fresh run's crash-safe file ([ActiveRunStore.begin]). */
    fun openSidecar()

    /** An arm reached a live recording: re-assert the surface as it is. */
    fun reassertSurface()

    /** Show the current state on the surface, if what it shows changed. */
    fun refresh()

    /** The next [refresh] shows the state even when it looks unchanged. */
    fun invalidateSurface()

    fun cue(cue: RecorderCue)

    /** Free what the cues hold (the tone generator). */
    fun releaseCues()

    /** The stream is down for good: take the surface down. [userStop]: a stop the
     *  runner asked for, not a failed start. */
    fun finished(userStop: Boolean)

    /** A deadline that fires on time with the screen off. While one is pending, a
     *  request for the same or a later moment keeps it (the action re-arms for the
     *  rest); an earlier one replaces it. */
    fun wakeTimer(tag: String, action: () -> Unit): RecorderTimer

    /** A plain main-thread post, for the once-a-second step tick. */
    fun ticker(action: () -> Unit): RecorderTimer
}

/**
 * The recording state machine both platforms run: locate-only preview, armed start,
 * recording, pause, the signal-loss watchdog and the guided-run step ticker. It
 * feeds accepted fixes into [RunSession] and mirrors them to [ActiveRunStore]; the
 * platform ([RecorderHost]) owns the location stream and the system surface.
 * Android: RunTrackingService; iOS: the CoreLocation tracker. Main thread only.
 */
internal class RunRecorder(private val host: RecorderHost) {
    // Locate-only vs recording. Opening the tracker starts the stream to preview fix
    // quality, but records NOTHING until the user taps Start; no route point, no
    // crash-durable sidecar line. `recording` flips true on a recording start, or at
    // the armed trigger ([beginRecording]).
    var recording = false
        private set

    // A stream was started ([start]) on this recorder; Android tells a live
    // locate/armed/recording session from a service instance a stray intent created.
    var started = false
        private set

    // Armed: Start was tapped before a lock. The stream is acquiring GPS (like a
    // locate) but flips to recording AUTOMATICALLY at the first fix that clears
    // [armTrigger]'s gates. The recorder owns [RunSession.armed]; this stream's
    // `armed` is the surface copy's. Both are cleared the instant recording begins
    // ([beginRecording]) or on any teardown ([clearArmState]).
    var armed = false
        private set
    private var armTrigger: ArmTrigger? = null

    // Set once the stream is tearing down. A location result already queued on the
    // main thread can still reach onFix AFTER the stop; without this guard it would
    // re-post the surface that nothing then takes down ("Finding GPS…" stuck).
    @Volatile var stopped = false
        private set

    // The [RunSession.generation] this stream started under; onFix stands down once
    // it differs (see [RunSession.generation]).
    private var sessionGeneration = 0L

    // Screen-off signal-loss watchdog. The in-app pill only ages the last fix while
    // the tracker is visible, so a phone-in-pocket runner would never feel a GPS
    // drop; the recorder owns this check instead. Re-armed on every accepted
    // recording fix; when [SIGNAL_LOSS_MS] passes with no fix it cues once and
    // refreshes the surface with the signal-lost line. [signalLost] latches the
    // episode so it cues once, and clears on the next accepted fix.
    var signalLost = false
        private set
    private val signalWake = host.wakeTimer("lollo:signal-watchdog") { checkSignal() }

    // Guided-run step ticker: once a second while recording a planned, structured
    // run, advance [RunSession]'s step machine on the monotonic clock and cue each
    // step change, so a pocketed phone with the screen off still says "rep 3 starts
    // now". The 1 s tick refreshes the countdown; a wake timer at the timed step's
    // end makes the change cue on time with the screen off.
    private val stepTicker = host.ticker { tickSteps() }
    private val stepWake = host.wakeTimer("lollo:step-guide") { tickSteps() }

    /**
     * Start a locate preview ([record] false), a recording ([record] true: the
     * crash-resume start), or an armed preview ([arming]) that begins recording at the
     * first fix clearing [ArmTrigger]'s gates. Every failure goes through
     * [unwindLocationFailure], which also clears the tap's armed flag ([armRequested]).
     * An arm reaching a live recording only re-asserts the surface, so it can't demote
     * the run and merge two runs in one sidecar.
     */
    fun start(record: Boolean, arming: Boolean = false, requirePrecise: Boolean = false, requireMoving: Boolean = false) {
        if (arming && recording) {
            RunSession.setArmed(false)
            host.reassertSurface()
            return
        }
        started = true
        recording = record
        stopped = false
        sessionGeneration = RunSession.generation
        // Publish the arm state before the surface goes up, so its copy and the
        // screen agree from the first frame.
        if (arming) {
            armTrigger = ArmTrigger(requirePrecise = requirePrecise, requireMoving = requireMoving)
            armed = true
            RunSession.setArmed(true)
        } else {
            clearArmState()
        }
        if (!host.startStream()) {
            unwindLocationFailure()
            return
        }
        RunSession.streamStarted(recording = record)
        // A direct recording start (crash-resume) watches for signal loss too.
        if (record) armSignalWatchdog()
    }

    /** Armed → recording now, skipping the start gates. Only the live armed stream
     *  of the current session may begin. */
    fun startNow() {
        if (armed && !recording && !stopped && RunSession.generation == sessionGeneration) beginRecording(emptyList())
    }

    /** Pause/Resume from the system surface. Only a live recording may toggle: on a
     *  locate/armed stream the paused flag would drop the run's first real fixes. */
    fun setPaused(paused: Boolean) {
        if (recording) RunSession.setPaused(paused)
    }

    /** The runner's stop (or a teardown with no run to keep): stream and surface down. */
    fun stop() {
        haltStream()
        host.finished(userStop = true)
    }

    /** The stream failed after it started (Android: the fused client's async error;
     *  iOS: CoreLocation reporting the grant gone). */
    fun onStreamFailed() = unwindLocationFailure()

    /** One fix → preview state always; a route point + sidecar line only while
     *  recording. [batchSize]: how many fixes the platform delivered together. */
    fun onFix(fix: LiveFix, batchSize: Int) {
        if (stopped) return
        // Stale session (see [RunSession.generation]): stand down without touching
        // any state, not even the preview lastFix the reset just cleared.
        if (RunSession.generation != sessionGeneration) {
            unwindLocationFailure(failed = false)
            return
        }
        // Gate implausible fixes for the live display + map ([armedFixPassesAgeGate],
        // [CoreConstants.MAX_GPS_ACCURACY_M]); the core's QC still gates the of-record figures.
        val accuracyM = fix.accuracyM
        val ageMs = (host.nowNanos() - fix.monoNanos) / 1_000_000
        if (accuracyM == null || !isPlausibleLiveFix(accuracyM, ageMs, recording = recording, batchSize = batchSize)) return
        val point = GpsPoint(lat = fix.lat, lon = fix.lon, observedAt = fix.wallMs / 1000, accuracyM = accuracyM)
        // Preview: drives the "GPS lock · ±N m" readout, the self-location dot
        // before Start, and the fresh-lock/signal-loss pill. Set on EVERY accepted
        // fix (recording, armed, or locate-only). Uses the fix's OWN monotonic stamp,
        // the SAME timebase [isFreshLock] ages against: a ~10 s-old batched fix must
        // not read "fresh lock" for ~20 s, as the delivery time would have made it.
        RunSession.previewFix(point, fix.monoNanos / 1_000_000)
        if (recording) {
            onSignalFix()
            // Paused: drop the fix so the paused span records no route; the stream
            // stays up. [RunSession.setPaused] already marked the boundary.
            if (RunSession.paused.value) return
            record(point, fix.monoNanos)
            // Refresh the surface so a glance at the lock screen confirms GPS is
            // still capturing; the host re-posts only when what it shows changed.
            host.refresh()
            return
        }
        // Armed: feed the fix to the trigger. A non-null seed means the gates are
        // satisfied, begin recording FROM that seed (moving-start run-up kept);
        // otherwise stay armed (static surface, at most one re-post).
        if (armed) {
            val seed = armTrigger?.offer(point, fix.monoNanos)
            if (seed != null) {
                beginRecording(seed)
            } else {
                host.refresh()
            }
            return
        }
        // Locate-only: nothing is recorded until the user taps Start. The preview
        // surface is static, so [RecorderHost.refresh] re-posts it at most once.
        host.refresh()
    }

    /** Armed → recording: the trigger's gates are satisfied. Clears the arm state,
     *  flips to recording, and flushes the [seed] run-up fixes through the session +
     *  sidecar so a moving-start's approach distance is kept. The sidecar opens
     *  HERE, not on Start: a crash while merely armed leaves no sidecar (no false
     *  recovery prompt), and the sidecar exists from the first REAL recorded fix. */
    private fun beginRecording(seed: List<Pair<GpsPoint, Long>>) {
        clearArmState()
        recording = true
        RunSession.recordingStarted()
        host.openSidecar()
        for ((pt, nanos) in seed) record(pt, nanos)
        // A planned, structured run starts its step guide with the recording (the
        // run-up seed counts as already covered, so the first step starts here).
        RunSession.startSteps(host.nowMs())
        tickSteps()
        // Force a re-post: the title/copy switch from armed to "Tracking run".
        host.invalidateSurface()
        host.refresh()
        // Armed → recording happens hands-free (often phone already pocketed), so
        // confirm it with a cue that works screen-off.
        host.cue(RecorderCue.RecordingStarted)
        armSignalWatchdog()
    }

    /** Feed [point] into the session and, once it is accepted, into the crash-durable
     *  sidecar (off the main thread), so a kill mid-run is recoverable. A fix the
     *  session dropped as non-monotonic is not mirrored. */
    private fun record(point: GpsPoint, nanos: Long) {
        if (RunSession.add(point, nanos)) ActiveRunStore.append(point, segmentStart = RunSession.isSegmentStart(point))
    }

    /** Stop the stream and clear every engine-state flag. Marks [stopped] first, so a
     *  fix already queued on the main thread returns without re-posting the surface.
     *  Shared by every teardown path (Android's onDestroy calls it directly). */
    fun haltStream() {
        stopped = true
        host.stopStream()
        recording = false
        clearArmState()
        clearSignalWatchdog()
        clearStepTicker()
        RunSession.markStopped()
    }

    /** Unwind a failed location start or a stale session, so the stream never
     *  lingers with its surface up and no fixes. [failed] publishes
     *  [RunSession.locationFailed] so the screen stops re-launching the preview;
     *  false for the stale-session stand-down. Idempotent once [stopped]. */
    private fun unwindLocationFailure(failed: Boolean = true) {
        if (stopped) return
        haltStream()
        if (failed) RunSession.markLocationFailed()
        host.finished(userStop = false)
    }

    /** Every teardown path clears the arm state too, so a crash/stop while merely
     *  armed never leaves the UI stuck armed. */
    private fun clearArmState() {
        armed = false
        armTrigger = null
        RunSession.setArmed(false)
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
            (last + SIGNAL_LOSS_MS + 1L - host.nowMs()).coerceAtLeast(0L)
        }
        signalWake.schedule(delayMs + SIGNAL_CHECK_SLACK_MS)
    }

    /** Watchdog body: on a due loss while recording+unpaused, cue once and show the
     *  signal-lost line. A paused run gets no cue (no fixes are expected to matter),
     *  but is re-checked so a resume into a dead signal still warns. Not yet due (a
     *  fix landed in between) → re-arm for the remainder. */
    private fun checkSignal() {
        if (stopped || !recording) return
        if (!signalLossDue(RunSession.lastFixElapsedMs.value, host.nowMs())) {
            armSignalWatchdog()
            return
        }
        if (RunSession.paused.value) {
            signalWake.schedule(SIGNAL_LOSS_MS)
            return
        }
        if (!signalLost) {
            signalLost = true
            host.cue(RecorderCue.SignalLost)
            host.refresh()
        }
    }

    /** An accepted fix while recording: end any signal-loss episode (re-posting the
     *  normal progress line) and push the watchdog out again. */
    private fun onSignalFix() {
        if (signalLost) {
            signalLost = false
            host.invalidateSurface()
        }
        armSignalWatchdog()
    }

    private fun clearSignalWatchdog() {
        signalWake.cancel()
        signalLost = false
    }

    private fun tickSteps() {
        stepTicker.cancel()
        if (stopped || !recording || RunSession.stepProgress.value == null) {
            stepWake.cancel()
            return
        }
        if (RunSession.tickSteps(host.nowMs())) {
            // The pending deadline belonged to the step that just ended.
            stepWake.cancel()
            host.cue(RecorderCue.StepChange)
        }
        host.refresh()
        val progress = RunSession.stepProgress.value
        if (progress != null && !progress.finished) {
            stepTicker.schedule(STEP_TICK_MS)
            // Distance steps advance on fixes, which wake the CPU anyway.
            val remainingSec = progress.remainingSec
            if (remainingSec != null) stepWake.schedule((remainingSec * 1000L).coerceAtLeast(STEP_TICK_MS)) else stepWake.cancel()
        } else {
            stepWake.cancel()
        }
    }

    private fun clearStepTicker() {
        stepTicker.cancel()
        stepWake.cancel()
        host.releaseCues()
    }

    companion object {
        /** The runner tapped an armed Start: the session shows armed from this frame,
         *  before the platform's recorder starts (Android starts its service
         *  asynchronously). That recorder's [start] confirms it; a failed start clears it. */
        fun armRequested() = RunSession.setArmed(true)

        /** The runner's stop cannot reach a recorder (Android: no service runs and none
         *  may be started from the background): the session shows the stopped state
         *  [haltStream] would leave, so "Finding GPS…" never latches with no engine. */
        fun stopUndelivered() = RunSession.markStopped()

        // Small margin past the loss deadline so the watchdog never fires a hair early
        // and has to re-arm.
        private const val SIGNAL_CHECK_SLACK_MS = 250L

        private const val STEP_TICK_MS = 1000L
    }
}
