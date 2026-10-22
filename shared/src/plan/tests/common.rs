//! Shared fixtures for the plan tests.

use super::*;
use crate::schema::RunVolume;

pub(super) fn anchored(name: &str, e1rm: f64) -> Anchors {
    Anchors {
        lift_e1rm: vec![(name.to_string(), e1rm)],
        ..Default::default()
    }
}

/// Distance (km) of the long run in a synthesized program, if it is prescribed
/// as a distance (not a duration fallback).
pub(super) fn long_run_km(prog: &Program) -> Option<f64> {
    prog.mesocycle
        .sessions
        .iter()
        .find(|s| s.session_type == SessionType::Run(RunSessionType::LongRun))
        .and_then(|s| match &s.prescriptions[0].value {
            Prescription::Run(r) => match r.volume {
                RunVolume::DistanceKm(km) => Some(km as f64),
                RunVolume::DurationMin(_) => None,
                RunVolume::RunWalk { .. } => None,
            },
            _ => None,
        })
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
