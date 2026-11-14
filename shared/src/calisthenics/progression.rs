//! Calisthenics ladder progression: each ladder's rung REPLAYED in time order
//! from logged sets and rung placements, and the anchor the plan prescribes
//! from (KB calis-progression §2.2).

use super::{GateRung, Ladder, Tier};
use crate::day::{DAY_SEC, is_undated, local_day};
use crate::schema::LadderId;
use crate::strength::LiftGoal;
use std::collections::BTreeMap;

/// A logged set as the rung replay reads it.
pub(crate) trait RungSet {
    fn weight_kg(&self) -> f64;
    fn reps(&self) -> u32;
    /// Logged RPE; `<= 0` = not rated.
    fn rpe(&self) -> f64;
    /// Timed-hold duration; `None` for a reps set.
    fn hold_sec(&self) -> Option<u16>;
    /// Log time, unix seconds; `<= 0` when undated.
    fn observed_at(&self) -> i64;
    /// The one log order of sets, never their storage slot.
    fn chrono_cmp(&self, other: &Self) -> std::cmp::Ordering;
}

/// A calisthenics rung placement (`Event::SetCalisthenicsRung`): the athlete
/// chose this rung for a ladder, at setup (their starting rung) or via the
/// plan item's Easier / Harder choice. Replayed with the logged sets, in time
/// order, by [`build_anchors`].
// `Serialize`: the plan-cache fingerprint (`memo::fingerprint`) only.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub(crate) struct CalisPlacement {
    pub(crate) ladder: LadderId,
    pub(crate) rung: u8,
    pub(crate) observed_at: i64,
}

/// Derived tier state for one calisthenics ladder, replayed chronologically from
/// logged sets and rung placements: a state machine, not a monotone max (KB
/// calis-progression §2.2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CalisAnchor {
    pub ladder: LadderId,
    /// Current rung: where the athlete's newest accepted session (or rung
    /// placement) put this ladder. A harder rung logged once without reaching
    /// its band floor is an exploratory attempt and does not move it.
    pub tier: u8,
    /// Trigger fired → the plan prescribes `tier + advance_steps` at the band
    /// floor, or the next added-load step on a loaded weighted rung (subject to
    /// the tendon rate-limit on straight-arm rungs and the G8 pain suppression).
    pub advance_ready: bool,
    /// Rungs an advance moves: 1 normally, `calisthenics::SKIP_STEPS` when the
    /// newest session beat the CAL-PLACE-001 skip threshold.
    pub advance_steps: u8,
    /// Floor missed on the 2 newest sessions (or on the first session after a
    /// step up / placement) → the plan prescribes the rung below.
    pub regress: bool,
    /// The current rung was entered by a regress: it carries the tempo bridge
    /// (CAL-PROG-TEMPO-001) until the athlete steps back up.
    pub bridged: bool,
    /// Newest logged `weight_kg` at the current rung (0.0 default; the user's
    /// own last load, never invented).
    pub last_added_kg: f64,
    /// Skill tracks: the user has logged a rung AND the ladder's count gate (the
    /// muscle-up's G1) is met. Dynamic ladders are always active. A dormant
    /// skill track is not scheduled; an inactive muscle-up emits a guidance row
    /// instead.
    pub active: bool,
    /// The newest session met the advance target except that its last set was
    /// not rated: advance waits for an effort rating.
    pub needs_effort_rating: bool,
    /// Rungs to step down for a break from this ladder (CAL-REENTRY-001).
    pub reentry_steps: u8,
    /// The break exceeds 8 weeks: re-test the starting rung.
    pub retest: bool,
    /// The rung comes from a placement with no sets logged since.
    pub placed: bool,
    /// Newest loaded set at a weighted rung, `(rung, added_kg, reps, rpe)`, for
    /// the system-load e1RM line (CAL-E1RM-001).
    pub last_loaded_set: Option<(u8, f64, u32, f64)>,
}

/// How the current rung of a ladder was entered, while replaying its history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entry {
    /// First logged rung, a self-chosen rung, or a re-entry step.
    Logged,
    /// A rung placement (`Event::SetCalisthenicsRung`).
    Placed,
    /// The plan's own step up (advance or CAL-PLACE-001 skip).
    Advanced,
    /// The plan's own step down after missed floors (carries the tempo bridge).
    Regressed,
}

/// One ladder's replay state: the current rung and the sessions logged at it.
struct State<'s, S> {
    rung: u8,
    entry: Entry,
    /// (local day, sets at `rung` that day), chronological.
    days: Vec<(i64, Vec<&'s S>)>,
    /// Start of the acclimation span at this rung (unix seconds).
    since_ts: i64,
    /// Local day of the newest dated session (or placement) on this ladder.
    last_day: Option<i64>,
}

/// What the sessions at the current rung say.
struct Verdict {
    advance: bool,
    steps: u8,
    regress: bool,
    needs_rating: bool,
    loaded: bool,
}

/// CAL-RIR-ACC-001: a rated set qualifies at RIR ≥ 1, i.e. at or under this RPE.
const MAX_QUALIFYING_RPE: f64 = 9.0;
/// CAL-RIR-ACC-001: an unrated set qualifies only when it beats the band top by
/// this many reps (or hold seconds).
const UNRATED_MARGIN: u32 = 2;

fn metric<S: RungSet>(timed: bool, s: &S) -> u32 {
    if timed {
        u32::from(s.hold_sec().unwrap_or(0))
    } else {
        s.reps()
    }
}

/// Judge the sessions logged at one rung (CAL-PROG-TRIGGER-001 2-for-2 at the
/// band top, over test sessions on a straight-arm rung; CAL-PLACE-001
/// one-session skip far over the top; regress on missed floors on 2 sessions,
/// or on the first session after a placement). The effort test follows
/// CAL-RIR-ACC-001: high-rep, hold and ballistic rungs advance on reps/hold
/// alone; other rungs need the last set rated at [`MAX_QUALIFYING_RPE`] or
/// under; an unrated set only counts when it beats the top by
/// [`UNRATED_MARGIN`].
fn judge_rung<S: RungSet>(
    l: &Ladder,
    tier: &Tier,
    days: &[(i64, Vec<&S>)],
    entry: Entry,
    goal: LiftGoal,
) -> Verdict {
    let loaded = days
        .last()
        .and_then(|(_, ss)| ss.last())
        .is_some_and(|s| s.weight_kg() > 0.0);
    let top = super::advance_top(tier, goal, loaded) as u32;
    let target = super::working_target(tier, goal, loaded);
    let floor = target.floor() as u32;
    let timed = tier.target.is_timed();
    let val = |s: &S| metric(timed, s);
    let perf_only = super::performance_gated(tier, target, loaded);
    let effort_ok = |s: &S| {
        perf_only
            || (s.rpe() > 0.0 && s.rpe() <= MAX_QUALIFYING_RPE)
            || (s.rpe() <= 0.0 && val(s) >= top + UNRATED_MARGIN)
    };
    // (qualifies, the only blocker is an unrated set)
    let qualifies = |ss: &[&S]| -> (bool, bool) {
        if l.skill {
            let hits: Vec<&&S> = ss.iter().filter(|s| val(s) >= top).collect();
            if hits.iter().any(|s| effort_ok(s)) {
                (true, false)
            } else {
                (false, hits.iter().any(|s| s.rpe() <= 0.0))
            }
        } else {
            if ss.len() < 3 || !ss.iter().all(|s| val(s) >= top) {
                return (false, false);
            }
            let last = ss[ss.len() - 1];
            if effort_ok(last) {
                (true, false)
            } else {
                (false, last.rpe() <= 0.0)
            }
        }
    };
    // A missed floor (KB §2.2 "cannot complete the floor of the band, e.g.
    // <3×5"): ANY dynamic set under the floor misses 3×5; a skill session
    // misses only when no hold or rep reached it.
    let below_floor = |ss: &[&S]| {
        !ss.is_empty()
            && if l.skill {
                ss.iter().all(|s| val(s) < floor)
            } else {
                ss.iter().any(|s| val(s) < floor)
            }
    };
    let n = days.len();
    // A straight-arm rung advances on its test hold, done once a week apart
    // from the working holds (which stay below it), so "2 in a row" runs over
    // TEST sessions only: a day with a hold past the working band is a test
    // (a missed test breaks the run; plain working days neither count nor break it).
    let work_top = target.top() as u32;
    let judged: Vec<&[&S]> = days
        .iter()
        .map(|(_, ss)| ss.as_slice())
        .filter(|ss| super::test_hold_sec(tier).is_none() || ss.iter().any(|s| val(s) > work_top))
        .collect();
    let m = judged.len();
    let (q_new, rating_new) = judged.last().map_or((false, false), |ss| qualifies(ss));
    let q_prev = m >= 2 && qualifies(judged[m - 2]).0;
    let skip = !loaded
        && super::skip_threshold(tier, goal).value.is_some_and(|th| {
            days.last()
                .is_some_and(|(_, ss)| !ss.is_empty() && ss.iter().all(|s| val(s) >= th as u32))
        });
    let consecutive = match (q_new, q_prev) {
        (true, true) => 2,
        (true, false) => 1,
        _ => 0,
    };
    let advance = skip || super::tier_advance_met(consecutive).value;
    let new_below = days.last().is_some_and(|(_, ss)| below_floor(ss));
    // Only a placement is judged on one session (CAL-PLACE-001 mis-placement);
    // the plan's own step up gets the KB's 2 sessions at the new rung (§2.2).
    let fresh = entry == Entry::Placed;
    let regress =
        !advance && new_below && ((n >= 2 && below_floor(&days[n - 2].1)) || (fresh && n == 1));
    Verdict {
        advance,
        steps: if skip { super::SKIP_STEPS } else { 1 },
        regress,
        needs_rating: !advance && rating_new,
        loaded,
    }
}

/// Straight-arm tendon rate-limit (CAL-TENDON-001): the span from the
/// acclimation start to the newest session at the rung must reach
/// [`super::STRAIGHT_ARM_ACCLIMATION_DAYS`]. An unstarted clock (no dated
/// session at the rung yet) never counts as acclimated.
fn acclimated<S: RungSet>(state: &State<S>) -> bool {
    if is_undated(state.since_ts) {
        return false;
    }
    let newest = state
        .days
        .last()
        .and_then(|(_, ss)| ss.iter().map(|s| s.observed_at()).max())
        .unwrap_or(state.since_ts);
    super::static_acclimation_ok(newest.saturating_sub(state.since_ts).div_euclid(DAY_SEC)).value
}

/// Whether the verdict's advance stands: a straight-arm rung also needs its
/// tendon acclimation span (CAL-TENDON-001).
fn advance_met<S: RungSet>(v: &Verdict, tier: &Tier, cur: &State<S>) -> bool {
    v.advance && (!tier.straight_arm || acclimated(cur))
}

/// Best single-set reps logged at a count-gate rung or harder on its ladder.
fn gate_best<S: RungSet>(matched: &[(LadderId, u8, &S)], g: &GateRung) -> u8 {
    matched
        .iter()
        .filter(|(l, r, _)| *l == g.ladder && *r >= g.rung)
        .map(|(_, _, s)| s.reps().min(u8::MAX as u32) as u8)
        .max()
        .unwrap_or(0)
}

/// Per-ladder calisthenics tier state (the calisthenics analogue of the lift
/// anchors), REPLAYED in time order from logged sets and rung placements: a
/// state machine, not a monotone max (KB calis-progression §2.2):
///
/// - each session's rung is the rung with the most sets that local day (ties →
///   the easier rung); a set name matches by `calisthenics::tier_by_name`,
///   aliases included ("Pull-ups" → Full pull-up);
/// - a session at the current rung, an easier rung, or the rung the plan would
///   have prescribed (advance / skip / regress / re-entry) moves the ladder
///   there; a HARDER rung moves it only when a set reached that rung's band
///   floor (self-placement, CAL-PLACE-001), so one failed exploratory attempt
///   is ignored;
/// - a placement sets the rung outright from its time on, in timestamp order
///   even inside a session (sets logged after an Easier / Harder choice count
///   at the chosen rung); an undated placement starts the straight-arm
///   acclimation clock at the first dated session on it, never at the epoch;
/// - a break of 2+ weeks before a session restarts a straight-arm rung's
///   acclimation window (CAL-REENTRY-001), and a break up to `through_day`
///   steps the prescribed rung down (`reentry_steps`);
/// - `pain_present` (an active gating pain report, File 08 Table 4.1) freezes
///   progression on every ladder (CAL-GATE-NUMBERS-001); DOMS never does.
///
/// A ladder with no set and no placement produces no anchor (the plan starts it
/// at the easiest available rung); a skill track is dormant until a rung is
/// logged or placed. `placements` must already be filtered to the day being
/// prescribed. `through_day` is the last local day of history the anchors see
/// (the prescribed day is the next one); `None` disables the re-entry step.
///
/// Deterministic: "sessions" are distinct LOCAL calendar days (`observed_at`
/// shifted by `utc_offset_sec`); spans come from `observed_at`, never a clock.
/// `sets` is an ITERATOR so the caller can pass a date-filtered history slice;
/// each set comes with its name's rung ([`super::tier_by_name`]), which the
/// caller resolves once per distinct name.
pub(crate) fn build_anchors<'a, S: RungSet + 'a>(
    sets: impl Iterator<Item = (&'a S, Option<(LadderId, u8)>)>,
    placements: &[CalisPlacement],
    pain_present: bool,
    goal: LiftGoal,
    through_day: Option<i64>,
    utc_offset_sec: i64,
) -> Vec<CalisAnchor> {
    let day_of = |t: i64| local_day(t, utc_offset_sec);

    let matched: Vec<(LadderId, u8, &S)> = sets
        .filter_map(|(s, tier)| tier.map(|(l, r)| (l, r, s)))
        .collect();

    let mut out = Vec::new();
    for l in super::LADDERS {
        let mut by_day: BTreeMap<i64, Vec<(u8, &S)>> = BTreeMap::new();
        for (lid, r, s) in &matched {
            if *lid == l.id {
                by_day
                    .entry(day_of(s.observed_at()))
                    .or_default()
                    .push((*r, *s));
            }
        }
        let mut places: Vec<&CalisPlacement> =
            placements.iter().filter(|p| p.ladder == l.id).collect();
        if by_day.is_empty() && places.is_empty() {
            continue;
        }
        // Log order, never storage order: the day's last set is the
        // prescribed added load, and an Undo or amend moves a row to the tail.
        for v in by_day.values_mut() {
            v.sort_by(|(_, a), (_, b)| a.chrono_cmp(b));
        }
        places.sort_by_key(|p| p.observed_at);
        let sessions = split_at_placements(&by_day, &places);
        let Some(cur) = replay_ladder(l, &sessions, &places, goal, day_of) else {
            continue;
        };

        let tier = &l.tiers[cur.rung as usize];
        let v = judge_rung(l, tier, &cur.days, cur.entry, goal);
        // A break up to the prescribed day steps the rung down (CAL-REENTRY-001).
        let gap = match (through_day, cur.last_day) {
            (Some(t), Some(d)) => t.saturating_add(1).saturating_sub(d),
            _ => 0,
        };
        let re = super::reentry(gap).value;
        let advance_ready = advance_met(&v, tier, &cur) && !pain_present && re.steps == 0;
        let regress = v.regress && re.steps == 0;

        // Newest added load at the current rung (the user's own, never invented).
        let last_added_kg = cur
            .days
            .last()
            .and_then(|(_, ss)| ss.last())
            .map_or(0.0, |s| s.weight_kg().max(0.0));
        let last_loaded_set = matched
            .iter()
            .filter(|(lid, r, s)| {
                *lid == l.id && super::weighted_ok(l.id, *r) && s.weight_kg() > 0.0
            })
            .max_by(|(_, _, a), (_, _, b)| a.chrono_cmp(b))
            .map(|(_, r, s)| (*r, s.weight_kg(), s.reps(), s.rpe()));

        // Active: dynamic ladders always; skill tracks once logged, but a
        // gated track (muscle-up) only when its count gate is met (else a
        // guidance row, not a plan item).
        let active = l.gate.is_empty()
            || super::count_gate_met(l.gate.iter().map(|g| gate_best(&matched, g))).value;

        out.push(CalisAnchor {
            ladder: l.id,
            tier: cur.rung,
            advance_ready,
            advance_steps: v.steps,
            regress,
            bridged: cur.entry == Entry::Regressed,
            last_added_kg,
            active,
            needs_effort_rating: v.needs_rating && re.steps == 0 && !pain_present,
            reentry_steps: re.steps,
            retest: re.retest,
            placed: cur.entry == Entry::Placed && cur.days.is_empty(),
            last_loaded_set,
        });
    }
    out
}

/// Each day's sets split at any placement that falls between two of them, so
/// a placement applies in timestamp order: sets before it are judged at the
/// old rung, sets after it are the placed rung's first session.
fn split_at_placements<'s, S: RungSet>(
    by_day: &BTreeMap<i64, Vec<(u8, &'s S)>>,
    places: &[&CalisPlacement],
) -> Vec<(i64, Vec<(u8, &'s S)>)> {
    let mut sessions = Vec::new();
    for (&day, day_sets) in by_day {
        let mut chunk: Vec<(u8, &S)> = Vec::new();
        for &(r, s) in day_sets {
            if let Some(&(_, prev)) = chunk.last()
                && places
                    .iter()
                    .any(|p| p.observed_at >= prev.observed_at() && p.observed_at < s.observed_at())
            {
                sessions.push((day, std::mem::take(&mut chunk)));
            }
            chunk.push((r, s));
        }
        sessions.push((day, chunk));
    }
    sessions
}

/// One ladder's state after replaying its `sessions` and sorted `places` in
/// time order (the rules in [`build_anchors`]); `None` with neither.
fn replay_ladder<'s, S: RungSet>(
    l: &Ladder,
    sessions: &[(i64, Vec<(u8, &'s S)>)],
    places: &[&CalisPlacement],
    goal: LiftGoal,
    day_of: impl Fn(i64) -> i64,
) -> Option<State<'s, S>> {
    let n_rungs = l.tiers.len() as u8;
    let place = |p: &CalisPlacement| State {
        rung: p.rung.min(n_rungs - 1),
        entry: Entry::Placed,
        days: Vec::new(),
        // `<= 0` (undated) = the acclimation clock is not started yet.
        since_ts: p.observed_at,
        // A placement is a fresh start: it re-dates the break clock.
        last_day: (!is_undated(p.observed_at)).then(|| day_of(p.observed_at)),
    };
    let mut st: Option<State<S>> = None;
    let mut pi = 0;
    for (day, day_sets) in sessions {
        let day = *day;
        let first_ts = day_sets[0].1.observed_at();
        while pi < places.len() && places[pi].observed_at < first_ts {
            st = Some(place(places[pi]));
            pi += 1;
        }
        let mut counts: BTreeMap<u8, usize> = BTreeMap::new();
        for (r, _) in day_sets {
            *counts.entry(*r).or_default() += 1;
        }
        let r = counts
            .iter()
            .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
            .map_or(0, |(r, _)| *r)
            .min(n_rungs - 1);
        let r_tier = &l.tiers[r as usize];
        let timed = r_tier.target.is_timed();
        let at_r: Vec<&S> = day_sets
            .iter()
            .filter(|(rr, _)| *rr == r)
            .map(|(_, s)| *s)
            // A timed rung logged without a hold duration carries no metric:
            // neutral (skipped), never a failed 0-second hold.
            .filter(|s| !(timed && s.hold_sec().is_none()))
            .collect();
        let r_ts = at_r.first().map_or(first_ts, |s| s.observed_at());
        let dated = !is_undated(first_ts);
        let cur = match st.as_mut() {
            Some(cur) => cur,
            None => {
                st = Some(State {
                    rung: r,
                    entry: Entry::Logged,
                    days: if at_r.is_empty() {
                        Vec::new()
                    } else {
                        vec![(day, at_r)]
                    },
                    since_ts: r_ts,
                    last_day: dated.then_some(day),
                });
                continue;
            }
        };
        if at_r.is_empty() {
            // Only hold-less sets: a training day for the break clock, never
            // a session at the rung.
            if dated {
                cur.last_day = Some(day);
            }
            continue;
        }
        let gap = cur.last_day.filter(|_| dated).map_or(0, |d| day - d);
        let re = super::reentry(gap).value;
        let cur_tier = &l.tiers[cur.rung as usize];
        let v = judge_rung(l, cur_tier, &cur.days, cur.entry, goal);
        let expected = if re.steps > 0 {
            cur.rung.saturating_sub(re.steps)
        } else if advance_met(&v, cur_tier, cur) && !super::weighted_ok(l.id, cur.rung) {
            cur.rung.saturating_add(v.steps).min(n_rungs - 1)
        } else if v.regress && !v.loaded {
            cur.rung.saturating_sub(1)
        } else {
            cur.rung
        };
        let floor_met = {
            let loaded = at_r.last().is_some_and(|s| s.weight_kg() > 0.0);
            let floor = super::working_target(r_tier, goal, loaded).floor() as u32;
            at_r.iter().any(|s| metric(timed, *s) >= floor)
        };
        if r <= cur.rung.max(expected) || floor_met {
            if r == cur.rung {
                if re.steps > 0 && cur_tier.straight_arm {
                    // CAL-REENTRY-001: a straight-arm rung restarts its
                    // tendon acclimation window after a break.
                    cur.since_ts = r_ts;
                }
                if is_undated(cur.since_ts) {
                    // First dated session after an undated placement.
                    cur.since_ts = r_ts;
                }
                cur.days.push((day, at_r));
            } else {
                let entry = if r == expected && r > cur.rung {
                    Entry::Advanced
                } else if r == expected && r < cur.rung && v.regress && re.steps == 0 {
                    Entry::Regressed
                } else {
                    Entry::Logged
                };
                cur.rung = r;
                cur.entry = entry;
                cur.days = vec![(day, at_r)];
                cur.since_ts = r_ts;
            }
        }
        // Every session (accepted or an ignored exploratory attempt) is a
        // training day on this ladder for the break clock.
        if dated {
            cur.last_day = Some(day);
        }
    }
    while pi < places.len() {
        st = Some(place(places[pi]));
        pi += 1;
    }
    st
}
