package app.milestone

import app.milestone.resources.Res
import app.milestone.resources.plan_face_effort
import app.milestone.resources.track_plan_targets_line
import app.milestone.resources.track_target_hr
import app.milestone.resources.track_target_pace
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.IO
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.withContext

// The run tracker's pure logic: the finish flow's state machine, short-run and
// pace rules, the plan a run follows, and the recorded track as route segments.

/**
 * A captured run awaits save when the service no longer records yet fixes are still
 * held (a failed save or an unconfirmed stop); the locate preview records none.
 * Derived from the process-scoped [RunSession], not `remember`ed, so Save/Discard
 * survive rotation and Back+reopen instead of falling to Back (clears the sidecar)
 * or Start (truncates it).
 */
internal fun hasUnsavedCapture(tracking: Boolean, hasPoints: Boolean): Boolean =
    !tracking && hasPoints

/**
 * Claim [saving] for one save: the false→true compareAndSet admits exactly one caller
 * among a double Save tap, Save vs stopAndLog, and blocks a Discard under an in-flight
 * send. [ownsSaving]: the caller already holds the claim (stopAndLog → logCaptured).
 * Released by [RunSession.reset] or a save-failure path.
 */
internal fun claimSaving(saving: MutableStateFlow<Boolean>, ownsSaving: Boolean = false): Boolean =
    ownsSaving || saving.compareAndSet(expect = false, update = true)

/**
 * Save a captured run as one unit that survives the caller's scope being cancelled
 * (rotation): [send] persists (appends to the event log), then [clearSidecar] and
 * [resetSession] run in the same `NonCancellable` IO block, so a persisted run is
 * never left re-savable. [finish] (navigation) may be skipped on cancel, harmlessly.
 * A [CancellationException] is re-thrown, never sent to [onFailure]; any other
 * exception means [send] failed before clear/reset, with the run intact for a retry.
 */
internal suspend fun runSaveCriticalSection(
    send: suspend () -> Unit,
    clearSidecar: () -> Unit,
    resetSession: () -> Unit,
    finish: () -> Unit,
    onFailure: () -> Unit,
) {
    try {
        withContext(Dispatchers.IO + NonCancellable) {
            send()
            clearSidecar()
            resetSession()
        }
        withContext(NonCancellable) { finish() }
    } catch (e: CancellationException) {
        throw e
    } catch (e: Exception) {
        PlatformLog.w("RunSave", "run not saved, kept for a retry", e)
        onFailure()
    }
}

/** Below this measured distance (km) the Finish review notes a short run, so a
 *  drift/accidental "run" is a conscious keep rather than silent weekly-km / spike
 *  pollution. */
internal const val MIN_RUN_KM = 0.5

/** Below this measured MOVING duration (seconds) the review notes a short run too:
 *  a ≥0.5 km blip logged over ~90 s is as likely to be noise as a sub-0.5 km one. */
internal const val MIN_RUN_SEC = 180L

/** True when a captured run is under the keep threshold on distance OR moving time
 *  ([RunSession.elapsedSec], which excludes pauses and standing still, so a short
 *  run padded by a long pause still counts as short). */
internal fun isShortRun(km: Double, movingSec: Long): Boolean =
    km < MIN_RUN_KM || movingSec < MIN_RUN_SEC

/** The keep threshold as the short-run note states it: a round figure in the user's
 *  unit ("0.3 mi", "0.5 km"), not a converted 500 m ("0.31 mi"); the check itself
 *  stays at [MIN_RUN_KM]. */
internal fun shortRunThresholdLabel(unit: DistanceUnit): String =
    "${trimDecimal(metersToDisplay(MIN_RUN_KM * 1000.0, unit))} ${unit.distanceLabel}"

/** Where the Finish review is: not shown, reviewing the paused run, or confirming a
 *  discard. An enum so `rememberSaveable` keeps it across rotation. */
internal enum class FinishStep { Closed, Review, ConfirmDiscard }

/** What the runner did on the recording controls / Finish review. */
internal enum class FinishAction { Stop, Resume, Save, Discard, CancelDiscard, ConfirmDiscard }

/** The single side effect a Finish transition asks the screen to perform. */
internal enum class FinishEffect { None, PauseRecording, ResumeRecording, SaveRun, DiscardRun }

internal data class FinishTransition(val step: FinishStep, val effect: FinishEffect)

/**
 * The Finish review as a pure state machine, so its guarantees are JVM-testable
 * without Compose:
 *  - Stop never saves: it pauses recording and opens the review.
 *  - Resume closes the review and un-pauses the SAME live run (no restart, no new
 *    crash-recovery sidecar; the service never stopped).
 *  - Save is the only path to the of-record save (the existing stop → persist
 *    pipeline; a short run is flagged inline on the review, never a second prompt).
 *  - Discard only ASKS; the run is deleted solely by ConfirmDiscard from the confirm
 *    step, and CancelDiscard returns to the review.
 * Any action that doesn't apply to the current step is a no-op (a stale tap after a
 * recomposition can't skip the confirm or double-fire an effect).
 */
internal fun finishTransition(step: FinishStep, action: FinishAction): FinishTransition = when {
    step == FinishStep.Closed && action == FinishAction.Stop ->
        FinishTransition(FinishStep.Review, FinishEffect.PauseRecording)
    step == FinishStep.Review && action == FinishAction.Resume ->
        FinishTransition(FinishStep.Closed, FinishEffect.ResumeRecording)
    step == FinishStep.Review && action == FinishAction.Save ->
        FinishTransition(FinishStep.Closed, FinishEffect.SaveRun)
    step == FinishStep.Review && action == FinishAction.Discard ->
        FinishTransition(FinishStep.ConfirmDiscard, FinishEffect.None)
    step == FinishStep.ConfirmDiscard && action == FinishAction.CancelDiscard ->
        FinishTransition(FinishStep.Review, FinishEffect.None)
    step == FinishStep.ConfirmDiscard && action == FinishAction.ConfirmDiscard ->
        FinishTransition(FinishStep.Closed, FinishEffect.DiscardRun)
    else -> FinishTransition(step, FinishEffect.None)
}

/**
 * Today's open planned run, or null: the next session is dated today, still
 * trainable (next/adjusted), a run session, and has an item not yet done. Prefers
 * the item carrying run targets/structure. Pure so the tracker's plan hand-off is
 * testable.
 */
internal fun plannedRunToday(model: ViewModel): PrescriptionView? {
    val ns = model.next_session ?: return null
    if (ns.epoch_day != model.today()) return null
    if (!ns.actionable) return null
    if (ns.session_type.discipline != SessionDiscipline.Run) return null
    val open = ns.items.filter { !it.done }
    return open.firstOrNull { it.run_target != null || it.run_structure != null } ?: open.firstOrNull()
}

/** The run-type chip preselected on the Finish review for a followed plan's
 *  training band (view_types.rs RunTargetView.band); null for a free run. */
internal fun workoutTypeForBand(band: String?): WorkoutType? = when (band) {
    null, "" -> null
    "recovery" -> WorkoutType.Recovery
    "easy" -> WorkoutType.Easy
    "threshold" -> WorkoutType.Tempo
    "interval", "repetition", "hill effort" -> WorkoutType.Interval
    else -> WorkoutType.Steady
}

/** The run-type chip preselected for a followed plan: the planned session's type
 *  when it names one of the chips (a long run's band is "easy", so the band alone
 *  would read Steady), else [workoutTypeForBand]. */
internal fun workoutTypeForPlan(sessionType: SessionType?, band: String?): WorkoutType? = when (sessionType) {
    SessionType.RunLongRun -> WorkoutType.LongRun
    SessionType.RunRecovery -> WorkoutType.Recovery
    SessionType.RunEasy -> WorkoutType.Easy
    SessionType.RunTempo -> WorkoutType.Tempo
    SessionType.RunInterval, SessionType.RunRepetition, SessionType.RunHills -> WorkoutType.Interval
    else -> workoutTypeForBand(band)
}

/** The plan's targets as short lines: pace in the user's unit, heart rate, and the
 *  effort, each labelled like the Today run item ("Effort: 3–4 of 10"). Empty when
 *  the item carries no targets. */
internal fun runTargetLines(res: PlatformResources, t: RunTargetView?, unit: DistanceUnit): List<String> {
    if (t == null) return emptyList()
    val out = ArrayList<String>(3)
    val pace = t.paceLabel(unit)
    if (pace.isNotBlank()) out += res.getString(Res.string.track_target_pace, pace)
    if (t.hr_label.isNotBlank()) out += res.getString(Res.string.track_target_hr, t.hr_label)
    if (t.effort_detail.isNotBlank()) out += res.getString(Res.string.plan_face_effort, t.effort_detail)
    return out
}

/** The plan's targets folded into the one line shown while recording ("Plan
 *  9:00–9:50 /mi · ≤ 140 bpm · effort 3–5 of 10"), or null without targets. */
internal fun runTargetSummary(res: PlatformResources, t: RunTargetView?, unit: DistanceUnit): String? {
    if (t == null) return null
    val parts = listOf(t.paceLabel(unit), t.hr_label, t.effort_cue).filter { it.isNotBlank() }
    if (parts.isEmpty()) return null
    return res.getString(Res.string.track_plan_targets_line, parts.joinToString(" · "))
}

/** The planned distance (km) of a SINGLE-STEP planned run (a continuous run: no
 *  warm-up / reps / strides guide), for the live "X to go" line; null for a guided
 *  run (its steps carry their own countdown) or a plan without a distance. */
internal fun singleStepPlanKm(item: PrescriptionView): Double? {
    val steps = item.run_structure?.let { buildRunSteps(it) }.orEmpty()
    if (steps.size > 1) return null
    return item.run_head_km?.takeIf { it > 0.0 }
}

/** The plan preview's headline: the item's name plus its leading distance, never the
 *  targets (those are the lines below it). A plain distance lead (`run_head_km`) is
 *  re-expressed in the user's unit; any other lead ("6 × 800 m") is kept verbatim. */
internal fun runPlanHeadline(item: PrescriptionView, unit: DistanceUnit): String {
    val km = item.run_head_km
    val leadShown = if (km != null) {
        "${trimDecimal((metersToDisplay(km * 1000.0, unit) * 10.0).roundOrZero() / 10.0)} ${unit.distanceLabel}"
    } else {
        item.face_lead
    }
    return listOf(item.exercise.trim(), leadShown)
        .filter { it.isNotBlank() }
        .distinct()
        .joinToString(" · ")
        .ifBlank { item.summary }
}

/** Where the live pace sits against a followed plan's pace range. */
internal enum class PaceCue { OnTarget, SpeedUp, EaseOff }

/**
 * [paceMinPerKm] (the "Pace now" figure, smoothed over the trailing [RollingPace]
 * window, so it doesn't flicker leg to leg) against the plan's range in seconds per
 * km (RunTargetView pace_fast/slow_sec_per_km, either order). Null when the plan has
 * no pace range or there is no current pace.
 */
internal fun paceCue(paceMinPerKm: Double, fastSecPerKm: Int, slowSecPerKm: Int): PaceCue? {
    if (!paceMinPerKm.isFinite() || paceMinPerKm <= 0.0) return null
    if (fastSecPerKm <= 0 || slowSecPerKm <= 0) return null
    val secPerKm = paceMinPerKm * 60.0
    return when {
        secPerKm > maxOf(fastSecPerKm, slowSecPerKm) -> PaceCue.SpeedUp
        secPerKm < minOf(fastSecPerKm, slowSecPerKm) -> PaceCue.EaseOff
        else -> PaceCue.OnTarget
    }
}

/** The plan's pace range describes the WORK: it is judged on a continuous run (no
 *  step guide) and on its main-run / rep steps, never during a warm-up, recovery,
 *  cool-down, strides, run/walk bouts, or once the steps are done. */
internal fun paceTargetApplies(progress: StepProgress?): Boolean {
    if (progress == null) return true
    val kind = progress.step?.kind ?: return false
    return kind == RunStepKind.Main || kind == RunStepKind.Rep
}

/** What the location-permission panel says, matching what the user actually allowed. */
internal enum class LocationPrompt {
    /** Never asked: explain, then Continue opens the system prompt. */
    Rationale,
    /** Asked and refused (the system can still ask again). */
    Denied,
    /** Only approximate location was allowed, which can't measure a route. */
    CoarseOnly,
    /** "Don't ask again": only app settings can restore it. */
    Blocked,
}

internal fun locationPrompt(asked: Boolean, permaDenied: Boolean, coarseGranted: Boolean): LocationPrompt = when {
    permaDenied -> LocationPrompt.Blocked
    coarseGranted -> LocationPrompt.CoarseOnly
    !asked -> LocationPrompt.Rationale
    else -> LocationPrompt.Denied
}

/** Covered distance (km) below which the whole-run average pace is withheld: an
 *  average over a few metres is noise. */
private const val MIN_AVG_PACE_KM = 0.05

/** Whole-run average pace (canonical min/km) over MOVING time, or NaN before
 *  [MIN_AVG_PACE_KM] is covered. Shared by the stat tiles and the Finish review so
 *  both show the same figure. */
internal fun averagePaceMinPerKm(elapsedSec: Long, km: Double): Double =
    if (km >= MIN_AVG_PACE_KM && elapsedSec > 0) elapsedSec / 60.0 / km else Double.NaN

/** The recorded fixes as route segments (each [segmentStarts] index opens a new
 *  one, so a pause never draws a bridging line), for the Finish review's route
 *  thumbnail. Pure. */
internal fun trackSegments(points: List<GpsPoint>, segmentStarts: Collection<Int>): List<List<GpxFix>> {
    val starts = segmentStarts as? Set<Int> ?: segmentStarts.toHashSet()
    val out = ArrayList<List<GpxFix>>()
    var seg = ArrayList<GpxFix>()
    points.forEachIndexed { i, p ->
        if (i in starts && seg.isNotEmpty()) {
            out += seg
            seg = ArrayList()
        }
        seg += GpxFix(p.lat, p.lon, p.observedAt, hrBpm = null, accuracyM = p.accuracyM)
    }
    if (seg.isNotEmpty()) out += seg
    return out
}
