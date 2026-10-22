package app.milestone

import android.content.res.Resources
import java.util.Locale

/**
 * The guided-workout step machine for a planned run. A prescribed
 * [RunStructureView] becomes an ordered list of [RunStep]s (warm-up → rep k of N
 * with recovery between → cool-down → strides, or the run/walk bouts of a beginner
 * session). [RunStepMachine] walks them on two clocks the tracker already has:
 * ACTIVE time (monotonic, frozen while paused) and the live run distance. Pure: no
 * Android, no SystemClock; the foreground service feeds it `elapsedRealtime` and
 * [RunSession.distanceKm], so it keeps stepping with the screen off.
 */
internal enum class RunStepKind { WarmUp, Main, Rep, Recovery, CoolDown, Stride, StrideRecovery, Run, Walk }

/**
 * One step of a guided run. Exactly one of [durationSec] / [distanceM] is > 0 for a
 * bounded step; both 0 with [matchPreviousRep] means "as long as the last rep took";
 * both 0 otherwise is an OPEN step (the continuous part of a run) that only ends on
 * Skip step. [number] / [count] are 1-based "rep k of N" (0 when not numbered).
 */
internal data class RunStep(
    val kind: RunStepKind,
    val number: Int = 0,
    val count: Int = 0,
    val durationSec: Int = 0,
    val distanceM: Int = 0,
    val matchPreviousRep: Boolean = false,
) {
    val open: Boolean get() = durationSec <= 0 && distanceM <= 0 && !matchPreviousRep
}

/**
 * The ordered steps for [s]. Run/walk bouts win over [RunStructureView.reps] when
 * both are present (a run/walk session carries its bouts in both forms). A session
 * with no reps and no bouts gets one OPEN [RunStepKind.Main] step, the continuous
 * run between the warm-up and the cool-down/strides. Empty when nothing structured
 * remains (e.g. an all-zero structure).
 */
internal fun buildRunSteps(s: RunStructureView): List<RunStep> {
    val out = ArrayList<RunStep>()
    if (s.warmup_min > 0) out += RunStep(RunStepKind.WarmUp, durationSec = s.warmup_min * 60)
    val bouts = s.run_walk.filter { it.run_sec > 0 || it.walk_sec > 0 }
    if (bouts.isNotEmpty()) {
        bouts.forEachIndexed { i, b ->
            if (b.run_sec > 0) out += RunStep(RunStepKind.Run, i + 1, bouts.size, durationSec = b.run_sec)
            if (b.walk_sec > 0) out += RunStep(RunStepKind.Walk, i + 1, bouts.size, durationSec = b.walk_sec)
        }
    } else if (s.reps > 0 && (s.rep_m > 0 || s.rep_sec > 0)) {
        for (k in 1..s.reps) {
            out += if (s.rep_m > 0) {
                RunStep(RunStepKind.Rep, k, s.reps, distanceM = s.rep_m)
            } else {
                RunStep(RunStepKind.Rep, k, s.reps, durationSec = s.rep_sec)
            }
            if (k < s.reps) {
                when {
                    s.recovery_m > 0 -> out += RunStep(RunStepKind.Recovery, k, s.reps - 1, distanceM = s.recovery_m)
                    s.recovery_sec > 0 -> out += RunStep(RunStepKind.Recovery, k, s.reps - 1, durationSec = s.recovery_sec)
                    s.recovery_equal_to_rep -> out += RunStep(RunStepKind.Recovery, k, s.reps - 1, matchPreviousRep = true)
                }
            }
        }
    } else if (s.warmup_min > 0 || s.cooldown_min > 0 || s.strides_reps > 0) {
        out += RunStep(RunStepKind.Main)
    }
    if (s.cooldown_min > 0) out += RunStep(RunStepKind.CoolDown, durationSec = s.cooldown_min * 60)
    if (s.strides_reps > 0 && s.strides_sec > 0) {
        for (k in 1..s.strides_reps) {
            out += RunStep(RunStepKind.Stride, k, s.strides_reps, durationSec = s.strides_sec)
            if (k < s.strides_reps && s.strides_recovery_sec > 0) {
                out += RunStep(RunStepKind.StrideRecovery, k, s.strides_reps - 1, durationSec = s.strides_recovery_sec)
            }
        }
    }
    return out
}

/** A snapshot of the guide: the current step (null once every step is done), its
 *  0-based [index] of [total], what is left of it, and the step after it. */
internal data class StepProgress(
    val step: RunStep?,
    val index: Int,
    val total: Int,
    /** Seconds left in a timed step (null for distance/open steps). */
    val remainingSec: Int?,
    /** Metres left in a distance step (null for timed/open steps). */
    val remainingM: Int?,
    val next: RunStep?,
) {
    val finished: Boolean get() = step == null
}

/**
 * Walks [steps] on active time and distance. [advance] is fed the monotonic clock
 * ([nowMs]), the live run distance and the pause flag: while paused the active clock
 * does not move (and the tracker records no distance), so a step freezes exactly
 * where it was. It returns true when the step changed, the tracker's cue to buzz and
 * beep. A large tick can cross several short steps at once; each is consumed in turn
 * with its own overshoot carried forward, so the guide never falls behind the run.
 */
internal class RunStepMachine(val steps: List<RunStep>) {
    private var index = 0
    private var started = false
    private var lastNowMs = 0L
    private var activeMs = 0L
    private var stepStartActiveMs = 0L
    private var stepStartDistanceM = 0.0
    private var lastDistanceM = 0.0
    // Active ms the most recent rep took, for a "recover as long as the rep" step.
    private var lastRepMs = 0L

    val finished: Boolean get() = index >= steps.size

    /** Begin the first step at [nowMs] / [distanceM]. Idempotent. */
    fun start(nowMs: Long, distanceM: Double) {
        if (started) return
        started = true
        lastNowMs = nowMs
        lastDistanceM = distanceM
        stepStartActiveMs = 0L
        stepStartDistanceM = distanceM
    }

    /** Advance the clocks; true when at least one step boundary was crossed. */
    fun advance(nowMs: Long, distanceM: Double, paused: Boolean): Boolean {
        if (!started) {
            start(nowMs, distanceM)
            return false
        }
        val dt = (nowMs - lastNowMs).coerceAtLeast(0L)
        lastNowMs = nowMs
        if (!paused) activeMs += dt
        // Distance never runs backwards (a reset session would restart the machine).
        if (distanceM > lastDistanceM) lastDistanceM = distanceM
        var changed = false
        while (!finished) {
            val step = steps[index]
            // An open step (null) never auto-ends; a bounded one ends once its time
            // or distance is used up.
            if (stepEnd(step) != true) break
            completeCurrent(boundaryActiveMs(step), boundaryDistanceM(step))
            changed = true
        }
        return changed
    }

    /** Skip the current step now (the runner's "Skip step"); true when it moved. */
    fun skip(nowMs: Long, distanceM: Double, paused: Boolean): Boolean {
        advance(nowMs, distanceM, paused)
        if (finished) return false
        completeCurrent(activeMs, lastDistanceM)
        return true
    }

    fun progress(): StepProgress {
        val step = steps.getOrNull(index)
        val remSec = step?.let { s -> stepDurationMs(s)?.let { d -> ((d - (activeMs - stepStartActiveMs) + 999) / 1000).toInt().coerceAtLeast(0) } }
        val remM = step?.takeIf { it.distanceM > 0 }?.let { s ->
            Math.ceil(s.distanceM - (lastDistanceM - stepStartDistanceM)).toInt().coerceAtLeast(0)
        }
        return StepProgress(step, index, steps.size, remSec, remM, steps.getOrNull(index + 1))
    }

    /** Timed length of [s] in active ms, or null for a distance/open step. */
    private fun stepDurationMs(s: RunStep): Long? = when {
        s.durationSec > 0 -> s.durationSec * 1000L
        s.matchPreviousRep -> lastRepMs
        else -> null
    }

    /** True/false for a bounded step's end, null for an open step (never auto-ends). */
    private fun stepEnd(s: RunStep): Boolean? {
        stepDurationMs(s)?.let { return activeMs - stepStartActiveMs >= it }
        if (s.distanceM > 0) return lastDistanceM - stepStartDistanceM >= s.distanceM
        return null
    }

    /** The active-clock instant the step's time ran out (carry the overshoot), or
     *  "now" for a distance step. */
    private fun boundaryActiveMs(s: RunStep): Long =
        stepDurationMs(s)?.let { stepStartActiveMs + it } ?: activeMs

    /** The distance at which the step's metres ran out, or "now" for a timed step. */
    private fun boundaryDistanceM(s: RunStep): Double =
        if (s.distanceM > 0) stepStartDistanceM + s.distanceM else lastDistanceM

    private fun completeCurrent(endActiveMs: Long, endDistanceM: Double) {
        val step = steps[index]
        if (step.kind == RunStepKind.Rep) lastRepMs = (endActiveMs - stepStartActiveMs).coerceAtLeast(0L)
        index++
        stepStartActiveMs = endActiveMs
        stepStartDistanceM = endDistanceM
    }
}

/** Remaining-time readout for a step: "m:ss". */
internal fun formatStepSeconds(sec: Int): String {
    val s = sec.coerceAtLeast(0)
    return String.format(Locale.US, "%d:%02d", s / 60, s % 60)
}

/** A step's title, e.g. "Rep 2 of 6", "Warm-up", "Walk 3 of 8". */
internal fun runStepTitle(res: Resources, step: RunStep): String = when (step.kind) {
    RunStepKind.WarmUp -> res.getString(R.string.track_step_warmup)
    RunStepKind.Main -> res.getString(R.string.track_step_main)
    RunStepKind.Rep -> res.getString(R.string.track_step_rep, step.number, step.count)
    RunStepKind.Recovery -> res.getString(R.string.track_step_recovery)
    RunStepKind.CoolDown -> res.getString(R.string.track_step_cooldown)
    RunStepKind.Stride -> res.getString(R.string.track_step_stride, step.number, step.count)
    RunStepKind.StrideRecovery -> res.getString(R.string.track_step_stride_recovery)
    RunStepKind.Run -> res.getString(R.string.track_step_run, step.number, step.count)
    RunStepKind.Walk -> res.getString(R.string.track_step_walk, step.number, step.count)
}

/** What is left of the current step: "2:41 left", "320 m left", or the open-step
 *  hint. Empty once finished. */
internal fun runStepRemaining(res: Resources, p: StepProgress): String = when {
    p.step == null -> ""
    p.remainingSec != null -> res.getString(R.string.track_step_left, formatStepSeconds(p.remainingSec))
    p.remainingM != null -> res.getString(R.string.track_step_left, res.getString(R.string.track_step_metres, p.remainingM))
    else -> res.getString(R.string.track_step_open)
}

/** The one-line guide shown in the notification and read by TalkBack:
 *  "Rep 2 of 6 · 320 m left", or the all-done line. */
internal fun runStepLine(res: Resources, p: StepProgress): String =
    if (p.step == null) {
        res.getString(R.string.track_step_all_done)
    } else {
        res.getString(R.string.track_step_line, runStepTitle(res, p.step), runStepRemaining(res, p))
    }
