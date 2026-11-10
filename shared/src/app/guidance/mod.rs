//! Profile-driven, evidence-cited guidance rows (safety, strength, running,
//! environment, re-entry, hybrid, calisthenics) and the static reference list.

mod calisthenics;
mod hybrid;
mod profile;
mod reference;
mod rows;
mod running;
mod strength;

pub(super) use calisthenics::build_calisthenics_guidance;
#[cfg(test)]
pub(super) use profile::build_guidance;
pub(super) use profile::{build_guidance_measured, stated_or_measured_weekly_km};
pub(super) use reference::{DEFICIT_REFUSAL, build_reference};
pub(super) use rows::{push_guidance, push_guidance_basis, push_guidance_mi, push_hold_row};
