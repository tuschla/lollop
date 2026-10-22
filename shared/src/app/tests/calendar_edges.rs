//! Calendar edges through `update` / `view`: Monday-start weeks across year
//! ends (ISO week 53), leap days and a 29 February birthday crossing 18,
//! sessions crossing local midnight, UTC offsets from −12 h to +14 h and an
//! offset change (DST/travel), a clock moving backwards, future-dated and
//! backdated entries, a mid-week plan start and a race date today or past.
//!
//! The core only knows time as `SetToday { epoch_day, utc_offset_sec }` (the
//! shell's LOCAL day and offset) plus UTC `observed_at` stamps, so every edge
//! here is a combination of those two.

use super::*;
use crate::running::civil_from_days;

const HOUR: i64 = 3_600;

/// Days since 1970-01-01 of a proleptic Gregorian date (H. Hinnant's
/// algorithm), independent of the core's calendar code.
fn ymd(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let (m, d) = (i64::from(m), i64::from(d));
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The Monday on or before local day `d` (1970-01-05 was a Monday), computed
/// without the core's `mon0_weekday`.
fn monday_of(d: i64) -> i64 {
    d - (d - 4).rem_euclid(7)
}

/// `hour:min` local on local day `d` at `offset`, as a UTC stamp.
fn local(d: i64, hour: i64, min: i64, offset: i64) -> i64 {
    d * DAY_SEC + hour * HOUR + min * 60 - offset
}

fn set_today(app: &Engine, model: &mut Model, d: i64, offset: i64) {
    app.update(
        Event::SetToday {
            epoch_day: d,
            utc_offset_sec: offset,
        },
        model,
    )
    .expect_only_render();
}

fn send(app: &Engine, model: &mut Model, e: Event) {
    app.update(e, model).expect_only_render();
}

fn row(vm: &ViewModel, d: i64) -> SessionPlanView {
    vm.week_plan
        .iter()
        .find(|s| s.epoch_day == d)
        .unwrap_or_else(|| panic!("no strip row for day {d}: {:?}", vm.week_plan))
        .clone()
}

fn is_judged(s: SessionStatus) -> bool {
    matches!(
        s,
        SessionStatus::Done
            | SessionStatus::DoneLighter
            | SessionStatus::Missed
            | SessionStatus::Partial
    )
}

/// Log every item of `row` as prescribed, `at` local `hour` on its day. Lifts
/// get every set at the shown weight and top reps; runs their shown distance.
fn log_row(
    app: &Engine,
    model: &mut Model,
    row: &SessionPlanView,
    hour: i64,
    offset: i64,
    id: &mut u64,
) {
    let mut t = local(row.epoch_day, hour, 0, offset);
    for it in &row.items {
        *id += 1;
        if it.is_run() {
            let km = it.run_head_km.unwrap_or(6.0);
            send(app, model, log_run(km, km * 6.0, 70.0, 50.0, t, *id));
        } else {
            let kg = it.load_kg.or(it.suggested_load_kg).unwrap_or(40.0);
            for _ in 0..it.sets.max(1) {
                *id += 1;
                let reps = u32::from(it.reps_high.max(it.reps_low).max(1));
                let rpe = it.target_rpe.unwrap_or(7.0);
                let e = if it.timed_hold {
                    cal_hold(&it.exercise, it.hold_high_sec.max(10), rpe, t)
                } else {
                    with_entry_id(log_set(&it.exercise, kg, reps, rpe, t), *id)
                };
                send(app, model, e);
                t += 120;
            }
        }
        t += 300;
    }
}

/// The calendar invariants every view keeps for local day `today`.
fn assert_calendar(vm: &ViewModel, today: i64, ctx: &str) {
    assert_eq!(vm.today_epoch_day, Some(today), "{ctx}: today echo");
    let monday = monday_of(today);
    if !vm.week_plan.is_empty() {
        let days: Vec<i64> = vm.week_plan.iter().map(|s| s.epoch_day).collect();
        let want: Vec<i64> = (monday..monday + 7).collect();
        assert_eq!(days, want, "{ctx}: the strip is this local Mon–Sun week");
        for s in &vm.week_plan {
            if s.epoch_day > today {
                assert!(
                    !is_judged(s.status),
                    "{ctx}: future day {} judged {:?}",
                    s.epoch_day,
                    s.status
                );
            }
            if s.epoch_day < today && s.session_type != SessionType::Rest && !s.items.is_empty() {
                assert_ne!(
                    s.status,
                    SessionStatus::Next,
                    "{ctx}: past day {} is the hero",
                    s.epoch_day
                );
            }
            if s.status == SessionStatus::BeforePlan {
                assert!(s.items.is_empty(), "{ctx}: a pre-plan day has items");
            }
        }
    }
    if let Some(ns) = &vm.next_session {
        assert!(
            ns.epoch_day >= today,
            "{ctx}: next session {} before today {today}",
            ns.epoch_day
        );
        if ns.epoch_day < monday + 7 {
            assert_eq!(
                &row(vm, ns.epoch_day),
                ns,
                "{ctx}: the hero mirrors its strip row"
            );
        } else {
            assert_eq!(
                monday_of(ns.epoch_day),
                monday + 7,
                "{ctx}: a wrapped hero sits in next week"
            );
        }
    }
    if let Some(c) = &vm.consistency {
        let planned: Vec<&SessionPlanView> = vm
            .week_plan
            .iter()
            .filter(|s| {
                s.session_type != SessionType::Rest && s.status != SessionStatus::BeforePlan
            })
            .collect();
        assert_eq!(
            usize::from(c.this_week_planned),
            planned.len(),
            "{ctx}: planned = strip"
        );
        assert_eq!(
            usize::from(c.this_week_done),
            planned.iter().filter(|s| s.status.is_done()).count(),
            "{ctx}: done = strip"
        );
        for (k, w) in c.weeks.iter().enumerate() {
            assert_eq!(
                monday_of(w.week_monday_epoch_day),
                w.week_monday_epoch_day,
                "{ctx}"
            );
            assert!(w.done <= w.planned, "{ctx}: week {w:?}");
            let back = (c.weeks.len() - k) as i64;
            assert_eq!(
                w.week_monday_epoch_day,
                monday - 7 * back,
                "{ctx}: finished weeks are the consecutive weeks before this one"
            );
        }
        let summary = &c.notes[0].summary;
        assert!(
            summary.contains(&format!("{} planned", c.this_week_planned)),
            "{ctx}: {summary}"
        );
    }
    if let Some(rp) = &vm.run_progress {
        assert_eq!(rp.weekly.len(), 16, "{ctx}");
        for (k, w) in rp.weekly.iter().enumerate() {
            assert_eq!(
                w.week_start_epoch_day,
                monday - 7 * (15 - k as i64),
                "{ctx}: run weeks are the 16 local weeks ending this one"
            );
        }
    }
    if let Some(p) = &vm.program {
        assert!(
            p.week >= 1 && p.week <= p.weeks_total,
            "{ctx}: program week {} of {}",
            p.week,
            p.weeks_total
        );
    }
    if let Some(y) = &vm.youth_activity {
        assert_eq!(y.week_monday_epoch_day, monday, "{ctx}");
    }
}

#[test]
fn the_reference_calendar_agrees_with_the_core() {
    assert_eq!(ymd(1970, 1, 1), 0);
    assert_eq!(ymd(2026, 10, 17), 20_743, "the youth tests' 2026-10-17");
    for d in [
        ymd(2026, 12, 31),
        ymd(2028, 2, 29),
        ymd(2000, 2, 29),
        ymd(1969, 12, 31),
    ] {
        let (y, m, dd) = civil_from_days(d);
        assert_eq!(ymd(y, m, dd), d);
    }
    // 2026 is an ISO 53-week year: 1 January 2026 is a Thursday.
    assert_eq!(monday_of(ymd(2026, 1, 1)), ymd(2025, 12, 29));
    assert_eq!(monday_of(ymd(2026, 12, 31)), ymd(2026, 12, 28));
    assert_eq!(monday_of(ymd(2027, 1, 3)), ymd(2026, 12, 28));
}

/// A hybrid athlete with a plan, a varied log (runs, sets, check-ins; some
/// logged minutes either side of local midnight), viewed every day around
/// year ends, ISO week 53, leap days and offsets from −12 h to +14 h.
#[test]
fn calendar_invariants_hold_across_year_ends_leap_days_and_offsets() {
    let pivots = [
        ("iso53-end", ymd(2026, 12, 31)),
        ("new-year-2028", ymd(2028, 1, 1)),
        ("leap-2028", ymd(2028, 2, 29)),
        ("common-feb-2027", ymd(2027, 2, 28)),
        ("dst-eu-2027", ymd(2027, 3, 28)),
    ];
    let offsets = [-12 * HOUR, -5 * HOUR, 0, 5 * HOUR + 1_800, 14 * HOUR];
    for (name, pivot) in pivots {
        for (oi, &offset) in offsets.iter().enumerate() {
            let ctx0 = format!("{name} @ {offset}s");
            let app = Engine;
            let mut model = Model::default();
            let start = pivot - 30;
            send(
                &app,
                &mut model,
                Event::SetProfile(Profile {
                    effective_from_epoch_day: Some(start),
                    age_years: Some(33.0),
                    ..sample_profile()
                }),
            );
            // A plan start on a varying weekday, before the pivot.
            let plan_start = pivot - 17 + oi as i64;
            let mut id = 7_000;
            for d in start..=pivot + 8 {
                set_today(&app, &mut model, d, offset);
                if d == plan_start {
                    send(&app, &mut model, Event::GeneratePlan { start_epoch_day: d });
                }
                let wd = (d - 4).rem_euclid(7);
                if d % 3 != 0 {
                    send(
                        &app,
                        &mut model,
                        Event::SubmitCheckin(crate::schema::CheckinInput {
                            observed_at: local(d, 6, 45, offset),
                            sleep_quality: Some(4),
                            soreness: Some(2),
                            mood: Some(4),
                            fatigue: Some(2),
                            stress: Some(2),
                            resting_hr_bpm: Some(52.0 + (d % 4) as f64),
                            hrv_rmssd_ms: Some(60.0 + (d % 7) as f64),
                        }),
                    );
                }
                let vm = app.view(&model);
                assert_calendar(&vm, d, &format!("{ctx0} day {d}"));
                // Follow the plan when it has today's session; before it,
                // and on odd days, log free work, some near midnight.
                if let Some(ns) = vm
                    .next_session
                    .as_ref()
                    .filter(|s| s.epoch_day == d && d % 5 != 0)
                {
                    let hour = if wd == 6 { 23 } else { 18 };
                    log_row(&app, &mut model, ns, hour, offset, &mut id);
                } else if d < plan_start && wd % 2 == 1 {
                    id += 1;
                    let t = local(d, 23, 40, offset);
                    send(&app, &mut model, log_run(7.0, 42.0, 72.0, 50.0, t, id));
                }
                let vm = app.view(&model);
                assert_calendar(&vm, d, &format!("{ctx0} day {d} after logging"));
            }
        }
    }
}

/// ISO week 53 of 2026 (Mon 28 Dec – Sun 3 Jan) is one week everywhere: the
/// strip, the weekly running buckets and the consistency card.
#[test]
fn iso_week_53_is_one_week_across_the_year_boundary() {
    let mon = ymd(2026, 12, 28);
    let thu = ymd(2026, 12, 31);
    let sat = ymd(2027, 1, 2);
    let next_mon = ymd(2027, 1, 4);
    let (app, mut model) = planned_strength_model_on(mon - 14, true);
    let mut id = 1;
    for (d, km) in [(thu, 8.0), (sat, 12.0)] {
        id += 1;
        send(
            &app,
            &mut model,
            log_run(km, km * 6.0, 70.0, 50.0, local(d, 7, 0, 0), id),
        );
    }
    set_today(&app, &mut model, sat, 0);
    let vm = app.view(&model);
    assert_calendar(&vm, sat, "Sat 2 Jan 2027");
    let first = civil_from_days(vm.week_plan[0].epoch_day);
    let last = civil_from_days(vm.week_plan[6].epoch_day);
    assert_eq!((first, last), ((2026, 12, 28), (2027, 1, 3)));
    let week = vm
        .run_progress
        .as_ref()
        .unwrap()
        .weekly
        .last()
        .unwrap()
        .clone();
    assert_eq!((week.week_start_epoch_day, week.runs), (mon, 2), "{week:?}");
    assert!((week.km - 20.0).abs() < 1e-9, "{week:?}");

    set_today(&app, &mut model, next_mon, 0);
    let vm = app.view(&model);
    assert_calendar(&vm, next_mon, "Mon 4 Jan 2027");
    let weekly = &vm.run_progress.as_ref().unwrap().weekly;
    assert_eq!(weekly[15].runs, 0, "a new week starts empty");
    assert_eq!((weekly[14].week_start_epoch_day, weekly[14].runs), (mon, 2));
    let c = vm.consistency.unwrap();
    assert_eq!(c.weeks.last().unwrap().week_monday_epoch_day, mon);
}

/// Born 29 February 2008: 17 through 28 February 2026, 18 on 1 March 2026
/// (2026 has no 29 February), 20 on 29 February 2028. The under-18 hold and
/// the youth card follow the day, both ways, as the clock moves.
#[test]
fn a_feb_29_birthday_crosses_18_on_march_1_of_a_common_year() {
    let born = ymd(2008, 2, 29);
    let app = Engine;
    let mut model = Model::default();
    send(
        &app,
        &mut model,
        Event::SetProfile(Profile {
            birth_epoch_day: Some(born),
            ..sample_profile()
        }),
    );
    let age = |vm: &ViewModel| vm.profile.as_ref().unwrap().age_years.unwrap();
    let check = |model: &mut Model, d: i64, want_age: f64, youth: bool| {
        set_today(&app, model, d, 0);
        let vm = app.view(model);
        let ctx = format!("{:?}", civil_from_days(d));
        assert_eq!(age(&vm), want_age, "{ctx}");
        assert_eq!(
            vm.hold_reason == Some(HoldReason::Youth),
            youth,
            "{ctx}: {:?}",
            vm.hold_reason
        );
        assert_eq!(vm.youth_activity.is_some(), youth, "{ctx}");
        assert_eq!(vm.train_blocked, youth, "{ctx}");
        assert_eq!(vm.profile.as_ref().unwrap().health.youth, youth, "{ctx}");
    };
    check(&mut model, ymd(2026, 2, 28), 17.0, true);
    check(&mut model, ymd(2026, 3, 1), 18.0, false);
    // The clock moving back re-applies the hold: age comes from today.
    check(&mut model, ymd(2026, 2, 28), 17.0, true);
    check(&mut model, ymd(2027, 2, 28), 18.0, false);
    check(&mut model, ymd(2027, 3, 1), 19.0, false);
    check(&mut model, ymd(2028, 2, 28), 19.0, false);
    check(&mut model, ymd(2028, 2, 29), 20.0, false);

    // Once adult, a plan starts the same day and its strip is the local week.
    let d = ymd(2026, 3, 1);
    set_today(&app, &mut model, d, 0);
    send(&app, &mut model, Event::GeneratePlan { start_epoch_day: d });
    let vm = app.view(&model);
    assert!(vm.plan_withheld.is_none(), "{:?}", vm.plan_withheld);
    assert_calendar(&vm, d, "first adult day");
    assert_eq!(vm.next_session.as_ref().map(|s| s.epoch_day), Some(d));
}

/// A GPS run starting 23:40 local on Sunday and finishing after midnight
/// belongs to Sunday's week (its start), at any offset.
#[test]
fn a_run_across_local_midnight_counts_in_the_week_it_started() {
    let sunday = ymd(2027, 3, 7);
    assert_eq!((sunday - 4).rem_euclid(7), 6);
    for offset in [-10 * HOUR, 0, 2 * HOUR, 13 * HOUR] {
        let app = Engine;
        let mut model = Model::default();
        send(&app, &mut model, Event::SetProfile(running_profile()));
        let begin = local(sunday, 23, 40, offset);
        // 6 km north at 5:00/km: 30 minutes, ending 00:10 Monday local.
        let pts: Vec<GpsPoint> = (0..=360)
            .map(|i| GpsPoint {
                lat: 48.0 + 6.0 / 111.0 * f64::from(i) / 360.0,
                lon: 11.0,
                observed_at: begin + 5 * i64::from(i),
                accuracy_m: 4.0,
            })
            .collect();
        send(&app, &mut model, log_run_track(pts, 72.0, 50.0, begin, 1));
        set_today(&app, &mut model, sunday, offset);
        let vm = app.view(&model);
        let w = vm
            .run_progress
            .as_ref()
            .expect("a dated run")
            .weekly
            .last()
            .unwrap()
            .clone();
        assert_eq!(
            (w.week_start_epoch_day, w.runs),
            (sunday - 6, 1),
            "offset {offset}: {w:?}"
        );
        set_today(&app, &mut model, sunday + 1, offset);
        let vm = app.view(&model);
        let weekly = &vm.run_progress.as_ref().unwrap().weekly;
        assert_eq!(
            weekly[15].runs, 0,
            "offset {offset}: Monday's week is empty"
        );
        assert_eq!(weekly[14].runs, 1, "offset {offset}");
    }
}

/// A session logged late in the evening west of UTC (the next UTC day) or
/// early in the morning at +14 h (the previous UTC day) completes its LOCAL
/// day, and the next day is untouched.
#[test]
fn work_near_local_midnight_completes_its_local_day_at_extreme_offsets() {
    let mon = ymd(2027, 6, 7);
    assert_eq!(monday_of(mon), mon);
    for (offset, hour) in [
        (-5 * HOUR, 22),
        (-12 * HOUR, 23),
        (14 * HOUR, 0),
        (14 * HOUR, 7),
    ] {
        let (app, mut model) = planned_strength_model_on(mon, true);
        set_today(&app, &mut model, mon, offset);
        let ns = app.view(&model).next_session.expect("today's session");
        assert_eq!(ns.epoch_day, mon);
        let mut id = 100;
        log_row(&app, &mut model, &ns, hour, offset, &mut id);
        let vm = app.view(&model);
        let ctx = format!("offset {offset}, {hour}:00");
        assert!(
            row(&vm, mon).status.is_done(),
            "{ctx}: {:?}",
            row(&vm, mon).status
        );
        assert!(!is_judged(row(&vm, mon + 1).status), "{ctx}");
        assert_eq!(vm.consistency.as_ref().unwrap().this_week_done, 1, "{ctx}");
        assert!(
            vm.next_session.unwrap().epoch_day > mon,
            "{ctx}: the hero moves on"
        );
        let done = vm.session_done.expect("today's session summary");
        assert_eq!(done.epoch_day, mon, "{ctx}");
        assert_calendar(&app.view(&model), mon, &ctx);
    }
}

/// A lifting session started late on its plan day and finished after local
/// midnight is one session: its plan day reads done (not "done lighter" or
/// partial), the next day is not credited with it, and no "came up short"
/// note is written about sets that were all done.
#[test]
fn a_lift_session_running_past_local_midnight_stays_one_session() {
    let mon = ymd(2027, 6, 7);
    for offset in [0, 2 * HOUR, -7 * HOUR] {
        let (app, mut model) = planned_strength_model_on(mon, true);
        set_today(&app, &mut model, mon, offset);
        let ns = app.view(&model).next_session.expect("today's session");
        let total_sets: u32 = ns.items.iter().map(|i| u32::from(i.sets)).sum();
        // Every set as prescribed, 4 minutes apart, from 23:20 local: about
        // half of them land after midnight.
        let mut t = local(mon, 23, 20, offset);
        for it in &ns.items {
            let kg = it.load_kg.or(it.suggested_load_kg).unwrap_or(40.0);
            for _ in 0..it.sets {
                let reps = u32::from(it.reps_high.max(1));
                send(
                    &app,
                    &mut model,
                    log_set(&it.exercise, kg, reps, it.target_rpe.unwrap_or(7.0), t),
                );
                t += 240;
            }
        }
        assert!(
            crate::local_day(t, offset) == mon + 1 && total_sets >= 4,
            "the session must cross midnight"
        );
        set_today(&app, &mut model, mon + 1, offset);
        let vm = app.view(&model);
        let ctx = format!("offset {offset}");
        assert_eq!(
            row(&vm, mon).status,
            SessionStatus::Done,
            "{ctx}: {:?}",
            row(&vm, mon).items
        );
        assert_eq!(vm.consistency.as_ref().unwrap().this_week_done, 1, "{ctx}");
        if let Some(f) = &vm.feedback {
            assert!(!f.message.contains("Came up short"), "{ctx}: {}", f.message);
        }
    }
}

/// An 18th birthday on a Thursday: Monday–Wednesday were held by the
/// under-18 gate, so the strip must not call them missed and no "Missed
/// Monday's …" nudge may ask to make up a session the athlete was not
/// allowed to train (as for any other lifted gate, see the PAR-Q twin below).
#[test]
#[ignore = "BUG: after an 18th birthday, days held by the under-18 gate read Missed and get a make-up nudge (refresh_birthday_ages rewrites past profiles)"]
fn an_18th_birthday_mid_week_does_not_turn_held_days_into_missed_ones() {
    let mon = ymd(2027, 6, 7);
    let thu = mon + 3;
    let born = ymd(2009, 6, 10);
    assert_eq!(civil_from_days(thu), (2027, 6, 10));
    let app = Engine;
    let mut model = Model::default();
    send(
        &app,
        &mut model,
        Event::SetProfile(Profile {
            birth_epoch_day: Some(born),
            effective_from_epoch_day: Some(mon - 14),
            ..strength_profile()
        }),
    );
    for d in mon - 14..thu {
        set_today(&app, &mut model, d, 0);
        if d == mon - 14 {
            send(&app, &mut model, Event::GeneratePlan { start_epoch_day: d });
        }
        let vm = app.view(&model);
        assert_eq!(vm.hold_reason, Some(HoldReason::Youth), "day {d}");
    }
    set_today(&app, &mut model, thu, 0);
    let vm = app.view(&model);
    assert!(!vm.train_blocked, "18 today: {:?}", vm.hold_reason);
    assert_calendar(&vm, thu, "18th birthday");
    for d in mon..thu {
        let r = row(&vm, d);
        assert_ne!(
            r.status,
            SessionStatus::Missed,
            "held day {d}: {:?}",
            r.title
        );
    }
    // (Repeating the plan week after held weeks is the conservative,
    // factual ADH-PROGRESS-001 call; only the make-up nudge is wrong.)
    for n in &vm.consistency.as_ref().expect("a plan").notes {
        assert!(!n.summary.starts_with("Missed"), "{}", n.summary);
    }
}

/// The same mid-week lift of a profile health gate (a PAR-Q answer cleared
/// by a profile edit effective Thursday): the held days keep their own
/// day's profile, so they are not missed and no make-up is nudged.
#[test]
fn a_health_gate_lifted_mid_week_does_not_turn_held_days_into_missed_ones() {
    let mon = ymd(2027, 6, 7);
    let thu = mon + 3;
    let app = Engine;
    let mut model = Model::default();
    let gated = Profile {
        health: HealthScreen {
            parq_positive: true,
            ..HealthScreen::default()
        },
        effective_from_epoch_day: Some(mon - 14),
        ..strength_profile()
    };
    send(&app, &mut model, Event::SetProfile(gated));
    for d in mon - 14..thu {
        set_today(&app, &mut model, d, 0);
        if d == mon - 14 {
            send(&app, &mut model, Event::GeneratePlan { start_epoch_day: d });
        }
        assert!(app.view(&model).train_blocked, "day {d}");
    }
    set_today(&app, &mut model, thu, 0);
    send(
        &app,
        &mut model,
        Event::SetProfile(Profile {
            effective_from_epoch_day: Some(thu),
            ..strength_profile()
        }),
    );
    let vm = app.view(&model);
    assert!(!vm.train_blocked, "{:?}", vm.hold_reason);
    assert_calendar(&vm, thu, "gate lifted");
    for d in mon..thu {
        assert_ne!(row(&vm, d).status, SessionStatus::Missed, "held day {d}");
    }
    for n in &vm.consistency.as_ref().unwrap().notes {
        assert!(!n.summary.starts_with("Missed"), "{}", n.summary);
    }
}

/// "Came up short on X today" only on the day it was logged; the next day it
/// names the weekday, even when the sets sit on the next UTC day.
#[test]
fn a_short_lift_says_today_only_on_its_local_day() {
    let mon = ymd(2027, 6, 7);
    for offset in [0, -5 * HOUR, 10 * HOUR] {
        let (app, mut model) = planned_strength_model_on(mon, true);
        set_today(&app, &mut model, mon, offset);
        let vm = app.view(&model);
        let ns = vm.next_session.unwrap();
        let it = ns
            .items
            .iter()
            .find(|i| i.load_kg.is_some() && i.sets >= 2)
            .expect("an anchored multi-set lift")
            .clone();
        // Half the sets of one lift (late evening, ended early), every other
        // item in full: the day completes "done lighter" today.
        let mut t = local(mon, 21, 0, offset);
        for _ in 0..(it.sets / 2) {
            send(
                &app,
                &mut model,
                log_set(
                    &it.exercise,
                    it.load_kg.unwrap(),
                    u32::from(it.reps_low),
                    8.0,
                    t,
                ),
            );
            t += 120;
        }
        send(
            &app,
            &mut model,
            Event::SetExerciseEnded {
                exercise: it.exercise.clone(),
                epoch_day: mon,
                ended: true,
                observed_at: t,
            },
        );
        for other in ns
            .items
            .iter()
            .filter(|o| o.exercise != it.exercise && !o.is_run())
        {
            let kg = other.load_kg.or(other.suggested_load_kg).unwrap_or(40.0);
            for _ in 0..other.sets {
                send(
                    &app,
                    &mut model,
                    log_set(
                        &other.exercise,
                        kg,
                        u32::from(other.reps_high.max(1)),
                        7.0,
                        t,
                    ),
                );
                t += 120;
            }
        }
        let msg = app
            .view(&model)
            .feedback
            .map(|f| f.message)
            .unwrap_or_default();
        let ctx = format!("offset {offset}");
        assert!(
            msg.contains(&format!("Came up short on {} today", it.exercise)),
            "{ctx}: {msg}"
        );
        set_today(&app, &mut model, mon + 1, offset);
        let msg = app
            .view(&model)
            .feedback
            .map(|f| f.message)
            .unwrap_or_default();
        assert!(
            msg.contains(&format!("Came up short on {} on Monday", it.exercise)),
            "{ctx}: {msg}"
        );
        assert!(!msg.contains("today"), "{ctx}: {msg}");
    }
}

/// An offset change alone (DST, travel) with the same local day does not move
/// entries logged away from midnight: the strip, counts and buckets match.
/// (Entries within the shift of midnight are a documented limitation: past
/// entries are bucketed with today's offset, `planning/BUGS.md` 2026-08-05.)
#[test]
fn an_offset_change_does_not_move_entries_logged_mid_day() {
    let start = ymd(2027, 3, 15);
    let dst_day = ymd(2027, 3, 28);
    let (app, mut model) = {
        let app = Engine;
        let mut model = Model::default();
        send(
            &app,
            &mut model,
            Event::SetProfile(Profile {
                effective_from_epoch_day: Some(start - 7),
                ..sample_profile()
            }),
        );
        (app, model)
    };
    send(
        &app,
        &mut model,
        Event::GeneratePlan {
            start_epoch_day: start,
        },
    );
    let mut id = 0;
    for d in start..dst_day {
        set_today(&app, &mut model, d, HOUR);
        let vm = app.view(&model);
        if let Some(ns) = vm.next_session.filter(|s| s.epoch_day == d && d % 4 != 0) {
            log_row(&app, &mut model, &ns, 13, HOUR, &mut id);
        }
    }
    let key = |vm: &ViewModel| {
        (
            vm.week_plan
                .iter()
                .map(|s| (s.epoch_day, s.status))
                .collect::<Vec<_>>(),
            vm.consistency.as_ref().map(|c| {
                (
                    c.this_week_done,
                    c.this_week_planned,
                    c.weeks.clone(),
                    c.share_pct,
                )
            }),
            vm.run_progress.as_ref().map(|r| {
                r.weekly
                    .iter()
                    .map(|w| (w.week_start_epoch_day, w.runs))
                    .collect::<Vec<_>>()
            }),
            vm.program.as_ref().map(|p| (p.week, p.weeks_total)),
        )
    };
    for d in [dst_day - 1, dst_day] {
        set_today(&app, &mut model, d, HOUR);
        let winter = key(&app.view(&model));
        set_today(&app, &mut model, d, 2 * HOUR);
        let summer = key(&app.view(&model));
        assert_eq!(winter, summer, "day {d}");
        assert_calendar(&app.view(&model), d, "after DST");
    }
}

/// The clock moving backwards (a `SetToday` before the latest log) never
/// judges a future day done, never counts it this week and never says
/// "today" about it; moving forward again restores everything.
#[test]
fn a_clock_moving_backwards_never_judges_future_days() {
    let mon = ymd(2027, 6, 7);
    let (app, mut model) = planned_strength_model_on(mon, true);
    let mut id = 0;
    let wed;
    {
        let vm = app.view(&model);
        let lifts: Vec<SessionPlanView> = vm
            .week_plan
            .iter()
            .filter(|s| matches!(s.session_type, SessionType::Lift(_)))
            .cloned()
            .collect();
        assert!(lifts.len() >= 2);
        wed = lifts[1].epoch_day;
    }
    for d in [mon, wed] {
        set_today(&app, &mut model, d, 0);
        let r = row(&app.view(&model), d);
        log_row(&app, &mut model, &r, 17, 0, &mut id);
    }
    let forward = app.view(&model);
    assert!(row(&forward, wed).status.is_done());
    assert_eq!(forward.consistency.as_ref().unwrap().this_week_done, 2);

    set_today(&app, &mut model, mon, 0);
    let back = app.view(&model);
    assert_calendar(&back, mon, "clock back to Monday");
    assert!(
        !is_judged(row(&back, wed).status),
        "{:?}",
        row(&back, wed).status
    );
    assert_eq!(back.consistency.as_ref().unwrap().this_week_done, 1);
    if let Some(f) = &back.feedback {
        assert!(
            !f.message
                .contains(&format!("on {}", crate::app::plan_views::weekday_name(wed))),
            "{}",
            f.message
        );
    }

    set_today(&app, &mut model, wed, 0);
    let again = app.view(&model);
    assert_eq!(key_statuses(&again), key_statuses(&forward), "no data lost");
}

fn key_statuses(vm: &ViewModel) -> Vec<(i64, SessionStatus)> {
    vm.week_plan
        .iter()
        .map(|s| (s.epoch_day, s.status))
        .collect()
}

/// A check-in or a session dated tomorrow (a wrong phone clock, a typo in a
/// backdate) is not today's: no "checked in today", today's session not done.
#[test]
fn entries_dated_in_the_future_do_not_count_as_today() {
    let mon = ymd(2027, 6, 7);
    let (app, mut model) = planned_strength_model_on(mon, true);
    let today_row = app.view(&model).next_session.unwrap();
    send(
        &app,
        &mut model,
        Event::SubmitCheckin(crate::schema::CheckinInput {
            observed_at: local(mon + 1, 7, 0, 0),
            sleep_quality: Some(4),
            soreness: Some(2),
            mood: Some(4),
            fatigue: Some(2),
            stress: Some(2),
            ..Default::default()
        }),
    );
    let mut tomorrow = today_row.clone();
    tomorrow.epoch_day = mon + 1;
    let mut id = 0;
    log_row(&app, &mut model, &tomorrow, 9, 0, &mut id);
    let vm = app.view(&model);
    assert!(!vm.checked_in_today, "a check-in dated tomorrow");
    assert_eq!(
        row(&vm, mon).status,
        SessionStatus::Next,
        "today's session is still open"
    );
    assert!(vm.session_done.is_none());
    assert_eq!(vm.consistency.as_ref().unwrap().this_week_done, 0);
    assert_calendar(&vm, mon, "future-dated entries");
    if let Some(f) = &vm.feedback {
        assert!(!f.message.contains("today"), "{}", f.message);
    }
}

/// Backdating a session into a finished plan week raises that week's done
/// count only, and can lift the "repeating this week" note.
#[test]
fn backdating_into_a_past_plan_week_updates_that_week_only() {
    let mon = ymd(2027, 6, 7);
    let (app, mut model) = planned_strength_model_on(mon, true);
    let lift_rows: Vec<SessionPlanView> = app
        .view(&model)
        .week_plan
        .into_iter()
        .filter(|s| !s.items.is_empty() && s.session_type != SessionType::Rest)
        .collect();
    // Week 2 done in full; week 1 skipped.
    let mut id = 0;
    for r in &lift_rows {
        let mut r2 = r.clone();
        r2.epoch_day += 7;
        set_today(&app, &mut model, r2.epoch_day, 0);
        let live = row(&app.view(&model), r2.epoch_day);
        log_row(&app, &mut model, &live, 17, 0, &mut id);
    }
    let today = mon + 14 + 2;
    set_today(&app, &mut model, today, 0);
    let before = app.view(&model).consistency.unwrap();
    assert_eq!(before.weeks.len(), 2);
    assert_eq!(before.weeks[0].done, 0);
    let w2 = before.weeks[1].clone();
    assert_eq!(w2.done, w2.planned, "{w2:?}");

    // Backdate every week-1 session (logged now, dated then).
    for r in &lift_rows {
        log_row(&app, &mut model, r, 18, 0, &mut id);
    }
    let vm = app.view(&model);
    assert_calendar(&vm, today, "after backdating");
    let after = vm.consistency.unwrap();
    assert_eq!(
        after.weeks[0].done, after.weeks[0].planned,
        "{:?}",
        after.weeks
    );
    assert_eq!(after.weeks[1], w2, "the other week is untouched");
    assert_eq!(
        (after.this_week_done, after.this_week_planned),
        (before.this_week_done, before.this_week_planned)
    );
    assert_eq!(after.share_pct, Some(100));
}

/// A plan started on Thursday 31 December 2026 (mid-week, in ISO week 53):
/// Mon–Wed are before the plan, the partial week finishes into the first
/// record but never into the done share.
#[test]
fn a_mid_week_plan_start_in_iso_week_53() {
    let thu = ymd(2026, 12, 31);
    let app = Engine;
    let mut model = Model::default();
    send(&app, &mut model, Event::SetProfile(strength_profile()));
    send(
        &app,
        &mut model,
        Event::GeneratePlan {
            start_epoch_day: thu,
        },
    );
    set_today(&app, &mut model, thu, 0);
    let vm = app.view(&model);
    assert_calendar(&vm, thu, "plan day 1");
    for d in thu - 3..thu {
        assert_eq!(row(&vm, d).status, SessionStatus::BeforePlan, "day {d}");
    }
    let mut id = 0;
    for d in thu..thu + 4 {
        set_today(&app, &mut model, d, 0);
        let vm = app.view(&model);
        if let Some(ns) = vm.next_session.filter(|s| s.epoch_day == d) {
            log_row(&app, &mut model, &ns, 17, 0, &mut id);
        }
    }
    let next_mon = ymd(2027, 1, 4);
    set_today(&app, &mut model, next_mon, 0);
    let vm = app.view(&model);
    assert_calendar(&vm, next_mon, "first full week");
    assert!(
        vm.week_plan
            .iter()
            .all(|s| s.status != SessionStatus::BeforePlan)
    );
    let c = vm.consistency.unwrap();
    assert_eq!(c.weeks.len(), 1);
    assert_eq!(c.weeks[0].week_monday_epoch_day, ymd(2026, 12, 28));
    assert_eq!(
        c.share_pct, None,
        "a partial first week never counts toward the share"
    );
    assert!(!c.repeating_week, "a partial first week always advances");
}

/// A race today, earlier this week, or weeks ago never yields a program week
/// outside 1..=total, a negative count in copy, or a missing week strip.
#[test]
fn a_race_today_or_in_the_past_keeps_the_plan_sane() {
    let today = ymd(2027, 5, 12);
    for (label, race) in [
        ("today", today),
        ("this Monday", monday_of(today)),
        ("last week", today - 7),
        ("ten weeks ago", today - 70),
        ("tomorrow", today + 1),
    ] {
        for plan_start in [today - 21, today] {
            let app = Engine;
            let mut model = Model::default();
            send(
                &app,
                &mut model,
                Event::SetProfile(Profile {
                    goal_race_epoch_day: Some(race),
                    ..running_profile()
                }),
            );
            send(
                &app,
                &mut model,
                Event::GeneratePlan {
                    start_epoch_day: plan_start,
                },
            );
            set_today(&app, &mut model, today, 0);
            let vm = app.view(&model);
            let ctx = format!("race {label}, plan from {plan_start}");
            assert_calendar(&vm, today, &ctx);
            assert_eq!(vm.week_plan.len(), 7, "{ctx}");
            let p = vm.program.as_ref().expect("a program card");
            if race >= monday_of(today) {
                assert_eq!(p.week, p.weeks_total, "{ctx}: race week is the last week");
            }
            let json = serde_json::to_string(&vm).unwrap();
            for bad in ["-1 week", "−1 week", "0 weeks", "in -", "in −"] {
                assert!(!json.contains(bad), "{ctx}: `{bad}` in the view");
            }
        }
    }
}
