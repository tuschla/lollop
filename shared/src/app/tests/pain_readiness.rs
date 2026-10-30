//! Pain through the view: graded tendon pain modifies without blocking, a
//! sharp report stops training with a single call and names its site, and the
//! hold, its readiness row and the "remove the pain report" undo all resolve
//! the SAME worst-active report.

use super::*;

#[test]
fn removing_the_worst_active_pain_report_lifts_the_hold_in_one_tap() {
    // Sharp joint-line pain (a hard stop) then, later, a tolerable DOMS at a
    // different site. "Remove the pain report" must drop the report DRIVING the
    // hold (the sharp report the gate selects), not merely the newest DOMS, so a
    // single tap lifts the training hold as the dialog promises.
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SubmitReadiness(pain_row(
            1.0,
            Some(crate::schema::PainKind::SharpJoint),
            100,
        )),
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::SubmitReadiness(pain_row(1.0, Some(crate::schema::PainKind::Doms), 200)),
        &mut model,
    )
    .expect_only_render();
    assert!(
        app.view(&model).hold_reason.is_some(),
        "the sharp-joint report holds training"
    );

    app.update(
        Event::RemoveReadiness {
            signal: ReadinessSignal::Pain,
            observed_at: 300,
        },
        &mut model,
    )
    .expect_only_render();
    assert!(
        app.view(&model).hold_reason.is_none(),
        "removing the worst-active report lifts the hold in one tap"
    );
}

#[test]
fn graded_tendon_pain_modifies_without_blocking_through_the_view() {
    // File 08 Table 4.1 through the full event→view contract: tolerable
    // tendon pain (≤5/10, stable) surfaces the Pain tier and a
    // modify-and-monitor adjustment, but must NOT block training ("avoid
    // complete rest"), unlike the bare Pain report below, which stays a
    // conservative hard stop.
    let app = Engine;
    let mut model = Model::default();
    let event = r#"{"SubmitReadiness":{"signal":"Pain","value":1.0,"observed_at":0,"pain":{"kind":"TendonLoadRelated","severity":3,"trend":"Stable","persists":false}}}"#;
    let event: Event = serde_json::from_str(event).expect("graded pain event parses");
    app.update(event, &mut model).expect_only_render();

    let vm = app.view(&model);
    assert_eq!(vm.safety_tier, Some(SafetyTier::Pain));
    assert!(
        vm.hold_reason.is_none(),
        "tolerable tendon pain must not block"
    );
    let adj = vm
        .adjustments
        .iter()
        .find(|a| a.summary.contains("Avoid complete rest"))
        .expect("modify-and-monitor adjustment surfaces");
    assert!(adj.evidence.safety_critical);
    assert_eq!(adj.evidence.grade, Some(EvidenceGrade::Moderate));
}

#[test]
fn pain_blocks_training_with_a_single_stop() {
    let app = Engine;
    let mut model = Model::default();

    app.update(
        Event::SubmitReadiness(input(ReadinessSignal::Pain, 1.0)),
        &mut model,
    )
    .expect_only_render();

    let vm = app.view(&model);
    assert_eq!(vm.safety_tier, Some(SafetyTier::Pain));
    assert!(vm.hold_reason.is_some());
    assert_eq!(vm.adjustments.len(), 1);
    assert!(vm.adjustments[0].evidence.safety_critical);
    assert!(
        vm.adjustments[0].summary.contains("paused"),
        "{}",
        vm.adjustments[0].summary
    );
    assert_eq!(vm.today_headline.kind, HeadlineKind::SafetyHold);
}

#[test]
fn pain_location_surfaces_on_the_readiness_row_detail_through_the_view() {
    // A characterized pain report with a body-part location must reach the
    // shell's DO-NOT-TRAIN banner sub-line via ReadinessSignalView.detail;
    // an identical report with no location must leave detail body-part-free
    // (never fabricated, HARD RULE 1).
    let pain_row = |json: &str| -> ReadinessSignalView {
        let app = Engine;
        let mut model = Model::default();
        let event: Event = serde_json::from_str(json).expect("pain event parses");
        app.update(event, &mut model).expect_only_render();
        app.view(&model)
            .readiness_summary
            .into_iter()
            .find(|r| r.signal == ReadinessSignal::Pain)
            .expect("pain row present")
    };

    let with_loc = pain_row(
        r#"{"SubmitReadiness":{"signal":"Pain","value":1.0,"observed_at":0,"pain":{"kind":"SharpJoint","severity":6,"trend":"Stable","location":"Left knee"}}}"#,
    );
    assert!(
        with_loc.detail.contains("Left knee"),
        "banner sub-line must name the body part, got {:?}",
        with_loc.detail
    );

    let no_loc = pain_row(
        r#"{"SubmitReadiness":{"signal":"Pain","value":1.0,"observed_at":0,"pain":{"kind":"SharpJoint","severity":6,"trend":"Stable"}}}"#,
    );
    assert!(
        !no_loc.detail.to_lowercase().contains("knee"),
        "no location must be invented when None, got {:?}",
        no_loc.detail
    );
}

#[test]
fn the_pain_hold_line_says_paused_only_when_training_is_paused() {
    // A scoped sharp-joint stop leaves the rest of the session on: its resolve
    // line must name what stays out, not claim training is paused. A bare
    // report stops the whole day, and its line says so.
    let pain_line = |json: &str| -> (bool, String) {
        let app = Engine;
        let mut model = Model::default();
        let event: Event = serde_json::from_str(json).expect("pain event parses");
        app.update(event, &mut model).expect_only_render();
        let vm = app.view(&model);
        let line = vm
            .hold_resolutions
            .iter()
            .find(|h| h.signal == ReadinessSignal::Pain)
            .expect("a pain resolve action is offered")
            .paused_text
            .clone();
        (vm.hold_reason.is_some(), line)
    };
    let (blocked, scoped) = pain_line(
        r#"{"SubmitReadiness":{"signal":"Pain","value":1.0,"observed_at":0,"pain":{"kind":"SharpJoint","severity":6,"trend":"Stable","location":"Left knee"}}}"#,
    );
    assert!(!blocked, "a scoped stop must not block the day");
    assert!(!scoped.to_lowercase().contains("paused"), "{scoped}");
    assert!(
        scoped.contains("exercises that load the painful area"),
        "{scoped}"
    );

    let (blocked, bare) =
        pain_line(r#"{"SubmitReadiness":{"signal":"Pain","value":1.0,"observed_at":0}}"#);
    assert!(blocked);
    assert!(bare.starts_with("Training is paused"), "{bare}");
}
