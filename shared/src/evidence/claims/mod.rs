//! Claim ids by name, so a call site reads `claims::RUN_SPIKE_001`. The
//! registry rows live in one file per discipline; each file's
//! `claim_group!` re-exports here as `claims::<ROW>` constants.

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
