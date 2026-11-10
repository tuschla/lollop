use crate::evidence::contested::ContestedId;
use crate::evidence::grade::EvidenceGrade;

/// Re-grade cadence in months of a row that names none (File 09 review
/// schedule; fast-moving topics re-grade sooner).
const DEFAULT_REVIEW_MONTHS: u8 = 12;

/// A registry row before its discipline group assigns its
/// [`ClaimId`](super::ClaimId): built by [`claim`], then the modifiers for
/// metadata that differs from the defaults.
pub(super) struct ClaimDraft {
    pub(super) statement: &'static str,
    pub(super) grade: EvidenceGrade,
    pub(super) primary_citations: &'static [&'static str],
    pub(super) contradicting: &'static [&'static str],
    pub(super) safety_critical: bool,
    pub(super) contested: Option<ContestedId>,
    pub(super) review_months: u8,
}

/// A registry row with the defaults: no contradicting evidence on record,
/// not safety-critical, not contested, re-graded every
/// [`DEFAULT_REVIEW_MONTHS`].
pub(super) const fn claim(
    statement: &'static str,
    grade: EvidenceGrade,
    primary_citations: &'static [&'static str],
) -> ClaimDraft {
    ClaimDraft {
        statement,
        grade,
        primary_citations,
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: DEFAULT_REVIEW_MONTHS,
    }
}

impl ClaimDraft {
    /// The contradicting evidence on record.
    pub(super) const fn contradicting(mut self, notes: &'static [&'static str]) -> Self {
        self.contradicting = notes;
        self
    }

    /// Marks a non-negotiable deferral / stop signal.
    pub(super) const fn safety(mut self) -> Self {
        self.safety_critical = true;
        self
    }

    /// The contested question the claim is debated under.
    pub(super) const fn contested(mut self, id: ContestedId) -> Self {
        self.contested = Some(id);
        self
    }

    /// A re-grade cadence other than [`DEFAULT_REVIEW_MONTHS`].
    pub(super) const fn review(mut self, months: u8) -> Self {
        self.review_months = months;
        self
    }
}
