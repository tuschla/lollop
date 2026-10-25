//! RevTests review proofs (2026-10-25), two kinds:
//! - `#[ignore = "REVIEW: …"]` tests FAIL on the current tree and demonstrate
//!   one finding of `local://review-RevTests.md`;
//! - the other tests PASS on the current tree and FAIL on a surviving mutant of
//!   `planning/review/mutation/mutate.py` (named in each doc comment): the
//!   behaviour a 2026-10-23 fix introduced that no existing test pins.
//!
//! `cargo test -q -p shared review_tests -- --include-ignored`.

use super::*;

const DAY0: i64 = 20_458; // a Monday (long_horizon's START)

fn runner_profile(km_per_week: f64) -> Profile {
    Profile {
        goal_distance: GoalDistance::TenK,
        weekly_sets: 0,
        running_days_per_week: 4,
        running_km_per_week: km_per_week,
        concurrent_goal: ConcurrentGoal::EndurancePriority,
        effective_from_epoch_day: Some(DAY0),
        ..base_profile()
    }
}

fn today(m: &mut Model, d: i64) {
    let _ = Engine.update(
        Event::SetToday {
            epoch_day: d,
            utc_offset_sec: 0,
        },
        m,
    );
}

/// Planned run km of the rendered week.
fn planned_run_km(vm: &ViewModel) -> f64 {
    vm.week_plan
        .iter()
        .flat_map(|s| &s.items)
        .filter(|i| i.is_run())
        .filter_map(|i| i.run_head_km)
        .sum()
}

/// A runner who states 40 km/wk and runs 3 × 7 km every week for 60 weeks;
/// `skip` weeks are left empty (an undeclared break). The plan is generated
/// on week 58, so both lives sit at the same plan position with the same
/// compliance; only the history before the plan differs. Returns the view on
/// the Monday after week 60.
fn sixty_weeks(skip: std::ops::Range<i64>) -> ViewModel {
    let app = Engine;
    let mut m = Model::default();
    today(&mut m, DAY0);
    let _ = app.update(Event::SetProfile(runner_profile(40.0)), &mut m);
    let mut id = 0;
    for w in 0..60 {
        for wd in [0, 2, 5] {
            let d = DAY0 + 7 * w + wd;
            today(&mut m, d);
            if w == 58 && wd == 0 {
                let _ = app.update(Event::GeneratePlan { start_epoch_day: d }, &mut m);
            }
            if skip.contains(&w) {
                continue;
            }
            id += 1;
            let _ = app.update(
                log_run(7.0, 42.0, 70.0, 0.0, d * DAY_SEC + 7 * 3600, id),
                &mut m,
            );
        }
    }
    today(&mut m, DAY0 + 7 * 60);
    app.view(&m)
}

/// RUN-REENTRY-001 (File 08 Table 3.4b) is a RE-ENTRY: the restart fraction
/// after a break, ramping back to full over the bracket's window. A break's
/// effect ends with it (`build_run_break`), so a three-week break 55 weeks
/// ago no longer scales today's volume floor: two athletes with the same last
/// 55 weeks get the same plan (was F1, fixed by FixPlan2).
#[test]
fn a_running_break_a_year_ago_no_longer_changes_the_plan() {
    let steady = sixty_weeks(0..0);
    let broke_long_ago = sixty_weeks(2..5);
    let show = |vm: &ViewModel| {
        let p = vm
            .program
            .as_ref()
            .map(|p| (p.week, p.weeks_total, p.phase));
        let runs: Vec<_> = vm
            .week_plan
            .iter()
            .flat_map(|s| s.items.iter().map(move |i| (s.epoch_day - DAY0, i)))
            .filter(|(_, i)| i.is_run())
            .map(|(d, i)| format!("d{d}:{}", i.summary))
            .collect();
        format!("{p:?} {runs:?}")
    };
    assert_eq!(
        planned_run_km(&steady),
        planned_run_km(&broke_long_ago),
        "same 55 weeks of running, one with a 3-week break 55 weeks ago:\n steady {}\n broke  {}",
        show(&steady),
        show(&broke_long_ago)
    );
}

// ── Killers of surviving mutants (pass now, fail on the mutant) ────────────

/// Metres per degree of longitude on the equator (the core's haversine R).
const M_PER_DEG: f64 = 6_371_000.0 * std::f64::consts::PI / 180.0;

/// A fix `x_m` metres east of (0, 0) at unix second `t`.
fn fix(x_m: f64, t: i64) -> GpsPoint {
    GpsPoint {
        lat: 0.0,
        lon: x_m / M_PER_DEG,
        observed_at: t,
        accuracy_m: 5.0,
    }
}

fn run_json(vm: &ViewModel, id: u64) -> String {
    let r = vm
        .runs
        .iter()
        .find(|r| r.entry_id == id)
        .expect("run listed");
    serde_json::to_string(r).expect("run serializes")
}

/// M08: a runner at a constant 3 m/s stopping five times for 30 s at a light.
/// The fix (BUGS.md "distance lost the first leg after each stop") counts the
/// 3 m stride out of each stop, so the distance of record is every metre run:
/// 6 × 199 legs × 3 m + 5 strides × 3 m = 3 597 m. The existing guard only
/// checks distance against moving time, which a regression dropping that
/// stride from BOTH still passes.
#[test]
fn the_stride_out_of_each_stop_is_distance_of_record() {
    let mut pts = Vec::new();
    let (mut x, mut t) = (0.0, 1_790_000_000_i64);
    for _ in 0..6 {
        for _ in 0..200 {
            pts.push(fix(x, t));
            x += 3.0;
            t += 1;
        }
        x -= 3.0;
        for _ in 0..30 {
            pts.push(fix(x, t));
            t += 1;
        }
        x += 3.0;
    }
    let app = Engine;
    let mut m = Model::default();
    let _ = app.update(log_run_track(pts, 0.0, 0.0, t, 1), &mut m);
    let km = app.view(&m).runs[0].distance_km;
    assert!(
        (km * 1000.0 - 3597.0).abs() <= 1.0,
        "distance of record {:.1} m, ran 3597 m",
        km * 1000.0
    );
}

/// M05f: trimming a GPS run re-derives the card spike baseline of the runs
/// after it (`refresh_spike_baselines` in the `TrimRunTrack` arm): the later
/// run's card must equal the one it gets when the earlier run was logged
/// already trimmed.
#[test]
fn trimming_a_run_re_derives_the_next_runs_spike_chip() {
    let t0 = DAY0 * DAY_SEC + 7 * 3600;
    let full: Vec<GpsPoint> = (0..3000).map(|s| fix(3.0 * s as f64, t0 + s)).collect();
    let end = t0 + 2999;
    let later = (DAY0 + 2) * DAY_SEC + 7 * 3600;
    let app = Engine;
    let mut trimmed = Model::default();
    today(&mut trimmed, DAY0 + 2);
    let _ = app.update(log_run_track(full.clone(), 0.0, 0.0, end, 1), &mut trimmed);
    let _ = app.update(log_run(5.0, 30.0, 0.0, 0.0, later, 2), &mut trimmed);
    let _ = app.update(trim(1, end, t0, t0 + 600), &mut trimmed);
    let mut fresh = Model::default();
    today(&mut fresh, DAY0 + 2);
    let _ = app.update(
        log_run_track(full[..=600].to_vec(), 0.0, 0.0, end, 1),
        &mut fresh,
    );
    let _ = app.update(log_run(5.0, 30.0, 0.0, 0.0, later, 2), &mut fresh);
    let (a, b) = (app.view(&trimmed), app.view(&fresh));
    assert_eq!(run_json(&b, 1), run_json(&a, 1), "the trimmed run itself");
    assert_eq!(
        run_json(&b, 2),
        run_json(&a, 2),
        "the run after the trimmed one"
    );
}

/// M05c (RevLedger F2): a typed-bpm run's %HRmax is converted against the
/// profile of the run's local day ON THE CLOCK IT WAS LOGGED UNDER (the
/// `utc_offset_sec` the shell stamps), so a later UTC-offset change, followed
/// by an edit that re-derives every run, leaves it on its logged day; logged
/// under the other offset it reads the other day's profile.
#[test]
fn a_bpm_runs_hr_percentage_keeps_the_offset_it_was_logged_under() {
    let d = DAY0;
    let p = |age: f64, eff: i64| {
        Event::SetProfile(Profile {
            age_years: Some(age),
            effective_from_epoch_day: Some(eff),
            ..runner_profile(30.0)
        })
    };
    // 23:30 UTC on day d: local day d at UTC+0, day d + 1 at UTC+1.
    let run = |off: i64| Event::LogRun {
        distance_km: 8.0,
        duration_min: 45.0,
        hr_pct_max: 0.0,
        longest_recent_km: 0.0,
        observed_at: d * DAY_SEC + 23 * 3600 + 1800,
        entry_id: 7,
        workout_type: None,
        session_rpe: None,
        avg_hr_bpm: Some(150.0),
        utc_offset_sec: Some(off),
    };
    let set_today = |off: i64| Event::SetToday {
        epoch_day: d + 2,
        utc_offset_sec: off,
    };
    let app = Engine;
    let replay = |events: Vec<Event>| {
        let mut m = Model::default();
        for e in events {
            let _ = app.update(e, &mut m);
        }
        app.view(&m).runs[0].hr_pct_max
    };
    let logged_utc = replay(vec![set_today(0), p(30.0, d), p(60.0, d + 1), run(0)]);
    let travelled = replay(vec![
        set_today(0),
        p(30.0, d),
        p(60.0, d + 1),
        run(0),
        set_today(3600),
        p(60.0, d + 2),
    ]);
    let logged_utc1 = replay(vec![set_today(0), p(30.0, d), p(60.0, d + 1), run(3600)]);
    assert_eq!(
        travelled, logged_utc,
        "a later offset change moved the run's %HRmax"
    );
    assert_ne!(
        logged_utc1, logged_utc,
        "the offset the run was logged under picks its day"
    );
}

/// M09i1/M09i2/M09i3: the `saturating_sub`s added in `plan::apply_lift_week_modifiers`
/// (meet offset), `plan::run::cycle_pos` (race countdown) and
/// `run_views::progress` (Monday) only matter for an ADULT with a plan, a
/// meet, a race and runs; the existing extreme-day test uses a held youth
/// profile, which never reaches them.
#[test]
fn an_extreme_set_today_never_panics_a_planned_adult() {
    for day in [i64::MIN, i64::MIN / 2, i64::MAX] {
        let app = Engine;
        let mut m = Model::default();
        today(&mut m, DAY0);
        let _ = app.update(
            Event::SetProfile(Profile {
                goal_race_epoch_day: Some(DAY0 + 60),
                meet_epoch_day: Some(DAY0 + 40),
                weekly_sets: 12,
                lift_days_per_week: Some(3),
                ..runner_profile(30.0)
            }),
            &mut m,
        );
        let _ = app.update(
            Event::GeneratePlan {
                start_epoch_day: DAY0,
            },
            &mut m,
        );
        let at = DAY0 * DAY_SEC + 7 * 3600;
        let _ = app.update(log_run(6.0, 36.0, 70.0, 0.0, at, 1), &mut m);
        let _ = app.update(log_set_id("Back squat", 100.0, 2, at), &mut m);
        today(&mut m, day);
        let vm = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| app.view(&m)));
        assert!(vm.is_ok(), "view() panicked after SetToday {day}");
    }
}

/// M09f: a corrupt log's subnormal prior weight is no prior best for the
/// mastery check (`MIN_RATIO_BASE_KG`), so no "+inf%" reaches the view. The
/// existing guards are random fuzz seeds that never hit this.
#[test]
fn a_subnormal_prior_set_never_praises_an_infinite_gain() {
    let app = Engine;
    let mut m = Model::default();
    today(&mut m, DAY0 + 1);
    let _ = app.update(Event::SetProfile(base_profile()), &mut m);
    let _ = app.update(
        log_set_id("Back squat", 5e-324, 1, DAY0 * DAY_SEC + 7 * 3600),
        &mut m,
    );
    let _ = app.update(
        log_set_id("Back squat", 100.0, 2, (DAY0 + 1) * DAY_SEC + 7 * 3600),
        &mut m,
    );
    let json = serde_json::to_string(&app.view(&m)).expect("view serializes");
    assert!(
        !json.contains("inf%") && !json.contains("inf %"),
        "{}",
        &json[json.find("inf").map_or(0, |i| i.saturating_sub(80))..][..160.min(json.len())]
    );
}

/// M02c: Table 3.4b running, 2–4 weeks off: "rebuild long run". The long run
/// before a three-week break is no demonstrated capacity, so it may not lift
/// the first long run back above the share of the re-entry week (the
/// share/average-run floor of `plan::run::long_run_decision`).
#[test]
fn a_three_week_running_break_rebuilds_the_long_run() {
    let app = Engine;
    let mut m = Model::default();
    today(&mut m, DAY0);
    let _ = app.update(Event::SetProfile(runner_profile(30.0)), &mut m);
    let _ = app.update(
        Event::GeneratePlan {
            start_epoch_day: DAY0,
        },
        &mut m,
    );
    let mut id = 0;
    for w in 0..8 {
        for (wd, km) in [(0, 6.0), (2, 6.0), (4, 6.0), (6, 14.0)] {
            let d = DAY0 + 7 * w + wd;
            today(&mut m, d);
            id += 1;
            let _ = app.update(
                log_run(km, km * 6.0, 70.0, 0.0, d * DAY_SEC + 7 * 3600, id),
                &mut m,
            );
        }
    }
    // Last run on day 55 (a Sunday); back on day 77, 21 days off.
    let back = DAY0 + 77;
    today(&mut m, back);
    let vm = app.view(&m);
    let week: Vec<f64> = vm
        .week_plan
        .iter()
        .flat_map(|s| &s.items)
        .filter(|i| i.is_run())
        .filter_map(|i| i.run_head_km)
        .collect();
    let total: f64 = week.iter().sum();
    let long = week.iter().copied().fold(0.0, f64::max);
    let share_floor = (0.3 * total).max(total / week.len() as f64);
    assert!(
        long <= share_floor + 0.5,
        "after 3 weeks off the long run is {long} km of a {total:.1} km week {week:?}: the 14 km \
         long run from before the break still counts as capacity"
    );
}

/// "Storage-order sums and ties" (BUGS.md 2026-10-23): Undo re-pushes a row
/// at the tail of its list, so every tie in this history (same stamp, same
/// weight × reps, same km, two spellings of one lift) must be broken by time
/// and `entry_id` (the fix's own rule), never by storage slot: deleting any
/// one entry and logging it again (the shell's Undo) must render the view
/// byte for byte as before. `same_stamp_sets` puts the two tied sets of a
/// pair on the same second (else one second apart). Returns every diff.
fn tie_history_undo_diffs(same_stamp_sets: bool) -> Vec<String> {
    let s = i64::from(!same_stamp_sets);
    let app = Engine;
    let mut m = Model::default();
    let now = DAY0 + 56;
    today(&mut m, DAY0);
    let _ = app.update(
        Event::SetProfile(Profile {
            weekly_sets: 12,
            lift_days_per_week: Some(3),
            concurrent_goal: ConcurrentGoal::Strength,
            ..runner_profile(25.0)
        }),
        &mut m,
    );
    let _ = app.update(
        Event::GeneratePlan {
            start_epoch_day: DAY0,
        },
        &mut m,
    );
    today(&mut m, now);
    // A lift the plan shows today, else the next session's.
    let vm = app.view(&m);
    let lift = vm
        .week_plan
        .iter()
        .filter(|s| s.epoch_day >= now)
        .flat_map(|s| &s.items)
        .find(|i| !i.is_run() && i.load_kg.is_some() || !i.is_run() && !i.exercise.is_empty())
        .map_or("Back squat".to_owned(), |i| i.exercise.clone());
    let at = |d: i64, h: i64| d * DAY_SEC + h * 3600;
    let set = |ex: &str, kg: f64, rpe: f64, t: i64, id: u64| {
        with_entry_id(log_set(ex, kg, 5, rpe, t), id)
    };
    let gps = |km: f64, end: i64, id: u64| {
        let n = (km * 1000.0 / 3.0) as i64;
        let pts: Vec<GpsPoint> = (0..=n).map(|s| fix(3.0 * s as f64, end - n + s)).collect();
        log_run_track(pts, 0.0, 0.0, end, id)
    };
    let mut entries: Vec<(EntryKind, u64, i64, Event)> = vec![
        (
            EntryKind::Set,
            1,
            at(DAY0 + 20, 18),
            set("Back squat", 90.0, 8.0, at(DAY0 + 20, 18), 1),
        ),
        (
            EntryKind::Set,
            2,
            at(DAY0 + 50, 18),
            set("Back squat", 100.0, 8.0, at(DAY0 + 50, 18), 2),
        ),
        (
            EntryKind::Set,
            3,
            at(DAY0 + 50, 18) + s,
            set("back squat", 100.0, 9.0, at(DAY0 + 50, 18) + s, 3),
        ),
        (
            EntryKind::Set,
            4,
            at(now, 7),
            set(&lift, 60.0, 7.0, at(now, 7), 4),
        ),
        (
            EntryKind::Set,
            5,
            at(now, 7) + s,
            set(&lift, 60.0, 8.5, at(now, 7) + s, 5),
        ),
        (
            EntryKind::Run,
            6,
            at(DAY0 + 50, 8),
            log_run(10.0, 55.0, 70.0, 0.0, at(DAY0 + 50, 8), 6),
        ),
        (
            EntryKind::Run,
            7,
            at(DAY0 + 50, 8),
            log_run(10.0, 60.0, 80.0, 0.0, at(DAY0 + 50, 8), 7),
        ),
        (EntryKind::Run, 8, at(now, 6), gps(2.0, at(now, 6), 8)),
        (EntryKind::Run, 9, at(now, 6), gps(3.0, at(now, 6), 9)),
    ];
    for (.., e) in &entries {
        let _ = app.update(e.clone(), &mut m);
    }
    let json = |m: &Model| serde_json::to_string(&app.view(m)).expect("view serializes");
    let mut diffs = Vec::new();
    for (kind, id, at, e) in entries.drain(..) {
        let before = json(&m);
        let _ = app.update(
            Event::DeleteEntry {
                kind,
                entry_id: id,
                observed_at_fallback: at,
            },
            &mut m,
        );
        let _ = app.update(e, &mut m);
        let after = json(&m);
        if before != after {
            let i = before
                .bytes()
                .zip(after.bytes())
                .position(|(a, b)| a != b)
                .unwrap_or(0);
            let lo = i.saturating_sub(120);
            diffs.push(format!(
                "Undo of {kind:?} {id}: …{}… vs …{}…",
                &before[lo..(i + 60).min(before.len())],
                &after[lo..(i + 60).min(after.len())]
            ));
        }
    }
    diffs
}

/// Killer of M09k/M09l/M09m/M09q/M09r/M09s: the tied sets one second apart
/// (the fix's time-then-`entry_id` order is defined), runs on the same stamp.
#[test]
fn undo_in_a_tie_heavy_history_restores_the_view() {
    let diffs = tie_history_undo_diffs(false);
    assert!(diffs.is_empty(), "{}", diffs.join("\n"));
}

/// Two sets on the SAME second (an import, a double tap) tie in log order
/// (`LoggedSet::chrono_cmp`: stamp, `entry_id`, content), never by storage
/// slot: the plan item's display spelling (`anchors.rs` latest eligible set)
/// and the item's `logged_sets` order (`completion.rs::judge_lift_item`). Undo
/// moves the row to the tail and must neither rename the plan card ("back
/// squat" ⇄ "Back squat") nor reorder its logged sets.
#[test]
fn undo_of_a_same_second_set_restores_the_plan_card() {
    let diffs = tie_history_undo_diffs(true);
    assert!(diffs.is_empty(), "{}", diffs.join("\n"));
}

/// A hypertrophy lifter who logs Lateral raise 3 × 15 @ 10 kg (no e1RM
/// anchor: a carried last-working load) and Bench press twice a week for six
/// weeks, then stops lifting. Returns the model and the last lift day.
fn isolation_lifter() -> (Model, i64) {
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
                let _ = app.update(
                    with_entry_id(log_set("Lateral raise", 10.0, 15, 8.0, t), id),
                    &mut m,
                );
                id += 1;
                let _ = app.update(
                    with_entry_id(log_set("Bench press", 60.0, 8, 8.0, t - 1800), id),
                    &mut m,
                );
            }
        }
    }
    (m, DAY0 + 7 * 5 + 3)
}

fn lateral_raise_loads(vm: &ViewModel, from: i64) -> Vec<f64> {
    vm.week_plan
        .iter()
        .filter(|s| s.epoch_day >= from)
        .flat_map(|s| &s.items)
        .filter(|i| i.exercise == "Lateral raise")
        .filter_map(|i| i.load_kg)
        .collect()
}

/// M03b: Table 3.4b resistance, 2–4 weeks off: "start ~80–85 % of prior
/// loads" (REENTRY-001). The carried last-working load of a lift with no
/// e1RM anchor takes the same derate (`anchors.rs` last-working loop).
#[test]
fn a_carried_isolation_load_restarts_lighter_after_three_weeks_off() {
    let (mut m, last) = isolation_lifter();
    let back = last + 25;
    today(&mut m, back);
    let loads = lateral_raise_loads(&Engine.view(&m), back);
    assert!(
        !loads.is_empty() && loads.iter().all(|&kg| kg <= 0.85 * 10.0),
        "back after {} days: Lateral raise at {loads:?} kg, 10 kg before",
        back - last - 1
    );
}

/// M03c: REENTRY-RAMP-001 scales only numbers set BEFORE the break
/// (`Anchors::carried_derate_for`): a session lifted since the return already
/// reflects it, so its load is carried unscaled, not cut a second time.
#[test]
fn a_load_lifted_since_the_return_is_not_derated_again() {
    let (mut m, last) = isolation_lifter();
    let back = last + 25;
    today(&mut m, back);
    let restart = lateral_raise_loads(&Engine.view(&m), back)[0];
    for k in 0..3 {
        let t = back * DAY_SEC + 64_800 + k * 120;
        let _ = Engine.update(
            with_entry_id(
                log_set("Lateral raise", restart, 15, 8.0, t),
                900 + k as u64,
            ),
            &mut m,
        );
    }
    today(&mut m, back + 1);
    let next = lateral_raise_loads(&Engine.view(&m), back + 1);
    assert!(
        !next.is_empty() && next.iter().all(|&kg| kg >= restart),
        "restarted at {restart} kg and lifted it; the next sessions show {next:?} kg"
    );
}
