//! In-session autoregulation from an item's first logged work set.

use super::*;

/// Per-item performance autoregulation on TODAY's session, from the item's own
/// logged sets (the Strong-graded backbone, AUTOREG-RIR-001):
/// - the first qualifying work set's RPE minus the item's target RPE → a load
///   change on THAT item's remaining sets (autoreg-001/002/004/005);
/// - the item-scoped e1RM-gate recs already in `recommended` (today's top set
///   vs the 14-day best, autoreg-006/007).
///
/// The deepest cut wins (never compounding with the session-wide fold); a cut
/// item's logged sets are re-judged at the cut load, so following the lower
/// weight still completes it. Increases are an opt-in note (never folded) and
/// are dropped while `hold` (autoreg-044). Returns one in-session cue per
/// item-scoped recommendation that changed an item: the new first-set calls
/// (`fresh`) and the matched e1RM-gate recs already in `recommended`, each with
/// the plate-grid load for the remaining sets when the cue moves the load.
pub(in crate::app) fn apply_first_set_autoreg(
    ns: &mut SessionPlanView,
    recommended: &[Recommended<Adjustment>],
    review_recs: &[Recommended<Adjustment>],
    hold: bool,
    weight_lb: bool,
) -> Vec<ItemCue> {
    let mut all: Vec<Recommended<Adjustment>> = recommended.to_vec();
    all.extend(review_recs.iter().cloned());
    let session_cut = fold_completion_adjustments(&all).load_frac;
    // A safety easy day (tendon pain, head cold, first session back) is already
    // load-less and easy: no first-set rule on top of it. A tendon modify day
    // scoped to a body region eases only the lifts that load it; the rest are
    // held lifts like any other.
    let pain_modify = all.iter().find_map(|r| match &r.value {
        Adjustment::ModifyAndMonitor { location } => Some(crate::autoreg::pain_region(location)),
        _ => None,
    });
    let safety_day = matches!(pain_modify, Some(None))
        || all
            .iter()
            .any(|r| matches!(r.value, Adjustment::DowngradeSession) && safety_easy(r));
    let cx = FirstSetCtx {
        recommended,
        hold,
        weight_lb,
        session_cut,
        safety_day,
        cap_applied: f64::from(session_effort_cap(&all)),
    };
    let mut cues: Vec<ItemCue> = Vec::new();
    let mut applied: Option<AdjustmentView> = None;
    for it in ns.items.iter_mut() {
        if it.is_run() || it.timed_hold {
            continue;
        }
        if let Some(Some(region)) = pain_modify
            && !crate::autoreg::item_spared_by_pain(region, &it.exercise)
        {
            continue;
        }
        let item = item_cues(it, &cx);
        if let Some(c) = item.first() {
            applied.get_or_insert_with(|| to_view(&c.rec));
        }
        cues.extend(item);
    }
    if let Some(adj) = applied {
        ns.status = SessionStatus::Adjusted;
        ns.adjustment.get_or_insert(adj);
    }
    // Keep each item's status in step with the re-judged sets.
    refresh_item_statuses(&mut ns.items, &[]);
    cues
}

/// What [`apply_first_set_autoreg`] judges every item with.
struct FirstSetCtx<'a> {
    recommended: &'a [Recommended<Adjustment>],
    hold: bool,
    weight_lb: bool,
    /// The session-wide load cut already folded into every item.
    session_cut: f64,
    /// A safety easy day: no first-set rule on top of it.
    safety_day: bool,
    /// The readiness RPE caps already applied to every lift today (once, the
    /// deepest: holds never stack), so the first set is judged against the
    /// PLANNED target, not the held one.
    cap_applied: f64,
}

/// One lift item's first-set autoregulation: applies the deepest cut, the RPE
/// cap and the add/failed-set notes to `it`, and returns its cues (empty when
/// nothing changed it or the exercise is finished).
fn item_cues(it: &mut PrescriptionView, cx: &FirstSetCtx) -> Vec<ItemCue> {
    let FirstSetCtx {
        recommended,
        hold,
        weight_lb,
        session_cut,
        safety_day,
        cap_applied,
    } = *cx;
    // First-set RPE vs this item's PLANNED target: a %e1RM item's effort as
    // its card first stated it (`basis_rx`, before any fold; else from its
    // planned percent on the Epley inverse, File 02 strength-005), else the
    // prescribed RPE with today's readiness caps added back.
    let target = match it.intensity {
        PlanIntensity::PercentE1rm { pct, reduced_from } if pct > 0 => {
            Some(it.basis_rx.as_ref().and_then(|b| b.rir).map_or_else(
                || percent_effort_rpe(reduced_from.unwrap_or(pct).max(1) as f64, it.reps_low),
                |rir| crate::strength::rir_to_rpe(rir as f64),
            ))
        }
        _ => it.target_rpe.map(|t| (t + cap_applied).min(10.0)),
    };
    let first = (!safety_day)
        .then(|| {
            // A lighter set says nothing about the planned target's effort.
            it.logged_sets
                .iter()
                .find(|s| s.qualifies && !s.lighter && s.rpe > 0.0)
        })
        .flatten();
    let first_kg = first.map(|s| s.weight_kg).filter(|kg| *kg > 0.0);
    let first_at = first.map(|s| s.observed_at);
    // The last counted set: an add that would not take the lifter above it
    // is already done (or overtaken), never a cue to lower the bar.
    let last_kg = it
        .logged_sets
        .iter()
        .rev()
        .find(|s| s.qualifies)
        .map(|s| s.weight_kg);
    // The "first set felt easy" add is a call for the set right after it:
    // once a later set is logged (the add taken, declined, or overtaken by
    // a harder set) it is spent.
    let later_logged = first_at.is_some_and(|t0| it.logged_sets.iter().any(|s| s.observed_at > t0));
    let mut item_recs: Vec<Recommended<Adjustment>> = recommended
        .iter()
        .filter(|r| {
            matches!(&r.value, Adjustment::ForItem { exercise, .. }
                if crate::plan::same_movement(exercise, &it.exercise))
        })
        .cloned()
        .collect();
    // The recs already in `recommended`; the ones pushed below are fresh.
    let matched = item_recs.len();
    if let (Some(t), Some(s)) = (target, first)
        && let Some(rec) = crate::autoreg::first_set_rpe_adjust(&it.exercise, s.rpe - t)
    {
        let add = matches!(rec.value.base(), Adjustment::IncreaseLoadPct(_));
        if !add || !(hold || later_logged) {
            item_recs.push(rec);
        }
    }
    // A later work set that went to failure (no reps left, or missed
    // reps) while sets remain: stop here or drop the weight
    // (autoreg-013). Only the latest set decides, so a lighter set after
    // it clears the cue.
    let mut work = it.logged_sets.iter().filter(|s| s.qualifies);
    let failed_last = !safety_day
        && work.clone().count() >= 2
        && work
            .next_back()
            .is_some_and(|s| s.short || s.rpe >= crate::strength::rir_to_rpe(0.0));
    if failed_last {
        item_recs.push(crate::autoreg::failed_set_stop(&it.exercise));
    }
    if item_recs.is_empty() {
        return Vec::new();
    }
    let (mut cut, mut cap, mut add) = (0.0_f64, 0_u8, 0.0_f32);
    for r in &item_recs {
        match r.value.base() {
            Adjustment::ReduceLoadPct(p) => cut = cut.max((*p as f64 / 100.0).clamp(0.0, 1.0)),
            Adjustment::CapRpe(d) => cap = cap.max(*d),
            Adjustment::IncreaseLoadPct(p) if !hold => add = add.max(*p),
            _ => {}
        }
    }
    let mut notes: Vec<String> = Vec::new();
    let mut cut_load = None;
    if cut > session_cut
        && let Some(kg) = it.load_kg
    {
        // Deepest cut wins, taken off the heavier of the planned load
        // (before the session-wide cut) and the first set that felt hard.
        let planned = kg / (1.0 - session_cut).max(1e-6);
        let base = first_kg.map_or(planned, |f| f.max(planned));
        let new_load = round_load_or_drop(base * (1.0 - cut), weight_lb);
        it.load_kg = new_load;
        it.load_held = false;
        cut_load = new_load;
        if let Some(bar) = new_load {
            if bar < kg {
                reduce_percent_intensity(&mut it.intensity, 1.0 - bar / kg);
            }
            // Re-judge the logged sets at the cut load: the user followed
            // the coach, so a set at the cut load is a full set.
            rejudge_at_load(it, bar, weight_lb);
        } else {
            reduce_percent_intensity(&mut it.intensity, cut);
        }
        notes.push(format!("remaining sets −{:.0}%", cut * 100.0));
    } else if cut > 0.0 && it.load_kg.is_none() {
        // No prescribed load to cut (an effort-prescribed lift): say it.
        notes.push(format!(
            "use about {:.0}% less weight than your first set for the rest",
            cut * 100.0
        ));
    }
    if cap > 0 && cap_item_effort(it, cap, weight_lb) {
        notes.push(format!("stop {} earlier", reps_word(cap)));
    }
    if add > 0.0 {
        notes.push(format!("first set felt easy: you can add about {add:.0}%"));
    }
    if failed_last {
        notes.push("last set failed: end the exercise here, or drop the weight".into());
    }
    it.done = it.sets_done >= it.sets.max(1) || it.ended_early;
    // A finished exercise has no "remaining sets": no cue, no note.
    if notes.is_empty() || it.done {
        resync_intensity(it, weight_lb);
        return Vec::new();
    }
    it.adjusted_note = notes.join(", ");
    resync_intensity(it, weight_lb);
    // The load the remaining sets should use: the cut load (or, for an
    // effort-prescribed lift, the first set's weight less the cut), or the
    // opt-in add on top of the heavier of the planned and first-set weight
    // (never a chip at or below the last set's weight).
    let cue_load = if cut > 0.0 {
        match it.load_kg {
            Some(_) => cut_load,
            None => first_kg.and_then(|kg| round_load_or_drop(kg * (1.0 - cut), weight_lb)),
        }
    } else if add > 0.0 {
        let base = match (first_kg, it.load_kg) {
            (Some(f), Some(p)) => Some(f.max(p)),
            (f, p) => f.or(p),
        };
        base.and_then(|kg| round_load_or_drop(kg * (1.0 + f64::from(add) / 100.0), weight_lb))
            .filter(|to| last_kg.is_none_or(|last| *to > last + 1e-9))
    } else {
        None
    };
    item_recs
        .into_iter()
        .enumerate()
        .map(|(k, rec)| {
            let load_kg = match rec.value.base() {
                Adjustment::ReduceLoadPct(_) | Adjustment::IncreaseLoadPct(_) => cue_load,
                _ => None,
            };
            ItemCue {
                rec,
                fresh: k >= matched,
                load_kg,
            }
        })
        .collect()
}

/// One item-scoped in-session cue from [`apply_first_set_autoreg`].
pub(in crate::app) struct ItemCue {
    pub(in crate::app) rec: Recommended<Adjustment>,
    /// A new first-set call (not already among the readiness recommendations).
    pub(in crate::app) fresh: bool,
    /// Plate-grid load (kg) for the remaining sets when the cue moves the load.
    pub(in crate::app) load_kg: Option<f64>,
}
