//! The plan anchors of one plan week: the history through a day assembled into
//! the [`crate::plan::Anchors`] that week is synthesized against.

use crate::app::anchors::{AnchorCtx, build_plan_anchors, build_run_break};
use crate::app::calis_anchors::{build_calisthenics_anchors, calis_placements_through};
use crate::app::logged_days::lift_deload_days;
use crate::app::model::stated_since;
use crate::app::readiness::manual_inputs_on;
use crate::app::view_ctx::ViewCtx;
use crate::profile::Profile;

/// The anchors of the plan week starting `monday` (the plan started on
/// `plan_start`) for the day after `through`, prescribed with `day_profile`
/// (the profile effective that day): the history attributed to session days
/// `<= through` (undated rows always count), so the sets logged ON a day never
/// move its own target; readiness inputs use the same cutoff and keep
/// removals dated after it. `through` is also the decay/window reference.
/// `run_week_km` is `anchors::build_run_week_km` for `monday`.
pub(super) fn plan_week_anchors(
    cx: &ViewCtx,
    day_profile: &Profile,
    through: i64,
    monday: i64,
    plan_start: i64,
    run_week_km: &[f64],
) -> crate::plan::Anchors {
    let model = cx.model;
    let offset = model.today_utc_offset_sec;
    let dated_sets = || {
        cx.set_index
            .with_facts(&model.sets)
            .enumerate()
            .filter(|(i, _)| cx.days.sets[*i].is_none_or(|d| d <= through))
            .map(|(_, set)| set)
    };
    let stated_longest_on = stated_since(model, day_profile, |q| q.stated_longest_recent_km);
    // An undeclared break from running (RUN-REENTRY-001), unless the weekly
    // distance was stated after it began (a current answer). A longest run
    // stated after it began is a current answer too (RUN-START-001), so it
    // counts through the long-run rebuild.
    let run_break = build_run_break(&model.runs, &cx.runs, through, offset)
        .filter(|b| {
            stated_since(model, day_profile, |q| {
                Some(q.running_km_per_week).filter(|km| *km > 0.0)
            })
            .is_none_or(|s| s <= b.last_run_epoch_day)
        })
        .map(|b| crate::plan::RunBreak {
            stated_longest_after: stated_longest_on.is_some_and(|s| s > b.last_run_epoch_day),
            ..b
        });
    let day_inputs = manual_inputs_on(model, through, through.saturating_add(1), offset);
    crate::plan::Anchors {
        lift: build_plan_anchors(
            dated_sets(),
            AnchorCtx::for_profile(
                Some(day_profile),
                model,
                offset,
                lift_deload_days(model, &cx.days),
            ),
            through,
            offset,
        ),
        run: cx.run_anchors_on(through),
        stated_longest_in_window: stated_longest_on
            .is_some_and(|d| through.saturating_sub(d) <= crate::running::SPIKE_WINDOW_DAYS),
        calisthenics: build_calisthenics_anchors(
            dated_sets().map(|(s, facts)| (s, facts.calis_tier)),
            &calis_placements_through(model, through),
            &day_inputs,
            day_profile.lift_goal,
            Some(through),
            offset,
        ),
        // Pace anchor: a fresh recent race (≤8 wk) on the day being
        // prescribed.
        run_vdot: crate::app::run_targets::pace_anchor_vdot(day_profile, through.saturating_add(1)),
        // Race-date periodization dates the week; the cross-cycle volume
        // carry reads whole calendar weeks before it (RUN-VOLUME-001).
        week_start_epoch_day: Some(monday),
        run_week_km: run_week_km.to_vec(),
        plan_start_epoch_day: Some(plan_start),
        run_break,
    }
}
