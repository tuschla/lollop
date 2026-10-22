//! Calculator and report builders: training load, the weekly running report,
//! race and field-test estimates, lift-plan calculators, heart-rate zones.

use super::*;

mod hr;
mod lift_calcs;
mod race;
mod training_load;
mod weekly_report;

pub(super) use hr::*;
pub(super) use lift_calcs::*;
pub(super) use race::*;
pub(super) use training_load::*;
pub(super) use weekly_report::*;

#[cfg(test)]
mod tests;
