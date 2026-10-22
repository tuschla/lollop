//! App-level tests through `update` / `view`, one file per behaviour. This
//! module holds the shared fixtures; `base_profile` is also the fixture for
//! the `plan` and `log` tests.

use super::*;
use crate::DAY_SEC;
use crate::autoreg::SignalGroup;
use crate::autoreg::tests::input;
use crate::feedback::{FeedbackCategory, TrendDirection};
use crate::schema::{MesoPhase, ReadinessSignal, SafetyTier, SessionType, ThreeZone};

mod activity_log;
mod autoregulation;
mod calculators;
mod calendar_edges;
mod calisthenics;
mod calisthenics_plan;
mod completion;
mod copy_invariants;
mod entry_edits;
mod event_fuzz;
mod event_wire;
mod feedback_loop;
mod future_dated_runs;
mod guidance;
mod headline;
mod hypertrophy_plan;
mod illness;
mod illness_paths;
mod ingest_sanitize;
mod lift_audit;
mod lift_cues;
mod long_horizon;
mod no_age;
mod onboarding;
mod overtraining;
mod pain_readiness;
mod readiness_checkin;
mod readiness_feedback;
mod reviews;
mod run_data;
mod run_spike;
mod run_splits;
mod run_trim;
mod run_view;
mod running_plan;
mod schedule;
mod session_adjustments;
mod set_logging;
mod stale_signals;
mod strength_coaching;
mod strength_plan;
mod training_load;
mod view_equivalence;
mod view_wire;
mod why_disclosure;
mod youth_activity;

// ── Run-event constructor helpers ────────────────────────────────────────
// The `LogRun`/`LogRunTrack`/`AmendRun` struct-enum variants have no
// `Default`, so every literal construction site breaks when a field is
// added. Routing all test construction through these helpers isolates that
// churn to one place: adding an optional field edits only the helper
// body, never the ~30 call sites. Positional args mirror the variant fields.
fn log_run(
    distance_km: f64,
    duration_min: f64,
    hr_pct_max: f64,
    longest_recent_km: f64,
    observed_at: i64,
    entry_id: u64,
) -> Event {
    Event::LogRun {
        distance_km,
        duration_min,
        hr_pct_max,
        longest_recent_km,
        observed_at,
        entry_id,
        workout_type: None,
        session_rpe: None,
        avg_hr_bpm: None,
    }
}

fn log_run_track(
    points: Vec<GpsPoint>,
    hr_pct_max: f64,
    longest_recent_km: f64,
    observed_at: i64,
    entry_id: u64,
) -> Event {
    Event::LogRunTrack {
        points,
        hr_pct_max,
        longest_recent_km,
        observed_at,
        entry_id,
        workout_type: None,
        session_rpe: None,
        segment_starts: Vec::new(),
        avg_hr_bpm: None,
    }
}

/// [`log_run_track`] with explicit pause-bridge boundaries.
fn log_run_track_seg(
    points: Vec<GpsPoint>,
    hr_pct_max: f64,
    longest_recent_km: f64,
    observed_at: i64,
    entry_id: u64,
    segment_starts: Vec<u32>,
) -> Event {
    Event::LogRunTrack {
        points,
        hr_pct_max,
        longest_recent_km,
        observed_at,
        entry_id,
        workout_type: None,
        session_rpe: None,
        segment_starts,
        avg_hr_bpm: None,
    }
}

fn trim(
    entry_id: u64,
    observed_at_fallback: i64,
    keep_from_sec: i64,
    keep_until_sec: i64,
) -> Event {
    Event::TrimRunTrack {
        entry_id,
        observed_at_fallback,
        keep_from_sec,
        keep_until_sec,
    }
}

// ── Set and profile fixtures ───────────────────────────────────────────────

/// A `LogSet` of `weight_kg` × `reps` @ `rpe` at unix-seconds `at`, with no
/// entry id and no timed hold. Test sets go through here (or the wrappers
/// below) so a new `LogSet` field edits one literal.
fn log_set(exercise: &str, weight_kg: f64, reps: u32, rpe: f64, at: i64) -> Event {
    Event::LogSet {
        hold_sec: None,
        exercise: exercise.into(),
        weight_kg,
        reps,
        rpe,
        observed_at: at,
        entry_id: 0,
    }
}

/// `ev` (a [`log_set`] event) carrying entry id `id`.
fn with_entry_id(mut ev: Event, id: u64) -> Event {
    let Event::LogSet { entry_id, .. } = &mut ev else {
        unreachable!("with_entry_id takes a LogSet")
    };
    *entry_id = id;
    ev
}

/// A 5 × `weight_kg` @ RPE 8 set carrying entry id `id`.
fn log_set_id(exercise: &str, weight_kg: f64, id: u64, at: i64) -> Event {
    with_entry_id(log_set(exercise, weight_kg, 5, 8.0, at), id)
}

/// The one complete `Profile` literal: a neutral intermediate lifter with no
/// running and every optional field unset. Every other profile fixture
/// (here, `plan/tests` and the log tests) is `..base_profile()`.
pub(crate) fn base_profile() -> Profile {
    Profile {
        resistance_modality: ResistanceModality::Weights,
        progression_cadence: ProgressionCadence::WeekToWeek,
        lift_goal: LiftGoal::MaxStrength,
        goal_distance: GoalDistance::General,
        concurrent_goal: ConcurrentGoal::Strength,
        weekly_sets: 12,
        running_days_per_week: 0,
        running_km_per_week: 0.0,
        running_km_per_week_set: true,
        advanced: false,
        female: Some(false),
        high_load_block: false,
        health: HealthScreen::default(),
        environment: None,
        env_temp_c: None,
        env_altitude_m: None,
        weeks_off: None,
        bodyweight_kg: None,
        age_years: None,
        birth_epoch_day: None,
        resting_hr_bpm: None,
        measured_hr_max: None,
        weight_lb: false,
        effective_from_epoch_day: None,
        lift_days_per_week: None,
        training_weekdays: None,
        goal_race_epoch_day: None,
        recent_race: None,
        stated_longest_recent_km: None,
        layoff_returned_epoch_day: None,
        meet_epoch_day: None,
        calis_equipment: None,
        specialization_muscle: None,
        runner_strength: false,
    }
}

/// An advanced marathon-priority hybrid athlete (5 run days, 50 km/wk).
fn sample_profile() -> Profile {
    Profile {
        goal_distance: GoalDistance::Marathon,
        concurrent_goal: ConcurrentGoal::EndurancePriority,
        weekly_sets: 14,
        running_days_per_week: 5,
        running_km_per_week: 50.0,
        advanced: true,
        ..base_profile()
    }
}

/// A distinctive GPS run whose content is unlikely to be pre-cached by any
/// other test on this thread, so the memo counter delta is meaningful.
fn probe_run(entry_id: u64) -> LoggedRun {
    LoggedRun {
        distance_km: 0.0,
        duration_min: 0.0,
        hr_pct_max: 71.0,
        longest_recent_km: 3.3,
        track: vec![
            GpsPoint {
                lat: 12.5,
                lon: 34.5000,
                observed_at: 1_000,
                accuracy_m: 4.0,
            },
            GpsPoint {
                lat: 12.5,
                lon: 34.5010,
                observed_at: 1_050,
                accuracy_m: 4.0,
            },
            GpsPoint {
                lat: 12.5,
                lon: 34.5020,
                observed_at: 1_100,
                accuracy_m: 4.0,
            },
            GpsPoint {
                lat: 12.5,
                lon: 34.5030,
                observed_at: 1_150,
                accuracy_m: 4.0,
            },
        ],
        track_segment_starts: Vec::new(),
        observed_at: 1_000,
        entry_id,
        workout_type: None,
        session_rpe: None,
        avg_hr_bpm: None,
    }
}

fn profile_with_health(health: HealthScreen) -> Profile {
    Profile {
        health,
        ..sample_profile()
    }
}

/// One run's memoized card, as `view()` derives it.
fn run_card(r: &LoggedRun) -> RunResultView {
    let facts = RunFacts::new(std::slice::from_ref(r));
    to_run_view(r, facts.keys[0], facts.qc[0].as_deref())
}

/// [`build_run_anchors`] over `runs` with their realised distances.
fn run_anchors(runs: &[LoggedRun], today: i64, offset: i64) -> RunAnchors {
    build_run_anchors(runs, &RunFacts::new(runs), today, offset)
}

const WEEK: i64 = 7 * DAY_SEC;

fn log_run_at(km: f64, minutes: f64, hr: f64, at: i64) -> Event {
    // 50.0 longest_recent_km quiets the spike gate; not under test here.
    log_run(km, minutes, hr, 50.0, at, 0)
}

// ── Coach-as-planner ────────────────────────

/// A pure-strength profile with no running (guided-setup shape).
fn strength_profile() -> Profile {
    Profile {
        weekly_sets: 14,
        ..base_profile()
    }
}

// Monday epoch-day: mon0_weekday(4) == 0, so day-0 (Heavy) lands on `today`.
const MON: i64 = 4;

fn planned_strength_model(with_set: bool) -> (Engine, Model) {
    planned_strength_model_on(MON, with_set)
}

/// [`planned_strength_model`] with the plan starting (and today) on Monday
/// epoch-day `mon`.
fn planned_strength_model_on(mon: i64, with_set: bool) -> (Engine, Model) {
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(strength_profile()), &mut model)
        .expect_only_render();
    if with_set {
        // A pre-plan baseline set (dated the day BEFORE the plan starts): it
        // seeds the e1RM anchor (day-independent) WITHOUT logging today's
        // session, so `today` (mon) stays the unlogged upcoming session the
        // next-session/readiness tests exercise. Tests that need TODAY logged
        // (done-day advancement) log it explicitly.
        app.update(
            log_set("Back Squat", 120.0, 3, 8.0, (mon - 1) * DAY_SEC),
            &mut model,
        )
        .expect_only_render();
    }
    app.update(
        Event::GeneratePlan {
            start_epoch_day: mon,
        },
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::SetToday {
            epoch_day: mon,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    (app, model)
}

/// Log one Back Squat set on every day of `weeks` plan weeks starting at the
/// Monday `first_mon`, so those weeks count as completed: the plan advances by
/// completed work, not the calendar (ADH-PROGRESS-001).
fn complete_plan_weeks(app: &Engine, model: &mut Model, first_mon: i64, weeks: i64) {
    for day in first_mon..first_mon + weeks * 7 {
        app.update(
            log_set("Back Squat", 100.0, 3, 7.0, day * DAY_SEC + 12 * 3_600),
            model,
        )
        .expect_only_render();
    }
}

// ── Run planning reactive to logged run history ──

/// A pure-running half-marathon profile whose STATED weekly volume is the low
/// guided-setup heuristic (16 km/wk) that never updates on its own.
fn running_profile() -> Profile {
    Profile {
        goal_distance: GoalDistance::HalfMarathon,
        weekly_sets: 0,
        running_days_per_week: 3,
        running_km_per_week: 16.0,
        ..base_profile()
    }
}

fn logrun(distance_km: f64, day: i64) -> Event {
    Event::LogRun {
        distance_km,
        duration_min: distance_km * 6.0,
        hr_pct_max: 70.0,
        longest_recent_km: 0.0,
        observed_at: day * DAY_SEC,
        entry_id: 0,
        workout_type: None,
        session_rpe: None,
        avg_hr_bpm: None,
    }
}

fn planned_long_run_item(app: &Engine, model: &Model) -> PrescriptionView {
    app.view(model)
        .week_plan
        .iter()
        .find(|s| s.title == "Long run")
        .expect("a running week has a long run")
        .items[0]
        .clone()
}

// ── Calisthenics modality ────────────────────────────────────────────────
use crate::schema::LadderId as Lad;

fn calisthenics_profile() -> Profile {
    Profile {
        resistance_modality: ResistanceModality::Calisthenics,
        ..strength_profile()
    }
}

/// A timed-hold set (seconds × RPE) logged at unix-seconds `at`.
fn cal_hold(exercise: &str, sec: u16, rpe: f64, at: i64) -> Event {
    let mut ev = log_set(exercise, 0.0, 1, rpe, at);
    let Event::LogSet { hold_sec, .. } = &mut ev else {
        unreachable!()
    };
    *hold_sec = Some(sec);
    ev
}

fn planned_calisthenics() -> (Engine, Model) {
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(calisthenics_profile()), &mut model)
        .expect_only_render();
    app.update(
        Event::GeneratePlan {
            start_epoch_day: MON,
        },
        &mut model,
    )
    .expect_only_render();
    app.update(
        Event::SetToday {
            epoch_day: MON,
            utc_offset_sec: 0,
        },
        &mut model,
    )
    .expect_only_render();
    (app, model)
}

/// The first week-plan prescription whose exercise maps to `ladder`, if any.
fn cal_item(vm: &ViewModel, ladder: Lad) -> Option<PrescriptionView> {
    vm.week_plan
        .iter()
        .flat_map(|s| s.items.iter())
        .find(|it| crate::calisthenics::tier_by_name(&it.exercise).map(|(l, _)| l) == Some(ladder))
        .cloned()
}

// ── Rendered-session fixtures ──────────────────────────────────────────────

/// A 3 × 5 @ RIR 2 lift item at `load` kg.
fn lift_item(name: &str, load: f64) -> PrescriptionView {
    PrescriptionView {
        exercise: name.into(),
        sets: 3,
        reps_low: 5,
        reps_high: 5,
        load_kg: Some(load),
        intensity: PlanIntensity::Rir { rir: 2 },
        intensity_label: "RIR 2".into(),
        target_rpe: Some(8.0),
        ..PrescriptionView::default()
    }
}

/// Today's next session holding `items`.
fn session(items: Vec<PrescriptionView>) -> SessionPlanView {
    SessionPlanView {
        status: SessionStatus::Next,
        items,
        ..SessionPlanView::default()
    }
}

/// A pain report of `value`, graded severity 3 when `kind` is given.
fn pain_row(value: f64, kind: Option<crate::schema::PainKind>, observed_at: i64) -> ReadinessInput {
    ReadinessInput {
        signal: ReadinessSignal::Pain,
        value,
        observed_at,
        streak: 0,
        pain: kind.map(|k| crate::schema::PainDetail {
            kind: k,
            severity: Some(3),
            trend: crate::schema::PainTrend::Stable,
            persists: false,
            location: None,
        }),
        effort_min: None,
    }
}

// ── Guidance-list contracts ────────────────────────────────────────────────

/// HARD RULE 2 on a rendered list: every row cites evidence and none is
/// graded MarketingMyth. `ctx` names the input in a failure.
fn assert_cited_non_myth(rows: &[GuidanceView], ctx: &dyn std::fmt::Debug) {
    for row in rows {
        assert!(
            !row.evidence.citation.is_empty(),
            "uncited row for {ctx:?}: {}",
            row.summary
        );
        assert_ne!(
            row.evidence.grade,
            Some(EvidenceGrade::MarketingMyth),
            "myth surfaced for {ctx:?}: {}",
            row.summary
        );
    }
}

/// Each section forms one contiguous run. The shell prints a section header
/// only when the previous row's section differs, so a section reappearing
/// after a gap would render a duplicate header (an orphan card).
fn assert_sections_contiguous(rows: &[GuidanceView], ctx: &dyn std::fmt::Debug) {
    let mut runs: Vec<&str> = Vec::new();
    for row in rows {
        let s = row.section.as_str();
        if runs.last() != Some(&s) {
            assert!(
                !runs.contains(&s),
                "section {s:?} is non-contiguous for {ctx:?}"
            );
            runs.push(s);
        }
    }
}
