package app.milestone

import app.milestone.resources.Res
import app.milestone.resources.log_band_reps
import app.milestone.resources.log_rest_notif_extra_set
import app.milestone.resources.log_rest_notif_join
import app.milestone.resources.log_rest_notif_next
import app.milestone.resources.log_set_header_n
import app.milestone.resources.log_set_header_n_of_n
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow

/**
 * The set loop's rest timer: the running rest ([running], what the in-app
 * readout counts down), its countdown outside the app and the rest-done alert
 * that reaches the user while the loop's readout is not on screen. The platform
 * shows them ([RestAlarmPlatform]: Android an AlarmManager alarm and two
 * notifications, iOS a time-interval notification and the rest's Live Activity);
 * this object decides when.
 */
internal object RestAlarm {
    private class State {
        val running = MutableStateFlow<RestRun?>(null)
        val exactAlarmPrompt = MutableStateFlow(false)

        // The running rest's end alert is still to come.
        var pending = false
        var inAppVisible = false

        // What the loop logs next, kept current by the open loop.
        var nextSet: RestNextSet? = null

        // The exercise the rest-done alert names: null until the loop reports its
        // next set (the rested exercise), "" when it reported none.
        var doneExercise: String? = null

        // The user swiped the running rest's countdown away.
        var dismissed = false

        // The running rest's alarm is inexact (exact alarms were not allowed).
        var alarmInexact = false
    }

    private val state by ProcessLocal { State() }

    /** The running rest; null when none runs. */
    val running: StateFlow<RestRun?> get() = state.running

    /** The set loop shows its one-time notice: exact alarms are not allowed, so
     *  the rest-done alert may come late while the app is in the background.
     *  Always false where the platform has no such permission (iOS). */
    val exactAlarmPrompt: StateFlow<Boolean> get() = state.exactAlarmPrompt

    /** The loop's rest readout is on a resumed screen: its own cue covers the rest end. */
    val inAppVisible: Boolean get() = state.inAppVisible

    /** Start (or restart) the rest that follows a set of [exerciseName]. A set with
     *  no prescribed rest leaves a running rest alone. */
    fun schedule(ctx: PlatformContext, restSec: Int, exerciseName: String) {
        if (restSec <= 0) return
        val s = state
        clear(ctx, s)
        val now = AppClock.nowMillis()
        val restMs = restSec * 1000L
        val run = RestRun(now, now + restMs, exerciseName)
        s.running.value = run
        s.pending = true
        s.alarmInexact = RestAlarmPlatform.arm(ctx, run, restMs)
        when (RestAlarmPlatform.countdown) {
            RestCountdown.OffLoop -> {
                RestAlarmPlatform.hideCountdown(ctx)
                if (!s.inAppVisible) RestAlarmPlatform.showCountdown(ctx, run, null)
            }
            RestCountdown.WholeRest -> RestAlarmPlatform.showCountdown(ctx, run, null)
        }
        if (s.alarmInexact && RestAlarmPlatform.exactAlarms?.promptDone(ctx) == false) s.exactAlarmPrompt.value = true
    }

    /** Drop the running rest (an undone set, the loop closed with "Done for now"
     *  or "Finish session"), its alert and anything it posted. */
    fun cancel(ctx: PlatformContext) {
        clear(ctx, state)
        RestAlarmPlatform.hideCountdown(ctx)
    }

    private fun clear(ctx: PlatformContext, s: State) {
        RestAlarmPlatform.disarm(ctx)
        s.running.value = null
        s.pending = false
        s.nextSet = null
        s.doneExercise = null
        s.dismissed = false
        s.alarmInexact = false
    }

    /** The open loop's next set (null: none), which the rest's alerts name. */
    fun setNextSet(ctx: PlatformContext, next: RestNextSet?) {
        val s = state
        val run = s.running.value
        if (next != s.nextSet) {
            s.nextSet = next
            if (run != null && RestAlarmPlatform.countdown == RestCountdown.WholeRest) {
                RestAlarmPlatform.showCountdown(ctx, run, next)
            }
        }
        if (run == null || !s.pending) return
        val exercise = next?.exercise.orEmpty()
        if (exercise == s.doneExercise) return
        s.doneExercise = exercise
        RestAlarmPlatform.renameDone(ctx, run, exercise)
    }

    /** The loop's rest readout entered/left a resumed screen: while it is on
     *  one, the in-app cue covers the rest end and no alert shows. Leaving it
     *  while a rest is still running brings back an [RestCountdown.OffLoop] countdown. */
    fun setInAppVisible(ctx: PlatformContext, visible: Boolean) {
        val s = state
        s.inAppVisible = visible
        val offLoop = RestAlarmPlatform.countdown == RestCountdown.OffLoop
        if (visible) {
            if (offLoop) RestAlarmPlatform.hideCountdown(ctx)
            RestAlarmPlatform.withdrawDone(ctx)
        } else if (offLoop && s.pending && !s.dismissed) {
            RestAlarmPlatform.showCountdown(ctx, s.running.value ?: return, s.nextSet)
        }
    }

    /** Show the running rest's countdown again: the platform lost it (iOS: the
     *  Live Activity's host attached again). */
    fun showCountdownAgain(ctx: PlatformContext) {
        val s = state
        val run = s.running.value ?: return
        val shows = when (RestAlarmPlatform.countdown) {
            RestCountdown.OffLoop -> s.pending && !s.inAppVisible && !s.dismissed
            RestCountdown.WholeRest -> true
        }
        if (shows) RestAlarmPlatform.showCountdown(ctx, run, s.nextSet)
    }

    /** The countdown of the rest started at [startMillis] was swiped away: it is
     *  not shown again for that rest (the rest-done alert still comes). */
    fun onCountdownDismissed(startMillis: Long) {
        val s = state
        if (s.running.value?.startMillis == startMillis) s.dismissed = true
    }

    /** The running rest's end alert went off in this process (Android's alarm;
     *  iOS's alert is the system's): the rest is over, its readout counts the
     *  overtime until it is cancelled or replaced. */
    fun ended(ctx: PlatformContext) {
        val s = state
        s.pending = false
        s.alarmInexact = false
        if (RestAlarmPlatform.countdown == RestCountdown.OffLoop) RestAlarmPlatform.hideCountdown(ctx)
    }

    /** The set loop is in front again: once exact alarms are allowed (the user is
     *  back from the system screen) the notice goes, and a running rest's inexact
     *  alarm is set again as an exact one. */
    fun recheckExactAlarms(ctx: PlatformContext) {
        val exact = RestAlarmPlatform.exactAlarms ?: return
        if (!exact.allowed(ctx)) return
        val s = state
        s.exactAlarmPrompt.value = false
        val run = s.running.value ?: return
        val left = run.endAtMillis - AppClock.nowMillis()
        if (s.pending && s.alarmInexact && left > 0) s.alarmInexact = RestAlarmPlatform.arm(ctx, run, left)
    }

    /** The notice was dismissed or its action used: it is not shown again. */
    fun dismissExactAlarmPrompt(ctx: PlatformContext) {
        RestAlarmPlatform.exactAlarms?.setPromptDone(ctx)
        state.exactAlarmPrompt.value = false
    }

    /** The notice's action: the system screen where the user allows exact alarms
     *  for this app (the notice only shows where there is one). */
    fun openExactAlarmSettings(ctx: PlatformContext) {
        RestAlarmPlatform.exactAlarms?.openSettings(ctx)
    }
}

/** How a platform's countdown outside the app follows the rest ([RestAlarm] decides when it shows). */
internal enum class RestCountdown {
    /** A notification (Android): only while the loop's readout is not on screen,
     *  until the rest end, not again once the user swipes it away; it keeps the
     *  next set it was posted with. */
    OffLoop,

    /** A Live Activity (iOS): for the whole rest until it is cancelled or
     *  replaced, kept current with the next set; the system hides it while the
     *  app is in front. */
    WholeRest,
}

/** The user's grant for exact alarms (Android 12+): without it the rest-end
 *  alert may come late, and the set loop asks once. */
internal interface ExactAlarms {
    fun allowed(ctx: PlatformContext): Boolean

    /** The loop's notice was dismissed or its action used (kept across processes). */
    fun promptDone(ctx: PlatformContext): Boolean

    fun setPromptDone(ctx: PlatformContext)

    /** The system screen where the user allows exact alarms for this app. */
    fun openSettings(ctx: PlatformContext)
}

/** What a platform shows of the rest timer; [RestAlarm] decides when. */
internal expect object RestAlarmPlatform {
    val countdown: RestCountdown

    /** Null where the rest-end alert is always on time (iOS). */
    val exactAlarms: ExactAlarms?

    /** Set [run]'s rest-end alert [delayMs] from now, replacing any: the
     *  rest-done alert, naming the rested exercise until [renameDone]. True when
     *  it may come late (an inexact alarm). */
    fun arm(ctx: PlatformContext, run: RestRun, delayMs: Long): Boolean

    /** [run]'s rest-done alert, still to come, names [exercise] ("" none). */
    fun renameDone(ctx: PlatformContext, run: RestRun, exercise: String)

    /** Drop the rest-end alert: the one to come and a delivered rest-done alert. */
    fun disarm(ctx: PlatformContext)

    /** Take down a delivered rest-done alert. */
    fun withdrawDone(ctx: PlatformContext)

    /** Show [run]'s countdown with the [next] set, in place of a shown one;
     *  nothing when it can't show (no notification permission, the rest is over). */
    fun showCountdown(ctx: PlatformContext, run: RestRun, next: RestNextSet?)

    fun hideCountdown(ctx: PlatformContext)
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
