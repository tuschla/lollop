package app.milestone

import kotlinx.serialization.Serializable

// The core's wire types, platform-free: the decoded view model and the enums
// events carry. The Core object (JNI, event log) and the Event encoders stay in
// androidMain's Core.kt.

// ── View model (decode) ──────────────────────────────────────────────────────
// Mirrors shared/src/app/view_types.rs (serde names). Unknown keys are ignored.

@Serializable
data class ViewModel(
    val safety_tier: String? = null,
    val adjustments: List<AdjustmentView> = emptyList(),
    val review_adjustments: List<AdjustmentView> = emptyList(),
    val weekly_checkin_present: Boolean = false,
    // True: the morning check-in asks the Illness item first.
    val illness_check_prompt: Boolean = false,
    // Empty for a Weights profile.
    val calisthenics_ladders: List<CalisLadderView> = emptyList(),
    val input_count: Int = 0,
    val lifts: List<LiftResultView> = emptyList(),
    val runs: List<RunResultView> = emptyList(),
    val guidance: List<GuidanceView> = emptyList(),
    val feedback: FeedbackView? = null,
    val reference: List<GuidanceView> = emptyList(),
    val profile: ProfileView? = null,
    val race_prediction: RacePredictionView? = null,
    val hypertrophy_plan: List<GuidanceView> = emptyList(),
    val protein_targets: List<GuidanceView> = emptyList(),
    val hr_zones: List<GuidanceView> = emptyList(),
    // Null until a dated run with heart rate exists.
    val training_load: TrainingLoadView? = null,
    // Null until a run is rated.
    val srpe_load: SrpeLoadView? = null,
    // Null until a dated run with a measurable distance exists.
    val run_progress: RunProgressView? = null,
    // Empty until a dated run exists.
    val weekly_report: List<GuidanceView> = emptyList(),
    val lift_audit: List<GuidanceView> = emptyList(),
    // Calculator inputs echoed for form rehydration.
    val hr_zone_input: HrZoneInputView? = null,
    val protein_input: ProteinInputView? = null,
    val hypertrophy_input: HypertrophyInputView? = null,
    // Weekly direct-set counts; empty with no plan, a hold or an unknown muscle.
    val hypertrophy_ramp: List<Int> = emptyList(),
    val trend: AdjustmentView? = null,
    val provisional: AdjustmentView? = null,
    // Null until any readiness input exists.
    val autoreg_source: AdjustmentView? = null,
    val readiness_summary: List<ReadinessSignalView> = emptyList(),
    val today_headline: TodayHeadlineView? = null,
    val signal_groups: List<SignalGroupView> = emptyList(),
    // The latest check-in of any day; null until a check-in exists.
    val latest_checkin: CheckinEchoView? = null,
    val baseline_status: List<BaselineStatusView> = emptyList(),
    val grade_definitions: List<GradeDefView> = emptyList(),
    // Null/empty until the user accepts a plan.
    val next_session: SessionPlanView? = null,
    val week_plan: List<SessionPlanView> = emptyList(),
    // Next week's seven days as next week's own week_plan lays them out.
    val next_week_plan: List<SessionPlanView> = emptyList(),
    val program: ProgramSummaryView? = null,
    // Null until an HR-zone query is made, or when the age is out of range.
    val hr_max: HrMaxView? = null,
    // Empty until a protein query is made.
    val protein_figures: List<ProteinFigureView> = emptyList(),
    // Empty with no HR-zone query or a refused age.
    val hr_zone_bands: List<HrZoneBandView> = emptyList(),
    // Null whenever the planner shows no ramp.
    val hypertrophy_figures: HypertrophyFiguresView? = null,
    // The day SetToday last set; every plan gate reads this instead of the wall
    // clock. Null before the first SetToday: fall back to todayEpochDay().
    val today_epoch_day: Long? = null,
    // Null until today's planned session is complete.
    val session_done: SessionDoneView? = null,
    val hold_resolutions: List<HoldResolutionView> = emptyList(),
    // Null without a plan.
    val consistency: ConsistencyView? = null,
    // Null when training is not held: parq | youth | pregnancy | reds | injury_rehab |
    // symptomatic | acute_illness_profile | illness | pain | medical_signal.
    val hold_reason: String? = null,
    // False: the readiness pills come from an earlier check-in.
    val checked_in_today: Boolean = false,
    // health_hold | nothing_to_plan; null when GeneratePlan can build a plan.
    val plan_withheld: String? = null,
    val activities: List<ActivityResultView> = emptyList(),
    // Set only for an under-18 profile once SetToday has run.
    val youth_activity: YouthActivityView? = null,
) {
    /** Training is on hold (a Stop-level or rest-day call): [hold_reason] is set. */
    val train_blocked: Boolean get() = hold_reason != null
}

/** Mirrors view_types.rs HrMaxView. `tanaka_intercept`/`tanaka_slope` are 0 when [measured]. */
@Serializable
data class HrMaxView(
    val bpm: Double = 0.0,
    val measured: Boolean = false,
    val age_years: Double = 0.0,
    val tanaka_intercept: Double = 0.0,
    val tanaka_slope: Double = 0.0,
)

/** Mirrors view_types.rs ProteinFigureView. `kind`: "masters" | "deficit". g/day
 *  figures are 0 when [refused]; [row] indexes protein_targets. */
@Serializable
data class ProteinFigureView(
    val kind: String = "",
    val low_g_per_day: Double = 0.0,
    val high_g_per_day: Double = 0.0,
    val refused: Boolean = false,
    val low_g_per_kg: Double = 0.0,
    val high_g_per_kg: Double = 0.0,
    val row: Int = 0,
)

/** Mirrors view_types.rs HrZoneBandView. `pct_*` is %HRmax; a `pace_governed` band has
 *  no bpm target; `karvonen_*` is 0 without a resting HR; [row] indexes hr_zones. */
@Serializable
data class HrZoneBandView(
    val kind: String = "",
    val name: String = "",
    val pct_low: Double = 0.0,
    val pct_high: Double = 0.0,
    val bpm_low: Double = 0.0,
    val bpm_high: Double = 0.0,
    val pace_governed: Boolean = false,
    val karvonen_low_bpm: Double = 0.0,
    val karvonen_high_bpm: Double = 0.0,
    val row: Int = 0,
)

/** Mirrors view_types.rs HypertrophyFiguresView. Rest windows in seconds;
 *  [face_rows] index hypertrophy_plan, every other row is a note. */
@Serializable
data class HypertrophyFiguresView(
    val productive_low: Int = 0,
    val productive_high: Int = 0,
    val max_sets: Int = 0,
    val rir_by_week: List<Int> = emptyList(),
    val freq_low: Int = 0,
    val freq_high: Int = 0,
    val per_session_low: Int = 0,
    val per_session_high: Int = 0,
    val rest_heavy_low_s: Int = 0,
    val rest_heavy_high_s: Int = 0,
    val rest_isolation_low_s: Int = 0,
    val rest_isolation_high_s: Int = 0,
    val build_weeks_low: Int = 0,
    val build_weeks_high: Int = 0,
    val deload_weeks: Int = 0,
    val face_rows: List<Int> = emptyList(),
)

/** Mirrors view_types.rs WhyView. All empty: the card shows the plain evidence restatement. */
@Serializable
data class WhyView(
    val basis: String = "",
    val grade_note: String = "",
    val improves: String = "",
)

/** Mirrors view_types.rs PrescriptionView. `load_kg` is null unless anchored on a
 *  logged e1RM; `intensity_label` then carries the RIR/pace target. */
@Serializable
data class PrescriptionView(
    val summary: String = "",
    val exercise: String = "",
    val sets: Int = 0,
    val reps_low: Int = 0,
    val reps_high: Int = 0,
    val load_kg: Double? = null,
    val intensity_label: String = "",
    val rest_sec: Int = 0,
    // Timed-hold rung: rep_volume carries the hold band, reps_low/high stay 0.
    val timed_hold: Boolean = false,
    val target_rpe: Double? = null,
    val hold_low_sec: Int = 0,
    val hold_high_sec: Int = 0,
    // 0.0 = bodyweight.
    val added_kg: Double? = null,
    val rep_volume: String = "",
    val anchored_on: String = "",
    val anchored_on_last: Boolean = false,
    val adjusted_note: String = "",
    val return_check: Boolean = false,
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
    val why: WhyView = WhyView(),
    val done: Boolean = false,
    // sets_done is 0 for runs; run_logged_km/run_logged_min are 0 for other items.
    val sets_done: Int = 0,
    val logged_sets: List<ItemSetView> = emptyList(),
    val ended_early: Boolean = false,
    val run_logged_km: Double = 0.0,
    val run_logged_min: Double = 0.0,
    val last_set: ItemSetView? = null,
    // Starting weight (kg) for an effort-prescribed item with no load_kg.
    val suggested_load_kg: Double? = null,
    // Plates per side of a standard bar, in the display unit, largest first.
    val plates_per_side: List<Double> = emptyList(),
    // Short execution cue ("move fast"); already part of `summary`.
    val effort_cue: String = "",
    // Null for lifts and runs.
    val calis: CalisItemView? = null,
    // Run items only; run_structure is also null for a plain continuous run.
    val run_target: RunTargetView? = null,
    val run_structure: RunStructureView? = null,
    // open | in_progress | done | done_lighter | skipped | skipped_pain.
    val status: String = "open",
    // Why a safety cap shaped this item, distances in km; empty otherwise.
    val safety_reason: String = "",
    // `safety_reason` with its distance in miles; empty when it states none.
    val safety_reason_mi: String = "",
    // Face parts of `summary` (km); face_name/face_detail are empty for runs and the
    // return check.
    val face_name: String = "",
    val face_detail: String = "",
    val face_lead: String = "",
    val run_head: String = "",
    // The run head's km as shown when the head is a plain distance ("8 km" → 8.0).
    val run_head_km: Double? = null,
)

/** Mirrors view_types.rs RunTargetView. Pace in seconds per km (0 = none); the
 *  labels are display-ready. */
@Serializable
data class RunTargetView(
    val band: String = "",
    val pace_fast_sec_per_km: Int = 0,
    val pace_slow_sec_per_km: Int = 0,
    val pace_label_km: String = "",
    val pace_label_mi: String = "",
    val hr_label: String = "",
    val effort_cue: String = "",
    // `effort_cue` without its leading "effort" word ("3–4 of 10").
    val effort_detail: String = "",
    val citation: String = "",
    val grade: String = "",
    val confidence: Float = 0f,
)

/** Mirrors view_types.rs RunStructureView; 0/empty when a field does not apply. */
@Serializable
data class RunStructureView(
    val reps: Int = 0,
    val rep_m: Int = 0,
    val rep_sec: Int = 0,
    val recovery_sec: Int = 0,
    val recovery_m: Int = 0,
    val recovery_equal_to_rep: Boolean = false,
    val warmup_min: Int = 0,
    val cooldown_min: Int = 0,
    val strides_reps: Int = 0,
    val strides_sec: Int = 0,
    val strides_recovery_sec: Int = 0,
    val run_walk: List<RunWalkStepView> = emptyList(),
)

/** One run/walk bout of a beginner session: run [run_sec], then walk [walk_sec]. */
@Serializable
data class RunWalkStepView(
    val run_sec: Int = 0,
    val walk_sec: Int = 0,
)

/** Mirrors view_types.rs ItemSetView. `summary` is display-ready; a warm-up or
 *  below-target set has `qualifies` false. */
@Serializable
data class ItemSetView(
    val entry_id: Long = 0,
    val observed_at: Long = 0,
    val weight_kg: Double = 0.0,
    val reps: Int = 0,
    val rpe: Double = 0.0,
    val hold_sec: Int? = null,
    val qualifies: Boolean = false,
    // A counted set short of the prescribed reps / hold time ("missed reps").
    val short: Boolean = false,
    val summary: String = "",
)

/** One rung of a calisthenics ladder (view_types.rs CalisRungView). */
@Serializable
data class CalisRungView(
    val rung: Int = 0,
    val name: String = "",
    val target: String = "",
    val available: Boolean = false,
    // A timed-hold rung (its `target` is a hold band).
    val timed_hold: Boolean = false,
)

/** Mirrors view_types.rs CalisItemView. `load_capable`: show the added-load field
 *  even at 0 kg. `ballistic`: no RPE hint. */
@Serializable
data class CalisItemView(
    val ladder: CalisLadder = CalisLadder.HorizontalPush,
    val rung: Int = 0,
    val easier: CalisRungView? = null,
    val harder: CalisRungView? = null,
    val load_capable: Boolean = false,
    val ballistic: Boolean = false,
    val needs_effort_rating: Boolean = false,
)

/** One calisthenics ladder for pickers and the Log chooser (view_types.rs
 *  CalisLadderView). `pattern` is the movement name, e.g. "Vertical pull". */
@Serializable
data class CalisLadderView(
    val ladder: CalisLadder = CalisLadder.HorizontalPush,
    val pattern: String = "",
    val skill: Boolean = false,
    val power: Boolean = false,
    val current_rung: Int? = null,
    val rungs: List<CalisRungView> = emptyList(),
    // The rung the athlete placed themselves on, before the equipment check
    // (current_rung is the rung the kit allows; null when none fits).
    val placed_rung: Int? = null,
    // Empty unless the kit limits this ladder.
    val equipment_note: String = "",
)

/** Mirrors view_types.rs SessionPlanView. A hold sets status "blocked" and empties
 *  items. status: next | planned | done | done_lighter | missed | partial | adjusted |
 *  blocked | rest | before_plan. */
@Serializable
data class SessionPlanView(
    val epoch_day: Long = 0,
    val title: String = "",
    val session_type: String = "",
    val status: String = "",
    val items: List<PrescriptionView> = emptyList(),
    val adjustment: AdjustmentView? = null,
    // Event.MoveSession targets; empty for rest/done days.
    val move_targets: List<Long> = emptyList(),
    // The day the athlete moved this session from, when moved.
    val moved_from_epoch_day: Long? = null,
    // Why a requested move was refused, plain language.
    val move_note: String? = null,
    // The day whose logged session credited this one (the strip reads "Done Tue").
    val credited_from_epoch_day: Long? = null,
    // Why this session cannot move, plain language, when move_targets is empty.
    val move_blocked_reason: String? = null,
)

/** Mirrors view_types.rs ProgramSummaryView. */
@Serializable
data class ProgramSummaryView(
    val name: String = "",
    val goal: String = "",
    val phase: String = "",
    val week: Int = 0,
    val weeks_total: Int = 0,
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
    val why: WhyView = WhyView(),
    // Hypertrophy goal only; empty otherwise.
    val muscle_volume: List<MuscleVolumeView> = emptyList(),
    val muscle_volume_note: String = "",
)

/** Mirrors view_types.rs MuscleVolumeView. `target_sets` 0 = no direct target;
 *  `label` is display-ready. */
@Serializable
data class MuscleVolumeView(
    val muscle: String = "",
    val target_sets: Int = 0,
    val delivered_sets: Float = 0f,
    val label: String = "",
)

/** Mirrors view_types.rs SessionDoneView. */
@Serializable
data class SessionDoneView(
    val epoch_day: Long = 0,
    val title: String = "",
    val items: List<SessionDoneItemView> = emptyList(),
)

/** Mirrors view_types.rs SessionDoneItemView. status: done | done_short | done_lighter |
 *  skipped | skipped_pain | stopped_pain. Run figures in km / min; `summary` is km only. */
@Serializable
data class SessionDoneItemView(
    val exercise: String = "",
    val summary: String = "",
    val met: Boolean = false,
    val status: String = "",
    val run_km: Double? = null,
    val run_min: Double? = null,
    val run_planned_km: Double? = null,
    val run_planned_min: Double? = null,
)

/** Mirrors view_types.rs HoldResolutionView. `signal` is the ResolveHold wire name. */
@Serializable
data class HoldResolutionView(
    val signal: String = "",
    val paused_text: String = "",
    val action_label: String = "",
    val confirm_text: String = "",
    // Confirming also sends a SetProfile clearing the profile's illness answer.
    val ends_profile_illness: Boolean = false,
)

/** Mirrors view_types.rs ConsistencyView. */
@Serializable
data class ConsistencyView(
    val this_week_done: Int = 0,
    val this_week_planned: Int = 0,
    // The last (up to) 8 finished plan weeks, oldest first.
    val weeks: List<ConsistencyWeekView> = emptyList(),
    val share_pct: Int? = null,
    val repeating_week: Boolean = false,
    // The done/planned summary first, then any missed / repeated-week / re-entry note.
    val notes: List<GuidanceView> = emptyList(),
)

@Serializable
data class ConsistencyWeekView(
    val week_monday_epoch_day: Long = 0,
    val done: Int = 0,
)

/** Mirrors view_types.rs GradeDefView. */
@Serializable
data class GradeDefView(
    val grade: String = "",
    val label: String = "",
    val definition: String = "",
    val confidence: Float = 0f,
)

/** Mirrors view_types.rs CheckinEchoView. */
@Serializable
data class CheckinEchoView(
    val observed_at: Long = 0,
    val sleep_quality: Int? = null,
    val soreness: Int? = null,
    val mood: Int? = null,
    val fatigue: Int? = null,
    val stress: Int? = null,
    val resting_hr_bpm: Double? = null,
    val hrv_rmssd_ms: Double? = null,
)

/** Mirrors view_types.rs BaselineStatusView. */
@Serializable
data class BaselineStatusView(
    val signal: String = "",
    val label: String = "",
    val have: Int = 0,
    val need: Int = 0,
    // "Collecting" | "SparseWeek".
    val kind: String = "Collecting",
)

/** Mirrors view_types.rs ReadinessSignalView. `grade` is empty on factual rows that
 *  judge nothing. */
@Serializable
data class ReadinessSignalView(
    val signal: String = "",
    val group: String = "", // "metric" | "red_flag"
    val value: Double = 0.0,
    val state: String = "",
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
    // Empty: the shell falls back to its own echo.
    val detail: String = "",
    // Defaults true so a missing field never reads as stale.
    val from_today: Boolean = true,
    // True: `state` is the core's "out of date" text; add no "earlier" qualifier.
    val stale: Boolean = false,
)

/** Mirrors view_types.rs TodayHeadlineView. `kind`: safety_hold | adjustment |
 *  feedback | prescription | all_clear. */
@Serializable
data class TodayHeadlineView(
    val kind: String = "",
    val summary: String = "",
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
    val why: WhyView = WhyView(),
)

/** Mirrors view_types.rs SignalGroupView. */
@Serializable
data class SignalGroupView(
    val signal: String = "",
    val group: String = "",
)

/** Mirrors view_types.rs RacePredictionView. */
@Serializable
data class RacePredictionView(
    val goal_label: String = "",
    val predicted: String = "",
    val summary: String = "",
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
    // Inputs echoed for form rehydration.
    val recent_distance_m: Double = 0.0,
    val recent_time_sec: Double = 0.0,
    val goal_distance_m: Double = 0.0,
    val weekly_km: Double = 0.0,
    val weeks_since_race: Int? = null,
    val notes: List<GuidanceView> = emptyList(),
)

/** Mirrors view_types.rs HrZoneInputView. */
@Serializable
data class HrZoneInputView(
    val age_years: Double = 0.0,
    val resting_hr_bpm: Double? = null,
)

/** Mirrors view_types.rs ProteinInputView. */
@Serializable
data class ProteinInputView(
    val bodyweight_kg: Double = 0.0,
    val masters: Boolean = false,
    val deficit: Boolean = false,
)

/** Mirrors view_types.rs HypertrophyInputView. */
@Serializable
data class HypertrophyInputView(
    val muscle: String = "",
    val weeks: Int = 4,
    val not_growing: Boolean = false,
    val recovering_easily: Boolean = false,
)

/** Mirrors model.rs Profile (ViewModel.profile). */
@Serializable
data class ProfileView(
    val progression_cadence: String = "",
    val lift_goal: String = "",
    val goal_distance: String = "",
    val concurrent_goal: String = "",
    val weekly_sets: Int = 0,
    val running_days_per_week: Int = 0,
    val running_km_per_week: Double = 0.0,
    // False while the weekly distance was never given ("Not set").
    val running_km_per_week_set: Boolean = true,
    val advanced: Boolean = false,
    // Person data; null = not given.
    val female: Boolean? = null,
    val bodyweight_kg: Double? = null,
    val age_years: Double? = null,
    val resting_hr_bpm: Double? = null,
    val measured_hr_max: Double? = null,
    // Display weights in pounds. Numeric fields stay kg; core strings already use
    // the chosen unit.
    val weight_lb: Boolean = false,
    val health: HealthScreen = HealthScreen(),
    // "Weights" | "Calisthenics".
    val resistance_modality: String = "Weights",
    // `environment` is the bare Rust variant name, mapped back through
    // Environment.valueOf. Null = unset.
    val high_load_block: Boolean = false,
    val environment: String? = null,
    val env_temp_c: Double? = null,
    val env_altitude_m: Double? = null,
    val weeks_off: Double? = null,
    // Coaching inputs; null = unset.
    val lift_days_per_week: Int? = null,
    val training_weekdays: Int? = null,
    val goal_race_epoch_day: Long? = null,
    val recent_race: RecentRace? = null,
    val stated_longest_recent_km: Double? = null,
    val layoff_returned_epoch_day: Long? = null,
    val meet_epoch_day: Long? = null,
    val calis_equipment: CalisEquipment? = null,
    val specialization_muscle: String? = null,
    val runner_strength: Boolean = false,
    // Local epoch day; when set the core derives age_years from it.
    val birth_epoch_day: Long? = null,
)

/** A recent race or time trial the athlete reports (app::RecentRace): distance
 *  in km, finish time in whole seconds, optional local epoch day it was run. */
@Serializable
data class RecentRace(
    val distance_km: Double = 0.0,
    val time_sec: Int = 0,
    val epoch_day: Long? = null,
) : JvmSerializable

/** Calisthenics equipment on hand (app::CalisEquipment). The core reads an
 *  absent profile field as "everything available"; an answered set is sent whole. */
@Serializable
data class CalisEquipment(
    val pull_up_bar: Boolean = false,
    val dip_bars: Boolean = false,
    val rings: Boolean = false,
) : JvmSerializable

/** The seven PAR-Q+ general health questions (onboard-051), in form order. Each
 *  "Yes" maps onto one core gate flag: Q2/Q3 are symptoms (onboard-052, not
 *  clearable), Q6 is a bone/joint problem (injury/rehab), the rest are
 *  doctor-follow-up items (parq_positive). [bit] is the item's bit in
 *  [HealthScreen.parq_yes]. */
enum class ParqItem(val bit: Int) {
    HeartOrBloodPressure(0),
    ChestPain(1),
    DizzyOrFainting(2),
    OtherCondition(3),
    Medication(4),
    BoneOrJoint(5),
    SupervisedOnly(6),
    ;

    val symptom: Boolean get() = this == ChestPain || this == DizzyOrFainting
    val injury: Boolean get() = this == BoneOrJoint
    val followUp: Boolean get() = !symptom && !injury
}

/** The clearable doctor-follow-up questions (Q1/Q4/Q5/Q7) as a [HealthScreen.parq_yes]
 *  mask; mirrors schema.rs `PARQ_FOLLOWUP_BITS`. */
private val PARQ_FOLLOW_UP_BITS: Int = ParqItem.entries
    .filter { it.followUp }
    .fold(0) { acc, it -> acc or (1 shl it.bit) }

/** The symptom questions (Q2/Q3) as a [HealthScreen.parq_yes] mask. */
private val PARQ_SYMPTOM_BITS: Int = ParqItem.entries
    .filter { it.symptom }
    .fold(0) { acc, it -> acc or (1 shl it.bit) }

/**
 * Mirrors schema.rs HealthScreen. `youth` is derived core-side from age. Kotlin
 * `Serializable` so it can ride in the profile draft's rememberSaveable bundle.
 * The gate flags are derived from [parq_yes] by [withParqAnswer]: Q1/Q4/Q5/Q7 →
 * `parq_positive`, Q2/Q3 → `symptomatic`, Q6 → `injury_or_rehab`.
 */
@Serializable
data class HealthScreen(
    val youth: Boolean = false,
    val parq_positive: Boolean = false,
    val medically_cleared: Boolean = false,
    val pregnant: Boolean = false,
    val pregnancy_warning_sign: Boolean = false,
    val injury_or_rehab: Boolean = false,
    val reds_signal: Boolean = false,
    val symptomatic: Boolean = false,
    val acute_illness: Boolean = false,
    val parq_yes: Int = 0,
    /** Follow-up answers the doctor's clearance covers (schema.rs
     *  `cleared_parq_mask`). A follow-up Yes outside it gates again core-side. */
    val cleared_parq_mask: Int = 0,
) : JvmSerializable {
    /** Any SHELL-answerable flag raised. `youth` is deliberately EXCLUDED: it is
     *  core-derived from age, never asserted by the shell (a re-emitted SetProfile
     *  that carried a derived youth=true would pin the pediatric gate even after
     *  the age is raised). The gate-relevant `parq_positive` counts even when
     *  cleared, so a cleared PARQ still persists across a re-emitted SetProfile. */
    fun anyRaised(): Boolean =
        parq_positive || medically_cleared || pregnant ||
            pregnancy_warning_sign || injury_or_rehab || reds_signal ||
            symptomatic || acute_illness || parq_yes != 0

    /** A "Doctor-flagged" answer from the old single toggle, before the seven
     *  questions existed: raised, but no follow-up answer recorded. */
    val legacyParq: Boolean get() = parq_positive && parq_yes and PARQ_FOLLOW_UP_BITS == 0

    /** This PAR-Q+ question answered Yes. Q6 also reads the injury flag, which
     *  predates the questions and means the same thing. */
    fun parqYes(item: ParqItem): Boolean =
        parq_yes and (1 shl item.bit) != 0 || (item.injury && injury_or_rehab)

    /** How many of the seven questions are answered Yes. */
    val parqYesCount: Int get() = ParqItem.entries.count { parqYes(it) }

    /** Answer one PAR-Q+ question and re-derive the gate flags from all seven.
     *  A legacy flag stays until a question of its own kind is answered, so an
     *  unrelated answer never drops a hold: a legacy doctor flag ([legacyParq])
     *  until a follow-up question (Q1/Q4/Q5/Q7) is answered, a legacy
     *  `symptomatic` flag (no symptom bit) until a symptom question is
     *  (symptoms are never clearable, onboard-052). A clearance survives only
     *  while every follow-up Yes was already covered by it: a NEW follow-up Yes
     *  drops `medically_cleared` (safety-044). */
    fun withParqAnswer(item: ParqItem, yes: Boolean): HealthScreen {
        val bit = 1 shl item.bit
        // Fold a legacy injury flag into its question first, so answering another
        // question can't silently clear it.
        val base = if (injury_or_rehab) parq_yes or (1 shl ParqItem.BoneOrJoint.bit) else parq_yes
        val mask = if (yes) base or bit else base and bit.inv()
        fun has(i: ParqItem) = mask and (1 shl i.bit) != 0
        val followUpBits = PARQ_FOLLOW_UP_BITS
        val oldFollowUps = parq_yes and followUpBits
        val newFollowUps = mask and followUpBits
        val followUp = newFollowUps != 0 || (legacyParq && !item.followUp)
        // What the existing clearance covers: the recorded mask, or (a clearance
        // saved before the mask existed / a legacy toggle) the answers at the time.
        val covered = when {
            !medically_cleared -> 0
            cleared_parq_mask != 0 -> cleared_parq_mask
            else -> oldFollowUps
        }
        val stillCleared = medically_cleared && followUp &&
            (newFollowUps and covered.inv()) == 0 &&
            (newFollowUps != 0 || covered == 0)
        return copy(
            parq_yes = mask,
            parq_positive = followUp,
            medically_cleared = stillCleared,
            cleared_parq_mask = if (stillCleared) newFollowUps else 0,
            symptomatic = mask and PARQ_SYMPTOM_BITS != 0 ||
                (symptomatic && parq_yes and PARQ_SYMPTOM_BITS == 0 && !item.symptom),
            injury_or_rehab = has(ParqItem.BoneOrJoint),
        )
    }

    /** Set the "Cleared by a doctor" answer. The clearance covers exactly the
     *  follow-up answers given now, so a later new Yes is not covered. */
    fun withClearance(cleared: Boolean): HealthScreen =
        copy(
            medically_cleared = cleared,
            cleared_parq_mask = if (cleared) parq_yes and PARQ_FOLLOW_UP_BITS else 0,
        )
}

@Serializable
data class AdjustmentView(
    val summary: String = "",
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
    val why: WhyView = WhyView(),
    // Item-scoped cue: `exercise` is null on a session-wide row; `suggested_load_kg`
    // is the plate-grid load for the remaining sets.
    val exercise: String? = null,
    val suggested_load_kg: Double? = null,
    val suggests_end_exercise: Boolean = false,
)

/** Mirrors view_types.rs LiftResultView. */
@Serializable
data class LiftResultView(
    val exercise: String = "",
    val weight_kg: Double = 0.0,
    val reps: Int = 0,
    val rpe: Double = 0.0,
    val e1rm_kg: Double = 0.0,
    val pct_1rm: Double = 0.0,
    // False: omit the "· RIR x" caption.
    val rpe_recorded: Boolean = false,
    // Null when the estimate is unreliable: suggest a 3–6 rep test set instead.
    val cross_check: E1rmRangeView? = null,
    val summary: String = "",
    // Unix seconds; 0 when undated.
    val observed_at: Long = 0,
    // Sent back in AmendSet/DeleteEntry. 0 for a legacy row: target it by observed_at.
    val entry_id: Long = 0,
    // Null for a reps×load set.
    val hold_sec: Int? = null,
    val e1rm_pr: Boolean = false,
    // kg; 0 for a timed hold.
    val tonnage_kg: Double = 0.0,
)

/** Mirrors view_types.rs E1rmRangeView. */
@Serializable
data class E1rmRangeView(
    val low_kg: Double = 0.0,
    val high_kg: Double = 0.0,
    val formulas: Int = 0,
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
)

/** Mirrors view_types.rs SplitVerdictView. */
@Serializable
data class SplitVerdictView(
    val verdict: String = "", // "fade" | "even" | "negative"
    val label: String = "",
    val message: String = "",
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
)

/** Mirrors view_types.rs IntervalVerdictView. */
@Serializable
data class IntervalVerdictView(
    val kind: String = "", // "interval" | "steady"
    val label: String = "",
    val message: String = "",
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
)

/** Run-intent label (schema::WorkoutType). Entry names are the serde wire strings;
 *  [label] is the display text. */
enum class WorkoutType(val label: String) {
    Steady("Steady"),
    Interval("Interval"),
    Tempo("Tempo"),
    LongRun("Long run"),
    Recovery("Recovery"),
    Easy("Easy"),
    ;

    companion object {
        /** An unknown wire string decodes to null. */
        fun fromWire(s: String?): WorkoutType? =
            s?.let { w -> entries.firstOrNull { it.name == w } }
    }
}

/** Mirrors the core's ActivityKind; entry names are the serde wire strings. */
@Serializable
enum class ActivityKind { Sport, Cycling, Swimming, Walking, Dance, Strength, Other }

/** Mirrors view_types.rs ActivityResultView. */
@Serializable
data class ActivityResultView(
    val entry_id: Long = 0,
    val observed_at: Long = 0,
    val kind: ActivityKind = ActivityKind.Other,
    val minutes: Double = 0.0,
    val vigorous: Boolean = false,
    val strengthening: Boolean = false,
)

/** Mirrors view_types.rs YouthActivityView (local Mon–Sun week). */
@Serializable
data class YouthActivityView(
    val week_monday_epoch_day: Long = 0,
    val minutes: Int = 0,
    val run_minutes: Int = 0,
    val lift_minutes: Int = 0,
    val activity_minutes: Int = 0,
    val daily_target_minutes: Int = 0,
    val weekly_target_minutes: Int = 0,
    val vigorous_days: Int = 0,
    val strengthening_days: Int = 0,
    val target_days: Int = 0,
    val week_entries: List<ActivityResultView> = emptyList(),
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
    val why: WhyView = WhyView(),
)

/** Mirrors view_types.rs RunResultView. */
@Serializable
data class RunResultView(
    val zone: String = "",
    val pace: String = "",
    val distance_km: Double = 0.0,
    // Echoed for the manual-run edit prefill.
    val duration_min: Double = 0.0,
    val hr_pct_max: Double = 0.0,
    // bpm; null when none was entered (a tracked run has %HRmax only).
    val avg_hr_bpm: Double? = null,
    // Null for a legacy row; Undo re-sends it.
    val utc_offset_sec: Int? = null,
    // Display-ready; "" with no heart rate.
    val hr_label: String = "",
    val spike_flag: Boolean = false,
    val spike_note: String = "",
    // Null for a hand-entered run or a track too short to split.
    val split: SplitVerdictView? = null,
    // Null for a hand-entered run or a track too short.
    val interval: IntervalVerdictView? = null,
    // Raw wire string, read through WorkoutType.fromWire; null = untagged.
    val workout_type: String? = null,
    val summary: String = "",
    val citation: String = "",
    // Fetch the route on demand via [Core.exportGpx]; false for a hand-entered run.
    val has_gpx: Boolean = false,
    val observed_at: Long = 0,
    // 0 = legacy; see LiftResultView.entry_id.
    val entry_id: Long = 0,
    // Both lists carry the same cumulative distance_km; pick by distance unit.
    // Empty without a track.
    val splits_km: List<RunSplitView> = emptyList(),
    val splits_mi: List<RunSplitView> = emptyList(),
    // A prior 30-day baseline exists; only meaningful when spike_flag.
    val spike_has_baseline: Boolean = false,
    // CR-10; null = unrated.
    val session_rpe: Int? = null,
    // Moving minutes × session_rpe; null when unrated. Never added to the HR load.
    val srpe_load_au: Double? = null,
    // Best-effort records this run holds, e.g. ["Fastest 5K"].
    val best_effort_labels: List<String> = emptyList(),
    // Unix seconds; 0 when undated. observed_at is the log time (a GPS run's last fix).
    val started_at: Long = 0,
    // All fixes failed QC: no route, but [Core.exportGpx] returns the recorded fixes.
    val gps_unmeasurable: Boolean = false,
)

// [Core.savedFigures] result (ffi.rs track_figures_json). No defaults, so an error
// envelope fails to decode.
@Serializable
data class TrackFigures(
    val distance_km: Double,
    val duration_min: Double,
)

/** Mirrors view_types.rs RunSplitView. `pace` is display-ready; `distance_km` is
 *  cumulative. */
@Serializable
data class RunSplitView(
    val index: Int = 0,
    val pace: String = "",
    // Whole seconds; the figure `pace` is formatted from.
    val seconds_per_unit: Int = 0,
    val distance_km: Double = 0.0,
    val partial: Boolean = false,
)

/** Mirrors view_types.rs GuidanceView. */
@Serializable
data class GuidanceView(
    val section: String = "",
    val summary: String = "",
    // Empty on rows that state no distance.
    val summary_mi: String = "",
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
    val why: WhyView = WhyView(),
    // Set on the profile's training-age row only.
    val training_age: String = "",
)

/** Mirrors view_types.rs TrainingLoadView. */
@Serializable
data class TrainingLoadView(
    val days: Int = 0,
    val sessions_counted: Int = 0,
    val sessions_skipped: Int = 0,
    val method: String = "",
    val summary: String = "",
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
)

/** Mirrors view_types.rs SrpeLoadView. */
@Serializable
data class SrpeLoadView(
    val method: String = "",
    val summary: String = "",
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
)

/** Mirrors view_types.rs RunProgressView. 16 weeks, oldest first; distances in km;
 *  `planned_weekly_km` 0 = none. */
@Serializable
data class RunProgressView(
    val weekly: List<RunWeekView> = emptyList(),
    val planned_weekly_km: Double = 0.0,
    val longest_run_km: Double = 0.0,
    val longest_run_observed_at: Long = 0,
    val best_efforts: List<BestEffortView> = emptyList(),
    val citation: String = "",
    val grade: String = "",
    val confidence: Float = 0f,
)

/** Mirrors view_types.rs RunWeekView. */
@Serializable
data class RunWeekView(
    val week_start_epoch_day: Long = 0,
    val km: Double = 0.0,
    val runs: Int = 0,
    val srpe_load_au: Double = 0.0,
)

/** Mirrors view_types.rs BestEffortView. */
@Serializable
data class BestEffortView(
    val distance_m: Int = 0,
    val label: String = "",
    val time_sec: Int = 0,
    val time_label: String = "",
    val entry_id: Long = 0,
    val observed_at: Long = 0,
)

/** Mirrors view_types.rs FeedbackView. */
@Serializable
data class FeedbackView(
    val category: String = "",
    val category_label: String = "",
    val message: String = "",
    // `message` with its distances in miles; empty when it states none.
    val message_mi: String = "",
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
    val why: WhyView = WhyView(),
)

// ── Wire enums ───────────────────────────────────────────────────────────────

// Names must match the Rust `ReadinessSignal` variants exactly: `signal.name`
// is the wire value serde reads. The trailing medical-referral signals
// (Illness/RedS/CardiacRedFlag/BoneStress) sit at the top of the safety ladder;
// omitting them here makes that tier unreachable from the shell.
enum class ReadinessSignal {
    Rpe, EstimatedOneRm, BarVelocity, VelocityLoss, WellnessZ,
    HrvLnRmssd, HrvCv, AerobicDecoupling, RestingHr, Pain,
    // Soreness sits between Pain and Illness in the core's ladder (schema.rs);
    // it's a metric (1–7 Hooper), the autoreg-030 second-clause downgrade signal.
    Soreness,
    Illness, RedS, CardiacRedFlag, BoneStress,
}

enum class ProgressionCadence { EverySession, WeekToWeek, MonthToMonth }
enum class LiftGoal { MaxStrength, Power, Hypertrophy }
enum class GoalDistance { General, C25k, FiveK, TenK, HalfMarathon, Marathon }
enum class ConcurrentGoal { Strength, Power, Hypertrophy, EndurancePriority }
enum class ResistanceModality { Weights, Calisthenics }

// Calisthenics progression ladders (mirrors calisthenics::LadderId). Names cross
// the JSON FFI verbatim as the Rust variant names.
enum class CalisLadder {
    HorizontalPush, VerticalPull, Squat, Dip, VerticalPush, HorizontalPull, Hinge, Core,
    Handstand, FrontLever, Planche, BackLever, MuscleUp, Jump, PlyoPush,
}

// Training environment (mirrors individualization::Environment). Names must
// match the Rust variants exactly: they cross the JSON FFI as the bare variant
// name (the Option<Environment> profile field), arming the File-08 environment
// modifiers (indiv-025 / safety-024) and the running-041 pace-correction notes.
enum class Environment { Heat, Altitude, Cold, Neutral }

// Pain characterization for a Pain readiness report (schema.rs PainDetail,
// File-08 Table 4.1). Names must match the Rust `PainKind`/`PainTrend` variants
// exactly: they are the wire values serde reads.
enum class PainKind { SharpJoint, TendonLoadRelated, Doms, Other }
enum class PainTrend { Stable, Rising, WorseningAfter24h }

/**
 * A characterized pain report (schema.rs PainDetail). Attached to a
 * [Event.SubmitReadiness] whose signal is Pain so the core's graded pain gate
 * (autoreg pain_gate, File-08 Table 4.1) is reachable, SharpJoint→hard stop,
 * TendonLoadRelated→graded by severity/trend, Doms→continue. Without it every
 * pain report is the conservative bare-report hard stop.
 *
 * `location` is display-only context for the banner sub-line ("Left knee"), no
 * core rule branches on it. Every field is serde-defaulted on the Rust side so
 * old logs (no pain object) still replay.
 */
data class PainDetail(
    val kind: PainKind,
    // 0–10 numeric rating scale; null = not rated (omitted on the wire, the core
    // treats an unrated tendon report conservatively).
    val severity: Int? = null,
    val trend: PainTrend,
    val persists: Boolean = false,
    val location: String? = null,
)
