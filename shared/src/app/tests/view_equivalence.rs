//! Multi-year synthetic history: the serialized `ViewModel` is pinned
//! byte-for-byte (FNV-1a over the JSON) across several `SetToday` dates, so a
//! pure refactor of the view path (caching, threading per-view data) cannot
//! silently change output. A legitimate output change re-pins the table:
//! `VIEW_EQ_DUMP=/tmp/dir cargo test -p shared view_equivalence` writes each
//! JSON for diffing and prints the new hashes.
//!
//! `view_timing_large_history` (ignored) times the same fixture at three
//! years: `cargo test --release -p shared view_timing -- --ignored --nocapture`.

use super::*;
use serde_json::json;

/// A Monday (`(d - 4) % 7 == 0`): the last rendered week starts here.
pub(super) const END_MON: i64 = 20_500;

/// Deterministic 0..1 sequence (64-bit LCG), so the fixture is identical on
/// every run and platform.
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

pub(super) fn ev(v: serde_json::Value) -> Event {
    serde_json::from_value(v).expect("fixture event decodes")
}

fn gps_track(
    start_at: i64,
    km: f64,
    pace_s_per_km: f64,
    rng: &mut Lcg,
) -> (Vec<GpsPoint>, Vec<u32>) {
    const STEP_S: i64 = 5;
    let total_s = (km * pace_s_per_km) as i64;
    let n = (total_s / STEP_S) as usize + 1;
    // ~111 km per degree of latitude.
    let deg_per_s = 1.0 / pace_s_per_km / 111.0;
    let mut pts = Vec::with_capacity(n);
    let mut segs = Vec::new();
    let pause_at = if km > 14.0 { n / 2 } else { usize::MAX };
    let mut t = start_at;
    let mut lat = 48.0;
    for i in 0..n {
        if i == pause_at {
            segs.push(i as u32);
            t += 240;
        }
        let wiggle = ((i as f64) / 40.0).sin() * 0.0004;
        let outlier = rng.next() < 0.004;
        pts.push(GpsPoint {
            lat: lat + if outlier { 0.01 } else { 0.0 },
            lon: 11.0 + wiggle,
            observed_at: t,
            accuracy_m: if outlier {
                80.0
            } else {
                4.0 + (rng.next() * 6.0) as f32
            },
        });
        lat += deg_per_s * STEP_S as f64 * (0.9 + rng.next() * 0.2);
        t += STEP_S;
    }
    (pts, segs)
}

/// Replays `days` days of hybrid training ending the Sunday before
/// [`END_MON`] (plus a session straddling that midnight) through the real
/// `update`, one `SetToday` per day like the shell's foreground event.
pub(super) fn big_history(days: i64) -> (Engine, Model) {
    let app = Engine;
    let mut model = Model::default();
    let mut rng = Lcg(0x5eed_1234_abcd_0001);
    let start = END_MON - days;
    let mut id: u64 = 1_000;
    let apply = |app: &Engine, model: &mut Model, e: Event| {
        let _ = app.update(e, model);
    };

    let profile = Profile {
        age_years: Some(34.0),
        bodyweight_kg: Some(81.5),
        resting_hr_bpm: Some(52.0),
        effective_from_epoch_day: Some(start),
        ..sample_profile()
    };
    apply(&app, &mut model, Event::SetProfile(profile));

    let mut gps_ids = Vec::new();
    let mut set_ids = Vec::new();
    for d in start..END_MON {
        apply(
            &app,
            &mut model,
            ev(json!({"SetToday": {"epoch_day": d, "utc_offset_sec": 0}})),
        );
        let wd = (d - 4).rem_euclid(7);
        let week = (d - start) / 7;
        let deload = if week % 4 == 3 { 0.9 } else { 1.0 };
        let prog = (1.0 + 0.002 * week as f64) * deload;

        if rng.next() > 0.11 {
            let rhr = 52.0 + rng.next() * 4.0 + if END_MON - d <= 3 { 7.0 } else { 0.0 };
            let hrv = 55.0 + rng.next() * 18.0 - if END_MON - d <= 2 { 20.0 } else { 0.0 };
            apply(
                &app,
                &mut model,
                ev(json!({"SubmitCheckin": {
                    "observed_at": d * DAY_SEC + 7 * 3600,
                    "sleep_quality": rng.pick(2, 5),
                    "soreness": rng.pick(1, 4),
                    "mood": rng.pick(2, 5),
                    "fatigue": rng.pick(1, 4),
                    "stress": rng.pick(1, 4),
                    "resting_hr_bpm": (rhr * 10.0).round() / 10.0,
                    "hrv_rmssd_ms": (hrv * 10.0).round() / 10.0,
                }})),
            );
        }

        let lifts: &[(&str, f64, u32, u32)] = match wd {
            0 => &[
                ("Back Squat", 120.0, 4, 5),
                ("Bench Press", 85.0, 4, 5),
                ("Barbell Row", 70.0, 3, 8),
            ],
            3 => &[
                ("Deadlift", 150.0, 3, 3),
                ("Overhead Press", 52.5, 3, 6),
                ("Pull-up", 0.0, 3, 8),
            ],
            5 if week % 3 == 0 => &[
                ("Barbell back squat", 100.0, 3, 8),
                ("Bench Press", 75.0, 3, 8),
            ],
            _ => &[],
        };
        let mut at = d * DAY_SEC + 18 * 3600;
        for &(name, base, sets, reps) in lifts {
            let load = if base > 0.0 {
                (base * prog / 2.5).round() * 2.5
            } else {
                0.0
            };
            for s in 0..sets {
                id += 1;
                set_ids.push((id, name, load, reps, at));
                let rpe = 7.0 + f64::from(rng.pick(0, 4)) * 0.5;
                let r = if s + 1 == sets && rng.next() < 0.2 {
                    reps - 1
                } else {
                    reps
                };
                apply(
                    &app,
                    &mut model,
                    ev(
                        json!({"LogSet": {"exercise": name, "weight_kg": load, "reps": r, "rpe": rpe, "observed_at": at, "entry_id": id}}),
                    ),
                );
                at += 180;
            }
        }

        match wd {
            1 | 5 => {
                let long = wd == 5;
                let km = if long {
                    14.0 + (week % 6) as f64 * 2.0
                } else {
                    8.0 + rng.next() * 2.0
                };
                let pace = if long {
                    360.0
                } else {
                    330.0 + rng.next() * 20.0
                };
                let begin = d * DAY_SEC + 7 * 3600 + 1800;
                let (points, segs) = gps_track(begin, km, pace, &mut rng);
                id += 1;
                gps_ids.push((id, begin));
                apply(
                    &app,
                    &mut model,
                    ev(json!({"LogRunTrack": {
                        "points": points,
                        "hr_pct_max": if long { 78.0 } else { 71.0 },
                        "longest_recent_km": 0.0,
                        "segment_starts": segs,
                        "observed_at": begin,
                        "entry_id": id,
                        "workout_type": if long { "LongRun" } else { "Easy" },
                        "session_rpe": if long { 5 } else { 3 },
                    }})),
                );
            }
            4 => {
                id += 1;
                let km = 9.0 + rng.next() * 3.0;
                apply(
                    &app,
                    &mut model,
                    ev(json!({"LogRun": {
                        "distance_km": (km * 100.0).round() / 100.0,
                        "duration_min": (km * 4.6 * 10.0).round() / 10.0,
                        "hr_pct_max": 86.0,
                        "longest_recent_km": 0.0,
                        "observed_at": d * DAY_SEC + 17 * 3600,
                        "entry_id": id,
                        "workout_type": "Tempo",
                        "session_rpe": 7,
                        "avg_hr_bpm": 162.0,
                    }})),
                );
            }
            _ => {}
        }

        if d == END_MON - 56 {
            apply(
                &app,
                &mut model,
                ev(json!({"GeneratePlan": {"start_epoch_day": d + (7 - wd) % 7}})),
            );
        }
        if d == END_MON - 40 {
            apply(
                &app,
                &mut model,
                ev(
                    json!({"SubmitReadiness": {"signal": "Illness", "value": 1.0, "observed_at": d * DAY_SEC + 8 * 3600}}),
                ),
            );
        }
        if d == END_MON - 36 {
            apply(
                &app,
                &mut model,
                ev(
                    json!({"ResolveHold": {"signal": "Illness", "observed_at": d * DAY_SEC + 8 * 3600}}),
                ),
            );
        }
        if d == END_MON - 9 {
            apply(
                &app,
                &mut model,
                ev(json!({"SubmitReadiness": {
                    "signal": "Pain", "value": 1.0, "observed_at": d * DAY_SEC + 9 * 3600,
                    "pain": {"kind": "TendonLoadRelated", "severity": 3, "trend": "Stable", "location": "Left knee"},
                }})),
            );
        }
        if d == END_MON - 4 {
            apply(
                &app,
                &mut model,
                ev(
                    json!({"SubmitReadiness": {"signal": "Rpe", "value": 9.0, "observed_at": d * DAY_SEC + 19 * 3600}}),
                ),
            );
        }
    }

    // A session straddling Sunday→Monday midnight at the start of the week.
    for (i, at) in [
        END_MON * DAY_SEC - 1200,
        END_MON * DAY_SEC - 600,
        END_MON * DAY_SEC + 300,
        END_MON * DAY_SEC + 900,
    ]
    .into_iter()
    .enumerate()
    {
        id += 1;
        apply(
            &app,
            &mut model,
            ev(
                json!({"LogSet": {"exercise": "Back Squat", "weight_kg": 130.0, "reps": 3 + i as u32 % 2, "rpe": 8.5, "observed_at": at, "entry_id": id}}),
            ),
        );
    }

    // Edits over the history: amend, delete, trim, re-rate.
    let (sid, name, load, reps, at) = set_ids[set_ids.len() / 2];
    apply(
        &app,
        &mut model,
        ev(
            json!({"AmendSet": {"entry_id": sid, "exercise": name, "weight_kg": load + 5.0, "reps": reps, "rpe": 9.0, "observed_at": at}}),
        ),
    );
    let (sid, ..) = set_ids[set_ids.len() - 7];
    apply(
        &app,
        &mut model,
        ev(json!({"DeleteEntry": {"kind": "Set", "entry_id": sid}})),
    );
    let (rid, _) = gps_ids[gps_ids.len() / 3];
    apply(
        &app,
        &mut model,
        ev(json!({"DeleteEntry": {"kind": "Run", "entry_id": rid}})),
    );
    let (rid, begin) = gps_ids[gps_ids.len() - 2];
    apply(
        &app,
        &mut model,
        ev(
            json!({"TrimRunTrack": {"entry_id": rid, "keep_from_sec": begin + 120, "keep_until_sec": begin + 2400}}),
        ),
    );
    let (rid, _) = gps_ids[gps_ids.len() - 3];
    apply(
        &app,
        &mut model,
        ev(json!({"AmendRunMeta": {"entry_id": rid, "session_rpe": 6, "workout_type": "Steady"}})),
    );
    apply(
        &app,
        &mut model,
        ev(
            json!({"SetExerciseEnded": {"exercise": "Barbell Row", "epoch_day": END_MON - 7, "ended": true, "observed_at": (END_MON - 7) * DAY_SEC + 70_000}}),
        ),
    );
    apply(
        &app,
        &mut model,
        ev(
            json!({"PredictRace": {"recent_distance_m": 10_000.0, "recent_time_sec": 2_820.0, "goal_distance_m": 42_195.0, "weekly_km": 45.0}}),
        ),
    );
    apply(
        &app,
        &mut model,
        ev(json!({"ComputeHrZones": {"age_years": 34.0, "resting_hr_bpm": 52.0}})),
    );
    apply(
        &app,
        &mut model,
        ev(json!({"ComputeProtein": {"bodyweight_kg": 81.5, "masters": false, "deficit": true}})),
    );
    (app, model)
}

fn view_json(app: &Engine, model: &mut Model, epoch_day: i64) -> String {
    let _ = app.update(
        ev(json!({"SetToday": {"epoch_day": epoch_day, "utc_offset_sec": 0}})),
        model,
    );
    serde_json::to_string(&app.view(model)).expect("view serializes")
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// (SetToday epoch day, FNV-1a of the view JSON, JSON length).
const GOLDEN: [(i64, u64, usize); 4] = [
    (END_MON, 14_606_928_609_769_099_986, 1_180_221),
    (END_MON + 2, 16_649_657_609_372_544_035, 1_179_530),
    (END_MON - 1, 2_254_787_418_119_392_324, 1_174_603),
    (END_MON - 17, 6_614_925_389_751_529_916, 1_168_332),
];

#[test]
fn view_equivalence_large_history() {
    let (app, mut model) = big_history(420);
    let dump = std::env::var_os("VIEW_EQ_DUMP").map(std::path::PathBuf::from);
    let mut got = Vec::new();
    for (i, &(day, ..)) in GOLDEN.iter().enumerate() {
        let json = view_json(&app, &mut model, day);
        if i == 0 {
            // A second view of the unchanged model is served from the
            // per-thread memos: it must equal the cold one.
            assert_eq!(
                json,
                view_json(&app, &mut model, day),
                "memoized view differs from cold view"
            );
        }
        if let Some(dir) = &dump {
            std::fs::create_dir_all(dir).expect("dump dir");
            std::fs::write(dir.join(format!("view_{day}.json")), &json).expect("dump write");
        }
        got.push((day, fnv1a(json.as_bytes()), json.len()));
    }
    assert_eq!(
        got, GOLDEN,
        "serialized ViewModel changed; new table: {got:?}"
    );
}

#[test]
#[ignore = "timing probe; run in release with --ignored --nocapture"]
fn view_timing_large_history() {
    use std::time::Instant;
    let t = Instant::now();
    let (app, mut model) = big_history(3 * 365);
    eprintln!("replay 3y: {:?}", t.elapsed());
    let _ = app.update(
        ev(json!({"SetToday": {"epoch_day": END_MON, "utc_offset_sec": 0}})),
        &mut model,
    );

    let time = |label: &str, model: &Model| {
        let n = 5;
        let t = Instant::now();
        let mut bytes = 0;
        for _ in 0..n {
            bytes = serde_json::to_vec(&app.view(model))
                .expect("serializes")
                .len();
        }
        eprintln!("{label}: {:?}/view ({bytes} B JSON)", t.elapsed() / n);
    };
    time("view+serialize (memos warm after first)", &model);
    let mut entry = 9_000_000;
    let t = Instant::now();
    let n = 5;
    for k in 0..n {
        entry += 1;
        let _ = app.update(
            ev(json!({"LogSet": {"exercise": "Bench Press", "weight_kg": 90.0, "reps": 5, "rpe": 8.0,
                "observed_at": END_MON * DAY_SEC + 40_000 + k * 120, "entry_id": entry}})),
            &mut model,
        );
        let _ = serde_json::to_vec(&app.view(&model)).expect("serializes");
    }
    eprintln!("refresh after a new set: {:?}/view", t.elapsed() / n as u32);
}
