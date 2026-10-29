//! "Last time" on the next session: what the athlete did last on each lift and
//! what changed today.

use super::*;
use std::fmt::Write as _;

/// Append to each lift item's `why.basis` what the athlete did last time on
/// that movement and what changed today, with the reason when the engine
/// knows it (ADH-RATIONALE-001; feedback-019).
pub(in crate::app) fn annotate_last_time(
    ns: &mut SessionPlanView,
    model: &Model,
    set_index: &SetIndex,
    anchors: &crate::plan::Anchors,
    weight_lb: bool,
) {
    let offset = model.today_utc_offset_sec;
    for it in ns.items.iter_mut() {
        // Effort-prescribed items already show their last top set as the
        // `anchored_on` line ("Last: 60 kg × 8"); never repeat it in the basis.
        let last_on_face = it.anchored_on_single_set && it.last_set.is_some();
        if it.is_run() || it.timed_hold || last_on_face {
            continue;
        }
        // Same selection rule as the plan's "Last:" line, on the raw local day.
        let Some((_, top_i)) = last_top_set(
            &model.sets,
            set_index.same_movement_sets(&it.exercise),
            ns.epoch_day,
            |i| {
                let at = model.sets[i].observed_at;
                (at > 0).then(|| local_day(at, offset))
            },
        ) else {
            continue;
        };
        let top = &model.sets[top_i];
        // "with 2 reps left" from the rated effort (never "@ RPE 8" behind the
        // "?" either: one effort vocabulary everywhere).
        let left = if top.rpe > 0.0 {
            let n = crate::strength::rpe_to_rir(top.rpe).round().max(0.0) as u8;
            format!(" with {}", reserve(n))
        } else {
            String::new()
        };
        let mut line = format!(
            "Last time: {} × {}{left}.",
            fmt_weight(top.weight_kg, weight_lb),
            top.reps
        );
        if let Some(load) = it.load_kg {
            let diff = load - top.weight_kg;
            if diff.abs() >= 0.5 {
                let dir = if diff > 0.0 { "heavier" } else { "lighter" };
                let _ = write!(
                    line,
                    " Today is {} {dir}",
                    fmt_weight(diff.abs(), weight_lb)
                );
                let derated = anchors
                    .load_frac_for(&it.exercise)
                    .is_some_and(|d| matches!(d.cause, crate::plan::DerateCause::E1rmDrop));
                let last_e1 = pr_e1rm(top, set_index.facts(top_i), false);
                let anchor = anchors.e1rm_for(&it.exercise);
                let reason = if diff < 0.0 && derated {
                    format!(
                        " because your last session's estimated max came in more than {:.0}% under your best.",
                        (1.0 - crate::app::anchors::E1RM_DROP_GATE) * 100.0
                    )
                } else if diff < 0.0 && !it.adjusted_note.is_empty() {
                    " because of today's adjustment.".to_string()
                } else if diff > 0.0
                    && let (Some(e), Some(a)) = (last_e1, anchor)
                    && (e - a).abs() <= 0.5
                {
                    " because that session set your current estimated max.".to_string()
                } else {
                    ".".to_string()
                };
                line.push_str(&reason);
            }
        }
        it.why.append_basis(&line);
    }
}
