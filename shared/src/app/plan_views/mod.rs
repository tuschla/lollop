//! The plan surface: each day's prescriptions flattened into shell-facing
//! items, judged for completion, folded with the adjustments in force, and laid
//! out as the week strip, the next session, the program summary and the
//! consistency card.

use super::*;

mod adjustments;
mod calis_anchors;
mod calis_views;
mod completion;
mod consistency;
mod first_set;
mod flatten;
mod flatten_lift;
mod flatten_run;
mod item_render;
mod plan_week;
mod program_summary;
mod week_moves;

use adjustments::*;
use calis_anchors::*;
use calis_views::*;
use completion::*;
use consistency::*;
use flatten_lift::*;
use flatten_run::*;
use item_render::*;
use program_summary::*;
use week_moves::*;

pub(super) use adjustments::{name_modified_exercises, restate_basis_load};
pub(super) use calis_anchors::build_calisthenics_anchors;
pub(super) use calis_views::build_calisthenics_ladders;
pub(super) use completion::{LoggedDays, last_top_set};
pub(super) use consistency::{build_consistency, plan_week_index, weekday_name};
pub(super) use first_set::apply_first_set_autoreg;
pub(super) use flatten::{flatten_prescription, plan_session_title};
pub(super) use flatten_run::vdot_band_name;
pub(super) use item_render::run_planned;
pub(super) use plan_week::build_plan_views;

/// The one wording for "skipping a session is fine" (ADH-MISSED-001), shared
/// by the missed-session note and the move notes.
const MISSED_SESSION_COSTS_LITTLE: &str = "One missed session costs little.";

#[cfg(test)]
pub(super) use adjustments::apply_adjustments_to_session;
#[cfg(test)]
pub(super) use completion::{apply_last_sets, refresh_item_statuses};
#[cfg(test)]
pub(super) use item_render::plates_per_side;
#[cfg(test)]
pub(super) use plan_week::PLAN_SYNTH_BUILDS;
