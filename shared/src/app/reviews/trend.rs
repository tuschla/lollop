//! The longitudinal trend row: per main lift, the best estimated max over the
//! last 4 weeks against the 4 before.

use super::*;
use crate::feedback;

/// 4-week blocks the trend reads back from today: the current block, the one
/// it is compared against, and the rest of the plateau run.
const TREND_BLOCKS: usize = 7;

/// Best reliable e1RM per movement in each 4-week block back from `today`
/// (block `k` covers local days `today - 28k - 27 ..= today - 28k`), in one
/// pass over the sets.
fn block_bests(model: &Model, key: &str, today: i64, calis: bool) -> [Option<f64>; TREND_BLOCKS] {
    let offset = model.today_utc_offset_sec;
    let mut best = [0.0_f64; TREND_BLOCKS];
    for s in &model.sets {
        if s.observed_at <= 0 {
            continue;
        }
        let Ok(back) = usize::try_from(today.saturating_sub(local_day(s.observed_at, offset)))
        else {
            continue;
        };
        let k = back / 28;
        if k >= TREND_BLOCKS || !crate::plan::same_movement(&s.exercise, key) {
            continue;
        }
        if let Some(e) = pr_e1rm(s, calis) {
            best[k] = best[k].max(e);
        }
    }
    best.map(|b| (b > 0.0).then_some(b))
}

/// One lift's rolling trend: best estimated max over the last 4 weeks vs the
/// 4 weeks before, banded on the ±5 % noise band, and how many consecutive
/// 4-week blocks have been flat.
struct LiftTrend {
    exercise: String,
    current: f64,
    delta: f64,
    dir: feedback::TrendDirection,
    weeks_flat: u8,
}

fn lift_trend(model: &Model, exercise: &str, today: i64, calis: bool) -> Option<LiftTrend> {
    let blocks = block_bests(model, exercise, today, calis);
    let current = blocks[0]?;
    let previous = blocks[1]?;
    let delta = current / previous - 1.0;
    let dir = if delta > E1RM_NOISE_FRAC {
        feedback::TrendDirection::Up
    } else if delta < -E1RM_NOISE_FRAC {
        feedback::TrendDirection::Down
    } else {
        feedback::TrendDirection::Flat
    };
    let mut weeks_flat = 0u8;
    if dir == feedback::TrendDirection::Flat {
        for pair in blocks.windows(2) {
            match (pair[0], pair[1]) {
                (Some(a), Some(b)) if (a / b - 1.0).abs() <= E1RM_NOISE_FRAC => {
                    weeks_flat = weeks_flat.saturating_add(4);
                }
                _ => break,
            }
        }
    }
    Some(LiftTrend {
        exercise: exercise.to_string(),
        current,
        delta,
        dir,
        weeks_flat,
    })
}

/// The feedback-027/028/029 longitudinal trend message, derived from the
/// logged lifts (per main lift: best estimated max over the last 4 weeks vs
/// the 4 before, ±5 % noise band), with recovery read from the weekly
/// check-in's low-recovery flag or a ≥2-day derived wellness suppression. A
/// run spike never explains a lift trend; it only feeds the legacy stated
/// trend, and only while fresh. Falls back to a trend stated on a legacy
/// review. `None` when nothing decisive (a steady trend says nothing) or while
/// `suppressed` (a concern card leads or training is blocked: no praise and no
/// causal story competes with it, feedback-039/041/042).
pub(in crate::app) fn build_trend(
    model: &Model,
    weekly: Option<&SessionReview>,
    derived_wellness_streak: Option<u8>,
    spike: Option<RunSpike>,
    suppressed: bool,
    weight_lb: bool,
) -> Option<AdjustmentView> {
    if suppressed {
        return None;
    }
    let fresh_spike = spike.filter(|s| match (model.today_epoch_day, s.day) {
        (Some(t), Some(d)) => t.saturating_sub(d) <= FEEDBACK_FRESH_DAYS,
        _ => true,
    });
    let low_recovery =
        weekly.is_some_and(|w| w.low_recovery) || derived_wellness_streak.is_some_and(|s| s >= 2);
    let load_spike = fresh_spike.is_some_and(|s| s.frac > running::SPIKE_FRAC)
        || weekly
            .and_then(|w| w.single_session_spike_frac)
            .is_some_and(|f| f > running::SPIKE_FRAC);
    // feedback-028: exactly ONE lever, taken from the KB rule for the goal
    // (hypertrophy-008 adds sets; a strength stall takes a lighter week).
    let (lever, lever_claim) = if model
        .profile
        .as_ref()
        .is_some_and(|p| p.lift_goal == LiftGoal::Hypertrophy)
    {
        (
            "add 2 sets a week for the muscles involved in your next training cycle, as long as you're recovering easily",
            claims::HYP_MESO_ADD_001,
        )
    } else {
        (
            "take a lighter week, then build again",
            claims::STR_STALL_001,
        )
    };
    if let Some(today) = model.today_epoch_day {
        let calis = model
            .profile
            .as_ref()
            .is_some_and(|p| p.resistance_modality == ResistanceModality::Calisthenics);
        // Distinct movements trained in the last 4 weeks.
        let offset = model.today_utc_offset_sec;
        let mut names: Vec<&str> = Vec::new();
        for s in &model.sets {
            if s.observed_at > 0
                && today.saturating_sub(local_day(s.observed_at, offset)) <= 27
                && !names
                    .iter()
                    .any(|n| crate::plan::same_movement(n, &s.exercise))
            {
                names.push(&s.exercise);
            }
        }
        let mut picked: Option<(
            u8,
            LiftTrend,
            feedback::TrendSummary,
            Recommended<feedback::TrendSummary>,
        )> = None;
        for name in names {
            let Some(t) = lift_trend(model, name, today, calis) else {
                continue;
            };
            let rec = feedback::trend_summary(
                t.dir,
                t.weeks_flat,
                t.dir == feedback::TrendDirection::Down,
                false,
                low_recovery,
            );
            let rank = match rec.value {
                feedback::TrendSummary::LoadExplainedDecline => 3,
                feedback::TrendSummary::Improving => 2,
                feedback::TrendSummary::Plateau => 1,
                feedback::TrendSummary::Stable => 0,
            };
            if rank > 0 && picked.as_ref().is_none_or(|(r, ..)| rank > *r) {
                picked = Some((rank, t, rec.value, rec));
            }
        }
        if let Some((_, t, summary, rec)) = picked {
            let max = fmt_weight(t.current, weight_lb);
            let text = match summary {
                feedback::TrendSummary::Improving => format!(
                    "{}: your best estimated max over the last 4 weeks is {max}, up {:.0}% on the 4 weeks before. Next, keep each session's effort on target.",
                    t.exercise,
                    t.delta * 100.0
                ),
                feedback::TrendSummary::Plateau => {
                    let text = format!(
                        "{} has held steady for {} weeks (estimated max {max}). Plateaus like this are common. One change to try: {lever}.",
                        t.exercise, t.weeks_flat
                    );
                    return Some(to_view_with(text, &graded(summary, lever_claim)));
                }
                feedback::TrendSummary::LoadExplainedDecline => format!(
                    "{} is down {:.0}% over the last 4 weeks. Low recovery may explain it rather than lost fitness. A lighter week is worth trying first.",
                    t.exercise,
                    t.delta.abs() * 100.0,
                ),
                feedback::TrendSummary::Stable => unreachable!("Stable is never picked"),
            };
            return Some(to_view_with(text, &rec));
        }
    }
    // Legacy: a trend stated on a review payload.
    let w = weekly?;
    let dir = w.trend_direction?;
    let t = feedback::trend_summary(
        dir,
        w.weeks_flat.unwrap_or(0),
        w.performance_down,
        load_spike,
        w.low_recovery,
    );
    let text = match t.value {
        feedback::TrendSummary::Improving => {
            "Your lifts are trending up. Next, keep each session's effort on target.".to_string()
        }
        feedback::TrendSummary::Plateau => {
            let text = format!(
                "Flat for 4+ weeks. Plateaus like this are common. One change to try: {lever}."
            );
            return Some(to_view_with(text, &graded(t.value, lever_claim)));
        }
        feedback::TrendSummary::LoadExplainedDecline => {
            "The dip may come from training load and low recovery rather than lost fitness. A lighter week is worth trying first.".to_string()
        }
        feedback::TrendSummary::Stable => return None,
    };
    Some(to_view_with(text, &t))
}
