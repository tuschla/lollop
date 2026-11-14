//! Strength coaching: e1RM anchor windowing/aliasing/detraining, the layoff
//! re-entry ramp, the e1RM-drop load cut, and the lift card's effort target,
//! plates and "last time" line.

use super::*;
use crate::plan::DerateCause;
use crate::schema::{LiftIntensity, LiftPrescription, Prescription};

fn set(exercise: &str, weight_kg: f64, reps: u32, rpe: f64, day: i64) -> LoggedSet {
    LoggedSet {
        entry_id: day as u64,
        ..logged_set(exercise, weight_kg, reps, rpe, day * DAY_SEC + 3_600)
    }
}

/// A Monday, a lift day of the strength plan, far enough out that every set
/// below keeps a positive (dated) timestamp.
const T: i64 = POS_MON;

/// A logged set: `(exercise, kg, reps, rpe, epoch day)`, stamped 01:00.
type DaySet<'a> = (&'a str, f64, u32, f64, i64);

/// The cards on `day` (a lift day) once `profile` is set, `sets` are logged
/// and a plan is generated from `day` (also today).
fn cards_on(profile: Profile, sets: &[DaySet], day: i64) -> Vec<PrescriptionView> {
    let history = sets
        .iter()
        .map(|&(ex, kg, reps, rpe, d)| log_set(ex, kg, reps, rpe, d * DAY_SEC + 3_600));
    let (app, model) = planned_on(profile, history, day);
    day_row(&app, &model, day).items
}

/// The card of `exercise`'s movement among `cards`.
fn card(cards: Vec<PrescriptionView>, exercise: &str) -> PrescriptionView {
    cards
        .into_iter()
        .find(|i| crate::exercise::same_movement(&i.exercise, exercise))
        .unwrap_or_else(|| panic!("a {exercise} card"))
}

/// The Back Squat card on `day` ([`cards_on`]).
fn squat_on(profile: Profile, sets: &[DaySet], day: i64) -> PrescriptionView {
    card(cards_on(profile, sets, day), "Back Squat")
}

/// The strength profile back on `returned` from a declared `weeks_off` layoff.
fn back_from_layoff(weeks_off: f64, returned: i64) -> Profile {
    Profile {
        weeks_off: Some(weeks_off),
        layoff_returned_epoch_day: Some(returned),
        ..strength_profile()
    }
}

#[test]
fn one_rep_set_e1rm_equals_load() {
    // A single is its own 1RM (no +3.3 % Epley inflation).
    let squat = squat_on(
        strength_profile(),
        &[("Back Squat", 140.0, 1, 0.0, T - 1)],
        T,
    );
    assert!(
        squat.anchored_on.starts_with("Estimated max 140 kg"),
        "{}",
        squat.anchored_on
    );
}

#[test]
fn anchor_tracks_recent_window_not_all_time_max() {
    // One optimistic old best must not pin every later load: the anchor is the
    // best of the last 3 sessions / 21 days. Weekly sessions, so no week-long
    // break puts the old best on a re-entry ramp (REENTRY-RAMP-001). 100 kg × 5
    // at RPE 8 is 7 reps of capacity: 123.3 kg; the 120 kg PR would be 148.
    let mut sets = vec![("Back Squat", 120.0, 5, 8.0, T - 50)];
    for back in (1..=43).step_by(7) {
        sets.push(("Back Squat", 100.0, 5, 8.0, T - back));
    }
    let squat = squat_on(strength_profile(), &sets, T);
    assert!(
        squat.anchored_on.starts_with("Estimated max 123.3 kg"),
        "anchor = recent window best, not the old PR: {}",
        squat.anchored_on
    );
}

#[test]
fn anchor_merges_same_movement_names() {
    // Two names of one movement make one anchor: the 70 kg × 5 @ RPE 8 best
    // (86.3 kg) logged as "Row (barbell)", not the later 60 kg session (74 kg)
    // under the plan's own "Cable row".
    let row = card(
        cards_on(
            strength_profile(),
            &[
                ("Row (barbell)", 70.0, 5, 8.0, T - 3),
                ("Cable row", 60.0, 5, 8.0, T - 1),
            ],
            T,
        ),
        "Cable row",
    );
    assert!(
        row.anchored_on.starts_with("Estimated max 86.3 kg"),
        "{}",
        row.anchored_on
    );
}

#[test]
fn unrated_compliant_sets_hold_the_anchor_instead_of_failing_it() {
    // An unrated set is a lower bound, not a set to failure. A
    // rated 100×5 @ RPE 8 anchors ~123 kg; later unrated working sets at the
    // prescribed ~85 % (3 reps) keep that anchor instead of dragging it to
    // the reps-as-failure estimate (~94 kg).
    let mut sets = vec![("Back Squat", 100.0, 5, 8.0, T - 29)];
    for back in [22, 15, 8, 1] {
        sets.push(("Back Squat", 105.0, 3, 0.0, T - back));
    }
    let squat = squat_on(strength_profile(), &sets, T);
    assert!(
        squat.anchored_on.starts_with("Estimated max 123.3 kg"),
        "{}",
        squat.anchored_on
    );
    assert!(
        !squat.why.basis.contains("scaled to"),
        "{}",
        squat.why.basis
    );
}

#[test]
fn speed_set_without_rpe_does_not_create_or_move_an_anchor() {
    // Unrated sets under 70 % of the anchor are speed work: four weekly speed
    // sessions after a rated 100 kg × 5 @ RPE 8 neither push that session out
    // of the anchor window (3 sessions / 21 days) nor become the estimate.
    let mut sets = vec![("Back Squat", 100.0, 5, 8.0, T - 30)];
    for back in [26, 19, 12, 5] {
        sets.push(("Back Squat", 60.0, 3, 0.0, T - back));
    }
    let squat = squat_on(strength_profile(), &sets, T);
    assert!(
        squat.anchored_on.starts_with("Estimated max 123.3 kg"),
        "{}",
        squat.anchored_on
    );
    assert!(
        !squat.why.basis.contains("scaled to"),
        "{}",
        squat.why.basis
    );
}

#[test]
fn lift_anchor_after_44_days_is_70pct_not_50pct() {
    // 44 days without the lift scales the working load to 70 %
    // (DETRAIN-STR-001), never a linear-to-zero 50 %: 85 % of 123.3 kg at
    // 70 % is 73.4 kg, 72.5 on the plate grid.
    let squat = squat_on(
        strength_profile(),
        &[("Back Squat", 100.0, 5, 8.0, T - 44)],
        T,
    );
    assert_eq!(squat.load_kg, Some(72.5));
    assert!(
        squat
            .why
            .basis
            .contains("scaled to 70% after about 6 weeks off this lift"),
        "{}",
        squat.why.basis
    );
    // Past 8 weeks the anchor is dropped (fresh, novice-style start).
    let gone = squat_on(
        strength_profile(),
        &[("Back Squat", 100.0, 5, 8.0, T - 60)],
        T,
    );
    assert_eq!(gone.load_kg, None);
    assert!(
        !gone.anchored_on.starts_with("Estimated max"),
        "{}",
        gone.anchored_on
    );
}

#[test]
fn declared_layoff_and_inactivity_do_not_stack() {
    // A declared 6 wk layoff returning today: the pre-layoff anchor takes the
    // re-entry fraction INSTEAD of the detraining credit (70 %, not 49 %).
    let squat = squat_on(
        back_from_layoff(6.0, T),
        &[("Back Squat", 100.0, 5, 8.0, T - 44)],
        T,
    );
    assert!(
        squat
            .why
            .basis
            .contains("scaled to 70% in week 1 of 4 back from your break"),
        "{}",
        squat.why.basis
    );
    assert_eq!(squat.load_kg, Some(72.5), "one derate, never stacked");
}

#[test]
fn a_derated_percent_label_states_the_share_of_the_estimate_the_load_is() {
    // A derate (44 days off; a >5 % e1RM drop) scales the load, and the label
    // shows that same share of the estimated max, reduced from the planned
    // percent, not the planned percent over the lighter load.
    let detrained = [("Back Squat", 100.0, 5, 8.0, T - 44)];
    let dropped = [
        ("Back Squat", 100.0, 5, 8.0, T - 15),
        ("Back Squat", 100.0, 5, 8.0, T - 8),
        ("Back Squat", 100.0, 5, 10.0, T - 1),
    ];
    for sets in [&detrained[..], &dropped[..]] {
        let squat = squat_on(strength_profile(), sets, T);
        assert!(squat.why.basis.contains("scaled to"), "{}", squat.why.basis);
        let load = squat.load_kg.expect("an anchored load");
        let e1: f64 = squat
            .anchored_on
            .strip_prefix("Estimated max ")
            .and_then(|s| s.split(' ').next())
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| panic!("{}", squat.anchored_on));
        let (shown, planned) = squat
            .intensity_label
            .split_once("% e1RM (reduced from ")
            .and_then(|(p, rest)| {
                let orig = rest.strip_suffix("%)")?.parse::<f64>().ok()?;
                Some((p.parse::<f64>().ok()?, orig))
            })
            .unwrap_or_else(|| panic!("a reduced percent: {}", squat.intensity_label));
        assert!(shown < planned, "{}", squat.intensity_label);
        // The load is the shown share (to a whole percent) floored onto the
        // plate grid.
        let share = 100.0 * load / e1;
        let step = 100.0 * crate::plates::load_grid_step_kg(load, false) / e1;
        assert!(
            (-0.5..=step + 0.5).contains(&(shown - share)),
            "{} on {load} kg of {e1} kg ({share:.1}%)",
            squat.intensity_label
        );
    }
}

#[test]
fn six_weeks_off_starts_near_70pct_and_ramps_to_full() {
    // 6 wk off → 70 % on the return day, ramping to 100 % over the 4-wk
    // window; the athlete trains the lift lightly meanwhile, the last time the
    // day before the ramp ends (a full week without any lifting after the
    // ramp would be a new break, REENTRY-001). A day is prescribed from the
    // history through the day before it.
    let mut sets = vec![("Back Squat", 100.0, 5, 8.0, T - 50)];
    for d in [0, 7, 14, 21, 27] {
        sets.push(("Back Squat", 70.0, 5, 0.0, T + d));
    }
    let squat_on_day = |day: i64| {
        let before: Vec<DaySet> = sets.iter().copied().filter(|s| s.4 < day).collect();
        squat_on(back_from_layoff(6.0, T), &before, day)
    };
    let first = squat_on_day(T);
    assert!(
        first.why.basis.contains("scaled to 70% in week 1 of 4"),
        "{}",
        first.why.basis
    );
    let mid = squat_on_day(T + 14);
    assert!(
        mid.why.basis.contains("scaled to 84% in week 2 of 4"),
        "half-way up the ramp: {}",
        mid.why.basis
    );
    // The first lift day after the ramp (a Wednesday): full load on the
    // pre-layoff anchor.
    let full = squat_on_day(T + 30);
    assert!(!full.why.basis.contains("scaled to"), "{}", full.why.basis);
    assert!(
        full.anchored_on.starts_with("Estimated max 123.3 kg"),
        "the pre-layoff anchor is back at 100 %: {}",
        full.anchored_on
    );
}

#[test]
fn post_return_anchor_is_unscaled_and_long_layoff_window_ends() {
    // A heavier post-return set wins unscaled (95 kg × 5 @ RPE 8: 117.2 kg); a
    // >8 wk layoff sets pre-layoff anchors aside and prescribes every other
    // lift as a fresh novice (re-pointed to REENTRY-001) only while its
    // window runs.
    let sets = [
        ("Back Squat", 100.0, 5, 8.0, T - 102),
        ("Back Squat", 95.0, 5, 8.0, T - 1),
    ];
    let cards = cards_on(back_from_layoff(12.0, T - 2), &sets, T);
    let squat = card(cards.clone(), "Back Squat");
    assert!(
        squat.anchored_on.starts_with("Estimated max 117.2 kg"),
        "{}",
        squat.anchored_on
    );
    assert!(
        !squat.why.basis.contains("scaled to"),
        "post-return anchors are unscaled: {}",
        squat.why.basis
    );
    let deadlift = |cards: Vec<PrescriptionView>| card(cards, "Deadlift").evidence.grade;
    assert_eq!(deadlift(cards), Some(EvidenceGrade::ExpertOpinion));
    let later = cards_on(back_from_layoff(12.0, T - 2), &sets, T + 42);
    assert_eq!(
        deadlift(later),
        Some(EvidenceGrade::Strong),
        "the novice window ends with the ramp"
    );
}

#[test]
fn missed_target_cuts_next_load_5pct_unless_off_day() {
    // AUTOREG-E1RM-GATE-001: a session whose rated e1RM lands
    // >5 % under the baseline cuts that lift's next working load ~5 %. 100 kg
    // × 5 at RPE 10 reads 116.7 kg, 5.4 % under 123.3 kg, at an unchanged load.
    let sets = [
        set("Back Squat", 100.0, 5, 8.0, 0),
        set("Back Squat", 100.0, 5, 8.0, 7),
        set("Back Squat", 100.0, 5, 10.0, 14),
    ];
    let a = anchors_through(&sets, AnchorCtx::default(), 15);
    let d = a.load_frac_for("Back Squat").expect("an e1RM-drop cut");
    assert_eq!(d.frac, 0.95);
    assert!(matches!(d.cause, DerateCause::E1rmDrop));
    let off = AnchorCtx {
        off_day: Some(14),
        ..Default::default()
    };
    assert!(
        anchors_through(&sets, off, 15)
            .load_frac_for("Back Squat")
            .is_none(),
        "a session marked as an off day keeps the load"
    );
}

#[test]
fn a_past_day_keeps_the_off_day_of_the_review_in_force_that_day() {
    // AUTOREG-E1RM-GATE-001: Monday's squat lands >5 % under its baseline
    // and Monday's review marks it an off day, so Tuesday's and Wednesday's
    // plans keep the load. A later, ordinary review (Wednesday) must not
    // re-judge those past days: each reads the review in force on it.
    let start = POS_MON;
    let today = start + 4;
    let review = |bad_day: bool, day: i64| {
        Event::SubmitReview(SessionReview {
            bad_day,
            observed_at: day * DAY_SEC + 18 * 3_600,
            ..SessionReview::default()
        })
    };
    let model_with = |reviews: &[Event]| {
        let history = [
            log_set("Back Squat", 100.0, 5, 8.0, (start - 14) * DAY_SEC + 3_600),
            log_set("Back Squat", 100.0, 5, 8.0, (start - 7) * DAY_SEC + 3_600),
            log_set("Back Squat", 100.0, 5, 10.0, start * DAY_SEC + 3_600),
        ];
        let (app, mut model) = planned_on(strength_profile(), history, start);
        for ev in reviews {
            app.update(ev.clone(), &mut model).expect_only_render();
        }
        set_today(&app, &mut model, today);
        model
    };
    // Monday..Wednesday: prescribed through Wednesday's own morning, before
    // the ordinary review was submitted.
    let past_loads = |model: &Model| {
        Engine
            .view(model)
            .week_plan
            .into_iter()
            .filter(|s| (start..=start + 2).contains(&s.epoch_day))
            .map(|s| {
                let loads: Vec<(String, Option<f64>)> = s
                    .items
                    .into_iter()
                    .map(|i| (i.exercise, i.load_kg))
                    .collect();
                (s.epoch_day, loads)
            })
            .collect::<Vec<_>>()
    };
    let no_review = past_loads(&model_with(&[]));
    let off_day = past_loads(&model_with(&[review(true, start)]));
    assert_ne!(
        no_review, off_day,
        "the off day keeps a past day's squat load"
    );
    let later_review = past_loads(&model_with(&[
        review(true, start),
        review(false, start + 2),
    ]));
    assert_eq!(
        later_review, off_day,
        "a later review never re-judges a past day's off day"
    );
}

#[test]
fn a_light_lift_drop_at_the_same_load_is_cut() {
    // 45 kg × 3 at RPE 9.5 after 45 kg × 5 at RPE 8: the same bar weight, the
    // e1RM falls 55.5 → 50.25 kg (−9.5 %), past AUTOREG-E1RM-GATE-001's 5 %
    // however light the lift.
    let sets = [
        set("Overhead Press", 45.0, 5, 8.0, 0),
        set("Overhead Press", 45.0, 3, 9.5, 3),
    ];
    let d = anchors_through(&sets, AnchorCtx::default(), 4)
        .load_frac_for("Overhead Press")
        .cloned()
        .expect("an e1RM-drop cut");
    assert_eq!(d.cause, DerateCause::E1rmDrop);
}

#[test]
fn a_plate_floored_load_is_asked_at_the_effort_it_takes() {
    // 85 % × 3 of a 55.5 kg anchor is 47.2 kg, floored onto the 2.5 kg grid to
    // 45 kg: 81 % of the anchor, ~4 reps left on the Epley model where 85 %
    // would ask ~2. Done as asked, 45 kg × 3 reads the anchor back; at the
    // unfloored effort (RPE 8) it reads 52.5 kg, a 5.4 % drop
    // (AUTOREG-E1RM-GATE-001 at fixed RPE, autoreg-022).
    let anchors = crate::plan::Anchors {
        lift: crate::plan::LiftAnchors {
            e1rm: vec![("Back Squat".into(), 55.5)],
            ..Default::default()
        },
        ..Default::default()
    };
    let it = flatten_prescription(
        &pct_rx(85.0, 3, None),
        &anchors,
        None,
        false,
        DistanceUnit::Km,
    );
    assert_eq!((it.load_kg, it.target_rpe), (Some(45.0), Some(6.0)));
    let base = set("Back Squat", 45.0, 5, 8.0, 0);
    let asked = [base.clone(), set("Back Squat", 45.0, 3, 6.0, 3)];
    assert_eq!(
        anchors_through(&asked, AnchorCtx::default(), 4).load_frac_for("Back Squat"),
        None
    );
    let harder = [base, set("Back Squat", 45.0, 3, 8.0, 3)];
    let d = anchors_through(&harder, AnchorCtx::default(), 4)
        .load_frac_for("Back Squat")
        .cloned()
        .expect("an e1RM-drop cut");
    assert_eq!(d.cause, DerateCause::E1rmDrop);
}

#[test]
fn e1rm_drop_cut_also_leaves_one_more_rep_in_reserve() {
    // AUTOREG-E1RM-GATE-001 (autoreg-006): the cut session aims for one more
    // rep left than planned as well as ~5 % less weight, so the lighter load
    // done as asked reads at the estimate instead of as a second drop.
    let anchors = crate::plan::Anchors {
        lift: crate::plan::LiftAnchors {
            e1rm: vec![("Back Squat".into(), 100.0)],
            derates: vec![crate::plan::LiftDerate {
                exercise: "Back Squat".into(),
                frac: crate::autoreg::E1RM_REDUCE_RATIO,
                cause: DerateCause::E1rmDrop,
            }],
            ..Default::default()
        },
        ..Default::default()
    };
    let it = flatten_prescription(
        &pct_rx(85.0, 3, None),
        &anchors,
        None,
        false,
        DistanceUnit::Km,
    );
    // 100 kg × 85 % × 95 % = 80.75 kg, floored to 80 kg: 84 % of the cut
    // 95 kg estimate is ~3 reps left on its own, ~4 with the cut's extra rep.
    assert_eq!(it.load_kg, Some(80.0));
    assert_eq!(it.target_rpe, Some(6.0));
    assert!(it.summary.contains("4 reps left"), "{}", it.summary);
}

/// Three 100 kg × 5 @ RPE 8 sessions (epoch days 4–8, a Monday-start week),
/// then a planned deload week of 70 % × 5 @ RPE 7 on days 11 and 13.
fn deload_week_sets() -> [LoggedSet; 5] {
    [
        set("Back Squat", 100.0, 5, 8.0, 4),
        set("Back Squat", 100.0, 5, 8.0, 6),
        set("Back Squat", 100.0, 5, 8.0, 8),
        set("Back Squat", 70.0, 5, 7.0, 11),
        set("Back Squat", 70.0, 5, 7.0, 13),
    ]
}

#[test]
fn planned_deload_sets_neither_cut_nor_anchor_the_next_load() {
    // A planned deload's light sets (here 70 % at ~3 reps left, a hypertrophy
    // deload's RIR) are prescribed, not a performance drop (autoreg-022 reads
    // e1RM at fixed RPE): the e1RM-drop cut (AUTOREG-E1RM-GATE-001) and the
    // last working sets the next block resumes from (HYP-DELOAD-RX-001) skip
    // them.
    let sets = deload_week_sets();
    let deload = AnchorCtx {
        deloads: DeloadDays(&[11, 13]),
        ..Default::default()
    };
    let a = anchors_through(&sets, deload, 14);
    let work = (crate::strength::e1rm_epley_rpe(100.0, 5, 8.0) * 10.0).round() / 10.0;
    assert_eq!(a.e1rm_for("Back Squat"), Some(work));
    assert!(a.load_frac_for("Back Squat").is_none(), "no e1RM-drop cut");
    assert_eq!(a.last_working_for("Back Squat").unwrap().weight_kg, 100.0);
    // The same sets outside a planned deload are a >5 % drop.
    let b = anchors_through(&sets, AnchorCtx::default(), 14);
    assert!(
        b.load_frac_for("Back Squat")
            .is_some_and(|d| matches!(d.cause, DerateCause::E1rmDrop))
    );
    assert_eq!(b.last_working_for("Back Squat").unwrap().weight_kg, 70.0);
}

#[test]
fn planned_deload_sets_never_score_as_a_readiness_e1rm_drop() {
    // The same deload through the readiness gate (autoreg-006 today's cut,
    // autoreg-022 the decline deload; e1RM at fixed RPE,
    // extracted/06-autoregulation.md:278-280): a deload week's sets rated
    // exactly (70 % × 5 at RPE 7: 72 % of the 123.3 kg best) are left out, so
    // the last scored session is day 8's.
    let sets = deload_week_sets();
    let perf = |deloads| {
        let scored = crate::app::readiness::perf_sets(&sets, 13, 0, deloads);
        crate::autoreg::derive_performance(&scored, 13, 0)
    };
    let deload = perf(DeloadDays(&[11, 13]));
    assert!(
        deload
            .iter()
            .all(|p| !p.today && p.ratio >= 1.0 && p.decline_streak == 0),
        "{deload:?}"
    );
    // Outside a planned deload they are a drop: today's cut and a two-session
    // decline.
    let plain = perf(DeloadDays(&[]));
    assert!(
        plain.iter().any(|p| p.today && p.decline_streak == 2),
        "{plain:?}"
    );
}

fn pct_rx(pct: f32, reps: u8, velocity: Option<u8>) -> Rx {
    RxBasis::Workout.rx(
        Prescription::Lift(LiftPrescription {
            exercise: "Back Squat".into(),
            sets: 4,
            reps,
            intensity: LiftIntensity::PercentOneRm(pct),
            rest_sec: 180,
            velocity_loss_pct: velocity,
            reps_max: None,
            load_kg: None,
        }),
        claims::STR_DUP_001,
    )
}

#[test]
fn percent_item_carries_expected_target_rpe_and_plates() {
    // 85 % × 3 implies ~2 reps in reserve → RPE 8 on the face and the
    // editor hint. The barbell load comes with its plates.
    let anchors = crate::plan::Anchors {
        lift: crate::plan::LiftAnchors {
            e1rm: vec![("Back Squat".into(), 100.0)],
            ..Default::default()
        },
        ..Default::default()
    };
    let it = flatten_prescription(
        &pct_rx(85.0, 3, None),
        &anchors,
        None,
        false,
        DistanceUnit::Km,
    );
    assert_eq!(it.load_kg, Some(85.0));
    assert_eq!(it.target_rpe, Some(8.0));
    assert!(it.summary.contains("~2 reps left"), "{}", it.summary);
    assert!(
        !it.summary.contains("e1RM"),
        "the load, not the %, is on the face: {}",
        it.summary
    );
    assert_eq!(it.plates_per_side, vec![20.0, 10.0, 2.5]);
}

#[test]
fn power_item_face_shows_move_fast_without_a_device_number() {
    // The speed cue is on the face and needs no device.
    let anchors = crate::plan::Anchors {
        lift: crate::plan::LiftAnchors {
            e1rm: vec![("Back Squat".into(), 100.0)],
            ..Default::default()
        },
        ..Default::default()
    };
    let it = flatten_prescription(
        &pct_rx(50.0, 3, Some(10)),
        &anchors,
        None,
        false,
        DistanceUnit::Km,
    );
    assert_eq!(it.effort_cue, "move fast");
    assert!(it.summary.contains("move fast"), "{}", it.summary);
    assert!(it.why.basis.contains("visibly slows"), "{}", it.why.basis);
}

#[test]
fn derated_card_names_the_reason_and_cites_detraining() {
    let anchors = crate::plan::Anchors {
        lift: crate::plan::LiftAnchors {
            e1rm: vec![("Back Squat".into(), 100.0)],
            derates: vec![crate::plan::LiftDerate {
                exercise: "Back Squat".into(),
                frac: 0.70,
                cause: DerateCause::Detraining { days_since: 44 },
            }],
            ..Default::default()
        },
        ..Default::default()
    };
    let it = flatten_prescription(
        &pct_rx(80.0, 5, None),
        &anchors,
        None,
        false,
        DistanceUnit::Km,
    );
    assert_eq!(
        it.load_kg,
        Some(55.0),
        "100 × 0.80 × 0.70 = 56 → 55 on the grid"
    );
    assert!(it.why.basis.contains("scaled to 70%"), "{}", it.why.basis);
    assert_eq!(
        it.evidence.grade,
        Some(EvidenceGrade::Moderate),
        "re-pointed to DETRAIN-STR-001"
    );
}

#[test]
fn plates_per_side_greedy_kg_and_lb() {
    // 85 % of a 140 kg single is 119 → 117.5 kg: 48.75 kg a side on a 20 kg
    // bar.
    let kg = squat_on(
        strength_profile(),
        &[("Back Squat", 140.0, 1, 0.0, T - 1)],
        T,
    );
    assert_eq!(kg.load_kg, Some(117.5));
    assert_eq!(kg.plates_per_side, vec![20.0, 20.0, 5.0, 2.5, 1.25]);
    // 85 % of 121 kg is 226.7 lb → 225 lb on a 45 lb bar = 90 lb per side.
    let lb_profile = Profile {
        weight_lb: true,
        ..strength_profile()
    };
    let lb = squat_on(lb_profile, &[("Back Squat", 121.0, 1, 0.0, T - 1)], T);
    assert_eq!(lb.load_kg.map(|kg| (kg * LB_PER_KG).round()), Some(225.0));
    assert_eq!(lb.plates_per_side, vec![45.0, 45.0]);
}

#[test]
fn rir_item_carries_last_set_and_a_suggested_load() {
    // An effort-prescribed lift shows the athlete's last top
    // set from a prior day and starts from its weight: one plate step up
    // here, because 60 kg × 8 @ RPE 7 beat the 5-rep / 3-RIR target by 3 reps
    // (STR-LOADPROG-001).
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(strength_profile()), &mut model)
        .expect_only_render();
    app.update(
        with_entry_id(log_set("Row", 60.0, 8, 7.0, (MON - 1) * DAY_SEC + 3_600), 1),
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::GeneratePlan {
            start_epoch_day: MON,
        },
        &mut model,
    )
    .expect_only_render();
    set_today(&app, &mut model, MON);
    let item = app
        .view(&model)
        .week_plan
        .into_iter()
        .flat_map(|s| s.items.into_iter())
        .find(|i| crate::exercise::same_movement(&i.exercise, "Row") && i.load_kg.is_none())
        .expect("an effort-prescribed row item");
    let last = item.last_set.expect("last time is shown");
    assert_eq!((last.weight_kg, last.reps), (60.0, 8));
    assert_eq!(item.suggested_load_kg, Some(62.5));
    assert!(
        item.anchored_on.starts_with("Last: "),
        "{}",
        item.anchored_on
    );
}

/// The Row card on `T` after a 60 kg × 8 @ RPE 7 row `back` days before.
fn row_after(profile: Profile, back: i64) -> PrescriptionView {
    card(
        cards_on(profile, &[("Row", 60.0, 8, 7.0, T - back)], T),
        "Row",
    )
}

#[test]
fn no_start_weight_after_a_break_the_plan_treats_as_a_fresh_start() {
    // Past the 8-week horizon (DETRAIN-STR-001) or inside a declared
    // >8 wk re-entry (REENTRY-001: treat as novice) the plan carries no load,
    // so "Start from …" must not bring the pre-break weight back.
    let old = row_after(strength_profile(), 70);
    assert!(
        old.anchored_on.starts_with("Last: "),
        "last time is still shown"
    );
    assert_eq!(old.suggested_load_kg, None);
    let reentry = row_after(back_from_layoff(12.0, T - 1), 3);
    assert_eq!(reentry.suggested_load_kg, None);
    // Control: a recent set that beat the 5-rep / 3-RIR target by 3 reps
    // suggests one step up (STR-LOADPROG-001).
    assert_eq!(
        row_after(strength_profile(), 3).suggested_load_kg,
        Some(62.5)
    );
}

#[test]
fn start_weight_after_a_six_week_gap_is_derated() {
    // 31–56 days since the set → the detraining credit (70 %) scales the
    // suggestion, never the full pre-gap load.
    let item = row_after(strength_profile(), 40);
    assert_eq!(item.suggested_load_kg, Some(40.0), "60 × 0.70 = 42 → 40");
    assert!(
        item.why.basis.contains("70% of your last top set"),
        "{}",
        item.why.basis
    );
}

#[test]
fn an_undeclared_gap_ramps_back_instead_of_jumping_to_full_load() {
    // Squat e1RM 150 (125 kg × 5 @ RPE 9) 47, 44 and 40 days ago, then
    // nothing. Today is prescribed at the 70 % detraining credit; the light
    // session logged today must not restore 100 % on the next lift day
    // (REENTRY-001 4–8 wk: 70 %, ramp 2–4 weeks). Rated or not, the next
    // load stays on the re-entry ramp.
    let base: Vec<Event> = [47, 44, 40]
        .iter()
        .map(|&back| log_set("Back Squat", 125.0, 5, 9.0, (T - back) * DAY_SEC + 3_600))
        .collect();
    for rpe in [0.0, 7.0] {
        let (app, mut model) = planned_on(strength_profile(), base.clone(), T);
        let today = card(day_row(&app, &model, T).items, "Back Squat");
        assert!(
            today.anchored_on.starts_with("Estimated max 150 kg"),
            "{}",
            today.anchored_on
        );
        assert!(
            today.why.basis.contains("scaled to 70% after about"),
            "detraining credit: {}",
            today.why.basis
        );
        let heavy = today.load_kg.expect("a derated load");
        assert_eq!(heavy, 87.5);
        for k in 0..3 {
            app.update(
                log_set("Back Squat", heavy, 3, rpe, T * DAY_SEC + 3_600 + k * 300),
                &mut model,
            )
            .expect_only_render();
        }
        set_today(&app, &mut model, T + 2);
        let next = card(day_row(&app, &model, T + 2).items, "Back Squat");
        assert!(
            next.why
                .basis
                .contains("scaled to 71% in week 1 of 4 back from your break"),
            "rpe {rpe}: still on the ramp: {}",
            next.why.basis
        );
        let load = next.load_kg.expect("a ramped load");
        assert!(load <= heavy * 1.10, "rpe {rpe}: {heavy} → {load}");
    }
}

#[test]
fn an_open_ended_effort_set_is_a_lower_bound_for_the_anchor() {
    // "4+ reps left" (RPE 6) counts its reps as performed, like an
    // unrated set: it can raise the anchor only by what was actually lifted
    // (110 kg × 5: 128.3 kg, not the 146.7 kg its 4 reps left would read)
    // and never cuts it (100 kg × 5 @ RPE 8 stays at 123.3 kg).
    let base = ("Back Squat", 100.0, 5, 8.0, T - 4);
    let easy_heavy = squat_on(
        strength_profile(),
        &[base, ("Back Squat", 110.0, 5, 6.0, T - 1)],
        T,
    );
    assert!(
        easy_heavy.anchored_on.starts_with("Estimated max 128.3 kg"),
        "{}",
        easy_heavy.anchored_on
    );
    let easy_light = squat_on(
        strength_profile(),
        &[base, ("Back Squat", 90.0, 5, 6.0, T - 1)],
        T,
    );
    assert!(
        easy_light.anchored_on.starts_with("Estimated max 123.3 kg"),
        "{}",
        easy_light.anchored_on
    );
    assert!(
        !easy_light.why.basis.contains("scaled to"),
        "never an e1RM-drop cut: {}",
        easy_light.why.basis
    );
}

#[test]
fn warm_up_sets_do_not_silence_the_prilepin_audit() {
    // A warm-up ramp (STR-WARMUP-001) is not working volume. 5×3 at the
    // top load is 15 reps above 90 % (Prilepin 4–10): the row must fire even
    // though the warm-ups would pull the day's mean intensity under 70 %.
    let ramp = [(20.0, 5), (40.0, 5), (60.0, 3)];
    let events: Vec<Event> = ramp
        .into_iter()
        .chain([(100.0, 3); 5])
        .enumerate()
        .map(|(i, (kg, reps))| log_set("Back squat", kg, reps, 0.0, 10 * DAY_SEC + i as i64))
        .collect();
    let (app, model) = replay_rendering(&events);
    let rows = app.view(&model).lift_audit;
    assert!(
        rows.iter()
            .any(|r| r.summary.starts_with("Back squat: 15 reps")),
        "{rows:?}"
    );
}

#[test]
fn a_deload_lift_day_is_titled_as_the_lighter_version() {
    let rx = |basis: RxBasis, claim: ClaimId| {
        basis.rx(
            Prescription::Lift(LiftPrescription {
                exercise: "Back Squat".into(),
                sets: 3,
                reps: 3,
                intensity: LiftIntensity::PercentOneRm(76.5),
                rest_sec: 180,
                velocity_loss_pct: None,
                reps_max: None,
                load_kg: None,
            }),
            claim,
        )
    };
    let session = |basis: RxBasis, claim: ClaimId| crate::schema::Session {
        session_type: SessionType::Lift(LiftSessionType::MaxEffort),
        day: 0,
        prescriptions: vec![rx(basis, claim)],
        dup_day: None,
    };
    assert_eq!(
        plan_session_title(&session(
            RxBasis::StrengthDeload,
            claims::STR_DELOAD_VOL_001
        )),
        "Heavy day (lighter week)"
    );
    assert_eq!(
        plan_session_title(&session(RxBasis::Workout, claims::STR_DUP_001)),
        "Heavy day"
    );
}

#[test]
fn weights_render_once_rounded_in_the_display_unit() {
    // lb: whole or .5 (no "264.6 lb"); kg: rounded before the whole test, so
    // 99.97 kg is "100 kg", never "100.0 kg".
    assert_eq!(fmt_weight(120.0, true), "264.5 lb");
    assert_eq!(fmt_weight(290.0 / LB_PER_KG, true), "290 lb");
    assert_eq!(fmt_weight(99.97, false), "100 kg");
    assert_eq!(fmt_weight(92.5, false), "92.5 kg");
}

#[test]
fn twenty_rep_leg_press_does_not_anchor_a_pct_1rm_load() {
    // A 20-rep set is outside the reliable e1RM window (>10 effective reps), so
    // it never anchors a %1RM working load: the plan stays on RIR (effort)
    // prescriptions instead of a fabricated load.
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(strength_profile()), &mut model)
        .expect_only_render();
    app.update(
        log_set("Leg press", 200.0, 20, 8.0, (MON - 1) * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::GeneratePlan {
            start_epoch_day: MON,
        },
        &mut model,
    )
    .expect_only_render();
    set_today(&app, &mut model, MON);
    let ns = app
        .view(&model)
        .next_session
        .expect("a plan yields a next session");
    for it in ns.items.iter().filter(|i| !i.exercise.is_empty()) {
        assert!(
            it.load_kg.is_none(),
            "no lift may carry a %1RM load off a 20-rep set: {} @ {:?}",
            it.exercise,
            it.load_kg
        );
    }
}

#[test]
fn stale_pr_does_not_revive_on_a_light_set() {
    // A 14-month-old 140 kg PR has fully detrained; a recent light 60 kg set must
    // not inherit its credit. The working load anchors off the 60 kg set.
    let app = Engine;
    let mut model = Model::default();
    // A Monday far enough out that a 400-day-old PR keeps a positive (dated)
    // timestamp (a non-positive stamp reads as undated and skips decay).
    let mon = 403_i64;
    app.update(Event::SetProfile(strength_profile()), &mut model)
        .expect_only_render();
    app.update(
        log_set_id("Back squat", 140.0, 1, (mon - 400) * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    app.update(
        log_set_id("Back squat", 60.0, 2, (mon - 1) * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::GeneratePlan {
            start_epoch_day: mon,
        },
        &mut model,
    )
    .expect_only_render();
    set_today(&app, &mut model, mon);
    let ns = app
        .view(&model)
        .next_session
        .expect("a plan yields a next session");
    let squat = ns
        .items
        .iter()
        .find(|i| i.exercise.to_lowercase().contains("squat") && i.load_kg.is_some())
        .expect("an anchored squat item");
    let load = squat.load_kg.unwrap();
    assert!(
        load < 90.0,
        "load {load} must derive from the recent 60 kg set, not the stale 140 kg PR"
    );
}

#[test]
fn declared_layoff_derates_the_lift_load_not_the_logged_best() {
    // After a declared layoff the WORKING LOAD is derated (REENTRY-RAMP-001),
    // but the estimate shown stays the logged best so the honesty line never
    // lies, and the card re-points to the rule that scaled it. A 100 kg single
    // at 85 %: 85 kg fresh, 59.5 → 57.5 kg at the 70 % re-entry start.
    let best = [("Back Squat", 100.0, 1, 0.0, T - 1)];
    let full = squat_on(strength_profile(), &best, T);
    let re = squat_on(back_from_layoff(6.0, T), &best, T);
    assert_eq!((full.load_kg, re.load_kg), (Some(85.0), Some(57.5)));
    for card in [&full, &re] {
        assert!(
            card.anchored_on
                .starts_with("Estimated max 100 kg (your recent best)"),
            "anchored_on still names the logged best: {}",
            card.anchored_on
        );
    }
    assert_eq!(
        re.evidence.grade,
        Some(EvidenceGrade::ExpertOpinion),
        "derated load re-points to REENTRY-RAMP-001"
    );
    assert_eq!(
        full.evidence.grade,
        Some(EvidenceGrade::Moderate),
        "the full load keeps its loading-band grade"
    );
    // The why-panel note must match the re-pointed chip, not the stale band.
    assert!(
        re.why.grade_note.contains("Expert opinion"),
        "grade_note follows the re-pointed chip: {}",
        re.why.grade_note
    );
    assert!(
        full.why.grade_note.contains("Moderate evidence")
            && !re.why.grade_note.contains("Moderate evidence"),
        "no stale band note on a derated card: {} / {}",
        full.why.grade_note,
        re.why.grade_note
    );
}

#[test]
fn a_stale_pr_is_dropped_from_the_plan_anchors() {
    // A 14-month-old PR no longer reflects current strength, so it must not
    // anchor a %1RM working load (it decays to zero credit and drops out,
    // leaving a RIR prescription). A recent PR of the same load still anchors.
    let squat_after = |back: i64| {
        squat_on(
            strength_profile(),
            &[("Back Squat", 140.0, 1, 0.0, T - back)],
            T,
        )
    };
    let stale = squat_after(420);
    assert_eq!(
        stale.load_kg, None,
        "a 14-month-old PR no longer anchors a %1RM load"
    );
    assert!(
        stale.intensity_label.starts_with("RIR"),
        "{}",
        stale.intensity_label
    );
    assert_eq!(squat_after(5).load_kg, Some(117.5), "a recent PR anchors");
}

#[test]
fn a_weighted_timed_hold_produces_no_plan_anchor() {
    // A weighted hold has no e1RM: a hold is isometric time, not a rep-max
    // (CAL-E1RM-001). Without a hold guard a 100 kg × 30 s squat hold would
    // read as a 100 kg single and anchor an 85 kg working load, on a WEIGHTS
    // profile too, where the ladder-name check never catches it.
    let squat_after = |hold: Option<u16>| {
        let mut ev = log_set("Back Squat", 100.0, 1, 0.0, (T - 1) * DAY_SEC);
        let Event::LogSet { hold_sec, .. } = &mut ev else {
            unreachable!()
        };
        *hold_sec = hold;
        let (app, model) = planned_on(strength_profile(), [ev], T);
        card(day_row(&app, &model, T).items, "Back Squat")
    };
    assert_eq!(
        squat_after(Some(30)).load_kg,
        None,
        "a weighted hold must not fabricate a %1RM anchor"
    );
    assert_eq!(
        squat_after(None).load_kg,
        Some(85.0),
        "the same weight lifted once anchors an e1RM"
    );
}

#[test]
fn long_layoff_drops_the_anchor_and_represcribes_as_novice() {
    // A >8 wk layoff carries NO KB load fraction (Table 3.4b) and directs a
    // fresh-novice re-entry: while the re-entry window runs the pre-layoff
    // anchor is set aside and the lift is prescribed by RIR (technique first),
    // never a scaled %e1RM load (HARD RULE 1). The card still cites the
    // re-entry reason.
    let squat = squat_on(
        back_from_layoff(12.0, T),
        &[("Back Squat", 140.0, 5, 8.0, T - 90)],
        T,
    );
    assert_eq!(squat.load_kg, None, "no %e1RM load on a novice re-entry");
    assert!(
        squat.intensity_label.starts_with("RIR"),
        "novice re-entry is prescribed by RIR, not % e1RM: {}",
        squat.intensity_label
    );
    assert!(
        !squat.anchored_on.starts_with("Estimated max"),
        "no logged-best load line when the anchor is set aside: {}",
        squat.anchored_on
    );
    // The card re-points to REENTRY-001 (ExpertOpinion), citing the reason.
    assert_eq!(
        squat.evidence.grade,
        Some(EvidenceGrade::ExpertOpinion),
        "re-points to REENTRY-001"
    );
    assert!(
        squat.why.grade_note.contains("Expert opinion"),
        "grade_note follows the re-pointed chip: {}",
        squat.why.grade_note
    );
}

#[test]
fn zero_weight_set_does_not_anchor_a_load() {
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(strength_profile()), &mut model)
        .expect_only_render();
    // Only a 0 kg "set" is logged for this exercise, dated YESTERDAY so it is
    // part of the history today's prescription is synthesized from (a set logged
    // today never anchors today).
    app.update(
        log_set_id("Back Squat", 0.0, 1, (MON - 1) * DAY_SEC),
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::GeneratePlan {
            start_epoch_day: MON,
        },
        &mut model,
    )
    .expect_only_render();
    set_today(&app, &mut model, MON);
    let vm = app.view(&model);
    let ns = vm.next_session.expect("a plan renders");
    for it in ns.items.iter().filter(|i| !i.exercise.is_empty()) {
        assert!(
            it.load_kg.is_none_or(|kg| kg > 0.0),
            "a 0 kg set must never anchor a load: {}",
            it.summary
        );
        assert!(!it.summary.contains("@ 0 kg"), "no @ 0 kg: {}", it.summary);
    }
}
