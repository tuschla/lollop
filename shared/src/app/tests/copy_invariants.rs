//! Copy invariants over EVERY string the core sends: many varied
//! `ViewModel`s (every archetype, kg and lb, holds, empty states, a long
//! history, stale "today"s) are serialized and each string in the JSON is
//! walked. A violation names its field path, so a regression points straight
//! at the line that rendered it.
//!
//! Rules (owner copy rules 2026-10-18 in `planning/design/user-decisions.md`
//! plus mechanical hygiene): no NaN/inf/negative zero, no degenerate
//! `5–5` ranges, no hyphen-minus ranges between numbers (en dash), no double
//! or edge spaces, balanced brackets and quotes, no format placeholders, no
//! banned words (RIR, "short of failure", workout, load as a user word,
//! "recovery week"), no kg for a lb user, the miles twin of a km line present
//! and km-free, and no "today" pinned on an earlier reading.

use super::view_equivalence::{END_MON, big_history, ev};
use super::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;

const HOUR: i64 = 3_600;

/// Deterministic 0..1 sequence (same LCG as `view_equivalence`).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    fn pick(&mut self, lo: u8, hi: u8) -> u8 {
        lo + (self.next() * f64::from(hi - lo + 1))
            .floor()
            .min(f64::from(hi - lo)) as u8
    }
}

// ── String scanning ─────────────────────────────────────────────────────────

/// A numeric token: `[0-9]+([.:][0-9]+)*`, as a byte span of the string.
fn number_spans(s: &str) -> Vec<(usize, usize)> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let start = i;
            while i < b.len() {
                if b[i].is_ascii_digit() {
                    i += 1;
                } else if (b[i] == b'.' || b[i] == b':')
                    && i + 1 < b.len()
                    && b[i + 1].is_ascii_digit()
                {
                    i += 1;
                } else {
                    break;
                }
            }
            out.push((start, i));
        } else {
            i += 1;
        }
    }
    out
}

/// Whether the char ending right before byte `i` is an identifier char (a
/// letter or digit), i.e. a number starting at `i` is glued onto a word
/// (`s40279`, `Z2`), not a free-standing figure.
fn glued_before(s: &str, i: usize) -> bool {
    s[..i]
        .chars()
        .next_back()
        .is_some_and(|c| c.is_alphanumeric() || c == '/' || c == '.')
}

fn is_word_at(s: &str, start: usize, word: &str) -> bool {
    let end = start + word.len();
    let before = s[..start].chars().next_back();
    let after = s[end..].chars().next();
    !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
}

/// Every whole-word, case-insensitive occurrence of `word` in `s`.
fn has_word_ci(s: &str, word: &str) -> bool {
    let lower = s.to_lowercase();
    lower
        .match_indices(word)
        .any(|(i, _)| is_word_at(&lower, i, word))
}

fn has_word(s: &str, word: &str) -> bool {
    s.match_indices(word).any(|(i, _)| is_word_at(s, i, word))
}

/// `a – b` / `a-b` number pairs: (a, separator, b) with the separator span.
fn number_pairs(s: &str) -> Vec<(&str, &str, &str)> {
    let spans = number_spans(s);
    let mut out = Vec::new();
    for w in spans.windows(2) {
        let ((a0, a1), (b0, b1)) = (w[0], w[1]);
        let between = &s[a1..b0];
        let sep = between.trim();
        if (sep == "–" || sep == "-") && between.len() <= sep.len() + 2 {
            out.push((&s[a0..a1], sep, &s[b0..b1]));
        }
    }
    out
}

/// A unit word right after a number (`"5 km"`, `"5km"`).
fn number_then_unit(s: &str, unit: &str) -> bool {
    number_spans(s).into_iter().any(|(_, e)| {
        let rest = s[e..].trim_start_matches(' ');
        rest.starts_with(unit) && !rest[unit.len()..].starts_with(char::is_alphanumeric)
    })
}

/// Words of `s`, lowercased, split on everything but letters, digits and
/// in-word hyphens (`heart-rate` stays one word).
fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !(c.is_alphanumeric() || c == '-'))
        .map(|w| w.trim_matches('-').to_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

/// "load" as the user's word for the weight on the bar (the 2026-10-18 rule
/// "weight (not load)"): a load scheme heading (`Strength loading: 80–100%
/// 1RM`), a load figure (`load −10% today`, `load 80 kg`), or a weight-on-
/// the-bar phrase (`the same load`, `a lighter load`). Other senses keep the
/// word: the verb ("exercises that load it", spelled out in the 2026-10-18
/// copy fixes), joint stress ("under load"), and training-load metrics
/// ("running load", "this week's load", "effort load").
fn load_noun(s: &str) -> Option<String> {
    let w = words(s);
    for (i, word) in w.iter().enumerate() {
        let prev = i.checked_sub(1).map(|j| w[j].as_str());
        let hit = match word.as_str() {
            "loading" => {
                matches!(
                    prev,
                    Some("strength" | "power" | "hypertrophy" | "heavy" | "light")
                )
            }
            "load" | "loads" => matches!(
                prev,
                Some(
                    "same"
                        | "lighter"
                        | "heavier"
                        | "working"
                        | "planned"
                        | "bar"
                        | "starting"
                        | "target"
                        | "top-set"
                )
            ),
            _ => false,
        };
        if hit {
            return Some(word.clone());
        }
    }
    // A weight figure right after the word (`load −10%`, `load +5 kg`,
    // `load 80 kg`). A bare count (`Effort load 1179`) is the metric.
    let lower = s.to_lowercase();
    lower
        .match_indices("load ")
        .find(|(i, _)| {
            let rest = &lower[i + 5..];
            let signed = rest.starts_with(['−', '-', '+'])
                && rest.chars().nth(1).is_some_and(|c| c.is_ascii_digit());
            let weight = number_spans(rest).first().is_some_and(|&(a, e)| {
                let unit = rest[e..].trim_start();
                a == 0 && (unit.starts_with("kg") || unit.starts_with("lb"))
            });
            is_word_at(&lower, *i, "load") && (signed || weight)
        })
        .map(|_| "load".to_string())
}

/// A lb user's string still showing kg. Per-bodyweight units (`g/kg`,
/// `ml/kg/min`, "per kg of bodyweight") are not weights on the bar, and a
/// dual figure that restates the pounds right after (`74 kg (163 lb)`) shows
/// the user's unit.
fn kg_for_lb_user(s: &str) -> bool {
    s.match_indices("kg").any(|(i, _)| {
        if !is_word_at(s, i, "kg") || s[..i].ends_with('/') || s[..i].ends_with("per ") {
            return false;
        }
        let after = s[i + 2..].trim_start();
        let dual = after.starts_with('(')
            && after
                .find(')')
                .is_some_and(|close| after[..close].contains("lb"));
        !dual
    })
}

/// The copy rules every string obeys, each tagged with its family
/// (`number:`, `spacing:`, `bracket:`, `placeholder:`, `banned:`, `unit:`).
/// `key` is the JSON field the string sits in: `citation` rows are paper
/// references and keep their DOIs and titles verbatim.
fn string_violations(key: &str, s: &str, lb: bool) -> Vec<String> {
    let mut v = Vec::new();
    let citation = key == "citation";
    if s.contains("NaN") {
        v.push("number: NaN".into());
    }
    if has_word(s, "inf") || s.contains("Infinity") {
        v.push("number: inf".into());
    }
    // Negative zero: a minus sign in front of a figure that is all zeros.
    for (a, e) in number_spans(s) {
        let before = &s[..a];
        let signed = before.ends_with('-') || before.ends_with('−');
        if !signed {
            continue;
        }
        let sign_at = before.len() - before.chars().next_back().map_or(0, char::len_utf8);
        // A range separator (`4 - 0`, `3-0`) is a separate rule.
        if s[..sign_at]
            .trim_end()
            .ends_with(|c: char| c.is_ascii_digit())
        {
            continue;
        }
        if s[a..e].chars().all(|c| c == '0' || c == '.' || c == ':') {
            v.push(format!("number: negative zero `{}`", &s[sign_at..e]));
        }
    }
    if !citation {
        for (a, sep, b) in number_pairs(s) {
            if a == b {
                v.push(format!("number: degenerate range `{a}{sep}{b}`"));
            }
        }
        let spans = number_spans(s);
        for w in spans.windows(2) {
            let ((a0, a1), (b0, _)) = (w[0], w[1]);
            if glued_before(s, a0) {
                continue;
            }
            let between = &s[a1..b0];
            if between.trim() == "-" && between.len() <= 3 {
                v.push(format!("number: hyphen-minus range `{}`", &s[a0..b0 + 1]));
            }
        }
    }
    if s.contains("  ") {
        v.push("spacing: double space".into());
    }
    if s != s.trim() {
        v.push("spacing: leading/trailing whitespace".into());
    }
    if s.contains(['\n', '\t']) {
        v.push("spacing: control whitespace".into());
    }
    let count = |c: char| s.chars().filter(|&x| x == c).count();
    for (o, c) in [('(', ')'), ('[', ']'), ('“', '”')] {
        if count(o) != count(c) {
            v.push(format!("bracket: unbalanced `{o}{c}`"));
        }
    }
    if count('"') % 2 == 1 {
        v.push("bracket: unbalanced `\"`".into());
    }
    if s.contains('{') || s.contains('}') || s.contains("%s") || s.contains("%d") {
        v.push("placeholder: format placeholder".into());
    }
    if !citation {
        if has_word(s, "RIR") {
            v.push("banned: RIR".into());
        }
        for phrase in ["short of failure", "recovery week"] {
            if s.to_lowercase().contains(phrase) {
                v.push(format!("banned: {phrase}"));
            }
        }
        if has_word_ci(s, "workout") || has_word_ci(s, "workouts") {
            v.push("banned: workout".into());
        }
        if let Some(w) = load_noun(s) {
            v.push(format!("banned: `{w}` for weight"));
        }
        if lb && kg_for_lb_user(s) {
            v.push("unit: kg shown to a lb user".into());
        }
    }
    v
}

/// One distinct violation: the field path (array indices collapsed to `[]`),
/// the rule, and the first offending string.
#[derive(Debug)]
struct Hit {
    example: String,
    scenario: String,
    count: usize,
}

type Hits = BTreeMap<(String, String), Hit>;

fn record(out: &mut Hits, path: String, rule: String, example: &str, scenario: &str) {
    out.entry((path, rule))
        .and_modify(|h| h.count += 1)
        .or_insert(Hit {
            example: example.to_string(),
            scenario: scenario.into(),
            count: 1,
        });
}

/// Wire strings no shell renders: the shell model has no such field
/// (`LiftResultView.summary` is a CLI/debug line), or the calculator has no UI
/// (Cooper, Critical Speed, APRE: "those calculators have no UI",
/// `AppScaffold.kt`). Only the numeric-sanity family applies to them.
fn is_dormant(path: &str) -> bool {
    path == "lifts[].summary"
        || ["cooper", "critical_speed", "apre"]
            .iter()
            .any(|root| path == *root || path.starts_with(&format!("{root}[]")))
}

/// Every violation in `view`, keyed by (path, rule).
fn walk(v: &Value, path: &mut String, key: &str, lb: bool, scenario: &str, out: &mut Hits) {
    match v {
        Value::String(s) => {
            let dormant = is_dormant(path);
            for rule in string_violations(key, s, lb) {
                if dormant && !rule.starts_with("number:") {
                    continue;
                }
                record(out, path.clone(), rule, s, scenario);
            }
        }
        Value::Array(items) => {
            let len = path.len();
            path.push_str("[]");
            for item in items {
                walk(item, path, key, lb, scenario, out);
            }
            path.truncate(len);
        }
        Value::Object(map) => {
            for (k, child) in map {
                // The profile echo is the user's own input, not copy.
                if path.is_empty() && k == "profile" {
                    continue;
                }
                let len = path.len();
                if !path.is_empty() {
                    path.push('.');
                }
                path.push_str(k);
                // A plan item's `intensity_label` is shown only when it has no
                // `target_rpe` (the shell phrases an RPE target as reps left,
                // `PlanCompletion.setTargetBand`).
                let rpe_face =
                    k == "intensity_label" && map.get("target_rpe").is_some_and(|t| !t.is_null());
                if rpe_face {
                    let mut sub = Hits::new();
                    walk(child, path, k, lb, scenario, &mut sub);
                    for ((p, rule), h) in sub {
                        if rule != "banned: RIR" {
                            for _ in 0..h.count {
                                record(out, p.clone(), rule.clone(), &h.example, &h.scenario);
                            }
                        }
                    }
                } else {
                    walk(child, path, k, lb, scenario, out);
                }
                path.truncate(len);
            }
            if !is_dormant(path) {
                twin_violations(map, path, scenario, out);
            }
        }
        _ => {}
    }
}

/// The miles twins: where a km line has a `*_mi` sibling, the shell shows the
/// twin to a miles user (`summary_mi.ifBlank { summary }`), so a km line with
/// an empty twin leaks km, and a twin must never say km itself.
fn twin_violations(
    map: &serde_json::Map<String, Value>,
    path: &str,
    scenario: &str,
    out: &mut Hits,
) {
    let at_path = |field: &str| {
        if path.is_empty() {
            field.to_string()
        } else {
            format!("{path}.{field}")
        }
    };
    for (base, twin) in [
        ("summary", "summary_mi"),
        ("message", "message_mi"),
        ("safety_reason", "safety_reason_mi"),
        ("pace_label_km", "pace_label_mi"),
    ] {
        let (Some(Value::String(b)), Some(Value::String(t))) = (map.get(base), map.get(twin))
        else {
            continue;
        };
        let km_line = number_then_unit(b, "km") || b.contains("/km");
        if km_line && t.is_empty() {
            let rule = "unit: km line without its miles twin".to_string();
            record(out, at_path(twin), rule, b, scenario);
        }
        if number_then_unit(t, "km") || t.contains("/km") || has_word(t, "km") {
            record(
                out,
                at_path(twin),
                "unit: miles twin says km".into(),
                t,
                scenario,
            );
        }
    }
    // A run head in km is converted by the shell from `run_head_km`.
    if let (Some(Value::String(h)), Some(km)) = (map.get("run_head"), map.get("run_head_km"))
        && number_then_unit(h, "km")
        && km.is_null()
    {
        let rule = "unit: km run head without run_head_km".to_string();
        record(out, at_path("run_head_km"), rule, h, scenario);
    }
}

// ── Scenarios ───────────────────────────────────────────────────────────────

/// Shell-side local time: `hour` o'clock on local day `d` as a UTC stamp.
fn at(d: i64, hour: i64, offset: i64) -> i64 {
    d * DAY_SEC + hour * HOUR - offset
}

fn today(app: &Engine, model: &mut Model, d: i64, offset: i64) {
    let _ = app.update(
        Event::SetToday {
            epoch_day: d,
            utc_offset_sec: offset,
        },
        model,
    );
}

/// A short GPS track heading north at `pace` s/km.
fn track(start: i64, km: f64, pace: f64) -> Vec<GpsPoint> {
    let n = ((km * pace) / 5.0) as usize + 1;
    let deg_per_s = 1.0 / pace / 111.0;
    (0..n)
        .map(|i| GpsPoint {
            lat: 48.0 + deg_per_s * 5.0 * i as f64,
            lon: 11.0 + ((i as f64) / 30.0).sin() * 0.0003,
            observed_at: start + 5 * i as i64,
            accuracy_m: 5.0,
        })
        .collect()
}

/// Sends `e` and records it, so a history can be replayed under overlays
/// (`Model` is not `Clone`).
fn send(app: &Engine, model: &mut Model, log: &mut Vec<Event>, e: Event) {
    log.push(e.clone());
    let _ = app.update(e, model);
}

/// Replays an event list into a fresh model.
fn replay(events: &[Event]) -> (Engine, Model) {
    let app = Engine;
    let mut model = Model::default();
    for e in events {
        let _ = app.update(e.clone(), &mut model);
    }
    (app, model)
}

/// The event log of `days` of follow-the-plan training for `profile`,
/// ending on `end` (local day, `offset` east of UTC). The first `days / 3`
/// are pre-plan baseline logs; then the plan starts on a Monday and each
/// day's hero session is logged with some misses, lighter days, short sets
/// and skipped days, plus morning check-ins.
fn drive(profile: Profile, seed: u64, days: i64, end: i64, offset: i64) -> Vec<Event> {
    let app = Engine;
    let mut model = Model::default();
    let mut log = Vec::new();
    let mut rng = Lcg(seed);
    let start = end - days;
    send(
        &app,
        &mut model,
        &mut log,
        Event::SetProfile(Profile {
            effective_from_epoch_day: Some(start),
            ..profile.clone()
        }),
    );
    let calis = profile.resistance_modality == ResistanceModality::Calisthenics;
    let runs = profile.running_days_per_week > 0;
    let lifts = profile.weekly_sets > 0;
    let plan_day = {
        let d = start + days / 3;
        d + (7 - mon0_weekday(d)) % 7
    };
    let mut id: u64 = seed % 1_000 * 10_000;
    let mut next_id = || {
        id += 1;
        id
    };
    for d in start..=end {
        send(
            &app,
            &mut model,
            &mut log,
            Event::SetToday {
                epoch_day: d,
                utc_offset_sec: offset,
            },
        );
        if d == plan_day {
            send(
                &app,
                &mut model,
                &mut log,
                Event::GeneratePlan { start_epoch_day: d },
            );
        }
        if rng.next() < 0.8 {
            send(
                &app,
                &mut model,
                &mut log,
                ev(json!({"SubmitCheckin": {
                    "observed_at": at(d, 7, offset),
                    "sleep_quality": rng.pick(2, 5),
                    "soreness": rng.pick(1, 4),
                    "mood": rng.pick(2, 5),
                    "fatigue": rng.pick(1, 4),
                    "stress": rng.pick(1, 4),
                    "resting_hr_bpm": 50.0 + f64::from(rng.pick(0, 8)),
                    "hrv_rmssd_ms": 50.0 + f64::from(rng.pick(0, 25)),
                }})),
            );
        }
        let skip_day = rng.next() < 0.15;
        let mut t = at(d, 17, offset);
        if d < plan_day {
            if skip_day {
                continue;
            }
            let wd = mon0_weekday(d);
            if lifts && wd % 2 == 0 {
                let ex: &[(&str, f64)] = if calis {
                    &[("Push-up", 0.0), ("Pull-up", 0.0), ("Squat", 0.0)]
                } else {
                    &[
                        ("Back Squat", 100.0),
                        ("Bench Press", 70.0),
                        ("Deadlift", 130.0),
                    ]
                };
                for &(name, kg) in ex {
                    for _ in 0..3 {
                        let ev = with_entry_id(
                            log_set(name, kg, u32::from(rng.pick(5, 9)), 7.5, t),
                            next_id(),
                        );
                        send(&app, &mut model, &mut log, ev);
                        t += 150;
                    }
                }
                if calis {
                    send(&app, &mut model, &mut log, cal_hold("Plank", 45, 7.0, t));
                }
            }
            if runs && wd % 2 == 1 {
                let km = 4.0 + rng.next() * 6.0;
                send(
                    &app,
                    &mut model,
                    &mut log,
                    log_run(km, km * 6.0, 72.0, 0.0, at(d, 7, offset) + 1_800, next_id()),
                );
            }
            continue;
        }
        let vm = app.view(&model);
        let Some(ns) = vm.next_session.filter(|s| s.epoch_day == d) else {
            continue;
        };
        if skip_day {
            continue;
        }
        let lighter = rng.next() < 0.15;
        for item in &ns.items {
            if item.done {
                continue;
            }
            if let Some(km) = item.run_head_km.or(item.run_target.as_ref().map(|_| 5.0)) {
                let km = km * if lighter { 0.6 } else { 1.0 };
                let begin = at(d, 7, offset);
                if rng.next() < 0.3 {
                    send(
                        &app,
                        &mut model,
                        &mut log,
                        log_run_track(track(begin, km, 330.0), 74.0, 0.0, begin, next_id()),
                    );
                } else {
                    let mut e = log_run(km, km * 5.8, 74.0, 0.0, begin, next_id());
                    if let Event::LogRun { session_rpe, .. } = &mut e {
                        *session_rpe = Some(rng.pick(3, 8));
                    }
                    send(&app, &mut model, &mut log, e);
                }
                continue;
            }
            if item.sets == 0 {
                continue;
            }
            let kg = item.suggested_load_kg.or(item.load_kg).unwrap_or(0.0)
                * if lighter { 0.85 } else { 1.0 };
            let rpe = item.target_rpe.unwrap_or(8.0);
            let sets = if rng.next() < 0.1 {
                item.sets.saturating_sub(1).max(1)
            } else {
                item.sets
            };
            for _ in 0..sets {
                let reps = if rng.next() < 0.1 {
                    item.reps_low.saturating_sub(1).max(1)
                } else {
                    item.reps_high.max(1)
                };
                let e = if item.timed_hold {
                    cal_hold(&item.exercise, item.hold_high_sec.max(10), rpe, t)
                } else {
                    with_entry_id(
                        log_set(&item.exercise, kg, u32::from(reps), rpe, t),
                        next_id(),
                    )
                };
                send(&app, &mut model, &mut log, e);
                t += 150;
            }
        }
    }
    log
}

/// The calculator queries, so their rows (and inputs) render too.
fn calculators(app: &Engine, model: &mut Model) {
    for e in [
        json!({"PredictRace": {"recent_distance_m": 10_000.0, "recent_time_sec": 2_900.0, "goal_distance_m": 21_097.5, "weekly_km": 35.0}}),
        json!({"ComputeHrZones": {"age_years": 41.0, "resting_hr_bpm": 55.0}}),
        json!({"ComputeProtein": {"bodyweight_kg": 74.0, "masters": false, "deficit": true}}),
        json!({"PlanHypertrophyMeso": {"muscle": "chest", "weeks": 4, "not_growing": false, "recovering_easily": true}}),
        json!({"ComputeCooper": {"distance_m_12min": 2_650.0}}),
        json!({"ComputeCriticalSpeed": {"efforts": [{"distance_m": 1_200.0, "time_sec": 210.0}, {"distance_m": 5_000.0, "time_sec": 1_260.0}]}}),
        json!({"ComputeApre": {"scheme": "Apre6", "reps": 9, "current_load_kg": 82.5}}),
    ] {
        let _ = app.update(ev(e), model);
    }
}

fn archetypes(end: i64) -> Vec<(&'static str, Profile)> {
    let lb = |p: Profile| Profile {
        weight_lb: true,
        ..p
    };
    vec![
        ("strength", strength_profile()),
        ("strength_lb", lb(strength_profile())),
        (
            "strength_beginner_meet",
            Profile {
                progression_cadence: ProgressionCadence::EverySession,
                meet_epoch_day: Some(end + 20),
                ..strength_profile()
            },
        ),
        (
            "power_lb",
            lb(Profile {
                lift_goal: LiftGoal::Power,
                concurrent_goal: ConcurrentGoal::Power,
                ..strength_profile()
            }),
        ),
        (
            "hypertrophy_advanced",
            Profile {
                lift_goal: LiftGoal::Hypertrophy,
                concurrent_goal: ConcurrentGoal::Hypertrophy,
                progression_cadence: ProgressionCadence::MonthToMonth,
                advanced: true,
                weekly_sets: 16,
                ..base_profile()
            },
        ),
        (
            "hypertrophy_lb_female",
            lb(Profile {
                lift_goal: LiftGoal::Hypertrophy,
                concurrent_goal: ConcurrentGoal::Hypertrophy,
                female: Some(true),
                bodyweight_kg: Some(61.0),
                ..base_profile()
            }),
        ),
        (
            "calisthenics",
            Profile {
                calis_equipment: Some(CalisEquipment {
                    pull_up_bar: true,
                    dip_bars: true,
                    rings: false,
                }),
                ..calisthenics_profile()
            },
        ),
        ("calisthenics_lb_no_gear", lb(calisthenics_profile())),
        ("half_marathon", running_profile()),
        (
            "five_k_race",
            Profile {
                goal_distance: GoalDistance::FiveK,
                goal_race_epoch_day: Some(end + 30),
                running_days_per_week: 4,
                running_km_per_week: 25.0,
                ..running_profile()
            },
        ),
        (
            "c25k_masters",
            Profile {
                goal_distance: GoalDistance::C25k,
                running_days_per_week: 3,
                running_km_per_week: 6.0,
                age_years: Some(67.0),
                progression_cadence: ProgressionCadence::EverySession,
                ..running_profile()
            },
        ),
        (
            "marathon_hybrid_lb",
            lb(Profile {
                goal_race_epoch_day: Some(end + 60),
                age_years: Some(38.0),
                resting_hr_bpm: Some(48.0),
                measured_hr_max: Some(188.0),
                ..sample_profile()
            }),
        ),
        (
            "runner_strength",
            Profile {
                runner_strength: true,
                goal_distance: GoalDistance::TenK,
                concurrent_goal: ConcurrentGoal::EndurancePriority,
                running_days_per_week: 4,
                running_km_per_week: 30.0,
                weekly_sets: 6,
                ..base_profile()
            },
        ),
        (
            "heat_altitude_layoff",
            Profile {
                env_temp_c: Some(33.0),
                env_altitude_m: Some(2_300.0),
                weeks_off: Some(6.0),
                layoff_returned_epoch_day: Some(end - 10),
                ..sample_profile()
            },
        ),
        (
            "youth",
            Profile {
                age_years: Some(15.0),
                ..sample_profile()
            },
        ),
        (
            "parq_positive",
            Profile {
                health: HealthScreen {
                    parq_positive: true,
                    ..HealthScreen::default()
                },
                ..sample_profile()
            },
        ),
        (
            "pregnant",
            Profile {
                female: Some(true),
                health: HealthScreen {
                    pregnant: true,
                    ..HealthScreen::default()
                },
                ..strength_profile()
            },
        ),
        (
            "no_training",
            Profile {
                weekly_sets: 0,
                running_days_per_week: 0,
                ..base_profile()
            },
        ),
    ]
}

/// Holds, reviews and edits laid over a history so their copy renders.
fn overlays(end: i64) -> Vec<(&'static str, Vec<Event>)> {
    let d = end;
    vec![
        ("plain", vec![]),
        (
            "pain_hold",
            vec![ev(json!({"SubmitReadiness": {
                "signal": "Pain", "value": 1.0, "observed_at": d * DAY_SEC + 9 * HOUR,
                "pain": {"kind": "TendonLoadRelated", "severity": 3, "trend": "Stable", "location": "Left knee"},
            }}))],
        ),
        (
            "sharp_pain",
            vec![ev(json!({"SubmitReadiness": {
                "signal": "Pain", "value": 1.0, "observed_at": d * DAY_SEC + 9 * HOUR,
                "pain": {"kind": "SharpJoint", "severity": 4, "trend": "WorseningAfter24h", "location": "Right ankle"},
            }}))],
        ),
        (
            "illness",
            vec![ev(
                json!({"SubmitReadiness": {"signal": "Illness", "value": 1.0, "observed_at": (d - 1) * DAY_SEC + 8 * HOUR}}),
            )],
        ),
        (
            "illness_resolved",
            vec![
                ev(
                    json!({"SubmitReadiness": {"signal": "Illness", "value": 1.0, "observed_at": (d - 4) * DAY_SEC + 8 * HOUR}}),
                ),
                ev(
                    json!({"ResolveHold": {"signal": "Illness", "observed_at": (d - 1) * DAY_SEC + 8 * HOUR}}),
                ),
            ],
        ),
        (
            "reviews",
            vec![
                Event::SubmitReview(SessionReview {
                    rpe_load_gap_sessions: Some(2),
                    positive_split_pct: Some(6.0),
                    observed_at: d * DAY_SEC + 19 * HOUR,
                    ..SessionReview::default()
                }),
                Event::SubmitWeeklyCheckin(SessionReview {
                    overtraining_signal_count: 3,
                    overtraining_signal_weeks: Some(2.0),
                    decline_weeks: Some(2),
                    observed_at: d * DAY_SEC + 20 * HOUR,
                    ..SessionReview::default()
                }),
            ],
        ),
        (
            "high_rpe_rhr",
            vec![
                ev(
                    json!({"SubmitReadiness": {"signal": "Rpe", "value": 9.5, "observed_at": d * DAY_SEC + 6 * HOUR}}),
                ),
                ev(
                    json!({"SubmitCheckin": {"observed_at": d * DAY_SEC + 7 * HOUR, "resting_hr_bpm": 66.0, "hrv_rmssd_ms": 30.0, "sleep_quality": 1, "soreness": 5, "mood": 1, "fatigue": 5, "stress": 5}}),
                ),
            ],
        ),
    ]
}

/// Walks one view. `stale` marks a view whose today has nothing logged and no
/// check-in, so every reading in it is from an earlier day.
fn collect(app: &Engine, model: &Model, scenario: &str, stale: bool, out: &mut Hits) {
    let vm = app.view(model);
    let lb = vm.profile.as_ref().is_some_and(|p| p.weight_lb);
    let json = serde_json::to_value(&vm).expect("view serializes");
    if stale {
        today_on_stale(&vm, &json, scenario, out);
    }
    walk(&json, &mut String::new(), "", lb, scenario, out);
}

/// Every string of `v` with its collapsed path.
fn strings<'a>(v: &'a Value, path: &mut String, out: &mut Vec<(String, &'a str)>) {
    match v {
        Value::String(s) => out.push((path.clone(), s)),
        Value::Array(items) => {
            let len = path.len();
            path.push_str("[]");
            for item in items {
                strings(item, path, out);
            }
            path.truncate(len);
        }
        Value::Object(map) => {
            for (k, child) in map {
                let len = path.len();
                if !path.is_empty() {
                    path.push('.');
                }
                path.push_str(k);
                strings(child, path, out);
                path.truncate(len);
            }
        }
        _ => {}
    }
}

/// Phrases that describe something already logged or reported. On a day with
/// nothing logged, none of them may be pinned on "today".
const READING_PHRASES: [&str; 9] = [
    "came up short",
    "today's top set",
    "top set today",
    "you're stronger today",
    "you're weaker today",
    "logged today",
    "checked in today",
    "today's run",
    "today's check-in",
];

/// "today" attached to an earlier reading: a stale view (nothing logged, no
/// check-in today) must not say a reading happened today, and must not claim
/// today's check-in.
fn today_on_stale(vm: &ViewModel, json: &Value, scenario: &str, out: &mut Hits) {
    if vm.checked_in_today {
        record(
            out,
            "checked_in_today".into(),
            "today: true with no check-in today".into(),
            "true",
            scenario,
        );
    }
    let mut all = Vec::new();
    strings(json, &mut String::new(), &mut all);
    for (path, s) in all {
        if path == "profile" || path.starts_with("profile.") || is_dormant(&path) {
            continue;
        }
        let lower = s.to_lowercase();
        let pinned = READING_PHRASES
            .iter()
            .find(|p| lower.contains(**p) && (p.contains("today") || has_word_ci(s, "today")));
        if let Some(p) = pinned {
            record(
                out,
                path,
                format!("today: `{p}` on a day with nothing logged"),
                s,
                scenario,
            );
        }
    }
}

fn report(hits: &[(&(String, String), &Hit)]) -> String {
    let mut s = String::new();
    for ((path, rule), h) in hits {
        s.push_str(&format!(
            "\n- {path}: {rule} (x{}, e.g. [{}]) `{}`",
            h.count, h.scenario, h.example
        ));
    }
    s
}

/// Empty states: no profile, a profile with no today, each archetype with no
/// logs, a fresh plan, and the calculators.
fn sweep_empty(hits: &mut Hits) {
    let app = Engine;
    let mut model = Model::default();
    collect(&app, &model, "empty", false, hits);
    today(&app, &mut model, END_MON, 0);
    collect(&app, &model, "empty+today", true, hits);
    for (name, p) in archetypes(END_MON) {
        let mut model = Model::default();
        let _ = app.update(Event::SetProfile(p), &mut model);
        collect(&app, &model, &format!("{name}/no-today"), false, hits);
        today(&app, &mut model, END_MON, 0);
        collect(&app, &model, &format!("{name}/no-logs"), true, hits);
        let _ = app.update(
            Event::GeneratePlan {
                start_epoch_day: END_MON,
            },
            &mut model,
        );
        collect(&app, &model, &format!("{name}/fresh-plan"), true, hits);
        calculators(&app, &mut model);
        collect(&app, &model, &format!("{name}/calculators"), true, hits);
    }
}

/// Every archetype after 7 weeks of logs (follow-the-plan from week 3) at four UTC
/// offsets, plain and under two rotating overlays (each overlay meets six
/// archetypes; `COPY_SWEEP_FULL=1` runs every pairing), viewed on the last
/// logged day, the next day (nothing logged yet) and 3 days later (stale
/// readings).
fn sweep_archetype(i: usize, name: &str, p: Profile) -> Hits {
    let mut hits = Hits::new();
    let end = END_MON + 3;
    let full = std::env::var_os("COPY_SWEEP_FULL").is_some();
    let offset = [0, 2 * HOUR, -5 * HOUR, 9 * HOUR + 1_800][i % 4];
    let log = drive(p, 0xC0FF_EE00 + i as u64, 49, end, offset);
    for (j, (oname, events)) in overlays(end).into_iter().enumerate() {
        if !(j == 0 || full || (j - 1) % 3 == i % 3) {
            continue;
        }
        let (app, mut model) = replay(&log);
        for e in events {
            let _ = app.update(e, &mut model);
        }
        if oname == "plain" {
            calculators(&app, &mut model);
        }
        let tag = format!("{name}/{oname}");
        collect(&app, &model, &tag, false, &mut hits);
        for later in [1, 3] {
            today(&app, &mut model, end + later, offset);
            // Overlays are dated on `end`, so only `plain` is truly stale.
            let stale = oname == "plain";
            collect(&app, &model, &format!("{tag}/+{later}d"), stale, &mut hits);
        }
    }
    hits
}

/// The 420-day multi-year history (`view_equivalence`) in kg and in lb, over
/// several todays.
fn sweep_long_history() -> Hits {
    let mut hits = Hits::new();
    let (app, mut model) = big_history(420);
    for d in [END_MON, END_MON + 2, END_MON - 1, END_MON + 9] {
        today(&app, &mut model, d, 0);
        // END_MON carries the session straddling its midnight; later days
        // have nothing logged.
        collect(&app, &model, &format!("big/kg/{d}"), d > END_MON, &mut hits);
    }
    let p = Profile {
        weight_lb: true,
        ..model.profile.clone().expect("big history has a profile")
    };
    let _ = app.update(Event::SetProfile(p), &mut model);
    for d in [END_MON, END_MON + 2] {
        today(&app, &mut model, d, 0);
        collect(&app, &model, &format!("big/lb/{d}"), d > END_MON, &mut hits);
    }
    hits
}

fn sweep_empty_hits() -> Hits {
    let mut hits = Hits::new();
    sweep_empty(&mut hits);
    hits
}

/// All sweeps, run once per test binary and shared by the rule-family tests.
/// The independent sweeps run on a few worker threads (each `Engine` view
/// memo is thread-local), merged in a fixed order so the first example of a
/// violation is deterministic.
static ALL_HITS: std::sync::LazyLock<Hits> = std::sync::LazyLock::new(|| {
    type Job = Box<dyn FnOnce() -> Hits + Send>;
    let mut jobs: Vec<Job> = vec![Box::new(sweep_empty_hits), Box::new(sweep_long_history)];
    for (i, (name, p)) in archetypes(END_MON + 3).into_iter().enumerate() {
        jobs.push(Box::new(move || sweep_archetype(i, name, p)));
    }
    let n = jobs.len();
    let queue = std::sync::Mutex::new(jobs.into_iter().enumerate().collect::<Vec<_>>());
    let results = std::sync::Mutex::new((0..n).map(|_| None).collect::<Vec<Option<Hits>>>());
    let workers = std::thread::available_parallelism().map_or(2, |p| p.get().clamp(2, 6));
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                while let Some((k, job)) = queue.lock().unwrap().pop() {
                    let h = job();
                    results.lock().unwrap()[k] = Some(h);
                }
            });
        }
    });
    let mut all = Hits::new();
    for part in results.into_inner().unwrap().into_iter().flatten() {
        for ((path, rule), h) in part {
            all.entry((path, rule))
                .and_modify(|a| a.count += h.count)
                .or_insert(h);
        }
    }
    all
});

fn all_hits() -> &'static Hits {
    &ALL_HITS
}

/// Known bugs: (path, rule prefix). Excluded from the family tests so those
/// keep guarding everything else; each has its own ignored `BUG:` test.
const KNOWN: &[(&str, &str)] = &[
    // guidance/strength.rs:80,86 "Strength loading: …" / "Power loading: …".
    ("guidance[].summary", "banned: `loading`"),
    // plan_views/adjustments.rs:571 "load −10% today".
    ("next_session.items[].adjusted_note", "banned: `load`"),
    ("week_plan[].items[].adjusted_note", "banned: `load`"),
];

fn is_known(path: &str, rule: &str) -> bool {
    KNOWN.iter().any(|(p, r)| path == *p && rule.starts_with(r))
}

/// The family's hits, minus the known bugs.
fn assert_family(prefixes: &[&str]) {
    let bad: Vec<_> = all_hits()
        .iter()
        .filter(|((path, rule), _)| {
            prefixes.iter().any(|p| rule.starts_with(p)) && !is_known(path, rule)
        })
        .collect();
    assert!(
        bad.is_empty(),
        "{} distinct copy violations:{}",
        bad.len(),
        report(&bad)
    );
}

/// The known bug at `path` / `rule` is gone.
fn assert_fixed(path: &str, rule: &str) {
    let bad: Vec<_> = all_hits()
        .iter()
        .filter(|((p, r), _)| p == path && r.starts_with(rule))
        .collect();
    assert!(bad.is_empty(), "still present:{}", report(&bad));
}

// ── Scanner self-checks (the rules flag what they claim to) ─────────────────

#[test]
fn the_scanner_flags_each_rule_and_passes_clean_copy() {
    let flagged = |s: &str, lb: bool| string_violations("summary", s, lb);
    for clean in [
        "5×3 @ 85 kg · ~2 reps left, 6–8 reps, −0.5%",
        "Pace 4:50–5:05 /km, 150–162 bpm",
        "Pain: skip exercises that load it today",
        "Effort load 1179 this week · training load rising",
        "The ratio of this week's load to your usual load; your running load",
        "Progress only while bending the wrist back under load is pain-free",
    ] {
        assert!(
            flagged(clean, false).is_empty(),
            "{clean}: {:?}",
            flagged(clean, false)
        );
    }
    for (s, lb, rule) in [
        ("weight NaN kg", false, "NaN"),
        ("pace inf /km", false, "inf"),
        ("change −0%", false, "negative zero"),
        ("change -0.0 kg", false, "negative zero"),
        ("100–100 kg", false, "degenerate range"),
        ("5 - 5 km", false, "degenerate range"),
        ("4:50–4:50 /km", false, "degenerate range"),
        ("6-8 reps", false, "hyphen-minus range"),
        ("two  spaces", false, "double space"),
        (" lead", false, "leading/trailing"),
        ("open (paren", false, "unbalanced"),
        ("say \"hi", false, "unbalanced"),
        ("hello {name}", false, "placeholder"),
        ("hello %s", false, "placeholder"),
        ("@ RIR 2", false, "RIR"),
        ("stop 2 reps short of failure", false, "short of failure"),
        ("Workout done", false, "workout"),
        ("keep the same load next time", false, "load"),
        ("Strength loading: 80–100% 1RM", false, "loading"),
        ("load −10% today", false, "load"),
        ("a Recovery week", false, "recovery week"),
        ("5×3 @ 85 kg", true, "kg shown"),
    ] {
        let v = flagged(s, lb);
        assert!(
            v.iter().any(|r| r.contains(rule)),
            "`{s}` must flag {rule}: {v:?}"
        );
    }
    // Pounds, per-bodyweight units and a dual figure are clean for a lb user;
    // DOIs keep their hyphens.
    assert!(flagged("5×3 @ 185 lb", true).is_empty());
    assert!(flagged("1.6–2.2 g/kg × 74 kg (163 lb), VO2max 48 ml/kg/min", true).is_empty());
    assert!(string_violations("citation", "doi:10.1007/s40279-015-0303-1", false).is_empty());
}

// ── Rule families over every swept view ─────────────────────────────────────

#[test]
fn copy_numbers_are_sane() {
    assert_family(&["number:"]);
}

#[test]
fn copy_spacing_brackets_and_placeholders_are_clean() {
    assert_family(&["spacing:", "bracket:", "placeholder:"]);
}

#[test]
fn copy_uses_no_banned_words() {
    assert_family(&["banned:"]);
}

#[test]
fn copy_units_follow_the_user() {
    assert_family(&["unit:"]);
}

#[test]
fn copy_never_pins_today_on_an_earlier_reading() {
    assert_family(&["today:"]);
}

// ── Known copy bugs (each one KNOWN entry above) ────────────────────────────

#[test]
#[ignore = "BUG: guidance row says \"Strength loading: …\" / \"Power loading: …\" (load for weight, 2026-10-18 copy rule)"]
fn bug_guidance_loading_heading_uses_load_for_weight() {
    assert_fixed("guidance[].summary", "banned: `loading`");
}

#[test]
#[ignore = "BUG: a ReduceLoadPct item note reads \"load −10% today\" (load for weight, 2026-10-18 copy rule)"]
fn bug_load_cut_note_uses_load_for_weight() {
    assert_fixed("next_session.items[].adjusted_note", "banned: `load`");
    assert_fixed("week_plan[].items[].adjusted_note", "banned: `load`");
}
