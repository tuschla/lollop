//! The gates a profile still being edited raises before it is sent
//! (`profile_age::draft_facts`, the shell's `DraftFacts` op): the PAR-Q+
//! answers and the clearance that covers them (safety-044, onboard-052), the
//! health deferral gates, the age on a day and the pediatric gate
//! (SAFE-PEDS-001), and the equipment a pull-up rung needs. A sent profile
//! meets the same rules: the core owns `health.youth`.

use super::*;
use crate::app::profile_age::draft_facts;
use crate::envelope::{DraftFacts, ProfileDraft};
use crate::profile::CalisEquipment;
use crate::schema::{HealthAnswer, ParqQuestion};

use ParqQuestion::{
    BoneOrJoint, ChestPain, DizzyOrFainting, HeartOrBloodPressure, Medication, OtherCondition,
    SupervisedOnly,
};

fn answer(h: &HealthScreen, question: ParqQuestion, yes: bool) -> HealthScreen {
    h.answered(HealthAnswer::Parq { question, yes })
}

fn clearance(h: &HealthScreen, cleared: bool) -> HealthScreen {
    h.answered(HealthAnswer::Clearance { cleared })
}

fn facts(health: &HealthScreen) -> DraftFacts {
    draft_facts(ProfileDraft {
        health: health.clone(),
        ..ProfileDraft::default()
    })
}

fn defers(health: &HealthScreen) -> bool {
    facts(health).health_defers
}

fn aged(age_years: Option<f64>, birth: Option<i64>, today: Option<i64>) -> DraftFacts {
    draft_facts(ProfileDraft {
        age_years,
        birth_epoch_day: birth,
        today,
        ..ProfileDraft::default()
    })
}

const BORN_1991_03_12: i64 = 7_740;
const BORN_2008_02_29: i64 = 13_938;
const BORN_2008_10_18: i64 = 14_170;
const D_2026_02_28: i64 = 20_512;
const D_2026_03_01: i64 = 20_513;
const D_2026_03_11: i64 = 20_523;
const D_2026_03_12: i64 = 20_524;
const D_2026_10_17: i64 = 20_743;

#[test]
fn a_new_follow_up_yes_after_clearance_drops_the_clearance() {
    let cleared = clearance(
        &answer(&HealthScreen::default(), HeartOrBloodPressure, true),
        true,
    );
    assert!(cleared.medically_cleared);
    assert_eq!(cleared.cleared_parq_mask, 1);
    assert!(!defers(&cleared));

    let added = answer(&cleared, Medication, true);
    assert!(added.parq_positive);
    assert!(!added.medically_cleared);
    assert_eq!(added.cleared_parq_mask, 0);
    assert!(defers(&added));
}

#[test]
fn answering_a_covered_follow_up_no_keeps_the_clearance() {
    let both = answer(
        &answer(&HealthScreen::default(), HeartOrBloodPressure, true),
        Medication,
        true,
    );
    let fewer = answer(&clearance(&both, true), Medication, false);
    assert!(fewer.medically_cleared);
    assert_eq!(fewer.cleared_parq_mask, 1);

    // A symptom is no follow-up: the clearance stays, the symptom holds.
    let symptom = answer(&fewer, ChestPain, true);
    assert!(symptom.medically_cleared);
    assert!(symptom.symptomatic);
    assert!(defers(&symptom));
}

#[test]
fn a_clearance_saved_before_the_mask_still_drops_on_a_new_answer() {
    let old = HealthScreen {
        medically_cleared: true,
        ..answer(&HealthScreen::default(), OtherCondition, true)
    };
    assert_eq!(old.cleared_parq_mask, 0);
    assert!(!answer(&old, SupervisedOnly, true).medically_cleared);
    assert!(answer(&old, ChestPain, true).medically_cleared);
}

#[test]
fn a_legacy_symptom_hold_survives_an_unrelated_follow_up_toggle() {
    let legacy = HealthScreen {
        symptomatic: true,
        ..HealthScreen::default()
    };
    let yes = answer(&legacy, HeartOrBloodPressure, true);
    assert!(yes.symptomatic, "a follow-up Yes is not an answer to Q2/Q3");
    let back_to_no = answer(&yes, HeartOrBloodPressure, false);
    assert!(back_to_no.symptomatic, "the symptom hold is still raised");
    assert!(defers(&back_to_no));

    let chest = answer(&back_to_no, ChestPain, true);
    assert!(chest.symptomatic);
    assert!(
        !answer(&chest, ChestPain, false).symptomatic,
        "re-answering the symptom question replaces the legacy flag"
    );
}

#[test]
fn a_legacy_doctor_flag_survives_an_unrelated_no_with_a_legacy_injury() {
    let doctor = HealthScreen {
        parq_positive: true,
        ..HealthScreen::default()
    };
    let control = answer(&doctor, ChestPain, false);
    assert!(
        control.parq_positive,
        "without the injury flag the doctor flag stays"
    );

    let with_injury = answer(
        &HealthScreen {
            injury_or_rehab: true,
            ..doctor.clone()
        },
        ChestPain,
        false,
    );
    assert!(with_injury.injury_or_rehab, "the injury flag stays");
    assert!(
        with_injury.parq_positive,
        "the legacy doctor flag stays raised"
    );
    assert!(
        with_injury.legacy_parq(),
        "the legacy note still explains it"
    );

    assert!(
        answer(&control, DizzyOrFainting, true).parq_positive,
        "a symptom Yes is not a follow-up answer"
    );
    assert!(
        !answer(&with_injury, Medication, false).parq_positive,
        "answering a follow-up question replaces the legacy flag"
    );
}

#[test]
fn chest_pain_is_a_symptom_that_clearance_cannot_lift() {
    let chest = answer(&HealthScreen::default(), ChestPain, true);
    assert!(chest.symptomatic);
    assert!(!chest.parq_positive);
    assert_eq!(facts(&chest).parq_yes, [ChestPain]);
    assert!(defers(&chest));
    assert!(defers(&HealthScreen {
        medically_cleared: true,
        ..chest
    }));

    // Doctor follow-up items raise parq_positive, which clearance lifts.
    let heart = HealthScreen {
        medically_cleared: true,
        ..answer(&HealthScreen::default(), HeartOrBloodPressure, true)
    };
    assert!(heart.parq_positive);
    assert!(!defers(&heart));
    // Answering it No again drops the follow-up AND its clearance.
    let no = answer(&heart, HeartOrBloodPressure, false);
    assert!(!no.parq_positive);
    assert!(!no.medically_cleared);

    // The bone/joint question is the injury/rehab hold.
    let joint = answer(&HealthScreen::default(), BoneOrJoint, true);
    assert!(joint.injury_or_rehab);
    assert!(defers(&joint));

    // A legacy single-toggle answer stays raised until a follow-up question is
    // answered.
    let legacy = HealthScreen {
        parq_positive: true,
        ..HealthScreen::default()
    };
    assert!(facts(&legacy).legacy_parq);
    assert!(answer(&legacy, ChestPain, false).parq_positive);
}

#[test]
fn the_form_reads_the_answers_in_question_order() {
    let h = answer(
        &answer(&HealthScreen::default(), DizzyOrFainting, true),
        ChestPain,
        true,
    );
    let f = facts(&h);
    assert_eq!(f.parq_yes, [ChestPain, DizzyOrFainting]);
    assert_eq!(f.first_symptom, Some(ChestPain));
    assert!(!f.legacy_parq);
    // The injury flag predates the questions and answers Q6.
    let injured = HealthScreen {
        injury_or_rehab: true,
        ..HealthScreen::default()
    };
    let f = facts(&injured);
    assert_eq!(f.parq_yes, [BoneOrJoint]);
    assert_eq!(f.first_symptom, None);
    // The op applies the answer just given before it reads the form.
    let given = draft_facts(ProfileDraft {
        answer: Some(HealthAnswer::Parq {
            question: Medication,
            yes: true,
        }),
        ..ProfileDraft::default()
    });
    assert_eq!(given.parq_yes, [Medication]);
    assert!(given.health.parq_positive);
    assert!(given.health_defers);
}

#[test]
fn each_health_answer_defers_and_a_cleared_doctor_flag_does_not() {
    assert!(!defers(&HealthScreen::default()));
    let flag = |f: fn(&mut HealthScreen)| {
        let mut h = HealthScreen::default();
        f(&mut h);
        defers(&h)
    };
    assert!(flag(|h| h.parq_positive = true));
    assert!(flag(|h| h.symptomatic = true));
    assert!(flag(|h| h.acute_illness = true));
    assert!(flag(|h| h.pregnant = true));
    assert!(flag(|h| h.pregnancy_warning_sign = true));
    assert!(flag(|h| h.injury_or_rehab = true));
    assert!(flag(|h| h.reds_signal = true));
    assert!(!flag(|h| {
        h.parq_positive = true;
        h.medically_cleared = true;
    }));
    // Age is the youth gate, not a health answer.
    assert!(!flag(|h| h.youth = true));
}

#[test]
fn the_age_counts_completed_calendar_years() {
    let age = |birth, today| aged(None, Some(birth), Some(today)).age_years;
    assert_eq!(age(BORN_1991_03_12, D_2026_03_11), Some(34.0));
    assert_eq!(age(BORN_1991_03_12, D_2026_03_12), Some(35.0));
    // A 29 February birthday completes its year on 1 March in a common year.
    assert_eq!(age(BORN_2008_02_29, D_2026_02_28), Some(17.0));
    assert_eq!(age(BORN_2008_02_29, D_2026_03_01), Some(18.0));
    // A future birthday reads 0.
    assert_eq!(age(20_600, 20_500), Some(0.0));
}

#[test]
fn the_youth_gate_is_under_eighteen_and_about_age_alone() {
    let youth = |age| aged(age, None, None).youth;
    assert!(youth(Some(16.0)));
    assert!(youth(Some(17.0)));
    assert!(youth(Some(17.99)));
    assert!(!youth(Some(18.0)));
    assert!(!youth(None), "no age is an adult");
    // The health answers alone don't defer a 16-year-old: the hold is the age's.
    let f = aged(Some(16.0), None, None);
    assert!(f.youth && !f.health_defers);
}

#[test]
fn the_birthday_wins_over_a_stated_age() {
    let eve = aged(Some(30.0), Some(BORN_2008_10_18), Some(D_2026_10_17));
    assert_eq!(eve.age_years, Some(17.0));
    assert!(eve.youth);
    let birthday = aged(Some(30.0), Some(BORN_2008_10_18), Some(D_2026_10_17 + 1));
    assert_eq!(birthday.age_years, Some(18.0));
    assert!(!birthday.youth);
    // A legacy stated age stands until a birthday is picked.
    assert_eq!(
        aged(Some(30.0), None, Some(D_2026_10_17)).age_years,
        Some(30.0)
    );
    // Without today the birthday can't be counted: the stated age stands.
    assert_eq!(
        aged(Some(30.0), Some(BORN_2008_10_18), None).age_years,
        Some(30.0)
    );
}

#[test]
fn a_sent_youth_flag_never_outlives_its_age() {
    let app = Engine;
    let sent = |age_years, youth| {
        let mut model = Model::default();
        app.update(
            Event::SetProfile(Profile {
                age_years,
                health: HealthScreen {
                    youth,
                    ..HealthScreen::default()
                },
                ..strength_profile()
            }),
            &mut model,
        )
        .expect_only_render();
        model.profile().unwrap().health.youth
    };
    // The echo of a 17-year-old's profile, re-sent after the age changed.
    assert!(!sent(Some(18.0), true), "an adult age lifts the gate");
    assert!(!sent(None, true), "no age is an adult");
    assert!(
        sent(Some(17.0), false),
        "the age raises the gate on its own"
    );
}

#[test]
fn pull_up_rungs_need_something_to_hang_from() {
    let can_hang = |e: Option<CalisEquipment>| {
        draft_facts(ProfileDraft {
            calis_equipment: e,
            ..ProfileDraft::default()
        })
        .can_hang
    };
    let kit = |pull_up_bar, dip_bars, rings| {
        Some(CalisEquipment {
            pull_up_bar,
            dip_bars,
            rings,
        })
    };
    assert_eq!(can_hang(None), None, "no stated equipment");
    assert_eq!(can_hang(kit(false, false, false)), Some(false));
    assert_eq!(can_hang(kit(true, false, false)), Some(true));
    assert_eq!(can_hang(kit(false, false, true)), Some(true));
    assert_eq!(can_hang(kit(false, true, false)), Some(false));
}
