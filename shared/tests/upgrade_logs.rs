//! Upgrade corpus: event logs written by older builds must replay in this core
//! with no lost entries. Each `fixtures/upgrade/<sha>.log.ndjson` is the log file
//! an older build left on disk: its own shell's `Event.toJson()` lines, after its
//! own boot compaction rewrote them. `<sha>.history.json` is what that build's
//! core showed for the log on 2026-09-29 (UTC+2), copy text removed: the History
//! entries and profile for 203ab56 and 901d239, the whole view for 4728546 (the
//! last release before the 901d239 cleanup). `4728546.backup.ndjson` is that
//! build's Profile › Your data export; `203ab56-plan-removed.log.ndjson` a
//! removed plan after that build's rewrite (UpgradeFixturesTest.kt). Regenerate with
//! `scripts/upgrade-fixtures/generate.sh`.
//!
//! - 203ab56 (2026-08-08): persisted every `SetToday`, a legacy set without
//!   `entry_id`, `endurance_intensity_pct_vo2max` in `SetProfile`, no `female`
//!   when false, measured `longest_recent_km`, the `streak` field.
//! - 4728546 (2026-10-29): the Kotlin compaction, `streak` and
//!   `longest_recent_km` as the shell computed them. 4673acc writes the same
//!   bytes for the same scenario.
//! - 901d239: the Rust compaction over JNI, `streak`/`longest_recent_km` sent as 0.
//!
//! An intended engine change that moves a value the old build showed fails
//! `old_logs_show_the_same_history_and_plan` with the field's path; name the
//! change in a `diffs` arm (as for `.profile.female`), never regenerate the
//! expected files from this build.
//!
//! The 203ab56 and 4728546 logs also carry the retired Cooper, critical speed
//! and APRE calculator lines (raw old-core wire; no Android shell ever encoded
//! them, so a real log only has them from a hand-built file).

use std::fs;
use std::path::{Path, PathBuf};

use crux_core::App;
use serde_json::Value;
use shared::log::{LineFacts, compact_lines};
use shared::{Engine, Event, Model};

const TODAY: i64 = 20725;
const OFF: i64 = 7200;
const VERSIONS: [&str; 3] = ["203ab56", "4728546", "901d239"];
const RETIRED: [&str; 6] = [
    "ComputeCooper",
    "ClearCooper",
    "ComputeCriticalSpeed",
    "ClearCriticalSpeed",
    "ComputeApre",
    "ClearApre",
];

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/upgrade")
}

fn lines(name: &str) -> Vec<String> {
    fs::read_to_string(dir().join(name))
        .unwrap_or_else(|e| panic!("fixture {name}: {e}"))
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_owned)
        .collect()
}

fn tag(line: &str) -> String {
    match serde_json::from_str::<Value>(line).expect("fixture line is JSON") {
        Value::String(s) => s,
        Value::Object(m) => m.keys().next().cloned().unwrap_or_default(),
        _ => String::new(),
    }
}

/// The view after replaying `lines` and the boot `SetToday`, plus the indices of
/// the lines this core rejects.
fn replay<'a>(lines: impl IntoIterator<Item = &'a String>) -> (Value, Vec<usize>) {
    let app = Engine;
    let mut model = Model::default();
    let mut rejected = Vec::new();
    for (i, line) in lines.into_iter().enumerate() {
        match serde_json::from_str::<Event>(line) {
            Ok(e) => {
                let _ = app.update(e, &mut model);
            }
            Err(_) => rejected.push(i),
        }
    }
    let today = Event::SetToday {
        epoch_day: TODAY,
        utc_offset_sec: OFF,
    };
    let _ = app.update(today, &mut model);
    (serde_json::to_value(app.view(&model)).unwrap(), rejected)
}

/// The view minus its copy ([`is_copy`]).
fn data_only(v: &Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.iter()
                .filter(|(k, x)| !matches!(x, Value::String(s) if is_copy(&format!(".{k}"), s)))
                .map(|(k, x)| (k.clone(), data_only(x)))
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(data_only).collect()),
        _ => v.clone(),
    }
}

/// Every difference between the old build's `expected` and this core's `got`:
/// object keys present in both are compared (a view field the cleanup removed
/// or a new one is not a difference), lists must hold the same number of
/// entries, numbers agree to 1e-9 relative.
fn diffs(path: &str, expected: &Value, got: &Value, out: &mut Vec<String>) {
    match (expected, got) {
        (Value::Object(e), Value::Object(g)) => {
            for (k, v) in e {
                if let Some(w) = g.get(k) {
                    diffs(&format!("{path}.{k}"), v, w, out);
                }
            }
        }
        (Value::Array(e), Value::Array(g)) if e.len() == g.len() => {
            for (i, (v, w)) in e.iter().zip(g).enumerate() {
                diffs(&format!("{path}[{i}]"), v, w, out);
            }
        }
        (Value::Array(e), Value::Array(g)) => {
            out.push(format!("{path}: {} entries, now {}", e.len(), g.len()));
        }
        (Value::Number(a), Value::Number(b)) => {
            let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
            if (a - b).abs() > 1e-9 * a.abs().max(b.abs()).max(1.0) {
                out.push(format!("{path}: {a}, now {b}"));
            }
        }
        // Before 4673acc the shell left `female` out when false, and the core read
        // that as false; `Profile.female` is now an Option and an absent field is
        // None, "read as not-female" (app/model.rs).
        (Value::Bool(false), Value::Null) if path == ".profile.female" => {}
        // Text is copy: it was reworded between these builds.
        (Value::String(a), Value::String(_)) if is_copy(path, a) => {}
        _ if expected == got => {}
        _ => out.push(format!("{path}: {expected}, now {got}")),
    }
}

/// A string leaf holding a space is copy text, but an exercise name.
fn is_copy(path: &str, s: &str) -> bool {
    s.contains(' ') && !path.ends_with(".exercise")
}

fn expected(v: &str) -> Value {
    serde_json::from_str(&fs::read_to_string(dir().join(format!("{v}.history.json"))).unwrap())
        .unwrap()
}

fn assert_shows(v: &str, file: &str, view: &Value) {
    let mut out = Vec::new();
    diffs("", &expected(v), &data_only(view), &mut out);
    assert!(
        out.is_empty(),
        "{file} shows other data than {v} did:\n{}",
        out.join("\n")
    );
}

#[test]
fn old_logs_replay_every_line_but_the_retired_calculators() {
    for v in VERSIONS {
        let file = format!("{v}.log.ndjson");
        let lines = lines(&file);
        let (_, rejected) = replay(&lines);
        let unexpected: Vec<String> = rejected
            .iter()
            .filter(|&&i| !RETIRED.contains(&tag(&lines[i]).as_str()))
            .map(|&i| format!("#{i} {}", &lines[i][..lines[i].len().min(120)]))
            .collect();
        assert!(
            unexpected.is_empty(),
            "{file}: this core drops old lines:\n{}",
            unexpected.join("\n")
        );
        let facts: Vec<LineFacts> = lines.iter().map(|l| LineFacts::parse(l)).collect();
        let verdict = compact_lines(&facts);
        assert!(
            verdict.torn.is_empty(),
            "{file}: intact lines read as torn: {:?}",
            verdict.torn
        );
        for &i in &rejected {
            assert!(
                verdict.dropped.contains(&i),
                "{file}: retired line #{i} survives compaction"
            );
        }
        let max_id = lines
            .iter()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter_map(|j| j.as_object()?.values().next()?.get("entry_id")?.as_u64())
            .max()
            .unwrap_or(0);
        assert_eq!(verdict.max_entry_id, max_id, "{file}: max entry id");
        assert!(
            !verdict.plan_removed,
            "{file}: the plan was re-generated after its ClearPlan"
        );
    }
}

#[test]
fn old_logs_compact_to_the_same_view() {
    for v in VERSIONS {
        let file = format!("{v}.log.ndjson");
        let lines = lines(&file);
        let facts: Vec<LineFacts> = lines.iter().map(|l| LineFacts::parse(l)).collect();
        let dropped = compact_lines(&facts).dropped;
        let kept: Vec<&String> = lines
            .iter()
            .enumerate()
            .filter(|(i, _)| !dropped.contains(i))
            .map(|(_, l)| l)
            .collect();
        // 901d239 already compacted with these rules: a second pass drops nothing.
        assert_eq!(
            dropped.is_empty(),
            v == "901d239",
            "{file}: dropped {dropped:?}"
        );
        let (full, _) = replay(&lines);
        let (compacted, _) = replay(kept);
        assert_eq!(
            full, compacted,
            "{file}: the compacted log replays to another view"
        );
    }
}

#[test]
fn old_logs_show_the_same_history_and_plan() {
    for v in VERSIONS {
        let file = format!("{v}.log.ndjson");
        let (view, _) = replay(&lines(&file));
        assert_shows(v, &file, &view);
    }
}

#[test]
fn old_backup_replays_to_the_same_history_and_plan() {
    let (view, rejected) = replay(&lines("4728546.backup.ndjson"));
    assert!(rejected.is_empty(), "backup lines rejected: {rejected:?}");
    assert_shows("4728546", "4728546.backup.ndjson", &view);
}

/// The fixtures hold what the scenario logged: a regenerated corpus that lost
/// its entries would make the comparisons above vacuous.
#[test]
fn upgrade_fixtures_hold_the_logged_history() {
    for v in VERSIONS {
        let e = expected(v);
        let n = |k: &str| e[k].as_array().map_or(0, Vec::len);
        assert_eq!(
            n("lifts"),
            144 + if v == "203ab56" { 0 } else { 8 },
            "{v} lifts"
        );
        assert_eq!(n("runs"), 23, "{v} runs");
        assert!(e["profile"].is_object(), "{v} profile");
        assert_eq!(e["safety_tier"], "Pain", "{v} keeps the pain report");
    }
    let full = expected("4728546");
    assert!(
        full["program"].is_object() && !full["week_plan"].as_array().unwrap().is_empty(),
        "4728546 plan"
    );
}
