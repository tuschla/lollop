//! Calisthenics plan items and the rung picker.

use super::*;
use std::fmt::Write as _;

/// The face cue for a row done with no bar or rings (CAL-EQUIP-001).
const TABLE_ROW_CUE: &str = "use a sturdy table edge";

/// Flatten a `Prescription::Calisthenics` arm into its `PrescriptionView`.
/// Load is the leverage tier (never a settable bar weight), so `load_kg` stays
/// `None`; a MEASURED-fraction rung surfaces its honest %BW line (and an
/// effective-kg estimate when bodyweight is known) in `anchored_on`, and inferred
/// rungs surface no number (CAL-LOAD-INFERRED-001).
pub(super) fn flatten_calisthenics_prescription(
    c: &crate::schema::CalisthenicsPrescription,
    rx: &Recommended<Prescription>,
    anchors: &crate::plan::Anchors,
    bodyweight_kg: Option<f64>,
    weight_lb: bool,
) -> PrescriptionView {
    use crate::calisthenics as cal;
    let sh = CalisShape::of(c);
    let CalisShape {
        tier: t,
        timed,
        lo,
        hi,
        added,
        loaded,
        ..
    } = sh;

    let (reps_low, reps_high, rep_volume) = if timed {
        (0u8, 0u8, sh.hold_label.clone())
    } else {
        (lo as u8, hi as u8, String::new())
    };

    // Structured intensity: a dynamic rung is an adjustable RIR band; a timed
    // hold or a ballistic item carries a non-adjustable prose cue. The label is
    // rendered FROM it, so the RIR-band form matches "RIR {lo}–{hi}" exactly
    // (en-dash included) and CapRpe caps the band directly.
    let intensity = if timed || c.ballistic {
        PlanIntensity::Prose
    } else {
        PlanIntensity::RirRange {
            lo: c.rir.0 as i32,
            hi: c.rir.1 as i32,
        }
    };
    let intensity_label = intensity.label().unwrap_or_else(|| {
        if c.ballistic {
            "as fast as you can, stop when you slow down".to_string()
        } else {
            match t.map(|t| t.failure) {
                Some(cal::FailurePolicy::LeaveRir(_)) => {
                    "clean hold, stop before your form breaks".to_string()
                }
                _ => "hold until you almost can't".to_string(),
            }
        }
    });

    let anchored_on = calis_anchored_on(&sh, anchors, bodyweight_kg, weight_lb);
    let performance_only =
        c.ballistic || t.is_some_and(|t| cal::performance_gated(t, c.target, loaded));
    let mut why = why_from(
        Some(calis_basis(&sh, performance_only, bodyweight_kg, weight_lb)),
        rx,
    );
    why.improves = if c.needs_effort_rating {
        "Rate how hard the last set felt to move up to the next step.".to_string()
    } else if performance_only {
        "Log your reps (or hold time) so the plan can time your next step.".to_string()
    } else {
        "Log your reps (or hold time) and how hard the last set felt so the plan can time your \
         next step."
            .to_string()
    };
    let (lead, clauses) = calis_summary(&sh, &intensity, weight_lb);

    // Numeric prefill targets: hold band in seconds for a timed rung, and the
    // RPE mapped from the conservative (further-from-failure) end — the higher
    // RIR bound for a dynamic band, the tier's leave-RIR for a clean hold. A
    // ballistic item has no RPE target (power work states no numeric RIR).
    let (hold_low_sec, hold_high_sec) = if timed { (lo, hi) } else { (0u16, 0u16) };
    let target_rpe = if c.ballistic {
        None
    } else if timed {
        match t.map(|t| t.failure) {
            Some(cal::FailurePolicy::LeaveRir(n)) => Some(crate::strength::rir_to_rpe(n as f64)),
            _ => None,
        }
    } else {
        Some(crate::strength::rir_to_rpe(c.rir.1 as f64))
    };

    let mut it = PrescriptionView {
        summary: String::new(),
        exercise: c.exercise.clone(),
        sets: c.sets,
        reps_low,
        reps_high,
        load_kg: None,
        intensity_label,
        intensity,
        rest_sec: c.rest_sec,
        rep_count: 0,
        rep_volume,
        timed_hold: timed,
        target_rpe,
        hold_low_sec,
        hold_high_sec,
        // `Some(0.0)` = pure bodyweight; a load-capable rung's suggested load
        // (possibly 0) rides here too, and `calis.load_capable` tells the shell
        // to show the added-load field regardless.
        added_kg: Some(added),
        anchored_on,
        adjusted_note: String::new(),
        safety_reason: String::new(),
        evidence: EvidenceTagView::of(rx),
        why,
        effort_cue: if c.table_row {
            TABLE_ROW_CUE.to_string()
        } else {
            String::new()
        },
        calis: Some(CalisItemView {
            ladder: c.ladder,
            rung: c.tier,
            rung_count: cal::ladder(c.ladder).tiers.len() as u8,
            easier: c.easier.map(|r| calis_rung_view(c.ladder, r, true)),
            harder: c.harder.map(|r| calis_rung_view(c.ladder, r, true)),
            load_capable: c.load_capable,
            test_hold_sec: c.test_hold_sec.unwrap_or(0),
            ballistic: c.ballistic,
            needs_effort_rating: c.needs_effort_rating,
        }),
        ..Default::default()
    };
    set_named_summary(&mut it, ": ", lead, &clauses);
    it
}

/// The facts about a calisthenics prescription every part of its card reads.
struct CalisShape<'c> {
    c: &'c crate::schema::CalisthenicsPrescription,
    tier: Option<&'static crate::calisthenics::Tier>,
    timed: bool,
    /// The band floor and top (reps, or hold seconds for a timed rung).
    lo: u16,
    hi: u16,
    added: f64,
    loaded: bool,
    /// " per side" for a unilateral rung, else empty.
    per_side: &'static str,
    /// The timed-hold band, a degenerate band (lo == hi) collapsed to one
    /// figure so a fixed-duration rung reads "30 s hold", not "30–30 s hold".
    hold_label: String,
}

impl<'c> CalisShape<'c> {
    fn of(c: &'c crate::schema::CalisthenicsPrescription) -> Self {
        let tier = crate::calisthenics::tier(c.ladder, c.tier);
        let (lo, hi) = (c.target.floor(), c.target.top());
        let added = c.added_kg.unwrap_or(0.0);
        Self {
            c,
            tier,
            timed: c.target.is_timed(),
            lo,
            hi,
            added,
            loaded: added > 0.0,
            per_side: if tier.is_some_and(|t| t.unilateral) {
                " per side"
            } else {
                ""
            },
            hold_label: if lo == hi {
                format!("{hi} s hold")
            } else {
                format!("{lo}–{hi} s hold")
            },
        }
    }
}

/// The "anchored on" line: the MEASURED %BW line only (no invented number for
/// an inferred rung), plus the system-load e1RM of the newest loaded set
/// (CAL-E1RM-001).
fn calis_anchored_on(
    sh: &CalisShape,
    anchors: &crate::plan::Anchors,
    bodyweight_kg: Option<f64>,
    weight_lb: bool,
) -> String {
    use crate::calisthenics as cal;
    let c = sh.c;
    let mut anchored_on = match sh.tier.and_then(|t| t.bw_fraction.map(|f| (t, f))) {
        Some((tier, (flo, fhi))) => {
            let src = tier
                .load_claim
                .entry()
                .primary_citations
                .first()
                .copied()
                .unwrap_or("");
            let pct = if (flo - fhi).abs() < f64::EPSILON {
                format!("~{:.0}%", flo * 100.0)
            } else {
                format!("~{:.0}–{:.0}%", flo * 100.0, fhi * 100.0)
            };
            // The push/pull/dip ladders load the hands; the squat ladder loads the
            // working (stance/front) leg — never mislabel a leg share "at the hands".
            let site = if c.ladder == crate::schema::LadderId::Squat {
                "on the working leg"
            } else {
                "at the hands"
            };
            let mut s = format!("{pct} of your bodyweight {site} (measured, {src})");
            if let Some(bw) = bodyweight_kg
                && let Some(eff) = cal::effective_load_kg(tier, bw, sh.added)
            {
                let _ = write!(s, " · ≈ {} effective", fmt_weight(eff, weight_lb));
            }
            s
        }
        None => String::new(),
    };
    if let Some(bw) = bodyweight_kg
        && let Some((rung, kg, reps, rpe)) =
            anchors.calis_for(c.ladder).and_then(|a| a.last_loaded_set)
        && let Some(e1) = cal::system_load_e1rm(c.ladder, rung, bw, kg, reps, rpe)
    {
        if !anchored_on.is_empty() {
            anchored_on.push_str(" · ");
        }
        let _ = write!(
            anchored_on,
            "≈ {} estimated 1-rep max (bodyweight + added) from your last weighted set",
            fmt_weight(e1.value, weight_lb)
        );
    }
    anchored_on
}

/// The plain-language "why?" basis: where this step sits, what moves you on,
/// and the rule that shaped today's step. An advance on a load-capable rung
/// adds load rather than a rung (the leverage branch stays a "Harder" choice),
/// so it never promises the next rung.
fn calis_basis(
    sh: &CalisShape,
    performance_only: bool,
    bodyweight_kg: Option<f64>,
    weight_lb: bool,
) -> String {
    use crate::calisthenics as cal;
    let CalisShape {
        c,
        timed,
        lo,
        hi,
        added,
        loaded,
        per_side,
        ..
    } = *sh;
    let ladder = cal::ladder(c.ladder);
    let next_desc = if c.load_capable {
        "adding weight".to_string()
    } else {
        match cal::tier(c.ladder, c.tier + 1) {
            Some(nt) => nt.name.to_string(),
            None => "the hardest version".to_string(),
        }
    };
    let step_of = format!(
        "Step {}/{} of the {} progression",
        c.tier + 1,
        ladder.tiers.len(),
        ladder.pattern.to_lowercase()
    );
    let mut basis = if let Some(test) = c.test_hold_sec {
        format!(
            "{step_of}. Working holds stay at {lo}–{hi} s, well short of your max, to spare the \
             tendons. Once a week, finish with one test hold: a clean {test}-second hold on 2 \
             test days in a row moves you up to {next_desc}."
        )
    } else if c.ballistic {
        format!(
            "{step_of}. Explosive work: every rep as fast and high as you can, about 3 minutes' \
             rest, and stop a set as soon as you slow down, well before your limit. All sets at \
             {hi} reps{per_side} on 2 sessions moves you up to {next_desc}."
        )
    } else if timed {
        format!("{step_of}. Clean {hi}-second holds on 2 sessions move you up to {next_desc}.")
    } else if performance_only {
        format!(
            "{step_of}. Reaching {hi} reps{per_side} on every set on 2 sessions moves you up to \
             {next_desc}. Counted on reps, because reps-left guesses are least accurate on \
             high-rep sets."
        )
    } else {
        format!(
            "{step_of}. Reaching {hi} reps{per_side} on every set, with at least 1 rep left on the \
             last one, on 2 sessions in a row moves you up to {next_desc}."
        )
    };
    // CAL-INTENSITY-FLOOR-001: an easy high-rep rung must approach failure,
    // while harder leave-RIR rungs stop short — say why this one asks for less.
    if !loaded && !timed && !c.ballistic && c.rir.0 == 0 && cal::easy_rung(c.ladder, c.tier) {
        basis.push_str(
            " Easy versions like this only build strength when taken close to your limit, so this \
             one asks for fewer reps left than some harder steps.",
        );
    }
    if let Some(th) = c.skip_reps {
        let outcome = if c.load_capable {
            "the plan starts added weight after one session"
        } else {
            "you skip ahead after one session"
        };
        let _ = write!(
            basis,
            " Managing {th}+ reps{per_side} on every set means this step is too easy: {outcome}."
        );
    }
    if c.load_capable {
        if loaded {
            let step = cal::load_step_kg(bodyweight_kg, added, weight_lb).value;
            let _ = write!(
                basis,
                " Weighted: reach the top of the range on 2 sessions and the plan adds {} \
                 (about {}% of your bodyweight plus the added weight).",
                fmt_weight(step, weight_lb),
                cal::LOAD_STEP_SYSTEM_FRAC * 100.0
            );
        } else {
            basis.push_str(
                " This step takes added weight: reach the top of the range on 2 sessions and the \
                 plan starts a small belt or vest weight.",
            );
        }
        if let Some(h) = c.harder.and_then(|h| cal::tier(c.ladder, h)) {
            let _ = write!(
                basis,
                " Prefer a harder version instead of weight? Choose Harder for {}.",
                h.name
            );
        }
    }
    if let Some(b) = c.tempo_bridge {
        let _ = write!(
            basis,
            " A slow tempo, {} s down and a {} s pause at the hardest point, bridges the jump \
             up to the next step. Keep each rep well under 8 s.",
            b.eccentric_sec, b.pause_sec
        );
    }
    if c.reentry_steps > 0 {
        let steps = if c.reentry_steps == 1 {
            "one step".to_string()
        } else {
            format!("{} steps", c.reentry_steps)
        };
        let _ = write!(
            basis,
            " You haven't trained this for a while, so this is {steps} easier to ease back in."
        );
        if c.retest {
            basis
                .push_str(" Do one max-reps test set first: strong reps move you back up quickly.");
        }
    }
    if c.substituted {
        basis.push_str(" This is the nearest version your equipment allows.");
    }
    if c.table_row {
        basis.push_str(
            " No bar or rings: lie under a sturdy table and pull your chest to its edge; bend \
             your knees to make it easier.",
        );
    }
    if c.unplaced {
        let unit = if timed { "seconds" } else { "clean reps" };
        let _ = write!(
            basis,
            " New to this movement? Choose Easier or Harder to start at the hardest version you \
             can do for at least {lo} {unit}, or log one all-out set and the plan places you."
        );
    }
    basis
}

/// The face line after the name, as its lead and the clauses after it: "3×5–8"
/// with ["1–2 reps left"], or "4×5–10 s hold" (the summary reads "Full push-up:
/// 3×5–8 · 1–2 reps left").
fn calis_summary(
    sh: &CalisShape,
    intensity: &PlanIntensity,
    weight_lb: bool,
) -> (String, Vec<String>) {
    let CalisShape {
        c,
        timed,
        lo,
        hi,
        added,
        loaded,
        per_side,
        ..
    } = *sh;
    let mut lead = if timed {
        format!("{}×{}{per_side}", c.sets, sh.hold_label)
    } else if c.ballistic {
        format!("{}×{lo}–{hi}{per_side}, as fast as you can", c.sets)
    } else {
        format!("{}×{lo}–{hi}{per_side}", c.sets)
    };
    if loaded {
        let _ = write!(lead, " + {}", fmt_weight(added, weight_lb));
    }
    if let Some(b) = c.tempo_bridge {
        let _ = write!(
            lead,
            ", {} s down + {} s pause",
            b.eccentric_sec, b.pause_sec
        );
    }
    if let Some(test) = c.test_hold_sec {
        let _ = write!(lead, " (weekly test: {test} s clean)");
    }
    // The no-apparatus setup rides the lead, the line a compact row shows:
    // without it the row names a movement the athlete has nothing to do on.
    if c.table_row {
        let _ = write!(lead, " · {TABLE_ROW_CUE}");
    }
    let mut clauses = Vec::with_capacity(1);
    // A dynamic rung's effort, in the same "reps left" words as every lift.
    if let PlanIntensity::RirRange { lo: rlo, hi: rhi } = *intensity {
        clauses.push(if rlo == rhi {
            reps_left(i64::from(rlo))
        } else {
            format!("{rlo}–{rhi} reps left")
        });
    }
    (lead, clauses)
}

/// One rung as a picker row: canonical name, band label, equipment availability.
fn calis_rung_view(ladder: crate::schema::LadderId, rung: u8, available: bool) -> CalisRungView {
    let Some(t) = crate::calisthenics::tier(ladder, rung) else {
        return CalisRungView {
            rung,
            ..Default::default()
        };
    };
    let band = match t.target {
        crate::schema::TierTarget::Reps { low, high } if low == high => format!("{high} reps"),
        crate::schema::TierTarget::Reps { low, high } => format!("{low}–{high} reps"),
        crate::schema::TierTarget::HoldSec { low, high } if low == high => format!("{high} s hold"),
        crate::schema::TierTarget::HoldSec { low, high } => format!("{low}–{high} s hold"),
    };
    CalisRungView {
        rung,
        name: t.name.to_string(),
        target: if t.unilateral {
            format!("{band} per side")
        } else {
            band
        },
        available,
        timed_hold: matches!(t.target, crate::schema::TierTarget::HoldSec { .. }),
    }
}

/// Every ladder with its rungs for a `Calisthenics` profile (empty otherwise):
/// the setup rung picker and the Log chooser. The current rung is the replayed
/// anchor through today, moved to the nearest rung the equipment allows
/// (CAL-EQUIP-001) — the rung the plan actually prescribes — with the placed
/// rung and a plain-language note when it had to move.
pub(in crate::app) fn build_calisthenics_ladders(cx: &ViewCtx) -> Vec<CalisLadderView> {
    use crate::calisthenics as cal;
    let Some(profile) = cx
        .model
        .profile
        .as_ref()
        .filter(|p| p.resistance_modality == ResistanceModality::Calisthenics)
    else {
        return Vec::new();
    };
    let kit = cal::Kit::from_profile(profile);
    let anchors = cx.calis_anchors_now();
    cal::LADDERS
        .iter()
        .map(|l| {
            let anchored = anchors.iter().find(|a| a.ladder == l.id).map(|a| a.tier);
            let current_rung = anchored.and_then(|r| cal::nearest_available(l.id, r, kit));
            let placed_rung = anchored.filter(|&r| current_rung != Some(r));
            let equipment_note = placed_rung
                .and_then(|r| Some((cal::tier(l.id, r)?, cal::rung_equipment(l.id, r)?)))
                .map_or_else(String::new, |(placed, need)| {
                    match current_rung.and_then(|c| cal::tier(l.id, c)) {
                        Some(now) => format!(
                            "{} needs {need}, so you start at {} with the equipment you have.",
                            placed.name, now.name
                        ),
                        None if l.id == crate::schema::LadderId::VerticalPull => format!(
                            "{} needs {need}. Rows cover your pulling until you have one.",
                            placed.name
                        ),
                        None => format!(
                            "{} needs {need}, so this waits until you have it.",
                            placed.name
                        ),
                    }
                });
            CalisLadderView {
                ladder: l.id,
                pattern: l.pattern.to_string(),
                skill: l.skill,
                power: l.power,
                current_rung,
                placed_rung,
                equipment_note,
                rungs: (0..l.tiers.len() as u8)
                    .map(|r| calis_rung_view(l.id, r, cal::rung_available(l.id, r, kit)))
                    .collect(),
            }
        })
        .collect()
}
