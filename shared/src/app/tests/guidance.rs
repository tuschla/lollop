//! Profile guidance and reference cards: modality sections, cited rows, human
//! copy, and no panic anywhere in the profile domain.

use super::*;

#[test]
fn profile_drives_evidence_cited_guidance() {
    let app = Engine;
    let mut model = Model::default();
    assert!(app.view(&model).guidance.is_empty());

    app.update(Event::SetProfile(sample_profile()), &mut model)
        .expect_only_render();

    let vm = app.view(&model);
    assert!(!vm.guidance.is_empty());
    // Every surfaced row carries a grade + citation; none is MarketingMyth.
    assert!(
        vm.guidance
            .iter()
            .all(|g| g.evidence.grade.is_some() && !g.evidence.citation.is_empty())
    );
    assert!(
        vm.guidance
            .iter()
            .all(|g| g.evidence.grade != Some(EvidenceGrade::MarketingMyth))
    );
    // The running-days=5, km=50 profile trips the hybrid lower-lift cap.
    assert!(
        vm.guidance
            .iter()
            .any(|g| g.summary.contains("leg lifting is kept to"))
    );
    // 50 km/wk is under the ~64 km bone-stress threshold, so no BSI row yet.
    assert!(
        !vm.guidance
            .iter()
            .any(|g| g.summary.contains("Bone stress:"))
    );
    // MaxStrength surfaces the 20% velocity-loss set cutoff (AUTOREG-VL-001),
    // phrased so it works without a velocity device.
    assert!(
        vm.guidance
            .iter()
            .any(|g| g.summary.contains("Set cutoff") && g.summary.contains("visibly slows"))
    );
    // 14 weekly sets/muscle (>12) trips the session-split guidance (HYP-FREQ-001).
    assert!(
        vm.guidance
            .iter()
            .any(|g| g.summary.contains("spread them over 2 or more sessions"))
    );
}

#[test]
fn power_guidance_row_states_kb_language_not_the_rir_number() {
    // The Power loading row is cited to STR-PWR-001 (Moderate). The numeric
    // RIR 3-5 band is an expert-opinion encoding (STR-PWR-RIR-001), so it
    // must NOT print under this row; the row states the KB's qualitative
    // "never to failure" instruction instead.
    let mut p = sample_profile();
    p.lift_goal = LiftGoal::Power;
    let rows = build_guidance(&p);
    let loading = rows
        .iter()
        .find(|r| r.summary.contains("Power loading:"))
        .expect("a Power loading guidance row exists");
    assert!(
        loading.summary.contains("never to failure"),
        "row states the KB power instruction: {}",
        loading.summary
    );
    assert!(
        !loading.summary.contains("RIR "),
        "no expert-opinion RIR digits under the STR-PWR-001 row: {}",
        loading.summary
    );
    // The load spectrum is stated per exercise type, never as an
    // unreadable "0-95%1RM" envelope.
    assert!(
        loading.summary.contains("jumps") && !loading.summary.contains(" 0–95%"),
        "{}",
        loading.summary
    );
}

#[test]
fn c25k_running_guidance_row_has_no_long_run_fragment() {
    // C25K carries no long-run share (running-025 is run/walk to 30 min),
    // so the Running guidance row must not print a "long run …%" fragment.
    // The goal distance renders as the human label "Couch to 5K", never
    // the raw Debug enum name "C25k".
    let mut p = sample_profile();
    p.goal_distance = GoalDistance::C25k;
    let rows = build_guidance(&p);
    let running = rows
        .iter()
        .find(|r| r.summary.starts_with("Couch to 5K:"))
        .expect("a C25K running guidance row exists");
    assert!(
        !running.summary.contains("long run"),
        "C25K has no long-run share, so no long-run fragment: {}",
        running.summary
    );
    assert!(
        running.summary.contains("3 runs a week, none of them hard"),
        "the C25K session budget still renders: {}",
        running.summary
    );
    // The raw Debug enum name never leaks into any guidance summary.
    assert!(
        !rows.iter().any(|r| r.summary.contains("C25k")),
        "no PascalCase enum name in the guidance copy"
    );
}

#[test]
fn guidance_copy_uses_human_labels_not_debug_enums() {
    // The periodization, goal-distance and same-session-order rows render
    // human labels, never the raw Debug enum names ("Dup"/"FiveK"/"RunFirst").
    let mut p = sample_profile(); // WeekToWeek cadence → DUP; EndurancePriority.
    p.goal_distance = GoalDistance::FiveK;
    let rows = build_guidance(&p);
    let any = |needle: &str| rows.iter().any(|r| r.summary.contains(needle));
    assert!(
        any("Periodization model: daily undulating (heavy, medium and light days each week)"),
        "periodization human label"
    );
    assert!(any("5K: "), "goal-distance human label in the running row");
    assert!(
        any("Combined days: running first is fine (endurance priority)."),
        "same-session-order full sentence"
    );
    // No PascalCase enum name leaks into any guidance summary.
    for bad in ["Dup", "FiveK", "RunFirst", "EndurancePriority"] {
        assert!(
            !any(bad),
            "raw Debug enum name '{bad}' leaked into guidance copy"
        );
    }
}

#[test]
fn high_mileage_profile_surfaces_safety_critical_bsi_surveillance() {
    let app = Engine;
    let mut model = Model::default();
    let mut p = sample_profile();
    p.running_km_per_week = 70.0; // over the ~64 km bone-stress threshold
    app.update(Event::SetProfile(p), &mut model)
        .expect_only_render();

    let bsi = app
        .view(&model)
        .guidance
        .into_iter()
        .find(|g| g.summary.contains("Bone stress:"))
        .expect("70 km/wk must raise BSI surveillance");
    // File 10 hybrid-023 is safety-critical and evidence-cited (HARD RULE 2/3).
    assert!(bsi.evidence.safety_critical);
    assert_eq!(bsi.section, "Safety");
    assert!(bsi.evidence.grade.is_some() && !bsi.evidence.citation.is_empty());
}

#[test]
fn novice_load_jump_shown_only_for_novices() {
    let has_jump = |cadence: ProgressionCadence| -> bool {
        let mut p = sample_profile();
        p.progression_cadence = cadence;
        build_guidance(&p)
            .iter()
            .any(|g| g.summary.contains("Adding weight as a beginner"))
    };
    assert!(has_jump(ProgressionCadence::EverySession)); // Novice
    assert!(!has_jump(ProgressionCadence::WeekToWeek)); // Intermediate
    assert!(!has_jump(ProgressionCadence::MonthToMonth)); // Advanced
}

#[test]
fn weekly_pct_increment_shown_only_past_novice() {
    // The complement of the novice fixed-kg jump: intermediate/advanced
    // lifters get the per-week percentage progression instead, so every
    // training age has increment guidance.
    let row = |cadence: ProgressionCadence| -> Option<GuidanceView> {
        let mut p = sample_profile();
        p.progression_cadence = cadence;
        build_guidance(&p)
            .into_iter()
            .find(|g| g.summary.starts_with("Adding weight: "))
    };
    assert!(row(ProgressionCadence::EverySession).is_none()); // Novice
    for cadence in [
        ProgressionCadence::WeekToWeek,
        ProgressionCadence::MonthToMonth,
    ] {
        let r = row(cadence).expect("non-novice should get weekly % increment");
        assert!(
            !r.evidence.citation.is_empty(),
            "increment row must be evidence-cited"
        );
        assert_ne!(r.evidence.grade, Some(EvidenceGrade::MarketingMyth));
        // Half-percent bounds must render faithfully, not round to a whole
        // number that understates the cited band (upper 1-2.5%, lower 2.5-5%).
        assert!(
            r.summary.contains("+1–2.5% on upper-body"),
            "upper band must keep its half-percent: {}",
            r.summary
        );
        assert!(
            r.summary.contains("+2.5–5% on lower-body"),
            "lower band must keep its half-percent: {}",
            r.summary
        );
    }
}

#[test]
fn hypertrophy_rep_load_prescription_is_surfaced_and_cited() {
    // The rep/load row answers "how many reps and what load": absent before
    // this wire. Profile-independent (applies to all lifting), so any profile
    // shows it, evidence-cited and non-myth, with the heavy-compound band.
    let r = build_guidance(&sample_profile())
        .into_iter()
        .find(|g| g.summary.starts_with("Reps and weight:"))
        .expect("rep/load prescription row must be present");
    assert!(
        !r.evidence.citation.is_empty(),
        "rep/load row must be evidence-cited"
    );
    assert_ne!(r.evidence.grade, Some(EvidenceGrade::MarketingMyth));
    assert!(
        r.summary.contains("big lifts 5–10 reps at 75–85% 1RM"),
        "heavy band must render: {}",
        r.summary
    );
}

#[test]
fn lift_only_profile_omits_running_and_hybrid_guidance() {
    // Modality is implicit: running = running_days_per_week > 0. A pure
    // lifter (no running volume) must not be shown Running-section rows nor
    // the Hybrid concurrent-training block (the same-session-order card).
    let mut p = sample_profile();
    p.running_days_per_week = 0;
    p.running_km_per_week = 0.0;
    let rows = build_guidance(&p);
    assert!(
        !rows.iter().any(|g| g.section == "Hybrid"),
        "pure lifter must not see any Hybrid-section row"
    );
    assert!(
        !rows
            .iter()
            .any(|g| g.summary.contains("same-session order")),
        "pure lifter must not see the same-session-order card"
    );
    assert!(
        !rows.iter().any(|g| g.section == "Running"),
        "pure lifter must not see any Running-section row"
    );
    // Sanity: they still get their Strength guidance.
    assert!(
        rows.iter().any(|g| g.section == "Strength"),
        "pure lifter must still see Strength rows"
    );
}

#[test]
fn run_only_profile_omits_strength_and_hybrid_guidance() {
    // Modality is implicit: lifting = weekly_sets > 0. A pure runner (no
    // lifting volume) must not be shown Strength-section rows nor the Hybrid
    // concurrent-training block.
    let mut p = sample_profile();
    p.weekly_sets = 0;
    let rows = build_guidance(&p);
    assert!(
        !rows.iter().any(|g| g.section == "Strength"),
        "pure runner must not see any Strength-section row"
    );
    assert!(
        !rows.iter().any(|g| g.section == "Hybrid"),
        "pure runner must not see any Hybrid-section row"
    );
    // Sanity: they still get their Running guidance.
    assert!(
        rows.iter().any(|g| g.section == "Running"),
        "pure runner must still see Running rows"
    );
}

#[test]
fn hybrid_profile_keeps_all_modality_sections() {
    // Guard against over-gating: the hybrid sample_profile (lifts and runs)
    // must retain Strength, Running and Hybrid rows all at once.
    let rows = build_guidance(&sample_profile());
    assert!(
        rows.iter().any(|g| g.section == "Strength"),
        "hybrid profile must keep Strength rows"
    );
    assert!(
        rows.iter().any(|g| g.section == "Running"),
        "hybrid profile must keep Running rows"
    );
    assert!(
        rows.iter().any(|g| g.section == "Hybrid"),
        "hybrid profile must keep Hybrid rows"
    );
}

#[test]
fn bsi_surveillance_reads_measured_not_stated_mileage() {
    // A runner set up at 30 km/wk who now logs ~70 km/wk must still trip the
    // bone-stress-injury surveillance guard (>~64 km); the row states the
    // measured basis.
    let app = Engine;
    let mut model = Model::default();
    let mut p = running_profile();
    p.running_km_per_week = 30.0;
    p.running_days_per_week = 4;
    app.update(Event::SetProfile(p), &mut model)
        .expect_only_render();
    let today = 400i64;
    for w in 0..4 {
        app.update(logrun(70.0, today - 7 * w), &mut model)
            .expect_only_render();
    }
    app.update(
        Event::SetToday {
            epoch_day: today,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    let bsi = app
        .view(&model)
        .guidance
        .into_iter()
        .find(|g| g.summary.contains("Bone stress:"))
        .expect("a 70 km/wk runner sees the BSI row despite a 30 km stated setup");
    assert!(
        bsi.summary.contains("from your logged runs"),
        "the row states the measured basis: {}",
        bsi.summary
    );
    assert!(
        bsi.summary_mi.contains(" mi a week from your logged runs")
            && bsi.summary_mi.contains("~40 mi"),
        "a miles twin restates the distances: {}",
        bsi.summary_mi
    );
}

#[test]
fn build_guidance_never_panics_across_the_profile_domain() {
    // Exhaustive smoke test: build_guidance branches heavily on the profile
    // and does arithmetic on its integer fields (weekly_sets, running days,
    // km). Sweep every enum combination against the boundary integers the
    // Android profile editor can emit (weekly_sets 0..=30, days 0..=7, km
    // 0..=150) and assert (a) no panic (underflow / div-by-zero) and (b) the
    // HARD RULE 2 invariant holds everywhere: every surfaced row is
    // evidence-cited and none is graded MarketingMyth.
    /// Every profile in `ps` crossed with every value of one field.
    fn cross<T: Copy>(ps: Vec<Profile>, values: &[T], set: fn(&mut Profile, T)) -> Vec<Profile> {
        ps.into_iter()
            .flat_map(|p| {
                values.iter().map(move |&v| {
                    let mut q = p.clone();
                    set(&mut q, v);
                    q
                })
            })
            .collect()
    }
    use ProgressionCadence as Pc;
    let mut profiles = vec![base_profile()];
    profiles = cross(
        profiles,
        &[Pc::EverySession, Pc::WeekToWeek, Pc::MonthToMonth],
        |p, v| p.progression_cadence = v,
    );
    profiles = cross(
        profiles,
        &[
            LiftGoal::MaxStrength,
            LiftGoal::Power,
            LiftGoal::Hypertrophy,
        ],
        |p, v| p.lift_goal = v,
    );
    profiles = cross(
        profiles,
        &[
            GoalDistance::General,
            GoalDistance::C25k,
            GoalDistance::FiveK,
            GoalDistance::TenK,
            GoalDistance::HalfMarathon,
            GoalDistance::Marathon,
        ],
        |p, v| p.goal_distance = v,
    );
    profiles = cross(
        profiles,
        &[
            ConcurrentGoal::Strength,
            ConcurrentGoal::Power,
            ConcurrentGoal::Hypertrophy,
            ConcurrentGoal::EndurancePriority,
        ],
        |p, v| p.concurrent_goal = v,
    );
    profiles = cross(profiles, &[0u8, 1, 30], |p, v| p.weekly_sets = v);
    profiles = cross(profiles, &[0u8, 1, 7], |p, v| p.running_days_per_week = v);
    profiles = cross(profiles, &[0.0, 0.1, 150.0], |p, v| {
        p.running_km_per_week = v
    });
    profiles = cross(profiles, &[false, true], |p, v| p.advanced = v);
    for p in &profiles {
        let rows = build_guidance(p);
        assert_cited_non_myth(&rows, p);
        assert_sections_contiguous(&rows, p);
    }
}

#[test]
fn build_reference_sections_are_contiguous_and_cited() {
    // The common (non-RED-S) reference is profile-independent, so one call
    // covers it. Same contract as the guidance list: every row evidence-cited, no
    // MarketingMyth, and each section forms a single contiguous run (the
    // reference list labels every card by section).
    let rows = build_reference(false);
    assert_cited_non_myth(&rows, &"reference");
    assert_sections_contiguous(&rows, &"reference");

    // Each newly-surfaced dormant calc must appear as its own card with a
    // non-empty citation. Match on a load-bearing substring of each summary.
    let card = |needle: &str| rows.iter().find(|r| r.summary.contains(needle));
    for needle in [
        "Olympic pulling derivatives",
        "RPE-anchored: top set",
        "Two-for-two rule",
        "Stall: if e1RM",
        "Double progression",
        "Age 65+ protein",
        "Protein while eating less",
        "Age 65+ per-meal protein",
        "Couch to 5K:",
        "Pyramidal split",
        "Polarized split",
        "To keep a fitness gain",
        "Joint pain at heavy weights",
        "To scale back, cut in this order",
        "To scale up, add in this order",
        "Home / minimal equipment",
        "Next steps name things you can control",
    ] {
        let row = card(needle).unwrap_or_else(|| panic!("missing reference card for {needle:?}"));
        assert!(
            !row.evidence.citation.is_empty(),
            "uncited reference card: {}",
            row.summary
        );
    }
}

#[test]
fn frequency_guidance_collapses_equal_bounds() {
    // A muscle whose peak set count lands in a fixed-frequency band (chest MRV
    // 22 → 3–3×/wk) must not print a degenerate "3–3×/wk" span; the per-session
    // spread (6–9) stays a real range.
    let (plan, _) = build_hypertrophy_plan(
        &HypertrophyPlanQuery {
            muscle: "chest".to_string(),
            weeks: 4,
            not_growing: false,
            recovering_easily: false,
        },
        None,
        None,
    );
    let peak = plan
        .iter()
        .find(|g| g.summary.starts_with("Peak frequency:"))
        .expect("peak frequency row");
    assert!(
        peak.summary.contains("3×/wk"),
        "expected collapsed frequency, got: {}",
        peak.summary
    );
    assert!(
        !peak.summary.contains("3–3"),
        "degenerate span not collapsed: {}",
        peak.summary
    );
    assert_eq!(fmt_u8_range(3, 3), "3");
    assert_eq!(fmt_u8_range(6, 9), "6–9");
}

#[test]
fn quality_caps_shown_only_when_the_athlete_runs() {
    let has_caps = |run_days: u8| -> bool {
        let mut p = sample_profile();
        p.running_days_per_week = run_days;
        build_guidance(&p)
            .iter()
            .any(|g| g.summary.contains("Hard runs: at most"))
    };
    assert!(has_caps(5));
    assert!(!has_caps(0));
}

#[test]
fn periodization_phase_plan_tracks_the_model_by_training_age() {
    // Cadence maps to training age → periodization model: EverySession→Novice
    // →Linear; MonthToMonth→Advanced→Block; WeekToWeek→Intermediate→DUP
    // (no phase table in the source).
    let summaries = |cadence: ProgressionCadence| -> Vec<String> {
        let mut p = sample_profile();
        p.progression_cadence = cadence;
        build_guidance(&p).into_iter().map(|g| g.summary).collect()
    };

    let linear = summaries(ProgressionCadence::EverySession);
    assert!(linear.iter().any(|s| s.contains("Linear Base: 67–75% 1RM")));
    assert!(
        linear
            .iter()
            .any(|s| s.contains("Linear Taper: keep the weights the same"))
    );
    assert!(!linear.iter().any(|s| s.starts_with("Block ")));

    let block = summaries(ProgressionCadence::MonthToMonth);
    assert!(
        block
            .iter()
            .any(|s| s.contains("Block Accumulation: 65–80% 1RM"))
    );
    assert!(
        block
            .iter()
            .any(|s| s.contains("Block Realization (peak): 1–3 reps"))
    );
    assert!(!block.iter().any(|s| s.starts_with("Linear ")));

    let dup = summaries(ProgressionCadence::WeekToWeek);
    assert!(
        !dup.iter()
            .any(|s| s.starts_with("Linear ") || s.starts_with("Block "))
    );
}

#[test]
fn peak_block_run_cap_is_a_reference_card_not_profile_guidance() {
    // The plan never builds a peak lifting block, so the CAP-2 card
    // is never shown as profile guidance; it stays in the reference library.
    let app = Engine;
    for goal in [
        LiftGoal::MaxStrength,
        LiftGoal::Power,
        LiftGoal::Hypertrophy,
    ] {
        let mut model = Model::default();
        let mut p = sample_profile();
        p.lift_goal = goal;
        app.update(Event::SetProfile(p), &mut model)
            .expect_only_render();
        let vm = app.view(&model);
        assert!(
            !vm.guidance
                .iter()
                .any(|g| g.summary.contains("peak strength")),
            "{goal:?}"
        );
        assert!(
            vm.reference
                .iter()
                .any(|g| g.summary.contains("peak strength"))
        );
    }
}

#[test]
fn lower_strength_interference_row_tracks_sex_and_running() {
    let app = Engine;
    let row_text = |female: Option<bool>, cadence: ProgressionCadence, run_days: u8| {
        let mut model = Model::default();
        let mut p = sample_profile();
        p.female = female;
        p.progression_cadence = cadence;
        p.running_days_per_week = run_days;
        app.update(Event::SetProfile(p), &mut model)
            .expect_only_render();
        app.view(&model)
            .guidance
            .into_iter()
            .find(|g| g.summary.contains("leg-strength gains"))
            .map(|g| g.summary)
    };
    // A man who runs: small leg-strength blunting possible, novice or not
    // (training status is not a reliable moderator, HYB-TRAINED-001).
    for cadence in [
        ProgressionCadence::WeekToWeek,
        ProgressionCadence::EverySession,
    ] {
        let t = row_text(Some(false), cadence, 5).expect("a hybrid shows the row");
        assert!(t.contains("can slightly blunt"), "{t}");
        assert!(!t.contains("spared"), "{t}");
    }
    // A woman: no blunting shown (HYB-SEX-001).
    assert!(
        row_text(Some(true), ProgressionCadence::WeekToWeek, 5)
            .expect("row")
            .contains("has not been shown")
    );
    // Pure lifter (no running) → row absent entirely.
    assert!(row_text(Some(false), ProgressionCadence::WeekToWeek, 0).is_none());
}

#[test]
fn hybrid_guidance_has_no_raw_true_false() {
    let app = Engine;
    for goal in [ConcurrentGoal::Strength, ConcurrentGoal::EndurancePriority] {
        let mut model = Model::default();
        let mut p = sample_profile();
        p.concurrent_goal = goal;
        app.update(Event::SetProfile(p), &mut model)
            .expect_only_render();
        for g in app.view(&model).guidance {
            assert!(
                !g.summary.contains(": true") && !g.summary.contains(": false"),
                "{}",
                g.summary
            );
            assert!(!g.summary.contains("spared"), "{}", g.summary);
        }
    }
}

#[test]
fn pure_runner_sees_no_endurance_frequency_cap_row_but_a_strength_offer() {
    let app = Engine;
    let mut model = Model::default();
    let mut p = sample_profile();
    p.weekly_sets = 0; // run only
    app.update(Event::SetProfile(p.clone()), &mut model)
        .expect_only_render();
    let vm = app.view(&model);
    assert!(
        !vm.guidance
            .iter()
            .any(|g| g.summary.contains("3-day limit")
                || g.summary.contains("endurance-frequency")),
        "a run-only athlete is never judged by the hybrid run-day default"
    );
    let offer = vm
        .guidance
        .iter()
        .find(|g| g.summary.starts_with("Add 2 short strength sessions"))
        .expect("run-only athletes are offered strength work");
    assert_eq!(offer.evidence.grade, Some(EvidenceGrade::Moderate));

    // Opted in: the row describes the scheduled sessions instead.
    p.runner_strength = true;
    app.update(Event::SetProfile(p), &mut model)
        .expect_only_render();
    let vm = app.view(&model);
    assert!(vm.guidance.iter().any(|g| {
        g.summary
            .starts_with("Your plan includes 2 short strength sessions")
    }));
}

#[test]
fn interference_row_reads_the_plans_own_run_days() {
    // A lifting-led hybrid asking for 5 run days is scheduled 3, so
    // the frequency arm reads 3 (not the stated 5).
    let app = Engine;
    let mut model = Model::default();
    let mut p = sample_profile();
    p.concurrent_goal = ConcurrentGoal::Strength;
    app.update(Event::SetProfile(p), &mut model)
        .expect_only_render();
    let vm = app.view(&model);
    let row = vm
        .guidance
        .iter()
        .find(|g| g.summary.contains("run days a week, hardest sessions"))
        .expect("interference row");
    assert!(
        row.summary.contains("(3 run days a week"),
        "{}",
        row.summary
    );
}

#[test]
fn newly_wired_guidance_rows_are_present_and_cited() {
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(sample_profile()), &mut model)
        .expect_only_render();
    let vm = app.view(&model);

    // Each wired dormant calc must surface a row, and every row still carries
    // an evidence grade + citation (HARD RULE 2, no bypasses).
    for needle in [
        "Fewest sets that still build muscle",
        "Added sets",
        "Lighter weeks: after every",
        "Early in a plan",
        "Prilepin chart at ~",
        "Weekly distance grows by at most",
    ] {
        let row = vm
            .guidance
            .iter()
            .find(|g| g.summary.contains(needle))
            .unwrap_or_else(|| panic!("missing guidance row: {needle}"));
        assert!(row.evidence.grade.is_some() && !row.evidence.citation.is_empty());
        assert_ne!(row.evidence.grade, Some(EvidenceGrade::MarketingMyth));
    }

    // The Prilepin row must resolve to its registered claim (Moderate grade,
    // Prilepin citation), not the unregistered-id fallback.
    let prilepin = vm
        .guidance
        .iter()
        .find(|g| g.summary.contains("Prilepin chart at ~"))
        .unwrap();
    assert_eq!(prilepin.evidence.grade, Some(EvidenceGrade::Moderate));
    assert!(
        prilepin.evidence.citation.contains("Prilepin"),
        "{}",
        prilepin.evidence.citation
    );
    assert!(
        prilepin.evidence.contested,
        "STR-PRILEPIN-001 is contested (CQ-F02-03)"
    );

    // WeekToWeek cadence → Intermediate age → not conservative → 3:1 cadence.
    assert!(
        vm.guidance
            .iter()
            .any(|g| g.summary.contains("after every 3 weeks of building"))
    );
}

#[test]
fn novice_profile_gets_conservative_running_deload() {
    let app = Engine;
    let mut model = Model::default();
    let mut p = sample_profile();
    p.progression_cadence = ProgressionCadence::EverySession; // → Novice
    app.update(Event::SetProfile(p), &mut model)
        .expect_only_render();
    let vm = app.view(&model);
    assert!(
        vm.guidance
            .iter()
            .any(|g| g.summary.contains("after every 2 weeks of building")),
        "novice (low training age) should get the conservative 2:1 cadence"
    );
}

#[test]
fn power_goal_surfaces_cited_plyo_cap() {
    let app = Engine;
    let mut model = Model::default();
    let mut p = sample_profile();
    p.lift_goal = LiftGoal::Power;
    app.update(Event::SetProfile(p), &mut model)
        .expect_only_render();
    let vm = app.view(&model);
    let plyo = vm
        .guidance
        .iter()
        .find(|g| g.summary.contains("foot contacts a session"))
        .expect("Power goal should surface the plyometric foot-contact cap row");
    assert!(!plyo.evidence.citation.is_empty());
}

#[test]
fn non_power_goal_omits_plyo_cap() {
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(sample_profile()), &mut model)
        .expect_only_render();
    assert!(
        !app.view(&model)
            .guidance
            .iter()
            .any(|g| g.summary.contains("foot contacts a session")),
        "MaxStrength goal should not surface the plyo cap"
    );
}

#[test]
fn clearing_profile_empties_guidance() {
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(sample_profile()), &mut model)
        .expect_only_render();
    app.update(Event::ClearProfile, &mut model)
        .expect_only_render();
    assert!(app.view(&model).guidance.is_empty());
}

#[test]
fn reference_cards_are_always_present_and_evidence_cited() {
    let app = Engine;
    let model = Model::default();
    let vm = app.view(&model);
    assert!(!vm.reference.is_empty());
    assert!(vm.reference.iter().all(|g| !g.evidence.citation.is_empty()
        && g.evidence.grade != Some(EvidenceGrade::MarketingMyth)));
}

#[test]
fn smith_machine_squat_does_not_clear_depth_jump_gate() {
    // Equipment-stripping merges "Smith machine back squat" onto "Barbell back squat",
    // but a guided-bar squat is not a free-barbell 1RM: the raw name's
    // "smith"/"machine" token bars it from the plyometric readiness gate. With a
    // 160 kg bodyweight the gate (240 kg) would be unmet, and say so, if it counted.
    let app = Engine;
    let mut model = Model::default();
    let mut p = strength_profile();
    p.bodyweight_kg = Some(160.0);
    app.update(Event::SetProfile(p), &mut model)
        .expect_only_render();
    app.update(
        log_set("Smith machine back squat", 200.0, 3, 8.0, 100 * WEEK),
        &mut model,
    )
    .expect_only_render();
    let audit = app.view(&model).lift_audit;
    assert!(
        !audit.iter().any(|r| r.summary.contains("depth jumps")),
        "a Smith-machine squat must not anchor the free-barbell plyometric gate: {:?}",
        audit.iter().map(|r| &r.summary).collect::<Vec<_>>()
    );
}

#[test]
fn guidance_carries_environment_reentry_and_run_rx_rows() {
    let app = Engine;
    let mut model = Model::default();
    let mut p = sample_profile();
    p.environment = Some(Environment::Heat);
    p.env_temp_c = Some(30.0);
    p.weeks_off = Some(6.0);
    app.update(Event::SetProfile(p), &mut model)
        .expect_only_render();
    let g = app.view(&model).guidance;
    let has = |needle: &str| g.iter().any(|r| r.summary.contains(needle));
    assert!(has("signs of heat illness"), "heat modifier row");
    assert!(has("In heat (above"), "running-041 trigger row");
    assert!(
        has("restart at ~70% of your old weights"),
        "reentry row: 6 wk off"
    );
    assert!(has("fewer sets regrow muscle"), "layoff MEV row");
    assert!(has("Easy runs:"), "easy Rx row");
    assert!(has("Cruise intervals"), "cruise Rx row");
    assert!(has("Hard intervals (interval pace)"), "vo2 Rx row");
    assert!(has("Strides:"), "strides row");
    assert!(has("Lighter week:"), "lighter week row");
    assert!(has("Race taper (Marathon)"), "distance taper row");
    assert!(
        has("raise either your distance or your effort"),
        "single-variable rule row"
    );
    assert!(has("Long runs:"), "long-run Rx row");
    assert!(has("Test a true 1RM only when"), "1RM-test gate row");
    // The heat-stop row is safety-critical, per ENV-001.
    let heat = g
        .iter()
        .find(|r| r.summary.contains("signs of heat illness"))
        .unwrap();
    assert!(heat.evidence.safety_critical);
}
