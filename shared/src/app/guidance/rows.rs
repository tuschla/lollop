use crate::app::evidence_views::why_from;
use crate::app::format::fmt_whole_km_or_mi;
use crate::app::view_types::{EvidenceTagView, GuidanceView};
use crate::evidence::EvidenceGrade;
use crate::individualization;
use crate::schema::{Adjustment, Recommended};
use crate::strength::LiftGoal;

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

/// The single row a held calculator or guidance list shows instead of
/// numbers, carrying the hold's own evidence.
pub(in crate::app) fn push_hold_row(
    rows: &mut Vec<GuidanceView>,
    section: &str,
    hold: &Recommended<Adjustment>,
) {
    push_guidance(
        rows,
        section,
        "No numbers while training is paused. The banner at the top says what to do next."
            .to_string(),
        hold,
    );
}

/// Where the weekly running volume the guidance reasons about came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum WeeklyBasis {
    /// Measured from the athlete's logged runs.
    Logged,
    /// The stated profile figure.
    Profile,
}

/// The weekly volume with its source, in km or miles: "70 km a week from your
/// logged runs" / "19 mi a week from your profile".
pub(super) fn weekly_basis_text(weekly_km: f64, basis: WeeklyBasis, mi: bool) -> String {
    let source = match basis {
        WeeklyBasis::Logged => "from your logged runs",
        WeeklyBasis::Profile => "from your profile",
    };
    format!("{} a week {source}", fmt_whole_km_or_mi(weekly_km, mi))
}

/// A lift goal as it reads mid-sentence in guidance copy ("Weights for muscle
/// building: …"), so a row never leaks the raw Debug enum name (e.g.
/// "MaxStrength") or a capitalized segment label.
pub(super) fn lift_goal_phrase(g: LiftGoal) -> &'static str {
    match g {
        LiftGoal::MaxStrength => "strength",
        LiftGoal::Power => "power",
        LiftGoal::Hypertrophy => "muscle building",
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
