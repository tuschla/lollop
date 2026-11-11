//! Running week synthesis. `run_week_plan` decides the week: its phase (a
//! race-date Base → Build → Peak → Taper sequence, or a rolling Base/Build
//! cycle), whether it is a planned recovery week, its weekly volume (cycle
//! ramp, cross-cycle carry from what the athlete actually ran, taper cut) and
//! the session kinds (long run + the goal/phase quality mix + easy fill), or
//! the C25K programme week. `schedule::place_week` places those kinds on days,
//! then `run_week_sessions` prescribes each placed day: sized within the KB
//! volume caps (one [`QualityRule`] per quality kind), the long-run decision
//! and the RUN-SPIKE-001 distance ceilings, with warm-up/cool-down and
//! rep/recovery structure the tracker can step through.

use std::collections::BTreeSet;

use crate::evidence::{ClaimId, claims};
use crate::individualization::training_age_years;
use crate::profile::{PlanProfile, Profile};
use crate::running::{self, GoalDistance, RunWorkoutRx, daily_avg_run_km};
use crate::schema::{
    MesoPhase, Prescription, RepRecovery, RunIntensity, RunPrescription, RunSessionType, RunVolume,
    RxBasis, Session, SessionType, StridesAddOn, VdotBand,
};

use super::anchors::{Anchors, RunBreak};

/// Effective weekly running volume (km) for plan synthesis:
/// `max(stated, measured 28-day average)`; the stated answer is a floor
/// (RUN-LONGRUN-001 sizes from the higher one).
/// - Declared layoff (`weeks_off > 0`, REENTRY-001): the stated figure is
///   pre-layoff, so the measured volume governs once known. A newer
///   undeclared break outranks the declared layoff.
/// - Undeclared break with an active ramp (`anchors.run_break`): the floor is
///   `max(stated, prior weekly) × ramp fraction` (RUN-REENTRY-001); measured
///   volume since the break builds on it.
pub(super) fn effective_weekly_km(profile: &Profile, anchors: &Anchors) -> f64 {
    let stated = profile.running_km_per_week.max(0.0);
    let declared = profile.declared_layoff_return();
    // A break counts as newer when it began on or after the declared return
    // (an undatable declared layoff stays in force).
    let newer_break = anchors
        .run_break
        .filter(|b| declared.is_none_or(|r| r.is_some_and(|r| b.last_run_epoch_day >= r)));
    let returning = declared.is_some() && newer_break.is_none();
    if let Some(b) = newer_break.filter(|b| b.reentry.active) {
        let floor = stated.max(b.prior_weekly_km.unwrap_or(0.0)) * b.reentry.volume_frac;
        return anchors.run.recent_weekly_km.map_or(floor, |m| m.max(floor));
    }
    match anchors.run.recent_weekly_km {
        // Re-entry: measured mileage overrides a stale pre-layoff figure even
        // when it is LOWER (the profile stops being a floor after a break).
        Some(measured) if returning => measured,
        Some(measured) if measured > stated => measured,
        _ => stated,
    }
}

/// Shortest run prescribed as a distance, km. Below it (a sub-1 km walk-jog
/// baseline or share) no honest distance exists, so the run is prescribed in
/// minutes instead.
const MIN_DISTANCE_KM: f64 = 1.0;

/// Floor a prescribed distance to the plan's resolution: 0.5 km at or above
/// 5 km, 0.1 km below. Always FLOORED, so a rendered figure never exceeds the
/// cap it came from, yet fine enough that the +10 % RUN-SPIKE-001 allowance
/// over a sub-10 km longest run is a real step (6.0 km → 6.5 km, not 6 again).
pub(super) fn floor_km(km: f64) -> f64 {
    if !km.is_finite() || km <= 0.0 {
        return 0.0;
    }
    let step = if km >= 5.0 { 0.5 } else { 0.1 };
    let steps = (km / step + 1e-9).floor();
    (steps * step * 10.0).round() / 10.0
}

/// The RUN-SPIKE-001 plan-time ceiling: at most 10 % over the demonstrated
/// 30-day-longest baseline, floored to the plan resolution. Single source for
/// every session type; the log-time gate (`running::single_session_spike`)
/// tests the same raw baseline. Flooring to 0.5 km (≥5 km) / 0.1 km never lands
/// below a ≥ [`MIN_DISTANCE_KM`] baseline: 10 % of it is at least one step.
fn spike_ceiling_km(baseline: f64) -> f64 {
    floor_km((1.0 + crate::running::SPIKE_FRAC) * baseline)
}

/// Cap a prescribed run DISTANCE (km) at the RUN-SPIKE-001 threshold over the
/// demonstrated (or, before any log, stated) 30-day-longest baseline. When the
/// ceiling drops below [`MIN_DISTANCE_KM`] (a sub-1 km walk-jog baseline) no
/// honest distance exists, so this returns `None` and the caller falls back to
/// a DURATION. With no baseline (`None`) the distance is unchanged: the
/// log-time flag handles a first run honestly.
pub(super) fn cap_km_to_spike(km: f64, longest_recent_km: Option<f64>) -> Option<f64> {
    match longest_recent_km {
        Some(baseline) if baseline > 0.0 => {
            let ceiling = spike_ceiling_km(baseline);
            (ceiling >= MIN_DISTANCE_KM).then(|| km.min(ceiling))
        }
        _ => Some(km),
    }
}

/// VO2max rep count bounds per session (RUN-INTERVAL-001).
const INTERVAL_REPS: (u8, u8) = (3, 8);

/// VO2max rep distances, longest first (running-019: 800–1600 m, ≤5 min).
const INTERVAL_REP_M: [u16; 2] = [1200, 800];

const SHORTEST_INTERVAL_SET: (u8, u16) = (INTERVAL_REPS.0, INTERVAL_REP_M[1]);

/// Natural VO2max-interval rep count the weekly volume supports at a given rep
/// DISTANCE before any rep-floor clamp: `floor(<=8% weekly cap ÷ rep_m)`
/// (RUN-INTERVAL-001).
fn interval_reps_natural_at(weekly_km: f64, rep_m: u16) -> i64 {
    let s = running::vo2max_interval_rx().value;
    (s.weekly_cap_frac * weekly_km * 1000.0 / rep_m.max(1) as f64).floor() as i64
}

/// VO2max rep count at `rep_m`: the natural count clamped to
/// [`INTERVAL_REPS`]. Shared by the weekly accounting and the prescription.
fn interval_reps(weekly_km: f64, rep_m: u16) -> u8 {
    interval_reps_natural_at(weekly_km, rep_m)
        .clamp(i64::from(INTERVAL_REPS.0), i64::from(INTERVAL_REPS.1)) as u8
}

/// Seconds one VO2max rep of `rep_m` takes at mid I pace; `None` without a
/// pace anchor.
fn interval_rep_sec(rep_m: u16, vdot: Option<f64>) -> Option<f64> {
    vdot.and_then(|v| crate::running::vdot_band_pace_sec_per_km(v, VdotBand::Interval))
        .map(|(fast, slow)| (fast + slow) / 2.0 * f64::from(rep_m) / 1000.0)
}

/// The longest [`INTERVAL_REP_M`] distance whose rep floor fits the ≤8 % weekly
/// cap and, with a pace anchor, whose rep takes ≤5 min at mid I pace. `None`
/// when none fits.
fn supported_rep_distance_m(weekly_km: f64, vdot: Option<f64>) -> Option<u16> {
    let max_sec = f64::from(running::vo2max_interval_rx().value.rep_duration_min.1) * 60.0;
    INTERVAL_REP_M.into_iter().find(|&rep_m| {
        interval_reps_natural_at(weekly_km, rep_m) >= i64::from(INTERVAL_REPS.0)
            && interval_rep_sec(rep_m, vdot).is_none_or(|sec| sec <= max_sec)
    })
}

/// Distance cap for a threshold run: total threshold work ≤10% of weekly
/// volume (RUN-TEMPO-001), floored to the plan resolution.
pub(super) fn tempo_km_cap(weekly_km: f64) -> f64 {
    floor_km(running::THRESHOLD_WEEKLY_CAP_FRAC * weekly_km)
}

/// The shortest threshold run worth prescribing, km: running-018's 20-minute
/// continuous floor at the fast end of the athlete's T band when a race
/// anchors it, else [`TEMPO_FLOOR_FALLBACK_KM`].
fn tempo_floor_km(vdot: Option<f64>) -> f64 {
    let floor_min = running::workout_rx(RunSessionType::Tempo)
        .value
        .duration_min
        .0
        .unwrap_or(20) as f64;
    vdot.and_then(|v| crate::running::vdot_band_pace_sec_per_km(v, VdotBand::Threshold))
        .map(|(fast, _)| floor_min * 60.0 / fast.max(1.0))
        .unwrap_or(TEMPO_FLOOR_FALLBACK_KM)
}

/// The threshold-run floor without a pace anchor (RUN-TEMPO-001): running-018's
/// 20-minute floor at ~6:40/km, an expert-opinion stand-in with no pace to go
/// on.
const TEMPO_FLOOR_FALLBACK_KM: f64 = 3.0;

/// Whether the weekly volume supports a threshold run at/above its 20-minute
/// floor. Below it the slot stays an easy run rather than a sub-threshold
/// "tempo".
fn tempo_supported(weekly_km: f64, vdot: Option<f64>) -> bool {
    tempo_km_cap(weekly_km) >= tempo_floor_km(vdot)
}

/// Repetition session for the week: `(reps, rep_m)` at the longest rep
/// distance (400 m, then 200 m) whose minimum rep count fits the ≤5 % weekly
/// R cap (running-051, RUN-REPETITION-002). `None` when even 4 × 200 m breaks it.
fn repetition_plan(weekly_km: f64) -> Option<(u8, u16)> {
    let r = running::repetition_rx().value;
    r.rep_m.iter().find_map(|&m| {
        let n = (r.weekly_cap_frac * weekly_km * 1000.0 / m as f64).floor() as i64;
        (n >= r.min_reps as i64).then(|| (n.min(r.max_reps as i64) as u8, m))
    })
}

/// The smallest repetition set: the minimum rep count at the shortest rep
/// distance (4 × 200 m, RUN-REPETITION-002).
fn shortest_repetition_set() -> (u8, u16) {
    let r = running::repetition_rx().value;
    (r.min_reps, r.rep_m[r.rep_m.len() - 1])
}

/// The marathon-pace block band, km (running-017: the 8 km minimum block up
/// to the 26 km band top).
fn race_pace_band_km() -> (f64, f64) {
    let band = running::workout_rx(RunSessionType::RacePace)
        .value
        .distance_km;
    (band.0.unwrap_or(8.0), band.1.unwrap_or(26.0))
}

/// Marathon-pace block size, km (running-017/053): the [`race_pace_band_km`]
/// minimum block, growing with volume at 15 % of the week, never above the
/// 20 % weekly M-pace share, the band top, or Daniels' 110 min / 29 km cap.
/// `None` when the minimum would exceed 20 % of the week (the volume can't
/// carry a real M-pace block yet).
fn race_pace_km(weekly_km: f64, vdot: Option<f64>) -> Option<f64> {
    let (lo, hi) = race_pace_band_km();
    let mp = running::race_pace_rx().value;
    let share_cap = floor_km(mp.weekly_share_max * weekly_km);
    if share_cap < lo {
        return None;
    }
    let mut km = floor_km(mp.weekly_share_min * weekly_km)
        .max(lo)
        .min(share_cap)
        .min(hi);
    km = km.min(mp.max_km);
    if let Some(pace) =
        vdot.and_then(|v| crate::running::vdot_band_pace_sec_per_km(v, VdotBand::Marathon))
    {
        // Slow end of the M band: the most time a block of `km` can take.
        let time_cap_km = floor_km(mp.max_min as f64 * 60.0 / pace.1.max(1.0));
        km = km.min(time_cap_km);
    }
    (km >= lo).then_some(km)
}

/// A quality session's main set.
#[derive(Debug, Clone, Copy, PartialEq)]
enum QualitySet {
    /// One continuous block, km.
    Block(f64),
    /// `reps × rep_m` with the recovery between reps.
    Reps {
        reps: u8,
        rep_m: u16,
        recovery: RepRecovery,
    },
}

impl QualitySet {
    /// Run km of the main set: the reps with their jog-distance recoveries (a
    /// timed recovery is not distance).
    fn km(self) -> f64 {
        match self {
            QualitySet::Block(km) => km,
            QualitySet::Reps {
                reps,
                rep_m,
                recovery,
            } => {
                let jog_m = match recovery {
                    RepRecovery::Metres(m) => m,
                    _ => 0,
                };
                f64::from(reps) * f64::from(u32::from(rep_m) + u32::from(jog_m)) / 1000.0
            }
        }
    }
}

/// One quality kind as the planner sizes it. `weekly_km` and the pace anchor
/// (`vdot`) are the inputs of every rule.
struct QualityRule {
    /// The week's volume carries the session; an unsupported slot is planned
    /// as an easy run rather than a cap-violating session.
    supported: fn(f64, Option<f64>) -> bool,
    /// The session the week's volume sizes, within the kind's weekly cap;
    /// `None` when no size fits.
    sized: fn(f64, Option<f64>) -> Option<QualitySet>,
    /// The smallest session: a taper never runs less (running-037/038: only
    /// volume drops), and a taper week over its cut runs exactly this.
    floor: fn(Option<f64>) -> QualitySet,
    claim: ClaimId,
}

/// The planner's quality kinds; `None` for every other run kind.
fn quality_rule(kind: RunSessionType) -> Option<QualityRule> {
    let rule = match kind {
        // Threshold (RUN-TEMPO-001): ≤10 % of the week; the taper floor is the
        // 20-minute run rounded up to the 0.1 km step, so a taper shortens the
        // session instead of dropping it.
        RunSessionType::Tempo => QualityRule {
            supported: tempo_supported,
            sized: |weekly_km, _| Some(QualitySet::Block(tempo_km_cap(weekly_km))),
            floor: |vdot| QualitySet::Block((tempo_floor_km(vdot) * 10.0).ceil() / 10.0),
            claim: running::workout_rx(kind).claim_id(),
        },
        // VO2max intervals (RUN-INTERVAL-001); the taper floor is 3 × 800 m.
        RunSessionType::Interval => QualityRule {
            supported: |weekly_km, vdot| supported_rep_distance_m(weekly_km, vdot).is_some(),
            sized: |weekly_km, vdot| {
                supported_rep_distance_m(weekly_km, vdot)
                    .map(|rep_m| interval_set(interval_reps(weekly_km, rep_m), rep_m, vdot))
            },
            floor: |vdot| interval_set(SHORTEST_INTERVAL_SET.0, SHORTEST_INTERVAL_SET.1, vdot),
            claim: running::workout_rx(kind).claim_id(),
        },
        // Repetitions (running-051, RUN-REPETITION-002); the taper floor is
        // 4 × 200 m.
        RunSessionType::Repetition => QualityRule {
            supported: |weekly_km, _| repetition_plan(weekly_km).is_some(),
            sized: |weekly_km, _| repetition_plan(weekly_km).map(repetition_set),
            floor: |_| repetition_set(shortest_repetition_set()),
            claim: running::repetition_rx().claim_id(),
        },
        // Marathon-pace block (running-017/053); the taper floor is the 8 km
        // "MP touch" minimum.
        RunSessionType::RacePace => QualityRule {
            supported: |weekly_km, vdot| race_pace_km(weekly_km, vdot).is_some(),
            sized: |weekly_km, vdot| race_pace_km(weekly_km, vdot).map(QualitySet::Block),
            floor: |_| QualitySet::Block(race_pace_band_km().0),
            claim: running::workout_rx(kind).claim_id(),
        },
        _ => return None,
    };
    Some(rule)
}

/// A VO2max set: recovery ≈ the rep's own time (ratio ceiling 1.0,
/// running-019), in seconds when a pace anchor turns the rep into time, else
/// "jog as long as the rep took".
fn interval_set(reps: u8, rep_m: u16, vdot: Option<f64>) -> QualitySet {
    let ratio = running::vo2max_interval_rx()
        .value
        .recovery_max_ratio_of_rep;
    QualitySet::Reps {
        reps,
        rep_m,
        recovery: match interval_rep_sec(rep_m, vdot) {
            Some(sec) => RepRecovery::Seconds((sec * ratio).round().min(u16::MAX as f64) as u16),
            None => RepRecovery::EqualToRep,
        },
    }
}

/// A repetition set: an easy jog as far as the rep (running-051).
fn repetition_set((reps, rep_m): (u8, u16)) -> QualitySet {
    QualitySet::Reps {
        reps,
        rep_m,
        recovery: RepRecovery::Metres(rep_m),
    }
}

/// What the scheduler asks of the run side for one week.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct RunWeekRequest {
    /// Plan block index (completion-gated by the scheduler: a week with too
    /// little done does not advance). C25K reads it as the programme week.
    pub(super) week_index: u32,
    /// Run days the scheduler will actually place.
    pub(super) n_run_days: usize,
    /// ≤1: scales the base weekly km when the hybrid cap / weekday mask cut
    /// the athlete's stated run days.
    pub(super) volume_scale: f64,
    /// Lift-lean hybrid: no load-week ramp and no cross-cycle carry (running
    /// held at base); recovery and taper cuts still apply.
    pub(super) hold_volume: bool,
    /// Cap on quality sessions (lift-lean hybrid: `Some(1)`); `None` = the
    /// goal table.
    pub(super) max_quality: Option<usize>,
}

/// The run side's plan for one week. `kinds` carries no days: the scheduler
/// places them, then hands the placement to [`run_week_sessions`].
#[derive(Debug, Clone, PartialEq)]
pub(super) struct RunWeekPlan {
    /// Base / Build / Peak / Taper, or Deload on a planned recovery week.
    pub(super) phase: MesoPhase,
    /// Planned recovery week (volume −30 %, no quality). Never set in a taper.
    pub(super) recovery: bool,
    /// Run block length (load weeks + 1 recovery week; C25K: the 9 programme
    /// weeks) for the summary's "week N of M".
    pub(super) cycle_len: u32,
    /// Planned weekly km incl. warm-ups/cool-downs (0 for C25K).
    pub(super) weekly_km: f64,
    /// Session kinds to place. Normally `n_run_days` entries; in the race week
    /// no long run and only the days before the race.
    pub(super) kinds: Vec<RunSessionType>,
    /// The race's day offset (0 = Monday) when it falls in this week: place
    /// nothing on or after it.
    pub(super) race_day: Option<u16>,
    /// `(k, n)`: this is taper week `k` of `n`.
    pub(super) taper_week: Option<(u32, u32)>,
    /// How the placed days are prescribed.
    pub(super) programme: RunProgramme,
}

/// The programme a run week belongs to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum RunProgramme {
    /// NHS Couch to 5K (running-025): programme week `week` (1-based) of the
    /// run/walk table; no weekly km to size from.
    C25k { week: u8 },
    /// A distance plan: each day sized from the week's km.
    Distance {
        /// Strides go on the week's first easy days (running-020, from a few
        /// weeks into base).
        strides: bool,
        /// Race week only: no easy day runs longer than the previous taper
        /// week's daily average.
        race_week_easy_cap_km: Option<f64>,
    },
}

/// Where a week sits in the run cycle.
struct CyclePos {
    phase: MesoPhase,
    recovery: bool,
    /// 0-based load-week position (ramp step); `load_weeks − 1` in a taper.
    pos: u32,
    /// Calendar weeks between this week and the first week of its cycle: the
    /// carry window (the previous cycle) starts that many weeks back.
    cycle_offset: u32,
    taper: Option<(u32, u32)>,
    race_day: Option<u16>,
}

/// The run cycle position. With a race date ahead (race goals only) the
/// cycles are aligned BACKWARD from the taper so the last pre-taper week is the
/// top load week and recovery weeks never sit right before the taper; the phase
/// follows `running::race_phase`. Without one, a rolling cycle on the plan
/// block index alternates Base and Build cycles (General stays Base).
fn cycle_pos(profile: &Profile, anchors: &Anchors, week_index: u32, load_weeks: u32) -> CyclePos {
    let goal = profile.goal_distance;
    let cycle_len = load_weeks + 1;
    let race = match (profile.goal_race_epoch_day, anchors.week_start_epoch_day) {
        (Some(race), Some(week_start)) if race >= week_start && running::taper_weeks(goal) > 0 => {
            Some(race.saturating_sub(week_start))
        }
        _ => None,
    };
    if let Some(days_out) = race {
        let wtr = (days_out / 7).min(u32::MAX as i64) as u32;
        let race_day = (days_out < 7).then_some(days_out as u16);
        let taper_n = running::taper_weeks(goal);
        let phase = running::race_phase(goal, wtr)
            .map(|p| p.value)
            .unwrap_or(MesoPhase::Build);
        if wtr < taper_n {
            let elapsed = taper_n - 1 - wtr;
            return CyclePos {
                phase: MesoPhase::Taper,
                recovery: false,
                pos: load_weeks.saturating_sub(1),
                cycle_offset: load_weeks + elapsed,
                taper: Some((elapsed + 1, taper_n)),
                race_day,
            };
        }
        // The week's slot in its cycle, counted back from the taper (r = 0:
        // the last pre-taper week, the top load week): a load week's ramp
        // position, or `None` for the recovery week.
        let r = (wtr - taper_n) % cycle_len;
        let load_slot = (r < load_weeks).then(|| load_weeks - 1 - r);
        let offset = load_slot.unwrap_or(load_weeks);
        // A plan opens on a load week and never above its own ramp.
        let recovery = load_slot.is_none() && week_index > 0;
        let mut pos = if recovery {
            load_weeks
        } else {
            offset.min(week_index)
        };
        if !recovery {
            // running-032: progress one variable at a time. A week whose phase
            // steps intensity up (Base → Build → Peak) holds the previous
            // week's volume, so it and the rest of its cycle sit one ramp step
            // lower. Steps at a cycle's first week are already at base volume.
            let rank = |w: u32| match running::race_phase(goal, w).map(|p| p.value) {
                Some(MesoPhase::Peak) => 2,
                Some(MesoPhase::Build) => 1,
                _ => 0,
            };
            let held = (0..pos)
                .filter(|&i| rank(wtr.saturating_add(i)) > rank(wtr.saturating_add(i + 1)))
                .count() as u32;
            pos -= held;
        }
        return CyclePos {
            phase: if recovery { MesoPhase::Deload } else { phase },
            recovery,
            pos,
            cycle_offset: offset,
            taper: None,
            race_day,
        };
    }
    let pos = week_index % cycle_len;
    let recovery = pos == load_weeks;
    let cycle_index = week_index / cycle_len;
    let phase = if recovery {
        MesoPhase::Deload
    } else if goal == GoalDistance::General || cycle_index.is_multiple_of(2) {
        MesoPhase::Base
    } else {
        MesoPhase::Build
    };
    CyclePos {
        phase,
        recovery,
        pos: pos.min(load_weeks.saturating_sub(1)),
        cycle_offset: pos,
        taper: None,
        race_day: None,
    }
}

/// Weekly km above which a goal stops carrying volume up: where the share-
/// based long run reaches the goal's long-run target ceiling (running-046).
fn goal_volume_plateau_km(goal: GoalDistance) -> Option<f64> {
    let hi = running::goal_long_run_target_km(goal)?.value.1;
    let at_max_share = hi / running::LONG_RUN_SHARE_MAX;
    Some(if at_max_share < running::LONG_RUN_SHARE_SPLIT_KM {
        at_max_share
    } else {
        hi / running::LONG_RUN_SHARE_MIN
    })
}

/// Plan the run side of one week (see [`RunWeekPlan`]). `load_week_km(k)` is
/// the run km prescribed in the week `k` weeks back, read only in taper week
/// `k`: the last full load week, which the taper cuts from.
pub(super) fn run_week_plan(
    pp: &PlanProfile,
    anchors: &Anchors,
    req: &RunWeekRequest,
    load_week_km: &dyn Fn(u32) -> f64,
) -> RunWeekPlan {
    let profile = pp.profile();
    let goal = profile.goal_distance;
    let n_days = req.n_run_days.max(1);
    if goal == GoalDistance::C25k {
        return c25k_week_plan(profile, req, n_days);
    }

    let load_weeks = pp.run_load_weeks();
    let cycle_len = load_weeks + 1;
    let cy = cycle_pos(profile, anchors, req.week_index, load_weeks);
    let base = base_weekly_km(profile, anchors, req, &cy, cycle_len);

    let cap = running::weekly_increase_cap_frac(training_age_years(pp.training_age())).value;
    let ramp = |p: u32| {
        if req.hold_volume {
            base
        } else {
            base * (1.0 + cap * p as f64)
        }
    };
    let taper_cut = |k: u32, n: u32| running::taper_volume_cut(goal, k, n).map_or(0.5, |c| c.value);
    // A taper cuts from the last full load week as its sessions add up: the
    // running-015 90-min easy-run ceiling can leave that week short of its
    // ramp volume, and a cut from the ramp figure would then fall short of
    // running-037/038's 41–60 %.
    let top_load_km = cy.taper.map_or(0.0, |(k, _)| load_week_km(k));
    let weekly_km = if let Some((k, n)) = cy.taper {
        top_load_km * (1.0 - taper_cut(k, n))
    } else if cy.recovery {
        base * (1.0 - recovery_week_cut())
    } else {
        ramp(cy.pos)
    };

    // The race week has no long run and only the days before the race.
    let race_week = cy.race_day.is_some();
    let n = match cy.race_day {
        Some(rd) => n_days.min(rd as usize),
        None => n_days,
    };
    // The race week's volume covers only the run days left before the race,
    // and no easy day outgrows the previous taper week's daily average
    // (running-037/038: the taper drops volume and adds no new stimulus).
    let (weekly_km, race_week_easy_cap_km) = match cy.taper {
        Some((k, tn)) if race_week => {
            let previous_km = if k > 1 {
                top_load_km * (1.0 - taper_cut(k - 1, tn))
            } else {
                top_load_km
            };
            (
                weekly_km * n as f64 / n_days as f64,
                Some(previous_km / n_days as f64),
            )
        }
        _ => (weekly_km, None),
    };
    // A taper keeps its quality sessions (running-037/038: intensity and
    // frequency held, only volume drops), so support is judged on the last
    // full load week; the sessions themselves shrink to their minimum.
    let quality_basis_km = if cy.taper.is_some() {
        top_load_km
    } else {
        weekly_km
    };
    let kinds = week_kinds(profile, req, &cy, n, quality_basis_km, anchors.run_vdot);

    // Strides from a few weeks into base (running-020: "introduced a few
    // weeks into base"; the third base week), never on a recovery week.
    let strides = !cy.recovery && (cy.phase != MesoPhase::Base || req.week_index >= 2);

    RunWeekPlan {
        phase: cy.phase,
        recovery: cy.recovery,
        cycle_len,
        weekly_km,
        kinds,
        race_day: cy.race_day,
        taper_week: cy.taper,
        programme: RunProgramme::Distance {
            strides,
            race_week_easy_cap_km,
        },
    }
}

/// NHS Couch to 5K (running-025): three run/walk sessions a week; the
/// scheduler's completion-gated block index is the programme week, so a week
/// with too few runs done repeats (RUN-C25K-002, ADH-PROGRESS-001).
fn c25k_week_plan(profile: &Profile, req: &RunWeekRequest, n_days: usize) -> RunWeekPlan {
    let c25k = running::c25k_plan().value;
    let cy = CyclePos {
        phase: MesoPhase::Base,
        recovery: false,
        pos: 0,
        cycle_offset: 0,
        taper: None,
        race_day: None,
    };
    let n = n_days.min(usize::from(c25k.runs_per_week));
    RunWeekPlan {
        phase: cy.phase,
        recovery: cy.recovery,
        cycle_len: u32::from(c25k.weeks.0),
        weekly_km: 0.0,
        kinds: week_kinds(profile, req, &cy, n, 0.0, None),
        race_day: cy.race_day,
        taper_week: cy.taper,
        programme: RunProgramme::C25k {
            week: req.week_index.saturating_add(1).min(u32::from(u8::MAX)) as u8,
        },
    }
}

/// Base weekly volume: the stated/measured floor, scaled for cut run days,
/// carried up to the heaviest week the athlete actually ran in the previous
/// cycle (RUN-VOLUME-001, running-033: progression resumes from the last load
/// level reached) and capped at the goal plateau. A week holding runs from
/// before a break is a pre-break load: the re-entry restarts below it
/// (RUN-REENTRY-001).
fn base_weekly_km(
    profile: &Profile,
    anchors: &Anchors,
    req: &RunWeekRequest,
    cy: &CyclePos,
    cycle_len: u32,
) -> f64 {
    let effective = effective_weekly_km(profile, anchors) * req.volume_scale;
    let pre_break = |back: usize| match (anchors.run_break, anchors.week_start_epoch_day) {
        (Some(b), Some(monday)) => monday - 7 * (back as i64 + 1) <= b.last_run_epoch_day,
        _ => false,
    };
    let carry = (!req.hold_volume && req.week_index >= cycle_len)
        .then(|| {
            let start = cy.cycle_offset as usize;
            let end = (start + cycle_len as usize).min(anchors.run_week_km.len());
            anchors.run_week_km.get(start..end).map(|w| {
                (start..)
                    .zip(w)
                    .filter(|(back, _)| !pre_break(*back))
                    .map(|(_, km)| *km)
                    .fold(0.0_f64, f64::max)
            })
        })
        .flatten()
        .map(|c| match goal_volume_plateau_km(profile.goal_distance) {
            Some(p) => c.min(p.max(effective)),
            None => c,
        });
    effective.max(carry.unwrap_or(0.0))
}

/// The week's session kinds: long run (none in the race week, none when the
/// goal table gives it no share: running-025's C25K) + the goal/phase quality
/// mix, each kind kept only when `quality_basis_km` supports it, + easy fill
/// up to `n` run days.
fn week_kinds(
    profile: &Profile,
    req: &RunWeekRequest,
    cy: &CyclePos,
    n: usize,
    quality_basis_km: f64,
    vdot: Option<f64>,
) -> Vec<RunSessionType> {
    let race_week = cy.race_day.is_some();
    let gwp = running::goal_week_plan(profile.goal_distance, profile.advanced).value;
    let reserve = if race_week { 1 } else { 2 };
    let mut q = (gwp.quality_per_week.1 as usize).min(n.saturating_sub(reserve));
    if let Some(m) = req.max_quality {
        q = q.min(m);
    }
    if race_week {
        q = q.min(1);
    }
    if cy.recovery {
        q = 0;
    }
    let mix = running::quality_mix(profile.goal_distance, cy.phase).value;
    let mut kinds: Vec<RunSessionType> = Vec::with_capacity(n);
    if !race_week && n > 0 && gwp.long_run_share.1 > 0.0 {
        kinds.push(RunSessionType::LongRun);
    }
    for &k in mix.iter().take(q) {
        let supported = quality_rule(k).is_none_or(|rule| (rule.supported)(quality_basis_km, vdot));
        kinds.push(if supported { k } else { RunSessionType::Easy });
    }
    while kinds.len() < n {
        kinds.push(RunSessionType::Easy);
    }
    kinds
}

/// Recovery-week volume cut: the midpoint of the KB depth band
/// (RUN-DELOAD-DEPTH-001, running-033: 20–40 % → 30 %), read from the same
/// rule the guidance card shows so the plan and the card never disagree.
fn recovery_week_cut() -> f64 {
    let (lo, hi) = running::recovery_week_rx(running::MileageBand::Unspecified)
        .value
        .volume_reduction_frac;
    (lo.unwrap_or(0.0) + hi) / 2.0
}

/// Everything a single day's prescription needs beyond its own kind.
pub(super) struct DayCtx {
    pub(super) weekly_km: f64,
    /// Every run's RUN-SPIKE-001 baseline: the longest run of the trailing
    /// window (or the stated one, `baseline_stated`); `detrained` when it is a
    /// DETRAIN-001-tapered value.
    pub(super) baseline: Option<f64>,
    pub(super) baseline_stated: bool,
    pub(super) detrained: bool,
    /// Demonstrated long-run capacity that may raise the long run above its
    /// share: the spike baseline, except through a RUN-REENTRY-001 rebuild,
    /// when only runs since the break count.
    pub(super) capacity: Option<f64>,
    pub(super) capacity_stated: bool,
    pub(super) capacity_detrained: bool,
    pub(super) n_run_days: usize,
    pub(super) recovery: bool,
    pub(super) taper: bool,
    /// A taper week whose sessions outgrew its cut: every quality session at
    /// its floor (see [`taper_steps`]).
    pub(super) quality_floor: bool,
    pub(super) goal: GoalDistance,
    pub(super) vdot: Option<f64>,
    /// Easy pace (s/km) warm-up/cool-down/recovery minutes are counted at.
    pub(super) easy_pace: f64,
    /// The week's long-run decision `(km, binding rule)`, set by
    /// [`size_week`] when the week has a long run.
    pub(super) long: Option<(f64, LongRunBind)>,
}

impl DayCtx {
    /// The long run's distance when the week has a positive one: no
    /// easy-paced day runs longer.
    fn long_km(&self) -> Option<f64> {
        self.long.map(|(km, _)| km).filter(|km| *km > 0.0)
    }

    /// The RUN-SPIKE-001 ceiling over a positive sub-1 km baseline leaves no
    /// honest distance for any run.
    fn no_distance(&self) -> bool {
        cap_km_to_spike(MIN_DISTANCE_KM, self.baseline).is_none()
    }

    /// The quality slot `rule` sizes, as this day runs it: the session the
    /// week's km sizes; in a taper never below its floor (`quality_floor`:
    /// the floor itself); a block held under the RUN-SPIKE-001 ceiling.
    fn quality_day(&self, rule: &QualityRule) -> QualityDay {
        if self.no_distance() {
            return QualityDay::NoDistance;
        }
        let sized = (rule.sized)(self.weekly_km, self.vdot);
        let set = if self.taper {
            let floor = (rule.floor)(self.vdot);
            Some(match sized {
                Some(s) if !self.quality_floor && s.km() >= floor.km() => s,
                _ => floor,
            })
        } else {
            sized
        };
        match set {
            Some(QualitySet::Block(km)) => cap_km_to_spike(km, self.baseline)
                .map_or(QualityDay::NoDistance, |km| {
                    QualityDay::Session(QualitySet::Block(km))
                }),
            Some(set) => QualityDay::Session(set),
            None => QualityDay::Unsized,
        }
    }
}

/// A quality slot as its day runs it. Without a session the day is an easy
/// run at the Recovery band's duration floor ([`easy_fallback`]).
#[derive(Debug, Clone, Copy, PartialEq)]
enum QualityDay {
    /// The session, warm-up and cool-down around it (RUN-WARMUP-001).
    Session(QualitySet),
    /// No size fits the week's km: the kind's rule cites the easy run, and
    /// the session's warm-up and cool-down stay counted in the week.
    Unsized,
    /// The RUN-SPIKE-001 ceiling leaves no honest distance: that rule cites
    /// the easy run, and the week counts none of it.
    NoDistance,
}

/// Easy pace (s/km) for weekly accounting: the middle of the athlete's VDOT
/// E band when a race anchors it, else the RUN-WARMUP-001 accounting default.
fn accounting_easy_pace(vdot: Option<f64>) -> f64 {
    vdot.and_then(|v| crate::running::vdot_band_pace_sec_per_km(v, VdotBand::Easy))
        .map(|(fast, slow)| (fast + slow) / 2.0)
        .unwrap_or(running::warmup_rx().value.accounting_easy_pace_sec_per_km)
}

/// Prescribe the scheduler's placement of this week's kinds: `placed` is
/// `(day, kind)` in any order. The race day and anything after it are dropped.
pub(super) fn run_week_sessions(
    plan: &RunWeekPlan,
    placed: &[(u16, RunSessionType)],
    profile: &Profile,
    anchors: &Anchors,
) -> Vec<Session> {
    let mut days: Vec<(u16, RunSessionType)> = placed
        .iter()
        .copied()
        .filter(|(d, _)| plan.race_day.is_none_or(|rd| *d < rd))
        .collect();
    days.sort_by_key(|(d, _)| *d);
    match plan.programme {
        RunProgramme::C25k { week } => c25k_sessions(week, &days),
        RunProgramme::Distance {
            strides,
            race_week_easy_cap_km,
        } => {
            let week = DistanceWeek {
                plan,
                race_week_easy_cap_km,
                profile,
                anchors,
            };
            distance_sessions(&week, &days, strides)
        }
    }
}

/// The C25K week from the NHS table. With fewer days than the programme's
/// runs, the week keeps its LAST runs: weeks 5–6 end on the bridge to
/// continuous running.
fn c25k_sessions(week: u8, days: &[(u16, RunSessionType)]) -> Vec<Session> {
    let runs = running::c25k_plan().value.runs_per_week as usize;
    let skipped = runs.saturating_sub(days.len());
    days.iter()
        .enumerate()
        .map(|(i, &(day, _))| {
            c25k_session(week, (i + 1 + skipped).min(u8::MAX as usize) as u8, day)
        })
        .collect()
}

/// A distance plan's week and the athlete facts its days are sized from.
struct DistanceWeek<'a> {
    plan: &'a RunWeekPlan,
    race_week_easy_cap_km: Option<f64>,
    profile: &'a Profile,
    anchors: &'a Anchors,
}

/// One run day sized: its prescription, the rule that sized it and the claim
/// it cites.
struct DayRun {
    day: u16,
    kind: RunSessionType,
    run: RunPrescription,
    basis: RxBasis,
    claim: ClaimId,
}

/// A distance week's sessions: an easy day directly after a hard day or the
/// long run becomes a Recovery run (running-014 vs running-015); easy days
/// share what's left of the weekly km after the long run, the quality work and
/// its warm-up/cool-down, up to the running-015 90-min easy-run ceiling
/// ([`size_week`]); strides go on the first easy days. A taper week runs the
/// first of its [`taper_steps`] whose sessions fit its cut.
fn distance_sessions(
    week: &DistanceWeek,
    days: &[(u16, RunSessionType)],
    strides: bool,
) -> Vec<Session> {
    let steps = match week.plan.taper_week {
        Some(_) => taper_steps(days, week.profile.goal_distance),
        None => vec![(days.to_vec(), false)],
    };
    let pace = accounting_easy_pace(week.anchors.run_vdot);
    let last = steps.len() - 1;
    let mut runs = Vec::new();
    for (i, (days, quality_floor)) in steps.into_iter().enumerate() {
        runs = size_days(week, &days, quality_floor);
        let km: f64 = runs.iter().map(|r| run_km(&r.run, pace)).sum();
        if i == last || km <= week.plan.weekly_km {
            break;
        }
    }

    // Strides on the first one or two easy days (running-020: 1–3×/week).
    let st = running::strides_rx().value;
    let strides_days: BTreeSet<u16> = if strides {
        runs.iter()
            .filter(|r| r.kind == RunSessionType::Easy)
            .map(|r| r.day)
            .take(2.min(st.per_week.1 as usize))
            .collect()
    } else {
        BTreeSet::new()
    };
    runs.into_iter()
        .map(
            |DayRun {
                 day,
                 kind,
                 mut run,
                 basis,
                 claim,
             }| {
                if strides_days.contains(&day) {
                    run.strides = Some(StridesAddOn {
                        reps: ((u16::from(st.reps.0) + u16::from(st.reps.1)) / 2) as u8,
                        rep_sec: (st.rep_sec.0 + st.rep_sec.1) / 2 / 5 * 5,
                        recovery_sec: (st.recovery_sec.0 + st.recovery_sec.1) / 2 / 5 * 5,
                    });
                }
                Session {
                    session_type: SessionType::Run(kind),
                    day,
                    prescriptions: vec![basis.rx(Prescription::Run(run), claim)],
                    dup_day: None,
                }
            },
        )
        .collect()
}

/// What a taper week over its cut (the session floors bind at low volume)
/// sheds, in order, every run day kept (running-037/038 hold frequency,
/// 04-running.md:496, :501). Each step is the week's `(days, every quality
/// session at its floor)`:
///  0. the week as planned;
///  1. a race-pace session becomes an easy run unless the goal's taper keeps
///     MP touches: only the marathon's does (04-running.md:500); 5K/10K keep
///     short sharp I/T sessions (:498), the half holds intensity (:499);
///  2. every quality session drops to its floor (only volume drops, :501).
fn taper_steps(
    days: &[(u16, RunSessionType)],
    goal: GoalDistance,
) -> Vec<(Vec<(u16, RunSessionType)>, bool)> {
    let mut steps = vec![(days.to_vec(), false)];
    let mut days = days.to_vec();
    let keeps_mp =
        running::distance_taper(goal).is_some_and(|t| t.value.keep_mp_touches_and_short_tempo);
    if !keeps_mp && days.iter().any(|(_, k)| *k == RunSessionType::RacePace) {
        for (_, k) in &mut days {
            if *k == RunSessionType::RacePace {
                *k = RunSessionType::Easy;
            }
        }
        steps.push((days.clone(), false));
    }
    steps.push((days, true));
    steps
}

/// One run per day of `days` (sorted, race day dropped), recovery days
/// marked and the week sized once ([`size_week`]).
fn size_days(
    week: &DistanceWeek,
    days: &[(u16, RunSessionType)],
    quality_floor: bool,
) -> Vec<DayRun> {
    let mut days = days.to_vec();
    mark_recovery_days(&mut days);
    let SizedWeek { ctx, soft } = size_week(week, &days, quality_floor);
    days.iter()
        .map(|&(day, kind)| {
            let (run, basis, claim) = match quality_rule(kind) {
                Some(rule) => quality_run(kind, &rule, &ctx),
                None if kind == RunSessionType::LongRun => long_run(&ctx),
                None => {
                    let soft_km = match kind {
                        RunSessionType::Easy => soft.easy_km,
                        RunSessionType::Recovery => soft.rec_km,
                        _ => 0.0,
                    };
                    soft_run(kind, &ctx, soft_km)
                }
            };
            DayRun {
                day,
                kind,
                run,
                basis,
                claim,
            }
        })
        .collect()
}

/// Run km of one run as the plan counts a week: its main set (reps with their
/// jog-distance recoveries, minutes at `easy_pace`) plus its warm-up and
/// cool-down minutes at `easy_pace`.
fn run_km(r: &RunPrescription, easy_pace: f64) -> f64 {
    let km_of_min = |min: f64| min * 60.0 / easy_pace.max(1.0);
    let km = |v: RunVolume| match v {
        RunVolume::DistanceKm(k) => f64::from(k),
        RunVolume::DurationMin(m) => km_of_min(f64::from(m)),
        RunVolume::RunWalk { .. } => 0.0,
    };
    let main = match r.repeats {
        Some((n, rep)) => {
            let jog = match r.recovery {
                Some(RepRecovery::Metres(m)) => f64::from(m) / 1000.0,
                _ => 0.0,
            };
            f64::from(n) * (km(rep) + jog)
        }
        None => km(r.volume),
    };
    let wu = r.warmup_min.unwrap_or(0) + r.cooldown_min.unwrap_or(0);
    main + km_of_min(f64::from(wu))
}

/// The run km a synthesized week prescribes, as the plan sizes a week: each
/// run's main set, its jog-distance recoveries and its warm-up and cool-down,
/// minutes counted at the athlete's accounting easy pace (`anchors`, the
/// anchors the week was synthesized with).
pub fn prescribed_run_km(sessions: &[Session], anchors: &Anchors) -> f64 {
    let easy_pace = accounting_easy_pace(anchors.run_vdot);
    sessions
        .iter()
        .filter(|s| matches!(s.session_type, SessionType::Run(_)))
        .flat_map(|s| &s.prescriptions)
        .filter_map(|rx| match &rx.value {
            Prescription::Run(r) => Some(run_km(r, easy_pace)),
            _ => None,
        })
        .sum()
}

/// An easy day right after a hard day or the long run is a recovery run
/// (running-014 vs running-015). `days` is sorted by day.
fn mark_recovery_days(days: &mut [(u16, RunSessionType)]) {
    let hard: BTreeSet<u16> = days
        .iter()
        .filter(|(_, k)| running::is_hard_or_long_session(*k))
        .map(|(d, _)| *d)
        .collect();
    for (d, k) in days.iter_mut() {
        if *k == RunSessionType::Easy && *d > 0 && hard.contains(&(*d - 1)) {
            *k = RunSessionType::Recovery;
        }
    }
}

/// A placed week sized: its day context (`weekly_km` = the week re-sized to
/// the easy-run ceiling, `long` = the long-run decision) and its soft days.
struct SizedWeek {
    ctx: DayCtx,
    soft: SoftDays,
}

/// Size the placed `days` (recovery days marked) of a distance week.
/// running-015: an easy run lasts at most 90 min, so a week whose easy days
/// would run longer holds less than planned: the week shrinks by what the
/// ceiling cut, and its long run, quality sessions and soft days are sized
/// again, so the long-run share (RUN-LONGRUN-001) and the T/I/R/M caps are
/// judged on the week the athlete runs (the week's fixed point). Every pass
/// shrinks the week by at least 0.05 km, so the loop ends.
fn size_week(
    week: &DistanceWeek,
    days: &[(u16, RunSessionType)],
    quality_floor: bool,
) -> SizedWeek {
    let mut ctx = day_ctx(week.plan, week.profile, week.anchors, days.len());
    ctx.quality_floor = quality_floor;
    let has_long = days.iter().any(|(_, k)| *k == RunSessionType::LongRun);
    loop {
        ctx.long = has_long.then(|| long_run_decision(&ctx));
        let soft = soft_day_km(week.race_week_easy_cap_km, &ctx, days);
        if soft.over_easy_ceiling_km < 0.05 {
            return SizedWeek { ctx, soft };
        }
        ctx.weekly_km -= soft.over_easy_ceiling_km;
    }
}

/// The day context of a placed week. After a break (RUN-REENTRY-001): 2+
/// weeks off, the long run is built up again (only runs since the break count
/// as demonstrated long-run capacity); 4+ weeks off, every run's spike
/// ceiling rests only on runs since the break. Otherwise a run of the trailing
/// window (RUN-SPIKE-001, DETRAIN-001 taper) counts.
fn day_ctx(plan: &RunWeekPlan, profile: &Profile, anchors: &Anchors, n_run_days: usize) -> DayCtx {
    let brk = anchors.run_break;
    let all_runs = (
        anchors.run.longest_recent_run_km,
        anchors.run.longest_run_detrained,
    );
    let since_break = |b: RunBreak| (b.longest_since_km, b.longest_since_detrained);
    let capacity_runs = brk
        .filter(|b| b.reentry.rebuild_long_run)
        .map_or(all_runs, since_break);
    let spike_runs = brk
        .filter(|b| b.reentry.near_novice_spike)
        .map_or(all_runs, since_break);
    let stated = profile
        .stated_longest_recent_km
        .filter(|k| k.is_finite() && *k > 0.0);
    // The stated longest run is a run of the last 30 days only while it is
    // inside that window; once runs were logged, they replace it (RUN-START-001).
    // Through a long-run rebuild only an answer given after the break began
    // is current: one from before it is no current capacity.
    let stated_current = brk.is_none_or(|b| !b.reentry.rebuild_long_run || b.stated_longest_after);
    // (baseline, stated, detrained) from a logged (km, detrained) source.
    let pick = |(logged, detrained): (Option<f64>, bool)| {
        let logged = logged.filter(|k| *k > 0.0);
        let stated_wins = stated_current
            && match (logged, stated) {
                (None, Some(_)) => anchors.stated_longest_in_window || brk.is_none(),
                (Some(l), Some(s)) => anchors.stated_longest_in_window && s > l,
                _ => false,
            };
        if stated_wins {
            (stated, true, false)
        } else {
            (logged, false, detrained && logged.is_some())
        }
    };
    let (baseline, baseline_stated, detrained) = pick(spike_runs);
    let (capacity, capacity_stated, capacity_detrained) = pick(capacity_runs);
    DayCtx {
        weekly_km: plan.weekly_km,
        baseline,
        baseline_stated,
        detrained,
        capacity,
        capacity_stated,
        capacity_detrained,
        n_run_days,
        recovery: plan.recovery,
        taper: plan.taper_week.is_some(),
        quality_floor: false,
        goal: profile.goal_distance,
        vdot: anchors.run_vdot,
        easy_pace: accounting_easy_pace(anchors.run_vdot),
        long: None,
    }
}

/// The easy and recovery days' sizes (see [`soft_day_km`]).
struct SoftDays {
    easy_km: f64,
    rec_km: f64,
    /// Week km the easy days could not hold under the running-015 90-min
    /// easy-run ceiling (summed over the easy days).
    over_easy_ceiling_km: f64,
}

/// The easy and recovery days' share of what's left of the week after the
/// committed volume (long run + each quality session's main set and its
/// warm-up/cool-down, RUN-WARMUP-001), each within its duration band.
fn soft_day_km(
    race_week_easy_cap_km: Option<f64>,
    ctx: &DayCtx,
    days: &[(u16, RunSessionType)],
) -> SoftDays {
    let km_of_min = |min: f64| min * 60.0 / ctx.easy_pace.max(1.0);
    let mut committed = ctx.long_km().unwrap_or(0.0);
    let wu = running::warmup_rx().value;
    let wu_km = km_of_min((wu.warmup_min.0 + wu.cooldown_min) as f64);
    for (_, k) in days {
        if let Some(rule) = quality_rule(*k) {
            committed += match ctx.quality_day(&rule) {
                QualityDay::Session(set) => set.km() + wu_km,
                QualityDay::Unsized => wu_km,
                QualityDay::NoDistance => 0.0,
            };
        }
    }
    let n_easy = days
        .iter()
        .filter(|(_, k)| *k == RunSessionType::Easy)
        .count();
    let n_rec = days
        .iter()
        .filter(|(_, k)| *k == RunSessionType::Recovery)
        .count();
    let resid = (ctx.weekly_km - committed).max(0.0);
    let per_soft = resid / (n_easy + n_rec).max(1) as f64;
    // A recovery run stays inside its 20–40 min band (running-014), an easy
    // run inside its 30–90 min band (running-015); the rest of the residual
    // moves to the easy days.
    let ceiling_km = |kind: RunSessionType| {
        running::workout_rx(kind)
            .value
            .duration_min
            .1
            .map_or(f64::INFINITY, |m| km_of_min(f64::from(m)))
    };
    let rec_km = floor_km(
        per_soft
            .min(ceiling_km(RunSessionType::Recovery))
            .min(race_week_easy_cap_km.unwrap_or(f64::INFINITY)),
    );
    let rec_counted = rec_km * n_rec as f64;
    let mut easy_km = if n_easy > 0 {
        (resid - rec_counted).max(0.0) / n_easy as f64
    } else {
        0.0
    };
    if let Some(l) = ctx.long_km() {
        easy_km = easy_km.min(l);
    }
    if let Some(c) = race_week_easy_cap_km {
        easy_km = easy_km.min(c);
    }
    // The weekly report judges every run against the EXECUTED week's daily
    // average (RUN-LONGRUN-001).
    if let Some(cap) =
        running::easy_run_cap_for_daily_avg(committed + rec_counted, n_easy, ctx.n_run_days)
    {
        easy_km = easy_km.min(cap);
    }
    let easy_ceiling = ceiling_km(RunSessionType::Easy);
    let held_easy_km = floor_km(easy_km.min(easy_ceiling));
    SoftDays {
        easy_km: held_easy_km,
        rec_km,
        over_easy_ceiling_km: n_easy as f64 * (easy_km - easy_ceiling).max(0.0),
    }
}

/// One C25K run (`run_of_week`-th of the week) from the NHS table.
fn c25k_session(week: u8, run_of_week: u8, day: u16) -> Session {
    let rx = running::c25k_session_rx(week, run_of_week);
    let recovery_rx = running::workout_rx(RunSessionType::Recovery);
    let mut pres = RunPrescription::new(
        rx.value.volume(),
        conservative_intensity(RunSessionType::Recovery, &recovery_rx.value),
        None,
    );
    pres.warmup_min = Some(rx.value.warmup_walk_min);
    pres.segments = rx.value.bouts.clone();
    Session {
        session_type: SessionType::Run(RunSessionType::Easy),
        day,
        prescriptions: vec![RxBasis::Workout.rx(Prescription::Run(pres), rx.claim_id())],
        dup_day: None,
    }
}

/// A quality slot's prescription, basis and claim: the session with its
/// warm-up and cool-down, or an easy run when it has none ([`QualityDay`]).
fn quality_run(
    kind: RunSessionType,
    rule: &QualityRule,
    ctx: &DayCtx,
) -> (RunPrescription, RxBasis, ClaimId) {
    let (run, basis, claim) = match ctx.quality_day(rule) {
        QualityDay::Session(set) => (
            with_warmup(quality_prescription(kind, set)),
            RxBasis::Workout,
            rule.claim,
        ),
        QualityDay::Unsized => (easy_fallback(ctx), RxBasis::Workout, rule.claim),
        QualityDay::NoDistance => (
            easy_fallback(ctx),
            RxBasis::SpikeCeiling,
            claims::RUN_SPIKE_001,
        ),
    };
    // Every run in a recovery week is a reduced-volume, reduced-intensity
    // session: cite the recovery-week depth rule.
    if ctx.recovery {
        (run, RxBasis::Workout, claims::RUN_DELOAD_DEPTH_001)
    } else {
        (run, basis, claim)
    }
}

/// A quality session's main set as a run: a block as a DISTANCE in the kind's
/// HR band, reps as per-rep structure at the kind's VDOT band with their
/// recovery.
fn quality_prescription(kind: RunSessionType, set: QualitySet) -> RunPrescription {
    match set {
        QualitySet::Block(km) => RunPrescription::new(
            RunVolume::DistanceKm(km as f32),
            conservative_intensity(kind, &running::workout_rx(kind).value),
            None,
        ),
        QualitySet::Reps {
            reps,
            rep_m,
            recovery,
        } => {
            let rep_km = rep_m as f32 / 1000.0;
            let mut r = RunPrescription::new(
                RunVolume::DistanceKm(rep_km * reps as f32),
                RunIntensity::Vdot(vdot_band_for(kind)),
                Some((reps, RunVolume::DistanceKm(rep_km))),
            );
            r.recovery = Some(recovery);
            r
        }
    }
}

/// The long run: the week's long-run decision as an easy-paced run (below
/// 1 km, the minutes that distance takes), citing whichever rule BINDS its
/// distance. In a recovery week it keeps its slot at the easy (Recovery)
/// intensity band and shrinks with the week (running-033 reduces both volume
/// AND intensity; see `long_run_decision`).
fn long_run(ctx: &DayCtx) -> (RunPrescription, RxBasis, ClaimId) {
    let rx = running::workout_rx(if ctx.recovery {
        RunSessionType::Recovery
    } else {
        RunSessionType::LongRun
    });
    let easy = running::workout_rx(RunSessionType::Easy).value;
    let run = RunPrescription::new(
        soft_volume(&easy, ctx, ctx.long.map_or(0.0, |(km, _)| km)),
        conservative_intensity(RunSessionType::LongRun, &rx.value),
        None,
    );
    let bind = match ctx.long {
        _ if ctx.recovery => return (run, RxBasis::Workout, claims::RUN_DELOAD_DEPTH_001),
        Some((_, bind)) => bind,
        None => return (run, RxBasis::Workout, rx.claim_id()),
    };
    let (stated, detrained) = match bind {
        LongRunBind::Demonstrated | LongRunBind::DailyAvgCap => {
            (ctx.capacity_stated, ctx.capacity_detrained)
        }
        LongRunBind::SpikeCeiling => (ctx.baseline_stated, ctx.detrained),
        _ => (false, false),
    };
    let (basis, claim) = if detrained {
        (RxBasis::Detrained, claims::DETRAIN_001)
    } else if bind.cites_spike() && stated {
        (RxBasis::StatedCeiling, claims::RUN_START_001)
    } else if bind.cites_spike() {
        (RxBasis::SpikeCeiling, claims::RUN_SPIKE_001)
    } else if bind == LongRunBind::Target {
        (RxBasis::LongRunTarget, claims::RUN_LONGRUN_TARGET_001)
    } else if ctx.taper {
        (RxBasis::Taper, claims::RUN_TAPER_001)
    } else if bind == LongRunBind::AverageRun {
        (RxBasis::AverageRun, claims::RUN_LONGRUN_001)
    } else {
        (RxBasis::Workout, rx.claim_id())
    };
    (run, basis, claim)
}

/// An easy or recovery day: its sized share of the week (`soft_km`; 0 →
/// nothing to size from, see [`soft_volume`]).
fn soft_run(
    kind: RunSessionType,
    ctx: &DayCtx,
    soft_km: f64,
) -> (RunPrescription, RxBasis, ClaimId) {
    let rx = running::workout_rx(kind);
    let run = RunPrescription::new(
        soft_volume(&rx.value, ctx, soft_km),
        conservative_intensity(kind, &rx.value),
        None,
    );
    if ctx.recovery {
        (run, RxBasis::Workout, claims::RUN_DELOAD_DEPTH_001)
    } else if ctx.taper && matches!(kind, RunSessionType::Easy | RunSessionType::Recovery) {
        // Taper: the easy volume is what the taper cut (intensity held).
        (run, RxBasis::Taper, claims::RUN_TAPER_001)
    } else {
        (run, RxBasis::Workout, rx.claim_id())
    }
}

/// Conservative intensity: for an HR-governed session carry the full %HRmax
/// band (and the Karvonen %HRR ceiling when the KB states one); otherwise
/// (pace-governed) the session's VDOT band.
fn conservative_intensity(kind: RunSessionType, rx: &RunWorkoutRx) -> RunIntensity {
    if rx.hr_governed {
        let (lo, hi) = rx.pct_hr_max;
        if lo.is_some() || hi.is_some() {
            return RunIntensity::HrPercentMax {
                lo: lo.map(|f| (f * 100.0) as f32),
                hi: hi.map(|f| (f * 100.0) as f32),
                hrr_hi: rx.pct_hrr_max.map(|f| (f * 100.0) as f32),
            };
        }
    }
    RunIntensity::Vdot(vdot_band_for(kind))
}

/// Warm-up + cool-down around a hard session (RUN-WARMUP-001): the band floor.
fn with_warmup(mut r: RunPrescription) -> RunPrescription {
    let wu = running::warmup_rx().value;
    r.warmup_min = Some(wu.warmup_min.0);
    r.cooldown_min = Some(wu.cooldown_min);
    r
}

/// A quality slot without a session: a conservative easy run at the Recovery
/// band, the band's duration floor held under the spike ceiling and the long
/// run.
fn easy_fallback(ctx: &DayCtx) -> RunPrescription {
    let recovery_rx = running::workout_rx(RunSessionType::Recovery);
    RunPrescription::new(
        soft_volume(&recovery_rx.value, ctx, 0.0),
        conservative_intensity(RunSessionType::Recovery, &recovery_rx.value),
        None,
    )
}

/// An easy-paced day's volume (easy, recovery, the long run, a degraded
/// quality slot), never above the RUN-SPIKE-001 ceiling or the week's long run.
/// `km` is the day's sized share of the week: a distance from 1 km, below that
/// the minutes it takes at the athlete's easy pace. With nothing sized
/// (`km` = 0: no stated or measured weekly volume, or nothing left after the
/// committed work) the band's duration floor (running-014 Recovery 20 min,
/// running-015 Easy 30 min), still held under both ceilings.
fn soft_volume(rx: &RunWorkoutRx, ctx: &DayCtx, km: f64) -> RunVolume {
    let limit = [
        ctx.baseline.filter(|b| *b > 0.0).map(spike_ceiling_km),
        ctx.long_km(),
    ]
    .into_iter()
    .flatten()
    .reduce(f64::min);
    let minutes = |km: f64| {
        (km * ctx.easy_pace / 60.0)
            .floor()
            .clamp(1.0, u16::MAX as f64) as u16
    };
    if km > 0.0 {
        let km = limit.map_or(km, |l| km.min(l));
        let d = floor_km(km);
        if d >= MIN_DISTANCE_KM {
            RunVolume::DistanceKm(d as f32)
        } else {
            RunVolume::DurationMin(minutes(km))
        }
    } else {
        let floor_min = rx.duration_min.0.unwrap_or_default().max(1);
        RunVolume::DurationMin(limit.map_or(floor_min, |l| floor_min.min(minutes(l))))
    }
}

/// Which KB rule set the long-run distance decides the honest face citation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LongRunBind {
    /// The weekly share (30 % under 64 km/wk, else 25 %) governed. →
    /// RUN-LONGRUN-001.
    Volume,
    /// The share was shorter than the week's average run, so the long run is
    /// the average run (RUN-LONGRUN-001's low-mileage guardrail keeps it the
    /// week's longest, within 2× the daily average). → RUN-LONGRUN-001, with
    /// copy that states this rule instead of the share.
    AverageRun,
    /// The demonstrated-capacity anchor set the value, within safety bounds. It
    /// exceeds the share but is 0 % over the 30-day longest run. → RUN-SPIKE-001.
    Demonstrated,
    /// The ≤2×-daily-average guardrail (running-016) limited the run. →
    /// RUN-LONGRUN-001.
    DailyAvgCap,
    /// The goal long-run ceiling or the 150-min time cap limited it
    /// (running-046). → RUN-LONGRUN-TARGET-001.
    Target,
    /// The RUN-SPIKE-001 safety ceiling (≤10 % over the longest recent run)
    /// capped a higher want. → RUN-SPIKE-001.
    SpikeCeiling,
}

impl LongRunBind {
    /// Whether the binding rule is the single-session spike rule (RUN-SPIKE-001)
    /// rather than the share / daily-average / target guardrails.
    pub(super) fn cites_spike(self) -> bool {
        matches!(self, LongRunBind::Demonstrated | LongRunBind::SpikeCeiling)
    }
}

/// Decide the long-run distance (km, floored to the plan resolution) and which
/// rule binds it ([`LongRunBind`] cites each), applied in order:
///  - the weekly share, raised to the week's average run (`AverageRun`);
///  - the ≤2×-daily-average guardrail;
///  - demonstrated capacity (`ctx.capacity`) may raise it up to that
///    guardrail, except in a recovery week or a taper;
///  - the goal ceiling and, with a pace anchor, the 150-min cap at easy pace;
///  - the RUN-SPIKE-001 ceiling over `ctx.baseline`, a hard ceiling on all.
pub(super) fn long_run_decision(ctx: &DayCtx) -> (f64, LongRunBind) {
    let w = ctx.weekly_km.max(0.0);
    let share = running::long_run_share(w).value;
    let mut target = floor_km(share * w);
    let mut bind = LongRunBind::Volume;
    if ctx.n_run_days > 0 {
        let average_run = floor_km(daily_avg_run_km(w, ctx.n_run_days));
        if average_run > target {
            target = average_run;
            bind = LongRunBind::AverageRun;
        }
    }
    let daily_avg_cap = (ctx.n_run_days > 0 && w > 0.0)
        .then(|| floor_km(running::daily_avg_run_cap_km(w, ctx.n_run_days)));
    if let Some(cap) = daily_avg_cap
        && cap < target
    {
        target = cap;
        bind = LongRunBind::DailyAvgCap;
    }
    let baseline = ctx.baseline.filter(|b| *b > 0.0);
    // Demonstrated CAPACITY (a distance, ≥ MIN_DISTANCE_KM) may raise the run
    // above the share, bounded by the daily-average guardrail; never in a
    // recovery week or a taper, whose cut volume keeps the long run at its
    // share (running-033; running-037/038: only volume drops).
    if !ctx.recovery
        && !ctx.taper
        && let Some(b) = ctx.capacity.filter(|b| *b >= MIN_DISTANCE_KM)
    {
        let dem = floor_km(b);
        let raised = daily_avg_cap.map_or(dem, |c| dem.min(c));
        if raised > target {
            target = raised;
            bind = match daily_avg_cap {
                Some(c) if dem > c => LongRunBind::DailyAvgCap,
                _ => LongRunBind::Demonstrated,
            };
        }
    }
    let goal_cap = running::goal_long_run_target_km(ctx.goal).map(|t| t.value.1);
    let time_cap = ctx
        .vdot
        .and_then(|v| crate::running::vdot_band_pace_sec_per_km(v, VdotBand::Easy))
        .map(|(_, slow)| floor_km(running::LONG_RUN_MAX_MIN * 60.0 / slow.max(1.0)));
    for cap in [goal_cap, time_cap].into_iter().flatten() {
        if cap < target {
            target = cap;
            bind = LongRunBind::Target;
        }
    }
    if let Some(b) = baseline {
        let ceiling = spike_ceiling_km(b);
        if ceiling < target {
            target = ceiling;
            bind = LongRunBind::SpikeCeiling;
        }
    }
    (target.max(0.0), bind)
}

/// A session type's Daniels VDOT band: the pace-governed intensity and the
/// interference check's intensity (`plan_run_intensity`).
pub(super) fn vdot_band_for(kind: RunSessionType) -> VdotBand {
    match kind {
        RunSessionType::Recovery | RunSessionType::LongRun | RunSessionType::Easy => VdotBand::Easy,
        RunSessionType::Tempo => VdotBand::Threshold,
        RunSessionType::Interval | RunSessionType::Hills => VdotBand::Interval,
        RunSessionType::Repetition | RunSessionType::Strides => VdotBand::Repetition,
        RunSessionType::RacePace => VdotBand::Marathon,
    }
}
