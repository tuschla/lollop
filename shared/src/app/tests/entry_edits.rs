//! Logged-entry identity: entry ids, delete / amend, backdated logging (ordered
//! by `observed_at`, not arrival), rejected sets, and clearing sets and runs.

use super::*;

#[test]
fn view_echoes_the_entry_id_for_the_shell() {
    let app = Engine;
    let mut model = Model::default();
    app.update(log_set_id("Bench", 100.0, 4242, 0), &mut model)
        .expect_only_render();
    assert_eq!(app.view(&model).lifts[0].entry_id, 4242);
}

#[test]
fn delete_entry_removes_the_targeted_set_by_id() {
    let app = Engine;
    let mut model = Model::default();
    app.update(log_set_id("Bench", 100.0, 1, 0), &mut model)
        .expect_only_render();
    app.update(log_set_id("Squat", 140.0, 2, 100), &mut model)
        .expect_only_render();
    app.update(
        Event::DeleteEntry {
            kind: EntryKind::Set,
            entry_id: 1,
            observed_at_fallback: 0,
        },
        &mut model,
    )
    .expect_only_render();
    let lifts = app.view(&model).lifts;
    assert_eq!(lifts.len(), 1);
    assert_eq!(lifts[0].exercise, "Squat");
    assert_eq!(lifts[0].entry_id, 2);
}

#[test]
fn amend_set_edits_fields_in_place_keeping_the_id() {
    let app = Engine;
    let mut model = Model::default();
    app.update(log_set_id("Bench", 100.0, 7, 0), &mut model)
        .expect_only_render();
    app.update(
        Event::AmendSet {
            hold_sec: None,
            entry_id: 7,
            exercise: "Bench".into(),
            weight_kg: 120.0,
            reps: 3,
            rpe: 9.0,
            observed_at: 0,
            observed_at_fallback: 0,
        },
        &mut model,
    )
    .expect_only_render();
    let lifts = app.view(&model).lifts;
    assert_eq!(lifts.len(), 1, "amend edits, never appends a second row");
    assert!((lifts[0].weight_kg - 120.0).abs() < f64::EPSILON);
    assert_eq!(lifts[0].reps, 3);
    assert_eq!(lifts[0].entry_id, 7, "the amended set keeps its id");
}

#[test]
fn amend_after_delete_is_a_no_op_never_resurrects() {
    // Full prevention: amend is a STRICT update. Log→Delete→Amend must leave
    // the set deleted; an amend whose target no longer exists adds nothing
    // (a fresh row would resurrect the deleted entry).
    let app = Engine;
    let mut model = Model::default();
    app.update(log_set_id("Bench", 100.0, 5, 0), &mut model)
        .expect_only_render();
    app.update(
        Event::DeleteEntry {
            kind: EntryKind::Set,
            entry_id: 5,
            observed_at_fallback: 0,
        },
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::AmendSet {
            hold_sec: None,
            entry_id: 5,
            exercise: "Bench".into(),
            weight_kg: 120.0,
            reps: 5,
            rpe: 8.0,
            observed_at: 0,
            observed_at_fallback: 0,
        },
        &mut model,
    )
    .expect_only_render();
    assert!(
        app.view(&model).lifts.is_empty(),
        "a strict amend on a deleted row is a no-op, not a resurrection"
    );

    // Same for a run.
    let mut model = Model::default();
    app.update(
        log_run_track(
            vec![
                GpsPoint {
                    lat: 0.0,
                    lon: 0.0,
                    observed_at: 0,
                    accuracy_m: 5.0,
                },
                GpsPoint {
                    lat: 0.01,
                    lon: 0.0,
                    observed_at: 300,
                    accuracy_m: 5.0,
                },
            ],
            0.0,
            0.0,
            10,
            42,
        ),
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::DeleteEntry {
            kind: EntryKind::Run,
            entry_id: 42,
            observed_at_fallback: 0,
        },
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::AmendRun {
            entry_id: 42,
            distance_km: 9.0,
            duration_min: 45.0,
            hr_pct_max: 0.0,
            longest_recent_km: 0.0,
            observed_at: 0,
            observed_at_fallback: 0,
            workout_type: None,
            session_rpe: None,
            avg_hr_bpm: None,
        },
        &mut model,
    )
    .expect_only_render();
    assert!(
        app.view(&model).runs.is_empty(),
        "a strict run amend on a deleted row is a no-op"
    );
}

#[test]
fn delete_entry_removes_a_gps_tracked_run() {
    let app = Engine;
    let mut model = Model::default();
    app.update(
        log_run_track(
            vec![
                GpsPoint {
                    lat: 0.0,
                    lon: 0.0,
                    observed_at: 0,
                    accuracy_m: 5.0,
                },
                GpsPoint {
                    lat: 0.01,
                    lon: 0.0,
                    observed_at: 300,
                    accuracy_m: 5.0,
                },
            ],
            0.0,
            0.0,
            10,
            99,
        ),
        &mut model,
    )
    .expect_only_render();
    assert_eq!(app.view(&model).runs.len(), 1);
    app.update(
        Event::DeleteEntry {
            kind: EntryKind::Run,
            entry_id: 99,
            observed_at_fallback: 0,
        },
        &mut model,
    )
    .expect_only_render();
    assert!(app.view(&model).runs.is_empty(), "the junk GPS run is gone");
}

#[test]
fn delete_entry_falls_back_to_observed_at_for_a_legacy_row() {
    // A legacy set has entry_id 0; the shell targets it by observed_at.
    let app = Engine;
    let mut model = Model::default();
    app.update(log_set_id("Old", 100.0, 0, 5000), &mut model)
        .expect_only_render();
    app.update(
        Event::DeleteEntry {
            kind: EntryKind::Set,
            entry_id: 0,
            observed_at_fallback: 5000,
        },
        &mut model,
    )
    .expect_only_render();
    assert!(app.view(&model).lifts.is_empty());
}

#[test]
fn delete_entry_with_no_match_is_a_no_op() {
    let app = Engine;
    let mut model = Model::default();
    app.update(log_set_id("Bench", 100.0, 1, 0), &mut model)
        .expect_only_render();
    app.update(
        Event::DeleteEntry {
            kind: EntryKind::Set,
            entry_id: 555,
            observed_at_fallback: 0,
        },
        &mut model,
    )
    .expect_only_render();
    assert_eq!(
        app.view(&model).lifts.len(),
        1,
        "unmatched delete changes nothing"
    );
}

#[test]
fn logged_at_timestamp_round_trips_to_the_history_views() {
    // A shell dates each history card from the view's `observed_at`; the core
    // must carry the shell-supplied log time verbatim (it holds no clock) from
    // the event through storage to both the lift and run views. A logged event
    // that omits the field (pre-timestamp persisted log) decodes as 0.
    let app = Engine;
    let mut model = Model::default();

    app.update(
        log_set("Back squat", 100.0, 5, 8.0, 1_700_000_000),
        &mut model,
    )
    .expect_only_render();
    app.update(log_run(5.0, 25.0, 70.0, 12.0, 1_700_000_500, 0), &mut model)
        .expect_only_render();

    let vm = app.view(&model);
    assert_eq!(vm.lifts[0].observed_at, 1_700_000_000);
    assert_eq!(vm.runs[0].observed_at, 1_700_000_500);

    // Absent from the wire (old persisted event) → 0, not a decode failure.
    // The view lists are chronological by observed_at, so the undated (0)
    // entry sorts BEFORE the dated set, not after it in append order.
    let undated: Event = serde_json::from_str(
        r#"{"LogSet":{"exercise":"Bench","weight_kg":60.0,"reps":8,"rpe":7.0}}"#,
    )
    .expect("pre-timestamp LogSet still decodes");
    app.update(undated, &mut model).expect_only_render();
    let vm = app.view(&model);
    assert_eq!(vm.lifts[0].observed_at, 0);
    assert_eq!(vm.lifts[0].exercise, "Bench");
    assert_eq!(vm.lifts[1].observed_at, 1_700_000_000);
    // entry_id is likewise serde-default: a legacy LogSet decodes with
    // id 0 (the shell then targets it by observed_at).
    assert_eq!(vm.lifts[0].entry_id, 0);
}

#[test]
fn clearing_sets_and_runs_empties_their_views() {
    let app = Engine;
    let mut model = Model::default();

    app.update(log_set("Back Squat", 140.0, 5, 8.0, 0), &mut model)
        .expect_only_render();
    app.update(log_run(10.0, 50.0, 70.0, 12.0, 0, 0), &mut model)
        .expect_only_render();
    assert_eq!(app.view(&model).lifts.len(), 1);
    assert_eq!(app.view(&model).runs.len(), 1);

    app.update(Event::ClearSets, &mut model)
        .expect_only_render();
    assert!(app.view(&model).lifts.is_empty());
    assert_eq!(
        app.view(&model).runs.len(),
        1,
        "clearing sets must not touch runs"
    );

    app.update(Event::ClearRuns, &mut model)
        .expect_only_render();
    assert!(app.view(&model).runs.is_empty());
}

#[test]
fn backdated_set_slots_into_the_e1rm_chain_chronologically() {
    let app = Engine;
    let mut model = Model::default();
    // Today's set first, then a BACKDATED earlier set of the same lift.
    app.update(log_set("Back Squat", 100.0, 5, 8.0, 2_000_000), &mut model)
        .expect_only_render();
    app.update(log_set("Back Squat", 95.0, 5, 8.0, 1_000_000), &mut model)
        .expect_only_render();
    let vm = app.view(&model);
    // Chronological view order: the backdated set renders first…
    assert_eq!(vm.lifts[0].observed_at, 1_000_000);
    assert_eq!(vm.lifts[0].weight_kg, 95.0);
    assert!(
        vm.lifts[0].e1rm_delta_kg.is_none(),
        "first in time has no baseline"
    );
    // …and the later set's delta compares against it (95→100 kg ⇒ up).
    assert_eq!(vm.lifts[1].observed_at, 2_000_000);
    assert_eq!(vm.lifts[1].e1rm_direction, Some(TrendDirection::Up));
    let expected = ((strength::e1rm_epley_rpe(100.0, 5, 8.0) * 10.0).round()
        - (strength::e1rm_epley_rpe(95.0, 5, 8.0) * 10.0).round())
        / 10.0;
    assert!(
        (vm.lifts[1].e1rm_delta_kg.unwrap() - expected).abs() < 1e-9,
        "delta {:?} vs expected {expected}",
        vm.lifts[1].e1rm_delta_kg
    );
}

// ── A zero-rep set with no hold is rejected (no fabricated e1RM) ──
#[test]
fn a_zero_rep_set_with_no_hold_is_rejected() {
    // A zero-rep set carries no e1RM (the formulas require reps >= 1); the core
    // must drop it as a no-op rather than fabricate one and panic in debug.
    let app = Engine;
    let mut model = Model::default();
    app.update(
        with_entry_id(log_set("Bench", 100.0, 0, 8.0, 100), 1),
        &mut model,
    )
    .expect_only_render();
    assert!(
        model.sets.is_empty(),
        "a zero-rep no-hold set is not persisted"
    );
    // A timed hold with zero reps is still valid data (no e1RM, real hold time).
    app.update(
        Event::LogSet {
            exercise: "Plank".into(),
            weight_kg: 0.0,
            reps: 0,
            rpe: 0.0,
            observed_at: 200,
            entry_id: 2,
            hold_sec: Some(60),
        },
        &mut model,
    )
    .expect_only_render();
    assert_eq!(
        model.sets.len(),
        1,
        "a timed hold with zero reps is still recorded"
    );
    let _ = app.view(&model); // must not panic
}

// ── Amending a same-second set preserves display + e1RM-chain order ──
#[test]
fn amend_of_a_same_second_set_preserves_order() {
    // Two Bench sets at the SAME second, distinct entry ids. AmendSet
    // remove+pushes the first set to the Vec tail; keying the history sort on
    // entry_id (not insertion order) keeps it ahead of the second set, so the
    // display order and the per-exercise e1RM-delta chain stay stable.
    let app = Engine;
    let mut model = Model::default();
    let t = MON * DAY_SEC;
    app.update(log_set_id("Bench", 100.0, 1000, t), &mut model)
        .expect_only_render();
    app.update(log_set_id("Bench", 140.0, 2000, t), &mut model)
        .expect_only_render();
    // Sanity: the first-logged (lower id) set leads before any amend.
    let before = app.view(&model).lifts;
    assert_eq!(before[0].entry_id, 1000);
    assert_eq!(before[1].entry_id, 2000);
    // Amend the FIRST set (id 1000), keeping its second.
    app.update(
        Event::AmendSet {
            hold_sec: None,
            entry_id: 1000,
            exercise: "Bench".into(),
            weight_kg: 110.0,
            reps: 5,
            rpe: 8.0,
            observed_at: t,
            observed_at_fallback: 0,
        },
        &mut model,
    )
    .expect_only_render();
    let lifts = app.view(&model).lifts;
    assert_eq!(lifts.len(), 2, "amend edits, never appends");
    assert_eq!(
        lifts[0].entry_id, 1000,
        "amended set holds its slot among ties"
    );
    assert!((lifts[0].weight_kg - 110.0).abs() < f64::EPSILON);
    assert_eq!(lifts[1].entry_id, 2000);
    // The e1RM-delta chain still compares the second set against the first.
    assert!(lifts[1].e1rm_delta_kg.is_some(), "chain intact after amend");
}

// ── Amending a legacy (id 0) row whose DATE changed does not duplicate ──
#[test]
fn amend_legacy_date_change_replaces_not_duplicates() {
    let app = Engine;
    let mut model = Model::default();
    let old_ts = MON * DAY_SEC;
    let new_ts = (MON + 1) * DAY_SEC;
    // Legacy log (entry_id 0).
    app.update(log_set("Bench", 100.0, 5, 8.0, old_ts), &mut model)
        .expect_only_render();
    assert_eq!(model.sets.len(), 1);
    // Amend it, CHANGING the date: must match the OLD row via fallback.
    app.update(
        Event::AmendSet {
            hold_sec: None,
            entry_id: 0,
            exercise: "Bench".into(),
            weight_kg: 110.0,
            reps: 5,
            rpe: 8.0,
            observed_at: new_ts,
            observed_at_fallback: old_ts,
        },
        &mut model,
    )
    .expect_only_render();
    assert_eq!(
        model.sets.len(),
        1,
        "a re-dated legacy amend must replace, not duplicate"
    );
    assert_eq!(model.sets[0].weight_kg, 110.0);
    assert_eq!(model.sets[0].observed_at, new_ts);
}
