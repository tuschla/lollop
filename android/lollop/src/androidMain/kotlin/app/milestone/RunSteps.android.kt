package app.milestone

import android.content.res.Resources

// The step machine's text (RunSteps.kt), from the Android string resources.

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
    p.remainingSec != null -> res.getString(R.string.track_step_left, formatElapsed(p.remainingSec.toLong()))
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
