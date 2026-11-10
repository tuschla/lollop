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
//! Every `statement` is shown to users verbatim, so it is plain language
//! (`statements_are_plain_language`); ids live only in `claim_id`. Short-form
//! citations only; full statistics + DOIs live in
//! `knowledge-base/extracted/09-evidence-map.md`.

/// The row identifier `$id` spelled with dashes (`RUN_SPIKE_001` →
/// `"RUN-SPIKE-001"`), built at compile time so each id is written once.
macro_rules! dashed_id {
    ($id:ident) => {{
        const BYTES: [u8; stringify!($id).len()] = $crate::evidence::dash_bytes(stringify!($id));
        const ID: &str = match ::core::str::from_utf8(&BYTES) {
            Ok(id) => id,
            Err(_) => panic!("row identifiers are ASCII"),
        };
        ID
    }};
}

/// Declares one discipline's rows: an enum with one variant per row, a
/// [`ClaimId`] constant per row, and the rows in variant order, so
/// `ROWS[row as usize]` is the row a variant names. A row's `claim_id` is its
/// name spelled with dashes (`RUN_SPIKE_001` → `"RUN-SPIKE-001"`). A row
/// prefixed `#[kb_only]` is recorded from the knowledge base but cited by no
/// engine rule.
macro_rules! claim_group {
    (@surfaced) => {
        true
    };
    (@surfaced kb_only) => {
        false
    };
    ($group:ident => $variant:ident; $( $(#[$marker:ident])? $id:ident => $draft:expr ),* $(,)?) => {
        /// One variant per registry row of this discipline group.
        #[allow(non_camel_case_types)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $group {
            $($id),*
        }

        $(pub const $id: $crate::evidence::ClaimId = $crate::evidence::ClaimId::$variant($group::$id);)*

        static ROWS: &[$crate::evidence::EvidenceEntry] = &[
            $($crate::evidence::EvidenceEntry::from_draft(
                $draft,
                $id,
                dashed_id!($id),
                claim_group!(@surfaced $($marker)?),
            )),*
        ];

        impl $group {
            /// The registry row this variant names.
            pub(crate) fn entry(self) -> &'static $crate::evidence::EvidenceEntry {
                &ROWS[self as usize]
            }

            /// Every row of this group, in variant order.
            pub(crate) fn rows() -> &'static [$crate::evidence::EvidenceEntry] {
                ROWS
            }
        }
    };
}

mod cite;
pub mod claims;
mod contested;
mod draft;
mod grade;
#[cfg(test)]
mod tests;

use std::fmt;

pub use contested::{CONTESTED_QUESTIONS, ContestedId, ContestedQuestion};
use draft::ClaimDraft;
pub use grade::EvidenceGrade;

/// `id` with every `_` replaced by `-`; `N` is `id.len()`.
const fn dash_bytes<const N: usize>(id: &str) -> [u8; N] {
    let bytes = id.as_bytes();
    let mut out = [0; N];
    let mut i = 0;
    while i < N {
        out[i] = if bytes[i] == b'_' { b'-' } else { bytes[i] };
        i += 1;
    }
    out
}

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
    /// The contested question the claim is under active debate in.
    pub contested: Option<ContestedId>,
    /// Re-grade cadence in months (12 by default; 6 for fast-moving topics).
    pub review_months: u8,
    /// `false` for a row marked `#[kb_only]`: recorded from the knowledge base
    /// for completeness but cited by no engine rule. Every surfaced non-myth
    /// row is cited somewhere (`every_surfaced_claim_is_cited`).
    pub surfaced: bool,
}

impl EvidenceEntry {
    /// The finished row `id` names from its `draft`, keyed `claim_id`.
    const fn from_draft(
        draft: ClaimDraft,
        id: ClaimId,
        claim_id: &'static str,
        surfaced: bool,
    ) -> Self {
        EvidenceEntry {
            id,
            claim_id,
            statement: draft.statement,
            grade: draft.grade,
            primary_citations: draft.primary_citations,
            contradicting: draft.contradicting,
            safety_critical: draft.safety_critical,
            contested: draft.contested,
            review_months: draft.review_months,
            surfaced,
        }
    }

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
