package app.milestone

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.launch

// What every platform's [RecorderHost] does alike: the cue patterns, the
// screen-off-safe deadline, the app-removed rule and the pause refresh.

/**
 * How a cue feels: buzzes of [pulseMs] with [gapMs] between them, plus a short
 * beep for [beep]. The counts tell the cues apart in a pocket: one firm pulse for
 * the start, two for a lost signal, three quick ones (and the beep) for a guided
 * run's step change. A platform without control over a buzz's length plays the
 * count.
 */
internal class CuePattern(val pulseMs: List<Long>, val gapMs: Long, val beep: Boolean)

internal val RecorderCue.pattern: CuePattern
    get() = when (this) {
        RecorderCue.RecordingStarted -> CuePattern(listOf(300L), gapMs = 0L, beep = false)
        RecorderCue.SignalLost -> CuePattern(listOf(250L, 250L), gapMs = 150L, beep = false)
        RecorderCue.StepChange -> CuePattern(listOf(150L, 150L, 150L), gapMs = 100L, beep = true)
    }

/**
 * One deadline on the [AppClock.monotonicMillis] clock that fires on time with the
 * screen off. While one is pending, a request for the same or a LATER moment keeps
 * it: the earlier one fires and the action re-arms for the remainder, so the
 * per-fix re-arm costs nothing. An earlier request replaces it.
 */
internal abstract class EarliestWinsTimer(private val action: () -> Unit) : RecorderTimer {
    private var dueAt = 0L

    /** Set the platform's deadline [delayMs] from now; it calls [fire]. */
    protected abstract fun arm(delayMs: Long)

    /** Take the pending deadline down. */
    protected abstract fun disarm()

    /** The deadline came: run the action once. */
    protected fun fire() {
        cancel()
        action()
    }

    final override fun schedule(delayMs: Long) {
        val delay = delayMs.coerceAtLeast(0L)
        val at = AppClock.monotonicMillis() + delay
        if (dueAt != 0L && dueAt <= at) return
        cancel()
        dueAt = at
        arm(delay)
    }

    final override fun cancel() {
        if (dueAt == 0L) return
        dueAt = 0L
        disarm()
    }
}

/**
 * The app's task went away while the process may live on (Android: a swipe in
 * Recents; iOS: the app terminating from the background). A live or armed run must
 * survive it: queued sidecar writes flush so nothing accepted is lost, and the
 * sidecar stays for the resume offer. A locate-only preview has no run to keep, so
 * it stops rather than leave the GPS radio on.
 */
internal fun RunRecorder.onAppRemoved() {
    if (recording || armed) ActiveRunStore.flush() else stop()
}

/**
 * Refresh the run's surface on every pause or resume from now on, until [scope]
 * ends: the in-app Pause and the surface's own Pause/Resume write the same flag, so
 * both show the same. drop(1) skips the current value; Main.immediate subscribes at
 * once on the calling main thread, so no later pause is missed.
 */
internal fun refreshSurfaceOnPause(scope: CoroutineScope, refresh: () -> Unit) {
    scope.launch(Dispatchers.Main.immediate) {
        RunSession.paused.drop(1).collect { refresh() }
    }
}
