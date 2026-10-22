use super::*;

/// Push one evidence-cited row, dropping `MarketingMyth`-graded claims (HARD
/// RULE 2, myths are never programmed or surfaced).
pub(in crate::app) fn push_guidance<T>(
    rows: &mut Vec<GuidanceView>,
    section: &str,
    summary: String,
    r: &Recommended<T>,
) {
    push_guidance_basis(rows, section, summary, None, r);
}

/// Like [`push_guidance`], with the summary's miles twin (`summary_mi`) for a
/// row that states a distance. Sets the twin only when the row was pushed.
pub(in crate::app) fn push_guidance_mi<T>(
    rows: &mut Vec<GuidanceView>,
    section: &str,
    summary: String,
    summary_mi: String,
    r: &Recommended<T>,
) {
    let before = rows.len();
    push_guidance_basis(rows, section, summary, None, r);
    if rows.len() > before
        && let Some(row) = rows.last_mut()
    {
        row.summary_mi = summary_mi;
    }
}

/// Like [`push_guidance`], but with a call-site-specific `basis` line for the
/// why? disclosure (a datum-rich restatement the generic claim statement can't
/// carry, e.g. the HR-zones card echoing the user's age through Tanaka). When
/// `basis` is `None`, the backing claim's registered statement is used.
pub(in crate::app) fn push_guidance_basis<T>(
    rows: &mut Vec<GuidanceView>,
    section: &str,
    summary: String,
    basis: Option<String>,
    r: &Recommended<T>,
) {
    if r.grade() == EvidenceGrade::MarketingMyth {
        return;
    }
    rows.push(GuidanceView {
        section: section.into(),
        summary,
        summary_mi: String::new(),
        evidence: EvidenceTagView::of(r),
        why: why_from(basis, r),
        training_age: String::new(),
    });
}

/// Human label for a running goal distance, mirroring the Kotlin
/// `GoalDistance.label` (ProfileEditor.kt) exactly. Used in guidance copy so the
/// row never leaks the raw Debug enum name (e.g. "FiveK", "C25k").
pub(super) fn goal_distance_label(g: GoalDistance) -> &'static str {
    match g {
        GoalDistance::General => "General fitness",
        GoalDistance::C25k => "Couch to 5K",
        GoalDistance::FiveK => "5K",
        GoalDistance::TenK => "10K",
        GoalDistance::HalfMarathon => "Half marathon",
        GoalDistance::Marathon => "Marathon",
    }
}

/// Human label for a lift goal, matching the Kotlin `LiftGoal.labelRes`
/// (ProfileEditor.kt: Strength / Muscle / Power). Used in guidance copy so the
/// row never leaks the raw Debug enum name (e.g. "MaxStrength").
pub(super) fn lift_goal_label(g: LiftGoal) -> &'static str {
    match g {
        LiftGoal::MaxStrength => "Strength",
        LiftGoal::Power => "Power",
        LiftGoal::Hypertrophy => "Muscle",
    }
}

/// Human label for a training age, so guidance copy never leaks the raw Debug
/// enum name (e.g. a future `LateIntermediate` would render as PascalCase).
pub(super) fn training_age_label(ta: individualization::TrainingAge) -> &'static str {
    match ta {
        individualization::TrainingAge::Novice => "Novice",
        individualization::TrainingAge::Intermediate => "Intermediate",
        individualization::TrainingAge::Advanced => "Advanced",
    }
}
