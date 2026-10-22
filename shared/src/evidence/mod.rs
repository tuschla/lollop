//! Canonical evidence registry (knowledge-base File 09 + per-rule claims from
//! Files 02–08, 10, 11 and the 2026-10 evidence updates in Files 12–13).
//!
//! Compile-time table of every graded claim + contested question the engine may
//! cite. Data only, no coaching logic. Every recommendation the core emits
//! names its claim through a typed [`ClaimId`] (`claims::RUN_SPIKE_001`), so a
//! typo is a build error; `MarketingMyth` claims are hard-blocked and must
//! never be surfaced as advice. The rows live in [`claims`], one file per
//! discipline; the contested questions in [`CONTESTED_QUESTIONS`].
//!
//! The File 09 canonical claims keep their original statements, except where
//! the 2026-10 evidence update (`knowledge-base/extracted/12-*.md`,
//! `13-*.md`) re-stated or re-graded them against newer primary sources
//! (RUN-DIST-001, CONC-MODE-001). Where
//! an engine rule comes from a specific KB rule whose grade/citation/safety
//! metadata differs from the nearest File 09 claim, a *per-rule* entry exists
//! (e.g. `STR-PAP-001` for File 02 strength-034) so call sites never over- or
//! under-state the source rule's grade. Grades and citations are transcribed
//! from the KB rule blocks, never overstated. Every primary citation of the
//! 2026-10 update rows was resolved against Crossref/PubMed (the per-rule
//! `verification` lines in the KB files); where a rule mixes a strong
//! principle with expert-opinion parameters, the claim carries the lower grade
//! (GRADE good-practice precedent) and flags the parameters in
//! `contradicting`.
//!
//! Every `statement` is shown to users verbatim (the "?" panel basis line), so
//! it is plain language: no arrows, rule numbers, claim ids, ALL_CAPS category
//! names or internal gate codes (`statements_are_plain_language` test). Ids
//! live only in `claim_id`.
//!
//! Contested-question convention: bare `CQ-##` ids always mean File 09's
//! GLOBAL contested-question index (CQ-01…CQ-15). Files 02–08/10 number their
//! local contested lists independently; when a local question corresponds to a
//! global one, the global id is used (File 02 block-periodization "CQ-02" →
//! global CQ-03; File 04 interval-length "CQ-06" → global CQ-10; File 10 ACWR
//! "CQ-04" → global CQ-05, HRV "CQ-05" → global CQ-04, interference "CQ-02/03"
//! → global CQ-06). A file-local question with no global counterpart gets a
//! namespaced id `CQ-F<file>-<local>` (e.g. `CQ-F03-04`) and its own
//! [`ContestedQuestion`] row, a global number is never reused for a different
//! question.
//!
//! Short-form citations only; full statistics + DOIs live in
//! `knowledge-base/extracted/09-evidence-map.md`. Verify DOIs against primary
//! sources before production use.

mod cite;
pub mod claims;
mod contested;
#[cfg(test)]
mod tests;

use std::fmt;

use crate::schema::{EvidenceGrade, Recommended};

pub use contested::CONTESTED_QUESTIONS;

/// One graded claim in the registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvidenceEntry {
    /// Typed handle of this row.
    pub id: ClaimId,
    /// Stable key, e.g. `"HYP-VOL-001"`: the [`ClaimId`] spelled with dashes.
    pub claim_id: &'static str,
    pub statement: &'static str,
    pub grade: EvidenceGrade,
    pub primary_citations: &'static [&'static str],
    pub contradicting: &'static [&'static str],
    /// Non-negotiable deferral / stop signal when true.
    pub safety_critical: bool,
    /// Contested-question id (`"CQ-01"`) if the claim is under active debate.
    pub contested: Option<&'static str>,
    /// Re-grade cadence in months (12 by default; 6 for fast-moving topics).
    pub review_months: u8,
    /// `false` for a row marked `#[kb_only]`: recorded from the knowledge base
    /// for completeness but cited by no engine rule. Every surfaced non-myth
    /// row is cited somewhere (`every_surfaced_claim_is_cited`).
    pub surfaced: bool,
}

impl EvidenceEntry {
    /// Default confidence from grade (File 09 mapping).
    pub const fn confidence_score(&self) -> f32 {
        self.grade.default_confidence()
    }

    /// True for `MarketingMyth`: must never be surfaced as advice.
    pub const fn is_blocked(&self) -> bool {
        matches!(self.grade, EvidenceGrade::MarketingMyth)
    }

    /// The citation line shown for this claim: its lead primary citation, or
    /// a plain statement that the KB records no published source.
    pub fn reference(&self) -> &'static str {
        self.primary_citations
            .first()
            .copied()
            .unwrap_or(NO_PUBLISHED_SOURCE)
    }
}

/// Citation line for a claim the KB records with no published source.
pub const NO_PUBLISHED_SOURCE: &str =
    "A common coaching rule; no published study behind the exact numbers";

/// Re-grade cadence in months of a row that names none (File 09 review
/// schedule; fast-moving topics re-grade sooner).
const DEFAULT_REVIEW_MONTHS: u8 = 12;

/// A registry row before its discipline group assigns its [`ClaimId`]: built
/// by [`claim`], then the modifiers for metadata that differs from the
/// defaults.
struct ClaimDraft {
    claim_id: &'static str,
    statement: &'static str,
    grade: EvidenceGrade,
    primary_citations: &'static [&'static str],
    contradicting: &'static [&'static str],
    safety_critical: bool,
    contested: Option<&'static str>,
    review_months: u8,
}

/// A registry row with the defaults: no contradicting evidence on record,
/// not safety-critical, not contested, re-graded every
/// [`DEFAULT_REVIEW_MONTHS`].
const fn claim(
    claim_id: &'static str,
    statement: &'static str,
    grade: EvidenceGrade,
    primary_citations: &'static [&'static str],
) -> ClaimDraft {
    ClaimDraft {
        claim_id,
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
    const fn contradicting(mut self, notes: &'static [&'static str]) -> Self {
        self.contradicting = notes;
        self
    }

    /// Marks a non-negotiable deferral / stop signal.
    const fn safety(mut self) -> Self {
        self.safety_critical = true;
        self
    }

    /// The contested question (`"CQ-01"`) the claim is debated under.
    const fn contested(mut self, id: &'static str) -> Self {
        self.contested = Some(id);
        self
    }

    /// A re-grade cadence other than [`DEFAULT_REVIEW_MONTHS`].
    const fn review(mut self, months: u8) -> Self {
        self.review_months = months;
        self
    }

    /// The finished row `id` names.
    const fn row(self, id: ClaimId, surfaced: bool) -> EvidenceEntry {
        EvidenceEntry {
            id,
            claim_id: self.claim_id,
            statement: self.statement,
            grade: self.grade,
            primary_citations: self.primary_citations,
            contradicting: self.contradicting,
            safety_critical: self.safety_critical,
            contested: self.contested,
            review_months: self.review_months,
            surfaced,
        }
    }
}

/// An open question where evidence conflicts; the engine holds a default lean.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContestedQuestion {
    pub id: &'static str,
    pub question: &'static str,
    /// Current engine default / lean while the question stays open.
    pub engine_default: &'static str,
    /// One credible OPPOSING view from the literature, the side the engine's
    /// `engine_default` does NOT take, in plain language, with its KB citation
    /// (shown as "experts differ … one view: {other_side}"). `None` when the KB
    /// names no attributable opposing reference for this question (the
    /// dissenting camp is only "some coaches" / vendor marketing /
    /// unattributed); NEVER fabricated - every populated string is sourced
    /// verbatim-in-substance from the contested-question tables in
    /// `knowledge-base/extracted/` (File 09 global table + Files 02/03 local
    /// lists). HARD RULE 1.
    pub other_side: Option<&'static str>,
}

/// Declares [`ClaimId`] over the discipline groups in [`claims`] plus the
/// lookups that dispatch on the group.
macro_rules! claim_groups {
    ($($variant:ident($group:ty)),* $(,)?) => {
        /// Compile-checked handle for one registry row: its discipline group
        /// and the row within it. Named through [`claims`]
        /// (`claims::RUN_SPIKE_001`).
        #[derive(Clone, Copy, PartialEq, Eq, Hash)]
        pub enum ClaimId {
            $($variant($group)),*
        }

        impl ClaimId {
            /// The registry row this id names.
            pub fn entry(self) -> &'static EvidenceEntry {
                match self {
                    $(Self::$variant(row) => row.entry()),*
                }
            }
        }

        /// The row name alone (`RUN_SPIKE_001`): the `claim_id` spelled with
        /// underscores.
        impl fmt::Debug for ClaimId {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                match self {
                    $(Self::$variant(row) => fmt::Debug::fmt(row, f)),*
                }
            }
        }

        /// Every registry row, group by group.
        pub fn all_claims() -> impl Iterator<Item = &'static EvidenceEntry> {
            [$(<$group>::rows()),*].into_iter().flatten()
        }
    };
}

claim_groups! {
    Canonical(claims::CanonicalClaim),
    Strength(claims::StrengthClaim),
    Hypertrophy(claims::HypertrophyClaim),
    Running(claims::RunningClaim),
    Readiness(claims::ReadinessClaim),
    Safety(claims::SafetyClaim),
    Feedback(claims::FeedbackClaim),
    Hybrid(claims::HybridClaim),
    Calisthenics(claims::CalisthenicsClaim),
    Individualization(claims::IndividualizationClaim),
    Myth(claims::MythClaim),
}

/// Wrap a raw value in a [`Recommended`] backed by registry claim `id`, the
/// constructor every engine rule funnels through.
pub(crate) fn graded<T>(value: T, id: ClaimId) -> Recommended<T> {
    Recommended::new(value, id)
}

/// Look up a contested question by id.
pub fn contested_question(id: &str) -> Option<&'static ContestedQuestion> {
    CONTESTED_QUESTIONS.iter().find(|q| q.id == id)
}
