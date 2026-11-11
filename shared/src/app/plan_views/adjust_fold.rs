//! The numeric fold of a day's adjustments: the adjustments in force composed
//! into one picture (deepest volume and load cuts, a dropped load bar) and
//! applied to an item's shown numbers, shared by the hero and every judged day.

use crate::app::plan_views::item_render::{resync_intensity, round_load_or_drop, set_run_summary};
use crate::app::plan_views::plan_item::PlanItem;
use crate::app::view_types::{PlanIntensity, PrescriptionView};
use crate::schema::{Adjustment, AdjustmentCall, AdjustmentKind, PainRegion};

/// The completion-relevant adjustments in force on a day, composed into a single
/// fold: the deepest volume and load cuts (stacked deloads take the max, they
/// never compound) and whether the load bar is removed (a downgrade or
/// modify-and-monitor day is shown with no load, so any working set counts). The
/// hero display fold and the per-day completion judgement share this composition
/// so a set logged today reads the same on tomorrow's strip.
#[derive(Clone, Copy, Default)]
pub(in crate::app) struct AdjustFold {
    pub(super) vol_frac: f64,
    pub(super) load_frac: f64,
    pub(super) load_dropped: bool,
    /// A tendon modify-and-monitor day scoped to a body region: only the lifts
    /// that load it lose the load bar.
    pub(super) pain_modify: Option<PainRegion>,
    /// A head cold caps every run at one short easy run (ILLNESS-COLD-RUN-001),
    /// so any run logged on the day completes the run item.
    pub(super) short_easy_run: bool,
}

impl AdjustFold {
    /// Whether this day shows `it` with no load bar: a whole-session drop, or
    /// a lift that loads the region a scoped tendon modify covers.
    pub(super) fn drops_load(&self, it: &PrescriptionView) -> bool {
        self.load_dropped
            || self
                .pain_modify
                .is_some_and(|region| !crate::autoreg::item_spared_by_pain(region, &it.exercise))
    }
}

pub(super) fn head_cold(r: &AdjustmentCall) -> bool {
    r.kind() == AdjustmentKind::HeadCold
}

/// One level of a readiness downgrade on a lift (autoreg-030 "one level",
/// autoreg-002 one RPE step ≈ 3–5 % load): −5 % load plus one more rep in
/// reserve, the numbers kept.
pub(super) const ONE_LEVEL_LOAD_FRAC: f64 = 0.05;

/// Whether a `DowngradeSession` is a SAFETY easy day (reactive tendon pain,
/// the first session back after a hold, a head cold): lifts lose the heavy load
/// and run at ≥ `adjustments::EASY_DAY_RIR` reps in reserve. Every other
/// downgrade is the readiness "one level" with the numbers kept.
pub(super) fn safety_easy(r: &AdjustmentCall) -> bool {
    matches!(
        r.kind(),
        AdjustmentKind::TendonReduce | AdjustmentKind::ReturnHold | AdjustmentKind::HeadCold
    )
}

/// How a non-safety `DowngradeSession` lands on lift items (ready-upd-001: HRV
/// and other non-subjective markers only HOLD progression on lifts):
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum LiftDowngrade {
    /// Multi-day wellness suppression or a single high resting-HR morning: one
    /// level, −5 % load and one more rep in reserve.
    OneLevel,
    /// The HRV trend (HRV-001) or a within-set velocity loss (autoreg-010,
    /// terminate the set): planned load, one more rep in reserve, no load cut.
    Hold,
    /// Aerobic decoupling (autoreg-037) is a run-side signal: lifts as planned.
    Unchanged,
}

pub(super) fn lift_downgrade(r: &AdjustmentCall) -> LiftDowngrade {
    match r.kind() {
        AdjustmentKind::HrvLower | AdjustmentKind::VelocityLoss => LiftDowngrade::Hold,
        AdjustmentKind::Decoupling => LiftDowngrade::Unchanged,
        _ => LiftDowngrade::OneLevel,
    }
}

pub(in crate::app) fn fold_completion_adjustments(recs: &[AdjustmentCall]) -> AdjustFold {
    let mut f = AdjustFold::default();
    for r in recs {
        match &r.value {
            Adjustment::Deload {
                volume_reduction_pct,
                load_reduction_pct,
                ..
            } => {
                f.vol_frac = f
                    .vol_frac
                    .max((*volume_reduction_pct as f64 / 100.0).clamp(0.0, 1.0));
                f.load_frac = f
                    .load_frac
                    .max((*load_reduction_pct as f64 / 100.0).clamp(0.0, 1.0));
            }
            Adjustment::ReduceLoadPct(p) => {
                f.load_frac = f.load_frac.max((*p as f64 / 100.0).clamp(0.0, 1.0));
            }
            Adjustment::ReduceVolumePct(p) => {
                f.vol_frac = f.vol_frac.max((*p as f64 / 100.0).clamp(0.0, 1.0));
            }
            Adjustment::DowngradeSession if !safety_easy(r) => {
                if lift_downgrade(r) == LiftDowngrade::OneLevel {
                    f.load_frac = f.load_frac.max(ONE_LEVEL_LOAD_FRAC);
                }
            }
            Adjustment::ModifyAndMonitor { .. } => match r.kind() {
                AdjustmentKind::TendonModify(Some(region)) => f.pain_modify = Some(region),
                _ => f.load_dropped = true,
            },
            Adjustment::DowngradeSession | Adjustment::ReturnCheck => {
                f.load_dropped = true;
                if head_cold(r) {
                    f.short_easy_run = true;
                }
            }
            _ => {}
        }
    }
    f
}

/// Apply a fractional load cut to a structured percent target: scale the shown
/// percent by `(1 - frac)` and record the pre-cut original so a STACKED cut
/// re-derives "{pct}% e1RM (reduced from {orig}%)" straight from the numbers.
/// The label is rendered from these numbers, so a second cut moves the load and
/// the label together. Non-percent targets are untouched.
pub(super) fn reduce_percent_intensity(intensity: &mut PlanIntensity, frac: f64) {
    if let PlanIntensity::PercentE1rm { pct, reduced_from } = intensity {
        let orig = reduced_from.unwrap_or(*pct);
        *pct = (*pct as f64 * (1.0 - frac)).round() as u32;
        *reduced_from = Some(orig);
    }
}

/// Rewrite one item's shown numbers by a day's adjustment fold: the deepest
/// volume cut scales a lift's set count and a run's duration/distance (or its
/// interval reps), the deepest load cut a lift's load (and a structured %e1RM
/// target), and a downgrade / modify-and-monitor day drops the load bar. The
/// label and summary are re-rendered from the folded numbers. An unadjusted
/// fold leaves the item untouched. The single numeric fold shared by the hero
/// and every judged week row.
pub(in crate::app) fn fold_item_numbers(it: &mut PlanItem, fold: AdjustFold, weight_lb: bool) {
    if it.is_run() {
        if fold.vol_frac > 0.0 {
            cut_run_volume(it, fold.vol_frac);
        }
        return;
    }
    let mut changed = false;
    if fold.vol_frac > 0.0 {
        it.sets = required_sets(it.sets, fold.vol_frac);
        changed = true;
    }
    if fold.load_frac > 0.0
        && let Some(kg) = it.load_kg
    {
        it.load_kg = round_load_or_drop(kg * (1.0 - fold.load_frac), weight_lb);
        reduce_percent_intensity(&mut it.intensity, fold.load_frac);
        changed = true;
    }
    if fold.drops_load(it) && it.load_kg.is_some() {
        it.load_kg = None;
        it.pick_load = true;
        changed = true;
    }
    if changed {
        resync_intensity(it, weight_lb);
    }
}

/// The set count required to complete a lift/calisthenics item after folding the
/// day's volume reduction: a deloaded week needs `round(sets·(1−pct/100)).max(1)`
/// sets. A day judged with `frac > 0` is never held to more sets than it showed.
pub(super) fn required_sets(sets: u8, vol_frac: f64) -> u8 {
    let scaled = (sets as f64 * (1.0 - vol_frac)).round() as i64;
    scaled.clamp(1, u8::MAX as i64) as u8
}

/// Cut a run item's volume by `frac` (autoreg-045 "cut duration ~50%"): an
/// interval day drops reps, a continuous run its minutes or kilometres, a
/// run/walk day its bouts. The summary and the tracker's structure are rebuilt
/// from the cut numbers, so the guided run walks the same session the card shows.
fn cut_run_volume(it: &mut PlanItem, frac: f64) {
    let keep = (1.0 - frac).clamp(0.0, 1.0);
    if let Some(rs) = it.run_structure.as_mut() {
        if rs.reps > 0 {
            rs.reps = ((f64::from(rs.reps) * keep).round() as u32).max(1);
        }
        if !rs.run_walk.is_empty() {
            let bouts = ((rs.run_walk.len() as f64 * keep).round() as usize).max(1);
            rs.run_walk.truncate(bouts);
            if let Some(last) = rs.run_walk.last_mut() {
                last.walk_sec = 0;
            }
        }
    }
    if it.rep_count > 0 {
        it.rep_count = ((f64::from(it.rep_count) * keep).round() as u8).max(1);
        let head = format!("{} × {}", it.rep_count, it.rep_volume);
        let label = it.intensity_label.clone();
        set_run_summary(it, Some(head), None, &label, "");
        return;
    }
    let shown = it
        .run_volume
        .and_then(|v| Some((v.label()?, v.scaled(keep)?)));
    if let Some((old, cut)) = shown
        && it.run_head == old
        && let Some(new) = cut.label()
    {
        // The summary after its head: the target line, and once a target is
        // resolved the structure notes after it (`run_targets::apply_target`).
        let rest = match it.run_target {
            Some(_) => format!(
                "{}{}",
                it.intensity_label,
                crate::app::run_targets::structure_suffix(it)
            ),
            None => it.intensity_label.clone(),
        };
        set_run_summary(it, Some(new), cut.shown_km(), &rest, "");
        it.run_volume = Some(cut);
    }
    if it.adjusted_note.is_empty() {
        it.adjusted_note = format!("{:.0}% less running today", frac * 100.0);
    }
}
