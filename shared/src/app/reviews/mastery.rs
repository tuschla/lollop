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
/// profile, ≤10 effective reps, not an isolation lift (strength-006). `facts`
/// is what the set's name says ([`SetIndex::facts`]).
pub(super) fn pr_e1rm(s: &LoggedSet, facts: NameFacts, calis: bool) -> Option<f64> {
    if s.hold_sec.is_some() || s.weight_kg <= 0.0 || s.reps == 0 {
        return None;
    }
    if calis && facts.calis_tier.is_some() {
        return None;
    }
    if strength::effective_reps(s.reps, s.rpe) > strength::E1RM_RELIABLE_REP_CAP || facts.isolation
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
    set_index: &SetIndex,
    facts: &RunFacts,
    week: &[SessionPlanView],
) -> Option<Mastery> {
    let offset = model.today_utc_offset_sec;
    let cutoff = end_of_local_today_sec(model);
    let today = model.today_epoch_day;
    let dated = |t: i64| t > 0 && cutoff.is_none_or(|c| t <= c);
    let calis = model
        .profile()
        .is_some_and(|p| p.resistance_modality == ResistanceModality::Calisthenics);
    // A run's day is the local day it STARTED (`RunFacts::started`), the day
    // plan completion credits and its card names.
    let run_day =
        |i: usize| dated(model.runs[i].observed_at).then(|| local_day(facts.started[i], offset));
    let latest = model
        .sets
        .iter()
        .map(|s| s.observed_at)
        .filter(|t| dated(*t))
        .map(|t| local_day(t, offset))
        .chain((0..model.runs.len()).filter_map(run_day))
        .max();
    if let Some(day) = latest
        && today.is_none_or(|t| t.saturating_sub(day) <= FEEDBACK_FRESH_DAYS)
    {
        let in_prior = |t: i64| {
            dated(t) && {
                let d = local_day(t, offset);
                d < day && d >= day.saturating_sub(PR_WINDOW_DAYS)
            }
        };
        let on_day = |t: i64| dated(t) && local_day(t, offset) == day;
        // Positions of the latest day's sets in `model.sets`, chronological
        // (`observed_at`, then `entry_id`, the `lifts` order), so a tie between
        // two bests keeps the earlier set whatever the storage order.
        let mut day_sets: Vec<usize> = (0..model.sets.len())
            .filter(|&i| on_day(model.sets[i].observed_at))
            .collect();
        day_sets.sort_by_key(|&i| (model.sets[i].observed_at, model.sets[i].entry_id));
        let kind = e1rm_best(model, set_index, &day_sets, &in_prior, calis)
            .or_else(|| rep_best(model, set_index, &day_sets, &in_prior))
            .or_else(|| {
                let run_on = |i: usize| run_day(i) == Some(day);
                let run_prior = |i: usize| {
                    run_day(i).is_some_and(|d| d < day && d >= day.saturating_sub(PR_WINDOW_DAYS))
                };
                longest_run(model, facts, &run_on, &run_prior)
            });
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
    set_index: &SetIndex,
    day_sets: &[usize],
    in_prior: &impl Fn(i64) -> bool,
    calis: bool,
) -> Option<MasteryKind> {
    let mut best: Option<(f64, MasteryKind)> = None;
    for &i in day_sets {
        let s = &model.sets[i];
        let facts = set_index.facts(i);
        let Some(e1) = pr_e1rm(s, facts, calis) else {
            continue;
        };
        let prior = set_index
            .movement_sets(facts.movement)
            .iter()
            .filter(|&&p| in_prior(model.sets[p].observed_at))
            .filter_map(|&p| pr_e1rm(&model.sets[p], set_index.facts(p), calis))
            .fold(0.0_f64, f64::max);
        // A prior under MIN_RATIO_BASE_KG is no prior best (a subnormal
        // corrupt-log weight would divide to "+inf%").
        if prior >= MIN_RATIO_BASE_KG && e1 > prior * (1.0 + E1RM_NOISE_FRAC) {
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
    set_index: &SetIndex,
    day_sets: &[usize],
    in_prior: &impl Fn(i64) -> bool,
) -> Option<MasteryKind> {
    let mut best: Option<(u32, MasteryKind)> = None;
    for &i in day_sets {
        let s = &model.sets[i];
        if s.hold_sec.is_some() || s.weight_kg <= 0.0 || s.reps == 0 {
            continue;
        }
        let prior_reps = set_index
            .movement_sets(set_index.facts(i).movement)
            .iter()
            .map(|&p| &model.sets[p])
            .filter(|p| {
                in_prior(p.observed_at) && p.hold_sec.is_none() && p.weight_kg >= s.weight_kg - 0.01
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

/// The day's run when it is the longest in 90 days, never a spike. `on_day`
/// and `in_prior` take a run index (a run is dated by its start).
fn longest_run(
    model: &Model,
    facts: &RunFacts,
    on_day: &impl Fn(usize) -> bool,
    in_prior: &impl Fn(usize) -> bool,
) -> Option<MasteryKind> {
    // Longest; a tie keeps the EARLIER run (`observed_at`, then `entry_id`),
    // independent of storage order (an Undo or amend moves a row to the tail).
    let (idx, r) = model
        .runs
        .iter()
        .enumerate()
        .filter(|(i, _)| on_day(*i))
        .max_by(|(a, ra), (b, rb)| {
            facts.km[*a]
                .total_cmp(&facts.km[*b])
                .then((rb.observed_at, rb.entry_id).cmp(&(ra.observed_at, ra.entry_id)))
        })?;
    let km = facts.km[idx];
    let prior_km = (0..model.runs.len())
        .filter(|&i| in_prior(i))
        .map(|i| facts.km[i])
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
