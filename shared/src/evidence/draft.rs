use crate::evidence::contested::ContestedId;
use crate::evidence::grade::EvidenceGrade;

/// Re-grade cadence in months of a row that names none (File 09 review
/// schedule; fast-moving topics re-grade sooner).
const DEFAULT_REVIEW_MONTHS: u8 = 12;

/// A registry row before its discipline group assigns its
/// [`ClaimId`](super::ClaimId).
pub(super) struct ClaimDraft {
    pub(super) statement: &'static str,
    pub(super) grade: EvidenceGrade,
    pub(super) primary_citations: &'static [&'static str],
    pub(super) contradicting: &'static [&'static str],
    pub(super) safety_critical: bool,
    pub(super) contested: Option<ContestedId>,
    pub(super) review_months: u8,
}

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
    pub(super) const fn contradicting(mut self, notes: &'static [&'static str]) -> Self {
        self.contradicting = notes;
        self
    }

    pub(super) const fn safety(mut self) -> Self {
        self.safety_critical = true;
        self
    }

    pub(super) const fn contested(mut self, id: ContestedId) -> Self {
        self.contested = Some(id);
        self
    }

    pub(super) const fn review(mut self, months: u8) -> Self {
        self.review_months = months;
        self
    }
}
