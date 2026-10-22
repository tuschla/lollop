//! Plan anchors derived from the logged history: per-lift e1RM loading
//! anchors, last working sets, and the run-history anchors.

use super::*;

/// A declared layoff (profile `weeks_off`) and the local day training resumed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Layoff {
    pub(super) weeks_off: f64,
    /// Local epoch day training resumed (`layoff_returned_epoch_day`, else the
    /// day the profile edit took effect). `None` = undatable: every anchor is
    /// treated as pre-layoff and the ramp never advances.
    pub(super) returned_epoch_day: Option<i64>,
}

impl Layoff {
    /// The layoff a profile declares, if any (`weeks_off > 0`).
    pub(super) fn of(p: &Profile) -> Option<Self> {
        p.weeks_off.filter(|w| *w > 0.0).map(|weeks_off| Layoff {
            weeks_off,
            returned_epoch_day: p.layoff_returned_epoch_day.or(p.effective_from_epoch_day),
        })
    }
}

/// Everything besides the sets and the reference day that shapes the anchors.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct AnchorCtx {
    pub(super) layoff: Option<Layoff>,
    /// Calisthenics profile: ladder-rung sets never anchor a %1RM load.
    pub(super) calisthenics: bool,
    /// A session day the athlete marked as an off day (session review
    /// `bad_day`): a low e1RM that day does not cut the next load.
    pub(super) off_day: Option<i64>,
}

impl AnchorCtx {
    /// The context for `profile` (the one effective on the day being
    /// prescribed), with the off day taken from the current session review.
    pub(super) fn for_profile(profile: Option<&Profile>, model: &Model, offset: i64) -> Self {
        AnchorCtx {
            layoff: profile.and_then(Layoff::of),
            calisthenics: profile
                .is_some_and(|p| p.resistance_modality == ResistanceModality::Calisthenics),
            off_day: off_day(model, offset),
        }
    }
}

/// The local day of the current session review when it marks an off day.
pub(super) fn off_day(model: &Model, offset: i64) -> Option<i64> {
    model
        .review
        .as_ref()
        .filter(|r| r.bad_day && !is_undated(r.observed_at))
        .map(|r| local_day(r.observed_at, offset))
}

/// The e1RM anchors per lift the plan synthesizes against
/// (`build_plan_anchors`), taken as-of today, so an audit judges a logged load
/// against the athlete's demonstrated 1RM rather than a rep-count guess.
pub(super) fn lift_anchors_now(model: &Model) -> crate::plan::Anchors {
    let offset = model.today_utc_offset_sec;
    let today = model.today_epoch_day.unwrap_or_else(|| {
        model
            .sets
            .iter()
            .filter(|s| s.observed_at > 0)
            .map(|s| local_day(s.observed_at, offset))
            .max()
            .unwrap_or(0)
    });
    let ctx = AnchorCtx::for_profile(model.profile.as_ref(), model, offset);
    build_plan_anchors(model.sets.iter(), ctx, today, offset)
}

/// Sessions of a movement kept in the anchor window as of a day: the most
/// recent `WINDOW_SESSIONS` sessions not older than the detraining horizon,
/// plus any within `WINDOW_DAYS` (STRENGTH-8). The best estimate inside the
/// window is the anchor, so one old optimistic set cannot pin every later load
/// (true 1RM varies day to day, strength-019; formula SD ±5 %, strength-005).
const WINDOW_SESSIONS: usize = 3;
pub(super) const WINDOW_DAYS: i64 = 21;
/// A session older than this (the 8-week re-entry bracket edge) never counts
/// toward the window, however few sessions followed it.
const WINDOW_HORIZON_DAYS: i64 = 56;
/// An unrated set below this fraction of the current anchor is speed work: it
/// neither creates nor raises an anchor (STRENGTH-3).
const SPEED_SET_FRAC: f64 = 0.70;
/// AUTOREG-E1RM-GATE-001: a session e1RM more than 5 % under baseline cuts the
/// next working load by about 5 %.
pub(super) const E1RM_DROP_GATE: f64 = 0.95;

/// One anchor-eligible set of a movement.
struct Obs {
    /// Local session day; `None` for an undated legacy set (oldest).
    day: Option<i64>,
    observed_at: i64,
    weight: f64,
    /// RPE-extended e1RM for a rated set; the reps-as-performed Epley LOWER
    /// BOUND for an unrated one.
    e1: f64,
    rated: bool,
}

/// The windowed anchor of a run of sessions plus the latest session's
/// e1RM-drop verdict.
#[derive(Clone, Copy)]
struct WindowResult {
    anchor: f64,
    /// The latest session's rated e1RM fell ≥5 % under the baseline before it
    /// (and that day was not marked an off day).
    dropped: bool,
    /// Whole weeks since the anchor last rose, counted only once ≥2 RATED
    /// sessions have failed to beat it (an unrated session is a lower bound,
    /// so it can neither prove progress nor a stall). STR-STALL-001 input.
    weeks_stalled: u8,
}

fn day_key(d: Option<i64>) -> i64 {
    d.unwrap_or(i64::MIN)
}

/// Best value inside the window as of day `as_of` (sessions in `list` are all
/// on or before it, oldest first).
fn window_best(list: &[(i64, f64)], as_of: i64) -> Option<f64> {
    list.iter()
        .rev()
        .enumerate()
        .filter(|(k, (d, _))| {
            let age = as_of.saturating_sub(*d);
            age <= WINDOW_DAYS || (*k < WINDOW_SESSIONS && age <= WINDOW_HORIZON_DAYS)
        })
        .map(|(_, (_, v))| *v)
        .reduce(f64::max)
}

/// An undeclared break inside one movement's own history, treated as a layoff
/// (REENTRY-001 / REENTRY-RAMP-001) while its re-entry ramp runs. `days` are
/// the movement's dated training days, sorted and unique. The latest gap
/// between consecutive training days that the detraining credit marks (more
/// than 30 days, DETRAIN-STR-001) gives `weeks_off`, and the first day after
/// it is the return day: the athlete then ramps back from 70 % over the
/// bracket's weeks instead of jumping straight back to full load the day after
/// one light session. A gap past 8 weeks returns `None`: the anchor window's
/// horizon already sets those pre-gap sessions aside (no load fraction).
fn implicit_layoff(days: &[i64], today: i64) -> Option<(Layoff, individualization::ReentryRamp)> {
    let gap = |w: &[i64]| w[1] - w[0];
    let w = days
        .windows(2)
        .rev()
        .find(|&w| strength::lift_detrain_credit(gap(w)).value != Some(1.0))?;
    strength::lift_detrain_credit(gap(w)).value?;
    let layoff = Layoff {
        weeks_off: gap(w) as f64 / 7.0,
        returned_epoch_day: Some(w[1]),
    };
    let ramp = individualization::reentry_ramp(layoff.weeks_off, today.saturating_sub(w[1])).value;
    (ramp.active && !ramp.treat_as_novice).then_some((layoff, ramp))
}

/// Score a movement's sessions (sorted by day) into a windowed anchor.
///
/// A RATED set scores its RPE-extended e1RM. An UNRATED set is a lower bound,
/// never a failure claim (STRENGTH-1): it raises the anchor only when it beats
/// it (more reps than the anchor allows), otherwise it re-confirms the anchor
/// in force, and below 70 % of that anchor it is speed work that carries no
/// information at all (STRENGTH-3). So an unrated, compliant lifter's anchor
/// holds while rated sets steer it.
///
/// `eff_day` maps a session's day onto the day its age is counted from (a
/// declared layoff dates pre-layoff sessions at the return day, so the break
/// itself never ages them — the re-entry ramp replaces that decay,
/// REENTRY-RAMP-001). `work_frac` is the working fraction in force on a day
/// (the ramp), against which an unrated set is judged as speed work or not.
fn score_sessions(
    obs: &[&Obs],
    off_day: Option<i64>,
    eff_day: &dyn Fn(i64) -> i64,
    work_frac: &dyn Fn(i64) -> f64,
) -> Option<WindowResult> {
    let mut list: Vec<(i64, f64)> = Vec::new();
    // (real day, rated best, working baseline before it)
    let mut last: Option<(i64, f64, Option<f64>)> = None;
    // (day the anchor last rose, rated sessions since that failed to beat it)
    let mut rise: (i64, u32) = (i64::MIN, 0);
    let mut i = 0;
    while i < obs.len() {
        let day = day_key(obs[i].day);
        let aged = if day == i64::MIN { day } else { eff_day(day) };
        let frac = if day == i64::MIN { 1.0 } else { work_frac(day) };
        // The anchor in force when this session started.
        let running = window_best(&list, aged);
        let mut j = i;
        let mut value: Option<f64> = None;
        let mut rated_best: Option<f64> = None;
        while j < obs.len() && day_key(obs[j].day) == day {
            let o = obs[j];
            let v = if o.rated {
                rated_best = Some(rated_best.map_or(o.e1, |r: f64| r.max(o.e1)));
                Some(o.e1)
            } else {
                match running {
                    Some(a) if o.weight < SPEED_SET_FRAC * a * frac => None,
                    Some(a) => Some(o.e1.max(a)),
                    None => Some(o.e1),
                }
            };
            if let Some(v) = v {
                value = Some(value.map_or(v, |x: f64| x.max(v)));
            }
            j += 1;
        }
        if let Some(v) = value {
            list.push((aged, v));
        }
        match (running, rated_best) {
            (None, _) => rise = (aged, 0),
            (Some(a), _) if value.is_some_and(|v| v > a) => rise = (aged, 0),
            (Some(_), Some(_)) => rise.1 += 1,
            (Some(_), None) => {}
        }
        last = Some((
            day,
            rated_best.unwrap_or(f64::NAN),
            running.map(|a| a * frac),
        ));
        i = j;
    }
    let &(latest, _) = list.last()?;
    let anchor = window_best(&list, latest)?;
    let dropped = match last {
        Some((day, r, Some(base))) => {
            day != i64::MIN && r.is_finite() && r < E1RM_DROP_GATE * base && off_day != Some(day)
        }
        _ => false,
    };
    let weeks_stalled = if rise.1 >= 2 && rise.0 != i64::MIN {
        (latest.saturating_sub(rise.0) / 7).clamp(0, u8::MAX as i64) as u8
    } else {
        0
    };
    Some(WindowResult {
        anchor,
        dropped,
        weeks_stalled,
    })
}

/// Logged e1RM anchors per MOVEMENT (0.1-rounded), most-recent-first, plus each
/// lift's working-load derate. Deterministic despite the HashMaps: the output
/// is sorted by (latest observed_at desc, name asc).
///
/// - Sets are bucketed by `movement_key`, so "Back squat" and "Barbell back
///   squat" build one anchor (display name = the most recent spelling).
/// - Only reliable estimates anchor: no holds, no calisthenics rungs on a
///   Calisthenics profile, no isolation lifts, ≤10 effective reps, a positive
///   e1RM (strength-006; never "@ 0 kg").
/// - The anchor is the best estimate inside the movement's recent window, with
///   unrated and open-ended ("4+ reps left") sets as lower bounds
///   ([`score_sessions`]).
/// - ONE derate per movement, never stacked: a pre-break anchor on a running
///   re-entry ramp (a declared layoff, or an undeclared 31–56-day gap in the
///   movement's own history, [`implicit_layoff`]) scales by the ramp
///   (REENTRY-RAMP-001) instead of any
///   detraining decay; otherwise 31–56 days without the movement scales by the
///   detraining credit and >56 days drops the anchor (DETRAIN-STR-001); else a
///   latest-session e1RM ≥5 % under baseline cuts the next load 5 %
///   (AUTOREG-E1RM-GATE-001).
///
/// `sets` is an ITERATOR so `build_plan_views` can hand over the history logged
/// strictly before each prescribed day without allocating; `today_epoch_day`
/// is that cutoff (the detraining and ramp reference).
pub(super) fn build_plan_anchors<'a>(
    sets: impl Iterator<Item = &'a LoggedSet>,
    ctx: AnchorCtx,
    today_epoch_day: i64,
    offset: i64,
) -> crate::plan::Anchors {
    use crate::plan::{DerateCause, LiftDerate};
    use std::collections::HashMap;
    let sets: Vec<&LoggedSet> = sets.collect();
    // Normalize each distinct name once: a history repeats a handful of names
    // thousands of times.
    let mut keys: HashMap<&str, Vec<String>> = HashMap::new();
    // Dated training days of each movement (any rep count: a >10-rep set
    // never anchors, but it IS training that keeps the anchor fresh).
    let mut trained_days: HashMap<Vec<String>, Vec<i64>> = HashMap::new();
    // movement → (eligible sets, display name, latest observed_at)
    let mut groups: HashMap<Vec<String>, (Vec<Obs>, String, i64)> = HashMap::new();
    for s in &sets {
        if s.hold_sec.is_some()
            || (ctx.calisthenics && crate::calisthenics::tier_by_name(&s.exercise).is_some())
        {
            continue;
        }
        let key = keys
            .entry(s.exercise.as_str())
            .or_insert_with(|| crate::plan::movement_key(&s.exercise))
            .clone();
        let day = (s.observed_at > 0).then(|| local_day(s.observed_at, offset));
        if let Some(d) = day {
            trained_days.entry(key.clone()).or_default().push(d);
        }
        if s.reps == 0
            || strength::effective_reps(s.reps, s.rpe) > strength::E1RM_RELIABLE_REP_CAP
            || crate::plan::is_isolation_lift(&s.exercise)
        {
            continue;
        }
        // An open-ended rating ("4+ reps left") is a lower bound like an
        // unrated set (C11, `strength::rpe_is_exact`): it can raise or
        // re-confirm an anchor, never cut it.
        let rated = strength::rpe_is_exact(s.rpe);
        let e1 = if rated {
            strength::e1rm_epley_rpe(s.weight_kg, s.reps, s.rpe)
        } else {
            strength::e1rm_epley(s.weight_kg, s.reps)
        };
        if e1.is_nan() || e1 <= 0.0 {
            continue;
        }
        let g = groups
            .entry(key)
            .or_insert_with(|| (Vec::new(), s.exercise.clone(), i64::MIN));
        if s.observed_at >= g.2 {
            g.1 = s.exercise.clone();
            g.2 = s.observed_at;
        }
        g.0.push(Obs {
            day,
            observed_at: s.observed_at,
            weight: s.weight_kg,
            e1,
            rated,
        });
    }

    // The declared layoff's re-entry ramp as of today (REENTRY-RAMP-001). An
    // undatable return never advances (every session counts as pre-layoff).
    let ramp_at = |weeks_off: f64, returned: Option<i64>, day: i64| {
        individualization::reentry_ramp(weeks_off, returned.map_or(0, |r| day.saturating_sub(r)))
            .value
    };
    let layoff = ctx.layoff.map(|l| {
        (
            l,
            ramp_at(l.weeks_off, l.returned_epoch_day, today_epoch_day),
        )
    });
    let reentry_novice = layoff.is_some_and(|(_, r)| r.active && r.treat_as_novice);

    let mut out: Vec<(String, f64, i64)> = Vec::new();
    let mut derates: Vec<LiftDerate> = Vec::new();
    let mut stalls: Vec<(String, u8)> = Vec::new();
    for (key, (mut obs, name, latest_at)) in groups {
        obs.sort_by_key(|o| (day_key(o.day), o.observed_at));
        let mut days = trained_days.remove(&key).unwrap_or_default();
        days.sort_unstable();
        days.dedup();
        let last_day = days.last().copied();
        // Detraining credit from the last training day, else the e1RM-drop cut
        // (one derate, never stacked; past 8 weeks the anchor is dropped).
        let finish = |w: WindowResult,
                      last: Option<i64>|
         -> Option<(f64, Option<(f64, DerateCause)>)> {
            let days_since = last.map(|d| today_epoch_day.saturating_sub(d));
            let credit = days_since.map_or(Some(1.0), |d| strength::lift_detrain_credit(d).value);
            match (credit, days_since) {
                (None, _) => None,
                (Some(c), Some(d)) if c < 1.0 => Some((
                    w.anchor,
                    Some((c, DerateCause::Detraining { days_since: d })),
                )),
                _ if w.dropped => Some((w.anchor, Some((E1RM_DROP_GATE, DerateCause::E1rmDrop)))),
                _ => Some((w.anchor, None)),
            }
        };
        let same_day = |d: i64| d;
        let full = |_: i64| 1.0;
        let all: Vec<&Obs> = obs.iter().collect();
        // The plain (no-layoff) scoring: the anchor without a break, and the
        // stall count either way.
        let plain = score_sessions(&all, ctx.off_day, &same_day, &full);
        // A declared layoff governs; else an undeclared break inside this
        // movement's own history runs the same re-entry ramp.
        let picked = match layoff.or_else(|| implicit_layoff(&days, today_epoch_day)) {
            None => plain.and_then(|w| finish(w, last_day)),
            Some((l, today_ramp)) => {
                let returned = l.returned_epoch_day;
                let is_pre = |o: &Obs| match returned {
                    Some(rd) => o.day.is_none_or(|d| d < rd),
                    None => true,
                };
                if today_ramp.treat_as_novice {
                    // >8 wk: the KB gives no fraction and directs a fresh,
                    // novice-style start, so pre-layoff anchors are set aside;
                    // a movement logged since the return anchors normally.
                    let post: Vec<&Obs> = obs.iter().filter(|o| !is_pre(o)).collect();
                    score_sessions(&post, ctx.off_day, &same_day, &full)
                        .and_then(|w| finish(w, last_day))
                } else {
                    // ≤8 wk: the break never ages a pre-layoff anchor (dated at
                    // the return day); the ramp replaces that decay.
                    let eff = |d: i64| match returned {
                        Some(r) if d < r => r,
                        _ => d,
                    };
                    let frac_at = |d: i64| match returned {
                        Some(r) if d >= r => ramp_at(l.weeks_off, returned, d).frac.unwrap_or(1.0),
                        Some(_) => 1.0,
                        None => today_ramp.frac.unwrap_or(1.0),
                    };
                    score_sessions(&all, ctx.off_day, &eff, &frac_at).and_then(|w| {
                        let frac = today_ramp.frac.unwrap_or(1.0);
                        if today_ramp.active && frac < 1.0 {
                            // A post-return RATED estimate above the ramped
                            // load is used unscaled (it already reflects the
                            // break).
                            let post_rated = obs
                                .iter()
                                .filter(|o| {
                                    !is_pre(o)
                                        && o.rated
                                        && o.day.is_some_and(|d| {
                                            today_epoch_day.saturating_sub(d) <= WINDOW_DAYS
                                        })
                                })
                                .map(|o| o.e1)
                                .fold(0.0_f64, f64::max);
                            let working = (w.anchor * frac).max(post_rated);
                            let f = working / w.anchor;
                            let day =
                                returned.map_or(0, |r| today_epoch_day.saturating_sub(r).max(0));
                            Some((
                                w.anchor,
                                (f < 1.0).then_some((
                                    f,
                                    DerateCause::Reentry {
                                        day,
                                        ramp_days: today_ramp.ramp_days,
                                    },
                                )),
                            ))
                        } else {
                            finish(w, last_day.map(|d| returned.map_or(d, |r| d.max(r))))
                        }
                    })
                }
            }
        };
        let Some((anchor, derate)) = picked else {
            continue;
        };
        if let Some((frac, cause)) = derate.filter(|(f, _)| *f < 1.0) {
            derates.push(LiftDerate {
                exercise: name.clone(),
                frac: (frac * 1000.0).round() / 1000.0,
                cause,
            });
        }
        let stalled = plain.map_or(0, |w| w.weeks_stalled);
        if stalled > 0 {
            stalls.push((name.clone(), stalled));
        }
        out.push((name, anchor, latest_at));
    }
    out.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
    derates.sort_by(|a, b| a.exercise.cmp(&b.exercise));
    stalls.sort();
    crate::plan::Anchors {
        lift_e1rm: out
            .into_iter()
            .map(|(k, e1, _)| (k, (e1 * 10.0).round() / 10.0))
            .collect(),
        lift_derates: derates,
        reentry_novice,
        lift_stall_weeks: stalls,
        last_working: build_last_working(&sets, ctx.calisthenics, today_epoch_day, offset),
        ..Default::default()
    }
}

/// Each movement's LAST logged session's working sets (hypertrophy double
/// progression, HYP-DOUBLEPROG-001), most-recent-first then by name. Per
/// movement (`movement_key`), the newest dated session day is taken; its
/// working load is that day's heaviest weight and its working sets are every
/// set logged at that load (lighter warm-ups / back-offs are not judged). A
/// bodyweight movement keeps load 0 (rep progression only). Holds and, on a
/// Calisthenics profile, ladder rungs are skipped (they have their own
/// progression). A movement untrained for more than 56 days drops out, the
/// same horizon at which an untrained e1RM anchor is dropped (DETRAIN-STR-001).
pub(super) fn build_last_working(
    sets: &[&LoggedSet],
    calisthenics: bool,
    today_epoch_day: i64,
    offset: i64,
) -> Vec<crate::plan::LastWorking> {
    const HORIZON_DAYS: i64 = 56;
    use std::collections::HashMap;
    let mut keys: HashMap<&str, Vec<String>> = HashMap::new();
    // movement → (newest day, sets on that day in log order)
    let mut newest: HashMap<Vec<String>, (i64, Vec<&LoggedSet>)> = HashMap::new();
    for s in sets {
        if s.observed_at <= 0
            || s.hold_sec.is_some()
            || s.weight_kg < 0.0
            || s.reps == 0
            || (calisthenics && crate::calisthenics::tier_by_name(&s.exercise).is_some())
        {
            continue;
        }
        let day = local_day(s.observed_at, offset);
        if day > today_epoch_day || today_epoch_day - day > HORIZON_DAYS {
            continue;
        }
        let key = keys
            .entry(s.exercise.as_str())
            .or_insert_with(|| crate::plan::movement_key(&s.exercise))
            .clone();
        let e = newest.entry(key).or_insert((day, Vec::new()));
        if day > e.0 {
            *e = (day, Vec::new());
        }
        if day == e.0 {
            e.1.push(*s);
        }
    }
    let mut out: Vec<(crate::plan::LastWorking, i64)> = newest
        .into_values()
        .filter_map(|(day, mut day_sets)| {
            day_sets.sort_by_key(|s| s.observed_at);
            let top = day_sets
                .iter()
                .map(|s| s.weight_kg)
                .fold(f64::NEG_INFINITY, f64::max);
            let working: Vec<(u32, f64)> = day_sets
                .iter()
                .filter(|s| (s.weight_kg - top).abs() < 0.01)
                .map(|s| (s.reps, s.rpe))
                .collect();
            let last = day_sets.last()?;
            (!working.is_empty()).then(|| {
                (
                    crate::plan::LastWorking {
                        exercise: last.exercise.clone(),
                        weight_kg: top.max(0.0),
                        sets: working,
                        age_days: today_epoch_day - day,
                    },
                    last.observed_at,
                )
            })
        })
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.exercise.cmp(&b.0.exercise)));
    out.into_iter().map(|(w, _)| w).collect()
}

/// Logged running km per COMPLETED local calendar week before
/// `week_start_epoch_day` (a Monday), most-recent-first: index 0 is the week
/// just before, up to `RUN_WEEK_HISTORY` weeks back. Runs are bucketed by
/// their LOCAL day exactly like `build_run_anchors`; undated (`observed_at <=
/// 0`) and same-week-or-later rows never count. Feeds the plan's cross-cycle
/// volume carry (RUN-VOLUME-001), a measured fact, never a target.
pub(super) fn build_run_week_km(
    runs: &[LoggedRun],
    facts: &RunFacts,
    week_start_epoch_day: i64,
    utc_offset_sec: i64,
) -> Vec<f64> {
    const RUN_WEEK_HISTORY: i64 = 12;
    let mut weeks = vec![0.0_f64; RUN_WEEK_HISTORY as usize];
    for (r, &km) in runs.iter().zip(&facts.km) {
        if r.observed_at <= 0 {
            continue;
        }
        let day = local_day(r.observed_at, utc_offset_sec);
        if day >= week_start_epoch_day {
            continue;
        }
        let back = (week_start_epoch_day - 1 - day).div_euclid(7);
        if (0..RUN_WEEK_HISTORY).contains(&back) {
            weeks[back as usize] += km;
        }
    }
    // Drop trailing never-run weeks so an athlete with no history yields an
    // empty list rather than twelve zeros.
    while weeks.last() == Some(&0.0) {
        weeks.pop();
    }
    weeks
}

/// Full-credit window of the longest-recent-run anchor, days (RUN-SPIKE-001's
/// "longest run in the last 30 days"); the stated longest run shares it.
pub(super) const LONGEST_RUN_WINDOW_DAYS: i64 = 30;
/// Days past [`LONGEST_RUN_WINDOW_DAYS`] over which a demonstrated long run's
/// credit tapers linearly to zero (DETRAIN-001). The linear shape and the
/// length are an expert-opinion parameter the KB does not state, flagged per
/// the GRADE good-practice-statement precedent
/// (`knowledge-base/autoreg-citation-provenance-resolution.md`); the retention
/// principle itself is graded (Moderate).
const RUN_DETRAIN_TAPER_DAYS: i64 = 28;
/// Age at which a demonstrated long run carries no credit: 30 + 28 = 58 days,
/// about 8 weeks, the edge of the KB's re-entry bracket (Table 3.4b: 4–8 wk off
/// → "treat near-novice for spike caps").
const RUN_DECAY_END_DAYS: i64 = LONGEST_RUN_WINDOW_DAYS + RUN_DETRAIN_TAPER_DAYS;
/// Trailing window of the measured weekly running volume, days (4 weeks).
const WEEKLY_VOLUME_WINDOW_DAYS: i64 = 28;

/// Run-history anchors for plan synthesis, derived from `model.runs` the same
/// way [`build_plan_anchors`] anchors lifts to logged e1RM. Returns
/// [`RunAnchors`]: `(longest_recent_run_km, recent_weekly_km,
/// longest_run_detrained)`.
///
/// Deterministic: the "now" reference is the shell-supplied `today_epoch_day`,
/// never a wall clock. Runs are bucketed by their LOCAL calendar day
/// (`observed_at` shifted by `utc_offset_sec`, like every plan-strip day
/// bucket), so a window ends at the end of local today: a run logged earlier
/// today counts, and a future-dated row never does.
///
/// - `longest_recent_run_km`: the longest run of the trailing
///   [`LONGEST_RUN_WINDOW_DAYS`], the window [`spike_baseline_km`] uses
///   (RUN-SPIKE-001, Frandsen et al. 2025). A long run prescribed at this value
///   is no single-session spike, so it is a safe demonstrated-capacity anchor;
///   progression beyond it stays governed by the spike and ramp rules. Past the
///   window a run's credit tapers linearly to zero over
///   [`RUN_DETRAIN_TAPER_DAYS`] instead of vanishing in one step (DETRAIN-001 /
///   File 08 load-037, Moderate: trained aerobic capacity is retained ~2 wk and
///   declines only ~6–20 % over ~4 wk; Mujika & Padilla 2000, Bosquet et al.
///   2013).
/// - `recent_weekly_km`: the measured average weekly volume, the km of the
///   trailing [`WEEKLY_VOLUME_WINDOW_DAYS`] divided by the weeks that window
///   actually spans since its first run (1–4), fed to the long-run 25 % share
///   rule (RUN-LONGRUN-001, running-016).
/// - `longest_run_detrained`: the winning run is older than the full-credit
///   window, so the plan re-points the long-run citation to DETRAIN-001.
///
/// Each distance is `None` when its window holds no runs. The downstream
/// guardrails (≤2× the daily average, the spike ceiling, the ≤25 % share)
/// still bound the long run, and the log-time spike gate keeps its hard 30-day
/// window: the taper is a planning-capacity concept only.
pub(super) fn build_run_anchors(
    runs: &[LoggedRun],
    facts: &RunFacts,
    today_epoch_day: i64,
    utc_offset_sec: i64,
) -> RunAnchors {
    let local_day = |t: i64| crate::local_day(t, utc_offset_sec);

    // Longest recent run, detraining-adjusted: the max over every non-future
    // run inside the decay horizon of `distance × credit(age)`. An in-window
    // run counts at full credit, so an older, decayed run only wins when it
    // was longer than every in-window run.
    let mut best: f64 = 0.0;
    let mut best_detrained = false;
    for (r, &km) in runs.iter().zip(&facts.km) {
        let d = local_day(r.observed_at);
        if d > today_epoch_day {
            continue; // future-dated row excluded, like spike_baseline_km
        }
        let age = today_epoch_day.saturating_sub(d);
        if age >= RUN_DECAY_END_DAYS {
            continue; // fully detrained → no capacity credit
        }
        let eff = km * detrain_credit(age);
        if eff > best {
            best = eff;
            best_detrained = age > LONGEST_RUN_WINDOW_DAYS;
        }
    }
    let longest_recent = (best > 0.0).then_some(best);
    let detrained = longest_recent.is_some() && best_detrained;

    let mut total_km = 0.0;
    let mut first_run_day: Option<i64> = None;
    for (r, &km) in runs.iter().zip(&facts.km) {
        let d = local_day(r.observed_at);
        if d <= today_epoch_day && today_epoch_day.saturating_sub(d) < WEEKLY_VOLUME_WINDOW_DAYS {
            total_km += km;
            first_run_day = Some(first_run_day.map_or(d, |f| f.min(d)));
        }
    }
    // Divide by the number of weeks the window ACTUALLY spans, so a single
    // recent week of running is not diluted by empty weeks into a phantom low
    // average.
    let max_weeks = WEEKLY_VOLUME_WINDOW_DAYS / 7;
    let recent_weekly = (total_km > 0.0).then(|| {
        let weeks = first_run_day.map_or(max_weeks, |first| {
            (((today_epoch_day.saturating_sub(first) + 1) as f64 / 7.0).ceil() as i64)
                .clamp(1, max_weeks)
        });
        total_km / weeks as f64
    });

    (longest_recent, recent_weekly, detrained)
}

/// Detraining-credit factor for a demonstrated running capacity `age_days`
/// old: full credit through [`LONGEST_RUN_WINDOW_DAYS`], then a linear taper
/// to 0 at [`RUN_DECAY_END_DAYS`] (DETRAIN-001).
fn detrain_credit(age_days: i64) -> f64 {
    if age_days <= LONGEST_RUN_WINDOW_DAYS {
        1.0
    } else if age_days >= RUN_DECAY_END_DAYS {
        0.0
    } else {
        1.0 - (age_days - LONGEST_RUN_WINDOW_DAYS) as f64 / RUN_DETRAIN_TAPER_DAYS as f64
    }
}
