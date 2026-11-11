package app.milestone

import kotlinx.datetime.LocalDate

/** Modality derivation from the echoed profile: a side is "shown" when it carries
 *  volume. Callers OR in `profile == null` wherever a not-yet-set-up user should
 *  still see everything (the chooser, Profile's calculators, history segment). */
internal fun ViewModel.showLifting(): Boolean = (profile?.weekly_sets ?: 0) > 0
internal fun ViewModel.showRunning(): Boolean = (profile?.running_days_per_week ?: 0) > 0

/** The session can still be trained today: the hero, possibly adjusted (not blocked). */
internal val SessionPlanView.actionable: Boolean get() = status == SessionStatus.Next || status == SessionStatus.Adjusted

/** A finished session: every item done, or done lighter. */
internal val SessionPlanView.isDone: Boolean get() = status.isDone

/**
 * Priority sort for adjustments: safety-critical first, then strongest grade,
 * then confidence descending. Applied within one list (adjustments or review
 * adjustments), never across them.
 */
internal fun List<AdjustmentView>.byAdjustmentPriority(): List<AdjustmentView> =
    sortedWith(evidencePriority({ it.safety_critical }, { it.grade }, { it.confidence }))

/** The single highest-priority safety-critical adjustment, or null. Shared by the
 *  Today safety card and the global SafetyBanner so the lookup is defined once
 *  (wrap in `remember(model.adjustments)` at each call site). */
internal fun dominantSafetyAdjustment(model: ViewModel): AdjustmentView? =
    model.adjustments.byAdjustmentPriority().firstOrNull { it.safety_critical }

/** [byAdjustmentPriority]'s order for guidance rows. */
internal fun List<GuidanceView>.byGuidancePriority(): List<GuidanceView> =
    sortedWith(evidencePriority({ it.safety_critical }, { it.grade }, { it.confidence }))

private fun <T> evidencePriority(safety: (T) -> Boolean, grade: (T) -> EvidenceGrade?, confidence: (T) -> Float): Comparator<T> =
    compareByDescending(safety).thenByDescending { grade(it)?.ordinal ?: -1 }.thenByDescending(confidence)

/** The core's guidance section of profile-context rows (shared/src/app/guidance.rs). */
internal const val PROFILE_GUIDANCE_SECTION = "Profile"

/** A plan day's short date in the app's language ([appLocale]: "Thu, Oct 15"). */
internal fun planDateLabel(epochDay: Long): String =
    localizedDayFormatter("EEEMMMd").format(LocalDate.fromEpochDays(epochDay))

/** The post-illness "Return check" ([PrescriptionView.return_check]) shown as
 *  its own card in a lifting session: neither a run to track nor sets to count;
 *  any logged session today completes it. In a run session it stays a run. */
internal fun isReturnCheckItem(item: PrescriptionView, sessionType: SessionType): Boolean =
    item.return_check && sessionType.discipline == SessionDiscipline.Lift

/** A plan item tracked or logged as a run: any item of a run session, or an
 *  item with no exercise; never the return check. */
internal fun isRunItem(item: PrescriptionView, sessionType: SessionType): Boolean =
    !isReturnCheckItem(item, sessionType) && (sessionType.discipline == SessionDiscipline.Run || item.exercise.isEmpty())
