//! Claim ids by name, so a call site reads `claims::RUN_SPIKE_001`. The
//! registry rows live in one file per discipline; each file's
//! `claim_group!` re-exports here as `claims::<ROW>` constants.

/// Declares one discipline's rows: an enum with one variant per row, a
/// [`ClaimId`](crate::evidence::ClaimId) constant per row, and the rows in
/// variant order, so `ROWS[row as usize]` is the row a variant names. A row
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
            $($draft.row($id, claim_group!(@surfaced $($marker)?))),*
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

mod calisthenics;
mod canonical;
mod feedback;
mod hybrid;
mod hypertrophy;
mod individualization;
mod myths;
mod readiness;
mod running;
mod safety;
mod strength;

pub use calisthenics::*;
pub use canonical::*;
pub use feedback::*;
pub use hybrid::*;
pub use hypertrophy::*;
pub use individualization::*;
pub use myths::*;
pub use readiness::*;
pub use running::*;
pub use safety::*;
pub use strength::*;
