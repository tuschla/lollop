//! MutationAudit (2026-10-25): guards for 2026-10-23..25 fixes whose mutant
//! (`planning/review/mutation/mutate.py`) SURVIVED the whole standing
//! `cargo test -p shared` (`mutate.py --suite`), plus equivalence witnesses
//! for the mutants that cannot change behaviour. Each test names its mutant;
//! the verdict table is `planning/review/mutation/AUDIT.md`.
//!
//! `cargo test -p shared --lib mutation_guards`.

use super::*;

const DAY0: i64 = 20_458; // a Monday (long_horizon's START)

fn today(m: &mut Model, d: i64) {
    let _ = Engine.update(
        Event::SetToday {
            epoch_day: d,
            utc_offset_sec: 0,
        },
        m,
    );
}

fn set_id(exercise: &str, kg: f64, reps: u32, id: u64, at: i64) -> Event {
    let mut ev = log_set(exercise, kg, reps, 8.0, at);
    if let Event::LogSet { entry_id, .. } = &mut ev {
        *entry_id = id;
    }
    ev
}

fn is_bench(name: &str) -> bool {
    crate::plan::same_movement(name, "Bench press")
}

/// Planned Bench press loads of the rendered week from local day `from`.
fn bench_loads(vm: &ViewModel, from: i64) -> Vec<f64> {
    vm.week_plan
        .iter()
        .filter(|s| s.epoch_day >= from)
        .flat_map(|s| &s.items)
        .filter(|i| is_bench(&i.exercise))
        .filter_map(|i| i.load_kg)
        .collect()
}

/// A hypertrophy lifter benching 60 kg × 8 @ RPE 8 (an e1RM-ANCHORED lift)
/// twice a week for six weeks; returns the model and the last lifting day.
fn anchored_hypertrophy_lifter() -> (Model, i64) {
    let app = Engine;
    let mut m = Model::default();
    today(&mut m, DAY0);
    let _ = app.update(
        Event::SetProfile(Profile {
            weekly_sets: 16,
            lift_days_per_week: Some(4),
            lift_goal: LiftGoal::Hypertrophy,
            concurrent_goal: ConcurrentGoal::Hypertrophy,
            effective_from_epoch_day: Some(DAY0),
            ..base_profile()
        }),
        &mut m,
    );
    let _ = app.update(
        Event::GeneratePlan {
            start_epoch_day: DAY0,
        },
        &mut m,
    );
    let mut id = 0;
    for w in 0..6 {
        for wd in [0, 3] {
            let d = DAY0 + 7 * w + wd;
            today(&mut m, d);
            for k in 0..3 {
                id += 1;
                let t = d * DAY_SEC + 64_800 + k * 120;
                let _ = app.update(set_id("Bench press", 60.0, 8, id, t), &mut m);
            }
        }
    }
    (m, DAY0 + 7 * 5 + 3)
}

/// M03c (REENTRY-RAMP-001, File 08 Table 3.4b: the re-entry ramp scales only
/// numbers set BEFORE the break). An e1RM-anchored lift keeps a ramping
/// `Reentry` derate on its anchor after the return, so a load lifted since
/// the return must not take it again when carried (`carried_derate_for`).
/// The standing `a_load_lifted_since_the_return_is_not_derated_again` uses an
/// UNANCHORED isolation lift, whose derate the M03b `pre_break` filter
/// already drops, so it no longer reaches `carried_derate_for`'s match.
#[test]
fn an_anchored_load_lifted_since_the_return_is_not_derated_again() {
    let (mut m, last) = anchored_hypertrophy_lifter();
    let back = last + 25;
    today(&mut m, back);
    let restart = bench_loads(&Engine.view(&m), back);
    assert!(
        !restart.is_empty() && restart.iter().all(|&kg| kg < 60.0),
        "back after {} days the bench restarts lighter: {restart:?}",
        back - last - 1
    );
    let restart = restart[0];
    for k in 0..3 {
        let t = back * DAY_SEC + 64_800 + k * 120;
        let _ = Engine.update(set_id("Bench press", restart, 8, 900 + k as u64, t), &mut m);
    }
    // Bench is a Monday lift: the next one is a week after the return, still
    // inside the 3-weeks-off ramp.
    today(&mut m, back + 7);
    let vm = Engine.view(&m);
    let next = bench_loads(&vm, back + 7);
    assert!(
        !next.is_empty() && next.iter().all(|&kg| kg >= restart),
        "restarted at {restart} kg and lifted it; the next sessions show {next:?} kg"
    );
}

/// `n` + 1 fixes due north from (50°N, 8°E), 4 s apart, each leg `step_deg`
/// of latitude (≈ 11.1 m per 1e-4°, ≈ 2.8 m/s: no QC gate trips).
fn north_track(start: (f64, i64), legs: &[f64]) -> Vec<GpsPoint> {
    let (mut lat, mut t) = start;
    let mut v = vec![GpsPoint {
        lat,
        lon: 8.0,
        observed_at: t,
        accuracy_m: 3.0,
    }];
    for step in legs {
        lat += step;
        t += 4;
        v.push(GpsPoint {
            lat,
            lon: 8.0,
            observed_at: t,
            accuracy_m: 3.0,
        });
    }
    v
}

/// M09b: a run whose second half is faster by a hair rounds its positive
/// split to -0.0; the `+ 0.0` keeps "-0.0 %" off the run card ("-0.0" never
/// a shown figure, BUGS.md 2026-10-23). The standing guards only catch it
/// through the `view_equivalence` hash pin.
#[test]
fn a_dead_even_gps_run_never_shows_a_negative_zero_split() {
    let mut legs = vec![1.0e-4; 50];
    legs.extend(std::iter::repeat_n(1.0e-4 * (1.0 + 1.0e-5), 50));
    let start = 1_700_000_000;
    let points = north_track((50.0, start), &legs);
    let raw = crate::running::track_positive_split_pct(&points, crate::running::MAX_GPS_ACCURACY_M)
        .expect("a 100-leg track splits");
    assert_eq!(raw, 0.0, "the split rounds to zero: {raw}");
    assert!(!raw.is_sign_negative(), "positive split is -0.0");

    let mut m = Model::default();
    today(&mut m, start / DAY_SEC);
    let _ = Engine.update(
        log_run_track(points, 70.0, 0.0, start + 400, 7_310_001),
        &mut m,
    );
    let vm = Engine.view(&m);
    let split = vm.runs[0].split_pct.expect("the card shows the split");
    assert!(!split.is_sign_negative(), "the card's split_pct is -0.0");
    let json = serde_json::to_string(&vm.runs[0]).unwrap();
    assert!(!json.contains("-0.0"), "{json}");
}

/// M09c (equivalence witness). `week_km`'s `+ 0.0` (in `sum0`) only matters
/// when the sum is -0.0: an EMPTY week (Rust's float `Sum` starts at -0.0) or
/// a week whose every run is -0.0 km. The earlier weeks (`prev_km`,
/// `baseline2_km`, three weeks back) only feed `> 0.0` /
/// `>= MIN_RATIO_BASE_KM` gates, which -0.0 and 0.0 fail alike; the current
/// week holds a run by construction, and ingest stores
/// `sanitize_f64(km).max(0.0)`, so a "-0.0 km" manual run reaches the sum as
/// +0.0 (-0.0 + 0.0 = +0.0). Two lives drive it: empty prior weeks, and a
/// full prior week, so the "Week-over-week" row PRINTS the current week's
/// km (a lone -0.0 km run). Both pass with and without the mutant
/// (`mutate.py M09cW`).
#[test]
fn weekly_report_with_empty_and_negative_zero_weeks_shows_no_negative_zero() {
    for prior_week in [0, 2] {
        let mut m = Model::default();
        today(&mut m, DAY0 + 20);
        let _ = Engine.update(
            Event::SetProfile(Profile {
                running_days_per_week: 3,
                running_km_per_week: 20.0,
                goal_distance: GoalDistance::TenK,
                ..base_profile()
            }),
            &mut m,
        );
        // Week `prior_week`: real runs; the others empty; week 3 (current):
        // one -0.0 km run.
        for (i, d) in [0, 2, 4].into_iter().enumerate() {
            let day = DAY0 + 7 * prior_week + d;
            let _ = Engine.update(
                log_run(
                    6.0,
                    36.0,
                    70.0,
                    0.0,
                    day * DAY_SEC + 3_600,
                    7_320_001 + i as u64,
                ),
                &mut m,
            );
        }
        let _ = Engine.update(
            log_run(
                -0.0,
                30.0,
                70.0,
                0.0,
                (DAY0 + 21) * DAY_SEC + 3_600,
                7_320_010,
            ),
            &mut m,
        );
        today(&mut m, DAY0 + 21);
        let json = serde_json::to_string(&Engine.view(&m)).unwrap();
        if prior_week == 2 {
            assert!(
                json.contains("Week-over-week"),
                "fixture: the ramp row prints this week's km"
            );
        }
        for bad in ["-0.0", "-0 km", "(-0%", "-0%"] {
            assert!(
                !json.contains(bad),
                "{bad}: {}",
                &json[json.find(bad).map_or(0, |i| i.saturating_sub(120))..][..240.min(json.len())]
            );
        }
    }
}

/// M09f: a corrupt log's subnormal prior weight is no prior best
/// (`MIN_RATIO_BASE_KG`). The standing `a_subnormal_prior_set_never_praises_
/// an_infinite_gain` runs a novice profile, whose plain-language mastery copy
/// prints no percentage, so "+inf%" never shows there; an ADVANCED lifter
/// (MonthToMonth cadence, feedback-023/024 verbosity) gets the
/// "+N% on your 90-day best" copy.
#[test]
fn a_subnormal_prior_set_never_praises_an_advanced_lifter_with_infinite_gain() {
    let app = Engine;
    let mut m = Model::default();
    today(&mut m, DAY0 + 1);
    let _ = app.update(
        Event::SetProfile(Profile {
            progression_cadence: ProgressionCadence::MonthToMonth,
            ..base_profile()
        }),
        &mut m,
    );
    let _ = app.update(
        set_id("Back squat", 5e-324, 5, 1, DAY0 * DAY_SEC + 7 * 3600),
        &mut m,
    );
    let _ = app.update(
        set_id("Back squat", 100.0, 5, 2, (DAY0 + 1) * DAY_SEC + 7 * 3600),
        &mut m,
    );
    let json = serde_json::to_string(&app.view(&m)).expect("view serializes");
    assert!(
        !json.contains("inf%") && !json.contains("on your 90-day best"),
        "{}",
        &json[json.find("90-day").map_or(0, |i| i.saturating_sub(160))..][..320.min(json.len())]
    );
}

/// M09i3 (equivalence witness). `cur_monday = today - weekday` is only
/// reached with a run dated on or before `today`, and a run's day is
/// `local_day` = `saturating_add(t, offset).div_euclid(86_400)` >=
/// `i64::MIN.div_euclid(86_400)`, so `today - 6` cannot overflow there and
/// `saturating_sub` returns the same value. The extreme days below (with and
/// without runs, extreme offsets) render identically with the mutant
/// (`mutate.py M09i3W`).
#[test]
fn run_progress_at_extreme_days_never_overflows() {
    for (day, offset) in [
        (i64::MIN, 0),
        (i64::MIN + 3, i64::MIN),
        (i64::MIN.div_euclid(DAY_SEC), i64::MIN),
        (i64::MAX, i64::MAX),
    ] {
        let mut m = Model::default();
        let _ = Engine.update(
            log_run(5.0, 30.0, 70.0, 0.0, 1_700_000_000, 7_330_001),
            &mut m,
        );
        let _ = Engine.update(
            Event::SetToday {
                epoch_day: day,
                utc_offset_sec: offset,
            },
            &mut m,
        );
        let vm = Engine.view(&m);
        assert!(
            serde_json::to_string(&vm).is_ok(),
            "day {day} offset {offset}"
        );
    }
}

/// M10c: lollop's own GPX export names the run's logged-at stamp
/// (`<lollop:logged_at>`), so re-importing the file is recognised as the run
/// it came from (BUGS.md 2026-10-23 B2/B3: "re-imported own GPX was
/// duplicated (standing or trimmed end)"). The standing Rust guards test
/// `export_gpx_seg` itself, not that `export_run_gpx` passes the stamp.
#[test]
fn a_runs_gpx_export_names_its_logged_at_stamp() {
    let start = 1_700_100_000;
    let points = north_track((50.0, start), &[1.0e-4; 20]);
    // Logged 10 minutes after the last fix (a standing-still finish).
    let logged_at = start + 80 + 600;
    let mut m = Model::default();
    today(&mut m, start / DAY_SEC);
    let _ = Engine.update(
        log_run_track(points, 70.0, 0.0, logged_at, 7_340_001),
        &mut m,
    );
    let vm = Engine.view(&m);
    assert!(vm.runs[0].has_gpx);
    let gpx = export_run_gpx(7_340_001, logged_at);
    assert!(
        gpx.contains("<lollop:logged_at>2023-11-16T02:11:20Z</lollop:logged_at>"),
        "{gpx}"
    );
}

// ── Storage-order ties (BUGS.md 2026-10-23: "storage-order sums and ties").
// An Undo is `DeleteEntry` + the original `Log*` event again: the row comes
// back with its stamp and `entry_id`, at the END of storage. Every view must
// read the same afterwards.

/// The view JSON before vs after an Undo of entry `id`; `None` when equal,
/// else the first differing context.
fn undo_diff(m: &mut Model, kind: EntryKind, id: u64, at: i64, ev: Event) -> Option<String> {
    let json = |m: &Model| serde_json::to_string(&Engine.view(m)).expect("view serializes");
    let before = json(m);
    let _ = Engine.update(
        Event::DeleteEntry {
            kind,
            entry_id: id,
            observed_at_fallback: at,
        },
        m,
    );
    let _ = Engine.update(ev, m);
    let after = json(m);
    (before != after).then(|| {
        let i = before
            .bytes()
            .zip(after.bytes())
            .position(|(a, b)| a != b)
            .unwrap_or(0);
        let lo = i.saturating_sub(160);
        format!(
            "Undo of {kind:?} {id}: …{}… vs …{}…",
            &before[lo..(i + 80).min(before.len())],
            &after[lo..(i + 80).min(after.len())]
        )
    })
}

/// M09l: two sets of the same lift set the same new best on the latest day
/// (equal gain). Mastery names the EARLIER one (`day_sets` sorted by time,
/// then `entry_id`); in storage order an Undo of it moves it to the tail and
/// the praise names the other spelling.
#[test]
fn undo_of_one_of_two_tied_bests_keeps_the_mastery_praise() {
    let mut m = Model::default();
    today(&mut m, DAY0 + 10);
    let _ = Engine.update(Event::SetProfile(strength_profile()), &mut m);
    let prior = DAY0 * DAY_SEC + 7 * 3600;
    let _ = Engine.update(set_id("Back squat", 80.0, 5, 1, prior), &mut m);
    let a_at = (DAY0 + 10) * DAY_SEC + 7 * 3600;
    let a = set_id("Back squat", 100.0, 5, 10, a_at);
    let _ = Engine.update(a.clone(), &mut m);
    let _ = Engine.update(set_id("back squat", 100.0, 5, 11, a_at + 300), &mut m);
    let fb = serde_json::to_string(&Engine.view(&m).feedback).unwrap();
    assert!(
        fb.contains("Back squat"),
        "fixture: the praise names the earlier set: {fb}"
    );
    let diff = undo_diff(&mut m, EntryKind::Set, 10, a_at, a);
    assert!(diff.is_none(), "{}", diff.unwrap_or_default());
}

/// M09q: two manual runs on the same stamp (an import, a double save). The
/// latest run of the spike gate is the later-LOGGED one by `entry_id`
/// (RUN-SPIKE-001 safety gate); by storage slot an Undo of the other run
/// makes it "the latest" and the spike warning disappears.
#[test]
fn undo_of_a_same_stamp_run_keeps_the_spike_gate() {
    let mut m = Model::default();
    let day = DAY0 + 10;
    today(&mut m, day);
    let _ = Engine.update(
        Event::SetProfile(Profile {
            running_days_per_week: 3,
            running_km_per_week: 25.0,
            goal_distance: GoalDistance::TenK,
            ..base_profile()
        }),
        &mut m,
    );
    let _ = Engine.update(
        log_run(
            10.0,
            60.0,
            70.0,
            0.0,
            (day - 5) * DAY_SEC + 3_600,
            7_350_001,
        ),
        &mut m,
    );
    let at = day * DAY_SEC + 3_600;
    let short = log_run(5.0, 30.0, 70.0, 0.0, at, 7_350_020);
    let _ = Engine.update(short.clone(), &mut m);
    let _ = Engine.update(log_run(14.0, 84.0, 70.0, 0.0, at, 7_350_021), &mut m);
    let diff = undo_diff(&mut m, EntryKind::Run, 7_350_020, at, short);
    assert!(diff.is_none(), "{}", diff.unwrap_or_default());
}

/// M09r: two GPS runs on the same stamp, one fading 25 % in its second half.
/// The pacing cue (feedback-016/017) reads the latest tracked run's split on
/// the reviewed session's day, a stamp tie going to the later-logged run by
/// `entry_id`; by storage slot an Undo of the even run makes it "the latest"
/// and the cue changes. The split only reaches feedback through a session
/// review (or a done planned run day), hence the routine review.
#[test]
fn undo_of_a_same_stamp_gps_run_keeps_the_pacing_cue() {
    let t0 = (DAY0 + 10) * DAY_SEC + 3_600;
    let even = north_track((50.0, t0), &[1.0e-4; 270]);
    let mut fade_legs = vec![1.0e-4; 135];
    fade_legs.extend(std::iter::repeat_n(0.8e-4, 169));
    let fade = north_track((50.1, t0), &fade_legs);
    let at = t0 + 4 * 304 + 60;
    let mut m = Model::default();
    today(&mut m, DAY0 + 10);
    let _ = Engine.update(Event::SetProfile(base_profile()), &mut m);
    let even_ev = log_run_track(even, 0.0, 0.0, at, 7_360_041);
    let _ = Engine.update(even_ev.clone(), &mut m);
    let _ = Engine.update(log_run_track(fade, 0.0, 0.0, at, 7_360_042), &mut m);
    let _ = Engine.update(
        Event::SubmitReview(SessionReview {
            observed_at: at + 60,
            ..SessionReview::default()
        }),
        &mut m,
    );
    let vm = Engine.view(&m);
    assert!(
        vm.runs
            .iter()
            .any(|r| r.split_pct.is_some_and(|p| p > 10.0)),
        "fixture: one run fades"
    );
    let fb = serde_json::to_string(&vm.feedback).unwrap();
    assert!(
        fb.contains("Second half 25% slower"),
        "fixture: the pacing cue shows: {fb}"
    );
    let diff = undo_diff(&mut m, EntryKind::Run, 7_360_041, at, even_ev);
    assert!(diff.is_none(), "{}", diff.unwrap_or_default());
}

/// M09s (equivalence witness). `top_set` takes the max of `logged_sets`,
/// which `judge_lift_item` builds sorted by `LoggedSet::chrono_cmp` (stamp,
/// `entry_id`, content) and later code only edits in place. `max_by` returns
/// the LAST maximum, so on a (weight, reps) tie the last tied set in that
/// order wins, and it is also the one with the greatest (stamp, `entry_id`):
/// dropping the explicit tie-break picks the same set. This drives a done
/// plan day whose top set ties on weight and reps and Undoes each tied set;
/// it passes with and without the mutant (`mutate.py M09sW`).
#[test]
fn undo_of_a_tied_top_set_keeps_the_session_summary() {
    let (_, mut m) = planned_strength_model_on(MON, true);
    let vm = Engine.view(&m);
    let items: Vec<PrescriptionView> = vm
        .week_plan
        .iter()
        .find(|s| s.epoch_day == MON)
        .expect("a planned Monday")
        .items
        .iter()
        .filter(|i| !i.is_run())
        .cloned()
        .collect();
    assert!(!items.is_empty());
    let mut id = 7_370_000;
    let mut t = MON * DAY_SEC + 17 * 3600;
    let mut ties = Vec::new();
    for (n, it) in items.iter().enumerate() {
        let kg = it.load_kg.unwrap_or(40.0);
        for k in 0..=it.sets {
            id += 1;
            t += 120;
            // One extra set: the first item's last two sets tie on weight and
            // reps (different effort).
            let rpe = if k == it.sets { 9.0 } else { 8.0 };
            let ev = with_rpe(
                set_id(&it.exercise, kg, u32::from(it.reps_high), id, t),
                rpe,
            );
            if n == 0 && k + 1 >= it.sets {
                ties.push((id, t, ev.clone()));
            }
            let _ = Engine.update(ev, &mut m);
        }
    }
    let done = serde_json::to_string(&Engine.view(&m).session_done).unwrap();
    assert!(
        done.contains("best "),
        "fixture: a done session with a top set: {done}"
    );
    for (id, t, ev) in ties {
        let diff = undo_diff(&mut m, EntryKind::Set, id, t, ev);
        assert!(diff.is_none(), "{}", diff.unwrap_or_default());
    }
}

fn with_rpe(mut ev: Event, v: f64) -> Event {
    if let Event::LogSet { rpe, .. } = &mut ev {
        *rpe = v;
    }
    ev
}

/// M09t: two weighted pull-up sets on the same second (+12.5 kg, then
/// +10 kg by `entry_id`). The "≈ N estimated 1-rep max … from your last
/// weighted set" line reads the LAST set in log order (`chrono_cmp`); by
/// storage slot an Undo of the first set makes it "the last" and the line
/// changes.
#[test]
fn undo_of_a_same_second_weighted_set_keeps_the_system_load_line() {
    let (app, mut m) = planned_calisthenics();
    let _ = app.update(
        Event::SetProfile(Profile {
            bodyweight_kg: Some(80.0),
            ..calisthenics_profile()
        }),
        &mut m,
    );
    let at = (MON - 1) * DAY_SEC + 18 * 3600;
    let first = set_id("Full pull-up", 12.5, 5, 21, at);
    let _ = app.update(first.clone(), &mut m);
    let _ = app.update(set_id("Full pull-up", 10.0, 5, 22, at), &mut m);
    let item = cal_item(&app.view(&m), Lad::VerticalPull).expect("a pull-up item");
    let line = serde_json::to_string(&item).unwrap();
    assert!(
        line.contains("from your last weighted set"),
        "fixture: the system-load line shows: {line}"
    );
    let diff = undo_diff(&mut m, EntryKind::Set, 21, at, first);
    assert!(diff.is_none(), "{}", diff.unwrap_or_default());
}
