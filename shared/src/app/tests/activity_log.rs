//! Logging other activities: `LogActivity` (minutes gate, entry ids),
//! `DeleteEntry { kind: Activity }` and `ClearActivities`, on the wire and
//! through replay.

use super::*;

fn log(minutes: f64, observed_at: i64, entry_id: u64) -> Event {
    Event::LogActivity {
        kind: ActivityKind::Sport,
        minutes,
        vigorous: true,
        strengthening: false,
        observed_at,
        entry_id,
    }
}

fn delete(entry_id: u64, observed_at_fallback: i64) -> Event {
    Event::DeleteEntry {
        kind: EntryKind::Activity,
        entry_id,
        observed_at_fallback,
    }
}

fn apply(app: &Engine, model: &mut Model, events: Vec<Event>) {
    for ev in events {
        app.update(ev, model).expect_only_render();
    }
}

fn ids(model: &Model) -> Vec<u64> {
    Engine
        .view(model)
        .activities
        .iter()
        .map(|a| a.entry_id)
        .collect()
}

#[test]
fn the_contract_wire_forms_parse() {
    let ev: Event = serde_json::from_str(
        r#"{"LogActivity":{"kind":"Sport","minutes":45.0,"vigorous":true,"strengthening":false,"observed_at":1760000000,"entry_id":123}}"#,
    )
    .unwrap();
    assert_eq!(ev, log(45.0, 1_760_000_000, 123));
    let bare: Event =
        serde_json::from_str(r#"{"LogActivity":{"kind":"Strength","minutes":20}}"#).unwrap();
    assert_eq!(
        bare,
        Event::LogActivity {
            kind: ActivityKind::Strength,
            minutes: 20.0,
            vigorous: false,
            strengthening: false,
            observed_at: 0,
            entry_id: 0,
        }
    );
    for kind in [
        "Sport", "Cycling", "Swimming", "Walking", "Dance", "Strength", "Other",
    ] {
        let k: ActivityKind = serde_json::from_value(serde_json::json!(kind)).unwrap();
        assert_eq!(serde_json::to_value(k).unwrap(), serde_json::json!(kind));
    }
    assert_eq!(
        serde_json::from_str::<Event>(r#""ClearActivities""#).unwrap(),
        Event::ClearActivities
    );
    assert_eq!(
        serde_json::from_str::<Event>(
            r#"{"DeleteEntry":{"kind":"Activity","entry_id":123,"observed_at_fallback":1760000000}}"#
        )
        .unwrap(),
        delete(123, 1_760_000_000)
    );
}

#[test]
fn a_logged_activity_replays_into_the_view() {
    let app = Engine;
    let mut model = Model::default();
    apply(&app, &mut model, vec![log(45.0, 1_760_000_000, 123)]);
    let vm = app.view(&model);
    assert_eq!(
        vm.activities,
        vec![ActivityResultView {
            entry_id: 123,
            observed_at: 1_760_000_000,
            kind: ActivityKind::Sport,
            minutes: 45.0,
            vigorous: true,
            strengthening: false,
        }]
    );
    let json = serde_json::to_value(&vm).unwrap();
    assert_eq!(
        json["activities"][0],
        serde_json::json!({"entry_id":123,"observed_at":1760000000,"kind":"Sport","minutes":45.0,"vigorous":true,"strengthening":false})
    );
    assert_eq!(json["youth_activity"], serde_json::Value::Null);
}

#[test]
fn activities_list_chronologically() {
    let app = Engine;
    let mut model = Model::default();
    apply(
        &app,
        &mut model,
        vec![
            log(30.0, 2_000, 2),
            log(30.0, 1_000, 3),
            log(30.0, 2_000, 1),
        ],
    );
    assert_eq!(ids(&model), vec![3, 1, 2]);
}

#[test]
fn minutes_outside_the_gate_are_ignored() {
    let app = Engine;
    let mut model = Model::default();
    for minutes in [0.0, -5.0, 600.5, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        apply(&app, &mut model, vec![log(minutes, 1_000, 1)]);
    }
    assert!(model.activities.is_empty(), "no row for an invalid line");
    apply(
        &app,
        &mut model,
        vec![log(600.0, 1_000, 2), log(0.5, 1_000, 3)],
    );
    assert_eq!(ids(&model), vec![2, 3], "(0, 600] is accepted");
}

#[test]
fn delete_entry_removes_the_newest_matching_activity() {
    let app = Engine;
    let mut model = Model::default();
    apply(
        &app,
        &mut model,
        vec![
            log(30.0, 1_000, 7),
            log(40.0, 2_000, 7),
            log(50.0, 3_000, 8),
            log(60.0, 4_000, 0),
            log(70.0, 5_000, 0),
        ],
    );
    apply(&app, &mut model, vec![delete(7, 0)]);
    assert_eq!(
        model
            .activities
            .iter()
            .map(|a| a.minutes)
            .collect::<Vec<_>>(),
        vec![30.0, 50.0, 60.0, 70.0],
        "an id delete removes the newest row with that id"
    );
    apply(&app, &mut model, vec![delete(0, 4_000)]);
    assert_eq!(
        model
            .activities
            .iter()
            .map(|a| a.minutes)
            .collect::<Vec<_>>(),
        vec![30.0, 50.0, 70.0],
        "a legacy delete matches on observed_at"
    );
    apply(&app, &mut model, vec![delete(99, 0)]);
    assert_eq!(model.activities.len(), 3, "an unmatched delete is a no-op");
    apply(
        &app,
        &mut model,
        vec![Event::DeleteEntry {
            kind: EntryKind::Run,
            entry_id: 8,
            observed_at_fallback: 0,
        }],
    );
    assert_eq!(
        model.activities.len(),
        3,
        "a run delete never touches activities"
    );
}

#[test]
fn clear_activities_empties_only_activities() {
    let app = Engine;
    let mut model = Model::default();
    apply(
        &app,
        &mut model,
        vec![
            log(30.0, 1_000, 1),
            log_set("Back Squat", 100.0, 5, 8.0, 1_000),
            log_run_at(5.0, 30.0, 70.0, 1_000),
            Event::ClearActivities,
        ],
    );
    let vm = app.view(&model);
    assert!(vm.activities.is_empty());
    assert_eq!((vm.lifts.len(), vm.runs.len()), (1, 1));
    apply(&app, &mut model, vec![log(20.0, 2_000, 2)]);
    assert_eq!(ids(&model), vec![2], "logging works again after a clear");
}
