use super::*;

/// The hold that keeps the hypertrophy-plan calculator and the
/// prescriptive guidance from giving load or volume numbers: today's training
/// hold (`SafetyState::hold`: a profile gate, a readiness Stop / RestDay /
/// Defer, or a weekly-check-in deferral; the same call that blocks Today),
/// else a RED-S signal (onboard-050 "gates BEFORE any prescription";
/// safety-011 no autonomous loading for minors; safety-040/043 defer "before
/// prescribing"; safety-045 none in pregnancy; safety-049 no volume or
/// intensity increase under RED-S; autoreg-046 no training with a fever).
pub(in crate::app) fn calculator_hold(
    training_hold: Option<&Recommended<Adjustment>>,
    reds_present: bool,
) -> Option<Recommended<Adjustment>> {
    training_hold
        .cloned()
        .or_else(|| reds_present.then(autoreg::reds_referral))
}

/// The single row a held calculator or guidance list shows instead of
/// numbers, carrying the hold's own evidence.
pub(in crate::app) fn push_hold_row(
    rows: &mut Vec<GuidanceView>,
    section: &str,
    hold: &Recommended<Adjustment>,
) {
    push_guidance(
        rows,
        section,
        "No numbers while training is paused. The banner at the top says what to do next."
            .to_string(),
        hold,
    );
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
    // already fitted to the whole-session cap (HYP-VOL-CAP-001) block-wide.
    if let (Some(p), Some(ta)) = (profile, ta)
        && crate::plan::uses_double_progression(p)
    {
        let days = crate::plan::week_shape(p).lift_days;
        let fitted: Vec<u8> = crate::plan::hypertrophy_block_targets(p, ta, weeks, days)
            .iter()
            .map(|week| {
                week.iter()
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

/// Build a graded per-week hypertrophy accumulation plan for one muscle. Every
/// row carries its own evidence + confidence via [`push_guidance`] (HARD RULE 2);
/// no training numbers are invented: all come from [`hypertrophy`]. An unknown
/// muscle yields a single explanatory row (no fabricated landmarks). `weeks == 0`
/// yields no plan rows beyond the landmarks context. Alongside the rows it
/// returns the plan's figures and the weekly set ramp (`None` / empty whenever
/// no ramp is shown), each figure computed by the same engine call as the row
/// it stands for.
pub(in crate::app) fn build_hypertrophy_plan(
    q: &HypertrophyInputView,
    profile: Option<&Profile>,
    hold: Option<&Recommended<Adjustment>>,
) -> (Vec<GuidanceView>, Option<HypertrophyFiguresView>, Vec<u8>) {
    let mut rows = Vec::new();
    let section = q.muscle.clone();
    if let Some(hold) = hold {
        push_hold_row(&mut rows, &section, hold);
        return (rows, None, Vec::new());
    }

    let Some(lm) = hypertrophy::resolve_muscle(&q.muscle) else {
        // No landmarks for this muscle: say so plainly rather than guess. The
        // note itself is the RP landmark framing, so it is graded from that claim.
        let note = graded((), claims::HYP_LANDMARKS_001);
        push_guidance(
            &mut rows,
            &section,
            format!(
                "\"{}\" is not a known muscle. Pick one of: {}",
                q.muscle,
                hypertrophy::LANDMARKS
                    .iter()
                    .map(|l| l.muscle)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            &note,
        );
        return (rows, None, Vec::new());
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
        return (rows, None, Vec::new());
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
    let (rir_by_week, freq) = push_dose_rows(&mut rows, &section, lm, &ramp.value, q.weeks, novice);
    let rest_row = rows.len() as u32;
    let [heavy, isolation] = push_rest_rows(&mut rows, &section);
    let shape_row = rows.len() as u32;
    let ms = push_execution_rows(&mut rows, &section);
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
    let ramp = ramp.value;
    let figures = (!ramp.is_empty()).then_some(HypertrophyFiguresView {
        productive_low: lm.mav.0,
        productive_high: lm.mav.1,
        max_sets: lm.mrv,
        rir_by_week,
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
    (rows, figures, ramp)
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
        .map(ToString::to_string)
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
            "Focus block: weekly set ramp {ramp_str}{cap_note}, starting above your usual target"
        ),
        hypertrophy::VolumeRole::Maintenance => format!(
            "Held at maintenance ({} sets/wk) while another muscle has a focus block: {ramp_str}",
            lm.mv
        ),
        hypertrophy::VolumeRole::Standard => format!("Weekly set ramp: {ramp_str}{cap_note}"),
    };
    push_guidance(rows, section, ramp_text, ramp);
}

/// The block's dose rows: the RIR schedule, peak-week frequency and the
/// per-session cap check. Returns the RIR by week and the frequency the rows
/// state.
fn push_dose_rows(
    rows: &mut Vec<GuidanceView>,
    section: &str,
    lm: &hypertrophy::VolumeLandmarks,
    ramp: &[u8],
    weeks: u8,
    novice: bool,
) -> (Vec<u8>, hypertrophy::FrequencyRx) {
    // RIR schedule across the block, the plan's own `block_rir`: held inside
    // the 1–3 default band (a novice runs the 4 → 2 calibration ramp). The
    // first week's Recommended carries the grade.
    let rir_vals: Vec<u8> = (1..=weeks)
        .map(|w| hypertrophy::block_rir(w, weeks, novice).value)
        .collect();
    let first = hypertrophy::block_rir(1, weeks, novice);
    let rir_str = rir_vals
        .iter()
        .map(ToString::to_string)
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
            fmt_u8_range(freq.value.freq),
            fmt_u8_range(freq.value.per_session)
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
    (rir_vals, freq.value)
}

/// hypertrophy-013/039 rest prescriptions per exercise class, so the plan
/// states how long to rest, not only how much to lift. Returns the
/// (heavy compound, isolation) windows, s.
fn push_rest_rows(rows: &mut Vec<GuidanceView>, section: &str) -> [(u16, u16); 2] {
    let mut windows = [(0, 0); 2];
    for (slot, (label, class)) in [
        ("heavy compounds", hypertrophy::ExerciseClass::HeavyCompound),
        (
            "machines and isolation work",
            hypertrophy::ExerciseClass::Isolation,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let rest = hypertrophy::rest_sec_for(class);
        let (lo, hi) = rest.value;
        windows[slot] = rest.value;
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
    windows
}

/// How to execute the block: mesocycle shape, proximity to failure, where
/// failure is allowed, the high-skill guard, load interchangeability and tempo.
/// Returns the mesocycle shape the first row states.
fn push_execution_rows(rows: &mut Vec<GuidanceView>, section: &str) -> hypertrophy::MesoStructure {
    // hypertrophy-031: the block's macro-shape (accumulate : deload cadence).
    let ms = hypertrophy::meso_structure();
    push_guidance(
        rows,
        section,
        format!(
            "Block shape: {} building weeks, then {} lighter week{} (every {} weeks)",
            fmt_u8_range(ms.value.accumulation_weeks),
            ms.value.deload_weeks,
            if ms.value.deload_weeks == 1 { "" } else { "s" },
            fmt_u8_range(ms.value.deload_cadence_weeks)
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
            fmt_u8_range(rir_band.value)
        ),
        &rir_band,
    );

    // hypertrophy-021: failure only on machines/isolation. The plan programs
    // no set to failure on any weighted lift (RIR ≥ 1); this row states the rule.
    let fail = graded((), claims::HYP_FAIL_SAFE_001);
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
            fmt_u8_range(guard.value.min_rir)
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
    ms.value
}

/// The next-mesocycle volume decision and the first-week jump check.
fn push_next_meso_rows(
    rows: &mut Vec<GuidanceView>,
    section: &str,
    lm: &hypertrophy::VolumeLandmarks,
    q: &HypertrophyInputView,
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
                "Not growing while recovering easily: raise next block to {} sets/wk (from {current})",
                next.value
            )
        } else {
            format!("Next block: stay at {current} sets/wk")
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
                        "If you're new to counting reps left, start with {} reps left and now and then take a set to failure to check your count",
                        fmt_u8_range(start.value)
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
                        fmt_u8_range(prog.value.weekly_sets),
                        fmt_u8_range(prog.value.compound_reps),
                        fmt_u8_range(prog.value.isolation_reps),
                        prog.value.week1_rir,
                        prog.value.final_rir,
                        fmt_u8_range(prog.value.deload_week)
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
    q: &ProteinInputView,
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
    // The figure for the row just pushed (`rows_len` = rows after the push).
    let fig = |kind, (lo, hi): (f64, f64), refused, rows_len: usize| ProteinFigureView {
        kind,
        low_g_per_day: grams(lo),
        high_g_per_day: grams(hi),
        refused,
        low_g_per_kg: lo,
        high_g_per_kg: hi,
        row: rows_len as u32 - 1,
    };

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
        figures.push(fig(ProteinTargetKind::Masters, (lo, hi), false, rows.len()));
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
                        "Eating less to lose weight: {:.0}–{:.0} g/day · {lo:.1}–{hi:.1} g/kg × {}",
                        grams(lo),
                        grams(hi),
                        protein_bodyweight(q.bodyweight_kg, weight_lb),
                    ),
                    &r,
                );
                figures.push(fig(ProteinTargetKind::Deficit, (lo, hi), false, rows.len()));
            }
            None => {
                // safety-022: the deficit is refused, not silently omitted -
                // the user sees why, cited to the RED-S deferral.
                push_guidance(&mut rows, "Protein", DEFICIT_REFUSAL.to_string(), &r);
                figures.push(fig(
                    ProteinTargetKind::Deficit,
                    (0.0, 0.0),
                    true,
                    rows.len(),
                ));
            }
        }
    }

    (rows, figures)
}
