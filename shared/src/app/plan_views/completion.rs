//! Completion judgement: which logged sets and runs count toward a plan item,
//! and the resulting item and session statuses.

use super::*;
use std::fmt::Write as _;

/// One plate step of completion tolerance: 2.5 kg, or 5 lb for a pounds
/// profile. A set is full work once it reaches the prescribed load minus this
/// much (lighter sets may still count, see [`set_credit`]).
const KG_STEP_KG: f64 = 2.5;
/// One 5 lb plate step, in kg.
const LB_STEP_KG: f64 = 5.0 / LB_PER_KG;
pub(super) fn movement_step_kg(weight_lb: bool) -> f64 {
    if weight_lb { LB_STEP_KG } else { KG_STEP_KG }
}

/// A set logged before this local time of day (04:00) may still belong to the
/// previous evening's session.
const SESSION_CHAIN_CUTOFF_SEC: i64 = 4 * 3_600;
/// The longest gap (3 h) after the previous set at which such an early set
/// still chains to that set's session day.
const SESSION_CHAIN_GAP_SEC: i64 = 3 * 3_600;

/// Session-day attribution for every logged set, aligned to `sets` by
/// index: `None` for an undated legacy row ([`is_undated`], which never
/// counts toward a day), else the local day it belongs to. Sets are walked in
/// `observed_at` order; a set logged before [`SESSION_CHAIN_CUTOFF_SEC`]
/// inherits the previous set's session day when that set is at most
/// [`SESSION_CHAIN_GAP_SEC`] earlier (chaining transitively), so a late
/// session that crosses midnight scores as ONE day.
/// The single attribution pass every `view()` shares — the completion counts and
/// the per-day plan-snapshot cutoff both read it, so no rescan per rendered day.
fn attribute_session_days(sets: &[LoggedSet], offset: i64) -> Vec<Option<i64>> {
    let mut order: Vec<usize> = (0..sets.len())
        .filter(|&i| !is_undated(sets[i].observed_at))
        .collect();
    order.sort_by_key(|&i| sets[i].observed_at);
    let mut out = vec![None; sets.len()];
    let mut prev: Option<(i64, i64)> = None; // (observed_at, session_day)
    for &i in &order {
        let oa = sets[i].observed_at;
        let shifted = oa.saturating_add(offset);
        let local = shifted.div_euclid(DAY_SEC);
        let tod = shifted.rem_euclid(DAY_SEC);
        let sd = match prev {
            Some((poa, psd))
                if tod < SESSION_CHAIN_CUTOFF_SEC
                    && oa.saturating_sub(poa) <= SESSION_CHAIN_GAP_SEC =>
            {
                psd
            }
            _ => local,
        };
        out[i] = Some(sd);
        prev = Some((oa, sd));
    }
    out
}

/// Per-run session day, aligned to `runs` by index: the local day of the run's
/// START, [`RunFacts::started`] (its first QC'd fix, else its stamp minus its
/// duration), the one start every view and coaching bucket uses. `None` for an
/// undated legacy run ([`is_undated`]), which never counts.
fn attribute_run_days(runs: &[LoggedRun], facts: &RunFacts, offset: i64) -> Vec<Option<i64>> {
    runs.iter()
        .zip(&facts.started)
        .map(|(r, &start)| (!is_undated(r.observed_at)).then(|| local_day(start, offset)))
        .collect()
}

/// Every logged set's and run's session day plus the distinct local days with
/// lifting and with running, computed once per `view()` and shared by the
/// completion counts, the per-day snapshot cutoff and the plan-week records.
pub(in crate::app) struct LoggedDays {
    /// Per-set session day, aligned to `model.sets`.
    pub(super) sets: Vec<Option<i64>>,
    /// Per-run session day (its start day), aligned to `model.runs`.
    pub(super) runs: Vec<Option<i64>>,
    /// Distinct days with lifting / running, indexed by [`Discipline`].
    pub(super) by_discipline: [std::collections::BTreeSet<i64>; 2],
    /// Distinct days with any logged training.
    pub(super) active: std::collections::BTreeSet<i64>,
    /// The view's plan-week records through today's week, built on first use
    /// (`consistency::plan_weeks`).
    pub(super) plan_weeks: std::cell::OnceCell<Option<PlanWeeks>>,
}

impl LoggedDays {
    pub(in crate::app) fn new(model: &Model, facts: &RunFacts) -> Self {
        let offset = model.today_utc_offset_sec;
        let sets = attribute_session_days(&model.sets, offset);
        let runs = attribute_run_days(&model.runs, facts, offset);
        let by_discipline: [std::collections::BTreeSet<i64>; 2] = [
            sets.iter().flatten().copied().collect(),
            runs.iter().flatten().copied().collect(),
        ];
        let active = by_discipline[0].union(&by_discipline[1]).copied().collect();
        Self {
            sets,
            runs,
            by_discipline,
            active,
            plan_weeks: std::cell::OnceCell::new(),
        }
    }

    /// Per-run session day (the day the run STARTED), aligned to `model.runs`;
    /// `None` for an undated run.
    pub(in crate::app) fn run_days(&self) -> &[Option<i64>] {
        &self.runs
    }
}

/// How one logged set counts toward a plan item.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SetCredit {
    /// A warm-up, or a set too light to be a work set.
    NotCounted,
    /// A work set at the full (shown) prescribed load.
    Full,
    /// A work set below the full prescribed load ("lighter than planned").
    Lighter,
}

/// The share of the shown prescribed load a set must reach to count as a
/// (lighter) work set: the specific warm-up ramp tops out at ~80 % of the
/// working load (STR-WARMUP-001), so a lighter set is never work.
const WORK_SET_FLOOR: f64 = 0.80;
/// Without a prescribed load, a working set is one at this share of the day's
/// heaviest same-movement weight; lighter sets are warm-ups.
const NO_LOAD_WORK_SHARE: f64 = 0.9;
/// The set screen's warm-up ramp after its first step (STR-WARMUP-001; the
/// exact steps are expert opinion): 40 % × 5, 60 % × 3 and 80 % × 2 of the
/// working load.
const WARM_UP_RAMP: [(f64, u32); 3] = [(0.4, 5), (0.6, 3), (0.8, 2)];

/// How a logged set that is not a warm-up counts toward a plan item. The
/// caller has matched movement (`same_movement`); RPE never gates. Any set of
/// one rep (or one hold second) or more can count: a short set still fills its
/// slot. `fold` is the adjustment picture shown for that day: a load cut
/// lowers the bar to what the user actually saw, and a downgrade/modify day
/// (`load_dropped`) removes the load bar entirely. With a prescribed load, a
/// set within one plate step of it is full work and one at ≥80 % of it is
/// lighter work. Without a prescribed load the bar is [`NO_LOAD_WORK_SHARE`]
/// of the day's heaviest same-movement weight, so a 0 kg bodyweight set
/// passes (0 >= 0.9·0).
fn set_credit(
    it: &PrescriptionView,
    s: &LoggedSet,
    heaviest: f64,
    step: f64,
    fold: AdjustFold,
) -> SetCredit {
    let worked = if it.timed_hold {
        s.hold_sec.unwrap_or(0) > 0
    } else {
        s.reps >= 1
    };
    if !worked {
        return SetCredit::NotCounted;
    }
    let near_heaviest = if s.weight_kg >= NO_LOAD_WORK_SHARE * heaviest {
        SetCredit::Full
    } else {
        SetCredit::NotCounted
    };
    if fold.drops_load(it) {
        // The day was shown with no load bar (easy/pain-free session), so any
        // working set counts.
        return near_heaviest;
    }
    let bar = match (it.load_kg, it.added_kg.filter(|a| *a > 0.0)) {
        // The load shown after a readiness/deload cut, so following the shown
        // session is full work.
        (Some(load), _) => load * (1.0 - fold.load_frac),
        (None, Some(added)) => added,
        (None, None) => return near_heaviest,
    };
    if s.weight_kg >= bar - step {
        SetCredit::Full
    } else if s.weight_kg >= WORK_SET_FLOOR * bar {
        SetCredit::Lighter
    } else {
        SetCredit::NotCounted
    }
}

/// The warm-up steps `(kg, reps)` the set screen offers before the first work
/// set at `work_kg` of a barbell lift (`warmUpSteps` in `SetEditorState.kt`):
/// a first step × 5 (the empty bar, or for a pull from the floor the bar with
/// one full plate a side), then [`WARM_UP_RAMP`] on the unit's plate grid. A
/// ramp step that rounds onto the first step or up to the work load is
/// dropped. Empty at or below the first step.
fn warm_up_steps(work_kg: f64, weight_lb: bool, exercise: &str) -> Vec<(f64, u32)> {
    let lb_kg = |lb: f64| (lb / LB_PER_KG * 100.0).round() / 100.0;
    let first = match (starts_from_floor(exercise), weight_lb) {
        (true, true) => lb_kg(135.0),
        (true, false) => 60.0,
        (false, true) => lb_kg(45.0),
        (false, false) => 20.0,
    };
    if work_kg <= first + 1e-6 {
        return Vec::new();
    }
    let mut steps = vec![(first, 5)];
    for (share, reps) in WARM_UP_RAMP {
        let kg = if weight_lb {
            lb_kg((work_kg * share * LB_PER_KG / 5.0).round() * 5.0)
        } else {
            (work_kg * share / 2.5).round() * 2.5
        };
        if kg > first + 1e-6 && kg < work_kg - 1e-6 && steps.iter().all(|(k, _)| *k != kg) {
            steps.push((kg, reps));
        }
    }
    steps
}

/// Whether `exercise` starts each rep from the floor (a deadlift, a clean or
/// snatch from the floor, a Pendlay row; not a Romanian, stiff-leg or hang
/// variant), as `isFloorLift` in `SetEditorState.kt` reads it.
fn starts_from_floor(exercise: &str) -> bool {
    let e = exercise.to_lowercase();
    let has = |k: &str| e.contains(k);
    !(has("romanian") || has("rdl") || has("stiff") || has("hang"))
        && (has("deadlift") || has("clean") || has("snatch") || has("pendlay"))
}

/// Whether a set fell short of the prescribed reps (or hold).
fn short_of_target(it: &PrescriptionView, reps: u32, hold_sec: Option<u16>) -> bool {
    if it.timed_hold {
        hold_sec.unwrap_or(0) < it.hold_low_sec
    } else {
        reps < u32::from(it.reps_low.max(1))
    }
}

/// A display-ready, unit-aware one-line summary of a logged set for a shell:
/// "120 kg × 3", "265 lb × 3", "30 s hold", "30 s hold · +10 kg". Weight and
/// reps (or hold) only: the effort a set was rated at stays on the set's own
/// `rpe` field and behind the "?", so the faces carry one effort wording.
fn item_set_summary(s: &LoggedSet, weight_lb: bool) -> String {
    if let Some(hold) = s.hold_sec {
        let mut base = format!("{hold} s hold");
        if s.weight_kg > 0.0 {
            let _ = write!(base, " · +{}", fmt_weight(s.weight_kg, weight_lb));
        }
        base
    } else {
        format!("{} × {}", fmt_weight(s.weight_kg, weight_lb), s.reps)
    }
}

impl ItemSetView {
    /// `s` as an item shows it, not yet judged (not qualifying, lighter or short).
    fn of(s: &LoggedSet, weight_lb: bool) -> Self {
        ItemSetView {
            entry_id: s.entry_id,
            observed_at: s.observed_at,
            weight_kg: s.weight_kg,
            reps: s.reps,
            rpe: s.rpe,
            hold_sec: s.hold_sec,
            qualifies: false,
            lighter: false,
            short: false,
            summary: item_set_summary(s, weight_lb),
        }
    }
}

/// STR-LOADPROG-001 (add load once 1–2 reps over target; strength-012
/// 2-for-2): the reps a last top set must beat today's target (reps + reserve)
/// by before the suggested start steps up one plate.
const STEP_UP_SURPLUS_REPS: f64 = 2.0;

/// Give every barbell/dumbbell lift item its `last_set` — the top working set
/// (heaviest; then most reps; then latest) of the most recent PRIOR session
/// day of the same movement — and, when the item prescribes no load (an
/// effort target), a `suggested_load_kg` from it: the weight as logged, or one
/// plate step up once that set beat the target by [`STEP_UP_SURPLUS_REPS`],
/// never a jump above +10 % (`MAX_LOAD_STEP_FRAC`). An unrated or open-ended
/// set counts only its reps (a lower bound). A set with fewer reps in the tank
/// than today's target is lightened to that target on the Epley model
/// (strength-005), and the movement's one working-load derate (or, without an
/// anchor, the detraining credit for the time since the set, DETRAIN-STR-001)
/// scales it. Past the 8-week horizon or inside a declared >8 wk re-entry
/// (REENTRY-001: treat as novice) nothing is suggested. The athlete's own
/// data, never an invented load.
pub(in crate::app) fn apply_last_sets(
    items: &mut [PrescriptionView],
    model: &Model,
    set_index: &SetIndex,
    anchors: &crate::plan::Anchors,
    session_days: &[Option<i64>],
    epoch_day: i64,
    weight_lb: bool,
) {
    let step = movement_step_kg(weight_lb);
    for it in items.iter_mut() {
        if it.is_run() || it.added_kg.is_some() || it.timed_hold {
            continue;
        }
        let Some((set_day, i)) = last_top_set(
            &model.sets,
            set_index.same_movement_sets(&it.exercise),
            epoch_day,
            |i| session_days.get(i).copied().flatten(),
        ) else {
            continue;
        };
        let s = &model.sets[i];
        let last = ItemSetView {
            qualifies: true,
            ..ItemSetView::of(s, weight_lb)
        };
        let summary = last.summary.clone();
        it.last_set = Some(last);
        let age_days = epoch_day.saturating_sub(set_day);
        if it.load_kg.is_none()
            && s.weight_kg > 0.0
            && !anchors.reentry_novice
            && let Some(credit) = crate::strength::lift_detrain_credit(age_days).value
            && let Some(target_rpe) = it.target_rpe
        {
            let target = it.reps_high as f64 + crate::strength::rpe_to_rir(target_rpe);
            let capacity = crate::strength::effective_reps_f(s.reps, s.rpe);
            // One derate, never stacked: the anchor's own when the movement has
            // one, else the detraining credit for this set's age.
            let derate = anchors.carried_derate_for(&it.exercise);
            let frac = derate.map_or(credit, |d| d.frac);
            let scale = if capacity < target {
                crate::strength::e1rm_epley_frac(1.0, capacity)
                    / crate::strength::e1rm_epley_frac(1.0, target)
            } else {
                1.0
            };
            let step_up = frac >= 1.0
                && capacity - target >= STEP_UP_SURPLUS_REPS
                && step <= crate::strength::MAX_LOAD_STEP_FRAC * s.weight_kg;
            // A load the athlete lifted is suggested exactly as logged; only a
            // computed one (stepped, derated, rep-scaled) lands on the plate
            // grid (`round_load_grid` keeps an on-grid lb load on its plate).
            let suggested = if step_up {
                round_load_grid(s.weight_kg + step, weight_lb)
            } else if frac < 1.0 || scale < 1.0 {
                round_load_grid(s.weight_kg * frac * scale, weight_lb)
            } else {
                s.weight_kg
            };
            if suggested > 0.0 {
                it.suggested_load_kg = Some(suggested);
                let reason = if step_up {
                    format!(
                        "one step up: your last set beat the target by {STEP_UP_SURPLUS_REPS}+ reps"
                    )
                } else if frac < 1.0 {
                    let cause = derate.map_or(
                        crate::plan::DerateCause::Detraining {
                            days_since: age_days,
                        },
                        |d| d.cause,
                    );
                    format!(
                        "{:.0}% of your last top set, {}",
                        frac * 100.0,
                        derate_reason(cause)
                    )
                } else if scale < 1.0 {
                    "your last top set, lightened for today's rep target".to_string()
                } else {
                    "the weight of your last top set".to_string()
                };
                let line = format!(
                    "Start from {} ({reason}), then adjust to hit the target effort.",
                    fmt_weight(suggested, weight_lb)
                );
                it.why.append_basis(&line);
            }
        }
        // `Last: 60 kg × 8`, the one "last time" wording every item face uses.
        if it.anchored_on.is_empty() {
            it.anchored_on = format!("Last: {summary}");
            it.anchored_on_last = true;
            it.anchored_on_single_set = true;
        }
        refresh_lift_summary(it, weight_lb);
    }
}

/// The top working set of the most recent day before `before_day` among the
/// `candidates` (positions in `sets`, ascending: one movement's sets, see
/// [`SetIndex::same_movement_sets`]): heaviest, then most reps, then the
/// latest stamp, and on a same-stamp tie the FIRST logged
/// ([`LoggedSet::chrono_cmp`]: lower `entry_id`, then content), never the
/// storage slot an Undo moves. Returns its day and
/// position. `day_of(i)` is set `i`'s day, `None` when it cannot be dated.
/// Timed holds and rep-less sets never count. The one rule behind the plan
/// item's "Last:" line and the review's "Last time" basis; only the day
/// attribution differs between them.
pub(in crate::app) fn last_top_set(
    sets: &[LoggedSet],
    candidates: &[usize],
    before_day: i64,
    mut day_of: impl FnMut(usize) -> Option<i64>,
) -> Option<(i64, usize)> {
    let mut best: Option<(i64, usize)> = None;
    for &i in candidates {
        let s = &sets[i];
        let Some(d) = day_of(i) else {
            continue;
        };
        if d >= before_day || s.hold_sec.is_some() || s.reps == 0 {
            continue;
        }
        let better = best.is_none_or(|(bd, bi)| {
            let b = &sets[bi];
            d.cmp(&bd)
                .then(s.weight_kg.total_cmp(&b.weight_kg))
                .then(s.reps.cmp(&b.reps))
                .then(s.observed_at.cmp(&b.observed_at))
                .then_with(|| b.chrono_cmp(s))
                .is_gt()
        });
        if better {
            best = Some((d, i));
        }
    }
    best
}

/// Give each loaded lift item its `hold_load_cap_kg`: the heaviest weight
/// lifted on the same movement for at least the item's prescribed reps on a
/// day inside the anchor window before `before_day`. A held day may not go
/// above it (ready-upd-001: no load added). Matching the reps keeps the cap to
/// comparable work under DUP: a heavy day's triples never cap a ×5 day, and a
/// deload or speed day never drags a heavier day down (the window's best wins).
fn apply_hold_caps(
    items: &mut [PrescriptionView],
    model: &Model,
    set_index: &SetIndex,
    session_days: &[Option<i64>],
    before_day: i64,
) {
    // Saturating: a corrupt log's SetToday near i64::MIN must not overflow.
    let from = before_day.saturating_sub(crate::app::anchors::WINDOW_DAYS);
    for it in items.iter_mut() {
        if it.is_run() || it.timed_hold || it.reps_low == 0 {
            continue;
        }
        let reps = u32::from(it.reps_low);
        it.hold_load_cap_kg = set_index
            .same_movement_sets(&it.exercise)
            .iter()
            .map(|&i| (i, &model.sets[i]))
            .filter(|(i, s)| {
                session_days
                    .get(*i)
                    .copied()
                    .flatten()
                    .is_some_and(|d| d >= from && d < before_day)
                    && s.hold_sec.is_none()
                    && s.reps >= reps
                    && s.weight_kg > 0.0
            })
            .map(|(_, s)| s.weight_kg)
            .reduce(f64::max);
    }
}

/// The share of a run item's prescribed volume that completes it.
const RUN_DONE_SHARE: f64 = 0.8;

/// The share of a session's planned work that still credits it "done
/// lighter", not "missed": half the planned work sets of a lift day, half the
/// planned distance or time of a run. A shortened session still trained
/// (strength holds on far less volume when the load is kept, Spiering 2021 in
/// the dose-response review), and a short session is normal, never a failure
/// (ADH-MISSED-001).
const LIGHTER_CREDIT_SHARE: f64 = 0.5;

/// The share of a run item's prescribed volume its attributed km/min cover,
/// judged against the run as shown that day. A non-run prescription covers
/// nothing; a zero-volume run is always covered.
fn run_item_share(rx: &Recommended<Prescription>, km: f64, min: f64, vol_frac: f64) -> f64 {
    let Prescription::Run(r) = &rx.value else {
        return 0.0;
    };
    // A volume cut in force (deload, head cold) shortens the run it is judged
    // against, the same way `required_sets` shortens a lift.
    let keep = 1.0 - vol_frac.clamp(0.0, 1.0);
    let (logged, planned) = match r.volume {
        crate::schema::RunVolume::DistanceKm(d) => (km, f64::from(d)),
        crate::schema::RunVolume::DurationMin(m) => (min, f64::from(m)),
        crate::schema::RunVolume::RunWalk {
            run_sec,
            walk_sec,
            rounds,
        } => (
            min,
            f64::from(rounds) * (f64::from(run_sec) + f64::from(walk_sec)) / 60.0,
        ),
    };
    let target = keep * planned;
    if target > 0.0 {
        logged / target
    } else {
        f64::INFINITY
    }
}

/// The set count required to complete a lift/calisthenics item after folding the
/// day's volume reduction: a deloaded week needs `round(sets·(1−pct/100)).max(1)`
/// sets. A day judged with `frac > 0` is never held to more sets than it showed.
pub(super) fn required_sets(sets: u8, vol_frac: f64) -> u8 {
    let scaled = (sets as f64 * (1.0 - vol_frac)).round() as i64;
    scaled.clamp(1, u8::MAX as i64) as u8
}

/// Whether the user ended this exercise early on `epoch_day`: the latest
/// same-movement marker for the day wins (last-write-wins on `observed_at`).
fn ended_early(model: &Model, exercise: &str, epoch_day: i64) -> bool {
    model
        .ended_exercises
        .iter()
        .filter(|m| m.epoch_day == epoch_day && crate::plan::same_movement(&m.exercise, exercise))
        .max_by_key(|m| m.observed_at)
        .is_some_and(|m| m.ended)
}

/// The week-loop-invariant context for [`flatten_session`]: the clock reference,
/// the plan start day, the model, the display unit, and the once-per-view set/run
/// session-day attribution — so the per-day call carries only the `epoch_day`,
/// snapshot, and adjustment fold that actually vary.
#[derive(Clone, Copy)]
pub(super) struct FlattenCtx<'a> {
    pub(super) today: i64,
    pub(super) start: i64,
    pub(super) model: &'a Model,
    /// Current display unit for the logged-set summaries.
    pub(super) weight_lb: bool,
    /// Per-set session day, aligned to `model.sets`.
    pub(super) session_days: &'a [Option<i64>],
    /// Name-derived facts of `model.sets`.
    pub(super) set_index: &'a SetIndex,
    /// Per-run session day, aligned to `model.runs`.
    pub(super) run_days: &'a [Option<i64>],
    /// Realised distance/moving time per run, aligned to `model.runs`.
    pub(super) run_facts: &'a RunFacts,
}

/// What a planned session is judged against: the local day whose logs count
/// (its own day, or the nearby day a swapped/shifted same-discipline session
/// was done), that day's completion fold, and the adjustments in force then
/// (a localized pain stop marks the items it covers as skipped for pain).
#[derive(Clone, Copy)]
pub(super) struct DayJudge<'r> {
    pub(super) day: i64,
    pub(super) fold: AdjustFold,
    pub(super) recs: &'r [Recommended<Adjustment>],
}

impl<'r> DayJudge<'r> {
    pub(super) fn new(day: i64, recs: &'r [Recommended<Adjustment>]) -> Self {
        Self {
            day,
            fold: fold_completion_adjustments(recs),
            recs,
        }
    }
}

/// Whether a localized pain report in `recs` stops this item: the region rule
/// `annotate_adjustments` applies (safety-038 "stop that exercise").
fn pain_stopped(it: &PrescriptionView, recs: &[Recommended<Adjustment>]) -> bool {
    recs.iter().any(|r| match &r.value {
        Adjustment::StopItems { location } => crate::autoreg::pain_region(location)
            .is_some_and(|region| !crate::autoreg::item_spared_by_pain(region, &it.exercise)),
        _ => false,
    })
}

impl PrescriptionView {
    /// A run item with a run logged against it.
    pub(super) fn ran(&self) -> bool {
        self.run_logged_km > 0.0 || self.run_logged_min > 0.0
    }

    /// Whether any work is logged against the item: a run, or a set of its
    /// movement.
    pub(super) fn has_work(&self) -> bool {
        if self.is_run() {
            self.ran()
        } else {
            self.sets_done > 0 || !self.logged_sets.is_empty()
        }
    }
}

/// One item's status from its completion fields. `pain`: a pain report
/// stopped it.
fn item_status(it: &PrescriptionView, pain: bool) -> ItemStatus {
    if it.is_run() {
        let ran = it.ran();
        return if pain && !ran {
            ItemStatus::SkippedPain
        } else if it.done {
            ItemStatus::Done
        } else if ran && it.run_lighter {
            ItemStatus::DoneLighter
        } else if ran {
            ItemStatus::InProgress
        } else {
            ItemStatus::Open
        };
    }
    if it.sets_done == 0 {
        if pain {
            return ItemStatus::SkippedPain;
        }
        if it.ended_early {
            return ItemStatus::Skipped;
        }
    }
    let required = usize::from(it.sets.max(1));
    let counted = || it.logged_sets.iter().filter(|s| s.qualifies);
    let full = counted().filter(|s| !s.lighter && !s.short).count();
    if full >= required {
        ItemStatus::Done
    } else if pain {
        // A pain report ended it after some work: stopped, never "lighter".
        ItemStatus::StoppedPain
    } else if usize::from(it.sets_done) >= required || it.done {
        // Every slot filled at the planned load with some reps missed reads
        // short; a lighter set or an early end (fewer sets) reads lighter.
        if usize::from(it.sets_done) >= required && !counted().any(|s| s.lighter) {
            ItemStatus::DoneShort
        } else {
            ItemStatus::DoneLighter
        }
    } else if it.has_work() {
        ItemStatus::InProgress
    } else {
        ItemStatus::Open
    }
}

/// Refresh every item's [`ItemStatus`] from its completion fields. `recs` are
/// the adjustments in force on the judged day: a pain stop leaves the items it
/// covers nothing to do (`done`), skipped for pain when no work set was
/// logged. Idempotent (a pain-skipped item stays one), so a caller that
/// re-judges `done`/`sets_done` after this re-runs it.
pub(in crate::app) fn refresh_item_statuses(
    items: &mut [PrescriptionView],
    recs: &[Recommended<Adjustment>],
) {
    for it in items.iter_mut() {
        let pain = matches!(it.status, ItemStatus::SkippedPain | ItemStatus::StoppedPain)
            || pain_stopped(it, recs);
        it.status = item_status(it, pain);
        if pain {
            it.done = true;
        }
    }
}

/// Whether a past day logged at least [`LIGHTER_CREDIT_SHARE`] of its planned
/// work sets with every run item credited, so it reads "done lighter", not
/// "missed". Pain-stopped items are left out.
fn most_work_logged(items: &[PrescriptionView]) -> bool {
    let (mut planned, mut logged) = (0u32, 0u32);
    for it in items {
        if matches!(it.status, ItemStatus::SkippedPain | ItemStatus::StoppedPain) {
            continue;
        }
        if it.is_run() {
            if !matches!(it.status, ItemStatus::Done | ItemStatus::DoneLighter) {
                return false;
            }
            continue;
        }
        let required = u32::from(it.sets.max(1));
        planned += required;
        logged += u32::from(it.sets_done).min(required);
    }
    planned > 0 && f64::from(logged) >= LIGHTER_CREDIT_SHARE * f64::from(planned)
}

/// Whether any of a day's (not pain-stopped) items has work logged against
/// it: a lift set of the movement, or a run.
fn any_work_logged(items: &[PrescriptionView]) -> bool {
    items.iter().any(|it| {
        !matches!(it.status, ItemStatus::SkippedPain | ItemStatus::StoppedPain) && it.has_work()
    })
}

/// A session's status once every item is finished: `done` when every item is
/// done, `done_lighter` when all are finished (done, short, lighter, skipped
/// or stopped) with some work; `None` otherwise or with no items.
pub(in crate::app) fn finished_status(items: &[PrescriptionView]) -> Option<SessionStatus> {
    use ItemStatus::*;
    if items.is_empty() {
        return None;
    }
    if items.iter().all(|it| it.status == Done) {
        return Some(SessionStatus::Done);
    }
    let finished = items.iter().all(|it| {
        matches!(
            it.status,
            Done | DoneLighter | DoneShort | Skipped | SkippedPain | StoppedPain
        )
    });
    let worked = items
        .iter()
        .any(|it| matches!(it.status, Done | DoneLighter | DoneShort | StoppedPain));
    (finished && worked).then_some(SessionStatus::DoneLighter)
}

/// A flattened session's status from its item statuses. A day before the plan
/// started is `before_plan` (never "missed", never counted); a past day reads
/// `done` / `done_lighter` / `partial` (some work, under the lighter credit) /
/// `missed`, today `planned` until finished, the future `planned`. A past
/// session every item of which a pain report stopped is `blocked`, never
/// "missed" (the safety rule stopped it).
fn session_status(
    items: &[PrescriptionView],
    rest: bool,
    epoch_day: i64,
    today: i64,
    start: i64,
) -> SessionStatus {
    if epoch_day < start {
        return SessionStatus::BeforePlan;
    }
    if rest {
        return SessionStatus::Rest;
    }
    if epoch_day > today {
        return SessionStatus::Planned;
    }
    if items.is_empty() {
        return if epoch_day < today {
            SessionStatus::Missed
        } else {
            SessionStatus::Planned
        };
    }
    if let Some(status) = finished_status(items) {
        status
    } else if epoch_day == today {
        SessionStatus::Planned
    } else if items.iter().all(|it| it.status == ItemStatus::SkippedPain) {
        SessionStatus::Blocked
    } else if most_work_logged(items) {
        SessionStatus::DoneLighter
    } else if any_work_logged(items) {
        SessionStatus::Partial
    } else {
        SessionStatus::Missed
    }
}

/// Flatten one plan session onto a calendar day, judging its completion + past
/// status against `judge` (see [`session_status`]): a day BEFORE the plan
/// existed is never scored (a mid-week plan must not back-date the week's
/// earlier days as missed adherence). Each judged lift/calisthenics row then
/// shows the judged `fold` in its own numbers (set count, load, summary), the
/// same numeric fold the hero applies, so a completed deload day reads "3/3"
/// against the 3 sets it showed, never "3/5".
pub(super) fn flatten_session(
    s: &Session,
    epoch_day: i64,
    anchors: &crate::plan::Anchors,
    judge: &DayJudge,
    ctx: &FlattenCtx,
) -> SessionPlanView {
    let FlattenCtx {
        today,
        start,
        model,
        weight_lb,
        session_days,
        set_index,
        ..
    } = *ctx;
    let fold = judge.fold;
    let judge_day = judge.day;
    let bodyweight = model.profile().and_then(|p| p.bodyweight_kg);
    let mut items: Vec<PrescriptionView> = s
        .prescriptions
        .iter()
        .map(|rx| flatten_prescription(rx, anchors, bodyweight, weight_lb))
        .collect();
    // Every run item reads as numbers a runner can follow (pace from race
    // fitness, bpm from HRmax, effort cue), resolved from the anchors.
    crate::app::run_targets::apply_run_targets(&mut items, s, anchors, model, epoch_day);
    // Each lift item carries the athlete's last same-movement top set from a
    // day before the judged work, and an effort-prescribed item a starting
    // weight.
    apply_last_sets(
        &mut items,
        model,
        set_index,
        anchors,
        session_days,
        epoch_day.min(judge_day),
        weight_lb,
    );
    if epoch_day == today {
        apply_hold_caps(
            &mut items,
            model,
            set_index,
            session_days,
            epoch_day.min(judge_day),
        );
    }
    // Fill each item's completion from the judged day's logs — only meaningful
    // inside the plan window (a pre-plan or future day has nothing logged
    // against it), so the flags stay their flattened defaults elsewhere.
    let judged = epoch_day >= start && epoch_day <= today && judge_day <= today;
    if judged {
        for (rx, it) in s.prescriptions.iter().zip(items.iter_mut()) {
            if it.is_run() {
                judge_run_item(it, rx, judge, ctx);
            } else {
                judge_lift_item(it, judge, ctx);
            }
        }
    }
    // Show the day's fold in every lift row's own numbers, after judging (the
    // judgement reads the unfolded prescription + `fold`). A future day's fold
    // is empty, so its rows stay the clean prescription.
    for it in items.iter_mut() {
        fold_item_numbers(it, fold, weight_lb);
    }
    if judged {
        refresh_item_statuses(&mut items, judge.recs);
    }
    let status = session_status(
        &items,
        matches!(s.session_type, SessionType::Rest),
        epoch_day,
        today,
        start,
    );
    SessionPlanView {
        epoch_day,
        title: plan_session_title(s),
        session_type: s.session_type,
        status,
        items,
        adjustment: None,
        // Filled by `build_plan_views` from the week's arrangement.
        move_targets: Vec::new(),
        moved_from_epoch_day: None,
        move_note: None,
        move_blocked_reason: None,
        credited_from_epoch_day: None,
    }
}

/// Judge a run item against the runs attributed to the judged day: done when
/// they clear [`RUN_DONE_SHARE`] of the volume shown that day, done lighter
/// from [`LIGHTER_CREDIT_SHARE`] of it (under a head cold, any run: the shown
/// run is the short easy one).
fn judge_run_item(
    it: &mut PrescriptionView,
    rx: &Recommended<Prescription>,
    judge: &DayJudge,
    ctx: &FlattenCtx,
) {
    let facts = ctx.run_facts;
    let mut km = 0.0;
    let mut min = 0.0;
    // Chronological (`RunFacts::chrono`), never storage order: the logged
    // km/min are float sums the card shows rounded.
    for &i in &facts.chrono {
        if ctx.run_days.get(i).copied().flatten() == Some(judge.day) {
            km += facts.km[i];
            min += facts.minutes[i];
        }
    }
    it.run_logged_km = km;
    it.run_logged_min = min;
    let share = run_item_share(rx, km, min, judge.fold.vol_frac);
    it.run_lighter = share >= LIGHTER_CREDIT_SHARE;
    it.done = if judge.fold.short_easy_run {
        km > 0.0 || min > 0.0
    } else {
        share >= RUN_DONE_SHARE
    };
}

/// Judge a lift/calisthenics item: every same-movement set attributed to the
/// judged day, chronological, against the load and set count shown for the
/// day (`judge.fold`).
fn judge_lift_item(it: &mut PrescriptionView, judge: &DayJudge, ctx: &FlattenCtx) {
    let FlattenCtx {
        model,
        weight_lb,
        session_days,
        set_index,
        ..
    } = *ctx;
    let fold = judge.fold;
    let step = movement_step_kg(weight_lb);
    let mut day_sets: Vec<&LoggedSet> = set_index
        .same_movement_sets(&it.exercise)
        .iter()
        .filter(|&&i| session_days.get(i).copied().flatten() == Some(judge.day))
        .map(|&i| &model.sets[i])
        .collect();
    day_sets.sort_by(|a, b| a.chrono_cmp(b));
    let heaviest = day_sets
        .iter()
        .map(|srec| srec.weight_kg)
        .fold(0.0, f64::max);
    let credits = day_credits(it, &day_sets, heaviest, step, fold, weight_lb);
    let mut sets_done = 0u8;
    let mut logged = Vec::with_capacity(day_sets.len());
    for (srec, credit) in day_sets.iter().zip(credits) {
        let q = credit != SetCredit::NotCounted;
        if q {
            sets_done = sets_done.saturating_add(1);
        }
        logged.push(ItemSetView {
            qualifies: q,
            lighter: credit == SetCredit::Lighter,
            short: q && short_of_target(it, srec.reps, srec.hold_sec),
            ..ItemSetView::of(srec, weight_lb)
        });
    }
    it.sets_done = sets_done;
    it.logged_sets = logged;
    it.ended_early = ended_early(model, &it.exercise, judge.day);
    it.done = sets_done >= required_sets(it.sets, fold.vol_frac) || it.ended_early;
}

/// Each of a day's chronological same-movement sets credited ([`set_credit`]),
/// except a warm-up: a set logged before the first work set that matches a
/// not-yet-taken step of the set screen's ramp ([`warm_up_steps`]) for the
/// load shown that day, else the suggested start (`SetEditor.kt`:
/// `load_kg ?: suggested_load_kg`): within half a plate of the step's load,
/// at most its reps. Each step is taken once, so a day holds at most as many
/// warm-ups as the ramp has steps.
fn day_credits(
    it: &PrescriptionView,
    day_sets: &[&LoggedSet],
    heaviest: f64,
    step: f64,
    fold: AdjustFold,
    weight_lb: bool,
) -> Vec<SetCredit> {
    let shown_load = if fold.drops_load(it) {
        None
    } else if fold.load_frac > 0.0 {
        it.load_kg
            .and_then(|kg| round_load_or_drop(kg * (1.0 - fold.load_frac), weight_lb))
    } else {
        it.load_kg
    };
    let mut ramp = match shown_load.or(it.suggested_load_kg) {
        Some(work) if !it.timed_hold && crate::plan::is_barbell_lift(&it.exercise) => {
            warm_up_steps(work, weight_lb, &it.exercise)
        }
        _ => Vec::new(),
    };
    let mut working = false;
    day_sets
        .iter()
        .map(|s| {
            let matches = |&(kg, reps): &(f64, u32)| {
                (s.weight_kg - kg).abs() <= step / 2.0 + 1e-9 && (1..=reps).contains(&s.reps)
            };
            if !working
                && s.hold_sec.is_none()
                && let Some(i) = ramp.iter().position(matches)
            {
                ramp.remove(i);
                return SetCredit::NotCounted;
            }
            let credit = set_credit(it, s, heaviest, step, fold);
            working |= credit != SetCredit::NotCounted;
            credit
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::LoggedSet;

    fn set_at(weight_kg: f64, reps: u32, observed_at: i64) -> LoggedSet {
        LoggedSet {
            exercise: "Barbell Bench Press".to_string(),
            weight_kg,
            reps,
            rpe: 8.0,
            observed_at,
            entry_id: 0,
            hold_sec: None,
        }
    }

    #[test]
    fn no_load_item_qualifies_a_working_set_but_not_a_warmup() {
        // A RIR-prescribed "Bench press" (no load anchor) counts a working set at
        // 90 %+ of the day's heaviest same-movement weight, but a 20 kg warm-up
        // against an 80 kg top set does not.
        let it = PrescriptionView {
            exercise: "Bench press".to_string(),
            reps_low: 5,
            reps_high: 5,
            load_kg: None,
            added_kg: None,
            ..Default::default()
        };
        let heaviest = 80.0;
        let step = movement_step_kg(false);
        assert_eq!(
            set_credit(
                &it,
                &set_at(80.0, 5, 0),
                heaviest,
                step,
                AdjustFold::default()
            ),
            SetCredit::Full
        );
        assert_eq!(
            set_credit(
                &it,
                &set_at(20.0, 5, 0),
                heaviest,
                step,
                AdjustFold::default()
            ),
            SetCredit::NotCounted
        );
    }

    const LB: f64 = 0.453_592_37;

    fn squat(load_lb: f64, reps: u8, sets: u8) -> PrescriptionView {
        PrescriptionView {
            exercise: "Back Squat".to_string(),
            sets,
            reps_low: reps,
            reps_high: reps,
            load_kg: Some(load_lb * LB),
            ..Default::default()
        }
    }

    #[test]
    fn a_short_set_at_the_prescribed_load_is_a_counted_short_set() {
        // 290 lb × 4 against 290 lb × 5: a missed rep still did the set.
        let it = squat(290.0, 5, 6);
        let s = set_at(290.0 * LB, 4, 0);
        let credit = set_credit(
            &it,
            &s,
            s.weight_kg,
            movement_step_kg(true),
            AdjustFold::default(),
        );
        assert_eq!(credit, SetCredit::Full);
        assert!(
            short_of_target(&it, 4, None),
            "it fills its slot as a short set"
        );
    }

    #[test]
    fn a_set_at_ninety_percent_of_the_prescribed_load_counts_lighter() {
        // 264.6 lb × 5 against 290 lb × 3 (91 %, more reps than prescribed).
        let it = squat(290.0, 3, 5);
        let s = set_at(264.6 * LB, 5, 0);
        assert_eq!(
            set_credit(
                &it,
                &s,
                s.weight_kg,
                movement_step_kg(true),
                AdjustFold::default()
            ),
            SetCredit::Lighter
        );
    }

    /// The credits of `sets` (kg, reps), logged in that order, for `it`.
    fn credits(it: &PrescriptionView, sets: &[(f64, u32)], weight_lb: bool) -> Vec<SetCredit> {
        let logged: Vec<LoggedSet> = sets.iter().map(|&(kg, r)| set_at(kg, r, 0)).collect();
        let day: Vec<&LoggedSet> = logged.iter().collect();
        let heaviest = sets.iter().map(|s| s.0).fold(0.0, f64::max);
        let step = movement_step_kg(weight_lb);
        day_credits(it, &day, heaviest, step, AdjustFold::default(), weight_lb)
    }

    #[test]
    fn the_set_screens_warm_up_ramp_before_the_first_work_set_does_not_count() {
        use SetCredit::*;
        // The ramp for 285 lb: the bar × 5, 115 × 5, 170 × 3 and 230 × 2 (80 %
        // is 228 lb, on the 5 lb grid 230).
        let it = squat(285.0, 5, 3);
        let lb = |sets: &[(f64, u32)]| -> Vec<(f64, u32)> {
            sets.iter().map(|&(w, r)| (w * LB, r)).collect()
        };
        let ramp = [(45.0, 5), (115.0, 5), (170.0, 3), (230.0, 2)];
        let day = lb(&[ramp.as_slice(), &[(285.0, 5), (285.0, 5)]].concat());
        assert_eq!(
            credits(&it, &day, true),
            [NotCounted, NotCounted, NotCounted, NotCounted, Full, Full]
        );
        // After a work set the same double is a lighter back-off set.
        let back_off = lb(&[(285.0, 5), (230.0, 2)]);
        assert_eq!(credits(&it, &back_off, true), [Full, Lighter]);
        // Each step is taken once; more reps than the step is work.
        let twice = lb(&[(230.0, 2), (230.0, 2), (230.0, 3)]);
        assert_eq!(credits(&it, &twice, true), [NotCounted, Lighter, Lighter]);
        // Under 80 % is never work, and a set of no reps never counts.
        let short = lb(&[(200.0, 5), (285.0, 0)]);
        assert_eq!(credits(&it, &short, true), [NotCounted, NotCounted]);
    }

    #[test]
    fn a_suggested_load_ramps_like_a_prescribed_one() {
        use SetCredit::*;
        // No load anchor, a 62.5 kg suggested start: the ramp is the bar, 25 × 5,
        // 37.5 × 3 and 50 × 2. The first step is no work set although it is the
        // day's heaviest set when logged.
        let it = PrescriptionView {
            exercise: "Back Squat".to_string(),
            sets: 3,
            reps_low: 5,
            reps_high: 5,
            suggested_load_kg: Some(62.5),
            ..Default::default()
        };
        assert_eq!(credits(&it, &[(25.0, 5)], false), [NotCounted]);
        assert_eq!(
            credits(&it, &[(20.0, 5), (25.0, 5), (62.5, 5), (62.5, 5)], false),
            [NotCounted, NotCounted, Full, Full]
        );
        // A floor pull starts at 60 kg, so 25 kg × 5 is no step of its ramp.
        assert_eq!(warm_up_steps(62.5, false, "Deadlift"), [(60.0, 5)]);
        assert_eq!(warm_up_steps(20.0, false, "Back Squat"), []);
    }

    #[test]
    fn a_bodyweight_set_passes_its_own_zero_load_bar() {
        // A pure-bodyweight rung (added_kg Some(0.0)) has no prescribed load; a
        // 0 kg logged set passes 0 >= 0.9·0.
        let it = PrescriptionView {
            exercise: "Full push-up".to_string(),
            reps_low: 8,
            reps_high: 8,
            load_kg: None,
            added_kg: Some(0.0),
            ..Default::default()
        };
        let mut s = set_at(0.0, 8, 0);
        s.exercise = "Full push-up".to_string();
        assert_eq!(
            set_credit(&it, &s, 0.0, movement_step_kg(false), AdjustFold::default()),
            SetCredit::Full
        );
    }

    #[test]
    fn a_set_just_after_midnight_chains_to_the_previous_days_session() {
        // 23:50 then 00:06 (16 min later) both score as the earlier local day.
        let day = 700i64;
        let late = day * DAY_SEC + 23 * 3_600 + 50 * 60; // 23:50
        let early = (day + 1) * DAY_SEC + 6 * 60; // next local day 00:06
        let sets = vec![set_at(120.0, 3, late), set_at(120.0, 3, early)];
        let days = attribute_session_days(&sets, 0);
        assert_eq!(days[0], Some(day));
        assert_eq!(days[1], Some(day), "00:06 chains back to the 23:50 session");
    }

    #[test]
    fn a_morning_set_more_than_three_hours_later_starts_its_own_day() {
        // 00:06 with the previous set > 3 h earlier does NOT chain.
        let day = 700i64;
        let prev = day * DAY_SEC + 20 * 3_600; // 20:00
        let morning = (day + 1) * DAY_SEC + 6 * 60; // 00:06, ~4 h later
        let sets = vec![set_at(120.0, 3, prev), set_at(120.0, 3, morning)];
        let days = attribute_session_days(&sets, 0);
        assert_eq!(days[0], Some(day));
        assert_eq!(days[1], Some(day + 1), "a 4 h gap breaks the chain");
    }

    #[test]
    fn an_lb_step_up_from_225_lb_suggests_230_lb() {
        // 225 lb × 9 at RPE 8 beats a 5-rep / 3-RIR target by 3 reps: the
        // suggested start is one 5 lb plate step up, never floored back to
        // the logged 225 lb.
        let logged_kg = 225.0 / LB_PER_KG;
        let mut item = PrescriptionView {
            exercise: "Barbell Bench Press".to_string(),
            sets: 3,
            reps_low: 5,
            reps_high: 5,
            intensity: PlanIntensity::Rir { rir: 3 },
            target_rpe: Some(crate::strength::rir_to_rpe(3.0)),
            ..Default::default()
        };
        let mut model = Model::default();
        model.sets.push(set_at(logged_kg, 9, 97 * DAY_SEC));
        apply_last_sets(
            std::slice::from_mut(&mut item),
            &model,
            &SetIndex::new(&model.sets),
            &crate::plan::Anchors::default(),
            &[Some(97)],
            100,
            true,
        );
        assert_eq!(item.suggested_load_kg, Some(230.0 / LB_PER_KG));
    }
}
