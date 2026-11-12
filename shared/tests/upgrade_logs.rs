//! Upgrade corpus: event logs written by older builds must replay in this core
//! with no lost entries. Each `fixtures/upgrade/<sha>.log.ndjson` is the log file
//! an older build left on disk: its own shell's `Event.toJson()` lines, after its
//! own boot compaction rewrote them. `<sha>.history.json` is what that build's
//! core showed for the log on 2026-09-29 (UTC+2), copy text removed: the History
//! entries and profile for 203ab56 and 901d239, the whole view for 4728546 (the
//! last release before the 901d239 cleanup). `4728546.backup.ndjson` (that
//! build's Profile › Your data export) and `203ab56-plan-removed.log.ndjson` (a
//! removed plan after that build's rewrite) are read by UpgradeFixturesTest.kt.
//! Regenerate with `scripts/upgrade-fixtures/generate.sh`.
//!
//! - 203ab56 (2026-08-08): persisted every `SetToday`, a legacy set without
//!   `entry_id`, `endurance_intensity_pct_vo2max` in `SetProfile`, no `female`
//!   when false, measured `longest_recent_km`, the `streak` field.
//! - 4728546 (2026-10-29): the Kotlin compaction, `streak` and
//!   `longest_recent_km` as the shell computed them.
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

use serde_json::Value;
use shared::Event;
use shared::event::RETIRED_TAGS;
use shared::log::{LineFacts, compact_lines};

mod common;

const TODAY: i64 = 20725;
const OFF: i64 = 7200;

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/upgrade")
}

/// The builds with a log in the corpus: each `<sha>.log.ndjson`, sorted.
fn versions() -> Vec<String> {
    let out: Vec<String> = common::cases(&dir(), ".log.ndjson")
        .into_iter()
        .filter(|stem| !stem.contains('-'))
        .collect();
    assert!(!out.is_empty(), "no upgrade logs in {}", dir().display());
    out
}

fn lines(name: &str) -> Vec<String> {
    common::lines(&dir().join(name))
}

/// The view after replaying `lines` and the boot `SetToday`, plus the indices of
/// the lines this core rejects.
fn replay<'a>(lines: impl IntoIterator<Item = &'a String>) -> (Value, Vec<usize>) {
    let mut rejected = Vec::new();
    let mut events = Vec::new();
    for (i, line) in lines.into_iter().enumerate() {
        match serde_json::from_str::<Event>(line) {
            Ok(e) => events.push(e),
            Err(_) => rejected.push(i),
        }
    }
    let today = Event::SetToday {
        epoch_day: TODAY,
        utc_offset_sec: OFF,
    };
    (
        common::view_json(events.into_iter().chain([today])),
        rejected,
    )
}

/// View fields an old build showed that this core no longer sends, by path
/// with list indices as `[]`: the 901d239 cleanup's removals (mile twins, the
/// pre-computed plan targets, the retired Cooper, critical-speed and APRE
/// calculators) and the later ones. Any other field an old build showed must
/// still be there.
const REMOVED: &[&str] = &[
    ".adjustments[].neutral",
    ".apre",
    ".apre_input",
    ".autoreg_source.neutral",
    ".checkin_today",
    ".consistency.notes[].summary_mi",
    ".cooper",
    ".cooper_input",
    ".critical_speed",
    ".critical_speed_input",
    ".feedback.anchor_mastery",
    ".feedback.max_metrics",
    ".feedback.max_takeaways",
    ".feedback.message_mi",
    ".feedback.minimize_jargon",
    ".feedback.rationale_mandatory",
    ".feedback.suppresses_praise",
    ".grade_definitions[].label",
    ".guidance[].summary_mi",
    ".hr_zone_input.weeks_since_pace_test",
    ".hr_zone_input.weeks_since_recalc",
    ".hr_zones[].summary_mi",
    ".hypertrophy_figures.min_sets",
    ".hypertrophy_plan[].summary_mi",
    ".lifts[].e1rm_delta_kg",
    ".lifts[].e1rm_direction",
    ".next_session.adjustment.neutral",
    ".next_session.items[].intensity",
    ".next_session.items[].rep_count",
    ".next_session.items[].run_target.band",
    ".next_session.items[].run_target.hr_high_bpm",
    ".next_session.items[].run_target.hr_low_bpm",
    ".next_session.items[].run_target.hr_max_bpm",
    ".next_session.items[].run_target.hr_max_measured",
    ".next_session.items[].run_target.hr_rest_bpm",
    ".next_session.items[].run_target.pace_label_km",
    ".next_session.items[].run_target.pace_label_mi",
    ".next_session.items[].run_target.rpe_high",
    ".next_session.items[].run_target.rpe_low",
    ".next_session.items[].run_target.vdot",
    ".next_session.items[].safety_reason_mi",
    ".profile.endurance_intensity_pct_vo2max",
    ".race_prediction.agreed",
    ".race_prediction.high_sec",
    ".race_prediction.low_sec",
    ".readiness_summary[].detail",
    ".reference[].summary_mi",
    ".review_present",
    ".run_progress.best_efforts[].from_split",
    ".run_progress.weekly[].longest_km",
    ".run_progress.weekly[].moving_min",
    ".run_progress.weekly[].rated_runs",
    ".runs[].interval.variability_index",
    ".runs[].qc_dropped",
    ".runs[].split_pct",
    ".runs[].splits_km",
    ".runs[].splits_mi",
    ".srpe_load.prev_week_au",
    ".srpe_load.rated_runs",
    ".srpe_load.unrated_runs",
    ".srpe_load.week_au",
    ".train_blocked",
    ".training_load.atl",
    ".training_load.ctl",
    ".training_load.tsb",
    ".trend.neutral",
    ".weekly_report[].summary_mi",
    ".week_plan[].adjustment.neutral",
    ".week_plan[].items[].intensity",
    ".week_plan[].items[].rep_count",
    ".week_plan[].items[].run_target.band",
    ".week_plan[].items[].run_target.hr_high_bpm",
    ".week_plan[].items[].run_target.hr_low_bpm",
    ".week_plan[].items[].run_target.hr_max_bpm",
    ".week_plan[].items[].run_target.hr_max_measured",
    ".week_plan[].items[].run_target.hr_rest_bpm",
    ".week_plan[].items[].run_target.pace_label_km",
    ".week_plan[].items[].run_target.pace_label_mi",
    ".week_plan[].items[].run_target.rpe_high",
    ".week_plan[].items[].run_target.rpe_low",
    ".week_plan[].items[].run_target.vdot",
    ".week_plan[].items[].safety_reason_mi",
];

/// `path` with every list index as `[]`.
fn shape(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut in_index = false;
    for c in path.chars() {
        match c {
            '[' => in_index = true,
            ']' => {
                in_index = false;
                out.push_str("[]");
            }
            _ if in_index => {}
            _ => out.push(c),
        }
    }
    out
}

/// Every difference between the old build's `expected` and this core's `got`:
/// every object key `expected` holds is compared, and one `got` lacks is a
/// difference unless [`REMOVED`] names it (a field `got` adds is none), lists
/// must hold the same number of entries, numbers agree to 1e-9 relative. The
/// expected files hold no copy text but the best-effort labels.
fn diffs(path: &str, expected: &Value, got: &Value, out: &mut Vec<String>) {
    match (expected, got) {
        (Value::Object(e), Value::Object(g)) => {
            for (k, v) in e {
                let at = format!("{path}.{k}");
                match g.get(k) {
                    Some(w) => diffs(&at, v, w, out),
                    None if REMOVED.contains(&shape(&at).as_str()) => {}
                    None => out.push(format!("{at}: {v:.80}, now missing")),
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
        // Changed on purpose in review round 5 (BUGS.md 2026-10-31): the week's
        // run target is the km the plan prescribes, not the stated profile
        // figure, and a %1RM item's effort target comes from its plate-floored
        // load, so it can be up to one RPE lower.
        (Value::Number(_), Value::Number(_)) if path == ".run_progress.planned_weekly_km" => {}
        // A row an old build logged without an id showed id 0; it now carries
        // the id its date gives (`event::entry::row_id`), which the shell edits by.
        (Value::Number(a), Value::Number(b))
            if path.ends_with(".entry_id") && a.as_u64() == Some(0) && b.as_u64() != Some(0) => {}
        (Value::Number(a), Value::Number(b))
            if path.ends_with(".target_rpe")
                && (0.0..=1.0).contains(&(a.as_f64().unwrap() - b.as_f64().unwrap())) => {}
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
        // Copy, sent as its sentences now so the shell never splits the text.
        (Value::String(_), Value::Array(g))
            if path.ends_with(".why.basis") && g.iter().all(Value::is_string) => {}
        // "Nothing would sharpen this call" was sent as "-" ("" on a card with no
        // data need) and is null now (BUGS.md 2026-11-11).
        (Value::String(s), Value::Null)
            if path.ends_with(".why.improves") && (s == "-" || s.is_empty()) => {}
        // A best-effort label is copy: a rewording is not a difference.
        (Value::String(_), Value::String(_)) if shape(path).ends_with(".best_effort_labels[]") => {}
        _ if expected == got => {}
        _ => out.push(format!("{path}: {expected}, now {got}")),
    }
}

fn expected(v: &str) -> Value {
    serde_json::from_str(&fs::read_to_string(dir().join(format!("{v}.history.json"))).unwrap())
        .unwrap()
}

fn assert_shows(v: &str, file: &str, view: &Value) {
    // The profile's guidance row (the training-age row these builds listed
    // first in `guidance`) is sent apart as `profile_guidance` now.
    let mut view = view.clone();
    if let Some(Value::Array(profile)) = view.get("profile_guidance").cloned()
        && let Some(Value::Array(rows)) = view.get_mut("guidance")
    {
        rows.splice(0..0, profile);
    }
    let mut out = Vec::new();
    diffs("", &expected(v), &view, &mut out);
    assert!(
        out.is_empty(),
        "{file} shows other data than {v} did:\n{}",
        out.join("\n")
    );
}

#[test]
fn old_logs_replay_every_line_but_the_retired_calculators() {
    for v in versions() {
        let file = format!("{v}.log.ndjson");
        let lines = lines(&file);
        let (_, rejected) = replay(&lines);
        let unexpected: Vec<String> = rejected
            .iter()
            .filter(|&&i| {
                let tag =
                    common::tag(&serde_json::from_str(&lines[i]).expect("fixture line is JSON"));
                !RETIRED_TAGS.contains(&tag.as_str())
            })
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
    for v in versions() {
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
    for v in versions() {
        let file = format!("{v}.log.ndjson");
        let (view, _) = replay(&lines(&file));
        assert_shows(&v, &file, &view);
    }
}

/// The fixtures hold what the scenario logged: a regenerated corpus that lost
/// its entries would make the comparisons above vacuous.
#[test]
fn upgrade_fixtures_hold_the_logged_history() {
    for v in versions() {
        let e = expected(&v);
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
