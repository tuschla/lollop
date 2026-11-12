//! Folding readiness and weekly check-in adjustments into a session's shown
//! items.

use crate::app::evidence_views::{
    CallCtx, RETURN_CHECK_STOP, RETURN_DOSE_EFFORT_PCT, RETURN_DOSE_MIN, deload_cut_clauses,
    lift_work_logged, return_check_headline, to_view_in, why_from,
};
use crate::app::format::fmt_reps;
use crate::app::logged_days::LoggedDays;
use crate::app::model::{Model, profile_for_day};
use crate::app::plan_views::adjust_fold::{
    LiftDowngrade, ONE_LEVEL_LOAD_FRAC, head_cold, lift_downgrade, reduce_percent_intensity,
    safety_easy,
};
use crate::app::plan_views::completion::refresh_item_statuses;
use crate::app::plan_views::item_render::{
    FaceClause, cap_run_item_easy, clear_run_reps, refresh_lift_summary, repoint_evidence,
    resync_intensity, set_easy_run_face, set_named_summary, set_run_summary,
};
use crate::app::plan_views::plan_item::{PlanItem, PlanRow, RunAmount};
use crate::app::readiness::readiness_picture;
use crate::app::review_deloads::review_deloads;
use crate::app::view_types::{
    EvidenceTagView, ItemKind, PlanIntensity, PrescriptionView, SessionStatus,
};
use crate::day::{is_undated, local_day};
use crate::event::SessionReview;
use crate::evidence::{ClaimId, claims};
use crate::plates::load_grid_step_kg;
use crate::profile::Profile;
use crate::schema::{
    Adjustment, AdjustmentCall, AdjustmentKind, DeloadTrigger, PainRegion, ReadinessSignal,
    SessionType,
};

/// The claim behind the head-cold run cap: one short easy run of 10–20 min at
/// 60–70 % of usual effort, or rest (IOC 2022 exercise-challenge dose).
const HEAD_COLD_RUN_CLAIM: ClaimId = claims::ILLNESS_COLD_RUN_001;

/// The reps in reserve a safety easy day or a pain-free modify day holds every
/// lift at, at least (HARD RULE 3: never a top-end into active pain).
const EASY_DAY_RIR: u8 = 3;

/// Whether a non-safety `DowngradeSession` eases runs: a within-set velocity
/// loss is a lifting signal (autoreg-010), so runs stay as planned.
fn downgrade_eases_runs(r: &AdjustmentCall) -> bool {
    r.kind() != AdjustmentKind::VelocityLoss
}

/// The run-side evidence for a hold's easy run (ready-upd-001 "run items: easy
/// (unchanged autoreg-028)"): the hold itself cites the lifting-only claim, so a
/// run eased by it re-points to the trigger's own endurance rule.
fn run_side_rec(r: &AdjustmentCall) -> AdjustmentCall {
    match r.kind() {
        AdjustmentKind::HrvHold => r.cited_to(claims::HRV_001),
        AdjustmentKind::RhrHold => r.cited_to(claims::AUTOREG_RHR_DOWN_001),
        AdjustmentKind::WellnessHold => r.cited_to(claims::WELLNESS_001),
        _ => r.clone(),
    }
}

/// Whether a deload also defers hard running: the overtraining recovery week
/// (HYB-DELOAD-001; running-034's down week) and the failed-key-session volume
/// deload (autoreg-036 "reduce weekly volume; defer hard work"). Their quality
/// runs become easy.
fn deload_defers_hard_running(r: &AdjustmentCall) -> bool {
    matches!(
        r.kind(),
        AdjustmentKind::WeeklyDeload(DeloadTrigger::RecoveryWeek | DeloadTrigger::FailedSessions)
    )
}

/// One weekly check-in's recommendations with the local-day span each can
/// govern. `start` is the check-in's local day (`None` when undated);
/// `current_until` is the last day it was the check-in in force: the day the
/// next dated check-in replaced it (never before its own day), else today.
pub(super) struct ReviewWindow {
    start: Option<i64>,
    current_until: i64,
    recs: Vec<AdjustmentCall>,
}

/// Every weekly check-in's recommendations with their windows, built once per
/// view. Check-ins carrying no recommendation are skipped. Session reviews
/// carry no week-level recommendation, so they never open or close a window.
pub(super) fn review_windows(model: &Model, today: i64, offset: i64) -> Vec<ReviewWindow> {
    let history = &model.weekly_history;
    let day_of =
        |r: &SessionReview| (!is_undated(r.observed_at)).then(|| local_day(r.observed_at, offset));
    history
        .iter()
        .enumerate()
        .filter_map(|(k, review)| {
            let recs = review_deloads(review);
            if recs.is_empty() {
                return None;
            }
            let start = day_of(review);
            let current_until = history[k + 1..]
                .iter()
                .find_map(day_of)
                .map_or(today, |next| next.max(start.unwrap_or(next)).min(today));
            Some(ReviewWindow {
                start,
                current_until,
                recs,
            })
        })
        .collect()
}

/// The weekly check-in recommendations in force on past local day `day`, over
/// EVERY submitted check-in, so a later check-in or a `ClearWeeklyCheckin`
/// never takes a window back from a day it covered:
/// - a `Deload { weeks }` from a check-in dated D0 is active on `[D0, D0 + 7·weeks)`,
///   so a deload aging out today still governs a day it covered;
/// - every other recommendation (e.g. an HRV `DowngradeSession`) is active from
///   the check-in's day through the last day it was in force — the span
///   today's view applied it over. A check-in cleared by the undated
///   `ClearWeeklyCheckin` has no known end and stays in force through today.
///
/// An undated review has no start: its recommendations apply on any day up to
/// the end of their window (the lenient fallback).
fn review_recs_active_on(windows: &[ReviewWindow], day: i64, today: i64) -> Vec<AdjustmentCall> {
    let mut out = Vec::new();
    for w in windows {
        if w.start.is_some_and(|s| s > day) {
            continue;
        }
        for r in &w.recs {
            let active = match (&r.value, w.start) {
                (Adjustment::Deload { .. }, None) => day <= today,
                (Adjustment::Deload { weeks, .. }, Some(s)) => {
                    day < s.saturating_add(7 * i64::from((*weeks).max(1)))
                }
                _ => day <= w.current_until,
            };
            if active {
                out.push(r.clone());
            }
        }
    }
    out
}

/// The readiness adjustments in force on each past day of the rendered week,
/// from the shared pipeline as it stood that day (a signal removed on a later
/// day still shaped it; only an undated `ClearReadiness` or legacy undated
/// remove, which cannot say when, drops it from past days too).
pub(super) struct PastReadiness(Vec<(i64, Vec<AdjustmentCall>)>);

impl PastReadiness {
    /// Every local day in `[week_monday, today)`, each judged with the profile
    /// in force that day.
    pub(super) fn build(
        model: &Model,
        days: &LoggedDays,
        profile: &Profile,
        week_monday: i64,
        today: i64,
    ) -> Self {
        Self(
            (week_monday..today)
                .map(|day| {
                    let dp = profile_for_day(model, day);
                    let picture = readiness_picture(
                        model,
                        days,
                        Some(dp.as_deref().unwrap_or(profile)),
                        Some(day),
                    );
                    (day, picture.adjustments(model, days))
                })
                .collect(),
        )
    }

    fn on(&self, day: i64) -> &[AdjustmentCall] {
        self.0
            .iter()
            .find(|(d, _)| *d == day)
            .map_or(&[], |(_, recs)| recs.as_slice())
    }
}

/// The adjustments in force on a judged day: today reuses `today_recs`, the
/// live readiness + review picture (the hero display fold); a past day reads
/// its own readiness plus the weekly check-ins active on it. A future day is
/// never judged (the caller folds an active illness hold into it).
pub(super) fn completion_recs_for_day<'a>(
    day: i64,
    today: i64,
    past: &PastReadiness,
    today_recs: &'a [AdjustmentCall],
    windows: &[ReviewWindow],
) -> std::borrow::Cow<'a, [AdjustmentCall]> {
    if day >= today {
        return std::borrow::Cow::Borrowed(today_recs);
    }
    let mut recs = past.on(day).to_vec();
    recs.extend(review_recs_active_on(windows, day, today));
    std::borrow::Cow::Owned(recs)
}

/// Cap RPE by raising reps-in-reserve on a structured RIR target. `CapRpe(d)`
/// adds `d` reps of reserve (planned RPE − d), clamped to [0, 10]. Returns the
/// reserve the numeric prefill (`target_rpe`) should map from — a single value's
/// cap, or a band's new HIGH (conservative) bound — or `None` when nothing
/// changed (a prose / hold / %e1RM item, or a cap that moved no rung). For a
/// band both ends shift up by `d` (the low end never left above the new high),
/// so the top-end (hardest) rung is always brought within the cap.
fn cap_rpe_intensity(intensity: &mut PlanIntensity, d: u8) -> Option<i32> {
    let d = i32::from(d);
    match intensity {
        PlanIntensity::Rir { rir } => {
            let capped = (*rir + d).clamp(0, 10);
            (capped != *rir).then(|| {
                *rir = capped;
                capped
            })
        }
        PlanIntensity::RirRange { lo, hi } => {
            let new_hi = (*hi + d).clamp(0, 10);
            let new_lo = (*lo + d).clamp(0, 10).min(new_hi);
            (new_hi != *hi || new_lo != *lo).then(|| {
                *lo = new_lo;
                *hi = new_hi;
                new_hi
            })
        }
        PlanIntensity::PercentE1rm { .. } | PlanIntensity::Prose => None,
    }
}

/// The hero's fold: [`annotate_adjustments`], then each item's status
/// re-judged against the same adjustments.
pub(in crate::app) fn annotate_and_rejudge(
    ns: &mut PlanRow,
    recommended: &[AdjustmentCall],
    today: Option<&CallCtx>,
    weight_lb: bool,
) {
    annotate_adjustments(ns, recommended, today, weight_lb);
    refresh_item_statuses(&mut ns.items, recommended);
}

/// Fold the active (non-blocking) readiness adjustments INTO today's session,
/// or an active head cold into an upcoming day's session (the key safety
/// composition): a load cut, an RPE cap, or a
/// downgrade modifies the shown items so the plan never renders a top-end above
/// what readiness allows. Blocking holds never reach here (handled upstream by
/// `train_blocked`). Each applied adjustment carries its own evidence. The
/// items already carry the numeric fold of `recommended` ([`fold_item_numbers`],
/// applied by [`flatten_session`]); this annotates each affected item, applies
/// the RPE cap and the easy/pain-free rewrites, re-judges completion and marks
/// the session adjusted. `today` carries today's call wording when `ns` is
/// today's session.
pub(super) fn annotate_adjustments(
    ns: &mut PlanRow,
    recommended: &[AdjustmentCall],
    today: Option<&CallCtx>,
    weight_lb: bool,
) {
    let effort = EffortPass::apply(&mut ns.items, recommended, weight_lb);
    let mut applied: Option<&AdjustmentCall> = None;
    // Items stopped by a localized pain report: nothing to do today.
    let mut stopped = vec![false; ns.items.len()];
    for r in recommended {
        let changed = match (&r.value, r.kind()) {
            (Adjustment::ReduceLoadPct(p), _) => note_load_cut(&mut ns.items, *p, weight_lb),
            (
                Adjustment::Deload {
                    volume_reduction_pct,
                    load_reduction_pct,
                    ..
                },
                _,
            ) => note_deload(
                &mut ns.items,
                r,
                (*volume_reduction_pct, *load_reduction_pct),
                weight_lb,
            ),
            (Adjustment::CapRpe(_), _) => note_rpe_cap(&mut ns.items, &effort.eased),
            (Adjustment::CapTopEnd { trigger }, _) => {
                apply_progression_hold(&mut ns.items, r, *trigger, &effort)
            }
            (Adjustment::DowngradeSession, _) if !safety_easy(r) => {
                apply_one_level_downgrade(&mut ns.items, r, &effort, weight_lb)
            }
            (Adjustment::DowngradeSession, _) => apply_safety_easy_day(&mut ns.items, r, weight_lb),
            (Adjustment::ReduceVolumePct(p), _) => note_volume_cut(&mut ns.items, *p),
            (Adjustment::StopItems { location }, AdjustmentKind::PainScoped(region)) => {
                apply_pain_stop(
                    &mut ns.items,
                    r,
                    (location, region),
                    &effort.clamped,
                    &mut stopped,
                )
            }
            (Adjustment::ReturnCheck, _) => {
                // ready-upd-003 (IOC 2022): the first session after a fever or
                // chest illness is a short, easy self-test, not the planned work.
                let logged = ns.items.iter().any(|it| it.has_work());
                let kind = match ns.session_type {
                    SessionType::Lift(_) => ItemKind::ReturnCheck,
                    _ => ItemKind::Run,
                };
                ns.items = vec![return_check_item(r, kind, logged)];
                stopped = vec![false];
                applied = Some(r);
                break;
            }
            (Adjustment::ModifyAndMonitor { location }, AdjustmentKind::TendonModify(region)) => {
                apply_modify_and_monitor(&mut ns.items, r, (location, region), &effort, weight_lb)
            }
            // Item-scoped performance cuts are applied by
            // [`apply_first_set_autoreg`] (they need the item's logged sets);
            // blocking holds never reach here; increases are opt-in, never folded.
            _ => false,
        };
        if changed && applied.is_none() {
            applied = Some(r);
        }
    }
    // Re-judge completion after the fold so the mirrored strip row and the hero
    // agree on the reduced set count (a downgrade/modify no-load day is done once
    // its shown session is logged). A pain-stopped item has nothing to do.
    for (idx, it) in ns.items.iter_mut().enumerate() {
        if stopped.get(idx).copied().unwrap_or(false) {
            it.done = true;
        } else if it.is_lift() {
            it.done = it.sets_done >= it.sets.max(1) || it.ended_early;
        }
    }
    if let Some(r) = applied {
        let ctx = match today {
            Some(t) => CallCtx {
                first_set_logged: lift_work_logged(&ns.items),
                items: &ns.items,
                ..*t
            },
            None => CallCtx {
                items: &ns.items,
                ..CallCtx::default()
            },
        };
        let adj = to_view_in(r, &ctx);
        ns.status = SessionStatus::Adjusted;
        ns.adjustment = Some(adj);
    }
}

/// The one effort pass for the whole session: every readiness hold and RPE cap
/// in force moves a lift's reserve once, by the deepest of them (holds never
/// stack), and a held day (ready-upd-001 "no load added"; a pain stop,
/// autoreg-044) keeps each lift at or under the heaviest weight already lifted
/// for its reps.
struct EffortPass {
    /// The extra reps in reserve each item was given (0: unchanged).
    eased: Vec<u8>,
    /// Whether each item's load was clamped to its hold cap.
    clamped: Vec<bool>,
}

impl EffortPass {
    fn apply(items: &mut [PlanItem], recommended: &[AdjustmentCall], weight_lb: bool) -> Self {
        let hold_cap = recommended.iter().map(lift_effort_hold).max().unwrap_or(0);
        let rpe_cap = recommended
            .iter()
            .filter_map(|r| match r.value {
                Adjustment::CapRpe(d) => Some(d),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        let hold_load = recommended.iter().any(holds_load);
        let mut eased = vec![0u8; items.len()];
        let mut clamped = vec![false; items.len()];
        for (i, it) in items.iter_mut().enumerate() {
            if !it.is_lift() {
                continue;
            }
            let rir_target = matches!(
                it.intensity,
                PlanIntensity::Rir { .. } | PlanIntensity::RirRange { .. }
            );
            let d = hold_cap.max(if rir_target { rpe_cap } else { 0 });
            if d > 0 && cap_item_effort(it, d, weight_lb) {
                eased[i] = d;
            }
            if hold_load && clamp_to_hold_cap(it, weight_lb) {
                clamped[i] = true;
                it.load_held = true;
                resync_intensity(it, weight_lb);
            }
        }
        Self { eased, clamped }
    }
}

/// `ReduceLoadPct`: the weight is already cut by the fold; annotate the
/// affected items and re-render their label off the folded numbers. A day
/// with no prescribed weight is unchanged.
fn note_load_cut(items: &mut [PlanItem], pct: f32, weight_lb: bool) -> bool {
    let mut any_changed = false;
    for it in items.iter_mut() {
        if it.is_lift() && it.load_kg.is_some() {
            it.adjusted_note = format!("{pct:.0}% less weight today");
            resync_intensity(it, weight_lb);
            any_changed = true;
        }
    }
    any_changed
}

/// `Deload`: volume + load are already folded (runs included); annotate and
/// re-render. A recovery / failed-session week also defers hard running: every
/// run goes easy at its cut volume.
fn note_deload(
    items: &mut [PlanItem],
    r: &AdjustmentCall,
    (volume_reduction_pct, load_reduction_pct): (f32, f32),
    weight_lb: bool,
) -> bool {
    let easy_runs = deload_defers_hard_running(r);
    for it in items.iter_mut() {
        if !it.is_lift() {
            if easy_runs {
                cap_run_item_easy(it, r, "lighter week: easy running only");
            }
            continue;
        }
        it.adjusted_note = format!(
            "lighter week{}",
            deload_cut_clauses(volume_reduction_pct, load_reduction_pct)
                .map(|c| format!(": {c}"))
                .unwrap_or_default()
        );
        resync_intensity(it, weight_lb);
    }
    true
}

/// `CapRpe`: the effort pass moved RIR targets (single value AND calisthenics
/// bands) off the STRUCTURED field; the session is "adjusted" ONLY when a rung
/// actually moved (a holds-only / prose / %e1RM session is a genuine no-op, not
/// an "adjusted" chip over an unchanged top-end — HARD RULE 3).
fn note_rpe_cap(items: &mut [PlanItem], eased: &[u8]) -> bool {
    let mut any_changed = false;
    for (it, &d) in items.iter_mut().zip(eased) {
        if d > 0 && it.is_lift() {
            if it.adjusted_note.is_empty() {
                it.adjusted_note = format!("stop {} earlier than planned", fmt_reps(d.into()));
            }
            any_changed = true;
        }
    }
    any_changed
}

/// `CapTopEnd` (ready-upd-001: hold progression). Lifts add no load over what
/// was already lifted for the reps and keep one more rep in reserve (both
/// applied once by the effort pass, however many holds fire); runs go easy
/// (except for soreness: "keep systemic intensity"), citing the trigger's own
/// run-side rule rather than the lifting-only hold claim.
fn apply_progression_hold(
    items: &mut [PlanItem],
    r: &AdjustmentCall,
    trigger: ReadinessSignal,
    effort: &EffortPass,
) -> bool {
    let run_rec = run_side_rec(r);
    let mut any_changed = false;
    for (i, it) in items.iter_mut().enumerate() {
        if !it.is_lift() {
            if trigger != ReadinessSignal::Soreness {
                cap_run_item_easy(it, &run_rec, "easy today: no hard running");
                any_changed = true;
            }
        } else if effort.eased[i] > 0 || effort.clamped[i] {
            it.adjusted_note = hold_note(effort.eased[i]);
            any_changed = true;
        }
    }
    any_changed
}

/// A non-safety `DowngradeSession`, "one level": lifts keep a numeric load (the
/// fold already took [`ONE_LEVEL_LOAD_FRAC`] off) and get one more rep in
/// reserve (from the effort pass); runs go easy, keeping their duration. HRV
/// and velocity loss only hold lifts; decoupling leaves them alone.
fn apply_one_level_downgrade(
    items: &mut [PlanItem],
    r: &AdjustmentCall,
    effort: &EffortPass,
    weight_lb: bool,
) -> bool {
    let lifts = lift_downgrade(r);
    let runs = downgrade_eases_runs(r);
    let mut any_changed = false;
    for (i, it) in items.iter_mut().enumerate() {
        if !it.is_lift() {
            if runs {
                cap_run_item_easy(it, r, "one step easier today");
                any_changed = true;
            }
            continue;
        }
        let eased = effort.eased[i];
        match lifts {
            LiftDowngrade::Unchanged => continue,
            LiftDowngrade::Hold => {
                if eased > 0 || effort.clamped[i] {
                    it.adjusted_note = hold_note(eased);
                    any_changed = true;
                }
            }
            LiftDowngrade::OneLevel => {
                let earlier = fmt_reps(eased.max(1).into());
                it.adjusted_note = if it.load_kg.is_some() {
                    format!(
                        "one step easier: −{:.0}% weight, stop {earlier} earlier",
                        ONE_LEVEL_LOAD_FRAC * 100.0
                    )
                } else {
                    format!("one step easier: stop {earlier} earlier")
                };
                any_changed = true;
            }
        }
        resync_intensity(it, weight_lb);
    }
    any_changed
}

/// A safety easy day (reactive tendon pain, a head cold, the first session
/// back after a hold): no heavy load, at least [`EASY_DAY_RIR`] reps in
/// reserve.
fn apply_safety_easy_day(items: &mut [PlanItem], r: &AdjustmentCall, weight_lb: bool) -> bool {
    let note = match r.kind() {
        AdjustmentKind::ReturnHold => "first session back: keep it easy",
        AdjustmentKind::HeadCold => "head cold: light, half the sets",
        _ => "easy today: protect the sore tendon",
    };
    for it in items.iter_mut() {
        if !it.is_lift() && head_cold(r) {
            // A head cold never keeps a long or hard run: every run becomes
            // one short easy run, or rest.
            cap_run_for_head_cold(it);
        } else if !it.is_lift() {
            cap_run_item_easy(it, r, note);
        } else {
            let label = if it.timed_hold {
                "easy: shorter holds, stop well before your form breaks"
            } else {
                "keep it easy"
            };
            ease_lift_item(it, label, note, weight_lb);
        }
    }
    true
}

/// `ReduceVolumePct` (autoreg-045): lifts' set counts and runs'
/// duration/distance (or interval reps) are already folded; only the lift note
/// is left.
fn note_volume_cut(items: &mut [PlanItem], pct: f32) -> bool {
    for it in items.iter_mut() {
        if it.is_lift() && it.adjusted_note.is_empty() {
            it.adjusted_note = format!("{pct:.0}% fewer sets today");
        }
    }
    true
}

/// `StopItems` (safety-038 "STOP that exercise"): only the movements that load
/// the painful region stop, marked in `stopped`; the rest goes ahead (no load
/// is ever auto-added while pain is active, autoreg-044).
fn apply_pain_stop(
    items: &mut [PlanItem],
    r: &AdjustmentCall,
    (location, region): (&str, PainRegion),
    clamped: &[bool],
    stopped: &mut [bool],
) -> bool {
    let mut any_changed = false;
    for (idx, it) in items.iter_mut().enumerate() {
        if crate::autoreg::item_spared_by_pain(region, &it.exercise) {
            if clamped[idx] {
                if it.adjusted_note.is_empty() {
                    it.adjusted_note = hold_note(0);
                }
                any_changed = true;
            }
            continue;
        }
        stop_item_for_pain(it, location, r);
        stopped[idx] = true;
        any_changed = true;
    }
    any_changed
}

/// `ModifyAndMonitor`: tolerable, stable tendon pain (Table 4.1 / safety-039):
/// "modify the provoking movement & monitor; avoid complete rest". Never
/// program a top-end into active pain (HARD RULE 3): the lifts that load the
/// painful region (every lift when the location maps to none) lose the heavy
/// load and go light and pain-free; the lifts it spares are held (no load
/// added, one rep earlier, from the effort pass, autoreg-044); runs go easy.
/// No fabricated % (the KB states none for this response).
fn apply_modify_and_monitor(
    items: &mut [PlanItem],
    r: &AdjustmentCall,
    (location, region): (&str, Option<PainRegion>),
    effort: &EffortPass,
    weight_lb: bool,
) -> bool {
    let note = if location.is_empty() {
        "eased for tendon pain".to_string()
    } else {
        format!("eased for {location} tendon pain")
    };
    for (i, it) in items.iter_mut().enumerate() {
        if !it.is_lift() {
            cap_run_item_easy(it, r, "tendon pain: keep it easy and pain-free");
        } else if region.is_some_and(|reg| crate::autoreg::item_spared_by_pain(reg, &it.exercise)) {
            if effort.eased[i] > 0 || effort.clamped[i] {
                it.adjusted_note = hold_note(effort.eased[i]);
            }
        } else {
            let label = if it.timed_hold {
                "light: shorter holds or an easier version; stop if your form slips"
            } else {
                "light and pain-free; change the movement if it hurts"
            };
            ease_lift_item(it, label, &note, weight_lb);
        }
    }
    true
}

/// Whether `r` is a tendon modify-and-monitor call scoped to a body region
/// (it eases only the lifts that load it, the rest are held).
fn scoped_modify(r: &AdjustmentCall) -> bool {
    matches!(r.kind(), AdjustmentKind::TendonModify(Some(_)))
}

/// The extra reps in reserve one readiness call holds every lift to: a
/// progression hold (ready-upd-001), a non-safety downgrade that touches
/// lifts, or a scoped tendon modify (for the lifts it spares) adds one;
/// anything else none.
fn lift_effort_hold(r: &AdjustmentCall) -> u8 {
    match &r.value {
        Adjustment::CapTopEnd { .. } => 1,
        Adjustment::DowngradeSession
            if !safety_easy(r) && lift_downgrade(r) != LiftDowngrade::Unchanged =>
        {
            1
        }
        Adjustment::ModifyAndMonitor { .. } if scoped_modify(r) => 1,
        _ => 0,
    }
}

/// The session-wide extra reserve today's readiness calls put on a lift: the
/// deepest single call, never their sum.
pub(super) fn session_effort_cap(recs: &[AdjustmentCall]) -> u8 {
    recs.iter()
        .map(|r| match r.value {
            Adjustment::CapRpe(d) => d,
            _ => lift_effort_hold(r),
        })
        .max()
        .unwrap_or(0)
}

/// Whether a call holds lift loads where they are (no load added): a
/// progression hold (ready-upd-001), an HRV / bar-speed hold downgrade, or a
/// localized pain stop or scoped tendon modify for the lifts it spares
/// (autoreg-044).
fn holds_load(r: &AdjustmentCall) -> bool {
    match &r.value {
        Adjustment::CapTopEnd { .. } | Adjustment::StopItems { .. } => true,
        Adjustment::ModifyAndMonitor { .. } => scoped_modify(r),
        Adjustment::DowngradeSession => !safety_easy(r) && lift_downgrade(r) == LiftDowngrade::Hold,
        _ => false,
    }
}

/// A held lift's note: no weight added, and how much earlier to stop.
fn hold_note(eased: u8) -> String {
    if eased > 0 {
        format!("no weight added, stop {} earlier", fmt_reps(eased.into()))
    } else {
        "no weight added".into()
    }
}

/// Keep a lift on a held day at or under the heaviest weight already lifted
/// for its reps (`hold_load_cap_kg`): the prescribed load (its %e1RM label
/// following) or, for an effort-prescribed lift, the suggested start. The
/// logged sets are re-judged at the held load. Returns whether it moved.
fn clamp_to_hold_cap(it: &mut PlanItem, weight_lb: bool) -> bool {
    let Some(cap) = it.hold_load_cap_kg.filter(|c| *c > 0.0) else {
        return false;
    };
    match (it.load_kg, it.suggested_load_kg) {
        (Some(load), _) if cap + 1e-9 < load => {
            it.load_kg = Some(cap);
            reduce_percent_intensity(&mut it.intensity, 1.0 - cap / load);
            rejudge_at_load(it, cap, weight_lb);
            true
        }
        (None, Some(start)) if cap + 1e-9 < start => {
            it.suggested_load_kg = Some(cap);
            true
        }
        _ => false,
    }
}

/// Re-judge an item's logged sets against a lowered load `bar`: a set at it
/// (within one plate step, [`load_grid_step_kg`]) for the prescribed reps is
/// a full set.
pub(super) fn rejudge_at_load(it: &mut PrescriptionView, bar: f64, weight_lb: bool) {
    let reps_low = u32::from(it.reps_low);
    let step = load_grid_step_kg(bar, weight_lb);
    for s in it.logged_sets.iter_mut() {
        if s.hold_sec.is_none() && s.reps >= reps_low && s.weight_kg + 1e-9 >= bar - step {
            s.qualifies = true;
            s.lighter = false;
        }
    }
    it.sets_done = it
        .logged_sets
        .iter()
        .filter(|s| s.qualifies)
        .count()
        .min(u8::MAX as usize) as u8;
}

/// The RPE a %e1RM target implies for `reps_low` reps on the Epley inverse
/// (File 02 strength-005), its reserve rounded and held to 0–4.
pub(super) fn percent_effort_rpe(pct: f64, reps_low: u8) -> f64 {
    let rir = crate::strength::expected_rir_at_pct(pct, u32::from(reps_low.max(1)))
        .round()
        .clamp(0.0, 4.0);
    crate::strength::rir_to_rpe(rir)
}

/// Raise reps-in-reserve on one lift item by `d` (planned RPE − d), whatever its
/// target form: a RIR target is capped structurally; a %e1RM target keeps its
/// load and gets a lowered RPE ceiling ([`percent_effort_rpe`]). Returns
/// whether anything changed.
pub(super) fn cap_item_effort(it: &mut PlanItem, d: u8, weight_lb: bool) -> bool {
    if let Some(reserve) = cap_rpe_intensity(&mut it.intensity, d) {
        it.target_rpe = Some(crate::strength::rir_to_rpe(reserve as f64));
        resync_intensity(it, weight_lb);
        return true;
    }
    let planned = it.target_rpe.or_else(|| match it.intensity {
        PlanIntensity::PercentE1rm { pct, .. } if pct > 0 => {
            Some(percent_effort_rpe(f64::from(pct), it.reps_low))
        }
        _ => None,
    });
    match planned {
        Some(rpe) => {
            it.target_rpe = Some((rpe - f64::from(d)).max(0.0));
            // The face's "reps left" reads `target_rpe` for these targets:
            // re-render it so the hero, the set loop and the banner agree.
            resync_intensity(it, weight_lb);
            true
        }
        None => false,
    }
}

/// Rewrite a lift item as an easy/light prose line for a safety easy day or a
/// modify-and-monitor pain response: no load, no adjustable RIR/%e1RM target,
/// and the numeric prefill capped at the easy [`EASY_DAY_RIR`] reps-in-reserve
/// ceiling, never raising an already-lower target nor seeding one above the
/// light, pain-free intent (HARD RULE 3).
fn ease_lift_item(it: &mut PlanItem, label: &str, note: &str, weight_lb: bool) {
    it.load_kg = None;
    it.pick_load = true;
    it.intensity_label = label.into();
    it.intensity = PlanIntensity::Prose;
    let easy = crate::strength::rir_to_rpe(f64::from(EASY_DAY_RIR));
    it.target_rpe = Some(it.target_rpe.map_or(easy, |r| r.min(easy)));
    clear_anchored_on(it);
    it.adjusted_note = note.into();
    refresh_lift_summary(it, weight_lb);
}

/// Drop the "anchored on" basis of an item whose load no longer comes from it.
fn clear_anchored_on(it: &mut PlanItem) {
    it.anchored_on.clear();
    it.anchored_on_last = false;
    it.anchored_on_single_set = false;
}

/// Render one plan item as stopped for a localized pain report:
/// no load, nothing to log, the evidence re-pointed to the pain rule.
fn stop_item_for_pain(it: &mut PlanItem, location: &str, r: &AdjustmentCall) {
    it.load_kg = None;
    it.intensity = PlanIntensity::Prose;
    it.intensity_label = "Skip today, or swap for a movement that doesn't hurt".into();
    if !it.is_lift() {
        let line = format!("Run: skip today ({location} pain)");
        set_run_summary(it, None, None, &line, &line);
    } else {
        let lead = FaceClause::plain(format!("skip today ({location} pain)"));
        set_named_summary(it, &[lead], &[]);
    }
    clear_run_reps(it);
    it.run_volume = None;
    // Nothing to run: the tracker must not walk the planned intervals.
    it.run_structure = None;
    clear_anchored_on(it);
    it.adjusted_note =
        format!("Stopped for {location} pain. Skip it, or swap in a movement that doesn't hurt.");
    repoint_evidence(it, r, None);
}

/// The one-item post-illness return check (ready-upd-003, ILLNESS-RTT-001):
/// [`RETURN_DOSE_MIN`] minutes at [`RETURN_DOSE_EFFORT_PCT`] of usual effort,
/// with the stop list, logged as `kind`: a return check in a lifting session,
/// else a run. `done` once anything was logged today.
fn return_check_item(r: &AdjustmentCall, kind: ItemKind, done: bool) -> PlanItem {
    let (pct_lo, pct_hi) = RETURN_DOSE_EFFORT_PCT;
    let summary = return_check_headline();
    PlanItem::from(PrescriptionView {
        face_lead: summary.clone(),
        summary,
        intensity_label: format!("{pct_lo}–{pct_hi}% of your usual effort"),
        adjusted_note: format!("{RETURN_CHECK_STOP} Check in again tomorrow morning."),
        return_check: true,
        kind,
        evidence: EvidenceTagView::of(r),
        why: why_from(None, r),
        done,
        ..PrescriptionView::default()
    })
}

/// A head cold (above-neck symptoms only): any planned run, long, hard or
/// easy, becomes one short easy run of [`RETURN_DOSE_MIN`] minutes at
/// [`RETURN_DOSE_EFFORT_PCT`] of usual effort, or rest (ILLNESS-COLD-RUN-001,
/// the IOC 2022 exercise-challenge dose). The
/// planned distance, reps and tracker structure are dropped, never shown. Done
/// once any run is logged on its day.
fn cap_run_for_head_cold(it: &mut PlanItem) {
    let r = AdjustmentKind::HeadCold.call(Adjustment::DowngradeSession, HEAD_COLD_RUN_CLAIM);
    clear_run_reps(it);
    it.run_volume = Some(RunAmount::MinutesRange(
        RETURN_DOSE_MIN.0,
        RETURN_DOSE_MIN.1,
    ));
    it.run_structure = None;
    it.intensity = PlanIntensity::Prose;
    set_easy_run_face(it);
    it.adjusted_note = "Head cold: short easy run only, or rest".into();
    repoint_evidence(it, &r, None);
    it.done = it.ran();
}
