//! Calisthenics ladder anchors, replayed from logged sets and rung placements.

use super::*;

/// How the current rung of a ladder was entered, while replaying its history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CalisEntry {
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
struct CalisState<'s> {
    rung: u8,
    entry: CalisEntry,
    /// (local day, sets at `rung` that day), chronological.
    days: Vec<(i64, Vec<&'s LoggedSet>)>,
    /// Start of the acclimation span at this rung (unix seconds).
    since_ts: i64,
    /// Local day of the newest dated session (or placement) on this ladder.
    last_day: Option<i64>,
}

/// What the sessions at the current rung say.
struct CalisVerdict {
    advance: bool,
    steps: u8,
    regress: bool,
    needs_rating: bool,
    loaded: bool,
}

/// CAL-RIR-ACC-001: a rated set qualifies at RIR ≥ 1, i.e. at or under this RPE.
const CALIS_MAX_QUALIFYING_RPE: f64 = 9.0;
/// CAL-RIR-ACC-001: an unrated set qualifies only when it beats the band top by
/// this many reps (or hold seconds).
const CALIS_UNRATED_MARGIN: u32 = 2;

/// The pull-up rung (Full pull-up) and dip rung (Full parallel-bar dip) from
/// which a set's reps count toward the muscle-up count gate
/// (`calisthenics::muscle_up_gate_met`).
const MUSCLE_UP_GATE_PULL_RUNG: u8 = 4;
const MUSCLE_UP_GATE_DIP_RUNG: u8 = 3;

/// Judge the sessions logged at one rung (CAL-PROG-TRIGGER-001 2-for-2 at the
/// band top, over test sessions on a straight-arm rung; CAL-PLACE-001
/// one-session skip far over the top; regress on missed floors on 2 sessions,
/// or on the first session after a placement). The effort test follows
/// CAL-RIR-ACC-001: high-rep, hold and ballistic rungs advance on reps/hold
/// alone; other rungs need the last set rated at [`CALIS_MAX_QUALIFYING_RPE`]
/// or under — an unrated set only counts when it beats the top by
/// [`CALIS_UNRATED_MARGIN`].
fn judge_calis_rung(
    l: &crate::calisthenics::Ladder,
    tier: &crate::calisthenics::Tier,
    days: &[(i64, Vec<&LoggedSet>)],
    entry: CalisEntry,
    goal: LiftGoal,
) -> CalisVerdict {
    use crate::calisthenics as cal;
    let loaded = days
        .last()
        .and_then(|(_, ss)| ss.last())
        .is_some_and(|s| s.weight_kg > 0.0);
    let top = cal::advance_top(tier, goal, loaded) as u32;
    let floor = cal::working_target(tier, goal, loaded).floor() as u32;
    let timed = tier.target.is_timed();
    let val = |s: &LoggedSet| -> u32 {
        if timed {
            s.hold_sec.unwrap_or(0) as u32
        } else {
            s.reps
        }
    };
    let perf_only = cal::performance_gated(tier, cal::working_target(tier, goal, loaded), loaded);
    let effort_ok = |s: &LoggedSet| {
        perf_only
            || (s.rpe > 0.0 && s.rpe <= CALIS_MAX_QUALIFYING_RPE)
            || (s.rpe <= 0.0 && val(s) >= top + CALIS_UNRATED_MARGIN)
    };
    // (qualifies, the only blocker is an unrated set)
    let qualifies = |ss: &[&LoggedSet]| -> (bool, bool) {
        if l.skill {
            // Skill rung: one clean set at the trigger (test hold or reps).
            let hits: Vec<&&LoggedSet> = ss.iter().filter(|s| val(s) >= top).collect();
            if hits.iter().any(|s| effort_ok(s)) {
                (true, false)
            } else {
                (false, hits.iter().any(|s| s.rpe <= 0.0))
            }
        } else {
            // Dynamic rung: >= 3 sets, every one at the band top.
            if ss.len() < 3 || !ss.iter().all(|s| val(s) >= top) {
                return (false, false);
            }
            let last = ss[ss.len() - 1];
            if effort_ok(last) {
                (true, false)
            } else {
                (false, last.rpe <= 0.0)
            }
        }
    };
    // A missed floor (KB §2.2 "cannot complete the floor of the band, e.g.
    // <3×5"): ANY dynamic set under the floor misses 3×5; a skill session
    // misses only when no hold or rep reached it.
    let below_floor = |ss: &[&LoggedSet]| {
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
    let work_top = cal::working_target(tier, goal, loaded).top() as u32;
    let judged: Vec<&[&LoggedSet]> = days
        .iter()
        .map(|(_, ss)| ss.as_slice())
        .filter(|ss| cal::test_hold_sec(tier).is_none() || ss.iter().any(|s| val(s) > work_top))
        .collect();
    let m = judged.len();
    let (q_new, rating_new) = judged.last().map_or((false, false), |ss| qualifies(ss));
    let q_prev = m >= 2 && qualifies(judged[m - 2]).0;
    let skip = !loaded
        && cal::skip_threshold(tier, goal).value.is_some_and(|th| {
            days.last()
                .is_some_and(|(_, ss)| !ss.is_empty() && ss.iter().all(|s| val(s) >= th as u32))
        });
    let consecutive = match (q_new, q_prev) {
        (true, true) => 2,
        (true, false) => 1,
        _ => 0,
    };
    let advance = skip || cal::tier_advance_met(consecutive).value;
    let new_below = days.last().is_some_and(|(_, ss)| below_floor(ss));
    // Only a placement is judged on one session (CAL-PLACE-001 mis-placement);
    // the plan's own step up gets the KB's 2 sessions at the new rung (§2.2).
    let fresh = entry == CalisEntry::Placed;
    let regress =
        !advance && new_below && ((n >= 2 && below_floor(&days[n - 2].1)) || (fresh && n == 1));
    CalisVerdict {
        advance,
        steps: if skip { cal::SKIP_STEPS } else { 1 },
        regress,
        needs_rating: !advance && rating_new,
        loaded,
    }
}

/// Straight-arm tendon rate-limit (CAL-TENDON-001): the span from the
/// acclimation start to the newest session at the rung must reach 28 days. An
/// unstarted clock (no dated session at the rung yet) never counts as acclimated.
fn calis_acclimated(state: &CalisState) -> bool {
    if state.since_ts <= 0 {
        return false;
    }
    let newest = state
        .days
        .last()
        .and_then(|(_, ss)| ss.iter().map(|s| s.observed_at).max())
        .unwrap_or(state.since_ts);
    crate::calisthenics::static_acclimation_ok(
        newest.saturating_sub(state.since_ts).div_euclid(DAY_SEC),
    )
    .value
}

/// Per-ladder calisthenics tier state (the calisthenics analogue of
/// [`build_plan_anchors`]), REPLAYED in time order from logged sets and rung
/// placements — a state machine, not a monotone max (KB calis-progression §2.2):
///
/// - each session's rung is the rung with the most sets that local day (ties →
///   the easier rung); a set name matches by `calisthenics::tier_by_name`,
///   aliases included ("Pull-ups" → Full pull-up);
/// - a session at the current rung, an easier rung, or the rung the plan would
///   have prescribed (advance / skip / regress / re-entry) moves the ladder
///   there; a HARDER rung moves it only when a set reached that rung's band
///   floor (self-placement, CAL-PLACE-001) — one failed exploratory attempt is
///   ignored;
/// - a placement sets the rung outright from its time on, in timestamp order
///   even inside a session (sets logged after an Easier / Harder choice count
///   at the chosen rung); an undated placement starts the straight-arm
///   acclimation clock at the first dated session on it, never at the epoch;
/// - a break of 2+ weeks before a session restarts a straight-arm rung's
///   acclimation window (CAL-REENTRY-001), and a break up to `through_day`
///   steps the prescribed rung down (`reentry_steps`).
///
/// A ladder with no set and no placement produces no anchor (the plan starts it
/// at the easiest available rung); a skill track is dormant until a rung is
/// logged or placed. `placements` must already be filtered to the day being
/// prescribed. `through_day` is the last local day of history the anchors see
/// (the prescribed day is the next one); `None` disables the re-entry step.
///
/// Deterministic: "sessions" are distinct LOCAL calendar days (`observed_at`
/// shifted by `utc_offset_sec`); spans come from `observed_at`, never a clock.
/// `sets` is an ITERATOR so the caller can pass a date-filtered history slice.
pub(in crate::app) fn build_calisthenics_anchors<'a>(
    sets: impl Iterator<Item = &'a LoggedSet>,
    placements: &[CalisPlacement],
    inputs: &[ReadinessInput],
    goal: LiftGoal,
    through_day: Option<i64>,
    utc_offset_sec: i64,
) -> Vec<crate::plan::CalisAnchor> {
    use crate::calisthenics as cal;
    use crate::plan::CalisAnchor;
    use std::collections::BTreeMap;

    // An ACTIVE, gating pain report suppresses difficulty progression on every
    // ladder (never advance through joint/tendon pain, CAL-GATE-NUMBERS-001).
    // Keyed off the authoritative File 08 Table 4.1 gate
    // (`autoreg::pain_blocks_progression`), NOT a bare "any Pain signal ever":
    // the WORST active pain report decides, a `value 0` pain-clear re-opens advance,
    // and normal training soreness (`Doms`, Table 4.1 "continue") never freezes
    // it. The whole-session SafetyTier::Pain block still governs independently.
    let pain_present = crate::autoreg::pain_blocks_progression(inputs);
    let day_of = |t: i64| local_day(t, utc_offset_sec);

    // Match every logged set to (ladder, rung) once.
    let matched: Vec<(crate::schema::LadderId, u8, &LoggedSet)> = sets
        .filter_map(|s| cal::tier_by_name(&s.exercise).map(|(l, r)| (l, r, s)))
        .collect();

    // Muscle-up gate bests: max single-set reps ever at Full pull-up+ and Full
    // parallel-bar dip+.
    let best_reps = |lad: crate::schema::LadderId, min_rung: u8| -> u8 {
        matched
            .iter()
            .filter(|(l, r, _)| *l == lad && *r >= min_rung)
            .map(|(_, _, s)| s.reps.min(u8::MAX as u32) as u8)
            .max()
            .unwrap_or(0)
    };
    let best_pullup = best_reps(
        crate::schema::LadderId::VerticalPull,
        MUSCLE_UP_GATE_PULL_RUNG,
    );
    let best_dip = best_reps(crate::schema::LadderId::Dip, MUSCLE_UP_GATE_DIP_RUNG);

    let mut out = Vec::new();
    for l in cal::LADDERS {
        let n_rungs = l.tiers.len() as u8;
        // This ladder's sessions: local day → (rung, set), chronological.
        let mut by_day: BTreeMap<i64, Vec<(u8, &LoggedSet)>> = BTreeMap::new();
        for (lid, r, s) in &matched {
            if *lid == l.id {
                by_day
                    .entry(day_of(s.observed_at))
                    .or_default()
                    .push((*r, *s));
            }
        }
        let mut places: Vec<&CalisPlacement> =
            placements.iter().filter(|p| p.ladder == l.id).collect();
        if by_day.is_empty() && places.is_empty() {
            continue;
        }
        for v in by_day.values_mut() {
            v.sort_by_key(|(_, s)| s.observed_at);
        }
        places.sort_by_key(|p| p.observed_at);
        let place = |p: &CalisPlacement| CalisState {
            rung: p.rung.min(n_rungs - 1),
            entry: CalisEntry::Placed,
            days: Vec::new(),
            // `<= 0` (undated) = the acclimation clock is not started yet.
            since_ts: p.observed_at,
            // A placement is a fresh start: it re-dates the break clock.
            last_day: (p.observed_at > 0).then(|| day_of(p.observed_at)),
        };

        // Each day's sets split at any placement that falls between two of
        // them, so a placement applies in timestamp order: sets before it are
        // judged at the old rung, sets after it are the placed rung's first
        // session.
        let mut sessions: Vec<(i64, Vec<(u8, &LoggedSet)>)> = Vec::new();
        for (&day, day_sets) in &by_day {
            let mut chunk: Vec<(u8, &LoggedSet)> = Vec::new();
            for &(r, s) in day_sets {
                if let Some(&(_, prev)) = chunk.last()
                    && places
                        .iter()
                        .any(|p| p.observed_at >= prev.observed_at && p.observed_at < s.observed_at)
                {
                    sessions.push((day, std::mem::take(&mut chunk)));
                }
                chunk.push((r, s));
            }
            sessions.push((day, chunk));
        }

        let mut st: Option<CalisState> = None;
        let mut pi = 0;
        for (day, day_sets) in &sessions {
            let day = *day;
            let first_ts = day_sets[0].1.observed_at;
            while pi < places.len() && places[pi].observed_at < first_ts {
                st = Some(place(places[pi]));
                pi += 1;
            }
            // The session's rung: the most sets that day (ties → the easier rung).
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
            let at_r: Vec<&LoggedSet> = day_sets
                .iter()
                .filter(|(rr, _)| *rr == r)
                .map(|(_, s)| *s)
                // A timed rung logged without a hold duration carries no metric:
                // neutral (skipped), never a failed 0-second hold.
                .filter(|s| !(r_tier.target.is_timed() && s.hold_sec.is_none()))
                .collect();
            let r_ts = at_r.first().map_or(first_ts, |s| s.observed_at);
            let dated = first_ts > 0;
            if st.is_none() {
                st = Some(CalisState {
                    rung: r,
                    entry: CalisEntry::Logged,
                    days: vec![(day, at_r)],
                    since_ts: r_ts,
                    last_day: dated.then_some(day),
                });
                continue;
            }
            let Some(cur) = st.as_mut() else { continue };
            let gap = cur.last_day.filter(|_| dated).map_or(0, |d| day - d);
            let re = cal::reentry(gap).value;
            let cur_tier = &l.tiers[cur.rung as usize];
            let v = judge_calis_rung(l, cur_tier, &cur.days, cur.entry, goal);
            let advance_ok = v.advance && (!cur_tier.straight_arm || calis_acclimated(cur));
            let expected = if re.steps > 0 {
                cur.rung.saturating_sub(re.steps)
            } else if advance_ok && !cal::weighted_ok(l.id, cur.rung) {
                cur.rung.saturating_add(v.steps).min(n_rungs - 1)
            } else if v.regress && !v.loaded {
                cur.rung.saturating_sub(1)
            } else {
                cur.rung
            };
            let floor_met = {
                let loaded = at_r.last().is_some_and(|s| s.weight_kg > 0.0);
                let floor = cal::working_target(r_tier, goal, loaded).floor() as u32;
                at_r.iter().any(|s| {
                    let x = if r_tier.target.is_timed() {
                        s.hold_sec.unwrap_or(0) as u32
                    } else {
                        s.reps
                    };
                    x >= floor
                })
            };
            if r <= cur.rung.max(expected) || floor_met {
                if r == cur.rung {
                    if re.steps > 0 && cur_tier.straight_arm {
                        // CAL-REENTRY-001: a straight-arm rung restarts its
                        // tendon acclimation window after a break.
                        cur.since_ts = r_ts;
                    }
                    if cur.since_ts <= 0 {
                        // First dated session after an undated placement.
                        cur.since_ts = r_ts;
                    }
                    cur.days.push((day, at_r));
                } else {
                    let entry = if r == expected && r > cur.rung {
                        CalisEntry::Advanced
                    } else if r == expected && r < cur.rung && v.regress && re.steps == 0 {
                        CalisEntry::Regressed
                    } else {
                        CalisEntry::Logged
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
        let Some(cur) = st else { continue };

        let tier = &l.tiers[cur.rung as usize];
        let v = judge_calis_rung(l, tier, &cur.days, cur.entry, goal);
        let mut advance_ready = v.advance;
        // Tendon rate-limit on straight-arm rungs (CAL-TENDON-001).
        if advance_ready && tier.straight_arm && !calis_acclimated(&cur) {
            advance_ready = false;
        }
        if pain_present {
            advance_ready = false;
        }
        let mut regress = v.regress;
        // A break up to the prescribed day steps the rung down (CAL-REENTRY-001).
        let gap = match (through_day, cur.last_day) {
            (Some(t), Some(d)) => t.saturating_add(1).saturating_sub(d),
            _ => 0,
        };
        let re = cal::reentry(gap).value;
        if re.steps > 0 {
            advance_ready = false;
            regress = false;
        }

        // Newest added load at the current rung (the user's own, never invented).
        let last_added_kg = cur
            .days
            .last()
            .and_then(|(_, ss)| ss.last())
            .map_or(0.0, |s| s.weight_kg.max(0.0));
        // Newest loaded set at a weighted rung, for the system-load e1RM line.
        let last_loaded_set = matched
            .iter()
            .filter(|(lid, r, s)| *lid == l.id && cal::weighted_ok(l.id, *r) && s.weight_kg > 0.0)
            .max_by_key(|(_, _, s)| s.observed_at)
            .map(|(_, r, s)| (*r, s.weight_kg, s.reps, s.rpe));

        // Active: dynamic ladders always; skill tracks once logged, but muscle-up
        // only when its count gate is met (else a guidance row, not a plan item).
        let active = if l.id == crate::schema::LadderId::MuscleUp {
            cal::muscle_up_gate_met(best_pullup, best_dip).value
        } else {
            true
        };

        out.push(CalisAnchor {
            ladder: l.id,
            tier: cur.rung,
            advance_ready,
            advance_steps: v.steps,
            regress,
            bridged: cur.entry == CalisEntry::Regressed,
            last_added_kg,
            active,
            needs_effort_rating: v.needs_rating && re.steps == 0 && !pain_present,
            reentry_steps: re.steps,
            retest: re.retest,
            placed: cur.entry == CalisEntry::Placed && cur.days.is_empty(),
            last_loaded_set,
        });
    }
    out
}

/// The placements in force for a prescription built from history through local
/// day `through`: every placement dated up to the NEXT day (the day being
/// prescribed) — an Easier / Harder choice applies to that same day's item —
/// plus undated ones.
pub(super) fn calis_placements_through(model: &Model, through: i64) -> Vec<CalisPlacement> {
    let offset = model.today_utc_offset_sec;
    model
        .calis_placements
        .iter()
        .filter(|p| {
            p.observed_at <= 0 || local_day(p.observed_at, offset) <= through.saturating_add(1)
        })
        .copied()
        .collect()
}
