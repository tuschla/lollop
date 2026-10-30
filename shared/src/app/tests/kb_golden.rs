//! KB golden plans: canonical athletes follow their plan exactly for 12–16
//! weeks through the real `update` / `view` (every prescribed set at its
//! load, reps and target effort, every run at its distance), and each week
//! is checked against the knowledge-base table or rule the plan claims to
//! follow. Every expected number cites its KB source as
//! `<file under planning/knowledge-base>:<line>`.
//!
//! Lives are simulated once per process (`LazyLock`) and shared by the tests
//! that read them.

use super::*;
use crate::app::format::mon0_weekday;
use crate::evidence::EvidenceEntry;
use crate::plan::is_quality_run;
use crate::schema::{LiftSessionType, RunSessionType};
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// Easy pace (min/km) the plan counts unanchored minutes at
/// (extracted/13-running-update.md:115: "count these minutes at 6:30/km").
const ACCOUNTING_MIN_PER_KM: f64 = 6.5;

fn week_of(d: i64) -> usize {
    ((d - DAY0) / 7) as usize
}

// ── The simulated life ─────────────────────────────────────────────────────

/// One plan week as the athlete saw it on its Monday.
#[derive(Clone, Debug)]
struct WeekRec {
    monday: i64,
    rows: Vec<SessionPlanView>,
    program: Option<ProgramSummaryView>,
}

/// A trained day's strip row as the evening view showed it (after logging).
#[derive(Clone, Debug)]
struct DayRec {
    day: i64,
    row: SessionPlanView,
    /// Warm-up sets logged before each exercise's first work set.
    warm_ups: BTreeMap<String, usize>,
    /// Performance-drop cuts in force after the day's sets: lifts the anchors
    /// derate for an e1RM drop (AUTOREG-E1RM-GATE-001) and readiness
    /// adjustments citing that gate (today's per-item cut).
    perf_cuts: Vec<String>,
}

#[derive(Debug)]
struct Life {
    weeks: Vec<WeekRec>,
    days: Vec<DayRec>,
}

impl Life {
    fn week(&self, i: usize) -> &WeekRec {
        &self.weeks[i]
    }

    fn days_of_week(&self, i: usize) -> impl Iterator<Item = &DayRec> {
        self.days.iter().filter(move |d| week_of(d.day) == i)
    }
}

/// A canonical athlete: profile, history logged before the plan, weeks.
struct Athlete {
    profile: Profile,
    history: Vec<Event>,
    weeks: i64,
    /// Log the STR-WARMUP-001 ramp before each loaded barbell lift.
    warm_ups: bool,
}

/// Starting guess for a never-logged movement, kg (a shell prefill).
fn first_load(ex: &str) -> f64 {
    let e = ex.to_lowercase();
    if e.contains("deadlift") {
        100.0
    } else if e.contains("squat") {
        80.0
    } else if e.contains("bench") {
        60.0
    } else if e.contains("row") {
        50.0
    } else if e.contains("press") {
        40.0
    } else if e.contains("curl") || e.contains("raise") || e.contains("fly") {
        10.0
    } else {
        25.0
    }
}

/// Main-set distance of a run item, km: the plain distance head, else the
/// rep volume, else the run/walk bouts or the minutes the summary leads with
/// at the accounting pace. Warm-up and cool-down are not included.
fn main_km(it: &PrescriptionView) -> f64 {
    if let Some(k) = it.run_head_km {
        return k;
    }
    if let Some(rs) = &it.run_structure {
        if rs.reps > 0 && rs.rep_m > 0 {
            return f64::from(rs.reps * rs.rep_m) / 1000.0;
        }
        if !rs.run_walk.is_empty() {
            let run: u32 = rs.run_walk.iter().map(|s| s.run_sec).sum();
            return f64::from(run) / 60.0 / ACCOUNTING_MIN_PER_KM;
        }
    }
    let head: String = it.summary.chars().take_while(|c| *c != '·').collect();
    head.find("min")
        .and_then(|i| {
            head[..i]
                .split(|c: char| !c.is_ascii_digit() && c != '.')
                .filter_map(|t| t.parse::<f64>().ok())
                .next_back()
        })
        .map_or(0.0, |m| m / ACCOUNTING_MIN_PER_KM)
}

/// Warm-up + cool-down of a run item, km at the accounting pace
/// (extracted/13-running-update.md:114-115: both count toward the week).
fn wucd_km(it: &PrescriptionView) -> f64 {
    it.run_structure.as_ref().map_or(0.0, |rs| {
        f64::from(rs.warmup_min + rs.cooldown_min) / ACCOUNTING_MIN_PER_KM
    })
}

fn run_kind(row: &SessionPlanView) -> Option<RunSessionType> {
    match row.session_type {
        SessionType::Run(k) => Some(k),
        _ => None,
    }
}

fn is_lift(row: &SessionPlanView) -> bool {
    matches!(row.session_type, SessionType::Lift(_))
}

/// The STR-WARMUP-001 ramp before a work set at `work` kg
/// (extracted/13-logging-warmup.md:12: ≈40 % × 5–8 → 60 % × 3–5 → 80 % × 1–3
/// of the working load), on the 2.5 kg grid, steps at or above the work load
/// or at or below the empty bar dropped.
fn ramp(work: f64) -> Vec<(f64, u32)> {
    [(0.4, 5), (0.6, 3), (0.8, 2)]
        .into_iter()
        .map(|(f, r)| (((work * f) / 2.5).round() * 2.5, r))
        .filter(|(kg, _)| *kg > 20.0 && *kg < work)
        .collect()
}

fn simulate(a: Athlete) -> Life {
    let app = Engine;
    let mut m = Model::default();
    let mut id = 0u64;
    let mut send = |m: &mut Model, mut ev: Event| {
        id += 1;
        match &mut ev {
            Event::LogSet { entry_id, .. } | Event::LogRun { entry_id, .. } => *entry_id = id,
            _ => {}
        }
        let _ = app.update(ev, m);
    };
    send(&mut m, set_today_ev(DAY0 - 14));
    send(
        &mut m,
        Event::SetProfile(Profile {
            effective_from_epoch_day: Some(DAY0 - 14),
            ..a.profile
        }),
    );
    for ev in a.history {
        send(&mut m, ev);
    }
    send(&mut m, set_today_ev(DAY0));
    send(
        &mut m,
        Event::GeneratePlan {
            start_epoch_day: DAY0,
        },
    );
    let mut last_kg: BTreeMap<String, f64> = BTreeMap::new();
    let (mut weeks, mut days) = (Vec::new(), Vec::new());
    for d in DAY0..DAY0 + 7 * a.weeks {
        send(&mut m, set_today_ev(d));
        let vm = app.view(&m);
        if mon0_weekday(d) == 0 {
            weeks.push(WeekRec {
                monday: d,
                rows: vm.week_plan.clone(),
                program: vm.program.clone(),
            });
        }
        let Some(ns) = vm.next_session.filter(|s| s.epoch_day == d) else {
            continue;
        };
        let mut t = d * DAY_SEC + 17 * 3_600;
        let mut warm_ups = BTreeMap::new();
        for it in &ns.items {
            if it.is_run() {
                let km = ((main_km(it) + wucd_km(it)) * 100.0).round() / 100.0;
                send(
                    &mut m,
                    log_run(km, km * ACCOUNTING_MIN_PER_KM, 72.0, 0.0, t, 0),
                );
                t += 3_600;
                continue;
            }
            let rpe = it.target_rpe.unwrap_or(8.0);
            if it.timed_hold {
                for _ in 0..it.sets.max(1) {
                    send(
                        &mut m,
                        cal_hold(&it.exercise, it.hold_high_sec.max(1), rpe, t),
                    );
                    t += 180;
                }
                continue;
            }
            let reps = u32::from(it.reps_high.max(it.reps_low).max(1));
            let kg = it
                .load_kg
                .or(it.suggested_load_kg)
                .or(it.added_kg)
                .or_else(|| last_kg.get(&it.exercise).copied())
                .unwrap_or_else(|| first_load(&it.exercise));
            last_kg.insert(it.exercise.clone(), kg);
            if a.warm_ups
                && let Some(work) = it.load_kg
                && crate::plan::is_barbell_lift(&it.exercise)
            {
                let steps = ramp(work);
                warm_ups.insert(it.exercise.clone(), steps.len());
                for (w, r) in steps {
                    // Warm-ups go unrated (extracted/13-logging-warmup.md:25).
                    send(&mut m, log_set(&it.exercise, w, r, 0.0, t));
                    t += 90;
                }
            }
            for _ in 0..it.sets.max(1) {
                send(&mut m, log_set(&it.exercise, kg, reps, rpe, t));
                t += i64::from(it.rest_sec.max(60));
            }
        }
        let vm = app.view(&m);
        if let Some(row) = vm.week_plan.into_iter().find(|r| r.epoch_day == d) {
            days.push(DayRec {
                day: d,
                row,
                warm_ups,
                perf_cuts: perf_cuts(&m, d),
            });
        }
    }
    Life { weeks, days }
}

/// The performance-drop cuts in force on day `d` after its sets are logged:
/// the lifts whose anchors carry an e1RM-drop derate, and the readiness
/// adjustments citing the e1RM gate (AUTOREG-E1RM-GATE-001; its +3.5 %
/// suggestion aside) or deloading for an e1RM decline (autoreg-022).
fn perf_cuts(m: &Model, d: i64) -> Vec<String> {
    use crate::schema::Adjustment;
    let cx = ViewCtx::new(m);
    let mut cuts: Vec<String> = cx
        .lift_anchors_now()
        .lift_derates
        .iter()
        .filter(|r| matches!(r.cause, crate::plan::DerateCause::E1rmDrop))
        .map(|r| format!("{} derated to {:.0}%", r.exercise, r.frac * 100.0))
        .collect();
    let increase = |a: &Adjustment| match a {
        Adjustment::ForItem { adjustment, .. } => {
            matches!(**adjustment, Adjustment::IncreaseLoadPct(_))
        }
        a => matches!(a, Adjustment::IncreaseLoadPct(_)),
    };
    let picture = readiness_picture(m, &cx.days, m.profile(), Some(d));
    cuts.extend(
        picture
            .adjustments(m, &cx.days)
            .iter()
            .filter(|a| {
                a.claim_id() == claims::AUTOREG_E1RM_GATE_001 && !increase(&a.value)
                    || a.claim_id() == claims::AUTOREG_DELOAD_001
            })
            .map(|a| format!("readiness {:?}", a.value)),
    );
    cuts
}

/// The five barbell lifts a trained lifter has logged three days before the
/// plan: each anchors a load (HARD RULE 1, loads only from logged history).
fn trained_lifts(day: i64) -> Vec<Event> {
    [
        ("Back Squat", 100.0),
        ("Bench Press", 75.0),
        ("Deadlift", 130.0),
        ("Overhead Press", 45.0),
        ("Barbell Row", 65.0),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (ex, kg))| log_set(ex, kg, 5, 8.0, day * DAY_SEC + 64_800 + 200 * i as i64))
    .collect()
}

macro_rules! life {
    ($fn:ident, $athlete:expr) => {
        fn $fn() -> &'static Life {
            static L: LazyLock<Life> = LazyLock::new(|| simulate($athlete));
            &L
        }
    };
}

life!(
    beginner_5k,
    Athlete {
        profile: Profile {
            goal_distance: GoalDistance::C25k,
            weekly_sets: 0,
            running_days_per_week: 3,
            running_km_per_week: 0.0,
            progression_cadence: ProgressionCadence::EverySession,
            concurrent_goal: ConcurrentGoal::EndurancePriority,
            age_years: Some(35.0),
            ..base_profile()
        },
        history: vec![],
        weeks: 12,
        warm_ups: false,
    }
);

/// A 10K race on the Sunday of plan week 12.
const TEN_K_RACE: i64 = DAY0 + 7 * 12 + 6;

life!(
    ten_k,
    Athlete {
        profile: Profile {
            goal_distance: GoalDistance::TenK,
            weekly_sets: 0,
            running_days_per_week: 4,
            running_km_per_week: 40.0,
            stated_longest_recent_km: Some(12.0),
            concurrent_goal: ConcurrentGoal::EndurancePriority,
            goal_race_epoch_day: Some(TEN_K_RACE),
            age_years: Some(38.0),
            ..base_profile()
        },
        history: vec![],
        weeks: 13,
        warm_ups: false,
    }
);

/// A marathon on the Sunday of plan week 16.
const MARATHON_RACE: i64 = DAY0 + 7 * 16 + 6;

life!(
    marathoner,
    Athlete {
        profile: Profile {
            goal_distance: GoalDistance::Marathon,
            weekly_sets: 0,
            running_days_per_week: 5,
            running_km_per_week: 70.0,
            stated_longest_recent_km: Some(22.0),
            concurrent_goal: ConcurrentGoal::EndurancePriority,
            progression_cadence: ProgressionCadence::MonthToMonth,
            advanced: true,
            goal_race_epoch_day: Some(MARATHON_RACE),
            age_years: Some(41.0),
            ..base_profile()
        },
        history: vec![],
        weeks: 17,
        warm_ups: false,
    }
);

life!(
    novice_lifter,
    Athlete {
        profile: Profile {
            progression_cadence: ProgressionCadence::EverySession,
            lift_days_per_week: Some(3),
            bodyweight_kg: Some(78.0),
            age_years: Some(24.0),
            ..strength_profile()
        },
        history: vec![],
        weeks: 12,
        warm_ups: true,
    }
);

life!(
    hypertrophy_lifter,
    Athlete {
        profile: Profile {
            lift_goal: LiftGoal::Hypertrophy,
            concurrent_goal: ConcurrentGoal::Hypertrophy,
            weekly_sets: 16,
            lift_days_per_week: Some(4),
            age_years: Some(29.0),
            ..base_profile()
        },
        history: trained_lifts(DAY0 - 3),
        weeks: 13,
        warm_ups: false,
    }
);

/// A powerlifting meet on the Saturday of plan week 12.
const MEET: i64 = DAY0 + 7 * 12 + 5;

life!(
    powerlifter,
    Athlete {
        profile: Profile {
            progression_cadence: ProgressionCadence::MonthToMonth,
            advanced: true,
            lift_days_per_week: Some(4),
            meet_epoch_day: Some(MEET),
            age_years: Some(30.0),
            ..strength_profile()
        },
        history: trained_lifts(DAY0 - 3),
        weeks: 13,
        warm_ups: true,
    }
);

life!(
    hybrid_athlete,
    Athlete {
        profile: Profile {
            goal_distance: GoalDistance::TenK,
            concurrent_goal: ConcurrentGoal::Strength,
            running_days_per_week: 3,
            running_km_per_week: 25.0,
            stated_longest_recent_km: Some(8.0),
            lift_days_per_week: Some(3),
            weekly_sets: 12,
            age_years: Some(34.0),
            ..base_profile()
        },
        history: trained_lifts(DAY0 - 3),
        weeks: 12,
        warm_ups: true,
    }
);

life!(
    calisthenics_beginner,
    Athlete {
        profile: Profile {
            progression_cadence: ProgressionCadence::EverySession,
            bodyweight_kg: Some(70.0),
            ..calisthenics_profile()
        },
        history: vec![],
        weeks: 12,
        warm_ups: false,
    }
);

life!(
    masters_67,
    Athlete {
        profile: Profile {
            age_years: Some(67.0),
            running_days_per_week: 3,
            running_km_per_week: 20.0,
            stated_longest_recent_km: Some(7.0),
            lift_days_per_week: Some(2),
            weekly_sets: 10,
            ..base_profile()
        },
        history: trained_lifts(DAY0 - 3),
        weeks: 12,
        warm_ups: false,
    }
);

fn female_lifter_profile(female: bool) -> Profile {
    Profile {
        female: Some(female),
        lift_days_per_week: Some(3),
        age_years: Some(27.0),
        ..strength_profile()
    }
}

life!(
    female_lifter,
    Athlete {
        profile: female_lifter_profile(true),
        history: trained_lifts(DAY0 - 3),
        weeks: 12,
        warm_ups: false,
    }
);

// The female lifter's twin: four weeks show the plan does not read the sex.
life!(
    male_twin,
    Athlete {
        profile: female_lifter_profile(false),
        history: trained_lifts(DAY0 - 3),
        weeks: 4,
        warm_ups: false,
    }
);

// ── Running week arithmetic ────────────────────────────────────────────────

/// One planned week's running, as the plan shows it on Monday.
#[derive(Debug, Default)]
struct RunWeek {
    /// Main sets + warm-up/cool-down, km.
    total_km: f64,
    /// Easy-intensity km: easy, recovery and long runs plus every warm-up and
    /// cool-down.
    easy_km: f64,
    long_km: Option<f64>,
    long_citation: String,
    run_days: usize,
    quality_days: Vec<(i64, RunSessionType)>,
    tempo_km: f64,
    interval_km: f64,
    /// The longest easy run's main set, km.
    longest_easy_km: f64,
}

fn run_week(rows: &[SessionPlanView]) -> RunWeek {
    let mut w = RunWeek::default();
    for row in rows {
        let Some(kind) = run_kind(row) else { continue };
        if row.items.iter().any(PrescriptionView::is_run) {
            w.run_days += 1;
        }
        for it in row.items.iter().filter(|i| i.is_run()) {
            let (main, wc) = (main_km(it), wucd_km(it));
            w.total_km += main + wc;
            w.easy_km += wc;
            match kind {
                k if is_quality_run(k) => {
                    w.quality_days.push((row.epoch_day, k));
                    match k {
                        RunSessionType::Tempo => w.tempo_km += main,
                        RunSessionType::Interval => {
                            w.interval_km += main;
                            // The jog between reps (as long as the rep took,
                            // 04-running.md:207-208) is easy running; counted
                            // conservatively at half the rep distance.
                            w.total_km += 0.5 * main;
                            w.easy_km += 0.5 * main;
                        }
                        _ => {}
                    }
                }
                RunSessionType::LongRun => {
                    w.easy_km += main;
                    w.long_km = Some(main);
                    w.long_citation = it.evidence.citation.clone();
                }
                RunSessionType::Easy => {
                    w.easy_km += main;
                    w.longest_easy_km = w.longest_easy_km.max(main);
                }
                _ => w.easy_km += main,
            }
        }
    }
    w
}

fn phase(w: &WeekRec) -> Option<MesoPhase> {
    w.program.as_ref().map(|p| p.phase)
}

/// The claims whose lead citation is `citation` (what a view row names).
fn claims_citing(citation: &str) -> Vec<&'static EvidenceEntry> {
    crate::evidence::all_claims()
        .filter(|c| c.reference() == citation)
        .collect()
}

/// A view row citing `citation` may stand for claim `id`.
fn cites(citation: &str, id: &str) -> bool {
    claims_citing(citation).iter().any(|c| c.claim_id == id)
}

/// The long-run ceiling: the share (long-run-weekly-volume-share.md:16-18:
/// ≤30 % under 64 km/wk, ≤25 % at or above it), raised to the week's average
/// run when that is larger (a week's longest run is never below its average;
/// at ≤3 run days the average is ≥33 %, open owner decision 3 in BUGS.md
/// 2026-10-25), and never above the low-mileage guardrail
/// (extracted/04-running.md:175: "low-mileage guardrail LR ≤2× average daily
/// run"); half a kilometre of plan rounding.
fn long_run_ceiling(w: &RunWeek) -> f64 {
    let share = if w.total_km < 64.0 { 0.30 } else { 0.25 };
    let daily_avg = w.total_km / w.run_days.max(1) as f64;
    (share * w.total_km).max(daily_avg).min(2.0 * daily_avg) + 0.5
}

/// Weeks whose long run rests on the athlete's own longest run
/// (RUN-SPIKE-001 / RUN-START-001, the demonstrated-capacity basis) rather
/// than the share rule.
fn long_run_on_capacity(w: &RunWeek) -> bool {
    cites(&w.long_citation, "RUN-SPIKE-001") || cites(&w.long_citation, "RUN-START-001")
}

/// The race-distance runner checks every life shares: ramp, share, intensity
/// distribution, quality spacing and caps, recovery-week cadence and depth,
/// and the single-run spike ceiling over `stated_longest` and the runs logged.
fn check_race_runner(
    name: &str,
    life: &Life,
    race: i64,
    load_weeks: usize,
    taper_weeks: usize,
    stated_longest: f64,
) {
    let race_week = week_of(race);
    assert!(
        life.weeks.len() > race_week,
        "{name}: the life ends in week {} before its race week {race_week}",
        life.weeks.len() - 1
    );
    let weeks: Vec<(usize, RunWeek, Option<MesoPhase>)> = life
        .weeks
        .iter()
        .take(race_week + 1)
        .map(|w| (week_of(w.monday), run_week(&w.rows), phase(w)))
        .collect();
    // RUN-SPIKE-001: no single run >10 % beyond the longest run of the prior
    // 30 days (extracted/04-running.md:317-318); the athlete logged every run
    // as planned (main set plus warm-up/cool-down).
    for w in life.weeks.iter().take(race_week + 1) {
        for row in w.rows.iter().filter(|r| run_kind(r).is_some()) {
            let base = life
                .days
                .iter()
                .filter(|d| d.day < w.monday && w.monday - d.day <= 30)
                .flat_map(|d| d.row.items.iter().filter(|i| i.is_run()))
                .map(|i| main_km(i) + wucd_km(i))
                .fold(stated_longest, f64::max);
            for it in row.items.iter().filter(|i| i.is_run()) {
                assert!(
                    main_km(it) <= 1.10 * base + 0.1,
                    "{name} day {}: {} over 110 % of the 30-day longest {base:.1} km (04-running.md:317-318)",
                    row.epoch_day - DAY0,
                    it.summary
                );
            }
        }
    }
    let mut last_load: Option<f64> = None;
    for (i, w, ph) in &weeks {
        let ctx = format!("{name} week {i} ({ph:?}): {w:?}");
        let taper = *i + taper_weeks > race_week;
        assert_eq!(
            taper,
            *ph == Some(MesoPhase::Taper),
            "{ctx}: the taper covers the last {taper_weeks} weeks before the race \
             (extracted/13-running-update.md:140: taper weeks = ceil(running-038 upper days / 7))"
        );
        let recovery = *ph == Some(MesoPhase::Deload);
        // 3:1 load:recovery (extracted/04-running.md:361-362), aligned so the
        // last pre-taper week is a top load week (13-running-update.md:141).
        let k = race_week - taper_weeks - i.min(&(race_week - taper_weeks));
        if !taper {
            assert_eq!(
                recovery,
                k % (load_weeks + 1) == load_weeks,
                "{ctx}: recovery weeks every {}th week counted back from the taper \
                 (04-running.md:361-362, 13-running-update.md:141)",
                load_weeks + 1
            );
        }
        // Quality: ≤2 a week for 5K–marathon goals (04-running.md:263),
        // ≥48 h apart (04-running.md:251-252), none in a recovery week beyond
        // the one it drops (04-running.md:362; the plan drops all).
        assert!(
            w.quality_days.len() <= 2,
            "{ctx}: at most 2 quality sessions (04-running.md:263)"
        );
        for pair in w.quality_days.windows(2) {
            assert!(
                pair[1].0 - pair[0].0 >= 2,
                "{ctx}: quality sessions ≥48 h apart (04-running.md:251-252)"
            );
        }
        if recovery {
            assert!(
                w.quality_days.is_empty(),
                "{ctx}: a recovery week drops its hard sessions (04-running.md:362)"
            );
        }
        // Weekly caps by intensity (04-running.md:485-486): T ≤10 %, I ≤8 %
        // (half a kilometre of plan rounding). Not judged in taper weeks: the
        // KB states these caps with no taper exception, while running-037/038
        // hold intensity and frequency and keep 1–2 short sharp sessions as
        // the volume drops 41–60 % (04-running.md:405-406, :416-417). The KB
        // conflicts with itself there.
        if !taper {
            assert!(
                w.tempo_km <= 0.10 * w.total_km + 0.5,
                "{ctx}: threshold ≤10 % of the week (04-running.md:485)"
            );
            assert!(
                w.interval_km <= 0.08 * w.total_km + 0.5,
                "{ctx}: intervals ≤8 % of the week (04-running.md:486)"
            );
        }
        // ~80 % easy by time (04-running.md:119-120); Daniels' own split
        // allows 70–80 % E (04-running.md:470), and a marathon week's M share
        // of 15–20 % (13-running-update.md:153) plus T ≤10 % can only fit the
        // 70 % end (a KB-internal conflict, 13-running-update.md:158).
        // Distance share is a lower bound on the time share (quality runs
        // faster than easy). Not judged in taper weeks: running-011 says
        // "always ~80% easy by time" (04-running.md:119-120) with no taper
        // exception, while running-037/038 keep the quality sessions at their
        // minimum (the 8 km MP block, 04-running.md:185-186; 20-min tempo,
        // :196-197) as the volume drops 41–60 % (04-running.md:405-406,
        // :416-417). The KB conflicts with itself there.
        if !taper {
            assert!(
                w.easy_km >= 0.70 * w.total_km,
                "{ctx}: easy share {:.0} % below 70–80 % E (04-running.md:119-120, :470)",
                100.0 * w.easy_km / w.total_km
            );
        }
        // Taper weeks before the race week cut 41–60 % from the last load
        // week (04-running.md:405-406, :417; half a kilometre of plan rounding).
        if taper
            && *i < race_week
            && let Some(prev) = last_load
        {
            assert!(
                (0.40 * prev - 0.5..=0.59 * prev + 0.5).contains(&w.total_km),
                "{ctx}: taper week {:.1} km is a {:.0} % cut from {prev:.1} km (41–60 %, 04-running.md:405-406)",
                w.total_km,
                100.0 * (1.0 - w.total_km / prev)
            );
        }
        // Long run: the share ceiling or the low-mileage guardrail
        // (04-running.md:175; long-run-weekly-volume-share.md:16-18) on the
        // prescribed week, unless it rests on the athlete's own longest run
        // (RUN-SPIKE-001). Exempt only a week whose easy day is capped at the
        // long run's length: the long-run share is 25–30 % of the week
        // (04-running.md:482, :174-175), and the engine's "no easy day longer
        // than the long run" comes from the same E-run single-run cap (:464).
        // With 2 quality sessions plus warm-up/cool-down and one easy day held
        // to the long run, those caps limit the week to about 2.5 × the
        // quality km, so it holds less than planned and the long run's share
        // of it reads high.
        if let Some(long) = w.long_km
            && !long_run_on_capacity(w)
            && (w.longest_easy_km - long).abs() > 0.05
        {
            assert!(
                long <= long_run_ceiling(w),
                "{ctx}: long run {long} km over the share ceiling (04-running.md:175)"
            );
        }
        if !taper && !recovery {
            // Weekly ramp: 5–10 %/wk (04-running.md:339-340), judged on the
            // previous load week (a recovery week in between is not the base).
            if let Some(prev) = last_load {
                assert!(
                    w.total_km <= prev * 1.10 + 0.6,
                    "{ctx}: {:.1} km after {prev:.1} km: more than +10 %/wk (04-running.md:339-340)",
                    w.total_km
                );
            }
            last_load = Some(w.total_km);
        }
        if recovery && let Some(prev) = last_load {
            // Recovery-week cut 20–40 % (10–30 % high mileage, up to 50 % low
            // mileage) (04-running.md:362).
            let cut = 1.0 - w.total_km / prev;
            assert!(
                (0.10..=0.50).contains(&cut),
                "{ctx}: recovery-week cut {:.0} % outside 10–50 % (04-running.md:362)",
                100.0 * cut
            );
        }
    }
}

/// Every run row's face states its KB band (04-running.md:152-208) and cites
/// a claim whose statement carries that band.
fn check_run_bands(name: &str, life: &Life) {
    let bands: &[(RunSessionType, &str, &str)] = &[
        // (kind, band on the face, claim stating it)
        (RunSessionType::Easy, "65–79% HRmax", "RUN-EASY-001"), // 04-running.md:163-164
        (RunSessionType::Tempo, "88–92% HRmax", "RUN-TEMPO-001"), // 04-running.md:196-197
        (RunSessionType::RacePace, "80–85% HRmax", "RUN-RACEPACE-001"), // 04-running.md:185-186
        (RunSessionType::Recovery, "≤ 76% HRmax", "RUN-RECOVERY-001"), // 04-running.md:152-153
    ];
    for w in &life.weeks {
        for row in &w.rows {
            let Some(kind) = run_kind(row) else { continue };
            for it in row
                .items
                .iter()
                .filter(|i| i.is_run() && !i.summary.contains("run /"))
            {
                let ctx = format!(
                    "{name} {} {}: {}",
                    row.epoch_day - DAY0,
                    row.title,
                    it.summary
                );
                if let Some((_, band, id)) = bands.iter().find(|(k, ..)| *k == kind) {
                    assert!(it.summary.contains(band), "{ctx}: face lacks {band}");
                    let stated = crate::evidence::all_claims()
                        .find(|c| c.claim_id == *id)
                        .unwrap_or_else(|| panic!("no claim {id}"))
                        .statement;
                    let num = band.trim_end_matches(" HRmax").trim_start_matches("≤ ");
                    assert!(stated.contains(num), "{id} does not state {num}: {stated}");
                    // Outside a recovery week, a taper and a readiness change
                    // the row's citation can stand for that claim (or for the
                    // spike rule that degraded it).
                    let plain = !matches!(phase(w), Some(MesoPhase::Deload | MesoPhase::Taper))
                        && row.adjustment.is_none();
                    if plain {
                        let c = &it.evidence.citation;
                        assert!(
                            cites(c, id) || cites(c, "RUN-SPIKE-001") || cites(c, "RUN-START-001"),
                            "{ctx}: cites {c:?}, which is not {id}"
                        );
                    }
                }
                if is_quality_run(kind) && kind != RunSessionType::Interval {
                    // RUN-WARMUP-001: 10 min easy before, ~10 min after
                    // (extracted/13-running-update.md:114).
                    let rs = it.run_structure.as_ref().expect("quality run structure");
                    assert_eq!((rs.warmup_min, rs.cooldown_min), (10, 10), "{ctx}");
                }
                if kind == RunSessionType::Interval {
                    // Reps of 800–1600 m, recovery ≈ rep time
                    // (04-running.md:207-208); RUN-WARMUP-001 around them.
                    let rs = it.run_structure.as_ref().expect("interval structure");
                    assert!(
                        (800..=1600).contains(&rs.rep_m),
                        "{ctx}: rep {} m",
                        rs.rep_m
                    );
                    assert!(rs.recovery_equal_to_rep || rs.recovery_sec > 0, "{ctx}");
                    assert_eq!((rs.warmup_min, rs.cooldown_min), (10, 10), "{ctx}");
                }
                if let Some(rs) = &it.run_structure
                    && rs.strides_reps > 0
                {
                    // Strides 4–8 × 15–30 s, 45 s–2 min between
                    // (04-running.md:218-219).
                    assert!((4..=8).contains(&rs.strides_reps), "{ctx}");
                    assert!((15..=30).contains(&rs.strides_sec), "{ctx}");
                    assert!((45..=120).contains(&rs.strides_recovery_sec), "{ctx}");
                }
            }
        }
    }
}

// ── Running ────────────────────────────────────────────────────────────────

/// A 40 km/wk 10K runner with a race in week 12: weekly ramp, long-run share,
/// ~80/20, quality count and spacing, intensity caps, 3:1 cadence and depth,
/// a two-week taper, and the KB intensity band on every run face.
#[test]
fn ten_k_runner_plan_follows_the_running_tables() {
    let life = ten_k();
    check_race_runner("10K", life, TEN_K_RACE, 3, 2, 12.0);
    check_run_bands("10K", life);
    // The 10K taper keeps 1–2 short sharp I/T sessions (04-running.md:417)
    // and every run day (frequency held, 04-running.md:405).
    let build = run_week(&life.week(week_of(TEN_K_RACE) - 3).rows);
    for i in [week_of(TEN_K_RACE) - 1, week_of(TEN_K_RACE)] {
        let w = run_week(&life.week(i).rows);
        assert!(
            (1..=2).contains(&w.quality_days.len()),
            "10K taper week {i} keeps 1–2 quality sessions (04-running.md:417): {w:?}"
        );
        if i < week_of(TEN_K_RACE) {
            assert_eq!(
                w.run_days, build.run_days,
                "frequency held (04-running.md:405)"
            );
        }
    }
}

/// The taper week's long run keeps its share of the cut week (running-037/038:
/// only volume drops); demonstrated capacity does not raise it to the 2×
/// daily-average guardrail.
#[test]
fn ten_k_taper_week_long_run_stays_within_its_share() {
    let life = ten_k();
    let race_week = week_of(TEN_K_RACE);
    let i = race_week - 1;
    let w = run_week(&life.week(i).rows);
    let long = w.long_km.expect("a taper week keeps its long run");
    // RUN-LONGRUN-001 (extracted/04-running.md:174-175; long-run-weekly-
    // volume-share.md:16-18): ≤30 % of the week under 64 km/wk. The taper cuts
    // volume by shortening sessions while frequency holds (04-running.md:405).
    assert!(
        long <= 0.30 * w.total_km + 0.5,
        "10K taper week {i}: long run {long} km is {:.0} % of a {:.1} km week \
         (≤30 %, 04-running.md:175); week: {w:?}",
        100.0 * long / w.total_km,
        w.total_km
    );
}

/// A 70 km/wk marathoner with a race in week 16: the same running tables at
/// ≥64 km/wk (25 % long-run share), a three-week taper, marathon-pace blocks
/// of 8–26 km and the 150-min long-run cap.
#[test]
fn marathoner_plan_follows_the_running_tables() {
    let life = marathoner();
    check_race_runner("marathon", life, MARATHON_RACE, 3, 3, 22.0);
    check_run_bands("marathon", life);
    for w in &life.weeks {
        for row in &w.rows {
            for it in row.items.iter().filter(|i| i.is_run()) {
                let km = main_km(it);
                match run_kind(row) {
                    // 8–26 km MP blocks (04-running.md:185-186).
                    Some(RunSessionType::RacePace) => assert!(
                        (8.0..=26.0).contains(&km),
                        "MP block {km} km (04-running.md:185-186)"
                    ),
                    // ≤150 min (long-run-weekly-volume-share.md:39-43) at the
                    // plan's accounting pace.
                    Some(RunSessionType::LongRun) => assert!(
                        km * ACCOUNTING_MIN_PER_KM <= 150.0 + 1.0,
                        "long run {km} km over 150 min (long-run-weekly-volume-share.md:39-43)"
                    ),
                    _ => {}
                }
            }
        }
    }
}

/// Easy runs stay inside running-015's 30–90 min and recovery runs inside
/// running-014's 20–40 min at the plan's own 6:30/km accounting pace. Taper
/// weeks are held to the ceilings only: a taper shortens every run
/// (running-037/038, 04-running.md:405-406, :416-417).
#[test]
fn marathon_easy_runs_stay_within_the_easy_run_duration_band() {
    let life = marathoner();
    // running-015: Easy 30–90 min (extracted/04-running.md:163-164);
    // running-014: Recovery 20–40 min (:152-153); one minute of plan rounding.
    let mut outside = Vec::new();
    for w in &life.weeks {
        let taper = phase(w) == Some(MesoPhase::Taper);
        for row in &w.rows {
            let (lo, hi) = match run_kind(row) {
                Some(RunSessionType::Easy) => (30.0, 90.0),
                Some(RunSessionType::Recovery) => (20.0, 40.0),
                _ => continue,
            };
            for it in row.items.iter().filter(|i| i.is_run()) {
                let min = main_km(it) * ACCOUNTING_MIN_PER_KM;
                if min > hi + 1.0 || (!taper && min < lo - 1.0) {
                    outside.push((week_of(row.epoch_day), row.title.clone(), main_km(it)));
                }
            }
        }
    }
    assert!(
        outside.is_empty(),
        "easy runs outside 30–90 min / recovery runs outside 20–40 min at 6:30/km \
         (04-running.md:152-153, :163-164), (week, run, km): {outside:?}"
    );
}

/// Couch-to-5K: the NHS week table bout by bout
/// (extracted/13-running-update.md:101), three run/walk sessions a week with a
/// rest day between, a 5-min warm-up walk, no quality and no long run
/// (04-running.md:263, 273-274; long-run-weekly-volume-share.md:147).
#[test]
fn couch_to_5k_follows_the_nhs_table() {
    let life = beginner_5k();
    for (i, table) in crate::running::tests::NHS_C25K.iter().enumerate() {
        let days: Vec<&DayRec> = life.days_of_week(i).collect();
        assert_eq!(
            days.len(),
            3,
            "C25K week {}: 3 sessions (04-running.md:273-274)",
            i + 1
        );
        for pair in days.windows(2) {
            assert!(
                pair[1].day - pair[0].day >= 2,
                "a rest day between runs (04-running.md:274)"
            );
        }
        for (d, want) in days.iter().zip(table) {
            let it = &d.row.items[0];
            let rs = it.run_structure.as_ref().expect("C25K structure");
            let got: Vec<(u32, u32)> = if rs.run_walk.is_empty() {
                vec![(
                    main_km(it)
                        .mul_add(ACCOUNTING_MIN_PER_KM * 60.0, 0.0)
                        .round() as u32,
                    0,
                )]
            } else {
                rs.run_walk
                    .iter()
                    .map(|s| (s.run_sec, s.walk_sec))
                    .collect()
            };
            assert_eq!(
                got,
                *want,
                "C25K week {} day {}: {} (13-running-update.md:101)",
                i + 1,
                d.day - DAY0,
                it.summary
            );
            assert_eq!(
                rs.warmup_min, 5,
                "5-min warm-up walk (13-running-update.md:101)"
            );
            assert!(
                cites(&it.evidence.citation, "RUN-C25K-002"),
                "{}",
                it.evidence.citation
            );
        }
        let w = run_week(&life.week(i).rows);
        assert!(
            w.quality_days.is_empty() && w.long_km.is_none(),
            "C25K week {}: {w:?}",
            i + 1
        );
    }
}

// ── Lifting ────────────────────────────────────────────────────────────────

/// The %e1RM a lift item is prescribed at, if any.
fn pct(it: &PrescriptionView) -> Option<u32> {
    match it.intensity {
        PlanIntensity::PercentE1rm { pct, .. } => Some(pct),
        _ => None,
    }
}

/// The reps in reserve a lift item targets, if stated.
fn rir(it: &PrescriptionView) -> Option<i32> {
    match it.intensity {
        PlanIntensity::Rir { rir } => Some(rir),
        _ => it.target_rpe.map(|r| (10.0 - r).round() as i32),
    }
}

fn lift_items(row: &SessionPlanView) -> impl Iterator<Item = &PrescriptionView> {
    row.items
        .iter()
        .filter(|i| !i.is_run() && i.calis.is_none())
}

/// A logged-and-followed warm-up ramp never counts as work: every work set
/// is credited and each warm-up shows as a non-qualifying set
/// (extracted/13-logging-warmup.md:12, :19).
fn check_warm_ups(name: &str, life: &Life) {
    let mut seen = 0;
    for d in &life.days {
        for it in lift_items(&d.row) {
            let Some(&n) = d.warm_ups.get(&it.exercise) else {
                continue;
            };
            seen += n;
            let ctx = format!("{name} day {} {}", d.day - DAY0, it.summary);
            assert_eq!(it.sets_done, it.sets, "{ctx}: every work set credited");
            assert_eq!(it.status, ItemStatus::Done, "{ctx}");
            let warm = it.logged_sets.iter().filter(|s| !s.qualifies).count();
            assert_eq!(
                warm, n,
                "{ctx}: {n} warm-ups are not work sets (13-logging-warmup.md:12)"
            );
        }
    }
    assert!(seen > 0, "{name}: no warm-up ramp was exercised");
}

/// A novice 3-day strength lifter: full body three times a week
/// (08-individualization-safety.md:42-43, :619), 3×5 inside the max-strength
/// row (02-strength-power.md:10: 80–100 % 1RM, 1–5 reps, 3–6+ sets, rest
/// 180–300 s, RIR 1–3), no calendar deload for linear progression
/// (02-strength-power.md:109, 08-individualization-safety.md:86-87), and the
/// STR-WARMUP-001 ramp never counted as work.
#[test]
fn novice_lifter_plan_follows_the_strength_tables() {
    let life = novice_lifter();
    for i in 0..life.weeks.len() {
        let days: Vec<&DayRec> = life.days_of_week(i).filter(|d| is_lift(&d.row)).collect();
        assert_eq!(
            days.len(),
            3,
            "novice week {i}: 3 lift days (08-individualization-safety.md:42-43)"
        );
        for d in days {
            assert_eq!(
                d.row.session_type,
                SessionType::Lift(LiftSessionType::FullBody)
            );
            assert!(
                !d.row.title.contains("lighter"),
                "no calendar deload for a novice"
            );
            for it in lift_items(&d.row) {
                let ctx = format!("novice day {}: {}", d.day - DAY0, it.summary);
                assert_eq!((it.sets, it.reps_low, it.reps_high), (3, 5, 5), "{ctx}");
                assert!(
                    (180..=300).contains(&it.rest_sec),
                    "{ctx}: rest (02-strength-power.md:10)"
                );
                let r = rir(it).expect("an effort target");
                assert!(
                    (1..=3).contains(&r),
                    "{ctx}: RIR {r} (02-strength-power.md:10)"
                );
                if let Some(p) = pct(it) {
                    assert!(
                        (80..=100).contains(&p),
                        "{ctx}: {p} % (02-strength-power.md:10)"
                    );
                }
            }
        }
    }
    check_warm_ups("novice", life);
}

/// A novice who completes every session is never handed a lighter load: the
/// %1RM ramp does not restart at the band floor, and a completed session's
/// load is held when the plate-grid floor of anchor × % would slide under it.
#[test]
fn novice_loads_never_drop_while_every_session_is_completed() {
    let life = novice_lifter();
    // STR-PCTPROG-001: add 2.5–5 % lower / 1–2.5 % upper after each
    // successful week (extracted/02-strength-power.md:153); novice linear
    // progression adds load each completed session and runs 3–6 months
    // (08-individualization-safety.md:86-87, :97-98).
    let mut last: BTreeMap<String, (i64, f64)> = BTreeMap::new();
    let mut drops = Vec::new();
    for d in &life.days {
        for it in lift_items(&d.row) {
            let Some(kg) = it.load_kg else { continue };
            assert_eq!(
                it.status,
                ItemStatus::Done,
                "the athlete completes every session"
            );
            if let Some(&(prev_day, prev)) = last.get(&it.exercise)
                && kg < prev
            {
                drops.push(format!(
                    "{}: {prev} kg on day {} → {kg} kg on day {}",
                    it.exercise,
                    prev_day - DAY0,
                    d.day - DAY0
                ));
            }
            last.insert(it.exercise.clone(), (d.day, kg));
        }
    }
    assert!(
        drops.is_empty(),
        "loads dropped after completed sessions: {drops:#?}"
    );
}

/// An athlete who lifts every set at its load, reps and target effort never
/// gets a performance-drop cut. AUTOREG-E1RM-GATE-001 cuts the next load ~5 %
/// when "today's e1RM < baseline − 5%" (extracted/06-autoregulation.md:133-134)
/// and autoreg-022 deloads on e1RM "at fixed RPE" down >10 % (:278-280); a set
/// done exactly as the card asked is no drop.
#[test]
fn compliant_athletes_never_get_a_performance_drop_cut() {
    let mut cuts = Vec::new();
    for (name, life) in [
        ("beginner-5k", beginner_5k()),
        ("10k", ten_k()),
        ("marathon", marathoner()),
        ("novice", novice_lifter()),
        ("hypertrophy", hypertrophy_lifter()),
        ("powerlifter", powerlifter()),
        ("hybrid", hybrid_athlete()),
        ("calisthenics", calisthenics_beginner()),
        ("masters", masters_67()),
        ("female", female_lifter()),
        ("male", male_twin()),
    ] {
        for d in &life.days {
            cuts.extend(
                d.perf_cuts
                    .iter()
                    .map(|c| format!("{name} day {}: {c}", d.day - DAY0)),
            );
        }
    }
    assert!(cuts.is_empty(), "compliant sessions cut: {cuts:#?}");
}

/// Table 1 MEV per muscle (extracted/03-hypertrophy.md:520-530).
const TABLE1_MEV: &[(&str, f32)] = &[
    ("Chest", 10.0),
    ("Back", 10.0),
    ("Quads", 8.0),
    ("Hamstrings", 6.0),
    ("Side delts", 8.0),
    ("Rear delts", 6.0),
    ("Biceps", 6.0),
    ("Triceps", 6.0),
    ("Calves", 6.0),
];

/// Table 1 MAV high per muscle, the default ramp's end
/// (extracted/03-hypertrophy.md:504-505, :520-530).
const TABLE1_MAV_HIGH: &[(&str, u8)] = &[
    ("Chest", 20),
    ("Back", 22),
    ("Quads", 18),
    ("Hamstrings", 16),
    ("Glutes", 12),
    ("Side delts", 22),
    ("Rear delts", 18),
    ("Biceps", 20),
    ("Triceps", 14),
    ("Calves", 16),
];

fn is_compound(it: &PrescriptionView) -> bool {
    crate::plan::is_barbell_lift(&it.exercise)
}

/// A 4-day intermediate hypertrophy lifter: rep ranges by exercise type
/// (03-hypertrophy.md:142, :164), rest (03-hypertrophy.md:416-417;
/// 13-hypertrophy-update.md:11-12), RIR 1–3 ramping toward 1 through the
/// block (03-hypertrophy.md:197, :471-472), a deload every sixth week
/// (13-hypertrophy-update.md:75) at about half the sets, RIR 2–4+ and
/// 60–70 % of the working load (03-hypertrophy.md:395), the 32-set session
/// cap (03-hypertrophy.md:505), weekly targets within Table 1 MAV high
/// (03-hypertrophy.md:504-505) and every muscle with >12 weekly sets trained
/// on ≥2 days (03-hypertrophy.md:274).
#[test]
fn hypertrophy_plan_follows_the_hypertrophy_tables() {
    let life = hypertrophy_lifter();
    let mut prev_rir: Option<i32> = None;
    for (i, w) in life.weeks.iter().enumerate() {
        let deload = phase(w) == Some(MesoPhase::Deload);
        assert_eq!(
            deload,
            i % 6 == 5,
            "week {i}: deload every 6th week (13-hypertrophy-update.md:75)"
        );
        for row in w.rows.iter().filter(|r| is_lift(r)) {
            let sets: u32 = lift_items(row).map(|it| u32::from(it.sets)).sum();
            assert!(
                sets <= 32,
                "week {i} {}: {sets} sets > 32 (03-hypertrophy.md:505)",
                row.title
            );
            for it in lift_items(row) {
                let ctx = format!("week {i} {}: {}", row.title, it.summary);
                let (lo, hi) = if is_compound(it) { (5, 10) } else { (12, 25) };
                assert!(
                    it.reps_low >= lo && it.reps_high <= hi,
                    "{ctx}: reps outside {lo}–{hi} (03-hypertrophy.md:142, :164)"
                );
                let rest_floor = if is_compound(it) { 120 } else { 90 };
                assert!(
                    (rest_floor..=180).contains(&it.rest_sec),
                    "{ctx}: rest (03-hypertrophy.md:416-417; 13-hypertrophy-update.md:11-12)"
                );
                let r = rir(it).expect("an effort target");
                if deload {
                    assert!(r >= 2, "{ctx}: deload RIR 2–4+ (03-hypertrophy.md:395)");
                } else {
                    assert!(
                        (1..=4).contains(&r),
                        "{ctx}: RIR (03-hypertrophy.md:197, :208)"
                    );
                }
            }
        }
        let week_rir = w.rows.iter().flat_map(lift_items).filter_map(rir).max();
        if !deload {
            if let (Some(p), Some(r)) = (prev_rir, week_rir)
                && i % 6 != 0
            {
                assert!(
                    r <= p,
                    "week {i}: RIR rises {p} → {r} inside the block (03-hypertrophy.md:471-472)"
                );
            }
            prev_rir = week_rir;
        }
        if let Some(p) = &w.program {
            for mv in &p.muscle_volume {
                if let Some((_, cap)) = TABLE1_MAV_HIGH.iter().find(|(m, _)| *m == mv.muscle) {
                    assert!(
                        mv.target_sets <= *cap,
                        "week {i} {}: target {} > MAV high {cap} (03-hypertrophy.md:504-505)",
                        mv.muscle,
                        mv.target_sets
                    );
                }
            }
        }
    }
    // Deload depth against the last load week (03-hypertrophy.md:395):
    // sets ≈ half, load ~60–70 % of working. Each lift's first deload session
    // (a later one re-derives from it:
    // `hypertrophy_loads_return_to_working_weight_after_the_deload`).
    let load_week = life.days_of_week(4).collect::<Vec<_>>();
    let mut seen = std::collections::BTreeSet::new();
    for d in life.days_of_week(5) {
        for it in lift_items(&d.row).filter(|it| seen.insert(it.exercise.clone())) {
            let Some(before) = load_week
                .iter()
                .flat_map(|d| lift_items(&d.row))
                .filter(|b| b.exercise == it.exercise)
                .last()
            else {
                continue;
            };
            let ctx = format!("deload {} vs {}", it.summary, before.summary);
            assert!(
                f64::from(it.sets) <= 0.6 * f64::from(before.sets) + 0.5,
                "{ctx}: sets ≈ MV (~50 %) (03-hypertrophy.md:395)"
            );
            if let (Some(kg), Some(work)) = (it.load_kg, before.load_kg) {
                // Within one 2.5 kg step of the 60–70 % band.
                assert!(
                    kg >= 0.6 * work - 2.5 && kg <= 0.7 * work + 2.5,
                    "{ctx}: load {:.0} % of working (03-hypertrophy.md:395)",
                    100.0 * kg / work
                );
            }
        }
    }
    // Frequency: a muscle whose weekly target is over 12 sets gets its sets on
    // ≥2 days (hypertrophy-025, 03-hypertrophy.md:273-274), counting every
    // exercise that credits it (HYP-VOL-DOSE-001's fractional counting).
    let (mut one_day, mut split) = (Vec::new(), 0);
    for (i, w) in life.weeks.iter().enumerate().filter(|(i, _)| i % 6 != 5) {
        let p = w.program.as_ref().expect("program card");
        for mv in p.muscle_volume.iter().filter(|mv| mv.target_sets > 12) {
            let days = w
                .rows
                .iter()
                .filter(|r| {
                    lift_items(r).any(|it| {
                        crate::hypertrophy::muscle_credits(&it.exercise)
                            .iter()
                            .any(|(m, _)| m.eq_ignore_ascii_case(&mv.muscle))
                    })
                })
                .count();
            match days {
                1 => one_day.push(format!("week {i}: {}", mv.label)),
                2.. => split += 1,
                0 => {}
            }
        }
    }
    assert!(
        one_day.is_empty(),
        "over 12 weekly sets on one day (03-hypertrophy.md:274): {one_day:#?}"
    );
    assert!(split > 0, "no muscle over 12 weekly sets was checked");
}

/// The deload week's light sets never become the double-progression anchor:
/// each deload session sits at ~60–70 % of the working load and the next
/// block resumes from the working load.
#[test]
fn hypertrophy_loads_return_to_working_weight_after_the_deload() {
    let life = hypertrophy_lifter();
    // The deload is one week at ~60–70 % of the working weight
    // (extracted/03-hypertrophy.md:395); double progression carries the
    // working load forward and adds the smallest step at the top of the range
    // (03-hypertrophy.md:361-362). The next block starts from the working
    // load, not the deload load.
    let last_load = |week: usize, ex: &str| {
        life.days_of_week(week)
            .flat_map(|d| lift_items(&d.row))
            .filter(|it| it.exercise == ex)
            .filter_map(|it| it.load_kg)
            .last()
    };
    let first_load = |week: usize, ex: &str| {
        life.days_of_week(week)
            .flat_map(|d| lift_items(&d.row))
            .filter(|it| it.exercise == ex)
            .find_map(|it| it.load_kg)
    };
    let mut lost = Vec::new();
    for ex in [
        "Bench Press",
        "Back Squat",
        "Barbell Row",
        "Deadlift",
        "Overhead Press",
        "Lateral raise",
    ] {
        let (Some(before), Some(after)) = (last_load(4, ex), first_load(6, ex)) else {
            panic!("{ex} trained in weeks 5 and 7");
        };
        if after < 0.9 * before {
            lost.push(format!("{ex}: {before} kg (week 5) → {after} kg (week 7)"));
        }
        // Every deload session sits at ~60–70 % of the working load, not a
        // cut of the deload's own earlier session.
        for it in life
            .days_of_week(5)
            .flat_map(|d| lift_items(&d.row))
            .filter(|it| it.exercise == ex)
        {
            if let Some(kg) = it.load_kg
                && kg < 0.6 * before - 2.5
            {
                lost.push(format!(
                    "{ex}: deload {} = {:.0} % of {before} kg",
                    it.summary,
                    100.0 * kg / before
                ));
            }
        }
    }
    assert!(
        lost.is_empty(),
        "working loads lost to the deload: {lost:#?}"
    );
}

/// Every muscle the week has an exercise for gets at least its Table 1 MEV,
/// including on the 32-set upper days.
#[test]
fn hypertrophy_week_delivers_at_least_mev_for_every_muscle_it_trains() {
    let life = hypertrophy_lifter();
    let mut short = Vec::new();
    for (i, w) in life.weeks.iter().enumerate().filter(|(i, _)| i % 6 != 5) {
        let p = w.program.as_ref().expect("program card");
        for mv in &p.muscle_volume {
            let Some((_, mev)) = TABLE1_MEV.iter().find(|(m, _)| *m == mv.muscle) else {
                continue;
            };
            // A muscle the plan has no exercise for is named in the note.
            if p.muscle_volume_note
                .to_lowercase()
                .contains(&mv.muscle.to_lowercase())
            {
                continue;
            }
            // Table 1 MEV (extracted/03-hypertrophy.md:520-530; MEV is the
            // mesocycle start point, :534).
            if mv.target_sets > 0 && mv.delivered_sets < *mev {
                short.push(format!("week {i}: {}", mv.label));
            }
        }
    }
    assert!(short.is_empty(), "muscles below Table 1 MEV: {short:#?}");
}

/// An advanced lifter peaking for a meet: max-strength rows
/// (02-strength-power.md:10; DUP days 02-strength-power.md:252), rest by
/// rep scheme (02-strength-power.md:10, :32), the lifting deload every sixth
/// week at ~half the sets and ≤10 points lighter (13-strength-power-update.md:52),
/// the meet taper (02-strength-power.md:285, :318, :493-495) and the
/// warm-up ramp never counted as work.
#[test]
fn powerlifter_plan_follows_the_strength_tables_and_peaks_for_the_meet() {
    let life = powerlifter();
    let meet_week = week_of(MEET);
    for d in life.days.iter().filter(|d| is_lift(&d.row)) {
        let i = week_of(d.day);
        let deload = d.row.title.contains("lighter week");
        assert_eq!(
            deload,
            i == 5,
            "day {}: lifting deload in week 6 only (13-hypertrophy-update.md:75)",
            d.day - DAY0
        );
        for it in lift_items(&d.row) {
            let ctx = format!("day {} {}: {}", d.day - DAY0, d.row.title, it.summary);
            let r = rir(it).expect("effort target");
            // ≥1 in reserve on max-strength work (02-strength-power.md:10,
            // "rarely to failure"); 4 on a week-1 volume day (03-hypertrophy.md:208).
            assert!((1..=4).contains(&r), "{ctx}: RIR {r}");
            let (rlo, rhi) = if it.reps_high <= 5 {
                (180, 300)
            } else {
                (60, 180)
            };
            assert!(
                (rlo..=rhi).contains(&it.rest_sec),
                "{ctx}: rest (02-strength-power.md:10, :32; 03-hypertrophy.md:416-417)"
            );
            if let Some(p) = pct(it) {
                if it.reps_high <= 5 && !deload {
                    assert!(
                        (80..=95).contains(&p),
                        "{ctx}: heavy {p} % (02-strength-power.md:10, :252)"
                    );
                } else if !deload {
                    assert!(
                        (65..=80).contains(&p),
                        "{ctx}: volume {p} % (02-strength-power.md:241, :252)"
                    );
                }
            }
        }
    }
    // Deload depth as planned on its Monday: sets ~−50 %, %1RM down ≤10
    // points (13-strength-power-update.md:52); the days as done match it
    // (`a_planned_deload_does_not_trigger_an_unplanned_one`).
    for row in life.week(5).rows.iter().filter(|r| is_lift(r)) {
        for it in lift_items(row) {
            let before = life
                .week(4)
                .rows
                .iter()
                .filter(|b| mon0_weekday(b.epoch_day) == mon0_weekday(row.epoch_day))
                .flat_map(lift_items)
                .find(|b| b.exercise == it.exercise)
                .expect("same lift the week before");
            let ctx = format!("deload {} vs {}", it.summary, before.summary);
            let f = f64::from(it.sets) / f64::from(before.sets);
            assert!(
                (0.4..=0.65).contains(&f),
                "{ctx}: sets ~−50 % (13-strength-power-update.md:52)"
            );
            if let (Some(p), Some(q)) = (pct(it), pct(before)) {
                assert!(
                    p + 10 >= q,
                    "{ctx}: %1RM cut ≤10 (13-strength-power-update.md:52)"
                );
            }
        }
    }
    // Meet taper: last heavy deadlift ≥10 days out, last heavy squat/bench
    // ~4–5+ days out (02-strength-power.md:318, safety-critical); volume cut
    // over the last two weeks with intensity held ≥85 % on the heavy day
    // (02-strength-power.md:285, :493-495).
    let heavy = |it: &PrescriptionView| pct(it).is_some_and(|p| p >= 80);
    for d in life.days.iter().filter(|d| d.day < MEET) {
        for it in lift_items(&d.row).filter(|it| heavy(it)) {
            let out = MEET - d.day;
            if it.exercise.to_lowercase().contains("deadlift") {
                assert!(
                    out >= 10,
                    "heavy deadlift {out} days before the meet (02-strength-power.md:318)"
                );
            }
            assert!(
                out >= 4,
                "heavy {} {out} days out (02-strength-power.md:318)",
                it.exercise
            );
        }
    }
    let week_sets = |i: usize| -> u32 {
        life.days_of_week(i)
            .flat_map(|d| lift_items(&d.row))
            .map(|it| u32::from(it.sets))
            .sum()
    };
    let peak = week_sets(meet_week - 3);
    let taper = week_sets(meet_week - 1);
    let cut = 1.0 - f64::from(taper) / f64::from(peak);
    assert!(
        (0.30..=0.70).contains(&cut),
        "meet taper cuts {:.0} % of the sets ({peak} → {taper}), 30–70 % (02-strength-power.md:296)",
        100.0 * cut
    );
    let held = life
        .days_of_week(meet_week - 1)
        .flat_map(|d| lift_items(&d.row))
        .filter_map(pct)
        .max()
        .unwrap_or(0);
    assert!(
        held >= 85,
        "taper keeps ≥85 % work (02-strength-power.md:296): {held}"
    );
    check_warm_ups("powerlifter", life);
}

/// Every main lift is lifted at ≥80 % 1RM at least twice a week on the 4-day
/// upper/lower max-strength week: the DUP rotation runs per region.
#[test]
fn powerlifter_lifts_every_main_lift_heavy_twice_a_week() {
    let life = powerlifter();
    // STR-HEAVYFREQ-001: ≥80 % 1RM on each main lift in ≥2 sessions a week;
    // lighter days supplement, not replace, the heavy exposures
    // (extracted/13-strength-power-update.md:12-13).
    let mut short = Vec::new();
    for i in [0, 1, 2, 3, 4, 6, 7, 8, 9] {
        let mut heavy: BTreeMap<&str, usize> = BTreeMap::new();
        for d in life.days_of_week(i) {
            for it in lift_items(&d.row) {
                let n = heavy.entry(it.exercise.as_str()).or_default();
                if pct(it).is_some_and(|p| p >= 80) {
                    *n += 1;
                }
            }
        }
        for (ex, n) in heavy {
            if n < 2 {
                short.push(format!("week {i}: {ex} heavy ×{n}"));
            }
        }
    }
    assert!(
        short.is_empty(),
        "main lifts under two heavy exposures: {short:#?}"
    );
}

/// A planned lifting deload is not read as a performance decline: its sets,
/// logged at their prescribed load and ~4 reps left, are left out of the e1RM
/// gate (autoreg-022) and the anchors, so no readiness deload lands on the
/// deload's last day or on the Mondays after it.
#[test]
fn a_planned_deload_does_not_trigger_an_unplanned_one() {
    // autoreg-022 deloads when e1RM *at fixed RPE* falls >10 % for ≥2
    // sessions (extracted/06-autoregulation.md:278-280); the planned deload is
    // ~7 days every 4–6 weeks, earlier only on ≥2 triggers
    // (13-hypertrophy-update.md:75; 03-hypertrophy.md:383-384). An athlete
    // who did every set as prescribed gets no second deload.
    let mut extra = Vec::new();
    for (name, life) in [
        ("powerlifter", powerlifter()),
        ("hybrid", hybrid_athlete()),
        ("masters", masters_67()),
        ("female", female_lifter()),
    ] {
        for w in &life.weeks {
            for row in w.rows.iter().filter(|r| r.adjustment.is_some()) {
                let a = row.adjustment.as_ref().unwrap();
                extra.push(format!(
                    "{name} week {} {} ({}): {}",
                    week_of(row.epoch_day) + 1,
                    row.title,
                    row.items.first().map_or("", |i| i.summary.as_str()),
                    a.summary
                ));
            }
        }
        // Nor is a deload's light work an e1RM drop that scales the next
        // loads (AUTOREG-E1RM-GATE-001 cuts ~5 % after a session >5 % under
        // the estimate): the anchors leave the deload's sets out. A lift's
        // first deload session reads its last load week; its later deload
        // sessions and its first session after the deload would read the
        // deload's own sets. A lift already scaled in week 5 or at its first
        // deload session is not the deload's doing.
        let derated = |d: &DayRec| -> Vec<String> {
            lift_items(&d.row)
                .filter(|i| i.why.basis.contains("your last session came in more than"))
                .map(|i| i.exercise.clone())
                .collect()
        };
        let mut sessions: BTreeMap<String, Vec<(i64, bool)>> = BTreeMap::new();
        for d in life.days.iter().filter(|d| week_of(d.day) >= 5) {
            let cut = derated(d);
            for it in lift_items(&d.row) {
                sessions
                    .entry(it.exercise.clone())
                    .or_default()
                    .push((d.day, cut.contains(&it.exercise)));
            }
        }
        let week5: Vec<String> = life.days_of_week(4).flat_map(derated).collect();
        for (ex, s) in &sessions {
            let Some(&(_, first_cut)) = s.first() else {
                continue;
            };
            if first_cut || week5.contains(ex) {
                continue;
            }
            let deload_rest = s.iter().skip(1).take_while(|(d, _)| week_of(*d) == 5);
            let first_after = s.iter().find(|(d, _)| week_of(*d) >= 6);
            for &(day, cut) in deload_rest.chain(first_after) {
                if cut {
                    extra.push(format!(
                        "{name} day {}: {ex} scaled for an e1RM drop",
                        day - DAY0
                    ));
                }
            }
        }
    }
    // Nor is the planned deload cut again once its first light sessions
    // are logged: each deload day as shown matches the deload planned on its
    // Monday (hypertrophy-036: one week at ~half the sets,
    // 03-hypertrophy.md:393-395; 13-strength-power-update.md:52).
    let life = powerlifter();
    for d in life.days_of_week(5) {
        let planned = life
            .week(5)
            .rows
            .iter()
            .find(|r| r.epoch_day == d.day)
            .expect("planned row");
        for (done, plan) in lift_items(&d.row).zip(lift_items(planned)) {
            if (done.sets, done.load_kg) != (plan.sets, plan.load_kg) {
                extra.push(format!(
                    "powerlifter deload day {}: planned {} → shown {}",
                    d.day - DAY0,
                    plan.summary,
                    done.summary
                ));
            }
        }
    }
    assert!(
        extra.is_empty(),
        "adjustments for a fully compliant athlete: {extra:#?}"
    );
}

/// A lifting-led hybrid (3 lift days, 3 run days, 25 km/wk): run days ≤3
/// with strength co-primary (10-hybrid-concurrent.md:142), every week keeps
/// its lift and run days, never a lift and a run on one day
/// (10-hybrid-concurrent.md:131, CAP-3), the long run within its share or the
/// 2× daily guardrail (04-running.md:175), and the warm-up ramp never counted
/// as work.
#[test]
fn hybrid_plan_keeps_lift_and_run_days_apart() {
    let life = hybrid_athlete();
    for (i, w) in life.weeks.iter().enumerate() {
        let rw = run_week(&w.rows);
        let lifts = w.rows.iter().filter(|r| is_lift(r)).count();
        assert_eq!(
            (lifts, rw.run_days),
            (3, 3),
            "week {i}: 3 lift + 3 run days"
        );
        for row in &w.rows {
            assert!(
                !(is_lift(row) && row.items.iter().any(PrescriptionView::is_run)),
                "week {i}: lift and run on one day"
            );
        }
        if let Some(long) = rw.long_km
            && !long_run_on_capacity(&rw)
        {
            assert!(
                long <= long_run_ceiling(&rw),
                "week {i}: long run {long} km: {rw:?}"
            );
        }
    }
    check_warm_ups("hybrid", life);
}

/// A calisthenics beginner: every rung advances only after two consecutive
/// sessions with all sets at the top of the band and then restarts at the
/// band floor (calisthenics/calis-progression.md:58, :68), working sets
/// leave 1–3 in reserve (calis-progression.md:97), 3–5 hard sets with
/// 3–5 min rest (calis-programming.md:55), holds inside the ladder's band
/// (calis-progression.md:170, :217-219), and every pattern ≥2×/wk
/// (calis-programming.md:46, :54).
#[test]
fn calisthenics_rungs_advance_on_two_top_of_band_sessions() {
    let life = calisthenics_beginner();
    // Per ladder: the rung and whether all sets reached the top, per session.
    let mut hist: BTreeMap<String, Vec<(i64, u8, bool)>> = BTreeMap::new();
    for d in &life.days {
        for it in d.row.items.iter() {
            let Some(c) = &it.calis else { continue };
            let ctx = format!("day {}: {}", d.day - DAY0, it.summary);
            assert!(
                (3..=5).contains(&it.sets),
                "{ctx}: 3–5 sets (calis-programming.md:55)"
            );
            assert!(
                (180..=300).contains(&it.rest_sec),
                "{ctx}: 3–5 min rest (calis-programming.md:55)"
            );
            if let PlanIntensity::RirRange { lo, hi } = it.intensity {
                assert!(
                    lo >= 1 && hi <= 3,
                    "{ctx}: RIR 1–3 (calis-progression.md:97)"
                );
            }
            if it.timed_hold {
                assert!(
                    it.hold_low_sec >= 5 && it.hold_high_sec <= 60,
                    "{ctx}: hold band"
                );
            }
            let top = it.sets_done >= it.sets && it.status == ItemStatus::Done;
            hist.entry(format!("{:?}", c.ladder))
                .or_default()
                .push((d.day, c.rung, top));
        }
    }
    for (ladder, h) in &hist {
        for (k, w) in h.windows(2).enumerate() {
            let (day, rung, _) = w[1];
            if rung > w[0].1 {
                assert_eq!(rung, w[0].1 + 1, "{ladder}: one rung at a time");
                // Two consecutive top-of-band sessions on the previous rung.
                let on_prev: Vec<bool> = h[..=k]
                    .iter()
                    .rev()
                    .take_while(|s| s.1 == w[0].1)
                    .map(|s| s.2)
                    .collect();
                assert!(
                    on_prev.len() >= 2 && on_prev[..2].iter().all(|t| *t),
                    "{ladder} day {}: advanced after {on_prev:?} (calis-progression.md:58)",
                    day - DAY0
                );
            }
        }
        // A beginner who tops every session climbs (2-for-2 is reachable).
        let (first, last) = (h.first().unwrap().1, h.last().unwrap().1);
        assert!(last > first, "{ladder} never advanced: {h:?}");
    }
    for i in 0..life.weeks.len() {
        let mut per: BTreeMap<String, usize> = BTreeMap::new();
        for d in life.days_of_week(i) {
            for c in d.row.items.iter().filter_map(|it| it.calis.as_ref()) {
                // Dip and vertical push are one vertical-push family that
                // alternates by upper day (plan/calis.rs CAL-FREQ-VOL-002).
                let l = format!("{:?}", c.ladder).replace("VerticalPush", "Dip");
                *per.entry(l).or_default() += 1;
            }
        }
        for (ladder, n) in per {
            assert!(n >= 2, "week {i}: {ladder} ×{n} (calis-programming.md:46)");
        }
    }
}

/// A 67-year-old hybrid athlete: the 2:1 running cadence for older athletes
/// (04-running.md:361-362) and no age cap on lifting intensity —
/// "moderate–heavy loads appropriate" (08-individualization-safety.md:603;
/// the KB states no other masters load rule, :597-604).
#[test]
fn masters_runner_gets_the_two_to_one_running_cadence() {
    let life = masters_67();
    for (i, w) in life.weeks.iter().enumerate() {
        let rw = run_week(&w.rows);
        let recovery = w
            .rows
            .iter()
            .filter(|r| run_kind(r).is_some())
            .flat_map(|r| &r.items)
            .any(|it| cites(&it.evidence.citation, "RUN-DELOAD-DEPTH-001"));
        assert_eq!(
            recovery,
            i % 3 == 2,
            "week {i}: 2:1 cadence (04-running.md:361-362): {rw:?}"
        );
    }
    let heavy = life
        .days
        .iter()
        .flat_map(|d| lift_items(&d.row))
        .filter_map(pct)
        .max()
        .unwrap_or(0);
    assert!(
        heavy >= 80,
        "masters keep heavy loads (08-individualization-safety.md:603): max {heavy} %"
    );
}

/// Sex is no programming variable: "Do NOT hard-code divergent
/// volume/intensity by sex" (08-individualization-safety.md:231-232) and no
/// menstrual-cycle periodization (:198-199). A female lifter and her male
/// twin logging the same sets see the same plan.
#[test]
fn female_and_male_twins_get_the_same_plan() {
    let (f, m) = (female_lifter(), male_twin());
    assert!(!m.weeks.is_empty());
    for (a, b) in f.weeks.iter().zip(&m.weeks) {
        assert_eq!(a.rows, b.rows, "week {}", week_of(a.monday));
        assert_eq!(a.program, b.program, "week {}", week_of(a.monday));
    }
}

// ── Cited claims ───────────────────────────────────────────────────────────

/// Every row of every life cites a registered, non-myth claim; a row citing
/// STR-DELOAD-VOL-001 is a lifting deload day, and an easy run citing
/// RUN-EASY-001 lasts at least its 30 min at the accounting pace outside the
/// taper.
#[test]
fn every_cited_claim_supports_the_number_on_its_row() {
    let lives: [(&str, &Life); 9] = [
        ("10K", ten_k()),
        ("marathon", marathoner()),
        ("C25K", beginner_5k()),
        ("novice", novice_lifter()),
        ("hypertrophy", hypertrophy_lifter()),
        ("powerlifter", powerlifter()),
        ("hybrid", hybrid_athlete()),
        ("calisthenics", calisthenics_beginner()),
        ("masters", masters_67()),
    ];
    let mut wrong = Vec::new();
    for (name, life) in lives {
        for w in &life.weeks {
            let taper = phase(w) == Some(MesoPhase::Taper);
            for row in &w.rows {
                // An adjusted row carries a readiness change, not the plan's
                // own claim.
                for it in row.items.iter().filter(|_| row.adjustment.is_none()) {
                    let c = &it.evidence.citation;
                    let candidates = claims_citing(c);
                    assert!(
                        !candidates.is_empty(),
                        "{name}: {} cites an unregistered source {c:?}",
                        it.summary
                    );
                    assert!(
                        candidates.iter().any(|e| !e.is_blocked()),
                        "{name}: only a myth cites {c}"
                    );
                    if it.is_run()
                        && run_kind(row) == Some(RunSessionType::Easy)
                        && cites(c, "RUN-EASY-001")
                        && !taper
                    {
                        let min = main_km(it) * ACCOUNTING_MIN_PER_KM;
                        if min < 30.0 - 1.0 {
                            wrong.push(format!(
                                "{name} week {}: {} ({min:.0} min) cites RUN-EASY-001 30–90 min",
                                week_of(w.monday),
                                it.summary
                            ));
                        }
                    }
                    if cites(c, "STR-DELOAD-VOL-001") {
                        assert!(
                            row.title.contains("lighter"),
                            "{name}: {} cites the deload rule",
                            it.summary
                        );
                    }
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}
