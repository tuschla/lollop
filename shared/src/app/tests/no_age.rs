//! Age is optional (owner decision 2026-10-17; KB onboard-050 note): a
//! profile with no age is an adult. It plans normally, has no youth hold, no
//! masters adjustments and no Tanaka HRmax, so run targets fall back to a
//! measured HRmax or to the effort cues.

use super::*;

fn planned(profile: Profile) -> (Engine, Model) {
    let app = Engine;
    let mut model = Model::default();
    for ev in [
        Event::SetProfile(profile),
        Event::GeneratePlan {
            start_epoch_day: MON,
        },
        Event::SetToday {
            epoch_day: MON,
            utc_offset_sec: 0,
        },
    ] {
        app.update(ev, &mut model).expect_only_render();
    }
    (app, model)
}

fn no_age(p: Profile) -> Profile {
    Profile {
        age_years: None,
        birth_epoch_day: None,
        ..p
    }
}

#[test]
fn a_profile_with_no_age_plans_like_an_adult() {
    let (app, model) = planned(no_age(strength_profile()));
    let p = model.profile.as_ref().unwrap();
    assert!(!p.health.youth, "no age is not youth");
    assert_eq!(
        p.age_years, None,
        "SetToday derives no age without a birthday"
    );
    let vm = app.view(&model);
    assert!(!vm.train_blocked);
    assert_eq!(vm.hold_reason, None);
    assert_eq!(vm.plan_withheld, None);
    assert!(vm.program.is_some(), "GeneratePlan builds a program");
    assert!(
        vm.week_plan.iter().any(|s| !s.items.is_empty()),
        "the week has sessions: {:?}",
        vm.week_plan
    );
    assert!(vm.next_session.is_some());
    assert_eq!(vm.youth_activity, None, "no youth card for an adult");
    for g in &vm.guidance {
        assert!(!g.summary.trim().is_empty(), "blank guidance row: {g:?}");
        assert!(!g.summary.contains("NaN"), "{}", g.summary);
    }
}

#[test]
fn no_age_is_never_masters() {
    let weeks = |age: Option<f64>| {
        let (app, model) = planned(Profile {
            age_years: age,
            ..running_profile()
        });
        app.view(&model)
            .program
            .expect("a running program")
            .weeks_total
    };
    assert_eq!(
        weeks(None),
        weeks(Some(40.0)),
        "no age plans like a 40-year-old"
    );
    assert_ne!(
        weeks(None),
        weeks(Some(70.0)),
        "the masters deload cadence needs a stated age"
    );
}

#[test]
fn no_age_run_targets_carry_no_age_estimated_heart_rate() {
    let (app, model) = planned(no_age(running_profile()));
    let vm = app.view(&model);
    let targets: Vec<RunTargetView> = vm
        .week_plan
        .iter()
        .flat_map(|s| s.items.iter())
        .filter_map(|it| it.run_target.clone())
        .collect();
    assert!(!targets.is_empty(), "the running week has run targets");
    for t in &targets {
        assert!(t.hr_label.is_empty(), "no HRmax, no bpm: {t:?}");
        assert_eq!((t.hr_low_bpm, t.hr_high_bpm), (0, 0), "{t:?}");
        assert_eq!(t.hr_max_bpm, 0.0, "{t:?}");
        assert!(
            !t.effort_cue.is_empty() || t.rpe_high > 0,
            "the effort cue stays: {t:?}"
        );
    }
    let long = planned_long_run_item(&app, &model);
    let t = long.run_target.expect("the long run has a target");
    assert!(t.effort_cue.contains("talk in full sentences"), "{t:?}");
}

#[test]
fn no_age_run_targets_use_a_measured_hrmax() {
    let (app, model) = planned(Profile {
        measured_hr_max: Some(190.0),
        ..no_age(running_profile())
    });
    let t = planned_long_run_item(&app, &model)
        .run_target
        .expect("the long run has a target");
    assert!(t.hr_max_measured, "{t:?}");
    assert_eq!(t.hr_max_bpm, 190.0);
    assert!(t.hr_high_bpm > 0 && t.hr_high_bpm <= 190, "{t:?}");
}
