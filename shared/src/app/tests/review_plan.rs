//! Run-plan and re-entry guards: the 3-day long run (`plan/run.rs`), the run
//! and lifting breaks (`app/anchors.rs` `build_run_break`,
//! `latest_break`, `implicit_layoff`) and the File 08 Table 3.4b
//! re-entry ramp (`individualization::run_reentry`, RUN-REENTRY-001,
//! REENTRY-RAMP-001).

use super::*;

/// A Monday (`day ≡ 4 mod 7` starts local weeks).
const MON: i64 = 20_745;

fn runner(days: u8, km: f64) -> Profile {
    Profile {
        goal_distance: GoalDistance::TenK,
        concurrent_goal: ConcurrentGoal::EndurancePriority,
        running_days_per_week: days,
        running_km_per_week: km,
        effective_from_epoch_day: Some(MON),
        ..base_profile()
    }
}

fn setup(p: Profile, start: i64) -> (Engine, Model) {
    let app = Engine;
    let mut model = Model::default();
    for e in [
        set_today_ev(start),
        Event::SetProfile(p),
        Event::GeneratePlan {
            start_epoch_day: start,
        },
    ] {
        let _ = app.update(e, &mut model);
    }
    (app, model)
}

fn run(app: &Engine, model: &mut Model, km: f64, day: i64) {
    let _ = app.update(
        log_run(km, km * 6.0, 70.0, 0.0, day * DAY_SEC + 8 * 3600, 0),
        model,
    );
}

/// (day, session title, shown km) of every run item on a day after `from`.
fn runs_after(vm: &ViewModel, from: i64) -> Vec<(i64, String, Option<f64>, String)> {
    vm.week_plan
        .iter()
        .filter(|s| s.epoch_day > from)
        .flat_map(|s| {
            s.items.iter().filter(|i| i.is_run()).map(move |i| {
                (
                    s.epoch_day,
                    s.title.clone(),
                    i.run_head_km,
                    i.summary.clone(),
                )
            })
        })
        .collect()
}

fn lift(exercise: &str, kg: f64, day: i64) -> LoggedSet {
    LoggedSet {
        hold_sec: None,
        exercise: exercise.into(),
        weight_kg: kg,
        reps: 5,
        rpe: 8.0,
        observed_at: day * DAY_SEC + 3_600,
        entry_id: day as u64,
    }
}

/// Anchors prescribed from the history through local day `through`.
fn anchors_through(sets: &[LoggedSet], ctx: AnchorCtx, through: i64) -> crate::plan::Anchors {
    build_plan_anchors(
        SetIndex::new(sets)
            .with_facts(sets)
            .filter(|(s, _)| s.observed_at / DAY_SEC <= through),
        ctx,
        through,
        0,
    )
}

/// Squat 3×/wk (Mon/Wed/Fri) on days `0..days`.
fn squat_block(days: i64) -> Vec<LoggedSet> {
    (0..days)
        .filter(|d| matches!(d % 7, 0 | 2 | 4))
        .map(|d| lift("Back Squat", 100.0, d))
        .collect()
}

/// The share of the anchored load the next squat is prescribed at: the
/// derate, 1.0 without one, 0.0 once the anchor is dropped (>8 wk: novice
/// restart, no %1RM load at all).
fn squat_frac(a: &crate::plan::Anchors) -> f64 {
    if a.e1rm_for("Back Squat").is_none() {
        return 0.0;
    }
    a.load_frac_for("Back Squat").map_or(1.0, |d| d.frac)
}

// ── Lifting re-entry: more days off never restart heavier (Table 3.4b, REENTRY-001) ──

#[test]
fn lifting_break_derate_is_monotone_in_days_off() {
    let sets = squat_block(42);
    let last = sets.iter().map(|s| s.observed_at / DAY_SEC).max().unwrap();
    let fracs: Vec<(i64, f64)> = (1..=60)
        .map(|off| {
            (
                off,
                squat_frac(&anchors_through(&sets, AnchorCtx::default(), last + off)),
            )
        })
        .collect();
    // More time off never restarts heavier (File 08 Table 3.4b brackets).
    for w in fracs.windows(2) {
        assert!(
            w[1].1 <= w[0].1 + 1e-9,
            "{} days off: {} → {} days off: {}",
            w[0].0,
            w[0].1,
            w[1].0,
            w[1].1
        );
    }
}

// ── Carried isolation load: the re-entry derate past 30 days (Table 3.4b) ──

#[test]
fn carried_isolation_load_derate_is_monotone_in_days_off() {
    assert!(crate::plan::is_isolation_lift("Lateral raise"));
    let sets: Vec<LoggedSet> = (0..42)
        .filter(|d| matches!(d % 7, 0 | 2 | 4))
        .map(|d| LoggedSet {
            reps: 12,
            ..lift("Lateral raise", 10.0, d)
        })
        .collect();
    let last = 39;
    let fracs: Vec<(i64, f64)> = (1..=56)
        .map(|off| {
            let a = anchors_through(&sets, AnchorCtx::default(), last + off);
            assert!(a.last_working_for("Lateral raise").is_some(), "{off}");
            (
                off,
                a.carried_derate_for("Lateral raise")
                    .map_or(1.0, |d| d.frac),
            )
        })
        .collect();
    for w in fracs.windows(2) {
        assert!(
            w[1].1 <= w[0].1 + 1e-9,
            "{} days off: {} → {} days off: {}",
            w[0].0,
            w[0].1,
            w[1].0,
            w[1].1
        );
    }
}

// ── An ended declared layoff never shadows a later lifting break ──

#[test]
fn an_old_declared_layoff_does_not_disable_later_lifting_reentry() {
    // Declared 3 wk off, returned on day 0; trained regularly for 20 weeks,
    // then 18 days off ALL lifting (2–4 wk bracket: start ~80–85 %).
    let sets = squat_block(140);
    let last = sets.iter().map(|s| s.observed_at / DAY_SEC).max().unwrap();
    let through = last + 18;
    let control = squat_frac(&anchors_through(&sets, AnchorCtx::default(), through));
    let declared = AnchorCtx {
        layoff: Some(Layoff {
            weeks_off: 3.0,
            returned_epoch_day: Some(0),
        }),
        ..AnchorCtx::default()
    };
    let with_old_layoff = squat_frac(&anchors_through(&sets, declared, through));
    assert!(control < 1.0, "control is on the re-entry ramp: {control}");
    assert_eq!(
        with_old_layoff, control,
        "a layoff that ended 20 weeks ago must not cancel today's re-entry"
    );
}

// ── An ended declared layoff never shadows a later run break (RUN-REENTRY-001) ──

/// Total km of the run items on days after `from` (the rest of the week).
fn week_run_km(vm: &ViewModel, from: i64) -> f64 {
    runs_after(vm, from).iter().filter_map(|r| r.2).sum()
}

/// 3 runs a week (Mon/Wed/Sat, 10 km) on days `MON..MON+weeks*7`, then
/// nothing; today = `today_off` days after the last run.
fn run_history(p: Profile, weeks: i64, today_off: i64) -> (Engine, Model, i64) {
    let (app, mut model) = setup(p, MON);
    let mut last = MON;
    for d in 0..weeks * 7 {
        if matches!(d % 7, 0 | 2 | 5) {
            set_today(&app, &mut model, MON + d);
            run(&app, &mut model, 10.0, MON + d);
            last = MON + d;
        }
    }
    set_today(&app, &mut model, last + today_off);
    (app, model, last + today_off)
}

#[test]
fn an_old_declared_layoff_does_not_disable_run_reentry() {
    let control = runner(3, 30.0);
    let declared = Profile {
        weeks_off: Some(3.0),
        layoff_returned_epoch_day: Some(MON),
        ..runner(3, 30.0)
    };
    // 20 weeks of 30 km/wk, then 10 weeks (72 days, today a Monday) off.
    let (app, m, t) = run_history(control, 20, 72);
    let control_km = week_run_km(&app.view(&m), t);
    let (app, m, t) = run_history(declared, 20, 72);
    let declared_km = week_run_km(&app.view(&m), t);
    assert!(
        declared_km <= control_km + 0.5,
        "after 10 weeks off the old declared layoff restarts at {declared_km} km vs {control_km} km"
    );
}

// ── A run break ends with its re-entry ramp (RUN-REENTRY-001) ──

/// 3 runs/wk (Mon/Wed/Sat) for `weeks`, skipping days in `gap`; the last
/// 5 weeks: one 18 km long run 40 days before today, else 8 km.
fn long_history(p: Profile, gap: std::ops::Range<i64>) -> (Engine, Model, i64) {
    let (app, mut model) = setup(p, MON);
    let end = 40 * 7;
    for d in 0..end {
        if !matches!(d % 7, 0 | 2 | 5) || gap.contains(&d) {
            continue;
        }
        set_today(&app, &mut model, MON + d);
        let km = if d == end - 40 { 18.0 } else { 8.0 };
        run(&app, &mut model, km, MON + d);
    }
    let t = MON + end;
    set_today(&app, &mut model, t);
    (app, model, t)
}

fn long_run_km(vm: &ViewModel, from: i64) -> f64 {
    runs_after(vm, from)
        .iter()
        .filter(|r| r.1 == "Long run")
        .filter_map(|r| r.2)
        .fold(0.0, f64::max)
}

#[test]
fn a_months_old_run_break_does_not_change_todays_long_run() {
    // Identical last 30 weeks; one athlete skipped weeks 2–4 (days 14..35).
    let (app, m, t) = long_history(runner(3, 24.0), 0..0);
    let control = long_run_km(&app.view(&m), t);
    let (app, m2, t2) = long_history(runner(3, 24.0), 14..35);
    let with_old_break = long_run_km(&app.view(&m2), t2);
    assert_eq!(
        with_old_break, control,
        "a break 8 months ago still governs"
    );
}

#[test]
fn a_fresh_stated_longest_run_counts_after_an_old_break() {
    let stated = |mut p: Profile| {
        p.stated_longest_recent_km = Some(16.0);
        p.effective_from_epoch_day = Some(MON + 40 * 7);
        p
    };
    let mut out = Vec::new();
    for gap in [0..0, 14..35] {
        let (app, mut m, t) = long_history(runner(3, 24.0), gap);
        // Today the athlete answers "longest run in the last 30 days: 16 km".
        let _ = app.update(Event::SetProfile(stated(runner(3, 24.0))), &mut m);
        out.push(long_run_km(&app.view(&m), t));
    }
    assert_eq!(
        out[1], out[0],
        "same answer, same recent runs, different long run"
    );
}

/// Distance km of the run prescriptions in a synthesized week.
fn program_run_km(p: &crate::schema::Program) -> f64 {
    use crate::schema::{Prescription, RunVolume};
    p.mesocycle
        .sessions
        .iter()
        .flat_map(|s| &s.prescriptions)
        .filter_map(|r| match &r.value {
            Prescription::Run(rx) => match rx.volume {
                RunVolume::DistanceKm(k) => Some(k as f64),
                _ => None,
            },
            _ => None,
        })
        .sum()
}

#[test]
fn a_months_old_short_break_does_not_lower_the_stated_floor() {
    // 3 × 8 km = 24 km/wk logged, 30 km/wk stated; one 8-day gap (days
    // 12 → 21) 37 weeks before today.
    let (_, m, t) = long_history(runner(3, 30.0), 14..21);
    let facts = RunFacts::new(&m.runs);
    // The gap is a 1–2 wk break the day after it (90 %, 1-week ramp) ...
    let fresh = crate::app::anchors::build_run_break(&m.runs, &facts, MON + 21, 0);
    assert!(
        fresh.is_some_and(|b| b.reentry.active && b.reentry.volume_frac < 1.0),
        "{fresh:?}"
    );
    // ... and its ramp ended long ago: it no longer shapes today's plan.
    let brk = crate::app::anchors::build_run_break(&m.runs, &facts, t, 0);
    assert_eq!(brk, None);
    // The same anchors with and without that break.
    let anchors = |run_break| crate::plan::Anchors {
        recent_weekly_km: Some(24.0),
        longest_recent_run_km: Some(8.0),
        week_start_epoch_day: Some(t),
        plan_start_epoch_day: Some(MON),
        run_break,
        ..Default::default()
    };
    let p = runner(3, 30.0);
    let without = crate::plan::synthesize(&p, &anchors(None), 0).unwrap();
    let with = crate::plan::synthesize(&p, &anchors(brk), 0).unwrap();
    assert_eq!(
        program_run_km(&with),
        program_run_km(&without),
        "a 37-week-old 8-day gap still changes the week"
    );
}

// ── More stated volume never prescribes a smaller week (RUN-LONGRUN-001) ──

#[test]
fn more_stated_volume_never_prescribes_a_smaller_week() {
    let mut prev: Option<(f64, f64, f64)> = None;
    for km in [58.0, 60.0, 62.0, 63.0, 64.0, 66.0, 68.0] {
        let (app, m) = setup(runner(3, km), MON);
        let vm = app.view(&m);
        let rows = runs_after(&vm, MON - 1);
        let total: f64 = rows.iter().filter_map(|r| r.2).sum();
        let long = long_run_km(&vm, MON - 1);
        if let Some((pk, pl, pt)) = prev {
            assert!(
                total >= pt && long >= pl,
                "{pk} km/wk → long {pl} / week {pt}; {km} km/wk → long {long} / week {total}"
            );
        }
        prev = Some((km, long, total));
    }
}

// ── Stated rules of the run plan and the re-entry brackets ─────────────────

/// The 3-day long run: 16 km/wk gives 5 km and 40 km/wk 13; 60 km/wk is more
/// than three runs hold under the 90-min easy-run ceiling (running-015), so
/// the long run is sized on the week they hold: 13.5. At 4+ days the 30 %
/// share governs; no easy day outruns the long run; the long run stays within
/// 2× the daily average.
#[test]
fn claim_three_day_long_run_examples() {
    for (days, km, want) in [
        (3u8, 16.0, 5.0),
        (3, 40.0, 13.0),
        (3, 60.0, 13.5),
        (4, 40.0, 12.0),
    ] {
        let (app, m) = setup(runner(days, km), MON);
        let vm = app.view(&m);
        let rows = runs_after(&vm, MON - 1);
        let long = long_run_km(&vm, MON - 1);
        assert_eq!(long, want, "{days} d {km} km: {rows:?}");
        let easiest_max = rows
            .iter()
            .filter(|r| r.1 != "Long run")
            .filter_map(|r| r.2)
            .fold(0.0, f64::max);
        assert!(easiest_max <= long, "{rows:?}");
        assert!(long <= 2.0 * km / f64::from(days), "{rows:?}");
    }
}

/// RUN-REENTRY-001 brackets at the day boundaries (days fully off before the
/// prescribed day): 6 → none, 7–13 → 90 %, 14–27 → 75 % + rebuild, 28–56 →
/// 55 %, 57+ → no pre-break floor (Table 3.4b "rebuild base", no fraction).
/// A break still running is day 0 of its ramp, so the fraction is the
/// bracket's start.
#[test]
fn claim_run_reentry_bracket_boundaries() {
    let (_, m, _) = run_history(runner(3, 30.0), 6, 2);
    let last = m
        .runs
        .iter()
        .map(|r| r.observed_at / DAY_SEC)
        .max()
        .unwrap();
    let facts = RunFacts::new(&m.runs);
    let mut got = Vec::new();
    for off in [6, 7, 8, 13, 14, 15, 27, 28, 29, 55, 56, 57] {
        let b = crate::app::anchors::build_run_break(&m.runs, &facts, last + off, 0);
        assert!(b.is_none_or(|b| b.reentry.active), "{off}: {b:?}");
        got.push((
            off,
            b.map(|b| (b.reentry.volume_frac, b.reentry.rebuild_long_run)),
        ));
    }
    let want = [
        (6, None),
        (7, Some((0.90, false))),
        (8, Some((0.90, false))),
        (13, Some((0.90, false))),
        (14, Some((0.75, true))),
        (15, Some((0.75, true))),
        (27, Some((0.75, true))),
        (28, Some((0.55, true))),
        (29, Some((0.55, true))),
        (55, Some((0.55, true))),
        (56, Some((0.55, true))),
        (57, Some((0.0, true))),
    ];
    assert_eq!(got, want);
}

/// The 3-day runner's 4 rest days are never a "break", and a brand-new user
/// without a logged run has none.
#[test]
fn claim_plan_rest_days_and_no_history_are_no_break() {
    let (_, m, t) = run_history(runner(3, 30.0), 12, 2);
    let facts = RunFacts::new(&m.runs);
    for through in t - 60..=t {
        assert_eq!(
            crate::app::anchors::build_run_break(&m.runs, &facts, through, 0),
            None,
            "day {through}"
        );
    }
    assert_eq!(
        crate::app::anchors::build_run_break(&[], &RunFacts::new(&[]), MON, 0),
        None
    );
}

// ── A long run over the 25–30 % share does not cite the share rule ─────────

#[test]
fn a_long_run_over_the_share_does_not_cite_the_share_rule() {
    let (app, m) = setup(runner(3, 40.0), MON);
    let it = planned_long_run_item(&app, &m);
    let km = it.run_head_km.unwrap();
    let cites_share = it.why.basis.contains("25–30% of your weekly distance");
    assert!(
        !(km > 0.30 * 40.0 && cites_share),
        "{km} km of 40 km/wk ({:.1} %) justified by: {}",
        km / 40.0 * 100.0,
        it.why.basis
    );
    // The card states the rule that set it, under the RUN-LONGRUN-001 claim
    // (the low-mileage guardrail is part of it; Daniels 2022).
    assert!(it.why.basis.contains("average run"), "{}", it.why.basis);
    assert!(
        it.evidence.citation.contains("Daniels"),
        "{}",
        it.evidence.citation
    );
    // A 4-day week keeps its 30 % share and the claim's own statement.
    let (app, m) = setup(runner(4, 40.0), MON);
    let it = planned_long_run_item(&app, &m);
    assert_eq!(it.run_head_km, Some(12.0));
    assert!(
        it.why.basis.contains("25–30% of your weekly distance"),
        "{}",
        it.why.basis
    );
}

/// The same runs logged in a different order give the same week (the run
/// break, its prior volume and the since-break longest are order-free).
#[test]
fn claim_reentry_plan_is_independent_of_log_order() {
    let days: Vec<(f64, i64)> = (0..28)
        .filter(|d| matches!(d % 7, 0 | 2 | 5))
        .map(|d| (6.0 + (d % 5) as f64, MON + d))
        .chain([(4.0, MON + 47), (5.0, MON + 49)])
        .collect();
    let mut views = Vec::new();
    for order in [false, true] {
        let (app, mut m) = setup(runner(3, 30.0), MON);
        set_today(&app, &mut m, MON + 50);
        let mut ds = days.clone();
        if order {
            ds.reverse();
        }
        for (km, d) in ds {
            run(&app, &mut m, km, d);
        }
        let vm = app.view(&m);
        views.push(serde_json::to_string(&vm.week_plan).unwrap());
    }
    assert_eq!(views[0], views[1]);
}

/// `carried_derate_for` never applies the re-entry reduction again to a load
/// lifted since the return.
#[test]
fn claim_carried_load_is_not_derated_twice() {
    let mut sets: Vec<LoggedSet> = (0..42)
        .filter(|d| matches!(d % 7, 0 | 2 | 4))
        .map(|d| LoggedSet {
            reps: 12,
            ..lift("Lateral raise", 10.0, d)
        })
        .collect();
    // 18 days off all lifting, back on day 58 at the ramped load.
    let pre = anchors_through(&sets, AnchorCtx::default(), 57);
    assert_eq!(
        pre.carried_derate_for("Lateral raise").map(|d| d.frac),
        Some(0.825)
    );
    sets.push(LoggedSet {
        reps: 12,
        ..lift("Lateral raise", 8.25, 58)
    });
    let post = anchors_through(&sets, AnchorCtx::default(), 58);
    assert_eq!(post.carried_derate_for("Lateral raise"), None);
    assert_eq!(
        post.last_working_for("Lateral raise").unwrap().weight_kg,
        8.25
    );
}

// ── 4–8 wk off: near-novice spike caps (Table 3.4b running) ─────────────────

/// Table 3.4b running: 2–4 wk off → rebuild the long run (only runs since the
/// return count as long-run capacity, but a run from before the break inside
/// the 30-day window is still RUN-SPIKE-001's baseline); 4–8 wk off → treat
/// near-novice for spike caps: every single run is judged only against what
/// was run since the return.
#[test]
fn four_to_eight_weeks_off_judges_single_runs_differently_from_two_to_four() {
    let p = runner(3, 30.0);
    let prog = |weeks_off: f64| {
        let a = crate::plan::Anchors {
            // Measured since the return above both re-entry floors, so the
            // volume is the same and only the bracket differs. The 14 km run
            // is from before the break, the 6 km one since.
            recent_weekly_km: Some(30.0),
            longest_recent_run_km: Some(14.0),
            week_start_epoch_day: Some(MON + 100),
            plan_start_epoch_day: Some(MON),
            run_break: Some(crate::plan::RunBreak {
                weeks_off,
                last_run_epoch_day: MON + 60,
                prior_weekly_km: Some(30.0),
                reentry: crate::individualization::run_reentry(weeks_off, 7).value,
                longest_since_km: Some(6.0),
                longest_since_detrained: false,
                stated_longest_after: false,
            }),
            ..Default::default()
        };
        crate::plan::synthesize(&p, &a, 0).unwrap()
    };
    let run_kms = |prog: &crate::schema::Program| -> Vec<f64> {
        use crate::schema::{Prescription, RunVolume};
        prog.mesocycle
            .sessions
            .iter()
            .flat_map(|s| &s.prescriptions)
            .filter_map(|r| match &r.value {
                Prescription::Run(rx) => match rx.volume {
                    RunVolume::DistanceKm(k) => Some(f64::from(k)),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    };
    let (three, five) = (run_kms(&prog(3.0)), run_kms(&prog(5.0)));
    // 5 wk: no run more than 10 % past the 6 km run since the return.
    assert!(five.iter().all(|&k| k <= 6.5), "{five:?}");
    // 3 wk: the long run is rebuilt from the week's share, not raised to the
    // 14 km pre-break run, yet that run still bounds single runs.
    let long3 = three.iter().copied().fold(0.0, f64::max);
    assert!(long3 > 6.5 && long3 < 14.0, "{three:?}");
}

// ── The running re-entry ramps back and ends (RUN-REENTRY-001) ─────────────

/// Table 3.4b is a re-entry RAMP (load-036): the bracket's start climbs
/// linearly back to the prior volume over the row's window (1, 2, 4, 6+
/// weeks: the table's resistance column, the only ramp length it states),
/// then the break no longer shapes the plan. Past 8 weeks it starts from
/// nothing before the break ("rebuild base", no fraction).
#[test]
fn run_reentry_ramps_back_to_full_and_ends() {
    let r = |weeks_off: f64, t: i64| crate::individualization::run_reentry(weeks_off, t).value;
    let none = r(0.5, 0);
    assert_eq!((none.volume_frac, none.active), (1.0, false));
    for (weeks_off, start, ramp_days) in [
        (1.5, 0.90, 7),
        (3.0, 0.75, 14),
        (5.0, 0.55, 28),
        (10.0, 0.0, 42),
    ] {
        assert_eq!(r(weeks_off, 0).volume_frac, start, "{weeks_off} wk");
        let t = ramp_days / 2;
        let mid = r(weeks_off, t).volume_frac;
        let linear = start + (1.0 - start) * t as f64 / ramp_days as f64;
        assert!((mid - linear).abs() < 1e-9, "{weeks_off} wk day {t}: {mid}");
        assert!(r(weeks_off, ramp_days - 1).active, "{weeks_off} wk");
        let end = r(weeks_off, ramp_days);
        assert_eq!(
            (end.volume_frac, end.active),
            (1.0, false),
            "{weeks_off} wk"
        );
    }
}

/// RUN-START-001: a longest run the athlete states AFTER the break began is a
/// current answer, so it counts through the long-run rebuild (one stated
/// before the break does not: `a_three_week_running_break_rebuilds_the_long_run`).
#[test]
fn a_longest_run_stated_after_the_break_counts_through_the_rebuild() {
    let mut out = Vec::new();
    for state_it in [false, true] {
        let (app, mut m) = setup(runner(3, 24.0), MON);
        // Six weeks of 8 km runs with a 12 km Saturday long run, then three
        // weeks off; back on Monday with two 5 km runs.
        for d in 0..42 {
            if matches!(d % 7, 0 | 2 | 5) {
                set_today(&app, &mut m, MON + d);
                run(&app, &mut m, if d % 7 == 5 { 12.0 } else { 8.0 }, MON + d);
            }
        }
        for d in [63, 65] {
            set_today(&app, &mut m, MON + d);
            run(&app, &mut m, 5.0, MON + d);
        }
        let t = MON + 66;
        set_today(&app, &mut m, t);
        if state_it {
            let p = Profile {
                stated_longest_recent_km: Some(9.0),
                effective_from_epoch_day: Some(t),
                ..runner(3, 24.0)
            };
            let _ = app.update(Event::SetProfile(p), &mut m);
        }
        let vm = app.view(&m);
        out.push(long_run_km(&vm, t - 1));
    }
    assert!(
        out[0] < 9.0 && out[1] == 9.0,
        "long run without / with the answer: {out:?}"
    );
}
