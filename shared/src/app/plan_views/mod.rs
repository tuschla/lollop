//! The plan surface: each day's prescriptions flattened into shell-facing
//! items, judged for completion, folded with the adjustments in force, and laid
//! out as the week strip, the next session, the program summary and the
//! consistency card.

mod adjust_fold;
mod adjustments;
mod basis;
mod calis_views;
mod completion;
mod consistency;
mod first_set;
mod flatten;
mod flatten_lift;
mod flatten_run;
mod item_render;
mod plan_item;
mod plan_week;
mod program_summary;
mod run_targets;
mod week_anchors;
mod week_moves;

pub(super) use basis::render_basis;
pub(super) use calis_views::build_calisthenics_ladders;
pub(super) use completion::{finished_status, last_top_set};
pub(super) use consistency::build_consistency;
pub(super) use first_set::{ItemCue, apply_first_set_autoreg};
pub(super) use flatten_run::vdot_band_name;
pub(super) use item_render::run_planned;
pub(super) use plan_item::{PlanItem, PlanRow};
pub(super) use plan_week::{PlanViews, build_plan_views};

#[cfg(test)]
pub(super) use flatten::{flatten_prescription, plan_session_title};
#[cfg(test)]
pub(super) use item_render::EASY_RUN_LABEL;
#[cfg(test)]
pub(super) use plan_item::HrAnchor;
#[cfg(test)]
pub(super) use run_targets::{TargetBand, band_for, resolve_target};
