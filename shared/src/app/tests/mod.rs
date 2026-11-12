//! App-level tests through `update` / `view`, one file per behaviour. This
//! module holds the shared app fixtures.

use super::*;
use crate::app::anchors::{AnchorCtx, Layoff, build_run_anchors, build_run_week_km};
use crate::app::calculators::{build_hr_zones, race_band};
use crate::app::evidence_views::grade_note_str;
use crate::app::format::{
    fmt_clock, fmt_km_or_mi, fmt_mss, fmt_weight, fmt_weight_num, mon0_weekday,
    race_distance_label, race_distance_title,
};
use crate::app::guidance::{build_guidance_measured, stated_or_measured_weekly_km};
use crate::app::lift_audit::to_lift_view;
use crate::app::logged_days::DeloadDays;
use crate::app::model::{LoggedRun, LoggedSet, RaceQuery};
use crate::app::plan_views::{PlanItem, PlanRow, flatten_prescription, plan_session_title};
use crate::app::profile_age::whole_years_between;
use crate::app::run_metrics::{MAX_LOAD_DAYS, RunFacts};
use crate::app::run_views::{run_card as memo_run_card, split_verdict_view};
use crate::app::set_index::SetIndex;
use crate::app::view_types::{
    EvidenceTagView, HeadlineKind, ItemStatus, PaceVariability, PlanIntensity, ProteinTargetKind,
    RunTargetView, SplitVerdict,
};
use crate::autoreg::SignalGroup;
use crate::autoreg::tests::input;
use crate::day::DAY_SEC;
use crate::event::{
    ActivityKind, Decouple, EntryKind, IntervalExec, LiftExec, RunMeta, SessionReview,
};
use crate::evidence::{ClaimId, EvidenceGrade, claims};
use crate::feedback::{self, FeedbackCategory, TrendDirection};
use crate::gps::GpsPoint;
use crate::hybrid::ConcurrentGoal;
use crate::individualization::{Environment, ProgressionCadence};
use crate::memo::MemoId;
use crate::plates::{LB_PER_KG, round_load_grid};
use crate::profile::test_support::base_profile;
use crate::profile::{CalisEquipment, DistanceUnit, Profile, RecentRace, ResistanceModality};
use crate::running::{self, GoalDistance};
use crate::schema::{
    HealthScreen, LiftIntensity, LiftSessionType, MesoPhase, PainRegion, Prescription,
    ReadinessSignal, Recommended, RunIntensity, RunSessionType, RunVolume, Rx, RxBasis, SafetyTier,
    SessionType, ThreeZone, VdotBand, WorkoutType,
};
use crate::strength::{self, LiftGoal};
use crate::test_support::{M_PER_DEG, SplitMix};
use crate::{hypertrophy, load};

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
mod draft_facts;
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
mod text_parts;
mod today_surface;
mod track_figures_consistency;
mod training_load;
mod view_equivalence;
mod view_wire;
mod why_disclosure;
mod youth_activity;

// The `LogRun`/`LogRunTrack`/`AmendRun` struct-enum variants have no
// `Default`, so every literal construction site breaks when a field is
// added. Routing all test construction through these helpers isolates that
// churn to one place: adding an optional field edits only the helper
// body, never the ~30 call sites. Positional args mirror the variant fields.
pub(crate) fn log_run(
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
        meta: RunMeta::default(),
    }
}

pub(crate) fn log_run_track(
    points: Vec<GpsPoint>,
    hr_pct_max: f64,
    longest_recent_km: f64,
    observed_at: i64,
    entry_id: u64,
) -> Event {
    log_run_track_seg(
        points,
        hr_pct_max,
        longest_recent_km,
        observed_at,
        entry_id,
        Vec::new(),
    )
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
        segment_starts,
        meta: RunMeta::default(),
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

/// A 5 m-accuracy fix on the equator at longitude `lon`, unix-seconds `t`.
pub(crate) fn gps(lon: f64, t: i64) -> GpsPoint {
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
    let mut pts = vec![fix(0.0, t0)];
    let (mut x, mut t) = (0.0_f64, t0);
    for &(metres, speed) in legs {
        let n = (metres / speed / step_s as f64).round() as i64;
        let per = metres / n as f64;
        for _ in 0..n {
            x += per;
            t += step_s;
            pts.push(fix(x, t));
        }
    }
    pts
}

/// A straight GPS track due north from (`lat0`°, 8°E): a fix at `t0`, then
/// one after each leg of `legs_m` metres, `step_s` seconds apart, every fix
/// `accuracy_m` accurate.
fn north_track(
    lat0: f64,
    t0: i64,
    step_s: i64,
    legs_m: impl IntoIterator<Item = f64>,
    accuracy_m: f32,
) -> Vec<GpsPoint> {
    let at = |lat: f64, t: i64| GpsPoint {
        lat,
        lon: 8.0,
        observed_at: t,
        accuracy_m,
    };
    let (mut lat, mut t) = (lat0, t0);
    let mut pts = vec![at(lat, t)];
    for leg in legs_m {
        lat += leg / M_PER_DEG;
        t += step_s;
        pts.push(at(lat, t));
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
        hr_pinned: false,
        meta: RunMeta::default(),
    }
}

/// A `LogSet` of `weight_kg` × `reps` @ `rpe` at unix-seconds `at`, with no
/// entry id and no timed hold. Test sets go through here (or the wrappers
/// below) so a new `LogSet` field edits one literal.
pub(crate) fn log_set(exercise: &str, weight_kg: f64, reps: u32, rpe: f64, at: i64) -> Event {
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
pub(crate) fn with_entry_id(mut ev: Event, id: u64) -> Event {
    let Event::LogSet { entry_id, .. } = &mut ev else {
        unreachable!("with_entry_id takes a LogSet")
    };
    *entry_id = id;
    ev
}

/// An `AmendRun` of entry `entry_id` (a legacy id-0 row: the one stamped
/// `fallback`) to `distance_km` in `duration_min` at `hr_pct_max`% HRmax,
/// stamped `observed_at`, without run meta.
pub(crate) fn amend_run(
    entry_id: u64,
    distance_km: f64,
    duration_min: f64,
    hr_pct_max: f64,
    observed_at: i64,
    fallback: i64,
) -> Event {
    Event::AmendRun {
        entry_id,
        distance_km,
        duration_min,
        hr_pct_max,
        longest_recent_km: 0.0,
        observed_at,
        observed_at_fallback: fallback,
        meta: RunMeta::default(),
    }
}

/// `ev` (a [`log_run`], [`log_run_track`] or [`amend_run`] event) carrying the
/// optional run meta: workout tag, session RPE and average HR in bpm.
pub(crate) fn with_run_meta(
    mut ev: Event,
    tag: Option<WorkoutType>,
    rpe: Option<u8>,
    bpm: Option<f64>,
) -> Event {
    let (Event::LogRun { meta, .. }
    | Event::LogRunTrack { meta, .. }
    | Event::AmendRun { meta, .. }) = &mut ev
    else {
        unreachable!("with_run_meta takes a LogRun, LogRunTrack or AmendRun")
    };
    meta.workout_type = tag;
    meta.session_rpe = rpe;
    meta.avg_hr_bpm = bpm;
    ev
}

/// A 5 × `weight_kg` @ RPE 8 set carrying entry id `id`.
fn log_set_id(exercise: &str, weight_kg: f64, id: u64, at: i64) -> Event {
    with_entry_id(log_set(exercise, weight_kg, 5, 8.0, at), id)
}

/// A stored set of `weight_kg` × `reps` @ `rpe` at unix-seconds `at`, with no
/// entry id and no timed hold: the stored twin of [`log_set`].
fn logged_set(exercise: &str, weight_kg: f64, reps: u32, rpe: f64, at: i64) -> LoggedSet {
    LoggedSet {
        exercise: exercise.into(),
        weight_kg,
        reps,
        rpe,
        observed_at: at,
        entry_id: 0,
        hold_sec: None,
    }
}

/// A `LogActivity` of `minutes` of `kind` at unix-seconds `at`.
pub(crate) fn log_activity(
    kind: ActivityKind,
    minutes: f64,
    vigorous: bool,
    strengthening: bool,
    observed_at: i64,
    entry_id: u64,
) -> Event {
    Event::LogActivity {
        kind,
        minutes,
        vigorous,
        strengthening,
        observed_at,
        entry_id,
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

fn profile_with_health(health: HealthScreen) -> Profile {
    Profile {
        health,
        ..sample_profile()
    }
}

/// The second-half slowdown percent behind one run's memoized card, as
/// `view()` derives it.
fn run_split_pct(r: &LoggedRun) -> Option<f64> {
    let facts = RunFacts::new(std::slice::from_ref(r));
    memo_run_card(r, facts.keys[0], facts.qc[0].as_deref(), DistanceUnit::Km).split_pct
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

/// The lift anchors a plan day after local day `through` is prescribed from
/// (`anchors::lift_anchors_through`), for `sets` stamped in UTC.
fn anchors_through(sets: &[LoggedSet], ctx: AnchorCtx, through: i64) -> crate::plan::LiftAnchors {
    crate::app::anchors::lift_anchors_through(
        sets,
        &SetIndex::new(sets),
        &crate::app::logged_days::attribute_session_days(sets, 0),
        ctx,
        through,
        0,
    )
}

const HOUR: i64 = 3_600;
const WEEK: i64 = 7 * DAY_SEC;

fn log_run_at(km: f64, minutes: f64, hr: f64, at: i64) -> Event {
    // 50.0 longest_recent_km quiets the spike gate; not under test here.
    log_run(km, minutes, hr, 50.0, at, 0)
}

/// `SetToday` to `epoch_day` at UTC.
pub(crate) fn set_today_ev(epoch_day: i64) -> Event {
    set_today_off_ev(epoch_day, 0)
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

/// Monday-based weekday of epoch day `d` (1970-01-05, day 4, was a Monday),
/// computed without the core's `mon0_weekday`.
fn wd(d: i64) -> i64 {
    (d - 4).rem_euclid(7)
}

/// The Monday on or before day `d`.
fn monday_of(d: i64) -> i64 {
    d - wd(d)
}

/// `hour:min` local on local day `d` at `offset` seconds east of UTC, as the
/// UTC stamp the shell sends.
fn local_at(d: i64, hour: i64, min: i64, offset: i64) -> i64 {
    d * DAY_SEC + hour * HOUR + min * 60 - offset
}

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

/// A `distance_km` run at 6:00/km and 70 % HRmax, stamped midnight UTC of
/// `day`.
fn logrun(distance_km: f64, day: i64) -> Event {
    logrun_at(distance_km, day * DAY_SEC)
}

/// [`logrun`] stamped at unix-seconds `at`.
fn logrun_at(distance_km: f64, at: i64) -> Event {
    log_run(distance_km, distance_km * 6.0, 70.0, 0.0, at, 0)
}

/// The first long run of the week strip, as `view()` shows it.
fn planned_long_run_item(model: &Model) -> PrescriptionView {
    Engine
        .view(model)
        .week_plan
        .into_iter()
        .find(|s| s.title == "Long run")
        .expect("a running week has a long run")
        .items
        .swap_remove(0)
}

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
    let events: Vec<Event> = ndjson
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("fixture line"))
        .collect();
    replay(&events).1
}

/// A race prediction as `view()` renders it, with the band it renders from.
#[derive(Debug)]
struct Race {
    view: RacePredictionView,
    agreed: bool,
    low_sec: f64,
    high_sec: f64,
}

impl std::ops::Deref for Race {
    type Target = RacePredictionView;

    fn deref(&self) -> &RacePredictionView {
        &self.view
    }
}

/// [`to_race_view`] and its [`race_band`].
fn race(q: &RaceQuery, longest_logged_km: Option<f64>, unit: DistanceUnit) -> Race {
    let band = race_band(q, longest_logged_km);
    Race {
        view: to_race_view(q, longest_logged_km, unit),
        agreed: band.agreed,
        low_sec: band.low_sec,
        high_sec: band.high_sec,
    }
}

/// `view()`'s race prediction, with the band behind it.
fn race_of(model: &Model) -> Race {
    let q = model.race_query.as_ref().expect("a race query");
    let band = race_band(q, longest_logged_run_km(&ViewCtx::new(model).runs));
    Race {
        view: Engine
            .view(model)
            .race_prediction
            .expect("a race prediction"),
        agreed: band.agreed,
        low_sec: band.low_sec,
        high_sec: band.high_sec,
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

/// `events` replayed on a fresh model.
pub(crate) fn replay(events: &[Event]) -> (Engine, Model) {
    let app = Engine;
    let mut model = Model::default();
    for e in events {
        let _ = app.update(e.clone(), &mut model);
    }
    (app, model)
}

/// [`replay`], failing on any event the core does not take (a render only).
fn replay_rendering(events: &[Event]) -> (Engine, Model) {
    let app = Engine;
    let mut model = Model::default();
    for e in events {
        app.update(e.clone(), &mut model).expect_only_render();
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
            let (app, model) = replay(&events);
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
pub(crate) fn assert_compaction_equivalent(name: &str, events: Vec<Event>) {
    let full = replay_cold(events.clone()).expect("full replay");
    let compacted = replay_cold(crate::log::compact_event_log(events)).expect("compacted replay");
    assert!(
        full == compacted,
        "{name}: the compacted log renders a different view: {}",
        json_diff(&full, &compacted)
    );
}

/// `SetToday` to `epoch_day` at UTC offset `utc_offset_sec`.
pub(crate) fn set_today_off_ev(epoch_day: i64, utc_offset_sec: i64) -> Event {
    Event::SetToday {
        epoch_day,
        utc_offset_sec,
    }
}

/// 2026-01-05, a Monday: the first simulated day of every `long_horizon` life.
const DAY0: i64 = 20_458;

/// A 5 m-accuracy fix `x_m` metres east of (0, 0) at unix second `t`.
fn fix(x_m: f64, t: i64) -> GpsPoint {
    gps(x_m / M_PER_DEG, t)
}

/// `ev` (a [`log_run`] or [`log_run_track`] event) stamped with the device's
/// UTC offset at logging time, as the shell logs it.
fn stamped(mut ev: Event, offset: i64) -> Event {
    let (Event::LogRun { meta, .. } | Event::LogRunTrack { meta, .. }) = &mut ev else {
        unreachable!("stamped takes a LogRun or LogRunTrack")
    };
    meta.utc_offset_sec = Some(offset);
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

/// The env knob `key` as a number (decimal, or hex as `0x…`), if set.
fn env_u64(key: &str) -> Option<u64> {
    let s = std::env::var(key).ok()?;
    let s = s.trim();
    match s.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => s.parse().ok(),
    }
}

/// The env knob `key` ([`env_u64`]), or `default` when unset: the case,
/// step and seed knobs of the seeded sweeps.
fn env_or(key: &str, default: u64) -> u64 {
    env_u64(key).unwrap_or(default)
}

/// A morning check-in at unix-seconds `at` answering sleep, soreness and
/// mood (1–5); every other item unanswered.
pub(crate) fn checkin(at: i64, sleep: u8, sore: u8, mood: u8) -> CheckinInput {
    CheckinInput {
        observed_at: at,
        sleep_quality: Some(sleep),
        soreness: Some(sore),
        mood: Some(mood),
        ..CheckinInput::default()
    }
}

/// [`checkin`] as the event the shell sends.
pub(crate) fn checkin_ev(at: i64, sleep: u8, sore: u8, mood: u8) -> Event {
    Event::SubmitCheckin(checkin(at, sleep, sore, mood))
}

/// KB Table 1 weekly sets per muscle (extracted/03-hypertrophy.md:520-530):
/// (muscle, MV, MEV, MAV low, MAV high, MRV "N+").
const KB_TABLE1: [(&str, u8, u8, u8, u8, u8); 11] = [
    ("chest", 8, 10, 12, 20, 22),
    ("back", 8, 10, 14, 22, 25),
    ("quads", 6, 8, 12, 18, 20),
    ("hamstrings", 4, 6, 10, 16, 20),
    ("glutes", 0, 0, 4, 12, 16),
    ("side delts", 6, 8, 16, 22, 26),
    ("rear delts", 0, 6, 12, 18, 22),
    ("biceps", 4, 6, 14, 20, 26),
    ("triceps", 4, 6, 10, 14, 18),
    ("calves", 4, 6, 8, 16, 20),
    ("abs", 0, 0, 10, 16, 20),
];

/// The shell's History delete of row `(kind, entry_id, observed_at)`
/// (`HistoryEntry.kt::deleteEventFor`, and the activity list's delete).
fn delete_of(kind: EntryKind, entry_id: u64, observed_at: i64) -> Event {
    Event::DeleteEntry {
        kind,
        entry_id,
        observed_at_fallback: observed_at,
    }
}

/// The event the core answers the shell's `RelogEvent` op with for row
/// `(kind, id, at)` ([`crate::app::relog_event`]), passed through the wire
/// JSON as the bridge passes it.
fn relogged(
    m: &Model,
    kind: EntryKind,
    id: u64,
    at: i64,
    amended_at: Option<i64>,
) -> Option<Event> {
    let event = crate::app::relog_event(m, kind, id, at, amended_at)?;
    let json = serde_json::to_string(&event).expect("an event serializes");
    Some(serde_json::from_str(&json).expect("the shell's copy decodes"))
}

/// Delete History row `(kind, id, at)` ([`delete_of`]), then Undo it with
/// its [`relogged`] event, read before the delete. `None` when the delete
/// removed exactly one row and the Undo renders the view byte for byte as
/// before; else what differed (the first differing JSON path).
fn undo_diff(m: &mut Model, kind: EntryKind, id: u64, at: i64) -> Option<String> {
    let rows = |vm: &ViewModel| match kind {
        EntryKind::Set => vm.lifts.len(),
        EntryKind::Run => vm.runs.len(),
        EntryKind::Activity => vm.activities.len(),
    };
    let Some(restore) = relogged(m, kind, id, at, None) else {
        return Some(format!("no Undo for {kind:?} {id}@{at}"));
    };
    let before = Engine.view(m);
    let _ = Engine.update(delete_of(kind, id, at), m);
    let removed = rows(&before) as i64 - rows(&Engine.view(m)) as i64;
    let _ = Engine.update(restore, m);
    if removed != 1 {
        return Some(format!(
            "deleting {kind:?} {id}@{at} removed {removed} rows"
        ));
    }
    let (before, after) = (vm_json(&before), vm_json(&Engine.view(m)));
    (before != after).then(|| {
        format!(
            "Undo of {kind:?} {id}@{at} changed the view: {}",
            json_diff(&before, &after)
        )
    })
}

/// The CR-10 ceiling a run target's effort cue states (`"effort 3–4 of 10…"`
/// → 4), `None` when it states none.
pub(crate) fn effort_ceiling(t: &RunTargetView) -> Option<u8> {
    let (head, _) = t.effort_cue.split_once(" of 10")?;
    head.rsplit(['–', ' ']).next()?.parse().ok()
}

/// The highest CR-10 ceiling of an easy-family run: recovery, easy or long
/// run (running-014…016).
pub(crate) fn easy_effort_ceiling() -> u8 {
    [
        RunSessionType::Recovery,
        RunSessionType::Easy,
        RunSessionType::LongRun,
    ]
    .into_iter()
    .filter_map(|k| running::workout_rx(k).value.rpe.map(|(_, hi)| hi))
    .max()
    .expect("the easy bands state an effort")
}
