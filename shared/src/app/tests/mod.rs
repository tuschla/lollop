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
mod copy_tone;
mod differential_calcs;
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
mod kb_golden;
mod lift_audit;
mod lift_cues;
mod long_horizon;
mod mutation_guards;
mod no_age;
mod onboarding;
mod overtraining;
mod pain_readiness;
mod parser_fuzz;
mod readiness_checkin;
mod readiness_feedback;
mod review_ledger;
mod review_plan;
mod review_robust;
mod review_runs;
mod review_tests;
mod reviews;
mod run_data;
mod run_spike;
mod run_splits;
mod run_trim;
mod run_view;
mod running_plan;
mod safety_redteam;
mod schedule;
mod session_adjustments;
mod set_logging;
mod stale_signals;
mod strength_coaching;
mod strength_plan;
mod track_figures_consistency;
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
        utc_offset_sec: None,
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
        utc_offset_sec: None,
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
        utc_offset_sec: None,
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

// ── GPS and logged-run fixtures ────────────────────────────────────────────

/// A 5 m-accuracy fix on the equator at longitude `lon`, unix-seconds `t`.
fn gps(lon: f64, t: i64) -> GpsPoint {
    GpsPoint {
        lat: 0.0,
        lon,
        observed_at: t,
        accuracy_m: 5.0,
    }
}

/// A straight eastward GPS track along the equator: `(metres, m/s)` legs
/// sampled every `step_s` seconds, starting at `t0`.
fn equator_track(t0: i64, legs: &[(f64, f64)], step_s: i64) -> Vec<GpsPoint> {
    let m_per_deg = crate::running::haversine_m(gps(0.0, 0), gps(1.0, 0));
    let mut pts = vec![gps(0.0, t0)];
    let (mut lon, mut t) = (0.0_f64, t0);
    for &(metres, speed) in legs {
        let n = (metres / speed / step_s as f64).round() as i64;
        let per = metres / n as f64;
        for _ in 0..n {
            lon += per / m_per_deg;
            t += step_s;
            pts.push(gps(lon, t));
        }
    }
    pts
}

/// A stored hand-logged run (no track, no meta) of `km` in `minutes` at
/// `hr`% HRmax, stamped `at`.
pub(in crate::app) fn logged_run(km: f64, minutes: f64, hr: f64, at: i64) -> LoggedRun {
    LoggedRun {
        distance_km: km,
        duration_min: minutes,
        hr_pct_max: hr,
        wire_hr_pct_max: hr,
        longest_recent_km: 0.0,
        wire_longest_recent_km: 0.0,
        track: Vec::new(),
        track_segment_starts: Vec::new(),
        observed_at: at,
        entry_id: 0,
        logged_utc_offset_sec: None,
        hr_pinned: false,
        workout_type: None,
        session_rpe: None,
        avg_hr_bpm: None,
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

/// `ev` (a [`log_run`] or [`log_run_track`] event) carrying the optional run
/// meta: workout tag, session RPE and average HR in bpm.
fn with_run_meta(
    mut ev: Event,
    tag: Option<WorkoutType>,
    rpe: Option<u8>,
    bpm: Option<f64>,
) -> Event {
    let (Event::LogRun {
        workout_type,
        session_rpe,
        avg_hr_bpm,
        ..
    }
    | Event::LogRunTrack {
        workout_type,
        session_rpe,
        avg_hr_bpm,
        ..
    }) = &mut ev
    else {
        unreachable!("with_run_meta takes a LogRun or LogRunTrack")
    };
    *workout_type = tag;
    *session_rpe = rpe;
    *avg_hr_bpm = bpm;
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
        edited_at: None,
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
        wire_hr_pct_max: 71.0,
        longest_recent_km: 3.3,
        wire_longest_recent_km: 3.3,
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
        logged_utc_offset_sec: None,
        hr_pinned: false,
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

/// [`build_run_anchors`] over `runs` with their realised distances, as
/// `(longest_recent_run_km, recent_weekly_km, longest_run_detrained)` for
/// terse test destructuring.
fn run_anchors(runs: &[LoggedRun], today: i64, offset: i64) -> (Option<f64>, Option<f64>, bool) {
    let a = build_run_anchors(&RunFacts::new(runs), today, offset);
    (
        a.longest_recent_run_km,
        a.recent_weekly_km,
        a.longest_run_detrained,
    )
}

const HOUR: i64 = 3_600;
const WEEK: i64 = 7 * DAY_SEC;

fn log_run_at(km: f64, minutes: f64, hr: f64, at: i64) -> Event {
    // 50.0 longest_recent_km quiets the spike gate; not under test here.
    log_run(km, minutes, hr, 50.0, at, 0)
}

/// `SetToday` to `epoch_day` at UTC.
fn set_today_ev(epoch_day: i64) -> Event {
    Event::SetToday {
        epoch_day,
        utc_offset_sec: 0,
    }
}

/// Move the shell's clock to `epoch_day` (UTC).
fn set_today(app: &Engine, model: &mut Model, epoch_day: i64) {
    app.update(set_today_ev(epoch_day), model)
        .expect_only_render();
}

/// Epoch day of a civil date (Hinnant's `days_from_civil`).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
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

/// A Monday (`mon0_weekday == 0`) whose unix timestamps are positive, so dated
/// inputs, reviews and removals take their dated (windowed) paths.
const POS_MON: i64 = 20_353;

/// `profile` set, `history` replayed, then a plan generated from `start` with
/// today on `start` (UTC).
fn planned_on(
    profile: Profile,
    history: impl IntoIterator<Item = Event>,
    start: i64,
) -> (Engine, Model) {
    let app = Engine;
    let mut model = Model::default();
    app.update(Event::SetProfile(profile), &mut model)
        .expect_only_render();
    for ev in history {
        app.update(ev, &mut model).expect_only_render();
    }
    app.update(
        Event::GeneratePlan {
            start_epoch_day: start,
        },
        &mut model,
    )
    .expect_only_render();
    set_today(&app, &mut model, start);
    (app, model)
}

/// The week-strip row for `epoch_day`.
fn day_row(app: &Engine, model: &Model, epoch_day: i64) -> SessionPlanView {
    app.view(model)
        .week_plan
        .into_iter()
        .find(|s| s.epoch_day == epoch_day)
        .expect("a week-strip row for the day")
}

fn planned_strength_model(with_set: bool) -> (Engine, Model) {
    planned_strength_model_on(MON, with_set)
}

/// [`planned_strength_model`] with the plan starting (and today) on Monday
/// epoch-day `mon`.
fn planned_strength_model_on(mon: i64, with_set: bool) -> (Engine, Model) {
    // A pre-plan baseline set (dated the day BEFORE the plan starts): it
    // seeds the e1RM anchor (day-independent) WITHOUT logging today's
    // session, so `today` (mon) stays the unlogged upcoming session the
    // next-session/readiness tests exercise. Tests that need TODAY logged
    // (done-day advancement) log it explicitly.
    let baseline = with_set.then(|| log_set("Back Squat", 120.0, 3, 8.0, (mon - 1) * DAY_SEC));
    planned_on(strength_profile(), baseline, mon)
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
        utc_offset_sec: None,
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
    planned_on(calisthenics_profile(), [], MON)
}

/// The first week-plan prescription whose exercise maps to `ladder`, if any.
fn cal_item(vm: &ViewModel, ladder: Lad) -> Option<PrescriptionView> {
    vm.week_plan
        .iter()
        .flat_map(|s| s.items.iter())
        .find(|it| crate::calisthenics::tier_by_name(&it.exercise).map(|(l, _)| l) == Some(ladder))
        .cloned()
}

/// The model after replaying a `tests/fixtures/view_snapshot` event log.
fn snapshot_fixture_model(ndjson: &str) -> Model {
    let mut model = Model::default();
    for line in ndjson.lines().filter(|l| !l.trim().is_empty()) {
        let event: Event = serde_json::from_str(line).expect("fixture line");
        let _ = Engine.update(event, &mut model);
    }
    model
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

// ── Seeded generators ──────────────────────────────────────────────────────

/// Deterministic 0..1 sequence (64-bit LCG, Knuth MMIX constants): the same
/// stream on every run and platform, so the `view_equivalence` pins hold.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    fn chance(&mut self, p: f64) -> bool {
        self.next() < p
    }

    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next()
    }

    /// In `lo..=hi`.
    fn int(&mut self, lo: u32, hi: u32) -> u32 {
        lo + ((self.next() * f64::from(hi - lo + 1)) as u32).min(hi - lo)
    }

    /// In `lo..=hi`.
    fn pick(&mut self, lo: u8, hi: u8) -> u8 {
        lo + (self.next() * f64::from(hi - lo + 1))
            .floor()
            .min(f64::from(hi - lo)) as u8
    }
}

/// xorshift64*: deterministic, dependency-free.
struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// In [0, 1).
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// `next() % n`, in `0..n` (`n` = 0 reads as 1).
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }

    /// `floor(unit() × n)`, in `0..n`.
    fn scaled(&mut self, n: u64) -> u64 {
        (self.unit() * n as f64) as u64
    }

    fn chance(&mut self, p: f64) -> bool {
        self.unit() <= p
    }

    fn pick<T: Clone>(&mut self, v: &[T]) -> T {
        v[self.below(v.len() as u64) as usize].clone()
    }
}

/// SplitMix64: a seedable stream; `SplitMix(x).next()` is one step from `x`.
struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// In `0..n`; `n` = 0 gives 0 without a draw.
    fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }

    /// `next() % n` as an index, in `0..n` (`n` = 0 reads as 1, one draw).
    fn index(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    /// In `lo..=hi`.
    fn int(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1) as u64) as i64
    }

    /// In [0, 1).
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }

    fn pick<T: Clone>(&mut self, xs: &[T]) -> T {
        xs[self.below(xs.len() as u64) as usize].clone()
    }
}

// ── Replay and view diffs ──────────────────────────────────────────────────

/// `events` replayed on a fresh model.
fn replay(events: &[Event]) -> (Engine, Model) {
    let app = Engine;
    let mut model = Model::default();
    for e in events {
        let _ = app.update(e.clone(), &mut model);
    }
    (app, model)
}

fn vm_json(vm: &ViewModel) -> String {
    serde_json::to_string(vm).expect("a ViewModel always serializes")
}

fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<String>()
        .cloned()
        .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_else(|| "<non-string panic>".into())
}

/// `events` replayed from an empty model on a fresh thread, so every
/// thread-local view cache starts cold: the final view JSON, or the panic.
fn replay_cold(events: Vec<Event>) -> Result<String, String> {
    std::thread::Builder::new()
        .stack_size(32 << 20)
        .spawn(move || {
            let app = Engine;
            let mut model = Model::default();
            for e in events {
                let _ = app.update(e, &mut model);
            }
            vm_json(&app.view(&model))
        })
        .expect("spawn replay thread")
        .join()
        .map_err(|p| format!("cold replay PANIC {}", panic_text(&*p)))
}

/// The largest char boundary of `s` at or before byte `i`.
fn char_floor(s: &str, i: usize) -> usize {
    let mut i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Up to `len` bytes of `s` from `before` bytes ahead of byte `at`, cut on
/// char boundaries: the context of a hit in a failure message.
fn excerpt(s: &str, at: usize, before: usize, len: usize) -> &str {
    let lo = char_floor(s, at.saturating_sub(before));
    &s[lo..char_floor(s, lo + len)]
}

/// The first path where two JSON values differ, with both values
/// (shortened). Numbers within `rel_tol` (relative) count as equal.
fn value_diff(a: &serde_json::Value, b: &serde_json::Value, rel_tol: f64) -> Option<String> {
    fn walk(
        a: &serde_json::Value,
        b: &serde_json::Value,
        tol: f64,
        path: &mut String,
    ) -> Option<String> {
        use serde_json::Value as V;
        match (a, b) {
            (V::Object(x), V::Object(y)) => {
                for (k, va) in x {
                    let len = path.len();
                    path.push('.');
                    path.push_str(k);
                    match y.get(k) {
                        None => return Some(format!("{path}: only on the left")),
                        Some(vb) => {
                            if let Some(d) = walk(va, vb, tol, path) {
                                return Some(d);
                            }
                        }
                    }
                    path.truncate(len);
                }
                y.keys()
                    .find(|k| !x.contains_key(*k))
                    .map(|k| format!("{path}.{k}: only on the right"))
            }
            (V::Array(x), V::Array(y)) => {
                for (i, (va, vb)) in x.iter().zip(y).enumerate() {
                    let len = path.len();
                    path.push_str(&format!("[{i}]"));
                    if let Some(d) = walk(va, vb, tol, path) {
                        return Some(d);
                    }
                    path.truncate(len);
                }
                (x.len() != y.len()).then(|| format!("{path}: length {} vs {}", x.len(), y.len()))
            }
            _ if a == b => None,
            (V::Number(x), V::Number(y))
                if x.as_f64()
                    .zip(y.as_f64())
                    .is_some_and(|(x, y)| (x - y).abs() <= tol * x.abs().max(y.abs())) =>
            {
                None
            }
            _ => {
                let cut = |v: &V| {
                    let s = v.to_string();
                    if s.len() > 160 {
                        format!("{}…", &s[..char_floor(&s, 160)])
                    } else {
                        s
                    }
                };
                Some(format!("{path}: {} vs {}", cut(a), cut(b)))
            }
        }
    }
    walk(a, b, rel_tol, &mut String::new())
}

/// The first path where two view JSON documents differ.
fn json_diff(a: &str, b: &str) -> String {
    let va: serde_json::Value = serde_json::from_str(a).expect("left view parses");
    let vb: serde_json::Value = serde_json::from_str(b).expect("right view parses");
    value_diff(&va, &vb, 0.0).unwrap_or_else(|| "byte-different but JSON-equal".into())
}

/// The full log and its compaction (`compact_event_log`, what the shell
/// replays after a relaunch) render the same view on cold threads.
fn assert_compaction_equivalent(name: &str, events: Vec<Event>) {
    let full = replay_cold(events.clone()).expect("full replay");
    let compacted = replay_cold(crate::compact_event_log(events)).expect("compacted replay");
    assert!(
        full == compacted,
        "{name}: the compacted log renders a different view: {}",
        json_diff(&full, &compacted)
    );
}

// ── Clock, lifter, GPS and run-stamp fixtures ──────────────────────────────

/// `SetToday` to `epoch_day` at UTC offset `utc_offset_sec`.
fn set_today_off_ev(epoch_day: i64, utc_offset_sec: i64) -> Event {
    Event::SetToday {
        epoch_day,
        utc_offset_sec,
    }
}

/// A Monday (`long_horizon`'s START, 2026-01-05).
const DAY0: i64 = 20_458;

/// Metres per degree of longitude on the equator for the core's haversine
/// (R = 6 371 000 m), so `x` metres east is `x / M_PER_DEG` degrees.
const M_PER_DEG: f64 = 6_371_000.0 * std::f64::consts::PI / 180.0;

/// A 5 m-accuracy fix `x_m` metres east of (0, 0) at unix second `t`.
fn fix(x_m: f64, t: i64) -> GpsPoint {
    gps(x_m / M_PER_DEG, t)
}

/// `ev` (a [`log_run`] or [`log_run_track`] event) stamped with the device's
/// UTC offset at logging time, as the shell logs it.
fn stamped(mut ev: Event, offset: i64) -> Event {
    let (Event::LogRun { utc_offset_sec, .. } | Event::LogRunTrack { utc_offset_sec, .. }) =
        &mut ev
    else {
        unreachable!("stamped takes a LogRun or LogRunTrack")
    };
    *utc_offset_sec = Some(offset);
    ev
}

/// A hypertrophy lifter (16 sets, 4 lift days, plan from [`DAY0`]) who logs
/// three sets of each `(exercise, kg, reps, seconds after 18:00)` of `lifts`,
/// in order, on Mondays and Thursdays for six weeks, then stops lifting.
/// Returns the model and the last lift day.
fn hypertrophy_lifter(lifts: &[(&str, f64, u32, i64)]) -> (Model, i64) {
    let app = Engine;
    let mut m = Model::default();
    set_today(&app, &mut m, DAY0);
    let _ = app.update(
        Event::SetProfile(Profile {
            weekly_sets: 16,
            lift_days_per_week: Some(4),
            lift_goal: LiftGoal::Hypertrophy,
            concurrent_goal: ConcurrentGoal::Hypertrophy,
            effective_from_epoch_day: Some(DAY0),
            ..base_profile()
        }),
        &mut m,
    );
    let _ = app.update(
        Event::GeneratePlan {
            start_epoch_day: DAY0,
        },
        &mut m,
    );
    let mut id = 0;
    for w in 0..6 {
        for wd in [0, 3] {
            let d = DAY0 + 7 * w + wd;
            set_today(&app, &mut m, d);
            for k in 0..3 {
                for &(exercise, kg, reps, after) in lifts {
                    id += 1;
                    let t = d * DAY_SEC + 64_800 + k * 120 + after;
                    let _ = app.update(
                        with_entry_id(log_set(exercise, kg, reps, 8.0, t), id),
                        &mut m,
                    );
                }
            }
        }
    }
    (m, DAY0 + 7 * 5 + 3)
}
