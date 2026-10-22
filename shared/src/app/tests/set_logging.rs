//! Logging a set: derived e1RM and RIR, raw-input echo, unit rendering and
//! history order.

use super::*;

#[test]
fn logging_a_set_derives_e1rm_and_rir() {
    let app = Engine;
    let mut model = Model::default();

    app.update(log_set("Back squat", 100.0, 5, 8.0, 0), &mut model)
        .expect_only_render();

    let vm = app.view(&model);
    assert_eq!(vm.lifts.len(), 1);
    // RPE 8 → 2 RIR → 7 effective reps; Epley: 100 * (1 + 7/30) = 123.3.
    assert!((vm.lifts[0].e1rm_kg - 123.3).abs() < 0.05);
    // Epley inverse for 7 effective reps: 100 / (1 + 7/30) = 81.1 → 81%1RM.
    assert!((vm.lifts[0].pct_1rm - 81.0).abs() < f64::EPSILON);
    // RPE 8 → 2 RIR.
    assert!((vm.lifts[0].rir - 2.0).abs() < f64::EPSILON);
}

/// A higher RPE means fewer reps in reserve, so the SAME load and reps taken
/// at RPE 7 (3 RIR) scores a higher e1RM than at RPE 9 (1 RIR): each set is
/// extended to its effective reps-to-failure before Epley. RPE 0 ("not
/// recorded") falls back to the plain reps-to-failure Epley and is flagged
/// as not RPE-recorded.
#[test]
fn rpe_extends_effective_reps_for_e1rm() {
    let app = Engine;
    let log_at = |rpe: f64| {
        let mut m = Model::default();
        app.update(log_set("Back squat", 100.0, 5, rpe, 0), &mut m)
            .expect_only_render();
        app.view(&m).lifts[0].clone()
    };
    let hard = log_at(9.0);
    let easy = log_at(7.0);
    assert!(
        easy.e1rm_kg > hard.e1rm_kg,
        "RPE 7 (3 RIR) out-scores RPE 9 (1 RIR): {} vs {}",
        easy.e1rm_kg,
        hard.e1rm_kg
    );
    assert!(hard.rpe_recorded && easy.rpe_recorded);

    let unrecorded = log_at(0.0);
    let plain = (strength::e1rm_epley(100.0, 5) * 10.0).round() / 10.0;
    assert!(
        (unrecorded.e1rm_kg - plain).abs() < f64::EPSILON,
        "RPE 0 scores as plain reps-to-failure Epley: {} vs {plain}",
        unrecorded.e1rm_kg
    );
    assert!(!unrecorded.rpe_recorded, "RPE 0 is not a recorded RPE");
}

/// A profile with `weight_lb` set flips every core-rendered weight string to
/// pounds (display-only; the numeric `weight_kg`/`e1rm_kg` fields stay kg).
#[test]
fn weight_lb_profile_renders_pounds_in_the_lift_summary() {
    let app = Engine;
    let mut model = Model::default();
    app.update(log_set("Back squat", 100.0, 5, 8.0, 0), &mut model)
        .expect_only_render();
    app.update(
        Event::SetProfile(Profile {
            weight_lb: true,
            ..sample_profile()
        }),
        &mut model,
    )
    .expect_only_render();

    let vm = app.view(&model);
    let summary = &vm.lifts[0].summary;
    // 100 kg × 2.2046226218 = 220.46 → "220.5lb"; e1RM 123.3 kg = 271.83 lb →
    // "272lb" (lb figures round once to the nearest 0.5 lb).
    assert!(summary.contains("220.5lb"), "load in lb: {summary}");
    assert!(summary.contains("272lb"), "e1RM in lb: {summary}");
    assert!(!summary.contains("kg"), "no kg under weight_lb: {summary}");
    // The NUMERIC field is untouched by the display unit (stays kg).
    assert!((vm.lifts[0].weight_kg - 100.0).abs() < f64::EPSILON);

    // The protein "… g/kg × NN kg" bodyweight echo multiplies a g/kg range,
    // so it stays in kg; a pound profile gets the pound figure alongside,
    // never in nested brackets (spot-runs-setup N13).
    // 80 kg × 2.2046226218 = 176.37 → "× 80 kg (176 lb)".
    app.update(
        Event::ComputeProtein {
            bodyweight_kg: 80.0,
            masters: true,
            deficit: false,
        },
        &mut model,
    )
    .expect_only_render();
    let psummary = &app.view(&model).protein_targets[0].summary;
    assert!(
        psummary.contains("g/day · ") && psummary.ends_with(" g/kg × 80 kg (176 lb)"),
        "bodyweight echo in kg with lb alongside: {psummary}"
    );
    assert!(!psummary.contains("))"), "no nested brackets: {psummary}");
    // The g/kg scientific unit itself is unconverted.
    assert!(psummary.contains("g/kg"), "g/kg unit stays: {psummary}");
}

/// Regression: the default (metric) profile renders the weight strings
/// unchanged from the pre-unit-flag core.
#[test]
fn default_metric_profile_renders_kg_weight_strings_unchanged() {
    let app = Engine;
    let mut model = Model::default();
    app.update(log_set("Back squat", 100.0, 5, 8.0, 0), &mut model)
        .expect_only_render();
    const EXPECT: &str = "Back squat 100kg × 5 @RPE8.0 → e1RM 123.3kg (~81%1RM, 2.0 RIR)";
    // No profile at all → weight_lb defaults false.
    assert_eq!(app.view(&model).lifts[0].summary, EXPECT);
    // An explicit metric profile renders identically.
    app.update(
        Event::SetProfile(Profile {
            weight_lb: false,
            ..sample_profile()
        }),
        &mut model,
    )
    .expect_only_render();
    assert_eq!(app.view(&model).lifts[0].summary, EXPECT);
}

#[test]
fn logging_a_set_echoes_raw_input_alongside_derived_metrics() {
    // The view carries the set exactly as logged, not only the derived e1RM: a
    // lifter must see what they actually did (weight × reps @ RPE), which the
    // shell renders beside the coaching metrics. Locks that echo contract, a
    // fractional plate load must survive to the view unrounded.
    let app = Engine;
    let mut model = Model::default();

    app.update(log_set("Front squat", 92.5, 3, 9.0, 0), &mut model)
        .expect_only_render();

    let lift = app.view(&model).lifts.remove(0);
    assert_eq!(lift.exercise, "Front squat");
    assert!((lift.weight_kg - 92.5).abs() < f64::EPSILON);
    assert_eq!(lift.reps, 3);
    assert!((lift.rpe - 9.0).abs() < f64::EPSILON);
    // The summary is the human line the shell shows; it names the raw set (reps
    // @ RPE), not only the derived e1RM. The fractional plate load must survive
    // here too: a 92.5 kg lift must read "92.5kg", not a truncated "92kg".
    assert!(lift.summary.contains("92.5kg"), "got {}", lift.summary);
    assert!(lift.summary.contains("× 3 @RPE9.0"), "got {}", lift.summary);
    assert!(lift.summary.contains("RIR"), "got {}", lift.summary);
}

#[test]
fn logged_sets_accumulate_in_log_order() {
    // The view lists sets oldest-first, in the order they were logged: the
    // shell renders this sequence directly, so a regression that reversed or
    // de-duplicated the list would misreport a session. Two distinct sets must
    // both survive, in order.
    let app = Engine;
    let mut model = Model::default();
    for (exercise, reps) in [("Deadlift", 3), ("Bench", 8)] {
        app.update(log_set(exercise, 80.0, reps, 7.0, 0), &mut model)
            .expect_only_render();
    }

    let lifts = app.view(&model).lifts;
    assert_eq!(lifts.len(), 2);
    assert_eq!(lifts[0].exercise, "Deadlift");
    assert_eq!(lifts[0].reps, 3);
    assert_eq!(lifts[1].exercise, "Bench");
    assert_eq!(lifts[1].reps, 8);
}

#[test]
fn e1rm_trend_tracks_previous_set_of_the_same_lift_only() {
    // The core computes per-lift e1RM delta + direction so a shell renders
    // without arithmetic. Direction is a factual measurement
    // (what changed), not an improving/declining judgment: that phrasing
    // is the feedback-027/028/029 trend arm.
    let app = Engine;
    let mut model = Model::default();
    let mut log = |exercise: &str, weight_kg: f64, reps: u32| {
        app.update(log_set(exercise, weight_kg, reps, 8.0, 0), &mut model)
            .expect_only_render();
    };
    log("Squat", 100.0, 5); // e1RM 123.3 (RPE 8 → 7 eff reps)
    log("Bench", 80.0, 5); // other lift must not interfere
    log("Squat", 102.5, 5); // e1RM 126.4 → +3.1, up
    log("Squat", 102.5, 5); // unchanged → 0.0, flat
    log("Squat", 95.0, 5); // e1RM 117.2 → -9.2, down

    let lifts = app.view(&model).lifts;
    let squats: Vec<_> = lifts.iter().filter(|l| l.exercise == "Squat").collect();

    // First set of a lift: nothing to compare against.
    assert!(squats[0].e1rm_delta_kg.is_none());
    assert!(squats[0].e1rm_direction.is_none());
    // Bench in between must not break the Squat chain, and Bench's own
    // first set carries no trend either.
    let bench = lifts.iter().find(|l| l.exercise == "Bench").unwrap();
    assert!(bench.e1rm_delta_kg.is_none());

    assert!((squats[1].e1rm_delta_kg.unwrap() - 3.1).abs() < 0.05);
    assert_eq!(squats[1].e1rm_direction, Some(TrendDirection::Up));

    assert_eq!(squats[2].e1rm_delta_kg, Some(0.0));
    assert_eq!(squats[2].e1rm_direction, Some(TrendDirection::Flat));

    assert!((squats[3].e1rm_delta_kg.unwrap() + 9.2).abs() < 0.05);
    assert_eq!(squats[3].e1rm_direction, Some(TrendDirection::Down));
}
