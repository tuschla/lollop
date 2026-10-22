//! Flattening a `Prescription::Lift` arm.

use super::*;

/// What one lift intensity arm resolves before the item is assembled.
#[derive(Default)]
struct LiftArm {
    load_kg: Option<f64>,
    intensity: PlanIntensity,
    /// The verbatim label of a `Prose` target; a structured target renders its
    /// own (`PlanIntensity::label`).
    prose_label: String,
    anchored_on: String,
    /// How many sets `anchored_on` names as last time (`"Last: 60 kg × 8, 8,
    /// 7"` is 3); 0 when it is no last-time line.
    anchored_sets: usize,
    /// The "why?" basis; `None` keeps the claim statement.
    basis: Option<String>,
    /// The "why?" `improves` line: an honest ENGINE data-need (which input
    /// would sharpen this load), never a training claim (HARD RULE 1). It
    /// overrides the claim-keyed default so a prescription card carries the
    /// full 3-part disclosure like adjustment/guidance cards do.
    improves: Option<String>,
    target_rpe: Option<f64>,
    /// The rule that set the number instead of the loading band (a derate or
    /// a novice re-entry): the card's evidence triad re-points to it.
    repoint: Option<ClaimId>,
}

/// Flatten a `Prescription::Lift` arm: resolves the load from the anchor for a
/// %1RM target and re-points the evidence triad for a declared layoff.
pub(super) fn flatten_lift_prescription(
    l: &crate::schema::LiftPrescription,
    anchors: &crate::plan::Anchors,
    rx: &Recommended<Prescription>,
    weight_lb: bool,
) -> PrescriptionView {
    let LiftArm {
        load_kg,
        intensity,
        prose_label,
        anchored_on,
        anchored_sets,
        basis,
        improves,
        target_rpe,
        repoint,
    } = match l.intensity {
        LiftIntensity::PercentOneRm(p) => percent_arm(l, p, anchors, weight_lb),
        LiftIntensity::Rir(n) => rir_arm(l, n, anchors, rx, weight_lb),
        LiftIntensity::Rpe(r) => rpe_arm(l, r, anchors),
        LiftIntensity::VelocityMs(v) => LiftArm {
            intensity: PlanIntensity::Prose,
            prose_label: format!("{v:.2} m/s"),
            ..LiftArm::default()
        },
        LiftIntensity::MaxIntent => max_intent_arm(l),
    };
    let intensity_label = intensity.label().unwrap_or(prose_label);
    let mut why = why_from(basis, rx);
    if let Some(imp) = improves {
        why.improves = imp;
    }
    // Power/speed prescriptions carry a velocity intent (File 02 strength-023
    // "50–70% FAST"; AUTOREG-VL-001): the load is only a power stimulus at
    // maximal concentric intent. Without a velocity device nobody can perceive
    // a ~10% bar-speed loss, so the cue is the visible slowdown; the face
    // carries "move fast" (`effort_cue`), the basis the full sentence.
    let effort_cue = if l.velocity_loss_pct.is_some() {
        let cue = "Move every rep as fast as you can; end the set when a rep visibly slows.";
        why.basis = if why.basis.is_empty() {
            cue.to_string()
        } else {
            format!("{} {}", why.basis, cue)
        };
        "move fast".to_string()
    } else {
        String::new()
    };
    let mut evidence = EvidenceTagView::of(rx);
    if let Some(id) = repoint {
        let re = graded((), id);
        evidence = EvidenceTagView::of(&re);
        why.grade_note = grade_note_str(re.grade(), re.contested());
    }
    let mut it = PrescriptionView {
        summary: String::new(),
        exercise: l.exercise.clone(),
        sets: l.sets,
        reps_low: l.reps,
        reps_high: l.reps_max.unwrap_or(l.reps).max(l.reps),
        load_kg,
        intensity_label,
        intensity,
        rest_sec: l.rest_sec,
        rep_count: 0,
        rep_volume: String::new(),
        timed_hold: false,
        target_rpe,
        hold_low_sec: 0,
        hold_high_sec: 0,
        added_kg: None,
        anchored_on,
        anchored_on_last: anchored_sets > 0,
        anchored_on_single_set: anchored_sets == 1,
        adjusted_note: String::new(),
        return_check: false,
        safety_reason: String::new(),
        safety_reason_mi: String::new(),
        evidence,
        why,
        done: false,
        sets_done: 0,
        logged_sets: Vec::new(),
        ended_early: false,
        run_logged_km: 0.0,
        run_logged_min: 0.0,
        run_target: None,
        calis: None,
        last_set: None,
        suggested_load_kg: None,
        plates_per_side: Vec::new(),
        effort_cue,
        run_structure: None,
        face_name: String::new(),
        face_detail: String::new(),
        face_lead: String::new(),
        run_head: String::new(),
        run_head_km: None,
        status: ItemStatus::Open,
        run_volume: None,
        rep_amount: None,
        run_lighter: false,
        rpe_target: matches!(l.intensity, LiftIntensity::Rpe(_)),
        hold_load_cap_kg: None,
        pick_load: false,
        basis_load_kg: load_kg,
        load_held: false,
    };
    refresh_lift_summary(&mut it, weight_lb);
    it
}

/// A %1RM target. The working LOAD = logged e1RM × % × this lift's single
/// derate (detraining, layoff re-entry ramp, or an e1RM drop; never stacked).
/// The e1RM itself stays the logged value (honest `anchored_on`). A >8 wk
/// layoff's pre-layoff anchor never reaches this arm: it is set aside and the
/// lift prescribed by RIR.
fn percent_arm(
    l: &crate::schema::LiftPrescription,
    p: f32,
    anchors: &crate::plan::Anchors,
    weight_lb: bool,
) -> LiftArm {
    let e1 = anchors.e1rm_for(&l.exercise);
    let derate = anchors.load_frac_for(&l.exercise);
    let frac = derate.map_or(1.0, |d| d.frac);
    let load = e1.map(|e| round_load_grid(e * p as f64 / 100.0 * frac, weight_lb));
    // The derate, not the loading band, set the number: re-point the card's
    // evidence triad to the rule that scaled it.
    let derate_claim = derate.map(|d| match d.cause {
        crate::plan::DerateCause::Detraining { .. } => claims::DETRAIN_STR_001,
        crate::plan::DerateCause::Reentry { .. } => claims::REENTRY_RAMP_001,
        crate::plan::DerateCause::E1rmDrop => claims::AUTOREG_E1RM_GATE_001,
    });
    let reps_txt = match l.reps_max.filter(|top| *top > l.reps) {
        Some(top) => format!("{}–{}", l.reps, top),
        None => l.reps.to_string(),
    };
    // The effort the percentage implies on the Epley model (85 %×3 → ~2 in
    // reserve), 1–4 RIR: the "how hard" target on the face and the editor's
    // RPE hint. A set that feels much harder than this means today sits below
    // the estimate (true 1RM varies day to day).
    let effort_rir = crate::strength::effort_target_rir(p as f64, l.reps as u32);
    if matches!(load, Some(kg) if kg <= 0.0) {
        // The anchor exists but shrinks below the plate grid (a tiny dumbbell
        // best, or a real anchor cut hard by a derate), so the rounded working
        // load is 0. Never render "@ 0 kg": prescribe by effort like an
        // unanchored lift, at the reserve the percentage implies (floored at
        // 1 RIR, never to failure).
        let rir = effort_rir as i32;
        return LiftArm {
            load_kg: None,
            intensity: PlanIntensity::Rir { rir },
            prose_label: String::new(),
            anchored_on: String::new(),
            anchored_sets: 0,
            basis: Some(format!(
                "{}×{} {}: {:.0}% of your estimated max is lighter than any weight you can load, so go by effort. Log a heavier set and the plan sets your weight from it.",
                l.sets, reps_txt, with_reps_left(i64::from(rir)), p
            )),
            improves: Some(
                "Log a heavier set of this lift so its weights can follow your measured max instead of an effort target."
                    .to_string(),
            ),
            target_rpe: Some(crate::strength::rir_to_rpe(rir as f64)),
            repoint: derate_claim,
        };
    }
    let anchored_on = e1
        .map(|e| {
            format!(
                "Estimated max {} (your recent best)",
                fmt_weight(e, weight_lb)
            )
        })
        .unwrap_or_default();
    let reps_word = if effort_rir == 1 { "rep" } else { "reps" };
    let effort = format!(
        "Aim to finish each set with about {effort_rir} {reps_word} left; if a set feels much harder than that, today is below your estimate, so stop the exercise there."
    );
    let (basis, improves) = match (load, e1) {
        (Some(kg), Some(e)) => {
            let scaled = match derate {
                Some(d) => format!(
                    ", scaled to {:.0}% {}",
                    d.frac * 100.0,
                    derate_reason(d.cause)
                ),
                None => String::new(),
            };
            (
                format!(
                    "{}×{} at {:.0}% of your estimated max ({}){scaled}: {}. {effort}",
                    l.sets,
                    reps_txt,
                    p,
                    fmt_weight(e, weight_lb),
                    fmt_weight(kg, weight_lb)
                ),
                // Anchored: the load is only as fresh as the e1RM behind it. A
                // rated session keeps it current.
                "Rate how hard each set felt so your estimated max, and next week's weights, stay current.",
            )
        }
        _ => (
            format!(
                "{}×{} at {:.0}% of your estimated max. {effort}",
                l.sets, reps_txt, p
            ),
            // No anchor yet: a logged set replaces the estimate with a
            // measured e1RM the load can key off.
            "Log a set of this lift so its weights can follow your measured max instead of a typical value.",
        ),
    };
    LiftArm {
        load_kg: load,
        intensity: PlanIntensity::PercentE1rm {
            pct: p.round() as u32,
            reduced_from: None,
        },
        prose_label: String::new(),
        anchored_on,
        anchored_sets: 0,
        basis: Some(basis),
        improves: Some(improves.to_string()),
        target_rpe: Some(crate::strength::rir_to_rpe(effort_rir as f64)),
        repoint: derate_claim.filter(|_| load.is_some()),
    }
}

/// A basis line's effort clause in the face wording: "with ~2 reps left", or
/// "to your last good rep" for a zero reserve.
fn with_reps_left(n: i64) -> String {
    if n <= 0 {
        reps_left(n)
    } else {
        format!("with {}", reps_left(n))
    }
}

/// A reps-in-reserve target: a hypertrophy rep range ([`rep_range_arm`]) or a
/// fixed rep count prescribed by effort until a set is logged. A >8 wk novice
/// re-entry (`reentry_novice`) lands here instead of on a %1RM anchor (KB
/// Table 3.4b: treat as novice, technique first); it has no load to derate
/// but still cites the re-entry reason so the card explains why the anchor is
/// set aside.
fn rir_arm(
    l: &crate::schema::LiftPrescription,
    n: u8,
    anchors: &crate::plan::Anchors,
    rx: &Recommended<Prescription>,
    weight_lb: bool,
) -> LiftArm {
    let arm = match l.reps_max.filter(|top| *top >= l.reps) {
        Some(top) => rep_range_arm(l, n, top, anchors, rx, weight_lb),
        None => LiftArm {
            load_kg: None,
            intensity: PlanIntensity::Rir { rir: n as i32 },
            prose_label: String::new(),
            anchored_on: String::new(),
            anchored_sets: 0,
            basis: Some(format!(
                "{}×{} {}. Log a set and the plan sets your weight from it.",
                l.sets,
                l.reps,
                with_reps_left(i64::from(n))
            )),
            // No e1RM anchor: loads are RIR-based until a set is logged.
            improves: Some(
                "Log a set of this lift so its weights can follow your measured max instead of an effort target."
                    .to_string(),
            ),
            target_rpe: Some(crate::strength::rir_to_rpe(n as f64)),
            repoint: None,
        },
    };
    LiftArm {
        repoint: anchors.reentry_novice.then_some(claims::REENTRY_001),
        ..arm
    }
}

/// Hypertrophy double progression (HYP-DOUBLEPROG-001): a rep range up to
/// `top`, a target effort of `n` reps in reserve, and (once logged) the
/// athlete's own last working load carried forward or stepped up.
fn rep_range_arm(
    l: &crate::schema::LiftPrescription,
    n: u8,
    top: u8,
    anchors: &crate::plan::Anchors,
    rx: &Recommended<Prescription>,
    weight_lb: bool,
) -> LiftArm {
    let (bottom, _) =
        crate::hypertrophy::rep_range_for(crate::hypertrophy::classify(&l.exercise)).value;
    let bottom = bottom.min(l.reps);
    let frac = anchors.load_frac_for(&l.exercise).map_or(1.0, |d| d.frac);
    let last = anchors.last_working_for(&l.exercise);
    let last_kg = last.map(|w| w.weight_kg).filter(|w| *w > 0.0);
    // A load the athlete actually lifted is carried exactly as logged (a 6 kg
    // dumbbell stays 6 kg); only a COMPUTED load (derate, deload, re-derived
    // range) lands on the plate grid. A step up lands on the grid when the
    // grid can express a step within the cap, else stays the capped step
    // itself (`hypertrophy::load_step_kg`, STR-LOADPROG-001 ≤ +10 %).
    // `+ 1e-6`: a load logged in lb converts to a kg float a hair under its own
    // grid point; never round it down a plate.
    let load = l.load_kg.filter(|kg| *kg > 0.0).and_then(|kg| {
        let kg = f64::from(kg);
        match last_kg {
            Some(w) if frac >= 1.0 && (kg - w).abs() < 1e-3 => Some(w),
            Some(w) if frac >= 1.0 && kg > w => {
                let grid = round_load_grid(kg + 1e-6, weight_lb);
                let capped_step = kg <= w + crate::hypertrophy::load_step_kg(w) + 1e-3;
                if capped_step && grid <= w + 1e-6 {
                    Some(kg)
                } else {
                    (grid > 0.0).then_some(grid)
                }
            }
            _ => round_load_or_drop(kg * frac + 1e-6, weight_lb),
        }
    });
    let last_shown = last.filter(|w| w.weight_kg > 0.0 && load.is_some());
    let anchored_on = last_shown
        .map(|w| {
            let reps: Vec<String> = w.sets.iter().map(|(r, _)| r.to_string()).collect();
            format!(
                "Last: {} × {}",
                fmt_weight(w.weight_kg, weight_lb),
                reps.join(", ")
            )
        })
        .unwrap_or_default();
    let rule = format!(
        "Add a rep each session. Once every set reaches {top} reps with at least {} left, the weight goes up a small step (at most a tenth) and you start again at {bottom}.",
        reps_word(n)
    );
    let reps_txt = if top > l.reps {
        format!("{}–{top}", l.reps)
    } else {
        l.reps.to_string()
    };
    let step = crate::hypertrophy::SMALLEST_LOAD_STEP_KG;
    let deload = rx.claim_id() == claims::HYP_DELOAD_RX_001;
    let effort = with_reps_left(i64::from(n));
    let basis = match (load, last) {
        _ if deload => format!(
            "Lighter week: {}×{reps_txt}{} with at least {} left, about half your usual sets at a lighter weight so you come back fresher.",
            l.sets,
            load.map(|kg| format!(" at {}", fmt_weight(kg, weight_lb)))
                .unwrap_or_default(),
            reps_word(n)
        ),
        (Some(kg), Some(w)) if kg > w.weight_kg + 0.01 && kg <= w.weight_kg + step + 0.01 => {
            format!(
                "{}×{reps_txt} at {} {effort}: every set reached the top of the range last time, so the weight goes up. {rule}",
                l.sets,
                fmt_weight(kg, weight_lb)
            )
        }
        (Some(kg), Some(w)) if (kg - w.weight_kg).abs() < 0.01 => format!(
            "{}×{reps_txt} at {} {effort}: same weight as last time, aim for {} reps per set. {rule}",
            l.sets,
            fmt_weight(kg, weight_lb),
            l.reps
        ),
        (Some(kg), _) => format!(
            "{}×{reps_txt} at {} {effort}: worked out for this rep range from your last session of this lift. {rule}",
            l.sets,
            fmt_weight(kg, weight_lb)
        ),
        (None, _) => format!(
            "{}×{reps_txt} {effort}: pick a weight you could lift about {} times. Log it and the next session carries your weight forward. {rule}",
            l.sets,
            u32::from(l.reps) + u32::from(n)
        ),
    };
    LiftArm {
        load_kg: load,
        intensity: PlanIntensity::Rir { rir: n as i32 },
        prose_label: String::new(),
        anchored_sets: last_shown.map_or(0, |w| w.sets.len()),
        anchored_on,
        basis: Some(basis),
        improves: Some(
            "Log every working set with how hard it felt so the next weight step comes at the right time."
                .to_string(),
        ),
        target_rpe: Some(crate::strength::rir_to_rpe(n as f64)),
        repoint: None,
    }
}

/// An RPE target. An unanchored lift prescribed at an RPE is a calibration set
/// (STR-E1RM-CHECK-001: a 3–6 rep test set): the athlete picks the weight, and
/// the logged set sets their working loads.
fn rpe_arm(l: &crate::schema::LiftPrescription, r: f32, anchors: &crate::plan::Anchors) -> LiftArm {
    let rir = crate::strength::rpe_to_rir(r as f64);
    let calibration = anchors.e1rm_for(&l.exercise).is_none();
    let reps_word = if rir.round() == 1.0 { "rep" } else { "reps" };
    let basis = if calibration {
        format!(
            "Warm up, then work up to {} set{} of {} with about {:.0} {reps_word} left (a weight you could lift about {:.0} times). Log it with how hard it felt; your working weights for this lift are set from it.",
            l.sets,
            if l.sets == 1 { "" } else { "s" },
            l.reps,
            rir,
            l.reps as f64 + rir
        )
    } else {
        format!(
            "{}×{} with about {:.0} {reps_word} left.",
            l.sets, l.reps, rir
        )
    };
    LiftArm {
        load_kg: None,
        intensity: PlanIntensity::Prose,
        prose_label: format!("RPE {}", fmt_rpe(r as f64)),
        anchored_on: String::new(),
        anchored_sets: 0,
        basis: Some(basis),
        improves: calibration.then(|| {
            "Log this set with how hard it felt so the plan can set this lift's weights from your measured max."
                .to_string()
        }),
        target_rpe: Some(r as f64),
        repoint: None,
    }
}

/// A ballistic jump set at maximal intent: no load and no effort target.
fn max_intent_arm(l: &crate::schema::LiftPrescription) -> LiftArm {
    LiftArm {
        intensity: PlanIntensity::Prose,
        prose_label: "max height".to_string(),
        basis: Some(format!(
            "{} sets of {} jumps ({} in total), each as high as you can, landing softly; rest about {} min between sets. End a set early if a jump is clearly lower than your first. Jumps come first, while you are fresh.",
            l.sets,
            l.reps,
            u32::from(l.sets) * u32::from(l.reps),
            (l.rest_sec as f64 / 60.0).round()
        )),
        ..LiftArm::default()
    }
}
