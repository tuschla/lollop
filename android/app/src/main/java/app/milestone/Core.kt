package app.milestone

import android.content.Context
import java.io.File
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.add
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray

/**
 * JNI bridge to the Rust/crux core (`libshared.so`).
 *
 * The core speaks JSON over the FFI: an [Event] is serialised to the exact
 * serde wire form (externally-tagged enums), pushed via [update]; the resulting
 * [ViewModel] is read back via [view]. All coaching logic, evidence grades,
 * safety tiers, contested markers lives in Rust and is rendered verbatim.
 */
object Core {
    init {
        System.loadLibrary("shared")
    }

    private external fun update(event: ByteArray): ByteArray
    private external fun view(): ByteArray

    private val json = Json {
        ignoreUnknownKeys = true
        encodeDefaults = true
    }

    // Append-only, newline-delimited event log. The crux core holds all state in
    // memory only, so we persist the event *stream* (not the derived model) and
    // rebuild by replaying it, the same approach as the web shell's localStorage
    // log. Each line is one event's exact serde wire JSON.
    private var log: File? = null

    // The native core holds all state in memory for the whole process lifetime,
    // so replaying the log more than once (e.g. on Activity recreation across a
    // config change) would double every logged lift/run. Guard so replay happens
    // exactly once per process.
    private var restored = false

    /**
     * Point the core at [ctx]'s event log and replay it to rebuild state. Safe to
     * call on every Activity creation: replay runs only on the first call per
     * process. Returns true only on a genuinely FRESH INSTALL: the log file
     * never existed, so the caller may seed a default profile + onboarding. A
     * log that exists but compacts to zero surviving lines (a returning user who
     * cleared everything) returns false: replaying "nothing left" is their real
     * state and must not be re-seeded over. Read+compact live in [EventLog.load]
     * (pure Kotlin, unit tested); this only replays the survivors.
     */
    fun restore(ctx: Context): Boolean {
        val file = File(ctx.filesDir, "event-log.ndjson")
        log = file
        // Already replayed this process (Activity recreation): the only way this
        // is still a fresh install is if nothing was ever persisted.
        if (restored) return !file.exists()
        restored = true
        // Compacting before replay matters here: a run's GPS track (thousands of
        // points) is the heaviest line in the log, and every `Clear*` supersedes
        // its whole family: the compacted replay is model-equivalent.
        val loaded = EventLog.load(file)
        loaded.lines.forEach { update(it.toByteArray(Charsets.UTF_8)) }
        return loaded.freshInstall
    }

    /** Dispatch an event, persist it to the log, then return the fresh view model. */
    fun send(event: Event): ViewModel {
        val wire = event.toJson().toString()
        update(wire.toByteArray(Charsets.UTF_8))
        log?.appendText(wire + "\n")
        return currentView()
    }

    /** Current view model without dispatching an event. */
    fun currentView(): ViewModel =
        json.decodeFromString(ViewModel.serializer(), String(view(), Charsets.UTF_8))
}

// ── View model (decode) ──────────────────────────────────────────────────────
// Field names + shapes mirror shared/src/app.rs::ViewModel exactly.

@Serializable
data class ViewModel(
    val safety_tier: String? = null,
    val train_blocked: Boolean = false,
    val adjustments: List<AdjustmentView> = emptyList(),
    val review_adjustments: List<AdjustmentView> = emptyList(),
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
    // KB-honest per-signal readiness summary (app.rs readiness_summary): the
    // latest state of each observed signal judged by the core's own KB
    // thresholds. Deliberately NO composite 0–100 score exists on the wire.
    val readiness_summary: List<ReadinessSignalView> = emptyList(),
    // Core-owned "today's call": safety hold > adjustment > feedback >
    // all-clear. Null only against a pre-headline core build.
    val today_headline: TodayHeadlineView? = null,
    // Static signal→group metadata ("metric" | "red_flag") driving the
    // readiness picker's red-flag fence from the core.
    val signal_groups: List<SignalGroupView> = emptyList(),
)

/** One readiness signal's latest state (app.rs ReadinessSignalView). The
 *  evidence fields cite the rule whose threshold judged `state`; grade is
 *  empty for plain factual rows ("recorded"/"clear") that judge nothing. */
@Serializable
data class ReadinessSignalView(
    val signal: String = "",
    val group: String = "", // "metric" | "red_flag"
    val value: Double = 0.0,
    val streak: Int = 0,
    val state: String = "",
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
)

/** The core's single highest-priority call for today (app.rs TodayHeadlineView).
 *  `kind`: "safety_hold" | "adjustment" | "feedback" | "all_clear". The
 *  all-clear default carries an empty evidence tag (it asserts no claim). */
@Serializable
data class TodayHeadlineView(
    val kind: String = "",
    val summary: String = "",
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
)

/** Signal→group row for the static readiness-picker fence metadata. */
@Serializable
data class SignalGroupView(
    val signal: String = "",
    val group: String = "",
)

/** Goal-race finish prediction (app.rs RacePredictionView). Daniels+Riegel. */
@Serializable
data class RacePredictionView(
    val goal_label: String = "",
    val predicted: String = "",
    val agreed: Boolean = false,
    val low_sec: Double = 0.0,
    val high_sec: Double = 0.0,
    val summary: String = "",
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
)

/** Profile echoed by the core (app.rs ViewModel.profile) for editor hydration. */
@Serializable
data class ProfileView(
    val progression_cadence: String = "",
    val lift_goal: String = "",
    val goal_distance: String = "",
    val concurrent_goal: String = "",
    val weekly_sets: Int = 0,
    val running_days_per_week: Int = 0,
    val running_km_per_week: Double = 0.0,
    val advanced: Boolean = false,
    val endurance_intensity_pct_vo2max: Double = 0.0,
)

@Serializable
data class AdjustmentView(
    val summary: String = "",
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
)

@Serializable
data class LiftResultView(
    val exercise: String = "",
    val weight_kg: Double = 0.0,
    val reps: Int = 0,
    val rpe: Double = 0.0,
    val e1rm_kg: Double = 0.0,
    val pct_1rm: Double = 0.0,
    val rir: Double = 0.0,
    // e1RM change vs the previous set of the same exercise, kg (core-computed,
    // app.rs LiftResultView.e1rm_delta_kg). Null for an exercise's first set.
    // Factual measurement: the shell must NOT phrase it as improving/declining
    // (that judgment belongs to the core's trend feedback, FB-TREND-001).
    val e1rm_delta_kg: Double? = null,
    // "up" / "down" / "flat" direction of the delta; null iff the delta is null.
    val e1rm_direction: String? = null,
    val summary: String = "",
    val observed_at: Long = 0,
)

/**
 * Core-owned pacing verdict for a run's measured split (app.rs SplitVerdictView,
 * feedback-016/017). Carries the chip label, coaching copy, and the full evidence
 * tag, so the ~3% fade threshold never leaks into shell code.
 */
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

@Serializable
data class RunResultView(
    val zone: String = "",
    val pace: String = "",
    val distance_km: Double = 0.0,
    val spike_flag: Boolean = false,
    val spike_note: String = "",
    val split_pct: Double? = null,
    // Pacing verdict + chip data for split_pct; null exactly when split_pct is
    // null (hand-entered run or a track too short/degenerate to split).
    val split: SplitVerdictView? = null,
    val summary: String = "",
    val citation: String = "",
    val gpx: String = "",
    val observed_at: Long = 0,
)

@Serializable
data class GuidanceView(
    val section: String = "",
    val summary: String = "",
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
)

@Serializable
data class FeedbackView(
    val category: String = "",
    val message: String = "",
    val suppresses_praise: Boolean = false,
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
)

// ── Events (encode) ──────────────────────────────────────────────────────────
// Hand-built JSON to match serde's externally-tagged enum representation:
//   unit variant      -> "VariantName"
//   struct variant    -> {"VariantName": { ...fields }}
//   newtype variant   -> {"VariantName": <inner value>}

// Names must match the Rust `ReadinessSignal` variants exactly: `signal.name`
// is the wire value serde reads. The trailing medical-referral signals
// (Illness/RedS/CardiacRedFlag/BoneStress) sit at the top of the safety ladder;
// omitting them here makes that tier unreachable from the shell.
enum class ReadinessSignal {
    Rpe, EstimatedOneRm, BarVelocity, VelocityLoss, WellnessZ,
    HrvLnRmssd, HrvCv, AerobicDecoupling, RestingHr, Pain,
    Illness, RedS, CardiacRedFlag, BoneStress,
}

enum class ProgressionCadence { EverySession, WeekToWeek, MonthToMonth }
enum class LiftGoal { MaxStrength, Power, Hypertrophy }
enum class GoalDistance { General, C25k, FiveK, TenK, HalfMarathon, Marathon }
enum class ConcurrentGoal { Strength, Power, Hypertrophy, EndurancePriority }

sealed interface Event {
    fun toJson(): JsonElement

    data class SubmitReadiness(
        val signal: ReadinessSignal,
        val value: Double,
        val observedAt: Long,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("SubmitReadiness", buildJsonObject {
                put("signal", signal.name)
                put("value", value)
                put("observed_at", observedAt)
            })
        }
    }

    data object ClearReadiness : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearReadiness")
    }

    /** Undo one accidental report: core drops its most recent input carrying [signal]. */
    data class RemoveReadiness(val signal: ReadinessSignal) : Event {
        override fun toJson() = buildJsonObject {
            put("RemoveReadiness", buildJsonObject { put("signal", signal.name) })
        }
    }

    data class SetProfile(
        val progressionCadence: ProgressionCadence,
        val liftGoal: LiftGoal,
        val goalDistance: GoalDistance,
        val concurrentGoal: ConcurrentGoal,
        val weeklySets: Int,
        val runningDaysPerWeek: Int,
        val runningKmPerWeek: Double,
        val advanced: Boolean,
        val enduranceIntensityPctVo2max: Double,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("SetProfile", buildJsonObject {
                put("progression_cadence", progressionCadence.name)
                put("lift_goal", liftGoal.name)
                put("goal_distance", goalDistance.name)
                put("concurrent_goal", concurrentGoal.name)
                put("weekly_sets", weeklySets)
                put("running_days_per_week", runningDaysPerWeek)
                put("running_km_per_week", runningKmPerWeek)
                put("advanced", advanced)
                put("endurance_intensity_pct_vo2max", enduranceIntensityPctVo2max)
            })
        }
    }

    data object ClearProfile : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearProfile")
    }

    data class LogSet(
        val exercise: String,
        val weightKg: Double,
        val reps: Int,
        val rpe: Double,
        // Log time, unix seconds. Defaulted to now at construction so the history
        // card can be dated; baked into the persisted line, so replay keeps the
        // original stamp (the core never re-stamps on replay).
        val observedAt: Long = System.currentTimeMillis() / 1000,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("LogSet", buildJsonObject {
                put("exercise", exercise)
                put("weight_kg", weightKg)
                put("reps", reps)
                put("rpe", rpe)
                put("observed_at", observedAt)
            })
        }
    }

    data object ClearSets : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearSets")
    }

    data class LogRun(
        val distanceKm: Double,
        val durationMin: Double,
        val hrPctMax: Double,
        val longestRecentKm: Double,
        val observedAt: Long = System.currentTimeMillis() / 1000,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("LogRun", buildJsonObject {
                put("distance_km", distanceKm)
                put("duration_min", durationMin)
                put("hr_pct_max", hrPctMax)
                put("longest_recent_km", longestRecentKm)
                put("observed_at", observedAt)
            })
        }
    }

    data class LogRunTrack(
        val points: List<GpsPoint>,
        val hrPctMax: Double,
        val longestRecentKm: Double,
        // The session's logged-at stamp for history display; distinct from each
        // GPS fix's own per-point `observed_at`.
        val observedAt: Long = System.currentTimeMillis() / 1000,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("LogRunTrack", buildJsonObject {
                putJsonArray("points") {
                    points.forEach { p ->
                        add(buildJsonObject {
                            put("lat", p.lat)
                            put("lon", p.lon)
                            put("observed_at", p.observedAt)
                            put("accuracy_m", p.accuracyM)
                        })
                    }
                }
                put("hr_pct_max", hrPctMax)
                put("longest_recent_km", longestRecentKm)
                put("observed_at", observedAt)
            })
        }
    }

    data object ClearRuns : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearRuns")
    }

    data class SubmitReview(
        val bonePainRedFlag: Boolean = false,
        val compulsiveFlag: Boolean = false,
        val overtrainingSignalCount: Int = 0,
        val singleSessionSpikeFrac: Double? = null,
        val lift: LiftExec? = null,
        val decoupling: Decouple? = null,
        val easyFracTimeAboveVt1: Double? = null,
        val positiveSplitPct: Double? = null,
        val rpeLoadGapSessions: Int? = null,
        val weeklyVelocityDropMs: Double? = null,
        val failedKeySessions: Int? = null,
        val badDay: Boolean = false,
        // When the review was submitted, unix seconds (backdatable). Baked into
        // the persisted line so replay keeps the original stamp.
        val observedAt: Long = System.currentTimeMillis() / 1000,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("SubmitReview", buildJsonObject {
                put("bone_pain_red_flag", bonePainRedFlag)
                put("compulsive_flag", compulsiveFlag)
                put("overtraining_signal_count", overtrainingSignalCount)
                if (singleSessionSpikeFrac != null)
                    put("single_session_spike_frac", singleSessionSpikeFrac)
                if (lift != null) put("lift", buildJsonObject {
                    put("reps_met", lift.repsMet)
                    put("rir_actual", lift.rirActual)
                    put("rir_target", lift.rirTarget)
                })
                if (decoupling != null) put("decoupling", buildJsonObject {
                    put("drift_pct", decoupling.driftPct)
                    put("cool_steady_context", decoupling.coolSteadyContext)
                })
                if (easyFracTimeAboveVt1 != null)
                    put("easy_frac_time_above_vt1", easyFracTimeAboveVt1)
                if (positiveSplitPct != null)
                    put("positive_split_pct", positiveSplitPct)
                if (rpeLoadGapSessions != null)
                    put("rpe_load_gap_sessions", rpeLoadGapSessions)
                if (weeklyVelocityDropMs != null)
                    put("weekly_velocity_drop_m_s", weeklyVelocityDropMs)
                if (failedKeySessions != null)
                    put("failed_key_sessions", failedKeySessions)
                put("bad_day", badDay)
                put("observed_at", observedAt)
            })
        }
    }

    data object ClearReview : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearReview")
    }

    data class PredictRace(
        val recentDistanceM: Double,
        val recentTimeSec: Double,
        val goalDistanceM: Double,
        val weeklyKm: Double,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("PredictRace", buildJsonObject {
                put("recent_distance_m", recentDistanceM)
                put("recent_time_sec", recentTimeSec)
                put("goal_distance_m", goalDistanceM)
                put("weekly_km", weeklyKm)
            })
        }
    }

    data object ClearRacePrediction : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearRacePrediction")
    }

    data class PlanHypertrophyMeso(
        val muscle: String,
        val weeks: Int,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("PlanHypertrophyMeso", buildJsonObject {
                put("muscle", muscle)
                put("weeks", weeks)
            })
        }
    }

    data object ClearHypertrophyPlan : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearHypertrophyPlan")
    }

    data class ComputeProtein(
        val bodyweightKg: Double,
        val masters: Boolean,
        val deficit: Boolean,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("ComputeProtein", buildJsonObject {
                put("bodyweight_kg", bodyweightKg)
                put("masters", masters)
                put("deficit", deficit)
            })
        }
    }

    data object ClearProtein : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearProtein")
    }

    data class ComputeHrZones(val ageYears: Double) : Event {
        override fun toJson() = buildJsonObject {
            put("ComputeHrZones", buildJsonObject {
                put("age_years", ageYears)
            })
        }
    }

    data object ClearHrZones : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearHrZones")
    }
}

data class LiftExec(val repsMet: Boolean, val rirActual: Int, val rirTarget: Int)

/** Aerobic-decoupling context for a run review. Mirrors shared/src/app.rs::Decouple. */
data class Decouple(val driftPct: Double, val coolSteadyContext: Boolean)

/** One GPS fix. Mirrors shared/src/running.rs::GpsPoint. */
data class GpsPoint(
    val lat: Double,
    val lon: Double,
    val observedAt: Long,
    val accuracyM: Double,
)
