//! Ledger and derivation guards: compaction (`compact_event_log`) stays
//! replay-equivalent, a run's stored %HRmax and spike baseline are pure
//! functions of the surviving model, profile history (dated, timed and
//! legacy undated edits, day-aged birthdays) and the clock a run was logged
//! under.

use super::*;
use crate::compact_event_log;

const D0: i64 = 20_400;

/// The ledger generator's stream for `seed`.
fn rng(seed: u64) -> SplitMix {
    SplitMix(seed ^ 0x5DEE_CE66_D1CE_4E5B)
}

#[allow(clippy::too_many_arguments)]
fn run(km: f64, min: f64, hr: f64, wire_longest: f64, at: i64, id: u64, bpm: Option<f64>) -> Event {
    with_run_meta(log_run(km, min, hr, wire_longest, at, id), None, None, bpm)
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
    with_run_meta(log_run_track(points, 0.0, 0.0, at, id), None, None, bpm)
}

fn set(ex: &str, kg: f64, reps: u32, at: i64, id: u64) -> Event {
    with_entry_id(log_set(ex, kg, reps, 8.0, at), id)
}

fn checkin(at: i64, rng: &mut SplitMix) -> Event {
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
    let mut g = rng(seed);
    let mut ev = Vec::with_capacity(len);
    let mut t = D0 * DAY_SEC + 7 * 3600;
    let mut runs: Vec<RunRef> = Vec::new();
    let mut sets: Vec<(u64, i64)> = Vec::new();
    let mut next_id = 1_760_000_000_000u64;
    let offsets = [0i64, 3600, 7200, -18_000, 19_800, 50_400, -43_200];
    let mut offset = g.pick(&offsets);
    ev.push(set_today_off_ev(D0, offset));
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
        let id = |g: &mut SplitMix, next: &mut u64| {
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
                ev.push(set_today_off_ev(d, offset));
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
            _ => ev.push(set_today_off_ev(day, offset)),
        }
    }
    ev.push(set_today_off_ev(t.div_euclid(DAY_SEC), offset));
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

/// Compaction stays replay-equivalent because the run %HRmax and spike
/// baseline are pure functions of the surviving model (log.rs, ingest.rs).
/// Full vs compacted, cold threads, plus a compacted prefix + appended suffix
/// (the shell's launch-then-append path).
#[test]
fn review_compaction_equivalence_on_run_heavy_logs() {
    let mut fails = Vec::new();
    for case in 0..iters(150) as u64 {
        let seed = 0xA11C_E000 + case;
        let log = gen_log(seed, 120);
        let full = replay_cold(log.clone()).expect("full replay");
        let compacted = replay_cold(compact_event_log(log.clone())).expect("compacted replay");
        if full != compacted {
            fails.push(format!(
                "seed {seed:#x}: compacted differs: {}",
                json_diff(&full, &compacted)
            ));
            continue;
        }
        let cut = log.len() / 2;
        let mut relaunch = compact_event_log(log[..cut].to_vec());
        relaunch.extend_from_slice(&log[cut..]);
        let relaunched = replay_cold(relaunch).expect("relaunched replay");
        if full != relaunched {
            fails.push(format!(
                "seed {seed:#x}: compact-prefix+suffix differs: {}",
                json_diff(&full, &relaunched)
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

/// Stored `hr_pct_max` / `longest_recent_km` (the `LoggedRun` docs)
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

// ── Targeted compaction cases ──────────────────────────────────────────────

#[test]
fn review_delete_across_another_run_is_replay_equivalent() {
    let at = D0 * DAY_SEC + 8 * 3600;
    // A 20 km run, then a 10 km run 3 days later (its card baseline is 20),
    // then the 20 km run is deleted with another run logged in between.
    assert_compaction_equivalent(
        "delete across",
        vec![
            set_today_off_ev(D0 + 5, 3600),
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
    assert_compaction_equivalent(
        "amend chain + legacy collision",
        vec![
            set_today_ev(D0 + 5),
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

/// A typed-bpm run is converted with the Tanaka age of ITS day: the day before
/// the 40th birthday uses 39, the birthday 40, and a later SetToday past the
/// birthday leaves the earlier run on 39.
#[test]
fn review_bpm_run_uses_the_age_of_its_own_day_across_birthdays() {
    // 40th birthday on D0 + 10.
    let b40 = {
        let (y, m, d) = crate::civil_from_days(D0 + 10);
        days_from_civil(y - 40, m, d)
    };
    let eve = (D0 + 9) * DAY_SEC + 12 * 3600;
    let bday = (D0 + 10) * DAY_SEC + 12 * 3600;
    let log = vec![
        set_today_ev(D0),
        profile(Some(D0), Some(b40), None, None),
        run(8.0, 48.0, 0.0, 0.0, eve, 1, Some(162.0)),
        run(8.0, 48.0, 0.0, 0.0, bday, 2, Some(162.0)),
        set_today_ev(D0 + 30),
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

/// `ingest::aged_on`: after the 18th birthday a past day keeps its under-18
/// gate (SAFE-PEDS-001); a 29 February birthday completes on 1 March in a
/// common year.
#[test]
fn review_youth_gate_is_aged_per_day_across_the_18th_birthday() {
    // Feb 29 2008 birthday: 2026-02-28 → 17 (youth held), 2026-03-01 → 18.
    let feb29 = days_from_civil(2008, 2, 29);
    let feb28_26 = days_from_civil(2026, 2, 28);
    let log = vec![
        set_today_ev(feb28_26 - 60),
        profile(Some(feb28_26 - 60), Some(feb29), None, None),
        set_today_ev(feb28_26 + 30),
    ];
    let (_, model) = replay(&log);
    let youth = |d| profile_for_day(&model, d).map(|p| p.health.youth);
    assert_eq!(youth(feb28_26), Some(true), "28 Feb 2026: still 17");
    assert_eq!(youth(feb28_26 + 1), Some(false), "1 Mar 2026: 18");
}

/// A bpm run's %HRmax is converted on the clock it was
/// LOGGED under (the wire `utc_offset_sec` the shell stamps, kept as
/// `LoggedRun::logged_utc_offset_sec`), so travelling across time zones never
/// re-converts an old run with another day's age / profile: not on the
/// `SetToday` itself, nor on a later re-derivation (an unrelated profile edit,
/// an amend of the row), and a compacted log agrees. A legacy row with no
/// stamp still reads the current offset (its logged one is unknown).
#[test]
fn a_past_runs_hr_keeps_its_logged_offset_across_a_timezone_change() {
    let b40 = {
        let (y, m, d) = crate::civil_from_days(D0 + 10);
        days_from_civil(y - 40, m, d)
    };
    // 00:30 local on the 40th birthday at UTC+2 (= 22:30 UTC the day before).
    let at = (D0 + 10) * DAY_SEC - 2 * 3600 + 30 * 60;
    let log = vec![
        set_today_off_ev(D0 + 20, 7200),
        profile(Some(D0), Some(b40), None, None),
        stamped(run(8.0, 48.0, 0.0, 0.0, at, 1, Some(162.0)), 7200),
        run(8.0, 48.0, 0.0, 0.0, at, 2, Some(162.0)),
    ];
    let (app, mut model) = replay(&log);
    let before = hr_of(&app, &model, 1);
    assert_eq!(before, 90.0, "162 bpm at Tanaka(40) = 180");
    // Weeks later the user is in New York (UTC-5): nothing about the run changed.
    let later = vec![
        set_today_off_ev(D0 + 40, -18_000),
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
    assert_compaction_equivalent("time-zone change", full);
}

/// A profile edit carries the instant it was made
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
        set_today_ev(D0),
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
    assert_compaction_equivalent("two same-day edits", two);

    // A legacy edit with no instant is in force for its whole day.
    let mut legacy = base;
    legacy.push(profile(Some(D0), None, Some(200.0), Some(30.0)));
    let (app, model) = replay(&legacy);
    assert_eq!(hr_of(&app, &model, 1), 80.0);
}

/// A legacy undated SetProfile replaces the profile
/// history but applies to logged runs only from its position in the log: the
/// runs logged before it keep the %HRmax they had (`LoggedRun::hr_pinned`),
/// even across a later re-derivation, while a run logged after it uses it.
/// `ClearProfile` releases the pin. A compacted log (which keeps the history
/// a pin read) agrees.
#[test]
fn review_undated_profile_rewrites_all_past_runs() {
    let log = vec![
        set_today_ev(D0),
        profile(Some(D0 - 300), None, Some(190.0), Some(30.0)),
        run(8.0, 48.0, 0.0, 0.0, (D0 - 200) * DAY_SEC, 1, Some(160.0)),
    ];
    let (app, mut model) = replay(&log);
    let before = hr_of(&app, &model, 1);
    assert_eq!(before, 84.2, "160 bpm at the measured 190");
    let later = vec![
        profile(None, None, Some(170.0), Some(30.0)),
        set_today_ev(D0 + 5),
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
    assert_compaction_equivalent("undated edit after a run", full.clone());
    full.push(Event::ClearProfile);
    let (app, model) = replay(&full);
    assert_eq!(
        hr_of(&app, &model, 1),
        0.0,
        "ClearProfile: no profile, the wire figure"
    );
    assert_compaction_equivalent("undated edit, then ClearProfile", full);
}

// ── Check-ins a later SetToday reads again ─────────────────────────────────

/// A later SetToday can move today back into the window of a check-in, so a
/// compacted log (the relaunch) keeps every check-in it may read again and
/// renders like the full log.
#[test]
fn review_rule4_root_cause_settoday_back_reads_old_checkins() {
    let mut g = rng(7);
    let mut log = Vec::new();
    for d in 0..120 {
        log.push(checkin((D0 + d) * DAY_SEC + 7 * 3600, &mut g));
    }
    log.push(set_today_ev(D0 + 120));
    let launched = compact_event_log(log.clone());
    let back = set_today_ev(D0 + 40);
    let mut full = log;
    full.push(back.clone());
    let mut relaunch = launched;
    relaunch.push(back);
    let (a, b) = (
        replay_cold(full).expect("full replay"),
        replay_cold(relaunch).expect("relaunched replay"),
    );
    assert!(a == b, "{}", json_diff(&a, &b));
}

// ── Undated races ──────────────────────────────────────────────────────────

/// A race stated in a legacy UNDATED profile edit stays undated (its day is
/// unknown), but a later dated re-save of the same race (the editor re-sends
/// it undated) does not copy that `None`: a copied date that is missing falls
/// back to the edit's effective day, so the race the user re-states today
/// anchors paces (running-041).
#[test]
fn a_dated_resave_of_a_legacy_undated_race_anchors_paces_again() {
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
        set_today_ev(D0),
        race(None),
        set_today_ev(D0 + 1),
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
        set_today_ev(D0 + 10),
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
