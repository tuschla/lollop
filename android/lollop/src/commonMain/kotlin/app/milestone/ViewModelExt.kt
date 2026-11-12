package app.milestone

import kotlinx.datetime.LocalDate
import kotlinx.serialization.Serializable

/** The sides the profile trains, as the core's plan reads them ([ViewModel.trains]);
 *  both before setup, so a not-yet-set-up user still sees everything. */
internal fun ViewModel.showLifting(): Boolean = trains?.lifts != false
internal fun ViewModel.showRunning(): Boolean = trains?.runs != false

/** The session can still be trained today: the hero, possibly adjusted (not blocked). */
internal val SessionPlanView.actionable: Boolean get() = status == SessionStatus.Next || status == SessionStatus.Adjusted

/** A finished session: every item done, or done lighter. */
internal val SessionPlanView.isDone: Boolean get() = status.isDone

/** A run session's run item ([SessionPlanView.run_item]), else null. */
internal val SessionPlanView.runItem: PrescriptionView? get() = run_item?.let(items::getOrNull)

/** How one plan day reads at a glance: the app's week strip and the widget's. */
@Serializable
internal enum class DayMark {
    /** Trained (done, or done lighter). */
    Done,

    /** The plan's next session (adjusted or not). */
    Next,

    /** A session still to come. */
    Planned,

    /** A past session not logged, or only partly done. */
    Missed,

    /** A plan rest day. */
    Rest,

    /** Before the plan started. */
    None,
}

internal val SessionStatus.mark: DayMark get() = when (this) {
    SessionStatus.Done, SessionStatus.DoneLighter -> DayMark.Done
    SessionStatus.Next, SessionStatus.Adjusted -> DayMark.Next
    SessionStatus.Missed, SessionStatus.Partial -> DayMark.Missed
    SessionStatus.Rest -> DayMark.Rest
    SessionStatus.BeforePlan -> DayMark.None
    SessionStatus.Planned, SessionStatus.Blocked -> DayMark.Planned
}

/**
 * Priority sort for adjustments: safety-critical first, then strongest grade,
 * then confidence descending. Applied within one list (adjustments or review
 * adjustments), never across them.
 */
internal fun List<AdjustmentView>.byAdjustmentPriority(): List<AdjustmentView> =
    sortedWith(evidencePriority({ it.safety_critical }, { it.grade }, { it.confidence }))

internal fun dominantSafetyAdjustment(model: ViewModel): AdjustmentView? =
    model.adjustments.byAdjustmentPriority().firstOrNull { it.safety_critical }

/** [byAdjustmentPriority]'s order for guidance rows. */
internal fun List<GuidanceView>.byGuidancePriority(): List<GuidanceView> =
    sortedWith(evidencePriority({ it.safety_critical }, { it.grade }, { it.confidence }))

private fun <T> evidencePriority(safety: (T) -> Boolean, grade: (T) -> EvidenceGrade?, confidence: (T) -> Float): Comparator<T> =
    compareByDescending(safety).thenByDescending { grade(it)?.ordinal ?: -1 }.thenByDescending(confidence)

/** A plan day's short date in the app's language ([appLocale]: "Thu, Oct 15"). */
internal fun planDateLabel(epochDay: Long): String =
    localizedDayFormatter("EEEMMMd").format(LocalDate.fromEpochDays(epochDay))

/** Today's epoch-day for every plan gate: the core's clock echoed on the view
 *  (SetToday) rather than the wall clock, so a session that crosses midnight
 *  while foregrounded rolls over in lockstep with the core instead of stranding
 *  its Log buttons. Falls back to the device clock before the boot's SetToday. */
internal fun ViewModel.today(): Long = today_epoch_day ?: todayEpochDay()

/** Today's session still to train: the hero dated today, with items, not blocked. */
internal fun ViewModel.todaysPlan(): SessionPlanView? =
    next_session?.takeIf { it.epoch_day == today() && it.items.isNotEmpty() && it.actionable }

/** Today's lift session once something of it is logged. After Finish the core moves
 *  next_session off today, so this is how the day is resumed. */
internal fun ViewModel.resumableLiftDay(): SessionPlanView? = week_plan.firstOrNull {
    it.epoch_day == today() && it.items.isNotEmpty() && it.session_type.discipline == SessionDiscipline.Lift &&
        it.items.any { i -> i.sets_done > 0 || i.logged_sets.isNotEmpty() }
}

/** The lift item a set logs against today, with its session: today's plan lift
 *  session's first open item (else its first), or once something is logged and
 *  the core moved next_session on, today's started lift session's first open item
 *  (else its last, for an extra set). */
internal fun ViewModel.openLiftTarget(): Pair<SessionPlanView, PrescriptionView>? {
    val plan = todaysPlan()
    if (plan != null && plan.session_type.discipline == SessionDiscipline.Lift) {
        return plan to (firstOpenItem(plan.items) ?: plan.items.first())
    }
    val started = resumableLiftDay() ?: return null
    return started to (firstOpenItem(started.items) ?: started.items.last())
}

/** The first not-done item of a session — the "Today's plan" prefill target and
 *  the hero's next-open item. Null when every item is done (next_session keeps
 *  done items, so the shell picks the open one). */
internal fun firstOpenItem(items: List<PrescriptionView>): PrescriptionView? =
    items.firstOrNull { !it.done }
