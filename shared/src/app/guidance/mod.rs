//! Profile-driven, evidence-cited guidance rows (safety, strength, running,
//! environment, re-entry, hybrid, calisthenics) and the static reference list.

use super::*;

mod calisthenics;
mod hybrid;
mod profile;
mod reference;
mod rows;
mod running;
mod strength;

pub(super) use calisthenics::*;
pub(super) use profile::*;
pub(super) use reference::*;
pub(super) use rows::*;

// Guidance-internal: assembled into the profile list by `profile.rs`.
use hybrid::build_hybrid_guidance;
use running::build_running_guidance;
use strength::build_strength_guidance;
