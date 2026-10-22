//! Weekday placement for one week's lift and run sessions.
//!
//! The week is laid out by an exhaustive, deterministic search over the
//! athlete's available weekdays (ADH-DAYS-001: the user picks the days), scoring
//! every arrangement against the spacing rules the KB states:
//!
//! - same-muscle lift days need about 48 h between them (LIFT-DAYS-001
//!   templates; ADH-RESCHED-001 "48+ hours before the next hard session");
//! - long or hard runs stay at least 24 h, ideally 48 h, from heavy leg work in
//!   BOTH directions (HYB-CAP-001 / File 10 CAP-3, hybrid-012);
//! - two hard sessions never land on back-to-back days (quality sessions spaced
//!   ≥48 h, RUN-QUALITY limits);
//! - otherwise spread sessions so rest days fall between them, and prefer a
//!   late-week long run.
//!
//! A week of 7 days is small enough to enumerate fully (at most a few thousand
//! arrangements), so the result is the exact minimum of the cost below with a
//! fixed lexicographic tie-break: pure, no clock, no randomness.

use super::DayRegion;
use super::lift::{HEAVY_PCT_1RM, MovementPattern, movement_pattern};
use crate::schema::{
    LiftIntensity, LiftSessionType, Prescription, RunSessionType, Session, SessionType,
};

/// One lift day of the week's template before it is placed on a weekday.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct LiftSpec {
    /// Body region the day trains (Full / Upper / Lower).
    pub(super) region: DayRegion,
    /// The DUP emphasis index handed to `lift_session` (`dup_emphasis(.., i)`).
    pub(super) emphasis_index: usize,
    /// Heavy lower-body work (≥ [`HEAVY_PCT_1RM`]): the CAP-3 spacing anchor.
    pub(super) heavy_legs: bool,
}

/// Where the week's sessions landed. `lifts` is in weekday order, so a lift's
/// index in it is its `BlockCtx::slot`.
#[derive(Debug, Clone, PartialEq, Default)]
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

/// Penalty for training the same muscles on consecutive days (~48 h rule).
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
/// Costs at or above this are spacing-rule violations (not mere preferences);
/// a session move that creates one is refused.
const VIOLATION: i32 = HARD_BEFORE_HEAVY_LEGS;

fn is_hard_run(kind: RunSessionType) -> bool {
    kind == RunSessionType::LongRun || super::run::is_quality(kind)
}

/// Cost of `first` on one day followed by `second` the next day, plus the
/// plain-language reason when the pair breaks a spacing rule.
pub(crate) fn adjacent_cost(first: DayLoad, second: DayLoad) -> (i32, Option<&'static str>) {
    use DayLoad::*;
    let spread = if first != Rest && second != Rest {
        CONSECUTIVE
    } else {
        0
    };
    let (cost, reason) = match (first, second) {
        (Lift { region: a, .. }, Lift { region: b, .. }) if a.overlaps(b) => (
            SAME_MUSCLE,
            Some(
                "the same muscles would be trained on back-to-back days (keep about 48 hours between them)",
            ),
        ),
        (
            Lift {
                heavy_legs: true, ..
            },
            Run { hard: true },
        ) => (
            HARD_AFTER_HEAVY_LEGS,
            Some(
                "a hard or long run would come the day after heavy leg work (keep at least 24 hours between them)",
            ),
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
            Some(
                "a hard or long run would come the day before heavy leg work (keep at least 24 hours between them)",
            ),
        ),
        (
            Run { hard: true },
            Lift {
                region,
                heavy_legs: false,
            },
        ) if region.has_lower() => (HARD_BEFORE_LEGS, None),
        (Run { hard: true }, Run { hard: true }) => (
            HARD_BACK_TO_BACK,
            Some(
                "two hard sessions would land on back-to-back days (keep about 48 hours between them)",
            ),
        ),
        _ => (0, None),
    };
    (cost + spread, reason)
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
    let mut keep: Vec<RunSessionType> = kinds
        .iter()
        .copied()
        .filter(|k| *k == RunSessionType::LongRun)
        .collect();
    keep.extend(kinds.iter().copied().filter(|k| super::run::is_quality(*k)));
    keep.extend(
        kinds
            .iter()
            .copied()
            .filter(|k| *k != RunSessionType::LongRun && !super::run::is_quality(*k)),
    );
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
pub(super) fn place_week(
    avail: [bool; 7],
    lifts: &[LiftSpec],
    runs: &[RunSessionType],
    run_before: Option<u16>,
    anchor: Option<u16>,
) -> Placement {
    let anchor = anchor.filter(|&a| avail.get(usize::from(a)).copied().unwrap_or(false));
    match anchor {
        Some(a) => search_week(avail, lifts, runs, run_before, Some(a))
            .or_else(|| search_week(avail, lifts, runs, run_before, None)),
        None => search_week(avail, lifts, runs, run_before, None),
    }
    .unwrap_or_default()
}

/// The cheapest arrangement for [`place_week`]; with `anchor`, only the
/// arrangements that train on that weekday. `None` when none qualifies.
fn search_week(
    avail: [bool; 7],
    lifts: &[LiftSpec],
    runs: &[RunSessionType],
    run_before: Option<u16>,
    anchor: Option<u16>,
) -> Option<Placement> {
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

    let mut best: Option<(i32, Placement)> = None;
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
            if best.as_ref().is_some_and(|(c, _)| lift_cost >= *c) {
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
                            hard: is_hard_run(k),
                        };
                        if k == RunSessionType::LongRun {
                            long_day = Some(d);
                        }
                    }
                    let c = week_cost(&l, long_day);
                    if anchor.is_some_and(|a| l[usize::from(a)] == DayLoad::Rest) {
                        continue;
                    }
                    if best.as_ref().is_none_or(|(bc, _)| c < *bc) {
                        best = Some((
                            c,
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
}

/// Classify a synthesized session's load for the spacing rules: a lift day's
/// region comes from its exercises' movement patterns, and it is "heavy legs"
/// when it is a heavy (≥ [`HEAVY_PCT_1RM`]) lower-body day.
pub(crate) fn session_load(s: &Session) -> DayLoad {
    match s.session_type {
        SessionType::Rest => DayLoad::Rest,
        SessionType::Run(k) => DayLoad::Run {
            hard: is_hard_run(k),
        },
        SessionType::Lift(lt) => {
            let (mut lower, mut upper, mut heavy) = (false, false, false);
            for rx in &s.prescriptions {
                match &rx.value {
                    Prescription::Lift(l) => match movement_pattern(&l.exercise) {
                        Some(MovementPattern::Squat | MovementPattern::Hinge) => {
                            lower = true;
                            if let LiftIntensity::PercentOneRm(p) = l.intensity
                                && p >= HEAVY_PCT_1RM
                            {
                                heavy = true;
                            }
                        }
                        Some(MovementPattern::Push | MovementPattern::Pull) => upper = true,
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
) -> Option<String> {
    if from > 6 || to > 6 || from == to {
        return Some("pick a different day in the same week".into());
    }
    if loads[from] == DayLoad::Rest {
        return Some("there is no session on that day to move".into());
    }
    if loads[to] != DayLoad::Rest {
        return Some("that day already has a session".into());
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
    for (a, b) in [(prev, after[to]), (after[to], next)] {
        let (c, reason) = adjacent_cost(a, b);
        if c >= VIOLATION {
            return Some(
                reason
                    .unwrap_or("it breaks the spacing between hard sessions")
                    .into(),
            );
        }
    }
    None
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
    /// spacing rule `reason` names (as do all the other free candidates).
    Spacing { day: usize, reason: String },
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
            Some(reason) if loads[to] == DayLoad::Rest && spacing.is_none() => {
                spacing = Some(MoveBlock::Spacing { day: to, reason });
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
            if is_hard_run(*k) {
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
