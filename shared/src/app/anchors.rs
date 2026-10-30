//! Plan anchors derived from the logged history: per-lift e1RM loading
//! anchors, last working sets, and the run-history anchors.

use super::*;
use crate::plan::{DerateCause, LiftDerate};
use std::collections::HashMap;

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
        Some(Layoff {
            returned_epoch_day: p.declared_layoff_return()?,
            weeks_off: p.weeks_off?,
        })
    }
}

/// Everything besides the sets and the reference day that shapes the anchors.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct AnchorCtx<'a> {
    pub(super) layoff: Option<Layoff>,
    /// Calisthenics profile: ladder-rung sets never anchor a %1RM load.
    pub(super) calisthenics: bool,
    /// A session day the athlete marked as an off day (session review
    /// `bad_day`): a low e1RM that day does not cut the next load.
    pub(super) off_day: Option<i64>,
    /// The planned lifting deload weeks: their prescribed light sets neither
    /// anchor an e1RM nor become a lift's last working sets (the deload is
    /// taken from the working weight and the next block resumes from it,
    /// HYP-DELOAD-RX-001, 03-hypertrophy.md:395; HYP-DOUBLEPROG-001 carries the
    /// working load, :361-362). They still count as training days.
    pub(super) deloads: DeloadWeeks<'a>,
    /// The profile weighs in pounds: the plan floors loads onto the lb plate
    /// grid, which sets how far under its target a plan load can land.
    pub(super) weight_lb: bool,
}

impl<'a> AnchorCtx<'a> {
    /// The context for `profile` (the one effective on the day being
    /// prescribed), with the off day taken from the current session review.
    pub(super) fn for_profile(
        profile: Option<&Profile>,
        model: &Model,
        offset: i64,
        deloads: DeloadWeeks<'a>,
    ) -> Self {
        AnchorCtx {
            layoff: profile.and_then(Layoff::of),
            calisthenics: profile
                .is_some_and(|p| p.resistance_modality == ResistanceModality::Calisthenics),
            off_day: off_day(model, offset),
            deloads,
            weight_lb: profile.is_some_and(|p| p.weight_lb),
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
pub(super) fn lift_anchors_now(
    model: &Model,
    set_index: &SetIndex,
    days: &LoggedDays,
) -> crate::plan::Anchors {
    let offset = model.today_utc_offset_sec;
    let today = model.today_epoch_day.unwrap_or_else(|| {
        model
            .sets
            .iter()
            .filter(|s| !is_undated(s.observed_at))
            .map(|s| local_day(s.observed_at, offset))
            .max()
            .unwrap_or(0)
    });
    let ctx = AnchorCtx::for_profile(
        model.profile(),
        model,
        offset,
        lift_deload_weeks(model, days),
    );
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
/// next working load by about 5 %. A rated set is judged on its plate-grid
/// credit (`Obs::e1_grid`).
pub(super) const E1RM_DROP_GATE: f64 = 0.95;
/// [`E1RM_DROP_GATE`] as the percentage drop the copy states.
pub(super) const E1RM_DROP_PCT: f64 = (1.0 - E1RM_DROP_GATE) * 100.0;

/// One anchor-eligible set of a movement.
struct Obs {
    /// Local session day; `None` for an undated legacy set (oldest).
    day: Option<i64>,
    observed_at: i64,
    weight: f64,
    /// RPE-extended e1RM for a rated set; the reps-as-performed Epley LOWER
    /// BOUND for an unrated one.
    e1: f64,
    /// A rated set's e1RM one plate-grid step heavier (`load_grid_step_kg`)
    /// at the same reps and RPE. The plan floors the load it asks for onto the
    /// grid, so a set done as asked shows its baseline only within that step:
    /// 85 % of a 55.5 kg overhead-press anchor is 47.2 kg, floored to 45 kg,
    /// and 45 kg × 3 at ~2 reps left reads 52.5 kg (94.6 %). The e1RM-drop
    /// gate compares at the RPE the set was done at (autoreg-022, e1RM "at
    /// fixed RPE") and credits it this step.
    e1_grid: f64,
    rated: bool,
}

/// The windowed anchor of a run of sessions plus the latest session's
/// e1RM-drop verdict.
#[derive(Clone, Copy)]
struct WindowResult {
    anchor: f64,
    /// The latest session's best rated e1RM, credited one plate-grid step
    /// (`Obs::e1_grid`), fell ≥5 % under the baseline before it (and that day
    /// was not marked an off day).
    dropped: bool,
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

/// The latest stretch of at least a full week off in `days` (dated training
/// days on or before `day`, sorted and unique), as (last training day before
/// it, return day): between two training days, or from the last one to
/// `day + 1` (the day prescribed from the history through `day`) while the
/// break still runs. File 08 Table 3.4b brackets re-entry by that time off.
fn latest_break(days: &[i64], day: i64) -> Option<(i64, i64)> {
    let pending = [*days.last()?, day.saturating_add(1)];
    std::iter::once(&pending[..])
        .chain(days.windows(2).rev())
        .find(|w| off_weeks(w[1].saturating_sub(w[0])) >= 1.0)
        .map(|w| (w[0], w[1]))
}

/// Weeks fully off between two training days `gap` days apart (a lift on
/// Monday and the next on the following Monday is six days off, under a week).
fn off_weeks(gap: i64) -> f64 {
    (gap - 1).max(0) as f64 / 7.0
}

/// An undeclared break in one movement's training, treated as a layoff
/// (REENTRY-001 / REENTRY-RAMP-001) while its re-entry ramp runs. `days` are
/// the movement's dated training days, sorted and unique; `break_return` is
/// the return day of the athlete's latest break from all lifting
/// ([`latest_break`] over every lift day). Two kinds of break, the latest one
/// governs:
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
    (ramp.active && !ramp.treat_as_novice()).then_some((layoff, ramp))
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
    // (real day, best grid-credited rated e1RM, working baseline before it)
    let mut last: Option<(i64, f64, Option<f64>)> = None;
    let mut i = 0;
    while i < obs.len() {
        let day = day_key(obs[i].day);
        let aged = if day == i64::MIN { day } else { eff_day(day) };
        let frac = if day == i64::MIN { 1.0 } else { work_frac(day) };
        // The anchor in force when this session started.
        let running = window_best(&list, aged);
        let mut j = i;
        let mut value: Option<f64> = None;
        let mut rated_credit: Option<f64> = None;
        while j < obs.len() && day_key(obs[j].day) == day {
            let o = obs[j];
            let v = if o.rated {
                rated_credit = Some(rated_credit.map_or(o.e1_grid, |r: f64| r.max(o.e1_grid)));
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
        last = Some((
            day,
            rated_credit.unwrap_or(f64::NAN),
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
    Some(WindowResult { anchor, dropped })
}

/// Logged e1RM anchors per MOVEMENT (0.1-rounded), most-recent-first, plus each
/// lift's working-load derate. Deterministic despite the HashMaps: the output
/// is sorted by (latest observed_at desc, name asc). Which sets anchor:
/// [`collect_movements`]; the windowed anchor: [`score_sessions`]; the one
/// derate per movement, never stacked: [`score_movement`] / [`finish`], and
/// [`last_working_derates`] for lifts without an anchor. `sets` (for a plan
/// day, the history logged strictly before it) come with their names' facts
/// ([`SetIndex`]). `today_epoch_day` is the detraining and ramp reference; a
/// set dated after it is ignored.
pub(super) fn build_plan_anchors<'a>(
    sets: impl Iterator<Item = (&'a LoggedSet, NameFacts)>,
    ctx: AnchorCtx,
    today_epoch_day: i64,
    offset: i64,
) -> crate::plan::Anchors {
    let sets: Vec<(&LoggedSet, NameFacts)> = sets.collect();
    let (lift_days, movements) = collect_movements(&sets, &ctx, today_epoch_day, offset);
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
        // A movement logged only in a planned deload week has no observation.
        if let Some((obs, latest)) = m.group.filter(|(obs, _)| !obs.is_empty()) {
            groups.insert(key, (obs, latest.exercise.clone(), latest.observed_at));
        }
    }

    let layoff = ctx.layoff.map(|l| {
        (
            l,
            ramp_at(l.weeks_off, l.returned_epoch_day, today_epoch_day),
        )
    });
    let reentry_novice = layoff.is_some_and(|(_, r)| r.active && r.treat_as_novice());
    // A missed session of one lift is no break; a week with no resistance
    // training at all is (REENTRY-001).
    let break_return = latest_break(&lift_days, today_epoch_day).map(|(_, back)| back);
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
    for (key, (mut obs, name, latest_at)) in groups {
        obs.sort_by_key(|o| (day_key(o.day), o.observed_at));
        let mut days = trained_days.remove(&key).unwrap_or_default();
        days.sort_unstable();
        days.dedup();
        let picked = score_movement(&obs, &days, governing(&days), ctx.off_day, today_epoch_day);
        let Some((anchor, derate)) = picked else {
            continue;
        };
        if let Some((frac, cause)) = derate.filter(|(f, _)| *f < 1.0) {
            derates.push(lift_derate(&name, frac, cause));
        }
        out.push((name, anchor, latest_at));
    }
    let last_working = build_last_working(&sets, &ctx, today_epoch_day, offset);
    derates.extend(last_working_derates(
        &last_working,
        &mut trained_days,
        governing,
        today_epoch_day,
    ));
    out.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
    derates.sort_by(|a, b| a.exercise.cmp(&b.exercise));
    crate::plan::Anchors {
        lift_e1rm: out
            .into_iter()
            .map(|(k, e1, _)| (k, round_tenths(e1)))
            .collect(),
        lift_derates: derates,
        reentry_novice,
        last_working,
        ..Default::default()
    }
}

/// One movement ([`NameFacts::movement`]) as [`collect_movements`] fills it:
/// one of its names, its dated training days, and its anchor-eligible sets with
/// the latest of them in log order ([`LoggedSet::chrono_cmp`]), whose spelling
/// the plan shows.
#[derive(Default)]
struct Movement<'a> {
    name: Option<&'a str>,
    days: Vec<i64>,
    group: Option<(Vec<Obs>, &'a LoggedSet)>,
}

/// One pass over `sets`: every dated lift day (any set; holds and ladder rungs
/// are resistance training too), sorted and unique, and the movements indexed
/// by id. A set dated after `today` is skipped; a planned deload week's set
/// keeps its training day and its name but never anchors an e1RM.
fn collect_movements<'a>(
    sets: &[(&'a LoggedSet, NameFacts)],
    ctx: &AnchorCtx,
    today: i64,
    offset: i64,
) -> (Vec<i64>, Vec<Movement<'a>>) {
    let mut movements: Vec<Movement> = Vec::new();
    let mut lift_days: Vec<i64> = Vec::new();
    for &(s, facts) in sets {
        let day = (!is_undated(s.observed_at)).then(|| local_day(s.observed_at, offset));
        if day.is_some_and(|d| d > today) {
            continue;
        }
        lift_days.extend(day);
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
        m.days.extend(day);
        if s.reps == 0
            || strength::effective_reps(s.reps, s.rpe) > strength::E1RM_RELIABLE_REP_CAP
            || facts.isolation
        {
            continue;
        }
        let deload = day.is_some_and(|d| ctx.deloads.contains(d));
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
        if deload {
            continue;
        }
        let e1_grid = if rated {
            let step = load_grid_step_kg(s.weight_kg, ctx.weight_lb);
            strength::e1rm_epley_rpe(s.weight_kg + step, s.reps, s.rpe)
        } else {
            e1
        };
        g.0.push(Obs {
            day,
            observed_at: s.observed_at,
            weight: s.weight_kg,
            e1,
            e1_grid,
            rated,
        });
    }
    lift_days.sort_unstable();
    lift_days.dedup();
    (lift_days, movements)
}

/// The declared layoff's re-entry ramp on `day` (REENTRY-RAMP-001). An
/// undatable return never advances (every session counts as pre-layoff).
fn ramp_at(weeks_off: f64, returned: Option<i64>, day: i64) -> individualization::ReentryRamp {
    individualization::reentry_ramp(weeks_off, returned.map_or(0, |r| day.saturating_sub(r))).value
}

/// An anchor and its working-load derate (fraction, cause), if any.
type Picked = (f64, Option<(f64, DerateCause)>);

/// Detraining credit from the last training day `last`, else the e1RM-drop
/// cut (one derate, never stacked; past 8 weeks the anchor is dropped).
fn finish(w: WindowResult, last: Option<i64>, today: i64) -> Option<Picked> {
    let days_since = last.map(|d| today.saturating_sub(d));
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
}

/// Score one movement's eligible sets (sorted by day) under the re-entry that
/// `governing` names: its anchor and derate (`None`: no anchor).
fn score_movement(
    obs: &[Obs],
    days: &[i64],
    governing: Option<(Layoff, individualization::ReentryRamp)>,
    off_day: Option<i64>,
    today: i64,
) -> Option<Picked> {
    let last_day = days.last().copied();
    let same_day = |d: i64| d;
    let full = |_: i64| 1.0;
    let all: Vec<&Obs> = obs.iter().collect();
    let Some((l, today_ramp)) = governing else {
        return score_sessions(&all, off_day, &same_day, &full)
            .and_then(|w| finish(w, last_day, today));
    };
    let returned = l.returned_epoch_day;
    let is_pre = |o: &Obs| returned.is_none_or(|rd| o.day.is_none_or(|d| d < rd));
    if today_ramp.treat_as_novice() {
        // >8 wk: the KB gives no fraction and directs a fresh, novice-style
        // start, so pre-layoff anchors are set aside; a movement logged since
        // the return anchors normally.
        let post: Vec<&Obs> = obs.iter().filter(|o| !is_pre(o)).collect();
        return score_sessions(&post, off_day, &same_day, &full)
            .and_then(|w| finish(w, last_day, today));
    }
    // ≤8 wk: the break itself never ages a pre-layoff anchor: pre-layoff
    // sessions move forward by the break's length (the last one lands on the
    // return day, the ones before keep their spacing, so a best that had
    // already left the window stays out); the ramp replaces that decay.
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
    score_sessions(&all, off_day, &eff, &frac_at).and_then(|w| {
        let frac = today_ramp.frac.unwrap_or(1.0);
        if today_ramp.active && frac < 1.0 {
            // A post-return RATED estimate above the ramped load is used
            // unscaled (it already reflects the break).
            let post_rated = obs
                .iter()
                .filter(|o| {
                    !is_pre(o)
                        && o.rated
                        && o.day
                            .is_some_and(|d| today.saturating_sub(d) <= WINDOW_DAYS)
                })
                .map(|o| o.e1)
                .fold(0.0_f64, f64::max);
            let f = (w.anchor * frac).max(post_rated) / w.anchor;
            let cause = reentry_cause(returned, today, today_ramp.ramp_days);
            Some((w.anchor, (f < 1.0).then_some((f, cause))))
        } else {
            finish(w, last_day.map(|d| returned.map_or(d, |r| d.max(r))), today)
        }
    })
}

/// Day `day` of a `ramp_days` re-entry ramp, counted from the return day
/// (0 when the return is undatable).
fn reentry_cause(returned: Option<i64>, today: i64, ramp_days: i64) -> DerateCause {
    DerateCause::Reentry {
        day: returned.map_or(0, |r| today.saturating_sub(r).max(0)),
        ramp_days,
    }
}

/// A working-load derate of `exercise`, the fraction rounded to 0.001.
fn lift_derate(exercise: &str, frac: f64, cause: DerateCause) -> LiftDerate {
    LiftDerate {
        exercise: exercise.to_owned(),
        frac: (frac * 1000.0).round() / 1000.0,
        cause,
    }
}

/// Derates of the carried last working loads of lifts with no e1RM anchor
/// (isolation, high-rep; the hypertrophy double progression and the
/// start-weight suggestion). A load lifted before a break scales by the same
/// re-entry ramp (REENTRY-RAMP-001: only numbers set before the break);
/// otherwise 31–56 days without the movement scale it by the detraining
/// credit, as an anchored lift's [`finish`] does (DETRAIN-STR-001), so more
/// time off never carries a heavier load. `trained_days` holds the movements
/// not anchored.
fn last_working_derates(
    last_working: &[crate::plan::LastWorking],
    trained_days: &mut HashMap<Vec<String>, Vec<i64>>,
    governing: impl Fn(&[i64]) -> Option<(Layoff, individualization::ReentryRamp)>,
    today: i64,
) -> Vec<LiftDerate> {
    let mut derates = Vec::new();
    for lw in last_working {
        let Some(mut days) = trained_days.remove(&crate::plan::movement_key(&lw.exercise)) else {
            continue;
        };
        days.sort_unstable();
        days.dedup();
        let governed = governing(&days);
        if let Some((l, ramp)) = governed {
            let lifted = today.saturating_sub(lw.age_days);
            let pre_break = l.returned_epoch_day.is_none_or(|r| lifted < r);
            if let Some(frac) = ramp.frac.filter(|f| pre_break && ramp.active && *f < 1.0) {
                let cause = reentry_cause(l.returned_epoch_day, today, ramp.ramp_days);
                derates.push(lift_derate(&lw.exercise, frac, cause));
                continue;
            }
        }
        // A declared return day restarts the count, as in `finish`.
        let returned = governed.and_then(|(l, _)| l.returned_epoch_day);
        let Some(days_since) = days
            .last()
            .map(|&d| today.saturating_sub(returned.map_or(d, |r| d.max(r))))
        else {
            continue;
        };
        if let Some(credit) = strength::lift_detrain_credit(days_since)
            .value
            .filter(|c| *c < 1.0)
        {
            derates.push(lift_derate(
                &lw.exercise,
                credit,
                DerateCause::Detraining { days_since },
            ));
        }
    }
    derates
}

/// Each movement's LAST logged session's working sets (hypertrophy double
/// progression, HYP-DOUBLEPROG-001), most-recent-first then by name. Per
/// movement ([`NameFacts::movement`]), the newest dated session day is taken;
/// its working load is that day's heaviest weight and its working sets are
/// every set logged at that load (lighter warm-ups / back-offs are not
/// judged). A bodyweight movement keeps load 0 (rep progression only). Holds
/// and, on a Calisthenics profile, ladder rungs are skipped (they have their
/// own progression), and so are a planned deload week's sets: the deload is
/// cut from the working load and the next block resumes from it
/// (HYP-DELOAD-RX-001, 03-hypertrophy.md:395; HYP-DOUBLEPROG-001, :361-362),
/// so a deload session never becomes the load the next one is cut from or
/// progresses from. A movement untrained for more than
/// [`WINDOW_HORIZON_DAYS`] drops out, the horizon at which an untrained e1RM
/// anchor is dropped (DETRAIN-STR-001).
pub(super) fn build_last_working(
    sets: &[(&LoggedSet, NameFacts)],
    ctx: &AnchorCtx,
    today_epoch_day: i64,
    offset: i64,
) -> Vec<crate::plan::LastWorking> {
    // movement → (newest day, sets on that day)
    let mut newest: HashMap<u32, (i64, Vec<&LoggedSet>)> = HashMap::new();
    for &(s, facts) in sets {
        if is_undated(s.observed_at) || s.hold_sec.is_some() || s.weight_kg < 0.0 || s.reps == 0 {
            continue;
        }
        let day = local_day(s.observed_at, offset);
        if day > today_epoch_day
            || today_epoch_day.saturating_sub(day) > WINDOW_HORIZON_DAYS
            || (ctx.calisthenics && facts.calis_tier.is_some())
            || ctx.deloads.contains(day)
        {
            continue;
        }
        let e = newest.entry(facts.movement).or_insert((day, Vec::new()));
        if day > e.0 {
            *e = (day, Vec::new());
        }
        if day == e.0 {
            e.1.push(s);
        }
    }
    let mut out: Vec<(crate::plan::LastWorking, i64)> = newest
        .into_values()
        .map(|(day, mut day_sets)| {
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
            let last = day_sets[day_sets.len() - 1];
            (
                crate::plan::LastWorking {
                    exercise: last.exercise.clone(),
                    // A -0.0 kg set passes the ingest gate; `+ 0.0` reads it as 0.
                    weight_kg: top + 0.0,
                    sets: working,
                    age_days: today_epoch_day - day,
                },
                last.observed_at,
            )
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
        if is_undated(r.observed_at) {
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

/// Run-history anchors for plan synthesis ([`RunAnchors`]), the run twin of
/// [`build_plan_anchors`]. The "now" is the shell-supplied `today_epoch_day`,
/// and runs are bucketed by the LOCAL day they STARTED (`RunFacts::started`
/// shifted by `utc_offset_sec`), so a run started earlier today counts and a
/// future-dated one never does. The longest run uses the RUN-SPIKE-001 window,
/// then tapers over [`RUN_DETRAIN_TAPER_DAYS`] (DETRAIN-001); the weekly volume
/// feeds the RUN-LONGRUN-001 share rule. The taper is a planning-capacity
/// concept only: the log-time spike gate keeps its hard 30-day window.
pub(super) fn build_run_anchors(
    facts: &RunFacts,
    today_epoch_day: i64,
    utc_offset_sec: i64,
) -> RunAnchors {
    // An in-window run counts at full credit, so an older, decayed run only
    // wins when it was longer than every in-window run.
    let longest = longest_credited(
        facts
            .started
            .iter()
            .zip(&facts.km)
            .map(|(&s, &km)| (crate::local_day(s, utc_offset_sec), km)),
        today_epoch_day,
    );
    RunAnchors {
        longest_recent_run_km: longest.map(|(km, _)| km),
        recent_weekly_km: recent_weekly_km(facts, today_epoch_day, utc_offset_sec),
        longest_run_detrained: longest.is_some_and(|(_, d)| d),
    }
}

/// The measured weekly running volume as of local day `day`: the km of the
/// trailing [`WEEKLY_VOLUME_WINDOW_DAYS`] divided by the weeks that window
/// actually spans since its first run (1–4), so a single recent week of
/// running is not diluted by empty weeks into a phantom low average. `None`
/// with no run in the window.
fn recent_weekly_km(facts: &RunFacts, day: i64, utc_offset_sec: i64) -> Option<f64> {
    let mut total_km = 0.0;
    let mut first_run_day: Option<i64> = None;
    for &i in &facts.chrono {
        let km = facts.km[i];
        let d = crate::local_day(facts.started[i], utc_offset_sec);
        if d <= day && day.saturating_sub(d) < WEEKLY_VOLUME_WINDOW_DAYS {
            total_km += km;
            first_run_day = Some(first_run_day.map_or(d, |f| f.min(d)));
        }
    }
    let max_weeks = WEEKLY_VOLUME_WINDOW_DAYS / 7;
    (total_km > 0.0).then(|| {
        let weeks = first_run_day.map_or(max_weeks, |first| {
            (((day.saturating_sub(first) + 1) as f64 / 7.0).ceil() as i64).clamp(1, max_weeks)
        });
        total_km / weeks as f64
    })
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
        .filter(|(r, _)| !is_undated(r.observed_at))
        .map(|(_, &start)| local_day(start))
        .filter(|d| *d <= day)
        .collect();
    days.sort_unstable();
    days.dedup();
    let (last_run, back) = latest_break(&days, day)?;
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
    // Longest run since the return, credited like `build_run_anchors`' longest
    // recent run.
    let longest = longest_credited(
        runs.iter()
            .zip(&facts.started)
            .zip(&facts.km)
            .filter(|((r, _), _)| !is_undated(r.observed_at))
            .map(|((_, &s), &km)| (local_day(s), km))
            .filter(|&(d, _)| d >= back),
        day,
    );
    Some(crate::plan::RunBreak {
        weeks_off,
        last_run_epoch_day: last_run,
        prior_weekly_km: recent_weekly_km(facts, last_run, utc_offset_sec),
        reentry,
        longest_since_km: longest.map(|(km, _)| km),
        longest_since_detrained: longest.is_some_and(|(_, d)| d),
        stated_longest_after: false,
    })
}

/// The longest of `runs` (local start day, km) on or before `today` inside the
/// decay horizon, each credited by its age ([`detrain_credit`]), and whether
/// it is older than the full-credit window. `None` when no run carries credit.
fn longest_credited(runs: impl Iterator<Item = (i64, f64)>, today: i64) -> Option<(f64, bool)> {
    let mut best = 0.0_f64;
    let mut detrained = false;
    for (d, km) in runs {
        let age = today.saturating_sub(d);
        // A future-dated row never counts, like `spike_baseline_km`.
        if d > today || age >= RUN_DECAY_END_DAYS {
            continue;
        }
        let eff = km * detrain_credit(age);
        if eff > best {
            best = eff;
            detrained = age > LONGEST_RUN_WINDOW_DAYS;
        }
    }
    (best > 0.0).then_some((best, detrained))
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
