//! Session execution: each completed plan day judged from the logged sets and
//! runs against what was shown that day, the "session done" summary, and the
//! single execution verdict the feedback card leads with.

use super::*;
use crate::feedback;
use std::fmt::Write as _;

/// One prescribed exercise's outcome on a completed plan day, judged from the
/// logged sets against the prescription as shown that day.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::app) struct LiftOutcome {
    pub(in crate::app) exercise: String,
    /// Heaviest qualifying set's weight (else the prescribed load), kg.
    pub(in crate::app) load_kg: Option<f64>,
    /// Reps of that heaviest set.
    pub(in crate::app) top_reps: u32,
    pub(in crate::app) sets_done: u8,
    pub(in crate::app) sets_planned: u8,
    pub(in crate::app) reps_target: u8,
    /// Every planned set reached the target reps (an item ended early with
    /// fewer qualifying sets did not).
    pub(in crate::app) reps_met: bool,
    /// Fewest reps in reserve over the qualifying sets, from the logged RPE
    /// (`None` when no set was rated: never invented).
    pub(in crate::app) rir_actual: Option<u8>,
    /// Reserve the prescription planned (RIR target, or the reserve implied by
    /// the %1RM × reps via the Epley inverse). `None` when not judgeable.
    pub(in crate::app) rir_target: Option<u8>,
    /// The load the plan showed for the item that day, kg.
    pub(in crate::app) planned_kg: Option<f64>,
    /// The plan day it was judged on (set for a derived day).
    pub(in crate::app) day: Option<i64>,
}

/// The run work on a completed plan day, with the volume shown for it that day
/// (distance or time, whichever the plan stated).
#[derive(Debug, Clone, PartialEq)]
pub(in crate::app) struct RunOutcome {
    pub(in crate::app) km: f64,
    pub(in crate::app) min: f64,
    pub(in crate::app) planned_km: Option<f64>,
    pub(in crate::app) planned_min: Option<f64>,
}

/// The latest completed plan day's execution, derived from logs.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::app) struct DerivedExec {
    pub(in crate::app) epoch_day: i64,
    pub(in crate::app) lifts: Vec<LiftOutcome>,
    pub(in crate::app) run: Option<RunOutcome>,
}

/// The reserve a prescription planned, as its face states it: a RIR target
/// structurally; any other target by its `target_rpe` (a %e1RM item's effort
/// after the ≥ 1 RIR floor and any cut or cap of the day). A %e1RM item
/// without one reads its reserve off the Epley inverse (reps achievable at p%
/// minus the prescribed reps, strength-005); beyond 4 reps in reserve a rating
/// is too far from failure to judge (feedback-020/022: RIR is trusted within
/// ~1–3 reps of failure).
pub(in crate::app) fn target_rir(it: &PrescriptionView) -> Option<u8> {
    match it.intensity {
        PlanIntensity::Rir { rir } => Some(rir.clamp(0, 10) as u8),
        PlanIntensity::RirRange { lo, .. } => Some(lo.clamp(0, 10) as u8),
        PlanIntensity::PercentE1rm { pct, .. }
            if it.target_rpe.is_none() && pct > 0 && pct < 100 && it.reps_low > 0 =>
        {
            let rir = strength::expected_rir_at_pct(f64::from(pct), u32::from(it.reps_low)).round();
            (0.0..=4.0).contains(&rir).then_some(rir as u8)
        }
        _ => it
            .target_rpe
            .map(|r| strength::rpe_to_rir(r).round().clamp(0.0, 10.0) as u8),
    }
}

/// The item's top qualifying logged set: heaviest, more reps breaking a load
/// tie, the later set on a full tie.
fn top_set(it: &PrescriptionView) -> Option<&ItemSetView> {
    it.logged_sets
        .iter()
        .filter(|s| s.qualifies)
        .max_by(|a, b| {
            a.weight_kg
                .total_cmp(&b.weight_kg)
                .then(a.reps.cmp(&b.reps))
        })
}

fn lift_outcome(it: &PrescriptionView) -> LiftOutcome {
    let qualifying: Vec<&ItemSetView> = it.logged_sets.iter().filter(|s| s.qualifies).collect();
    let top = top_set(it);
    let rir_actual = qualifying
        .iter()
        .filter(|s| s.rpe > 0.0)
        .map(|s| strength::rpe_to_rir(s.rpe).round().clamp(0.0, 10.0) as u8)
        .min();
    let sets_planned = it.sets.max(1);
    LiftOutcome {
        exercise: it.exercise.clone(),
        load_kg: top.map(|s| s.weight_kg).filter(|w| *w > 0.0).or(it.load_kg),
        top_reps: top.map(|s| s.reps).unwrap_or(0),
        sets_done: it.sets_done,
        sets_planned,
        reps_target: it.reps_low,
        reps_met: it.sets_done >= sets_planned,
        rir_actual,
        rir_target: target_rir(it),
        planned_kg: it.load_kg,
        day: None,
    }
}

/// The latest completed (status `"done"`) plan day on or before today, judged
/// item by item against what was shown that day: reps met, reserve vs target,
/// run volume. `None` when no plan day is complete this week.
pub(in crate::app) fn derive_session_exec(
    week: &[SessionPlanView],
    today: i64,
) -> Option<DerivedExec> {
    let day = week
        .iter()
        .filter(|s| s.status.is_done() && s.epoch_day <= today && !s.items.is_empty())
        .max_by_key(|s| s.epoch_day)?;
    // A deliberately skipped exercise is not a missed target: it never reads
    // as "came up short".
    let lifts: Vec<LiftOutcome> = day
        .items
        .iter()
        .filter(|it| {
            !it.is_run()
                && !matches!(
                    it.status,
                    ItemStatus::Skipped | ItemStatus::SkippedPain | ItemStatus::StoppedPain
                )
        })
        .map(|it| LiftOutcome {
            day: Some(day.epoch_day),
            ..lift_outcome(it)
        })
        .collect();
    let mut run = RunOutcome {
        km: 0.0,
        min: 0.0,
        planned_km: None,
        planned_min: None,
    };
    for it in day.items.iter().filter(|it| it.is_run()) {
        run.km += it.run_logged_km;
        run.min += it.run_logged_min;
        let (km, min) = crate::app::plan_views::run_planned(it);
        let add = |acc: Option<f64>, v: Option<f64>| match (acc, v) {
            (Some(a), Some(b)) => Some(a + b),
            (a, b) => a.or(b),
        };
        run.planned_km = add(run.planned_km, km);
        run.planned_min = add(run.planned_min, min);
    }
    let run = (run.km > 0.0 || run.min > 0.0).then_some(run);
    Some(DerivedExec {
        epoch_day: day.epoch_day,
        lifts,
        run,
    })
}

/// Today's completed session for the Today screen: one line per item, from the
/// logged work. `None` unless today's plan day is complete.
pub(in crate::app) fn build_session_done(
    week: &[SessionPlanView],
    today: Option<i64>,
) -> Option<SessionDoneView> {
    let today = today?;
    let day = week
        .iter()
        .find(|s| s.epoch_day == today && s.status.is_done() && !s.items.is_empty())?;
    let items = day
        .items
        .iter()
        .map(|it| {
            if it.is_run() {
                let (planned_km, planned_min) = crate::app::plan_views::run_planned(it);
                SessionDoneItemView {
                    exercise: "Run".to_string(),
                    summary: format!("{:.1} km · {:.0} min", it.run_logged_km, it.run_logged_min),
                    met: it.status == ItemStatus::Done,
                    status: it.status,
                    run_km: Some(it.run_logged_km),
                    run_min: Some(it.run_logged_min),
                    run_planned_km: planned_km,
                    run_planned_min: planned_min,
                }
            } else {
                let o = lift_outcome(it);
                let mut summary = format!("{} of {} sets", o.sets_done, o.sets_planned);
                if let Some(best) = top_set(it) {
                    let _ = write!(summary, " · best {}", best.summary);
                }
                match it.status {
                    ItemStatus::StoppedPain => summary.push_str(" · stopped (pain)"),
                    _ if it.ended_early && !o.reps_met => summary.push_str(" · ended early"),
                    _ => {}
                }
                SessionDoneItemView {
                    exercise: it.exercise.clone(),
                    summary,
                    met: it.status == ItemStatus::Done,
                    status: it.status,
                    ..SessionDoneItemView::default()
                }
            }
        })
        .collect();
    Some(SessionDoneView {
        epoch_day: day.epoch_day,
        title: day.title.clone(),
        items,
    })
}

/// The completed-run line: "as planned" only when the run reached the volume
/// shown for the day (at the precision shown), else "8.1 km of 9.5 km"; `mi`
/// states the distances in miles.
pub(super) fn run_done_message(r: &RunOutcome, mi: bool) -> String {
    let tenths = |x: f64| (x * 10.0).round() / 10.0;
    let d = |km: f64| fmt_km_or_mi(km, mi);
    match (r.planned_km, r.planned_min) {
        (Some(p), _) if r.km > 0.0 && tenths(r.km) < tenths(p) => {
            format!("Run done: {} of {} in {:.0} min.", d(r.km), d(p), r.min)
        }
        (_, Some(p)) if r.min.round() < p.round() => {
            if r.km > 0.0 {
                format!("Run done: {} in {:.0} of {:.0} min.", d(r.km), r.min, p)
            } else {
                format!("Run done: {:.0} of {:.0} min.", r.min, p)
            }
        }
        _ if r.km > 0.0 => format!("Run done: {} in {:.0} min, as planned.", d(r.km), r.min),
        _ => format!("Run done: {:.0} min, as planned.", r.min),
    }
}

/// Which arm produced the execution verdict, so the copy names the right thing
/// with its numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum Trigger {
    Missed,
    RepsMetAtRir0,
    HarderThanPlanned,
    Nudge,
    LiftMastery,
    LiftCompleted,
    IntervalMastery,
    DecouplingHigh,
    MildDrift,
    Durability,
    EasyHr,
    PositiveSplit,
    Pacing,
    RunCompleted,
    BadDay,
    Mastery,
}

/// One lift's evaluation input: reps met + reserve actual/target, plus the
/// derived outcome for copy when it came from the logs.
#[derive(Debug, Clone)]
pub(in crate::app) struct LiftEval {
    pub(super) reps_met: bool,
    pub(super) rir_actual: Option<u8>,
    pub(super) rir_target: Option<u8>,
    pub(super) detail: Option<LiftOutcome>,
}

/// Everything the execution arms read for one session.
#[derive(Debug, Clone, Default)]
pub(in crate::app) struct ExecInput {
    pub(super) lift: Option<LiftEval>,
    pub(super) interval: Option<IntervalExec>,
    pub(super) decoupling: Option<Decouple>,
    pub(super) easy_frac: Option<f64>,
    pub(super) split_pct: Option<f64>,
    pub(super) bad_day: bool,
    pub(super) run: Option<RunOutcome>,
}

/// A resolved execution verdict with its copy context.
#[derive(Debug, Clone)]
pub(in crate::app) struct Exec {
    pub(super) rec: Recommended<FeedbackCategory>,
    pub(super) trigger: Trigger,
    pub(super) lift: Option<LiftEval>,
}

/// Severity of one lift's evaluation, for picking the single takeaway on a
/// multi-lift day: a miss outranks a costly set, which outranks a harder-than-
/// planned note, a nudge, mastery, then plain completion.
pub(super) fn lift_severity(l: &LiftEval) -> u8 {
    if !l.reps_met {
        return 5;
    }
    match (l.rir_actual, l.rir_target) {
        (Some(a), Some(t)) => match feedback::lifting_feedback(true, a, t) {
            (_, Some(feedback::CorrectiveTrigger::RepsMetAtRir0)) => 4,
            (r, _) if r.value == FeedbackCategory::InformationalNeutral => 3,
            (r, _) if r.value == FeedbackCategory::ProgressionNudge => 2,
            _ => 1,
        },
        _ => 0,
    }
}

pub(super) fn eval_of(o: &LiftOutcome) -> LiftEval {
    LiftEval {
        reps_met: o.reps_met,
        rir_actual: o.rir_actual,
        rir_target: o.rir_target,
        detail: Some(o.clone()),
    }
}

/// Pick the single execution verdict, in a fixed priority order (lifting →
/// interval → decoupling → easy-intensity → pacing → run completed → off day).
/// A flagged off day turns any corrective, discipline or neutral verdict into
/// the off-day note (feedback-018/025); praise still wins. `None` when no arm
/// fires.
pub(in crate::app) fn session_execution(inp: &ExecInput) -> Option<Exec> {
    let exec = execution_arm(inp)?;
    let positive = matches!(
        exec.rec.value,
        FeedbackCategory::PositiveMastery | FeedbackCategory::PositiveExecution
    );
    if inp.bad_day && !positive {
        return Some(Exec {
            rec: feedback::bad_day_feedback(),
            trigger: Trigger::BadDay,
            lift: exec.lift,
        });
    }
    Some(exec)
}

fn execution_arm(inp: &ExecInput) -> Option<Exec> {
    if let Some(l) = &inp.lift {
        let (rec, trigger) = match (l.reps_met, l.rir_actual, l.rir_target) {
            (false, ..) => {
                let (r, _) = feedback::lifting_feedback(false, 0, 0);
                (r, Trigger::Missed)
            }
            (true, Some(a), Some(t)) => {
                let (r, ct) = feedback::lifting_feedback(true, a, t);
                let trig = match (r.value, ct) {
                    (_, Some(feedback::CorrectiveTrigger::RepsMetAtRir0)) => Trigger::RepsMetAtRir0,
                    (FeedbackCategory::ProgressionNudge, _) => Trigger::Nudge,
                    (FeedbackCategory::PositiveMastery, _) => Trigger::LiftMastery,
                    _ => Trigger::HarderThanPlanned,
                };
                (r, trig)
            }
            // Every planned set at target reps but no effort rating: a
            // completed session is itself a mastery experience (feedback-005),
            // praised with its numbers, never with an invented reserve.
            (true, ..) => (
                graded(
                    FeedbackCategory::PositiveMastery,
                    claims::FB_MASTERY_ANCHOR_001,
                ),
                Trigger::LiftCompleted,
            ),
        };
        return Some(Exec {
            rec,
            trigger,
            lift: Some(l.clone()),
        });
    }
    let plain = |rec, trigger| {
        Some(Exec {
            rec,
            trigger,
            lift: None,
        })
    };
    // feedback-015: interval/threshold mastery.
    if let Some(i) = &inp.interval
        && let Some(f) =
            feedback::interval_mastery_feedback(i.target_paces_met, i.rpe_at_or_below_target)
    {
        return plain(f, Trigger::IntervalMastery);
    }
    if let Some(d) = &inp.decoupling
        && let Some((f, ct)) = feedback::decoupling_feedback(d.drift_pct, d.cool_steady_context)
    {
        let trig = match (f.value, ct) {
            (_, Some(_)) => Trigger::DecouplingHigh,
            (FeedbackCategory::PositiveExecution, _) => Trigger::Durability,
            _ => Trigger::MildDrift,
        };
        return plain(f, trig);
    }
    if let Some(frac) = inp.easy_frac
        && let Some(f) = feedback::easy_run_intensity_discipline(frac)
    {
        return plain(f, Trigger::EasyHr);
    }
    if let Some(ps) = inp.split_pct {
        let f = feedback::positive_split_discipline(ps);
        let trig = if f.value == FeedbackCategory::IntensityDiscipline {
            Trigger::PositiveSplit
        } else {
            Trigger::Pacing
        };
        return plain(f, trig);
    }
    if inp.run.is_some() {
        return plain(
            graded(
                FeedbackCategory::PositiveMastery,
                claims::FB_MASTERY_ANCHOR_001,
            ),
            Trigger::RunCompleted,
        );
    }
    if inp.bad_day {
        return plain(feedback::bad_day_feedback(), Trigger::BadDay);
    }
    None
}
