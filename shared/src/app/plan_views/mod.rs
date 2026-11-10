//! The plan surface: each day's prescriptions flattened into shell-facing
//! items, judged for completion, folded with the adjustments in force, and laid
//! out as the week strip, the next session, the program summary and the
//! consistency card.

mod adjust_fold;
mod adjustments;
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
mod week_anchors;
mod week_moves;

pub(super) use adjustments::{name_modified_exercises, restate_basis};
pub(super) use calis_views::build_calisthenics_ladders;
pub(super) use completion::{finished_status, last_top_set};
pub(super) use consistency::build_consistency;
pub(super) use first_set::apply_first_set_autoreg;
pub(super) use flatten_run::vdot_band_name;
pub(super) use item_render::run_planned;
pub(super) use plan_week::{PlanViews, build_plan_views};

#[cfg(test)]
pub(super) use adjustments::apply_adjustments_to_session;
#[cfg(test)]
pub(super) use completion::{apply_last_sets, refresh_item_statuses};
#[cfg(test)]
pub(super) use flatten::{flatten_prescription, plan_session_title};
#[cfg(test)]
pub(super) use plan_week::PLAN_SYNTH_BUILDS;
#[cfg(test)]
pub(super) use program_summary::program_name;
