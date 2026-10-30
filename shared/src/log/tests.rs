use super::*;
use crate::DAY_SEC;
use crate::app::tests::base_profile;
use crate::app::{Engine, Model, Profile, SessionReview};
use crate::running::GoalDistance;
use crate::schema::{ReadinessInput, ReadinessSignal};
use crux_core::App;

fn set(exercise: &str) -> Event {
    Event::LogSet {
        hold_sec: None,
        exercise: exercise.into(),
        weight_kg: 100.0,
        reps: 5,
        rpe: 8.0,
        observed_at: 0,
        entry_id: 0,
    }
}

fn run(distance_km: f64) -> Event {
    Event::LogRun {
        distance_km,
        duration_min: distance_km * 5.0,
        hr_pct_max: 75.0,
        longest_recent_km: 0.0,
        observed_at: 0,
        entry_id: 0,
        workout_type: None,
        session_rpe: None,
        avg_hr_bpm: None,
        utc_offset_sec: None,
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
        goal_distance: GoalDistance::TenK,
        weekly_sets,
        running_days_per_week: 4,
        running_km_per_week: 45.0,
        ..base_profile()
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

fn session_review(bad_day: bool) -> Event {
    Event::SubmitReview(SessionReview {
        bad_day,
        ..SessionReview::default()
    })
}

fn weekly(overtraining: u8) -> Event {
    Event::SubmitWeeklyCheckin(SessionReview {
        overtraining_signal_count: overtraining,
        ..SessionReview::default()
    })
}

#[test]
fn clear_review_keeps_earlier_reviews_and_only_a_trailing_clear_survives() {
    let events = vec![review(1), Event::ClearReview, review(2), Event::ClearReview];
    assert_eq!(
        compact_event_log(events.clone()),
        vec![review(1), review(2), Event::ClearReview]
    );
    assert_replay_equivalent(events);

    // A clear followed by a later SESSION review is overwritten by it: inert.
    let events = vec![
        session_review(true),
        Event::ClearReview,
        session_review(false),
    ];
    assert_eq!(
        compact_event_log(events.clone()),
        vec![session_review(true), session_review(false)]
    );
    assert_replay_equivalent(events);

    // A weekly-only payload does not replace the session review, so the
    // clear before it stays load-bearing.
    let events = vec![session_review(true), Event::ClearReview, review(2)];
    assert_eq!(compact_event_log(events.clone()), events);
    assert_replay_equivalent(events);
}

#[test]
fn clear_weekly_checkin_is_dropped_only_when_a_later_weekly_line_replaces_it() {
    // A later weekly check-in (new event or legacy weekly-carrying review)
    // replaces the store: the clear is inert.
    let events = vec![weekly(2), Event::ClearWeeklyCheckin, weekly(0)];
    assert_eq!(
        compact_event_log(events.clone()),
        vec![weekly(2), weekly(0)]
    );
    assert_replay_equivalent(events);
    let events = vec![weekly(2), Event::ClearWeeklyCheckin, review(1)];
    assert_eq!(
        compact_event_log(events.clone()),
        vec![weekly(2), review(1)]
    );
    assert_replay_equivalent(events);
    // A session review never replaces the weekly check-in: the clear stays.
    let events = vec![weekly(2), Event::ClearWeeklyCheckin, session_review(true)];
    assert_eq!(compact_event_log(events.clone()), events);
    assert_replay_equivalent(events);
    // And a ClearReview never stands in for a ClearWeeklyCheckin.
    let events = vec![weekly(2), Event::ClearWeeklyCheckin, Event::ClearReview];
    assert_eq!(compact_event_log(events.clone()), events);
    assert_replay_equivalent(events);
}

/// A Monday (`mon0_weekday == 0`) with a positive unix timestamp.
const PLAN_MON: i64 = 20_353;

/// A plan starting on [`PLAN_MON`], viewed on `today`, with a Back Squat
/// logged the day before so the plan's squat carries a load.
fn planned_week(today: i64) -> Vec<Event> {
    vec![
        profile(10),
        Event::LogSet {
            hold_sec: None,
            exercise: "Back Squat".into(),
            weight_kg: 120.0,
            reps: 3,
            rpe: 8.0,
            observed_at: (PLAN_MON - 1) * DAY_SEC,
            entry_id: 1,
        },
        Event::GeneratePlan {
            start_epoch_day: PLAN_MON,
        },
        Event::SetToday {
            epoch_day: today,
            utc_offset_sec: 0,
        },
    ]
}

/// Monday's week-strip row of the replayed view.
fn monday_row(events: &[Event]) -> crate::app::SessionPlanView {
    replay(events)
        .week_plan
        .into_iter()
        .find(|s| s.epoch_day == PLAN_MON)
        .expect("Monday row")
}

#[test]
fn a_dated_remove_keeps_its_submit_for_the_day_it_shaped() {
    // Monday's head-cold downgrade (load-free, half volume), removed on
    // Tuesday: the input still shaped Monday, so neither line may go.
    let mut events = planned_week(PLAN_MON + 1);
    events.push(Event::SubmitReadiness(ReadinessInput {
        signal: ReadinessSignal::Illness,
        value: 1.0,
        observed_at: PLAN_MON * DAY_SEC + 3_600,
        streak: 0,
        pain: None,
        effort_min: None,
    }));
    events.push(remove_at(
        ReadinessSignal::Illness,
        (PLAN_MON + 1) * DAY_SEC + 3_600,
    ));
    assert_eq!(compact_event_log(events.clone()), events);
    assert_replay_equivalent(events.clone());
    // The pair is load-bearing: dropping it re-renders Monday.
    let without: Vec<Event> = events
        .iter()
        .filter(|e| !matches!(e, Event::SubmitReadiness(_) | Event::RemoveReadiness { .. }))
        .cloned()
        .collect();
    assert_ne!(monday_row(&events), monday_row(&without));
}

#[test]
fn a_dated_resolve_hold_keeps_every_report_and_an_undated_one_is_dropped() {
    // "I'm better": every active Pain report is retracted, so both submits
    // and the resolution survive; a later undated remove has nothing left to
    // pair with and drops alone. An undated resolution is a no-op.
    let resolve = |observed_at| Event::ResolveHold {
        signal: ReadinessSignal::Pain,
        observed_at,
    };
    let events = vec![pain(), pain(), resolve(500), remove(ReadinessSignal::Pain)];
    assert_eq!(
        compact_event_log(events.clone()),
        vec![pain(), pain(), resolve(500)]
    );
    assert_replay_equivalent(events);
    let undated = vec![pain(), resolve(0)];
    assert_eq!(compact_event_log(undated.clone()), vec![pain()]);
    assert_replay_equivalent(undated);
}

#[test]
fn a_later_review_and_a_clear_keep_an_earlier_deload_window() {
    // Sunday's weekly deload, then Tuesday's default session review and a
    // clear: Monday stays inside the Sunday deload window, so compaction
    // must keep the Sunday review.
    let mut events = planned_week(PLAN_MON + 2);
    let sunday_deload = Event::SubmitReview(SessionReview {
        rpe_load_gap_sessions: Some(2),
        observed_at: (PLAN_MON - 1) * DAY_SEC + 20 * 3_600,
        ..SessionReview::default()
    });
    events.push(sunday_deload.clone());
    events.push(Event::SubmitReview(SessionReview {
        observed_at: (PLAN_MON + 1) * DAY_SEC + 3_600,
        ..SessionReview::default()
    }));
    events.push(Event::ClearReview);
    assert_eq!(compact_event_log(events.clone()), events);
    assert_replay_equivalent(events.clone());
    let without: Vec<Event> = events
        .iter()
        .filter(|e| **e != sunday_deload)
        .cloned()
        .collect();
    assert_ne!(monday_row(&events), monday_row(&without));
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

fn pain() -> Event {
    Event::SubmitReadiness(ReadinessInput {
        signal: ReadinessSignal::Pain,
        value: 1.0,
        observed_at: 0,
        streak: 0,
        pain: None,
        effort_min: None,
    })
}

/// An undated (legacy) remove: deletes its input outright.
fn remove(signal: ReadinessSignal) -> Event {
    Event::RemoveReadiness {
        signal,
        observed_at: 0,
    }
}

/// A dated remove: retracts its input from `observed_at` on.
fn remove_at(signal: ReadinessSignal, observed_at: i64) -> Event {
    Event::RemoveReadiness {
        signal,
        observed_at,
    }
}

#[test]
fn remove_readiness_cancels_only_the_latest_matching_submit() {
    // Two pain reports, one undo: the earlier report must still replay
    // (and still hard-stop training).
    let events = vec![pain(), pain(), remove(ReadinessSignal::Pain)];
    let out = compact_event_log(events.clone());
    assert_eq!(out, vec![pain()]);
    assert_replay_equivalent(events);
    assert!(replay(&[pain()]).hold_reason.is_some());
}

#[test]
fn unmatched_remove_readiness_is_dropped_alone() {
    // A remove with no prior matching submit replays as a no-op: dropping
    // it (and nothing else) is replay-equivalent, also across a clear.
    let across_clear = vec![pain(), Event::ClearReadiness, remove(ReadinessSignal::Pain)];
    assert!(compact_event_log(across_clear.clone()).is_empty());
    assert_replay_equivalent(across_clear);

    let before_submit = vec![remove(ReadinessSignal::Pain), pain()];
    assert_eq!(compact_event_log(before_submit.clone()), vec![pain()]);
    assert_replay_equivalent(before_submit);
}

#[test]
fn replay_equivalence_preserves_run_spike_baseline() {
    // A run's card spike baseline is derived from the runs around it;
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
    }
}

#[test]
fn calculators_keep_the_last_query_and_a_clear_wipes_their_family() {
    for (first, last, clear) in [
        (
            predict(10_000.0),
            predict(21_097.5),
            Event::ClearRacePrediction,
        ),
        (plan("chest"), plan("back"), Event::ClearHypertrophyPlan),
        (protein(70.0), protein(90.0), Event::ClearProtein),
        (hr_zones(30.0), hr_zones(40.0), Event::ClearHrZones),
    ] {
        let out = compact_event_log(vec![first.clone(), set("a"), last.clone()]);
        assert_eq!(out, vec![set("a"), last.clone()]);
        assert!(compact_event_log(vec![first, last, clear]).is_empty());
    }
}

fn checkin(observed_at: i64) -> Event {
    Event::SubmitCheckin(crate::schema::CheckinInput {
        observed_at,
        sleep_quality: Some(3),
        soreness: Some(3),
        mood: Some(3),
        fatigue: None,
        stress: None,
        resting_hr_bpm: None,
        hrv_rmssd_ms: None,
    })
}

/// A resting-HR-ONLY check-in (no wellness items).
fn checkin_rhr(observed_at: i64, rhr: f64) -> Event {
    Event::SubmitCheckin(crate::schema::CheckinInput {
        observed_at,
        sleep_quality: None,
        soreness: None,
        mood: None,
        fatigue: None,
        stress: None,
        resting_hr_bpm: Some(rhr),
        hrv_rmssd_ms: None,
    })
}

#[test]
fn clear_checkins_only_touches_its_own_family() {
    // A ClearCheckins must not disturb readiness/sets/runs.
    let out = compact_event_log(vec![
        readiness(-1.0),
        set("a"),
        checkin(0),
        Event::ClearCheckins,
    ]);
    assert_eq!(out, vec![readiness(-1.0), set("a")]);
}

#[test]
fn a_clear_sweeps_only_the_checkins_before_it() {
    // Rule 1 is the only check-in reducer compaction applies.
    let out = compact_event_log(vec![
        checkin(0),
        checkin(DAY_SEC),
        Event::ClearCheckins,
        checkin(100 * DAY_SEC),
    ]);
    assert_eq!(out, vec![checkin(100 * DAY_SEC)]);
}

#[test]
fn checkins_are_never_windowed() {
    // An old baseline week, a sparse resting-HR week and a fresh burst two
    // months later: every check-in survives, however old.
    let mut events = Vec::new();
    for d in 0..8 {
        events.push(checkin(d * DAY_SEC));
        events.push(checkin_rhr(d * DAY_SEC + 3600, 50.0 + d as f64));
    }
    events.push(set_today(70));
    for d in 60..67 {
        events.push(checkin(d * DAY_SEC));
    }
    assert_eq!(compact_event_log(events.clone()), events);
    assert_replay_equivalent(events);
}

#[test]
fn a_clock_moved_back_after_a_launch_still_reads_the_old_checkins() {
    // The shell compacts on launch and keeps appending. A launch on day 70
    // (old week on days 0..=7, fresh burst on days 60..=66), then the clock
    // moves back to day 6: the old week is the newest reading on or before
    // today again, so the view reads it. A check-in window anchored on the
    // launch-time readings dropped it and rendered another baseline.
    // (The shape of fuzz seeds 0xb184bb3cf2143208 / 0x7cd3a85e3770ff5e.)
    let mut launch = Vec::new();
    for d in 0..8 {
        launch.push(checkin(d * DAY_SEC));
    }
    launch.push(set_today(70));
    for d in 60..67 {
        launch.push(checkin(d * DAY_SEC));
    }
    let later = [set_today(6)];
    let full: Vec<Event> = launch.iter().cloned().chain(later.clone()).collect();
    let mut resumed = compact_event_log(launch);
    resumed.extend(later);
    assert_eq!(replay(&full), replay(&resumed));
}

#[test]
fn replay_equivalence_with_checkins_interleaved() {
    assert_replay_equivalent(vec![
        checkin(0),
        readiness(-1.0),
        checkin(DAY_SEC),
        set("a"),
        Event::ClearCheckins,
        checkin(2 * DAY_SEC),
        run(5.0),
    ]);
}

fn generate_plan(start: i64) -> Event {
    Event::GeneratePlan {
        start_epoch_day: start,
    }
}

fn set_today(day: i64) -> Event {
    Event::SetToday {
        epoch_day: day,
        utc_offset_sec: 0,
    }
}

#[test]
fn session_moves_survive_keep_first_and_clear_with_the_plan() {
    let mv = |from: i64, to: i64| Event::MoveSession {
        from_epoch_day: from,
        to_epoch_day: to,
    };
    // Every move survives (they replay in order); only the duplicate
    // GeneratePlan is dropped.
    let out = compact_event_log(vec![generate_plan(1), mv(1, 2), generate_plan(8), mv(2, 3)]);
    assert_eq!(out, vec![generate_plan(1), mv(1, 2), mv(2, 3)]);
    // ClearPlan sweeps the moves with the plan (update clears them too); the
    // plan's GeneratePlan and the ClearPlan stay (the removed plan's span).
    let out = compact_event_log(vec![generate_plan(1), mv(1, 2), Event::ClearPlan]);
    assert_eq!(out, vec![generate_plan(1), Event::ClearPlan]);
}

#[test]
fn removed_plans_keep_their_span_and_a_no_op_clear_is_dropped() {
    // Each removed plan keeps its GeneratePlan and the ClearPlan that removed
    // it (`Model::past_plans`); a ClearPlan with no plan is a no-op, kept only
    // as the final "plan removed" record.
    let events = vec![
        generate_plan(1),
        Event::ClearPlan,
        Event::ClearPlan,
        generate_plan(2),
        Event::ClearPlan,
    ];
    let out = compact_event_log(events);
    let kept = vec![
        generate_plan(1),
        Event::ClearPlan,
        generate_plan(2),
        Event::ClearPlan,
    ];
    assert_eq!(out, kept);
    // A fixed point.
    assert_eq!(compact_event_log(out), kept);
    // A lone no-op ClearPlan is still the final record.
    assert_eq!(
        compact_event_log(vec![Event::ClearPlan]),
        vec![Event::ClearPlan]
    );
    let out = compact_event_log(vec![generate_plan(1), Event::ClearPlan, generate_plan(3)]);
    assert_eq!(
        out,
        vec![generate_plan(1), Event::ClearPlan, generate_plan(3)]
    );
    // Replay-equivalent with and without surrounding lines.
    assert_replay_equivalent(vec![
        profile(12),
        generate_plan(1),
        set_today(2),
        Event::ClearPlan,
        set_today(3),
    ]);
}

#[test]
fn replay_equivalence_across_plan_and_today_families() {
    assert_replay_equivalent(vec![
        profile(12),
        set_today(1),
        generate_plan(1),
        set_today(2),
        generate_plan(2),
        Event::ClearPlan,
        generate_plan(3),
        set_today(3),
    ]);
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

// ── Rule 3: DeleteEntry / AmendSet / AmendRun ──────────────

fn set_id(exercise: &str, id: u64, observed_at: i64) -> Event {
    Event::LogSet {
        hold_sec: None,
        exercise: exercise.into(),
        weight_kg: 100.0,
        reps: 5,
        rpe: 8.0,
        observed_at,
        entry_id: id,
    }
}

fn run_id(distance_km: f64, id: u64, observed_at: i64) -> Event {
    Event::LogRun {
        distance_km,
        duration_min: distance_km * 5.0,
        hr_pct_max: 75.0,
        longest_recent_km: 0.0,
        observed_at,
        entry_id: id,
        workout_type: None,
        session_rpe: None,
        avg_hr_bpm: None,
        utc_offset_sec: None,
    }
}

fn del_set(id: u64) -> Event {
    Event::DeleteEntry {
        kind: crate::app::EntryKind::Set,
        entry_id: id,
        observed_at_fallback: 0,
    }
}

fn del_run(id: u64) -> Event {
    Event::DeleteEntry {
        kind: crate::app::EntryKind::Run,
        entry_id: id,
        observed_at_fallback: 0,
    }
}

fn amend_set(id: u64, weight_kg: f64) -> Event {
    Event::AmendSet {
        hold_sec: None,
        entry_id: id,
        exercise: "Bench".into(),
        weight_kg,
        reps: 5,
        rpe: 8.0,
        observed_at: 0,
        observed_at_fallback: 0,
    }
}

#[test]
fn delete_skips_an_invalid_phantom_log_and_cancels_the_real_row() {
    // A valid legacy row, a phantom invalid LogSet sharing its (id 0, time), then
    // a DeleteEntry. `update` drops the invalid line (it never becomes a row) and
    // the delete removes the ONE real row. Compaction must pair the delete with
    // the VALID row, not the phantom — else the deleted set resurrects after
    // compaction. The inert invalid line survives (a no-op on replay).
    let valid = Event::LogSet {
        hold_sec: None,
        exercise: "Bench".into(),
        weight_kg: 100.0,
        reps: 5,
        rpe: 8.0,
        observed_at: 100,
        entry_id: 0,
    };
    let invalid = Event::LogSet {
        hold_sec: None,
        exercise: "Bench".into(),
        weight_kg: -5.0,
        reps: 5,
        rpe: 8.0,
        observed_at: 100,
        entry_id: 0,
    };
    let delete = Event::DeleteEntry {
        kind: crate::app::EntryKind::Set,
        entry_id: 0,
        observed_at_fallback: 100,
    };
    let events = vec![valid, invalid.clone(), delete];
    let out = compact_event_log(events.clone());
    assert_eq!(
        out,
        vec![invalid],
        "the delete cancels the real row; only the inert invalid line remains"
    );
    assert_replay_equivalent(events);
}

#[test]
fn amend_after_delete_never_resurrects_the_entry() {
    // Core prevention: Log→Delete→Amend, where the amend targets the row the
    // delete already removed. The delete cancels the entry (log + itself); the
    // amend then matches nothing and is dropped; the entry stays deleted, both
    // in a live `update` replay and after compaction.
    let events = vec![set_id("Bench", 5, 0), del_set(5), amend_set(5, 120.0)];
    // Live replay: the amend is a no-op on the missing row → nothing survives.
    assert!(
        replay(&events).lifts.is_empty(),
        "the deleted set is not resurrected"
    );
    // Compaction: the whole sequence is inert.
    assert!(
        compact_event_log(events.clone()).is_empty(),
        "delete cancels the entry; the orphan amend is dropped"
    );
    assert_replay_equivalent(events);

    // Same for a run, and with an unrelated survivor in between.
    let run_events = vec![
        run_id(5.0, 2, 0),
        run_id(8.0, 3, DAY_SEC),
        del_run(3),
        Event::AmendRun {
            entry_id: 3,
            distance_km: 9.0,
            duration_min: 45.0,
            hr_pct_max: 0.0,
            longest_recent_km: 0.0,
            observed_at: DAY_SEC,
            observed_at_fallback: 0,
            workout_type: None,
            session_rpe: None,
            avg_hr_bpm: None,
        },
    ];
    assert_eq!(
        compact_event_log(run_events.clone()),
        vec![run_id(5.0, 2, 0)],
        "only the unrelated run survives; the deleted run is not resurrected"
    );
    assert_replay_equivalent(run_events);
}

#[test]
fn unmatched_delete_is_dropped_alone() {
    // A delete with no matching prior line replays as a no-op → dropped, and
    // nothing else is touched.
    let before = vec![del_set(42), set_id("Bench", 42, 0)];
    assert_eq!(
        compact_event_log(before.clone()),
        vec![set_id("Bench", 42, 0)]
    );
    assert_replay_equivalent(before);

    let orphan = vec![run(5.0), del_set(1)];
    assert_eq!(compact_event_log(orphan.clone()), vec![run(5.0)]);
    assert_replay_equivalent(orphan);
}

#[test]
fn delete_across_a_clear_stays_replay_equivalent() {
    // The set is cleared before the delete ever runs; the delete is a no-op.
    // Rule 3 may match the pre-clear line, but Rule 1 sweeps it regardless.
    let events = vec![set_id("Bench", 4, 0), Event::ClearSets, del_set(4)];
    assert!(compact_event_log(events.clone()).is_empty());
    assert_replay_equivalent(events);

    // A re-logged post-clear set with the same id is the real delete target.
    let relog = vec![
        set_id("Bench", 4, 0),
        Event::ClearSets,
        set_id("Bench", 4, DAY_SEC),
        del_set(4),
    ];
    assert!(compact_event_log(relog.clone()).is_empty());
    assert_replay_equivalent(relog);
}

#[test]
fn legacy_row_without_id_is_matched_on_observed_at() {
    // A legacy set (entry_id 0) is deleted by its observed_at fallback.
    let events = vec![
        set_id("Old", 0, 5 * DAY_SEC),
        Event::DeleteEntry {
            kind: crate::app::EntryKind::Set,
            entry_id: 0,
            observed_at_fallback: 5 * DAY_SEC,
        },
    ];
    assert!(compact_event_log(events.clone()).is_empty());
    assert_replay_equivalent(events);
}

#[test]
fn run_amend_keeps_its_base_log() {
    // The strict amend replaces the base row on replay, so both survive.
    let amended = vec![
        run_id(5.0, 6, 0),
        Event::AmendRun {
            entry_id: 6,
            distance_km: 6.0,
            duration_min: 30.0,
            hr_pct_max: 0.0,
            longest_recent_km: 0.0,
            observed_at: 0,
            observed_at_fallback: 0,
            workout_type: None,
            session_rpe: None,
            avg_hr_bpm: None,
        },
    ];
    assert_eq!(compact_event_log(amended.clone()), amended);
    assert_replay_equivalent(amended);
}

// ── Rule 3 + Rule 1: TrimRunTrack ──────────────────────────

fn run_track_id(id: u64, observed_at: i64) -> Event {
    Event::LogRunTrack {
        points: vec![
            crate::running::GpsPoint {
                lat: 0.0,
                lon: 0.000,
                observed_at: 0,
                accuracy_m: 5.0,
            },
            crate::running::GpsPoint {
                lat: 0.0,
                lon: 0.005,
                observed_at: 100,
                accuracy_m: 5.0,
            },
            crate::running::GpsPoint {
                lat: 0.0,
                lon: 0.010,
                observed_at: 200,
                accuracy_m: 5.0,
            },
            crate::running::GpsPoint {
                lat: 0.0,
                lon: 0.015,
                observed_at: 300,
                accuracy_m: 5.0,
            },
            crate::running::GpsPoint {
                lat: 0.0,
                lon: 0.020,
                observed_at: 400,
                accuracy_m: 5.0,
            },
        ],
        hr_pct_max: 0.0,
        longest_recent_km: 0.0,
        observed_at,
        entry_id: id,
        workout_type: None,
        session_rpe: None,
        segment_starts: Vec::new(),
        avg_hr_bpm: None,
        utc_offset_sec: None,
    }
}

fn trim_run(id: u64, observed_at_fallback: i64, from: i64, until: i64) -> Event {
    Event::TrimRunTrack {
        entry_id: id,
        observed_at_fallback,
        keep_from_sec: from,
        keep_until_sec: until,
    }
}

#[test]
fn trim_lines_survive_compaction_alongside_the_base_log() {
    // Base LogRunTrack + two trims (no delete/clear): every trim line survives
    // and the base stays alive — replay is sequential slicing, nothing inert.
    let events = vec![
        run_track_id(7_200_001, 0),
        trim_run(7_200_001, 0, 0, 300),
        trim_run(7_200_001, 0, 100, 300),
    ];
    assert_eq!(
        compact_event_log(events.clone()),
        events,
        "no trim line is dropped"
    );
    assert_replay_equivalent(events);
}

#[test]
fn delete_of_a_trimmed_run_replays_to_nothing() {
    // A DeleteEntry cancels the base LogRunTrack AND its trims on replay, so the
    // delete's chain-walk drops base + trims + delete entirely. This pins the
    // delete chain-walk: without walking THROUGH the trims to the base log,
    // compaction would drop the delete+trim while resurrecting the untrimmed
    // base, and replay would diverge (a run would reappear).
    assert!(
        compact_event_log(vec![
            run_track_id(7_200_002, 0),
            trim_run(7_200_002, 0, 100, 300),
            del_run(7_200_002),
        ])
        .is_empty(),
        "delete of a trimmed run drops base + trim + delete"
    );
    assert_replay_equivalent(vec![
        run_track_id(7_200_002, 0),
        trim_run(7_200_002, 0, 100, 300),
        del_run(7_200_002),
    ]);
    assert_replay_equivalent(vec![
        run_track_id(7_200_003, 0),
        trim_run(7_200_003, 0, 0, 300),
        trim_run(7_200_003, 0, 100, 300),
        del_run(7_200_003),
    ]);
}

#[test]
fn clear_runs_wipes_a_base_and_its_trims() {
    let events = vec![
        run_track_id(7_200_004, 0),
        trim_run(7_200_004, 0, 100, 300),
        Event::ClearRuns,
    ];
    assert!(
        compact_event_log(events.clone()).is_empty(),
        "ClearRuns wipes the base log and its trims"
    );
    assert_replay_equivalent(events);
}

#[test]
fn a_run_delete_drops_its_pair_across_other_run_lines() {
    // No run carries another's distance (the card spike baseline is derived
    // from the surviving runs), so another run's log and trim between a run
    // and its delete do not hold the pair back.
    let events = vec![
        run_track_id(7_200_005, 0),
        run_id(5.0, 7_200_006, DAY_SEC),
        trim_run(7_200_006, DAY_SEC, 100, 300),
        del_run(7_200_005),
    ];
    assert_eq!(compact_event_log(events.clone()), events[1..3].to_vec());
    assert_replay_equivalent(events);
}

#[test]
fn an_orphan_trim_survives_compaction_alone() {
    // A trim is NEVER a Rule-3 trigger, so — unlike an unmatched amend/delete,
    // which are dropped alone — an orphan trim (no surviving base log) is left
    // in place and replays as an inert no-op. Pins the "not a trigger" face.
    let lone = vec![trim_run(7_200_007, 0, 100, 300)];
    assert_eq!(
        compact_event_log(lone.clone()),
        lone,
        "a lone trim is not dropped"
    );
    assert_replay_equivalent(lone);
    // A trim trailing its base's delete is likewise inert and replay-equivalent.
    assert_replay_equivalent(vec![
        run_track_id(7_200_008, 0),
        del_run(7_200_008),
        trim_run(7_200_008, 0, 100, 300),
    ]);
}

fn meta_run(id: u64, observed_at_fallback: i64, rpe: Option<u8>) -> Event {
    Event::AmendRunMeta {
        entry_id: id,
        observed_at_fallback,
        session_rpe: rpe,
        workout_type: Some(crate::schema::WorkoutType::Tempo),
    }
}

#[test]
fn amend_run_meta_round_trips_through_compaction() {
    // A GPS run rated + tagged later, then re-rated: every meta line survives
    // with its base log (in-place edits, never telescoped), and the compacted
    // log replays to the same rated, tagged run.
    let events = vec![
        run_track_id(7_300_001, 0),
        meta_run(7_300_001, 0, Some(6)),
        meta_run(7_300_001, 0, Some(7)),
    ];
    let compacted = compact_event_log(events.clone());
    assert_eq!(compacted, events, "no meta line is dropped");
    let view = replay(&compacted);
    assert_eq!(view.runs[0].session_rpe, Some(7));
    assert_eq!(
        view.runs[0].workout_type,
        Some(crate::schema::WorkoutType::Tempo)
    );
    assert_replay_equivalent(events);
    // A minimal wire line decodes with defaults for the optional fields.
    let minimal: Event =
        serde_json::from_str(r#"{"AmendRunMeta":{"entry_id":5}}"#).expect("minimal meta");
    assert!(matches!(
        minimal,
        Event::AmendRunMeta {
            observed_at_fallback: 0,
            session_rpe: None,
            workout_type: None,
            ..
        }
    ));
}

#[test]
fn delete_of_a_rated_run_drops_base_and_meta() {
    let events = vec![
        run_track_id(7_300_002, 0),
        meta_run(7_300_002, 0, Some(5)),
        del_run(7_300_002),
    ];
    assert!(compact_event_log(events.clone()).is_empty());
    assert_replay_equivalent(events);
    // ClearRuns sweeps a meta line like any run-family line.
    let cleared = vec![
        run_id(5.0, 7_300_003, DAY_SEC),
        meta_run(7_300_003, DAY_SEC, Some(3)),
        Event::ClearRuns,
    ];
    assert!(compact_event_log(cleared.clone()).is_empty());
    assert_replay_equivalent(cleared);
    // An amend after a meta edit stays replay-equivalent.
    assert_replay_equivalent(vec![
        run_id(5.0, 7_300_004, DAY_SEC),
        meta_run(7_300_004, DAY_SEC, Some(3)),
        Event::AmendRun {
            entry_id: 7_300_004,
            distance_km: 6.0,
            duration_min: 30.0,
            hr_pct_max: 0.0,
            longest_recent_km: 0.0,
            observed_at: DAY_SEC,
            observed_at_fallback: DAY_SEC,
            workout_type: None,
            session_rpe: Some(6),
            avg_hr_bpm: None,
        },
    ]);
}

// A legacy (entry_id 0) set log + amend, matched on observed_at.
fn legacy_set(observed_at: i64, weight_kg: f64) -> Event {
    Event::LogSet {
        hold_sec: None,
        exercise: "Bench".into(),
        weight_kg,
        reps: 5,
        rpe: 8.0,
        observed_at,
        entry_id: 0,
    }
}

fn legacy_amend(observed_at: i64, observed_at_fallback: i64, weight_kg: f64) -> Event {
    Event::AmendSet {
        hold_sec: None,
        entry_id: 0,
        exercise: "Bench".into(),
        weight_kg,
        reps: 5,
        rpe: 8.0,
        observed_at,
        observed_at_fallback,
    }
}

#[test]
fn rule3_walk_assumes_legacy_rows_are_never_redated() {
    // Regression guard (latent, shell-blocked). Rule 3's chain-walk keys a
    // whole legacy (entry_id 0) set/run chain by ONE (fam, id, observed_at)
    // match key: correct ONLY while a legacy row's date is immutable, which
    // the shell enforces (LogEntry.kt pins observed_at == observed_at_fallback
    // for id-0 rows, blocking re-dating). This guard pins that invariant: if a
    // future change lets legacy rows be re-dated, the `assert_ne!`s below start
    // failing and force revisiting Rule 3 and the shell pin TOGETHER.

    // PINNED (supported): every link shares one date (observed_at ==
    // observed_at_fallback == DAY_SEC) → the legacy chain collapses
    // replay-equivalently, exactly like an id-bearing one.
    assert_replay_equivalent(vec![
        legacy_set(DAY_SEC, 100.0),
        legacy_amend(DAY_SEC, DAY_SEC, 110.0),
        legacy_amend(DAY_SEC, DAY_SEC, 130.0),
    ]);

    // RE-DATED Log→Amend→Amend (unsupported): each amend moves the row's date,
    // so the single-key walk telescopes the middle re-date link away and the
    // final amend then matches nothing on replay → reverts to the ORIGINAL
    // value after compaction. The full and compacted views MUST differ today.
    let redated_chain = vec![
        legacy_set(DAY_SEC, 100.0),
        legacy_amend(2 * DAY_SEC, DAY_SEC, 110.0), // re-date DAY_SEC → 2*DAY_SEC
        legacy_amend(3 * DAY_SEC, 2 * DAY_SEC, 130.0), // re-date 2*DAY_SEC → 3*DAY_SEC
    ];
    assert_ne!(
        replay(&redated_chain),
        replay(&compact_event_log(redated_chain.clone())),
        "legacy re-dating became replay-safe - update Rule 3 AND remove the \
         LogEntry.kt pin together (see the legacy-row invariant comment)"
    );

    // RE-DATED Log→Amend→Delete (unsupported): the delete's fallback matches
    // the re-dated amend but its base-chain walk (same key) can't reach the
    // differently-dated base log, so the base log survives compaction and the
    // deleted row is RESURRECTED. Full and compacted views MUST differ today.
    let redated_delete = vec![
        legacy_set(DAY_SEC, 100.0),
        legacy_amend(2 * DAY_SEC, DAY_SEC, 110.0), // re-date DAY_SEC → 2*DAY_SEC
        Event::DeleteEntry {
            kind: crate::app::EntryKind::Set,
            entry_id: 0,
            observed_at_fallback: 2 * DAY_SEC, // delete the re-dated (current) row
        },
    ];
    assert_ne!(
        replay(&redated_delete),
        replay(&compact_event_log(redated_delete.clone())),
        "legacy re-dating became replay-safe - update Rule 3 AND remove the \
         LogEntry.kt pin together (see the legacy-row invariant comment)"
    );
}

#[test]
fn delete_and_amend_interleaved_with_everything_replay_equivalent() {
    assert_replay_equivalent(vec![
        profile(10),
        set_id("Bench", 1, 0),
        run_id(5.0, 2, 0),
        amend_set(1, 120.0),
        set_id("Squat", 3, DAY_SEC),
        del_set(3),
        run_id(8.0, 4, DAY_SEC),
        del_run(2),
        Event::ClearSets,
        set_id("Deadlift", 5, 2 * DAY_SEC),
        amend_set(5, 200.0),
    ]);
}

// ── Family 15: logged other activities ──

fn act(minutes: f64, id: u64, observed_at: i64) -> Event {
    Event::LogActivity {
        kind: crate::app::ActivityKind::Sport,
        minutes,
        vigorous: true,
        strengthening: false,
        observed_at,
        entry_id: id,
    }
}

fn del_act(id: u64, observed_at_fallback: i64) -> Event {
    Event::DeleteEntry {
        kind: crate::app::EntryKind::Activity,
        entry_id: id,
        observed_at_fallback,
    }
}

#[test]
fn activities_form_their_own_family() {
    let family = |e: &Event| LineFacts::of(e).family();
    assert_eq!(family(&act(30.0, 1, 0)), Some((Family::Activities, false)));
    assert_eq!(
        family(&Event::ClearActivities),
        Some((Family::Activities, true))
    );
    assert_eq!(family(&del_act(1, 0)), Some((Family::Activities, false)));
}

#[test]
fn activity_delete_cancels_its_log_pairwise() {
    // A run between the pair does not block it: activities bake no state.
    let events = vec![act(30.0, 7, 0), run(5.0), del_act(7, 0)];
    assert_eq!(compact_event_log(events.clone()), vec![run(5.0)]);
    assert_replay_equivalent(events);
}

#[test]
fn activity_delete_targets_the_newest_match_and_legacy_rows_by_time() {
    let events = vec![
        act(30.0, 3, 0),
        act(40.0, 3, DAY_SEC),
        act(50.0, 0, 2 * DAY_SEC),
        act(60.0, 0, 3 * DAY_SEC),
        del_act(3, 0),
        del_act(0, 2 * DAY_SEC),
    ];
    assert_eq!(
        compact_event_log(events.clone()),
        vec![act(30.0, 3, 0), act(60.0, 0, 3 * DAY_SEC)]
    );
    assert_replay_equivalent(events);
}

#[test]
fn unmatched_activity_delete_is_dropped_alone_and_skips_invalid_logs() {
    // The invalid line never became a row, so the delete matches nothing.
    let events = vec![act(0.0, 4, 0), act(30.0, 5, 0), del_act(4, 0), del_set(5)];
    assert_eq!(
        compact_event_log(events.clone()),
        vec![act(0.0, 4, 0), act(30.0, 5, 0)],
        "a set delete never pairs with an activity of the same id"
    );
    assert_replay_equivalent(events);
}

#[test]
fn clear_activities_sweeps_only_its_family() {
    let events = vec![
        act(30.0, 1, 0),
        set_id("Bench", 1, 0),
        del_act(9, 0),
        run_id(5.0, 1, 0),
        Event::ClearActivities,
        act(45.0, 2, DAY_SEC),
    ];
    assert_eq!(
        compact_event_log(events.clone()),
        vec![
            set_id("Bench", 1, 0),
            run_id(5.0, 1, 0),
            act(45.0, 2, DAY_SEC)
        ]
    );
    assert_replay_equivalent(events);
    assert!(compact_event_log(vec![act(30.0, 1, 0), Event::ClearActivities]).is_empty());
}

#[test]
fn activities_survive_other_families_clears() {
    let events = vec![
        act(30.0, 1, 0),
        Event::ClearSets,
        Event::ClearRuns,
        Event::ClearCheckins,
        Event::ClearReadiness,
        act(20.0, 2, DAY_SEC),
    ];
    assert_eq!(
        compact_event_log(events.clone()),
        vec![act(30.0, 1, 0), act(20.0, 2, DAY_SEC)]
    );
    assert_replay_equivalent(events);
}

#[test]
fn activities_interleaved_with_everything_replay_equivalent() {
    // An under-18 profile with today set, so the WHO activity card (which
    // reads runs, sets and activities) is part of the compared view.
    let youth = Event::SetProfile(Profile {
        age_years: Some(15.0),
        ..base_profile()
    });
    let monday = 4 + 7 * 2_900;
    let t = |d: i64| (monday + d) * DAY_SEC + 12 * 3_600;
    assert_replay_equivalent(vec![
        youth,
        act(30.0, 1, t(0)),
        set_id("Bench", 2, t(1)),
        set_id("Bench", 3, t(1) + 1_800),
        run_id(5.0, 4, t(2)),
        act(45.0, 5, t(3)),
        del_act(1, 0),
        act(f64::NAN, 6, t(4)),
        del_set(3),
        act(25.0, 7, t(4)),
        Event::ClearActivities,
        act(35.0, 8, t(5)),
        del_act(6, 0),
        act(15.0, 0, t(6)),
        del_run(4),
        set_today(monday + 2),
    ]);
}

// ── Rule 2: SetToday ──

#[test]
fn set_today_keeps_every_offset_change_and_the_last_day() {
    let today = |epoch_day, utc_offset_sec| Event::SetToday {
        epoch_day,
        utc_offset_sec,
    };
    let events = vec![
        today(1, 0),
        today(2, 3_600),
        today(3, 3_600),
        today(4, 0),
        today(5, 0),
    ];
    assert_eq!(
        compact_event_log(events),
        vec![today(2, 3_600), today(4, 0), today(5, 0)]
    );
}

/// A legacy run row (no `utc_offset_sec`) converts its typed bpm under the
/// clock offset in force when it is ingested, and a later undated SetProfile
/// pins that figure, so the SetToday that set the offset must survive.
#[test]
fn compaction_keeps_a_pinned_legacy_runs_hr_figure() {
    const D: i64 = 20_000;
    let dated = |eff: i64, edited_at: i64, hr_max: f64| {
        Event::SetProfile(Profile {
            measured_hr_max: Some(hr_max),
            effective_from_epoch_day: Some(eff),
            edited_at: Some(edited_at),
            ..base_profile()
        })
    };
    let today = |epoch_day| Event::SetToday {
        epoch_day,
        utc_offset_sec: 3_600,
    };
    let events = vec![
        today(D + 1),
        // HRmax 200 from day D, 160 from day D+1 (edited the day before).
        dated(D, D * DAY_SEC - 100, 200.0),
        dated(D + 1, D * DAY_SEC, 160.0),
        // 23:30 UTC on day D = 00:30 local on day D+1 at UTC+1.
        Event::LogRun {
            distance_km: 8.0,
            duration_min: 48.0,
            hr_pct_max: 0.0,
            longest_recent_km: 0.0,
            observed_at: (D + 1) * DAY_SEC - 1_800,
            entry_id: 1,
            workout_type: None,
            session_rpe: None,
            avg_hr_bpm: Some(150.0),
            utc_offset_sec: None,
        },
        // A legacy undated edit pins the run's figure.
        Event::SetProfile(Profile {
            measured_hr_max: Some(190.0),
            ..base_profile()
        }),
        today(D + 2),
    ];
    let full = replay(&events);
    let compacted = replay(&compact_event_log(events));
    assert_eq!(full.runs[0].hr_pct_max, compacted.runs[0].hr_pct_max);
    assert_eq!(full, compacted);
}

// ── Raw wire lines (the shell's JNI path) ──

fn wire(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .map(|e| serde_json::to_string(e).unwrap())
        .collect()
}

fn compact_wire(lines: &[String]) -> Compaction {
    compact_lines(
        &lines
            .iter()
            .map(|l| LineFacts::parse(l))
            .collect::<Vec<_>>(),
    )
}

#[test]
fn wire_lines_compact_like_their_events() {
    let events = vec![
        set_id("Bench", 900, 0),
        run_id(5.0, 12, 0),
        Event::ClearSets,
        set_id("Squat", 5, DAY_SEC),
        Event::GeneratePlan { start_epoch_day: 1 },
        Event::ClearPlan,
    ];
    let out = compact_wire(&wire(&events));
    assert_eq!(out.dropped, vec![0, 2]);
    assert!(out.torn.is_empty());
    assert_eq!(out.max_entry_id, 900, "an id compacted away still counts");
    assert!(out.plan_removed);
}

#[test]
fn a_torn_line_is_reported_and_never_removed_by_a_rule() {
    let mut lines = vec![r#"{"LogSet":{"exercise":"Bench","weigh"#.to_owned()];
    lines.extend(wire(&[set("a"), Event::ClearSets]));
    let out = compact_wire(&lines);
    assert_eq!(out.torn, vec![0]);
    assert_eq!(out.dropped, vec![1, 2]);
}

#[test]
fn unknown_and_malformed_lines_are_kept_and_never_act_as_a_clear() {
    // A newer build's events, and a unit variant given a body (which the core
    // rejects on replay, so it must not wipe the sets).
    let mut lines = wire(&[set("a")]);
    lines.extend(
        [
            r#"{"FutureStructEvent":{"weeks":6,"entry_id":77}}"#,
            r#""FutureUnitEvent""#,
            r#"{"ClearSets":{}}"#,
        ]
        .map(str::to_owned),
    );
    let out = compact_wire(&lines);
    assert_eq!(
        out,
        Compaction {
            max_entry_id: 77,
            ..Compaction::default()
        }
    );
}

#[test]
fn retired_calculator_lines_are_dropped() {
    let lines = [
        r#"{"ComputeCooper":{"distance_m_12min":2400.0}}"#,
        r#""ClearCriticalSpeed""#,
        r#"{"ComputeApre":{"scheme":"Apre6","reps":4,"current_load_kg":100.0}}"#,
    ]
    .map(str::to_owned);
    assert_eq!(compact_wire(&lines).dropped, vec![0, 1, 2]);
}

/// A Turkish keyboard's "İNCLİNE BENCH PRESS" / "Inclıne bench press" is the same
/// `SetExerciseEnded` key as "Incline bench press" (`names::exercise_key`), as
/// `update` treats it (`plan::same_movement`): the later marker replaces the
/// earlier one, and the compacted log replays to the same view.
#[test]
fn turkish_keyboard_spellings_share_one_ended_marker_key() {
    let ended = |exercise: &str, ended: bool, at: i64| Event::SetExerciseEnded {
        exercise: exercise.into(),
        epoch_day: 700,
        ended,
        observed_at: at,
    };
    let events = vec![
        set("Incline bench press"),
        ended("Incline bench press", true, 700 * DAY_SEC + 10),
        ended("İNCLİNE BENCH PRESS", false, 700 * DAY_SEC + 20),
        ended("Inclıne bench press", true, 700 * DAY_SEC + 30),
    ];
    assert_eq!(
        compact_event_log(events.clone()),
        vec![events[0].clone(), events[3].clone()]
    );
    assert_replay_equivalent(events);
}
