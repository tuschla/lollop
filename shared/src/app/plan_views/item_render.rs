//! A plan item's face rendered from its structured fields: the summary line,
//! the effort wording, the plates and the run amounts.

use super::*;

/// Rebuild a lift item's one-line summary from its (possibly adjusted) fields.
pub(super) fn refresh_lift_summary(it: &mut PrescriptionView, weight_lb: bool) {
    if it.is_run() {
        return; // a run item - its summary is not sets×reps
    }
    // A timed-hold calisthenics item has no reps (reps_low == 0): render the hold
    // volume (carried in `rep_volume`, e.g. "30 s hold"), never "×0".
    if it.timed_hold {
        let lead = format!("{}×{}", it.sets, it.rep_volume);
        let label = it.intensity_label.clone();
        set_named_summary(it, ": ", lead, &[label]);
        return;
    }
    // Dynamic items show the rep RANGE when the plan gives one (calisthenics
    // rungs are "5–8"); fixed-rep lifts (reps_high == reps_low) show one figure.
    let reps = if it.reps_high > it.reps_low {
        format!("{}–{}", it.reps_low, it.reps_high)
    } else {
        it.reps_low.to_string()
    };
    // The face line: load in the headline, then ONE effort statement in the
    // "reps left" vocabulary, then a speed/jump item's execution cue ("move
    // fast"). A %e1RM item shows its load, not the percentage (the percentage
    // and the e1RM behind it stay in `intensity_label` / the "?" basis); its
    // effort comes from `target_rpe`, so an adjustment that moves the target
    // moves the line. An effort-prescribed lift with a suggested starting
    // weight from its last session shows that weight in the headline too; a
    // prose (easy / light / pain-free) target never borrows it.
    let load = it.load_kg.or_else(|| {
        it.suggested_load_kg
            .filter(|_| !matches!(it.intensity, PlanIntensity::Prose))
    });
    // A lift whose load the day took away says what to load instead: a
    // weight for its reps-left target ("6×5 · pick a weight for ~3 reps
    // left"), the line a compact row shows.
    let pick_rir = it
        .target_rpe
        .filter(|_| it.pick_load && load.is_none())
        .map(|rpe| crate::strength::rpe_to_rir(rpe).round() as i64)
        .filter(|rir| *rir >= 1);
    let mut parts: Vec<String> = Vec::with_capacity(2);
    let effort = match pick_rir {
        Some(_) if matches!(it.intensity, PlanIntensity::Prose) => it.intensity_label.clone(),
        Some(_) => String::new(),
        None => effort_phrase(it, load.is_some()),
    };
    if !effort.is_empty() {
        parts.push(effort);
    }
    // A calisthenics item's cue is its no-apparatus setup ("use a sturdy
    // table edge"): it rides the lead, the line a compact row shows.
    let setup_cue = it.calis.is_some() && !it.effort_cue.is_empty();
    if !it.effort_cue.is_empty() && !setup_cue {
        parts.push(it.effort_cue.clone());
    }
    let mut lead = match load {
        Some(kg) => format!("{}×{} @ {}", it.sets, reps, fmt_weight(kg, weight_lb)),
        None => format!("{}×{}", it.sets, reps),
    };
    if let Some(rir) = pick_rir {
        lead = format!("{lead} · pick a weight for {}", reps_left(rir));
    }
    if setup_cue {
        lead = format!("{lead} · {}", it.effort_cue);
    }
    set_named_summary(it, ": ", lead, &parts);
    // The plate breakdown always follows the load actually shown.
    it.plates_per_side = if crate::plan::is_barbell_lift(&it.exercise) {
        it.load_kg
            .or(it.suggested_load_kg)
            .map(|kg| plates_per_side(kg, weight_lb))
            .unwrap_or_default()
    } else {
        Vec::new()
    };
}

/// Set a named item's `summary` (`"<exercise><sep><lead> · <clause> · …"`) and
/// the face parts the shell renders from it: the name line, the prescription
/// line after it and that line's lead.
pub(super) fn set_named_summary(
    it: &mut PrescriptionView,
    sep: &str,
    lead: String,
    clauses: &[String],
) {
    let mut detail = lead.clone();
    for c in clauses {
        detail.push_str(" · ");
        detail.push_str(c);
    }
    it.summary = format!("{}{sep}{detail}", it.exercise);
    it.face_name.clone_from(&it.exercise);
    it.face_detail = detail;
    it.face_lead = lead;
}

/// Set a run item's `summary` to `"<head> · <rest>"` (just `rest` without a
/// head) and its face parts: `run_head` is the head (`head_km` its distance
/// when it is one), and the compact row's lead is the head, else `rest_lead`
/// (the first clause of `rest`).
pub(super) fn set_run_summary(
    it: &mut PrescriptionView,
    head: Option<String>,
    head_km: Option<f64>,
    rest: &str,
    rest_lead: &str,
) {
    it.face_name.clear();
    it.face_detail.clear();
    it.run_head_km = head.as_ref().and(head_km);
    match head {
        Some(h) => {
            it.summary = format!("{h} · {rest}");
            it.face_lead.clone_from(&h);
            it.run_head = h;
        }
        None => {
            it.summary = rest.to_string();
            it.face_lead = rest_lead.to_string();
            it.run_head.clear();
        }
    }
}

/// "~2 reps left" for a single reps-in-reserve target: the one effort wording
/// every face uses (0 = "to your last good rep", never "0 reps left").
pub(super) fn reps_left(n: i64) -> String {
    match n {
        n if n <= 0 => "to your last good rep".to_string(),
        1 => "~1 rep left".to_string(),
        n => format!("~{n} reps left"),
    }
}

/// A lift item's effort in the face vocabulary: a structured reps-in-reserve
/// target as "~N reps left" / "1–2 reps left"; a %e1RM target by the reserve
/// its percentage implies (`target_rpe`), falling back to the percentage only
/// when it implies none; an "RPE n" prose target as its reps left; any other
/// prose target verbatim.
fn effort_phrase(it: &PrescriptionView, load_shown: bool) -> String {
    let from_rpe = |rpe: f64| reps_left(crate::strength::rpe_to_rir(rpe).round() as i64);
    match it.intensity {
        PlanIntensity::Rir { rir } => reps_left(i64::from(rir)),
        PlanIntensity::RirRange { lo, hi } if lo == hi => reps_left(i64::from(lo)),
        PlanIntensity::RirRange { lo, hi } => format!("{lo}–{hi} reps left"),
        PlanIntensity::PercentE1rm { .. } => match (load_shown, it.target_rpe) {
            (_, Some(rpe)) => from_rpe(rpe),
            (true, None) => String::new(),
            (false, None) => it.intensity_label.clone(),
        },
        PlanIntensity::Prose => match it.target_rpe {
            Some(rpe) if it.rpe_target => from_rpe(rpe),
            _ => it.intensity_label.clone(),
        },
    }
}

/// Plates per side of a standard bar (20 kg, or 45 lb for an lb profile) for
/// a total `load_kg`, largest first, in the display unit. Greedy over the
/// common plate set (20/15/10/5/2.5/1.25 kg; 45/35/25/10/5/2.5 lb). Loads are
/// already on the display-unit grid, so the greedy split is exact for every
/// grid load; a remainder below the smallest plate is dropped (never an
/// invented plate). Empty at or below the bar, and for a non-finite or
/// implausible load (more than `MAX_PLATES_PER_SIDE` plates would be needed —
/// no real sleeve holds that), so a corrupt wire weight can never blow up the
/// view. Arithmetic only, no claim.
pub(in crate::app) fn plates_per_side(load_kg: f64, weight_lb: bool) -> Vec<f64> {
    const MAX_PLATES_PER_SIDE: f64 = 20.0;
    let (total, bar, plates): (f64, f64, &[f64]) = if weight_lb {
        (
            weight_num(load_kg, true),
            45.0,
            &[45.0, 35.0, 25.0, 10.0, 5.0, 2.5],
        )
    } else {
        (load_kg, 20.0, &[20.0, 15.0, 10.0, 5.0, 2.5, 1.25])
    };
    // Round the per-side figure to 0.01 so a kg→lb float (e.g. 224.9999) does
    // not lose its last plate.
    let mut side = (((total - bar) / 2.0) * 100.0).round() / 100.0;
    if !side.is_finite() || side <= 0.0 || side / plates[0] > MAX_PLATES_PER_SIDE {
        return Vec::new();
    }
    let mut out = Vec::new();
    for &p in plates {
        let n = ((side + 1e-9) / p).floor().max(0.0);
        out.extend(std::iter::repeat_n(p, n as usize));
        side -= n * p;
    }
    if out.len() as f64 > MAX_PLATES_PER_SIDE {
        return Vec::new();
    }
    out
}

/// Plain-language reason a lift's working load is scaled (the clause after
/// "scaled to N%").
pub(super) fn derate_reason(cause: crate::plan::DerateCause) -> String {
    match cause {
        crate::plan::DerateCause::Detraining { days_since } => format!(
            "after about {} weeks off this lift",
            (days_since as f64 / 7.0).round().max(1.0)
        ),
        crate::plan::DerateCause::Reentry { day, ramp_days } => format!(
            "in week {} of {} back from your break",
            day / 7 + 1,
            (ramp_days / 7).max(1)
        ),
        crate::plan::DerateCause::E1rmDrop => format!(
            "because your last session came in more than {:.0}% under your estimate",
            (1.0 - crate::app::anchors::E1RM_DROP_GATE) * 100.0
        ),
    }
}

/// Round a working load DOWN onto the profile's display-unit plate grid
/// (`round_load_grid`), dropping it to `None` when it collapses to 0. A sub-grid
/// anchor — or a real anchor cut below a loadable weight by a readiness/re-entry
/// derate — must never render "@ 0"; the item is prescribed by effort instead
/// (mirroring the non-positive-e1RM guard in `build_plan_anchors`).
pub(super) fn round_load_or_drop(kg: f64, weight_lb: bool) -> Option<f64> {
    let r = round_load_grid(kg, weight_lb);
    (r > 0.0).then_some(r)
}

impl PlanIntensity {
    /// The display label for a structured (adjustable) target, or `None` for a
    /// `Prose` target whose label is carried verbatim in `intensity_label`.
    /// Labels are rendered FROM the structured field so an adjustment mutates the
    /// numbers and never the string. The en-dash in the range form
    /// matches the calisthenics rung label exactly.
    pub(super) fn label(&self) -> Option<String> {
        match self {
            PlanIntensity::Rir { rir } => Some(format!("RIR {rir}")),
            PlanIntensity::RirRange { lo, hi } => Some(format!("RIR {lo}–{hi}")),
            PlanIntensity::PercentE1rm {
                pct,
                reduced_from: None,
            } => Some(format!("{pct}% e1RM")),
            PlanIntensity::PercentE1rm {
                pct,
                reduced_from: Some(orig),
            } => Some(format!("{pct}% e1RM (reduced from {orig}%)")),
            PlanIntensity::Prose => None,
        }
    }
}

/// Re-render an item's `intensity_label` from its (just-mutated) structured
/// `intensity`, then rebuild the one-line summary — the single sync point every
/// adjustment routes through so the label never drifts from the numbers.
/// A `Prose` target keeps its verbatim label.
pub(super) fn resync_intensity(it: &mut PrescriptionView, weight_lb: bool) {
    if let Some(label) = it.intensity.label() {
        it.intensity_label = label;
    }
    refresh_lift_summary(it, weight_lb);
}

impl RunAmount {
    pub(super) fn of(v: RunVolume) -> RunAmount {
        match v {
            RunVolume::DurationMin(m) => RunAmount::Minutes(f64::from(m)),
            RunVolume::DistanceKm(k) => RunAmount::PlanKm(k),
            RunVolume::RunWalk { .. } => RunAmount::RunWalk,
        }
    }

    /// The amount as card text; `None` for run/walk bouts, which read as
    /// their bouts, not a single number.
    pub(super) fn label(self) -> Option<String> {
        match self {
            RunAmount::Minutes(m) => Some(format!("{m:.0} min")),
            RunAmount::MinutesRange(lo, hi) => Some(format!("{lo}–{hi} min")),
            RunAmount::PlanKm(k) => Some(fmt_run_km(k)),
            RunAmount::Km(k) => Some(format!("{k:.1} km")),
            RunAmount::RunWalk => None,
        }
    }

    /// The km figure [`Self::label`] shows; `None` when the amount is not a
    /// distance.
    pub(super) fn shown_km(self) -> Option<f64> {
        let tenths = |km: f64| (km * 10.0).round() / 10.0;
        match self {
            RunAmount::PlanKm(k) => Some(tenths(f64::from(k))),
            RunAmount::Km(k) => Some(tenths(k)),
            RunAmount::Minutes(_) | RunAmount::MinutesRange(..) | RunAmount::RunWalk => None,
        }
    }

    /// `keep` of the amount, as a readiness cut shows it, from the shown
    /// (one-decimal) kilometres; `None` when there is no single number to cut.
    pub(super) fn scaled(self, keep: f64) -> Option<RunAmount> {
        let tenths = |km: f64| (km * 10.0).round() / 10.0;
        match self {
            RunAmount::Minutes(m) => Some(RunAmount::Minutes((m * keep).round().max(1.0))),
            RunAmount::PlanKm(k) => Some(RunAmount::Km((tenths(f64::from(k)) * keep).max(0.1))),
            RunAmount::Km(k) => Some(RunAmount::Km((tenths(k) * keep).max(0.1))),
            RunAmount::MinutesRange(..) | RunAmount::RunWalk => None,
        }
    }
}

/// The whole-session volume of an interval run as one continuous amount: the
/// rep count × the per-rep volume ("6 × 4 min" → 24 min, "6 × 800 m" →
/// 4.8 km). `None` for a continuous run or run/walk reps. The recoveries are
/// not counted (their length is not on the item).
fn interval_total(it: &PrescriptionView) -> Option<RunAmount> {
    if it.rep_count == 0 {
        return None;
    }
    let n = f64::from(it.rep_count);
    match it.rep_amount? {
        RunAmount::Minutes(m) => Some(RunAmount::Minutes(n * m)),
        // The rep shows as whole metres ("800 m"); the total sums those.
        RunAmount::PlanKm(k) => Some(RunAmount::Km(n * (f64::from(k) * 1000.0).round() / 1000.0)),
        RunAmount::Km(_) | RunAmount::MinutesRange(..) | RunAmount::RunWalk => None,
    }
}

/// A run item's shown whole-session volume as `(km, minutes)`, whichever the
/// plan states (an interval day: its reps' total; a minutes band: its low
/// end). Both `None` for run/walk bouts, which have no single number.
pub(in crate::app) fn run_planned(it: &PrescriptionView) -> (Option<f64>, Option<f64>) {
    let vol = if it.rep_count > 0 {
        interval_total(it)
    } else {
        it.run_volume
    };
    match vol {
        Some(RunAmount::PlanKm(k)) => (Some((f64::from(k) * 100.0).round() / 100.0), None),
        Some(RunAmount::Km(k)) => (Some(k), None),
        Some(RunAmount::Minutes(m)) => (None, Some(m)),
        Some(RunAmount::MinutesRange(lo, _)) => (None, Some(f64::from(lo))),
        Some(RunAmount::RunWalk) | None => (None, None),
    }
}

/// The intensity label of a run capped to easy.
pub(super) const EASY_RUN_LABEL: &str = "Easy: zone 1–2";

/// Cap a RUN item to an easy target: keep the bounded volume but replace the
/// hard intensity with an easy zone. An interval day keeps its numbers as one
/// continuous easy run of the same total work ("6 × 4 min" → "24 min"; never a
/// card with no duration). Used by DowngradeSession / CapTopEnd /
/// ModifyAndMonitor so a run prescription is actually modified, not merely
/// annotated.
pub(super) fn cap_run_item_easy(
    it: &mut PrescriptionView,
    r: &Recommended<Adjustment>,
    note: &str,
) {
    // Downgraded to a continuous easy run: the original rep structure is
    // deferred, not prescribed, so drop it (and any "N × …" rep or bout
    // structure that would otherwise read as an interval on an easy card),
    // carrying its total.
    let vol = if it.rep_count > 0 {
        interval_total(it)
    } else {
        it.run_volume.filter(|v| *v != RunAmount::RunWalk)
    };
    it.rep_count = 0;
    it.rep_volume = String::new();
    it.rep_amount = None;
    it.run_volume = vol;
    // The tracker walks `run_structure`: drop the reps and strides with the
    // card's hard work. A beginner's run/walk bouts are the easy session itself
    // (taking them away would make it harder), so they stay with their warm-up.
    it.run_structure = it
        .run_structure
        .take()
        .filter(|rs| !rs.run_walk.is_empty())
        .map(|rs| RunStructureView {
            warmup_min: rs.warmup_min,
            cooldown_min: rs.cooldown_min,
            run_walk: rs.run_walk,
            ..RunStructureView::default()
        });
    it.intensity_label = EASY_RUN_LABEL.into();
    // Re-point the numeric run target to the easy band (may append the easy
    // pace/bpm to the label).
    crate::app::run_targets::downgrade_run_target_to_easy(it);
    let label = it.intensity_label.clone();
    set_run_summary(
        it,
        vol.and_then(RunAmount::label),
        vol.and_then(RunAmount::shown_km),
        &label,
        EASY_RUN_LABEL,
    );
    it.adjusted_note = note.to_string();
    // The item was downgraded to easy, so EVERY evidence field must surface the
    // decision that downgraded it, this readiness/safety adjustment `r`, not the
    // stale ORIGINAL band (e.g. Tempo Strong). Re-point the grade chip + citation
    // + confidence to `r`, and rebuild the whole why? triad from `r` via
    // `why_from` (basis overridden with the honest downgrade rationale, grade_note
    // now consistent with the re-pointed chip, improves keyed to `r`'s claim).
    it.evidence = EvidenceTagView::of(r);
    it.safety_reason.clear();
    it.safety_reason_mi.clear();
    it.why = why_from(
        Some(format!(
            "Changed to an easy run ({note}). The planned harder effort waits for another day."
        )),
        r,
    );
}

/// Render an RPE for a set summary: integer when whole (`8`), else one decimal.
pub(super) fn fmt_rpe(rpe: f64) -> String {
    if rpe.fract() == 0.0 {
        format!("{rpe:.0}")
    } else {
        format!("{rpe:.1}")
    }
}
