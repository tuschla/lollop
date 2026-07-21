//! Event-log compaction. Shells persist the raw [`Event`] stream and rebuild
//! state by replaying it (the core keeps no durable state), so the log grows
//! without bound, a GPS-tracked run alone is thousands of points on one line.
//!
//! [`compact_event_log`] drops lines whose effect on the replayed model is
//! provably nil, returning a shorter but **replay-equivalent** stream (same
//! relative order of survivors). Two rules, both grounded in [`crate::app`]'s
//! `update`, where the model stores only raw inputs and no event's `update`
//! reads another family's state:
//!
//! 1. **A `Clear<F>` supersedes its family.** It empties family `F`'s vec, so
//!    every `F` event (and the clear) at or before the *last* `Clear<F>` leaves
//!    no residue, `F` events after it replay against an already-empty vec.
//!    (A run's `longest_recent_km` reads prior runs, but only within the run
//!    family and only forward of a clear, so this stays exact.)
//! 2. **Last write wins for singletons.** `SetProfile` / `SubmitReview` assign
//!    `model.profile` / `model.review` outright, and nothing else reads them at
//!    update time, so only the last surviving one matters.
//!
//! This is the authoritative implementation, and the replay-equivalence tests
//! below pin its correctness. Because it is *not* exposed over the JSON FFI, the
//! Kotlin Android shell reimplements the same two rules over raw wire lines in
//! `android/.../EventLog.kt::compact` (called from `Core.kt`). The two must stay
//! in lockstep: any change to the families here (or a new `Event` variant) must
//! be mirrored there.

use crate::app::Event;

/// Family id + `is_clear` for one event. The `match` is exhaustive (no `_`) so a
/// new [`Event`] variant fails to compile until it is placed in a family, a
/// mis-filed variant would silently corrupt compaction.
fn classify(event: &Event) -> (u8, bool) {
    match event {
        Event::SubmitReadiness(_) => (0, false),
        Event::ClearReadiness => (0, true),
        Event::LogSet { .. } => (1, false),
        Event::ClearSets => (1, true),
        Event::LogRun { .. } | Event::LogRunTrack { .. } => (2, false),
        Event::ClearRuns => (2, true),
        Event::SetProfile(_) => (3, false),
        Event::ClearProfile => (3, true),
        Event::SubmitReview(_) => (4, false),
        Event::ClearReview => (4, true),
        Event::PredictRace { .. } => (5, false),
        Event::ClearRacePrediction => (5, true),
        Event::PlanHypertrophyMeso { .. } => (6, false),
        Event::ClearHypertrophyPlan => (6, true),
        Event::ComputeProtein { .. } => (7, false),
        Event::ClearProtein => (7, true),
        Event::ComputeHrZones { .. } => (8, false),
        Event::ClearHrZones => (8, true),
        Event::ComputeCooper { .. } => (9, false),
        Event::ClearCooper => (9, true),
        Event::ComputeCriticalSpeed { .. } => (10, false),
        Event::ClearCriticalSpeed => (10, true),
        Event::ComputeApre { .. } => (11, false),
        Event::ClearApre => (11, true),
    }
}

/// Number of distinct families [`classify`] partitions events into.
const FAMILIES: u8 = 12;
/// Singleton families whose members are last-write-wins (profile, review,
/// race prediction, hypertrophy plan, protein target, HR-zone table, Cooper
/// test, critical-speed fit, APRE adjustment).
const SINGLETONS: [u8; 9] = [3, 4, 5, 6, 7, 8, 9, 10, 11];

/// Drop provably-inert events from a persisted log, preserving replay order.
pub fn compact_event_log(events: Vec<Event>) -> Vec<Event> {
    let kinds: Vec<(u8, bool)> = events.iter().map(classify).collect();
    let mut remove = vec![false; events.len()];

    // Rule 1: everything in family F at or before its last clear.
    for fam in 0..FAMILIES {
        if let Some(last_clear) = kinds.iter().rposition(|&(f, c)| f == fam && c) {
            for (j, &(f, _)) in kinds.iter().enumerate().take(last_clear + 1) {
                if f == fam {
                    remove[j] = true;
                }
            }
        }
    }

    // Rule 2: for each singleton family, keep only the last surviving member.
    for fam in SINGLETONS {
        let survivors: Vec<usize> = kinds
            .iter()
            .enumerate()
            .filter(|&(j, &(f, c))| f == fam && !c && !remove[j])
            .map(|(j, _)| j)
            .collect();
        for &j in survivors.iter().rev().skip(1) {
            remove[j] = true;
        }
    }

    events
        .into_iter()
        .enumerate()
        .filter(|(j, _)| !remove[*j])
        .map(|(_, e)| e)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{Engine, Model, Profile, SessionReview};
    use crate::hybrid::ConcurrentGoal;
    use crate::individualization::ProgressionCadence;
    use crate::running::GoalDistance;
    use crate::schema::ReadinessInput;
    use crate::schema::ReadinessSignal;
    use crate::strength::LiftGoal;
    use crux_core::App;

    fn set(exercise: &str) -> Event {
        Event::LogSet {
            exercise: exercise.into(),
            weight_kg: 100.0,
            reps: 5,
            rpe: 8.0,
            observed_at: 0,
        }
    }

    fn run(distance_km: f64) -> Event {
        Event::LogRun {
            distance_km,
            duration_min: distance_km * 5.0,
            hr_pct_max: 75.0,
            longest_recent_km: 0.0,
            observed_at: 0,
        }
    }

    fn readiness(value: f64) -> Event {
        Event::SubmitReadiness(ReadinessInput {
            signal: ReadinessSignal::WellnessZ,
            value,
            observed_at: 0,
            streak: 0,
            pain: None,
            effort_min: None,
        })
    }

    fn profile(weekly_sets: u8) -> Event {
        Event::SetProfile(Profile {
            progression_cadence: ProgressionCadence::WeekToWeek,
            lift_goal: LiftGoal::MaxStrength,
            goal_distance: GoalDistance::TenK,
            concurrent_goal: ConcurrentGoal::Strength,
            weekly_sets,
            running_days_per_week: 4,
            running_km_per_week: 45.0,
            advanced: false,
            endurance_intensity_pct_vo2max: 75.0,
            female: false,
            high_load_block: false,
            health: Default::default(),
            environment: None,
            env_temp_c: None,
            env_altitude_m: None,
            weeks_off: None,
            bodyweight_kg: None,
        })
    }

    fn review(overtraining: u8) -> Event {
        Event::SubmitReview(SessionReview {
            overtraining_signal_count: overtraining,
            ..SessionReview::default()
        })
    }

    /// Rebuild the view a shell would render from a raw event stream.
    fn replay(events: &[Event]) -> crate::app::ViewModel {
        let app = Engine;
        let mut model = Model::default();
        for ev in events {
            let _ = app.update(ev.clone(), &mut model);
        }
        app.view(&model)
    }

    /// The core invariant: compaction must not change what the shell renders.
    fn assert_replay_equivalent(events: Vec<Event>) {
        let compacted = compact_event_log(events.clone());
        assert!(
            compacted.len() <= events.len(),
            "compaction must never grow the log"
        );
        assert_eq!(
            replay(&events),
            replay(&compacted),
            "compacted log replayed to a different view"
        );
    }

    #[test]
    fn clear_drops_its_family_prefix_but_keeps_later_entries() {
        let out = compact_event_log(vec![set("a"), set("b"), Event::ClearSets, set("c")]);
        assert_eq!(out, vec![set("c")], "only the post-clear set survives");
    }

    #[test]
    fn clear_with_no_later_entry_collapses_to_empty() {
        let out = compact_event_log(vec![run(5.0), run(6.0), Event::ClearRuns]);
        assert!(out.is_empty(), "a trailing clear leaves nothing to replay");
    }

    #[test]
    fn clear_only_touches_its_own_family() {
        let out = compact_event_log(vec![set("a"), run(5.0), Event::ClearSets]);
        assert_eq!(out, vec![run(5.0)], "the run survives the ClearSets");
    }

    #[test]
    fn only_the_last_profile_survives() {
        let out = compact_event_log(vec![profile(10), set("a"), profile(20)]);
        assert_eq!(
            out,
            vec![set("a"), profile(20)],
            "earlier profile is dropped"
        );
    }

    #[test]
    fn only_the_last_review_survives() {
        let out = compact_event_log(vec![review(1), review(2), review(3)]);
        assert_eq!(out, vec![review(3)]);
    }

    #[test]
    fn empty_log_stays_empty() {
        assert!(compact_event_log(vec![]).is_empty());
    }

    #[test]
    fn replay_equivalence_across_interleaved_families() {
        assert_replay_equivalent(vec![
            profile(10),
            readiness(-1.0),
            set("a"),
            run(5.0),
            Event::ClearReadiness,
            readiness(-2.0),
            set("b"),
            Event::ClearSets,
            set("c"),
            run(6.0),
            Event::ClearRuns,
            run(7.0),
            review(1),
            profile(20),
            review(2),
        ]);
    }

    #[test]
    fn replay_equivalence_when_a_clear_wipes_a_safety_hold() {
        // A Pain flag blocks training; ClearReadiness must lift it identically
        // whether or not the pre-clear inputs were compacted away.
        assert_replay_equivalent(vec![
            Event::SubmitReadiness(ReadinessInput {
                signal: ReadinessSignal::Pain,
                value: 1.0,
                observed_at: 0,
                streak: 0,
                pain: None,
                effort_min: None,
            }),
            Event::ClearReadiness,
            readiness(-1.5),
        ]);
    }

    #[test]
    fn replay_equivalence_preserves_run_spike_baseline() {
        // A run's longest_recent_km is derived from prior runs at update time;
        // compaction across a ClearRuns must not perturb the surviving runs'
        // spike baseline.
        assert_replay_equivalent(vec![
            run(20.0),
            run(5.0),
            Event::ClearRuns,
            run(8.0),
            run(30.0),
        ]);
    }

    fn predict(goal_distance_m: f64) -> Event {
        Event::PredictRace {
            recent_distance_m: 5000.0,
            recent_time_sec: 1200.0,
            goal_distance_m,
            weekly_km: 40.0,
            weeks_since_race: None,
        }
    }

    fn plan(muscle: &str) -> Event {
        Event::PlanHypertrophyMeso {
            muscle: muscle.into(),
            weeks: 4,
            not_growing: false,
            recovering_easily: false,
        }
    }

    fn protein(bodyweight_kg: f64) -> Event {
        Event::ComputeProtein {
            bodyweight_kg,
            masters: false,
            deficit: true,
        }
    }

    fn hr_zones(age_years: f64) -> Event {
        Event::ComputeHrZones {
            age_years,
            resting_hr_bpm: None,
            weeks_since_recalc: None,
            weeks_since_pace_test: None,
        }
    }

    #[test]
    fn only_the_last_race_prediction_survives() {
        // PredictRace is a last-write-wins singleton (family 5); an earlier query
        // is provably inert once a later one replaces model.race_query.
        let out = compact_event_log(vec![predict(10_000.0), plan("chest"), predict(21_097.5)]);
        assert_eq!(out, vec![plan("chest"), predict(21_097.5)]);
    }

    #[test]
    fn only_the_last_protein_target_survives() {
        // ComputeProtein is a last-write-wins singleton (family 7); an earlier
        // query is provably inert once a later one replaces model.protein_query.
        let out = compact_event_log(vec![protein(70.0), plan("chest"), protein(90.0)]);
        assert_eq!(out, vec![plan("chest"), protein(90.0)]);
    }

    #[test]
    fn clear_wipes_the_protein_family() {
        // A trailing ClearProtein (family 7) supersedes every ComputeProtein
        // before it, leaving nothing to replay in that family.
        let out = compact_event_log(vec![protein(70.0), protein(90.0), Event::ClearProtein]);
        assert!(out.is_empty());
    }

    #[test]
    fn clear_wipes_the_hypertrophy_plan_family() {
        // A trailing ClearHypertrophyPlan (family 6) supersedes every plan before
        // it, leaving nothing to replay in that family.
        let out = compact_event_log(vec![
            plan("chest"),
            plan("back"),
            Event::ClearHypertrophyPlan,
        ]);
        assert!(out.is_empty());
    }

    #[test]
    fn only_the_last_hr_zone_query_survives() {
        // ComputeHrZones is a last-write-wins singleton (family 8); an earlier
        // query is provably inert once a later one replaces model.hr_zone_query.
        let out = compact_event_log(vec![hr_zones(30.0), plan("chest"), hr_zones(40.0)]);
        assert_eq!(out, vec![plan("chest"), hr_zones(40.0)]);
    }

    #[test]
    fn clear_wipes_the_hr_zone_family() {
        // A trailing ClearHrZones (family 8) supersedes every ComputeHrZones
        // before it, leaving nothing to replay in that family.
        let out = compact_event_log(vec![hr_zones(30.0), hr_zones(40.0), Event::ClearHrZones]);
        assert!(out.is_empty());
    }

    #[test]
    fn replay_equivalence_across_new_singleton_families() {
        // Race prediction (5) and hypertrophy plan (6) interleaved with their
        // clears must replay identically before and after compaction.
        assert_replay_equivalent(vec![
            predict(10_000.0),
            plan("chest"),
            predict(21_097.5),
            Event::ClearRacePrediction,
            plan("back"),
            predict(42_195.0),
        ]);
    }
}
