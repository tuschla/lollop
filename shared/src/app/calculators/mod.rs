//! Calculator and report builders: training load, the weekly running report,
//! race predictions, lift-plan calculators, heart-rate zones.

mod hr;
mod lift_calcs;
mod race;
mod training_load;
mod weekly_report;

#[cfg(test)]
pub(super) use hr::build_hr_zones;
pub(super) use hr::hr_zones_for;
pub(super) use lift_calcs::{build_hypertrophy_plan, build_protein_targets, calculator_hold};
pub(super) use race::to_race_view;
pub(super) use training_load::{build_srpe_load, build_training_load};
pub(super) use weekly_report::build_weekly_report;

#[cfg(test)]
mod tests;
