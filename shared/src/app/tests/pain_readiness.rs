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
        app.view(&model).train_blocked,
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
        !app.view(&model).train_blocked,
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
    assert!(!vm.train_blocked, "tolerable tendon pain must not block");
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
    assert!(vm.train_blocked);
    assert_eq!(vm.adjustments.len(), 1);
    assert_eq!(vm.adjustments[0].summary, "Stop: do not train");
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
