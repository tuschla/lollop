//! Shared fixtures for the plan tests.

use super::*;
use crate::evidence::ClaimId;
use crate::plan::LastWorking;
use crate::schema::{LiftPrescription, RunPrescription, RunVolume};

/// Monday epoch day the dated tests count from.
pub(super) const MON0: i64 = 20_000 - (20_000_i64.rem_euclid(7) + 3).rem_euclid(7);

/// Anchors with one logged lift's e1RM.
pub(super) fn anchored(name: &str, e1rm: f64) -> Anchors {
    anchored_many(&[(name, e1rm)])
}

/// Anchors with these logged lifts' e1RMs, most recent first.
pub(super) fn anchored_many(lifts: &[(&str, f64)]) -> Anchors {
    Anchors {
        lift: crate::plan::LiftAnchors {
            e1rm: lifts.iter().map(|(n, kg)| (n.to_string(), *kg)).collect(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// The last session's working sets of one lift, two days ago.
pub(super) fn worked(name: &str, kg: f64, sets: &[(u32, f64)]) -> LastWorking {
    LastWorking {
        exercise: name.to_string(),
        weight_kg: kg,
        sets: sets.to_vec(),
        age_days: 2,
    }
}

/// Every lift prescription of a program with its session and claim.
pub(super) fn lift_rx(prog: &Program) -> Vec<(&Session, &LiftPrescription, ClaimId)> {
    prog.mesocycle
        .sessions
        .iter()
        .flat_map(|s| {
            s.prescriptions
                .iter()
                .filter_map(move |rx| match &rx.value {
                    Prescription::Lift(l) => Some((s, l, rx.claim_id())),
                    _ => None,
                })
        })
        .collect()
}

/// The program's lift sessions, in day order.
pub(super) fn lift_sessions(prog: &Program) -> Vec<&Session> {
    prog.mesocycle
        .sessions
        .iter()
        .filter(|s| matches!(s.session_type, SessionType::Lift(_)))
        .collect()
}

/// A run session's prescription.
pub(super) fn run_of(s: &Session) -> &RunPrescription {
    match &s.prescriptions[0].value {
        Prescription::Run(r) => r,
        other => panic!("not a run: {other:?}"),
    }
}

/// A run volume's distance (0 for a duration or run/walk).
pub(super) fn km_of(v: RunVolume) -> f64 {
    match v {
        RunVolume::DistanceKm(k) => f64::from(k),
        _ => 0.0,
    }
}

/// The program's run session kinds.
pub(super) fn run_kinds_of(prog: &Program) -> Vec<RunSessionType> {
    prog.mesocycle
        .sessions
        .iter()
        .filter_map(|s| match s.session_type {
            SessionType::Run(k) => Some(k),
            _ => None,
        })
        .collect()
}

/// Distance (km) of the long run in a synthesized program, if it is prescribed
/// as a distance (not a duration fallback).
pub(super) fn long_run_km(prog: &Program) -> Option<f64> {
    prog.mesocycle
        .sessions
        .iter()
        .find(|s| s.session_type == SessionType::Run(RunSessionType::LongRun))
        .and_then(|s| match run_of(s).volume {
            RunVolume::DistanceKm(km) => Some(f64::from(km)),
            _ => None,
        })
}

/// The program's run distance (km), main sets only.
pub(super) fn run_distance_km(prog: &Program) -> f64 {
    prog.mesocycle
        .sessions
        .iter()
        .filter(|s| matches!(s.session_type, SessionType::Run(_)))
        .map(|s| km_of(run_of(s).volume))
        .sum()
}

pub(super) fn half_marathoner() -> Profile {
    let mut p = base_profile();
    p.weekly_sets = 0;
    p.goal_distance = GoalDistance::HalfMarathon;
    p.running_days_per_week = 3;
    // Static guided-setup heuristic, a low 16 km/wk that never updates.
    p.running_km_per_week = 16.0;
    p
}

pub(super) fn runner(goal: GoalDistance, days: u8, km: f64) -> Profile {
    let mut p = base_profile();
    p.weekly_sets = 0;
    p.goal_distance = goal;
    p.running_days_per_week = days;
    p.running_km_per_week = km;
    p
}
