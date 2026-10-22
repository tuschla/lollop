//! Mastery: the personal best or completed week worth naming on the card.

use super::*;

/// Look-back window for personal bests: a best counts against the prior 90
/// days of logged history.
const PR_WINDOW_DAYS: i64 = 90;

/// A concrete mastery experience worth naming (feedback-005; ADH-PRAISE-001).
#[derive(Debug, Clone, PartialEq)]
pub(in crate::app) enum MasteryKind {
    /// Estimated max beyond the ±5 % noise band over the prior 90-day best.
    E1rmPr {
        exercise: String,
        e1rm: f64,
        prior: f64,
        weight: f64,
        reps: u32,
        rpe: f64,
    },
    /// More reps at a load than at that load or heavier in the prior 90 days.
    RepPr {
        exercise: String,
        weight: f64,
        reps: u32,
        prior_reps: u32,
    },
    /// Longest run in 90 days, only within the 10 % single-run spike rule.
    LongestRun { km: f64, prior_km: f64 },
    /// Every planned training session of this week done.
    WeekComplete { sessions: u8 },
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::app) struct Mastery {
    pub(in crate::app) day: i64,
    pub(in crate::app) kind: MasteryKind,
}

/// The e1RM a set supports when it can estimate one reliably — the same gate
/// the plan anchors use: not a hold, not a calisthenics rung on a calisthenics
/// profile, ≤10 effective reps, not an isolation lift (strength-006).
pub(super) fn pr_e1rm(s: &LoggedSet, calis: bool) -> Option<f64> {
    if s.hold_sec.is_some() || s.weight_kg <= 0.0 || s.reps == 0 {
        return None;
    }
    if calis && crate::calisthenics::tier_by_name(&s.exercise).is_some() {
        return None;
    }
    if strength::effective_reps(s.reps, s.rpe) > strength::E1RM_RELIABLE_REP_CAP
        || crate::plan::is_isolation_lift(&s.exercise)
    {
        return None;
    }
    let e1 = strength::e1rm_epley_rpe(s.weight_kg, s.reps, s.rpe);
    (e1 > 0.0 && e1.is_finite()).then_some(e1)
}

/// Detect the strongest mastery experience on the latest logged day (within a
/// week of today), else a fully completed planned week. Priority: e1RM best >
/// rep best > longest run > completed week. A run that tripped the 10 % spike
/// rule is never praised (feedback-037).
pub(in crate::app) fn detect_mastery(
    model: &Model,
    facts: &RunFacts,
    week: &[SessionPlanView],
) -> Option<Mastery> {
    let offset = model.today_utc_offset_sec;
    let cutoff = end_of_local_today_sec(model);
    let today = model.today_epoch_day;
    let dated = |t: i64| t > 0 && cutoff.is_none_or(|c| t <= c);
    let calis = model
        .profile
        .as_ref()
        .is_some_and(|p| p.resistance_modality == ResistanceModality::Calisthenics);
    let latest = model
        .sets
        .iter()
        .map(|s| s.observed_at)
        .chain(model.runs.iter().map(|r| r.observed_at))
        .filter(|t| dated(*t))
        .map(|t| local_day(t, offset))
        .max();
    if let Some(day) = latest
        && today.is_none_or(|t| t.saturating_sub(day) <= FEEDBACK_FRESH_DAYS)
    {
        let in_prior = |t: i64| {
            dated(t) && {
                let d = local_day(t, offset);
                d < day && d >= day - PR_WINDOW_DAYS
            }
        };
        let on_day = |t: i64| dated(t) && local_day(t, offset) == day;
        let day_sets: Vec<&LoggedSet> = model
            .sets
            .iter()
            .filter(|s| on_day(s.observed_at))
            .collect();
        let kind = e1rm_best(model, &day_sets, &in_prior, calis)
            .or_else(|| rep_best(model, &day_sets, &in_prior))
            .or_else(|| longest_run(model, facts, &on_day, &in_prior));
        if let Some(kind) = kind {
            return Some(Mastery { day, kind });
        }
    }
    week_complete(week)
}

/// The day's biggest e1RM best: per movement, the day's best against the prior
/// 90 days, beyond the noise band.
fn e1rm_best(
    model: &Model,
    day_sets: &[&LoggedSet],
    in_prior: &impl Fn(i64) -> bool,
    calis: bool,
) -> Option<MasteryKind> {
    let mut best: Option<(f64, MasteryKind)> = None;
    for s in day_sets {
        let Some(e1) = pr_e1rm(s, calis) else {
            continue;
        };
        let prior = model
            .sets
            .iter()
            .filter(|p| {
                in_prior(p.observed_at) && crate::plan::same_movement(&p.exercise, &s.exercise)
            })
            .filter_map(|p| pr_e1rm(p, calis))
            .fold(0.0_f64, f64::max);
        if prior > 0.0 && e1 > prior * (1.0 + E1RM_NOISE_FRAC) {
            let gain = e1 / prior - 1.0;
            if best.as_ref().is_none_or(|(g, _)| gain > *g) {
                best = Some((
                    gain,
                    MasteryKind::E1rmPr {
                        exercise: s.exercise.clone(),
                        e1rm: e1,
                        prior,
                        weight: s.weight_kg,
                        reps: s.reps,
                        rpe: s.rpe,
                    },
                ));
            }
        }
    }
    best.map(|(_, kind)| kind)
}

/// The day's biggest rep best at a load against the prior 90 days.
fn rep_best(
    model: &Model,
    day_sets: &[&LoggedSet],
    in_prior: &impl Fn(i64) -> bool,
) -> Option<MasteryKind> {
    let mut best: Option<(u32, MasteryKind)> = None;
    for s in day_sets {
        if s.hold_sec.is_some() || s.weight_kg <= 0.0 || s.reps == 0 {
            continue;
        }
        let prior_reps = model
            .sets
            .iter()
            .filter(|p| {
                in_prior(p.observed_at)
                    && p.hold_sec.is_none()
                    && p.weight_kg >= s.weight_kg - 0.01
                    && crate::plan::same_movement(&p.exercise, &s.exercise)
            })
            .map(|p| p.reps)
            .max();
        if let Some(prior_reps) = prior_reps
            && s.reps > prior_reps
        {
            let gain = s.reps - prior_reps;
            if best.as_ref().is_none_or(|(g, _)| gain > *g) {
                best = Some((
                    gain,
                    MasteryKind::RepPr {
                        exercise: s.exercise.clone(),
                        weight: s.weight_kg,
                        reps: s.reps,
                        prior_reps,
                    },
                ));
            }
        }
    }
    best.map(|(_, kind)| kind)
}

/// The day's run when it is the longest in 90 days, never a spike.
fn longest_run(
    model: &Model,
    facts: &RunFacts,
    on_day: &impl Fn(i64) -> bool,
    in_prior: &impl Fn(i64) -> bool,
) -> Option<MasteryKind> {
    let (idx, r) = model
        .runs
        .iter()
        .enumerate()
        .filter(|(_, r)| on_day(r.observed_at))
        .max_by(|(a, _), (b, _)| facts.km[*a].total_cmp(&facts.km[*b]))?;
    let km = facts.km[idx];
    let prior_km = model
        .runs
        .iter()
        .zip(&facts.km)
        .filter(|(o, _)| in_prior(o.observed_at))
        .map(|(_, &k)| k)
        .fold(0.0_f64, f64::max);
    // The 30-day spike baseline excludes the run itself.
    let baseline = spike_baseline_km(&model.runs, &facts.km, r.observed_at, Some(idx));
    let within_spike_rule = baseline > 0.0 && km <= baseline * (1.0 + running::SPIKE_FRAC);
    (prior_km > 0.0 && km > prior_km && within_spike_rule)
        .then_some(MasteryKind::LongestRun { km, prior_km })
}

/// A fully completed planned week (every training day done).
fn week_complete(week: &[SessionPlanView]) -> Option<Mastery> {
    let training: Vec<&SessionPlanView> = week
        .iter()
        .filter(|s| !s.items.is_empty() && s.status != SessionStatus::Rest)
        .collect();
    if training.len() >= 2 && training.iter().all(|s| s.status == SessionStatus::Done) {
        let day = training.iter().map(|s| s.epoch_day).max().unwrap_or(0);
        return Some(Mastery {
            day,
            kind: MasteryKind::WeekComplete {
                sessions: training.len().min(u8::MAX as usize) as u8,
            },
        });
    }
    None
}
