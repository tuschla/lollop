package app.milestone

import app.milestone.resources.Res
import app.milestone.resources.activity_kind_cycling
import app.milestone.resources.activity_kind_dance
import app.milestone.resources.activity_kind_other
import app.milestone.resources.activity_kind_sport
import app.milestone.resources.activity_kind_strength
import app.milestone.resources.activity_kind_swimming
import app.milestone.resources.activity_kind_walking
import app.milestone.resources.history_run_easy
import app.milestone.resources.history_run_interval
import app.milestone.resources.history_run_long
import app.milestone.resources.history_run_recovery
import app.milestone.resources.history_run_steady
import app.milestone.resources.history_run_tempo
import app.milestone.resources.log_muscle_abs
import app.milestone.resources.log_muscle_back
import app.milestone.resources.log_muscle_biceps
import app.milestone.resources.log_muscle_calves
import app.milestone.resources.log_muscle_chest
import app.milestone.resources.log_muscle_glutes
import app.milestone.resources.log_muscle_hamstrings
import app.milestone.resources.log_muscle_quads
import app.milestone.resources.log_muscle_rear_delts
import app.milestone.resources.log_muscle_side_delts
import app.milestone.resources.log_muscle_triceps
import app.milestone.resources.log_signal_bar_velocity
import app.milestone.resources.log_signal_bone_stress
import app.milestone.resources.log_signal_cardiac
import app.milestone.resources.log_signal_decoupling
import app.milestone.resources.log_signal_e1rm
import app.milestone.resources.log_signal_hrv
import app.milestone.resources.log_signal_hrv_cv
import app.milestone.resources.log_signal_illness
import app.milestone.resources.log_signal_pain
import app.milestone.resources.log_signal_reds
import app.milestone.resources.log_signal_rhr
import app.milestone.resources.log_signal_rpe
import app.milestone.resources.log_signal_soreness
import app.milestone.resources.log_signal_velocity_loss
import app.milestone.resources.log_signal_wellness
import app.milestone.resources.run_type_easy
import app.milestone.resources.run_type_interval
import app.milestone.resources.run_type_long
import app.milestone.resources.run_type_recovery
import app.milestone.resources.run_type_steady
import app.milestone.resources.run_type_tempo
import app.milestone.resources.setup_env_altitude
import app.milestone.resources.setup_env_cold
import app.milestone.resources.setup_env_heat
import app.milestone.resources.setup_env_neutral
import app.milestone.resources.setup_goaldist_10k
import app.milestone.resources.setup_goaldist_5k
import app.milestone.resources.setup_goaldist_c25k
import app.milestone.resources.setup_goaldist_general
import app.milestone.resources.setup_goaldist_half
import app.milestone.resources.setup_goaldist_marathon
import app.milestone.resources.setup_liftgoal_muscle
import app.milestone.resources.setup_liftgoal_power
import app.milestone.resources.setup_liftgoal_strength
import org.jetbrains.compose.resources.StringResource

// The labels of the core's enums. Display-only, the wire value is still each
// variant's name, so these strings can change freely. Exhaustive `when` (no
// `else`) so a new variant fails to compile until it is given a label rather than
// silently showing a cryptic raw name.

/** Display label of an [ActivityKind]. */
internal val ActivityKind.labelRes: StringResource
    get() = when (this) {
        ActivityKind.Sport -> Res.string.activity_kind_sport
        ActivityKind.Cycling -> Res.string.activity_kind_cycling
        ActivityKind.Swimming -> Res.string.activity_kind_swimming
        ActivityKind.Walking -> Res.string.activity_kind_walking
        ActivityKind.Dance -> Res.string.activity_kind_dance
        ActivityKind.Strength -> Res.string.activity_kind_strength
        ActivityKind.Other -> Res.string.activity_kind_other
    }

/** The one lifting-goal vocabulary: the Profile Training goal segments, the
 *  wizard's lift-goal cards and the review row all read these labels. */
internal val LiftGoal.labelRes: StringResource
    get() = when (this) {
        LiftGoal.MaxStrength -> Res.string.setup_liftgoal_strength
        LiftGoal.Power -> Res.string.setup_liftgoal_power
        LiftGoal.Hypertrophy -> Res.string.setup_liftgoal_muscle
    }

internal val GoalDistance.labelRes: StringResource
    get() = when (this) {
        GoalDistance.General -> Res.string.setup_goaldist_general
        GoalDistance.C25k -> Res.string.setup_goaldist_c25k
        GoalDistance.FiveK -> Res.string.setup_goaldist_5k
        GoalDistance.TenK -> Res.string.setup_goaldist_10k
        GoalDistance.HalfMarathon -> Res.string.setup_goaldist_half
        GoalDistance.Marathon -> Res.string.setup_goaldist_marathon
    }

internal val Environment.labelRes: StringResource
    get() = when (this) {
        Environment.Heat -> Res.string.setup_env_heat
        Environment.Altitude -> Res.string.setup_env_altitude
        Environment.Cold -> Res.string.setup_env_cold
        Environment.Neutral -> Res.string.setup_env_neutral
    }

/** The signal's picker label (the readiness editor and Today's chips). */
internal val ReadinessSignal.labelRes: StringResource
    get() = when (this) {
        ReadinessSignal.Rpe -> Res.string.log_signal_rpe
        ReadinessSignal.EstimatedOneRm -> Res.string.log_signal_e1rm
        ReadinessSignal.BarVelocity -> Res.string.log_signal_bar_velocity
        ReadinessSignal.VelocityLoss -> Res.string.log_signal_velocity_loss
        ReadinessSignal.WellnessZ -> Res.string.log_signal_wellness
        ReadinessSignal.HrvLnRmssd -> Res.string.log_signal_hrv
        ReadinessSignal.HrvCv -> Res.string.log_signal_hrv_cv
        ReadinessSignal.AerobicDecoupling -> Res.string.log_signal_decoupling
        ReadinessSignal.RestingHr -> Res.string.log_signal_rhr
        ReadinessSignal.Soreness -> Res.string.log_signal_soreness
        ReadinessSignal.Pain -> Res.string.log_signal_pain
        ReadinessSignal.Illness -> Res.string.log_signal_illness
        ReadinessSignal.RedS -> Res.string.log_signal_reds
        ReadinessSignal.CardiacRedFlag -> Res.string.log_signal_cardiac
        ReadinessSignal.BoneStress -> Res.string.log_signal_bone_stress
    }

/** A run type's words: [label] on the RUN TYPE chips and row ("Tempo"), [title]
 *  as a tagged run's card title ("Tempo run"). */
internal class WorkoutTypeLabels(val label: StringResource, val title: StringResource)

internal val WorkoutType.labels: WorkoutTypeLabels
    get() = when (this) {
        WorkoutType.Steady -> WorkoutTypeLabels(Res.string.run_type_steady, Res.string.history_run_steady)
        WorkoutType.Interval -> WorkoutTypeLabels(Res.string.run_type_interval, Res.string.history_run_interval)
        WorkoutType.Tempo -> WorkoutTypeLabels(Res.string.run_type_tempo, Res.string.history_run_tempo)
        WorkoutType.LongRun -> WorkoutTypeLabels(Res.string.run_type_long, Res.string.history_run_long)
        WorkoutType.Recovery -> WorkoutTypeLabels(Res.string.run_type_recovery, Res.string.history_run_recovery)
        WorkoutType.Easy -> WorkoutTypeLabels(Res.string.run_type_easy, Res.string.history_run_easy)
    }

/**
 * Muscle picker for the hypertrophy volume planner. [wire] is the exact string the
 * core's `hypertrophy::landmarks_for` looks up (case-insensitive, but sent verbatim);
 * [labelRes] is display-only. The set MUST match the LANDMARKS table in
 * `hypertrophy.rs`: an unknown muscle yields a single explanatory row instead of a plan.
 */
internal enum class HypertrophyMuscle(val wire: String, val labelRes: StringResource) {
    Chest("chest", Res.string.log_muscle_chest),
    Back("back", Res.string.log_muscle_back),
    Quads("quads", Res.string.log_muscle_quads),
    Hamstrings("hamstrings", Res.string.log_muscle_hamstrings),
    Glutes("glutes", Res.string.log_muscle_glutes),
    SideDelts("side delts", Res.string.log_muscle_side_delts),
    RearDelts("rear delts", Res.string.log_muscle_rear_delts),
    Biceps("biceps", Res.string.log_muscle_biceps),
    Triceps("triceps", Res.string.log_muscle_triceps),
    Calves("calves", Res.string.log_muscle_calves),
    Abs("abs", Res.string.log_muscle_abs);

    companion object {
        /** The muscle for a wire name, matched case-insensitively as the core does; null if none. */
        fun of(wire: String?): HypertrophyMuscle? = entries.firstOrNull { it.wire.equals(wire, ignoreCase = true) }
    }
}
