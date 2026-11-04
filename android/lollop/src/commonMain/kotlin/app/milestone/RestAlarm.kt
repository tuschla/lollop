package app.milestone

import app.milestone.resources.Res
import app.milestone.resources.log_band_reps
import app.milestone.resources.log_rest_notif_extra_set
import app.milestone.resources.log_rest_notif_join
import app.milestone.resources.log_rest_notif_next
import app.milestone.resources.log_set_header_n
import app.milestone.resources.log_set_header_n_of_n
import kotlinx.coroutines.flow.StateFlow

/**
 * The set loop's rest timer: the running rest ([running], what the in-app
 * readout counts down) and the rest-done alert that reaches the user while the
 * loop's readout is not on screen. Android: an AlarmManager alarm, a countdown
 * notification and a rest-done notification (RestAlarm.android.kt); iOS: a
 * UNUserNotificationCenter alert at the rest end (RestAlarm.ios.kt).
 */
internal expect object RestAlarm {
    /** The running rest; null when none runs. */
    val running: StateFlow<RestRun?>

    /** The set loop shows its one-time notice: exact alarms are not allowed, so
     *  the rest-done alert may come late while the app is in the background.
     *  Always false where the platform has no such permission (iOS). */
    val exactAlarmPrompt: StateFlow<Boolean>

    /** Start (or restart) the rest that follows a set of [exerciseName]. A set with
     *  no prescribed rest leaves a running rest alone. */
    fun schedule(ctx: PlatformContext, restSec: Int, exerciseName: String)

    /** Drop the running rest, its alert and anything it posted. */
    fun cancel(ctx: PlatformContext)

    /** The open loop's next set (null: none), which the rest's alerts name. */
    fun setNextSet(ctx: PlatformContext, next: RestNextSet?)

    /** The loop's rest readout entered/left a resumed screen: while it is on
     *  one, the in-app cue covers the rest end and no alert shows. */
    fun setInAppVisible(ctx: PlatformContext, visible: Boolean)

    /** The set loop is in front again: once exact alarms are allowed the notice
     *  goes and a running rest's alarm is set again as an exact one. */
    fun recheckExactAlarms(ctx: PlatformContext)

    /** The notice was dismissed or its action used: it is not shown again. */
    fun dismissExactAlarmPrompt(ctx: PlatformContext)

    /** The notice's action: open the system screen where the user allows exact
     *  alarms for this app. */
    fun openExactAlarmSettings(ctx: PlatformContext)
}

/** The set the loop logs next, as the rest's alerts name it: its exercise and
 *  "Set 3 of 4 · 100 kg × 5" (null when the loop shows no set line). */
internal data class RestNextSet(val exercise: String, val detail: String?)

/** The countdown's body line: "Next: Set 3 of 4 · 100 kg × 5" for another set of
 *  the rested [exercise], "Next: Bench press · Set 1 of 3" when the loop moves on.
 *  Null when the loop shows no next set. Pure. */
internal fun countdownText(res: PlatformResources, exercise: String, next: RestNextSet?): String? {
    next ?: return null
    val what = when {
        sameExercise(next.exercise, exercise) -> next.detail ?: return null
        next.detail == null -> next.exercise
        else -> res.getString(Res.string.log_rest_notif_join, next.exercise, next.detail)
    }
    return res.getString(Res.string.log_rest_notif_next, what)
}

/**
 * The set the loop's primary action logs next, as the set loop shows it: its
 * "Set k of N" (a plan item), "Set k" (an off-plan exercise, [offPlanSetNumber])
 * or "Extra set", then the load and reps in the fields ([load], null when not
 * entered). Once the item is complete it is the next item's first open set. Null
 * when the action logs no set (finish, a blank editor, an edit). Pure.
 */
internal fun restNextSet(
    res: PlatformResources,
    primary: LoopPrimary,
    exercise: String,
    liveItem: PrescriptionView?,
    nextItem: PrescriptionView?,
    offPlanSetNumber: Int,
    load: String?,
): RestNextSet? {
    fun joined(set: String) = load?.let { res.getString(Res.string.log_rest_notif_join, set, it) } ?: set
    return when (primary) {
        LoopPrimary.LogPlannedSet -> liveItem?.let {
            RestNextSet(it.exercise, joined(res.getString(Res.string.log_set_header_n_of_n, nextSetNumber(it), plannedSetCount(it))))
        }
        LoopPrimary.LogOffPlanSet ->
            RestNextSet(exercise, joined(res.getString(Res.string.log_set_header_n, offPlanSetNumber)))
        LoopPrimary.LogExtraSet ->
            RestNextSet(exercise, joined(res.getString(Res.string.log_rest_notif_extra_set)))
        LoopPrimary.Next -> nextItem?.let {
            RestNextSet(it.exercise, res.getString(Res.string.log_set_header_n_of_n, nextSetNumber(it), plannedSetCount(it)))
        }
        LoopPrimary.Save, LoopPrimary.LogBlank, LoopPrimary.FinishSession -> null
    }
}

/** The set loop's fields as the countdown names the next set: "100 kg × 5",
 *  "30 s hold", "12 reps" for a bodyweight rung. Null while the load or reps are
 *  not entered. Pure. */
internal fun restNextLoad(
    res: PlatformResources,
    holdSec: Int?,
    weightKg: Double?,
    reps: Int?,
    bodyweight: Boolean,
    weightUnit: WeightUnit,
): String? = when {
    holdSec != null -> liftSetLine(res, holdSec, weightKg ?: 0.0, 1, 0.0, false, weightUnit)
    reps == null -> null
    bodyweight -> res.getQuantityString(Res.plurals.log_band_reps, reps, reps)
    weightKg == null -> null
    else -> liftSetLine(res, null, weightKg, reps, 0.0, false, weightUnit)
}
