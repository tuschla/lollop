package app.milestone

/** Modality derivation from the echoed profile: a side is "shown" when it carries
 *  volume. Callers OR in `profile == null` wherever a not-yet-set-up user should
 *  still see everything (the chooser, Profile's calculators, history segment). */
internal fun ViewModel.showLifting(): Boolean = (profile?.weekly_sets ?: 0) > 0
internal fun ViewModel.showRunning(): Boolean = (profile?.running_days_per_week ?: 0) > 0

/** The session can still be trained today: the hero, possibly adjusted (not blocked). */
internal val SessionPlanView.actionable: Boolean get() = status == "next" || status == "adjusted"

/** A finished session: every item done, or done lighter (`SessionStatus::is_done`). */
internal val SessionPlanView.isDone: Boolean get() = status == "done" || status == "done_lighter"

/** Compact day-type token for the week strip, from the core's debug session_type
 *  ("Lift(MaxEffort)" / "Run(LongRun)" / "Rest"). Discipline-level so it fits a
 *  7-column strip; the full session title shows in the expanded day card. */
// l10n: output doubles as a comparison key (== "Run"/"Lift"); localizing it would
// break call-site logic. Needs decoupling of display from key before extraction.
internal fun sessionDiscipline(sessionType: String): String = when {
    sessionType.startsWith("Lift") -> "Lift"
    sessionType.startsWith("Run") -> "Run"
    else -> "Rest"
}

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

private fun <T> evidencePriority(safety: (T) -> Boolean, grade: (T) -> String, confidence: (T) -> Float): Comparator<T> =
    compareByDescending(safety).thenByDescending { gradeRank(grade(it)) }.thenByDescending(confidence)

/**
 * Grade rank for the priority sorts: higher = stronger evidence. Mirrors
 * `EvidenceGrade` ordering in `schema.rs`; unknown grades sort last.
 */
private fun gradeRank(grade: String): Int = when (grade) {
    "Strong" -> 4
    "Moderate" -> 3
    "Weak" -> 2
    "ExpertOpinion" -> 1
    "MarketingMyth" -> 0
    else -> -1
}
