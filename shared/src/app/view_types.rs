//! The `ViewModel` and every `*View` wire type the shells render.

use super::*;

/// The evidence chrome every graded row carries, flattened onto its parent
/// view so the wire keys stay `grade`/`citation`/`confidence`/
/// `safety_critical`/`contested`. `Default` is the factual row with no claim:
/// empty grade and citation.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
#[serde(default)]
pub struct EvidenceTagView {
    /// Evidence grade, e.g. `"Strong"`; `""` for a row that asserts no claim.
    #[serde(with = "grade_or_empty")]
    pub grade: Option<EvidenceGrade>,
    /// Backing reference (author/year or DOI).
    pub citation: String,
    /// 0.05–0.90 confidence score.
    pub confidence: f32,
    pub safety_critical: bool,
    pub contested: bool,
}

impl EvidenceTagView {
    /// The evidence chrome of `r`'s backing claim.
    pub fn of<T>(r: &Recommended<T>) -> Self {
        EvidenceTagView {
            grade: Some(r.grade()),
            citation: r.reference().to_owned(),
            confidence: r.confidence(),
            safety_critical: r.safety_critical(),
            contested: r.contested().is_some(),
        }
    }
}

impl<T> From<&Recommended<T>> for EvidenceTagView {
    fn from(r: &Recommended<T>) -> Self {
        EvidenceTagView::of(r)
    }
}

/// Wire form of an optional grade: the variant name, or `""` for none.
pub(crate) mod grade_or_empty {
    use crate::schema::EvidenceGrade;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(g: &Option<EvidenceGrade>, s: S) -> Result<S::Ok, S::Error> {
        match g {
            Some(g) => g.serialize(s),
            None => s.serialize_str(""),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<EvidenceGrade>, D::Error> {
        use serde::de::IntoDeserializer;
        let s = String::deserialize(d)?;
        if s.is_empty() {
            return Ok(None);
        }
        EvidenceGrade::deserialize(s.into_deserializer()).map(Some)
    }
}

/// Wire form of a planned day's [`SessionType`]: its stable
/// [`SessionType::wire_name`] (`"Lift(MaxEffort)"`, `"Run(LongRun)"`, `"Rest"`),
/// which the shell keys its discipline on.
pub(crate) mod session_type_wire {
    use crate::schema::SessionType;
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(t: &SessionType, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(t.wire_name())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<SessionType, D::Error> {
        let name = String::deserialize(d)?;
        SessionType::from_wire_name(&name)
            .ok_or_else(|| D::Error::custom(format!("unknown session type {name:?}")))
    }
}

/// Wire form of a run's 3-zone label: [`zone_label`] (`"Z1"`..`"Z3"`, or
/// `"-"` when no zone is shown), the literals the shell colours by.
pub(crate) mod zone_or_dash {
    use crate::schema::ThreeZone;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(z: &Option<ThreeZone>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(super::zone_label(*z))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<ThreeZone>, D::Error> {
        let label = String::deserialize(d)?;
        Ok([ThreeZone::Z1, ThreeZone::Z2, ThreeZone::Z3]
            .into_iter()
            .find(|&z| super::zone_label(Some(z)) == label))
    }
}

/// A run's zone as card text: `"Z1"`..`"Z3"`, or `"-"` when none is shown.
pub(crate) fn zone_label(zone: Option<ThreeZone>) -> &'static str {
    match zone {
        Some(ThreeZone::Z1) => "Z1",
        Some(ThreeZone::Z2) => "Z2",
        Some(ThreeZone::Z3) => "Z3",
        None => "-",
    }
}

/// One adjustment flattened for shells: human summary + its evidence tag.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct AdjustmentView {
    pub summary: String,
    #[serde(flatten)]
    pub evidence: EvidenceTagView,
    /// The three-part "why?" disclosure (basis → why this grade → what would
    /// improve it).
    #[serde(default)]
    pub why: WhyView,
    /// True for a *neutral* verdict row: a graded "hold / no change" call that
    /// carries no real `Adjustment` (e.g. VBT bar-speed within the ±0.06 m/s
    /// reliability band, or a first-work-set outcome that keeps the planned set
    /// count). Such rows still render in the readiness section, but must never
    /// displace the prescription headline — `model.review` is a persisted
    /// singleton, so a stale "nothing changed" verdict would otherwise outrank
    /// "Next: `<session>`" forever. `build_headline` skips these in its `best`
    /// chain.
    #[serde(default)]
    pub neutral: bool,
    /// For an item-scoped in-session cue (a first-set or top-set call on ONE
    /// lift): the plan item's exact exercise name. `None` for session-wide rows.
    #[serde(default)]
    pub exercise: Option<String>,
    /// For an item-scoped cue that moves the load: the plate-grid load for the
    /// remaining sets, kg. `None` when the cue does not change load.
    #[serde(default)]
    pub suggested_load_kg: Option<f64>,
    /// The cue says to stop this exercise now (tap End exercise).
    #[serde(default)]
    pub suggests_end_exercise: bool,
    /// The §5 safety-ladder tier this call belongs to, for ranking the Today
    /// headline. Core-internal: never on the wire.
    #[serde(skip)]
    pub(crate) tier: Option<SafetyTier>,
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
    /// True when the set carried a recorded RPE (`rpe > 0`). When false the
    /// e1RM/%1RM are scored as reps-to-failure and the RIR is not meaningful,
    /// so a shell omits the RIR caption.
    #[serde(default)]
    pub rpe_recorded: bool,
    /// Cross-checked e1RM range across Epley/Brzycki/Lombardi (strength-005/
    /// 006). `None` when the estimate is unreliable per strength-006 (0 reps,
    /// >10 reps, or an isolation lift), the shell should then suggest a 3–6
    /// > rep test set instead of showing a false range.
    #[serde(default)]
    pub cross_check: Option<E1rmRangeView>,
    /// e1RM change vs the previous logged set of the same exercise, kg
    /// (rounded to 0.1). `None` for the first logged set of an exercise -
    /// there is nothing to compare against. Factual measurement (like pace or
    /// zone), NOT a `Recommended`: it states what changed, not whether the
    /// athlete is "improving"/"declining": that judgment is the trend arm
    /// (feedback-027/028/029, `feedback::trend_summary`, FB-TREND-001), which
    /// a shell must not phrase from this delta alone.
    #[serde(default)]
    pub e1rm_delta_kg: Option<f64>,
    /// Direction of `e1rm_delta_kg` (flat when the 0.1 kg-rounded delta is
    /// zero). `None` when `e1rm_delta_kg` is `None`. Same factual-measurement
    /// caveat as the delta.
    #[serde(default)]
    pub e1rm_direction: Option<crate::feedback::TrendDirection>,
    pub summary: String,
    /// When the set was logged, unix seconds; 0 when undated. A shell dates the
    /// history card from this; the core does no formatting (it holds no clock).
    pub observed_at: i64,
    /// Stable per-entry id echoed for the shell's edit/delete affordance: the
    /// shell sends it back in `AmendSet`/`DeleteEntry`. 0 for a legacy row (the
    /// shell then falls back to `observed_at`).
    #[serde(default)]
    pub entry_id: u64,
    /// Timed-hold duration, seconds, for a static/hold set (`None` for a
    /// reps×load set). When `Some`, `summary` reads "`<name> - <s> s hold`" and a
    /// shell renders/exports a hold row instead of reps×load.
    #[serde(default)]
    pub hold_sec: Option<u16>,
    /// `true` when this set's e1RM beats every PRIOR e1RM logged for the same
    /// exercise (a neutral PR marker for the history card; the first set of an
    /// exercise is never flagged). Serde-default.
    #[serde(default)]
    pub e1rm_pr: bool,
    /// Load × reps lifted in this set, kg; `0.0` for a timed hold. The one
    /// tonnage definition a shell sums for a session's or a week's volume.
    /// Serde-default.
    #[serde(default)]
    pub tonnage_kg: f64,
}

/// The strength-006 multi-formula e1RM cross-check, flattened for shells.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct E1rmRangeView {
    /// Lowest estimate across the formulas, kg (0.1-rounded).
    pub low_kg: f64,
    /// Highest estimate across the formulas, kg (0.1-rounded).
    pub high_kg: f64,
    /// Number of formulas cross-checked (Epley, Brzycki, Lombardi).
    pub formulas: u8,
    #[serde(flatten)]
    pub evidence: EvidenceTagView,
}

/// The CTL/ATL/TSB fitness–fatigue bookkeeping over the logged history,
/// flattened for shells (File 07 impulse-response; LOAD-PMC-001). Daily load
/// is a Lucia 3-zone TRIMP from each dated, HR-carrying run, the method
/// string states exactly what was counted so the shell never has to guess.
/// Bookkeeping only: TSB is NOT a validated performance predictor (the claim
/// statement carries that caveat).
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct TrainingLoadView {
    /// Chronic Training Load (fitness): EWMA τ=42 d of daily load.
    pub ctl: f64,
    /// Acute Training Load (fatigue): EWMA τ=7 d of daily load.
    pub atl: f64,
    /// Training Stress Balance (form) entering the next day: CTL − ATL.
    pub tsb: f64,
    /// Days spanned from the first to the last counted session.
    pub days: u32,
    /// Sessions that contributed load (dated runs with HR + duration).
    pub sessions_counted: u32,
    /// Logged sessions that could NOT contribute heart-rate load (undated, no
    /// HR, or lifts). Rated HR-less runs feed the separate session-RPE series
    /// ([`SrpeLoadView`]) instead; the two are never summed.
    pub sessions_skipped: u32,
    /// How daily load was quantified, e.g. `"Lucia TRIMP (3-zone HR)"`.
    pub method: String,
    pub summary: String,
    #[serde(flatten)]
    pub evidence: EvidenceTagView,
}

/// Session-RPE training load (Foster: minutes × the 0–10 whole-session rating;
/// LOAD-SRPE-001): the sensor-free load measure for runs without heart rate,
/// kept as its OWN weekly series because it is not interchangeable with the
/// heart-rate load. Derived from the run-progress weekly series.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct SrpeLoadView {
    /// Load this local week (Monday start), arbitrary units.
    pub week_au: f64,
    /// Load last week, arbitrary units.
    pub prev_week_au: f64,
    /// Rated runs this week / runs this week still unrated.
    pub rated_runs: u32,
    pub unrated_runs: u32,
    pub method: String,
    pub summary: String,
    #[serde(with = "grade_or_empty")]
    pub grade: Option<EvidenceGrade>,
    pub citation: String,
    pub confidence: f32,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct ViewModel {
    /// Highest safety tier triggered, e.g. `"Pain"`; `None` when all clear.
    pub safety_tier: Option<SafetyTier>,
    /// True when a Stop-level or rest-day condition fires, do not train.
    pub train_blocked: bool,
    /// Why training is on hold, the most specific reason; set whenever
    /// `train_blocked`, `None` otherwise. Lets a shell show the banner line and
    /// the resolve action that fit the reason.
    #[serde(default)]
    pub hold_reason: Option<HoldReason>,
    /// True when a morning check-in is dated today (the readiness strip is
    /// current); false before today's check-in.
    #[serde(default)]
    pub checked_in_today: bool,
    pub adjustments: Vec<AdjustmentView>,
    /// Week-level deload triggers carried by the weekly check-in in force, plus
    /// the session review's execution rows. Kept separate from `adjustments`
    /// (which are readiness-driven and cleared by `ClearReadiness`): the weekly
    /// rows clear with `ClearWeeklyCheckin`, the session rows with
    /// `ClearReview`, so a shell renders them under the review, not the
    /// readiness section.
    pub review_adjustments: Vec<AdjustmentView>,
    /// True when a persisted per-session review is present, so a shell can show
    /// a "Clear review" affordance (dispatching `ClearReview`).
    #[serde(default)]
    pub review_present: bool,
    /// True when a weekly check-in is in force, so a shell can offer "Clear
    /// weekly check-in" (dispatching `ClearWeeklyCheckin`).
    #[serde(default)]
    pub weekly_checkin_present: bool,
    pub input_count: usize,
    pub lifts: Vec<LiftResultView>,
    pub runs: Vec<RunResultView>,
    /// Evidence-cited programming guidance derived from the profile.
    pub guidance: Vec<GuidanceView>,
    /// The single resolved coaching message: a weekly-check-in safety concern,
    /// a run-spike warning, or one takeaway on the latest session (from the
    /// session review, or derived from the logged sets/runs of a completed
    /// plan day — no form needed), or a new personal best. `None` when nothing
    /// applies.
    pub feedback: Option<FeedbackView>,
    /// Always-available evidence-cited reference defaults (profile-independent).
    pub reference: std::borrow::Cow<'static, [GuidanceView]>,
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
    /// CTL/ATL/TSB training-load bookkeeping over the logged run history.
    /// `None` until at least one dated, HR-carrying run is logged.
    #[serde(default)]
    pub training_load: Option<TrainingLoadView>,
    /// Session-RPE load (minutes × whole-session 0–10 rating): the separate,
    /// sensor-free load series for rated runs. `None` until a run is rated.
    #[serde(default)]
    pub srpe_load: Option<SrpeLoadView>,
    /// Weekly running-volume system rows for the most recent logged week:
    /// week-over-week increase check, two-week ramp flag, Daniels volume caps,
    /// easy-share floor, quality-plan check, hybrid combined-load guard.
    /// Empty until a dated run exists.
    #[serde(default)]
    pub weekly_report: Vec<GuidanceView>,
    /// Running progress: 16-week weekly volume (+ its separate session-RPE
    /// load), longest run and best efforts with PR identity. `None` until a
    /// dated run with a measurable distance exists. Serde-default.
    #[serde(default)]
    pub run_progress: Option<RunProgressView>,
    /// Lift-session audit rows for the most recent dated lifting day:
    /// Prilepin volume check per exercise, plus the depth-jump readiness gate
    /// when bodyweight + a squat e1RM are known. Empty otherwise.
    #[serde(default)]
    pub lift_audit: Vec<GuidanceView>,
    /// The last requested Cooper 12-min-test estimate, as graded rows.
    #[serde(default)]
    pub cooper: Vec<GuidanceView>,
    /// The last requested Critical-Speed protocol fit, as graded rows.
    #[serde(default)]
    pub critical_speed: Vec<GuidanceView>,
    /// The last requested APRE next-load adjustment, as graded rows.
    #[serde(default)]
    pub apre: Vec<GuidanceView>,
    /// Raw calculator inputs echoed back so shells rehydrate their forms after a
    /// log replay / process death, keeping the core the single source of truth.
    /// Each is a sibling of its result field above; `None`/empty until queried.
    /// (`race_prediction` echoes its inputs inline; these cover the Vec-result
    /// calculators.)
    #[serde(default)]
    pub hr_zone_input: Option<HrZoneInputView>,
    #[serde(default)]
    pub protein_input: Option<ProteinInputView>,
    #[serde(default)]
    pub hypertrophy_input: Option<HypertrophyInputView>,
    /// The weekly direct-set counts of the hypertrophy plan's ramp row (e.g.
    /// `[12, 16, 20, 22]`), for a compact summary of the last plan. Empty when no
    /// plan exists, a calculator hold applies, the muscle is unknown or the
    /// block has zero weeks.
    #[serde(default)]
    pub hypertrophy_ramp: Vec<u8>,
    #[serde(default)]
    pub cooper_input: Option<f64>,
    #[serde(default)]
    pub critical_speed_input: Vec<CsEffortIn>,
    #[serde(default)]
    pub apre_input: Option<ApreInputView>,
    /// The feedback-027/028/029 longitudinal trend message, derived from the
    /// logged lifts (best estimated max over the last 4 weeks vs the 4 before,
    /// ±5 % noise band); `None` when nothing decisive.
    #[serde(default)]
    pub trend: Option<AdjustmentView>,
    /// feedback-040 provisional framing: present while fewer than ~14 distinct
    /// days of logged data exist, recommendations are population defaults
    /// still converging on the user.
    #[serde(default)]
    pub provisional: Option<AdjustmentView>,
    /// Which autoregulation signal source is active given the submitted
    /// readiness data (autoreg-047/048 graceful fallback): HRV rolling,
    /// subjective + performance, or performance-only-hold. `None` until any
    /// readiness input exists.
    #[serde(default)]
    pub autoreg_source: Option<AdjustmentView>,
    /// KB-honest per-signal readiness summary: the latest state of every
    /// observed readiness signal, judged by the SAME File 06/08 thresholds the
    /// adjustment rules use, each citing the rule's evidence. Deliberately NO
    /// composite 0–100 readiness score, the KB defines none (HARD RULE 1).
    /// Metrics first, then the red-flag block (see `signal_groups`).
    #[serde(default)]
    pub readiness_summary: Vec<ReadinessSignalView>,
    /// The single highest-priority call for today (usability-ia-spec §7), in
    /// rank order: train-blocked safety hold > safety-critical feedback (a
    /// medical referral) > real adjustment (deload/readiness) > prescription
    /// (next session) > non-critical feedback > all-clear default.
    /// Prioritization is coaching logic, so it lives here, not in shells.
    #[serde(default)]
    pub today_headline: TodayHeadlineView,
    /// Static signal→group metadata for every readiness signal (in picker
    /// order, metrics before red flags), so the shell's red-flag fence in the
    /// readiness picker is data-driven from the core, not a shell predicate.
    #[serde(default)]
    pub signal_groups: std::borrow::Cow<'static, [SignalGroupView]>,
    /// The most recent morning check-in, echoed back so the shell can rehydrate
    /// the check-in sheet and show that today's check-in is recorded. `None`
    /// until any check-in exists. Additive/serde-default.
    #[serde(default)]
    pub checkin_today: Option<CheckinEchoView>,
    /// Honest "collecting your baseline" status for each check-in channel that
    /// has readings but not yet enough history to emit a z-score/delta, so the
    /// shell shows honesty, never a fabricated number (HARD RULE 1). Empty once
    /// every channel is either baseline-ready (its derived signal appears in
    /// `readiness_summary`) or has no data.
    #[serde(default)]
    pub baseline_status: Vec<BaselineStatusView>,
    /// The evidence-grade legend, core-provided so the "How evidence grading
    /// works" sheet renders the File 09 definitions from core data rather than
    /// hardcoded shell copy. Static; always populated.
    #[serde(default)]
    pub grade_definitions: std::borrow::Cow<'static, [GradeDefView]>,
    /// Today's concrete next session: the hero of the inverted Coach.
    /// `None` until the user accepts a plan. Rendered strictly downstream of
    /// the safety gates: a `train_blocked` hold sets `status = "blocked"` and
    /// empties its items (HARD RULE 3).
    #[serde(default)]
    pub next_session: Option<SessionPlanView>,
    /// The seven days of the current week around today, for the week strip.
    #[serde(default)]
    pub week_plan: Vec<SessionPlanView>,
    /// The active program summary card (name/goal/phase/week), when a plan is set.
    #[serde(default)]
    pub program: Option<ProgramSummaryView>,
    /// Structured HRmax figure (bpm + measured/estimate + Tanaka split) for
    /// the last HR-zone query, so no shell regex-scrapes `hr_zones`.
    /// `None` until an HR-zone query is made (or the age is out of range).
    #[serde(default)]
    pub hr_max: Option<HrMaxView>,
    /// Structured protein g/day figures paralleling `protein_targets`, so no
    /// shell regex-scrapes those rows. Empty until a protein query is made.
    #[serde(default)]
    pub protein_figures: Vec<ProteinFigureView>,
    /// Structured Daniels bands of the last HR-zone query, one per band row of
    /// `hr_zones`. Empty until a query is made (or the age is out of range).
    #[serde(default)]
    pub hr_zone_bands: Vec<HrZoneBandView>,
    /// Structured figures of the hypertrophy plan. `None` whenever the planner
    /// shows no ramp (a hold, an unknown muscle, a zero-week block).
    #[serde(default)]
    pub hypertrophy_figures: Option<HypertrophyFiguresView>,
    /// The core's current local epoch-day (`SetToday`), echoed so the shell reads
    /// `today_epoch_day ?: todayEpochDay()` for every plan gate instead of
    /// re-deriving a clock during composition. `None` until `SetToday`.
    #[serde(default)]
    pub today_epoch_day: Option<i64>,
    /// Today's completed plan session, summarized from the logged work (one
    /// line per item), shown on Today once the hero's last item is done; the
    /// matching coaching takeaway is `feedback`. `None` until today's planned
    /// session is complete.
    #[serde(default)]
    pub session_done: Option<SessionDoneView>,
    /// Every calisthenics ladder with its rungs, for a `Calisthenics` profile
    /// (empty otherwise): the shell's rung picker in setup (starting rung →
    /// `Event::SetCalisthenicsRung`) and the Log chooser grouped by movement.
    #[serde(default)]
    pub calisthenics_ladders: Vec<CalisLadderView>,
    /// One row per active pain / illness hold: the banner's
    /// "I'm pain-free now" / "I've recovered" action. Confirming sends
    /// `Event::ResolveHold { signal, observed_at: now }` (every report of that
    /// signal retracted, history kept; the coach restarts with an easier
    /// session, RETURN-HOLD-001 / ILLNESS-RTT-001). "Logged by mistake" stays
    /// the undated `RemoveReadiness`. Empty when no hold.
    #[serde(default)]
    pub hold_resolutions: Vec<HoldResolutionView>,
    /// True while an illness report is active or a single high resting-HR
    /// morning asks for the illness check (ready-upd-002): the morning
    /// check-in asks "Still unwell?" / "Feeling ill?" first.
    #[serde(default)]
    pub illness_check_prompt: bool,
    /// Planned-vs-done consistency for the active plan
    /// (ADH-SELFMON-001): this week's done/planned, the last 8 plan weeks, and
    /// neutral coaching notes (missed session, repeated week, re-entry
    /// prompt). No streaks, no loss framing (ADH-GAMIFY-001). `None` without
    /// a plan.
    #[serde(default)]
    pub consistency: Option<ConsistencyView>,
    /// Why no plan can be built right now (`None` when one can): today's
    /// profile raises a health gate, or it plans neither lifting nor running.
    /// While set, `GeneratePlan` cannot produce a session, so a shell offers no
    /// "Plan my training" and says why instead (the hold banner carries the way
    /// back for a health gate).
    #[serde(default)]
    pub plan_withheld: Option<PlanWithheld>,
    /// Every logged other activity, chronological (`observed_at`, then
    /// `entry_id`), like `lifts` / `runs`.
    #[serde(default)]
    pub activities: Vec<ActivityResultView>,
    /// The WHO 5-17 weekly activity card (YOUTH-PA-001): `Some` only when
    /// today's profile is under 18 and the shell has sent today
    /// (`SetToday`). Tracking and information only; the plan hold stays.
    #[serde(default)]
    pub youth_activity: Option<YouthActivityView>,
}

/// One logged other activity ([`ViewModel::activities`]).
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct ActivityResultView {
    pub entry_id: u64,
    pub observed_at: i64,
    pub kind: ActivityKind,
    pub minutes: f64,
    pub vigorous: bool,
    pub strengthening: bool,
}

/// This local Mon–Sun week's activity against the WHO 5-17 reference
/// (YOUTH-PA-001; counting rules in `extracted/08` youth-pa-001).
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct YouthActivityView {
    /// The local epoch day of this week's Monday.
    pub week_monday_epoch_day: i64,
    /// Moderate-to-vigorous minutes this week (runs + lift spans + logged
    /// activities), rounded.
    pub minutes: u32,
    pub run_minutes: u32,
    pub lift_minutes: u32,
    pub activity_minutes: u32,
    /// 60: the WHO daily average.
    pub daily_target_minutes: u32,
    /// 420: the daily average over the week.
    pub weekly_target_minutes: u32,
    /// Distinct local days this week with vigorous activity.
    pub vigorous_days: u8,
    /// Distinct local days this week with muscle- or bone-strengthening
    /// activity.
    pub strengthening_days: u8,
    /// 3: the WHO days-a-week target for each of the two day counts.
    pub target_days: u8,
    /// This week's logged activities, chronological, for remove.
    pub week_entries: Vec<ActivityResultView>,
    #[serde(flatten)]
    pub evidence: EvidenceTagView,
    pub why: WhyView,
}

/// Why no plan can be built ([`ViewModel::plan_withheld`], wire: snake_case).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanWithheld {
    /// A profile health answer holds training (illness, a doctor's OK, age…).
    HealthHold,
    /// The profile plans no lifting sessions and no running days.
    NothingToPlan,
}

/// Planned vs done for the active plan (ADH-SELFMON-001).
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct ConsistencyView {
    /// Sessions done (or done lighter) so far in the current plan week.
    pub this_week_done: u8,
    /// Sessions the current plan week schedules (from the plan start on).
    pub this_week_planned: u8,
    /// The last (up to) 8 finished plan weeks, oldest first.
    #[serde(default)]
    pub weeks: Vec<ConsistencyWeekView>,
    /// Share of planned sessions done over `weeks`, percent. `None` until a
    /// plan week has finished.
    #[serde(default)]
    pub share_pct: Option<u8>,
    /// The plan is repeating this week because last week had fewer than about
    /// two-thirds of its sessions done (ADH-PROGRESS-001).
    #[serde(default)]
    pub repeating_week: bool,
    /// Display-ready, evidence-cited lines: the done/planned summary first,
    /// then any missed-session / repeated-week / re-entry note.
    #[serde(default)]
    pub notes: Vec<GuidanceView>,
}

/// One finished plan week's done/planned counts.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct ConsistencyWeekView {
    pub week_monday_epoch_day: i64,
    pub done: u8,
    pub planned: u8,
}

/// The resolve action for one active pain / illness hold.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct HoldResolutionView {
    /// The signal `Event::ResolveHold` targets: `"Pain"` or `"Illness"`.
    pub signal: ReadinessSignal,
    /// Why training is paused and how it clears, one plain sentence.
    pub paused_text: String,
    /// The banner action label, e.g. "I'm pain-free now".
    pub action_label: String,
    /// The confirm-dialog body.
    pub confirm_text: String,
    /// Confirming also ends the profile's "I'm ill right now" answer: the shell
    /// sends that `SetProfile` (acute illness off, effective today) with the
    /// `ResolveHold`, so one tap ends every illness hold the user reported.
    #[serde(default)]
    pub ends_profile_illness: bool,
}

/// One calisthenics ladder for pickers.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CalisLadderView {
    /// Ladder id, the `ladder` of `Event::SetCalisthenicsRung`.
    pub ladder: crate::schema::LadderId,
    /// Movement name, e.g. "Pull-up".
    pub pattern: String,
    /// Optional skill track (handstand, levers, planche, muscle-up).
    #[serde(default)]
    pub skill: bool,
    /// Power ladder (jumps, explosive push-ups), used on a Power goal.
    #[serde(default)]
    pub power: bool,
    /// The coach's current rung for this ladder: the placed / trained rung, or
    /// the nearest one the athlete's equipment allows; `None` = not started,
    /// or no rung of the ladder fits the equipment (see `equipment_note`).
    #[serde(default)]
    pub current_rung: Option<u8>,
    /// The placed / trained rung when the equipment can't support it (so
    /// `current_rung` differs from it); `None` when nothing was moved.
    #[serde(default)]
    pub placed_rung: Option<u8>,
    /// Plain-language note when the equipment moved or paused this ladder,
    /// e.g. "Full pull-up needs a pull-up bar or rings, so you start at …";
    /// empty otherwise.
    #[serde(default)]
    pub equipment_note: String,
    /// Every rung, easiest first.
    #[serde(default)]
    pub rungs: Vec<CalisRungView>,
}

/// One rung of a calisthenics ladder.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct CalisRungView {
    /// Rung index, the `rung` of `Event::SetCalisthenicsRung`.
    pub rung: u8,
    /// Canonical exercise name (log under this name).
    pub name: String,
    /// The rung's rep/hold band, e.g. "5–8 reps" or "20–45 s hold" (+ " per side").
    #[serde(default)]
    pub target: String,
    /// False when the athlete's equipment can't support it (pickers grey it out).
    #[serde(default)]
    pub available: bool,
    /// The rung is a timed hold (its `target` is a hold band in seconds).
    #[serde(default)]
    pub timed_hold: bool,
}

/// Calisthenics-only data on a plan item.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CalisItemView {
    pub ladder: crate::schema::LadderId,
    /// Prescribed rung index.
    pub rung: u8,
    /// Rungs on this ladder.
    #[serde(default)]
    pub rung_count: u8,
    /// "Easier" / "Harder" choices (nearest rung the equipment allows). Choosing
    /// one sends `Event::SetCalisthenicsRung{ladder, rung}`; the item then
    /// shows that rung the same day.
    #[serde(default)]
    pub easier: Option<CalisRungView>,
    #[serde(default)]
    pub harder: Option<CalisRungView>,
    /// The rung takes external load (belt / vest / plates): show the "added
    /// load (kg)" field even when the suggested `added_kg` is 0.
    #[serde(default)]
    pub load_capable: bool,
    /// Straight-arm skill hold: the clean test hold (seconds) done once at the
    /// end of a session, once a week, that advances the rung; the working holds
    /// are `hold_low_sec..hold_high_sec`. 0 = no separate test.
    #[serde(default)]
    pub test_hold_sec: u16,
    /// Ballistic power item: max intent, never to failure (no RPE prefill).
    #[serde(default)]
    pub ballistic: bool,
    /// The rung waits only for an effort rating on the last set.
    #[serde(default)]
    pub needs_effort_rating: bool,
}

/// Today's completed plan session: what was done, per item.
/// Purely descriptive (a summary of logs), so it carries no evidence tag; the
/// coaching call lives in `ViewModel::feedback`.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct SessionDoneView {
    #[serde(default)]
    pub epoch_day: i64,
    /// The session title, e.g. "Heavy day".
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub items: Vec<SessionDoneItemView>,
}

/// One exercise (or the run) of a completed session.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct SessionDoneItemView {
    /// Exercise name, or "Run".
    #[serde(default)]
    pub exercise: String,
    /// Display-ready line, e.g. "4 of 4 sets · best 120 kg × 5 @ RPE 8" or
    /// "8.2 km · 46 min". A run's distance is in km: render a run from the
    /// numeric `run_*` fields in the user's unit instead.
    #[serde(default)]
    pub summary: String,
    /// The item met its plan: every planned set at the target reps and load,
    /// or the run its done threshold. Equals `status == "done"`.
    #[serde(default)]
    pub met: bool,
    /// The item's status on the hero, so Session Done reads the same verdict
    /// ("done", "done_short", "done_lighter", "stopped_pain", …); a run under
    /// its done threshold but past half its plan is `done_lighter`.
    #[serde(default)]
    pub status: ItemStatus,
    /// Run items: the logged distance in km (`None` for lifts).
    #[serde(default)]
    pub run_km: Option<f64>,
    /// Run items: the logged moving time in minutes (`None` for lifts).
    #[serde(default)]
    pub run_min: Option<f64>,
    /// Run items: the planned distance in km as shown that day, when the run
    /// was planned by distance. With `run_km` under it, read "5 of 5.9 mi".
    #[serde(default)]
    pub run_planned_km: Option<f64>,
    /// Run items: the planned time in minutes, when the run was planned by
    /// time.
    #[serde(default)]
    pub run_planned_min: Option<f64>,
}

/// The most recent check-in echoed for rehydration + a "checked in today" cue
/// (`ViewModel::checkin_today`).
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct CheckinEchoView {
    pub observed_at: i64,
    #[serde(default)]
    pub sleep_quality: Option<u8>,
    #[serde(default)]
    pub soreness: Option<u8>,
    #[serde(default)]
    pub mood: Option<u8>,
    #[serde(default)]
    pub fatigue: Option<u8>,
    #[serde(default)]
    pub stress: Option<u8>,
    #[serde(default)]
    pub resting_hr_bpm: Option<f64>,
    #[serde(default)]
    pub hrv_rmssd_ms: Option<f64>,
}

/// One check-in channel still collecting its baseline (`app/mod.rs` `baseline_status`):
/// an honest progress row shown in place of a derived signal until enough
/// history exists. Carries NO evidence tag: it asserts no training claim, it
/// states the absence of one (HARD RULE 2 is about recommendations).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct BaselineStatusView {
    /// The signal this baseline feeds, e.g. `"WellnessZ"`.
    pub signal: ReadinessSignal,
    /// Friendly channel label, e.g. `"Sleep, soreness, fatigue, stress & mood"`.
    pub label: String,
    /// Check-in days collected so far.
    pub have: u32,
    /// Days needed before a baseline is emitted.
    pub need: u32,
    /// `"Collecting"` (first baseline), `"SparseWeek"` (an established channel
    /// with too few readings this week) or `"Suspended"`; lets a shell pick the
    /// row's phrasing.
    pub kind: crate::autoreg::BaselineKind,
}

/// One readiness signal's latest observation, flattened for shells: the raw
/// value plus a qualitative state string decided by the same KB threshold that
/// drives the matching adjustment rule (`autoreg::signal_states_in`). The
/// evidence fields cite that rule; they are empty (grade `""`) for the plain
/// factual rows ("recorded"/"clear") that judge nothing.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ReadinessSignalView {
    /// The signal, e.g. `"HrvLnRmssd"`.
    pub signal: ReadinessSignal,
    /// `"metric"` or `"red_flag"` (medical-referral / hard-stop block).
    pub group: crate::autoreg::SignalGroup,
    pub value: f64,
    pub streak: u8,
    /// Qualitative state, e.g. `"below normal"`, `"high 2+ days"`.
    pub state: String,
    /// Display-only sub-line context, e.g. a pain report's
    /// `"Left knee · sharp/joint · 6/10"`. Empty for signals without such
    /// detail.
    #[serde(default)]
    pub detail: String,
    #[serde(flatten)]
    pub evidence: EvidenceTagView,
    /// False when the shown value comes from an earlier check-in, not today's.
    #[serde(default)]
    pub from_today: bool,
    /// True when the shown reading is recency-expired: `state` is then the
    /// "out of date" text, which already says the value is not today's, so a
    /// shell adds no "earlier" qualifier of its own.
    #[serde(default)]
    pub stale: bool,
}

/// Why training is on hold ([`ViewModel::hold_reason`]), most specific first.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HoldReason {
    /// PAR-Q+ follow-up answer not yet cleared by a doctor.
    Parq,
    /// Under 18, or age not confirmed.
    Youth,
    /// Pregnancy warning sign / contraindication.
    Pregnancy,
    /// RED-S / low-energy-availability signs (profile, readiness or weekly).
    Reds,
    /// Declared injury or rehab.
    InjuryRehab,
    /// Declared exercise symptoms (chest pain, dizziness…).
    Symptomatic,
    /// "I'm ill right now" in the profile health screen.
    AcuteIllnessProfile,
    /// An illness report (fever / below-neck) or a corroborated resting-HR rest day.
    Illness,
    /// A pain report that stops training.
    Pain,
    /// Another medical red flag (cardiac, bone stress) or an overtraining referral.
    MedicalSignal,
}

/// Which rung of the Today ranking produced the headline (wire: snake_case).
#[derive(Serialize, Deserialize, Debug, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HeadlineKind {
    /// A train-blocking safety hold.
    SafetyHold,
    /// A readiness / review adjustment.
    Adjustment,
    /// A session-feedback card.
    Feedback,
    /// Today's next planned session.
    Prescription,
    /// No rule triggered: train as planned.
    #[default]
    AllClear,
}

/// The core-owned "today's call" headline: one field carrying the
/// highest-priority call for today so no
/// shell re-implements the ranking. `kind` is the priority bucket that won, in
/// rank order: `"safety_hold"` (train-blocked) > `"feedback"` when
/// `safety_critical` (a medical referral) > `"adjustment"` (a real
/// deload/readiness call) > `"prescription"` (the next session) > `"feedback"`
/// (non-critical) > `"all_clear"`. The
/// all-clear default states the ABSENCE of any triggered rule: it makes no
/// evidence-bearing claim, so its evidence fields are empty.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct TodayHeadlineView {
    pub kind: HeadlineKind,
    pub summary: String,
    #[serde(flatten)]
    pub evidence: EvidenceTagView,
    /// The three-part "why?" disclosure (basis → why this grade → what would
    /// improve it).
    #[serde(default)]
    pub why: WhyView,
}

/// The three-part "why?" disclosure carried by every action-bearing card:
/// `basis` (the rule/method and the user datum behind the call), `grade_note`
/// (a one-sentence gloss of this claim's evidence grade, contested question
/// appended when relevant), and `improves` (what data would sharpen the call).
/// `basis` and `improves` describe the engine's own reasoning and data needs,
/// never new training advice, so they invent no claim (HARD RULE 1).
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct WhyView {
    pub basis: String,
    pub grade_note: String,
    pub improves: String,
}

/// One logged set attributed to a plan item's session day, flattened for
/// shells. Both the qualifying sets (counted toward `sets_done`) and the
/// non-qualifying ones (warm-ups / below target) are listed so the shell can
/// show the whole session. `summary` is display-ready and unit-aware.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct ItemSetView {
    pub entry_id: u64,
    pub observed_at: i64,
    pub weight_kg: f64,
    pub reps: u32,
    pub rpe: f64,
    #[serde(default)]
    pub hold_sec: Option<u16>,
    /// Whether this set counts toward completing the item (movement + one rep
    /// or hold second + a work-set load; RPE never gates). A short set (fewer
    /// reps than prescribed) still counts and fills its slot. `false` for a
    /// warm-up or a set too light to be a work set.
    pub qualifies: bool,
    /// A counted set below the full prescribed load ("lighter than planned").
    #[serde(default)]
    pub lighter: bool,
    /// A counted set that fell short of the prescribed reps (or hold time):
    /// "missed reps", independent of the load (`lighter`).
    #[serde(default)]
    pub short: bool,
    /// Display-ready, unit-aware line, e.g. "120 kg × 3", "265 lb × 3",
    /// "30 s hold", "30 s hold · +10 kg" (the rated effort rides on `rpe`).
    pub summary: String,
}

/// One prescribed exercise, flattened for shells.
/// The concrete "do X" contract: sets/reps + either a load (from the user's
/// logged e1RM) or a proximity-to-failure target when no anchor exists, plus the
/// same evidence block every action-bearing view carries (HARD RULE 2).
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct PrescriptionView {
    /// The full human line, e.g. `"Back squat: 5×3 @ 85 kg · ~2 reps left"`.
    pub summary: String,
    pub exercise: String,
    pub sets: u8,
    pub reps_low: u8,
    pub reps_high: u8,
    /// Working load in kg when anchored to a logged e1RM; `None` → the
    /// `intensity_label` carries the RIR/RPE/pace target instead (no invented
    /// load, HARD RULE 1).
    #[serde(default)]
    pub load_kg: Option<f64>,
    /// Intensity target, e.g. `"85% e1RM"` | `"RIR 3"` | `"90% HRmax"`.
    pub intensity_label: String,
    /// Structured form of `intensity_label` — the machine-readable target that
    /// readiness adjustments (CapRpe, load reductions) mutate directly; the
    /// label is re-rendered FROM it, so no adjustment path parses the display
    /// string. `Prose` (the default) is the free-text label.
    #[serde(default)]
    pub intensity: PlanIntensity,
    /// Rest between sets, seconds (0 for runs).
    #[serde(default)]
    pub rest_sec: u16,
    /// For an interval/repetition run, the rep count. `0` for a continuous
    /// run or a lift: the `summary` then carries the whole-session volume. When
    /// nonzero the `summary` already reads as "`N × <rep_volume> · <pace>`".
    #[serde(default)]
    pub rep_count: u8,
    /// For an interval/repetition run, the per-rep volume label, e.g. `"4 min"`
    /// or `"800 m"`. Empty for a continuous run or a lift.
    #[serde(default)]
    pub rep_volume: String,
    /// True for a timed-hold calisthenics prescription (lever/plank/dead-hang):
    /// `rep_volume` carries "20–45 s hold" and `reps_low/high` stay 0. Kotlin
    /// default `false`; additive, so old shells ignore it.
    #[serde(default)]
    pub timed_hold: bool,
    /// Target proximity-to-failure as an RPE for prefilling a log entry:
    /// derived 1:1 from the prescribed RIR via `strength::rir_to_rpe` (or the
    /// prescribed RPE verbatim); for a %e1RM item, the effort the percentage
    /// implies on the Epley model (85 %×3 → RIR 2 → RPE 8). `None` for
    /// velocity / jump / run items. Never invented (HARD RULE 1).
    #[serde(default)]
    pub target_rpe: Option<f64>,
    /// The athlete's most recent same-movement top working set from a PRIOR
    /// day (heaviest set of that session, unit-formatted `summary`), so the log
    /// editor can prefill a weight and show "Last: 12.5 kg × 12".
    /// `None` when the movement was never logged before this day.
    #[serde(default)]
    pub last_set: Option<ItemSetView>,
    /// For an effort-prescribed item with no prescribed load (`load_kg ==
    /// None`): the weight to start from, taken from `last_set` (same weight, or
    /// one plate step up once the last set beat the target by ≥2 reps,
    /// STR-LOADPROG-001). kg. `None` without history.
    #[serde(default)]
    pub suggested_load_kg: Option<f64>,
    /// Plates to load on EACH side of a standard bar (20 kg / 45 lb) for a
    /// barbell lift's `load_kg` (else `suggested_load_kg`), largest first, in the
    /// profile's display unit, e.g. `[20, 20, 2.5, 1.25]` for 117.5 kg. Empty for
    /// non-barbell lifts or loads at/below the bar.
    #[serde(default)]
    pub plates_per_side: Vec<f64>,
    /// A short qualitative execution cue for the face, e.g. "move fast" on a
    /// power/speed set or "max height" on a jump. Empty when none applies. The
    /// summary also carries it.
    #[serde(default)]
    pub effort_cue: String,
    /// Timed-hold band in seconds (both 0 for non-hold items). Mirrors the
    /// text in `rep_volume` ("20–45 s hold") numerically so the shell can
    /// prefill the hold editor without parsing strings.
    #[serde(default)]
    pub hold_low_sec: u16,
    #[serde(default)]
    pub hold_high_sec: u16,
    /// External ADDED load for a calisthenics item (`Some(0.0)` = pure
    /// bodyweight; `Some(x)` = weighted rung). `None` for barbell lifts and
    /// runs — those use `load_kg`.
    #[serde(default)]
    pub added_kg: Option<f64>,
    /// The honesty line for a load, e.g. `"e1RM 120.0 kg (your logged best)"`.
    /// Empty when RIR-prescribed or a run.
    #[serde(default)]
    pub anchored_on: String,
    /// `anchored_on` is a last-time line (`"Last: 60 kg × 8"`), shown on the
    /// face as what was lifted last time; `false` for an estimate / %BW
    /// honesty line and an empty one.
    #[serde(default)]
    pub anchored_on_last: bool,
    /// Set when a readiness adjustment modified this item, e.g.
    /// `"load −10% - readiness (AUTOREG-RIR-001)"`. Empty otherwise.
    #[serde(default)]
    pub adjusted_note: String,
    /// This item is the post-illness return check (ready-upd-003,
    /// ILLNESS-RTT-001) standing in for the whole session: its dose and stop
    /// list are the advice, so the shell shows them once, here. Sent only when
    /// true, so every other item keeps its wire.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub return_check: bool,
    /// For a run held by the single-run spike ceiling (RUN-SPIKE-001 /
    /// RUN-START-001): the plain reason the safety badge stands for, e.g.
    /// `"Capped at 12.5 km: at most 10% over your longest recent run"`
    /// (distances in km like `summary`). The shell shows it inline instead
    /// of a bare badge. Empty otherwise.
    #[serde(default)]
    pub safety_reason: String,
    /// `safety_reason` with its distance in miles (`"Capped at 4 mi: …"`), for a
    /// shell showing miles. Empty when the reason states no distance.
    #[serde(default)]
    pub safety_reason_mi: String,
    #[serde(flatten)]
    pub evidence: EvidenceTagView,
    #[serde(default)]
    pub why: WhyView,
    /// True when this item's prescribed work is complete for its session day: a
    /// lift/calisthenics item once `sets_done >= max(sets,1)`, it is ended
    /// early, or a pain report stopped it; a run item once the run rule is met.
    /// The week strip hides the per-item Log action on this flag and the shell
    /// collapses done hero items. `false` for a day outside the plan window.
    #[serde(default)]
    pub done: bool,
    /// Where this item stands on its session day (`done` stays the "nothing
    /// left to log" flag; this says how it ended).
    #[serde(default)]
    pub status: ItemStatus,
    /// Qualifying same-movement sets attributed to this item's session day;
    /// 0 for runs. A set qualifies on movement + one rep (or hold second) + a
    /// work-set load, so short and lighter sets count (RPE never gates); see
    /// `logged_sets` for the per-set breakdown.
    #[serde(default)]
    pub sets_done: u8,
    /// Every same-movement set attributed to this item's session day, in
    /// chronological order — qualifying and non-qualifying (warm-ups / below
    /// target), each flagged. Empty for runs and days outside the plan window.
    #[serde(default)]
    pub logged_sets: Vec<ItemSetView>,
    /// The user ended this exercise early via `SetExerciseEnded`: the item is
    /// done regardless of `sets_done`. The shell shows "Reopen".
    #[serde(default)]
    pub ended_early: bool,
    /// For a run item, the total km / minutes of the runs attributed to this
    /// session day (0 for lifts/calisthenics). Drives the run completion rule.
    #[serde(default)]
    pub run_logged_km: f64,
    #[serde(default)]
    pub run_logged_min: f64,
    /// For a run item: numeric pace / heart-rate / effort targets.
    /// `None` for lifts/calisthenics. The `intensity_label` already reads the
    /// numbers; this carries them structured. Serde-default.
    #[serde(default)]
    pub run_target: Option<RunTargetView>,
    /// Calisthenics plan item data (ladder, rung, Easier/Harder, added-load
    /// capability, skill test hold). `None` for lifts and runs.
    #[serde(default)]
    pub calis: Option<CalisItemView>,
    /// For a run item: the numeric session structure a tracker can step
    /// through (warm-up, reps + recovery, cool-down, strides, run/walk bouts).
    /// `None` for lifts/calisthenics and for a plain continuous run with no
    /// warm-up. Serde-default; additive.
    #[serde(default)]
    pub run_structure: Option<RunStructureView>,
    /// The bold name line of a lift / calisthenics face (`"Back squat"`): the
    /// `summary` up to its `": "` / `" — "`. Empty for runs and the return check.
    #[serde(default)]
    pub face_name: String,
    /// The prescription line under `face_name` (`"5×3 @ 85 kg · ~2 reps
    /// left"`): the `summary` after the name. Empty when `face_name` is.
    #[serde(default)]
    pub face_detail: String,
    /// A compact row's one line: the first ` · ` clause of `face_detail`, else
    /// of `summary` (`"5×3 @ 85 kg"`, a run's `"8 km"`).
    #[serde(default)]
    pub face_lead: String,
    /// For a run item: the session volume its `summary` leads with, before the
    /// target (`"6 × 800 m"`, `"8 km"`, run/walk bouts), distances in km like
    /// `summary`. Empty when the summary leads with no volume (a pain stop, an
    /// easy cap of run/walk bouts) and for lifts.
    #[serde(default)]
    pub run_head: String,
    /// `run_head`'s distance in km as shown (`8.0` for `"8 km"`) when the head
    /// is a plain distance; `None` for reps, minutes and run/walk bouts.
    #[serde(default)]
    pub run_head_km: Option<f64>,
    /// For a run item: the shown whole-session volume as a number, so a
    /// readiness cut or an easy downgrade rebuilds `summary` without parsing
    /// it. Core-internal.
    #[serde(skip)]
    pub(crate) run_volume: Option<RunAmount>,
    /// For an interval/repetition run: the per-rep volume behind `rep_volume`.
    /// Core-internal.
    #[serde(skip)]
    pub(crate) rep_amount: Option<RunAmount>,
    /// For a run item: the judged day's runs cover at least half its planned
    /// volume, so a run under the done threshold still reads done lighter.
    /// Core-internal.
    #[serde(skip)]
    pub(crate) run_lighter: bool,
    /// The target is an RPE (`intensity_label` "RPE 8", `target_rpe` its
    /// value), so the face phrases it as reps left. Core-internal.
    #[serde(skip)]
    pub(crate) rpe_target: bool,
    /// The heaviest weight already lifted on this movement for at least
    /// `reps_low` reps in the recent anchor window, before this day: the load a
    /// held day (ready-upd-001, no load added) may not exceed. Core-internal.
    #[serde(skip)]
    pub(crate) hold_load_cap_kg: Option<f64>,
    /// `anchored_on` is a last-time line naming one set: the face already
    /// shows the last top set. Core-internal.
    #[serde(skip)]
    pub(crate) anchored_on_single_set: bool,
    /// The day took this lift's load away (an easy / pain-free day): the face
    /// says to pick a weight for the reps-left target. Core-internal.
    #[serde(skip)]
    pub(crate) pick_load: bool,
    /// The load `why.basis` states: the plan's derived load, before any fold,
    /// hold or in-session cue moved `load_kg`. Core-internal.
    #[serde(skip)]
    pub(crate) basis_load_kg: Option<f64>,
    /// A held day (ready-upd-001) clamped `load_kg` to `hold_load_cap_kg`.
    /// Core-internal.
    #[serde(skip)]
    pub(crate) load_held: bool,
}

/// What a plan item trains. On the wire a run item is the one with no
/// exercise name; a calisthenics item carries `calis`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Lift,
    Calisthenics,
    Run,
}

impl PrescriptionView {
    pub fn kind(&self) -> ItemKind {
        if self.exercise.is_empty() {
            ItemKind::Run
        } else if self.calis.is_some() {
            ItemKind::Calisthenics
        } else {
            ItemKind::Lift
        }
    }

    pub fn is_run(&self) -> bool {
        self.kind() == ItemKind::Run
    }
}

/// A run item's volume as a number (core-internal; the label lives in
/// `summary` / `rep_volume`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum RunAmount {
    Minutes(f64),
    /// Kilometres as the plan quantised them, shown by `fmt_run_km` ("8 km",
    /// "6.5 km").
    PlanKm(f32),
    /// A minutes band ("10–20 min"), never scaled.
    MinutesRange(u16, u16),
    /// Kilometres after a readiness change, shown to one decimal ("4.0 km").
    Km(f64),
    /// Run/walk bouts: no single number to scale.
    RunWalk,
}

/// Numeric structure of a prescribed run, every field 0/empty when it
/// does not apply. Built from the core's `RunPrescription`, never parsed from
/// the summary text.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct RunStructureView {
    /// Rep count of an interval/repetition set (0 = continuous).
    #[serde(default)]
    pub reps: u32,
    /// Rep length in metres for distance reps (0 otherwise).
    #[serde(default)]
    pub rep_m: u32,
    /// Rep length in seconds for timed reps (0 otherwise).
    #[serde(default)]
    pub rep_sec: u32,
    /// Recovery between reps, seconds (0 = not a number; see the two fields
    /// below).
    #[serde(default)]
    pub recovery_sec: u32,
    /// Recovery between reps as an easy jog of this many metres (0 = n/a).
    #[serde(default)]
    pub recovery_m: u32,
    /// Recovery = easy jog for as long as the rep took.
    #[serde(default)]
    pub recovery_equal_to_rep: bool,
    /// Easy warm-up before the main set, minutes (C25K: brisk walk).
    #[serde(default)]
    pub warmup_min: u32,
    /// Easy cool-down after the main set, minutes.
    #[serde(default)]
    pub cooldown_min: u32,
    /// Strides to finish with: count, seconds each, recovery seconds between.
    #[serde(default)]
    pub strides_reps: u32,
    #[serde(default)]
    pub strides_sec: u32,
    #[serde(default)]
    pub strides_recovery_sec: u32,
    /// Ordered run/walk bouts of a beginner session (empty otherwise).
    #[serde(default)]
    pub run_walk: Vec<RunWalkStepView>,
}

/// One run/walk bout: run `run_sec`, then walk `walk_sec` (0 on the last).
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct RunWalkStepView {
    pub run_sec: u32,
    pub walk_sec: u32,
}

/// The machine-readable intensity target behind [`PrescriptionView::intensity_label`].
/// Readiness adjustments apply against THIS (never by re-parsing the display
/// string) and the label is rendered from it. Defaults to `Prose`.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub enum PlanIntensity {
    /// A single reps-in-reserve target (barbell `LiftIntensity::Rir`), e.g.
    /// "RIR 3".
    Rir { rir: i32 },
    /// A reps-in-reserve BAND (calisthenics dynamic rungs), `lo`..=`hi`, e.g.
    /// "RIR 1–2". CapRpe raises the high end (and pulls the low end up only
    /// when the cap would otherwise fall below it).
    RirRange { lo: i32, hi: i32 },
    /// A percent-of-e1RM anchor. `reduced_from` records the pre-cut percent so a
    /// stacked load reduction re-derives "{pct}% e1RM (reduced from {orig}%)"
    /// straight from the numbers instead of parsing the label.
    PercentE1rm { pct: u32, reduced_from: Option<u32> },
    /// Any non-adjustable target rendered verbatim from `intensity_label`: RPE,
    /// velocity, run pace/zone, a timed-hold cue, or an already-downgraded line.
    #[default]
    Prose,
}

/// Where a planned day stands (wire: snake_case).
#[derive(Serialize, Deserialize, Debug, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    /// The hero: today's (or the next) session to train.
    Next,
    #[default]
    Planned,
    Done,
    /// Finished with lighter or fewer sets, a skipped exercise, or (on a past
    /// day) at least half the planned work sets.
    DoneLighter,
    Missed,
    /// A past day with some work logged, under the share that credits it
    /// "done lighter": partly done, neither done nor missed.
    Partial,
    /// A readiness adjustment changed the session (`adjustment` says which).
    Adjusted,
    /// A safety hold or a pain stop blanked the session.
    Blocked,
    Rest,
    /// A day before the plan started (no items, never counted).
    BeforePlan,
}

impl SessionStatus {
    /// A finished session: every item done, or done lighter.
    pub fn is_done(self) -> bool {
        matches!(self, SessionStatus::Done | SessionStatus::DoneLighter)
    }
}

/// One planned day in the week, flattened for shells.
/// The plan renders strictly downstream of the safety gates: a `train_blocked`
/// hold empties `items` and sets `status = "blocked"` (HARD RULE 3).
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct SessionPlanView {
    /// Calendar day of this session, unix epoch-day (days since 1970-01-01).
    pub epoch_day: i64,
    /// Human title, e.g. `"Heavy day"`, `"Long run"`, `"Rest"`.
    pub title: String,
    /// The day's session type, on the wire as its stable name, e.g.
    /// `"Lift(MaxEffort)"`, `"Rest"`.
    #[serde(with = "session_type_wire")]
    pub session_type: SessionType,
    pub status: SessionStatus,
    pub items: Vec<PrescriptionView>,
    /// The readiness adjustment folded into this session, when `status ==
    /// "adjusted"`. Carries the adjustment's own evidence.
    #[serde(default)]
    pub adjustment: Option<AdjustmentView>,
    /// Epoch days this session can be moved to (ADH-RESCHED-001):
    /// rest days of the same week from today on (today included) where the
    /// move keeps ~48 h before the next hard session and long/hard runs ≥24 h
    /// from heavy leg work. Send `Event::MoveSession { from_epoch_day:
    /// epoch_day, to_epoch_day }`. Empty for rest/done/blocked days.
    #[serde(default)]
    pub move_targets: Vec<i64>,
    /// The day this session was moved from, when the athlete moved it.
    #[serde(default)]
    pub moved_from_epoch_day: Option<i64>,
    /// Why a requested move of this session was refused, in plain language.
    #[serde(default)]
    pub move_note: Option<String>,
    /// Why no day is offered in `move_targets`, in plain language. Set only
    /// for a movable (not done, not rest, not blocked) session with no target.
    #[serde(default)]
    pub move_blocked_reason: Option<String>,
    /// The day the work that completed this session was actually logged, when
    /// a same-discipline session done on a nearby day of the week (a swap or a
    /// shift) is credited to it.
    #[serde(default)]
    pub credited_from_epoch_day: Option<i64>,
}

/// How one plan item stands on its session day (wire: snake_case).
#[derive(Serialize, Deserialize, Debug, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    /// Nothing logged yet.
    #[default]
    Open,
    /// Some sets (or some running) logged, not finished.
    InProgress,
    /// Every prescribed set at the prescribed load and reps (or the run rule met).
    Done,
    /// Finished with lighter sets (below the planned load) or fewer sets
    /// (ended early after some work).
    DoneLighter,
    /// Every set at the planned load, but some fell short of the prescribed
    /// reps (or hold time).
    DoneShort,
    /// Ended without any work set.
    Skipped,
    /// Stopped by a pain report before any work set.
    SkippedPain,
    /// Stopped by a pain report after some work sets were logged.
    StoppedPain,
}

/// The program summary card: what the plan IS, plus its
/// representative evidence chip.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ProgramSummaryView {
    pub name: String,
    /// Human goal, e.g. `"Strength"`, `"Hybrid"`, `"10K race"`.
    pub goal: String,
    /// Phase, e.g. `"Build"` or `"Deload"` (a recovery week).
    pub phase: MesoPhase,
    pub week: u8,
    pub weeks_total: u8,
    #[serde(flatten)]
    pub evidence: EvidenceTagView,
    #[serde(default)]
    pub why: WhyView,
    /// Hypertrophy goal only (empty otherwise): weekly working sets per muscle
    /// this week's plan delivers vs the target, Table 1 order. A set counts 1
    /// toward the muscle it trains directly and ½ toward each listed helper
    /// muscle (bench → chest 1, triceps ½).
    #[serde(default)]
    pub muscle_volume: Vec<MuscleVolumeView>,
    /// One plain-language line about the volume rows (specialization state, a
    /// muscle the plan has no exercise for, deload week), or empty.
    #[serde(default)]
    pub muscle_volume_note: String,
}

/// One muscle's weekly volume on the program card.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct MuscleVolumeView {
    /// Plain muscle name, e.g. `"Side delts"`.
    pub muscle: String,
    /// This week's target working sets (0 = no direct target, e.g. glutes,
    /// which squats and deadlifts already train).
    pub target_sets: u8,
    /// Fractional working sets the week's plan delivers (one decimal).
    pub delivered_sets: f32,
    /// Human line, e.g. `"Triceps: 12 sets (4.5 from presses) · target 12"`.
    pub label: String,
}

/// One row of the evidence-grade legend ("How evidence grading works"),
/// exported from core data so the shell renders the KB definitions rather than
/// hardcoding them. Carries no per-claim evidence;
/// it defines the grading scale itself (File 09).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct GradeDefView {
    /// Wire name matching the grade strings on every card, e.g. `"Strong"`.
    pub grade: EvidenceGrade,
    /// Friendly label, e.g. `"Expert opinion"`.
    pub label: String,
    /// One-sentence KB definition of the grade (File 09 scale).
    pub definition: String,
    /// Default confidence this grade maps to (File 09), e.g. `0.90`.
    pub confidence: f32,
}

/// Signal→group row for the static readiness-picker metadata (`signal_groups`).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SignalGroupView {
    pub signal: ReadinessSignal,
    /// `"metric"` or `"red_flag"`.
    pub group: crate::autoreg::SignalGroup,
}

/// A goal-race finish prediction, flattened for shells. Combines a Daniels VDOT
/// projection with a Riegel one (running-039): when the two agree within ~2% a
/// single `predicted` time is shown; otherwise a low–high range, so a single
/// method's false precision is never presented alone.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct RacePredictionView {
    /// Human label for the goal distance, e.g. `"10K"`, `"Half marathon"` or `"15 km"`.
    pub goal_label: String,
    /// Formatted finish time (`"41:30"`) or range (`"41:30–43:00"`).
    pub predicted: String,
    /// True when the two methods agreed (single time); false = range.
    pub agreed: bool,
    /// Range bounds in seconds (equal when `agreed`); 0.0 on degenerate input.
    pub low_sec: f64,
    pub high_sec: f64,
    pub summary: String,
    #[serde(flatten)]
    pub evidence: EvidenceTagView,
    /// Graded caveat rows riding on the prediction: input-race staleness
    /// (running-041) and the under-long-run marathon-optimism warning
    /// (running-040). Empty when nothing applies.
    #[serde(default)]
    pub notes: Vec<GuidanceView>,
    /// The raw inputs this prediction was computed from, echoed back so a shell
    /// can rehydrate its form after a log replay instead of falling back to a
    /// hardcoded default (single source of truth stays the core).
    #[serde(default)]
    pub recent_distance_m: f64,
    #[serde(default)]
    pub recent_time_sec: f64,
    #[serde(default)]
    pub goal_distance_m: f64,
    #[serde(default)]
    pub weekly_km: f64,
    #[serde(default)]
    pub weeks_since_race: Option<u8>,
}

/// The raw heart-rate-zone query echoed back so a shell rehydrates its form
/// (`app/mod.rs` `hr_zone_input`). Sibling to the `hr_zones` result rows.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct HrZoneInputView {
    pub age_years: f64,
    #[serde(default)]
    pub resting_hr_bpm: Option<f64>,
    #[serde(default)]
    pub weeks_since_recalc: Option<u8>,
    #[serde(default)]
    pub weeks_since_pace_test: Option<u8>,
}

/// The raw protein query echoed back so a shell rehydrates its form
/// (`app/mod.rs` `protein_input`). Sibling to the `protein_targets` result rows.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct ProteinInputView {
    pub bodyweight_kg: f64,
    pub masters: bool,
    pub deficit: bool,
}

/// Structured form of the HRmax figure, so a shell never regex-scrapes it out
/// of the `hr_zones` summary rows (incl. the "208 − 0.7 × age" Tanaka split).
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct HrMaxView {
    /// Resolved max heart rate, bpm, the measured value when available, else the
    /// age-based Tanaka estimate. Drives the %HRmax band targets in `hr_zones`.
    pub bpm: f64,
    /// `true` when `bpm` is the user's logged measured maximum; `false` when it
    /// is the Tanaka estimate (the `tanaka_*` fields then describe it).
    pub measured: bool,
    /// Age used for the estimate. `0.0` when a measured max bypassed age entirely.
    pub age_years: f64,
    /// Tanaka intercept (208) so the shell renders `208 − 0.7 × age` from data,
    /// not by paren-scraping the summary. `0.0` when `measured`.
    pub tanaka_intercept: f64,
    /// Tanaka slope (0.7). `0.0` when `measured`.
    pub tanaka_slope: f64,
}

/// Structured form of a protein target, so a shell never regex-scrapes it out
/// of the `protein_targets` summary rows. One per emitted row (masters and/or
/// deficit). Additive / serde-default.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ProteinFigureView {
    /// Which target this is: `"masters"` or `"deficit"`.
    pub kind: ProteinTargetKind,
    /// Low end of the daily target, grams. `0.0` when `refused`.
    pub low_g_per_day: f64,
    /// High end of the daily target, grams. `0.0` when `refused`.
    pub high_g_per_day: f64,
    /// `true` when a deficit target was refused (RED-S / low-energy-availability
    /// signal present, safety-022), no g/day figure is offered.
    pub refused: bool,
    /// Low end of the graded g/kg band the grams come from. `0.0` when `refused`.
    #[serde(default)]
    pub low_g_per_kg: f64,
    /// High end of the graded g/kg band. `0.0` when `refused`.
    #[serde(default)]
    pub high_g_per_kg: f64,
    /// Index of this figure's evidence row in `protein_targets`.
    #[serde(default)]
    pub row: u32,
}

/// One Daniels training band of the HR-zone table, structured so a shell draws
/// the table from numbers instead of the `hr_zones` prose.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct HrZoneBandView {
    /// Which Daniels band this is (`"Easy"` … `"Repetition"`), the typed key a
    /// shell colours the band by, never its position in the list.
    pub kind: crate::schema::VdotBand,
    /// Band name ("Easy", "Marathon", …), the same word its row leads with.
    pub name: String,
    /// %HRmax band, whole numbers (equal ends for a single value).
    pub pct_low: f64,
    pub pct_high: f64,
    /// The band's absolute bpm range, rounded.
    pub bpm_low: f64,
    pub bpm_high: f64,
    /// `true` when pace, not heart rate, governs the band (running-002): the
    /// row then names no bpm target.
    pub pace_governed: bool,
    /// Heart-rate-reserve (Karvonen) range when the resting HR selected it,
    /// rounded; `0.0` otherwise.
    pub karvonen_low_bpm: f64,
    pub karvonen_high_bpm: f64,
    /// Index of this band's evidence row in `hr_zones`.
    pub row: u32,
}

/// The hypertrophy plan's figures, structured so a shell shows the plan as
/// numbers (landmarks, reps-left schedule, frequency, rest, block shape) without
/// scraping `hypertrophy_plan`. `face_rows` lists the rows these figures stand
/// for. Additive / serde-default.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct HypertrophyFiguresView {
    /// Weekly-set landmarks: minimum (MEV), productive range (MAV), max (MRV).
    pub min_sets: u8,
    pub productive_low: u8,
    pub productive_high: u8,
    pub max_sets: u8,
    /// Reps left in reserve, week by week.
    pub rir_by_week: Vec<u8>,
    /// Peak-week sessions per week and sets per session.
    pub freq_low: u8,
    pub freq_high: u8,
    pub per_session_low: u8,
    pub per_session_high: u8,
    /// Rest windows between sets, seconds.
    pub rest_heavy_low_s: u16,
    pub rest_heavy_high_s: u16,
    pub rest_isolation_low_s: u16,
    pub rest_isolation_high_s: u16,
    /// Mesocycle shape: building weeks, then deload weeks.
    pub build_weeks_low: u8,
    pub build_weeks_high: u8,
    pub deload_weeks: u8,
    /// Indices of the `hypertrophy_plan` rows these figures stand for.
    pub face_rows: Vec<u32>,
}

/// Which protein target a [`ProteinFigureView`] is (wire: snake_case).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProteinTargetKind {
    Masters,
    Deficit,
}

/// The raw hypertrophy-plan query echoed back so a shell rehydrates its form
/// (`app/mod.rs` `hypertrophy_input`). Sibling to the `hypertrophy_plan` result rows.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct HypertrophyInputView {
    pub muscle: String,
    pub weeks: u8,
    pub not_growing: bool,
    pub recovering_easily: bool,
}

/// The raw APRE query echoed back so a shell rehydrates its form
/// (`app/mod.rs` `apre_input`). Sibling to the `apre` result rows.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ApreInputView {
    pub scheme: autoreg::ApreScheme,
    pub reps: u8,
    pub current_load_kg: f64,
}

/// The one resolved coaching message for a session, flattened for shells.
/// Safety concerns short-circuit and suppress competing praise (HARD RULE 3).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct FeedbackView {
    /// Category name, e.g. `"ConcernInjury"`.
    pub category: crate::feedback::FeedbackCategory,
    /// Human overline for the card face; render verbatim.
    pub category_label: String,
    /// Human-readable coaching copy for the category.
    pub message: String,
    /// `message` with its distances in miles, for a shell showing miles.
    /// Empty when the message states no distance.
    #[serde(default)]
    pub message_mi: String,
    /// True when this message suppresses all competing praise this cycle.
    pub suppresses_praise: bool,
    #[serde(flatten)]
    pub evidence: EvidenceTagView,
    /// Verbosity personalization (feedback-023/024): cap on takeaways the
    /// shell should render for this user (beginner: 1).
    #[serde(default)]
    pub max_takeaways: u8,
    /// Cap on metrics surfaced (advanced: up to 3; beginner: 1).
    #[serde(default)]
    pub max_metrics: u8,
    /// Always include the "why" (mandatory for beginners).
    #[serde(default)]
    pub rationale_mandatory: bool,
    /// Keep jargon minimal (beginners).
    #[serde(default)]
    pub minimize_jargon: bool,
    /// Praise copy must name a concrete mastery experience, a specific PR,
    /// completed session, or barrier overcome (feedback-005).
    #[serde(default)]
    pub anchor_mastery: bool,
    /// The three-part "why?" disclosure (basis → why this grade → what would
    /// improve it).
    #[serde(default)]
    pub why: WhyView,
}

/// One evidence-cited programming recommendation, flattened for shells.
/// `MarketingMyth`-graded claims are never emitted here.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct GuidanceView {
    /// Grouping label, e.g. `"Strength"`, `"Running"`, `"Hybrid"`.
    pub section: String,
    pub summary: String,
    /// `summary` with its distances in miles, for a shell showing miles.
    /// Empty when the summary states no distance.
    #[serde(default)]
    pub summary_mi: String,
    #[serde(flatten)]
    pub evidence: EvidenceTagView,
    /// The three-part "why?" disclosure (basis → why this grade → what would
    /// improve it).
    #[serde(default)]
    pub why: WhyView,
    /// On the profile's training-age row, the training age (`"Intermediate"`);
    /// empty on every other row.
    #[serde(default)]
    pub training_age: String,
}

/// The core-owned pacing verdict for a run's measured split (feedback-016/017;
/// FB-PACING-001). Carries everything a shell needs to render the chip -
/// verdict, label, coaching copy, and the full evidence tag, so the ~3%
/// threshold (`feedback::POSITIVE_SPLIT_FLAG_PCT`) never leaks into shell code.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SplitVerdictView {
    /// `"fade"` (back half >3% slower), `"even"` (within ±3%), or
    /// `"negative"` (back half >3% faster).
    pub verdict: SplitVerdict,
    /// Short chip label, e.g. `"FADE +5%"`, `"EVEN SPLIT"`, `"NEG SPLIT 4%"`.
    pub label: String,
    /// Evidence-backed coaching copy: the feedback-016 easier-start cue on a
    /// fade, the feedback-017 pacing-discipline praise otherwise.
    pub message: String,
    #[serde(flatten)]
    pub evidence: EvidenceTagView,
}

/// How a run's back half compared with its front half (wire: snake_case).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SplitVerdict {
    Fade,
    Even,
    Negative,
}

/// Core-owned interval-vs-steady verdict for a GPS run (RUN-INTERVAL-VI-001).
/// Built from the track's variability index (normalized speed ÷ average speed);
/// lets the shell render why two runs of the *same average pace* rate
/// differently, with the same evidence chrome as any other recommendation.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct IntervalVerdictView {
    /// `"interval"` (variability index ≥ threshold) or `"steady"`. Wire string
    /// the shell matches on, never re-derives the threshold.
    pub kind: PaceVariability,
    /// Short chip label, e.g. `"INTERVAL · VI 1.7"` / `"STEADY · VI 1.0"`.
    pub label: String,
    /// Evidence-backed copy explaining the differentiation honestly.
    pub message: String,
    /// The variability index itself (normalized speed ÷ average speed), 2 dp.
    pub variability_index: f64,
    #[serde(flatten)]
    pub evidence: EvidenceTagView,
}

/// Whether a GPS run was paced as intervals or steadily (wire: snake_case).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaceVariability {
    Interval,
    Steady,
}

/// One per-unit (per-km or per-mile) split of a GPS run, flattened for shells.
/// Purely descriptive, a measurement of the run, not a recommendation, so it
/// carries no evidence chrome. Built from [`running::track_splits`]; the core
/// owns the pace formatting so a shell renders `pace` verbatim.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct RunSplitView {
    /// 1-based split index (1 = first km/mile).
    pub index: u32,
    /// Pace over this split as `m:ss` (seconds per km / per mile; the final
    /// partial split's pace is normalized to a full unit so it stays comparable).
    pub pace: String,
    /// The same normalized seconds-per-km/mile figure `pace` is formatted from,
    /// as whole seconds — the numeric pace a shell needs to draw a splits bar
    /// chart without re-parsing the `m:ss` string. Both are derived from one
    /// value at the construction site, so `pace == fmt_mss(seconds_per_unit)`
    /// is guaranteed.
    #[serde(default)]
    pub seconds_per_unit: u32,
    /// Cumulative track distance at the END of this split, in km (both the km
    /// and mile lists express the running total in km).
    pub distance_km: f64,
    /// True only for a final split shorter than a full unit.
    pub partial: bool,
}

/// One logged run with derived zone / pace / spike flag, flattened for shells.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct RunResultView {
    /// 3-zone lactate classification from % HRmax, e.g. `"Z2"`. `"-"` (`None`)
    /// when the run has no HR sample, or when the athlete rated the session at
    /// or above the tempo effort floor while its HR reads Z1 (the label would
    /// contradict the effort; see `hr_label`).
    #[serde(with = "zone_or_dash")]
    pub zone: Option<ThreeZone>,
    /// Pace as `m:ss/km`.
    pub pace: String,
    /// Measured distance in km (0.0 for a GPS run with no usable fixes). Lets a
    /// shell render the run's structured fields without re-parsing `summary`.
    pub distance_km: f64,
    /// Duration in minutes (moving time for a GPS run). Echoed so the shell can
    /// prefill the run editor for a manual-run amend. 0.0 when
    /// unmeasurable.
    #[serde(default)]
    pub duration_min: f64,
    /// The run's average % HRmax as logged (0.0 = no HR sample). Echoed for the
    /// manual-run edit prefill. Serde-default.
    #[serde(default)]
    pub hr_pct_max: f64,
    /// Core-owned heart-rate line for the run card, rendered verbatim: the
    /// average bpm the athlete entered when known ("148 bpm · Z2"), else the
    /// whole-percent %HRmax ("HR 80% · Z2"); the zone is left off when `zone`
    /// is `"-"`. Empty when the run has no heart-rate figure. Serde-default.
    #[serde(default)]
    pub hr_label: String,
    /// The average heart rate the athlete entered, bpm, exactly as stored
    /// (`hr_pct_max` is then derived from it). `None` when not entered. A shell
    /// re-logging the run (delete → undo) sends it back as `avg_hr_bpm` so the
    /// restored run is identical. Serde-default.
    #[serde(default)]
    pub avg_hr_bpm: Option<f64>,
    /// True when this run's distance spikes >10% over the recent longest.
    pub spike_flag: bool,
    /// Why the spike gate fired, phrased honestly (empty when it did not). A
    /// first run with no baseline says so rather than claiming a ">10%" jump.
    pub spike_note: String,
    /// Second-half pace slowdown percent for a GPS-tracked run (a positive split;
    /// positive = slowed in the back half). `None` for a hand-entered run or a
    /// track too short/degenerate to split. Descriptive, not a prescription.
    pub split_pct: Option<f64>,
    /// Core-owned pacing verdict + chip data for `split_pct` (feedback-016/017).
    /// `None` when `split_pct` is `None` and when the track has fewer than two
    /// full km splits (too short to judge pacing).
    #[serde(default)]
    pub split: Option<SplitVerdictView>,
    /// Core-owned interval-vs-steady verdict from the track's variability index
    /// (RUN-INTERVAL-VI-001). `None` for a hand-entered run or a track too short
    /// to derive a variability index.
    #[serde(default)]
    pub interval: Option<IntervalVerdictView>,
    /// User-declared run-intent label echoed for history display; the weekly
    /// hard-session checks honor it as the session's declared goal. USER DATA,
    /// carries no evidence of its own. `None` = untagged.
    #[serde(default)]
    pub workout_type: Option<WorkoutType>,
    /// One-line run recap. For a MEASURED run this is internal/citation-only -
    /// never rendered; the shell builds unit-aware labels (`runDistLabel` /
    /// `runPaceLabel`), so the hardcoded `km` here never reaches a mi-preferring
    /// user. Only the zero-distance fallback ("GPS signal too poor to measure
    /// this run", no unit) is surfaced, in `RunCard`.
    pub summary: String,
    /// Evidence backing the spike gate.
    pub citation: String,
    /// Evidence grade backing the spike gate, e.g. `"Strong"`; empty for an
    /// unmeasurable run (no gate ran).
    #[serde(default, with = "grade_or_empty")]
    pub grade: Option<EvidenceGrade>,
    /// Confidence score of the spike-gate claim (0.0 when no gate ran).
    #[serde(default)]
    pub confidence: f32,
    /// Whether the spike-gate claim is safety-critical.
    #[serde(default)]
    pub safety_critical: bool,
    /// Whether the spike-gate claim is contested.
    #[serde(default)]
    pub contested: bool,
    /// GPS fixes dropped by the quality gates before distance/pace were
    /// derived (File 07 QC: accuracy >30 m, implied speed >12 m/s, <2.5 m
    /// move, or a non-advancing timestamp). 0 for a hand-entered run.
    #[serde(default)]
    pub qc_dropped: u32,
    /// Whether this run has an exportable GPS route (≥2 QC-passing fixes). The
    /// full GPX document (a ~0.5 MB string) stays out of the list payload, which
    /// is re-serialized over JNI on every event; the shell reads this cheap flag
    /// to decide whether to offer the map / share affordances and fetches the
    /// document on demand via the `exportGpx` FFI query. A hand-entered run has
    /// no route.
    #[serde(default)]
    pub has_gpx: bool,
    /// When the run was logged, unix seconds; 0 when undated. A shell dates the
    /// history card from this; the core does no formatting (it holds no clock).
    pub observed_at: i64,
    /// Stable per-entry id echoed for the shell's edit/delete affordance; see
    /// [`LiftResultView::entry_id`]. 0 for a legacy row.
    #[serde(default)]
    pub entry_id: u64,
    /// Per-kilometre splits for a GPS run (one entry per completed km + a final
    /// partial), derived core-side from the same QC-gated track as
    /// distance/pace. Empty for a hand-entered run or a track too short to split.
    #[serde(default)]
    pub splits_km: Vec<RunSplitView>,
    /// Per-mile splits for a GPS run (same shape and track as
    /// [`splits_km`](Self::splits_km),
    /// unit = one international mile). Empty for a hand-entered run.
    #[serde(default)]
    pub splits_mi: Vec<RunSplitView>,
    /// Structured form of the spike-baseline provenance, so no shell scrapes it
    /// from `spike_note` (`contains("no prior run")`). `true` when a
    /// prior 30-day baseline distance exists to gauge this run against; `false`
    /// for a first run with no baseline (the spike gate then errs safe). Only
    /// meaningful when `spike_flag`.
    #[serde(default)]
    pub spike_has_baseline: bool,
    /// Whole-session effort, CR-10 (0–10), as rated by the athlete
    /// (LOAD-SRPE-001). `None` = unrated: the shell may offer "Rate this run"
    /// via `Event::AmendRunMeta`. Serde-default.
    #[serde(default)]
    pub session_rpe: Option<u8>,
    /// Session-RPE load in arbitrary units = moving minutes × `session_rpe`
    /// (Foster; LOAD-SRPE-001). `None` when unrated or unmeasurable. A separate
    /// measure from the heart-rate load, never added to it. Serde-default.
    #[serde(default)]
    pub srpe_load_au: Option<f64>,
    /// Best-effort records this run currently holds, e.g. `["Fastest 5K"]`
    /// (see [`RunProgressView::best_efforts`]). Empty otherwise. Serde-default.
    #[serde(default)]
    pub best_effort_labels: Vec<String>,
}

/// Running progress over time (self-monitoring against the plan,
/// ADH-SELFMON-001): 16-week weekly volume, the longest run, and best efforts.
/// Factual aggregates of logged runs only; best efforts are never fed into
/// training paces (a training-run segment is not a race effort).
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct RunProgressView {
    /// The last 16 Monday-start local weeks, oldest first, ending with the
    /// current week (empty weeks included as zeros).
    pub weekly: Vec<RunWeekView>,
    /// Planned weekly km from the profile, for a target marker (0 = none).
    #[serde(default)]
    pub planned_weekly_km: f64,
    /// Longest logged run ever, km (0 when none) and when it was logged.
    pub longest_run_km: f64,
    #[serde(default)]
    pub longest_run_observed_at: i64,
    /// Fastest known time per standard distance (1K, 5K, 10K, half marathon),
    /// from GPS splits (fastest contiguous stretch) or a whole hand-logged run
    /// of that distance. Only distances with an effort are listed.
    pub best_efforts: Vec<BestEffortView>,
    pub citation: String,
    #[serde(with = "grade_or_empty")]
    pub grade: Option<EvidenceGrade>,
    pub confidence: f32,
}

/// One Monday-start local week of running.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct RunWeekView {
    /// Local epoch day of the week's Monday.
    pub week_start_epoch_day: i64,
    pub km: f64,
    pub longest_km: f64,
    pub runs: u32,
    pub moving_min: f64,
    /// Session-RPE load for the week (sum of minutes × CR-10 over rated runs;
    /// LOAD-SRPE-001), its own series. 0 when nothing was rated.
    #[serde(default)]
    pub srpe_load_au: f64,
    /// Rated runs counted in `srpe_load_au`.
    #[serde(default)]
    pub rated_runs: u32,
}

/// The fastest known effort over one standard distance.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct BestEffortView {
    /// Standard distance in metres: 1000, 5000, 10000 or 21097.
    pub distance_m: u32,
    /// Plain label, e.g. `"5K"`, `"Half marathon"`.
    pub label: String,
    pub time_sec: u32,
    /// `h:mm:ss` / `m:ss` finish time.
    pub time_label: String,
    /// Average pace over the effort, `m:ss /km`.
    pub pace_label: String,
    /// The run holding the record (identity for the shell's PR chip).
    pub entry_id: u64,
    pub observed_at: i64,
    /// `true` when measured from a stretch of a longer GPS run; `false` for a
    /// whole run of that distance.
    pub from_split: bool,
}

/// Numeric execution targets for one run prescription: a pace range
/// from the athlete's current race fitness, a heart-rate range when HRmax is
/// known, and an effort cue (RPE, plus the talk test when nothing is measured).
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct RunTargetView {
    /// Training band, plain word: `"easy"`, `"recovery"`, `"marathon pace"`,
    /// `"threshold"`, `"interval"`, `"repetition"`, `"hill effort"`.
    pub band: String,
    /// Pace range, seconds per km (fast, slow). Both 0 without a pace anchor.
    /// A recovery run carries only `pace_slow_sec_per_km` ("slower than");
    /// a repetition run only `pace_fast_sec_per_km` ("this fast or faster").
    pub pace_fast_sec_per_km: u32,
    pub pace_slow_sec_per_km: u32,
    /// Rendered pace in km and miles, e.g. `"4:50–5:05 /km"` / `"7:47–8:11 /mi"`.
    /// Empty without a pace anchor.
    pub pace_label_km: String,
    pub pace_label_mi: String,
    /// Heart-rate range in bpm (0 = not given). Not given for interval /
    /// repetition work, where pace governs and heart rate lags.
    pub hr_low_bpm: u32,
    pub hr_high_bpm: u32,
    /// e.g. `"150–162 bpm"` / `"≤ 143 bpm"`; empty when not given.
    pub hr_label: String,
    /// CR-10 effort band (both 0 when the evidence states none).
    pub rpe_low: u8,
    pub rpe_high: u8,
    /// Always-present plain effort cue, e.g. `"effort 3–4 of 10, easy enough to
    /// talk in full sentences"`.
    pub effort_cue: String,
    /// `effort_cue` without its leading "effort" word, for a face line that
    /// labels it itself (`"3–4 of 10, easy enough to talk in full sentences"`).
    #[serde(default)]
    pub effort_detail: String,
    /// The fitness score behind the paces (0 = none) and the heart-rate figures
    /// used (0 = unknown), so a downgrade can re-derive the easy band.
    #[serde(default)]
    pub vdot: f64,
    #[serde(default)]
    pub hr_max_bpm: f64,
    #[serde(default)]
    pub hr_max_measured: bool,
    #[serde(default)]
    pub hr_rest_bpm: f64,
    /// Evidence for the numeric anchor (pace: RUN-VDOT-001; heart rate:
    /// RUN-HRMAX-001; talk test: RUN-TALKTEST-001).
    pub citation: String,
    #[serde(with = "grade_or_empty")]
    pub grade: Option<EvidenceGrade>,
    pub confidence: f32,
}
