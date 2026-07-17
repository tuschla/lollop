//! The side-effect-free core (crux `App`). First coaching slice: readiness
//! inputs accumulate in the model; `view()` runs the pure autoregulation layer
//! (`crate::autoreg`) to surface the highest safety tier plus every
//! evidence-cited adjustment. No IO, no clock, no randomness.

use crux_core::{
    App, Command,
    macros::effect,
    render::{RenderOperation, render},
};
use serde::{Deserialize, Serialize};

use crate::feedback::FeedbackCategory;
use crate::hybrid::ConcurrentGoal;
use crate::individualization::ProgressionCadence;
use crate::running::{GoalDistance, GpsPoint};
use crate::schema::{Adjustment, EvidenceGrade, MesoPhase, ReadinessInput, Recommended, VdotBand};
use crate::strength::LiftGoal;
use crate::{autoreg, feedback, hybrid, hypertrophy, individualization, load, running, strength};

#[derive(Clone)]
struct LoggedSet {
    exercise: String,
    weight_kg: f64,
    reps: u32,
    rpe: f64,
}

#[derive(Clone)]
struct LoggedRun {
    /// Manual distance; ignored when `track` is non-empty (derived instead).
    distance_km: f64,
    /// Manual duration; ignored when `track` is non-empty (derived instead).
    duration_min: f64,
    hr_pct_max: f64,
    longest_recent_km: f64,
    /// GPS fixes for a tracked run; empty for a hand-entered run.
    track: Vec<GpsPoint>,
}

/// The athlete's programming profile: drives the evidence-cited guidance
/// section. Shell-supplied; no logged data required.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Profile {
    /// Cadence at which load can still be added → training age.
    pub progression_cadence: ProgressionCadence,
    /// Target lifting quality for loading bands.
    pub lift_goal: LiftGoal,
    /// Target running goal distance.
    pub goal_distance: GoalDistance,
    /// Concurrent-training priority for same-session ordering.
    pub concurrent_goal: ConcurrentGoal,
    /// Planned weekly sets per muscle (frequency / cap logic).
    pub weekly_sets: u8,
    /// Running days per week (hybrid lower-lift cap trigger).
    pub running_days_per_week: u8,
    /// Running km per week (hybrid lower-lift cap trigger).
    pub running_km_per_week: f64,
    /// Advanced marathoner → extended session range.
    pub advanced: bool,
    /// Endurance session intensity as % VO2max (interference trigger).
    pub endurance_intensity_pct_vo2max: f64,
}

/// Lifting-set execution context for post-session feedback.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LiftExec {
    pub reps_met: bool,
    pub rir_actual: u8,
    pub rir_target: u8,
}

/// Aerobic-decoupling context for a run's feedback.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Decouple {
    pub drift_pct: f64,
    /// Only cool, steady, sub-threshold efforts get a decoupling verdict.
    pub cool_steady_context: bool,
}

/// One session's post-hoc review, safety signals plus optional execution
/// context. Feeds the safety-first feedback resolver.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct SessionReview {
    pub bone_pain_red_flag: bool,
    pub compulsive_flag: bool,
    pub overtraining_signal_count: u8,
    /// Single-session distance over the prior-30-day longest, as a fraction.
    pub single_session_spike_frac: Option<f64>,
    pub lift: Option<LiftExec>,
    pub decoupling: Option<Decouple>,
    /// Fraction of an easy run spent above VT1/Zone-2 ceiling.
    pub easy_frac_time_above_vt1: Option<f64>,
    /// Second-half slowdown percent on an even-effort run.
    pub positive_split_pct: Option<f64>,
    /// Count of sessions this week where the planned RPE was only met at loads
    /// ≥7% below plan, a fatigue-accumulation deload trigger (autoreg-023).
    pub rpe_load_gap_sessions: Option<u8>,
    /// Reference-load bar-velocity drop across the week, m/s (autoreg-026).
    pub weekly_velocity_drop_m_s: Option<f64>,
    /// Failed key (top-set / quality) sessions this week (autoreg-036).
    pub failed_key_sessions: Option<u8>,
    /// Target missed on a genuine high-RPE off day.
    pub bad_day: bool,
}

#[derive(Default)]
pub struct Model {
    /// Observed readiness signals, in submission order.
    inputs: Vec<ReadinessInput>,
    /// Logged lift sets, in submission order.
    sets: Vec<LoggedSet>,
    /// Logged runs, in submission order.
    runs: Vec<LoggedRun>,
    /// The athlete's programming profile, if set.
    profile: Option<Profile>,
    /// The current session review, if submitted.
    review: Option<SessionReview>,
    /// The last race-time prediction query, if requested. Stored as the raw
    /// inputs so the two-method estimate is re-derived in `view` (like a logged
    /// run), keeping the core deterministic and the prediction reproducible.
    race_query: Option<RaceQuery>,
    /// The last hypertrophy-mesocycle volume-plan query, if requested. Stored as
    /// the raw inputs so the graded per-week plan is re-derived in `view`.
    hypertrophy_plan_query: Option<HypertrophyPlanQuery>,
    /// The last absolute-protein-target query, if requested. Stored as the raw
    /// inputs so the graded g/day range is re-derived in `view` (bodyweight ×
    /// each graded g/kg bound). Self-contained so old logs never carry it.
    protein_query: Option<ProteinQuery>,
    /// The last heart-rate-zone query, if requested. Stored as the raw age so the
    /// graded HRmax + %HRmax band table is re-derived in `view`.
    hr_zone_query: Option<HrZoneQuery>,
}

/// Inputs for a hypertrophy accumulation-block volume plan: a target muscle and
/// the number of accumulation weeks. Retained so the view recomputes the graded
/// per-week plan (landmarks, set ramp, RIR schedule, frequency) deterministically.
#[derive(Debug, Clone, PartialEq)]
struct HypertrophyPlanQuery {
    muscle: String,
    weeks: u8,
}

/// Inputs for an absolute daily protein target: the athlete's bodyweight and
/// which graded goal contexts to surface. Retained so the view recomputes the
/// g/day range (bodyweight × each graded g/kg bound) deterministically.
#[derive(Debug, Clone, PartialEq)]
struct ProteinQuery {
    bodyweight_kg: f64,
    masters: bool,
    deficit: bool,
}

/// Inputs for a Daniels+Riegel race-time prediction (running-039), retained so
/// the view can recompute the graded estimate. Distances in metres, time in
/// seconds, `weekly_km` selects the Riegel fatigue exponent.
#[derive(Debug, Clone, PartialEq)]
struct RaceQuery {
    recent_distance_m: f64,
    recent_time_sec: f64,
    goal_distance_m: f64,
    weekly_km: f64,
}

/// Inputs for a heart-rate-zone table: the athlete's age in years. Retained so
/// the view recomputes the graded HRmax estimate (Tanaka) and the five Daniels
/// %HRmax band ranges deterministically.
#[derive(Debug, Clone, PartialEq)]
struct HrZoneQuery {
    age_years: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Event {
    /// Record one readiness observation, then recompute adjustments.
    SubmitReadiness(ReadinessInput),
    /// Drop all accumulated inputs (new day / new session).
    ClearReadiness,
    /// Log one completed lift set (weight in kg, reps, session RPE).
    LogSet {
        exercise: String,
        weight_kg: f64,
        reps: u32,
        rpe: f64,
    },
    /// Drop all logged sets.
    ClearSets,
    /// Log one run (distance km, duration min, average % HRmax, and the
    /// longest run in the last 30 days for spike detection).
    LogRun {
        distance_km: f64,
        duration_min: f64,
        hr_pct_max: f64,
        longest_recent_km: f64,
    },
    /// Log one GPS-tracked run. Distance and duration are derived in-core from
    /// the fix track (haversine + time span); `hr_pct_max` comes from a paired
    /// HR sensor (0.0 when none), `longest_recent_km` drives the spike gate.
    LogRunTrack {
        points: Vec<GpsPoint>,
        hr_pct_max: f64,
        longest_recent_km: f64,
    },
    /// Drop all logged runs.
    ClearRuns,
    /// Set the athlete's programming profile, then recompute guidance.
    SetProfile(Profile),
    /// Drop the profile (clears the guidance section).
    ClearProfile,
    /// Submit a post-session review, then resolve one feedback message.
    SubmitReview(SessionReview),
    /// Drop the session review (clears the feedback section).
    ClearReview,
    /// Predict a goal-race finish time from a recent race (Daniels VDOT +
    /// Riegel, combined per running-039). Distances in metres, time in seconds;
    /// `weekly_km` selects the Riegel fatigue exponent.
    PredictRace {
        recent_distance_m: f64,
        recent_time_sec: f64,
        goal_distance_m: f64,
        weekly_km: f64,
    },
    /// Drop the race prediction (clears the prediction section).
    ClearRacePrediction,
    /// Plan a hypertrophy accumulation block for one muscle over `weeks`
    /// accumulation weeks, producing a graded per-week volume plan.
    PlanHypertrophyMeso { muscle: String, weeks: u8 },
    /// Drop the hypertrophy volume plan (clears the planner section).
    ClearHypertrophyPlan,
    /// Compute an absolute daily protein target (g/day) by scaling the graded
    /// g/kg ranges by `bodyweight_kg`. `masters`/`deficit` select which graded
    /// goal contexts to surface; neither set yields no rows (no general number
    /// is invented, HARD RULE 1).
    ComputeProtein {
        bodyweight_kg: f64,
        masters: bool,
        deficit: bool,
    },
    /// Drop the protein target (clears the protein section).
    ClearProtein,
    /// Compute a heart-rate-zone table from age: an estimated HRmax (Tanaka) and
    /// the five Daniels %HRmax training bands mapped to absolute bpm ranges.
    ComputeHrZones { age_years: f64 },
    /// Drop the heart-rate-zone table (clears the zone section).
    ClearHrZones,
}

/// One adjustment flattened for shells: human summary + its evidence tag.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct AdjustmentView {
    pub summary: String,
    /// Evidence grade, e.g. `"Strong"`.
    pub grade: String,
    /// Backing reference (author/year or DOI).
    pub citation: String,
    /// 0.05–0.90 confidence score.
    pub confidence: f32,
    pub safety_critical: bool,
    pub contested: bool,
}

/// One logged set with its derived strength metrics, flattened for shells.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct LiftResultView {
    pub exercise: String,
    /// The set as logged, the raw input a shell shows alongside the derived
    /// metrics, so a lifter sees what they did (100 kg × 5 @ RPE 8), not only the
    /// estimated 1RM.
    pub weight_kg: f64,
    pub reps: u32,
    pub rpe: f64,
    /// Estimated 1RM (Epley), kg, rounded to 0.1.
    pub e1rm_kg: f64,
    /// The set's load as an estimated %1RM (Epley inverse of the rep count),
    /// rounded to a whole percent. Descriptive intensity, not a prescription.
    pub pct_1rm: f64,
    /// Reps in reserve implied by the session RPE.
    pub rir: f64,
    pub summary: String,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct ViewModel {
    /// Highest safety tier triggered, e.g. `"Pain"`; `None` when all clear.
    pub safety_tier: Option<String>,
    /// True when a Stop-level or rest-day condition fires, do not train.
    pub train_blocked: bool,
    pub adjustments: Vec<AdjustmentView>,
    /// Week-level deload triggers carried by the current review. Kept separate
    /// from `adjustments` (which are readiness-driven and cleared by
    /// `ClearReadiness`) because these share the review's lifecycle, they clear
    /// with `ClearReview`, so a shell must render them under the review, not the
    /// readiness section.
    pub review_adjustments: Vec<AdjustmentView>,
    pub input_count: usize,
    pub lifts: Vec<LiftResultView>,
    pub runs: Vec<RunResultView>,
    /// Evidence-cited programming guidance derived from the profile.
    pub guidance: Vec<GuidanceView>,
    /// The single resolved post-session feedback message, if a review is set.
    pub feedback: Option<FeedbackView>,
    /// Always-available evidence-cited reference defaults (profile-independent).
    pub reference: Vec<GuidanceView>,
    /// The currently-set profile, echoed back so a shell can hydrate its editor
    /// after a log replay instead of falling back to a hardcoded default.
    pub profile: Option<Profile>,
    /// The last requested race-time prediction, if any.
    pub race_prediction: Option<RacePredictionView>,
    /// The last requested hypertrophy volume plan, as graded rows. Empty when no
    /// plan has been requested.
    pub hypertrophy_plan: Vec<GuidanceView>,
    /// The last requested absolute protein target(s), as graded rows. Empty when
    /// no query has been made (or neither goal context was selected).
    pub protein_targets: Vec<GuidanceView>,
    /// The last requested heart-rate-zone table, as graded rows (HRmax + five
    /// %HRmax band bpm ranges). Empty when no query has been made.
    pub hr_zones: Vec<GuidanceView>,
}

/// A goal-race finish prediction, flattened for shells. Combines a Daniels VDOT
/// projection with a Riegel one (running-039): when the two agree within ~2% a
/// single `predicted` time is shown; otherwise a low–high range, so a single
/// method's false precision is never presented alone.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct RacePredictionView {
    /// Human label for the goal distance, e.g. `"10K"` or `"21.1 km"`.
    pub goal_label: String,
    /// Formatted finish time (`"41:30"`) or range (`"41:30–43:00"`).
    pub predicted: String,
    /// True when the two methods agreed (single time); false = range.
    pub agreed: bool,
    /// Range bounds in seconds (equal when `agreed`); 0.0 on degenerate input.
    pub low_sec: f64,
    pub high_sec: f64,
    pub summary: String,
    pub grade: String,
    pub citation: String,
    pub confidence: f32,
    pub safety_critical: bool,
    pub contested: bool,
}

/// The one resolved coaching message for a session, flattened for shells.
/// Safety concerns short-circuit and suppress competing praise (HARD RULE 3).
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct FeedbackView {
    /// Category name, e.g. `"ConcernInjury"`.
    pub category: String,
    /// Human-readable coaching copy for the category.
    pub message: String,
    /// True when this message suppresses all competing praise this cycle.
    pub suppresses_praise: bool,
    pub grade: String,
    pub citation: String,
    pub confidence: f32,
    pub safety_critical: bool,
    pub contested: bool,
}

/// One evidence-cited programming recommendation, flattened for shells.
/// `MarketingMyth`-graded claims are never emitted here.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct GuidanceView {
    /// Grouping label, e.g. `"Strength"`, `"Running"`, `"Hybrid"`.
    pub section: String,
    pub summary: String,
    pub grade: String,
    pub citation: String,
    pub confidence: f32,
    pub safety_critical: bool,
    pub contested: bool,
}

/// One logged run with derived zone / pace / spike flag, flattened for shells.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct RunResultView {
    /// 3-zone lactate classification from % HRmax, e.g. `"Z2"`.
    pub zone: String,
    /// Pace as `m:ss/km`.
    pub pace: String,
    /// True when this run's distance spikes >10% over the recent longest.
    pub spike_flag: bool,
    /// Second-half pace slowdown percent for a GPS-tracked run (a positive split;
    /// positive = slowed in the back half). `None` for a hand-entered run or a
    /// track too short/degenerate to split. Descriptive, not a prescription.
    pub split_pct: Option<f64>,
    pub summary: String,
    /// Evidence backing the spike gate.
    pub citation: String,
    /// GPX 1.1 document for a GPS-tracked run, ready for the shell to write and
    /// share; empty string for a hand-entered run with no fix track.
    pub gpx: String,
}

#[effect(typegen)]
#[derive(Debug)]
pub enum Effect {
    Render(RenderOperation),
}

#[derive(Default)]
pub struct Engine;

impl App for Engine {
    type Event = Event;
    type Model = Model;
    type ViewModel = ViewModel;
    type Effect = Effect;

    fn update(&self, event: Self::Event, model: &mut Self::Model) -> Command<Effect, Event> {
        match event {
            Event::SubmitReadiness(input) => model.inputs.push(input),
            Event::ClearReadiness => model.inputs.clear(),
            Event::LogSet {
                exercise,
                weight_kg,
                reps,
                rpe,
            } => model.sets.push(LoggedSet {
                exercise,
                weight_kg,
                reps,
                rpe,
            }),
            Event::ClearSets => model.sets.clear(),
            Event::LogRun {
                distance_km,
                duration_min,
                hr_pct_max,
                longest_recent_km,
            } => {
                // Floor the spike baseline to the longest run we already hold, so a
                // manual entry gets the same gate a GPS-tracked one does: an
                // explicit caller value (paired 30-day history) still wins when
                // larger.
                let prior_longest = model
                    .runs
                    .iter()
                    .map(run_distance_km)
                    .fold(0.0_f64, f64::max);
                model.runs.push(LoggedRun {
                    distance_km,
                    duration_min,
                    hr_pct_max,
                    longest_recent_km: longest_recent_km.max(prior_longest),
                    track: Vec::new(),
                });
            }
            Event::LogRunTrack {
                points,
                hr_pct_max,
                longest_recent_km,
            } => {
                // Spike baseline = longest prior run we already hold, so the gate
                // works without the shell fabricating a recent-longest figure. An
                // explicit caller value (paired history from a tracker) still wins
                // when larger.
                let prior_longest = model
                    .runs
                    .iter()
                    .map(run_distance_km)
                    .fold(0.0_f64, f64::max);
                model.runs.push(LoggedRun {
                    distance_km: 0.0,
                    duration_min: 0.0,
                    hr_pct_max,
                    longest_recent_km: longest_recent_km.max(prior_longest),
                    track: points,
                });
            }
            Event::ClearRuns => model.runs.clear(),
            Event::SetProfile(profile) => model.profile = Some(profile),
            Event::ClearProfile => model.profile = None,
            Event::SubmitReview(review) => model.review = Some(review),
            Event::ClearReview => model.review = None,
            Event::PredictRace {
                recent_distance_m,
                recent_time_sec,
                goal_distance_m,
                weekly_km,
            } => {
                model.race_query = Some(RaceQuery {
                    recent_distance_m,
                    recent_time_sec,
                    goal_distance_m,
                    weekly_km,
                });
            }
            Event::ClearRacePrediction => model.race_query = None,
            Event::PlanHypertrophyMeso { muscle, weeks } => {
                model.hypertrophy_plan_query = Some(HypertrophyPlanQuery { muscle, weeks });
            }
            Event::ClearHypertrophyPlan => model.hypertrophy_plan_query = None,
            Event::ComputeProtein {
                bodyweight_kg,
                masters,
                deficit,
            } => {
                model.protein_query = Some(ProteinQuery {
                    bodyweight_kg,
                    masters,
                    deficit,
                });
            }
            Event::ClearProtein => model.protein_query = None,
            Event::ComputeHrZones { age_years } => {
                model.hr_zone_query = Some(HrZoneQuery { age_years });
            }
            Event::ClearHrZones => model.hr_zone_query = None,
        }
        render()
    }

    fn view(&self, model: &Self::Model) -> Self::ViewModel {
        let recommended = autoreg::adjustments(&model.inputs);

        let train_blocked = recommended.iter().any(|r| {
            matches!(
                r.value,
                Adjustment::Stop | Adjustment::RestDay | Adjustment::Defer { .. }
            )
        });
        let review_adjustments = model
            .review
            .as_ref()
            .map(|r| review_deloads(r).iter().map(to_view).collect())
            .unwrap_or_default();

        ViewModel {
            safety_tier: autoreg::resolve_safety(&model.inputs).map(|t| format!("{t:?}")),
            train_blocked,
            adjustments: recommended.iter().map(to_view).collect(),
            review_adjustments,
            input_count: model.inputs.len(),
            lifts: model.sets.iter().map(to_lift_view).collect(),
            runs: model.runs.iter().map(to_run_view).collect(),
            guidance: model
                .profile
                .as_ref()
                .map(build_guidance)
                .unwrap_or_default(),
            feedback: model.review.as_ref().map(|r| {
                build_feedback(r, latest_track_split(model), latest_run_spike_frac(model))
            }),
            reference: build_reference(),
            profile: model.profile.clone(),
            race_prediction: model.race_query.as_ref().map(to_race_view),
            hypertrophy_plan: model
                .hypertrophy_plan_query
                .as_ref()
                .map(build_hypertrophy_plan)
                .unwrap_or_default(),
            protein_targets: model
                .protein_query
                .as_ref()
                .map(build_protein_targets)
                .unwrap_or_default(),
            hr_zones: model
                .hr_zone_query
                .as_ref()
                .map(build_hr_zones)
                .unwrap_or_default(),
        }
    }
}

/// Profile-independent evidence-cited reference defaults, surfaced always so a
/// shell can show coaching rationale without a full profile set.
fn build_reference() -> Vec<GuidanceView> {
    let mut rows = Vec::new();

    let sp = hybrid::session_spacing();
    push_guidance(
        &mut rows,
        "Hybrid",
        format!(
            "Same-day spacing: {}-{} h ideal, ≥{} h fallback",
            sp.value.ideal_min_hours, sp.value.ideal_max_hours, sp.value.fallback_min_hours
        ),
        &sp,
    );

    let ml = hybrid::maintenance_lift_sessions();
    push_guidance(
        &mut rows,
        "Hybrid",
        format!(
            "Keep {}-{} lift sessions/wk in a running block",
            ml.value.0, ml.value.1
        ),
        &ml,
    );

    let efc = hybrid::endurance_frequency_cap();
    push_guidance(
        &mut rows,
        "Hybrid",
        format!(
            "Endurance frequency cap: ≤{} d/wk when strength co-primary",
            efc.value
        ),
        &efc,
    );

    let md = hybrid::maintenance_dose_fraction();
    push_guidance(
        &mut rows,
        "Hybrid",
        format!(
            "Maintaining a quality (not improving): ~{:.0}% of the improvement volume (~2 low-volume sessions/wk)",
            md.value * 100.0
        ),
        &md,
    );

    let tr = strength::taper_rx();
    push_guidance(
        &mut rows,
        "Strength",
        format!(
            "Peak taper: cut volume {:.0}-{:.0}% over {}-{} days, hold intensity",
            tr.value.volume_reduction_frac.0 * 100.0,
            tr.value.volume_reduction_frac.1 * 100.0,
            tr.value.duration_days.0,
            tr.value.duration_days.1
        ),
        &tr,
    );

    let dl = strength::deadlift_peak_days_out();
    push_guidance(
        &mut rows,
        "Strength",
        format!(
            "Last near-max deadlift {}-{} days out from a meet",
            dl.value.0, dl.value.1
        ),
        &dl,
    );

    let pap = strength::pap_rest_window_min();
    push_guidance(
        &mut rows,
        "Strength",
        format!(
            "PAP/PAPE contrast rest window: {}-{} min",
            pap.value.0, pap.value.1
        ),
        &pap,
    );

    // Grouped with the Strength rows above (not the Hypertrophy block below) so
    // the section runs stay contiguous, reference cards are labelled per-section,
    // and a stray out-of-order Strength row reads as an orphan.
    let mf = individualization::maintenance_frequency_per_week();
    push_guidance(
        &mut rows,
        "Strength",
        format!(
            "Maintenance: train each lift {}×/wk to hold strength",
            mf.value
        ),
        &mf,
    );

    let od = strength::olympic_derivative_rx();
    push_guidance(
        &mut rows,
        "Strength",
        format!(
            "Olympic pulling derivatives: {}-{} sets × {}-{} reps at {}-{}% 1RM, {}-{} min rest",
            od.value.sets.0,
            od.value.sets.1,
            od.value.reps.0,
            od.value.reps.1,
            od.value.pct_1rm.0,
            od.value.pct_1rm.1,
            od.value.rest_sec.0 / 60,
            od.value.rest_sec.1 / 60
        ),
        &od,
    );

    let bo = strength::rpe_anchored_back_off();
    push_guidance(
        &mut rows,
        "Strength",
        format!(
            "RPE-anchored: top set at RPE {}, back-offs {:.0}-{:.0}% lighter",
            bo.value.top_set_rpe,
            bo.value.drop_frac.0 * 100.0,
            bo.value.drop_frac.1 * 100.0
        ),
        &bo,
    );

    // Call with the rule's own trigger thresholds purely to inherit the graded
    // citation; the prose states the rule itself (educational, not a live verdict).
    let two = strength::two_for_two_met(2, 2);
    push_guidance(
        &mut rows,
        "Strength",
        "Two-for-two rule: add load once you beat the top of the rep target by ≥2 reps on the last set for 2 straight sessions".to_string(),
        &two,
    );

    let stall = strength::stall_triggers_deload(2, true);
    push_guidance(
        &mut rows,
        "Strength",
        "Stall: if e1RM stays flat ≥2 weeks despite adequate recovery, deload or switch periodization model".to_string(),
        &stall,
    );

    let dbl = individualization::double_progression_add_load(true);
    push_guidance(
        &mut rows,
        "Strength",
        "Double progression: once you reach the top of the rep range on every set, add load and drop back to the range bottom".to_string(),
        &dbl,
    );

    let hd = hypertrophy::deload_rx();
    push_guidance(
        &mut rows,
        "Hypertrophy",
        format!(
            "Deload week: ~{:.0}% of sets, {}-{} RIR, load {:.0}-{:.0}% of working",
            hd.value.sets_fraction * 100.0,
            hd.value.rir.0,
            hd.value.rir.1,
            hd.value.load_frac_of_working.0 * 100.0,
            hd.value.load_frac_of_working.1 * 100.0
        ),
        &hd,
    );

    let mex = individualization::min_muscle_exposures_per_week();
    push_guidance(
        &mut rows,
        "Hypertrophy",
        format!("Minimum {} muscle exposures/wk", mex.value),
        &mex,
    );

    let jp = hypertrophy::joint_pain_rep_shift();
    push_guidance(
        &mut rows,
        "Hypertrophy",
        format!(
            "Joint pain at heavy load: shift that muscle to {}-{} reps at {}-{}% 1RM (growth preserved via load interchangeability)",
            jp.value.reps.0, jp.value.reps.1, jp.value.pct_1rm.0, jp.value.pct_1rm.1
        ),
        &jp,
    );

    let mp = individualization::masters_protein_target();
    push_guidance(
        &mut rows,
        "Nutrition",
        format!(
            "Masters (65+) protein: {:.1}-{:.1} g/kg/day for anabolic resistance",
            mp.value.g_per_kg.0, mp.value.g_per_kg.1
        ),
        &mp,
    );

    let dp = individualization::deficit_protein_target();
    push_guidance(
        &mut rows,
        "Nutrition",
        format!(
            "Deficit protein (lean-mass preserving): {:.1}-{:.1} g/kg/day",
            dp.value.g_per_kg.0, dp.value.g_per_kg.1
        ),
        &dp,
    );

    let mpm = individualization::masters_protein_per_meal();
    push_guidance(
        &mut rows,
        "Nutrition",
        format!(
            "Masters (65+) per-meal protein: ~{:.1} g/kg to overcome anabolic resistance",
            mpm.value
        ),
        &mpm,
    );

    let c25k = running::c25k_plan();
    push_guidance(
        &mut rows,
        "Running",
        format!(
            "Couch-to-5K: {} run/walk sessions/wk over {}-{} weeks, rest day between; repeat a hard week without penalty",
            c25k.value.runs_per_week, c25k.value.weeks.0, c25k.value.weeks.1
        ),
        &c25k,
    );

    let pyr = running::intensity_distribution(running::DistributionModel::Pyramidal);
    push_guidance(
        &mut rows,
        "Running",
        format!(
            "Pyramidal split (base / newer runners): {}% easy / {}% moderate / {}% hard",
            pyr.value.easy_pct, pyr.value.moderate_pct, pyr.value.hard_pct
        ),
        &pyr,
    );

    let pol = running::intensity_distribution(running::DistributionModel::Polarized);
    push_guidance(
        &mut rows,
        "Running",
        format!(
            "Polarized split (trained / peak phase): {}% easy / {}% moderate / {}% hard",
            pol.value.easy_pct, pol.value.moderate_pct, pol.value.hard_pct
        ),
        &pol,
    );

    let join_levers = |levers: &[individualization::ScaleLever]| -> String {
        levers
            .iter()
            .map(|l| match l {
                individualization::ScaleLever::AccessoryVolume => "accessory volume",
                individualization::ScaleLever::SetsTowardMev => "sets → MEV",
                individualization::ScaleLever::Frequency => "frequency",
                individualization::ScaleLever::SecondaryQuality => "secondary quality",
                individualization::ScaleLever::IntensityAndMainCompounds => "intensity/main lifts",
            })
            .collect::<Vec<_>>()
            .join(" → ")
    };

    let sd = individualization::scale_down_order();
    push_guidance(
        &mut rows,
        "Scaling",
        format!(
            "Scale DOWN (cut first → protect last): {}",
            join_levers(&sd.value)
        ),
        &sd,
    );

    let su = individualization::scale_up_order();
    push_guidance(
        &mut rows,
        "Scaling",
        format!(
            "Scale UP (add first → add last): {}",
            join_levers(&su.value)
        ),
        &su,
    );

    let sub = individualization::substitution_rule();
    let mut sub_parts = Vec::new();
    if sub.value.match_movement_pattern {
        sub_parts.push("match the movement pattern");
    }
    if sub.value.compensate_with_reps_near_failure {
        sub_parts.push("add reps near failure to offset the lighter load");
    }
    push_guidance(
        &mut rows,
        "Substitutions",
        format!("Home / minimal equipment: {}", sub_parts.join(", then ")),
        &sub,
    );

    let framing = feedback::default_goal_framing();
    let framing_text = match framing.value {
        feedback::GoalFraming::Process => {
            "Goals are framed as controllable process targets (cadence, pacing discipline, RIR) - you steer the process, the outcome follows"
        }
        feedback::GoalFraming::Outcome => {
            "Outcome/result framing (only where an individual goal-efficacy signal supports it)"
        }
    };
    push_guidance(&mut rows, "Coaching", framing_text.to_string(), &framing);

    rows
}

/// Pick the single execution category for a review, in a fixed priority order
/// (lifting → decoupling → easy-intensity → pacing → off-day). `None` when no
/// execution arm fires: the resolver then emits a neutral competence note.
fn session_execution(r: &SessionReview) -> Option<Recommended<FeedbackCategory>> {
    if let Some(l) = &r.lift {
        return Some(feedback::lifting_feedback(
            l.reps_met,
            l.rir_actual,
            l.rir_target,
        ));
    }
    if let Some(d) = &r.decoupling
        && let Some(f) = feedback::decoupling_feedback(d.drift_pct, d.cool_steady_context)
    {
        return Some(f);
    }
    if let Some(frac) = r.easy_frac_time_above_vt1
        && let Some(f) = feedback::easy_run_intensity_discipline(frac)
    {
        return Some(f);
    }
    if let Some(ps) = r.positive_split_pct
        && let Some(f) = feedback::positive_split_discipline(ps)
    {
        return Some(f);
    }
    if r.bad_day {
        return Some(feedback::bad_day_feedback());
    }
    None
}

/// Week-level deload triggers carried by a review. Distinct from the readiness
/// adjustments (which react to a single session's markers), these fire off
/// accumulated weekly fatigue counts the shell tallies. Each dormant autoreg
/// fn returns `None` until its threshold is crossed, so an unset field or a
/// sub-threshold count contributes nothing.
fn review_deloads(r: &SessionReview) -> Vec<Recommended<Adjustment>> {
    let mut out = Vec::new();
    if let Some(n) = r.rpe_load_gap_sessions
        && let Some(d) = autoreg::deload_from_rpe_load_gap(n)
    {
        out.push(d);
    }
    if let Some(v) = r.weekly_velocity_drop_m_s
        && let Some(d) = autoreg::deload_from_velocity_drop(v)
    {
        out.push(d);
    }
    if let Some(n) = r.failed_key_sessions
        && let Some(d) = autoreg::deload_from_failed_sessions(n)
    {
        out.push(d);
    }
    out
}

/// Second-half slowdown of the most recent GPS-tracked run, if any. Lets the
/// positive-split coaching feedback fire from a logged run when the review does
/// not carry an explicit figure: the measurement is descriptive (like pace or
/// zone), so feeding it to the resolver introduces no unevidenced claim.
fn latest_track_split(model: &Model) -> Option<f64> {
    model
        .runs
        .iter()
        .rev()
        .find(|r| !r.track.is_empty())
        .and_then(|r| running::track_positive_split_pct(&r.track, running::MAX_GPS_ACCURACY_M))
}

/// Fraction by which the most recent logged run's distance exceeds the athlete's
/// recent-longest baseline (e.g. `0.15` = 15 % over). `None` when there is no
/// baseline yet, a first run has nothing to be a spike *over*, so the safety
/// gate must not defer on it (the run view already says why it looks unbounded).
/// Lets a logged over-distance run drive the safety gate even when the review
/// omits an explicit figure, mirroring the positive-split fallback.
fn latest_run_spike_frac(model: &Model) -> Option<f64> {
    let r = model.runs.last()?;
    if r.longest_recent_km <= 0.0 {
        return None;
    }
    Some(run_distance_km(r) / r.longest_recent_km - 1.0)
}

/// Resolve one session's feedback message (safety gate first), flattened.
///
/// `track_split` is a run-derived positive-split fallback and `spike_frac` a
/// run-derived distance-spike fallback, each used only when the review omits its
/// own figure, so a run-only day still gets pacing and safety feedback.
fn build_feedback(
    r: &SessionReview,
    track_split: Option<f64>,
    spike_frac: Option<f64>,
) -> FeedbackView {
    let safety = feedback::SafetySignals {
        bone_pain_red_flag: r.bone_pain_red_flag,
        compulsive_flag: r.compulsive_flag,
        overtraining_signal_count: r.overtraining_signal_count,
        single_session_spike_frac: r.single_session_spike_frac.or(spike_frac),
    };
    let effective = SessionReview {
        positive_split_pct: r.positive_split_pct.or(track_split),
        ..r.clone()
    };
    let resolved = feedback::resolve_feedback(safety, session_execution(&effective));
    FeedbackView {
        category: format!("{:?}", resolved.value),
        message: feedback_message(resolved.value).into(),
        suppresses_praise: resolved.value.suppresses_competing_praise(),
        grade: format!("{:?}", resolved.evidence.grade),
        citation: resolved.evidence.citation.reference.clone(),
        confidence: resolved.confidence.score,
        safety_critical: resolved.confidence.safety_critical,
        contested: resolved.confidence.contested,
    }
}

/// Coaching copy for a feedback category (File 05 voice: autonomy-supportive,
/// process-framed, never guilt-inducing).
fn feedback_message(cat: FeedbackCategory) -> &'static str {
    match cat {
        FeedbackCategory::ConcernInjury => {
            "Stop training this area and see a professional - this looks like a bone-stress red flag."
        }
        FeedbackCategory::ConcernRecovery => {
            "Several overtraining signals are stacking up. Back off and prioritize recovery this week."
        }
        FeedbackCategory::ConcernBehavior => {
            "This pattern looks compulsive. A rest day is not lost progress - consider stepping back."
        }
        FeedbackCategory::DangerousProgression => {
            "That was a large single-session jump. Rein in the progression to protect connective tissue."
        }
        FeedbackCategory::IntensityDiscipline => {
            "Easy days should stay easy - dial the effort back to build the aerobic base."
        }
        FeedbackCategory::PositiveExecution => "Well-paced, durable effort. Nicely controlled.",
        FeedbackCategory::InformationalNeutral => {
            "Mild aerobic fatigue noted - nothing to correct."
        }
        FeedbackCategory::CorrectiveProcess => {
            "Missed target is data, not failure. Here's an adjustment for next time - your call."
        }
        FeedbackCategory::PositiveMastery => {
            "Target hit at planned cost. You've earned the next planned progression."
        }
        FeedbackCategory::ContextualBadDay => {
            "Off day - normal variation. The stimulus still counts; no guilt."
        }
        FeedbackCategory::ProgressionNudge => {
            "That was well under target effort. Room to add load next session."
        }
    }
}

/// Push one evidence-cited row, dropping `MarketingMyth`-graded claims (HARD
/// RULE 2, myths are never programmed or surfaced).
fn push_guidance<T>(
    rows: &mut Vec<GuidanceView>,
    section: &str,
    summary: String,
    r: &Recommended<T>,
) {
    if r.evidence.grade == EvidenceGrade::MarketingMyth {
        return;
    }
    rows.push(GuidanceView {
        section: section.into(),
        summary,
        grade: format!("{:?}", r.evidence.grade),
        citation: r.evidence.citation.reference.clone(),
        confidence: r.confidence.score,
        safety_critical: r.confidence.safety_critical,
        contested: r.confidence.contested,
    });
}

/// Run the programming engine over the profile, producing evidence-cited rows.
fn build_guidance(p: &Profile) -> Vec<GuidanceView> {
    let mut rows = Vec::new();

    let age_r = individualization::training_age_from_cadence(p.progression_cadence);
    let age = age_r.value;
    push_guidance(
        &mut rows,
        "Profile",
        format!("Training age: {age:?}"),
        &age_r,
    );

    let sd = individualization::strength_defaults(age);
    push_guidance(
        &mut rows,
        "Strength",
        format!(
            "Defaults: {}%1RM, {}×/muscle/wk, {} sets/muscle",
            sd.value.intensity_pct_1rm, sd.value.freq_per_muscle, sd.value.sets_per_muscle
        ),
        &sd,
    );

    let lr = strength::loading_rx(p.lift_goal);
    push_guidance(
        &mut rows,
        "Strength",
        format!(
            "{:?} loading: {}-{}%1RM, {}-{} reps, {}-{} sets, RIR {}-{}",
            p.lift_goal,
            lr.value.pct_1rm.0,
            lr.value.pct_1rm.1,
            lr.value.reps.0,
            lr.value.reps.1,
            lr.value.sets.0,
            lr.value.sets.1,
            lr.value.rir.0,
            lr.value.rir.1
        ),
        &lr,
    );

    let vlt = strength::vl_termination_threshold(p.lift_goal);
    push_guidance(
        &mut rows,
        "Strength",
        format!(
            "Velocity-loss set cutoff for {:?}: end the set at ~{:.0}% bar-speed loss",
            p.lift_goal,
            vlt.value * 100.0
        ),
        &vlt,
    );

    let pm = strength::periodization_model(age);
    push_guidance(
        &mut rows,
        "Strength",
        format!("Periodization model: {:?}", pm.value),
        &pm,
    );

    // Surface the actual phase-by-phase prescription for the athlete's model, so
    // the periodization label above becomes an actionable mesocycle plan. Only
    // Linear and Block have phase tables in the source; DUP does not, so it is
    // left as the label only.
    let fmt_phase = |rx: &strength::PhaseRx| -> String {
        let mut parts = Vec::new();
        if let Some((lo, hi)) = rx.pct_1rm {
            parts.push(format!("{lo}-{hi}%1RM"));
        }
        if let (Some((slo, shi)), Some((rlo, rhi))) = (rx.sets, rx.reps) {
            parts.push(format!("{slo}-{shi}×{rlo}-{rhi}"));
        } else if let Some((rlo, rhi)) = rx.reps {
            parts.push(format!("{rlo}-{rhi} reps"));
        }
        if parts.is_empty() {
            parts.push("maintain intensity (taper template)".to_string());
        }
        format!("{} · wk {}-{}", parts.join(", "), rx.weeks.0, rx.weeks.1)
    };
    match pm.value {
        strength::PeriodizationModel::Linear => {
            use strength::LinearPhase::*;
            for (name, phase) in [
                ("Base", Base),
                ("Build", Build),
                ("Peak", Peak),
                ("Taper", Taper),
            ] {
                let ph = strength::linear_phase_rx(phase);
                push_guidance(
                    &mut rows,
                    "Strength",
                    format!("Linear {name}: {}", fmt_phase(&ph.value)),
                    &ph,
                );
            }
        }
        strength::PeriodizationModel::Block => {
            use strength::BlockPhase::*;
            for (name, phase) in [
                ("Accumulation", Accumulation),
                ("Transmutation", Transmutation),
                ("Realization", Realization),
            ] {
                let ph = strength::block_phase_rx(phase);
                push_guidance(
                    &mut rows,
                    "Strength",
                    format!("Block {name}: {}", fmt_phase(&ph.value)),
                    &ph,
                );
            }
        }
        strength::PeriodizationModel::Dup | strength::PeriodizationModel::Conjugate => {}
    }

    // Double-progression load jump, only actionable for a novice on linear
    // progression; intermediate/advanced lifters no longer add fixed load every
    // session, so showing it to them would misdescribe their programming.
    if matches!(age, individualization::TrainingAge::Novice) {
        let inc = individualization::novice_load_increment();
        push_guidance(
            &mut rows,
            "Strength",
            format!(
                "Novice load jump: +{:.1} kg upper / +{:.1} kg lower per session (double progression)",
                inc.value.upper_kg, inc.value.lower_kg
            ),
            &inc,
        );
    } else {
        // Past the novice phase the fixed per-session jump no longer applies;
        // progression moves to a per-week percentage of load. Surface both body
        // regions since the bands differ (upper is more conservative).
        let upper = strength::weekly_pct_increment(true);
        let lower = strength::weekly_pct_increment(false);
        // Render each bound faithfully: some bands carry a half-percent (e.g.
        // 2.5%), so a whole-number format would understate the cited value. Drop
        // the decimal only when the bound is whole.
        let pct = |frac: f64| -> String {
            let v = frac * 100.0;
            if v.fract().abs() < 1e-9 {
                format!("{v:.0}")
            } else {
                format!("{v:.1}")
            }
        };
        push_guidance(
            &mut rows,
            "Strength",
            format!(
                "Weekly load increment: +{}-{}% upper / +{}-{}% lower per successful week",
                pct(upper.value.0),
                pct(upper.value.1),
                pct(lower.value.0),
                pct(lower.value.1)
            ),
            &upper,
        );
    }

    // Prilepin volume ceiling at the midpoint of the goal's loading intensity -
    // the reps/set + optimal-total governor that keeps quality high per zone.
    // Kept adjacent to the other Strength rows (before the Power block below) so
    // the section stays contiguous: the shell suppresses a repeated section
    // header only for consecutive same-section rows.
    let pct_mid = f64::from(lr.value.pct_1rm.0 + lr.value.pct_1rm.1) / 2.0;
    if let Some(pr) = strength::prilepin_for(pct_mid) {
        let pr = strength::recommend(*pr, "STR-PRILEPIN-001");
        push_guidance(
            &mut rows,
            "Strength",
            format!(
                "Prilepin @~{:.0}%1RM: {}-{} reps/set, optimal {} total ({}-{} range)",
                pct_mid,
                pr.value.reps_per_set.0,
                pr.value.reps_per_set.1,
                pr.value.optimal_total,
                pr.value.total_range.0,
                pr.value.total_range.1
            ),
            &pr,
        );
    }

    // Plyometric foot-contact ceiling, only relevant when the goal is Power.
    // Placed after all Strength rows so the "Power" section forms its own
    // contiguous block rather than splitting Strength in two.
    if p.lift_goal == LiftGoal::Power {
        let plyo = strength::plyo_foot_contact_cap(age);
        push_guidance(
            &mut rows,
            "Power",
            format!(
                "Plyo foot-contact cap: {}-{}/session (progress volume OR intensity, not both)",
                plyo.value.0, plyo.value.1
            ),
            &plyo,
        );
    }

    let fr = hypertrophy::frequency_for_weekly_sets(p.weekly_sets);
    push_guidance(
        &mut rows,
        "Hypertrophy",
        format!(
            "{} weekly sets → {}-{}×/wk, {}-{} sets/session",
            p.weekly_sets,
            fr.value.freq.0,
            fr.value.freq.1,
            fr.value.per_session.0,
            fr.value.per_session.1
        ),
        &fr,
    );

    let split = hypertrophy::needs_session_split(p.weekly_sets);
    push_guidance(
        &mut rows,
        "Hypertrophy",
        format!(
            "{} weekly sets/muscle: {}",
            p.weekly_sets,
            if split.value {
                "split across ≥2 sessions/wk to keep per-session quality"
            } else {
                "fit in one weekly session"
            }
        ),
        &split,
    );

    // Rep/load prescription by exercise class. The frequency/volume rows above
    // answer "how many sets"; this answers "how many reps and what load", the
    // per-set target that was otherwise absent from the hypertrophy guidance.
    // All three classes share one evidence source, so surface them together.
    let heavy = hypertrophy::rep_load(hypertrophy::ExerciseClass::HeavyCompound);
    let moderate = hypertrophy::rep_load(hypertrophy::ExerciseClass::ModerateCompound);
    let iso = hypertrophy::rep_load(hypertrophy::ExerciseClass::Isolation);
    push_guidance(
        &mut rows,
        "Hypertrophy",
        format!(
            "Rep/load: heavy compound {}-{} @{}-{}%1RM, moderate {}-{} @{}-{}%, isolation {}-{} @{}-{}%",
            heavy.value.reps.0,
            heavy.value.reps.1,
            heavy.value.pct_1rm.0,
            heavy.value.pct_1rm.1,
            moderate.value.reps.0,
            moderate.value.reps.1,
            moderate.value.pct_1rm.0,
            moderate.value.pct_1rm.1,
            iso.value.reps.0,
            iso.value.reps.1,
            iso.value.pct_1rm.0,
            iso.value.pct_1rm.1
        ),
        &heavy,
    );

    let cap = hypertrophy::cap_weekly_growth_target(p.weekly_sets);
    push_guidance(
        &mut rows,
        "Hypertrophy",
        format!("Growth-target weekly sets capped at {}", cap.value),
        &cap,
    );

    let mev = hypertrophy::mev_sets_by_training_age(age);
    push_guidance(
        &mut rows,
        "Hypertrophy",
        format!(
            "MEV for {age:?}: {}-{} sets/muscle/wk",
            mev.value.0, mev.value.1
        ),
        &mev,
    );

    let hvs = individualization::high_volume_sensitivity(age);
    push_guidance(
        &mut rows,
        "Individualization",
        format!(
            "High-volume sensitivity: {}",
            if hvs.value {
                "yes - cap added volume"
            } else {
                "no"
            }
        ),
        &hvs,
    );

    let gp = running::goal_week_plan(p.goal_distance, p.advanced);
    push_guidance(
        &mut rows,
        "Running",
        format!(
            "{:?}: {}-{} sessions/wk, {}-{} quality, long run {:.0}-{:.0}% of volume",
            p.goal_distance,
            gp.value.sessions_per_week.0,
            gp.value.sessions_per_week.1,
            gp.value.quality_per_week.0,
            gp.value.quality_per_week.1,
            gp.value.long_run_share.0 * 100.0,
            gp.value.long_run_share.1 * 100.0
        ),
        &gp,
    );

    // Quality-session governance caps, the guardrail behind the "quality/wk"
    // count above. Only relevant to someone who actually runs, so a pure lifter
    // is not shown running caps.
    if p.running_days_per_week > 0 {
        let ql = running::quality_limits();
        push_guidance(
            &mut rows,
            "Running",
            format!(
                "Quality-session caps: ≤{}/wk, ≥{} h apart, no back-to-back Z3",
                ql.value.max_per_week, ql.value.min_spacing_hours
            ),
            &ql,
        );
    }

    // Default to the base-phase distribution (pyramidal), the sound starting
    // point before a mesocycle phase is chosen.
    let id = running::distribution_for_phase(MesoPhase::Base);
    push_guidance(
        &mut rows,
        "Running",
        format!(
            "Base-phase intensity: {}/{}/{} easy/moderate/hard %time",
            id.value.easy_pct, id.value.moderate_pct, id.value.hard_pct
        ),
        &id,
    );

    let ef = hybrid::endurance_frequency_ok(p.running_days_per_week);
    push_guidance(
        &mut rows,
        "Running",
        format!(
            "{} running days/wk within endurance-frequency guidance: {}",
            p.running_days_per_week, ef.value
        ),
        &ef,
    );

    let conservative = matches!(age, individualization::TrainingAge::Novice);
    let dc = running::deload_cadence(conservative);
    push_guidance(
        &mut rows,
        "Running",
        format!(
            "Deload cadence: {}:{} load:recovery, cut {:.0}-{:.0}%",
            dc.value.load_weeks,
            dc.value.recovery_weeks,
            dc.value.reduction_frac.0 * 100.0,
            dc.value.reduction_frac.1 * 100.0
        ),
        &dc,
    );

    // Weekly volume-increase ceiling. The cap only distinguishes sub-1-year
    // runners from everyone else, so map the training age onto a representative
    // year count rather than inventing a precise figure the profile lacks.
    let age_years = match age {
        individualization::TrainingAge::Novice => 0.5,
        _ => 2.0,
    };
    let wc = running::weekly_increase_cap_frac(age_years);
    push_guidance(
        &mut rows,
        "Running",
        format!(
            "Weekly volume increase cap: +{:.0}% max week-to-week",
            wc.value * 100.0
        ),
        &wc,
    );

    let so = hybrid::same_session_order(p.concurrent_goal);
    push_guidance(
        &mut rows,
        "Hybrid",
        format!("{:?} same-session order: {:?}", p.concurrent_goal, so.value),
        &so,
    );

    // Peak strength/power block running override (File 10 CAP-2): only relevant
    // when the lifting goal is a maximal quality, so a hypertrophy or endurance
    // athlete is not shown a cap that does not apply to their block.
    if matches!(p.lift_goal, LiftGoal::MaxStrength | LiftGoal::Power) {
        let pk = hybrid::peak_phase_run_cap();
        push_guidance(
            &mut rows,
            "Hybrid",
            format!(
                "Peak block: cap running to {}-{} easy runs/wk, no hard intervals, long runs ≥{} h from heavy-lower days",
                pk.value.max_easy_runs_per_week.0,
                pk.value.max_easy_runs_per_week.1,
                pk.value.long_run_min_gap_hours
            ),
            &pk,
        );
    }

    if let Some(llc) = hybrid::lower_lift_cap(p.running_days_per_week, p.running_km_per_week) {
        push_guidance(
            &mut rows,
            "Hybrid",
            format!(
                "Lower-lift cap: ≤{}/wk, cut lower-hyp volume {:.0}-{:.0}%",
                llc.value.max_lower_sessions,
                llc.value.volume_reduction_frac.0 * 100.0,
                llc.value.volume_reduction_frac.1 * 100.0
            ),
            &llc,
        );
    }

    let ie =
        hybrid::interference_expected(p.running_days_per_week, p.endurance_intensity_pct_vo2max);
    push_guidance(
        &mut rows,
        "Hybrid",
        format!("Interference expected: {}", ie.value),
        &ie,
    );

    // Whether *this* athlete's training age makes them susceptible to the small
    // trained-lower-body 1RM decrement (File 10 hybrid-009): only trained lifters
    // (>1 yr) show it. Reuses the representative `age_years` above and is only
    // relevant when the athlete actually runs, so a pure lifter is not shown it.
    if p.running_days_per_week > 0 {
        let li = hybrid::expect_lower_strength_interference(age_years);
        push_guidance(
            &mut rows,
            "Hybrid",
            format!(
                "Lower-body strength interference susceptibility: {}",
                if li.value {
                    "yes - trained lifter, expect a small lower-body 1RM decrement"
                } else {
                    "no - novice/untrained lower body is spared"
                }
            ),
            &li,
        );
    }

    // Safety-critical: high weekly mileage raises bone-stress-injury surveillance
    // (File 10 hybrid-023). Only surfaced once the profile's mileage crosses the
    // threshold, so a low-volume runner is not warned needlessly.
    let bsi = hybrid::bsi_surveillance_flag(p.running_km_per_week);
    if bsi.value {
        push_guidance(
            &mut rows,
            "Safety",
            format!(
                "Bone-stress-injury surveillance: {:.0} km/wk exceeds ~64 km - monitor for focal bone pain; keep energy availability adequate",
                p.running_km_per_week
            ),
            &bsi,
        );
    }

    rows
}

/// One run's realised distance in km: derived from the GPS track when present,
/// otherwise the hand-entered scalar.
fn run_distance_km(r: &LoggedRun) -> f64 {
    if r.track.is_empty() {
        r.distance_km
    } else {
        running::track_distance_km(&r.track, running::MAX_GPS_ACCURACY_M)
    }
}

/// Derive zone + pace + distance-spike flag for one logged run.
fn to_run_view(r: &LoggedRun) -> RunResultView {
    // A GPS track derives its own distance/duration; a manual run uses scalars.
    let gps = !r.track.is_empty();

    // A GPS run whose fixes all fail the accuracy gate has no measurable route:
    // distance/duration collapse to 0, which would otherwise render as a
    // "0.0km @ -" entry *and* trip the spike gate against a phantom baseline.
    // Report the signal problem honestly instead of fabricating a null run.
    if gps && running::usable_track(&r.track, running::MAX_GPS_ACCURACY_M).len() < 2 {
        return RunResultView {
            zone: "-".to_string(),
            pace: "-".to_string(),
            spike_flag: false,
            split_pct: None,
            summary: "GPS signal too poor to measure this run".to_string(),
            citation: String::new(),
            gpx: String::new(),
        };
    }

    let distance_km = run_distance_km(r);
    let duration_min = if gps {
        running::track_duration_min(&r.track, running::MAX_GPS_ACCURACY_M)
    } else {
        r.duration_min
    };

    // Zone is a heart-rate classification (File 04). With no HR sample we do not
    // fabricate one, report it as unknown rather than defaulting to Z1.
    let has_hr = r.hr_pct_max > 0.0;
    let zone = running::classify_three_zone(r.hr_pct_max);
    let zone_str = if has_hr {
        format!("{zone:?}")
    } else {
        "-".to_string()
    };
    let spike = running::single_session_spike_flag(distance_km, r.longest_recent_km);

    let pace = if distance_km > 0.0 && duration_min > 0.0 {
        let sec_per_km = (duration_min * 60.0) / distance_km;
        format!(
            "{}:{:02}/km",
            (sec_per_km as u32) / 60,
            (sec_per_km as u32) % 60
        )
    } else {
        "-".to_string()
    };

    let split_pct = if gps {
        running::track_positive_split_pct(&r.track, running::MAX_GPS_ACCURACY_M)
    } else {
        None
    };

    // A positive split is a back-half slowdown; a negative value is a negative
    // split (faster finish). Thresholds are strict (>3 %, <-3 %) to match
    // `feedback::positive_split_discipline`, so this note and the coaching cue it
    // pairs with never disagree at exactly 3 %.
    let split_note = match split_pct {
        Some(p) if p > feedback::POSITIVE_SPLIT_FLAG_PCT => {
            format!(" · +{p:.0}% back-half slowdown")
        }
        Some(p) if p < -feedback::POSITIVE_SPLIT_FLAG_PCT => {
            format!(" · {:.0}% negative split", p.abs())
        }
        _ => String::new(),
    };

    RunResultView {
        zone: zone_str.clone(),
        pace: pace.clone(),
        spike_flag: spike.value,
        split_pct,
        summary: format!(
            "{}{:.1}km @ {} ({}){}{}",
            if gps { "GPS " } else { "" },
            distance_km,
            pace,
            zone_str,
            // The spike gate errs safe with no history (see single_session_spike),
            // so a user's first run trips it. Say *why* rather than claiming a
            // ">10%" jump over a baseline that does not exist yet: the SPIKE flag
            // itself is unchanged, only the wording is honest about the cause.
            if spike.value {
                if r.longest_recent_km > 0.0 {
                    " - distance spike >10% over recent longest"
                } else {
                    " - flagged: no prior run to gauge distance yet"
                }
            } else {
                ""
            },
            split_note,
        ),
        citation: spike.evidence.citation.reference.clone(),
        gpx: {
            // Export the same accuracy-gated fixes used for distance/duration so
            // the file's distance matches what the app shows. A track whose fixes
            // all fail the accuracy gate leaves fewer than two usable points: no
            // real route, so emit no GPX rather than a degenerate file the shell
            // would still offer an "Export" button for.
            let usable = running::usable_track(&r.track, running::MAX_GPS_ACCURACY_M);
            if usable.len() >= 2 {
                running::export_gpx(&usable, &format!("Run {distance_km:.1}km"))
            } else {
                String::new()
            }
        },
    }
}

/// Format a duration in seconds as a race clock: `h:mm:ss` past an hour, else
/// `m:ss`. Non-positive input renders as `-` (no false prediction).
fn fmt_race_clock(sec: f64) -> String {
    if sec <= 0.0 {
        return "-".to_string();
    }
    let s = sec.round() as u64;
    let (h, m, s) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Human label for a goal race distance: the classic names at their standard
/// metrages (within 10 m), otherwise a plain kilometre figure.
fn race_distance_label(m: f64) -> String {
    match m {
        d if (d - 5_000.0).abs() < 10.0 => "5K".to_string(),
        d if (d - 10_000.0).abs() < 10.0 => "10K".to_string(),
        d if (d - 21_097.5).abs() < 10.0 => "Half marathon".to_string(),
        d if (d - 42_195.0).abs() < 10.0 => "Marathon".to_string(),
        d => format!("{:.1} km", d / 1000.0),
    }
}

/// Combine a Daniels VDOT projection and a Riegel projection into a graded
/// finish-time prediction (running-039). Agreement within ~2% collapses to a
/// single time; otherwise a low–high range is shown so neither method's false
/// precision stands alone. Degenerate input yields an empty (`-`) prediction.
fn to_race_view(q: &RaceQuery) -> RacePredictionView {
    let exponent = load::riegel_exponent(q.weekly_km);
    let riegel_sec = load::riegel_predict(
        q.recent_time_sec,
        q.recent_distance_m,
        q.goal_distance_m,
        exponent,
    );
    let vdot = load::vdot(q.recent_distance_m, q.recent_time_sec);
    let daniels_sec = load::daniels_predict(vdot, q.goal_distance_m);
    let eq = running::race_equivalency(riegel_sec, daniels_sec);
    let (agreed, low_sec, high_sec) = match eq.value {
        running::Equivalency::Agreed(mid) => (true, mid, mid),
        running::Equivalency::Range(lo, hi) => (false, lo, hi),
    };

    let goal_label = race_distance_label(q.goal_distance_m);
    let degenerate = low_sec <= 0.0;
    let predicted = if degenerate {
        "-".to_string()
    } else if agreed {
        fmt_race_clock(low_sec)
    } else {
        format!("{}–{}", fmt_race_clock(low_sec), fmt_race_clock(high_sec))
    };
    let summary = if degenerate {
        format!("{goal_label}: need a valid recent race to predict")
    } else if agreed {
        format!("{goal_label} ≈ {predicted} (Daniels & Riegel agree)")
    } else {
        format!("{goal_label} ≈ {predicted} (Daniels–Riegel range)")
    };

    RacePredictionView {
        goal_label,
        predicted,
        agreed,
        low_sec,
        high_sec,
        summary,
        grade: format!("{:?}", eq.evidence.grade),
        citation: eq.evidence.citation.reference.clone(),
        confidence: eq.confidence.score,
        safety_critical: eq.confidence.safety_critical,
        contested: eq.confidence.contested,
    }
}

/// Wrap a raw value in a `Recommended` carrying a registry claim's evidence +
/// confidence, the same mechanism `hypertrophy::recommend` uses, so a
/// non-`Recommended` engine value (the volume landmarks) still surfaces graded
/// (HARD RULE 2). Panics only on a missing claim id (a compile-time constant).
fn graded<T>(value: T, claim_id: &str) -> Recommended<T> {
    let c = crate::evidence::claim(claim_id).expect("known claim id");
    Recommended {
        value,
        evidence: c.to_evidence(),
        confidence: c.to_confidence_tag(),
    }
}

/// Build a graded per-week hypertrophy accumulation plan for one muscle. Every
/// row carries its own evidence + confidence via [`push_guidance`] (HARD RULE 2);
/// no training numbers are invented: all come from [`hypertrophy`]. An unknown
/// muscle yields a single explanatory row (no fabricated landmarks). `weeks == 0`
/// yields no plan rows beyond the landmarks context.
fn build_hypertrophy_plan(q: &HypertrophyPlanQuery) -> Vec<GuidanceView> {
    let mut rows = Vec::new();
    let section = q.muscle.clone();

    let Some(lm) = hypertrophy::landmarks_for(&q.muscle) else {
        // No landmarks for this muscle: say so plainly rather than guess. The
        // note itself is the RP landmark framing, so it is graded from that claim.
        let note = graded((), "HYP-LANDMARKS-001");
        push_guidance(
            &mut rows,
            &section,
            format!(
                "\"{}\" is not a known muscle - pick one of: chest, back, quads, hamstrings, glutes, side delts, rear delts, biceps, triceps, calves, abs",
                q.muscle
            ),
            &note,
        );
        return rows;
    };

    // Landmarks row, landmarks are a raw engine value, not a Recommended, so
    // wrap them in their HYP-LANDMARKS-001 evidence (ExpertOpinion) to grade it.
    let landmarks = graded((), "HYP-LANDMARKS-001");
    push_guidance(
        &mut rows,
        &section,
        format!(
            "Landmarks - MEV {} · MAV {}–{} · MRV {} sets/wk",
            lm.mev, lm.mav.0, lm.mav.1, lm.mrv
        ),
        &landmarks,
    );

    // `weeks == 0` is degenerate: the ramp is empty and there is no schedule to
    // show, so stop after the landmarks context rather than emit empty rows.
    if q.weeks == 0 {
        return rows;
    }

    // Weekly set ramp MEV → MRV across the accumulation block.
    let ramp = hypertrophy::weekly_set_ramp(lm.mev, lm.mrv, q.weeks);
    let ramp_str = ramp
        .value
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(" → ");
    push_guidance(
        &mut rows,
        &section,
        format!("Weekly set ramp: {ramp_str}"),
        &ramp,
    );

    // RIR schedule across the block (first week's Recommended carries the grade).
    let rir_vals: Vec<u8> = (1..=q.weeks)
        .filter_map(|w| hypertrophy::rir_for_week(w, q.weeks).map(|r| r.value))
        .collect();
    if let Some(first) = hypertrophy::rir_for_week(1, q.weeks) {
        let rir_str = rir_vals
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(" → ");
        push_guidance(
            &mut rows,
            &section,
            format!("RIR schedule: {rir_str}"),
            &first,
        );
    }

    // Peak-week frequency + per-session spread (from the ramp's top set count).
    let peak_sets = ramp.value.iter().copied().max().unwrap_or(lm.mrv);
    let freq = hypertrophy::frequency_for_weekly_sets(peak_sets);
    push_guidance(
        &mut rows,
        &section,
        format!(
            "Peak frequency: {}–{}×/wk · {}–{} sets/session",
            freq.value.freq.0,
            freq.value.freq.1,
            freq.value.per_session.0,
            freq.value.per_session.1
        ),
        &freq,
    );

    rows
}

/// Build absolute daily protein target rows by scaling each graded g/kg range by
/// the athlete's bodyweight. Multiplying a graded g/kg bound by bodyweight is
/// honest arithmetic, the grade travels with the underlying claim via
/// [`push_guidance`] (HARD RULE 2). No general/default protein number is
/// invented: if neither goal context is selected (or bodyweight is non-positive)
/// the section is empty (HARD RULE 1).
fn build_protein_targets(q: &ProteinQuery) -> Vec<GuidanceView> {
    let mut rows = Vec::new();

    // Cannot derive g/day from a non-positive bodyweight, say nothing rather
    // than emit a nonsensical or zero target.
    if q.bodyweight_kg <= 0.0 {
        return rows;
    }

    if q.masters {
        let r = individualization::masters_protein_target();
        let (lo, hi) = r.value.g_per_kg;
        push_guidance(
            &mut rows,
            "Protein",
            format!(
                "Masters (65+): {:.0}–{:.0} g/day ({lo:.1}–{hi:.1} g/kg × {:.0} kg)",
                q.bodyweight_kg * lo,
                q.bodyweight_kg * hi,
                q.bodyweight_kg
            ),
            &r,
        );
    }

    if q.deficit {
        let r = individualization::deficit_protein_target();
        let (lo, hi) = r.value.g_per_kg;
        push_guidance(
            &mut rows,
            "Protein",
            format!(
                "Deficit (lean-mass preserving): {:.0}–{:.0} g/day ({lo:.1}–{hi:.1} g/kg × {:.0} kg)",
                q.bodyweight_kg * lo,
                q.bodyweight_kg * hi,
                q.bodyweight_kg
            ),
            &r,
        );
    }

    rows
}

/// Build a graded heart-rate-zone table from age: the Tanaka HRmax estimate plus
/// the five Daniels %HRmax training bands mapped to absolute bpm ranges
/// (`running::vdot_band_hr_pct`, running-007). Every row is graded RUN-VDOT-001
/// (Moderate), no training numbers are invented (HARD RULE 1/2). A non-positive
/// or implausible age yields a single explanatory row rather than a bogus HRmax.
fn build_hr_zones(q: &HrZoneQuery) -> Vec<GuidanceView> {
    let mut rows = Vec::new();

    // HRmax from age is only meaningful for a realistic adult/junior age; refuse
    // to emit a fabricated maximum outside that range.
    if !(5.0..=100.0).contains(&q.age_years) {
        let note = graded((), "RUN-VDOT-001");
        push_guidance(
            &mut rows,
            "Heart-rate zones",
            "Enter an age between 5 and 100 to estimate HRmax and training zones.".to_string(),
            &note,
        );
        return rows;
    }

    let hr_max = load::hr_max_estimate(q.age_years, load::HrMaxFormula::Tanaka);
    let hr_max_row = graded((), "RUN-VDOT-001");
    push_guidance(
        &mut rows,
        "Heart-rate zones",
        format!(
            "Estimated HRmax: {hr_max:.0} bpm (Tanaka 208 − 0.7 × {:.0})",
            q.age_years
        ),
        &hr_max_row,
    );

    // Daniels VDOT bands, easy → hard. Each carries its own RUN-VDOT-001 grade.
    for band in [
        VdotBand::Easy,
        VdotBand::Marathon,
        VdotBand::Threshold,
        VdotBand::Interval,
        VdotBand::Repetition,
    ] {
        let (lo_pct, hi_pct) = running::vdot_band_hr_pct(band);
        let bpm_lo = hr_max * lo_pct / 100.0;
        let bpm_hi = hr_max * hi_pct / 100.0;
        let band_row = graded((), "RUN-VDOT-001");
        let range = if (bpm_hi - bpm_lo).abs() < 0.5 {
            format!("{bpm_lo:.0} bpm")
        } else {
            format!("{bpm_lo:.0}–{bpm_hi:.0} bpm")
        };
        push_guidance(
            &mut rows,
            "Heart-rate zones",
            format!("{band:?}: {lo_pct:.0}–{hi_pct:.0} %HRmax → {range}"),
            &band_row,
        );
    }

    rows
}

/// Derive strength metrics for one logged set (Epley e1RM, RIR from RPE).
fn to_lift_view(s: &LoggedSet) -> LiftResultView {
    let e1rm_kg = (strength::e1rm_epley(s.weight_kg, s.reps) * 10.0).round() / 10.0;
    let pct_1rm = strength::est_pct_1rm_from_reps(s.reps).round();
    let rir = strength::rpe_to_rir(s.rpe);
    LiftResultView {
        exercise: s.exercise.clone(),
        weight_kg: s.weight_kg,
        reps: s.reps,
        rpe: s.rpe,
        e1rm_kg,
        pct_1rm,
        rir,
        summary: format!(
            // `{}` (not `{:.0}`) on the logged weight so a fractional plate load
            // (e.g. 92.5 kg on 2.5 kg jumps) shows as "92.5kg", not a truncated
            // "92kg": the summary is the human line the shell renders, and it must
            // echo what the lifter actually did. Integer loads still print clean
            // ("100kg", not "100.0kg").
            "{} {}kg × {} @RPE{:.1} → e1RM {:.1}kg (~{:.0}%1RM, {:.1} RIR)",
            s.exercise, s.weight_kg, s.reps, s.rpe, e1rm_kg, pct_1rm, rir
        ),
    }
}

/// Flatten one evidence-wrapped adjustment into a shell-facing row.
fn to_view(r: &Recommended<Adjustment>) -> AdjustmentView {
    AdjustmentView {
        summary: describe(&r.value),
        grade: format!("{:?}", r.evidence.grade),
        citation: r.evidence.citation.reference.clone(),
        confidence: r.confidence.score,
        safety_critical: r.confidence.safety_critical,
        contested: r.confidence.contested,
    }
}

/// Human-readable one-liner for an adjustment.
fn describe(a: &Adjustment) -> String {
    match a {
        Adjustment::ReduceLoadPct(p) => format!("Reduce load {p:.0}% for remaining sets"),
        Adjustment::IncreaseLoadPct(p) => format!("Increase load {p:.0}% - readiness is high"),
        Adjustment::Deload {
            volume_reduction_pct,
            load_reduction_pct,
            weeks,
        } => format!(
            "Deload {weeks} wk: volume −{volume_reduction_pct:.0}%, load −{load_reduction_pct:.0}%"
        ),
        Adjustment::DowngradeSession => "Downgrade to an easier session".into(),
        Adjustment::RestDay => "Take a full rest day".into(),
        Adjustment::Stop => "Stop - do not train".into(),
        Adjustment::Defer { reason } => reason.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::ReadinessSignal;

    fn input(signal: ReadinessSignal, value: f64) -> ReadinessInput {
        ReadinessInput {
            signal,
            value,
            observed_at: 0,
        }
    }

    /// The Android shell hand-serializes each [`Event`] to serde's external
    /// tagging (`Core.kt::toJson`) and omits every absent optional field. Replay
    /// feeds those exact bytes back through `update`, and the FFI *swallows*
    /// deserialize errors (`ffi::process_event` clears the output on failure), so
    /// a drift between Kotlin's wire form and this enum's `Deserialize` would
    /// silently drop the event on every cold start. Pin the omit-optionals form.
    #[test]
    fn android_wire_form_deserializes_with_optionals_omitted() {
        // Exactly what Core.kt emits for a review with no run/lift context: only
        // the four non-optional booleans/count, every Option field absent.
        let bare = r#"{"SubmitReview":{"bone_pain_red_flag":false,"compulsive_flag":false,"overtraining_signal_count":0,"bad_day":false}}"#;
        match serde_json::from_str::<Event>(bare).expect("bare review must parse") {
            Event::SubmitReview(r) => assert_eq!(
                r,
                SessionReview::default(),
                "omitted optionals must deserialize to None/default"
            ),
            other => panic!("expected SubmitReview, got {other:?}"),
        }

        // SetProfile carries no optionals, but pin its snake_case field contract
        // too, a renamed field here silently drops the profile on replay.
        let profile = r#"{"SetProfile":{"progression_cadence":"WeekToWeek","lift_goal":"MaxStrength","goal_distance":"TenK","concurrent_goal":"Strength","weekly_sets":14,"running_days_per_week":4,"running_km_per_week":45.0,"advanced":false,"endurance_intensity_pct_vo2max":75.0}}"#;
        assert!(
            matches!(
                serde_json::from_str::<Event>(profile),
                Ok(Event::SetProfile(_))
            ),
            "profile wire form must parse"
        );
    }

    /// HARD RULE 3 at the wire level: the shell's red-flag readiness buttons must
    /// never be silently dropped on replay. Each safety signal's Kotlin `.name`
    /// string (`Core.kt::ReadinessSignal`) must deserialize to the matching
    /// variant, a rename on either side would make the FFI swallow the event
    /// (deserialize error → cleared output) and leave a red-flag session
    /// un-blocked. Pin the safety signals' exact wire names.
    #[test]
    fn safety_readiness_signals_deserialize_from_shell_names() {
        for (name, expected) in [
            ("Pain", ReadinessSignal::Pain),
            ("Illness", ReadinessSignal::Illness),
            ("RedS", ReadinessSignal::RedS),
            ("CardiacRedFlag", ReadinessSignal::CardiacRedFlag),
            ("BoneStress", ReadinessSignal::BoneStress),
        ] {
            let wire = format!(
                r#"{{"SubmitReadiness":{{"signal":"{name}","value":1.0,"observed_at":0}}}}"#
            );
            match serde_json::from_str::<Event>(&wire).expect("safety signal must parse") {
                Event::SubmitReadiness(i) => {
                    assert_eq!(i.signal, expected, "{name} deserialized to wrong signal")
                }
                other => panic!("expected SubmitReadiness for {name}, got {other:?}"),
            }
        }
    }

    #[test]
    fn pain_blocks_training_with_a_single_stop() {
        let app = Engine;
        let mut model = Model::default();

        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::Pain, 1.0)),
            &mut model,
        )
        .expect_only_render();

        let vm = app.view(&model);
        assert_eq!(vm.safety_tier.as_deref(), Some("Pain"));
        assert!(vm.train_blocked);
        assert_eq!(vm.adjustments.len(), 1);
        assert_eq!(vm.adjustments[0].summary, "Stop - do not train");
    }

    #[test]
    fn medical_red_flags_surface_referral_tier_through_the_view() {
        // The Android shell can now submit the medical-referral signals
        // (Illness/RedS/CardiacRedFlag/BoneStress). Lock the full event→view
        // contract those buttons depend on: each must raise the top
        // `MedicalReferral` tier and block training, so the SafetyBanner shows a
        // DO-NOT-TRAIN state rather than silently swallowing a red flag.
        for signal in [
            ReadinessSignal::RedS,
            ReadinessSignal::CardiacRedFlag,
            ReadinessSignal::BoneStress,
        ] {
            let app = Engine;
            let mut model = Model::default();
            app.update(Event::SubmitReadiness(input(signal, 1.0)), &mut model)
                .expect_only_render();
            let vm = app.view(&model);
            assert_eq!(
                vm.safety_tier.as_deref(),
                Some("MedicalReferral"),
                "{signal:?} must raise the referral tier"
            );
            assert!(vm.train_blocked, "{signal:?} must block training");
        }
    }

    #[test]
    fn illness_severity_bands_map_to_tier_through_the_view() {
        // The Android ReadinessEditor sends Illness as a severity value (0 none,
        // 1 above-neck, 2 below-neck/fever) via its IllnessLevel picker. Lock the
        // full event→view contract that picker depends on: above-neck raises the
        // Illness tier without blocking (a downgraded session), while below-neck/
        // fever must block training outright. A drift in the value bands would let
        // the shell submit a fever as a mere downgrade.
        let app = Engine;
        let mut above = Model::default();
        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::Illness, 1.0)),
            &mut above,
        )
        .expect_only_render();
        let vm = app.view(&above);
        assert_eq!(vm.safety_tier.as_deref(), Some("Illness"));
        assert!(
            !vm.train_blocked,
            "above-neck illness downgrades but must not block training"
        );

        let mut fever = Model::default();
        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::Illness, 2.0)),
            &mut fever,
        )
        .expect_only_render();
        let vm = app.view(&fever);
        assert_eq!(vm.safety_tier.as_deref(), Some("Illness"));
        assert!(
            vm.train_blocked,
            "below-neck/fever illness must block training"
        );
    }

    #[test]
    fn resting_hr_plus_ten_blocks_training_through_the_view() {
        // autoreg-041: a morning RHR ≥ +10 bpm over baseline forces a rest day.
        // The autoreg unit tests pin rhr_stop → RestDay; this locks the rest of
        // the contract the shell renders, that the RestDay actually trips
        // train_blocked and surfaces the single-day-marker tier. A +5 bpm reading
        // stays below the stop: it downgrades intensity but must NOT block.
        let app = Engine;
        let mut blocked = Model::default();
        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::RestingHr, 10.0)),
            &mut blocked,
        )
        .expect_only_render();
        let vm = app.view(&blocked);
        assert_eq!(vm.safety_tier.as_deref(), Some("SingleDayMarker"));
        assert!(vm.train_blocked, "RHR +10 bpm must block training");

        let mut downgrade = Model::default();
        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::RestingHr, 5.0)),
            &mut downgrade,
        )
        .expect_only_render();
        let vm = app.view(&downgrade);
        assert!(
            !vm.train_blocked,
            "RHR +5 bpm downgrades intensity but must not block"
        );
    }

    #[test]
    fn clean_inputs_leave_training_open() {
        let app = Engine;
        let mut model = Model::default();

        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::Rpe, 0.0)),
            &mut model,
        )
        .expect_only_render();

        let vm = app.view(&model);
        assert_eq!(vm.safety_tier, None);
        assert!(!vm.train_blocked);
        assert!(vm.adjustments.is_empty());
        assert_eq!(vm.input_count, 1);
    }

    #[test]
    fn low_wellness_surfaces_cited_downgrade_without_blocking() {
        // Mirrors the shell's "Readiness" quick action (WellnessZ = −1.5): a
        // sub-threshold subjective signal must yield an optimization adjustment
        // that stays evidence-cited and does not trip the safety block.
        let app = Engine;
        let mut model = Model::default();

        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::WellnessZ, -1.5)),
            &mut model,
        )
        .expect_only_render();

        let vm = app.view(&model);
        // Subjective wellness registers a SubjectiveMultiDay tier but, unlike a
        // Pain/Illness red flag, never hard-blocks training.
        assert!(!vm.train_blocked);
        assert_eq!(vm.safety_tier.as_deref(), Some("SubjectiveMultiDay"));
        let adj = vm
            .adjustments
            .iter()
            .find(|a| a.summary == "Downgrade to an easier session")
            .expect("WellnessZ −1.5 should surface a downgrade adjustment");
        assert!(!adj.grade.is_empty() && !adj.citation.is_empty());
    }

    #[test]
    fn logging_a_set_derives_e1rm_and_rir() {
        let app = Engine;
        let mut model = Model::default();

        app.update(
            Event::LogSet {
                exercise: "Back squat".into(),
                weight_kg: 100.0,
                reps: 5,
                rpe: 8.0,
            },
            &mut model,
        )
        .expect_only_render();

        let vm = app.view(&model);
        assert_eq!(vm.lifts.len(), 1);
        // Epley: 100 * (1 + 5/30) = 116.7
        assert!((vm.lifts[0].e1rm_kg - 116.7).abs() < 0.05);
        // Epley inverse for 5 reps: 100 / (1 + 5/30) = 85.7 → 86%1RM.
        assert!((vm.lifts[0].pct_1rm - 86.0).abs() < f64::EPSILON);
        // RPE 8 → 2 RIR.
        assert!((vm.lifts[0].rir - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn logging_a_set_echoes_raw_input_alongside_derived_metrics() {
        // The view carries the set exactly as logged, not only the derived e1RM: a
        // lifter must see what they actually did (weight × reps @ RPE), which the
        // shell renders beside the coaching metrics. Locks that echo contract, a
        // fractional plate load must survive to the view unrounded.
        let app = Engine;
        let mut model = Model::default();

        app.update(
            Event::LogSet {
                exercise: "Front squat".into(),
                weight_kg: 92.5,
                reps: 3,
                rpe: 9.0,
            },
            &mut model,
        )
        .expect_only_render();

        let lift = app.view(&model).lifts.remove(0);
        assert_eq!(lift.exercise, "Front squat");
        assert!((lift.weight_kg - 92.5).abs() < f64::EPSILON);
        assert_eq!(lift.reps, 3);
        assert!((lift.rpe - 9.0).abs() < f64::EPSILON);
        // The summary is the human line the shell shows; it names the raw set (reps
        // @ RPE), not only the derived e1RM. The fractional plate load must survive
        // here too: a 92.5 kg lift must read "92.5kg", not a truncated "92kg".
        assert!(lift.summary.contains("92.5kg"), "got {}", lift.summary);
        assert!(lift.summary.contains("× 3 @RPE9.0"), "got {}", lift.summary);
        assert!(lift.summary.contains("RIR"), "got {}", lift.summary);
    }

    #[test]
    fn logged_sets_accumulate_in_log_order() {
        // The view lists sets oldest-first, in the order they were logged: the
        // shell renders this sequence directly, so a regression that reversed or
        // de-duplicated the list would misreport a session. Two distinct sets must
        // both survive, in order.
        let app = Engine;
        let mut model = Model::default();
        for (exercise, reps) in [("Deadlift", 3), ("Bench", 8)] {
            app.update(
                Event::LogSet {
                    exercise: exercise.into(),
                    weight_kg: 80.0,
                    reps,
                    rpe: 7.0,
                },
                &mut model,
            )
            .expect_only_render();
        }

        let lifts = app.view(&model).lifts;
        assert_eq!(lifts.len(), 2);
        assert_eq!(lifts[0].exercise, "Deadlift");
        assert_eq!(lifts[0].reps, 3);
        assert_eq!(lifts[1].exercise, "Bench");
        assert_eq!(lifts[1].reps, 8);
    }

    #[test]
    fn logging_a_run_classifies_zone_and_pace() {
        let app = Engine;
        let mut model = Model::default();

        app.update(
            Event::LogRun {
                distance_km: 10.0,
                duration_min: 50.0,
                hr_pct_max: 70.0,
                longest_recent_km: 12.0,
            },
            &mut model,
        )
        .expect_only_render();

        let vm = app.view(&model);
        assert_eq!(vm.runs.len(), 1);
        assert_eq!(vm.runs[0].pace, "5:00/km");
        assert!(!vm.runs[0].spike_flag);
        assert_eq!(vm.runs[0].zone, "Z1");
    }

    #[test]
    fn run_zone_strings_match_the_shell_colour_contract() {
        // The view emits the zone as a bare `"Z1"/"Z2"/"Z3"` string (or `"-"`
        // with no HR), and the Android shell's `hrZoneColor` matches on exactly
        // those literals. Renaming the `ThreeZone` variants would compile clean
        // but silently drop every zone colour on-device: this locks the wire
        // strings so such a rename fails here instead.
        let app = Engine;
        let cases = [(70.0, "Z1"), (85.0, "Z2"), (92.0, "Z3"), (0.0, "-")];
        for (hr, expected) in cases {
            let mut model = Model::default();
            app.update(
                Event::LogRun {
                    distance_km: 10.0,
                    duration_min: 50.0,
                    hr_pct_max: hr,
                    longest_recent_km: 12.0,
                },
                &mut model,
            )
            .expect_only_render();
            assert_eq!(app.view(&model).runs[0].zone, expected, "hr {hr}%");
        }
    }

    #[test]
    fn readiness_signal_wire_names_match_the_shell_contract() {
        // Each `ReadinessSignal` serialises to its bare variant name, and the
        // Android/web shells hand-build that exact string as the event's `signal`
        // field (Core.kt `ReadinessSignal.name`). Renaming a variant here would
        // compile clean but silently drop every readiness submission for it -
        // including the medical-referral flags at the top of the safety ladder.
        // This locks the wire names so such a rename fails here instead.
        let cases = [
            (ReadinessSignal::Rpe, "Rpe"),
            (ReadinessSignal::EstimatedOneRm, "EstimatedOneRm"),
            (ReadinessSignal::BarVelocity, "BarVelocity"),
            (ReadinessSignal::VelocityLoss, "VelocityLoss"),
            (ReadinessSignal::WellnessZ, "WellnessZ"),
            (ReadinessSignal::HrvLnRmssd, "HrvLnRmssd"),
            (ReadinessSignal::HrvCv, "HrvCv"),
            (ReadinessSignal::AerobicDecoupling, "AerobicDecoupling"),
            (ReadinessSignal::RestingHr, "RestingHr"),
            (ReadinessSignal::Pain, "Pain"),
            (ReadinessSignal::Illness, "Illness"),
            (ReadinessSignal::RedS, "RedS"),
            (ReadinessSignal::CardiacRedFlag, "CardiacRedFlag"),
            (ReadinessSignal::BoneStress, "BoneStress"),
        ];
        for (signal, wire) in cases {
            assert_eq!(
                serde_json::to_string(&signal).unwrap(),
                format!("\"{wire}\""),
                "wire name for {signal:?}"
            );
        }
    }

    #[test]
    fn zero_duration_run_reports_unknown_pace_not_zero() {
        // A run with distance but zero elapsed time (e.g. GPS fixes sharing a
        // timestamp) must not derive a nonsense "0:00/km" pace.
        let app = Engine;
        let mut model = Model::default();

        app.update(
            Event::LogRun {
                distance_km: 5.0,
                duration_min: 0.0,
                hr_pct_max: 70.0,
                longest_recent_km: 6.0,
            },
            &mut model,
        )
        .expect_only_render();

        assert_eq!(app.view(&model).runs[0].pace, "-");
    }

    #[test]
    fn spike_summary_explains_a_missing_baseline_vs_a_real_jump() {
        // The spike gate errs safe with no history, so the first run trips it. Its
        // summary must not claim a ">10%" jump over a baseline that does not exist;
        // a later run that genuinely exceeds the prior longest gets the ">10%"
        // wording. Both still set spike_flag, only the wording differs.
        let app = Engine;
        let mut model = Model::default();

        app.update(
            Event::LogRun {
                distance_km: 5.0,
                duration_min: 25.0,
                hr_pct_max: 70.0,
                longest_recent_km: 0.0,
            },
            &mut model,
        )
        .expect_only_render();
        let vm = app.view(&model);
        assert!(vm.runs[0].spike_flag);
        assert!(
            vm.runs[0].summary.contains("no prior run"),
            "first run should explain the missing baseline, got: {}",
            vm.runs[0].summary
        );

        app.update(
            Event::LogRun {
                distance_km: 20.0,
                duration_min: 100.0,
                hr_pct_max: 70.0,
                longest_recent_km: 0.0,
            },
            &mut model,
        )
        .expect_only_render();
        let vm = app.view(&model);
        assert!(vm.runs[1].spike_flag);
        assert!(
            vm.runs[1].summary.contains(">10% over recent longest"),
            "a real jump over the prior longest should say so, got: {}",
            vm.runs[1].summary
        );
    }

    #[test]
    fn pace_truncates_fractional_seconds_and_zero_pads() {
        // Pace is formatted `m:ss/km` by flooring seconds (a `u32` cast) then
        // zero-padding. 40.95 min / 10 km = 245.7 s/km, which must render as
        // "4:05/km": 245.7 floored to 245 (not rounded up to 4:06) and the 5 s
        // padded to "05". Flooring also structurally prevents a "4:60" overflow,
        // since `245 % 60` can never reach 60. Locks both against a formatting
        // regression.
        let app = Engine;
        let mut model = Model::default();

        app.update(
            Event::LogRun {
                distance_km: 10.0,
                duration_min: 40.95,
                hr_pct_max: 70.0,
                longest_recent_km: 12.0,
            },
            &mut model,
        )
        .expect_only_render();

        assert_eq!(app.view(&model).runs[0].pace, "4:05/km");
    }

    #[test]
    fn manual_run_spike_baseline_is_derived_from_prior_runs() {
        // Same guarantee as the GPS path: a manual run's spike gate floors its
        // baseline to the longest run already held, even when the caller sends 0.
        let app = Engine;
        let mut model = Model::default();

        app.update(
            Event::LogRun {
                distance_km: 20.0,
                duration_min: 100.0,
                hr_pct_max: 70.0,
                longest_recent_km: 0.0,
            },
            &mut model,
        )
        .expect_only_render();

        // 10 km is well under the derived 20 km baseline → no spike.
        app.update(
            Event::LogRun {
                distance_km: 10.0,
                duration_min: 50.0,
                hr_pct_max: 70.0,
                longest_recent_km: 0.0,
            },
            &mut model,
        )
        .expect_only_render();

        assert!(!app.view(&model).runs[1].spike_flag);
    }

    #[test]
    fn manual_run_spike_baseline_reads_a_prior_gps_runs_derived_distance() {
        // Cross-type baseline: a GPS run stores distance_km = 0.0 (it derives from
        // the track), so a later manual run's spike gate only sees a real baseline
        // if `run_distance_km` reconstructs the GPS run's distance from its fixes.
        // If that derivation regressed to the stored 0.0, the baseline would be 0,
        // the safe-with-no-history gate would fire, and this 1 km run would flag -
        // so asserting NO spike pins the GPS branch of the fold.
        let app = Engine;
        let mut model = Model::default();

        // GPS run: ~1.11 km along the equator (0 → 0.01° longitude).
        app.update(
            Event::LogRunTrack {
                points: vec![
                    GpsPoint {
                        lat: 0.0,
                        lon: 0.0,
                        observed_at: 0,
                        accuracy_m: 5.0,
                    },
                    GpsPoint {
                        lat: 0.0,
                        lon: 0.01,
                        observed_at: 400,
                        accuracy_m: 5.0,
                    },
                ],
                hr_pct_max: 0.0,
                longest_recent_km: 0.0,
            },
            &mut model,
        )
        .expect_only_render();

        // Manual 1 km run: under the ~1.11 km baseline derived from the GPS track,
        // so no spike, but only because the baseline came from the GPS run.
        app.update(
            Event::LogRun {
                distance_km: 1.0,
                duration_min: 6.0,
                hr_pct_max: 70.0,
                longest_recent_km: 0.0,
            },
            &mut model,
        )
        .expect_only_render();

        assert!(!app.view(&model).runs[1].spike_flag);
    }

    #[test]
    fn run_distance_spike_is_flagged() {
        let app = Engine;
        let mut model = Model::default();

        app.update(
            Event::LogRun {
                distance_km: 20.0,
                duration_min: 100.0,
                hr_pct_max: 75.0,
                longest_recent_km: 12.0,
            },
            &mut model,
        )
        .expect_only_render();

        assert!(app.view(&model).runs[0].spike_flag);
    }

    #[test]
    fn gps_tracked_run_derives_distance_and_pace_from_fixes() {
        let app = Engine;
        let mut model = Model::default();

        // ~1.11 km along the equator (0 → 0.01° lon) over 400 s.
        let points = vec![
            GpsPoint {
                lat: 0.0,
                lon: 0.0,
                observed_at: 0,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.005,
                observed_at: 200,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.01,
                observed_at: 400,
                accuracy_m: 5.0,
            },
        ];
        app.update(
            Event::LogRunTrack {
                points,
                hr_pct_max: 70.0,
                longest_recent_km: 12.0,
            },
            &mut model,
        )
        .expect_only_render();

        let vm = app.view(&model);
        assert_eq!(vm.runs.len(), 1);
        assert!(
            vm.runs[0].summary.starts_with("GPS "),
            "got {}",
            vm.runs[0].summary
        );
        assert!(
            vm.runs[0].summary.contains("1.1km"),
            "got {}",
            vm.runs[0].summary
        );
        assert_ne!(vm.runs[0].pace, "-");
        assert!(!vm.runs[0].spike_flag);
        // GPS-tracked run carries a GPX document ready for export.
        assert!(
            vm.runs[0].gpx.contains("<trkpt "),
            "gpx: {}",
            vm.runs[0].gpx
        );
    }

    #[test]
    fn gps_run_of_only_noise_fixes_offers_no_gpx_export() {
        let app = Engine;
        let mut model = Model::default();

        // Every fix is worse than the accuracy gate, so none survive filtering:
        // fewer than two usable points is not a route, and offering an "Export
        // GPX" button (gpx non-empty) for a 0 km run would be misleading.
        let points = vec![
            GpsPoint {
                lat: 0.0,
                lon: 0.0,
                observed_at: 0,
                accuracy_m: 80.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.01,
                observed_at: 400,
                accuracy_m: 120.0,
            },
        ];
        app.update(
            Event::LogRunTrack {
                points,
                hr_pct_max: 70.0,
                longest_recent_km: 12.0,
            },
            &mut model,
        )
        .expect_only_render();

        let vm = app.view(&model);
        assert_eq!(vm.runs.len(), 1);
        assert_eq!(vm.runs[0].pace, "-");
        assert!(
            vm.runs[0].gpx.is_empty(),
            "gpx should be empty for a noise-only track, got {}",
            vm.runs[0].gpx
        );
        // An unmeasurable track must say so, not render a "0.0km" entry, and must
        // not trip the distance-spike gate against a phantom zero baseline.
        assert!(
            vm.runs[0].summary.contains("too poor"),
            "summary should flag the poor GPS signal, got {}",
            vm.runs[0].summary
        );
        assert!(!vm.runs[0].spike_flag);
    }

    #[test]
    fn gps_run_without_hr_reports_unknown_zone_not_fabricated_z1() {
        let app = Engine;
        let mut model = Model::default();
        let points = vec![
            GpsPoint {
                lat: 0.0,
                lon: 0.0,
                observed_at: 0,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.01,
                observed_at: 400,
                accuracy_m: 5.0,
            },
        ];
        // hr_pct_max = 0.0 → no HR sample → zone must not be fabricated.
        app.update(
            Event::LogRunTrack {
                points,
                hr_pct_max: 0.0,
                longest_recent_km: 0.0,
            },
            &mut model,
        )
        .expect_only_render();

        let vm = app.view(&model);
        assert_eq!(vm.runs[0].zone, "-");
        assert!(
            vm.runs[0].summary.contains("(-)"),
            "got {}",
            vm.runs[0].summary
        );
    }

    #[test]
    fn gps_spike_baseline_is_derived_from_prior_runs() {
        let app = Engine;
        let mut model = Model::default();
        let one_km = || {
            vec![
                GpsPoint {
                    lat: 0.0,
                    lon: 0.0,
                    observed_at: 0,
                    accuracy_m: 5.0,
                },
                GpsPoint {
                    lat: 0.0,
                    lon: 0.01,
                    observed_at: 400,
                    accuracy_m: 5.0,
                },
            ]
        };

        // Run 1: ~1.11 km with no history. The core spike gate errs safe with no
        // baseline, so this first run is flagged, expected and intentional.
        app.update(
            Event::LogRunTrack {
                points: one_km(),
                hr_pct_max: 0.0,
                longest_recent_km: 0.0,
            },
            &mut model,
        )
        .expect_only_render();
        assert!(app.view(&model).runs[0].spike_flag);

        // Run 2: same ~1.11 km. Shell again sends baseline 0.0, but the core now
        // derives the baseline from run 1 (~1.11 km): 1.11 is not >10 % over
        // itself, so NO spike. This only passes if the baseline came from history.
        app.update(
            Event::LogRunTrack {
                points: one_km(),
                hr_pct_max: 0.0,
                longest_recent_km: 0.0,
            },
            &mut model,
        )
        .expect_only_render();
        assert!(!app.view(&model).runs[1].spike_flag);

        // Run 3: ~2.22 km, nearly double the derived baseline → spike.
        let long = vec![
            GpsPoint {
                lat: 0.0,
                lon: 0.0,
                observed_at: 0,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.02,
                observed_at: 800,
                accuracy_m: 5.0,
            },
        ];
        app.update(
            Event::LogRunTrack {
                points: long,
                hr_pct_max: 0.0,
                longest_recent_km: 0.0,
            },
            &mut model,
        )
        .expect_only_render();
        assert!(app.view(&model).runs[2].spike_flag);
    }

    fn sample_profile() -> Profile {
        Profile {
            progression_cadence: ProgressionCadence::WeekToWeek,
            lift_goal: LiftGoal::MaxStrength,
            goal_distance: GoalDistance::Marathon,
            concurrent_goal: ConcurrentGoal::EndurancePriority,
            weekly_sets: 14,
            running_days_per_week: 5,
            running_km_per_week: 50.0,
            advanced: true,
            endurance_intensity_pct_vo2max: 75.0,
        }
    }

    #[test]
    fn profile_drives_evidence_cited_guidance() {
        let app = Engine;
        let mut model = Model::default();
        assert!(app.view(&model).guidance.is_empty());

        app.update(Event::SetProfile(sample_profile()), &mut model)
            .expect_only_render();

        let vm = app.view(&model);
        assert!(!vm.guidance.is_empty());
        // Every surfaced row carries a grade + citation; none is MarketingMyth.
        assert!(
            vm.guidance
                .iter()
                .all(|g| !g.grade.is_empty() && !g.citation.is_empty())
        );
        assert!(vm.guidance.iter().all(|g| g.grade != "MarketingMyth"));
        // The running-days=5, km=50 profile trips the hybrid lower-lift cap.
        assert!(
            vm.guidance
                .iter()
                .any(|g| g.summary.contains("Lower-lift cap"))
        );
        // 50 km/wk is under the ~64 km bone-stress threshold, so no BSI row yet.
        assert!(
            !vm.guidance
                .iter()
                .any(|g| g.summary.contains("Bone-stress-injury"))
        );
        // MaxStrength surfaces the 20% velocity-loss set cutoff (AUTOREG-VL-001).
        assert!(
            vm.guidance
                .iter()
                .any(|g| g.summary.contains("Velocity-loss set cutoff"))
        );
        // 14 weekly sets/muscle (>12) trips the session-split guidance (HYP-FREQ-001).
        assert!(
            vm.guidance
                .iter()
                .any(|g| g.summary.contains("split across ≥2 sessions"))
        );
    }

    #[test]
    fn predict_race_produces_graded_two_method_estimate() {
        let app = Engine;
        let mut model = Model::default();
        assert!(app.view(&model).race_prediction.is_none());

        // 5 km in 20:00, predict a 10 km at ~40 km/wk.
        app.update(
            Event::PredictRace {
                recent_distance_m: 5000.0,
                recent_time_sec: 1200.0,
                goal_distance_m: 10_000.0,
                weekly_km: 40.0,
            },
            &mut model,
        )
        .expect_only_render();

        let pred = app
            .view(&model)
            .race_prediction
            .expect("a prediction must be present after PredictRace");
        assert_eq!(pred.goal_label, "10K");
        // Carries a real grade + citation (evidence discipline).
        assert!(!pred.grade.is_empty() && !pred.citation.is_empty());
        assert_ne!(pred.grade, "MarketingMyth");
        // A 20:00 5K projects a 10K well over 40:00 (both methods, endurance fade).
        assert!(
            pred.low_sec > 2400.0,
            "10K should exceed 40:00, got {}",
            pred.low_sec
        );
        assert!(pred.high_sec >= pred.low_sec);
        assert!(
            pred.predicted.contains(':'),
            "clock-formatted, got {}",
            pred.predicted
        );

        // Clearing drops the section.
        app.update(Event::ClearRacePrediction, &mut model)
            .expect_only_render();
        assert!(app.view(&model).race_prediction.is_none());
    }

    #[test]
    fn predict_race_with_degenerate_input_surfaces_no_false_time() {
        // A zero recent time is undefined for both Riegel and VDOT; the view must
        // say so rather than fabricate a finish clock (evidence discipline).
        let app = Engine;
        let mut model = Model::default();
        app.update(
            Event::PredictRace {
                recent_distance_m: 5000.0,
                recent_time_sec: 0.0,
                goal_distance_m: 10_000.0,
                weekly_km: 40.0,
            },
            &mut model,
        )
        .expect_only_render();

        let pred = app
            .view(&model)
            .race_prediction
            .expect("the section is present, just with no numeric estimate");
        assert_eq!(pred.predicted, "-");
        assert_eq!(pred.low_sec, 0.0);
        assert!(!pred.agreed);
        assert!(
            pred.summary.contains("need a valid recent race"),
            "degenerate summary should explain the missing input, got {}",
            pred.summary
        );
        assert!(
            !pred.predicted.contains(':'),
            "must not render a clock time for undefined input"
        );
    }

    #[test]
    fn plan_hypertrophy_meso_produces_graded_rows() {
        let app = Engine;
        let mut model = Model::default();
        assert!(app.view(&model).hypertrophy_plan.is_empty());

        // A known muscle + 4 accumulation weeks yields several graded rows.
        app.update(
            Event::PlanHypertrophyMeso {
                muscle: "chest".into(),
                weeks: 4,
            },
            &mut model,
        )
        .expect_only_render();

        let plan = app.view(&model).hypertrophy_plan;
        assert!(plan.len() >= 3, "expected several rows, got {}", plan.len());
        // Every row is graded and never a hard-blocked myth (evidence discipline).
        for row in &plan {
            assert!(!row.grade.is_empty() && !row.citation.is_empty());
            assert_ne!(row.grade, "MarketingMyth");
        }
        // The volume-ramp row is present (chest MEV 10 → MRV 22).
        assert!(
            plan.iter().any(|r| r.summary.contains("Weekly set ramp:")),
            "ramp row missing"
        );

        // Clearing empties the section.
        app.update(Event::ClearHypertrophyPlan, &mut model)
            .expect_only_render();
        assert!(app.view(&model).hypertrophy_plan.is_empty());

        // An unknown muscle yields a single explanatory row (no fake data).
        app.update(
            Event::PlanHypertrophyMeso {
                muscle: "tail".into(),
                weeks: 4,
            },
            &mut model,
        )
        .expect_only_render();
        let unknown = app.view(&model).hypertrophy_plan;
        assert_eq!(unknown.len(), 1, "unknown muscle should be one row");
        assert!(unknown[0].summary.contains("not a known muscle"));

        // weeks == 0 is degenerate: show the landmarks context only, not an
        // empty ramp/RIR schedule.
        app.update(
            Event::PlanHypertrophyMeso {
                muscle: "chest".into(),
                weeks: 0,
            },
            &mut model,
        )
        .expect_only_render();
        let zero = app.view(&model).hypertrophy_plan;
        assert_eq!(zero.len(), 1, "zero-week plan is landmarks-only");
        assert!(zero[0].summary.contains("Landmarks"));
        assert!(
            !zero.iter().any(|r| r.summary.contains("ramp")),
            "no ramp row without a week count"
        );
    }

    #[test]
    fn compute_protein_scales_graded_targets_by_bodyweight() {
        let app = Engine;
        let mut model = Model::default();
        assert!(app.view(&model).protein_targets.is_empty());

        // Deficit context at 80 kg: 1.8–2.7 g/kg → 144–216 g/day, still graded.
        app.update(
            Event::ComputeProtein {
                bodyweight_kg: 80.0,
                masters: false,
                deficit: true,
            },
            &mut model,
        )
        .expect_only_render();

        let targets = app.view(&model).protein_targets;
        assert_eq!(targets.len(), 1, "one selected context → one row");
        let row = &targets[0];
        assert!(
            row.summary.contains("g/day"),
            "must state an absolute target"
        );
        assert!(
            !row.grade.is_empty() && row.grade != "MarketingMyth",
            "row must carry real evidence"
        );
        // The absolute bounds scale from bodyweight × each graded g/kg bound.
        assert!(
            row.summary.contains("144") && row.summary.contains("216"),
            "80 kg × (1.8, 2.7) should surface 144–216 g/day, got {}",
            row.summary
        );

        // Neither context selected → no fabricated general target (HARD RULE 1).
        app.update(
            Event::ComputeProtein {
                bodyweight_kg: 80.0,
                masters: false,
                deficit: false,
            },
            &mut model,
        )
        .expect_only_render();
        assert!(app.view(&model).protein_targets.is_empty());

        // Clearing empties the section.
        app.update(
            Event::ComputeProtein {
                bodyweight_kg: 80.0,
                masters: true,
                deficit: false,
            },
            &mut model,
        )
        .expect_only_render();
        assert!(!app.view(&model).protein_targets.is_empty());
        app.update(Event::ClearProtein, &mut model)
            .expect_only_render();
        assert!(app.view(&model).protein_targets.is_empty());
    }

    #[test]
    fn compute_protein_handles_both_flags_and_rejects_bad_weight() {
        let app = Engine;
        let mut model = Model::default();

        // Both contexts selected → one graded row each (masters + deficit).
        app.update(
            Event::ComputeProtein {
                bodyweight_kg: 70.0,
                masters: true,
                deficit: true,
            },
            &mut model,
        )
        .expect_only_render();
        let targets = app.view(&model).protein_targets;
        assert_eq!(targets.len(), 2, "both contexts → two rows");
        assert!(targets.iter().all(|r| r.summary.contains("g/day")));
        assert!(
            targets.iter().all(|r| !r.grade.is_empty()),
            "every context row stays evidence-graded"
        );

        // A non-positive bodyweight cannot yield a g/day figure: emit nothing
        // rather than a zero or nonsensical target.
        app.update(
            Event::ComputeProtein {
                bodyweight_kg: 0.0,
                masters: true,
                deficit: true,
            },
            &mut model,
        )
        .expect_only_render();
        assert!(
            app.view(&model).protein_targets.is_empty(),
            "non-positive bodyweight must surface no target"
        );
    }

    #[test]
    fn compute_hr_zones_builds_graded_hrmax_and_five_bands() {
        let app = Engine;
        let mut model = Model::default();
        assert!(app.view(&model).hr_zones.is_empty());

        app.update(Event::ComputeHrZones { age_years: 30.0 }, &mut model)
            .expect_only_render();

        let zones = app.view(&model).hr_zones;
        // HRmax header + 5 Daniels bands.
        assert_eq!(zones.len(), 6, "one HRmax row + five band rows");

        // Tanaka at 30: 208 − 0.7·30 = 187 bpm.
        let hrmax_row = &zones[0];
        assert!(
            hrmax_row.summary.contains("187 bpm"),
            "{}",
            hrmax_row.summary
        );

        // Easy band is 65–79 %HRmax → 122–148 bpm at HRmax 187.
        let easy = zones
            .iter()
            .find(|z| z.summary.starts_with("Easy"))
            .expect("Easy band row present");
        assert!(easy.summary.contains("122–148 bpm"), "{}", easy.summary);

        // Every row is evidence-graded, never a myth (HARD RULE 2).
        for z in &zones {
            assert!(!z.grade.is_empty() && !z.citation.is_empty());
            assert_ne!(z.grade, "MarketingMyth");
        }

        // Clear empties the section.
        app.update(Event::ClearHrZones, &mut model)
            .expect_only_render();
        assert!(app.view(&model).hr_zones.is_empty());
    }

    #[test]
    fn compute_hr_zones_rejects_implausible_age() {
        let app = Engine;
        let mut model = Model::default();
        app.update(Event::ComputeHrZones { age_years: 0.0 }, &mut model)
            .expect_only_render();
        let zones = app.view(&model).hr_zones;
        assert_eq!(zones.len(), 1, "bad age yields a single explanatory row");
        assert!(zones[0].summary.contains("between 5 and 100"));
    }

    #[test]
    fn high_mileage_profile_surfaces_safety_critical_bsi_surveillance() {
        let app = Engine;
        let mut model = Model::default();
        let mut p = sample_profile();
        p.running_km_per_week = 70.0; // over the ~64 km bone-stress threshold
        app.update(Event::SetProfile(p), &mut model)
            .expect_only_render();

        let bsi = app
            .view(&model)
            .guidance
            .into_iter()
            .find(|g| g.summary.contains("Bone-stress-injury"))
            .expect("70 km/wk must raise BSI surveillance");
        // File 10 hybrid-023 is safety-critical and evidence-cited (HARD RULE 2/3).
        assert!(bsi.safety_critical);
        assert_eq!(bsi.section, "Safety");
        assert!(!bsi.grade.is_empty() && !bsi.citation.is_empty());
    }

    #[test]
    fn novice_load_jump_shown_only_for_novices() {
        let has_jump = |cadence: ProgressionCadence| -> bool {
            let mut p = sample_profile();
            p.progression_cadence = cadence;
            build_guidance(&p)
                .iter()
                .any(|g| g.summary.contains("Novice load jump"))
        };
        assert!(has_jump(ProgressionCadence::EverySession)); // Novice
        assert!(!has_jump(ProgressionCadence::WeekToWeek)); // Intermediate
        assert!(!has_jump(ProgressionCadence::MonthToMonth)); // Advanced
    }

    #[test]
    fn weekly_pct_increment_shown_only_past_novice() {
        // The complement of the novice fixed-kg jump: intermediate/advanced
        // lifters get the per-week percentage progression instead, so this gap
        // (previously no increment guidance at all for them) is now filled.
        let row = |cadence: ProgressionCadence| -> Option<GuidanceView> {
            let mut p = sample_profile();
            p.progression_cadence = cadence;
            build_guidance(&p)
                .into_iter()
                .find(|g| g.summary.contains("Weekly load increment"))
        };
        assert!(row(ProgressionCadence::EverySession).is_none()); // Novice
        for cadence in [
            ProgressionCadence::WeekToWeek,
            ProgressionCadence::MonthToMonth,
        ] {
            let r = row(cadence).expect("non-novice should get weekly % increment");
            assert!(
                !r.citation.is_empty(),
                "increment row must be evidence-cited"
            );
            assert_ne!(r.grade, "MarketingMyth");
            // Half-percent bounds must render faithfully, not round to a whole
            // number that understates the cited band (upper 1-2.5%, lower 2.5-5%).
            assert!(
                r.summary.contains("+1-2.5% upper"),
                "upper band must keep its half-percent: {}",
                r.summary
            );
            assert!(
                r.summary.contains("+2.5-5% lower"),
                "lower band must keep its half-percent: {}",
                r.summary
            );
        }
    }

    #[test]
    fn hypertrophy_rep_load_prescription_is_surfaced_and_cited() {
        // The rep/load row answers "how many reps and what load": absent before
        // this wire. Profile-independent (applies to all lifting), so any profile
        // shows it, evidence-cited and non-myth, with the heavy-compound band.
        let r = build_guidance(&sample_profile())
            .into_iter()
            .find(|g| g.summary.starts_with("Rep/load:"))
            .expect("rep/load prescription row must be present");
        assert!(
            !r.citation.is_empty(),
            "rep/load row must be evidence-cited"
        );
        assert_ne!(r.grade, "MarketingMyth");
        assert!(
            r.summary.contains("heavy compound 5-10 @75-85%1RM"),
            "heavy band must render: {}",
            r.summary
        );
    }

    #[test]
    fn build_guidance_never_panics_across_the_profile_domain() {
        // Exhaustive smoke test: build_guidance branches heavily on the profile
        // and does arithmetic on its integer fields (weekly_sets, running days,
        // km). Sweep every enum combination against the boundary integers the
        // Android profile editor can emit (weekly_sets 0..=30, days 0..=7, km
        // 0..=150) and assert (a) no panic (underflow / div-by-zero) and (b) the
        // HARD RULE 2 invariant holds everywhere: every surfaced row is
        // evidence-cited and none is graded MarketingMyth.
        let cadences = [
            ProgressionCadence::EverySession,
            ProgressionCadence::WeekToWeek,
            ProgressionCadence::MonthToMonth,
        ];
        let lift_goals = [
            LiftGoal::MaxStrength,
            LiftGoal::Power,
            LiftGoal::Hypertrophy,
        ];
        let distances = [
            GoalDistance::General,
            GoalDistance::C25k,
            GoalDistance::FiveK,
            GoalDistance::TenK,
            GoalDistance::HalfMarathon,
            GoalDistance::Marathon,
        ];
        let concurrent = [
            ConcurrentGoal::Strength,
            ConcurrentGoal::Power,
            ConcurrentGoal::Hypertrophy,
            ConcurrentGoal::EndurancePriority,
        ];
        for pc in cadences {
            for lg in lift_goals {
                for gd in distances {
                    for cg in concurrent {
                        for weekly_sets in [0u8, 1, 30] {
                            for running_days_per_week in [0u8, 1, 7] {
                                for running_km_per_week in [0.0, 0.1, 150.0] {
                                    for advanced in [false, true] {
                                        let p = Profile {
                                            progression_cadence: pc,
                                            lift_goal: lg,
                                            goal_distance: gd,
                                            concurrent_goal: cg,
                                            weekly_sets,
                                            running_days_per_week,
                                            running_km_per_week,
                                            advanced,
                                            endurance_intensity_pct_vo2max: 75.0,
                                        };
                                        let rows = build_guidance(&p);
                                        for row in &rows {
                                            assert!(
                                                !row.citation.is_empty(),
                                                "uncited row for {p:?}: {}",
                                                row.summary
                                            );
                                            assert_ne!(
                                                row.grade, "MarketingMyth",
                                                "myth surfaced for {p:?}: {}",
                                                row.summary
                                            );
                                        }
                                        // Sections must be contiguous: the shell
                                        // suppresses a repeated section header
                                        // only when the previous row shares the
                                        // section, so a section that reappears
                                        // after a gap would render a duplicate
                                        // header. Once a section run closes it
                                        // must never return.
                                        let mut closed: Vec<&str> = Vec::new();
                                        let mut prev: Option<&str> = None;
                                        for row in &rows {
                                            let s = row.section.as_str();
                                            if prev != Some(s) {
                                                assert!(
                                                    !closed.contains(&s),
                                                    "section {s:?} is non-contiguous for {p:?}"
                                                );
                                                if let Some(p) = prev {
                                                    closed.push(p);
                                                }
                                                prev = Some(s);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn build_reference_sections_are_contiguous_and_cited() {
        // build_reference is profile-independent, so one call covers its domain.
        // Same contract as the guidance list: every row evidence-cited, no
        // MarketingMyth, and each section forms a single contiguous run (a
        // section that reappears after a gap would read as an orphan card, since
        // the reference list labels every card by section).
        let rows = build_reference();
        for row in &rows {
            assert!(
                !row.citation.is_empty(),
                "uncited reference row: {}",
                row.summary
            );
            assert_ne!(
                row.grade, "MarketingMyth",
                "myth in reference: {}",
                row.summary
            );
        }
        let mut closed: Vec<&str> = Vec::new();
        let mut prev: Option<&str> = None;
        for row in &rows {
            let s = row.section.as_str();
            if prev != Some(s) {
                assert!(
                    !closed.contains(&s),
                    "reference section {s:?} is non-contiguous"
                );
                if let Some(p) = prev {
                    closed.push(p);
                }
                prev = Some(s);
            }
        }

        // Each newly-surfaced dormant calc must appear as its own card with a
        // non-empty citation. Match on a load-bearing substring of each summary.
        let card = |needle: &str| rows.iter().find(|r| r.summary.contains(needle));
        for needle in [
            "Olympic pulling derivatives",
            "RPE-anchored: top set",
            "Two-for-two rule",
            "Stall: if e1RM",
            "Double progression",
            "Masters (65+) protein",
            "Deficit protein",
            "Masters (65+) per-meal protein",
            "Couch-to-5K",
            "Pyramidal split",
            "Polarized split",
            "Maintaining a quality",
            "Joint pain at heavy load",
            "Scale DOWN",
            "Scale UP",
            "Home / minimal equipment",
            "Goals are framed as controllable",
        ] {
            let row =
                card(needle).unwrap_or_else(|| panic!("missing reference card for {needle:?}"));
            assert!(
                !row.citation.is_empty(),
                "uncited reference card: {}",
                row.summary
            );
        }
    }

    #[test]
    fn quality_caps_shown_only_when_the_athlete_runs() {
        let has_caps = |run_days: u8| -> bool {
            let mut p = sample_profile();
            p.running_days_per_week = run_days;
            build_guidance(&p)
                .iter()
                .any(|g| g.summary.contains("Quality-session caps"))
        };
        assert!(has_caps(5));
        assert!(!has_caps(0));
    }

    #[test]
    fn periodization_phase_plan_tracks_the_model_by_training_age() {
        // Cadence maps to training age → periodization model: EverySession→Novice
        // →Linear; MonthToMonth→Advanced→Block; WeekToWeek→Intermediate→DUP
        // (no phase table in the source).
        let summaries = |cadence: ProgressionCadence| -> Vec<String> {
            let mut p = sample_profile();
            p.progression_cadence = cadence;
            build_guidance(&p).into_iter().map(|g| g.summary).collect()
        };

        let linear = summaries(ProgressionCadence::EverySession);
        assert!(linear.iter().any(|s| s.contains("Linear Base: 67-75%1RM")));
        assert!(
            linear
                .iter()
                .any(|s| s.contains("Linear Taper: maintain intensity"))
        );
        assert!(!linear.iter().any(|s| s.starts_with("Block ")));

        let block = summaries(ProgressionCadence::MonthToMonth);
        assert!(
            block
                .iter()
                .any(|s| s.contains("Block Accumulation: 65-80%1RM"))
        );
        assert!(
            block
                .iter()
                .any(|s| s.contains("Block Realization: 1-3 reps"))
        );
        assert!(!block.iter().any(|s| s.starts_with("Linear ")));

        let dup = summaries(ProgressionCadence::WeekToWeek);
        assert!(
            !dup.iter()
                .any(|s| s.starts_with("Linear ") || s.starts_with("Block "))
        );
    }

    #[test]
    fn peak_block_run_cap_shows_only_for_maximal_lift_goals() {
        let app = Engine;
        let has_peak_row = |goal: LiftGoal| {
            let mut model = Model::default();
            let mut p = sample_profile();
            p.lift_goal = goal;
            app.update(Event::SetProfile(p), &mut model)
                .expect_only_render();
            app.view(&model)
                .guidance
                .iter()
                .any(|g| g.summary.contains("Peak block"))
        };
        // Maximal-quality goals get the File 10 CAP-2 running override…
        assert!(has_peak_row(LiftGoal::MaxStrength));
        assert!(has_peak_row(LiftGoal::Power));
        // …a hypertrophy goal is not in a peak strength/power block, so no cap.
        assert!(!has_peak_row(LiftGoal::Hypertrophy));
    }

    #[test]
    fn lower_strength_interference_row_tracks_training_age_and_running() {
        let app = Engine;
        let row_text = |cadence: ProgressionCadence, run_days: u8| {
            let mut model = Model::default();
            let mut p = sample_profile();
            p.progression_cadence = cadence;
            p.running_days_per_week = run_days;
            app.update(Event::SetProfile(p), &mut model)
                .expect_only_render();
            app.view(&model)
                .guidance
                .into_iter()
                .find(|g| g.summary.contains("Lower-body strength interference"))
                .map(|g| g.summary)
        };
        // Trained lifter who runs → susceptible.
        assert!(
            row_text(ProgressionCadence::WeekToWeek, 5)
                .expect("trained runner shows the row")
                .contains("yes")
        );
        // Novice who runs → spared.
        assert!(
            row_text(ProgressionCadence::EverySession, 5)
                .expect("novice runner shows the row")
                .contains("no")
        );
        // Pure lifter (no running) → row absent entirely.
        assert!(row_text(ProgressionCadence::WeekToWeek, 0).is_none());
    }

    #[test]
    fn newly_wired_guidance_rows_are_present_and_cited() {
        let app = Engine;
        let mut model = Model::default();
        app.update(Event::SetProfile(sample_profile()), &mut model)
            .expect_only_render();
        let vm = app.view(&model);

        // Each wired dormant calc must surface a row, and every row still carries
        // an evidence grade + citation (HARD RULE 2, no bypasses).
        for needle in [
            "MEV for",
            "High-volume sensitivity",
            "Deload cadence",
            "Base-phase intensity",
            "Prilepin @~",
            "Weekly volume increase cap",
        ] {
            let row = vm
                .guidance
                .iter()
                .find(|g| g.summary.contains(needle))
                .unwrap_or_else(|| panic!("missing guidance row: {needle}"));
            assert!(!row.grade.is_empty() && !row.citation.is_empty());
            assert_ne!(row.grade, "MarketingMyth");
        }

        // The Prilepin row must resolve to its registered claim (Moderate grade,
        // Prilepin citation), not the unregistered-id fallback.
        let prilepin = vm
            .guidance
            .iter()
            .find(|g| g.summary.contains("Prilepin @~"))
            .unwrap();
        assert_eq!(prilepin.grade, "Moderate");
        assert!(
            prilepin.citation.contains("Prilepin"),
            "{}",
            prilepin.citation
        );
        assert!(prilepin.contested, "STR-PRILEPIN-001 is contested (CQ-03)");

        // WeekToWeek cadence → Intermediate age → not conservative → 3:1 cadence.
        assert!(
            vm.guidance
                .iter()
                .any(|g| g.summary.contains("Deload cadence: 3:1"))
        );
    }

    #[test]
    fn novice_profile_gets_conservative_running_deload() {
        let app = Engine;
        let mut model = Model::default();
        let mut p = sample_profile();
        p.progression_cadence = ProgressionCadence::EverySession; // → Novice
        app.update(Event::SetProfile(p), &mut model)
            .expect_only_render();
        let vm = app.view(&model);
        assert!(
            vm.guidance
                .iter()
                .any(|g| g.summary.contains("Deload cadence: 2:1")),
            "novice (low training age) should get the conservative 2:1 cadence"
        );
    }

    #[test]
    fn power_goal_surfaces_cited_plyo_cap() {
        let app = Engine;
        let mut model = Model::default();
        let mut p = sample_profile();
        p.lift_goal = LiftGoal::Power;
        app.update(Event::SetProfile(p), &mut model)
            .expect_only_render();
        let vm = app.view(&model);
        let plyo = vm
            .guidance
            .iter()
            .find(|g| g.summary.contains("Plyo foot-contact cap"))
            .expect("Power goal should surface the plyometric foot-contact cap row");
        assert!(!plyo.citation.is_empty());
    }

    #[test]
    fn non_power_goal_omits_plyo_cap() {
        let app = Engine;
        let mut model = Model::default();
        app.update(Event::SetProfile(sample_profile()), &mut model)
            .expect_only_render();
        assert!(
            !app.view(&model)
                .guidance
                .iter()
                .any(|g| g.summary.contains("Plyo foot-contact cap")),
            "MaxStrength goal should not surface the plyo cap"
        );
    }

    #[test]
    fn clearing_profile_empties_guidance() {
        let app = Engine;
        let mut model = Model::default();
        app.update(Event::SetProfile(sample_profile()), &mut model)
            .expect_only_render();
        app.update(Event::ClearProfile, &mut model)
            .expect_only_render();
        assert!(app.view(&model).guidance.is_empty());
    }

    #[test]
    fn profile_is_echoed_back_for_shell_hydration() {
        let app = Engine;
        let mut model = Model::default();
        assert!(app.view(&model).profile.is_none());

        app.update(Event::SetProfile(sample_profile()), &mut model)
            .expect_only_render();
        assert_eq!(app.view(&model).profile.as_ref(), Some(&sample_profile()));

        app.update(Event::ClearProfile, &mut model)
            .expect_only_render();
        assert!(app.view(&model).profile.is_none());
    }

    #[test]
    fn reference_cards_are_always_present_and_evidence_cited() {
        let app = Engine;
        let model = Model::default();
        let vm = app.view(&model);
        assert!(!vm.reference.is_empty());
        assert!(
            vm.reference
                .iter()
                .all(|g| !g.citation.is_empty() && g.grade != "MarketingMyth")
        );
    }

    #[test]
    fn review_bone_pain_short_circuits_to_injury_concern() {
        let app = Engine;
        let mut model = Model::default();
        let review = SessionReview {
            bone_pain_red_flag: true,
            // Execution says "mastery", but safety must suppress it.
            lift: Some(LiftExec {
                reps_met: true,
                rir_actual: 2,
                rir_target: 2,
            }),
            ..Default::default()
        };
        app.update(Event::SubmitReview(review), &mut model)
            .expect_only_render();

        let fb = app.view(&model).feedback.expect("feedback present");
        assert_eq!(fb.category, "ConcernInjury");
        assert!(fb.suppresses_praise);
        assert!(fb.safety_critical);
        assert!(!fb.message.is_empty());
    }

    #[test]
    fn review_clean_lift_yields_mastery() {
        let app = Engine;
        let mut model = Model::default();
        let review = SessionReview {
            lift: Some(LiftExec {
                reps_met: true,
                rir_actual: 2,
                rir_target: 2,
            }),
            ..Default::default()
        };
        app.update(Event::SubmitReview(review), &mut model)
            .expect_only_render();

        let fb = app.view(&model).feedback.expect("feedback present");
        assert_eq!(fb.category, "PositiveMastery");
        assert!(!fb.suppresses_praise);
    }

    #[test]
    fn review_week_level_deloads_surface_as_adjustments() {
        // Two failed key sessions is a week-level fatigue deload trigger
        // (autoreg-036) carried on the review, not a single-session readiness
        // marker. It must reach the adjustments surface, and, being a volume
        // deload, must not block training.
        let app = Engine;
        let mut model = Model::default();
        let review = SessionReview {
            failed_key_sessions: Some(2),
            ..Default::default()
        };
        app.update(Event::SubmitReview(review), &mut model)
            .expect_only_render();

        let vm = app.view(&model);
        assert!(
            vm.review_adjustments
                .iter()
                .any(|a| a.summary.starts_with("Deload")),
            "a two-failed-session review should surface a deload adjustment"
        );
        assert!(
            vm.adjustments.is_empty(),
            "a review deload must not leak into the readiness-driven list"
        );
        assert!(!vm.train_blocked, "a volume deload must not block training");
    }

    #[test]
    fn review_sub_threshold_deload_counts_add_nothing() {
        // One failed session / an unset velocity drop are below every trigger,
        // so the adjustments surface stays as bare as a default review's.
        let app = Engine;
        let mut model = Model::default();
        let review = SessionReview {
            failed_key_sessions: Some(1),
            rpe_load_gap_sessions: Some(1),
            ..Default::default()
        };
        app.update(Event::SubmitReview(review), &mut model)
            .expect_only_render();

        let vm = app.view(&model);
        assert!(
            vm.review_adjustments.is_empty(),
            "sub-threshold counts must not fabricate a deload"
        );
    }

    #[test]
    fn run_only_review_derives_positive_split_feedback_from_last_track() {
        // A GPS run whose back half is far slower than its front half, then a
        // run-only review (no lift, no explicit split figure). The core should
        // fall back to the track-derived split so pacing feedback still fires.
        let app = Engine;
        let mut model = Model::default();
        let points = vec![
            GpsPoint {
                lat: 0.0,
                lon: 0.000,
                observed_at: 0,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.001,
                observed_at: 20,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.002,
                observed_at: 40,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.003,
                observed_at: 90,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.004,
                observed_at: 140,
                accuracy_m: 5.0,
            },
        ];
        app.update(
            Event::LogRunTrack {
                points,
                hr_pct_max: 70.0,
                longest_recent_km: 12.0,
            },
            &mut model,
        )
        .expect_only_render();
        app.update(Event::SubmitReview(SessionReview::default()), &mut model)
            .expect_only_render();

        let fb = app.view(&model).feedback.expect("feedback present");
        assert_eq!(fb.category, "IntensityDiscipline");
    }

    #[test]
    fn negative_split_note_reads_as_a_magnitude_not_a_signed_number() {
        // A track whose back half is faster than its front half is a negative
        // split. The run summary must phrase it as a positive magnitude
        // ("N% negative split"), not a signed "-N%" that reads awkwardly next to
        // the word "negative".
        let run = LoggedRun {
            distance_km: 0.0,
            duration_min: 0.0,
            hr_pct_max: 70.0,
            longest_recent_km: 12.0,
            track: vec![
                GpsPoint {
                    lat: 0.0,
                    lon: 0.000,
                    observed_at: 0,
                    accuracy_m: 5.0,
                },
                GpsPoint {
                    lat: 0.0,
                    lon: 0.001,
                    observed_at: 50,
                    accuracy_m: 5.0,
                },
                GpsPoint {
                    lat: 0.0,
                    lon: 0.002,
                    observed_at: 100,
                    accuracy_m: 5.0,
                },
                GpsPoint {
                    lat: 0.0,
                    lon: 0.003,
                    observed_at: 120,
                    accuracy_m: 5.0,
                },
                GpsPoint {
                    lat: 0.0,
                    lon: 0.004,
                    observed_at: 140,
                    accuracy_m: 5.0,
                },
            ],
        };
        let view = to_run_view(&run);
        assert!(
            view.split_pct.expect("split derived") < 0.0,
            "back-half-faster track should read as a negative split"
        );
        assert!(view.summary.contains("negative split"));
        assert!(
            !view.summary.contains("-"),
            "note should show magnitude, not a signed number: {}",
            view.summary
        );
    }

    #[test]
    fn positive_split_note_reads_as_a_back_half_slowdown() {
        // A track whose back half is slower than its front half is a positive
        // split. The run summary must call it out as a "+N% back-half slowdown"
        // so the note and the coaching cue (positive_split_discipline) agree.
        let run = LoggedRun {
            distance_km: 0.0,
            duration_min: 0.0,
            hr_pct_max: 70.0,
            longest_recent_km: 12.0,
            // Front half (0 → 0.002° over 40 s) is fast; back half
            // (0.002 → 0.004° over 100 s) is slow, a clear back-half slowdown.
            track: vec![
                GpsPoint {
                    lat: 0.0,
                    lon: 0.000,
                    observed_at: 0,
                    accuracy_m: 5.0,
                },
                GpsPoint {
                    lat: 0.0,
                    lon: 0.001,
                    observed_at: 20,
                    accuracy_m: 5.0,
                },
                GpsPoint {
                    lat: 0.0,
                    lon: 0.002,
                    observed_at: 40,
                    accuracy_m: 5.0,
                },
                GpsPoint {
                    lat: 0.0,
                    lon: 0.003,
                    observed_at: 90,
                    accuracy_m: 5.0,
                },
                GpsPoint {
                    lat: 0.0,
                    lon: 0.004,
                    observed_at: 140,
                    accuracy_m: 5.0,
                },
            ],
        };
        let view = to_run_view(&run);
        assert!(
            view.split_pct.expect("split derived") > feedback::POSITIVE_SPLIT_FLAG_PCT,
            "back-half-slower track should read as a positive split above the flag"
        );
        assert!(
            view.summary.contains("back-half slowdown"),
            "got {}",
            view.summary
        );
        assert!(
            view.summary.contains('+'),
            "positive split note should carry a leading +: {}",
            view.summary
        );
    }

    #[test]
    fn logged_distance_spike_drives_the_safety_gate_without_a_manual_figure() {
        // A logged run 20 % over the recent-longest baseline is the strongest
        // running injury signal (RUN-SPIKE-001). A plain review that carries no
        // explicit spike figure must still trip the safety gate off the logged
        // run, so the deferral is reachable from ordinary logging.
        let app = Engine;
        let mut model = Model::default();
        app.update(
            Event::LogRun {
                distance_km: 12.0,
                duration_min: 60.0,
                hr_pct_max: 70.0,
                longest_recent_km: 10.0,
            },
            &mut model,
        )
        .expect_only_render();
        app.update(Event::SubmitReview(SessionReview::default()), &mut model)
            .expect_only_render();

        let fb = app.view(&model).feedback.expect("feedback present");
        assert_eq!(fb.category, "DangerousProgression");
    }

    #[test]
    fn a_first_run_with_no_baseline_does_not_trip_the_spike_safety_gate() {
        // With no recent-longest baseline the run view flags the distance
        // descriptively, but there is nothing for the run to be a spike *over*,
        // so the safety gate must not defer; otherwise every first-ever run
        // would raise a dangerous-progression deferral.
        let app = Engine;
        let mut model = Model::default();
        app.update(
            Event::LogRun {
                distance_km: 12.0,
                duration_min: 60.0,
                hr_pct_max: 70.0,
                longest_recent_km: 0.0,
            },
            &mut model,
        )
        .expect_only_render();
        app.update(Event::SubmitReview(SessionReview::default()), &mut model)
            .expect_only_render();

        let fb = app.view(&model).feedback.expect("feedback present");
        assert_ne!(fb.category, "DangerousProgression");
    }

    #[test]
    fn explicit_review_split_wins_over_track_fallback() {
        // GPS run finishes faster than it starts (a negative split), so the
        // track-derived fallback would not fire pacing feedback. An explicit
        // positive figure on the review must still win and fire it, proving the
        // review value takes precedence over the derived fallback.
        let app = Engine;
        let mut model = Model::default();
        let points = vec![
            GpsPoint {
                lat: 0.0,
                lon: 0.000,
                observed_at: 0,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.001,
                observed_at: 50,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.002,
                observed_at: 100,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.003,
                observed_at: 120,
                accuracy_m: 5.0,
            },
            GpsPoint {
                lat: 0.0,
                lon: 0.004,
                observed_at: 140,
                accuracy_m: 5.0,
            },
        ];
        app.update(
            Event::LogRunTrack {
                points,
                hr_pct_max: 70.0,
                longest_recent_km: 12.0,
            },
            &mut model,
        )
        .expect_only_render();
        let review = SessionReview {
            positive_split_pct: Some(10.0),
            ..Default::default()
        };
        app.update(Event::SubmitReview(review), &mut model)
            .expect_only_render();

        let fb = app.view(&model).feedback.expect("feedback present");
        assert_eq!(fb.category, "IntensityDiscipline");
    }

    // The Android shell hand-builds SubmitReview JSON (Core.kt), omitting the
    // Option fields it does not set. These two tests pin the exact wire form the
    // shell emits for the decoupling / easy-run-intensity review context, so a
    // rename on either side of the FFI fails here rather than silently dropping
    // the field to None.
    #[test]
    fn decoupling_review_wire_json_drives_feedback() {
        let wire = r#"{"SubmitReview":{"bone_pain_red_flag":false,"compulsive_flag":false,"overtraining_signal_count":0,"decoupling":{"drift_pct":12.0,"cool_steady_context":true},"bad_day":false}}"#;
        let event: Event = serde_json::from_str(wire).expect("shell wire form parses");

        let app = Engine;
        let mut model = Model::default();
        app.update(event, &mut model).expect_only_render();

        let fb = app.view(&model).feedback.expect("feedback present");
        assert_eq!(fb.category, "CorrectiveProcess");
    }

    #[test]
    fn easy_run_intensity_wire_json_drives_feedback() {
        let wire = r#"{"SubmitReview":{"bone_pain_red_flag":false,"compulsive_flag":false,"overtraining_signal_count":0,"easy_frac_time_above_vt1":0.3,"bad_day":false}}"#;
        let event: Event = serde_json::from_str(wire).expect("shell wire form parses");

        let app = Engine;
        let mut model = Model::default();
        app.update(event, &mut model).expect_only_render();

        let fb = app.view(&model).feedback.expect("feedback present");
        assert_eq!(fb.category, "IntensityDiscipline");
    }

    #[test]
    fn clearing_review_empties_feedback() {
        let app = Engine;
        let mut model = Model::default();
        app.update(Event::SubmitReview(SessionReview::default()), &mut model)
            .expect_only_render();
        assert!(app.view(&model).feedback.is_some());
        app.update(Event::ClearReview, &mut model)
            .expect_only_render();
        assert!(app.view(&model).feedback.is_none());
    }

    #[test]
    fn clearing_review_also_drops_its_week_deloads() {
        // review_adjustments share the review's lifecycle, so ClearReview must
        // take both the feedback and the deload cards, and ClearReadiness, which
        // owns the separate readiness list, must leave them untouched.
        let app = Engine;
        let mut model = Model::default();
        let review = SessionReview {
            failed_key_sessions: Some(2),
            ..Default::default()
        };
        app.update(Event::SubmitReview(review), &mut model)
            .expect_only_render();
        assert!(!app.view(&model).review_adjustments.is_empty());

        app.update(Event::ClearReadiness, &mut model)
            .expect_only_render();
        assert!(
            !app.view(&model).review_adjustments.is_empty(),
            "clearing readiness must not touch review-owned deloads"
        );

        app.update(Event::ClearReview, &mut model)
            .expect_only_render();
        assert!(
            app.view(&model).review_adjustments.is_empty(),
            "clearing the review must drop its deloads"
        );
    }

    #[test]
    fn clear_resets_inputs() {
        let app = Engine;
        let mut model = Model::default();

        app.update(
            Event::SubmitReadiness(input(ReadinessSignal::Pain, 1.0)),
            &mut model,
        )
        .expect_only_render();
        app.update(Event::ClearReadiness, &mut model)
            .expect_only_render();

        let vm = app.view(&model);
        assert_eq!(vm.input_count, 0);
        assert!(!vm.train_blocked);
    }

    #[test]
    fn clearing_sets_and_runs_empties_their_views() {
        let app = Engine;
        let mut model = Model::default();

        app.update(
            Event::LogSet {
                exercise: "Back Squat".into(),
                weight_kg: 140.0,
                reps: 5,
                rpe: 8.0,
            },
            &mut model,
        )
        .expect_only_render();
        app.update(
            Event::LogRun {
                distance_km: 10.0,
                duration_min: 50.0,
                hr_pct_max: 70.0,
                longest_recent_km: 12.0,
            },
            &mut model,
        )
        .expect_only_render();
        assert_eq!(app.view(&model).lifts.len(), 1);
        assert_eq!(app.view(&model).runs.len(), 1);

        app.update(Event::ClearSets, &mut model)
            .expect_only_render();
        assert!(app.view(&model).lifts.is_empty());
        assert_eq!(
            app.view(&model).runs.len(),
            1,
            "clearing sets must not touch runs"
        );

        app.update(Event::ClearRuns, &mut model)
            .expect_only_render();
        assert!(app.view(&model).runs.is_empty());
    }
}
