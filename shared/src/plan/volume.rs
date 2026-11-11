//! Per-session working sets from the athlete's weekly per-muscle target,
//! shared by the lift and calisthenics day builders.

use crate::hypertrophy;
use crate::individualization::TrainingAge;

/// What [`sized_sets`] decided: the per-session working sets, plus whether the
/// accumulation ramp rather than the stated weekly target produced that count
/// (the evidence face follows the claim that set the number).
pub(super) struct SizedSets {
    pub(super) per_session: u8,
    pub(super) ramped: bool,
}

/// Per-session working sets from the stated weekly per-muscle target
/// (`weekly_sets`). The target is bounded to the primary muscle's Table 1
/// MEV..MRV (HYP-VOL-DOSE-001, HYP-LANDMARKS-001; without a Table 1 row the
/// training-age MEV, HYP-MEV-AGE-001, and the generic ~20-set MRV,
/// HYP-MRV-DELOAD-001), ramped across the block toward the muscle's MAV high
/// (HYP-VOL-CAP-001, HYP-SETRAMP-001; week 1 is the stated target), split evenly
/// over the `n_lift_days` that train it (EXPERT-OPINION, flagged on
/// HYP-VOL-DOSE-001), then clamped to the per-exercise set limits
/// (HYP-SESSCAP-001) and the day's own `band`. `ramped` tells the caller which
/// claim set the number.
pub(super) fn sized_sets(
    weekly_sets: u8,
    ta: TrainingAge,
    n_lift_days: usize,
    band: (u8, u8),
    landmarks: Option<&hypertrophy::VolumeLandmarks>,
    week_in_block: u8,
    block_weeks: u8,
) -> SizedSets {
    let mev_floor = landmarks
        .map(|l| l.mev)
        .unwrap_or_else(|| hypertrophy::mev_sets_by_training_age(ta).value.0);
    let mrv_ceiling = landmarks.map_or(hypertrophy::GENERIC_MRV_SETS, |l| l.mrv);
    let start = weekly_sets.clamp(mev_floor, mrv_ceiling);
    // This week's point on the ramp; `weekly_set_ramp` opens at `start`, and a
    // stated target above the ramp top is held.
    let ramp_top = landmarks.map_or(mrv_ceiling, |l| l.mav.1);
    let ramp = hypertrophy::weekly_set_ramp(start, ramp_top, block_weeks).value;
    let bounded_weekly = ramp
        .get((week_in_block.max(1) - 1) as usize)
        .copied()
        .unwrap_or(start);
    let n = n_lift_days.max(1) as f64;
    let split = |weekly: u8| -> u8 {
        ((weekly as f64 / n).round() as u8)
            .clamp(
                hypertrophy::MIN_SETS_PER_EXERCISE,
                hypertrophy::PER_SESSION_SET_CEILING,
            )
            .clamp(band.0, band.1)
    };
    let per_session = split(bounded_weekly);
    SizedSets {
        per_session,
        // Only claim the ramp when it actually moved the prescribed count: a
        // week whose extra weekly volume vanishes in the split or the band clamp
        // is still the stated-target number.
        ramped: per_session != split(start),
    }
}
