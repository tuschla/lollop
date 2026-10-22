//! Coach calculators end to end: race prediction, HR zones, protein, critical
//! speed, Cooper, APRE and the hypertrophy meso planner; each is graded, echoes
//! its inputs and refuses degenerate queries.

use super::*;

#[test]
fn predict_race_produces_graded_two_method_estimate() {
    let app = Engine;
    let mut model = Model::default();
    assert!(app.view(&model).race_prediction.is_none());

    // 5 km in 20:00, predict a 10 km at ~40 km/wk.
    app.update(
        Event::PredictRace {
            recent_distance_m: 5000.0,
            recent_time_sec: 1200.0,
            goal_distance_m: 10_000.0,
            weekly_km: 40.0,
            weeks_since_race: None,
        },
        &mut model,
    )
    .expect_only_render();

    let pred = app
        .view(&model)
        .race_prediction
        .expect("a prediction must be present after PredictRace");
    assert_eq!(pred.goal_label, "10K");
    // Carries a real grade + citation (evidence discipline).
    assert!(pred.evidence.grade.is_some() && !pred.evidence.citation.is_empty());
    assert_ne!(pred.evidence.grade, Some(EvidenceGrade::MarketingMyth));
    // A 20:00 5K projects a 10K well over 40:00 (both methods, endurance fade).
    assert!(
        pred.low_sec > 2400.0,
        "10K should exceed 40:00, got {}",
        pred.low_sec
    );
    assert!(pred.high_sec >= pred.low_sec);
    assert!(
        pred.predicted.contains(':'),
        "clock-formatted, got {}",
        pred.predicted
    );

    // Clearing drops the section.
    app.update(Event::ClearRacePrediction, &mut model)
        .expect_only_render();
    assert!(app.view(&model).race_prediction.is_none());
}

#[test]
fn predict_race_with_degenerate_input_surfaces_no_false_time() {
    // A zero recent time is undefined for both Riegel and VDOT; the view must
    // say so rather than fabricate a finish clock (evidence discipline).
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::PredictRace {
            recent_distance_m: 5000.0,
            recent_time_sec: 0.0,
            goal_distance_m: 10_000.0,
            weekly_km: 40.0,
            weeks_since_race: None,
        },
        &mut model,
    )
    .expect_only_render();

    let pred = app
        .view(&model)
        .race_prediction
        .expect("the section is present, just with no numeric estimate");
    assert_eq!(pred.predicted, "-");
    assert_eq!(pred.low_sec, 0.0);
    assert!(!pred.agreed);
    assert!(
        pred.summary.contains("enter a recent race"),
        "degenerate summary should explain the missing input, got {}",
        pred.summary
    );
    assert!(
        !pred.predicted.contains(':'),
        "must not render a clock time for undefined input"
    );
}

#[test]
fn plan_hypertrophy_meso_produces_graded_rows() {
    let app = Engine;
    let mut model = Model::default();
    assert!(app.view(&model).hypertrophy_plan.is_empty());

    // A known muscle + 4 accumulation weeks yields several graded rows.
    app.update(
        Event::PlanHypertrophyMeso {
            muscle: "chest".into(),
            weeks: 4,
            not_growing: false,
            recovering_easily: false,
        },
        &mut model,
    )
    .expect_only_render();

    let view = app.view(&model);
    let plan = view.hypertrophy_plan;
    assert!(plan.len() >= 3, "expected several rows, got {}", plan.len());
    // Every row is graded and never a hard-blocked myth (evidence discipline).
    for row in &plan {
        assert!(row.evidence.grade.is_some() && !row.evidence.citation.is_empty());
        assert_ne!(row.evidence.grade, Some(EvidenceGrade::MarketingMyth));
    }
    // The volume-ramp row is present (chest MEV 10 → MRV 22).
    assert!(
        plan.iter().any(|r| r.summary.contains("Weekly set ramp:")),
        "ramp row missing"
    );
    // The structured ramp is exactly the ramp row's numbers (no profile → opens
    // at chest MEV 10), so a compact summary can never disagree with the card.
    let ramp = view.hypertrophy_ramp;
    assert_eq!(ramp.len(), 4);
    assert_eq!(ramp[0], 10);
    let ramp_str = ramp
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(" → ");
    assert!(
        plan.iter().any(|r| r.summary.contains(&ramp_str)),
        "ramp field {ramp_str} not shown in the ramp row"
    );

    // The structured figures stand for the landmark, ramp, reps-left,
    // frequency, rest and shape rows; every other row stays a note.
    let fig = view.hypertrophy_figures.expect("figures with a ramp");
    assert_eq!(
        (
            fig.min_sets,
            fig.productive_low,
            fig.productive_high,
            fig.max_sets
        ),
        (10, 12, 20, 22)
    );
    assert_eq!(fig.rir_by_week.len(), 4);
    let face = |prefix: &str| {
        fig.face_rows
            .iter()
            .any(|&i| plan[i as usize].summary.starts_with(prefix))
    };
    for prefix in [
        "Minimum",
        "Weekly set ramp:",
        "Reps left by week:",
        "Peak frequency:",
        "Rest between sets",
        "Mesocycle shape:",
    ] {
        assert!(face(prefix), "{prefix} row is not a face row");
    }
    assert_eq!(fig.face_rows.len(), 7);

    // Clearing empties the section.
    app.update(Event::ClearHypertrophyPlan, &mut model)
        .expect_only_render();
    assert!(app.view(&model).hypertrophy_plan.is_empty());
    assert!(app.view(&model).hypertrophy_ramp.is_empty());

    // An unknown muscle yields a single explanatory row (no fake data).
    app.update(
        Event::PlanHypertrophyMeso {
            muscle: "tail".into(),
            weeks: 4,
            not_growing: false,
            recovering_easily: false,
        },
        &mut model,
    )
    .expect_only_render();
    let unknown_view = app.view(&model);
    assert!(unknown_view.hypertrophy_ramp.is_empty());
    let unknown = unknown_view.hypertrophy_plan;
    assert_eq!(unknown.len(), 1, "unknown muscle should be one row");
    assert!(unknown[0].summary.contains("not a known muscle"));

    // weeks == 0 is degenerate: show the landmarks context only, not an
    // empty ramp/RIR schedule.
    app.update(
        Event::PlanHypertrophyMeso {
            muscle: "chest".into(),
            weeks: 0,
            not_growing: false,
            recovering_easily: false,
        },
        &mut model,
    )
    .expect_only_render();
    let zero_view = app.view(&model);
    assert!(zero_view.hypertrophy_ramp.is_empty());
    let zero = zero_view.hypertrophy_plan;
    assert_eq!(zero.len(), 1, "zero-week plan is landmarks-only");
    assert!(zero[0].summary.contains("Minimum 10 · productive"));
    assert!(
        !zero.iter().any(|r| r.summary.contains("ramp")),
        "no ramp row without a week count"
    );
}

#[test]
fn compute_protein_scales_graded_targets_by_bodyweight() {
    let app = Engine;
    let mut model = Model::default();
    assert!(app.view(&model).protein_targets.is_empty());

    // Deficit context at 80 kg: 1.8–2.7 g/kg → 144–216 g/day, still graded.
    app.update(
        Event::ComputeProtein {
            bodyweight_kg: 80.0,
            masters: false,
            deficit: true,
        },
        &mut model,
    )
    .expect_only_render();

    let targets = app.view(&model).protein_targets;
    assert_eq!(targets.len(), 1, "one selected context → one row");
    let row = &targets[0];
    assert!(
        row.summary.contains("g/day"),
        "must state an absolute target"
    );
    assert!(
        row.evidence.grade.is_some() && row.evidence.grade != Some(EvidenceGrade::MarketingMyth),
        "row must carry real evidence"
    );
    // The absolute bounds scale from bodyweight × each graded g/kg bound.
    assert!(
        row.summary.contains("144") && row.summary.contains("216"),
        "80 kg × (1.8, 2.7) should surface 144–216 g/day, got {}",
        row.summary
    );

    // Neither context selected → no fabricated general target (HARD RULE 1).
    app.update(
        Event::ComputeProtein {
            bodyweight_kg: 80.0,
            masters: false,
            deficit: false,
        },
        &mut model,
    )
    .expect_only_render();
    assert!(app.view(&model).protein_targets.is_empty());

    // Clearing empties the section.
    app.update(
        Event::ComputeProtein {
            bodyweight_kg: 80.0,
            masters: true,
            deficit: false,
        },
        &mut model,
    )
    .expect_only_render();
    assert!(!app.view(&model).protein_targets.is_empty());
    app.update(Event::ClearProtein, &mut model)
        .expect_only_render();
    assert!(app.view(&model).protein_targets.is_empty());
}

#[test]
fn compute_protein_handles_both_flags_and_rejects_bad_weight() {
    let app = Engine;
    let mut model = Model::default();

    // Both contexts selected → one graded row each (masters + deficit).
    app.update(
        Event::ComputeProtein {
            bodyweight_kg: 70.0,
            masters: true,
            deficit: true,
        },
        &mut model,
    )
    .expect_only_render();
    let targets = app.view(&model).protein_targets;
    assert_eq!(targets.len(), 2, "both contexts → two rows");
    assert!(targets.iter().all(|r| r.summary.contains("g/day")));
    assert!(
        targets.iter().all(|r| r.evidence.grade.is_some()),
        "every context row stays evidence-graded"
    );

    // A non-positive bodyweight cannot yield a g/day figure: emit nothing
    // rather than a zero or nonsensical target.
    app.update(
        Event::ComputeProtein {
            bodyweight_kg: 0.0,
            masters: true,
            deficit: true,
        },
        &mut model,
    )
    .expect_only_render();
    assert!(
        app.view(&model).protein_targets.is_empty(),
        "non-positive bodyweight must surface no target"
    );
}

#[test]
fn compute_hr_zones_builds_graded_hrmax_and_five_bands() {
    let app = Engine;
    let mut model = Model::default();
    assert!(app.view(&model).hr_zones.is_empty());

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

    let zones = app.view(&model).hr_zones;
    // HRmax header + 5 Daniels bands + the MAF base-phase option row.
    assert_eq!(zones.len(), 7, "one HRmax row + five band rows + MAF cap");
    assert!(
        zones.last().unwrap().summary.contains("MAF aerobic cap"),
        "MAF row closes the table: {}",
        zones.last().unwrap().summary
    );

    // Tanaka at 30: 208 − 0.7·30 = 187 bpm.
    let hrmax_row = &zones[0];
    assert!(
        hrmax_row.summary.contains("187 bpm"),
        "{}",
        hrmax_row.summary
    );

    // Easy band is 65–79 %HRmax → 122–148 bpm at HRmax 187.
    let easy = zones
        .iter()
        .find(|z| z.summary.starts_with("Easy"))
        .expect("Easy band row present");
    assert!(easy.summary.contains("122–148 bpm"), "{}", easy.summary);

    // The structured bands mirror the band rows: five, in order, each pointing
    // at its own row, the fast bands pace-governed.
    let bands = app.view(&model).hr_zone_bands;
    assert_eq!(bands.len(), 5);
    for b in &bands {
        assert!(zones[b.row as usize].summary.starts_with(&b.name), "{b:?}");
    }
    assert_eq!(
        (
            bands[0].pct_low,
            bands[0].pct_high,
            bands[0].bpm_low,
            bands[0].bpm_high
        ),
        (65.0, 79.0, 122.0, 148.0)
    );
    assert!(!bands[0].pace_governed && bands[3].pace_governed && bands[4].pace_governed);
    {
        use crate::schema::VdotBand::*;
        assert_eq!(
            bands.iter().map(|b| b.kind).collect::<Vec<_>>(),
            [Easy, Marathon, Threshold, Interval, Repetition]
        );
    }

    // Every row is evidence-graded, never a myth (HARD RULE 2).
    for z in &zones {
        assert!(z.evidence.grade.is_some() && !z.evidence.citation.is_empty());
        assert_ne!(z.evidence.grade, Some(EvidenceGrade::MarketingMyth));
    }

    // Clear empties the section.
    app.update(Event::ClearHrZones, &mut model)
        .expect_only_render();
    assert!(app.view(&model).hr_zones.is_empty());
}

#[test]
fn compute_hr_zones_rejects_implausible_age() {
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::ComputeHrZones {
            age_years: 0.0,
            resting_hr_bpm: None,
            weeks_since_recalc: None,
            weeks_since_pace_test: None,
        },
        &mut model,
    )
    .expect_only_render();
    let zones = app.view(&model).hr_zones;
    assert_eq!(zones.len(), 1, "bad age yields a single explanatory row");
    assert!(zones[0].summary.contains("between 5 and 100"));
}

#[test]
fn coach_query_builders_never_panic_on_degenerate_inputs() {
    // The four Coach tool builders take free-form numeric/string queries from
    // the shell (a text field the user can leave empty, zero, or negative).
    // Each guards its own degenerate domain; sweep the boundaries and assert
    // (a) no panic (div-by-zero / NaN-format / underflow) and (b) HARD RULE 2:
    // every surfaced row is evidence-cited and none is graded MarketingMyth.
    let cited_non_myth = |rows: &[GuidanceView]| assert_cited_non_myth(rows, &"coach query");

    // Race predictor: zero/negative distances and times must degrade to the
    // "need a valid recent race" summary, never a NaN clock or panic.
    for &d in &[0.0, -1.0, 5000.0] {
        for &t in &[0.0, -1.0, 1200.0] {
            for &g in &[0.0, -1.0, 10_000.0] {
                for &wk in &[0.0, -5.0, 60.0] {
                    let v = to_race_view(
                        &RaceQuery {
                            recent_distance_m: d,
                            recent_time_sec: t,
                            goal_distance_m: g,
                            weekly_km: wk,
                            weeks_since_race: None,
                        },
                        None,
                    );
                    assert!(
                        !v.predicted.contains("NaN"),
                        "NaN clock for d={d} t={t} g={g}"
                    );
                    assert!(!v.evidence.citation.is_empty(), "uncited race view");
                }
            }
        }
    }

    // Hypertrophy plan: unknown muscle + zero/huge week counts.
    for muscle in ["", "chest", "not-a-muscle"] {
        for &weeks in &[0u8, 1, 12, 255] {
            cited_non_myth(
                &build_hypertrophy_plan(
                    &HypertrophyPlanQuery {
                        muscle: muscle.to_string(),
                        weeks,
                        not_growing: false,
                        recovering_easily: false,
                    },
                    None,
                    None,
                )
                .0,
            );
        }
    }

    // Protein: non-positive bodyweight yields no rows; positive scales
    // cleanly, with and without the RED-S refusal path (safety-022).
    for &bw in &[0.0, -70.0, 70.0, 1e6] {
        for &m in &[false, true] {
            for &d in &[false, true] {
                for &reds in &[false, true] {
                    for &lb in &[false, true] {
                        cited_non_myth(
                            &build_protein_targets(
                                &ProteinQuery {
                                    bodyweight_kg: bw,
                                    masters: m,
                                    deficit: d,
                                },
                                reds,
                                lb,
                            )
                            .0,
                        );
                    }
                }
            }
        }
    }

    // HR zones: ages outside 5..=100 yield the explanatory row, never a bogus
    // HRmax; in-range ages produce cited bands.
    for &age in &[-1.0, 0.0, 4.9, 5.0, 30.0, 100.0, 100.1, 200.0] {
        cited_non_myth(
            &build_hr_zones(
                &HrZoneQuery {
                    age_years: age,
                    resting_hr_bpm: None,
                    weeks_since_recalc: None,
                    weeks_since_pace_test: None,
                },
                None,
            )
            .0,
        );
    }
}

#[test]
fn cooper_calculator_estimates_vo2max_and_refuses_the_floor() {
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::ComputeCooper {
            distance_m_12min: 2400.0,
        },
        &mut model,
    )
    .expect_only_render();
    let rows = app.view(&model).cooper;
    assert_eq!(rows.len(), 1);
    // (2400 − 504.9)/44.73 ≈ 42.4 ml/kg/min.
    assert!(rows[0].summary.contains("42.4"), "{}", rows[0].summary);
    assert_eq!(rows[0].evidence.grade, Some(EvidenceGrade::Moderate));

    app.update(
        Event::ComputeCooper {
            distance_m_12min: 400.0,
        },
        &mut model,
    )
    .expect_only_render();
    let rows = app.view(&model).cooper;
    assert!(rows[0].summary.contains("too short"), "{}", rows[0].summary);

    app.update(Event::ClearCooper, &mut model)
        .expect_only_render();
    assert!(app.view(&model).cooper.is_empty());
}

#[test]
fn critical_speed_calculator_fits_the_protocol_and_names_violations() {
    let app = Engine;
    let mut model = Model::default();
    // Ideal pair: 1200 m / 3 min + 5000 m / 20 min.
    app.update(
        Event::ComputeCriticalSpeed {
            efforts: vec![
                CsEffortIn {
                    distance_m: 1200.0,
                    time_sec: 180.0,
                },
                CsEffortIn {
                    distance_m: 5000.0,
                    time_sec: 1200.0,
                },
            ],
        },
        &mut model,
    )
    .expect_only_render();
    let rows = app.view(&model).critical_speed;
    assert_eq!(rows.len(), 1, "ideal pairing → no protocol note: {rows:#?}");
    // CS = 3800/1020 ≈ 3.73 m/s, D′ = 1200 − 3.73·180 ≈ 529 m.
    assert!(rows[0].summary.contains("3.73"), "{}", rows[0].summary);
    assert!(rows[0].summary.contains("529"), "{}", rows[0].summary);

    // A single effort violates the 2-effort minimum, explained not faked.
    app.update(
        Event::ComputeCriticalSpeed {
            efforts: vec![CsEffortIn {
                distance_m: 1200.0,
                time_sec: 180.0,
            }],
        },
        &mut model,
    )
    .expect_only_render();
    let rows = app.view(&model).critical_speed;
    assert!(
        rows[0].summary.contains("at least 2"),
        "{}",
        rows[0].summary
    );

    app.update(Event::ClearCriticalSpeed, &mut model)
        .expect_only_render();
    assert!(app.view(&model).critical_speed.is_empty());
}

#[test]
fn apre_calculator_applies_the_small_lifter_cap() {
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::ComputeApre {
            scheme: autoreg::ApreScheme::Apre6,
            reps: 9,
            current_load_kg: 50.0,
        },
        &mut model,
    )
    .expect_only_render();
    let rows = app.view(&model).apre;
    assert!(
        rows[0].summary.contains("+2.5 to +5 kg"),
        "{}",
        rows[0].summary
    );

    // Small lifter: the +kg band shrinks proportionally under the 50 kg ref.
    app.update(
        Event::ComputeApre {
            scheme: autoreg::ApreScheme::Apre6,
            reps: 9,
            current_load_kg: 30.0,
        },
        &mut model,
    )
    .expect_only_render();
    let rows = app.view(&model).apre;
    assert!(
        rows[0].summary.contains("+1.5 to +3 kg"),
        "{}",
        rows[0].summary
    );

    app.update(Event::ClearApre, &mut model)
        .expect_only_render();
    assert!(app.view(&model).apre.is_empty());
}

#[test]
fn apre_for_a_lb_profile_shows_the_source_pound_increments() {
    // Mann 2010 states APRE in lb: APRE-6 at 9 reps is +5 to +10 lb, never a
    // 2.5 kg plate converted back to "+5.5 lb". Under 100 lb the same
    // autoreg-019 cap applies in lb (+10 lb on 100 lb = +10 %).
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::SetProfile(Profile {
            weight_lb: true,
            ..sample_profile()
        }),
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::ComputeApre {
            scheme: autoreg::ApreScheme::Apre6,
            reps: 9,
            current_load_kg: 185.0 / LB_PER_KG,
        },
        &mut model,
    )
    .expect_only_render();
    let s = &app.view(&model).apre[0].summary;
    assert!(s.contains("+5 to +10 lb"), "{s}");
    assert!(!s.contains("5.5"), "{s}");
    app.update(
        Event::ComputeApre {
            scheme: autoreg::ApreScheme::Apre6,
            reps: 9,
            current_load_kg: 60.0 / LB_PER_KG,
        },
        &mut model,
    )
    .expect_only_render();
    let s = &app.view(&model).apre[0].summary;
    assert!(s.contains("+3 to +6 lb"), "{s}");
}

#[test]
fn hr_zone_table_extends_with_karvonen_maf_and_recalc_cues() {
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::ComputeHrZones {
            age_years: 30.0,
            resting_hr_bpm: Some(50.0),
            weeks_since_recalc: Some(5),
            weeks_since_pace_test: Some(6),
        },
        &mut model,
    )
    .expect_only_render();
    let zones = app.view(&model).hr_zones;
    let has = |needle: &str| zones.iter().any(|z| z.summary.contains(needle));
    assert!(
        has("heart-rate-reserve range (Karvonen)"),
        "preference row: {zones:#?}"
    );
    // Easy band: %HRR ≈ %VO2max (59–74%), so Karvonen at HRmax 187 / RHR 50 →
    // 131–151 bpm (NOT the %HRmax 65–79% fractions, which read ~8 bpm hotter).
    let easy = zones
        .iter()
        .find(|z| z.summary.starts_with("Easy"))
        .expect("easy row");
    assert!(
        easy.summary.contains("Karvonen 131–151 bpm"),
        "{}",
        easy.summary
    );
    // Repetition is pace-governed: no HR validity claimed.
    let rep = zones
        .iter()
        .find(|z| z.summary.starts_with("Repetition"))
        .expect("repetition row");
    assert!(
        rep.summary.contains("go by pace, not heart rate"),
        "{}",
        rep.summary
    );
    assert!(has("MAF aerobic cap"), "MAF row");
    assert!(has("Recompute from a measured HRmax"), "recalc-due row");
    assert!(has("Re-test to set paces"), "pace-retest row");
}

#[test]
fn hypertrophy_plan_decides_the_next_meso_and_states_effort_rules() {
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(sample_profile()), &mut model)
        .expect_only_render();
    app.update(
        Event::PlanHypertrophyMeso {
            muscle: "chest".into(),
            weeks: 4,
            not_growing: true,
            recovering_easily: true,
        },
        &mut model,
    )
    .expect_only_render();
    let plan = app.view(&model).hypertrophy_plan;
    let has = |needle: &str| plan.iter().any(|r| r.summary.contains(needle));
    // sample_profile plans 14 weekly sets → +2 next block.
    assert!(has("raise next mesocycle to 16 sets/wk"), "{plan:#?}");
    assert!(has("Rest between sets (heavy compounds)"), "rest row");
    assert!(has("Mesocycle shape"), "meso structure row");
    assert!(
        has("Stop most sets with 1–3 reps left"),
        "default effort band"
    );
    assert!(has("only on machines/isolation"), "failure gate");
    assert!(has("30–85% 1RM"), "load interchangeability");
    assert!(has("Tempo: controlled"), "tempo row");
}

#[test]
fn race_prediction_notes_flag_stale_input_and_marathon_optimism() {
    let app = Engine;
    let mut model = Model::default();
    // Longest logged run: 20 km, under the 30 km marathon-support line.
    app.update(log_run_at(20.0, 110.0, 75.0, 100 * WEEK), &mut model)
        .expect_only_render();
    app.update(
        Event::PredictRace {
            recent_distance_m: 10_000.0,
            recent_time_sec: 2_520.0,
            goal_distance_m: 42_195.0,
            weekly_km: 50.0,
            weeks_since_race: Some(10),
        },
        &mut model,
    )
    .expect_only_render();
    let pred = app.view(&model).race_prediction.expect("prediction");
    assert_eq!(pred.notes.len(), 2, "stale + optimism: {:#?}", pred.notes);
    assert!(
        pred.notes
            .iter()
            .any(|n| n.summary.contains("Race or time-trial again"))
    );
    assert!(
        pred.notes.iter().any(
            |n| n.summary.contains("marathon predictions come out too fast")
                && n.summary.contains("about 2–3 VDOT points")
                && n.summary.contains("20.0 km")
                && n.summary_mi.contains("12.4 mi")
                && !n.summary_mi.contains("km")
        ),
        "optimism note carries the running-008 derate and a miles twin: {:#?}",
        pred.notes
    );

    // A fresh input race against a 5K goal carries no notes.
    app.update(
        Event::PredictRace {
            recent_distance_m: 10_000.0,
            recent_time_sec: 2_520.0,
            goal_distance_m: 5_000.0,
            weekly_km: 50.0,
            weeks_since_race: Some(2),
        },
        &mut model,
    )
    .expect_only_render();
    assert!(
        app.view(&model)
            .race_prediction
            .expect("prediction")
            .notes
            .is_empty()
    );
}

#[test]
fn marathon_under_mileage_flags_and_derates_the_displayed_band() {
    // running-040/008 option B ("flag AND derate"): an under-mileaged
    // marathon prediction's SHOWN band must move slower, not just carry a
    // caveat. The base band depends only on the recent race/goal, not on
    // the longest logged run, so comparing an under-mileaged runner (20 km)
    // to a well-supported one (32 km) isolates exactly the derate.
    let marathon = RaceQuery {
        recent_distance_m: 10_000.0,
        recent_time_sec: 2_520.0, // 42:00 10K
        goal_distance_m: 42_195.0,
        weekly_km: 50.0,
        weeks_since_race: Some(2), // fresh → no freshness note in the way
    };
    let supported = to_race_view(&marathon, Some(32.0)); // ≥30 km → no derate
    let under = to_race_view(&marathon, Some(20.0)); // <30 km → derate

    // The displayed band shifts strictly SLOWER (later finish) at both ends.
    assert!(
        under.low_sec > supported.low_sec && under.high_sec > supported.high_sec,
        "under-mileaged band is slower: under=({}, {}) supported=({}, {})",
        under.low_sec,
        under.high_sec,
        supported.low_sec,
        supported.high_sec
    );
    // The caveat note is retained on the derated prediction, absent on the
    // well-supported one.
    assert!(
        under
            .notes
            .iter()
            .any(|n| n.summary.contains("marathon predictions come out too fast")),
        "caveat retained: {:#?}",
        under.notes
    );
    assert!(
        !supported
            .notes
            .iter()
            .any(|n| n.summary.contains("marathon predictions come out too fast")),
        "well-supported marathon carries no optimism caveat: {:#?}",
        supported.notes
    );
    // The derated number still travels with running-040/008 evidence
    // (RUN-VDOT-001) via the retained note (HARD RULE 2).
    assert!(
        under
            .notes
            .iter()
            .any(|n| n.summary.contains("VDOT points")),
        "derate magnitude + evidence present: {:#?}",
        under.notes
    );

    // Other distances are UNTOUCHED by the under-mileage gate: 5K/10K/half
    // predictions are identical whether the runner is under-mileaged
    // or not, and carry no marathon caveat.
    for goal in [5_000.0, 10_000.0, 21_097.5] {
        let q = RaceQuery {
            goal_distance_m: goal,
            ..marathon.clone()
        };
        let a = to_race_view(&q, Some(20.0));
        let b = to_race_view(&q, Some(32.0));
        assert_eq!(
            (a.low_sec, a.high_sec, a.predicted.clone()),
            (b.low_sec, b.high_sec, b.predicted.clone()),
            "distance {goal} m is untouched by the marathon derate"
        );
        assert!(
            a.notes
                .iter()
                .all(|n| !n.summary.contains("marathon predictions come out too fast")),
            "no marathon caveat leaks onto {goal} m: {:#?}",
            a.notes
        );
    }

    // The well-supported marathon is itself identical to the raw
    // (no-history) prediction: the derate is the ONLY thing 20 km changes.
    let no_history = to_race_view(&marathon, None);
    assert_eq!(
        (
            supported.low_sec,
            supported.high_sec,
            supported.predicted.clone()
        ),
        (
            no_history.low_sec,
            no_history.high_sec,
            no_history.predicted.clone()
        ),
        "an adequately-mileaged marathon matches the un-derated prediction"
    );
}

#[test]
fn race_prediction_absurd_goal_is_degenerate_not_infinite() {
    // An absurd goal distance overflows Riegel's power to f64::INFINITY. The
    // prediction must collapse to the degenerate "-"/"need a valid recent
    // race" path, never leak Inf (→ serde null) into low_sec/high_sec (a
    // `low_sec <= 0.0` guard alone does not catch Inf).
    let app = Engine;
    let mut model = Model::default();
    app.update(
        Event::PredictRace {
            recent_distance_m: 10_000.0,
            recent_time_sec: 2_520.0,
            goal_distance_m: 1e300,
            weekly_km: 50.0,
            weeks_since_race: Some(2),
        },
        &mut model,
    )
    .expect_only_render();
    let pred = app.view(&model).race_prediction.expect("prediction");
    assert_eq!(pred.predicted, "-", "absurd goal → degenerate dash");
    assert!(pred.summary.contains("enter a recent race"));
    assert!(
        pred.low_sec.is_finite() && pred.high_sec.is_finite(),
        "no Inf reaches the view: low {} high {}",
        pred.low_sec,
        pred.high_sec
    );
}

#[test]
fn calculators_echo_their_inputs_for_form_rehydration() {
    // Each calculator's result view must carry back the raw query it was
    // computed from, so a shell rehydrates its form after a log replay
    // instead of resetting to hardcoded defaults (the "it reset" bug).
    let app = Engine;
    let mut model = Model::default();

    app.update(
        Event::PredictRace {
            recent_distance_m: 10_000.0,
            recent_time_sec: 2_520.0,
            goal_distance_m: 21_097.5,
            weekly_km: 55.0,
            weeks_since_race: Some(3),
        },
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::ComputeHrZones {
            age_years: 47.0,
            resting_hr_bpm: Some(52.0),
            weeks_since_recalc: Some(5),
            weeks_since_pace_test: None,
        },
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::ComputeProtein {
            bodyweight_kg: 82.5,
            masters: true,
            deficit: false,
        },
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::PlanHypertrophyMeso {
            muscle: "back".into(),
            weeks: 6,
            not_growing: true,
            recovering_easily: false,
        },
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::ComputeCooper {
            distance_m_12min: 2_750.0,
        },
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::ComputeCriticalSpeed {
            efforts: vec![
                CsEffortIn {
                    distance_m: 1_200.0,
                    time_sec: 200.0,
                },
                CsEffortIn {
                    distance_m: 3_000.0,
                    time_sec: 600.0,
                },
            ],
        },
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::ComputeApre {
            scheme: autoreg::ApreScheme::Apre6,
            reps: 9,
            current_load_kg: 185.0,
        },
        &mut model,
    )
    .expect_only_render();

    let v = app.view(&model);

    let rp = v.race_prediction.expect("prediction");
    assert_eq!(rp.recent_distance_m, 10_000.0);
    assert_eq!(rp.recent_time_sec, 2_520.0);
    assert_eq!(rp.goal_distance_m, 21_097.5);
    assert_eq!(rp.weekly_km, 55.0);
    assert_eq!(rp.weeks_since_race, Some(3));

    let hz = v.hr_zone_input.expect("hr zone input");
    assert_eq!(hz.age_years, 47.0);
    assert_eq!(hz.resting_hr_bpm, Some(52.0));
    assert_eq!(hz.weeks_since_recalc, Some(5));
    assert_eq!(hz.weeks_since_pace_test, None);

    let pr = v.protein_input.expect("protein input");
    assert_eq!(pr.bodyweight_kg, 82.5);
    assert!(pr.masters && !pr.deficit);

    let hy = v.hypertrophy_input.expect("hypertrophy input");
    assert_eq!(hy.muscle, "back");
    assert_eq!(hy.weeks, 6);
    assert!(hy.not_growing && !hy.recovering_easily);

    assert_eq!(v.cooper_input, Some(2_750.0));
    assert_eq!(v.critical_speed_input.len(), 2);
    assert_eq!(v.critical_speed_input[0].distance_m, 1_200.0);

    let ap = v.apre_input.expect("apre input");
    assert_eq!(ap.scheme, autoreg::ApreScheme::Apre6);
    assert_eq!(ap.reps, 9);
    assert_eq!(ap.current_load_kg, 185.0);
}

// ── Wire contract: HR zones consume the profile's measured HRmax ──
#[test]
fn measured_hr_max_supersedes_the_tanaka_estimate() {
    let q = HrZoneQuery {
        age_years: 30.0,
        resting_hr_bpm: None,
        weeks_since_recalc: None,
        weeks_since_pace_test: None,
    };
    // Tanaka(30) = 187; a measured 200 must drive the table instead.
    let (rows, hr_max, _) = build_hr_zones(&q, Some(200.0));
    let hrmax_row = rows
        .iter()
        .find(|r| r.summary.contains("HRmax"))
        .expect("an HRmax row is present");
    assert!(
        hrmax_row.summary.contains("200") && hrmax_row.summary.to_lowercase().contains("measured"),
        "the measured max must be shown: {}",
        hrmax_row.summary
    );
    // The structured figure mirrors the prose: measured, no Tanaka split.
    let m = hr_max.expect("structured hr_max present");
    assert!(m.measured && m.bpm == 200.0 && m.tanaka_intercept == 0.0);
    // Implausible measured values fall back to the age estimate.
    let (rows, hr_max, _) = build_hr_zones(&q, Some(9999.0));
    let hrmax_row = rows.iter().find(|r| r.summary.contains("HRmax")).unwrap();
    assert!(
        hrmax_row.summary.contains("187"),
        "an implausible measured max falls back to Tanaka: {}",
        hrmax_row.summary
    );
    // Fell back to Tanaka → the estimate carries the 208 − 0.7 × age split.
    let e = hr_max.expect("structured hr_max present");
    assert!(!e.measured && e.bpm == 187.0 && e.tanaka_intercept == 208.0 && e.tanaka_slope == 0.7);
}
