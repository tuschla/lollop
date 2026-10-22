use super::*;

/// The hold that keeps the plan calculators (hypertrophy plan, APRE) from
/// giving load or volume numbers: the first Stage-0 onboarding gate, else a
/// RED-S signal (onboard-050 "gates BEFORE any prescription"; safety-011 no
/// autonomous loading for minors; safety-045 none in pregnancy; safety-049 no
/// volume or intensity increase under RED-S).
pub(in crate::app) fn calculator_hold(
    profile: Option<&Profile>,
    reds_present: bool,
) -> Option<Recommended<Adjustment>> {
    profile
        .and_then(|p| {
            individualization::onboarding_gates(&p.health)
                .into_iter()
                .next()
        })
        .or_else(|| {
            reds_present.then(|| {
                graded(
                    Adjustment::Defer {
                        reason: String::new(),
                    },
                    claims::SAFE_REDS_001,
                )
            })
        })
}

/// The single row a held calculator shows instead of numbers, carrying the
/// hold's own evidence.
fn push_hold_row(rows: &mut Vec<GuidanceView>, section: &str, hold: &Recommended<Adjustment>) {
    push_guidance(
        rows,
        section,
        "No numbers while your training is on hold for a health reason. See the safety note for what to do next.".to_string(),
        hold,
    );
}

/// APRE next-load adjustment (File 06 autoreg-015…021 with the autoreg-019
/// small-lifter cap), as one graded row. Under a [`calculator_hold`] the row
/// names the hold instead.
pub(in crate::app) fn build_apre(
    q: &ApreQuery,
    weight_lb: bool,
    hold: Option<&Recommended<Adjustment>>,
) -> Vec<GuidanceView> {
    let mut rows = Vec::new();
    if let Some(hold) = hold {
        push_hold_row(&mut rows, "APRE", hold);
        return rows;
    }
    let adj = autoreg::apre_load_adjustment_capped_kg(q.scheme, q.reps, q.current_load_kg);
    let (lo, hi) = adj.value;
    let label = match q.scheme {
        autoreg::ApreScheme::Apre3 => "APRE-3",
        autoreg::ApreScheme::Apre6 => "APRE-6",
        autoreg::ApreScheme::Apre10 => "APRE-10",
    };
    let unit = weight_suffix(weight_lb);
    let load = weight_num(q.current_load_kg, weight_lb);
    let text = if lo == 0.0 && hi == 0.0 {
        format!(
            "{label}: {} reps on the AMRAP set at {load:.0} {unit}: keep the same weight",
            q.reps
        )
    } else {
        let (lo, hi) = if weight_lb {
            apre_source_lb(q)
        } else {
            (lo, hi)
        };
        let fmt = |v: f64| if weight_lb { fmt_lb(v) } else { fmt_kg(v) };
        let signed = |v: f64| {
            if v < 0.0 {
                format!("-{}", fmt(-v))
            } else {
                format!("+{}", fmt(v))
            }
        };
        format!(
            "{label}: {} reps on the AMRAP set at {load:.0} {unit}: change the next weight by {} to {} {unit}",
            q.reps,
            signed(lo),
            signed(hi)
        )
    };
    push_guidance(&mut rows, "APRE", text, &adj);
    rows
}

/// The APRE adjustment in the source's own pounds for a lb profile. Mann 2010
/// states the increments in lb (±5 / ±10 / ±15 lb); the engine's kg table is
/// those exact values halved onto kg plate steps (`apre_load_adjustment_kg`),
/// so doubling it recovers the source figures instead of converting a kg plate
/// back into an unloadable "+5.5 lb". The autoreg-019 small-lifter cap is
/// applied in lb as the KB states it (+15 lb on 100 lb = +15 %: each positive
/// bound capped at `load_lb × bound / 100`); load cuts are never capped.
fn apre_source_lb(q: &ApreQuery) -> (f64, f64) {
    const SOURCE_LB_PER_TABLE_KG: f64 = 2.0;
    const CAP_REFERENCE_LB: f64 = 100.0;
    let (lo, hi) = autoreg::apre_load_adjustment_kg(q.scheme, q.reps).value;
    let load_lb = q.current_load_kg * LB_PER_KG;
    let cap = |kg: f64| {
        let lb = kg * SOURCE_LB_PER_TABLE_KG;
        if lb > 0.0 && load_lb > 0.0 {
            lb.min(load_lb * lb / CAP_REFERENCE_LB)
        } else {
            lb
        }
    };
    (cap(lo), cap(hi))
}

/// Weekly set ramp across the accumulation block: the SAME
/// `hypertrophy::weekly_targets` the plan sizes its week from, so the planner
/// and the Today plan never disagree. With a profile the ramp opens at the
/// athlete's own weekly target (bounded to this muscle's MEV..MRV) under any
/// specialization block; without one it opens at MEV. It climbs to the top of
/// the muscle's productive range (MAV high, HYP-VOL-CAP-001); only a
/// specialization priority muscle climbs to MRV. A MEV-0 muscle gets no direct
/// target in the plan (compounds cover it); its optional direct-work ramp is
/// shown across its MAV range (`optional_only`). The
/// [`hypertrophy::MAX_WEEKLY_SET_INCREMENT`] step cap can top the ramp out
/// below its ceiling on a short block; `session_trimmed` says the
/// whole-session cap cut it (HYP-VOL-CAP-001).
fn block_ramp(
    lm: &hypertrophy::VolumeLandmarks,
    profile: Option<&Profile>,
    ta: Option<individualization::TrainingAge>,
    weeks: u8,
) -> (Recommended<Vec<u8>>, hypertrophy::VolumeRole, bool, bool) {
    let role = match (profile, ta) {
        (Some(p), Some(ta)) => {
            hypertrophy::volume_role(lm.muscle, p.specialization_muscle.as_deref(), ta).value
        }
        _ => hypertrophy::VolumeRole::Standard,
    };
    let stated = profile.map_or(lm.mev, |p| p.weekly_sets);
    let mut planned = hypertrophy::weekly_targets(lm, stated, role, weeks);
    let mut session_trimmed = false;
    // A profile that runs the hypertrophy week reads the plan's own targets,
    // already fitted to the whole-session cap (HYP-VOL-CAP-001), week by week.
    if let (Some(p), Some(ta)) = (profile, ta)
        && crate::plan::uses_double_progression(p)
    {
        let days = crate::plan::week_shape(p).lift_days;
        let fitted: Vec<u8> = (1..=weeks)
            .map(|w| {
                crate::plan::hypertrophy_week_targets(p, ta, w, weeks, days)
                    .iter()
                    .find(|t| t.muscle == lm.muscle)
                    .map_or(0, |t| t.target)
            })
            .collect();
        session_trimmed = fitted != planned.value;
        planned.value = fitted;
    }
    let optional_only = planned.value.iter().all(|&s| s == 0);
    let ramp = if optional_only {
        graded(
            hypertrophy::weekly_set_ramp(lm.mav.0, lm.mav.1, weeks).value,
            claims::HYP_VOL_CAP_001,
        )
    } else {
        planned
    };
    (ramp, role, optional_only, session_trimmed && !optional_only)
}

/// The weekly set counts of the planner's ramp row, for a compact summary
/// elsewhere (the Profile calculator row). Empty whenever the planner shows no
/// ramp: a calculator hold, an unknown muscle, or a zero-week block.
pub(in crate::app) fn hypertrophy_ramp(
    q: &HypertrophyPlanQuery,
    profile: Option<&Profile>,
    hold: Option<&Recommended<Adjustment>>,
) -> Vec<u8> {
    if hold.is_some() || q.weeks == 0 {
        return Vec::new();
    }
    let Some(lm) = hypertrophy::resolve_muscle(&q.muscle) else {
        return Vec::new();
    };
    let ta =
        profile.map(|p| individualization::training_age_from_cadence(p.progression_cadence).value);
    block_ramp(lm, profile, ta, q.weeks).0.value
}

/// Build a graded per-week hypertrophy accumulation plan for one muscle. Every
/// row carries its own evidence + confidence via [`push_guidance`] (HARD RULE 2);
/// no training numbers are invented: all come from [`hypertrophy`]. An unknown
/// muscle yields a single explanatory row (no fabricated landmarks). `weeks == 0`
/// yields no plan rows beyond the landmarks context. Alongside the rows it
/// returns the plan's figures (`None` whenever no ramp is shown), each figure
/// computed by the same engine call as the row it stands for.
pub(in crate::app) fn build_hypertrophy_plan(
    q: &HypertrophyPlanQuery,
    profile: Option<&Profile>,
    hold: Option<&Recommended<Adjustment>>,
) -> (Vec<GuidanceView>, Option<HypertrophyFiguresView>) {
    let mut rows = Vec::new();
    let section = q.muscle.clone();
    if let Some(hold) = hold {
        push_hold_row(&mut rows, &section, hold);
        return (rows, None);
    }

    let Some(lm) = hypertrophy::resolve_muscle(&q.muscle) else {
        // No landmarks for this muscle: say so plainly rather than guess. The
        // note itself is the RP landmark framing, so it is graded from that claim.
        let note = graded((), claims::HYP_LANDMARKS_001);
        push_guidance(
            &mut rows,
            &section,
            format!(
                "\"{}\" is not a known muscle. Pick one of: chest, back, quads, hamstrings, glutes, side delts, rear delts, biceps, triceps, calves, abs",
                q.muscle
            ),
            &note,
        );
        return (rows, None);
    };

    // Landmarks row, landmarks are a raw engine value, not a Recommended, so
    // wrap them in their HYP-LANDMARKS-001 evidence (ExpertOpinion) to grade it.
    let landmarks = graded((), claims::HYP_LANDMARKS_001);
    push_guidance(
        &mut rows,
        &section,
        format!(
            "Minimum {} · productive {}–{} · max {} sets/week",
            lm.mev, lm.mav.0, lm.mav.1, lm.mrv
        ),
        &landmarks,
    );

    // `weeks == 0` is degenerate: the ramp is empty and there is no schedule to
    // show, so stop after the landmarks context rather than emit empty rows.
    if q.weeks == 0 {
        return (rows, None);
    }

    let ta =
        profile.map(|p| individualization::training_age_from_cadence(p.progression_cadence).value);
    let (ramp, role, optional_only, session_trimmed) = block_ramp(lm, profile, ta, q.weeks);
    let ramp_row = rows.len() as u32;
    push_ramp_row(
        &mut rows,
        &section,
        lm,
        &ramp,
        role,
        optional_only,
        session_trimmed,
    );
    let novice = ta == Some(individualization::TrainingAge::Novice);
    let dose_row = rows.len() as u32;
    push_dose_rows(&mut rows, &section, lm, &ramp.value, q.weeks, novice);
    let rest_row = rows.len() as u32;
    push_rest_rows(&mut rows, &section);
    let shape_row = rows.len() as u32;
    push_execution_rows(&mut rows, &section);
    push_next_meso_rows(&mut rows, &section, lm, q, profile, &ramp.value);
    push_training_age_rows(&mut rows, &section, profile);

    // The ramp row stands for the chart only on a standard block: a
    // specialization, maintenance or optional-only ramp keeps its sentence.
    let standard = !optional_only && role == hypertrophy::VolumeRole::Standard;
    let mut face_rows = vec![0];
    if standard {
        face_rows.push(ramp_row);
    }
    face_rows.extend([dose_row, dose_row + 1, rest_row, rest_row + 1, shape_row]);
    let peak_sets = ramp.value.iter().copied().max().unwrap_or(lm.mrv);
    let freq = hypertrophy::frequency_for_weekly_sets(peak_sets).value;
    let heavy = hypertrophy::rest_sec_for(hypertrophy::ExerciseClass::HeavyCompound).value;
    let isolation = hypertrophy::rest_sec_for(hypertrophy::ExerciseClass::Isolation).value;
    let ms = hypertrophy::meso_structure().value;
    let figures = (!ramp.value.is_empty()).then(|| HypertrophyFiguresView {
        min_sets: lm.mev,
        productive_low: lm.mav.0,
        productive_high: lm.mav.1,
        max_sets: lm.mrv,
        rir_by_week: (1..=q.weeks)
            .map(|w| hypertrophy::block_rir(w, q.weeks, novice).value)
            .collect(),
        freq_low: freq.freq.0,
        freq_high: freq.freq.1,
        per_session_low: freq.per_session.0,
        per_session_high: freq.per_session.1,
        rest_heavy_low_s: heavy.0,
        rest_heavy_high_s: heavy.1,
        rest_isolation_low_s: isolation.0,
        rest_isolation_high_s: isolation.1,
        build_weeks_low: ms.accumulation_weeks.0,
        build_weeks_high: ms.accumulation_weeks.1,
        deload_weeks: ms.deload_weeks,
        face_rows,
    });
    (rows, figures)
}

/// The weekly set-ramp row ([`block_ramp`]), worded for the muscle's role.
fn push_ramp_row(
    rows: &mut Vec<GuidanceView>,
    section: &str,
    lm: &hypertrophy::VolumeLandmarks,
    ramp: &Recommended<Vec<u8>>,
    role: hypertrophy::VolumeRole,
    optional_only: bool,
    session_trimmed: bool,
) {
    let ramp_str = ramp
        .value
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(" → ");
    let cap_note = if session_trimmed {
        format!(
            " (trimmed so no lift day passes {} working sets)",
            hypertrophy::SESSION_WORKING_SET_CAP
        )
    } else if role != hypertrophy::VolumeRole::Maintenance
        && ramp
            .value
            .last()
            .is_some_and(|&last| last < hypertrophy::ramp_ceiling(lm, role))
    {
        format!(
            " (capped at +{} sets/wk)",
            hypertrophy::MAX_WEEKLY_SET_INCREMENT
        )
    } else {
        String::new()
    };
    let ramp_text = match role {
        _ if optional_only => format!(
            "No direct sets: compound lifts already train this muscle. Optional direct work: {ramp_str} sets/wk{cap_note}"
        ),
        hypertrophy::VolumeRole::Priority => format!(
            "Specialization block: weekly set ramp {ramp_str}{cap_note}, starting above your usual target"
        ),
        hypertrophy::VolumeRole::Maintenance => format!(
            "Held at maintenance ({} sets/wk) while another muscle is specialized: {ramp_str}",
            lm.mv
        ),
        hypertrophy::VolumeRole::Standard => format!("Weekly set ramp: {ramp_str}{cap_note}"),
    };
    push_guidance(rows, section, ramp_text, ramp);
}

/// The block's dose rows: the RIR schedule, peak-week frequency and the
/// per-session cap check.
fn push_dose_rows(
    rows: &mut Vec<GuidanceView>,
    section: &str,
    lm: &hypertrophy::VolumeLandmarks,
    ramp: &[u8],
    weeks: u8,
    novice: bool,
) {
    // RIR schedule across the block, the plan's own `block_rir`: held inside
    // the 1–3 default band (a novice runs the 4 → 2 calibration ramp). The
    // first week's Recommended carries the grade.
    let rir_vals: Vec<u8> = (1..=weeks)
        .map(|w| hypertrophy::block_rir(w, weeks, novice).value)
        .collect();
    let first = hypertrophy::block_rir(1, weeks, novice);
    let rir_str = rir_vals
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(" → ");
    push_guidance(
        rows,
        section,
        format!("Reps left by week: {rir_str}"),
        &first,
    );

    // Peak-week frequency + per-session spread (from the ramp's top set count).
    let peak_sets = ramp.iter().copied().max().unwrap_or(lm.mrv);
    let freq = hypertrophy::frequency_for_weekly_sets(peak_sets);
    push_guidance(
        rows,
        section,
        format!(
            "Peak frequency: {}×/wk · {} sets/session",
            fmt_u8_range(freq.value.freq.0, freq.value.freq.1),
            fmt_u8_range(freq.value.per_session.0, freq.value.per_session.1)
        ),
        &freq,
    );

    // hypertrophy-004: warn when the implied per-session dose exceeds the ~11
    // fractional-set ceiling, redistribute to another session, not more sets.
    let over_cap = hypertrophy::per_session_over_cap(freq.value.per_session.1);
    if over_cap.value {
        push_guidance(
            rows,
            section,
            format!(
                "{} sets in one session exceeds the ~{}-set per-session cap. Add a session instead of stacking sets",
                freq.value.per_session.1,
                hypertrophy::PER_SESSION_SET_CEILING
            ),
            &over_cap,
        );
    }
}

/// hypertrophy-013/039 rest prescriptions per exercise class, so the plan
/// states how long to rest, not only how much to lift.
fn push_rest_rows(rows: &mut Vec<GuidanceView>, section: &str) {
    for (label, class) in [
        ("heavy compounds", hypertrophy::ExerciseClass::HeavyCompound),
        (
            "machines and isolation work",
            hypertrophy::ExerciseClass::Isolation,
        ),
    ] {
        let rest = hypertrophy::rest_sec_for(class);
        let (lo, hi) = rest.value;
        // Whole minutes read as "2–3 min"; a 90 s floor as "90 s–2 min".
        let window = if lo.is_multiple_of(60) && hi.is_multiple_of(60) {
            format!("{}–{} min", lo / 60, hi / 60)
        } else {
            let part = |s: u16| {
                if s.is_multiple_of(60) {
                    format!("{} min", s / 60)
                } else {
                    format!("{s} s")
                }
            };
            format!("{}–{}", part(lo), part(hi))
        };
        push_guidance(
            rows,
            section,
            format!("Rest between sets ({label}): {window}"),
            &rest,
        );
    }
}

/// How to execute the block: mesocycle shape, proximity to failure, where
/// failure is allowed, the high-skill guard, load interchangeability and tempo.
fn push_execution_rows(rows: &mut Vec<GuidanceView>, section: &str) {
    // hypertrophy-031: the block's macro-shape (accumulate : deload cadence).
    let ms = hypertrophy::meso_structure();
    push_guidance(
        rows,
        section,
        format!(
            "Mesocycle shape: {} building weeks, then {} lighter week{} (every {} weeks)",
            fmt_u8_range(ms.value.accumulation_weeks.0, ms.value.accumulation_weeks.1),
            ms.value.deload_weeks,
            if ms.value.deload_weeks == 1 { "" } else { "s" },
            fmt_u8_range(
                ms.value.deload_cadence_weeks.0,
                ms.value.deload_cadence_weeks.1
            )
        ),
        &ms,
    );

    // hypertrophy-018: default working proximity to failure.
    let rir_band = hypertrophy::default_rir_band();
    push_guidance(
        rows,
        section,
        format!(
            "Stop most sets with {} reps left. Going to failure isn't needed and slows recovery",
            fmt_u8_range(rir_band.value.0, rir_band.value.1)
        ),
        &rir_band,
    );

    // hypertrophy-021: where failure is permitted at all (machines/isolation;
    // never heavy free-weight compounds). Educational, the calls' arguments
    // only inherit the rule's citation.
    let fail = hypertrophy::failure_allowed(hypertrophy::ExerciseClass::Isolation, true);
    push_guidance(
        rows,
        section,
        "Take sets to failure only on machines/isolation, never on heavy free-weight compounds"
            .to_string(),
        &fail,
    );

    // hypertrophy-017: high-skill exercise guard.
    let guard = hypertrophy::high_skill_guard();
    push_guidance(
        rows,
        section,
        format!(
            "Lifts that take a lot of skill or balance: at least {} reps a set, and stop with {} or more reps left, to protect technique",
            guard.value.min_reps,
            fmt_u8_range(guard.value.min_rir.0, guard.value.min_rir.1)
        ),
        &guard,
    );

    // hypertrophy-012: load interchangeability window.
    let range = hypertrophy::interchangeable_load_range();
    push_guidance(
        rows,
        section,
        format!(
            "Weights are interchangeable for growth across {}–{}% 1RM ({}–{} reps) when sets are near failure",
            range.value.pct_1rm.0, range.value.pct_1rm.1, range.value.reps.0, range.value.reps.1
        ),
        &range,
    );

    // hypertrophy-040: controlled tempo window.
    let tempo = hypertrophy::tempo_rx();
    push_guidance(
        rows,
        section,
        format!(
            "Tempo: controlled {}–{} s/rep ({}–{} s up, {}–{} s down). Avoid very slow reps (over 10 s); they force a lighter weight",
            tempo.value.rep_duration_s.0,
            tempo.value.rep_duration_s.1,
            tempo.value.concentric_s.0,
            tempo.value.concentric_s.1,
            tempo.value.eccentric_s.0,
            tempo.value.eccentric_s.1
        ),
        &tempo,
    );
}

/// The next-mesocycle volume decision and the first-week jump check.
fn push_next_meso_rows(
    rows: &mut Vec<GuidanceView>,
    section: &str,
    lm: &hypertrophy::VolumeLandmarks,
    q: &HypertrophyPlanQuery,
    profile: Option<&Profile>,
    ramp: &[u8],
) {
    // hypertrophy-008: next-mesocycle volume decision. The current weekly-set
    // base is the profile's planned weekly sets when one exists, else the
    // muscle's MEV (the block's own starting point).
    let current = profile.map(|p| p.weekly_sets).unwrap_or(lm.mev);
    let next = hypertrophy::next_meso_weekly_sets(current, q.not_growing, q.recovering_easily);
    push_guidance(
        rows,
        section,
        if next.value > current {
            format!(
                "Not growing while recovering easily: raise next mesocycle to {} sets/wk (from {current})",
                next.value
            )
        } else {
            format!("Next mesocycle: stay at {current} sets/wk")
        },
        &next,
    );

    // hyp-001 ramp discipline: warn when the plan's first week is an abrupt
    // jump from the athlete's current weekly sets.
    if let Some(p) = profile
        && let Some(&first_week) = ramp.first()
    {
        let jump = hypertrophy::abrupt_volume_jump(p.weekly_sets, first_week);
        if jump.value {
            push_guidance(
                rows,
                section,
                format!(
                    "The plan starts at {first_week} sets/wk, up from your current {}. That's a big jump; add sets gradually",
                    p.weekly_sets
                ),
                &jump,
            );
        }
    }
}

/// Training-age extras: the novice RIR calibration start, the intermediate
/// default program synthesis.
fn push_training_age_rows(rows: &mut Vec<GuidanceView>, section: &str, profile: Option<&Profile>) {
    if let Some(p) = profile {
        match individualization::training_age_from_cadence(p.progression_cadence).value {
            individualization::TrainingAge::Novice => {
                let start = hypertrophy::novice_start_rir();
                push_guidance(
                    rows,
                    section,
                    format!(
                        "New to counting reps left? Start with {} reps left, and now and then take a set to failure to check your count",
                        fmt_u8_range(start.value.0, start.value.1)
                    ),
                    &start,
                );
            }
            individualization::TrainingAge::Intermediate => {
                let prog = hypertrophy::intermediate_default_program();
                push_guidance(
                    rows,
                    section,
                    format!(
                        "Standard plan: each muscle {}×/wk, {} sets/wk; compound lifts {} reps, single-joint lifts {}; {} reps left in week 1 down to {}; a lighter week in week {}",
                        prog.value.frequency_per_week,
                        fmt_u8_range(prog.value.weekly_sets.0, prog.value.weekly_sets.1),
                        fmt_u8_range(prog.value.compound_reps.0, prog.value.compound_reps.1),
                        fmt_u8_range(prog.value.isolation_reps.0, prog.value.isolation_reps.1),
                        prog.value.week1_rir,
                        prog.value.final_rir,
                        fmt_u8_range(prog.value.deload_week.0, prog.value.deload_week.1)
                    ),
                    &prog,
                );
            }
            individualization::TrainingAge::Advanced => {}
        }
    }
}

/// The bodyweight a g/kg range multiplies: always in kg (the unit g/kg
/// needs), with the pound figure alongside for a pound profile, e.g.
/// "72.6 kg (160 lb)".
fn protein_bodyweight(kg: f64, weight_lb: bool) -> String {
    if weight_lb {
        format!("{} kg ({:.0} lb)", fmt_kg(kg), weight_num(kg, true))
    } else {
        format!("{} kg", fmt_kg(kg))
    }
}

/// Build absolute daily protein target rows by scaling each graded g/kg range by
/// the athlete's bodyweight. Multiplying a graded g/kg bound by bodyweight is
/// honest arithmetic, the grade travels with the underlying claim via
/// [`push_guidance`] (HARD RULE 2). No general/default protein number is
/// invented: if neither goal context is selected (or bodyweight is non-positive)
/// the section is empty (HARD RULE 1).
///
/// `reds_present` carries the RED-S flag from the readiness inputs or the
/// onboarding screen: with it set, a requested deficit target is REFUSED inside
/// `individualization::deficit_protein_target` (File 08 safety-022) and the row
/// becomes the safety-critical deferral instead of a number.
pub(in crate::app) fn build_protein_targets(
    q: &ProteinQuery,
    reds_present: bool,
    weight_lb: bool,
) -> (Vec<GuidanceView>, Vec<ProteinFigureView>) {
    let mut rows = Vec::new();
    // Structured g/day figures paralleling the prose rows (see `ProteinFigureView`).
    let mut figures = Vec::new();

    // Cannot derive g/day from a non-positive bodyweight, say nothing rather
    // than emit a nonsensical or zero target.
    if q.bodyweight_kg <= 0.0 {
        return (rows, figures);
    }

    // The prose row and the structured figure share one rounding so the card
    // and the profile row can never show different gram counts.
    let grams = |g_per_kg: f64| (q.bodyweight_kg * g_per_kg).round();

    if q.masters {
        let r = individualization::masters_protein_target();
        let (lo, hi) = r.value.g_per_kg;
        push_guidance(
            &mut rows,
            "Protein",
            format!(
                "Age 65+: {:.0}–{:.0} g/day · {lo:.1}–{hi:.1} g/kg × {}",
                grams(lo),
                grams(hi),
                protein_bodyweight(q.bodyweight_kg, weight_lb),
            ),
            &r,
        );
        figures.push(ProteinFigureView {
            kind: ProteinTargetKind::Masters,
            low_g_per_day: grams(lo),
            high_g_per_day: grams(hi),
            refused: false,
            low_g_per_kg: lo,
            high_g_per_kg: hi,
            row: rows.len() as u32 - 1,
        });
    }

    if q.deficit {
        let r = individualization::deficit_protein_target(reds_present);
        match r.value {
            Some(t) => {
                let (lo, hi) = t.g_per_kg;
                push_guidance(
                    &mut rows,
                    "Protein",
                    format!(
                        "Deficit (lean-mass preserving): {:.0}–{:.0} g/day · {lo:.1}–{hi:.1} g/kg × {}",
                        grams(lo),
                        grams(hi),
                        protein_bodyweight(q.bodyweight_kg, weight_lb),
                    ),
                    &r,
                );
                figures.push(ProteinFigureView {
                    kind: ProteinTargetKind::Deficit,
                    low_g_per_day: grams(lo),
                    high_g_per_day: grams(hi),
                    refused: false,
                    low_g_per_kg: lo,
                    high_g_per_kg: hi,
                    row: rows.len() as u32 - 1,
                });
            }
            None => {
                // safety-022: the deficit is refused, not silently omitted -
                // the user sees why, cited to the RED-S deferral.
                push_guidance(&mut rows, "Protein", DEFICIT_REFUSAL.to_string(), &r);
                figures.push(ProteinFigureView {
                    kind: ProteinTargetKind::Deficit,
                    low_g_per_day: 0.0,
                    high_g_per_day: 0.0,
                    refused: true,
                    low_g_per_kg: 0.0,
                    high_g_per_kg: 0.0,
                    row: rows.len() as u32 - 1,
                });
            }
        }
    }

    (rows, figures)
}
