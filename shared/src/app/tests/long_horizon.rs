//! Long-horizon training-life simulations: whole training lives replayed day
//! by day for one to three years through the real `update` / `view`, one
//! seeded PRNG per archetype, so a run is identical on every machine. Each
//! simulated day sends `SetToday`, then follows the plan with noisy
//! adherence (skips, lighter days, overshoots, off-plan sessions, backdated
//! logs, amends and deletes) plus check-ins and weekly reviews, and checks
//! consumer-visible invariants on that day's `ViewModel`:
//!
//! - numbers: no NaN/inf, no negative distance/load/duration, the JSON wire
//!   round-trips (a NaN serializes as `null` and fails to decode);
//! - running: no prescribed run more than 10 % over the longest run of the
//!   window the plan anchors on (RUN-SPIKE-001 / RUN-START-001), goal long-run
//!   ceilings (RUN-LONGRUN-TARGET-001), weekly planned volume never more than
//!   30 % over the last two weeks' planned/run volume (RUN-PROGRESS-001), the
//!   recovery week cut (RUN-DELOAD-001 3:1 or 2:1, RUN-DELOAD-DEPTH-001);
//! - lifting: loads only from logged history (HARD RULE 1), never above the
//!   1-rep-in-reserve load of the best logged e1RM (STR-INTENT-001 /
//!   HYP-FAIL-SAFE-001, Epley strength-005), no week-to-week jump past the
//!   2–10 % progression step unless the logged e1RM rose as much
//!   (STR-LOADPROG-001 / STR-PCTPROG-001), the lifting deload week every
//!   sixth plan week for non-novices (STR-DELOAD-VOL-001 /
//!   HYP-DELOAD-CADENCE-001);
//! - holds (HARD RULE 3): pain (SAFE-PAIN-001), illness (ILLNESS-NECK-001),
//!   pregnancy (SAFE-PREG-001), RED-S (SAFE-REDS-001 / REDS-SCREEN-001) and
//!   under-18 (SAFE-PEDS-001) blank what they hold and lift when resolved,
//!   the first session back easy (RETURN-HOLD-001 / ILLNESS-RTT-001);
//! - re-entry after an undeclared break follows the File 08 Table 3.4b
//!   brackets for lifts and runs (REENTRY-RAMP-001, DETRAIN-STR-001,
//!   RUN-REENTRY-001);
//! - week strip, consistency card (ADH-SELFMON-001 / ADH-PROGRESS-001) and
//!   History totals agree with the entries actually logged;
//! - units: a pound user sees no kilogram text, `*_mi` strings carry no km;
//! - view build time stays bounded as the history grows.

use super::*;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::{Duration, Instant};

// ── Calendar helpers ───────────────────────────────────────────────────────

/// Monday-based weekday of an epoch day (epoch day 4 is a Monday).
fn wd(d: i64) -> i64 {
    (d - 4).rem_euclid(7)
}

fn monday_of(d: i64) -> i64 {
    d - wd(d)
}

/// 2026-01-05, a Monday: the first simulated day of every life.
const START: i64 = 20_458;

/// The planner's run-anchor horizon (`anchors.rs`): the 30-day spike window
/// plus the 28-day detraining taper. A prescribed run is spike-bounded by the
/// longest logged run inside it (RUN-SPIKE-001, DETRAIN-001).
const RUN_HORIZON_DAYS: i64 = 58;

// ── Ledger of what the simulated athlete actually logged ───────────────────

#[derive(Clone, Debug)]
struct SetRec {
    id: u64,
    day: i64,
    ex: String,
    kg: f64,
    reps: u32,
    rpe: f64,
    hold: Option<u16>,
    at: i64,
}

#[derive(Clone, Debug)]
struct RunRec {
    id: u64,
    day: i64,
    km: f64,
    min: f64,
    at: i64,
}

/// A hold the simulated athlete is under, by the view's `HoldReason` rank
/// (most specific first, `view_types::HoldReason`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Hold {
    Youth,
    Pregnancy,
    Reds,
    Illness,
    Pain,
}

impl Hold {
    fn reason(self) -> HoldReason {
        match self {
            Hold::Youth => HoldReason::Youth,
            Hold::Pregnancy => HoldReason::Pregnancy,
            Hold::Reds => HoldReason::Reds,
            Hold::Illness => HoldReason::Illness,
            Hold::Pain => HoldReason::Pain,
        }
    }
    /// A profile health gate: the whole plan is withheld.
    fn withholds_plan(self) -> bool {
        matches!(self, Hold::Youth | Hold::Pregnancy)
    }
}

/// How the simulated athlete follows the plan, as per-event probabilities.
#[derive(Clone, Copy)]
struct Adherence {
    skip: f64,
    lighter: f64,
    overshoot: f64,
    off_plan: f64,
    backdate: f64,
    amend: f64,
    delete: f64,
    checkin: f64,
    weekly_review: f64,
}

const STEADY: Adherence = Adherence {
    skip: 0.08,
    lighter: 0.12,
    overshoot: 0.08,
    off_plan: 0.06,
    backdate: 0.05,
    amend: 0.02,
    delete: 0.01,
    checkin: 0.6,
    weekly_review: 0.5,
};

/// A break the athlete takes without telling the app (no `SetToday`, no
/// logs), and what the plan showed before it.
struct ReEntry {
    back_day: i64,
    /// Days from the last lift / run before the break to the return day
    /// (`None`: never trained that way). Days fully off = gap − 1.
    lift_gap: Option<i64>,
    run_gap: Option<i64>,
    /// Per exercise, the heaviest prescribed load in the 4 weeks before as a
    /// share of the 1-RIR load its e1RM bound allowed at those reps (so a
    /// different rep scheme on the return day compares like for like), with
    /// that e1RM bound.
    pre_lifts: HashMap<String, (f64, f64)>,
    /// The pre-break weekly volume the running re-entry scales: the higher of
    /// the stated weekly km and the 28-day average logged before the break.
    pre_week_km: f64,
    /// Prescriptions judged so far for this break.
    lift_hits: u32,
    run_hits: u32,
}

impl ReEntry {
    /// What a break of a week or more left unjudged, had there been
    /// something before it to compare against.
    fn misses(&self) -> Vec<String> {
        let back = self.back_day - START;
        let mut out = Vec::new();
        if self.lift_gap.is_some_and(|g| g > 7) && !self.pre_lifts.is_empty() && self.lift_hits == 0
        {
            out.push(format!("day {back}: no lift checked"));
        }
        if self.run_gap.is_some_and(|g| g > 7) && self.pre_week_km > 0.0 && self.run_hits == 0 {
            out.push(format!("day {back}: no run checked"));
        }
        out
    }
}

/// One planned week as first seen (Monday, or the first day the app opened).
#[derive(Clone, Debug)]
struct WeekSnap {
    run_km: f64,
    lift_sets: u32,
    week: u8,
    weeks_total: u8,
    phase: MesoPhase,
    clean: bool,
}

/// (day, prescribed load, e1RM bound).
type LiftSeen = (i64, f64, f64);

struct Sim {
    name: &'static str,
    app: Engine,
    model: Model,
    rng: Lcg,
    today: i64,
    off: i64,
    id: u64,
    profile: Profile,
    adh: Adherence,
    plan_start: Option<i64>,
    sets: Vec<SetRec>,
    runs: Vec<RunRec>,
    activities: usize,
    holds: BTreeSet<Hold>,
    /// A pain/illness hold resolved: the next planned session is the easy
    /// return (RETURN-HOLD-001 / ILLNESS-RTT-001) until it is trained.
    returning: Option<Hold>,
    /// A profile gate lifted on this day: the plan resumes from it.
    gate_lifted: Option<(Hold, i64, i64)>,
    last_profile_change: i64,
    last_hold_day: i64,
    /// Last day an entry was deleted: a deleted set can retroactively un-count
    /// a finished plan week, so the week index is re-derived, not advanced.
    last_delete_day: i64,
    away_until: Option<i64>,
    last_open_day: Option<i64>,
    /// Days a profile gate (under 18, pregnancy) withheld the plan.
    gated: BTreeSet<i64>,
    reentry: Option<ReEntry>,
    /// Breaks of a week or more whose return day checked nothing (a lift or
    /// run re-entry with no item to judge): a vacuous re-entry invariant.
    reentry_misses: Vec<String>,
    backdated: Vec<(Event, Option<SetRec>, Option<RunRec>)>,
    weeks: BTreeMap<i64, WeekSnap>,
    /// Today's prescribed load per (exercise, reps): (day, load, e1RM bound).
    lift_seen: HashMap<(String, u8), Vec<LiftSeen>>,
    rhr_base: f64,
    max_view: Duration,
    sum_view: Duration,
    n_views: u32,
    /// How often each invariant was actually exercised (non-vacuous).
    hits: BTreeMap<&'static str, u32>,
    failures: Vec<String>,
}

fn round_to(x: f64, step: f64) -> f64 {
    (x / step).round() * step
}

fn norm(s: &str) -> String {
    s.to_lowercase()
}

/// Starting guess for a never-logged movement, kg (a shell prefill).
fn first_load(ex: &str) -> f64 {
    let e = norm(ex);
    if e.contains("deadlift") {
        70.0
    } else if e.contains("squat") {
        55.0
    } else if e.contains("bench") {
        42.5
    } else if e.contains("row") {
        40.0
    } else if e.contains("press") {
        27.5
    } else if e.contains("curl") || e.contains("raise") || e.contains("fly") {
        10.0
    } else {
        20.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ItemKind {
    Lift,
    Calisthenics,
    Run,
}

fn kind(it: &PrescriptionView) -> ItemKind {
    if it.is_run() {
        ItemKind::Run
    } else if it.calis.is_some() {
        ItemKind::Calisthenics
    } else {
        ItemKind::Lift
    }
}

/// Main-set distance of a run item, km: the plain distance head, else an
/// estimate from the structure or the minutes the summary leads with.
fn item_km(it: &PrescriptionView) -> f64 {
    if let Some(k) = it.run_head_km {
        return k;
    }
    if let Some(rs) = &it.run_structure {
        let wc = f64::from(rs.warmup_min + rs.cooldown_min) / 6.5;
        if rs.reps > 0 && rs.rep_m > 0 {
            return f64::from(rs.reps * rs.rep_m) / 1000.0 + wc;
        }
        if rs.reps > 0 && rs.rep_sec > 0 {
            return f64::from(rs.reps * rs.rep_sec) / 60.0 / 5.0 + wc;
        }
        if !rs.run_walk.is_empty() {
            let run: u32 = rs.run_walk.iter().map(|s| s.run_sec).sum();
            let walk: u32 = rs.run_walk.iter().map(|s| s.walk_sec).sum();
            return f64::from(run) / 60.0 / 7.5 + f64::from(walk) / 60.0 / 12.0 + wc;
        }
    }
    let text = if it.run_head.is_empty() {
        &it.summary
    } else {
        &it.run_head
    };
    minutes_lead(text).map_or(3.0, |m| m / 6.5)
}

/// The minutes a run line leads with: "30 min", "10–20 min" (midpoint).
fn minutes_lead(s: &str) -> Option<f64> {
    let head: String = s.chars().take_while(|c| *c != '·').collect();
    let i = head.find("min")?;
    let nums: Vec<f64> = head[..i]
        .split(|c: char| !c.is_ascii_digit() && c != '.')
        .filter_map(|t| t.parse().ok())
        .collect();
    match nums.as_slice() {
        [a] => Some(*a),
        [a, b, ..] => Some((a + b) / 2.0),
        _ => None,
    }
}

/// Epley e1RM with the RPE's reps in reserve added (strength-005), an upper
/// bound on any anchor the engine derives from the set.
fn e1rm_bound(kg: f64, reps: u32, rpe: f64) -> f64 {
    let rir = (10.0 - rpe).max(0.0);
    kg * (1.0 + (f64::from(reps) + rir) / 30.0)
}

/// The heaviest load `reps` reps allow with one rep in reserve on an e1RM
/// (Epley), the bound `check_lifts` holds every prescribed load to.
fn one_rir_load(e1rm: f64, reps: u8) -> f64 {
    e1rm / (1.0 + (f64::from(reps) + 1.0) / 30.0)
}

/// Number keys that may legitimately go negative on the wire.
const SIGNED_KEYS: &[&str] = &[
    "delta_pct",
    "delta_bpm",
    "delta",
    "z",
    "trend_pct",
    "change_pct",
    "drift_pct",
    "utc_offset_sec",
    // Local epoch days: a birthday before 1970 is negative.
    "birth_epoch_day",
];

/// Walk the wire JSON: every number finite and, unless its key is signed by
/// nature, non-negative; no string rendering a non-number. `path` names the
/// enclosing keys for the report.
fn walk_json(v: &serde_json::Value, path: &str, key: &str, out: &mut Vec<String>) {
    match v {
        serde_json::Value::Number(n) => {
            let x = n.as_f64().unwrap_or(0.0);
            if !x.is_finite() {
                out.push(format!("{path} = {n} is not finite"));
            } else if x < 0.0
                && !SIGNED_KEYS.contains(&key)
                && !path.starts_with(".readiness_summary.")
            {
                out.push(format!("{path} = {x} is negative"));
            }
        }
        serde_json::Value::String(s) => {
            if s.contains("NaN") || s.contains("inf ") || s.contains("∞") {
                out.push(format!("{path}: {s:?} renders a non-number"));
            }
        }
        serde_json::Value::Array(a) => a.iter().for_each(|x| walk_json(x, path, key, out)),
        serde_json::Value::Object(o) => o
            .iter()
            .for_each(|(k, x)| walk_json(x, &format!("{path}.{k}"), k, out)),
        _ => {}
    }
}

/// Text claims a weight in `unit` ("120 kg", "265lb").
fn mentions_unit(s: &str, unit: &str) -> bool {
    let b = s.as_bytes();
    s.match_indices(unit).any(|(i, _)| {
        let end_ok = b
            .get(i + unit.len())
            .is_none_or(|c| !c.is_ascii_alphabetic());
        let mut j = i;
        while j > 0 && b[j - 1] == b' ' {
            j -= 1;
        }
        end_ok && j > 0 && b[j - 1].is_ascii_digit()
    })
}

impl Sim {
    fn new(name: &'static str, seed: u64, profile: Profile, adh: Adherence) -> Sim {
        let mut sim = Sim {
            name,
            app: Engine,
            model: Model::default(),
            rng: Lcg(seed),
            today: START,
            off: 0,
            id: 1,
            profile: profile.clone(),
            adh,
            plan_start: None,
            sets: Vec::new(),
            runs: Vec::new(),
            activities: 0,
            holds: BTreeSet::new(),
            returning: None,
            gate_lifted: None,
            last_profile_change: START,
            last_hold_day: i64::MIN / 2,
            last_delete_day: i64::MIN / 2,
            away_until: None,
            last_open_day: None,
            gated: BTreeSet::new(),
            reentry: None,
            reentry_misses: Vec::new(),
            backdated: Vec::new(),
            weeks: BTreeMap::new(),
            lift_seen: HashMap::new(),
            rhr_base: profile.resting_hr_bpm.unwrap_or(58.0),
            max_view: Duration::ZERO,
            sum_view: Duration::ZERO,
            n_views: 0,
            hits: BTreeMap::new(),
            failures: Vec::new(),
        };
        sim.send(set_today_ev(START));
        sim.set_profile(Profile {
            effective_from_epoch_day: Some(START),
            ..profile
        });
        sim
    }

    fn send(&mut self, e: Event) {
        let _ = self.app.update(e, &mut self.model);
    }

    fn next_id(&mut self) -> u64 {
        self.id += 1;
        self.id
    }

    /// Unix seconds of local `hour` on local day `day`.
    fn at(&self, day: i64, hour: f64) -> i64 {
        day * DAY_SEC + (hour * 3_600.0) as i64 - self.off
    }

    fn hit(&mut self, check: &'static str) {
        *self.hits.entry(check).or_default() += 1;
    }

    fn fail(&mut self, check: &'static str, msg: String) {
        if self.failures.len() < 40 {
            self.failures.push(format!(
                "[{}] day {} {check}: {msg}",
                self.name,
                self.today - START
            ));
        }
    }

    fn set_profile(&mut self, p: Profile) {
        self.profile = p.clone();
        self.last_profile_change = self.today;
        self.send(Event::SetProfile(p));
    }

    /// A dated profile edit from today on (the shell sends the core's today).
    fn edit_profile(&mut self, f: impl FnOnce(&mut Profile)) {
        let mut p = self.profile.clone();
        f(&mut p);
        p.effective_from_epoch_day = Some(self.today);
        self.set_profile(p);
    }

    fn generate_plan(&mut self) {
        self.plan_start = Some(self.today);
        self.send(Event::GeneratePlan {
            start_epoch_day: self.today,
        });
    }

    // ── Logging ────────────────────────────────────────────────────────

    fn log_set_rec(&mut self, r: SetRec, backdate: bool) {
        let e = Event::LogSet {
            exercise: r.ex.clone(),
            weight_kg: r.kg,
            reps: r.reps,
            rpe: r.rpe,
            observed_at: r.at,
            entry_id: r.id,
            hold_sec: r.hold,
        };
        if backdate {
            self.backdated.push((e, Some(r), None));
        } else {
            self.send(e);
            self.sets.push(r);
        }
    }

    fn log_run_rec(&mut self, r: RunRec, backdate: bool) {
        let e = Event::LogRun {
            distance_km: r.km,
            duration_min: r.min,
            hr_pct_max: self.rng.range(64.0, 76.0).round(),
            longest_recent_km: 0.0,
            observed_at: r.at,
            entry_id: r.id,
            workout_type: None,
            session_rpe: self.rng.chance(0.5).then(|| self.rng.int(2, 6) as u8),
            avg_hr_bpm: None,
            utc_offset_sec: None,
        };
        if backdate {
            self.backdated.push((e, None, Some(r)));
        } else {
            self.send(e);
            self.runs.push(r);
        }
    }

    fn log_run_km(&mut self, km: f64, hour: f64, backdate: bool) {
        let km = round_to(km.max(0.5), 0.01);
        let pace = if self.profile.progression_cadence == ProgressionCadence::EverySession {
            7.0
        } else {
            6.0
        };
        let r = RunRec {
            id: self.next_id(),
            day: self.today,
            km,
            min: round_to(km * pace * self.rng.range(0.95, 1.05), 0.1),
            at: self.at(self.today, hour),
        };
        self.log_run_rec(r, backdate);
    }

    /// Follow today's planned session with noisy adherence.
    fn train(&mut self, vm: &ViewModel) {
        let Some(ns) = vm.next_session.as_ref() else {
            return self.maybe_off_plan();
        };
        if ns.epoch_day != self.today
            || ns.items.is_empty()
            || matches!(
                ns.status,
                SessionStatus::Blocked | SessionStatus::Rest | SessionStatus::BeforePlan
            )
        {
            return self.maybe_off_plan();
        }
        if self.returning.is_none() && self.rng.chance(self.adh.skip) {
            return;
        }
        let lighter = self.rng.chance(self.adh.lighter);
        let over = !lighter && self.rng.chance(self.adh.overshoot);
        let backdate = self.rng.chance(self.adh.backdate);
        let lb = self.profile.weight_lb;
        let mut hour = self.rng.range(6.5, 19.0);
        for it in ns.items.iter().filter(|i| !i.done) {
            match kind(it) {
                ItemKind::Run => {
                    let f = if lighter {
                        0.7
                    } else if over {
                        self.rng.range(1.05, 1.15)
                    } else {
                        self.rng.range(0.97, 1.03)
                    };
                    self.log_run_km(item_km(it) * f, hour, backdate);
                    hour += 1.0;
                }
                kind => {
                    let calis = kind == ItemKind::Calisthenics;
                    let base = if calis {
                        it.added_kg.unwrap_or(0.0)
                    } else {
                        it.load_kg
                            .or(it.suggested_load_kg)
                            .or(it.last_set.as_ref().map(|s| s.weight_kg))
                            .unwrap_or_else(|| first_load(&it.exercise))
                    };
                    let f = if lighter {
                        0.85
                    } else if over {
                        1.05
                    } else {
                        1.0
                    };
                    // Plates in the athlete's unit: a pound user loads whole
                    // 5 lb steps, logged in kg as the shell converts them.
                    let kg = if base <= 0.0 {
                        0.0
                    } else if lb {
                        round_to(base * f * 2.204_622_621_8, 5.0) / 2.204_622_621_8
                    } else {
                        round_to(base * f, 2.5)
                    };
                    let n = if lighter {
                        it.sets.saturating_sub(1).max(1)
                    } else {
                        it.sets.max(1)
                    };
                    for s in 0..n {
                        let target = it.target_rpe.unwrap_or(8.0);
                        let rpe = (target + [-0.5, 0.0, 0.0, 0.5][self.rng.int(0, 3) as usize])
                            .clamp(6.0, 10.0);
                        let (reps, hold) = if it.timed_hold {
                            let lo = f64::from(it.hold_low_sec.max(5));
                            let hi = f64::from(it.hold_high_sec.max(it.hold_low_sec).max(5));
                            let sec = self.rng.range(lo, hi) * if lighter { 0.7 } else { 1.0 };
                            (1, Some(sec.round() as u16))
                        } else {
                            let hi = u32::from(it.reps_high.max(it.reps_low).max(1));
                            let lo = u32::from(it.reps_low.max(1));
                            let r = self.rng.int(lo, hi);
                            let r = if s + 1 == n && self.rng.chance(0.2) {
                                r.saturating_sub(1).max(1)
                            } else {
                                r + u32::from(over)
                            };
                            (r, None)
                        };
                        let rec = SetRec {
                            id: self.next_id(),
                            day: self.today,
                            ex: it.exercise.clone(),
                            kg,
                            reps,
                            rpe,
                            hold,
                            at: self.at(self.today, hour),
                        };
                        self.log_set_rec(rec, backdate);
                        hour += 0.05;
                    }
                }
            }
        }
        self.returning = None;
    }

    fn maybe_off_plan(&mut self) {
        if !self.rng.chance(self.adh.off_plan) {
            return;
        }
        match self.rng.int(0, 2) {
            0 => {
                let km = self.rng.range(2.5, 6.0);
                self.log_run_km(km, 17.5, false);
            }
            1 => {
                for _ in 0..3 {
                    let rec = SetRec {
                        id: self.next_id(),
                        day: self.today,
                        ex: "Dumbbell curl".into(),
                        kg: 12.5,
                        reps: 12,
                        rpe: 8.0,
                        hold: None,
                        at: self.at(self.today, 18.0),
                    };
                    self.log_set_rec(rec, false);
                }
            }
            _ => {
                let id = self.next_id();
                self.send(Event::LogActivity {
                    kind: ActivityKind::Cycling,
                    minutes: 45.0,
                    vigorous: false,
                    strengthening: false,
                    observed_at: self.at(self.today, 16.0),
                    entry_id: id,
                });
                self.activities += 1;
            }
        }
    }

    /// Amend or delete a recent entry, as the History editor does.
    fn edit_history(&mut self) {
        let recent = |d: i64, today: i64| today - d <= 14;
        if self.rng.chance(self.adh.amend) {
            let idx: Vec<usize> = (0..self.sets.len())
                .filter(|&i| recent(self.sets[i].day, self.today) && self.sets[i].hold.is_none())
                .collect();
            if !idx.is_empty() {
                let i = idx[self.rng.int(0, idx.len() as u32 - 1) as usize];
                let s = &mut self.sets[i];
                s.kg = (s.kg + if s.kg >= 5.0 { -2.5 } else { 2.5 }).max(0.0);
                let e = Event::AmendSet {
                    entry_id: s.id,
                    exercise: s.ex.clone(),
                    weight_kg: s.kg,
                    reps: s.reps,
                    rpe: s.rpe,
                    observed_at: s.at,
                    observed_at_fallback: s.at,
                    hold_sec: s.hold,
                };
                self.send(e);
            }
        }
        if self.rng.chance(self.adh.delete) {
            if self.rng.chance(0.6) {
                let idx: Vec<usize> = (0..self.sets.len())
                    .filter(|&i| recent(self.sets[i].day, self.today))
                    .collect();
                if !idx.is_empty() {
                    let i = idx[self.rng.int(0, idx.len() as u32 - 1) as usize];
                    let s = self.sets.remove(i);
                    self.last_delete_day = self.today;
                    self.send(Event::DeleteEntry {
                        kind: EntryKind::Set,
                        entry_id: s.id,
                        observed_at_fallback: s.at,
                    });
                }
            } else {
                let idx: Vec<usize> = (0..self.runs.len())
                    .filter(|&i| recent(self.runs[i].day, self.today))
                    .collect();
                if !idx.is_empty() {
                    let i = idx[self.rng.int(0, idx.len() as u32 - 1) as usize];
                    let r = self.runs.remove(i);
                    self.last_delete_day = self.today;
                    self.send(Event::DeleteEntry {
                        kind: EntryKind::Run,
                        entry_id: r.id,
                        observed_at_fallback: r.at,
                    });
                }
            }
        }
    }

    fn morning(&mut self) {
        if self.rng.chance(self.adh.checkin) {
            let c = CheckinInput {
                observed_at: self.at(self.today, 7.0),
                sleep_quality: Some(self.rng.int(3, 5) as u8),
                soreness: Some(self.rng.int(1, 2) as u8),
                mood: Some(self.rng.int(3, 5) as u8),
                fatigue: Some(self.rng.int(1, 2) as u8),
                stress: Some(self.rng.int(1, 3) as u8),
                resting_hr_bpm: Some(round_to(self.rhr_base + self.rng.range(-1.5, 1.5), 0.1)),
                hrv_rmssd_ms: Some(round_to(60.0 + self.rng.range(-5.0, 5.0), 0.1)),
            };
            self.send(Event::SubmitCheckin(c));
        }
        // The Sunday weekly check-in answers the screens afresh: an athlete
        // under a RED-S hold keeps answering yes until it is resolved.
        if wd(self.today) == 6
            && !self.holds.contains(&Hold::Reds)
            && self.rng.chance(self.adh.weekly_review)
        {
            self.send(Event::SubmitWeeklyCheckin(SessionReview::default()));
        }
    }

    // ── Holds ──────────────────────────────────────────────────────────

    fn start_hold(&mut self, h: Hold) {
        let at = self.at(self.today, 7.5);
        match h {
            Hold::Pain => self.send(Event::SubmitReadiness(ReadinessInput {
                signal: ReadinessSignal::Pain,
                value: 1.0,
                observed_at: at,
                streak: 0,
                pain: Some(crate::schema::PainDetail {
                    kind: crate::schema::PainKind::SharpJoint,
                    severity: Some(6),
                    trend: crate::schema::PainTrend::Stable,
                    persists: false,
                    // Unlocated: a located report scopes the stop to the
                    // movements loading that site (Table 4.1 "STOP that
                    // exercise") instead of the whole day.
                    location: None,
                }),
                effort_min: None,
            })),
            Hold::Illness => self.send(Event::SubmitReadiness(ReadinessInput {
                observed_at: at,
                ..input(ReadinessSignal::Illness, 2.0)
            })),
            Hold::Reds => self.send(Event::SubmitWeeklyCheckin(SessionReview {
                reds_signal_reported: true,
                ..SessionReview::default()
            })),
            Hold::Pregnancy => self.edit_profile(|p| p.health.pregnant = true),
            Hold::Youth => unreachable!("youth is derived from the birthday"),
        }
        self.holds.insert(h);
        self.last_hold_day = self.today;
    }

    fn resolve_hold(&mut self, h: Hold) {
        let at = self.at(self.today, 7.6);
        match h {
            Hold::Pain => self.send(Event::ResolveHold {
                signal: ReadinessSignal::Pain,
                observed_at: at,
            }),
            Hold::Illness => self.send(Event::ResolveHold {
                signal: ReadinessSignal::Illness,
                observed_at: at,
            }),
            Hold::Reds => self.send(Event::SubmitWeeklyCheckin(SessionReview::default())),
            Hold::Pregnancy => self.edit_profile(|p| p.health.pregnant = false),
            // The core derives the gate from the birthday on `SetToday`.
            Hold::Youth => {}
        }
        if h.withholds_plan() {
            self.gate_lifted = Some((h, self.today, self.today));
            self.mark_reentry(self.today);
        }
        self.holds.remove(&h);
        self.last_hold_day = self.today;
        if matches!(h, Hold::Pain | Hold::Illness) {
            self.returning = Some(h);
        }
    }

    // ── The day loop ───────────────────────────────────────────────────

    /// Run `days` simulated days; `script(sim, day_index)` fires the
    /// archetype's scheduled events each morning before the view.
    fn run(&mut self, days: i64, mut script: impl FnMut(&mut Sim, i64)) {
        for i in 0..days {
            self.today = START + i;
            if let Some(until) = self.away_until {
                if self.today < until {
                    continue;
                }
                self.away_until = None;
            }
            self.send(Event::SetToday {
                epoch_day: self.today,
                utc_offset_sec: self.off,
            });
            for (e, s, r) in std::mem::take(&mut self.backdated) {
                self.send(e);
                self.sets.extend(s);
                self.runs.extend(r);
            }
            script(self, i);
            if self.holds.iter().any(|h| h.withholds_plan()) {
                self.gated.insert(self.today);
            }
            if self.away_until.is_some_and(|u| u > self.today) {
                continue;
            }
            self.morning();
            let t = Instant::now();
            let vm = self.app.view(&self.model);
            let el = t.elapsed();
            self.max_view = self.max_view.max(el);
            self.sum_view += el;
            self.n_views += 1;
            let full = wd(self.today) == 6 || i + 1 == days || self.today == self.last_hold_day;
            self.check(&vm, full);
            self.last_open_day = Some(self.today);
            if self.holds.is_empty() {
                self.train(&vm);
            }
            self.edit_history();
        }
    }

    /// Leave the app for `days` days from tomorrow (a holiday, a busy spell).
    fn go_away(&mut self, days: i64) {
        let back = self.today + 1 + days;
        self.away_until = Some(back);
        self.mark_reentry(back);
    }

    /// Training resumes on `back` after a break: remember what the plan
    /// prescribed in the four weeks before the last lift, and the running
    /// volume before the last run.
    fn mark_reentry(&mut self, back: i64) {
        self.close_reentry();
        let last_lift = self.sets.iter().map(|s| s.day).max();
        let last_run = self.runs.iter().map(|r| r.day).max();
        let mut pre_lifts: HashMap<String, (f64, f64)> = HashMap::new();
        if let Some(last) = last_lift {
            for ((ex, reps), v) in &self.lift_seen {
                for &(d, l, e) in v {
                    let ratio = l / one_rir_load(e, *reps);
                    if (0..=28).contains(&(last - d)) {
                        let p = pre_lifts.entry(ex.clone()).or_default();
                        if ratio > p.0 {
                            *p = (ratio, e);
                        }
                    }
                }
            }
        }
        // The engine's measured weekly volume as of the last run: the km of
        // its trailing 28 days over the weeks that window spans.
        let measured = last_run.map_or(0.0, |last| {
            let window: Vec<&RunRec> = self
                .runs
                .iter()
                .filter(|r| (0..28).contains(&(last - r.day)))
                .collect();
            let first = window.iter().map(|r| r.day).min().unwrap_or(last);
            let weeks = (((last - first + 1) as f64) / 7.0).ceil().clamp(1.0, 4.0);
            window.iter().map(|r| r.km).sum::<f64>() / weeks
        });
        self.reentry = Some(ReEntry {
            back_day: back,
            lift_gap: last_lift.map(|d| back - d),
            run_gap: last_run.map(|d| back - d),
            pre_lifts,
            pre_week_km: measured.max(self.profile.running_km_per_week),
            lift_hits: 0,
            run_hits: 0,
        });
    }

    /// The re-entry window is over (or a new break starts): record a break
    /// whose return was never judged.
    fn close_reentry(&mut self) {
        if let Some(re) = self.reentry.take() {
            self.reentry_misses.extend(re.misses());
        }
    }

    // ── Invariants ─────────────────────────────────────────────────────

    /// `LONG_HORIZON_DUMP=<name>:<day>,<day>…[;<name>:…]` prints that life's
    /// plan on those simulated days, for triaging a failure.
    fn dump(&self, vm: &ViewModel) {
        let Ok(spec) = std::env::var("LONG_HORIZON_DUMP") else {
            return;
        };
        let i = self.today - START;
        let wanted = spec
            .split(';')
            .filter_map(|s| s.split_once(':'))
            .any(|(name, days)| name == self.name && days.split(',').any(|d| d.parse() == Ok(i)));
        if !wanted {
            return;
        }
        eprintln!(
            "--- {} day {i}: program {:?} blocked {} {:?}",
            self.name,
            vm.program
                .as_ref()
                .map(|p| (p.phase, p.week, p.weeks_total)),
            vm.train_blocked,
            vm.hold_reason
        );
        for s in &vm.week_plan {
            eprintln!("  day {} {} {:?}", s.epoch_day - START, s.title, s.status);
            for it in &s.items {
                eprintln!(
                    "    {} | load {:?} sugg {:?} {:?} | {} | {}",
                    it.summary,
                    it.load_kg,
                    it.suggested_load_kg,
                    it.intensity,
                    it.anchored_on,
                    it.adjusted_note
                );
            }
        }
        if let Some(c) = &vm.consistency {
            eprintln!("  consistency {:?} repeating {}", c.weeks, c.repeating_week);
        }
    }

    fn check(&mut self, vm: &ViewModel, full: bool) {
        self.dump(vm);
        self.check_numbers(vm, full);
        self.check_units(vm);
        self.check_holds(vm);
        self.check_history(vm);
        if vm.program.is_some() && self.holds.is_empty() {
            self.check_strip(vm);
            self.check_runs(vm);
            self.check_lifts(vm);
            self.check_reentry(vm);
            self.snapshot_week(vm);
        }
    }

    fn check_numbers(&mut self, vm: &ViewModel, full: bool) {
        let items = vm
            .next_session
            .iter()
            .chain(&vm.week_plan)
            .flat_map(|s| &s.items);
        let mut bad = Vec::new();
        for it in items {
            for (what, x) in [
                ("load_kg", it.load_kg),
                ("suggested_load_kg", it.suggested_load_kg),
                ("added_kg", it.added_kg),
                ("run_head_km", it.run_head_km),
                ("run_logged_km", Some(it.run_logged_km)),
                ("run_logged_min", Some(it.run_logged_min)),
                ("target_rpe", it.target_rpe),
            ] {
                if let Some(x) = x
                    && (!x.is_finite() || x < 0.0)
                {
                    bad.push(format!("{} {what} = {x}", it.summary));
                }
            }
            if it.run_head_km.is_some_and(|k| k <= 0.0 || k > 45.0) {
                bad.push(format!("implausible run distance: {}", it.summary));
            }
            if it.load_kg.is_some_and(|l| l > 400.0) || it.sets > 12 || it.reps_high > 60 {
                bad.push(format!("implausible lift: {}", it.summary));
            }
        }
        for l in &vm.lifts {
            let tonnage = if l.hold_sec.is_some() {
                0.0
            } else {
                l.weight_kg * f64::from(l.reps)
            };
            if !l.e1rm_kg.is_finite()
                || l.e1rm_kg < 0.0
                || !l.tonnage_kg.is_finite()
                || (l.tonnage_kg - tonnage).abs() > 0.01 + 1e-6 * tonnage
            {
                bad.push(format!("lift row {l:?}"));
            }
        }
        for r in &vm.runs {
            if !r.distance_km.is_finite()
                || r.distance_km < 0.0
                || !r.duration_min.is_finite()
                || r.duration_min < 0.0
            {
                bad.push(format!("run row {} / {}", r.distance_km, r.duration_min));
            }
        }
        if let Some(tl) = &vm.training_load
            && (!tl.ctl.is_finite() || !tl.atl.is_finite() || tl.ctl < 0.0 || tl.atl < 0.0)
        {
            bad.push(format!("training load {} / {}", tl.ctl, tl.atl));
        }
        if full {
            self.hit("wire");
            let json = serde_json::to_string(vm).expect("view serializes");
            if let Err(e) = serde_json::from_str::<ViewModel>(&json) {
                bad.push(format!(
                    "view JSON does not decode (a NaN became null?): {e}"
                ));
            }
            let v: serde_json::Value = serde_json::from_str(&json).expect("json");
            walk_json(&v, "", "", &mut bad);
        }
        self.hit("numbers");
        for b in bad {
            self.fail("numbers", b);
        }
    }

    fn check_units(&mut self, vm: &ViewModel) {
        let lb = self.profile.weight_lb;
        let (wrong, right) = if lb { ("kg", "lb") } else { ("lb", "kg") };
        let mut texts: Vec<&str> = Vec::new();
        for s in vm.next_session.iter().chain(&vm.week_plan) {
            for it in &s.items {
                if it.is_run() {
                    continue;
                }
                texts.extend([
                    it.summary.as_str(),
                    it.face_detail.as_str(),
                    it.face_lead.as_str(),
                    it.anchored_on.as_str(),
                ]);
                texts.extend(it.last_set.iter().map(|s| s.summary.as_str()));
                texts.extend(it.logged_sets.iter().map(|s| s.summary.as_str()));
            }
        }
        texts.extend(vm.lifts.iter().rev().take(30).map(|l| l.summary.as_str()));
        if let Some(d) = &vm.session_done {
            texts.extend(d.items.iter().map(|i| i.summary.as_str()));
        }
        let mut bad: Vec<String> = texts
            .iter()
            .filter(|t| mentions_unit(t, wrong))
            .map(|t| format!("{right} user sees {t:?}"))
            .collect();
        let mut mi: Vec<&str> = Vec::new();
        for s in vm.next_session.iter().chain(&vm.week_plan) {
            for it in &s.items {
                mi.push(&it.safety_reason_mi);
                mi.extend(it.run_target.iter().map(|t| t.pace_label_mi.as_str()));
                if let Some(t) = &it.run_target
                    && t.pace_label_km.contains("/mi")
                {
                    bad.push(format!("km pace label {:?}", t.pace_label_km));
                }
            }
        }
        mi.extend(vm.guidance.iter().map(|g| g.summary_mi.as_str()));
        mi.extend(vm.weekly_report.iter().map(|g| g.summary_mi.as_str()));
        mi.extend(vm.feedback.iter().map(|f| f.message_mi.as_str()));
        bad.extend(
            mi.iter()
                .filter(|t| mentions_unit(t, "km") || t.contains("/km"))
                .map(|t| format!("miles text carries km: {t:?}")),
        );
        if !texts.is_empty() {
            self.hit("units");
        }
        for b in bad {
            self.fail("units", b);
        }
    }

    fn check_holds(&mut self, vm: &ViewModel) {
        let Some(&top) = self.holds.iter().next() else {
            // No hold the athlete reported: benign check-ins never block.
            if self.today - self.last_hold_day <= 14 {
                self.hit("hold lifts");
            }
            if vm.train_blocked {
                let msg = format!(
                    "blocked with no hold: {:?} {:?}",
                    vm.hold_reason, vm.today_headline.summary
                );
                self.fail("hold lifts", msg);
            }
            if let Some((h, day, plan_day)) = self.gate_lifted {
                self.hit("gate lifts");
                if vm.plan_withheld.is_some() {
                    self.fail(
                        "gate lifts",
                        format!(
                            "{h:?} lifted on day {} but the plan is still withheld",
                            day - START
                        ),
                    );
                }
                if self.today >= plan_day && vm.next_session.is_none() {
                    self.fail("gate lifts", format!("{h:?} lifted but no next session"));
                }
                if self.today > plan_day + 7 {
                    self.gate_lifted = None;
                }
            }
            if let Some(h) = self.returning {
                self.check_return(vm, h);
            }
            return;
        };
        self.hit("hold blocks");
        if top != Hold::Youth && !vm.train_blocked {
            self.fail(
                "hold blocks",
                format!("{top:?} active but training not blocked"),
            );
        }
        if vm.hold_reason != Some(top.reason()) {
            self.fail(
                "hold blocks",
                format!("{top:?} active, hold_reason {:?}", vm.hold_reason),
            );
        }
        if top.withholds_plan() {
            if vm.plan_withheld != Some(PlanWithheld::HealthHold) {
                self.fail(
                    "hold blocks",
                    format!("{top:?}: plan_withheld {:?}", vm.plan_withheld),
                );
            }
            if vm.next_session.is_some() || !vm.week_plan.is_empty() {
                self.fail(
                    "hold blocks",
                    format!("{top:?}: the gated plan still renders"),
                );
            }
        } else if let Some(ns) = &vm.next_session
            && (ns.status != SessionStatus::Blocked || !ns.items.is_empty())
        {
            let msg = format!(
                "{top:?}: next session {:?} with {} items",
                ns.status,
                ns.items.len()
            );
            self.fail("hold blocks", msg);
        }
        if top == Hold::Illness {
            let today = self.today;
            for row in vm.week_plan.iter().filter(|s| s.epoch_day > today) {
                if !row.items.is_empty() {
                    self.fail(
                        "hold blocks",
                        format!("flu: day +{} still planned", row.epoch_day - today),
                    );
                }
            }
        }
        if top == Hold::Youth {
            if vm.youth_activity.is_none() {
                self.fail(
                    "hold blocks",
                    "under 18 without the WHO activity card".into(),
                );
            }
        } else if vm.youth_activity.is_some() {
            self.fail(
                "hold blocks",
                "adult shows the under-18 activity card".into(),
            );
        }
    }

    /// The first session after a pain/illness hold is the easy return
    /// (RETURN-HOLD-001: lifts with 3+ reps left, runs at easy effort;
    /// ILLNESS-RTT-001: the 10–20 min return check).
    fn check_return(&mut self, vm: &ViewModel, h: Hold) {
        let Some(ns) = vm.next_session.as_ref().filter(|ns| !ns.items.is_empty()) else {
            return;
        };
        self.hit("easy return");
        for it in &ns.items {
            let easy = if it.return_check {
                true
            } else if it.is_run() {
                it.rep_count == 0
                    && it
                        .run_target
                        .as_ref()
                        .is_none_or(|t| t.band == "easy" || t.band == "recovery")
            } else {
                it.target_rpe.is_none_or(|r| r <= 7.0)
            };
            if !easy {
                self.fail("easy return", format!("after {h:?}: {}", it.summary));
            }
        }
        if h == Hold::Illness && !ns.items.iter().any(|i| i.return_check) {
            let s: Vec<_> = ns.items.iter().map(|i| i.summary.clone()).collect();
            self.fail("easy return", format!("after flu no return check: {s:?}"));
        }
    }

    fn check_history(&mut self, vm: &ViewModel) {
        self.hit("history");
        let mut bad = Vec::new();
        if vm.lifts.len() != self.sets.len() {
            bad.push(format!(
                "{} lift rows vs {} logged",
                vm.lifts.len(),
                self.sets.len()
            ));
        }
        if vm.runs.len() != self.runs.len() {
            bad.push(format!(
                "{} run rows vs {} logged",
                vm.runs.len(),
                self.runs.len()
            ));
        }
        if vm.activities.len() != self.activities {
            bad.push(format!(
                "{} activities vs {}",
                vm.activities.len(),
                self.activities
            ));
        }
        let ton: f64 = vm.lifts.iter().map(|l| l.tonnage_kg).sum();
        let want: f64 = self
            .sets
            .iter()
            .filter(|s| s.hold.is_none())
            .map(|s| s.kg * f64::from(s.reps))
            .sum();
        if (ton - want).abs() > 0.05 + 1e-6 * want {
            bad.push(format!("tonnage {ton} vs logged {want}"));
        }
        let km: f64 = vm.runs.iter().map(|r| r.distance_km).sum();
        let want_km: f64 = self.runs.iter().map(|r| r.km).sum();
        if (km - want_km).abs() > 0.01 * self.runs.len() as f64 + 1e-6 {
            bad.push(format!("run km {km} vs logged {want_km}"));
        }
        if let Some(rp) = &vm.run_progress {
            let longest = self.runs.iter().map(|r| r.km).fold(0.0, f64::max);
            if (rp.longest_run_km - longest).abs() > 0.051 {
                bad.push(format!("longest run {} vs {longest}", rp.longest_run_km));
            }
            for w in &rp.weekly {
                let in_week: Vec<&RunRec> = self
                    .runs
                    .iter()
                    .filter(|r| {
                        (w.week_start_epoch_day..w.week_start_epoch_day + 7).contains(&r.day)
                    })
                    .collect();
                let k: f64 = in_week.iter().map(|r| r.km).sum();
                if in_week.len() as u32 != w.runs
                    || (w.km - k).abs() > 0.06 * in_week.len() as f64 + 0.01
                {
                    bad.push(format!(
                        "week {}: {} runs / {} km vs logged {} / {k:.2}",
                        w.week_start_epoch_day - START,
                        w.runs,
                        w.km,
                        in_week.len()
                    ));
                }
            }
        } else if !self.runs.is_empty() {
            bad.push("runs logged but no run progress".into());
        }
        // The consistency card's finished weeks count distinct logged days.
        if let (Some(c), Some(start)) = (&vm.consistency, self.plan_start) {
            for w in &c.weeks {
                // ADH-SELFMON-001 / ADH-GAMIFY-001 (no loss framing): a week a
                // health gate held throughout was not a missed plan week.
                let m = w.week_monday_epoch_day;
                if (m..m + 7).all(|d| self.gated.contains(&d)) && w.done < w.planned {
                    bad.push(format!(
                        "held week {} scored {} of {} planned",
                        m - START,
                        w.done,
                        w.planned
                    ));
                }
                if w.week_monday_epoch_day < start
                    || w.week_monday_epoch_day < self.last_profile_change
                {
                    continue;
                }
                let days: BTreeSet<i64> = self
                    .sets
                    .iter()
                    .map(|s| s.day)
                    .chain(self.runs.iter().map(|r| r.day))
                    .filter(|d| (w.week_monday_epoch_day..w.week_monday_epoch_day + 7).contains(d))
                    .collect();
                let want = days.len().min(usize::from(w.planned)) as u8;
                if w.done != want {
                    bad.push(format!(
                        "consistency week {}: done {} of {} vs {} logged days",
                        w.week_monday_epoch_day - START,
                        w.done,
                        w.planned,
                        days.len()
                    ));
                }
            }
            if let Some(last) = c.weeks.last()
                && last.week_monday_epoch_day >= start
                && last.week_monday_epoch_day + 7 == monday_of(self.today)
            {
                // ADH-PROGRESS-001: a week under two-thirds done repeats.
                let repeat = u32::from(last.done) * 3 < u32::from(last.planned) * 2;
                if c.repeating_week != repeat {
                    bad.push(format!(
                        "repeating_week {} after {} of {}",
                        c.repeating_week, last.done, last.planned
                    ));
                }
            }
        }
        for b in bad {
            self.fail("history", b);
        }
    }

    fn logged_on(&self, day: i64, run: bool) -> bool {
        if run {
            self.runs.iter().any(|r| r.day == day)
        } else {
            self.sets.iter().any(|s| s.day == day)
        }
    }

    fn check_strip(&mut self, vm: &ViewModel) {
        self.hit("week strip");
        let mon = monday_of(self.today);
        let mut bad = Vec::new();
        let days: Vec<i64> = vm.week_plan.iter().map(|s| s.epoch_day).collect();
        if days != (mon..mon + 7).collect::<Vec<_>>() {
            bad.push(format!(
                "strip days {:?}",
                days.iter().map(|d| d - START).collect::<Vec<_>>()
            ));
        }
        let live: BTreeSet<u64> = self.sets.iter().map(|s| s.id).collect();
        let set_day: HashMap<u64, i64> = self.sets.iter().map(|s| (s.id, s.day)).collect();
        for row in &vm.week_plan {
            let d = row.epoch_day;
            if self.gated.contains(&d) && !row.items.is_empty() {
                bad.push(format!("day {} was gated yet shows a plan", d - START));
            }
            if d > self.today
                && matches!(
                    row.status,
                    SessionStatus::Done
                        | SessionStatus::DoneLighter
                        | SessionStatus::Missed
                        | SessionStatus::Partial
                )
            {
                bad.push(format!("future day {} is {:?}", d - START, row.status));
            }
            if d < self.today && row.status == SessionStatus::Next {
                bad.push(format!("past day {} is the next session", d - START));
            }
            let run = matches!(row.session_type, SessionType::Run(_));
            let src = row.credited_from_epoch_day.unwrap_or(d);
            if row.status.is_done() {
                if (src - d).abs() > 2 || monday_of(src) != mon {
                    bad.push(format!("day {} credited from {}", d - START, src - START));
                }
                if !self.logged_on(src, run) {
                    bad.push(format!(
                        "day {} {:?} with nothing logged on {}",
                        d - START,
                        row.status,
                        src - START
                    ));
                }
            }
            for it in &row.items {
                for s in &it.logged_sets {
                    if !live.contains(&s.entry_id) {
                        bad.push(format!(
                            "day {} lists deleted/unknown set {}",
                            d - START,
                            s.entry_id
                        ));
                    } else if set_day[&s.entry_id] != d && set_day[&s.entry_id] != src {
                        bad.push(format!(
                            "day {} lists a set from day {}",
                            d - START,
                            set_day[&s.entry_id] - START
                        ));
                    }
                }
                if usize::from(it.sets_done) > it.logged_sets.len() {
                    bad.push(format!(
                        "{}: {} done of {} logged",
                        it.summary,
                        it.sets_done,
                        it.logged_sets.len()
                    ));
                }
                if it.is_run() && d <= self.today {
                    let k: f64 = self
                        .runs
                        .iter()
                        .filter(|r| r.day == src)
                        .map(|r| r.km)
                        .sum();
                    if it.run_logged_km > k + 0.06 {
                        bad.push(format!(
                            "day {} run_logged_km {} > logged {k:.2}",
                            d - START,
                            it.run_logged_km
                        ));
                    }
                }
            }
        }
        for b in bad {
            self.fail("week strip", b);
        }
    }

    fn longest_before(&self, through: i64) -> Option<f64> {
        self.runs
            .iter()
            .filter(|r| r.day <= through && through - r.day < RUN_HORIZON_DAYS)
            .map(|r| r.km)
            .reduce(f64::max)
    }

    fn check_runs(&mut self, vm: &ViewModel) {
        let goal_cap = match self.profile.goal_distance {
            GoalDistance::HalfMarathon => Some(21.0),
            GoalDistance::Marathon => Some(32.0),
            _ => None,
        };
        let stated = self.profile.stated_longest_recent_km;
        let mut bad = Vec::new();
        let mut hits = 0;
        for row in vm
            .week_plan
            .iter()
            .filter(|s| s.epoch_day >= self.today && !s.status.is_done())
        {
            for it in row.items.iter().filter(|i| i.is_run()) {
                let Some(km) = it.run_head_km else { continue };
                let through = (row.epoch_day - 1).min(self.today);
                // RUN-SPIKE-001 over the logged runs, else the stated longest
                // run while the plan still reads it (RUN-START-001).
                let stated_in = stated.filter(|_| through - self.last_profile_change <= 30);
                let base = match (self.longest_before(through), stated_in) {
                    (Some(a), Some(b)) => Some(a.max(b)),
                    (a, b) => a.or(b),
                };
                if let Some(b) = base {
                    hits += 1;
                    if km > 1.1 * b + 0.01 {
                        bad.push(format!(
                            "day {}: {km} km > 110% of {b} km",
                            row.epoch_day - START
                        ));
                    }
                }
                if let Some(cap) = goal_cap
                    && km > cap
                {
                    bad.push(format!("{km} km over the {cap} km goal long run"));
                }
            }
        }
        if hits > 0 {
            self.hit("spike ceiling");
        }
        for b in bad {
            self.fail("spike ceiling", b);
        }
    }

    fn check_lifts(&mut self, vm: &ViewModel) {
        let lb_slack = if self.profile.weight_lb { 2.3 } else { 0.0 };
        let mut bad = Vec::new();
        let mut seen_today: Vec<((String, u8), f64, f64)> = Vec::new();
        for row in vm
            .week_plan
            .iter()
            .filter(|s| s.epoch_day >= self.today && !s.status.is_done())
        {
            let through = (row.epoch_day - 1).min(self.today);
            for it in row.items.iter().filter(|i| kind(i) == ItemKind::Lift) {
                // STR-INTENT-001 / HYP-FAIL-SAFE-001: never below 1 RIR.
                if it.target_rpe.is_some_and(|r| r > 9.0 + 1e-9) {
                    bad.push(format!("{}: target RPE {:?}", it.summary, it.target_rpe));
                }
                if let PlanIntensity::PercentE1rm { pct, .. } = it.intensity {
                    let rir = 30.0 * (100.0 / f64::from(pct) - 1.0) - f64::from(it.reps_high);
                    if rir < 0.95 {
                        bad.push(format!(
                            "{}: {pct}% × {} = {rir:.2} RIR",
                            it.summary, it.reps_high
                        ));
                    }
                }
                let Some(load) = it.load_kg else { continue };
                let name = norm(&it.exercise);
                let e = self
                    .sets
                    .iter()
                    .filter(|s| s.day <= through && norm(&s.ex) == name && s.hold.is_none())
                    .map(|s| e1rm_bound(s.kg, s.reps, s.rpe))
                    .reduce(f64::max);
                let Some(e) = e else {
                    bad.push(format!(
                        "{}: a load with no logged set (HARD RULE 1)",
                        it.summary
                    ));
                    continue;
                };
                let cap = e / (1.0 + f64::from(it.reps_low + 1) / 30.0);
                if load > cap + 2.5 + lb_slack {
                    bad.push(format!(
                        "{}: {load} kg over the 1-RIR load {cap:.1} of e1RM {e:.1}",
                        it.summary
                    ));
                }
                if row.epoch_day == self.today {
                    seen_today.push(((name, it.reps_low), load, e));
                }
            }
        }
        self.hit("lift caps");
        // STR-LOADPROG-001 (+2–10 %) / STR-PCTPROG-001: the prescribed load
        // rises no faster than the step, the logged e1RM itself, or what was
        // actually lifted (last-time loads follow the athlete's own sets).
        // After a week or more without lifting (a trip, a hold, missed
        // sessions) the re-entry ramp climbs back to the pre-break loads,
        // which may sit outside the 28-day lookback (behind a lighter or
        // adjusted week), so the step there is the ramp's (REENTRY-RAMP-001).
        let ramp_until = {
            let mut days: Vec<i64> = self.sets.iter().map(|s| s.day).collect();
            days.sort_unstable();
            days.dedup();
            let pending = days
                .last()
                .filter(|&&d| self.today - d >= 8)
                .map(|_| self.today);
            pending
                .or_else(|| {
                    days.windows(2)
                        .rev()
                        .find(|w| w[1] - w[0] >= 8)
                        .map(|w| w[1])
                })
                .map(|back| back + 42)
        };
        for (key, load, e) in seen_today {
            let hist = self.lift_seen.entry(key.clone()).or_default();
            if hist.last().is_some_and(|h| h.0 == self.today) {
                continue;
            }
            let prior = hist
                .iter()
                .filter(|h| self.today - h.0 <= 28)
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .copied();
            hist.push((self.today, load, e));
            let Some((pd, pl, pe)) = prior else { continue };
            *self.hits.entry("load step").or_default() += 1;
            let lifted = self
                .sets
                .iter()
                .filter(|s| self.today - s.day <= 28 && norm(&s.ex) == key.0 && s.hold.is_none())
                .map(|s| s.kg)
                .fold(0.0, f64::max);
            let in_ramp = ramp_until.is_some_and(|u| self.today <= u);
            // The %1RM step (STR-PCTPROG-001: at most +5 % a week) compounds
            // with a logged e1RM rise; the load-step rule (STR-LOADPROG-001)
            // allows +10 % on its own; the re-entry ramp climbs from 70 %.
            let step: f64 = if in_ramp { 1.0 / 0.7 } else { 1.10 };
            let allowed = (pl * step.max(1.05 * e / pe)).max(1.10 * lifted) + 2.5 + lb_slack;
            if load > allowed {
                bad.push(format!(
                    "{} ×{}: {pl} kg (day {}) → {load} kg, e1RM bound {pe:.1} → {e:.1}",
                    key.0,
                    key.1,
                    pd - START
                ));
            }
        }
        for b in bad {
            self.fail("lift caps", b);
        }
    }

    /// Table 3.4b re-entry, judged from the return day until the first lift
    /// (resp. run) is logged after it: until then every prescription is the
    /// engine's first session back (a break that still runs is day 0 of its
    /// ramp), whichever day the athlete opens the app.
    fn check_reentry(&mut self, vm: &ViewModel) {
        let Some(re) = &self.reentry else { return };
        if self.today < re.back_day {
            return;
        }
        if self.today > re.back_day + 42 {
            self.close_reentry();
            return;
        }
        let lifted_since = self.sets.iter().any(|s| s.day >= re.back_day);
        let ran_since = self.runs.iter().any(|r| r.day >= re.back_day);
        let lb_slack = if self.profile.weight_lb { 2.3 } else { 0.0 };
        let mut bad = Vec::new();
        let (mut lift_hits, mut run_hits) = (0, 0);
        // File 08 Table 3.4b for resistance re-entry: <1 wk resume as planned
        // (one progression step allowed), 1–2 wk ~90 %, 2–4 wk ~80–85 %,
        // 4–8 wk ~70 % (REENTRY-RAMP-001); past 8 weeks no fraction (novice
        // restart), so nothing above the 4–8 wk start. The fraction is of the
        // load the current e1RM implies at today's reps: a heavier last
        // session before the break raises it, as it would have without one.
        if let Some(gap) = re.lift_gap.filter(|_| !lifted_since) {
            let frac = match gap {
                ..=7 => 1.10,
                8..=14 => 0.90,
                15..=30 => 0.85,
                _ => 0.70,
            };
            for row in vm.week_plan.iter().filter(|s| s.epoch_day >= self.today) {
                for it in row.items.iter().filter(|i| kind(i) == ItemKind::Lift) {
                    let ex = norm(&it.exercise);
                    let Some(&(ratio, pre_e)) = re.pre_lifts.get(&ex) else {
                        continue;
                    };
                    lift_hits += 1;
                    let e = self
                        .sets
                        .iter()
                        .filter(|s| s.day < self.today && norm(&s.ex) == ex && s.hold.is_none())
                        .map(|s| e1rm_bound(s.kg, s.reps, s.rpe))
                        .fold(pre_e, f64::max);
                    // A prescribed load is floored to the 2.5 kg grid, so the
                    // load it stood for may sit up to one step above it.
                    let pre = ratio * one_rir_load(pre_e, it.reps_low);
                    let allowed = (pre + 2.5) * frac * (e / pre_e).max(1.0) + 2.5 + lb_slack;
                    for (what, l) in [
                        ("load", it.load_kg),
                        ("suggested load", it.suggested_load_kg),
                    ] {
                        let Some(l) = l else { continue };
                        if l > allowed {
                            bad.push(format!(
                                "{gap} days since the last lift: {what} {l} kg ({}) vs {pre:.1} kg before \
                                 (e1RM {pre_e:.1} → {e:.1})",
                                it.summary
                            ));
                        }
                    }
                }
            }
        }
        // Table 3.4b running (RUN-REENTRY-001), by days fully off running:
        // 1–2 wk ~90 % of the prior volume, 2–4 wk ~70–80 % and the long run
        // rebuilt, 4–8 wk ~50–60 %, past 8 weeks "rebuild base" (never more
        // than the 4–8 wk restart). The rest of the return week stays under
        // that share of the pre-break week, with room for the plan's cycle
        // ramp (at most two +10 % steps, RUN-PROGRESS-001); with the long run
        // rebuilt, no run outgrows the long-run share of that week (30 %, or
        // the average run with three days or fewer, RUN-LONGRUN-001).
        let n_days = f64::from(self.profile.running_days_per_week.max(1));
        if let Some(gap) = re.run_gap.filter(|_| re.pre_week_km > 0.0 && !ran_since) {
            let weeks_off = (gap - 1) as f64 / 7.0;
            let upper = if weeks_off < 1.0 {
                None
            } else if weeks_off < 2.0 {
                Some(0.90)
            } else if weeks_off < 4.0 {
                Some(0.80)
            } else {
                Some(0.60)
            };
            if let Some(upper) = upper {
                let week_cap = upper * re.pre_week_km * 1.2;
                let long_cap =
                    week_cap * crate::running::LONG_RUN_SHARE_MAX.max(1.0 / n_days) + 0.5;
                let runs: Vec<(f64, &PrescriptionView)> = vm
                    .week_plan
                    .iter()
                    .filter(|s| s.epoch_day >= self.today)
                    .flat_map(|s| {
                        s.items
                            .iter()
                            .filter(|i| i.is_run())
                            .map(move |i| (item_km(i), i))
                    })
                    .collect();
                let total: f64 = runs.iter().map(|r| r.0).sum();
                if std::env::var_os("LONG_HORIZON_TRACE").is_some() {
                    let kms: Vec<f64> = runs.iter().map(|r| r.0).collect();
                    eprintln!(
                        "  [{}] day {} run re-entry: {} days off, pre {:.1} km/wk, caps week {week_cap:.1} \
                         long {long_cap:.1}, planned {kms:?}",
                        self.name,
                        self.today - START,
                        gap - 1,
                        re.pre_week_km
                    );
                }
                if !runs.is_empty() {
                    run_hits += 1;
                }
                if total > week_cap + 1.0 {
                    bad.push(format!(
                        "{} days off running: {total:.1} km planned for the rest of the week, \
                         over {upper} × {:.1} km/wk before",
                        gap - 1,
                        re.pre_week_km
                    ));
                }
                for (km, it) in runs.iter().filter(|_| weeks_off >= 2.0) {
                    if *km > long_cap {
                        bad.push(format!(
                            "{} days off running: {} ({km:.1} km) over the rebuilt long run {long_cap:.1} km",
                            gap - 1,
                            it.summary
                        ));
                    }
                }
            }
        }
        let re = self.reentry.as_mut().expect("checked above");
        re.lift_hits += lift_hits;
        re.run_hits += run_hits;
        if lift_hits > 0 {
            self.hit("re-entry");
        }
        if run_hits > 0 {
            self.hit("run re-entry");
        }
        for b in bad {
            self.fail("re-entry", b);
        }
    }

    /// First view of each plan week: the week's planned volume, position and
    /// phase; the ramp and deload checks run across these snapshots.
    fn snapshot_week(&mut self, vm: &ViewModel) {
        let mon = monday_of(self.today);
        if self.weeks.contains_key(&mon) {
            return;
        }
        let Some(p) = &vm.program else { return };
        let run_km: f64 = vm
            .week_plan
            .iter()
            .flat_map(|s| &s.items)
            .filter(|i| i.is_run())
            .map(item_km)
            .sum();
        let lift_sets: u32 = vm
            .week_plan
            .iter()
            .flat_map(|s| &s.items)
            .filter(|i| !i.is_run())
            .map(|i| u32::from(i.sets))
            .sum();
        let clean = self.today - self.last_profile_change >= 14
            && self.today - self.last_hold_day >= 14
            && self.reentry.is_none()
            && self.plan_start.is_some_and(|s| mon - s >= 14)
            && self.last_open_day.is_some_and(|d| self.today - d <= 3);
        let snap = WeekSnap {
            run_km,
            lift_sets,
            week: p.week,
            weeks_total: p.weeks_total,
            phase: p.phase,
            clean,
        };
        let prev = self.weeks.get(&(mon - 7)).cloned();
        let prev2 = self.weeks.get(&(mon - 14)).cloned();
        self.weeks.insert(mon, snap.clone());
        if !clean {
            return;
        }
        // Compare two settled weeks only (the earlier one not just after an
        // edit, a hold or a break either).
        let (Some(prev), Some(prev2)) = (prev, prev2) else {
            return;
        };
        if !prev.clean {
            return;
        }
        let race = self.profile.goal_race_epoch_day.is_some_and(|r| r >= mon);
        let running = self.profile.running_days_per_week > 0;
        let lifting = self.profile.weekly_sets > 0;

        // RUN-PROGRESS-001: >30 % over two weeks is the injury-linked ramp.
        if running {
            self.hit("run ramp");
            let actual = (1..=8)
                .map(|k| {
                    let m = mon - 7 * k;
                    self.runs
                        .iter()
                        .filter(|r| (m..m + 7).contains(&r.day))
                        .map(|r| r.km)
                        .sum::<f64>()
                })
                .fold(0.0, f64::max);
            let reference = prev.run_km.max(prev2.run_km).max(actual);
            if run_km > 1.3 * reference + 1.0 {
                self.fail(
                    "run ramp",
                    format!(
                        "planned {run_km:.1} km after {:.1} / {:.1} planned, {actual:.1} run",
                        prev.run_km, prev2.run_km
                    ),
                );
            }
        }

        // Deload cadence (non-race plans). Running only: RUN-DELOAD-001 3:1,
        // or 2:1 for masters / novices. Lifting: the 5-week block + 1
        // deload week for non-novices (STR-DELOAD-VOL-001,
        // HYP-DELOAD-CADENCE-001); novices deload on triggers only.
        let novice = self.profile.progression_cadence == ProgressionCadence::EverySession;
        // The core derives the age from the birthday.
        let age = vm.profile.as_ref().and_then(|p| p.age_years);
        if !race && snap.weeks_total == prev.weeks_total {
            self.hit("deload cadence");
            let total = snap.weeks_total;
            let want_total = if lifting {
                if novice { 5 } else { 6 }
            } else if novice || age.is_some_and(|a| a >= 65.0) {
                3
            } else {
                4
            };
            if total != want_total {
                self.fail(
                    "deload cadence",
                    format!("block of {total} weeks, expected {want_total}"),
                );
            }
            let deload_expected = !(lifting && novice) && snap.week == total;
            // A hybrid's card also reads Deload on the running side's own
            // recovery week (its 3:1 cycle runs beside the lifting block), so
            // only the lifting deload week is pinned there.
            let hybrid_run_recovery = lifting && running && !deload_expected;
            if (snap.phase == MesoPhase::Deload) != deload_expected && !hybrid_run_recovery {
                self.fail(
                    "deload cadence",
                    format!("week {}/{total} phase {:?}", snap.week, snap.phase),
                );
            }
            let repeating = vm.consistency.as_ref().is_some_and(|c| c.repeating_week);
            let want_week = if repeating {
                prev.week
            } else {
                prev.week % total + 1
            };
            if snap.week != want_week && self.today - self.last_delete_day > 21 {
                self.fail(
                    "deload cadence",
                    format!(
                        "week {} after {} (repeating {repeating})",
                        snap.week, prev.week
                    ),
                );
            }
        }
        if snap.phase == MesoPhase::Deload && prev.phase != MesoPhase::Deload {
            self.hit("deload depth");
            // RUN-DELOAD-DEPTH-001: the recovery week cuts 20–40 %.
            if running && !lifting && run_km > 0.8 * prev.run_km.max(prev2.run_km) + 0.5 {
                self.fail(
                    "deload depth",
                    format!("recovery week {run_km:.1} km after {:.1}", prev.run_km),
                );
            }
            // STR-DELOAD-VOL-001: about half the sets.
            if lifting && !running && f64::from(lift_sets) > 0.75 * f64::from(prev.lift_sets) {
                self.fail(
                    "deload depth",
                    format!("deload week {lift_sets} sets after {}", prev.lift_sets),
                );
            }
        }
    }

    /// Fail the test with every collected violation, then assert each named
    /// invariant was actually exercised and the view stayed fast.
    fn finish(&self, must_hit: &[&str]) {
        let avg = self.sum_view / self.n_views.max(1);
        if std::env::var_os("LONG_HORIZON_TRACE").is_some() {
            for (m, w) in &self.weeks {
                eprintln!("  week {:>3}: {w:?}", (m - START) / 7);
            }
            eprintln!(
                "[{}] {} views, max {:?}, mean {:?}, {} sets, {} runs, hits {:?}",
                self.name,
                self.n_views,
                self.max_view,
                avg,
                self.sets.len(),
                self.runs.len(),
                self.hits
            );
        }
        assert!(self.failures.is_empty(), "{}", self.failures.join("\n"));
        for c in must_hit {
            assert!(
                self.hits.get(c).is_some_and(|n| *n > 0),
                "[{}] invariant {c:?} never exercised: {:?}",
                self.name,
                self.hits
            );
        }
        // A re-entry invariant counts only if EVERY break of a week or more
        // was judged after its return, not just one of them (a break still
        // open at the end of the life counts only once judged).
        if must_hit
            .iter()
            .any(|c| matches!(*c, "re-entry" | "run re-entry"))
        {
            let open = self
                .reentry
                .as_ref()
                .filter(|re| re.lift_hits + re.run_hits > 0)
                .map(ReEntry::misses);
            let misses: Vec<&String> = self
                .reentry_misses
                .iter()
                .chain(open.iter().flatten())
                .collect();
            assert!(
                misses.is_empty(),
                "[{}] breaks whose return was never judged: {misses:?}",
                self.name,
            );
        }
        // A catastrophic-regression guard only (debug build, a loaded CI
        // box): the owner's 250 ms bound is pinned deterministically (per-view
        // name-classification and `SetIndex` build counts) on a dedicated
        // 3-year history by `view_time_stays_bounded_for_a_heavy_lifter_over_three_years`.
        assert!(
            self.max_view < Duration::from_secs(3),
            "[{}] slowest view {:?}",
            self.name,
            self.max_view
        );
    }
}

// ── Archetype lives ────────────────────────────────────────────────────────

fn runner(goal: GoalDistance, days: u8, km: f64) -> Profile {
    Profile {
        goal_distance: goal,
        weekly_sets: 0,
        running_days_per_week: days,
        running_km_per_week: km,
        concurrent_goal: ConcurrentGoal::EndurancePriority,
        ..base_profile()
    }
}

#[test]
fn beginner_runner_to_half_marathon_over_two_years() {
    let p = Profile {
        progression_cadence: ProgressionCadence::EverySession,
        stated_longest_recent_km: Some(4.0),
        goal_race_epoch_day: Some(START + 7 * 40 + 6),
        age_years: Some(29.0),
        ..runner(GoalDistance::HalfMarathon, 3, 12.0)
    };
    let mut sim = Sim::new("beginner-half", 0xbe61_0001, p, STEADY);
    sim.generate_plan();
    sim.run(730, |s, i| {
        match i {
            // Six months in, a week-to-week runner.
            182 => s.edit_profile(|p| p.progression_cadence = ProgressionCadence::WeekToWeek),
            // After the race: the next one a year later, a fourth run day.
            290 => s.edit_profile(|p| {
                p.goal_race_epoch_day = Some(START + 7 * 92 + 6);
                p.running_days_per_week = 4;
            }),
            // A ten-day holiday in the second summer.
            560 => s.go_away(10),
            _ => {}
        }
    });
    sim.finish(&[
        "spike ceiling",
        "run ramp",
        "run re-entry",
        "week strip",
        "history",
        "wire",
    ]);
}

fn lifter() -> Profile {
    Profile {
        weekly_sets: 14,
        lift_days_per_week: Some(4),
        bodyweight_kg: Some(82.0),
        age_years: Some(31.0),
        ..base_profile()
    }
}

fn hybrid() -> Profile {
    Profile {
        goal_distance: GoalDistance::TenK,
        concurrent_goal: ConcurrentGoal::Strength,
        running_days_per_week: 3,
        running_km_per_week: 25.0,
        stated_longest_recent_km: Some(8.0),
        lift_days_per_week: Some(3),
        weekly_sets: 12,
        bodyweight_kg: Some(76.0),
        age_years: Some(34.0),
        resting_hr_bpm: Some(54.0),
        ..base_profile()
    }
}

/// Simulated days: `short` by default, so every life stays well under a
/// minute in a debug build; `full` (one to three years) with
/// `LONG_HORIZON_FULL=1`, best in release:
/// `LONG_HORIZON_FULL=1 cargo test --release -p shared long_horizon`.
fn horizon(short: i64, full: i64) -> i64 {
    if std::env::var_os("LONG_HORIZON_FULL").is_some() {
        full
    } else {
        short
    }
}

#[test]
fn intermediate_lifter_strength_then_hypertrophy() {
    let days = horizon(200, 730);
    let mut sim = Sim::new("lifter", 0x11f7_0002, lifter(), STEADY);
    sim.generate_plan();
    sim.run(days, |s, i| {
        if i == days / 2 {
            s.edit_profile(|p| {
                p.lift_goal = LiftGoal::Hypertrophy;
                p.concurrent_goal = ConcurrentGoal::Hypertrophy;
                p.weekly_sets = 16;
            });
        }
    });
    sim.finish(&[
        "lift caps",
        "load step",
        "deload cadence",
        "deload depth",
        "week strip",
        "history",
        "wire",
        "units",
    ]);
}

#[test]
fn hybrid_athlete_in_utc_plus_one() {
    let days = horizon(170, 3 * 365);
    let mut sim = Sim::new("hybrid", 0x4b1d_0003, hybrid(), STEADY);
    // Berlin winter time: local days are UTC+1.
    sim.off = 3_600;
    sim.generate_plan();
    sim.run(days, |s, i| {
        if i == days * 2 / 5 {
            s.edit_profile(|p| p.running_km_per_week = 30.0);
        } else if i == days * 3 / 4 {
            s.go_away(6);
        }
    });
    sim.finish(&[
        "spike ceiling",
        "run ramp",
        "lift caps",
        "load step",
        "deload cadence",
        "week strip",
        "history",
        "wire",
    ]);
}

#[test]
fn calisthenics_athlete() {
    let p = Profile {
        bodyweight_kg: Some(70.0),
        ..calisthenics_profile()
    };
    let mut sim = Sim::new("calisthenics", 0xca15_0004, p, STEADY);
    sim.generate_plan();
    sim.run(horizon(130, 365), |_, _| {});
    sim.finish(&["deload cadence", "week strip", "history", "wire", "units"]);
}

#[test]
fn masters_runner_sixty_seven_eighteen_months() {
    let p = Profile {
        birth_epoch_day: Some(days_from_civil(1958, 9, 14)),
        stated_longest_recent_km: Some(7.0),
        resting_hr_bpm: Some(60.0),
        ..runner(GoalDistance::TenK, 3, 18.0)
    };
    let mut sim = Sim::new("masters", 0x6500_0005, p, STEADY);
    sim.generate_plan();
    sim.run(548, |_, _| {});
    // RUN-DELOAD-001: 2:1 for older athletes (masters 65+, MASTERS-001).
    assert!(
        sim.weeks
            .values()
            .any(|w| w.weeks_total == 3 && w.phase == MesoPhase::Deload),
        "a 67-year-old gets the 2:1 recovery cadence"
    );
    sim.finish(&[
        "spike ceiling",
        "run ramp",
        "deload cadence",
        "deload depth",
        "week strip",
        "history",
    ]);
}

#[test]
fn goal_and_unit_switcher() {
    let days = horizon(230, 730);
    let at = |k: i64| days * k / 6;
    let mut sim = Sim::new("switcher", 0x5017_0006, hybrid(), STEADY);
    sim.generate_plan();
    sim.run(days, |s, i| {
        if i == at(1) {
            s.edit_profile(|p| p.weight_lb = true);
        } else if i == at(2) {
            s.edit_profile(|p| {
                p.goal_distance = GoalDistance::Marathon;
                p.concurrent_goal = ConcurrentGoal::EndurancePriority;
                p.running_days_per_week = 4;
                p.running_km_per_week = 32.0;
            });
        } else if i == at(3) {
            s.edit_profile(|p| p.weight_lb = false);
        } else if i == at(4) {
            s.edit_profile(|p| {
                p.lift_goal = LiftGoal::Hypertrophy;
                p.concurrent_goal = ConcurrentGoal::Hypertrophy;
                p.goal_distance = GoalDistance::FiveK;
            });
        } else if i == at(5) {
            s.edit_profile(|p| {
                p.weight_lb = true;
                p.running_days_per_week = 5;
            });
        }
    });
    sim.finish(&[
        "units",
        "spike ceiling",
        "lift caps",
        "load step",
        "week strip",
        "history",
        "wire",
    ]);
}

#[test]
fn athlete_with_holidays_and_breaks() {
    // One week away, five weeks (Table 3.4b 4–8 wk), three weeks (2–4 wk:
    // rebuild the long run while the pre-break runs are still recent), ten
    // weeks (past 8 wk: novice restart, rebuild the base); the full life adds
    // a six-week break. Every return day judges both lifts and runs.
    let full = std::env::var_os("LONG_HORIZON_FULL").is_some();
    let (days, breaks): (i64, &[(i64, i64)]) = if full {
        (730, &[(90, 7), (200, 35), (340, 42), (420, 20), (500, 70)])
    } else {
        (330, &[(50, 7), (110, 35), (190, 20), (250, 70)])
    };
    let mut sim = Sim::new("breaks", 0x6a95_0007, hybrid(), STEADY);
    sim.generate_plan();
    sim.run(days, |s, i| {
        if let Some(&(_, len)) = breaks.iter().find(|b| b.0 == i) {
            s.go_away(len);
        }
    });
    sim.finish(&[
        "re-entry",
        "run re-entry",
        "lift caps",
        "load step",
        "spike ceiling",
        "week strip",
        "history",
    ]);
}

#[test]
fn illness_pain_and_reds_episodes_resolved() {
    let full = std::env::var_os("LONG_HORIZON_FULL").is_some();
    let days = if full { 540 } else { 260 };
    // A RED-S screen positive on a Sunday weekly check-in, cleared by the
    // check-in two weeks later (REDS-SCREEN-001).
    let sunday = |i: i64| i - wd(START + i) + 6;
    let mut sim = Sim::new("episodes", 0x111e_0008, hybrid(), STEADY);
    sim.generate_plan();
    sim.run(days, |s, i| match i {
        40 => s.start_hold(Hold::Illness),
        46 => s.resolve_hold(Hold::Illness),
        100 => s.start_hold(Hold::Pain),
        110 => s.resolve_hold(Hold::Pain),
        400 | 480 if full => s.start_hold(if i == 400 { Hold::Illness } else { Hold::Pain }),
        409 | 485 if full => s.resolve_hold(if i == 409 { Hold::Illness } else { Hold::Pain }),
        _ if i == sunday(170) => s.start_hold(Hold::Reds),
        _ if i == sunday(184) => s.resolve_hold(Hold::Reds),
        _ => {}
    });
    sim.finish(&[
        "hold blocks",
        "easy return",
        "hold lifts",
        "week strip",
        "history",
        "wire",
    ]);
}

#[test]
fn pregnancy_hold_then_return() {
    let p = Profile {
        female: Some(true),
        bodyweight_kg: Some(64.0),
        ..hybrid()
    };
    // Forty weeks pregnant, then the provider's all-clear eight weeks
    // postpartum: 336 days held (SAFE-PREG-001).
    let (from, days) = if std::env::var_os("LONG_HORIZON_FULL").is_some() {
        (120, 730)
    } else {
        (50, 440)
    };
    let mut sim = Sim::new("pregnancy", 0x9e60_0009, p, STEADY);
    sim.generate_plan();
    sim.run(days, |s, i| {
        if i == from {
            s.start_hold(Hold::Pregnancy);
        } else if i == from + 336 {
            s.resolve_hold(Hold::Pregnancy);
        }
    });
    sim.finish(&[
        "hold blocks",
        "gate lifts",
        "re-entry",
        "week strip",
        "history",
    ]);
}

/// The 18th birthday of a profile entered as 17 on day 21: the plan is
/// withheld (SAFE-PEDS-001) with the WHO activity card shown instead
/// (YOUTH-PA-001) until the birthday, then the plan resumes. The days before
/// the birthday stay held once it lifts the gate, never planned sessions
/// shown as missed.
fn youth_life(days: i64) -> Sim {
    let birth = days_from_civil(2008, 5, 20);
    let birthday = days_from_civil(2026, 5, 20);
    let p = Profile {
        age_years: None,
        ..hybrid()
    };
    let mut sim = Sim::new("youth", 0x0018_000a, p, STEADY);
    sim.generate_plan();
    sim.run(days, |s, i| {
        let d = START + i;
        if i == 21 {
            s.edit_profile(|p| p.birth_epoch_day = Some(birth));
            s.holds.insert(Hold::Youth);
            s.last_hold_day = d;
        }
        if s.holds.contains(&Hold::Youth) {
            if d == birthday {
                s.resolve_hold(Hold::Youth);
            } else if s.rng.chance(0.4) {
                // PE, sport, a run: the WHO card counts it all.
                let id = s.next_id();
                let strengthening = s.rng.chance(0.5);
                let observed_at = s.at(d, 15.0);
                s.send(Event::LogActivity {
                    kind: ActivityKind::Sport,
                    minutes: 60.0,
                    vigorous: true,
                    strengthening,
                    observed_at,
                    entry_id: id,
                });
                s.activities += 1;
                if s.rng.chance(0.3) {
                    s.log_run_km(4.0, 17.0, false);
                }
            }
        }
    });
    sim
}

#[test]
fn under_eighteen_turns_eighteen() {
    let sim = youth_life(horizon(190, 365));
    sim.finish(&["hold blocks", "gate lifts", "week strip", "history"]);
}

// ── Targeted scenarios ─────────────────────────────────────────────────────

/// The highest planned week km and long run over the last `n` weeks (a
/// recovery week among them does not hide the level reached).
fn peak_of_last(weeks: &[(f64, f64)], n: usize) -> (f64, f64) {
    weeks
        .iter()
        .rev()
        .take(n)
        .fold((0.0, 0.0), |a, b| (a.0.max(b.0), a.1.max(b.1)))
}

/// Planned run km of the rendered week and its longest planned run.
fn week_run_km(vm: &ViewModel) -> (f64, f64) {
    let runs = || {
        vm.week_plan
            .iter()
            .flat_map(|s| &s.items)
            .filter(|i| i.is_run())
    };
    (
        runs().map(item_km).sum(),
        runs().filter_map(|i| i.run_head_km).fold(0.0, f64::max),
    )
}

/// Replay `days` days of an athlete who runs exactly what the plan shows
/// (every run, at its distance), from `from`. Returns the (planned week km,
/// long run) of each Monday.
fn run_as_prescribed(
    app: &Engine,
    m: &mut Model,
    from: i64,
    days: i64,
    id: &mut u64,
) -> Vec<(f64, f64)> {
    let mut weeks = Vec::new();
    for d in from..from + days {
        let _ = app.update(set_today_ev(d), m);
        let vm = app.view(m);
        if wd(d) == 0 {
            weeks.push(week_run_km(&vm));
        }
        let Some(ns) = vm.next_session.filter(|ns| ns.epoch_day == d) else {
            continue;
        };
        for it in ns.items.iter().filter(|i| i.is_run()) {
            *id += 1;
            let km = round_to(item_km(it), 0.01);
            let _ = app.update(
                log_run(km, km * 6.5, 70.0, 0.0, d * DAY_SEC + 8 * 3_600, *id),
                m,
            );
        }
    }
    weeks
}

fn start_runner(p: Profile) -> (Engine, Model) {
    let app = Engine;
    let mut m = Model::default();
    for e in [
        set_today_ev(START),
        Event::SetProfile(Profile {
            effective_from_epoch_day: Some(START),
            ..p
        }),
        Event::GeneratePlan {
            start_epoch_day: START,
        },
    ] {
        let _ = app.update(e, &mut m);
    }
    (app, m)
}

/// RUN-VOLUME-001 (a plan keeps building volume across cycles, within the
/// RUN-PROGRESS-001 ramp) toward the half-marathon long run of 16–21 km
/// (RUN-LONGRUN-TARGET-001): a beginner who runs every session exactly as
/// shown must see the plan grow over 30 weeks.
#[test]
fn compliant_three_day_half_marathon_runner_keeps_building() {
    let (app, mut m) = start_runner(Profile {
        progression_cadence: ProgressionCadence::EverySession,
        stated_longest_recent_km: Some(4.0),
        ..runner(GoalDistance::HalfMarathon, 3, 12.0)
    });
    let weeks = run_as_prescribed(&app, &mut m, START, 7 * 30, &mut 1);
    // The first cycle's top week against the last cycle's.
    let (w0, l0) = peak_of_last(&weeks[..3], 3);
    let (w1, l1) = peak_of_last(&weeks, 3);
    assert!(
        w1 >= 1.3 * w0 && l1 >= 1.3 * l0,
        "30 compliant weeks: {w0:.1} km / {l0} km long run → {w1:.1} km / {l1} km; weeks {weeks:?}"
    );
}

/// File 08 Table 3.4b running re-entry: 4–8 weeks off → ~50–60 % of the
/// prior volume; more than 8 weeks → "rebuild base", with no fraction. So
/// nothing from before the break sizes the return (RUN-REENTRY-001): like a
/// new runner whose logged runs replace the numbers entered (RUN-START-001),
/// the first week back is the easy-run session floors (running-014/015,
/// prescribed by time) and builds from what the athlete then runs, never a
/// distance carried over from before, nor an invented share of it.
#[test]
fn return_after_ten_weeks_off_rebuilds_the_running_base() {
    let (app, mut m) = start_runner(Profile {
        stated_longest_recent_km: Some(10.0),
        ..runner(GoalDistance::TenK, 4, 30.0)
    });
    let mut id = 1;
    let before = run_as_prescribed(&app, &mut m, START, 70, &mut id);
    let (pre_week, pre_long) = peak_of_last(&before, 4);
    // Ten weeks with no app open and nothing logged; back on a Monday.
    let after = run_as_prescribed(&app, &mut m, START + 140, 8, &mut id);
    let (week, long) = after[0];
    assert!(
        long == 0.0 && week < pre_week,
        "before the break {pre_week:.1} km/wk, long run {pre_long} km; back after 70 days: \
         {week:.1} km/wk, a {long} km run sized by distance"
    );
    // A week later the plan builds from the running logged since the return.
    let (next_week, next_long) = after[1];
    assert!(
        next_long > 0.0 && next_week < pre_week,
        "second week back: {next_week:.1} km/wk, long run {next_long} km"
    );
}

/// File 08 Table 3.4b resistance re-entry: 1–2 weeks off → start at ~90 %
/// of the prior loads, 2–4 weeks → ~80–85 % (REENTRY-001 /
/// REENTRY-RAMP-001). A lifter who drops out for two weeks without telling
/// the app comes back to the plan's next progression step instead.
#[test]
fn return_after_two_weeks_off_starts_lifts_lighter() {
    let mut sim = Sim::new("lift-break", 0x11f7_00b2, lifter(), STEADY);
    sim.generate_plan();
    sim.run(80, |s, i| {
        if i == 55 {
            s.go_away(14);
        }
    });
    sim.finish(&["re-entry", "lift caps"]);
}

/// The same Table 3.4b resistance re-entry for a hypertrophy lifter, whose
/// isolation lifts carry no e1RM anchor: their last working load is carried
/// forward (HYP-DOUBLEPROG-001), and after three weeks off it restarts at
/// ~80–85 % like every anchored lift (REENTRY-001, REENTRY-RAMP-001).
#[test]
fn hypertrophy_lifter_restarts_carried_loads_lighter_after_three_weeks_off() {
    let p = Profile {
        lift_goal: LiftGoal::Hypertrophy,
        concurrent_goal: ConcurrentGoal::Hypertrophy,
        weekly_sets: 16,
        ..lifter()
    };
    let mut sim = Sim::new("hyp-break", 0x11f7_00b3, p, STEADY);
    sim.generate_plan();
    sim.run(80, |s, i| {
        if i == 50 {
            s.go_away(20);
        }
    });
    sim.finish(&["re-entry", "lift caps"]);
}

/// The owner's bound (a view under 250 ms in a debug build on a three-year
/// history), pinned deterministically: wall-clock time on a loaded test box
/// is not a gate. A lifter logging every set (four days a week, six
/// exercises, four sets each: ~15 000 sets in three years) is the heaviest
/// realistic history. What made the view slow was per-SET name
/// classification in every builder (~70 µs per set before `SetIndex`), so
/// the test counts that work per view instead of timing it:
/// - each distinct name is classified once per set-log change (`SetIndex`),
///   so a view's classifier calls ([`crate::plan::NAME_CLASSIFICATIONS`],
///   ~3 000, set by the plan's items) do not grow with the set count: on
///   the ≈15 000-set third year every view makes fewer than one call per two
///   sets (classifying each set even once would not);
/// - the index is memoized on the set log: a view after a new set builds it
///   once, a re-render after only the day changes builds it zero times
///   (`SET_INDEX_BUILDS`).
///
/// A wall-clock guard stays for a catastrophic regression only (10× the
/// owner's bound: a loaded debug box measured up to ~380 ms).
#[test]
fn view_time_stays_bounded_for_a_heavy_lifter_over_three_years() {
    use crate::app::view_ctx::SET_INDEX_BUILDS;
    use crate::plan::NAME_CLASSIFICATIONS;
    let app = Engine;
    let mut m = Model::default();
    let names = [
        "Back squat",
        "Bench press",
        "Cable row",
        "Deadlift",
        "Overhead press",
        "Seated leg curl",
        "Dumbbell curl",
        "Lateral raise",
    ];
    let send = |m: &mut Model, e: Event| {
        let _ = app.update(e, m);
    };
    let today = set_today_ev;
    // (SetIndex builds, name classifications, wall time) of one view.
    let measured = |m: &Model| {
        let (b0, c0) = (
            SET_INDEX_BUILDS.with(|c| c.get()),
            NAME_CLASSIFICATIONS.with(|c| c.get()),
        );
        let t = Instant::now();
        let vm = app.view(m);
        let wall = t.elapsed();
        let builds = SET_INDEX_BUILDS.with(|c| c.get()) - b0;
        let calls = NAME_CLASSIFICATIONS.with(|c| c.get()) - c0;
        (vm, builds, calls, wall)
    };
    send(&mut m, today(START));
    send(
        &mut m,
        Event::SetProfile(Profile {
            lift_goal: LiftGoal::Hypertrophy,
            concurrent_goal: ConcurrentGoal::Hypertrophy,
            weekly_sets: 16,
            effective_from_epoch_day: Some(START),
            ..lifter()
        }),
    );
    send(
        &mut m,
        Event::GeneratePlan {
            start_epoch_day: START,
        },
    );
    let mut id = 0u64;
    // Per year: (sets, [builds, classifier calls, wall] of the view after the
    // day changes, the refresh after one more set, the next day's re-render).
    let mut rows = Vec::new();
    for year in 1..=3 {
        for d in START + 365 * (year - 1)..START + 365 * year {
            send(&mut m, today(d));
            if !matches!(wd(d), 0 | 1 | 3 | 4) {
                continue;
            }
            let prog = 1.0 + 0.0005 * (d - START) as f64;
            for k in 0..6 {
                let name = names[(k + 2 * wd(d) as usize) % names.len()];
                let kg = round_to(first_load(name) * prog, 2.5);
                for s in 0..4 {
                    id += 1;
                    let at = d * DAY_SEC + 64_800 + (k * 4 + s) as i64 * 150;
                    send(&mut m, with_entry_id(log_set(name, kg, 10, 8.0, at), id));
                }
            }
        }
        let d = START + 365 * year;
        send(&mut m, today(d));
        let (vm, b1, c1, w1) = measured(&m);
        assert_eq!(vm.lifts.len() as u64, id);
        id += 1;
        let at = d * DAY_SEC + 40_000;
        send(
            &mut m,
            with_entry_id(log_set("Bench press", 60.0, 10, 8.0, at), id),
        );
        let (_, b2, c2, w2) = measured(&m);
        send(&mut m, today(d + 1));
        let (_, b3, c3, w3) = measured(&m);
        rows.push((id, [(b1, c1, w1), (b2, c2, w2), (b3, c3, w3)]));
    }
    for (sets, views) in &rows {
        let [(b1, ..), (b2, ..), (b3, ..)] = *views;
        assert!(
            (b1, b2, b3) == (1, 1, 0),
            "{sets} sets: SetIndex builds (new sets, one more set, day change only) = {:?}, want (1, 1, 0): {rows:?}",
            (b1, b2, b3)
        );
    }
    let (sets3, year3) = rows[2];
    let calls3 = year3.iter().map(|v| v.1).max().unwrap_or(0);
    assert!(
        calls3 * 2 < sets3,
        "a view classifies exercise names per set: {calls3} classifier calls on {sets3} sets: {rows:?}"
    );
    let slowest = rows
        .iter()
        .flat_map(|(_, v)| v.iter().map(|x| x.2))
        .max()
        .unwrap_or_default();
    assert!(
        slowest < Duration::from_millis(2500),
        "slowest view {slowest:?}: {rows:?}"
    );
}
