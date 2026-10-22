//! Running week synthesis. `run_week_plan` decides the week: its phase (a
//! race-date Base → Build → Peak → Taper sequence, or a rolling Base/Build
//! cycle), whether it is a planned recovery week, its weekly volume (cycle
//! ramp, cross-cycle carry from what the athlete actually ran, taper cut) and
//! the session kinds (long run + the goal/phase quality mix + easy fill).
//! The scheduler in `plan/mod.rs` places those kinds on days, then
//! `run_week_sessions` prescribes each placed day: sized within the KB volume
//! caps, the long-run decision and the RUN-SPIKE-001 distance ceilings, with
//! warm-up/cool-down and rep/recovery structure the tracker can step through.

use std::collections::BTreeSet;

use crate::app::Profile;
use crate::evidence::claims;
use crate::individualization::training_age_from_cadence;
use crate::running::{self, GoalDistance, RunWorkoutRx};
use crate::schema::{
    MesoPhase, Prescription, RepRecovery, RunIntensity, RunPrescription, RunSessionType, RunVolume,
    Session, SessionType, StridesAddOn, VdotBand,
};

use super::Anchors;

/// Effective weekly running volume (km) for plan synthesis. Normally
/// `max(profile.running_km_per_week, anchors.recent_weekly_km)`: the stated value
/// is the athlete's own answer (0 when they left it unset, so measured volume,
/// else the KB session-duration floors, size the plan); the anchor is the
/// athlete's MEASURED 28-day average. Measured volume is
/// a fact, not a recommendation, so feeding it into the KB-cited long-run share
/// rule (RUN-LONGRUN-001, running-016) is legitimate: the higher of the two is
/// taken so a runner who logs more than their profile claims gets a plan sized to
/// reality, while a runner who logs less than their profile is still served
/// their intended volume (the profile is a floor).
///
/// Post-layoff decay exception: when the profile says the athlete is
/// RETURNING from a break (`weeks_off = Some(w > 0)`, the REENTRY-001 signal),
/// the stated figure is a stale PRE-layoff number and must NOT floor the
/// re-entry plan. In that case the MEASURED reality governs when it is known.
/// With no logged runs to measure yet, the stated figure still stands.
pub(super) fn effective_weekly_km(profile: &Profile, anchors: &Anchors) -> f64 {
    let stated = profile.running_km_per_week.max(0.0);
    let returning = profile.weeks_off.is_some_and(|w| w > 0.0);
    match anchors.recent_weekly_km {
        // Re-entry: measured mileage overrides a stale pre-layoff figure even
        // when it is LOWER (the profile stops being a floor after a break).
        Some(measured) if returning => measured,
        // Normal: the profile is a floor; measured wins only when it is higher.
        Some(measured) if measured > stated => measured,
        _ => stated,
    }
}

/// The number of run days the week schedules for this profile: the stated
/// running-days answer, bounded by the goal table's session ceiling
/// (`goal_week_plan`) and, only when strength/hypertrophy/power is CO-PRIMARY,
/// the ≤3 d/wk concurrency cap. The cap protects LIFTING adaptations from
/// endurance interference, so it does NOT apply to an endurance-priority
/// hybrid. Shared by the plan layout and the weekly report so the two agree on
/// the run-day denominator.
pub fn run_slot_count(profile: &Profile) -> usize {
    let gwp = running::goal_week_plan(profile.goal_distance, profile.advanced).value;
    let mut n = (profile.running_days_per_week as usize)
        .min(gwp.sessions_per_week.1 as usize)
        .max(1);
    // running-025 (safety-critical): Couch to 5K is three runs a week, and the
    // third run of weeks 5–6 is the bridge to continuous running, so the
    // programme never plans fewer.
    if profile.goal_distance == GoalDistance::C25k {
        n = n.max(running::c25k_plan().value.runs_per_week as usize);
    }
    if profile.weekly_sets > 0
        && matches!(
            profile.concurrent_goal,
            crate::hybrid::ConcurrentGoal::Strength
                | crate::hybrid::ConcurrentGoal::Power
                | crate::hybrid::ConcurrentGoal::Hypertrophy
        )
    {
        n = n
            .min(crate::hybrid::endurance_frequency_cap().value as usize)
            .max(1);
    }
    n
}

/// A hard (quality) run day: threshold, VO2max intervals, repetitions,
/// race-pace work or hill repeats. The long run is easy-paced (E→E+) and is
/// not counted here; the scheduler spaces both.
pub(crate) fn is_quality(kind: RunSessionType) -> bool {
    matches!(
        kind,
        RunSessionType::Tempo
            | RunSessionType::Interval
            | RunSessionType::Repetition
            | RunSessionType::RacePace
            | RunSessionType::Hills
    )
}

/// Average distance per RUN day — the KB reading of "daily average" for the
/// long-run ≤2×-daily-average guardrail (running-016): weekly volume over the
/// days actually run, not calendar days. The single source shared by the plan's
/// long-run decision and the weekly report so the two never disagree.
pub fn daily_avg_run_km(weekly_km: f64, n_run_days: usize) -> f64 {
    weekly_km / n_run_days.max(1) as f64
}

// ---------------------------------------------------------------------------
// Distance resolution
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Quality-session sizing within the KB weekly caps
// ---------------------------------------------------------------------------

/// Natural VO2max-interval rep count the weekly volume supports at a given rep
/// DISTANCE before any rep-floor clamp: `floor(<=8% weekly cap ÷ rep_m)`
/// (RUN-INTERVAL-001).
pub(super) fn interval_reps_natural_at(weekly_km: f64, rep_m: u16) -> i64 {
    let s = running::vo2max_interval_rx().value;
    (s.weekly_cap_frac * weekly_km * 1000.0 / rep_m.max(1) as f64).floor() as i64
}

/// The largest KB rep distance (running-019: 800-1600 m) whose 3-rep floor fits
/// within the <=8% weekly cap, trying 1200 m before dropping to 800 m. `None`
/// when even 3 x 800 m breaches the cap.
pub(super) fn supported_rep_distance_m(weekly_km: f64) -> Option<u16> {
    [1200u16, 800]
        .into_iter()
        .find(|&rep_m| interval_reps_natural_at(weekly_km, rep_m) >= 3)
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
    let floor_min = running::run_workout_rx(RunSessionType::Tempo)
        .value
        .duration_min
        .0
        .unwrap_or(20) as f64;
    vdot.and_then(|v| crate::load::vdot_band_pace_sec_per_km(v, VdotBand::Threshold))
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

/// Marathon-pace block size, km (running-017/053): the running-017 8 km
/// minimum block, growing with volume at 15 % of the week, never above the
/// 20 % weekly M-pace share, the 26 km band top, or Daniels' 110 min / 29 km
/// cap. `None` when 8 km would exceed 20 % of the week (the volume can't carry
/// a real M-pace block yet).
fn race_pace_km(weekly_km: f64, vdot: Option<f64>) -> Option<f64> {
    let band = running::run_workout_rx(RunSessionType::RacePace)
        .value
        .distance_km;
    let lo = band.0.unwrap_or(8.0);
    let hi = band.1.unwrap_or(26.0);
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
        vdot.and_then(|v| crate::load::vdot_band_pace_sec_per_km(v, VdotBand::Marathon))
    {
        // Slow end of the M band: the most time a block of `km` can take.
        let time_cap_km = floor_km(mp.max_min as f64 * 60.0 / pace.1.max(1.0));
        km = km.min(time_cap_km);
    }
    (km >= lo).then_some(km)
}

/// Whether a quality kind is supported by this week's volume; an unsupported
/// slot becomes an easy run rather than a cap-violating session.
fn quality_supported(kind: RunSessionType, weekly_km: f64, vdot: Option<f64>) -> bool {
    match kind {
        RunSessionType::Tempo => tempo_supported(weekly_km, vdot),
        RunSessionType::Interval => supported_rep_distance_m(weekly_km).is_some(),
        RunSessionType::Repetition => repetition_plan(weekly_km).is_some(),
        RunSessionType::RacePace => race_pace_km(weekly_km, vdot).is_some(),
        _ => true,
    }
}

// ---------------------------------------------------------------------------
// The week: phase, recovery, volume, session kinds
// ---------------------------------------------------------------------------

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
    /// C25K programme week (1-based), `None` for distance plans.
    pub(super) c25k_week: Option<u8>,
    /// Strides go on the week's first easy days (running-020, from a few weeks
    /// into base).
    pub(super) strides: bool,
    /// Race week only: no easy day runs longer than the previous taper week's
    /// daily average.
    pub(super) race_week_easy_cap_km: Option<f64>,
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
            Some(race - week_start)
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
        // k = whole weeks until the taper starts (0 = the last pre-taper week).
        let k = wtr - taper_n;
        let r = k % cycle_len;
        let (mut pos, mut recovery, offset) = if r < load_weeks {
            let p = load_weeks - 1 - r;
            (p, false, p)
        } else {
            (load_weeks, true, load_weeks)
        };
        // A plan never opens on a recovery week or above its own ramp.
        if week_index == 0 && recovery {
            recovery = false;
            pos = 0;
        }
        if !recovery {
            pos = pos.min(week_index);
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

/// Plan the run side of one week (see [`RunWeekPlan`]).
pub(super) fn run_week_plan(
    profile: &Profile,
    anchors: &Anchors,
    req: &RunWeekRequest,
) -> RunWeekPlan {
    let goal = profile.goal_distance;
    let n_days = req.n_run_days.max(1);
    if goal == GoalDistance::C25k {
        // NHS Couch to 5K: 3 run/walk sessions a week; the scheduler's
        // completion-gated block index is the programme week, so a week with
        // too few runs done repeats (RUN-C25K-002, ADH-PROGRESS-001).
        let weeks = running::c25k_plan().value.weeks.0 as u32;
        let runs = running::c25k_plan().value.runs_per_week as usize;
        return RunWeekPlan {
            phase: MesoPhase::Base,
            recovery: false,
            cycle_len: weeks,
            weekly_km: 0.0,
            kinds: vec![RunSessionType::Easy; n_days.min(runs)],
            race_day: None,
            taper_week: None,
            c25k_week: Some(req.week_index.saturating_add(1).min(u8::MAX as u32) as u8),
            strides: false,
            race_week_easy_cap_km: None,
        };
    }

    let ta = training_age_from_cadence(profile.progression_cadence).value;
    let load_weeks = running::deload_cadence(super::conservative_deload(profile.age_years, ta))
        .value
        .load_weeks
        .max(1) as u32;
    let cycle_len = load_weeks + 1;
    let cy = cycle_pos(profile, anchors, req.week_index, load_weeks);

    // Base volume: the stated/measured floor, scaled for cut run days, carried
    // up to the heaviest week the athlete actually ran in the previous cycle
    // (RUN-VOLUME-001, running-033: progression resumes from the last load
    // level reached, so a compliant athlete grows cycle to cycle and a
    // non-compliant one repeats). Capped at the goal plateau.
    let scale = if req.volume_scale.is_finite() {
        req.volume_scale.clamp(0.0, 1.0)
    } else {
        1.0
    };
    let effective = effective_weekly_km(profile, anchors) * scale;
    let carry = (!req.hold_volume && req.week_index >= cycle_len)
        .then(|| {
            let start = cy.cycle_offset as usize;
            let end = (start + cycle_len as usize).min(anchors.run_week_km.len());
            anchors
                .run_week_km
                .get(start..end)
                .map(|w| w.iter().copied().fold(0.0_f64, f64::max))
        })
        .flatten()
        .map(|c| match goal_volume_plateau_km(goal) {
            Some(p) => c.min(p.max(effective)),
            None => c,
        });
    let base = effective.max(carry.unwrap_or(0.0));

    let cap = running::weekly_increase_cap_frac(super::training_age_years(ta)).value;
    let ramp = |p: u32| {
        if req.hold_volume {
            base
        } else {
            base * (1.0 + cap * p as f64)
        }
    };
    let taper_cut = |k: u32, n: u32| running::taper_volume_cut(goal, k, n).map_or(0.5, |c| c.value);
    let top_load_km = ramp(load_weeks.saturating_sub(1));
    let weekly_km = if let Some((k, n)) = cy.taper {
        top_load_km * (1.0 - taper_cut(k, n))
    } else if cy.recovery {
        base * (1.0 - recovery_week_cut())
    } else {
        ramp(cy.pos)
    };

    // Session kinds: long run + the goal/phase quality mix + easy fill. The
    // race week has no long run and only the days before the race.
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
    let gwp = running::goal_week_plan(goal, profile.advanced).value;
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
    let mix = running::quality_mix(goal, cy.phase).value;
    let mut kinds: Vec<RunSessionType> = Vec::with_capacity(n);
    if !race_week && n > 0 {
        kinds.push(RunSessionType::LongRun);
    }
    // A taper keeps its quality sessions (running-037/038: intensity and
    // frequency held, only volume drops), so support is judged on the last
    // full load week; the sessions themselves shrink to their minimum.
    let quality_basis_km = if cy.taper.is_some() {
        top_load_km
    } else {
        weekly_km
    };
    for &k in mix.iter().take(q) {
        kinds.push(
            if quality_supported(k, quality_basis_km, anchors.run_vdot) {
                k
            } else {
                RunSessionType::Easy
            },
        );
    }
    while kinds.len() < n {
        kinds.push(RunSessionType::Easy);
    }

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
        c25k_week: None,
        strides,
        race_week_easy_cap_km,
    }
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

// ---------------------------------------------------------------------------
// The placed week: one prescription per run day
// ---------------------------------------------------------------------------

/// Everything a single day's prescription needs beyond its own kind.
pub(super) struct DayCtx {
    pub(super) weekly_km: f64,
    pub(super) baseline: Option<f64>,
    pub(super) baseline_stated: bool,
    pub(super) detrained: bool,
    pub(super) n_run_days: usize,
    pub(super) recovery: bool,
    pub(super) taper: bool,
    pub(super) goal: GoalDistance,
    pub(super) vdot: Option<f64>,
    /// Easy pace (s/km) warm-up/cool-down/recovery minutes are counted at.
    pub(super) easy_pace: f64,
    /// The week's long-run distance when it has one: no easy-paced day runs
    /// longer.
    pub(super) long_km: Option<f64>,
}

impl DayCtx {
    /// Threshold distance: ≤10 % of the week; in a taper never below the
    /// 20-minute floor (rounded up to the 0.1 km step), so the session is
    /// shortened, not dropped.
    fn tempo_km(&self) -> f64 {
        let cap = tempo_km_cap(self.weekly_km);
        if self.taper {
            cap.max((tempo_floor_km(self.vdot) * 10.0).ceil() / 10.0)
        } else {
            cap
        }
    }

    /// VO2max rep distance; in a taper the 800 m minimum at 3 reps.
    fn interval_rep_m(&self) -> Option<u16> {
        supported_rep_distance_m(self.weekly_km).or(self.taper.then_some(800))
    }

    /// `(reps, rep_m)` of the repetition set; in a taper the 4 × 200 m minimum.
    fn repetition(&self) -> Option<(u8, u16)> {
        let r = running::repetition_rx().value;
        repetition_plan(self.weekly_km).or(self.taper.then_some((r.min_reps, r.rep_m[1])))
    }

    /// Marathon-pace block; in a taper the 8 km "MP touch" minimum.
    fn race_pace_km(&self) -> Option<f64> {
        let lo = running::run_workout_rx(RunSessionType::RacePace)
            .value
            .distance_km
            .0
            .unwrap_or(8.0);
        race_pace_km(self.weekly_km, self.vdot).or(self.taper.then_some(lo))
    }
}

/// Easy pace (s/km) for weekly accounting: the middle of the athlete's VDOT
/// E band when a race anchors it, else the RUN-WARMUP-001 accounting default.
fn accounting_easy_pace(vdot: Option<f64>) -> f64 {
    vdot.and_then(|v| crate::load::vdot_band_pace_sec_per_km(v, VdotBand::Easy))
        .map(|(fast, slow)| (fast + slow) / 2.0)
        .unwrap_or(running::warmup_rx().value.accounting_easy_pace_sec_per_km)
}

/// Prescribe the scheduler's placement of this week's kinds: `placed` is
/// `(day, kind)` in any order. The race day and anything after it are dropped;
/// an easy day directly after a hard day or the long run becomes a Recovery run
/// (running-014 vs running-015); the first easy days carry strides; easy days
/// share what's left of the weekly km after the long run, the quality work and
/// its warm-up/cool-down.
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

    if let Some(week) = plan.c25k_week {
        // With fewer days than the programme's runs, the week keeps its LAST
        // runs: weeks 5–6 end on the bridge to continuous running.
        let runs = running::c25k_plan().value.runs_per_week as usize;
        let skipped = runs.saturating_sub(days.len());
        return days
            .iter()
            .enumerate()
            .map(|(i, &(day, _))| {
                c25k_session(week, (i + 1 + skipped).min(u8::MAX as usize) as u8, day)
            })
            .collect();
    }

    // An easy day right after a hard day or the long run is a recovery run.
    let hard: BTreeSet<u16> = days
        .iter()
        .filter(|(_, k)| is_quality(*k) || *k == RunSessionType::LongRun)
        .map(|(d, _)| *d)
        .collect();
    for (d, k) in days.iter_mut() {
        if *k == RunSessionType::Easy && *d > 0 && hard.contains(&(*d - 1)) {
            *k = RunSessionType::Recovery;
        }
    }

    let stated = profile
        .stated_longest_recent_km
        .filter(|k| k.is_finite() && *k > 0.0);
    let logged = anchors.longest_recent_run_km.filter(|k| *k > 0.0);
    let stated_wins = match (logged, stated) {
        (None, Some(_)) => true,
        (Some(l), Some(s)) => anchors.stated_longest_in_window && s > l,
        _ => false,
    };
    let baseline = if stated_wins { stated } else { logged };
    let mut ctx = DayCtx {
        weekly_km: plan.weekly_km,
        baseline,
        baseline_stated: stated_wins,
        detrained: anchors.longest_run_detrained && !stated_wins && logged.is_some(),
        n_run_days: days.len(),
        recovery: plan.recovery,
        taper: plan.taper_week.is_some(),
        goal: profile.goal_distance,
        vdot: anchors.run_vdot,
        easy_pace: accounting_easy_pace(anchors.run_vdot),
        long_km: None,
    };

    // Committed volume: long run + each quality session's main set and its
    // warm-up/cool-down (RUN-WARMUP-001: they count toward the week).
    let long_km = days
        .iter()
        .any(|(_, k)| *k == RunSessionType::LongRun)
        .then(|| long_run_decision(&ctx).0);
    ctx.long_km = long_km.filter(|l| *l > 0.0);
    let km_of_min = |min: f64| min * 60.0 / ctx.easy_pace.max(1.0);
    let mut committed = long_km.unwrap_or(0.0);
    let wu = running::warmup_rx().value;
    for (_, k) in &days {
        if is_quality(*k) && !spike_degrades_quality(*k, &ctx) {
            committed +=
                quality_main_km(*k, &ctx) + km_of_min((wu.warmup_min.0 + wu.cooldown_min) as f64);
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
    // A recovery run stays inside its 20–40 min band (running-014); the rest
    // of the residual moves to the easy days.
    let rec_max_min = running::run_workout_rx(RunSessionType::Recovery)
        .value
        .duration_min
        .1
        .unwrap_or(40) as f64;
    let rec_km = floor_km(
        per_soft
            .min(km_of_min(rec_max_min))
            .min(plan.race_week_easy_cap_km.unwrap_or(f64::INFINITY)),
    );
    let rec_counted = rec_km * n_rec as f64;
    let mut easy_km = if n_easy > 0 {
        (resid - rec_counted).max(0.0) / n_easy as f64
    } else {
        0.0
    };
    // No easy day longer than the long run.
    if let Some(l) = ctx.long_km {
        easy_km = easy_km.min(l);
    }
    if let Some(c) = plan.race_week_easy_cap_km {
        easy_km = easy_km.min(c);
    }
    // The weekly report judges every run against 2× the EXECUTED week's daily
    // average (RUN-LONGRUN-001): e ≤ 2·fixed/(n − 2·n_easy) whenever easy days
    // are fewer than half the run days.
    let n = ctx.n_run_days;
    if n > 2 * n_easy && n_easy > 0 {
        let fixed = committed + rec_counted;
        easy_km = easy_km.min(2.0 * fixed / (n - 2 * n_easy) as f64);
    }
    let easy_km = floor_km(easy_km);

    // Strides on the first one or two easy days (running-020: 1–3×/week).
    let st = running::strides_rx().value;
    let strides_days: BTreeSet<u16> = if plan.strides {
        days.iter()
            .filter(|(_, k)| *k == RunSessionType::Easy)
            .map(|(d, _)| *d)
            .take(2.min(st.per_week.1 as usize))
            .collect()
    } else {
        BTreeSet::new()
    };

    days.iter()
        .map(|&(day, kind)| {
            let soft_km = match kind {
                RunSessionType::Easy => easy_km,
                RunSessionType::Recovery => rec_km,
                _ => 0.0,
            };
            let mut s = run_session(kind, day, &ctx, soft_km);
            if strides_days.contains(&day)
                && let Some(rx) = s.prescriptions.first_mut()
                && let Prescription::Run(r) = &mut rx.value
            {
                r.strides = Some(StridesAddOn {
                    reps: mid_u16((Some(st.reps.0 as u16), Some(st.reps.1 as u16)))
                        .unwrap_or(st.reps.0 as u16)
                        .min(u8::MAX as u16) as u8,
                    // Band midpoints rounded down to a whole 5 s.
                    rep_sec: (st.rep_sec.0 + st.rep_sec.1) / 2 / 5 * 5,
                    recovery_sec: (st.recovery_sec.0 + st.recovery_sec.1) / 2 / 5 * 5,
                });
            }
            s
        })
        .collect()
}

/// One C25K run (`run_of_week`-th of the week) from the NHS table.
fn c25k_session(week: u8, run_of_week: u8, day: u16) -> Session {
    let rx = running::c25k_session_rx(week, run_of_week);
    let recovery_rx = running::run_workout_rx(RunSessionType::Recovery);
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
        prescriptions: vec![rx.with_value(Prescription::Run(pres))],
        dup_day: None,
    }
}

/// Main-set km of a quality session (warm-up/cool-down excluded).
fn quality_main_km(kind: RunSessionType, ctx: &DayCtx) -> f64 {
    match kind {
        RunSessionType::Tempo => cap_km_to_spike(ctx.tempo_km(), ctx.baseline).unwrap_or(0.0),
        RunSessionType::Interval => ctx
            .interval_rep_m()
            .map(|rep_m| {
                let reps = interval_reps_natural_at(ctx.weekly_km, rep_m).clamp(3, 8) as f64;
                rep_m as f64 / 1000.0 * reps
            })
            .unwrap_or(0.0),
        RunSessionType::Repetition => ctx
            .repetition()
            .map(|(reps, m)| reps as f64 * m as f64 / 1000.0 * 2.0) // + equal-distance jogs
            .unwrap_or(0.0),
        RunSessionType::RacePace => ctx
            .race_pace_km()
            .and_then(|k| cap_km_to_spike(k, ctx.baseline))
            .unwrap_or(0.0),
        _ => 0.0,
    }
}

/// One run day's evidence-cited prescription. `soft_km` is the sized
/// easy/recovery distance (0 → nothing to size from; see [`soft_volume`]).
fn run_session(kind: RunSessionType, day: u16, ctx: &DayCtx, soft_km: f64) -> Session {
    // In a recovery week the long run keeps its slot at the easy (Recovery)
    // intensity band and shrinks with the week (running-033 reduces both
    // volume AND intensity; see `long_run_decision`).
    let rx = if ctx.recovery && kind == RunSessionType::LongRun {
        running::run_workout_rx(RunSessionType::Recovery)
    } else {
        running::run_workout_rx(kind)
    };
    let degraded = spike_degrades_quality(kind, ctx);
    let pres = Prescription::Run(translate_run(kind, &rx.value, ctx, soft_km));
    let session = if kind == RunSessionType::LongRun && !ctx.recovery {
        // Cite whichever rule actually BINDS the long-run distance.
        let bind = long_run_decision(ctx).1;
        if ctx.detrained && !matches!(bind, LongRunBind::Volume | LongRunBind::Target) {
            crate::evidence::graded(pres, claims::DETRAIN_001)
        } else if bind.cites_spike() && ctx.baseline_stated {
            // The ceiling rests on the athlete's stated longest run.
            crate::evidence::graded(pres, claims::RUN_START_001)
        } else if bind.cites_spike() {
            crate::evidence::graded(pres, claims::RUN_SPIKE_001)
        } else if bind == LongRunBind::Target {
            crate::evidence::graded(pres, claims::RUN_LONGRUN_TARGET_001)
        } else if ctx.taper {
            crate::evidence::graded(pres, claims::RUN_TAPER_001)
        } else {
            rx.with_value(pres)
        }
    } else if ctx.recovery {
        // Every run in a recovery week is a reduced-volume, reduced-intensity
        // session: cite the recovery-week depth rule.
        crate::evidence::graded(pres, claims::RUN_DELOAD_DEPTH_001)
    } else if degraded {
        // A quality slot forced off distance by the spike ceiling is now an
        // easy run; RUN-SPIKE-001 governs the substitution.
        crate::evidence::graded(pres, claims::RUN_SPIKE_001)
    } else if ctx.taper && matches!(kind, RunSessionType::Easy | RunSessionType::Recovery) {
        // Taper: the easy volume is what the taper cut (intensity held).
        crate::evidence::graded(pres, claims::RUN_TAPER_001)
    } else if kind == RunSessionType::Repetition {
        crate::evidence::graded(pres, claims::RUN_REPETITION_002)
    } else {
        rx.with_value(pres)
    };
    Session {
        session_type: SessionType::Run(kind),
        day,
        prescriptions: vec![session],
        dup_day: None,
    }
}

/// Conservative target from a `u16` band: the MIDPOINT of a two-sided band
/// (never the ceiling) or the low value of a low-only band. A ceiling-only band
/// gives no defensible interior target, so it propagates `None`.
pub(super) fn mid_u16(band: (Option<u16>, Option<u16>)) -> Option<u16> {
    match band {
        (Some(lo), Some(hi)) => Some(((lo as u32 + hi as u32) / 2) as u16),
        (Some(lo), None) => Some(lo),
        (None, Some(_)) => None,
        (None, None) => None,
    }
}

/// Conservative target from an `f64` band (km): the MIDPOINT of a two-sided band
/// (never the ceiling) or the low value of a low-only band; a ceiling-only band
/// propagates `None` (HARD RULE 1).
pub(super) fn mid_f64(band: (Option<f64>, Option<f64>)) -> Option<f64> {
    match band {
        (Some(lo), Some(hi)) => Some((lo + hi) / 2.0),
        (Some(lo), None) => Some(lo),
        (None, Some(_)) => None,
        (None, None) => None,
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

/// Turn a `RunWorkoutRx` band into a concrete `RunPrescription`. Intervals and
/// repetitions emit per-rep structure with their recovery, quality sessions
/// carry a warm-up/cool-down, the long run follows the long-run decision, easy
/// and recovery days take their sized share of the week, and a quality slot
/// whose RUN-SPIKE-001 ceiling can't fit its minimum distance degrades to an
/// easy run.
fn translate_run(
    kind: RunSessionType,
    rx: &RunWorkoutRx,
    ctx: &DayCtx,
    soft_km: f64,
) -> RunPrescription {
    if spike_degrades_quality(kind, ctx) {
        return easy_fallback(ctx);
    }
    match kind {
        RunSessionType::Interval => with_warmup(interval_prescription(ctx)),
        RunSessionType::Repetition => match ctx.repetition() {
            Some((reps, rep_m)) => {
                let rep_km = rep_m as f32 / 1000.0;
                let mut r = RunPrescription::new(
                    RunVolume::DistanceKm(rep_km * reps as f32),
                    RunIntensity::Vdot(VdotBand::Repetition),
                    Some((reps, RunVolume::DistanceKm(rep_km))),
                );
                // Jog as far as the rep (running-051).
                r.recovery = Some(RepRecovery::Metres(rep_m));
                with_warmup(r)
            }
            None => easy_fallback(ctx),
        },
        RunSessionType::LongRun => {
            // Very low weekly volume or a sub-1 km baseline: below 1 km the
            // run is the minutes that distance takes at easy pace.
            let (target_km, _) = long_run_decision(ctx);
            let easy = running::run_workout_rx(RunSessionType::Easy).value;
            RunPrescription::new(
                soft_volume(&easy, ctx, target_km),
                conservative_intensity(kind, rx),
                None,
            )
        }
        RunSessionType::Tempo => {
            // Threshold volume capped at ≤10% of weekly km (RUN-TEMPO-001),
            // bounded by the spike ceiling, as a DISTANCE with the %HRmax band.
            match cap_km_to_spike(ctx.tempo_km(), ctx.baseline) {
                Some(km) => with_warmup(RunPrescription::new(
                    RunVolume::DistanceKm(km as f32),
                    conservative_intensity(kind, rx),
                    None,
                )),
                None => easy_fallback(ctx),
            }
        }
        RunSessionType::RacePace => match ctx
            .race_pace_km()
            .and_then(|k| cap_km_to_spike(k, ctx.baseline))
        {
            Some(km) => with_warmup(RunPrescription::new(
                RunVolume::DistanceKm(km as f32),
                conservative_intensity(kind, rx),
                None,
            )),
            None => easy_fallback(ctx),
        },
        RunSessionType::Easy | RunSessionType::Recovery => RunPrescription::new(
            soft_volume(rx, ctx, soft_km),
            conservative_intensity(kind, rx),
            None,
        ),
        _ => continuous_prescription(kind, rx, ctx.baseline),
    }
}

/// A continuous run (Hills / Strides when scheduled on their own): conservative
/// distance capped at the spike ceiling, else a conservative duration.
pub(super) fn continuous_prescription(
    kind: RunSessionType,
    rx: &RunWorkoutRx,
    longest_recent_km: Option<f64>,
) -> RunPrescription {
    let capped_km = mid_f64(rx.distance_km).and_then(|km| cap_km_to_spike(km, longest_recent_km));
    let volume = if let Some(km) = capped_km {
        RunVolume::DistanceKm(km as f32)
    } else if let Some(m) = mid_u16(rx.duration_min) {
        RunVolume::DurationMin(m)
    } else {
        RunVolume::DurationMin(30)
    };
    RunPrescription::new(volume, conservative_intensity(kind, rx), None)
}

/// Smallest KB VO2max rep distance (running-019: 800 m) and repetition rep
/// (running-051: 200 m). Below the spike ceiling for even one rep, the slot has
/// no honest distance.
const SMALLEST_INTERVAL_REP_KM: f64 = 0.8;
const SMALLEST_REPETITION_REP_KM: f64 = 0.2;

/// Whether a quality slot must degrade to an easy run because the RUN-SPIKE-001
/// ceiling over a positive sub-1 km baseline can't fit its minimum distance.
/// Shared by `translate_run` (the prescription) and `run_session` (its
/// evidence face) so the two never disagree.
fn spike_degrades_quality(kind: RunSessionType, ctx: &DayCtx) -> bool {
    match kind {
        RunSessionType::Tempo => cap_km_to_spike(ctx.tempo_km(), ctx.baseline).is_none(),
        RunSessionType::Interval => {
            cap_km_to_spike(SMALLEST_INTERVAL_REP_KM, ctx.baseline).is_none()
        }
        RunSessionType::Repetition => {
            cap_km_to_spike(SMALLEST_REPETITION_REP_KM, ctx.baseline).is_none()
        }
        RunSessionType::RacePace => ctx
            .race_pace_km()
            .is_some_and(|k| cap_km_to_spike(k, ctx.baseline).is_none()),
        _ => false,
    }
}

/// A quality slot the RUN-SPIKE-001 ceiling forces off distance: a conservative
/// easy run at the Recovery band, the band's duration floor held under the
/// spike ceiling and the long run.
fn easy_fallback(ctx: &DayCtx) -> RunPrescription {
    let recovery_rx = running::run_workout_rx(RunSessionType::Recovery);
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
        ctx.long_km,
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
/// KB rule binds it:
///  - the weekly share: 30 % under 64 km/wk, 25 % at or above (running-046,
///    Daniels), the base;
///  - the ≤2×-daily-average guardrail (running-016);
///  - demonstrated capacity may raise it above the share, up to that guardrail,
///    except in a planned recovery week: there the long run shrinks with the
///    week and stays within its share (running-033 cuts volume; the owner's
///    "never below the demonstrated longest" governs load weeks);
///  - the goal long-run ceiling (HM 21 km, marathon 32 km) and, with a pace
///    anchor, the 150-min cap at easy pace (running-046);
///  - RUN-SPIKE-001: never more than 10 % over the 30-day longest run (logged,
///    or the stated one while it is longer and still inside its own 30 days:
///    RUN-START-001), a hard ceiling on all.
pub(super) fn long_run_decision(ctx: &DayCtx) -> (f64, LongRunBind) {
    let w = ctx.weekly_km.max(0.0);
    let share = running::long_run_share(w).value;
    let mut target = floor_km(share * w);
    let mut bind = LongRunBind::Volume;
    let daily_avg_cap = (ctx.n_run_days > 0 && w > 0.0)
        .then(|| floor_km(2.0 * daily_avg_run_km(w, ctx.n_run_days)));
    if let Some(cap) = daily_avg_cap
        && cap < target
    {
        target = cap;
        bind = LongRunBind::DailyAvgCap;
    }
    let baseline = ctx.baseline.filter(|b| *b > 0.0);
    // Demonstrated CAPACITY (a distance, ≥ MIN_DISTANCE_KM) may raise the run
    // above the share, bounded by the daily-average guardrail; never in a
    // recovery week.
    if !ctx.recovery
        && let Some(b) = baseline.filter(|b| *b >= MIN_DISTANCE_KM)
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
        .and_then(|v| crate::load::vdot_band_pace_sec_per_km(v, VdotBand::Easy))
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

/// VO2max intervals as PER-REP structure at the largest supported rep
/// distance (1200 m, then 800 m — running-019), rep count derived from the ≤8 %
/// weekly cap, clamped 3–8. Recovery ≈ the rep's own time (ratio ceiling 1.0):
/// seconds when a pace anchor turns the rep into time, else "jog as long as
/// the rep took".
fn interval_prescription(ctx: &DayCtx) -> RunPrescription {
    let s = running::vo2max_interval_rx().value;
    let rep_m = ctx.interval_rep_m().unwrap_or(800);
    let reps = interval_reps_natural_at(ctx.weekly_km, rep_m).clamp(3, 8) as u8;
    let rep_km = rep_m as f32 / 1000.0;
    let mut r = RunPrescription::new(
        RunVolume::DistanceKm(rep_km * reps as f32),
        RunIntensity::Vdot(VdotBand::Interval),
        Some((reps, RunVolume::DistanceKm(rep_km))),
    );
    match ctx
        .vdot
        .and_then(|v| crate::load::vdot_band_pace_sec_per_km(v, VdotBand::Interval))
    {
        Some((fast, slow)) => {
            let rep_sec = (fast + slow) / 2.0 * rep_km as f64;
            r.recovery = Some(RepRecovery::Seconds(
                (rep_sec * s.recovery_max_ratio_of_rep)
                    .round()
                    .min(u16::MAX as f64) as u16,
            ));
        }
        None => r.recovery = Some(RepRecovery::EqualToRep),
    }
    r
}

/// Map a session type to its Daniels VDOT band (used only when the KB gives no
/// %HRmax band for that session type).
fn vdot_band_for(kind: RunSessionType) -> VdotBand {
    match kind {
        RunSessionType::Recovery | RunSessionType::LongRun | RunSessionType::Easy => VdotBand::Easy,
        RunSessionType::Tempo => VdotBand::Threshold,
        RunSessionType::Interval | RunSessionType::Hills => VdotBand::Interval,
        RunSessionType::Repetition | RunSessionType::Strides => VdotBand::Repetition,
        RunSessionType::RacePace => VdotBand::Marathon,
    }
}
