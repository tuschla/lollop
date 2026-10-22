//! The under-18 WHO weekly activity card (YOUTH-PA-001): what counts, which
//! week, and who sees it.

use super::*;

/// A Monday epoch day (mon0_weekday == 0); today is its Wednesday.
const MONDAY: i64 = MON + 7 * 2_900;
const TODAY: i64 = MONDAY + 2;
const HOUR: i64 = 3_600;

fn at(day: i64, hour: i64, min: i64) -> i64 {
    day * DAY_SEC + hour * HOUR + min * 60
}

fn youth() -> Profile {
    Profile {
        age_years: Some(15.0),
        ..strength_profile()
    }
}

fn run(minutes: f64, rating: Option<u8>, observed_at: i64) -> Event {
    Event::LogRun {
        distance_km: minutes / 6.0,
        duration_min: minutes,
        hr_pct_max: 0.0,
        longest_recent_km: 50.0,
        observed_at,
        entry_id: 0,
        workout_type: None,
        session_rpe: rating,
        avg_hr_bpm: None,
    }
}

fn activity(
    kind: ActivityKind,
    minutes: f64,
    vigorous: bool,
    strengthening: bool,
    observed_at: i64,
    entry_id: u64,
) -> Event {
    Event::LogActivity {
        kind,
        minutes,
        vigorous,
        strengthening,
        observed_at,
        entry_id,
    }
}

fn replay(events: Vec<Event>) -> (Engine, Model) {
    let app = Engine;
    let mut model = Model::default();
    for ev in events {
        app.update(ev, &mut model).expect_only_render();
    }
    (app, model)
}

fn week_log(profile: Profile) -> Vec<Event> {
    vec![
        Event::SetProfile(profile),
        // Mon: an unrated 30-min run (bone-strengthening, not vigorous).
        run(30.0, None, at(MONDAY, 7, 0)),
        // Tue: a 40-min run rated 8/10 (vigorous + strengthening).
        run(40.0, Some(8), at(MONDAY + 1, 17, 0)),
        // Wed: three sets spanning 17:00-17:45 (45 lift minutes).
        log_set("Back Squat", 60.0, 5, 7.0, at(MONDAY + 2, 17, 0)),
        log_set("Back Squat", 60.0, 5, 7.0, at(MONDAY + 2, 17, 20)),
        log_set("Back Squat", 60.0, 5, 7.0, at(MONDAY + 2, 17, 45)),
        // Thu: one set: a strengthening day with no measurable time.
        log_set("Push-up", 0.0, 10, 7.0, at(MONDAY + 3, 18, 0)),
        // Sat: 45 min of vigorous sport.
        activity(
            ActivityKind::Sport,
            45.0,
            true,
            false,
            at(MONDAY + 5, 10, 0),
            11,
        ),
        // Sun: 30.4 min of easy cycling.
        activity(
            ActivityKind::Cycling,
            30.4,
            false,
            false,
            at(MONDAY + 6, 15, 0),
            12,
        ),
        // Last Sunday and next Monday lie outside this week.
        activity(
            ActivityKind::Dance,
            60.0,
            true,
            true,
            at(MONDAY - 1, 12, 0),
            13,
        ),
        run(50.0, Some(9), at(MONDAY + 7, 7, 0)),
        // Undated: listed, never bucketed into a week.
        activity(ActivityKind::Walking, 20.0, true, true, 0, 14),
        Event::SetToday {
            epoch_day: TODAY,
            utc_offset_sec: 0,
        },
    ]
}

#[test]
fn the_card_counts_runs_lift_spans_and_activities_within_the_week() {
    let (app, model) = replay(week_log(youth()));
    let vm = app.view(&model);
    assert!(vm.train_blocked, "the youth hold is unchanged");
    assert_eq!(vm.hold_reason, Some(HoldReason::Youth));
    let y = vm
        .youth_activity
        .expect("an under-18 profile gets the card");
    assert_eq!(y.week_monday_epoch_day, MONDAY);
    assert_eq!(y.run_minutes, 70);
    assert_eq!(y.lift_minutes, 45);
    assert_eq!(y.activity_minutes, 75, "45 + 30.4, rounded");
    assert_eq!(y.minutes, 190);
    assert_eq!(y.daily_target_minutes, 60);
    assert_eq!(y.weekly_target_minutes, 420);
    assert_eq!(y.target_days, 3);
    assert_eq!(y.vigorous_days, 2, "Tue (run rated 8) + Sat (sport)");
    assert_eq!(y.strengthening_days, 4, "Mon + Tue runs, Wed + Thu sets");
    let ids: Vec<u64> = y.week_entries.iter().map(|a| a.entry_id).collect();
    assert_eq!(ids, vec![11, 12], "this week's activities, chronological");
    let targets = individualization::youth_activity_targets();
    assert_eq!(y.evidence, EvidenceTagView::of(&targets));
    assert_eq!(y.evidence.grade, Some(EvidenceGrade::Strong));
    assert!(!y.evidence.safety_critical);
    assert_eq!(y.why.basis, targets.evidence().statement);
    assert!(!y.why.grade_note.is_empty() && y.why.improves != "-");
    assert_eq!(
        vm.activities.len(),
        4,
        "History lists every logged activity"
    );
}

#[test]
fn a_rating_below_seven_is_not_a_vigorous_day() {
    let (app, model) = replay(vec![
        Event::SetProfile(youth()),
        run(30.0, Some(6), at(MONDAY, 7, 0)),
        Event::SetToday {
            epoch_day: TODAY,
            utc_offset_sec: 0,
        },
    ]);
    let y = app.view(&model).youth_activity.unwrap();
    assert_eq!((y.vigorous_days, y.strengthening_days), (0, 1));
}

#[test]
fn lift_minutes_cap_at_three_hours_a_day() {
    let (app, model) = replay(vec![
        Event::SetProfile(youth()),
        log_set("Back Squat", 60.0, 5, 7.0, at(MONDAY, 8, 0)),
        log_set("Back Squat", 60.0, 5, 7.0, at(MONDAY, 20, 0)),
        Event::SetToday {
            epoch_day: TODAY,
            utc_offset_sec: 0,
        },
    ]);
    let y = app.view(&model).youth_activity.unwrap();
    assert_eq!(y.lift_minutes, 180);
}

#[test]
fn days_are_local_days() {
    // 23:00 UTC on last Sunday is 01:00 Monday at UTC+2: this week.
    let (app, model) = replay(vec![
        Event::SetProfile(youth()),
        activity(
            ActivityKind::Swimming,
            30.0,
            true,
            false,
            at(MONDAY, -1, 0),
            1,
        ),
        Event::SetToday {
            epoch_day: TODAY,
            utc_offset_sec: 2 * HOUR,
        },
    ]);
    let y = app.view(&model).youth_activity.unwrap();
    assert_eq!((y.activity_minutes, y.vigorous_days), (30, 1));
}

#[test]
fn no_card_for_adults_no_age_or_without_today() {
    let adult = Profile {
        age_years: Some(30.0),
        ..strength_profile()
    };
    let (app, model) = replay(week_log(adult));
    let vm = app.view(&model);
    assert_eq!(vm.youth_activity, None);
    assert_eq!(vm.activities.len(), 4, "adults still log activities");

    let (app, model) = replay(week_log(strength_profile()));
    assert_eq!(app.view(&model).youth_activity, None, "no age is an adult");

    let mut log = week_log(youth());
    log.pop();
    let (app, model) = replay(log);
    assert_eq!(app.view(&model).youth_activity, None, "no today, no week");
}

#[test]
fn the_card_follows_a_birthday_out_of_the_youth_band() {
    // Born 2008-10-18: 17 on 2026-10-17, 18 the next day.
    const BORN: i64 = 14_170;
    const D_2026_10_17: i64 = 20_743;
    let (app, mut model) = replay(vec![
        Event::SetProfile(Profile {
            birth_epoch_day: Some(BORN),
            ..strength_profile()
        }),
        Event::SetToday {
            epoch_day: D_2026_10_17,
            utc_offset_sec: 0,
        },
    ]);
    assert!(app.view(&model).youth_activity.is_some());
    app.update(
        Event::SetToday {
            epoch_day: D_2026_10_17 + 1,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    assert_eq!(app.view(&model).youth_activity, None);
}
