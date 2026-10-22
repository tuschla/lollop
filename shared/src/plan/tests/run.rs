//! Run week tests: phases and taper, volume carry, quality mix, long-run and
//! spike-ceiling sizing, easy vs recovery days, session structure, C25K.

use super::*;
use crate::app::Profile;
use crate::plan::run::{
    DayCtx, LongRunBind, RunWeekPlan, RunWeekRequest, cap_km_to_spike, continuous_prescription,
    daily_avg_run_km, effective_weekly_km, is_quality, long_run_decision, mid_f64, mid_u16,
    run_week_plan, run_week_sessions,
};
use crate::running::{self, RunWorkoutRx};
use crate::schema::{
    MesoPhase, Program, RepRecovery, RunIntensity, RunPrescription, RunVolume, Session,
};

// ── Helpers ──────────────────────────────────────────────────────────────

/// A run-only week request for `days` run days at `week_index`.
fn req(week_index: u32, days: usize) -> RunWeekRequest {
    RunWeekRequest {
        week_index,
        n_run_days: days,
        volume_scale: 1.0,
        hold_volume: false,
        max_quality: None,
    }
}

/// A simple placement for direct run-week tests: the long run on the last
/// available day, quality on days 1/3/5, easy fill on the rest in order.
fn place(plan: &RunWeekPlan) -> Vec<(u16, RunSessionType)> {
    let end = plan.race_day.unwrap_or(7);
    let mut free: Vec<u16> = (0..end).collect();
    let mut out = Vec::new();
    if plan.kinds.contains(&RunSessionType::LongRun)
        && let Some(d) = free.pop()
    {
        out.push((d, RunSessionType::LongRun));
    }
    for &k in plan.kinds.iter().filter(|k| is_quality(**k)) {
        let d = [1u16, 3, 5, 0, 2, 4]
            .into_iter()
            .find(|d| free.contains(d))
            .expect("a free day for quality");
        free.retain(|x| *x != d);
        out.push((d, k));
    }
    for &k in plan
        .kinds
        .iter()
        .filter(|k| !is_quality(**k) && **k != RunSessionType::LongRun)
    {
        if free.is_empty() {
            break;
        }
        out.push((free.remove(0), k));
    }
    out
}

/// Plan + prescribe one week directly (no lift days).
fn week(p: &Profile, a: &Anchors, week_index: u32) -> (RunWeekPlan, Vec<Session>) {
    let days = run_slot_count(p);
    let plan = run_week_plan(p, a, &req(week_index, days));
    let sessions = run_week_sessions(&plan, &place(&plan), p, a);
    (plan, sessions)
}

fn run_of(s: &Session) -> &RunPrescription {
    match &s.prescriptions[0].value {
        Prescription::Run(r) => r,
        other => panic!("not a run: {other:?}"),
    }
}

fn kind_of(s: &Session) -> RunSessionType {
    match s.session_type {
        SessionType::Run(k) => k,
        other => panic!("not a run: {other:?}"),
    }
}

fn claim_of(s: &Session) -> crate::evidence::ClaimId {
    s.prescriptions[0].claim_id()
}

/// The first plan block index whose rolling phase is Build (cycle 1).
fn build_week(p: &Profile) -> u32 {
    (0..16)
        .find(|&w| run_week_plan(p, &Anchors::default(), &req(w, 5)).phase == MesoPhase::Build)
        // General fitness stays in Base: its "build" week is the first week.
        .unwrap_or(0)
}

fn run_kinds_of(prog: &Program) -> Vec<RunSessionType> {
    prog.mesocycle
        .sessions
        .iter()
        .filter_map(|s| match s.session_type {
            SessionType::Run(k) => Some(k),
            _ => None,
        })
        .collect()
}

/// (day, kind) of every run session, sorted by day.
fn run_days_sorted(prog: &Program) -> Vec<(u16, RunSessionType)> {
    let mut v: Vec<(u16, RunSessionType)> = prog
        .mesocycle
        .sessions
        .iter()
        .filter_map(|s| match s.session_type {
            SessionType::Run(k) => Some((s.day, k)),
            _ => None,
        })
        .collect();
    v.sort_by_key(|(d, _)| *d);
    v
}

/// The claim id cited by the long-run prescription in a synthesized program.
fn long_run_claim(prog: &Program) -> Option<crate::evidence::ClaimId> {
    prog.mesocycle
        .sessions
        .iter()
        .find(|s| s.session_type == SessionType::Run(RunSessionType::LongRun))
        .map(|s| s.prescriptions[0].claim_id())
}

/// A long-run decision context with no pace anchor.
fn ctx(weekly_km: f64, baseline: Option<f64>, n: usize) -> DayCtx {
    DayCtx {
        weekly_km,
        baseline,
        baseline_stated: false,
        detrained: false,
        n_run_days: n,
        recovery: false,
        taper: false,
        goal: GoalDistance::TenK,
        vdot: None,
        easy_pace: 390.0,
        long_km: None,
    }
}

fn km_of(v: RunVolume) -> f64 {
    match v {
        RunVolume::DistanceKm(k) => k as f64,
        _ => 0.0,
    }
}

// ── Basic week shape ─────────────────────────────────────────────────────

#[test]
fn running_profile_yields_a_long_run() {
    let p = runner(GoalDistance::TenK, 4, 30.0);
    let prog = synthesize(&p, &Anchors::default(), 0).unwrap();
    assert!(matches!(prog.goal, Goal::RunningRace { .. }));
    assert!(
        run_kinds_of(&prog).contains(&RunSessionType::LongRun),
        "a running week must include a long run"
    );
}

#[test]
fn hybrid_caps_run_days_at_the_endurance_frequency_cap() {
    let mut p = base_profile(); // weekly_sets = 12 → lifting present
    p.goal_distance = GoalDistance::TenK;
    p.running_days_per_week = 5;
    p.running_km_per_week = 40.0;
    let prog = synthesize(&p, &anchored("Back Squat", 130.0), 0).unwrap();
    assert!(
        run_kinds_of(&prog).len() <= 3,
        "hybrid run days capped at 3"
    );
}

#[test]
fn tempo_carries_the_full_hrmax_band() {
    let p = runner(GoalDistance::TenK, 5, 50.0);
    let (_, sessions) = week(&p, &Anchors::default(), 0);
    let tempo = sessions
        .iter()
        .find(|s| kind_of(s) == RunSessionType::Tempo)
        .expect("a base week keeps a light tempo");
    match run_of(tempo).intensity {
        RunIntensity::HrPercentMax { lo, hi, .. } => {
            assert_eq!(lo, Some(88.0));
            assert_eq!(hi, Some(92.0));
        }
        other => panic!("tempo is HR-governed: {other:?}"),
    }
}

#[test]
fn tempo_volume_is_capped_at_ten_percent_of_weekly_km() {
    let tempo_of = |weekly: f64| -> Option<RunVolume> {
        let p = runner(GoalDistance::TenK, 5, weekly);
        let (_, sessions) = week(&p, &Anchors::default(), 0);
        sessions
            .iter()
            .find(|s| kind_of(s) == RunSessionType::Tempo)
            .map(|s| run_of(s).volume)
    };
    // 16 km/wk: 10% = 1.6 km < the 3 km threshold floor → no Tempo session.
    assert!(tempo_of(16.0).is_none());
    // 50 km/wk: 10% = 5 km threshold distance; 35 km/wk: 3.5 km (0.5 km steps).
    assert_eq!(tempo_of(50.0), Some(RunVolume::DistanceKm(5.0)));
    assert_eq!(tempo_of(35.0), Some(RunVolume::DistanceKm(3.5)));
}

#[test]
fn tempo_needs_twenty_minutes_at_the_athletes_threshold_pace() {
    // 35 km/wk allows a 3.5 km tempo. Without a pace anchor that clears the
    // 3 km stand-in; at VDOT 50 (T ≈ 4:15/km) it is ~15 min, under
    // running-018's 20-minute floor, so the slot stays easy.
    let vdot50 = Anchors {
        run_vdot: Some(50.0),
        ..Default::default()
    };
    let has_tempo = |km: f64, a: &Anchors| {
        let p = runner(GoalDistance::TenK, 5, km);
        week(&p, a, 0)
            .1
            .iter()
            .any(|s| kind_of(s) == RunSessionType::Tempo)
    };
    assert!(has_tempo(35.0, &Anchors::default()));
    assert!(!has_tempo(35.0, &vdot50));
    // 50 km/wk: 5 km at ~4:15/km is over 20 min.
    assert!(has_tempo(50.0, &vdot50));
}

// ── Periodization and taper ───────────────────────────────────────

/// Monday epoch day the race tests count from.
const MON0: i64 = 20_000 - (20_000_i64.rem_euclid(7) + 3).rem_euclid(7);

#[test]
fn tenk_race_six_weeks_out_tapers_the_final_two_weeks_per_bosquet() {
    let mut p = runner(GoalDistance::TenK, 5, 60.0);
    // Race on the Saturday six weeks after the first plan week's Monday.
    p.goal_race_epoch_day = Some(MON0 + 42 + 5);
    let mut phases = Vec::new();
    let mut plans = Vec::new();
    for i in 0..=6u32 {
        let a = Anchors {
            week_start_epoch_day: Some(MON0 + 7 * i as i64),
            ..Default::default()
        };
        let (plan, sessions) = week(&p, &a, i);
        phases.push(plan.phase);
        plans.push((plan, sessions));
    }
    // Phases run Build → Peak → Taper in order (a recovery week may sit inside).
    let rank = |ph: MesoPhase| match ph {
        MesoPhase::Base => 0,
        MesoPhase::Build => 1,
        MesoPhase::Peak => 2,
        MesoPhase::Taper => 3,
        MesoPhase::Deload => -1,
    };
    let ordered: Vec<i32> = phases
        .iter()
        .map(|p| rank(*p))
        .filter(|r| *r >= 0)
        .collect();
    assert!(
        ordered.windows(2).all(|w| w[0] <= w[1]),
        "phases in order: {phases:?}"
    );
    assert_eq!(phases[0], MesoPhase::Build);
    assert!(
        phases[2..=4].iter().all(|p| *p == MesoPhase::Peak),
        "{phases:?}"
    );
    assert_eq!(&phases[5..], &[MesoPhase::Taper, MesoPhase::Taper]);

    // The last pre-taper week is the top load week; both taper weeks cut its
    // volume by 41–60 % (Bosquet), deepening toward the race.
    let top = plans[4].0.weekly_km;
    let t1 = plans[5].0.weekly_km;
    let t2 = plans[6].0.weekly_km;
    for t in [t1, t2] {
        let cut = 1.0 - t / top;
        assert!(
            (0.41 - 1e-9..=0.60 + 1e-9).contains(&cut),
            "taper cut {cut}"
        );
    }
    assert!(t2 < t1);
    // Frequency held: taper week 1 runs as many days as the load weeks.
    assert_eq!(plans[5].1.len(), plans[4].1.len());
    // Intensity held: each taper week keeps a quality session.
    for (plan, sessions) in &plans[5..] {
        assert!(
            sessions.iter().any(|s| is_quality(kind_of(s))),
            "taper week {:?} keeps quality",
            plan.taper_week
        );
    }
    // Race week: no long run, nothing on or after race day (Saturday).
    let (race_plan, race_sessions) = &plans[6];
    assert_eq!(race_plan.race_day, Some(5));
    assert!(race_sessions.iter().all(|s| s.day < 5));
    assert!(
        !race_sessions
            .iter()
            .any(|s| kind_of(s) == RunSessionType::LongRun)
    );
}

#[test]
fn no_race_date_rolls_base_and_build_cycles_without_peak_or_taper() {
    let p = runner(GoalDistance::HalfMarathon, 5, 40.0);
    let phases: Vec<MesoPhase> = (0..16)
        .map(|w| run_week_plan(&p, &Anchors::default(), &req(w, 5)).phase)
        .collect();
    assert_eq!(phases[0], MesoPhase::Base);
    assert!(phases.contains(&MesoPhase::Build));
    assert!(phases.contains(&MesoPhase::Deload));
    assert!(!phases.contains(&MesoPhase::Peak) && !phases.contains(&MesoPhase::Taper));
}

#[test]
fn a_past_race_date_falls_back_to_the_rolling_cycle() {
    let mut p = runner(GoalDistance::TenK, 5, 40.0);
    p.goal_race_epoch_day = Some(MON0 - 3);
    let a = Anchors {
        week_start_epoch_day: Some(MON0),
        ..Default::default()
    };
    let plan = run_week_plan(&p, &a, &req(0, 5));
    assert_eq!(plan.phase, MesoPhase::Base);
    assert_eq!(plan.race_day, None);
}

#[test]
fn a_mid_week_race_prorates_race_week_volume_to_the_days_left() {
    let mut p = runner(GoalDistance::TenK, 5, 40.0);
    // Race on the Thursday two weeks after the first plan week's Monday.
    p.goal_race_epoch_day = Some(MON0 + 14 + 3);
    let at = |i: u32| Anchors {
        week_start_epoch_day: Some(MON0 + 7 * i as i64),
        ..Default::default()
    };
    let (taper, _) = week(&p, &at(1), 1);
    let (race, sessions) = week(&p, &at(2), 2);
    assert_eq!(taper.taper_week, Some((1, 2)));
    assert_eq!(race.race_day, Some(3));
    // Three of the five run days are left before the race: the week's volume
    // shrinks with them instead of piling onto Monday–Wednesday.
    assert!(
        race.weekly_km <= taper.weekly_km * 3.0 / 5.0 + 1e-9,
        "race week {} vs taper week {}",
        race.weekly_km,
        taper.weekly_km
    );
    // No easy day outgrows the previous taper week's daily average.
    let cap = taper.weekly_km / 5.0;
    for s in sessions
        .iter()
        .filter(|s| matches!(kind_of(s), RunSessionType::Easy | RunSessionType::Recovery))
    {
        let km = km_of(run_of(s).volume);
        assert!(km <= cap + 1e-6, "day {}: {km} km > {cap}", s.day);
    }
}

#[test]
fn volume_holds_in_the_week_the_phase_steps_intensity_up() {
    // running-032: progress volume OR intensity, never both in one week.
    let mut p = runner(GoalDistance::TenK, 5, 40.0);
    p.goal_race_epoch_day = Some(MON0 + 7 * 20 + 5);
    let plans: Vec<RunWeekPlan> = (0..=20u32)
        .map(|i| {
            let a = Anchors {
                week_start_epoch_day: Some(MON0 + 7 * i as i64),
                ..Default::default()
            };
            run_week_plan(&p, &a, &req(i, 5))
        })
        .collect();
    let rank = |ph: MesoPhase| match ph {
        MesoPhase::Base => Some(0),
        MesoPhase::Build => Some(1),
        MesoPhase::Peak => Some(2),
        MesoPhase::Taper | MesoPhase::Deload => None,
    };
    let mut steps = 0;
    for w in plans.windows(2) {
        if let (Some(a), Some(b)) = (rank(w[0].phase), rank(w[1].phase))
            && b > a
        {
            steps += 1;
            assert!(
                w[1].weekly_km <= w[0].weekly_km + 1e-9,
                "{:?} → {:?}: {} → {} km",
                w[0].phase,
                w[1].phase,
                w[0].weekly_km,
                w[1].weekly_km
            );
        }
    }
    assert!(steps > 0, "the plan steps up at least once");
}

#[test]
fn recovery_week_cuts_by_the_kb_midpoint_and_shrinks_the_long_run() {
    // A demonstrated 14 km longest run sets the load-week long run; in the
    // recovery week the long run shrinks with the week and keeps its share.
    let p = runner(GoalDistance::HalfMarathon, 3, 30.0);
    let a = Anchors {
        longest_recent_run_km: Some(14.0),
        ..Default::default()
    };
    let rec = (0..8u32)
        .find(|&w| run_week_plan(&p, &a, &req(w, 3)).recovery)
        .expect("a recovery week");
    let (load, load_sessions) = week(&p, &a, 0);
    let (plan, sessions) = week(&p, &a, rec);
    let (lo, hi) = running::recovery_week_rx(running::MileageBand::Unspecified)
        .value
        .volume_reduction_frac;
    let mid = (lo.unwrap_or(0.0) + hi) / 2.0;
    assert!((plan.weekly_km - load.weekly_km * (1.0 - mid)).abs() < 1e-9);
    let long_of = |ss: &[Session]| {
        ss.iter()
            .find(|s| kind_of(s) == RunSessionType::LongRun)
            .map(|s| km_of(run_of(s).volume))
            .expect("a long run")
    };
    let share = running::long_run_share(plan.weekly_km).value;
    assert!(long_of(&sessions) <= share * plan.weekly_km + 1e-6);
    assert!(long_of(&sessions) < long_of(&load_sessions));
}

// ── Volume carries across cycles ──────────────────────────────────

#[test]
fn compliant_twelve_week_history_grows_cycle_start_volume() {
    let p = runner(GoalDistance::TenK, 5, 40.0); // 3:1 cycle, +5 %/wk
    let w = |i: u32, hist: Vec<f64>| {
        let a = Anchors {
            run_week_km: hist,
            ..Default::default()
        };
        run_week_plan(&p, &a, &req(i, 5)).weekly_km
    };
    let c1 = w(0, vec![]);
    assert!((c1 - 40.0).abs() < 1e-9);
    // Cycle 1 run exactly as prescribed (most recent first): 28 recovery,
    // 44, 42, 40.
    let c2 = w(4, vec![28.0, 44.0, 42.0, 40.0]);
    assert!(c2 > c1, "cycle 2 week 1 ({c2}) > cycle 1 week 1 ({c1})");
    assert!(
        (c2 - 44.0).abs() < 1e-9,
        "resumes from the load reached: {c2}"
    );
    // Cycle 2 run as prescribed too → cycle 3 starts higher again.
    let c3 = w(8, vec![30.8, 48.4, 46.2, 44.0, 28.0, 44.0, 42.0, 40.0]);
    assert!(c3 > c2, "cycle 3 ({c3}) > cycle 2 ({c2})");
    // Mid-cycle weeks keep the same base (no compounding inside a cycle).
    let c2w2 = w(5, vec![44.0, 28.0, 44.0, 42.0, 40.0]);
    assert!((c2w2 - 44.0 * 1.05).abs() < 1e-9, "{c2w2}");
}

#[test]
fn noncompliant_cycle_repeats_base() {
    let p = runner(GoalDistance::TenK, 5, 40.0);
    let a = Anchors {
        run_week_km: vec![15.0, 20.0, 20.0, 18.0],
        ..Default::default()
    };
    assert_eq!(run_week_plan(&p, &a, &req(4, 5)).weekly_km, 40.0);
}

#[test]
fn running_volume_progresses_across_load_weeks() {
    // A novice (larger +10% cap): the long run grows on a load week.
    let mut p = runner(GoalDistance::TenK, 5, 40.0);
    p.progression_cadence = ProgressionCadence::EverySession;
    let long0 = long_run_km(&synthesize(&p, &Anchors::default(), 0).unwrap()).unwrap();
    let long1 = long_run_km(&synthesize(&p, &Anchors::default(), 1).unwrap()).unwrap();
    assert!(long1 > long0, "{long1} !> {long0}");
    assert!(long1 <= long0 * 1.10 + 0.01);
}

// ── Quality per goal ──────────────────────────────────────────────

#[test]
fn marathon_build_week_has_race_pace_session() {
    let p = runner(GoalDistance::Marathon, 5, 60.0);
    let (plan, sessions) = week(&p, &Anchors::default(), build_week(&p));
    assert_eq!(plan.phase, MesoPhase::Build);
    let mp = sessions
        .iter()
        .find(|s| kind_of(s) == RunSessionType::RacePace)
        .expect("marathon build carries marathon-pace work");
    let r = run_of(mp);
    let km = km_of(r.volume);
    assert!((8.0..=0.20 * 60.0 + 1e-9).contains(&km), "MP block {km}");
    assert_eq!(r.warmup_min, Some(10));
    assert_eq!(r.cooldown_min, Some(10));
}

#[test]
fn fivek_peak_has_interval_and_repetition() {
    let mut p = runner(GoalDistance::FiveK, 5, 50.0);
    p.goal_race_epoch_day = Some(MON0 + 28 + 5); // 4 weeks out → Peak
    let a = Anchors {
        week_start_epoch_day: Some(MON0),
        ..Default::default()
    };
    let (plan, sessions) = week(&p, &a, 3);
    assert_eq!(plan.phase, MesoPhase::Peak);
    let kinds: Vec<RunSessionType> = sessions.iter().map(kind_of).collect();
    assert!(kinds.contains(&RunSessionType::Interval), "{kinds:?}");
    let reps = sessions
        .iter()
        .find(|s| kind_of(s) == RunSessionType::Repetition)
        .expect("5K peak carries repetitions");
    let r = run_of(reps);
    let (n, rep) = r.repeats.expect("rep structure");
    assert!(n >= 4);
    let rep_km = km_of(rep);
    assert!(
        n as f64 * rep_km <= 0.05 * 50.0 + 1e-6,
        "R ≤5 % of the week"
    );
    assert_eq!(
        r.recovery,
        Some(RepRecovery::Metres((rep_km * 1000.0).round() as u16))
    );
    assert_eq!(claim_of(reps), claims::RUN_REPETITION_002);
}

#[test]
fn interval_falls_back_to_800m_before_dropping() {
    let p = runner(GoalDistance::FiveK, 5, 40.0);
    let bw = build_week(&p);
    let prog = synthesize(&p, &Anchors::default(), bw).unwrap();
    let interval = prog
        .mesocycle
        .sessions
        .iter()
        .find(|s| s.session_type == SessionType::Run(RunSessionType::Interval))
        .expect("40 km/wk supports 3×800 m intervals");
    let (reps, rep_vol) = run_of(interval).repeats.expect("interval carries reps");
    assert!(reps >= 3);
    assert_eq!(rep_vol, RunVolume::DistanceKm(0.8));
    let low = runner(GoalDistance::FiveK, 5, 16.0);
    let low_prog = synthesize(&low, &Anchors::default(), bw).unwrap();
    assert!(!run_kinds_of(&low_prog).contains(&RunSessionType::Interval));
}

#[test]
fn interval_dropped_for_the_degenerate_low_volume_profile() {
    let p = runner(GoalDistance::FiveK, 6, 5.0);
    let prog = synthesize(&p, &Anchors::default(), build_week(&p)).unwrap();
    assert!(!run_kinds_of(&prog).contains(&RunSessionType::Interval));
}

#[test]
fn interval_prescribed_and_within_cap_when_volume_supports_it() {
    let p = runner(GoalDistance::FiveK, 5, 45.0);
    let prog = synthesize(&p, &Anchors::default(), build_week(&p)).unwrap();
    let interval = prog
        .mesocycle
        .sessions
        .iter()
        .find(|s| s.session_type == SessionType::Run(RunSessionType::Interval))
        .expect("45 km/wk supports an interval");
    let (reps, rep) = run_of(interval).repeats.expect("interval carries reps");
    assert!(reps >= 3);
    assert!(reps as f64 * km_of(rep) <= 0.08 * 45.0 + 1e-6, "≤8 % cap");
    assert!(matches!(run_of(interval).intensity, RunIntensity::Vdot(_)));
}

#[test]
fn interval_rx_carries_recovery_equal_to_rep_time() {
    let p = runner(GoalDistance::FiveK, 5, 50.0);
    let bw = build_week(&p);
    // No pace anchor: "jog as long as the rep took".
    let (_, sessions) = week(&p, &Anchors::default(), bw);
    let i = sessions
        .iter()
        .find(|s| kind_of(s) == RunSessionType::Interval)
        .unwrap();
    assert_eq!(run_of(i).recovery, Some(RepRecovery::EqualToRep));
    // With a VDOT anchor the recovery is the rep's own time at I pace.
    let a = Anchors {
        run_vdot: Some(50.0),
        ..Default::default()
    };
    let (_, sessions) = week(&p, &a, bw);
    let r = run_of(
        sessions
            .iter()
            .find(|s| kind_of(s) == RunSessionType::Interval)
            .unwrap(),
    );
    let (_, rep) = r.repeats.unwrap();
    let (fast, slow) =
        crate::load::vdot_band_pace_sec_per_km(50.0, crate::schema::VdotBand::Interval).unwrap();
    let Some(RepRecovery::Seconds(rec)) = r.recovery else {
        panic!("numeric recovery: {:?}", r.recovery);
    };
    let rec = f64::from(rec);
    assert!(
        rec >= km_of(rep) * fast - 1.0 && rec <= km_of(rep) * slow + 1.0,
        "{rec}"
    );
}

#[test]
fn low_quality_budget_honours_the_goal_table_at_four_run_days() {
    let p = runner(GoalDistance::FiveK, 4, 50.0);
    let prog = synthesize(&p, &Anchors::default(), build_week(&p)).unwrap();
    let quality = run_days_sorted(&prog)
        .into_iter()
        .filter(|(_, k)| is_quality(*k))
        .count();
    assert_eq!(
        quality, 2,
        "5K build at 4 run days honours the 2-quality table"
    );
}

#[test]
fn quality_run_days_stay_spaced_across_goals_and_frequencies() {
    let goals = [
        GoalDistance::General,
        GoalDistance::FiveK,
        GoalDistance::TenK,
        GoalDistance::HalfMarathon,
        GoalDistance::Marathon,
    ];
    for goal in goals {
        for days in 2u8..=7 {
            let p = runner(goal, days, 80.0);
            let prog = synthesize(&p, &Anchors::default(), build_week(&p)).unwrap();
            let q_days: Vec<u16> = run_days_sorted(&prog)
                .into_iter()
                .filter(|(_, k)| is_quality(*k))
                .map(|(d, _)| d)
                .collect();
            assert!(
                q_days.windows(2).all(|w| w[1] - w[0] >= 2),
                "goal {goal:?} days {days}: quality days {q_days:?} are not ≥2 apart"
            );
        }
    }
}

// ── Sub-10 km progression and the spike ceiling ───────────────────

#[test]
fn sub_10km_long_run_progresses_under_spike_ceiling() {
    let (km, bind) = long_run_decision(&ctx(40.0, Some(6.0), 4));
    assert_eq!(km, 6.5, "6.0 km longest → 6.5 km next, not 6 again");
    assert_eq!(bind, LongRunBind::SpikeCeiling);
    assert!(!running::single_session_spike(6.5, 6.0));
}

#[test]
fn baseline_5_4_never_prescribed_below_demonstrated() {
    let (km, _) = long_run_decision(&ctx(40.0, Some(5.4), 4));
    assert!(km >= 5.4, "{km}");
    assert!(!running::single_session_spike(km, 5.4));
    for b in [1.0, 2.3, 4.6, 4.95, 5.0, 7.9, 9.9, 13.0] {
        let c = cap_km_to_spike(100.0, Some(b)).unwrap();
        assert!(c >= b && !running::single_session_spike(c, b), "{b} → {c}");
    }
}

#[test]
fn a_sub_1km_baseline_never_prescribes_a_run_its_own_spike_gate_would_flag() {
    let baseline = 0.5;
    assert_eq!(cap_km_to_spike(5.0, Some(baseline)), None);
    let (target, bind) = long_run_decision(&ctx(40.0, Some(baseline), 3));
    assert_eq!(target, 0.5, "capped by the 0.5 km spike ceiling");
    assert_eq!(bind, LongRunBind::SpikeCeiling);
    assert!(bind.cites_spike());
    assert!(!running::single_session_spike(target, baseline));
    let capped = cap_km_to_spike(5.0, Some(3.0)).expect("3 km baseline → a distance");
    assert_eq!(capped, 3.3);
    assert!(!running::single_session_spike(capped, 3.0));
    assert_eq!(cap_km_to_spike(5.0, None), Some(5.0));
}

#[test]
fn spike_ceiling_is_identical_across_long_run_and_continuous_run() {
    assert_eq!(cap_km_to_spike(10.0, Some(3.9)), Some(4.2));
    let (target, bind) = long_run_decision(&ctx(40.0, Some(3.9), 3));
    assert_eq!(target, 4.2);
    assert_eq!(bind, LongRunBind::SpikeCeiling);
}

#[test]
fn stated_longest_8km_with_no_logs_caps_week_one_long_run() {
    // The athlete states 66 km/wk and an 8 km longest run, nothing
    // logged: the first long run respects the spike rule from day one.
    let mut p = runner(GoalDistance::HalfMarathon, 6, 66.0);
    p.stated_longest_recent_km = Some(8.0);
    let prog = synthesize(&p, &Anchors::default(), 0).unwrap();
    let km = long_run_km(&prog).expect("a long-run distance");
    assert!(km <= 8.8 + 1e-9, "week-1 long run {km} ≤ 8.8 km");
    assert_eq!(long_run_claim(&prog), Some(claims::RUN_START_001));
    for s in &prog.mesocycle.sessions {
        if let Some(Prescription::Run(r)) = s.prescriptions.first().map(|x| &x.value) {
            assert!(
                km_of(r.volume) <= 8.8 + 1e-9,
                "no run over the stated ceiling"
            );
        }
    }
    // A logged run supersedes the stated figure.
    let logged = Anchors {
        longest_recent_run_km: Some(12.0),
        ..Default::default()
    };
    let prog = synthesize(&p, &logged, 0).unwrap();
    assert_eq!(long_run_claim(&prog), Some(claims::RUN_SPIKE_001));
}

#[test]
fn a_short_logged_run_does_not_collapse_an_in_window_stated_long_run() {
    // Stated 8 mi longest, then a 2 mi shakeout is logged. The
    // stated run is still a run of the last 30 days, so it stays the baseline.
    let mut p = runner(GoalDistance::TenK, 3, 32.19);
    p.stated_longest_recent_km = Some(12.87);
    let in_window = Anchors {
        longest_recent_run_km: Some(3.22),
        stated_longest_in_window: true,
        ..Default::default()
    };
    let prog = synthesize(&p, &in_window, 0).unwrap();
    let km = long_run_km(&prog).expect("a long-run distance");
    let mi = (km / 1.609_344 * 10.0).round() / 10.0;
    assert!(mi >= 7.8, "week-1 long run {km} km shows ≥ 7.8 mi");
    assert!(
        km <= 12.87 * 1.1 + 1e-9,
        "still within 10% of the stated run"
    );
    assert_ne!(long_run_claim(&prog), Some(claims::DETRAIN_001));
    // Past its window the logged run supersedes the stated figure.
    let stale = Anchors {
        stated_longest_in_window: false,
        ..in_window
    };
    let prog = synthesize(&p, &stale, 0).unwrap();
    let km = long_run_km(&prog).expect("a long-run distance");
    assert!(km <= 3.22 * 1.1 + 1e-9, "logged baseline governs: {km}");
    // A logged run longer than the stated one wins either way.
    let longer = Anchors {
        longest_recent_run_km: Some(14.0),
        stated_longest_in_window: true,
        ..Default::default()
    };
    let prog = synthesize(&p, &longer, 0).unwrap();
    assert_ne!(long_run_claim(&prog), Some(claims::RUN_START_001));
}

#[test]
fn sub_1km_baseline_tempo_becomes_an_easy_duration() {
    let p = runner(GoalDistance::TenK, 5, 50.0);
    let anchors = Anchors {
        longest_recent_run_km: Some(0.5),
        ..Default::default()
    };
    let (_, sessions) = week(&p, &anchors, 0);
    let tempo = sessions
        .iter()
        .find(|s| kind_of(s) == RunSessionType::Tempo)
        .expect("a tempo slot is scheduled");
    assert!(matches!(run_of(tempo).volume, RunVolume::DurationMin(_)));
    assert_eq!(claim_of(tempo), claims::RUN_SPIKE_001);
}

// ── Long-run share and goal targets ───────────────────────────────

#[test]
fn sub_64km_week_long_run_share_is_30pct() {
    assert_eq!(
        long_run_decision(&ctx(40.0, None, 4)),
        (12.0, LongRunBind::Volume)
    );
    assert_eq!(
        long_run_decision(&ctx(80.0, None, 5)),
        (20.0, LongRunBind::Volume)
    );
}

#[test]
fn marathon_long_run_trends_toward_target_under_spike_cap() {
    let m = |w: f64, b: Option<f64>| {
        let mut c = ctx(w, b, 6);
        c.goal = GoalDistance::Marathon;
        long_run_decision(&c)
    };
    // Volume wants 30 km but the 20 km longest run caps it at 22.
    assert_eq!(m(120.0, Some(20.0)), (22.0, LongRunBind::SpikeCeiling));
    // Once volume and capacity allow, the 32 km goal ceiling holds it.
    assert_eq!(m(140.0, Some(33.0)), (32.0, LongRunBind::Target));
}

#[test]
fn long_run_is_not_always_the_150_min_ceiling() {
    let p = runner(GoalDistance::HalfMarathon, 4, 40.0);
    let km = long_run_km(&synthesize(&p, &Anchors::default(), 0).unwrap()).unwrap();
    assert!(km > 0.0 && km <= 0.30 * 40.0 + 0.01, "{km}");
}

#[test]
fn a_logged_21k_long_run_is_prescribed_not_a_4k_run() {
    // 3 run days, effective 30 (measured), demonstrated 21: share 9, the
    // ≤2×-daily-average bound 20 binds.
    let p = half_marathoner();
    let anchors = Anchors {
        longest_recent_run_km: Some(21.0),
        recent_weekly_km: Some(30.0),
        ..Default::default()
    };
    let prog = synthesize(&p, &anchors, 0).unwrap();
    assert_eq!(long_run_km(&prog), Some(20.0));
    assert_eq!(long_run_claim(&prog), Some(claims::RUN_LONGRUN_001));
}

#[test]
fn full_demonstrated_distance_is_not_prescribed_weekly_when_it_dwarfs_volume() {
    // 16 km/wk over 3 days: 2×daily average = 10.67 → 10.5 km.
    let p = half_marathoner();
    let anchors = Anchors {
        longest_recent_run_km: Some(21.0),
        ..Default::default()
    };
    let prog = synthesize(&p, &anchors, 0).unwrap();
    assert_eq!(long_run_km(&prog), Some(10.5));
    assert_eq!(long_run_claim(&prog), Some(claims::RUN_LONGRUN_001));
}

#[test]
fn a_demonstrated_anchored_long_run_within_bounds_cites_the_spike_rule() {
    // 40 km/wk over 4 days, demonstrated 14: share 12 < 14 ≤ daily cap 20.
    let mut p = half_marathoner();
    p.running_days_per_week = 4;
    p.running_km_per_week = 40.0;
    let anchors = Anchors {
        longest_recent_run_km: Some(14.0),
        recent_weekly_km: Some(40.0),
        ..Default::default()
    };
    let prog = synthesize(&p, &anchors, 0).unwrap();
    assert_eq!(long_run_km(&prog), Some(14.0));
    assert_eq!(long_run_claim(&prog), Some(claims::RUN_SPIKE_001));
}

#[test]
fn a_volume_dominated_long_run_still_cites_the_share_cap() {
    let mut p = half_marathoner();
    p.running_km_per_week = 60.0;
    let prog = synthesize(&p, &Anchors::default(), 0).unwrap();
    assert_eq!(long_run_km(&prog), Some(18.0), "30 % of 60 km");
    assert_eq!(long_run_claim(&prog), Some(claims::RUN_LONGRUN_001));
}

#[test]
fn a_low_demonstrated_run_caps_a_higher_volume_target_at_the_spike_ceiling() {
    let mut p = half_marathoner();
    p.running_km_per_week = 60.0;
    let anchors = Anchors {
        longest_recent_run_km: Some(12.0),
        recent_weekly_km: Some(60.0),
        ..Default::default()
    };
    let prog = synthesize(&p, &anchors, 0).unwrap();
    assert_eq!(long_run_km(&prog), Some(13.0));
    assert_eq!(long_run_claim(&prog), Some(claims::RUN_SPIKE_001));
}

#[test]
fn no_run_history_matches_the_volume_only_plan() {
    let mut p = half_marathoner();
    p.running_km_per_week = 40.0;
    let prog = synthesize(&p, &Anchors::default(), 0).unwrap();
    assert_eq!(prog, synthesize(&p, &Anchors::default(), 0).unwrap());
    assert_eq!(long_run_km(&prog), Some(12.0));
}

#[test]
fn measured_volume_raises_the_long_run_even_without_a_longest_anchor() {
    let mut p = half_marathoner();
    p.running_km_per_week = 16.0;
    let anchors = Anchors {
        recent_weekly_km: Some(60.0),
        ..Default::default()
    };
    let prog = synthesize(&p, &anchors, 0).unwrap();
    assert_eq!(long_run_km(&prog), Some(18.0));
}

#[test]
fn a_high_volume_target_is_still_capped_by_a_low_demonstrated_run() {
    let mut p = half_marathoner();
    p.running_km_per_week = 16.0;
    let anchors = Anchors {
        longest_recent_run_km: Some(12.0),
        recent_weekly_km: Some(80.0),
        ..Default::default()
    };
    let prog = synthesize(&p, &anchors, 0).unwrap();
    assert_eq!(long_run_km(&prog), Some(13.0));
    assert_eq!(long_run_claim(&prog), Some(claims::RUN_SPIKE_001));
}

#[test]
fn effective_weekly_km_takes_the_higher_of_stated_and_measured() {
    let mut p = half_marathoner();
    p.running_km_per_week = 16.0;
    let with = |m: Option<f64>| Anchors {
        recent_weekly_km: m,
        ..Default::default()
    };
    assert_eq!(effective_weekly_km(&p, &with(Some(48.0))), 48.0);
    assert_eq!(effective_weekly_km(&p, &with(Some(8.0))), 16.0);
    assert_eq!(effective_weekly_km(&p, &Anchors::default()), 16.0);
}

#[test]
fn post_layoff_prefers_measured_over_a_stale_stated_volume() {
    let mut p = runner(GoalDistance::TenK, 4, 40.0);
    p.weeks_off = Some(4.0);
    let anchors = Anchors {
        recent_weekly_km: Some(10.0),
        ..Default::default()
    };
    assert_eq!(effective_weekly_km(&p, &anchors), 10.0);
    let mut p2 = p.clone();
    p2.weeks_off = None;
    assert_eq!(effective_weekly_km(&p2, &anchors), 40.0);
    assert_eq!(effective_weekly_km(&p, &Anchors::default()), 40.0);
}

// ── Detraining-adjusted anchors slope the long run, no cliff ─────────────

fn detrained_anchor(km: f64) -> Anchors {
    Anchors {
        longest_recent_run_km: Some(km),
        longest_run_detrained: true,
        ..Default::default()
    }
}

#[test]
fn detrained_anchor_keeps_a_long_run_instead_of_collapsing_to_the_volume_floor() {
    let p = half_marathoner(); // 16 km/wk, 3 days
    let cliff = synthesize(&p, &Anchors::default(), 0).unwrap();
    // The prescription stores km as f32: 4.8 widens to 4.80000019 in f64.
    let cliff_km = long_run_km(&cliff).expect("a long-run distance");
    assert!((cliff_km - 4.8).abs() < 1e-5, "{cliff_km}");
    assert_eq!(long_run_claim(&cliff), Some(claims::RUN_LONGRUN_001));
    // Day 31: anchor 20.3 km, held at the 2×-daily-average bound (10.5).
    let d31 = synthesize(&p, &detrained_anchor(21.1 * 27.0 / 28.0), 0).unwrap();
    assert_eq!(long_run_km(&d31), Some(10.5));
    assert_eq!(long_run_claim(&d31), Some(claims::DETRAIN_001));
    // Day 45: anchor 9.8 km → its own floor (9.5) governs.
    let d45 = synthesize(&p, &detrained_anchor(21.1 * 13.0 / 28.0), 0).unwrap();
    assert_eq!(long_run_km(&d45), Some(9.5));
    assert_eq!(long_run_claim(&d45), Some(claims::DETRAIN_001));
    assert!(long_run_km(&d31) > long_run_km(&d45));
    assert!(long_run_km(&d45) > long_run_km(&cliff));
}

#[test]
fn detrained_flag_does_not_relabel_a_pure_volume_long_run() {
    // 40 km/wk, 4 days, anchor exactly at the 30 % share (12 km).
    let mut p = half_marathoner();
    p.running_days_per_week = 4;
    p.running_km_per_week = 40.0;
    let prog = synthesize(&p, &detrained_anchor(12.0), 0).unwrap();
    assert_eq!(long_run_km(&prog), Some(12.0));
    assert_eq!(long_run_claim(&prog), Some(claims::RUN_LONGRUN_001));
}

// ── Easy runs vs recovery runs ────────────────────────────────────

#[test]
fn easy_fill_uses_easy_band_not_recovery() {
    let p = runner(GoalDistance::TenK, 5, 50.0);
    let (_, sessions) = week(&p, &Anchors::default(), 0);
    let easy = sessions
        .iter()
        .find(|s| kind_of(s) == RunSessionType::Easy)
        .expect("an easy day");
    assert_eq!(claim_of(easy), claims::RUN_EASY_001);
    match run_of(easy).intensity {
        RunIntensity::HrPercentMax { lo, hi, .. } => {
            assert_eq!((lo, hi), (Some(65.0), Some(79.0)));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn recovery_day_only_after_quality() {
    let p = runner(GoalDistance::TenK, 6, 60.0);
    let (_, sessions) = week(&p, &Anchors::default(), build_week(&p));
    let hard: Vec<u16> = sessions
        .iter()
        .filter(|s| is_quality(kind_of(s)) || kind_of(s) == RunSessionType::LongRun)
        .map(|s| s.day)
        .collect();
    let mut saw_recovery = false;
    for s in &sessions {
        match kind_of(s) {
            RunSessionType::Recovery => {
                saw_recovery = true;
                assert!(s.day > 0 && hard.contains(&(s.day - 1)), "day {}", s.day);
                assert_eq!(claim_of(s), claims::RUN_RECOVERY_001);
                // Inside the 20–40 min recovery band at the counted pace.
                if let RunVolume::DistanceKm(k) = run_of(s).volume {
                    assert!(k as f64 * 390.0 / 60.0 <= 40.0 + 1e-6, "{k}");
                }
            }
            RunSessionType::Easy => {
                assert!(s.day == 0 || !hard.contains(&(s.day - 1)), "day {}", s.day);
            }
            _ => {}
        }
    }
    assert!(
        saw_recovery,
        "the placement puts an easy day after a hard day"
    );
}

#[test]
fn hybrid_two_run_week_includes_an_easy_run() {
    let mut p = base_profile();
    p.goal_distance = GoalDistance::HalfMarathon;
    p.running_days_per_week = 2;
    p.running_km_per_week = 30.0;
    let prog = synthesize(&p, &anchored("Back Squat", 130.0), 0).unwrap();
    let kinds = run_kinds_of(&prog);
    assert!(
        kinds.contains(&RunSessionType::Easy) || kinds.contains(&RunSessionType::Recovery),
        "a hybrid week must include an easy run (80/20), got {kinds:?}"
    );
}

#[test]
fn low_volume_easy_days_stay_under_the_spike_ceiling_and_the_long_run() {
    // 8 km/wk over 3 days, longest recent run 2.5 km: the long run is 2.5 km
    // and each easy day takes its sized share as a distance, never a fixed
    // 30-min run (≈4 km at a beginner pace, +50 % over the longest run).
    let mut p = runner(GoalDistance::General, 3, 8.0);
    p.stated_longest_recent_km = Some(2.5);
    let (plan, sessions) = week(&p, &Anchors::default(), 0);
    let pace = running::warmup_rx().value.accounting_easy_pace_sec_per_km;
    let km_or_minutes = |v: RunVolume| match v {
        RunVolume::DistanceKm(k) => k as f64,
        RunVolume::DurationMin(m) => m as f64 * 60.0 / pace,
        other => panic!("unexpected volume {other:?}"),
    };
    let long = sessions
        .iter()
        .find(|s| kind_of(s) == RunSessionType::LongRun)
        .map(|s| km_or_minutes(run_of(s).volume))
        .expect("a long run");
    let ceiling = 2.7; // 2.5 km + 10 %, floored to 0.1 km
    let mut total = 0.0;
    for s in &sessions {
        let km = km_or_minutes(run_of(s).volume);
        assert!(
            km <= ceiling + 1e-6,
            "{:?} over the spike ceiling",
            run_of(s).volume
        );
        assert!(
            km <= long + 1e-6,
            "{:?} longer than the long run",
            run_of(s).volume
        );
        assert!(
            !matches!(run_of(s).volume, RunVolume::DurationMin(30)),
            "no fixed 30-min fallback"
        );
        total += km;
    }
    assert!(
        total <= plan.weekly_km + 1e-6,
        "{total} > {}",
        plan.weekly_km
    );
}

#[test]
fn a_sub_1km_baseline_never_gets_a_fixed_duration_above_its_ceiling() {
    // Longest run 0.5 km: the long run, the easy days and the degraded
    // quality slot all stay within the 0.5 km ceiling at easy pace.
    let p = runner(GoalDistance::TenK, 3, 40.0);
    let a = Anchors {
        longest_recent_run_km: Some(0.5),
        ..Default::default()
    };
    let (_, sessions) = week(&p, &a, 0);
    let pace = running::warmup_rx().value.accounting_easy_pace_sec_per_km;
    assert!(!sessions.is_empty());
    for s in &sessions {
        let km = match run_of(s).volume {
            RunVolume::DistanceKm(k) => k as f64,
            RunVolume::DurationMin(m) => m as f64 * 60.0 / pace,
            other => panic!("unexpected volume {other:?}"),
        };
        assert!(km <= 0.5 + 1e-6, "{:?}: {km} km", kind_of(s));
    }
}

#[test]
fn no_stated_or_measured_volume_plans_the_kb_duration_floors() {
    // Weekly distance not given (0) and nothing logged: easy days are the
    // running-015 30-min floor, never a distance sized from an invented figure.
    let mut p = runner(GoalDistance::General, 3, 0.0);
    p.running_km_per_week_set = false;
    let (_, sessions) = week(&p, &Anchors::default(), 0);
    assert_eq!(sessions.len(), 3);
    for s in &sessions {
        assert!(
            matches!(run_of(s).volume, RunVolume::DurationMin(20 | 30)),
            "{:?}: {:?}",
            kind_of(s),
            run_of(s).volume
        );
    }
}

// ── Warm-up/cool-down, rep structure, weekly accounting ───────────

#[test]
fn quality_week_with_wu_cd_sums_to_weekly_km() {
    let p = runner(GoalDistance::TenK, 5, 50.0);
    let (plan, sessions) = week(&p, &Anchors::default(), build_week(&p));
    let pace = running::warmup_rx().value.accounting_easy_pace_sec_per_km;
    let mut total = 0.0;
    let mut soft = 0;
    for s in &sessions {
        let r = run_of(s);
        let main = match (r.repeats, r.volume) {
            (Some((n, rep)), _) if matches!(r.recovery, Some(RepRecovery::Metres(_))) => {
                n as f64 * km_of(rep) * 2.0
            }
            (_, v) => km_of(v),
        };
        let wucd = (r.warmup_min.unwrap_or(0) + r.cooldown_min.unwrap_or(0)) as f64 * 60.0 / pace;
        if is_quality(kind_of(s)) {
            assert_eq!(r.warmup_min, Some(10), "quality runs carry a warm-up");
            assert_eq!(r.cooldown_min, Some(10), "and a cool-down");
        }
        if matches!(kind_of(s), RunSessionType::Easy | RunSessionType::Recovery) {
            soft += 1;
        }
        total += main + wucd;
    }
    assert!(
        (total - plan.weekly_km).abs() <= 0.5 * soft as f64 + 0.01,
        "week sums to {total:.2} vs {:.2}",
        plan.weekly_km
    );
}

#[test]
fn residual_easy_day_never_exceeds_twice_the_executed_daily_average() {
    let p = runner(GoalDistance::TenK, 4, 40.0);
    let prog = synthesize(&p, &Anchors::default(), 0).unwrap();
    let km: Vec<f64> = prog
        .mesocycle
        .sessions
        .iter()
        .filter(|s| matches!(s.session_type, SessionType::Run(_)))
        .map(|s| km_of(run_of(s).volume))
        .collect();
    let longest = km.iter().copied().fold(0.0_f64, f64::max);
    let executed: f64 = km.iter().sum();
    assert!(longest <= 2.0 * daily_avg_run_km(executed, 4));
}

#[test]
fn easy_days_are_never_longer_than_the_long_run() {
    let p = runner(GoalDistance::General, 3, 30.0);
    let (_, sessions) = week(&p, &Anchors::default(), 0);
    let long = sessions
        .iter()
        .find(|s| kind_of(s) == RunSessionType::LongRun)
        .map(|s| km_of(run_of(s).volume))
        .unwrap();
    for s in &sessions {
        assert!(km_of(run_of(s).volume) <= long + 1e-9);
    }
}

#[test]
fn strides_attached_to_easy_day_after_base_week_2() {
    let p = runner(GoalDistance::TenK, 5, 50.0);
    let strides_in = |w: u32| {
        week(&p, &Anchors::default(), w)
            .1
            .iter()
            .filter_map(|s| run_of(s).strides.map(|st| (kind_of(s), st)))
            .collect::<Vec<_>>()
    };
    assert!(
        strides_in(0).is_empty(),
        "no strides in the first base weeks"
    );
    let late = strides_in(2);
    assert!(!late.is_empty() && late.len() <= 2, "{late:?}");
    for (k, st) in late {
        assert_eq!(k, RunSessionType::Easy);
        assert!((4..=8).contains(&st.reps));
        assert!((15..=30).contains(&st.rep_sec));
        assert!((45..=120).contains(&st.recovery_sec));
    }
}

// ── C25K ────────────────────────────────────────────────────────

#[test]
fn c25k_week_one_prescribes_8x60s_90s_run_walk_and_no_long_run() {
    let p = runner(GoalDistance::C25k, 3, 15.0);
    let prog = synthesize(&p, &Anchors::default(), 0).unwrap();
    let kinds = run_kinds_of(&prog);
    assert!(!kinds.contains(&RunSessionType::LongRun));
    let run_walks = prog
        .mesocycle
        .sessions
        .iter()
        .filter(|s| matches!(s.session_type, SessionType::Run(_)))
        .filter(|s| {
            run_of(s).volume
                == RunVolume::RunWalk {
                    run_sec: 60,
                    walk_sec: 90,
                    rounds: 8,
                }
        })
        .count();
    assert_eq!(run_walks, 3);
}

#[test]
fn c25k_week_nine_is_30_min_continuous() {
    let p = runner(GoalDistance::C25k, 3, 15.0);
    let prog = synthesize(&p, &Anchors::default(), 8).unwrap();
    let continuous = prog
        .mesocycle
        .sessions
        .iter()
        .filter(|s| matches!(s.session_type, SessionType::Run(_)))
        .filter(|s| run_of(s).volume == RunVolume::DurationMin(30))
        .count();
    assert_eq!(continuous, 3);
}

#[test]
fn c25k_week_five_runs_follow_the_nhs_bouts_in_order() {
    let p = runner(GoalDistance::C25k, 3, 15.0);
    let (plan, sessions) = week(&p, &Anchors::default(), 4);
    assert_eq!(plan.c25k_week, Some(5));
    let bouts: Vec<Vec<(u16, u16)>> = sessions
        .iter()
        .map(|s| {
            run_of(s)
                .segments
                .iter()
                .map(|b| (b.run_sec, b.walk_sec))
                .collect()
        })
        .collect();
    assert_eq!(
        bouts,
        vec![
            vec![(300, 180), (300, 180), (300, 0)],
            vec![(480, 300), (480, 0)],
            vec![(1200, 0)],
        ]
    );
    for s in &sessions {
        assert_eq!(run_of(s).warmup_min, Some(5), "5-min brisk walk first");
        assert_eq!(claim_of(s), claims::RUN_C25K_002);
    }
}

#[test]
fn c25k_never_skips_the_bridge_run_with_fewer_run_days() {
    // running-025: three runs a week even when two days are stated.
    let p = runner(GoalDistance::C25k, 2, 0.0);
    assert_eq!(run_slot_count(&p), 3);
    // Only two days fit: the week keeps its last two runs, so week 5 still
    // ends on the first 20-min continuous run.
    let plan = run_week_plan(&p, &Anchors::default(), &req(4, 2));
    let sessions = run_week_sessions(&plan, &place(&plan), &p, &Anchors::default());
    let bouts: Vec<Vec<(u16, u16)>> = sessions
        .iter()
        .map(|s| {
            run_of(s)
                .segments
                .iter()
                .map(|b| (b.run_sec, b.walk_sec))
                .collect()
        })
        .collect();
    assert_eq!(bouts, vec![vec![(480, 300), (480, 0)], vec![(1200, 0)]]);
}

// ── Band helpers ─────────────────────────────────────────────────────────

#[test]
fn low_mid_u16_ceiling_only_band_propagates_none() {
    assert_eq!(mid_u16((None, Some(150))), None);
    assert_eq!(mid_u16((Some(20), Some(40))), Some(30));
    assert_eq!(mid_u16((Some(20), None)), Some(20));
    assert_eq!(mid_u16((None, None)), None);
}

#[test]
fn mid_f64_ceiling_only_band_propagates_none() {
    assert_eq!(mid_f64((None, Some(10.0))), None);
    assert_eq!(mid_f64((Some(4.0), Some(8.0))), Some(6.0));
    assert_eq!(mid_f64((Some(5.0), None)), Some(5.0));
    assert_eq!(mid_f64((None, None)), None);
}

#[test]
fn continuous_run_ceiling_only_distance_degrades_to_duration() {
    let base = RunWorkoutRx {
        pct_hr_max: (Some(0.7), Some(0.8)),
        pct_hrr_max: None,
        pct_slower_than_mp: (None, None),
        rpe: Some((3, 4)),
        duration_min: (None, None),
        distance_km: (None, Some(12.0)),
        hr_governed: true,
    };
    let p = continuous_prescription(RunSessionType::Hills, &base, None);
    assert_eq!(p.volume, RunVolume::DurationMin(30));
    let with_dur = RunWorkoutRx {
        duration_min: (Some(30), Some(60)),
        ..base
    };
    let p2 = continuous_prescription(RunSessionType::Hills, &with_dur, None);
    assert_eq!(p2.volume, RunVolume::DurationMin(45));
}
