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

fn profile_dated(weekly_sets: u8, eff: Option<i64>) -> Event {
    match profile(weekly_sets) {
        Event::SetProfile(mut p) => {
            p.effective_from_epoch_day = eff;
            Event::SetProfile(p)
        }
        _ => unreachable!(),
    }
}

fn ended(exercise: &str, epoch_day: i64, ended: bool) -> Event {
    Event::SetExerciseEnded {
        exercise: exercise.into(),
        epoch_day,
        ended,
        observed_at: 1_758_600_000,
    }
}

#[test]
fn dated_profiles_keep_the_history_subsequence() {
    // update()'s profile_history keeps [eff10, eff15] from
    // [eff10, eff20, eff15] (eff20 is dropped when eff15 is applied), so
    // compaction must keep exactly that subsequence — not just the last line.
    let out = compact_event_log(vec![
        profile_dated(10, Some(10)),
        profile_dated(20, Some(20)),
        profile_dated(30, Some(15)),
    ]);
    assert_eq!(
        out,
        vec![profile_dated(10, Some(10)), profile_dated(30, Some(15))]
    );
}

#[test]
fn ascending_dated_profiles_all_survive() {
    // A later profile with a HIGHER eff never supersedes the earlier one; a
    // plain keep-last singleton would wrongly drop the eff10 line.
    let out = compact_event_log(vec![
        profile_dated(10, Some(10)),
        profile_dated(20, Some(20)),
    ]);
    assert_eq!(
        out,
        vec![profile_dated(10, Some(10)), profile_dated(20, Some(20))]
    );
}

#[test]
fn none_dated_profile_replaces_the_whole_history() {
    // A None-dated (-inf) edit drops every prior entry (eff >= -inf).
    let out = compact_event_log(vec![profile_dated(10, Some(10)), profile_dated(20, None)]);
    assert_eq!(out, vec![profile_dated(20, None)]);
}

#[test]
fn undated_profiles_still_keep_last() {
    // Back-compat: two undated (None = -inf) profiles, the earlier is dead.
    let out = compact_event_log(vec![profile(10), profile(20)]);
    assert_eq!(out, vec![profile(20)]);
}

#[test]
fn set_exercise_ended_last_write_wins_by_exact_key() {
    // Same (exercise.trim().lowercase(), epoch_day) key → only the last
    // marker survives (here a later reopen).
    let out = compact_event_log(vec![
        ended("Back Squat", 20354, true),
        ended("back squat", 20354, false),
    ]);
    assert_eq!(out, vec![ended("back squat", 20354, false)]);
}

#[test]
fn set_exercise_ended_keeps_distinct_keys_and_equipment_variants() {
    // A different epoch_day, or a raw string differing by equipment words, is a
    // DIFFERENT exact key, so both survive (conservative: update() might collapse
    // the equipment variant, but keeping a would-be-overwritten line is inert).
    let events = vec![
        ended("Back Squat", 20354, true),
        ended("Barbell Back Squat", 20354, true),
        ended("Back Squat", 20355, true),
    ];
    assert_eq!(compact_event_log(events.clone()), events);
}

#[test]
fn clear_sets_sweeps_earlier_ended_markers() {
    // SetExerciseEnded is family 1: a later ClearSets drops prior markers (Rule 1).
    let out = compact_event_log(vec![
        ended("Bench", 20354, true),
        Event::ClearSets,
        ended("Bench", 20354, true),
    ]);
    assert_eq!(out, vec![ended("Bench", 20354, true)]);
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
fn every_review_survives() {
    // Each weekly check-in is a `weekly_history` entry past days are judged
    // against.
    let events = vec![review(1), review(2), review(3)];
    assert_eq!(compact_event_log(events.clone()), events);
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

fn soreness_at(value: f64, observed_at: i64) -> Event {
    Event::SubmitReadiness(ReadinessInput {
        signal: ReadinessSignal::Soreness,
        value,
        observed_at,
        streak: 0,
        pain: None,
        effort_min: None,
    })
}

#[test]
fn a_dated_remove_keeps_its_submit_for_the_day_it_shaped() {
    // Monday's head-cold downgrade (load-free, half volume), removed on
    // Tuesday: the input still shaped Monday, so neither line may go.
    // (A soreness report now only holds the top end, which leaves a
    // missed day's numbers unchanged, so it can no longer pin this.)
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
fn an_undated_remove_skips_the_submit_a_dated_remove_retracted() {
    // The dated remove retracts the newer soreness; the undated one then
    // deletes the older, so only the older line and its remove go.
    let older = soreness_at(6.0, 100);
    let newer = soreness_at(5.0, 200);
    let dated = remove_at(ReadinessSignal::Soreness, 300);
    let events = vec![
        older,
        newer.clone(),
        dated.clone(),
        remove(ReadinessSignal::Soreness),
    ];
    assert_eq!(compact_event_log(events.clone()), vec![newer, dated]);
    assert_replay_equivalent(events);
}

#[test]
fn an_unmatched_dated_remove_is_dropped_alone() {
    let events = vec![remove_at(ReadinessSignal::Pain, 300), pain()];
    assert_eq!(compact_event_log(events.clone()), vec![pain()]);
    assert_replay_equivalent(events);
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
fn remove_readiness_cancels_its_submit_pairwise() {
    // The mis-tap undo: pain + its removal are inert; the wellness input
    // logged in between must survive untouched.
    let out = compact_event_log(vec![pain(), readiness(-1.5), remove(ReadinessSignal::Pain)]);
    assert_eq!(out, vec![readiness(-1.5)]);
    assert_replay_equivalent(vec![pain(), readiness(-1.5), remove(ReadinessSignal::Pain)]);
}

#[test]
fn remove_readiness_cancels_only_the_latest_matching_submit() {
    // Two pain reports, one undo: the earlier report must still replay
    // (and still hard-stop training).
    let events = vec![pain(), pain(), remove(ReadinessSignal::Pain)];
    let out = compact_event_log(events.clone());
    assert_eq!(out, vec![pain()]);
    assert_replay_equivalent(events);
    assert!(replay(&[pain()]).train_blocked);
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

/// A resting-HR-ONLY check-in (no wellness items): exercises the RestingHr
/// channel in isolation for the per-channel Rule 4 window.
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

/// A sleep-ONLY check-in (no resting-HR/HRV): the wellness channel in
/// isolation for the per-channel Rule 4 window.
fn checkin_sleep(observed_at: i64, sleep: u8) -> Event {
    Event::SubmitCheckin(crate::schema::CheckinInput {
        observed_at,
        sleep_quality: Some(sleep),
        soreness: None,
        mood: None,
        fatigue: None,
        stress: None,
        resting_hr_bpm: None,
        hrv_rmssd_ms: None,
    })
}

/// A soreness-ONLY check-in (no sleep/mood/HR): drives the endpoint bridge's
/// recency gate for the Rule 4 replay-equivalence guard.
fn checkin_sore(observed_at: i64, sore: u8) -> Event {
    Event::SubmitCheckin(crate::schema::CheckinInput {
        observed_at,
        sleep_quality: None,
        soreness: Some(sore),
        mood: None,
        fatigue: None,
        stress: None,
        resting_hr_bpm: None,
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
fn rule4_anchors_on_the_newest_surviving_checkin_not_a_cleared_one() {
    // A ClearCheckins wipes the pre-clear history (Rule 1); Rule 4 then
    // anchors on the post-clear check-in, so a lone recent check-in after a
    // clear is never mistaken for "stale" against a wiped older reference.
    let out = compact_event_log(vec![
        checkin(0),
        checkin(DAY_SEC),
        Event::ClearCheckins,
        checkin(100 * DAY_SEC),
    ]);
    assert_eq!(out, vec![checkin(100 * DAY_SEC)]);
}

#[test]
fn replay_equivalence_drops_stale_checkins() {
    // A full baseline week on days 0..=6, then a fresh burst on days 60..=66.
    // The old week is >45 days before the newest check-in, so Rule 4 drops it
    // and since derive_readiness only reads the trailing 30 days, the
    // replayed view is identical before and after compaction.
    let mut events = Vec::new();
    for d in 0..7 {
        events.push(checkin(d * DAY_SEC));
    }
    for d in 60..67 {
        events.push(checkin(d * DAY_SEC));
    }
    let compacted = compact_event_log(events.clone());
    assert!(
        compacted.len() < events.len(),
        "the stale week must be dropped"
    );
    assert_replay_equivalent(events);
}

#[test]
fn rule4_per_channel_window_keeps_a_sparse_channel_alive() {
    // Replay-equivalence guard. `derive_readiness` windows EACH channel on
    // ITS OWN newest reading (per_day_series), not the log's global newest.
    // Eight resting-HR-ONLY check-ins on days 0..=7 build a full RHR baseline
    // (>= MIN_BASELINE_CHECKINS), then sleep-ONLY check-ins on days 60..=63
    // carry NO resting-HR. Anchoring Rule 4 on the GLOBAL newest (day 63)
    // would drop the whole RHR week (>45 days back) and silently vanish the
    // RestingHr readiness row on the next launch. The per-channel anchor
    // (min channel newest = day 7 for RHR) keeps them, so replay is identical.
    let mut events = Vec::new();
    for d in 0..8 {
        events.push(checkin_rhr(d * DAY_SEC, 50.0 + d as f64));
    }
    for d in 60..64 {
        events.push(checkin_sleep(d * DAY_SEC, 3));
    }
    let out = compact_event_log(events.clone());
    let rhr_kept = out
        .iter()
        .filter(|e| matches!(e, Event::SubmitCheckin(c) if c.resting_hr_bpm.is_some()))
        .count();
    assert_eq!(
        rhr_kept, 8,
        "all 8 resting-HR check-ins must survive - their channel still reads them"
    );
    // The full stream really does derive a RestingHr input (else the guard is
    // vacuous); compaction must not change the rendered view.
    assert_replay_equivalent(events);
}

#[test]
fn rule4_stale_soreness_bridge_is_replay_equivalent() {
    // Regression guard. A maxed-sore morning (day 1) followed only by sleep-only
    // check-ins on days 47..=61 folds into the "wellness" retention channel,
    // whose newest (day 61) pushes the Rule 4 cutoff (day 16) past the sore
    // reading → compaction evicts it. The soreness endpoint bridge is
    // recency-gated (SORENESS_RECENCY_DAYS), so the 60-day-old reading never
    // fires in the FULL log either; compacted and uncompacted logs replay to
    // the identical view. Pre-fix the ungated bridge fired only on the full
    // log (soreness series anchored on its OWN newest), so the views diverged
    // and this assertion failed.
    let mut events = vec![checkin_sore(DAY_SEC, 5)];
    for d in 47..=61 {
        events.push(checkin_sleep(d * DAY_SEC, 3));
    }
    // The sore line really is evicted by compaction (else the guard is
    // vacuous): its only channel anchor is the day-61 wellness newest.
    let out = compact_event_log(events.clone());
    let sore_kept = out
        .iter()
        .filter(|e| matches!(e, Event::SubmitCheckin(c) if c.soreness.is_some()))
        .count();
    assert_eq!(
        sore_kept, 0,
        "the stale soreness line is out of Rule 4's window"
    );
    assert_replay_equivalent(events);
}

#[test]
fn rule4_stale_hrv_verdict_is_replay_equivalent() {
    // Mirrors the soreness-bridge guard for the HRV recency gate. A matured
    // HRV baseline (22 daily readings) followed only by sleep-only check-ins
    // on days 22..=31 leaves the newest HRV reading more than HRV_RECENCY_DAYS
    // behind the newest check-in, so the rolling verdict expires in the FULL
    // log. Compaction retains or evicts the HRV lines by their own channel
    // anchor; either way the compacted and uncompacted logs must replay to the
    // identical view.
    let mut events = Vec::new();
    for d in 0..22 {
        events.push(Event::SubmitCheckin(crate::schema::CheckinInput {
            observed_at: d * DAY_SEC,
            hrv_rmssd_ms: Some(40.0),
            ..Default::default()
        }));
    }
    for d in 22..=31 {
        events.push(checkin_sleep(d * DAY_SEC, 3));
    }
    assert_replay_equivalent(events);
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
fn only_the_first_plan_request_survives() {
    // GeneratePlan is keep-FIRST (family 13): `update`'s GeneratePlan arm is
    // first-write-wins, so the original request defines the plan anchor and
    // every later one is a no-op. The first surviving request survives; the
    // rest are provably inert. See app/mod.rs `Event::GeneratePlan`.
    let out = compact_event_log(vec![generate_plan(1), plan("chest"), generate_plan(8)]);
    assert_eq!(out, vec![generate_plan(1), plan("chest")]);
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
    // ClearPlan sweeps the moves with the plan (update clears them too).
    let out = compact_event_log(vec![generate_plan(1), mv(1, 2), Event::ClearPlan]);
    assert!(out.is_empty(), "{out:?}");
}

#[test]
fn replay_equivalence_of_two_plan_requests_no_clear() {
    // Cross-language parity scenario (mirrors EventLogTest keep-first): two
    // GeneratePlans, no ClearPlan; the first survives and the compacted log
    // replays to the identical plan anchor.
    assert_replay_equivalent(vec![
        profile(12),
        set_today(10),
        generate_plan(1),
        generate_plan(8),
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
fn delete_cancels_its_logged_set_pairwise() {
    // A set logged then deleted is inert; an unrelated run survives.
    let events = vec![set_id("Bench", 7, 0), run(5.0), del_set(7)];
    let out = compact_event_log(events.clone());
    assert_eq!(out, vec![run(5.0)], "the set+delete pair is dropped");
    assert_replay_equivalent(events);
}

#[test]
fn delete_targets_the_newest_matching_row() {
    // Two sets share an id; the delete removes the newest: the earlier
    // survives, exactly as `update` (find_set = rposition) would.
    let events = vec![set_id("A", 3, 0), set_id("B", 3, DAY_SEC), del_set(3)];
    let out = compact_event_log(events.clone());
    assert_eq!(out, vec![set_id("A", 3, 0)]);
    assert_replay_equivalent(events);
}

#[test]
fn amend_supersedes_the_prior_set_but_survives() {
    // A strict amend needs its base row on replay, so Log→Amend keeps BOTH
    // the base log AND the amend (replay-equivalent to editing the set in
    // place). The amend keeps the entry's id for later edits.
    let events = vec![set_id("Bench", 5, 0), amend_set(5, 120.0)];
    let out = compact_event_log(events.clone());
    assert_eq!(
        out,
        vec![set_id("Bench", 5, 0), amend_set(5, 120.0)],
        "the base log survives so the strict amend can replace it on replay"
    );
    assert_replay_equivalent(events);
}

#[test]
fn amend_chain_collapses_to_log_plus_last_amend() {
    // A Log→Amend→Amend chain (no delete) collapses to `[log, last amend]`;
    // the superseded middle amend is dropped, the base log is retained so the
    // strict final amend has a row to replace on replay.
    let events = vec![
        set_id("Bench", 9, 0),
        amend_set(9, 110.0),
        amend_set(9, 130.0),
    ];
    let out = compact_event_log(events.clone());
    assert_eq!(
        out,
        vec![set_id("Bench", 9, 0), amend_set(9, 130.0)],
        "the middle amend is superseded; the base log and last amend remain"
    );
    assert_replay_equivalent(events);
}

#[test]
fn invalid_amend_does_not_revert_a_valid_edit() {
    // `update` rejects an invalid AmendSet (reps=0, no hold) as a NO-OP, so it
    // supersedes nothing. Compaction must drop it ALONE and keep the valid prior
    // amend — otherwise the compacted log replays to the pre-edit value and the
    // user's edit silently reverts on the next launch.
    let invalid_amend = Event::AmendSet {
        hold_sec: None,
        entry_id: 1,
        exercise: "Bench".into(),
        weight_kg: 110.0,
        reps: 0,
        rpe: 8.0,
        observed_at: 0,
        observed_at_fallback: 0,
    };
    let events = vec![set_id("Bench", 1, 0), amend_set(1, 110.0), invalid_amend];
    let out = compact_event_log(events.clone());
    assert_eq!(
        out,
        vec![set_id("Bench", 1, 0), amend_set(1, 110.0)],
        "the invalid trailing amend is dropped alone; the valid edit survives"
    );
    assert_replay_equivalent(events);
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
    // amend then matches nothing and is dropped; the entry STAYS deleted, both
    // in a live `update` replay and after compaction. Previously, the amend
    // replayed as a push and RESURRECTED the deleted row.
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
fn amend_chain_collapses_to_the_last_amend() {
    // Log→Amend→Amend→Delete is fully inert.
    let events = vec![
        set_id("Bench", 9, 0),
        amend_set(9, 110.0),
        amend_set(9, 130.0),
        del_set(9),
    ];
    let out = compact_event_log(events.clone());
    assert!(out.is_empty(), "the whole edit chain cancels away");
    assert_replay_equivalent(events);
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
fn run_delete_and_amend_replay_equivalent() {
    let deleted = vec![run_id(5.0, 2, 0), run_id(8.0, 3, DAY_SEC), del_run(3)];
    assert_eq!(compact_event_log(deleted.clone()), vec![run_id(5.0, 2, 0)]);
    assert_replay_equivalent(deleted);

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
    // The base run log is KEPT (the strict amend replaces it on replay);
    // the amend survives too, so `[log, amend]` both remain.
    assert_eq!(compact_event_log(amended.clone()).len(), 2);
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
    // A DeleteEntry cancels the base LogRunTrack AND its trims on replay. The
    // trims between the base and the delete are the delete's OWN removal chain,
    // so they are skipped by the spike-baseline guard: the delete's chain-walk
    // now drops base + trims + delete entirely. This pins the delete chain-walk:
    // without walking THROUGH the trims to the base log, compaction would drop
    // the delete+trim while resurrecting the untrimmed base, and replay would
    // diverge (a run would reappear).
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
fn trim_counts_as_a_run_line_for_the_spike_guard() {
    // Another run's trim sitting between a run and that run's delete is a run
    // line for the baseline-bake guard: it is NOT in the deleted run's removal
    // chain (it belongs to a DIFFERENT run, 7_200_006), so it is not skipped
    // and the delete's pair-drop is held back — kept lines, replay-equivalent
    // either way.
    assert_replay_equivalent(vec![
        run_track_id(7_200_005, 0),
        run_id(5.0, 7_200_006, DAY_SEC),
        trim_run(7_200_006, DAY_SEC, 100, 300),
        del_run(7_200_005),
    ]);
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
    // The wire form round-trips with defaults for the optional fields.
    let wire = serde_json::to_string(&meta_run(5, 9, Some(4))).unwrap();
    assert_eq!(
        serde_json::from_str::<Event>(&wire).unwrap(),
        meta_run(5, 9, Some(4))
    );
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
fn activities_are_family_15() {
    assert_eq!(classify(&act(30.0, 1, 0)), (Family::Activities, false));
    assert_eq!(
        classify(&Event::ClearActivities),
        (Family::Activities, true)
    );
    assert_eq!(classify(&del_act(1, 0)), (Family::Activities, false));
    assert_eq!(Family::Activities as u8, 15);
    assert_eq!(Family::ALL[15], Family::Activities);
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
