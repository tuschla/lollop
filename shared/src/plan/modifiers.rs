//! Week modifiers: rules that rewrite a week's already-built lift sessions in
//! place (volume conservation across day counts, the maintenance dose, the
//! hybrid lower-body cap, the meet taper, the lifting deload and the
//! running-recovery effort floor). Each rewritten prescription is re-graded to
//! the rule that set its new numbers.

use std::collections::BTreeMap;

use crate::evidence::{ClaimId, claims};
use crate::exercise::{MovementPattern, competition_lift, movement_key, movement_pattern};
use crate::hybrid;
use crate::hypertrophy;
use crate::schema::{LiftIntensity, Prescription, Recommended, Rx, RxBasis, Session, SessionType};
use crate::strength::{self, LiftGoal};

/// Rewrite every prescription of the week through `f`, re-grading each row it
/// returns to the rule it names; `None` leaves the row and its grade as built.
fn regrade_each(sessions: &mut [Session], mut f: impl FnMut(&Prescription) -> Option<Rx>) {
    for s in sessions.iter_mut() {
        for rx in &mut s.prescriptions {
            if let Some(new) = f(&rx.value) {
                *rx = new;
            }
        }
    }
}

/// Keep each exercise's WEEKLY set total at `targets`, the default layout's
/// weekly sets per movement (3 full-body days, 2 for a hybrid), whatever day
/// count the athlete chose: at matched weekly volume, training frequency has
/// no independent effect on strength (LIFT-DAYS-001). The total is spread
/// evenly over the days that train the exercise (remainder to the earliest),
/// within the per-session ceiling (HYP-SESSCAP-001). Changed rows cite
/// LIFT-DAYS-001.
pub(super) fn conserve_weekly_sets(sessions: &mut [Session], targets: &BTreeMap<Vec<String>, u32>) {
    for (key, total) in targets {
        let mut slots: Vec<(usize, usize)> = Vec::new();
        for (si, s) in sessions.iter().enumerate() {
            for (pi, rx) in s.prescriptions.iter().enumerate() {
                if let Prescription::Lift(l) = &rx.value
                    && movement_key(&l.exercise) == *key
                {
                    slots.push((si, pi));
                }
            }
        }
        let k = slots.len() as u32;
        if k == 0 {
            continue;
        }
        for (i, &(si, pi)) in slots.iter().enumerate() {
            let share = total / k + u32::from((i as u32) < total % k);
            let sets = share.clamp(1, hypertrophy::PER_SESSION_SET_CEILING as u32) as u8;
            let rx = &mut sessions[si].prescriptions[pi];
            if let Prescription::Lift(l) = &rx.value
                && l.sets != sets
            {
                let mut l = l.clone();
                l.sets = sets;
                *rx = RxBasis::Workout.rx(Prescription::Lift(l), claims::LIFT_DAYS_001);
            }
        }
    }
}

/// Lifting as the MAINTAINED side (running leads, or a runner's supplemental
/// strength): per-exercise sets cut to about a third, inside 2–4 heavy sets
/// (HYB-MAINT-001, File 10 CAP-7), load and intensity held (HYB-MAINT-002).
pub(super) fn apply_maintenance_dose(sessions: &mut [Session], claim: ClaimId) {
    regrade_each(sessions, |rx| match rx {
        Prescription::Lift(l) => {
            let mut l = l.clone();
            l.sets = hybrid::maintenance_sets_per_exercise(l.sets).value;
            Some(RxBasis::Maintenance.rx(Prescription::Lift(l), claim))
        }
        Prescription::Calisthenics(c) => {
            let mut c = c.clone();
            c.sets = hybrid::maintenance_sets_per_exercise(c.sets).value;
            Some(RxBasis::Maintenance.rx(Prescription::Calisthenics(c), claim))
        }
        Prescription::Run(_) => None,
    });
}

/// CAP-1 (HYB-CAP-001): trim the week's lower-body (squat / hinge) sets by the
/// midpoint of the KB's 20–33 % cut, one set at a time from the lower-body
/// exercise carrying the most sets, never below one set per exercise
/// (HYB-MAINT-002: one set at the same load holds strength).
pub(super) fn apply_lower_cap(sessions: &mut [Session], cap: &Recommended<hybrid::LowerLiftCap>) {
    let is_lower = |rx: &Rx| match &rx.value {
        Prescription::Lift(l) => {
            movement_pattern(&l.exercise).is_some_and(MovementPattern::is_lower)
        }
        _ => false,
    };
    let sets_of = |rx: &Rx| match &rx.value {
        Prescription::Lift(l) => l.sets as u32,
        _ => 0,
    };
    let mut lower: Vec<(usize, usize)> = Vec::new();
    for (si, s) in sessions.iter().enumerate() {
        for (pi, rx) in s.prescriptions.iter().enumerate() {
            if is_lower(rx) {
                lower.push((si, pi));
            }
        }
    }
    let total: u32 = lower
        .iter()
        .map(|&(si, pi)| sets_of(&sessions[si].prescriptions[pi]))
        .sum();
    let target = hybrid::capped_lower_weekly_sets(total, &cap.value, lower.len() as u32);
    let mut excess = total.saturating_sub(target);
    let mut changed: Vec<(usize, usize)> = Vec::new();
    while excess > 0 {
        // The lower-body row with the most sets (the earliest on a tie).
        let Some(&(si, pi)) = lower
            .iter()
            .rev()
            .max_by_key(|&&(si, pi)| sets_of(&sessions[si].prescriptions[pi]))
        else {
            break;
        };
        let rx = &mut sessions[si].prescriptions[pi];
        let Prescription::Lift(l) = &mut rx.value else {
            break;
        };
        if l.sets <= 1 {
            break;
        }
        l.sets -= 1;
        excess -= 1;
        if !changed.contains(&(si, pi)) {
            changed.push((si, pi));
        }
    }
    for (si, pi) in changed {
        let rx = &mut sessions[si].prescriptions[pi];
        *rx = cap.with_value(rx.value.clone()).with_kind(RxBasis::Workout);
    }
}

/// STR-DELOAD-VOL-001: loads at most ~10 % lighter in a strength deload.
const STRENGTH_DELOAD_LOAD_FRAC: f32 = 0.90;
/// Deload effort floor: at least 3 reps in reserve (STR-DELOAD-VOL-001 RIR ≥3;
/// inside HYP-DELOAD-RX-001's 2–4).
const DELOAD_MIN_RIR: u8 = 3;

/// `rir` reps in reserve on the RPE scale.
fn rpe_at_rir(rir: u8) -> f32 {
    strength::rir_to_rpe(f64::from(rir)) as f32
}

/// Deload the lifting work of a lift deload week in place, movement patterns
/// kept. Strength / power goals cut VOLUME and keep load: half the sets, %1RM
/// at most 10 % lighter, RIR ≥3 (STR-DELOAD-VOL-001). Hypertrophy and
/// calisthenics keep the standard hypertrophy deload: half the sets, loads
/// ~60–70 %, 2–4 RIR (HYP-DELOAD-RX-001). Each mutated prescription is
/// re-graded to the rule that set its numbers.
pub(super) fn apply_deload_week(sessions: &mut [Session], goal: LiftGoal) {
    let d = hypertrophy::deload_rx().value;
    let hyp_load_frac = ((d.load_frac_of_working.0 + d.load_frac_of_working.1) / 2.0) as f32;
    let strength_goal = matches!(goal, LiftGoal::MaxStrength | LiftGoal::Power);
    let (load_frac, lift_basis, lift_claim) = if strength_goal {
        (
            STRENGTH_DELOAD_LOAD_FRAC,
            RxBasis::StrengthDeload,
            claims::STR_DELOAD_VOL_001,
        )
    } else {
        (
            hyp_load_frac,
            RxBasis::HypertrophyDeload,
            claims::HYP_DELOAD_RX_001,
        )
    };
    let halve = |sets: u8| ((sets as f64 * d.sets_fraction).ceil() as u8).max(1);
    regrade_each(sessions, |rx| match rx {
        Prescription::Lift(l) => {
            let mut l = l.clone();
            l.sets = halve(l.sets);
            l.intensity = match l.intensity {
                LiftIntensity::PercentOneRm(p) => LiftIntensity::PercentOneRm(p * load_frac),
                LiftIntensity::Rir(r) => LiftIntensity::Rir(r.max(DELOAD_MIN_RIR)),
                LiftIntensity::Rpe(x) => LiftIntensity::Rpe(x.min(rpe_at_rir(DELOAD_MIN_RIR))),
                LiftIntensity::MaxIntent => LiftIntensity::MaxIntent,
            };
            l.load_kg = l.load_kg.map(|kg| kg * load_frac);
            Some(lift_basis.rx(Prescription::Lift(l), lift_claim))
        }
        Prescription::Calisthenics(c) => {
            let mut c = c.clone();
            c.sets = halve(c.sets);
            c.rir = d.rir;
            Some(
                RxBasis::HypertrophyDeload
                    .rx(Prescription::Calisthenics(c), claims::HYP_DELOAD_RX_001),
            )
        }
        Prescription::Run(_) => None,
    });
}

/// A running recovery week's lifting effort floor: no set closer than 2 reps
/// from failure (HYP-DELOAD-CADENCE-001: effort is the lever for a lighter week).
const RUN_RECOVERY_MIN_RIR: u8 = 2;

/// A RUNNING recovery week leaves the lifting as built (lifting deloads on its
/// own block, HYP-DELOAD-CADENCE-001), except that no set is taken closer than
/// [`RUN_RECOVERY_MIN_RIR`] reps from failure that week (effort is the lever
/// the claim names for a lighter week). Changed rows cite
/// HYP-DELOAD-CADENCE-001.
pub(super) fn apply_run_recovery_effort_floor(sessions: &mut [Session]) {
    regrade_each(sessions, |rx| {
        let value = match rx {
            Prescription::Lift(l) => match l.intensity {
                LiftIntensity::Rir(r) if r < RUN_RECOVERY_MIN_RIR => {
                    let mut l = l.clone();
                    l.intensity = LiftIntensity::Rir(RUN_RECOVERY_MIN_RIR);
                    Prescription::Lift(l)
                }
                LiftIntensity::Rpe(x) if x > rpe_at_rir(RUN_RECOVERY_MIN_RIR) => {
                    let mut l = l.clone();
                    l.intensity = LiftIntensity::Rpe(rpe_at_rir(RUN_RECOVERY_MIN_RIR));
                    Prescription::Lift(l)
                }
                _ => return None,
            },
            Prescription::Calisthenics(c) if c.rir.0 < RUN_RECOVERY_MIN_RIR => {
                let mut c = c.clone();
                c.rir = (RUN_RECOVERY_MIN_RIR, c.rir.1.max(RUN_RECOVERY_MIN_RIR));
                Prescription::Calisthenics(c)
            }
            _ => return None,
        };
        Some(RxBasis::Workout.rx(value, claims::HYP_DELOAD_CADENCE_001))
    });
}

/// Whether the meet taper ([`apply_meet_taper`]) reaches a lift session on one
/// of `lift_days` (0 = Monday): the session falls 0–14 days before the meet
/// (TAPER-001's duration).
pub(super) fn meet_taper_reaches(days_to_meet_from_week_start: i64, lift_days: &[u16]) -> bool {
    let taper_days = i64::from(strength::taper_rx().value.duration_days.1);
    lift_days
        .iter()
        .any(|&day| (0..=taper_days).contains(&(days_to_meet_from_week_start - i64::from(day))))
}

/// Shape a barbell MaxStrength/Power week toward a strength test / meet.
/// `days_to_meet_from_week_start` = meet day − this week's Monday; each
/// session's own distance is that minus its `day` offset. Lift sessions only;
/// runs and rest days are untouched. For a lift session `d` days out:
/// - `d ≤ 2` (meet day and the 2 days before): no lifting — the KB peak ends
///   with 2–7 days of reduced work/cessation (STR-PEAK-001).
/// - `3 ≤ d ≤ 14`: the taper — volume cut ~41 % (8–14 days out) then ~60 %
///   (3–7 days out), intensity and frequency held (TAPER-001, strength-026);
///   heavy deadlifts stop inside 10 days (STR-DLPEAK-001: last heavy pull 10–14
///   days out) and heavy squat/bench inside 4 days (strength-029: ~4–5 days).
///
/// A session left with nothing becomes a rest day. Beyond 14 days, or after the
/// meet, the week is unchanged.
pub(super) fn apply_meet_taper(sessions: &mut [Session], days_to_meet_from_week_start: i64) {
    let taper = strength::taper_rx().value;
    let dl_cutoff = strength::deadlift_peak_days_out().value.0 as i64;
    const SQUAT_BENCH_CUTOFF: i64 = 4;
    const CESSATION_DAYS: i64 = 2;
    const TAPER_DEEP_CUT_DAYS: i64 = 7;
    for s in sessions.iter_mut() {
        if !matches!(s.session_type, SessionType::Lift(_))
            || !meet_taper_reaches(days_to_meet_from_week_start, &[s.day])
        {
            continue;
        }
        let d = days_to_meet_from_week_start - s.day as i64;
        if d <= CESSATION_DAYS {
            s.session_type = SessionType::Rest;
            s.prescriptions.clear();
            continue;
        }
        let cut = if d <= TAPER_DEEP_CUT_DAYS {
            taper.volume_reduction_frac.1
        } else {
            taper.volume_reduction_frac.0
        };
        s.prescriptions.retain(|rx| {
            let Prescription::Lift(l) = &rx.value else {
                return true;
            };
            let lift = competition_lift(&l.exercise);
            let heavy = strength::is_heavy_intensity(l.intensity);
            let drop_dl = lift.deadlift && d < dl_cutoff && heavy;
            let drop_sb = lift.squat_or_bench && d <= SQUAT_BENCH_CUTOFF && heavy;
            !(drop_dl || drop_sb)
        });
        for rx in s.prescriptions.iter_mut() {
            if let Prescription::Lift(l) = &rx.value {
                let mut l = l.clone();
                l.sets = ((l.sets as f64 * (1.0 - cut)).round() as u8).max(1);
                *rx = RxBasis::Workout.rx(Prescription::Lift(l), claims::TAPER_001);
            }
        }
        if s.prescriptions.is_empty() {
            s.session_type = SessionType::Rest;
        }
    }
}
