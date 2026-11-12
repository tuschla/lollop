//! A plan item's face rendered from its structured fields: the summary line,
//! the effort wording, the plates and the run amounts.

use crate::app::evidence_views::why_from;
use crate::app::format::{fmt_km_or_mi, fmt_reps, fmt_weight, round_tenths};
use crate::app::plan_views::flatten_run::fmt_run_km;
use crate::app::plan_views::plan_item::{PlanItem, RunAmount};
use crate::app::view_types::{
    EvidenceTagView, PlanIntensity, PrescriptionView, RunStructureView, WhyView,
};
use crate::autoreg;
use crate::plates::{plates_per_side, round_load_grid};
use crate::profile::DistanceUnit;
use crate::schema::{AdjustmentCall, RunVolume};

pub(super) fn refresh_lift_summary(it: &mut PlanItem, weight_lb: bool) {
    if !it.is_lift() {
        return;
    }
    // A timed-hold calisthenics item has no reps (reps_low == 0): render the hold
    // volume (carried in `rep_volume`, e.g. "30 s hold"), never "×0".
    if it.timed_hold {
        let lead = FaceClause::plain(format!("{}×{}", it.sets, it.rep_volume));
        let label = FaceClause::plain(it.intensity_label.clone());
        set_named_summary(it, &[lead], &[label]);
        return;
    }
    let reps = range_text(it.reps_low, it.reps_high);
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
    let pick_rir = pick_rir(it);
    let mut parts: Vec<FaceClause> = Vec::with_capacity(2);
    let effort = match pick_rir {
        Some(_) if matches!(it.intensity, PlanIntensity::Prose) => {
            Some(FaceClause::plain(it.intensity_label.clone()))
        }
        Some(_) => None,
        None => effort_phrase(it, load.is_some()),
    };
    parts.extend(effort.filter(|c| !c.text.is_empty()));
    // A calisthenics item's cue is its no-apparatus setup ("use a sturdy
    // table edge"): it rides the lead, the line a compact row shows.
    let setup_cue = it.calis.is_some() && !it.effort_cue.is_empty();
    if !it.effort_cue.is_empty() && !setup_cue {
        parts.push(FaceClause::plain(it.effort_cue.clone()));
    }
    let mut lead = vec![FaceClause::plain(match load {
        Some(kg) => format!("{}×{} @ {}", it.sets, reps, fmt_weight(kg, weight_lb)),
        None => format!("{}×{}", it.sets, reps),
    })];
    if let Some(rir) = pick_rir {
        lead.push(FaceClause::effort(format!(
            "pick a weight for {}",
            reps_left(rir)
        )));
    }
    if setup_cue {
        lead.push(FaceClause::plain(it.effort_cue.clone()));
    }
    set_named_summary(it, &lead, &parts);
    it.plates_per_side = if crate::exercise::is_barbell_lift(&it.exercise) {
        it.load_kg
            .or(it.suggested_load_kg)
            .map(|kg| plates_per_side(kg, weight_lb))
            .unwrap_or_default()
    } else {
        Vec::new()
    };
}

/// One ` · ` clause of a face line. `effort` marks the reps-left wording
/// ("~2 reps left", "pick a weight for ~3 reps left"), which a face whose
/// newcomer guide states the effort leaves out.
pub(super) struct FaceClause {
    pub(super) text: String,
    pub(super) effort: bool,
}

impl FaceClause {
    pub(super) fn plain(text: String) -> Self {
        FaceClause {
            text,
            effort: false,
        }
    }

    pub(super) fn effort(text: String) -> Self {
        FaceClause { text, effort: true }
    }
}

fn join_clauses<'a>(clauses: impl Iterator<Item = &'a FaceClause>, with_effort: bool) -> String {
    clauses
        .filter(|c| with_effort || !c.effort)
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Set a named item's `summary` (`"<exercise>: <lead> · <clause> · …"`) and
/// the face parts the shell renders from it: the name line, the prescription
/// line after it (with and without its effort wording) and that line's lead.
pub(super) fn set_named_summary(
    it: &mut PrescriptionView,
    lead: &[FaceClause],
    clauses: &[FaceClause],
) {
    let detail = join_clauses(lead.iter().chain(clauses), true);
    it.summary = format!("{}: {detail}", it.exercise);
    it.face_name.clone_from(&it.exercise);
    it.face_detail = detail;
    it.face_detail_without_effort = join_clauses(lead.iter().chain(clauses), false);
    it.face_lead = join_clauses(lead.iter(), true);
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
    it.face_detail_without_effort.clear();
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

/// "~2 reps left" for a single reps-in-reserve target on a card face (0 = "to
/// your last good rep", never "0 reps left").
pub(super) fn reps_left(n: i64) -> String {
    if n <= 0 {
        "to your last good rep".to_string()
    } else {
        format!("~{} left", fmt_reps(n))
    }
}

/// A basis line's effort clause in the face wording: "with ~2 reps left", or
/// "to your last good rep" for a zero reserve.
pub(super) fn with_reps_left(n: i64) -> String {
    if n <= 0 {
        reps_left(n)
    } else {
        format!("with {}", reps_left(n))
    }
}

/// A rep band as the face writes it: `"5–8"`, or the one figure when `hi` is
/// not above `lo` (calisthenics rungs give a range, most lifts one count).
pub(super) fn range_text(lo: u8, hi: u8) -> String {
    if hi > lo {
        format!("{lo}–{hi}")
    } else {
        lo.to_string()
    }
}

/// A timed-hold band, collapsed to one figure when `lo == hi` so a
/// fixed-duration rung reads "30 s hold", not "30–30 s hold".
pub(super) fn hold_band(lo: u16, hi: u16) -> String {
    if lo == hi {
        format!("{hi} s hold")
    } else {
        format!("{lo}–{hi} s hold")
    }
}

/// A reps-in-reserve band in the face wording: "1–2 reps left", or
/// [`reps_left`] when both ends are equal.
pub(super) fn rir_band_left(lo: i32, hi: i32) -> String {
    if lo == hi {
        reps_left(i64::from(lo))
    } else {
        format!("{lo}–{hi} reps left")
    }
}

/// The face's sets×reps lead without the load: `"3×5"`, `"3×8–12"`.
pub(super) fn sets_reps_lead(it: &PrescriptionView) -> String {
    format!("{}×{}", it.sets, range_text(it.reps_low, it.reps_high))
}

/// The single reps-left number [`effort_phrase`] shows, if it shows one: a
/// structured target, or the reserve a %e1RM or RPE target implies. `None`
/// for a band, a prose target, or no target.
pub(super) fn face_rir(it: &PlanItem) -> Option<i64> {
    let from_rpe = |rpe: f64| crate::strength::rpe_to_rir(rpe).round() as i64;
    match it.intensity {
        PlanIntensity::Rir { rir } => Some(i64::from(rir)),
        PlanIntensity::RirRange { lo, hi } if lo == hi => Some(i64::from(lo)),
        PlanIntensity::RirRange { .. } => None,
        PlanIntensity::PercentE1rm { .. } => it.target_rpe.map(from_rpe),
        PlanIntensity::Prose => it.target_rpe.filter(|_| it.rpe_target).map(from_rpe),
    }
}

/// The reserve in "pick a weight for ~N reps left", shown when the day took
/// the lift's weight away and none is suggested.
fn pick_rir(it: &PlanItem) -> Option<i64> {
    let load_shown = it.load_kg.is_some()
        || (it.suggested_load_kg.is_some() && !matches!(it.intensity, PlanIntensity::Prose));
    it.target_rpe
        .filter(|_| it.pick_load && !load_shown)
        .map(|rpe| crate::strength::rpe_to_rir(rpe).round() as i64)
        .filter(|rir| *rir >= 1)
}

/// The reps left the face states: the pick-a-weight reserve, else
/// [`face_rir`].
pub(super) fn shown_rir(it: &PlanItem) -> Option<i64> {
    pick_rir(it).or_else(|| face_rir(it))
}

/// A lift item's effort in the face vocabulary: a structured reps-in-reserve
/// target as "~N reps left" / "1–2 reps left"; a %e1RM target by the reserve
/// its percentage implies (`target_rpe`), falling back to the percentage only
/// when it implies none; an "RPE n" prose target as its reps left; any other
/// prose target verbatim (not marked as effort wording). `None` when the
/// load already says it.
fn effort_phrase(it: &PlanItem, load_shown: bool) -> Option<FaceClause> {
    if let Some(n) = face_rir(it) {
        return Some(FaceClause::effort(reps_left(n)));
    }
    match it.intensity {
        PlanIntensity::RirRange { lo, hi } => Some(FaceClause::effort(rir_band_left(lo, hi))),
        PlanIntensity::PercentE1rm { .. } if load_shown => None,
        _ => Some(FaceClause::plain(it.intensity_label.clone())),
    }
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
            autoreg::E1RM_NOISE_FRAC * 100.0
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
pub(super) fn resync_intensity(it: &mut PlanItem, weight_lb: bool) {
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
            RunAmount::Km(k) => Some(fmt_km_or_mi(k, DistanceUnit::Km)),
            RunAmount::RunWalk => None,
        }
    }

    /// The km figure [`Self::label`] shows; `None` when the amount is not a
    /// distance.
    pub(super) fn shown_km(self) -> Option<f64> {
        match self {
            RunAmount::PlanKm(k) => Some(round_tenths(f64::from(k))),
            RunAmount::Km(k) => Some(round_tenths(k)),
            RunAmount::Minutes(_) | RunAmount::MinutesRange(..) | RunAmount::RunWalk => None,
        }
    }

    /// `keep` of the amount, as a readiness cut shows it, from the shown
    /// (one-decimal) kilometres; `None` when there is no single number to cut.
    pub(super) fn scaled(self, keep: f64) -> Option<RunAmount> {
        match self {
            RunAmount::Minutes(m) => Some(RunAmount::Minutes((m * keep).round().max(1.0))),
            RunAmount::PlanKm(k) => {
                Some(RunAmount::Km((round_tenths(f64::from(k)) * keep).max(0.1)))
            }
            RunAmount::Km(k) => Some(RunAmount::Km((round_tenths(k) * keep).max(0.1))),
            RunAmount::MinutesRange(..) | RunAmount::RunWalk => None,
        }
    }
}

/// The whole-session volume of an interval run as one continuous amount: the
/// rep count × the per-rep volume ("6 × 4 min" → 24 min, "6 × 800 m" →
/// 4.8 km). `None` for a continuous run or run/walk reps. The recoveries are
/// not counted (their length is not on the item).
fn interval_total(it: &PlanItem) -> Option<RunAmount> {
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
pub(in crate::app) fn run_planned(it: &PlanItem) -> (Option<f64>, Option<f64>) {
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

pub(super) fn clear_run_reps(it: &mut PlanItem) {
    it.rep_count = 0;
    it.rep_volume.clear();
    it.rep_amount = None;
}

pub(in crate::app) const EASY_RUN_LABEL: &str = "Easy: zone 1–2";

/// Cap a RUN item to an easy target: keep the bounded volume but replace the
/// hard intensity with an easy zone. An interval day keeps its numbers as one
/// continuous easy run of the same total work ("6 × 4 min" → "24 min"; never a
/// card with no duration).
pub(super) fn cap_run_item_easy(it: &mut PlanItem, r: &AdjustmentCall, note: &str) {
    // The rep structure is deferred, not prescribed: drop it (and any "N × …"
    // that would read as an interval on an easy card), carrying its total.
    let vol = if it.rep_count > 0 {
        interval_total(it)
    } else {
        it.run_volume.filter(|v| *v != RunAmount::RunWalk)
    };
    clear_run_reps(it);
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
    set_easy_run_face(it);
    it.adjusted_note = note.to_string();
    repoint_evidence(
        it,
        r,
        Some(format!(
            "Changed to an easy run ({note}). The planned harder effort waits for another day."
        )),
    );
}

/// Label a run (its volume already set) easy, with the easy pace/bpm target,
/// and re-render its face.
pub(super) fn set_easy_run_face(it: &mut PlanItem) {
    it.intensity_label = EASY_RUN_LABEL.into();
    super::run_targets::downgrade_run_target_to_easy(it);
    let label = it.intensity_label.clone();
    let vol = it.run_volume;
    set_run_summary(
        it,
        vol.and_then(RunAmount::label),
        vol.and_then(RunAmount::shown_km),
        &label,
        EASY_RUN_LABEL,
    );
}

/// Point an item's grade chip and why? at `r`, the call that changed it
/// (`basis` overrides the claim's own).
pub(super) fn repoint_evidence(
    it: &mut PrescriptionView,
    r: &AdjustmentCall,
    basis: Option<String>,
) {
    it.evidence = EvidenceTagView::of(r);
    it.safety_reason.clear();
    it.why = why_from(basis, r);
}

impl WhyView {
    pub(in crate::app) fn append_basis(&mut self, line: &str) {
        if !self.basis.is_empty() {
            self.basis.push(' ');
        }
        self.basis.push_str(line);
    }
}
