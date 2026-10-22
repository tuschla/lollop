package app.milestone

/** Modality derivation from the echoed profile: a side is "shown" when it carries
 *  volume. Callers OR in `profile == null` wherever a not-yet-set-up user should
 *  still see everything (the chooser, Profile's calculators, history segment). */
internal fun ViewModel.showLifting(): Boolean = (profile?.weekly_sets ?: 0) > 0
internal fun ViewModel.showRunning(): Boolean = (profile?.running_days_per_week ?: 0) > 0

/** The display unit for every kg figure, from the profile's weight-unit flag. */
internal fun ViewModel.weightUnit(): WeightUnit = if (profile?.weight_lb == true) WeightUnit.Lb else WeightUnit.Kg

/**
 * Priority sort for adjustments: safety-critical first, then strongest grade,
 * then confidence descending. Applied within one list (adjustments or review
 * adjustments), never across them.
 */
internal fun List<AdjustmentView>.byAdjustmentPriority(): List<AdjustmentView> =
    sortedWith(
        compareByDescending<AdjustmentView> { it.safety_critical }
            .thenByDescending { gradeRank(it.grade) }
            .thenByDescending { it.confidence },
    )

/** The single highest-priority safety-critical adjustment, or null. Shared by the
 *  Today safety card and the global SafetyBanner so the lookup is defined once
 *  (wrap in `remember(model.adjustments)` at each call site). */
internal fun dominantSafetyAdjustment(model: ViewModel): AdjustmentView? =
    model.adjustments.byAdjustmentPriority().firstOrNull { it.safety_critical }

/** [byAdjustmentPriority]'s order for guidance rows. */
internal fun List<GuidanceView>.byGuidancePriority(): List<GuidanceView> =
    sortedWith(
        compareByDescending<GuidanceView> { it.safety_critical }
            .thenByDescending { gradeRank(it.grade) }
            .thenByDescending { it.confidence },
    )

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
