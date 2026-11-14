//! Flattening a `Prescription::Lift` arm.

use crate::app::evidence_views::{grade_note_str, why_from};
use crate::app::format::{fmt_reps, fmt_rounded, fmt_weight};
use crate::app::plan_views::adjust_fold::reduce_percent_intensity;
use crate::app::plan_views::basis::planned_text;
use crate::app::plan_views::item_render::{
    derate_reason, face_rir, range_text, refresh_lift_summary, round_load_or_drop, sets_reps_lead,
};
use crate::app::plan_views::plan_item::{BasisPiece, BasisRx, PlanItem};
use crate::app::view_types::{EvidenceTagView, PlanIntensity, PrescriptionView};
use crate::evidence::{ClaimId, claims};
use crate::plates::{f32_load_ceil, round_load_grid};
use crate::schema::{LiftIntensity, Recommended, Rx, RxBasis};

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
    basis: Option<Vec<BasisPiece>>,
    /// The "why?" `improves` line: an ENGINE data-need (which input
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
    anchors: &crate::plan::LiftAnchors,
    rx: &Rx,
    weight_lb: bool,
) -> PlanItem {
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
        LiftIntensity::MaxIntent => max_intent_arm(l),
    };
    let intensity_label = intensity.label().unwrap_or(prose_label);
    // An arm's own basis states the prescription; restated if the face moves.
    let mut why = why_from(basis.as_deref().map(|b| planned_text(b, weight_lb)), rx);
    if improves.is_some() {
        why.improves = improves;
    }
    // Power/speed prescriptions carry a velocity intent (File 02 strength-023
    // "50–70% FAST"; AUTOREG-VL-001): the load is only a power stimulus at
    // maximal concentric intent. Without a velocity device nobody can perceive
    // a ~10% bar-speed loss, so the cue is the visible slowdown; the face
    // carries "move fast" (`effort_cue`), the basis the full sentence.
    let effort_cue = if l.velocity_loss_pct.is_some() {
        why.append_basis(
            "Move every rep as fast as you can; end the set when a rep visibly slows.",
        );
        "move fast".to_string()
    } else {
        String::new()
    };
    let mut evidence = EvidenceTagView::of(rx);
    if let Some(id) = repoint {
        let re = Recommended::new((), id);
        evidence = EvidenceTagView::of(&re);
        why.grade_note = grade_note_str(re.grade(), re.contested());
    }
    let mut it = PlanItem {
        view: PrescriptionView {
            exercise: l.exercise.clone(),
            sets: l.sets,
            reps_low: l.reps,
            reps_high: l.reps_max.unwrap_or(l.reps).max(l.reps),
            load_kg,
            intensity_label,
            rest_sec: l.rest_sec,
            target_rpe,
            anchored_on,
            anchored_on_last: anchored_sets > 0,
            evidence,
            why,
            effort_cue,
            ..PrescriptionView::default()
        },
        intensity,
        anchored_on_single_set: anchored_sets == 1,
        rpe_target: matches!(l.intensity, LiftIntensity::Rpe(_)),
        ..PlanItem::default()
    };
    refresh_lift_summary(&mut it, weight_lb);
    if let Some(pieces) = basis {
        it.basis_rx = Some(BasisRx {
            load_kg,
            lead: sets_reps_lead(&it),
            rir: face_rir(&it),
            pieces,
        });
    }
    it
}

/// A %1RM target. The working LOAD = logged e1RM × % × this lift's single
/// derate (detraining, layoff re-entry ramp, or an e1RM drop; never stacked),
/// and the shown percent is cut by the same derate, reduced from the planned
/// one, so the label states the share of the e1RM the load is. The e1RM
/// itself stays the logged value, so `anchored_on` shows what was logged.
/// The pre-layoff anchor of a layoff over 8 wk never reaches this arm: it is
/// set aside and the lift prescribed by RIR.
fn percent_arm(
    l: &crate::schema::LiftPrescription,
    p: f32,
    anchors: &crate::plan::LiftAnchors,
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
    let reps_txt = range_text(l.reps, l.reps_max.unwrap_or(l.reps));
    // The effort target: the Epley reps in reserve at the load given, as a
    // share of the (derated) estimate, so a load the plate grid floored is
    // asked at the effort it really takes and a set done as asked reads the
    // estimate back. An e1RM-drop cut adds one rep in reserve (autoreg-006,
    // extracted/06-autoregulation.md:134), capped at 4.
    let given_pct = match (load, e1) {
        (Some(kg), Some(e)) if kg > 0.0 && e > 0.0 => 100.0 * kg / (e * frac),
        _ => p as f64,
    };
    let drop_cut = derate.is_some_and(|d| d.cause == crate::plan::DerateCause::E1rmDrop);
    let effort_rir =
        (crate::strength::effort_target_rir(given_pct, l.reps as u32) + u8::from(drop_cut)).min(4);
    if matches!(load, Some(kg) if kg <= 0.0) {
        // The anchor exists but shrinks below the plate grid (a tiny dumbbell
        // best, or a real anchor cut hard by a derate), so the rounded working
        // load is 0. Never render "@ 0 kg": prescribe by effort like an
        // unanchored lift, at the reserve the percentage implies (floored at
        // 1 RIR, never to failure).
        let rir = effort_rir as i32;
        return LiftArm {
            intensity: PlanIntensity::Rir { rir },
            basis: Some(vec![
                lead(l.sets, &reps_txt),
                text(" "),
                BasisPiece::RepsLeft(i64::from(rir)),
                text(&format!(": {p:.0}% of your estimated max is lighter than any weight you can load, so go by effort. Log a heavier set and the plan sets your weight from it.")),
            ]),
            improves: Some(
                "Log a heavier set of this lift so its weights follow your estimated max instead of an effort target."
                    .to_string(),
            ),
            target_rpe: Some(crate::strength::rir_to_rpe(rir as f64)),
            repoint: derate_claim,
            ..LiftArm::default()
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
    let effort = [
        text(". Stop each set "),
        BasisPiece::RepsLeft(i64::from(effort_rir)),
        text(
            ". If a set feels much harder, you're below your estimate today: end the exercise there.",
        ),
    ];
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
                [
                    lead(l.sets, &reps_txt),
                    text(&format!(
                        " at {p:.0}% of your estimated max ({}){scaled}: ",
                        fmt_weight(e, weight_lb)
                    )),
                    BasisPiece::Load(kg),
                ]
                .into_iter()
                .chain(effort)
                .collect::<Vec<_>>(),
                // Anchored: the load is only as fresh as the e1RM behind it. A
                // rated session keeps it current.
                "Rate how hard each set felt so your estimated max, and next week's weights, stay current.",
            )
        }
        _ => (
            [
                lead(l.sets, &reps_txt),
                text(&format!(" at {p:.0}% of your estimated max")),
            ]
            .into_iter()
            .chain(effort)
            .collect(),
            // No anchor yet: a logged set replaces the estimate with a
            // measured e1RM the load can key off.
            "Log a set of this lift so its weights follow your estimated max instead of a typical value.",
        ),
    };
    let mut intensity = PlanIntensity::PercentE1rm {
        pct: p.round() as u32,
        reduced_from: None,
    };
    if load.is_some() && frac < 1.0 {
        reduce_percent_intensity(&mut intensity, 1.0 - frac);
    }
    LiftArm {
        load_kg: load,
        intensity,
        anchored_on,
        basis: Some(basis),
        improves: Some(improves.to_string()),
        target_rpe: Some(crate::strength::rir_to_rpe(effort_rir as f64)),
        repoint: derate_claim.filter(|_| load.is_some()),
        ..LiftArm::default()
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
    anchors: &crate::plan::LiftAnchors,
    rx: &Rx,
    weight_lb: bool,
) -> LiftArm {
    let arm = match l.reps_max.filter(|top| *top >= l.reps) {
        Some(top) => rep_range_arm(l, n, top, anchors, rx, weight_lb),
        None => LiftArm {
            intensity: PlanIntensity::Rir { rir: n as i32 },
            basis: Some(vec![
                lead(l.sets, &l.reps.to_string()),
                text(" "),
                BasisPiece::RepsLeft(i64::from(n)),
                text(". Log a set and the plan sets your weight from it."),
            ]),
            // No e1RM anchor: loads are RIR-based until a set is logged.
            improves: Some(
                "Log a set of this lift so its weights follow your estimated max instead of an effort target."
                    .to_string(),
            ),
            target_rpe: Some(crate::strength::rir_to_rpe(n as f64)),
            ..LiftArm::default()
        },
    };
    LiftArm {
        repoint: anchors
            .reentry_novice
            .then_some(claims::REENTRY_001)
            .or(arm.repoint),
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
    anchors: &crate::plan::LiftAnchors,
    rx: &Rx,
    weight_lb: bool,
) -> LiftArm {
    let (bottom, _) =
        crate::hypertrophy::rep_range_for(crate::exercise::classify(&l.exercise)).value;
    let bottom = bottom.min(l.reps);
    let derate = anchors.carried_derate_for(&l.exercise);
    let frac = derate.map_or(1.0, |d| d.frac);
    let last = anchors.last_working_for(&l.exercise);
    let last_kg = last.map(|w| w.weight_kg).filter(|w| *w > 0.0);
    // A load the athlete actually lifted is carried exactly as logged (a 6 kg
    // dumbbell stays 6 kg); only a COMPUTED load (derate, deload, re-derived
    // range) lands on the plate grid. A step up lands on the grid when the
    // grid can express a step within the cap, else stays the capped step
    // itself (`hypertrophy::load_step_kg`, STR-LOADPROG-001 ≤ +10 %).
    // `load_kg` is f32: an on-grid lb load can come back a hair under its grid
    // point, so it reaches the grid through `f32_load_ceil`.
    let drop = derate.is_some_and(|d| d.cause == crate::plan::DerateCause::E1rmDrop);
    // An e1RM-drop cut (autoreg-006) takes ~5 % off the last working load. A
    // load re-derived from that same hard session already sits under it: it
    // is not cut twice.
    let rederived_under_cut = drop
        && last_kg
            .zip(l.load_kg)
            .is_some_and(|(w, kg)| f64::from(kg) < w * frac);
    let load = l.load_kg.filter(|kg| *kg > 0.0).and_then(|kg| {
        let kg = f64::from(kg);
        match last_kg {
            Some(w) if frac >= 1.0 && (kg - w).abs() < 1e-3 => Some(w),
            Some(w) if frac >= 1.0 && kg > w => {
                let grid = round_load_grid(f32_load_ceil(kg), weight_lb);
                let capped_step = kg <= w + crate::hypertrophy::load_step_kg(w) + 1e-3;
                if capped_step && grid <= w + 1e-6 {
                    Some(kg)
                } else {
                    (grid > 0.0).then_some(grid)
                }
            }
            Some(w) if drop && kg < w => {
                round_load_or_drop(f32_load_ceil(kg).min(w * frac), weight_lb)
            }
            _ => round_load_or_drop(f32_load_ceil(kg) * frac, weight_lb),
        }
    });
    let last_shown = last.filter(|w| w.weight_kg > 0.0 && load.is_some());
    // An e1RM-drop cut also leaves one more rep in reserve, as on the %1RM
    // arm (autoreg-006).
    let drop_cut = load.is_some() && drop;
    let effort_rir = if drop_cut { (n + 1).min(4).max(n) } else { n };
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
    let rule = [
        text(&format!(
            "Add a rep each session. Once every set reaches {top} reps with "
        )),
        BasisPiece::AtLeast(i64::from(n)),
        text(&format!(
            ", the weight goes up a small step (at most a tenth) and you start again at {bottom}."
        )),
    ];
    let reps_txt = range_text(l.reps, top);
    let step = crate::hypertrophy::SMALLEST_LOAD_STEP_KG;
    let deload = rx.kind() == RxBasis::HypertrophyDeload;
    let effort = BasisPiece::RepsLeft(i64::from(effort_rir));
    // "3×8–12 at 60 kg with ~2 reps left<why>. <rule>"
    let loaded = |kg: f64, why: String| {
        [
            lead(l.sets, &reps_txt),
            text(" at "),
            BasisPiece::Load(kg),
            text(" "),
            effort.clone(),
            text(&why),
        ]
        .into_iter()
        .chain(rule.clone())
        .collect::<Vec<_>>()
    };
    let basis = match (load, last) {
        _ if deload => {
            let mut b = vec![text("Lighter week: "), lead(l.sets, &reps_txt)];
            if let Some(kg) = load {
                b.extend([text(" at "), BasisPiece::Load(kg)]);
            }
            b.extend([
                text(" with "),
                BasisPiece::AtLeast(i64::from(n)),
                text(", about half your usual sets at a lighter weight."),
            ]);
            b
        }
        (Some(kg), Some(w)) if kg > w.weight_kg + 0.01 && kg <= w.weight_kg + step + 0.01 => {
            loaded(
                kg,
                ": every set reached the top of the range last time, so the weight goes up. "
                    .to_string(),
            )
        }
        (Some(kg), Some(w)) if (kg - w.weight_kg).abs() < 0.01 => loaded(
            kg,
            format!(
                ": same weight as last time, aim for {} per set. ",
                fmt_reps(l.reps.into())
            ),
        ),
        (Some(kg), _) if drop_cut && !rederived_under_cut => loaded(
            kg,
            format!(
                ", scaled to {:.0}% {}. ",
                frac * 100.0,
                derate_reason(crate::plan::DerateCause::E1rmDrop)
            ),
        ),
        (Some(kg), _) => loaded(
            kg,
            ": worked out for this rep range from your last session of this lift. ".to_string(),
        ),
        (None, _) => [
            lead(l.sets, &reps_txt),
            text(" "),
            effort.clone(),
            text(": pick a weight you "),
            BasisPiece::CouldLift {
                reps: i64::from(l.reps),
                rir: i64::from(n),
            },
            text(". Log it and the next session carries your weight forward. "),
        ]
        .into_iter()
        .chain(rule.clone())
        .collect(),
    };
    LiftArm {
        load_kg: load,
        intensity: PlanIntensity::Rir { rir: n as i32 },
        anchored_sets: last_shown.map_or(0, |w| w.sets.len()),
        anchored_on,
        basis: Some(basis),
        improves: Some(
            "Log every working set with how hard it felt so the next weight step comes at the right time."
                .to_string(),
        ),
        target_rpe: Some(crate::strength::rir_to_rpe(effort_rir as f64)),
        repoint: drop_cut.then_some(claims::AUTOREG_E1RM_GATE_001),
        ..LiftArm::default()
    }
}

/// An RPE target. An unanchored lift prescribed at an RPE is a calibration set
/// (STR-E1RM-CHECK-001: a 3–6 rep test set): the athlete picks the weight, and
/// the logged set sets their working loads.
fn rpe_arm(
    l: &crate::schema::LiftPrescription,
    r: f32,
    anchors: &crate::plan::LiftAnchors,
) -> LiftArm {
    let rir = crate::strength::rpe_to_rir(r as f64).round();
    let calibration = anchors.e1rm_for(&l.exercise).is_none();
    let effort = BasisPiece::RepsLeft(rir as i64);
    let basis = if calibration {
        vec![
            text(&format!(
                "Warm up, then work up to {} set{} of {} ",
                l.sets,
                if l.sets == 1 { "" } else { "s" },
                l.reps
            )),
            effort,
            text(" (a weight you "),
            BasisPiece::CouldLift {
                reps: i64::from(l.reps),
                rir: rir as i64,
            },
            text(
                "). Log it with how hard it felt; your working weights for this lift are set from it.",
            ),
        ]
    } else {
        vec![
            lead(l.sets, &l.reps.to_string()),
            text(" "),
            effort,
            text("."),
        ]
    };
    LiftArm {
        intensity: PlanIntensity::Prose,
        prose_label: format!("RPE {}", fmt_rounded(f64::from(r))),
        basis: Some(basis),
        improves: calibration.then(|| {
            "Log this set with how hard it felt so the plan can set this lift's weights from it."
                .to_string()
        }),
        target_rpe: Some(r as f64),
        ..LiftArm::default()
    }
}

/// A ballistic jump set at maximal intent: no load and no effort target.
fn max_intent_arm(l: &crate::schema::LiftPrescription) -> LiftArm {
    LiftArm {
        intensity: PlanIntensity::Prose,
        prose_label: "max height".to_string(),
        basis: Some(vec![text(&format!(
            "{} sets of {} jumps ({} in total), each as high as you can, landing softly; rest about {} min between sets. End a set early if a jump is clearly lower than your first. Jumps come first, while you are fresh.",
            l.sets,
            l.reps,
            u32::from(l.sets) * u32::from(l.reps),
            (l.rest_sec as f64 / 60.0).round()
        ))]),
        ..LiftArm::default()
    }
}

fn text(t: &str) -> BasisPiece {
    BasisPiece::Text(t.to_string())
}

fn lead(sets: u8, reps: &str) -> BasisPiece {
    BasisPiece::Lead(format!("{sets}×{reps}"))
}
