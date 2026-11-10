//! Weekday placement for one week's lift and run sessions.
//!
//! An exhaustive search over the athlete's available weekdays (ADH-DAYS-001)
//! scores every arrangement with the spacing costs below and keeps the exact
//! minimum, with a fixed lexicographic tie-break. When the free days cannot keep
//! the hard runs ≥48 h apart, a quality session becomes easy (RUN-QUALITY-001).

use super::regions::DayRegion;
use crate::exercise::movement_pattern;
use crate::hybrid;
use crate::running;
use crate::schema::{
    LiftIntensity, LiftSessionType, Prescription, RunSessionType, Session, SessionType,
};
use crate::strength;

/// One lift day of the week's template before it is placed on a weekday.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct LiftSpec {
    /// Body region the day trains (Full / Upper / Lower).
    pub(super) region: DayRegion,
    /// The DUP emphasis index handed to `lift_session` (`dup_emphasis(.., i)`).
    pub(super) emphasis_index: usize,
    /// Heavy lower-body work (≥ `strength::HEAVY_PCT_1RM`): the CAP-3 spacing
    /// anchor.
    pub(super) heavy_legs: bool,
}

/// Where the week's sessions landed. `lifts` is in weekday order, so a lift's
/// index in it is its `LiftDay::slot`.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Placement {
    pub(super) lifts: Vec<(u16, LiftSpec)>,
    pub(super) runs: Vec<(u16, RunSessionType)>,
}

/// What one day asks of the body, for the spacing rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DayLoad {
    Rest,
    Lift {
        region: DayRegion,
        heavy_legs: bool,
    },
    /// `hard` = a quality session or the long run.
    Run {
        hard: bool,
    },
}

/// Hours kept between two sessions that train the same muscles (~48 h rule).
pub(crate) const SAME_MUSCLE_GAP_H: f64 = 48.0;

/// Penalty for training the same muscles on consecutive days
/// ([`SAME_MUSCLE_GAP_H`]).
const SAME_MUSCLE: i32 = 1000;
/// A hard/long run the day AFTER heavy leg work (CAP-3, the costlier side:
/// running economy stays impaired for up to 48 h after heavy lifting). Priced
/// above every other penalty combined in a realistic week, so the layout only
/// ever accepts it when no other arrangement exists.
const HARD_AFTER_HEAVY_LEGS: i32 = 5000;
/// Two hard sessions (quality or long runs) on consecutive days.
const HARD_BACK_TO_BACK: i32 = 400;
/// A hard/long run the day BEFORE heavy leg work (CAP-3, other direction).
const HARD_BEFORE_HEAVY_LEGS: i32 = 300;
/// A hard/long run the day after a moderate (non-heavy) leg day.
const HARD_AFTER_LEGS: i32 = 40;
/// A hard/long run the day before a moderate leg day.
const HARD_BEFORE_LEGS: i32 = 20;
/// Any two sessions on consecutive days: spread the week so rest days fall
/// between sessions where the day count allows.
const CONSECUTIVE: i32 = 5;

/// A spacing rule two sessions on consecutive days break.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpacingRule {
    /// The same muscles on back-to-back days ([`SAME_MUSCLE_GAP_H`]).
    SameMuscles,
    /// A hard or long run the day after heavy leg work (CAP-3,
    /// `hybrid::HEAVY_LEG_RUN_GAP_H`).
    HardRunAfterHeavyLegs,
    /// A hard or long run the day before heavy leg work (CAP-3).
    HardRunBeforeHeavyLegs,
    /// Two hard sessions on back-to-back days (RUN-QUALITY-001).
    HardBackToBack,
}

impl SpacingRule {
    /// The hours the rule keeps between the two sessions.
    pub(crate) fn min_gap_hours(self) -> f64 {
        match self {
            SpacingRule::SameMuscles => SAME_MUSCLE_GAP_H,
            SpacingRule::HardRunAfterHeavyLegs | SpacingRule::HardRunBeforeHeavyLegs => {
                hybrid::HEAVY_LEG_RUN_GAP_H
            }
            SpacingRule::HardBackToBack => {
                f64::from(running::quality_limits().value.min_spacing_hours)
            }
        }
    }
}

/// Cost of `first` on one day followed by `second` the next day, plus the
/// spacing rule the pair breaks, if any.
fn adjacent_cost(first: DayLoad, second: DayLoad) -> (i32, Option<SpacingRule>) {
    use DayLoad::*;
    let spread = if first != Rest && second != Rest {
        CONSECUTIVE
    } else {
        0
    };
    let (cost, rule) = match (first, second) {
        (Lift { region: a, .. }, Lift { region: b, .. }) if a.overlaps(b) => {
            (SAME_MUSCLE, Some(SpacingRule::SameMuscles))
        }
        (
            Lift {
                heavy_legs: true, ..
            },
            Run { hard: true },
        ) => (
            HARD_AFTER_HEAVY_LEGS,
            Some(SpacingRule::HardRunAfterHeavyLegs),
        ),
        (
            Lift {
                region,
                heavy_legs: false,
            },
            Run { hard: true },
        ) if region.has_lower() => (HARD_AFTER_LEGS, None),
        (
            Run { hard: true },
            Lift {
                heavy_legs: true, ..
            },
        ) => (
            HARD_BEFORE_HEAVY_LEGS,
            Some(SpacingRule::HardRunBeforeHeavyLegs),
        ),
        (
            Run { hard: true },
            Lift {
                region,
                heavy_legs: false,
            },
        ) if region.has_lower() => (HARD_BEFORE_LEGS, None),
        (Run { hard: true }, Run { hard: true }) => {
            (HARD_BACK_TO_BACK, Some(SpacingRule::HardBackToBack))
        }
        _ => (0, None),
    };
    (cost + spread, rule)
}

/// Whole-week cost of an arrangement (the week repeats, so Sunday→Monday is
/// adjacent too), plus a small preference for a late-week long run.
fn week_cost(loads: &[DayLoad; 7], long_day: Option<u16>) -> i32 {
    let mut c = 0;
    for d in 0..7 {
        c += adjacent_cost(loads[d], loads[(d + 1) % 7]).0;
    }
    if let Some(l) = long_day {
        c += 6 - l as i32;
    }
    c
}

/// All `k`-subsets of `items`, in lexicographic order.
fn combinations(items: &[u16], k: usize) -> Vec<Vec<u16>> {
    let n = items.len();
    if k > n {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut idx: Vec<usize> = (0..k).collect();
    loop {
        out.push(idx.iter().map(|&i| items[i]).collect());
        // Advance the rightmost index that can still move.
        let mut i = k;
        loop {
            if i == 0 {
                return out;
            }
            i -= 1;
            if idx[i] < i + n - k {
                break;
            }
        }
        idx[i] += 1;
        for j in i + 1..k {
            idx[j] = idx[j - 1] + 1;
        }
    }
}

/// Every distinct ordering of `kinds` (equal kinds are not permuted among
/// themselves), in a fixed order.
fn distinct_orderings(kinds: &[RunSessionType]) -> Vec<Vec<RunSessionType>> {
    fn go(
        kinds: &[RunSessionType],
        used: &mut Vec<bool>,
        cur: &mut Vec<RunSessionType>,
        out: &mut Vec<Vec<RunSessionType>>,
    ) {
        if cur.len() == kinds.len() {
            out.push(cur.clone());
            return;
        }
        let mut tried: Vec<RunSessionType> = Vec::new();
        for i in 0..kinds.len() {
            if used[i] || tried.contains(&kinds[i]) {
                continue;
            }
            tried.push(kinds[i]);
            used[i] = true;
            cur.push(kinds[i]);
            go(kinds, used, cur, out);
            cur.pop();
            used[i] = false;
        }
    }
    let mut out = Vec::new();
    go(
        kinds,
        &mut vec![false; kinds.len()],
        &mut Vec::new(),
        &mut out,
    );
    out
}

/// Keep at most `n` run kinds when fewer days are free than runs requested:
/// the long run first, then hard sessions, easy days dropped first.
fn trim_kinds(kinds: &[RunSessionType], n: usize) -> Vec<RunSessionType> {
    if kinds.len() <= n {
        return kinds.to_vec();
    }
    let mut keep = kinds.to_vec();
    keep.sort_by_key(|k| match k {
        RunSessionType::LongRun => 0,
        k if running::is_quality_session(*k) => 1,
        _ => 2,
    });
    keep.truncate(n);
    keep
}

/// Place the week: `lifts` is the lift-day template in its canonical order
/// (cyclic rotations of it are tried, so an upper/lower template keeps its
/// alternation), `runs` the run kinds to place, `run_before` the race-day
/// offset when the race falls in this week (no run on or after it).
/// `avail[d]` = the athlete can train on weekday `d` (0 = Monday). `anchor`:
/// a weekday that must hold a session when any arrangement allows it (the
/// plan's first day, so a new plan never opens on a rest day).
///
/// Hard runs closer than RUN-QUALITY-001's 48 h (the week repeats, so Sunday
/// and Monday are adjacent) cost a quality session: while the cheapest
/// arrangement puts two hard runs that close, the last quality session left in
/// `runs` becomes an easy run.
pub(super) fn place_week(
    avail: [bool; 7],
    lifts: &[LiftSpec],
    runs: &[RunSessionType],
    run_before: Option<u16>,
    anchor: Option<u16>,
) -> Placement {
    let anchor = anchor.filter(|&a| avail.get(usize::from(a)).copied().unwrap_or(false));
    let mut runs = runs.to_vec();
    let mut placement = search_week(avail, lifts, &runs, run_before, anchor);
    let quality: Vec<usize> = (0..runs.len())
        .filter(|&i| running::is_quality_session(runs[i]))
        .collect();
    for i in quality.into_iter().rev() {
        if !hard_runs_too_close(&placement.runs) {
            break;
        }
        runs[i] = RunSessionType::Easy;
        placement = search_week(avail, lifts, &runs, run_before, anchor);
    }
    placement
}

/// Two placed hard runs fall closer than RUN-QUALITY-001's minimum spacing.
fn hard_runs_too_close(runs: &[(u16, RunSessionType)]) -> bool {
    let min_gap = u16::from(running::quality_limits().value.min_spacing_hours / 24);
    let hard: Vec<u16> = runs
        .iter()
        .filter(|(_, k)| running::is_hard_or_long_session(*k))
        .map(|(d, _)| *d)
        .collect();
    hard.iter().enumerate().any(|(i, a)| {
        hard[i + 1..].iter().any(|b| {
            let gap = a.abs_diff(*b);
            gap.min(7 - gap) < min_gap
        })
    })
}

/// The cheapest arrangement for [`place_week`]; with `anchor`, any arrangement
/// that trains on that weekday beats every one that rests on it. Every week
/// has an arrangement: the lift days and run kinds are trimmed to the free
/// days.
fn search_week(
    avail: [bool; 7],
    lifts: &[LiftSpec],
    runs: &[RunSessionType],
    run_before: Option<u16>,
    anchor: Option<u16>,
) -> Placement {
    let days: Vec<u16> = (0u16..7).filter(|&d| avail[d as usize]).collect();
    let n_lift = lifts.len().min(days.len());
    let lifts = &lifts[..n_lift];

    // Distinct rotations of the lift template.
    let mut rotations: Vec<Vec<LiftSpec>> = Vec::new();
    for r in 0..n_lift.max(1) {
        let rot: Vec<LiftSpec> = (0..n_lift).map(|k| lifts[(k + r) % n_lift]).collect();
        if !rotations.contains(&rot) {
            rotations.push(rot);
        }
    }

    // (rests on the anchor, cost): compared in that order.
    let mut best: Option<((bool, i32), Placement)> = None;
    for lift_days in combinations(&days, n_lift) {
        for rot in &rotations {
            let mut loads = [DayLoad::Rest; 7];
            for (k, &d) in lift_days.iter().enumerate() {
                loads[d as usize] = DayLoad::Lift {
                    region: rot[k].region,
                    heavy_legs: rot[k].heavy_legs,
                };
            }
            // Lift-only cost is a lower bound on any completion: prune.
            let lift_cost = week_cost(&loads, None);
            if best.as_ref().is_some_and(|(k, _)| (false, lift_cost) >= *k) {
                continue;
            }
            let free: Vec<u16> = days
                .iter()
                .copied()
                .filter(|d| !lift_days.contains(d) && run_before.is_none_or(|rb| *d < rb))
                .collect();
            let kinds = trim_kinds(runs, free.len());
            let orderings = distinct_orderings(&kinds);
            for run_days in combinations(&free, kinds.len()) {
                for order in &orderings {
                    let mut l = loads;
                    let mut long_day = None;
                    for (&d, &k) in run_days.iter().zip(order.iter()) {
                        l[d as usize] = DayLoad::Run {
                            hard: running::is_hard_or_long_session(k),
                        };
                        if k == RunSessionType::LongRun {
                            long_day = Some(d);
                        }
                    }
                    let key = (
                        anchor.is_some_and(|a| l[usize::from(a)] == DayLoad::Rest),
                        week_cost(&l, long_day),
                    );
                    if best.as_ref().is_none_or(|(bk, _)| key < *bk) {
                        best = Some((
                            key,
                            Placement {
                                lifts: lift_days.iter().copied().zip(rot.iter().copied()).collect(),
                                runs: run_days
                                    .iter()
                                    .copied()
                                    .zip(order.iter().copied())
                                    .collect(),
                            },
                        ));
                    }
                }
            }
        }
    }
    best.map(|(_, p)| p)
        .expect("a week always has an arrangement: lift days and run kinds fit the free days")
}

/// Classify a synthesized session's load for the spacing rules: a lift day's
/// region comes from its exercises' movement patterns, and it is "heavy legs"
/// when it is a heavy (`strength::is_heavy_pct`) lower-body day.
pub(crate) fn session_load(s: &Session) -> DayLoad {
    match s.session_type {
        SessionType::Rest => DayLoad::Rest,
        SessionType::Run(k) => DayLoad::Run {
            hard: running::is_hard_or_long_session(k),
        },
        SessionType::Lift(lt) => {
            let (mut lower, mut upper, mut heavy) = (false, false, false);
            for rx in &s.prescriptions {
                match &rx.value {
                    Prescription::Lift(l) => match movement_pattern(&l.exercise) {
                        Some(p) if p.is_lower() => {
                            lower = true;
                            if let LiftIntensity::PercentOneRm(p) = l.intensity
                                && strength::is_heavy_pct(f64::from(p))
                            {
                                heavy = true;
                            }
                        }
                        Some(_) => upper = true,
                        None => {}
                    },
                    // Bodyweight ladders span the whole body; never "heavy".
                    Prescription::Calisthenics(_) => {
                        lower = true;
                        upper = true;
                    }
                    Prescription::Run(_) => {}
                }
            }
            let region = match (upper, lower) {
                (true, false) => DayRegion::Upper,
                (false, true) => DayRegion::Lower,
                _ => DayRegion::Full,
            };
            DayLoad::Lift {
                region,
                heavy_legs: lower && (heavy || lt == LiftSessionType::MaxEffort),
            }
        }
    }
}

/// Why a move cannot be made (see [`move_conflict`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MoveConflict {
    /// `from` or `to` lies outside the week, or they are the same day.
    NotAnotherDay,
    /// No session on the day to move from.
    NothingToMove,
    /// The day to move to already holds a session.
    DayTaken,
    /// The moved session would break a spacing rule.
    Spacing(SpacingRule),
}

/// Why moving the session on weekday `from` to weekday `to` would break a
/// spacing rule (ADH-RESCHED-001: the move must leave ~48 h before the next
/// hard session and keep long/hard runs ≥24 h from heavy leg work), or `None`
/// when the move is fine. `to` must be a rest day: never two sessions on one
/// day, never doubling up (ADH-MISSED-001). `idle[d]`: weekday `d` passed
/// with nothing trained, so its planned session loads nothing for spacing.
pub(crate) fn move_conflict(
    loads: [DayLoad; 7],
    idle: [bool; 7],
    from: usize,
    to: usize,
) -> Option<MoveConflict> {
    if from > 6 || to > 6 || from == to {
        return Some(MoveConflict::NotAnotherDay);
    }
    if loads[from] == DayLoad::Rest {
        return Some(MoveConflict::NothingToMove);
    }
    if loads[to] != DayLoad::Rest {
        return Some(MoveConflict::DayTaken);
    }
    let mut after = loads;
    after[to] = loads[from];
    after[from] = DayLoad::Rest;
    // The week repeats, so Sunday's next day is the coming Monday and
    // Monday's previous day last Sunday: `idle` (this week's days) never
    // speaks for them.
    let spaced = |d: usize, wraps: bool| {
        if idle[d] && !wraps {
            DayLoad::Rest
        } else {
            after[d]
        }
    };
    let prev = spaced((to + 6) % 7, to == 0);
    let next = spaced((to + 1) % 7, to == 6);
    [(prev, after[to]), (after[to], next)]
        .into_iter()
        .find_map(|(a, b)| adjacent_cost(a, b).1)
        .map(MoveConflict::Spacing)
}

/// Why a session has no day to move to, so the shell can explain instead of
/// offering an empty picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MoveBlock {
    /// No candidate day is left in the week.
    NoDaysLeft,
    /// Every candidate day already holds a session (never double up,
    /// ADH-MISSED-001).
    AllTaken,
    /// Weekday `day` is the first free candidate; moving there breaks the
    /// rule `conflict` names (as do all the other free candidates).
    Spacing { day: usize, conflict: MoveConflict },
}

/// Every weekday in `candidates` the session on weekday `from` can move to
/// under [`move_conflict`], in the order given; `Err` with the reason when
/// none can. `from` itself is never a candidate.
pub(crate) fn move_options(
    loads: [DayLoad; 7],
    idle: [bool; 7],
    from: usize,
    candidates: impl IntoIterator<Item = usize>,
) -> Result<Vec<usize>, MoveBlock> {
    let mut targets = Vec::new();
    let mut spacing = None;
    let mut any = false;
    for to in candidates {
        if to > 6 || to == from {
            continue;
        }
        any = true;
        match move_conflict(loads, idle, from, to) {
            None => targets.push(to),
            Some(conflict) if loads[to] == DayLoad::Rest && spacing.is_none() => {
                spacing = Some(MoveBlock::Spacing { day: to, conflict });
            }
            Some(_) => {}
        }
    }
    match (targets.is_empty(), spacing, any) {
        (false, ..) => Ok(targets),
        (true, Some(block), _) => Err(block),
        (true, None, true) => Err(MoveBlock::AllTaken),
        (true, None, false) => Err(MoveBlock::NoDaysLeft),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(region: DayRegion, heavy: bool) -> LiftSpec {
        LiftSpec {
            region,
            emphasis_index: 0,
            heavy_legs: heavy,
        }
    }

    #[test]
    fn combinations_enumerate_lexicographically() {
        let c = combinations(&[0, 1, 2, 3], 2);
        assert_eq!(c.len(), 6);
        assert_eq!(c[0], vec![0, 1]);
        assert_eq!(c[5], vec![2, 3]);
        assert_eq!(combinations(&[0, 1], 0), vec![Vec::<u16>::new()]);
    }

    #[test]
    fn three_full_body_days_default_to_mon_wed_fri() {
        let p = place_week(
            [true; 7],
            &[spec(DayRegion::Full, true); 3],
            &[],
            None,
            None,
        );
        let days: Vec<u16> = p.lifts.iter().map(|(d, _)| *d).collect();
        assert_eq!(days, vec![0, 2, 4]);
    }

    #[test]
    fn an_anchored_first_day_holds_a_session() {
        // Plan starts on a Thursday: the Mon/Wed/Fri default would open on a
        // rest day, so the anchored week trains on Thursday.
        let p = place_week(
            [true; 7],
            &[spec(DayRegion::Full, true); 3],
            &[],
            None,
            Some(3),
        );
        let days: Vec<u16> = p.lifts.iter().map(|(d, _)| *d).collect();
        assert!(days.contains(&3), "{days:?}");
        assert_eq!(days.len(), 3);
        // An anchor the default already trains on changes nothing.
        let same = place_week(
            [true; 7],
            &[spec(DayRegion::Full, true); 3],
            &[],
            None,
            Some(2),
        );
        let days: Vec<u16> = same.lifts.iter().map(|(d, _)| *d).collect();
        assert_eq!(days, vec![0, 2, 4]);
        // An unavailable anchor is ignored.
        let mut avail = [false; 7];
        avail[5] = true;
        avail[6] = true;
        let p = place_week(avail, &[spec(DayRegion::Upper, false)], &[], None, Some(3));
        assert_eq!(p.lifts.len(), 1);
        assert!(p.lifts[0].0 >= 5);
    }

    #[test]
    fn no_hard_run_the_day_after_heavy_legs_when_avoidable() {
        let lifts = [spec(DayRegion::Full, true), spec(DayRegion::Full, false)];
        let runs = [
            RunSessionType::LongRun,
            RunSessionType::Tempo,
            RunSessionType::Recovery,
        ];
        let p = place_week([true; 7], &lifts, &runs, None, None);
        let heavy: Vec<u16> = p
            .lifts
            .iter()
            .filter(|(_, s)| s.heavy_legs)
            .map(|(d, _)| *d)
            .collect();
        for (d, k) in &p.runs {
            if running::is_hard_or_long_session(*k) {
                for h in &heavy {
                    assert_ne!(*d, (h + 1) % 7, "{k:?} the day after heavy legs: {p:?}");
                    assert_ne!((*d + 1) % 7, *h, "{k:?} the day before heavy legs: {p:?}");
                }
            }
        }
    }

    #[test]
    fn weekend_only_mask_uses_only_weekend_days() {
        let mut avail = [false; 7];
        avail[5] = true;
        avail[6] = true;
        let p = place_week(
            avail,
            &[spec(DayRegion::Upper, false), spec(DayRegion::Lower, false)],
            &[],
            None,
            None,
        );
        let days: Vec<u16> = p.lifts.iter().map(|(d, _)| *d).collect();
        assert_eq!(days, vec![5, 6]);
    }

    #[test]
    fn move_next_to_heavy_legs_is_refused_with_a_reason() {
        let mut loads = [DayLoad::Rest; 7];
        loads[0] = DayLoad::Lift {
            region: DayRegion::Full,
            heavy_legs: true,
        };
        loads[3] = DayLoad::Run { hard: true };
        let busy = [false; 7];
        assert!(
            move_conflict(loads, busy, 3, 1).is_some(),
            "tempo the day after heavy squats"
        );
        assert!(
            move_conflict(loads, busy, 3, 2).is_none(),
            "48 h later is fine"
        );
        assert!(
            move_conflict(loads, busy, 3, 0).is_some(),
            "never double up"
        );
        // Monday's heavy legs never happened: Tuesday is fine for spacing,
        // but Monday itself still holds its (missed) session.
        let mut idle = [false; 7];
        idle[0] = true;
        assert!(move_conflict(loads, idle, 3, 1).is_none());
        assert!(move_conflict(loads, idle, 3, 0).is_some());
    }

    #[test]
    fn move_options_list_every_valid_day_or_say_why_none_works() {
        let heavy = DayLoad::Lift {
            region: DayRegion::Full,
            heavy_legs: true,
        };
        let mut loads = [DayLoad::Rest; 7];
        loads[0] = heavy;
        loads[3] = DayLoad::Run { hard: true };
        let busy = [false; 7];
        assert_eq!(move_options(loads, busy, 3, 1..7), Ok(vec![2, 4, 5]));
        assert_eq!(
            move_options(loads, busy, 3, std::iter::empty()),
            Err(MoveBlock::NoDaysLeft)
        );
        assert_eq!(
            move_options(loads, busy, 3, [0, 3]),
            Err(MoveBlock::AllTaken)
        );
        assert!(
            matches!(
                move_options(loads, busy, 3, [1]),
                Err(MoveBlock::Spacing { day: 1, .. })
            ),
            "the day after heavy legs is refused with its reason"
        );
    }
}
