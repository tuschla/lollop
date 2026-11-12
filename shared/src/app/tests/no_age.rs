//! Age is optional (KB onboard-050 note): a profile with no age is an adult.
//! It plans normally, has no youth hold, no masters adjustments and no Tanaka
//! HRmax, so run targets fall back to a measured HRmax or to the effort cues.

use super::*;

fn planned(profile: Profile) -> (Engine, Model) {
    planned_on(profile, [], MON)
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
    let vm = app.view(&model);
    let p = vm.profile.as_ref().unwrap();
    assert!(!p.health.youth, "no age is not youth");
    assert_eq!(
        p.age_years, None,
        "SetToday derives no age without a birthday"
    );
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
    let targets: Vec<&RunTargetView> = vm
        .week_plan
        .iter()
        .flat_map(|s| s.items.iter())
        .filter_map(|it| it.run_target.as_ref())
        .collect();
    assert!(!targets.is_empty(), "the running week has run targets");
    for t in &targets {
        assert!(t.hr_label.is_empty(), "no HRmax, no bpm: {t:?}");
        assert!(!t.effort_cue.is_empty(), "the effort cue stays: {t:?}");
    }
    let long = planned_long_run_item(&model);
    let t = long.run_target.as_ref().expect("the long run has a target");
    assert!(t.effort_cue.contains("talk in full sentences"), "{t:?}");
}

#[test]
fn no_age_run_targets_use_a_measured_hrmax() {
    // Without an age only a measured HRmax yields a bpm band
    // (`no_age_run_targets_carry_no_age_estimated_heart_rate`), and the band
    // scales with the measured value.
    let ceiling = |hr_max: f64| -> u32 {
        let (_, model) = planned(Profile {
            measured_hr_max: Some(hr_max),
            ..no_age(running_profile())
        });
        let long = planned_long_run_item(&model);
        let t = long.run_target.expect("the long run has a target");
        t.hr_label
            .strip_suffix(" bpm")
            .and_then(|s| s.rsplit(['–', ' ']).next())
            .and_then(|h| h.parse().ok())
            .unwrap_or_else(|| panic!("a bpm ceiling: {t:?}"))
    };
    let (high, lower) = (ceiling(190.0), ceiling(171.0));
    assert!(high > 0 && high <= 190, "{high}");
    assert!(
        (f64::from(lower) - f64::from(high) * 0.9).abs() <= 1.0,
        "the band follows the measured HRmax: {high} bpm at 190, {lower} bpm at 171"
    );
}

#[test]
fn the_masters_protein_context_and_bpm_conversion_follow_the_stated_age() {
    let view = |profile: Option<Profile>| {
        let app = Engine;
        let mut model = Model::default();
        if let Some(p) = profile {
            app.update(Event::SetProfile(p), &mut model)
                .expect_only_render();
        }
        let vm = app.view(&model);
        (vm.protein_masters_offered, vm.hr_max_known)
    };
    let aged = |age_years, measured_hr_max| {
        Some(Profile {
            age_years,
            measured_hr_max,
            ..no_age(strength_profile())
        })
    };
    // MASTERS-001: the masters target is offered unless a stated age is under
    // 65; an unknown age leaves both protein contexts open.
    assert_eq!(view(None), (true, false), "no profile");
    assert_eq!(view(aged(None, None)), (true, false), "no age");
    assert_eq!(view(aged(Some(64.0), None)), (false, true));
    assert_eq!(view(aged(Some(65.0), None)), (true, true));
    // The bpm → %HRmax anchor: a measured max, else an age Tanaka covers.
    assert_eq!(
        view(aged(None, Some(190.0))),
        (true, true),
        "a measured max"
    );
    assert_eq!(
        view(aged(None, Some(250.0))),
        (true, false),
        "an implausible max"
    );
    assert!(!view(aged(Some(101.0), None)).1, "past Tanaka's ages");
}
