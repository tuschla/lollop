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
pub(super) fn lift_anchors_now(model: &Model, set_index: &SetIndex) -> crate::plan::Anchors {
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
    build_plan_anchors(set_index.with_facts(&model.sets), ctx, today, offset)
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

/// The return day of the athlete's latest undeclared break from ALL lifting:
/// the first lift day after at least a full week without a single logged set,
/// or `today + 1` (the day prescribed from the history through `today`) while
/// the break still runs. A missed session of one lift is no break; a week with
/// no resistance training at all is (File 08 Table 3.4b brackets are by time
/// off training, REENTRY-001). `days` are every dated lift day on or before
/// `today`, sorted and unique.
fn lifting_break_return(days: &[i64], today: i64) -> Option<i64> {
    let pending = days.last().map(|&last| [last, today.saturating_add(1)]);
    pending
        .iter()
        .map(|w| &w[..])
        .chain(days.windows(2).rev())
        .find(|w| off_weeks(w[1].saturating_sub(w[0])) >= 1.0)
        .map(|w| w[1])
}

/// Weeks fully off between two training days `gap` days apart (a lift on
/// Monday and the next on the following Monday is six days off, under a week).
fn off_weeks(gap: i64) -> f64 {
    (gap - 1).max(0) as f64 / 7.0
}

/// An undeclared break in one movement's training, treated as a layoff
/// (REENTRY-001 / REENTRY-RAMP-001) while its re-entry ramp runs. `days` are
/// the movement's dated training days, sorted and unique; `break_return` is
/// the athlete's latest return from a break off all lifting
/// ([`lifting_break_return`]). Two kinds of break, the latest one governs:
///
/// - the athlete's break from all lifting (1–4 weeks without this movement):
///   its Table 3.4b bracket by this movement's own time off before the return
///   (1–2 wk ~90 %, 2–4 wk ~80–85 %), ramping back from the return day;
/// - a gap of more than 30 days in this movement's own history that the
///   detraining credit marks (DETRAIN-STR-001), even while other lifts went
///   on; the first day after it is the return day.
///
/// Either way the athlete ramps back over the bracket's weeks instead of
/// jumping straight back to full load the day after one light session. A
/// break past 8 weeks returns `None`: the anchor window's horizon already sets
/// those pre-gap sessions aside (no load fraction).
fn implicit_layoff(
    days: &[i64],
    break_return: Option<i64>,
    today: i64,
) -> Option<(Layoff, individualization::ReentryRamp)> {
    let gap = |w: &[i64]| w[1].saturating_sub(w[0]);
    let own = days
        .windows(2)
        .rev()
        .find(|&w| strength::lift_detrain_credit(gap(w)).value != Some(1.0))
        .filter(|&w| strength::lift_detrain_credit(gap(w)).value.is_some())
        .map(|w| (w[1], gap(w) as f64 / 7.0));
    let athlete = break_return.and_then(|r| {
        let last = *days.iter().rev().find(|&&d| d < r)?;
        let gap = r.saturating_sub(last);
        // The detraining credit takes over past 30 days without the
        // movement, counted the way `build_plan_anchors`' `finish` counts
        // them: to the return day, or to today while the break still runs
        // (`r = today + 1`). Counting the prescribed day too would hand a
        // 30-day pending break to neither rule (full load).
        let days_since = r.min(today).saturating_sub(last);
        (strength::lift_detrain_credit(days_since).value == Some(1.0)).then(|| (r, off_weeks(gap)))
    });
    let (returned, weeks_off) = match (own, athlete) {
        (Some(o), Some(a)) if a.0 > o.0 => a,
        (o, a) => o.or(a)?,
    };
    let layoff = Layoff {
        weeks_off,
        returned_epoch_day: Some(returned),
    };
    let ramp = individualization::reentry_ramp(weeks_off, today.saturating_sub(returned)).value;
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
///   re-entry ramp (a declared layoff, a week or more without any lifting, or
///   an undeclared 31–56-day gap in the movement's own history,
///   [`implicit_layoff`]) scales by the ramp (REENTRY-RAMP-001) instead of any
///   detraining decay; otherwise 31–56 days without the movement scales by the
///   detraining credit and >56 days drops the anchor (DETRAIN-STR-001); else a
///   latest-session e1RM ≥5 % under baseline cuts the next load 5 %
///   (AUTOREG-E1RM-GATE-001). A lift without an anchor that carries a
///   pre-break last working load gets the same ramp derate.
///
/// `sets` is an ITERATOR so `build_plan_views` can hand over the history logged
/// strictly before each prescribed day without allocating; each set comes with
/// its name's facts ([`SetIndex`]), resolved once per distinct name.
/// `today_epoch_day` is that cutoff (the detraining and ramp reference).
pub(super) fn build_plan_anchors<'a>(
    sets: impl Iterator<Item = (&'a LoggedSet, NameFacts)>,
    ctx: AnchorCtx,
    today_epoch_day: i64,
    offset: i64,
) -> crate::plan::Anchors {
    use crate::plan::{DerateCause, LiftDerate};
    use std::collections::HashMap;
    let sets: Vec<(&LoggedSet, NameFacts)> = sets.collect();
    // Per movement id ([`NameFacts::movement`]), filled per set and keyed by
    // `movement_key` once per movement after the pass: one of its names, its
    // dated training days, and its eligible sets with the display name and
    // latest observed_at.
    #[derive(Default)]
    struct Movement<'a> {
        name: Option<&'a str>,
        days: Vec<i64>,
        /// Eligible sets and the latest of them in log order
        /// ([`LoggedSet::chrono_cmp`]), whose spelling the plan shows.
        group: Option<(Vec<Obs>, &'a LoggedSet)>,
    }
    let mut movements: Vec<Movement> = Vec::new();
    // Every dated lift day up to today, any set (holds and ladder rungs are
    // resistance training too): the athlete's breaks from all lifting.
    let mut lift_days: Vec<i64> = Vec::new();
    for &(s, facts) in &sets {
        let day = (s.observed_at > 0).then(|| local_day(s.observed_at, offset));
        if let Some(d) = day.filter(|d| *d <= today_epoch_day) {
            lift_days.push(d);
        }
        if s.hold_sec.is_some() || (ctx.calisthenics && facts.calis_tier.is_some()) {
            continue;
        }
        let id = facts.movement as usize;
        if id >= movements.len() {
            movements.resize_with(id + 1, Movement::default);
        }
        let m = &mut movements[id];
        m.name = Some(&s.exercise);
        // Dated training days of the movement (any rep count: a >10-rep set
        // never anchors, but it IS training that keeps the anchor fresh).
        if let Some(d) = day {
            m.days.push(d);
        }
        if s.reps == 0
            || strength::effective_reps(s.reps, s.rpe) > strength::E1RM_RELIABLE_REP_CAP
            || facts.isolation
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
        let g = m.group.get_or_insert_with(|| (Vec::new(), s));
        // Log order, never storage order: an Undo re-pushes a same-second set
        // at the tail, which must not rename the plan item.
        if s.chrono_cmp(g.1).is_ge() {
            g.1 = s;
        }
        g.0.push(Obs {
            day,
            observed_at: s.observed_at,
            weight: s.weight_kg,
            e1,
            rated,
        });
    }
    // Dated training days of each movement, and movement → (eligible sets,
    // display name, latest observed_at).
    let mut trained_days: HashMap<Vec<String>, Vec<i64>> = HashMap::new();
    let mut groups: HashMap<Vec<String>, (Vec<Obs>, String, i64)> = HashMap::new();
    for m in movements {
        let Some(any_name) = m.name else {
            continue;
        };
        let key = crate::plan::movement_key(any_name);
        if !m.days.is_empty() {
            trained_days.insert(key.clone(), m.days);
        }
        if let Some((obs, latest)) = m.group {
            groups.insert(key, (obs, latest.exercise.clone(), latest.observed_at));
        }
    }
    let sets: Vec<&LoggedSet> = sets.into_iter().map(|(s, _)| s).collect();

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
    lift_days.sort_unstable();
    lift_days.dedup();
    let break_return = lifting_break_return(&lift_days, today_epoch_day);
    // The re-entry that governs a movement with these training days: the
    // declared layoff while its ramp runs; once it has ended, an undeclared
    // break that returned after it (a finished declared layoff never shadows
    // a newer break). An undatable declared layoff never ends.
    let governing = |days: &[i64]| {
        if layoff.is_some_and(|(_, r)| r.active) {
            return layoff;
        }
        let implicit = implicit_layoff(days, break_return, today_epoch_day);
        match (layoff, implicit) {
            (Some((d, _)), Some((i, _)))
                if d.returned_epoch_day
                    .is_some_and(|dr| i.returned_epoch_day.is_some_and(|ir| ir > dr)) =>
            {
                implicit
            }
            (Some(_), _) => layoff,
            (None, i) => i,
        }
    };

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
        // The governing re-entry (a declared layoff, or a newer undeclared
        // break from all lifting or inside this movement's own history) runs
        // the re-entry ramp.
        let picked = match governing(&days) {
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
                    // ≤8 wk: the break itself never ages a pre-layoff anchor:
                    // pre-layoff sessions move forward by the break's length
                    // (the last one lands on the return day, the ones before
                    // keep their spacing, so a best that had already left the
                    // window stays out); the ramp replaces that decay.
                    let shift = returned
                        .and_then(|r| {
                            days.iter()
                                .rev()
                                .find(|&&d| d < r)
                                .map(|&d| r.saturating_sub(d))
                        })
                        .unwrap_or(0);
                    let eff = |d: i64| match returned {
                        Some(r) if d < r => d.saturating_add(shift),
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
    let last_working = build_last_working(&sets, ctx.calisthenics, today_epoch_day, offset);
    // A lift with no e1RM anchor (isolation, high-rep) still carries its last
    // working load (hypertrophy double progression, the start-weight
    // suggestion): when that load was lifted before a break, the same re-entry
    // ramp scales it (REENTRY-RAMP-001: only numbers set before the break);
    // otherwise 31–56 days without the movement scale it by the detraining
    // credit, exactly as an anchored lift's `finish` does (DETRAIN-STR-001),
    // so more time off never carries a heavier load.
    for lw in &last_working {
        let Some(mut days) = trained_days.remove(&crate::plan::movement_key(&lw.exercise)) else {
            continue;
        };
        days.sort_unstable();
        days.dedup();
        let governed = governing(&days);
        if let Some((l, ramp)) = governed {
            let lifted = today_epoch_day.saturating_sub(lw.age_days);
            let pre_break = l.returned_epoch_day.is_none_or(|r| lifted < r);
            if let Some(frac) = ramp.frac.filter(|f| pre_break && ramp.active && *f < 1.0) {
                derates.push(LiftDerate {
                    exercise: lw.exercise.clone(),
                    frac: (frac * 1000.0).round() / 1000.0,
                    cause: DerateCause::Reentry {
                        day: l
                            .returned_epoch_day
                            .map_or(0, |r| today_epoch_day.saturating_sub(r).max(0)),
                        ramp_days: ramp.ramp_days,
                    },
                });
                continue;
            }
        }
        // A declared return day restarts the count, as in `finish`.
        let returned = governed.and_then(|(l, _)| l.returned_epoch_day);
        let Some(days_since) = days
            .last()
            .map(|&d| today_epoch_day.saturating_sub(returned.map_or(d, |r| d.max(r))))
        else {
            continue;
        };
        if let Some(credit) = strength::lift_detrain_credit(days_since)
            .value
            .filter(|c| *c < 1.0)
        {
            derates.push(LiftDerate {
                exercise: lw.exercise.clone(),
                frac: credit,
                cause: DerateCause::Detraining { days_since },
            });
        }
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
        last_working,
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
        if s.observed_at <= 0 || s.hold_sec.is_some() || s.weight_kg < 0.0 || s.reps == 0 {
            continue;
        }
        let day = local_day(s.observed_at, offset);
        if day > today_epoch_day
            || today_epoch_day.saturating_sub(day) > HORIZON_DAYS
            || (calisthenics && crate::calisthenics::tier_by_name(&s.exercise).is_some())
        {
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
            day_sets.sort_by(|a, b| a.chrono_cmp(b));
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
/// the LOCAL day they STARTED exactly like `build_run_anchors`; undated
/// (`observed_at <= 0`) and same-week-or-later rows never count. Feeds the
/// plan's cross-cycle volume carry (RUN-VOLUME-001), a measured fact, never a target.
pub(super) fn build_run_week_km(
    runs: &[LoggedRun],
    facts: &RunFacts,
    week_start_epoch_day: i64,
    utc_offset_sec: i64,
) -> Vec<f64> {
    const RUN_WEEK_HISTORY: i64 = 12;
    let mut weeks = vec![0.0_f64; RUN_WEEK_HISTORY as usize];
    // Chronological (`RunFacts::chrono`): a week's km is a float sum.
    for &i in &facts.chrono {
        let (r, km) = (&runs[i], facts.km[i]);
        if r.observed_at <= 0 {
            continue;
        }
        let day = local_day(facts.started[i], utc_offset_sec);
        if day >= week_start_epoch_day {
            continue;
        }
        let back = (week_start_epoch_day - 1).saturating_sub(day).div_euclid(7);
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
/// never a wall clock. Runs are bucketed by the LOCAL calendar day they
/// STARTED (`RunFacts::started` shifted by `utc_offset_sec`, like plan
/// completion), so a window ends at the end of local today: a run started
/// earlier today counts, and a future-dated row never does.
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
    for (i, &km) in facts.km.iter().enumerate() {
        let d = local_day(facts.started[i]);
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
    for &i in &facts.chrono {
        let km = facts.km[i];
        let d = local_day(facts.started[i]);
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

/// The athlete's latest undeclared break from running as of local day `day`
/// (the history the day `day + 1` is prescribed from): the latest stretch of
/// at least a full week without a logged run, between two run days or from
/// the last run to the prescribed day while it still runs. File 08 Table 3.4b
/// brackets the running re-entry by that time off (RUN-REENTRY-001, applied in
/// `plan::run`). A re-entry ends: the break is returned only while its ramp
/// back to the prior volume runs, or while a run from before it would still
/// carry long-run credit (DETRAIN-001, [`RUN_DECAY_END_DAYS`]) that the
/// rebuild sets aside. `None` without a dated run or such a gap, or once the
/// latest one no longer shapes the plan.
pub(super) fn build_run_break(
    runs: &[LoggedRun],
    facts: &RunFacts,
    day: i64,
    utc_offset_sec: i64,
) -> Option<crate::plan::RunBreak> {
    let local_day = |t: i64| crate::local_day(t, utc_offset_sec);
    let mut days: Vec<i64> = runs
        .iter()
        .zip(&facts.started)
        .filter(|(r, _)| r.observed_at > 0)
        .map(|(_, &start)| local_day(start))
        .filter(|d| *d <= day)
        .collect();
    days.sort_unstable();
    days.dedup();
    let pending = [*days.last()?, day.saturating_add(1)];
    let (last_run, back) = std::iter::once(&pending[..])
        .chain(days.windows(2).rev())
        .find(|w| off_weeks(w[1].saturating_sub(w[0])) >= 1.0)
        .map(|w| (w[0], w[1]))?;
    let weeks_off = off_weeks(back.saturating_sub(last_run));
    let mut reentry =
        individualization::run_reentry(weeks_off, day.saturating_add(1).saturating_sub(back)).value;
    // A pre-break run carries long-run credit until it is fully detrained.
    let pre_break_credit = day.saturating_sub(last_run) < RUN_DECAY_END_DAYS;
    reentry.rebuild_long_run &= pre_break_credit;
    reentry.near_novice_spike &= pre_break_credit;
    if !reentry.active && !reentry.rebuild_long_run {
        return None;
    }
    // Longest run since the return, credited exactly like
    // `build_run_anchors`' longest recent run.
    let mut best = 0.0_f64;
    let mut best_detrained = false;
    for ((r, &km), &start) in runs.iter().zip(&facts.km).zip(&facts.started) {
        let d = local_day(start);
        let age = day.saturating_sub(d);
        if r.observed_at <= 0 || d < back || d > day || age >= RUN_DECAY_END_DAYS {
            continue;
        }
        let eff = km * detrain_credit(age);
        if eff > best {
            best = eff;
            best_detrained = age > LONGEST_RUN_WINDOW_DAYS;
        }
    }
    Some(crate::plan::RunBreak {
        weeks_off,
        last_run_epoch_day: last_run,
        prior_weekly_km: build_run_anchors(facts, last_run, utc_offset_sec).1,
        reentry,
        longest_since_km: (best > 0.0).then_some(best),
        longest_since_detrained: best > 0.0 && best_detrained,
        stated_longest_after: false,
    })
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
