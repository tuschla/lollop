//! The three-part "why?" disclosure (basis, grade note, what improves it) and
//! the grade legend.

use super::*;

#[test]
fn why_disclosure_on_hr_zones_carries_basis_grade_note_and_improves() {
    // The device-verification target: expanding "why?" on the HRmax card
    // shows the Tanaka basis, a grade rationale, and the engagement line
    // ("log a measured max HR to improve"). None of these three strings may
    // be empty on the HRmax action row.
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::ComputeHrZones {
            age_years: 30.0,
            resting_hr_bpm: None,
            weeks_since_recalc: None,
            weeks_since_pace_test: None,
        },
        &mut model,
    )
    .expect_only_render();

    let hrmax = &app.view(&model).hr_zones[0];
    let why = &hrmax.why;
    // basis: datum-rich Tanaka restatement using the user's own age.
    assert!(
        why.basis.contains("Tanaka") && why.basis.contains("30"),
        "basis restates the rule + the user's datum: {}",
        why.basis
    );
    // grade_note: RUN-HRMAX-001 is Moderate → the Moderate gloss.
    assert_eq!(hrmax.evidence.grade, Some(EvidenceGrade::Moderate));
    assert!(
        why.grade_note.contains("Moderate evidence"),
        "grade_note glosses the grade: {}",
        why.grade_note
    );
    // improves: the engagement loop, replace the age estimate.
    assert!(
        why.improves.to_lowercase().contains("measured max hr"),
        "improves points at the data that would sharpen it: {}",
        why.improves
    );
    // The improves line describes the engine's data need, never a training
    // prescription (HARD RULE 1): no imperative training terms (word-level
    // match so "replace"/"estimate" don't false-trip on "rep"/"set").
    let banned = ["squat", "deadlift", "%1rm", "rir", "rpe", "reps", "sets"];
    let words: Vec<String> = why
        .improves
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '%')
        .map(|w| w.to_string())
        .collect();
    for b in banned {
        assert!(
            !words.iter().any(|w| w == b),
            "improves must not prescribe: {}",
            why.improves
        );
    }
}

#[test]
fn contested_guidance_grade_note_names_the_contested_question() {
    // A contested claim (HYP-VOL-001 → CQ-01) must append the contested
    // question to the grade note, not just restate the badge.
    let app = Engine;
    let model = Model {
        profile: Some(sample_profile()),
        ..Default::default()
    };
    let vm = app.view(&model);
    let g = vm
        .guidance
        .iter()
        .chain(vm.reference.iter())
        .find(|g| g.evidence.contested && !g.why.grade_note.is_empty())
        .expect("the sample profile surfaces a contested row (Prilepin)");
    assert!(
        g.why.grade_note.to_lowercase().contains("contested"),
        "contested claim's grade_note flags it: {}",
        g.why.grade_note
    );
}

#[test]
fn contested_render_shows_one_opposing_side_when_kb_supplies_it() {
    // CQ-01 carries a KB-sourced opposing citation → the disclosure shows
    // it as "one view on the other side"; CQ-02 has none → the render stops
    // at the engine lean and fabricates no opposing view.
    let with = grade_note_str(EvidenceGrade::Strong, Some("CQ-01"));
    assert!(
        with.contains("contested") && with.contains("What the app does for now:"),
        "contested render carries the app's lean: {with}"
    );
    assert!(
        with.contains("One view on the other side:") && with.to_lowercase().contains("schoenfeld"),
        "CQ-01's KB opposing cite surfaces: {with}"
    );

    let without = grade_note_str(EvidenceGrade::Moderate, Some("CQ-02"));
    assert!(
        without.contains("contested"),
        "CQ-02 still flags the debate: {without}"
    );
    assert!(
        !without.contains("One view on the other side:"),
        "CQ-02 has no KB opposing cite → no fabricated other side: {without}"
    );
}

#[test]
fn grade_definitions_export_all_five_grades_with_confidence() {
    // The legend sheet renders from core data, not hardcoded Kotlin.
    let app = Engine;
    let model = Model::default();
    let defs = app.view(&model).grade_definitions;
    assert_eq!(defs.len(), 5, "five File-09 grades");
    let strong = defs
        .iter()
        .find(|d| d.grade == EvidenceGrade::Strong)
        .expect("Strong grade present");
    assert!(
        strong.definition.to_lowercase().contains("studies"),
        "Strong definition is plain-language KB meaning: {}",
        strong.definition
    );
    assert!((strong.confidence - 0.90).abs() < 1e-6);
    // MarketingMyth is named in the legend but never emitted on a card.
    assert!(defs.iter().any(|d| d.grade == EvidenceGrade::MarketingMyth));
}

#[test]
fn prescription_why_is_a_complete_three_part_disclosure() {
    // A prescription card must carry the same 3-part "why?" as adjustment /
    // guidance cards: basis → grade_note → improves, none empty. The
    // `improves` line is an ENGINE data-need (which input sharpens the load),
    // never a training claim (HARD RULE 1).

    // Word-level no-prescriptive-language guard, mirroring the HR-zones why
    // test. Bans specific exercise names + volume/intensity prescriptions.
    // ("set" singular / "rir" / "e1rm" are the engine's own honest vocabulary
    // on a lift card: a logged set and the RIR method are what the data-need
    // is *about*, not a prescription to train, so they are not banned; the
    // singular/plural split keeps "set" from tripping on "sets".)
    let assert_data_need = |improves: &str| {
        assert!(
            !improves.is_empty(),
            "improves must be populated (3-part complete): {improves}"
        );
        let banned = ["squat", "deadlift", "bench", "%1rm", "rpe", "reps", "sets"];
        let words: Vec<String> = improves
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric() && c != '%')
            .map(|w| w.to_string())
            .collect();
        for b in banned {
            assert!(
                !words.iter().any(|w| w == b),
                "improves must not prescribe training: {improves}"
            );
        }
    };

    let check = |it: &PrescriptionView| {
        assert!(
            !it.why.basis.is_empty(),
            "basis must state the method behind the call: {}",
            it.summary
        );
        assert!(
            !it.why.grade_note.is_empty(),
            "grade_note must gloss the evidence grade: {}",
            it.summary
        );
        assert_data_need(&it.why.improves);
    };

    // Anchored (%1RM × logged e1RM): the improves line is about keeping the
    // e1RM anchor fresh.
    let (app, model) = planned_strength_model(true);
    let ns = app.view(&model).next_session.expect("anchored plan");
    let sq = ns
        .items
        .iter()
        .find(|i| i.exercise.eq_ignore_ascii_case("Back Squat"))
        .expect("the user's anchored lift");
    assert!(sq.load_kg.is_some(), "this arm is the anchored case");
    check(sq);
    assert!(
        sq.why.improves.contains("estimated max"),
        "anchored improves points at the estimated-max anchor: {}",
        sq.why.improves
    );

    // RIR fallback (no e1RM anchor yet): the improves line is about logging a
    // set so loads can anchor to a measured e1RM.
    let (app, model) = planned_strength_model(false);
    let ns = app.view(&model).next_session.expect("RIR plan");
    let it = ns
        .items
        .iter()
        .find(|i| i.intensity_label.starts_with("RIR"))
        .expect("an unanchored RIR lift");
    assert!(it.load_kg.is_none(), "this arm is the unanchored case");
    check(it);
    assert!(
        it.why.improves.contains("measured max"),
        "RIR improves points at anchoring to a measured max: {}",
        it.why.improves
    );
}
