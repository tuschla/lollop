//! The athlete's programming profile and its wire sub-types: the
//! shell-supplied input every plan, guidance and gate reads, plus
//! [`PlanProfile`], the facts derived from it once.

use serde::{Deserialize, Serialize};

use crate::hybrid::{self, ConcurrentGoal};
use crate::individualization::{
    Environment, ProgressionCadence, TrainingAge, conservative_deload, training_age_from_cadence,
};
use crate::running::{self, GoalDistance};
use crate::sanitize::Sanitize;
use crate::schema::{HealthScreen, Recommended};
use crate::strength::LiftGoal;

/// How the athlete trains resistance: external loads (barbell/dumbbell) or
/// bodyweight leverage (calisthenics). A calisthenics user's plan is built from
/// leverage ladders instead of %1RM; they may still log arbitrary barbell sets
/// (those stay plain logged lifts). Default `Weights`.
#[derive(
    Serialize, Deserialize, crate::wire::Wire, Debug, Clone, Copy, PartialEq, Eq, Default, Sanitize,
)]
pub enum ResistanceModality {
    #[default]
    Weights,
    Calisthenics,
}

/// The athlete's programming profile: drives the evidence-cited guidance
/// section. Shell-supplied; no logged data required.
///
/// Serialized it is the view's `profile` echo; as `SetProfile`'s payload the
/// shell writes the required eight first and omits every field at its
/// default, so an unanswered profile keeps its original eight-field line.
#[derive(Serialize, Deserialize, crate::wire::Wire, Debug, Clone, PartialEq, Sanitize)]
#[wire(
    payload_omits_defaults,
    payload_order(
        progression_cadence,
        lift_goal,
        goal_distance,
        concurrent_goal,
        weekly_sets,
        running_days_per_week,
        running_km_per_week,
        advanced,
        running_km_per_week_set,
        female,
        bodyweight_kg,
        age_years,
        resting_hr_bpm,
        measured_hr_max,
        weight_lb,
        resistance_modality,
        high_load_block,
        environment,
        env_temp_c,
        env_altitude_m,
        weeks_off,
        lift_days_per_week,
        training_weekdays,
        goal_race_epoch_day,
        recent_race,
        stated_longest_recent_km,
        layoff_returned_epoch_day,
        meet_epoch_day,
        calis_equipment,
        specialization_muscle,
        runner_strength,
        effective_from_epoch_day,
        birth_epoch_day,
        edited_at,
        health,
    )
)]
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
    /// Running km per week (hybrid lower-lift cap trigger). Always 0 when
    /// `running_km_per_week_set` is false.
    #[sanitize_with = "crate::sanitize::non_negative"]
    pub running_km_per_week: f64,
    /// Whether the athlete actually answered the weekly-distance question.
    /// `false` = "not set": the core zeroes `running_km_per_week` and the plan
    /// sizes from logged runs, else the KB session-duration floors, never from
    /// an invented figure. Missing on the wire = `true`.
    #[serde(default = "serde_true")]
    pub running_km_per_week_set: bool,
    /// Advanced marathoner → extended session range.
    pub advanced: bool,
    /// Female user: routes the feedback-035 menstrual/nutrition clinician
    /// prompt on a bone-stress referral and the hybrid-024 energy-availability
    /// risk cohort. `None` reads as not-female.
    pub female: Option<bool>,
    /// Currently inside a high-load/overload mesocycle: arms the autoreg-029
    /// parasympathetic-saturation guard (HRV above the SWC upper band then
    /// blocks auto load-adds).
    #[serde(default)]
    pub high_load_block: bool,
    /// Stage-0 onboarding health screen (File 08 onboard-050). Every flag
    /// defaults to `false` and gates nothing.
    #[serde(default)]
    pub health: HealthScreen,
    /// Training-environment context (File 08 indiv-025 / safety-024): heat,
    /// altitude, or cold arms the environment-modifier guidance. `None` states
    /// nothing and gates nothing.
    pub environment: Option<Environment>,
    /// Typical session temperature, °C, running-041 pace-correction trigger
    /// (heat correction above ~15 °C). `None` = unstated, no row.
    pub env_temp_c: Option<f64>,
    /// Typical session altitude, m, running-041 pace-correction trigger
    /// (altitude correction above ~900 m). `None` = unstated, no row.
    pub env_altitude_m: Option<f64>,
    /// Weeks fully off training when returning from a layoff: arms the
    /// REENTRY-001 resistance re-entry ramp and the hypertrophy-045 reduced
    /// re-entry MEV note. `None` = not returning.
    pub weeks_off: Option<f64>,
    /// Bodyweight, kg (optional): arms the strength-plyo depth-jump
    /// readiness gate (needs squat 1RM ≥ 1.5× bodyweight). `None` = no gate.
    /// Also the consolidated person-data source the protein calculator prefills
    /// from: person data is entered once, on the profile.
    pub bodyweight_kg: Option<f64>,
    /// Age in years (optional). Read by the under-18 gate (`ingest`), the
    /// Tanaka HRmax behind run targets and logged-run %HRmax
    /// (`run_targets::hr_max_anchor`) when no measured max is set, the HR-zone
    /// calculator's age input, and the conservative deload cadence
    /// (`individualization::conservative_deload`). `None` = unstated: no Tanaka estimate,
    /// adult rules. With a `birth_epoch_day` the core owns this value: it is
    /// re-derived from the birthday and the model's today on every
    /// `SetProfile` and `SetToday`; a profile with only an age keeps the
    /// stated value.
    pub age_years: Option<f64>,
    /// Birthday, local epoch day (optional). Source of the derived
    /// `age_years` and the under-18 gate. `None` = only a stated age, or no
    /// age at all.
    pub birth_epoch_day: Option<i64>,
    /// Resting HR, bpm (optional). Sets the Karvonen ceiling of run heart-rate
    /// targets (`run_targets::hr_rest_anchor`) and prefills the HR-zone
    /// calculator's Karvonen input. `None` = unstated.
    pub resting_hr_bpm: Option<f64>,
    /// Measured maximum HR, bpm (optional). When plausible it replaces the
    /// Tanaka age estimate in the HR-zone calculator, run targets and
    /// logged-run %HRmax, and dates the HR-zone re-test reminder. `None` =
    /// unstated.
    pub measured_hr_max: Option<f64>,
    /// Display weights in pounds instead of kilograms. DISPLAY-ONLY: every
    /// NUMERIC wire/view field (`weight_kg`, `e1rm_kg`, `load_kg`, …) stays in
    /// kg regardless; only the human-facing weight strings the shell renders
    /// verbatim convert (kg × `LB_PER_KG`, "lb" suffix, via the `weight_num` /
    /// `weight_suffix` / `fmt_weight` helpers).
    #[serde(default)]
    pub weight_lb: bool,
    /// Resistance-training modality: `Weights` (default) or `Calisthenics`.
    /// A calisthenics profile routes lift days through the leverage-ladder
    /// synthesizer (`plan::calisthenics_session`).
    #[serde(default)]
    pub resistance_modality: ResistanceModality,
    /// The local epoch-day this profile edit takes effect.
    /// The core keeps a `profile_history`; a past/today plan day is prescribed
    /// with the latest profile whose `effective_from_epoch_day <= that day`, so
    /// editing a goal or weekly-set target never re-prescribes an already-shown
    /// past day. `None` = effective from the beginning of time (−∞): a
    /// None-dated edit replaces the whole history, but applies only from its
    /// position in the log to logged runs (it pins the %HRmax of the runs
    /// already logged, `LoggedRun::hr_pinned`). Production shells send the
    /// core's today.
    pub effective_from_epoch_day: Option<i64>,
    /// When the edit was made, unix seconds (the shell's clock). On its
    /// effective day the edit is in force only from this instant, so an
    /// evening edit leaves a run logged that morning on the profile it was
    /// logged under (`app::profile_at`); it also orders same-day edits in the
    /// history. `None` = from the start of that day.
    /// Ignored on an undated edit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited_at: Option<i64>,
    /// Lifting sessions per week the athlete commits to, 2..=5. `None` = the
    /// fixed default lift layout.
    pub lift_days_per_week: Option<u8>,
    /// Days the athlete can train, as a bitmask: bit0 = Monday … bit6 =
    /// Sunday. `None` = the engine's default weekday layout.
    pub training_weekdays: Option<u8>,
    /// Target race date, local epoch day. `None` = no race scheduled.
    pub goal_race_epoch_day: Option<i64>,
    /// A recent race result the athlete reports (distance, finish time,
    /// optional date). `None` = none stated.
    pub recent_race: Option<RecentRace>,
    /// Athlete-stated longest run in the last 30 days, km. Logged runs
    /// supersede it once present. `None` = unstated.
    pub stated_longest_recent_km: Option<f64>,
    /// Local epoch day training resumed after the `weeks_off` layoff. `None`
    /// = unstated (re-entry is then dated from the profile edit).
    pub layoff_returned_epoch_day: Option<i64>,
    /// Strength test / meet date, local epoch day. `None` = no meet scheduled.
    pub meet_epoch_day: Option<i64>,
    /// Calisthenics equipment on hand. `None` = assume every piece is available.
    pub calis_equipment: Option<CalisEquipment>,
    /// Hypertrophy priority muscle key the athlete wants to specialize.
    /// `None` = no specialization.
    pub specialization_muscle: Option<String>,
    /// A pure runner opted into supplemental strength sessions.
    #[serde(default)]
    pub runner_strength: bool,
}

impl Profile {
    /// This edit's position in the profile history: its effective day
    /// (`None` = −∞ as `i64::MIN`), then the instant it was made (`edited_at`;
    /// `None`, or any undated edit, = the start of that day). The history is
    /// kept ascending by this key: an edit supersedes every entry at or after
    /// its own key (an undated one the whole history), so a later same-day edit
    /// leaves the earlier one in force for the part of the day before it.
    pub(crate) fn history_key(&self) -> (i64, i64) {
        match self.effective_from_epoch_day {
            Some(day) => (day, self.edited_at.unwrap_or(i64::MIN)),
            None => (i64::MIN, i64::MIN),
        }
    }

    /// The declared layoff's return day: `None` without a layoff (`weeks_off`
    /// unset or 0), `Some(None)` when it cannot be dated, else the local epoch
    /// day training resumed (`layoff_returned_epoch_day`, else the day this
    /// profile took effect).
    pub fn declared_layoff_return(&self) -> Option<Option<i64>> {
        self.weeks_off.is_some_and(|w| w > 0.0).then_some(
            self.layoff_returned_epoch_day
                .or(self.effective_from_epoch_day),
        )
    }

    /// Resistance is trained on bodyweight leverage ladders (the modality
    /// alone; a calisthenics PLAN also needs lift days,
    /// [`PlanProfile::plans_calisthenics`]).
    pub fn uses_calisthenics(&self) -> bool {
        self.resistance_modality == ResistanceModality::Calisthenics
    }

    /// The chosen training weekdays (bit 0 = Monday … bit 6 = Sunday), `None`
    /// when unset or empty: any day (ADH-DAYS-001, the athlete picks the days).
    pub fn chosen_weekdays(&self) -> Option<u8> {
        self.training_weekdays.map(|m| m & 0x7f).filter(|m| *m != 0)
    }

    /// Whether `age_years` falls under the pediatric gate (HARD RULE 3,
    /// SAFE-PEDS-001): under [`YOUTH_AGE_YEARS`]. The one age rule behind
    /// `health.youth`, whether the age is stated or derived from the birthday.
    pub(crate) fn youth_age(age_years: f64) -> bool {
        age_years < YOUTH_AGE_YEARS
    }
}

/// Age below which the pediatric gate is set (safety-011 names no numeric
/// cutoff for "child/adolescent"; 18 y is the core's choice).
const YOUTH_AGE_YEARS: f64 = 18.0;

/// Serde default for flags that were implicitly on before they existed.
fn serde_true() -> bool {
    true
}

/// A race result the athlete reports on their profile.
#[derive(Serialize, Deserialize, crate::wire::Wire, Debug, Clone, PartialEq, Sanitize)]
pub struct RecentRace {
    /// Race distance, km.
    pub distance_km: f64,
    /// Finish time, seconds.
    pub time_sec: u32,
    /// Race date, local epoch day. `None` = undated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epoch_day: Option<i64>,
}

/// Calisthenics equipment the athlete has access to.
#[derive(
    Serialize, Deserialize, crate::wire::Wire, Debug, Clone, Copy, PartialEq, Eq, Default, Sanitize,
)]
pub struct CalisEquipment {
    pub pull_up_bar: bool,
    pub dip_bars: bool,
    pub rings: bool,
}

/// Why no plan can be built (`ViewModel::plan_withheld`, wire: snake_case).
#[derive(Serialize, Deserialize, crate::wire::Wire, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanWithheld {
    /// A profile health answer holds training (illness, a doctor's OK, age…).
    HealthHold,
    /// The profile plans no lifting sessions and no running days.
    NothingToPlan,
}

/// Which side leads a hybrid week: the athlete's "which matters more" answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HybridLead {
    /// Strength, power or muscle leads; running is kept at a supportive dose.
    Lifting,
    /// Running leads; lifting is kept at a maintenance dose.
    Running,
}

/// How many lift and run days a week holds once fitted into the athlete's
/// available weekdays (one session per day: never doubling up).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeekShape {
    pub lift_days: usize,
    pub run_days: usize,
}

/// The programming facts of one [`Profile`], derived once and handed to the
/// plan, the guidance and the reports, so none of them re-derives "lifts",
/// "runs", the hybrid lead, the training age or the week shape on its own.
/// The raw answers stay reachable through [`PlanProfile::profile`].
#[derive(Debug, Clone)]
pub struct PlanProfile<'a> {
    profile: &'a Profile,
    lifts: bool,
    runs: bool,
    runner_strength: bool,
    hybrid_lead: Option<HybridLead>,
    training_age: Recommended<TrainingAge>,
    weekdays: [bool; 7],
    goal_table_run_days: usize,
    run_slots: usize,
    shape: WeekShape,
    withheld: Option<PlanWithheld>,
}

impl<'a> PlanProfile<'a> {
    pub fn new(profile: &'a Profile) -> Self {
        let lifts = profile.weekly_sets > 0;
        let runs = profile.running_days_per_week > 0;
        let hybrid_lead = (lifts && runs).then_some(match profile.concurrent_goal {
            ConcurrentGoal::EndurancePriority => HybridLead::Running,
            ConcurrentGoal::Strength | ConcurrentGoal::Power | ConcurrentGoal::Hypertrophy => {
                HybridLead::Lifting
            }
        });
        let mask = profile.chosen_weekdays().unwrap_or(0x7f);
        let goal_table_run_days = goal_table_run_days(profile);
        let mut pp = PlanProfile {
            profile,
            lifts,
            runs,
            // RUN-STRENGTH-001: two short heavy sessions a week help runners.
            runner_strength: !lifts && runs && profile.runner_strength,
            hybrid_lead,
            training_age: training_age_from_cadence(profile.progression_cadence),
            weekdays: std::array::from_fn(|d| mask & (1 << d) != 0),
            goal_table_run_days,
            run_slots: run_slot_count(profile, goal_table_run_days, hybrid_lead),
            shape: WeekShape {
                lift_days: 0,
                run_days: 0,
            },
            withheld: if profile.health.any_gate() {
                Some(PlanWithheld::HealthHold)
            } else if !lifts && !runs {
                Some(PlanWithheld::NothingToPlan)
            } else {
                None
            },
        };
        pp.shape = pp.fit_week_shape();
        pp
    }

    /// The raw profile answers.
    pub fn profile(&self) -> &'a Profile {
        self.profile
    }

    /// The profile plans lifting sessions.
    pub fn lifts(&self) -> bool {
        self.lifts
    }

    /// The profile plans running days.
    pub fn runs(&self) -> bool {
        self.runs
    }

    /// A run-only athlete who opted into supplemental strength work
    /// (RUN-STRENGTH-001).
    pub fn runner_strength(&self) -> bool {
        self.runner_strength
    }

    /// A lifting profile that trains bodyweight ladders instead of weights:
    /// its lift days are calisthenics days.
    pub fn plans_calisthenics(&self) -> bool {
        self.lifts && self.profile.uses_calisthenics()
    }

    /// The leading side of a hybrid (lift + run) profile; `None` when the
    /// profile does not both lift and run.
    pub fn hybrid_lead(&self) -> Option<HybridLead> {
        self.hybrid_lead
    }

    /// The training age from the progression cadence (INDIV-TRAGE-001).
    pub fn training_age(&self) -> TrainingAge {
        self.training_age.value
    }

    /// [`PlanProfile::training_age`] with its claim, for a surface that
    /// cites it.
    pub fn training_age_rec(&self) -> &Recommended<TrainingAge> {
        &self.training_age
    }

    /// Weekdays the athlete can train (ADH-DAYS-001), index 0 = Monday.
    pub fn available_weekdays(&self) -> [bool; 7] {
        self.weekdays
    }

    /// The fitted lift/run day counts of a week.
    pub fn shape(&self) -> WeekShape {
        self.shape
    }

    /// The goal table's run days: the stated running-days answer bounded by
    /// the goal's session ceiling (`goal_week_plan`), at least 1.
    pub fn goal_table_run_days(&self) -> usize {
        self.goal_table_run_days
    }

    /// The run days a week schedules before the weekday fit: the goal table,
    /// C25K's three runs and, only while lifting leads a hybrid, the ≤3 d/wk
    /// concurrency cap (HYB-RUNCAP-001). `None` for a profile that does not
    /// run. Shared by the plan layout and the weekly report so the two agree
    /// on the run-day denominator.
    pub fn run_slot_count(&self) -> Option<usize> {
        self.runs.then_some(self.run_slots)
    }

    /// The lifting goal the week is built for: a runner's supplemental
    /// strength block is heavy strength work (RUN-STRENGTH-001), else the
    /// athlete's own (stated) `lift_goal`.
    pub fn effective_lift_goal(&self) -> LiftGoal {
        if self.runner_strength {
            LiftGoal::MaxStrength
        } else {
            self.profile.lift_goal
        }
    }

    /// The lift-day count when the athlete chose none: 3 pure, 2 hybrid.
    pub fn default_lift_days(&self) -> usize {
        if self.runs { 2 } else { 3 }
    }

    /// Why no plan can be built, `None` when one can: any onboarding gate
    /// (HARD RULE 3: the engine never programs through a medical deferral),
    /// else a profile that describes neither lifting nor running.
    pub fn withheld(&self) -> Option<PlanWithheld> {
        self.withheld
    }

    /// Training sessions the plan schedules in a normal week (the "planned"
    /// side of planned-vs-done, ADH-SELFMON-001 / ADH-PROGRESS-001).
    pub fn planned_training_days(&self) -> usize {
        if self.withheld.is_some() {
            return 0;
        }
        self.shape.lift_days + self.shape.run_days
    }

    /// Load weeks of the running deload cadence (3:1, or 2:1 when
    /// [`conservative_deload`] holds, RUN-DELOAD-001).
    pub fn run_load_weeks(&self) -> u32 {
        running::deload_cadence(conservative_deload(
            self.profile.age_years,
            self.training_age(),
        ))
        .value
        .load_weeks as u32
    }

    /// The week's lift/run day counts. Lift days come from the athlete's
    /// chosen count (2–5, LIFT-DAYS-001), else the default (3 pure, 2
    /// hybrid); a running-led hybrid keeps 2–3 maintenance lift days
    /// (CONC-RE-001); a runner's supplemental strength is 2 days
    /// (RUN-STRENGTH-001). Run days are the run slots. When the chosen
    /// weekdays can't hold both, the SUPPORTING side gives way first (down to
    /// 2 days, HYB-MAINT-002's maintenance floor), then the leading side.
    fn fit_week_shape(&self) -> WeekShape {
        let (lifting, running) = (self.lifts, self.runs);
        let lead = self.hybrid_lead;
        let mut n_lift = if lifting {
            let chosen = self
                .profile
                .lift_days_per_week
                .map(|d| (d as usize).clamp(2, 5))
                .unwrap_or(self.default_lift_days());
            if lead == Some(HybridLead::Running) {
                chosen.min(hybrid::maintenance_lift_sessions().value.1 as usize)
            } else {
                chosen
            }
        } else if self.runner_strength {
            2
        } else {
            0
        };
        let mut n_run = if running { self.run_slots } else { 0 };
        let avail = self.weekdays.iter().filter(|a| **a).count();
        if !running {
            n_lift = n_lift.min(avail);
        } else if !lifting {
            n_run = n_run.min(avail);
            n_lift = n_lift.min(avail - n_run);
        } else {
            let lift_leads = lead == Some(HybridLead::Lifting);
            while n_lift + n_run > avail {
                let (leading, supporting) = if lift_leads {
                    (&mut n_lift, &mut n_run)
                } else {
                    (&mut n_run, &mut n_lift)
                };
                if *supporting > 2 {
                    *supporting -= 1;
                } else if *leading > 2 {
                    *leading -= 1;
                } else if *supporting > 1 {
                    *supporting -= 1;
                } else if *leading > 1 {
                    *leading -= 1;
                } else {
                    *supporting = 0;
                }
            }
        }
        WeekShape {
            lift_days: n_lift,
            run_days: n_run,
        }
    }
}

fn goal_table_run_days(profile: &Profile) -> usize {
    let gwp = running::goal_week_plan(profile.goal_distance, profile.advanced).value;
    (profile.running_days_per_week as usize)
        .min(gwp.sessions_per_week.1 as usize)
        .max(1)
}

fn run_slot_count(profile: &Profile, goal_table: usize, lead: Option<HybridLead>) -> usize {
    let mut n = goal_table;
    // running-025 (safety-critical): Couch to 5K is three runs a week, and the
    // third run of weeks 5–6 is the bridge to continuous running, so the
    // programme never plans fewer.
    if profile.goal_distance == GoalDistance::C25k {
        n = n.max(running::c25k_plan().value.runs_per_week as usize);
    }
    if lead == Some(HybridLead::Lifting) {
        n = n.min(hybrid::endurance_frequency_cap().value as usize);
    }
    n
}

#[cfg(test)]
pub(crate) mod test_support {
    use crate::hybrid::ConcurrentGoal;
    use crate::individualization::ProgressionCadence;
    use crate::profile::{Profile, ResistanceModality};
    use crate::running::GoalDistance;
    use crate::schema::HealthScreen;
    use crate::strength::LiftGoal;

    /// The one complete `Profile` literal: a neutral intermediate lifter with
    /// no running and every optional field unset. Every other profile fixture
    /// (app, plan and log tests) is `..base_profile()`.
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
}
