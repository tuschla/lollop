//! RevLedger review probes (2026-10-25) for the ledger/derivation slice of the
//! 2026-10-23 fixes: compaction after the Rule 4 / `run_line_between` removal,
//! the re-derived run %HRmax and spike baseline, day-aged birthday profiles.
//! Every probe that documented a defect was `#[ignore = "REVIEW: ..."]` until
//! FixLedger2 fixed it (RevLedger F1–F4, now standing guards); the rest pin
//! claims the fix reports make. `compaction_parity_corpus_is_current` owns the
//! committed Rust ↔ Kotlin compaction corpus (`ReviewLedgerTest`).

use super::*;
use crate::compact_event_log;

const D0: i64 = 20_400;

/// Epoch day of a civil date (Hinnant's `days_from_civil`).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

// ── helpers ───────────────────────────────────────────────────────────────

struct R(u64);
impl R {
    fn new(seed: u64) -> Self {
        R(seed ^ 0x5DEE_CE66_D1CE_4E5B)
    }
    fn u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.u64() % n }
    }
    fn int(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1) as u64) as i64
    }
    fn unit(&mut self) -> f64 {
        (self.u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }
    fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
        xs[self.below(xs.len() as u64) as usize]
    }
}

fn today(epoch_day: i64, utc_offset_sec: i64) -> Event {
    Event::SetToday {
        epoch_day,
        utc_offset_sec,
    }
}

#[allow(clippy::too_many_arguments)]
fn run(km: f64, min: f64, hr: f64, wire_longest: f64, at: i64, id: u64, bpm: Option<f64>) -> Event {
    Event::LogRun {
        distance_km: km,
        duration_min: min,
        hr_pct_max: hr,
        longest_recent_km: wire_longest,
        observed_at: at,
        entry_id: id,
        workout_type: None,
        session_rpe: None,
        avg_hr_bpm: bpm,
        utc_offset_sec: None,
    }
}

fn track(start: i64, n: i64, step_m: f64, acc: f32) -> Vec<GpsPoint> {
    (0..n)
        .map(|i| GpsPoint {
            lat: 48.1 + (i as f64) * step_m / 111_000.0,
            lon: 11.5,
            observed_at: start + i * 5,
            accuracy_m: acc,
        })
        .collect()
}

fn gps_run(points: Vec<GpsPoint>, at: i64, id: u64, bpm: Option<f64>) -> Event {
    Event::LogRunTrack {
        points,
        hr_pct_max: 0.0,
        longest_recent_km: 0.0,
        observed_at: at,
        entry_id: id,
        workout_type: None,
        session_rpe: None,
        segment_starts: Vec::new(),
        avg_hr_bpm: bpm,
        utc_offset_sec: None,
    }
}

/// `e` (a LogRun / LogRunTrack) as the shell logs it: stamped with the
/// device's UTC offset at that moment.
fn stamped(mut e: Event, offset: i64) -> Event {
    match &mut e {
        Event::LogRun { utc_offset_sec, .. } | Event::LogRunTrack { utc_offset_sec, .. } => {
            *utc_offset_sec = Some(offset);
        }
        _ => unreachable!("not a run log line"),
    }
    e
}

fn set(ex: &str, kg: f64, reps: u32, at: i64, id: u64) -> Event {
    Event::LogSet {
        exercise: ex.into(),
        weight_kg: kg,
        reps,
        rpe: 8.0,
        observed_at: at,
        entry_id: id,
        hold_sec: None,
    }
}

fn checkin(at: i64, rng: &mut R) -> Event {
    Event::SubmitCheckin(CheckinInput {
        observed_at: at,
        sleep_quality: Some(rng.int(1, 5) as u8),
        soreness: Some(rng.int(1, 5) as u8),
        mood: Some(rng.int(1, 5) as u8),
        fatigue: Some(rng.int(1, 5) as u8),
        stress: Some(rng.int(1, 5) as u8),
        resting_hr_bpm: rng.chance(0.6).then(|| 48.0 + rng.int(0, 15) as f64),
        hrv_rmssd_ms: rng.chance(0.4).then(|| 40.0 + rng.int(0, 40) as f64),
    })
}

fn profile(eff: Option<i64>, birth: Option<i64>, hr_max: Option<f64>, age: Option<f64>) -> Event {
    Event::SetProfile(Profile {
        effective_from_epoch_day: eff,
        birth_epoch_day: birth,
        measured_hr_max: hr_max,
        age_years: age,
        running_days_per_week: 3,
        running_km_per_week: 20.0,
        ..base_profile()
    })
}

fn vm_json(vm: &ViewModel) -> String {
    serde_json::to_string(vm).expect("view serializes")
}

/// Replay on a fresh thread (cold thread-local memos); final view JSON.
fn replay_cold(events: Vec<Event>) -> String {
    std::thread::Builder::new()
        .stack_size(32 << 20)
        .spawn(move || {
            let app = Engine;
            let mut model = Model::default();
            for e in events {
                let _ = app.update(e, &mut model);
            }
            vm_json(&app.view(&model))
        })
        .expect("spawn")
        .join()
        .expect("replay panicked")
}

fn replay(events: &[Event]) -> (Engine, Model) {
    let app = Engine;
    let mut model = Model::default();
    for e in events {
        let _ = app.update(e.clone(), &mut model);
    }
    (app, model)
}

/// The first differing JSON path, for messages.
fn first_diff(a: &str, b: &str) -> String {
    fn walk(a: &serde_json::Value, b: &serde_json::Value, p: &mut String) -> Option<String> {
        use serde_json::Value as V;
        match (a, b) {
            (V::Object(x), V::Object(y)) => {
                for (k, va) in x {
                    let l = p.len();
                    p.push('.');
                    p.push_str(k);
                    match y.get(k) {
                        None => return Some(format!("{p}: only left")),
                        Some(vb) => {
                            if let Some(d) = walk(va, vb, p) {
                                return Some(d);
                            }
                        }
                    }
                    p.truncate(l);
                }
                y.keys()
                    .find(|k| !x.contains_key(*k))
                    .map(|k| format!("{p}.{k}: only right"))
            }
            (V::Array(x), V::Array(y)) => {
                for (i, (va, vb)) in x.iter().zip(y).enumerate() {
                    let l = p.len();
                    p.push_str(&format!("[{i}]"));
                    if let Some(d) = walk(va, vb, p) {
                        return Some(d);
                    }
                    p.truncate(l);
                }
                (x.len() != y.len()).then(|| format!("{p}: len {} vs {}", x.len(), y.len()))
            }
            _ if a == b => None,
            _ => {
                let s = |v: &V| {
                    let s = v.to_string();
                    s.chars().take(200).collect::<String>()
                };
                Some(format!("{p}: {} vs {}", s(a), s(b)))
            }
        }
    }
    let va: serde_json::Value = serde_json::from_str(a).unwrap();
    let vb: serde_json::Value = serde_json::from_str(b).unwrap();
    walk(&va, &vb, &mut String::new()).unwrap_or_else(|| "JSON-equal".into())
}

// ── Random ledger generator: runs/profiles/clock heavy ─────────────────────

#[derive(Clone, Copy)]
struct RunRef {
    id: u64,
    at: i64,
    gps: Option<(i64, i64)>,
}

/// A seeded event log stressing exactly what the fixes touched: typed-bpm
/// runs, GPS runs, legacy (id 0) rows with colliding stamps, deletes and
/// amends across other runs, trims, profile edits with birthdays / measured
/// HRmax / effective days (incl. undated), offset-changing SetToday (both
/// directions), check-ins, clears.
fn gen_log(seed: u64, len: usize) -> Vec<Event> {
    let mut g = R::new(seed);
    let mut ev = Vec::with_capacity(len);
    let mut t = D0 * DAY_SEC + 7 * 3600;
    let mut runs: Vec<RunRef> = Vec::new();
    let mut sets: Vec<(u64, i64)> = Vec::new();
    let mut next_id = 1_760_000_000_000u64;
    let offsets = [0i64, 3600, 7200, -18_000, 19_800, 50_400, -43_200];
    let mut offset = g.pick(&offsets);
    ev.push(today(D0, offset));
    // Birthdays near the window so 18th / 40th birthdays fall inside it,
    // including 29 February birthdays.
    let feb29_2008 = 13_938; // 2008-02-29
    let births = [
        D0 - 18 * 365 - g.int(-30, 30),
        D0 - 40 * 365 - g.int(-30, 30),
        feb29_2008,
        D0 - 25 * 365,
    ];
    for _ in 0..len {
        t += g.int(2 * 3600, 2 * DAY_SEC);
        let day = t.div_euclid(DAY_SEC);
        let roll = g.below(100);
        let id = |g: &mut R, next: &mut u64| {
            if g.chance(0.2) {
                0
            } else {
                *next += 1;
                *next
            }
        };
        match roll {
            0..=7 => {
                if g.chance(0.25) {
                    offset = g.pick(&offsets);
                }
                let d = if g.chance(0.15) {
                    day - g.int(1, 90)
                } else {
                    day
                };
                ev.push(today(d, offset));
            }
            8..=13 => {
                let eff = if g.chance(0.08) {
                    None
                } else {
                    Some(day - g.int(0, 3) * i64::from(g.chance(0.2)))
                };
                let birth = g.chance(0.6).then(|| g.pick(&births));
                let hr_max = g.chance(0.4).then(|| 170.0 + g.int(0, 30) as f64);
                let age = (birth.is_none() && g.chance(0.6)).then(|| 20.0 + g.int(0, 40) as f64);
                let mut p = profile(eff, birth, hr_max, age);
                if let Event::SetProfile(pr) = &mut p {
                    // The shell stamps an edit with its instant; legacy lines carry none.
                    pr.edited_at = g.chance(0.6).then_some(t);
                    if g.chance(0.5) {
                        pr.recent_race = Some(RecentRace {
                            distance_km: 10.0,
                            time_sec: g.pick(&[2700, 2700, 2700, 2640]),
                            epoch_day: g.chance(0.15).then(|| day - g.int(0, 60)),
                        });
                    }
                }
                ev.push(p);
            }
            14 => ev.push(Event::ClearProfile),
            15..=36 => {
                let rid = id(&mut g, &mut next_id);
                // Back-dated entries and legacy stamp collisions.
                let mut at = if g.chance(0.2) {
                    t - g.int(0, 40) * DAY_SEC
                } else {
                    t
                };
                if rid == 0
                    && g.chance(0.3)
                    && let Some(r) = runs.iter().rev().find(|r| r.gps.is_none())
                {
                    at = r.at;
                }
                let km = (g.int(10, 250) as f64) / 10.0;
                let bpm = g.chance(0.5).then(|| 120.0 + g.int(0, 65) as f64);
                let hr = if bpm.is_some() {
                    0.0
                } else {
                    g.int(0, 95) as f64
                };
                let wire = if g.chance(0.1) {
                    g.int(1, 30) as f64
                } else {
                    0.0
                };
                // Current shells stamp the device offset; legacy lines carry none.
                let e = run(km, km * 6.0, hr, wire, at, rid, bpm);
                ev.push(if g.chance(0.75) {
                    stamped(e, offset)
                } else {
                    e
                });
                runs.push(RunRef {
                    id: rid,
                    at,
                    gps: None,
                });
            }
            37..=43 => {
                let rid = id(&mut g, &mut next_id);
                let n = g.int(12, 80);
                let acc = if g.chance(0.15) { 120.0 } else { 5.0 };
                let start = t - n * 5;
                let pts = track(start, n, 12.0 + g.int(0, 8) as f64, acc);
                let bpm = g.chance(0.4).then(|| 130.0 + g.int(0, 50) as f64);
                let e = gps_run(pts, t, rid, bpm);
                ev.push(if g.chance(0.75) {
                    stamped(e, offset)
                } else {
                    e
                });
                runs.push(RunRef {
                    id: rid,
                    at: t,
                    gps: Some((start, t - 5)),
                });
            }
            44..=53 if !runs.is_empty() => {
                let i = g.below(runs.len() as u64) as usize;
                let r = runs[i];
                ev.push(Event::DeleteEntry {
                    kind: EntryKind::Run,
                    entry_id: r.id,
                    observed_at_fallback: r.at,
                });
                if g.chance(0.7) {
                    runs.remove(i);
                }
            }
            54..=62 if !runs.is_empty() => {
                let i = g.below(runs.len() as u64) as usize;
                let r = runs[i];
                let new_at = if r.id != 0 && g.chance(0.3) {
                    r.at + g.int(-10, 10) * DAY_SEC
                } else {
                    r.at
                };
                let km = (g.int(10, 250) as f64) / 10.0;
                let bpm = g.chance(0.5).then(|| 120.0 + g.int(0, 65) as f64);
                ev.push(Event::AmendRun {
                    entry_id: r.id,
                    distance_km: km,
                    duration_min: km * 6.0,
                    hr_pct_max: if bpm.is_some() { 0.0 } else { 70.0 },
                    longest_recent_km: 0.0,
                    observed_at: new_at,
                    observed_at_fallback: r.at,
                    workout_type: None,
                    session_rpe: None,
                    avg_hr_bpm: bpm,
                });
                runs[i] = RunRef {
                    id: r.id,
                    at: new_at,
                    gps: None,
                };
            }
            63..=66 => {
                if let Some(r) = runs.iter().rev().find(|r| r.gps.is_some()).copied() {
                    let (a, b) = r.gps.unwrap();
                    let cut = (b - a) / 3;
                    ev.push(Event::TrimRunTrack {
                        entry_id: r.id,
                        observed_at_fallback: r.at,
                        keep_from_sec: a + cut * i64::from(g.chance(0.5)),
                        keep_until_sec: b - cut * i64::from(g.chance(0.5)),
                    });
                }
            }
            67..=68 if !runs.is_empty() => {
                let r = runs[g.below(runs.len() as u64) as usize];
                ev.push(Event::AmendRunMeta {
                    entry_id: r.id,
                    observed_at_fallback: r.at,
                    session_rpe: Some(g.int(1, 10) as u8),
                    workout_type: None,
                });
            }
            69..=78 => {
                let sid = id(&mut g, &mut next_id);
                ev.push(set(
                    "Back Squat",
                    60.0 + g.int(0, 60) as f64,
                    g.int(1, 10) as u32,
                    t,
                    sid,
                ));
                sets.push((sid, t));
            }
            79..=81 if !sets.is_empty() => {
                let i = g.below(sets.len() as u64) as usize;
                let (sid, at) = sets.remove(i);
                ev.push(Event::DeleteEntry {
                    kind: EntryKind::Set,
                    entry_id: sid,
                    observed_at_fallback: at,
                });
            }
            82..=93 => ev.push(checkin(t - g.int(0, 3) * DAY_SEC, &mut g)),
            94 => {
                ev.push(Event::ClearRuns);
                runs.clear();
            }
            95 => ev.push(Event::GeneratePlan {
                start_epoch_day: day,
            }),
            96 => ev.push(Event::ClearCheckins),
            _ => ev.push(today(day, offset)),
        }
    }
    ev.push(today(t.div_euclid(DAY_SEC), offset));
    ev
}

/// Stored %HRmax vs a from-scratch derivation (a row a legacy undated edit
/// pinned keeps its earlier figure by design), and stored card spike
/// baseline vs the pure function the doc comments promise.
fn derived_state_violations(model: &Model) -> Vec<String> {
    let mut out = Vec::new();
    for r in model.runs.iter().filter(|r| !r.hr_pinned) {
        let want = super::super::ingest::derived_hr_pct_max(model, r);
        if r.hr_pct_max.to_bits() != want.to_bits() {
            out.push(format!(
                "run {}@{} hr {} != derived {}",
                r.entry_id, r.observed_at, r.hr_pct_max, want
            ));
        }
    }
    for (i, r) in model.runs.iter().enumerate() {
        let longest = model
            .runs
            .iter()
            .enumerate()
            .filter(|&(j, o)| {
                j != i && super::super::run_metrics::in_spike_window(o.observed_at, r.observed_at)
            })
            .map(|(_, o)| super::super::run_metrics::spike_km(o))
            .fold(0.0_f64, f64::max);
        let want = r.wire_longest_recent_km.max(longest);
        if r.longest_recent_km.to_bits() != want.to_bits() {
            out.push(format!(
                "run {}@{} spike baseline {} != pure {}",
                r.entry_id, r.observed_at, r.longest_recent_km, want
            ));
        }
    }
    out
}

fn iters(default: usize) -> usize {
    std::env::var("REVLEDGER_ITERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

/// CLAIM (log.rs, ingest.rs): with `run_line_between` gone, compaction stays
/// replay-equivalent because the run %HRmax and spike baseline are pure
/// functions of the surviving model. Full vs compacted, cold threads, plus a
/// compacted prefix + appended suffix (the shell's launch-then-append path).
#[test]
fn review_compaction_equivalence_on_run_heavy_logs() {
    let mut fails = Vec::new();
    for case in 0..iters(150) as u64 {
        let seed = 0xA11C_E000 + case;
        let log = gen_log(seed, 120);
        let full = replay_cold(log.clone());
        let compacted = replay_cold(compact_event_log(log.clone()));
        if full != compacted {
            fails.push(format!(
                "seed {seed:#x}: compacted differs: {}",
                first_diff(&full, &compacted)
            ));
            continue;
        }
        let cut = log.len() / 2;
        let mut relaunch = compact_event_log(log[..cut].to_vec());
        relaunch.extend_from_slice(&log[cut..]);
        let relaunched = replay_cold(relaunch);
        if full != relaunched {
            fails.push(format!(
                "seed {seed:#x}: compact-prefix+suffix differs: {}",
                first_diff(&full, &relaunched)
            ));
        }
    }
    assert!(
        fails.is_empty(),
        "{} failing seeds:\n{}",
        fails.len(),
        fails.join("\n")
    );
}

/// CLAIM (model.rs LoggedRun docs): stored `hr_pct_max` / `longest_recent_km`
/// always equal a from-scratch derivation from the surviving model, whatever
/// the event order (log order independence).
#[test]
fn review_stored_run_figures_equal_their_pure_derivation() {
    let mut fails = Vec::new();
    for case in 0..iters(150) as u64 {
        let seed = 0xB0B0_0000 + case;
        let log = gen_log(seed, 120);
        let (_, model) = replay(&log);
        let v = derived_state_violations(&model);
        if !v.is_empty() {
            fails.push(format!(
                "seed {seed:#x}: {}",
                v[..v.len().min(3)].join("; ")
            ));
        }
    }
    assert!(
        fails.is_empty(),
        "{} failing seeds:\n{}",
        fails.len(),
        fails.join("\n")
    );
}

/// Cases and log length of the committed Rust ↔ Kotlin compaction parity
/// corpus.
const PARITY_CASES: u64 = 30;
const PARITY_LEN: usize = 120;

/// The Rust ↔ Kotlin compaction parity corpus,
/// `shared/tests/fixtures/compaction_parity/<seed>.{in,expected}.ndjson`:
/// [`gen_log`] logs and their `compact_event_log` output, which the Kotlin
/// `ReviewLedgerTest` compacts with `EventLog.compact` and must match line for
/// line. Fails when the committed corpus is stale (a generator, wire or
/// compaction change); `BLESS_COMPACTION_PARITY=1` rewrites it. Also pins that
/// the corpus exercises every rule the two must agree on.
#[test]
fn compaction_parity_corpus_is_current() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/compaction_parity");
    let bless = std::env::var_os("BLESS_COMPACTION_PARITY").is_some();
    if bless {
        std::fs::create_dir_all(&dir).unwrap();
    }
    let ndjson = |events: &[Event]| -> String {
        events
            .iter()
            .map(|e| serde_json::to_string(e).unwrap() + "\n")
            .collect()
    };
    let mut stale = Vec::new();
    let mut wanted = std::collections::BTreeSet::new();
    // Coverage of the rules the two compactions must agree on.
    let (mut dropped, mut same_day_kept, mut barrier_kept, mut stamped_runs) = (0, 0, 0, 0);
    for case in 0..PARITY_CASES {
        let seed = 0xC0DE_0000 + case;
        // Compaction never reads a track's fixes (only its identity), so the
        // committed corpus keeps 3 per GPS run to stay small.
        let mut log = gen_log(seed, PARITY_LEN);
        for e in &mut log {
            if let Event::LogRunTrack { points, .. } = e {
                points.truncate(3);
            }
        }
        let out = compact_event_log(log.clone());
        dropped += log.len() - out.len();
        let kept: Vec<&Profile> = out
            .iter()
            .filter_map(|e| match e {
                Event::SetProfile(p) => Some(p),
                _ => None,
            })
            .collect();
        for (i, p) in kept.iter().enumerate() {
            let later = &kept[i + 1..];
            // Kept although a later kept line's key supersedes it: a pinning
            // undated edit stands between them.
            barrier_kept += usize::from(later.iter().any(|q| history_key(q) <= history_key(p)));
            same_day_kept += usize::from(
                p.edited_at.is_some()
                    && later.iter().any(|q| {
                        q.effective_from_epoch_day == p.effective_from_epoch_day
                            && q.edited_at > p.edited_at
                    }),
            );
        }
        stamped_runs += out
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    Event::LogRun {
                        utc_offset_sec: Some(_),
                        ..
                    } | Event::LogRunTrack {
                        utc_offset_sec: Some(_),
                        ..
                    }
                )
            })
            .count();
        for (name, body) in [
            (format!("{seed:x}.in.ndjson"), ndjson(&log)),
            (format!("{seed:x}.expected.ndjson"), ndjson(&out)),
        ] {
            let path = dir.join(&name);
            if bless {
                std::fs::write(&path, &body).unwrap();
            } else if std::fs::read_to_string(&path).ok().as_deref() != Some(body.as_str()) {
                stale.push(name.clone());
            }
            wanted.insert(name);
        }
    }
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if !wanted.contains(&name) {
            if bless {
                std::fs::remove_file(&path).unwrap();
            } else {
                stale.push(format!("{name} (not generated)"));
            }
        }
    }
    assert!(
        stale.is_empty(),
        "compaction parity corpus is stale ({} files, e.g. {:?}); re-bless with \
         BLESS_COMPACTION_PARITY=1 cargo test -p shared compaction_parity_corpus_is_current",
        stale.len(),
        &stale[..stale.len().min(5)]
    );
    assert!(
        dropped > 0 && same_day_kept > 0 && barrier_kept > 0 && stamped_runs > 0,
        "the corpus must exercise every compaction rule: dropped {dropped}, same-day timed edits kept \
         {same_day_kept}, edits kept behind a pinning undated edit {barrier_kept}, stamped runs {stamped_runs}"
    );
}

// ── Targeted compaction cases after the run_line_between removal ───────────

fn assert_equiv(name: &str, log: Vec<Event>) {
    let full = replay_cold(log.clone());
    let compacted = replay_cold(compact_event_log(log));
    assert!(
        full == compacted,
        "{name}: {}",
        first_diff(&full, &compacted)
    );
}

#[test]
fn review_delete_across_another_run_is_replay_equivalent() {
    let at = D0 * DAY_SEC + 8 * 3600;
    // A 20 km run, then a 10 km run 3 days later (its card baseline is 20),
    // then the 20 km run is deleted: the old guard kept both lines.
    assert_equiv(
        "delete across",
        vec![
            today(D0 + 5, 3600),
            run(20.0, 120.0, 70.0, 0.0, at, 1, None),
            run(10.0, 60.0, 0.0, 0.0, at + 3 * DAY_SEC, 2, Some(150.0)),
            Event::DeleteEntry {
                kind: EntryKind::Run,
                entry_id: 1,
                observed_at_fallback: at,
            },
        ],
    );
    // Amend chain across another run, then delete; legacy ids sharing a stamp.
    assert_equiv(
        "amend chain + legacy collision",
        vec![
            today(D0 + 5, 0),
            run(5.0, 30.0, 70.0, 0.0, at, 0, None),
            run(15.0, 90.0, 70.0, 0.0, at, 0, None),
            run(8.0, 48.0, 70.0, 0.0, at + DAY_SEC, 3, None),
            Event::AmendRun {
                entry_id: 0,
                distance_km: 25.0,
                duration_min: 150.0,
                hr_pct_max: 0.0,
                longest_recent_km: 0.0,
                observed_at: at,
                observed_at_fallback: at,
                workout_type: None,
                session_rpe: None,
                avg_hr_bpm: Some(160.0),
            },
            run(9.0, 54.0, 70.0, 0.0, at + 2 * DAY_SEC, 4, None),
            Event::DeleteEntry {
                kind: EntryKind::Run,
                entry_id: 0,
                observed_at_fallback: at,
            },
            Event::AmendRun {
                entry_id: 0,
                distance_km: 30.0,
                duration_min: 150.0,
                hr_pct_max: 80.0,
                longest_recent_km: 0.0,
                observed_at: at,
                observed_at_fallback: at,
                workout_type: None,
                session_rpe: None,
                avg_hr_bpm: None,
            },
        ],
    );
}

// ── Birthday / timezone / %HRmax re-derivation ─────────────────────────────

/// The %HRmax a typed-bpm run shows, by entry id.
fn hr_of(app: &Engine, model: &Model, id: u64) -> f64 {
    app.view(model)
        .runs
        .iter()
        .find(|r| r.entry_id == id)
        .expect("run row")
        .hr_pct_max
}

/// CLAIM (BUGS 2026-10-23 "derived from the run's day now"): a typed-bpm run is
/// converted with the Tanaka age of ITS day: the day before the 40th birthday
/// uses 39, the birthday 40, and a later SetToday past the birthday leaves the
/// earlier run on 39. Feb 29 birthday completes on 1 March in a common year.
#[test]
fn review_bpm_run_uses_the_age_of_its_own_day_across_birthdays() {
    // 40th birthday on D0 + 10.
    let b40 = {
        let (y, m, d) = crate::running::civil_from_days(D0 + 10);
        days_from_civil(y - 40, m, d)
    };
    let eve = (D0 + 9) * DAY_SEC + 12 * 3600;
    let bday = (D0 + 10) * DAY_SEC + 12 * 3600;
    let log = vec![
        today(D0, 0),
        profile(Some(D0), Some(b40), None, None),
        run(8.0, 48.0, 0.0, 0.0, eve, 1, Some(162.0)),
        run(8.0, 48.0, 0.0, 0.0, bday, 2, Some(162.0)),
        today(D0 + 30, 0),
    ];
    let (app, model) = replay(&log);
    let tanaka = |a: f64| 208.0 - 0.7 * a;
    let pct = |max: f64| (162.0 / max * 1000.0_f64).round() / 10.0;
    assert_eq!(
        hr_of(&app, &model, 1),
        pct(tanaka(39.0)),
        "eve of the 40th birthday"
    );
    assert_eq!(hr_of(&app, &model, 2), pct(tanaka(40.0)), "40th birthday");
}

/// CLAIM (BUGS 2026-10-23, `ingest::aged_on`): after the 18th birthday a past
/// day keeps its under-18 gate (SAFE-PEDS-001); a 29 February birthday
/// completes on 1 March in a common year. Fails at HEAD (the birthday rewrote
/// every history entry's flag).
#[test]
fn review_youth_gate_is_aged_per_day_across_the_18th_birthday() {
    // Feb 29 2008 birthday: 2026-02-28 → 17 (youth held), 2026-03-01 → 18.
    let feb29 = days_from_civil(2008, 2, 29);
    let feb28_26 = days_from_civil(2026, 2, 28);
    let log = vec![
        today(feb28_26 - 60, 0),
        profile(Some(feb28_26 - 60), Some(feb29), None, None),
        today(feb28_26 + 30, 0),
    ];
    let (_, model) = replay(&log);
    let youth = |d| profile_for_day(&model, d).map(|p| p.health.youth);
    assert_eq!(youth(feb28_26), Some(true), "28 Feb 2026: still 17");
    assert_eq!(youth(feb28_26 + 1), Some(false), "1 Mar 2026: 18");
}

/// RevLedger F2 (fixed): a bpm run's %HRmax is converted on the clock it was
/// LOGGED under (the wire `utc_offset_sec` the shell stamps, kept as
/// `LoggedRun::logged_utc_offset_sec`), so travelling across time zones never
/// re-converts an old run with another day's age / profile: not on the
/// `SetToday` itself, nor on a later re-derivation (an unrelated profile edit,
/// an amend of the row), and a compacted log agrees. A legacy row with no
/// stamp still reads the current offset (its logged one is unknown).
#[test]
fn review_past_run_hr_moves_with_a_later_timezone_change() {
    let b40 = {
        let (y, m, d) = crate::running::civil_from_days(D0 + 10);
        days_from_civil(y - 40, m, d)
    };
    // 00:30 local on the 40th birthday at UTC+2 (= 22:30 UTC the day before).
    let at = (D0 + 10) * DAY_SEC - 2 * 3600 + 30 * 60;
    let log = vec![
        today(D0 + 20, 7200),
        profile(Some(D0), Some(b40), None, None),
        stamped(run(8.0, 48.0, 0.0, 0.0, at, 1, Some(162.0)), 7200),
        run(8.0, 48.0, 0.0, 0.0, at, 2, Some(162.0)),
    ];
    let (app, mut model) = replay(&log);
    let before = hr_of(&app, &model, 1);
    assert_eq!(before, 90.0, "162 bpm at Tanaka(40) = 180");
    // Weeks later the user is in New York (UTC-5): nothing about the run changed.
    let later = vec![
        today(D0 + 40, -18_000),
        // An edit effective after the run re-derives every run's figure.
        profile(Some(D0 + 40), Some(b40), None, None),
        // So does an amend that restates the row.
        Event::AmendRun {
            entry_id: 1,
            distance_km: 8.0,
            duration_min: 48.0,
            hr_pct_max: 0.0,
            longest_recent_km: 0.0,
            observed_at: at,
            observed_at_fallback: at,
            workout_type: None,
            session_rpe: None,
            avg_hr_bpm: Some(162.0),
        },
    ];
    for (i, e) in later.iter().enumerate() {
        let _ = app.update(e.clone(), &mut model);
        assert_eq!(
            before,
            hr_of(&app, &model, 1),
            "the run's %HRmax moved with the viewer's time zone (step {i})"
        );
    }
    assert_eq!(
        hr_of(&app, &model, 2),
        89.7,
        "an unstamped legacy row reads UTC-5: the eve, Tanaka(39)"
    );
    let mut full = log;
    full.extend(later);
    assert_equiv("time-zone change", full);
}

/// RevLedger F3 (fixed): a profile edit carries the instant it was made
/// (`Profile::edited_at`), and on its effective day it is in force only from
/// then (`profile_at`): an evening edit leaves the %HRmax of a run logged that
/// morning alone, converts a run logged after it, and a second same-day edit
/// leaves the first in force for the hours between. A legacy edit with no
/// instant still covers its whole day. A compacted log agrees.
#[test]
fn review_same_day_profile_edit_rewrites_a_logged_run() {
    let at = |h: i64| D0 * DAY_SEC + h * 3600;
    let timed = |hr_max: f64, h: i64| {
        let mut e = profile(Some(D0), None, Some(hr_max), Some(30.0));
        if let Event::SetProfile(p) = &mut e {
            p.edited_at = Some(at(h));
        }
        e
    };
    let base = vec![
        today(D0, 0),
        profile(Some(D0 - 30), None, None, Some(30.0)),
        run(8.0, 48.0, 0.0, 0.0, at(7), 1, Some(160.0)),
    ];
    let (app, mut model) = replay(&base);
    let before = hr_of(&app, &model, 1);
    assert_eq!(before, 85.6, "160 bpm at Tanaka(30) = 187");
    let _ = app.update(timed(200.0, 20), &mut model);
    assert_eq!(
        before,
        hr_of(&app, &model, 1),
        "the morning run was re-converted by the evening edit"
    );
    let _ = app.update(run(8.0, 48.0, 0.0, 0.0, at(21), 2, Some(160.0)), &mut model);
    assert_eq!(
        hr_of(&app, &model, 2),
        80.0,
        "a run logged after the edit uses it"
    );

    // Morning edit, run, evening edit: the run keeps the morning edit.
    let mut two = base.clone();
    two.insert(2, timed(190.0, 6));
    two.push(timed(200.0, 20));
    let (app, model) = replay(&two);
    assert_eq!(
        hr_of(&app, &model, 1),
        84.2,
        "160 bpm at the morning edit's 190"
    );
    assert_equiv("two same-day edits", two);

    // A legacy edit with no instant is in force for its whole day.
    let mut legacy = base;
    legacy.push(profile(Some(D0), None, Some(200.0), Some(30.0)));
    let (app, model) = replay(&legacy);
    assert_eq!(hr_of(&app, &model, 1), 80.0);
}

/// RevLedger F4 (fixed): a legacy undated SetProfile replaces the profile
/// history but applies to logged runs only from its position in the log: the
/// runs logged before it keep the %HRmax they had (`LoggedRun::hr_pinned`),
/// even across a later re-derivation, while a run logged after it uses it.
/// `ClearProfile` releases the pin. A compacted log (which keeps the history
/// a pin read) agrees.
#[test]
fn review_undated_profile_rewrites_all_past_runs() {
    let log = vec![
        today(D0, 0),
        profile(Some(D0 - 300), None, Some(190.0), Some(30.0)),
        run(8.0, 48.0, 0.0, 0.0, (D0 - 200) * DAY_SEC, 1, Some(160.0)),
    ];
    let (app, mut model) = replay(&log);
    let before = hr_of(&app, &model, 1);
    assert_eq!(before, 84.2, "160 bpm at the measured 190");
    let later = vec![
        profile(None, None, Some(170.0), Some(30.0)),
        today(D0 + 5, 0),
        run(8.0, 48.0, 0.0, 0.0, (D0 + 1) * DAY_SEC, 2, Some(160.0)),
        // A dated edit re-derives every unpinned run; the pinned one stays.
        profile(Some(D0 + 5), None, Some(180.0), Some(30.0)),
    ];
    for (i, e) in later.iter().enumerate() {
        let _ = app.update(e.clone(), &mut model);
        assert_eq!(
            before,
            hr_of(&app, &model, 1),
            "the undated edit re-converted a run logged before it (step {i})"
        );
    }
    assert_eq!(
        hr_of(&app, &model, 2),
        94.1,
        "a run after the undated edit uses it (160 / 170)"
    );
    let mut full = log;
    full.extend(later);
    assert_equiv("undated edit after a run", full.clone());
    full.push(Event::ClearProfile);
    let (app, model) = replay(&full);
    assert_eq!(
        hr_of(&app, &model, 1),
        0.0,
        "ClearProfile: no profile, the wire figure"
    );
    assert_equiv("undated edit, then ClearProfile", full);
}

// ── Rule 4 removal: root-cause check + log growth ──────────────────────────

/// CLAIM (BUGS 2026-10-23): Rule 4 was unsound because a later SetToday can
/// move today back into the window of a check-in the window dropped. Fails at
/// HEAD (Rule 4 present), passes after the removal.
#[test]
fn review_rule4_root_cause_settoday_back_reads_old_checkins() {
    let mut g = R::new(7);
    let mut log = Vec::new();
    for d in 0..120 {
        log.push(checkin((D0 + d) * DAY_SEC + 7 * 3600, &mut g));
    }
    log.push(today(D0 + 120, 0));
    let launched = compact_event_log(log.clone());
    let back = today(D0 + 40, 0);
    let mut full = log;
    full.push(back.clone());
    let mut relaunch = launched;
    relaunch.push(back);
    let (a, b) = (replay_cold(full), replay_cold(relaunch));
    assert!(a == b, "{}", first_diff(&a, &b));
}

/// Log size probe for a 3-year life (prints, never fails): bytes by family.
#[test]
#[ignore = "probe: prints log composition of a 3-year log (run explicitly)"]
fn review_measure_three_year_log_composition() {
    let log = three_year_log(false);
    let mut by: std::collections::BTreeMap<&str, (usize, usize)> = Default::default();
    for e in &log {
        let s = serde_json::to_string(e).unwrap();
        let k = match e {
            Event::SubmitCheckin(_) => "SubmitCheckin",
            Event::LogSet { .. } => "LogSet",
            Event::LogRun { .. } => "LogRun",
            Event::LogRunTrack { .. } => "LogRunTrack",
            Event::SetToday { .. } => "SetToday",
            Event::SetProfile(_) => "SetProfile",
            _ => "other",
        };
        let x = by.entry(k).or_default();
        x.0 += 1;
        x.1 += s.len() + 1;
    }
    let compacted = compact_event_log(log.clone());
    let total: usize = log
        .iter()
        .map(|e| serde_json::to_string(e).unwrap().len() + 1)
        .sum();
    let ctotal: usize = compacted
        .iter()
        .map(|e| serde_json::to_string(e).unwrap().len() + 1)
        .sum();
    let ck: usize = compacted
        .iter()
        .filter(|e| matches!(e, Event::SubmitCheckin(_)))
        .map(|e| serde_json::to_string(e).unwrap().len() + 1)
        .sum();
    eprintln!("REVLEDGER log composition (lines, bytes): {by:?}");
    eprintln!(
        "REVLEDGER total {total} B, compacted {ctotal} B, check-ins in compacted {ck} B ({:.1} %)",
        100.0 * ck as f64 / ctotal as f64
    );
}

// ── Perf: 3-year replay ────────────────────────────────────────────────────

/// A 3-year life: daily SetToday + check-in, 3 lift sessions × 15 sets a week,
/// 4 runs a week (every other one GPS, 1 Hz, 40 min), a profile edit a month.
/// `reverse_runs`: the runs arrive newest-first (a bulk import).
fn three_year_log(reverse_runs: bool) -> Vec<Event> {
    let mut g = R::new(3);
    let mut log = Vec::new();
    let mut runs = Vec::new();
    let mut id = 1_700_000_000_000u64;
    let birth = D0 - 39 * 365;
    for d in 0..(3 * 365) {
        let day = D0 + d;
        let t = day * DAY_SEC + 6 * 3600;
        log.push(today(day, 3600));
        log.push(checkin(t, &mut g));
        if d % 30 == 0 {
            log.push(profile(
                Some(day),
                Some(birth),
                (d > 400).then_some(188.0),
                None,
            ));
        }
        if [0, 2, 4].contains(&(d % 7)) {
            for s in 0..15 {
                id += 1;
                log.push(set(
                    "Back Squat",
                    80.0 + (d / 30) as f64,
                    5,
                    t + 3600 + s * 180,
                    id,
                ));
            }
        }
        if [1, 3, 5, 6].contains(&(d % 7)) {
            id += 1;
            let at = t + 10 * 3600;
            let e = if (d / 7 + d) % 2 == 0 {
                gps_run(track(at - 2400, 2400, 2.8, 5.0), at, id, Some(150.0))
            } else {
                run(6.0 + (d % 5) as f64, 40.0, 0.0, 0.0, at, id, Some(148.0))
            };
            runs.push(e);
        }
    }
    if reverse_runs {
        runs.reverse();
    }
    // Interleave runs after the daily lines (chronological) or append them as
    // a bulk import (reversed).
    if reverse_runs {
        log.extend(runs);
    } else {
        let mut merged = Vec::with_capacity(log.len() + runs.len());
        let mut ri = runs.into_iter().peekable();
        for e in log {
            let day_of = match &e {
                Event::SetToday { epoch_day, .. } => Some(*epoch_day),
                _ => None,
            };
            if let Some(day) = day_of {
                while let Some(r) = ri.peek() {
                    let at = match r {
                        Event::LogRun { observed_at, .. }
                        | Event::LogRunTrack { observed_at, .. } => *observed_at,
                        _ => 0,
                    };
                    if at.div_euclid(DAY_SEC) < day {
                        merged.push(ri.next().unwrap());
                    } else {
                        break;
                    }
                }
            }
            merged.push(e);
        }
        merged.extend(ri);
        log = merged;
    }
    log
}

fn time_replay(log: Vec<Event>) -> (std::time::Duration, std::time::Duration) {
    std::thread::Builder::new()
        .stack_size(32 << 20)
        .spawn(move || {
            let app = Engine;
            let mut model = Model::default();
            let t0 = std::time::Instant::now();
            for e in log {
                let _ = app.update(e, &mut model);
            }
            let upd = t0.elapsed();
            let t1 = std::time::Instant::now();
            let _ = app.view(&model);
            (upd, t1.elapsed())
        })
        .unwrap()
        .join()
        .unwrap()
}

#[test]
#[ignore = "probe: prints 3-year replay timings (run explicitly with --release)"]
fn review_time_three_year_replay() {
    for (name, rev) in [
        ("chronological", false),
        ("runs bulk-imported newest-first", true),
    ] {
        let log = three_year_log(rev);
        let n = log.len();
        let (upd, view) = time_replay(log);
        eprintln!("REVLEDGER {name}: {n} events, update loop {upd:?}, first view {view:?}");
    }
    // Profile edits on a large log: 50 extra SetProfile at the end.
    let mut log = three_year_log(false);
    let last = D0 + 3 * 365;
    for k in 0..50 {
        log.push(profile(
            Some(last),
            Some(D0 - 39 * 365),
            Some(180.0 + k as f64 * 0.1),
            None,
        ));
    }
    let (upd, _) = time_replay(log);
    eprintln!("REVLEDGER chronological + 50 SetProfile: update loop {upd:?}");
}

// ── Undated race after the apply_profile change ────────────────────────────

/// RevLedger F1 (fixed): a race stated in a legacy UNDATED profile edit stays
/// undated (its day is unknown), but a later dated re-save of the same race
/// (the editor re-sends it undated) no longer copies that `None`: a copied
/// date that is missing falls back to the edit's effective day, so the race
/// the user re-states today anchors paces again (running-041).
#[test]
fn review_legacy_undated_race_can_never_be_redated() {
    let race = |eff: Option<i64>| {
        Event::SetProfile(Profile {
            effective_from_epoch_day: eff,
            recent_race: Some(RecentRace {
                distance_km: 10.0,
                time_sec: 2700,
                epoch_day: None,
            }),
            running_days_per_week: 3,
            running_km_per_week: 20.0,
            ..base_profile()
        })
    };
    let log = vec![
        today(D0, 0),
        race(None),
        today(D0 + 1, 0),
        race(Some(D0 + 1)),
    ];
    let (_, model) = replay(&log);
    let p = model.profile.as_ref().unwrap();
    assert!(
        super::super::run_targets::pace_anchor_vdot(p, D0 + 1).is_some(),
        "the race re-entered today anchors no paces (race {:?})",
        p.recent_race
    );
}

/// M06b guard: an undated race copies the date of the same race on the
/// history entry the edit FOLLOWS (by history key), not of the latest-applied
/// profile. A back-dated edit restating the race that a later-dated entry
/// dropped keeps the race's original date.
#[test]
fn an_undated_race_copies_the_date_of_the_history_entry_it_follows() {
    let with_race = |eff: i64, race: Option<RecentRace>| {
        Event::SetProfile(Profile {
            effective_from_epoch_day: Some(eff),
            recent_race: race,
            running_days_per_week: 3,
            running_km_per_week: 20.0,
            ..base_profile()
        })
    };
    let race = |epoch_day: Option<i64>| {
        Some(RecentRace {
            distance_km: 10.0,
            time_sec: 2700,
            epoch_day,
        })
    };
    let log = vec![
        today(D0 + 10, 0),
        with_race(D0, race(Some(D0 - 3))),
        // A later-dated edit without the race is the latest applied profile.
        with_race(D0 + 10, None),
        // Back-dated between the two: it follows the D0 entry, whose race it restates.
        with_race(D0 + 5, race(None)),
    ];
    let (_, model) = replay(&log);
    let p = profile_for_day(&model, D0 + 5).expect("profile");
    assert_eq!(
        p.recent_race.as_ref().and_then(|r| r.epoch_day),
        Some(D0 - 3)
    );
}

// ── Fail-before / pass-after checks of the fix claims (run at HEAD too) ────

/// CLAIM (BUGS "Compaction lost the date of an undated race"): the fixer's
/// legacy scenario. Fails at HEAD (the race took the replay's today, which
/// compaction drops), passes now.
#[test]
fn review_claim_undated_race_compaction_equivalent() {
    assert_equiv(
        "legacy undated race",
        vec![
            today(D0, 0),
            Event::SetProfile(Profile {
                recent_race: Some(RecentRace {
                    distance_km: 10.0,
                    time_sec: 2700,
                    epoch_day: None,
                }),
                ..base_profile()
            }),
            today(D0 + 6, 0),
        ],
    );
}

/// CLAIM (BUGS "the spike chip ... Undo changed them"): delete + the shell's
/// manual-run Undo keeps the card chip. Fails at HEAD, passes now.
#[test]
fn review_claim_delete_undo_keeps_spike_chip() {
    let at = D0 * DAY_SEC + 8 * 3600;
    let (app, mut model) = replay(&[
        today(D0, 0),
        run(20.0, 110.0, 70.0, 0.0, at, 11, None),
        run(5.0, 30.0, 70.0, 0.0, at - 2 * DAY_SEC, 12, None),
    ]);
    let row = |m: &Model| {
        app.view(m)
            .runs
            .into_iter()
            .find(|r| r.entry_id == 11)
            .unwrap()
    };
    let before = row(&model);
    let _ = app.update(
        Event::DeleteEntry {
            kind: EntryKind::Run,
            entry_id: 11,
            observed_at_fallback: at,
        },
        &mut model,
    );
    let _ = app.update(
        run(
            before.distance_km,
            before.duration_min,
            before.hr_pct_max,
            0.0,
            at,
            11,
            before.avg_hr_bpm,
        ),
        &mut model,
    );
    let after = row(&model);
    assert_eq!(
        (before.spike_flag, before.spike_note.clone()),
        (after.spike_flag, after.spike_note.clone())
    );
}
