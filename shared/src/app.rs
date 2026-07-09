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

use crate::schema::{Adjustment, ReadinessInput, Recommended};
use crate::{autoreg, running, strength};

#[derive(Clone)]
struct LoggedSet {
    exercise: String,
    weight_kg: f64,
    reps: u32,
    rpe: f64,
}

#[derive(Clone)]
struct LoggedRun {
    distance_km: f64,
    duration_min: f64,
    hr_pct_max: f64,
    longest_recent_km: f64,
}

#[derive(Default)]
pub struct Model {
    /// Observed readiness signals, in submission order.
    inputs: Vec<ReadinessInput>,
    /// Logged lift sets, in submission order.
    sets: Vec<LoggedSet>,
    /// Logged runs, in submission order.
    runs: Vec<LoggedRun>,
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
    /// Drop all logged runs.
    ClearRuns,
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
    /// Estimated 1RM (Epley), kg, rounded to 0.1.
    pub e1rm_kg: f64,
    /// Reps in reserve implied by the session RPE.
    pub rir: f64,
    pub summary: String,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct ViewModel {
    /// Highest safety tier triggered, e.g. `"Pain"`; `None` when all clear.
    pub safety_tier: Option<String>,
    /// False when a Stop-level or rest-day condition fires, do not train.
    pub train_blocked: bool,
    pub adjustments: Vec<AdjustmentView>,
    pub input_count: usize,
    pub lifts: Vec<LiftResultView>,
    pub runs: Vec<RunResultView>,
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
    pub summary: String,
    /// Evidence backing the spike gate.
    pub citation: String,
}

#[effect]
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
            } => model.runs.push(LoggedRun {
                distance_km,
                duration_min,
                hr_pct_max,
                longest_recent_km,
            }),
            Event::ClearRuns => model.runs.clear(),
        }
        render()
    }

    fn view(&self, model: &Self::Model) -> Self::ViewModel {
        let recommended = autoreg::adjustments(&model.inputs);

        let train_blocked = recommended
            .iter()
            .any(|r| matches!(r.value, Adjustment::Stop | Adjustment::RestDay | Adjustment::Defer { .. }));

        ViewModel {
            safety_tier: autoreg::resolve_safety(&model.inputs).map(|t| format!("{t:?}")),
            train_blocked,
            adjustments: recommended.iter().map(to_view).collect(),
            input_count: model.inputs.len(),
            lifts: model.sets.iter().map(to_lift_view).collect(),
            runs: model.runs.iter().map(to_run_view).collect(),
        }
    }
}

/// Derive zone + pace + distance-spike flag for one logged run.
fn to_run_view(r: &LoggedRun) -> RunResultView {
    let zone = running::classify_three_zone(r.hr_pct_max);
    let spike = running::single_session_spike_flag(r.distance_km, r.longest_recent_km);

    let pace = if r.distance_km > 0.0 {
        let sec_per_km = (r.duration_min * 60.0) / r.distance_km;
        format!("{}:{:02}/km", (sec_per_km as u32) / 60, (sec_per_km as u32) % 60)
    } else {
        "-".to_string()
    };

    RunResultView {
        zone: format!("{zone:?}"),
        pace: pace.clone(),
        spike_flag: spike.value,
        summary: format!(
            "{:.1}km @ {} ({zone:?}){}",
            r.distance_km,
            pace,
            if spike.value { " - distance spike >10%" } else { "" }
        ),
        citation: spike.evidence.citation.reference.clone(),
    }
}

/// Derive strength metrics for one logged set (Epley e1RM, RIR from RPE).
fn to_lift_view(s: &LoggedSet) -> LiftResultView {
    let e1rm_kg = (strength::e1rm_epley(s.weight_kg, s.reps) * 10.0).round() / 10.0;
    let rir = strength::rpe_to_rir(s.rpe);
    LiftResultView {
        exercise: s.exercise.clone(),
        e1rm_kg,
        rir,
        summary: format!(
            "{} {:.0}kg × {} @RPE{:.1} → e1RM {:.1}kg ({:.0} RIR)",
            s.exercise, s.weight_kg, s.reps, s.rpe, e1rm_kg, rir
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

    #[test]
    fn pain_blocks_training_with_a_single_stop() {
        let app = Engine;
        let mut model = Model::default();

        app.update(Event::SubmitReadiness(input(ReadinessSignal::Pain, 1.0)), &mut model)
            .expect_only_render();

        let vm = app.view(&model);
        assert_eq!(vm.safety_tier.as_deref(), Some("Pain"));
        assert!(vm.train_blocked);
        assert_eq!(vm.adjustments.len(), 1);
        assert_eq!(vm.adjustments[0].summary, "Stop - do not train");
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
        // RPE 8 → 2 RIR.
        assert!((vm.lifts[0].rir - 2.0).abs() < f64::EPSILON);
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
        assert!(!vm.runs[0].zone.is_empty());
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
    fn clear_resets_inputs() {
        let app = Engine;
        let mut model = Model::default();

        app.update(Event::SubmitReadiness(input(ReadinessSignal::Pain, 1.0)), &mut model)
            .expect_only_render();
        app.update(Event::ClearReadiness, &mut model)
            .expect_only_render();

        let vm = app.view(&model);
        assert_eq!(vm.input_count, 0);
        assert!(!vm.train_blocked);
    }
}
