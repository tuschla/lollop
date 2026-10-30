//! Upgrade fixture generator, old-core half: copied into an old worktree's
//! `shared/tests/` by scripts/upgrade-fixtures/generate.sh. Replays the log the
//! old shell wrote (UpgradeFixtureGen.kt) through THIS version's core, checks the
//! old boot compaction and the old backup replay to the same view, and writes
//! `history.json`: what the old app showed, without its copy text.

use std::fs;
use std::path::PathBuf;

use crux_core::App;
use serde_json::{Map, Value};
use shared::{Engine, Event, Model};

const TODAY: i64 = 20725;
const OFF: i64 = 7200;

fn view_of(text: &str, name: &str) -> Value {
    let app = Engine;
    let mut model = Model::default();
    let mut bad = Vec::new();
    for (i, line) in text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
        match serde_json::from_str::<Event>(line) {
            Ok(e) => {
                let _ = app.update(e, &mut model);
            }
            Err(e) => bad.push(format!("{name}#{i}: {e}")),
        }
    }
    assert!(bad.is_empty(), "old core rejects its own lines:\n{}", bad.join("\n"));
    let today: Event =
        serde_json::from_str(&format!(r#"{{"SetToday":{{"epoch_day":{TODAY},"utc_offset_sec":{OFF}}}}}"#)).unwrap();
    let _ = app.update(today, &mut model);
    serde_json::to_value(app.view(&model)).unwrap()
}

/// The view minus its copy: every string leaf holding a space is text, but an
/// exercise name.
fn data_only(v: &Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.iter()
                .filter(|(k, x)| *k == "exercise" || !matches!(x, Value::String(s) if s.contains(' ')))
                .map(|(k, x)| (k.clone(), data_only(x)))
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(data_only).collect()),
        _ => v.clone(),
    }
}

fn pick(v: &Value, keys: &[&str]) -> Value {
    let m = v.as_object().unwrap();
    Value::Object(keys.iter().filter_map(|k| m.get(*k).map(|x| (k.to_string(), x.clone()))).collect())
}

/// The logged entries and profile as History lists them: raw inputs only.
fn history_only(view: &Value) -> Value {
    let mut out = Map::new();
    let list = |key: &str, fields: &[&str]| {
        Value::Array(view[key].as_array().into_iter().flatten().map(|e| pick(e, fields)).collect())
    };
    out.insert(
        "lifts".into(),
        list("lifts", &["exercise", "weight_kg", "reps", "rpe", "hold_sec", "observed_at", "entry_id"]),
    );
    out.insert(
        "runs".into(),
        list("runs", &["distance_km", "duration_min", "observed_at", "entry_id", "workout_type", "session_rpe", "avg_hr_bpm"]),
    );
    if view.get("activities").is_some() {
        out.insert("activities".into(), data_only(&view["activities"]));
    }
    out.insert("profile".into(), data_only(&view["profile"]));
    for k in ["input_count", "safety_tier", "train_blocked"] {
        out.insert(k.into(), view[k].clone());
    }
    Value::Object(out)
}

#[test]
fn dump_expected_history() {
    let wt = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
    let tag = wt.file_name().unwrap().to_str().unwrap().trim_start_matches("upg-").to_string();
    let gen_dir = std::env::var("UPG_GEN_DIR").expect("UPG_GEN_DIR unset: run scripts/upgrade-fixtures/generate.sh");
    let dir = PathBuf::from(gen_dir).join(&tag);
    let raw = view_of(&fs::read_to_string(dir.join("raw.ndjson")).unwrap(), "raw");
    let log = view_of(&fs::read_to_string(dir.join("log.ndjson")).unwrap(), "log");
    assert_eq!(raw, log, "old boot compaction changed the old view");
    if let Ok(b) = fs::read_to_string(dir.join("backup.ndjson")) {
        assert_eq!(raw, view_of(&b, "backup"), "old backup changed the old view");
    }
    // The last release before the cleanup keeps its whole view (minus copy); the
    // others keep the History entries.
    let expected = if std::env::var_os("UPG_FULL_VIEW").is_some() { data_only(&raw) } else { history_only(&raw) };
    let mut s = serde_json::to_string(&expected).unwrap();
    s.push('\n');
    fs::write(dir.join("history.json"), s).unwrap();
}
