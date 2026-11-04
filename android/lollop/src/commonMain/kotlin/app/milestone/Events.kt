package app.milestone

import kotlinx.atomicfu.locks.SynchronizedObject
import kotlinx.atomicfu.locks.synchronized
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObjectBuilder
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.add
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray

// ── Events (encode) ──────────────────────────────────────────────────────────
// Hand-built JSON to match serde's externally-tagged enum representation:
//   unit variant      -> "VariantName"
//   struct variant    -> {"VariantName": { ...fields }}
//   newtype variant   -> {"VariantName": <inner value>}

/** Monotonic per-entry id source. The wall clock ([wallClockMillis]) alone collides
 *  when two entries are created within the same millisecond (a multi-activity
 *  TCX/GPX import builds several runs in a tight loop), which would make a later
 *  DeleteEntry/AmendSet target the wrong row. Threading each id through this
 *  counter guarantees strict uniqueness while staying epoch-millis-shaped. */
object EntryIds : SynchronizedObject() {
    private var last = 0L

    fun next(): Long = synchronized(this) {
        last = maxOf(wallClockMillis(), last + 1)
        last
    }

    /** Never hand out an id at or below [floor]: boot raises it to the largest
     *  restored entry id, so a wall clock moved backwards can't reuse one. */
    fun raiseTo(floor: Long) = synchronized(this) {
        last = maxOf(last, floor)
    }
}

sealed interface Event {
    fun toJson(): JsonElement

    /** Whether this event is written to the durable log when the core accepts it
     *  (all but [SetToday]). */
    val persist: Boolean get() = true

    data class SubmitReadiness(
        val signal: ReadinessSignal,
        val value: Double,
        val observedAt: Long,
        // Pain characterization for a Pain report (ReadinessInput.pain). null keeps
        // the conservative bare-report hard stop; a full detail reaches the graded
        // File-08 pain gate.
        val pain: PainDetail? = null,
        // Effort duration (minutes) for a duration-gated signal (AerobicDecoupling
        // is valid only for efforts >20 min, File 06). null → the core cannot
        // validate it and discards it (ReadinessInput.effort_min serde-defaults None).
        val effortMin: Double? = null,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("SubmitReadiness", buildJsonObject {
                put("signal", signal.name)
                put("value", value)
                put("observed_at", observedAt)
                if (effortMin != null) put("effort_min", effortMin)
                // Always 0: the core derives streaks from check-ins.
                put("streak", 0)
                if (pain != null) put("pain", buildJsonObject {
                    put("kind", pain.kind.name)
                    if (pain.severity != null) put("severity", pain.severity)
                    put("trend", pain.trend.name)
                    put("persists", pain.persists)
                    // Optional free-text/body-part location; omitted when absent
                    // (serde default None on the Rust side).
                    if (pain.location != null) put("location", pain.location)
                })
            })
        }
    }

    data object ClearReadiness : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearReadiness")
    }

    /**
     * Undo one report: the core removes its most recent input carrying [signal]
     * (for Pain, the worst active report). Dated with the removal time, so the
     * input stays in force for the past days it shaped and is gone from today on.
     */
    data class RemoveReadiness(
        val signal: ReadinessSignal,
        val observedAt: Long = AppClock.nowSec(),
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("RemoveReadiness", buildJsonObject {
                put("signal", signal.name)
                put("observed_at", observedAt)
            })
        }
    }

    /**
     * One morning check-in: raw HUMAN observations only. The core
     * normalizes the retained history into the z-scores/deltas the autoreg rules
     * consume; the user never enters a z-score. Every scored field is optional;
     * only the answered items ride the wire (serde default None on the Rust side),
     * so a check-in written by any app version replays. Mirrors schema.rs::CheckinInput.
     */
    data class SubmitCheckin(
        val observedAt: Long,
        val sleepQuality: Int? = null,  // 1–5 (5 = slept great)
        val soreness: Int? = null,      // 1–5 (5 = very sore)
        val mood: Int? = null,          // 1–5 (5 = great mood/motivation); higher = better
        val fatigue: Int? = null,       // 1–5 (5 = utterly wiped); higher = worse
        val stress: Int? = null,        // 1–5 (5 = very stressed); higher = worse
        val restingHrBpm: Double? = null,
        val hrvRmssdMs: Double? = null,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("SubmitCheckin", buildJsonObject {
                put("observed_at", observedAt)
                if (sleepQuality != null) put("sleep_quality", sleepQuality)
                if (soreness != null) put("soreness", soreness)
                if (mood != null) put("mood", mood)
                if (fatigue != null) put("fatigue", fatigue)
                if (stress != null) put("stress", stress)
                if (restingHrBpm != null) put("resting_hr_bpm", restingHrBpm)
                if (hrvRmssdMs != null) put("hrv_rmssd_ms", hrvRmssdMs)
            })
        }
    }

    /** Drop the whole check-in history (part of "Clear all data"). */
    data object ClearCheckins : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearCheckins")
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
        // False = the athlete never gave a weekly distance: the core forces
        // runningKmPerWeek to 0.0 and plans from logged runs or its KB floors. A
        // real "I don't run yet" answer is 0.0 with this true.
        val runningKmPerWeekSet: Boolean = true,
        // Every field below mirrors app::Profile and is sent on EVERY SetProfile
        // (last-write-wins, so an omitted field would reset to the core default).
        // Optional fields are emitted only when set (serde defaults on the core),
        // so an unanswered profile keeps the base eight-field wire.
        val female: Boolean? = null,
        val bodyweightKg: Double? = null,
        val ageYears: Double? = null,
        val restingHrBpm: Double? = null,
        val measuredHrMax: Double? = null,
        // Display weights in pounds; display-only.
        val weightLb: Boolean = false,
        // Stage-0 health screen (schema.rs HealthScreen): the pediatric/PAR-Q+/
        // pregnancy/injury/RED-S deferral gates.
        val health: HealthScreen = HealthScreen(),
        val resistanceModality: ResistanceModality = ResistanceModality.Weights,
        //   high_load_block  — arms autoreg-029's parasympathetic-saturation guard
        //   environment       — File-08 environment modifiers (heat/altitude/cold)
        //   env_temp_c/_m     — running-041 pace-correction triggers
        //   weeks_off         — REENTRY-001 ramp + hypertrophy-045 reduced MEV note
        val highLoadBlock: Boolean = false,
        val environment: Environment? = null,
        val envTempC: Double? = null,
        val envAltitudeM: Double? = null,
        val weeksOff: Double? = null,
        //   lift_days_per_week        — 2..=5 lifting sessions the athlete can do
        //   training_weekdays         — bitmask bit0=Mon..bit6=Sun; null = any day
        //   goal_race_epoch_day       — race date (taper/periodisation target)
        //   recent_race               — a recent race/time trial (pace anchor)
        //   stated_longest_recent_km  — longest run in the last 30 days (spike cap)
        //   layoff_returned_epoch_day — the day training resumed after weeks_off
        //   meet_epoch_day            — strength test / meet date (taper/peak)
        //   calis_equipment           — bar / dip bars / rings on hand
        //   specialization_muscle     — hypertrophy priority muscle key
        //   runner_strength           — a pure runner opted into 2 strength sessions
        val liftDaysPerWeek: Int? = null,
        val trainingWeekdays: Int? = null,
        val goalRaceEpochDay: Long? = null,
        val recentRace: RecentRace? = null,
        val statedLongestRecentKm: Double? = null,
        val layoffReturnedEpochDay: Long? = null,
        val meetEpochDay: Long? = null,
        val calisEquipment: CalisEquipment? = null,
        val specializationMuscle: String? = null,
        val runnerStrength: Boolean = false,
        // The day this profile takes effect (app::Profile effective_from_epoch_day):
        // it supersedes every history entry from that day on, and a past day is
        // prescribed from the profile in force then. Production sites pass the
        // core's today; null replaces the whole history.
        val effectiveFromEpochDay: Long? = null,
        // Birthday, local epoch day: the core derives the age and the under-18 gate.
        val birthEpochDay: Long? = null,
        // When the edit was made (unix seconds): on its effective day the edit is in
        // force only from this instant, so an evening edit never re-converts a run
        // logged that morning. Stamped by [effectiveFrom] for a dated edit.
        val editedAtSec: Long? = null,
        // JvmSerializable: the Profile editor and guided setup keep the event
        // itself as their rememberSaveable draft.
    ) : Event, JvmSerializable {
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
                if (!runningKmPerWeekSet) put("running_km_per_week_set", false)
                if (female != null) put("female", female)
                if (bodyweightKg != null) put("bodyweight_kg", bodyweightKg)
                if (ageYears != null) put("age_years", ageYears)
                if (restingHrBpm != null) put("resting_hr_bpm", restingHrBpm)
                if (measuredHrMax != null) put("measured_hr_max", measuredHrMax)
                if (weightLb) put("weight_lb", true)
                if (resistanceModality != ResistanceModality.Weights) {
                    put("resistance_modality", resistanceModality.name)
                }
                // `environment` is the bare Rust variant name.
                if (highLoadBlock) put("high_load_block", true)
                if (environment != null) put("environment", environment.name)
                if (envTempC != null) put("env_temp_c", envTempC)
                if (envAltitudeM != null) put("env_altitude_m", envAltitudeM)
                if (weeksOff != null) put("weeks_off", weeksOff)
                if (liftDaysPerWeek != null) put("lift_days_per_week", liftDaysPerWeek)
                if (trainingWeekdays != null) put("training_weekdays", trainingWeekdays)
                if (goalRaceEpochDay != null) put("goal_race_epoch_day", goalRaceEpochDay)
                if (recentRace != null) put("recent_race", buildJsonObject {
                    put("distance_km", recentRace.distance_km)
                    put("time_sec", recentRace.time_sec)
                    if (recentRace.epoch_day != null) put("epoch_day", recentRace.epoch_day)
                })
                if (statedLongestRecentKm != null) put("stated_longest_recent_km", statedLongestRecentKm)
                if (layoffReturnedEpochDay != null) put("layoff_returned_epoch_day", layoffReturnedEpochDay)
                if (meetEpochDay != null) put("meet_epoch_day", meetEpochDay)
                if (calisEquipment != null) put("calis_equipment", buildJsonObject {
                    put("pull_up_bar", calisEquipment.pull_up_bar)
                    put("dip_bars", calisEquipment.dip_bars)
                    put("rings", calisEquipment.rings)
                })
                if (specializationMuscle != null) put("specialization_muscle", specializationMuscle)
                if (runnerStrength) put("runner_strength", true)
                if (effectiveFromEpochDay != null) {
                    put("effective_from_epoch_day", effectiveFromEpochDay)
                }
                if (birthEpochDay != null) put("birth_epoch_day", birthEpochDay)
                if (editedAtSec != null) put("edited_at", editedAtSec)
                // Health screen: only when a gate flag is raised.
                if (health.anyRaised()) put("health", buildJsonObject {
                    // youth is core-derived from age; the shell never asserts it,
                    // so re-emitting a profile can never pin the pediatric gate.
                    put("youth", false)
                    put("parq_positive", health.parq_positive)
                    put("medically_cleared", health.medically_cleared)
                    put("pregnant", health.pregnant)
                    put("pregnancy_warning_sign", health.pregnancy_warning_sign)
                    put("injury_or_rehab", health.injury_or_rehab)
                    put("reds_signal", health.reds_signal)
                    put("symptomatic", health.symptomatic)
                    put("acute_illness", health.acute_illness)
                    put("parq_yes", health.parq_yes)
                    put("cleared_parq_mask", health.cleared_parq_mask)
                })
            })
        }

        /** Holds [from] and [SEED] (ProfileEditor.kt). */
        companion object
    }

    data object ClearProfile : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearProfile")
    }

    /**
     * Place a calisthenics ladder at a rung (the setup "Where are you now?"
     * screen). [rung] is the 0-based index on that ladder; an out-of-range index
     * is a core no-op. Only answered ladders are sent: an unanswered one starts at
     * its easiest rung.
     */
    data class SetCalisthenicsRung(
        val ladder: CalisLadder,
        val rung: Int,
        val observedAt: Long,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("SetCalisthenicsRung", buildJsonObject {
                put("ladder", ladder.name)
                put("rung", rung)
                put("observed_at", observedAt)
            })
        }
    }

    data class LogSet(
        val exercise: String,
        val weightKg: Double,
        val reps: Int,
        val rpe: Double,
        // Log time, unix seconds. Defaulted to now at construction so the history
        // card can be dated; baked into the persisted line, so replay keeps the
        // original stamp (the core never re-stamps on replay).
        val observedAt: Long = AppClock.nowSec(),
        // Stable per-entry id: epoch-millis at log, so it survives
        // edits and never collides with a backdated observed_at. Sent back in
        // AmendSet/DeleteEntry to target this exact set.
        val entryId: Long = EntryIds.next(),
        // Timed-hold seconds for a static/hold set (calisthenics lever/plank/
        // dead-hang rung); null for a reps×load set. Omitted from the wire when
        // null so a normal set matches serde's `#[serde(default)]` shape.
        val holdSec: Int? = null,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("LogSet", buildJsonObject {
                put("exercise", exercise)
                put("weight_kg", weightKg)
                put("reps", reps)
                put("rpe", rpe)
                put("observed_at", observedAt)
                put("entry_id", entryId)
                if (holdSec != null) put("hold_sec", holdSec)
            })
        }
    }

    data object ClearSets : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearSets")
    }

    /**
     * Mark (or reopen) an exercise as ended for a session day. The
     * user tapped "End exercise" on a plan item to finish it before every
     * prescribed set is logged (an RPE-stop, a dropped last set); `ended=false`
     * (the "Reopen" action) undoes it. Last-write-wins per
     * (same_movement-normalized exercise, epoch_day); a later ClearSets drops
     * every marker. Persisted so the day stays settled across relaunches.
     */
    data class SetExerciseEnded(
        val exercise: String,
        val epochDay: Long,
        val ended: Boolean,
        val observedAt: Long = AppClock.nowSec(),
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("SetExerciseEnded", buildJsonObject {
                put("exercise", exercise)
                put("epoch_day", epochDay)
                put("ended", ended)
                put("observed_at", observedAt)
            })
        }
    }

    data class LogRun(
        val distanceKm: Double,
        val durationMin: Double,
        val hrPctMax: Double,
        val observedAt: Long = AppClock.nowSec(),
        val entryId: Long = EntryIds.next(),
        // User-declared run-intent label; null = untagged. Omitted from the
        // wire when null so the event matches serde's `#[serde(default)]` shape.
        val workoutType: WorkoutType? = null,
        // Whole-session effort, CR-10 (0–10); null = unrated, omitted from wire.
        val sessionRpe: Int? = null,
        // Average heart rate in bpm; the core converts it to %HRmax from the
        // profile's measured HRmax (else the age estimate). Null = omitted.
        val avgHrBpm: Double? = null,
        // The device's UTC offset (seconds east) when the run was logged: the
        // core reads the run's local day with it, so a later time-zone change
        // never re-converts its %HRmax. Production senders pass utcOffsetSec();
        // Undo re-sends the row's echoed one. Null (legacy) = omitted.
        val utcOffsetSec: Int? = null,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("LogRun", buildJsonObject {
                putManualRun(distanceKm, durationMin, hrPctMax, observedAt, workoutType, sessionRpe, avgHrBpm)
                put("entry_id", entryId)
                if (utcOffsetSec != null) put("utc_offset_sec", utcOffsetSec)
            })
        }
    }

    data class LogRunTrack(
        val points: List<GpsPoint>,
        val hrPctMax: Double,
        // The session's logged-at stamp for history display; distinct from each
        // GPS fix's own per-point `observed_at`.
        val observedAt: Long = AppClock.nowSec(),
        val entryId: Long = EntryIds.next(),
        // User-declared run-intent label; null = untagged, omitted from wire.
        val workoutType: WorkoutType? = null,
        // Indices into [points] that begin a new recording segment: the pause +
        // relocation boundaries. The core skips each pause-bridge leg
        // (no distance, no time) and breaks the GPX <trkseg> there, keeping the
        // TRUE coordinates. Empty (the common, un-paused run) is OMITTED from the
        // wire so it matches serde's `#[serde(default)]` shape; old-shape parity.
        val segmentStarts: List<Int> = emptyList(),
        // Whole-session effort, CR-10 (0–10); null = unrated, omitted from wire.
        val sessionRpe: Int? = null,
        // Average heart rate in bpm (see [LogRun.avgHrBpm]); null = omitted.
        val avgHrBpm: Double? = null,
        // The device's UTC offset when the run was logged (see [LogRun.utcOffsetSec]).
        val utcOffsetSec: Int? = null,
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
                put("longest_recent_km", 0.0)
                put("observed_at", observedAt)
                put("entry_id", entryId)
                if (workoutType != null) put("workout_type", workoutType.name)
                if (segmentStarts.isNotEmpty()) {
                    putJsonArray("segment_starts") { segmentStarts.forEach { add(it) } }
                }
                if (sessionRpe != null) put("session_rpe", sessionRpe)
                if (avgHrBpm != null) put("avg_hr_bpm", avgHrBpm)
                if (utcOffsetSec != null) put("utc_offset_sec", utcOffsetSec)
            })
        }
    }

    data object ClearRuns : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearRuns")
    }

    /**
     * Other moderate-or-harder activity minutes (sport, cycling, PE, …) for the
     * under-18 weekly activity card (youth-pa-001). The core ignores a row whose
     * minutes are not finite or outside (0, 600].
     */
    data class LogActivity(
        val kind: ActivityKind,
        val minutes: Double,
        val vigorous: Boolean = false,
        val strengthening: Boolean = false,
        val observedAt: Long = AppClock.nowSec(),
        val entryId: Long = EntryIds.next(),
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("LogActivity", buildJsonObject {
                put("kind", kind.name)
                put("minutes", minutes)
                put("vigorous", vigorous)
                put("strengthening", strengthening)
                put("observed_at", observedAt)
                put("entry_id", entryId)
            })
        }
    }

    /** Drop every logged activity (part of "Clear all data"). */
    data object ClearActivities : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearActivities")
    }

    /** Which logged family a [DeleteEntry] targets (mirrors app/mod.rs EntryKind). */
    enum class EntryKind { Set, Run, Activity }

    /**
     * Delete one logged set, run or activity. The core removes the newest
     * entry whose `entry_id` matches, or, for a legacy row with no id, whose
     * `observed_at` matches [observedAtFallback]. A no-op when nothing matches.
     */
    data class DeleteEntry(
        val kind: EntryKind,
        val entryId: Long,
        val observedAtFallback: Long = 0,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("DeleteEntry", buildJsonObject {
                put("kind", kind.name)
                put("entry_id", entryId)
                put("observed_at_fallback", observedAtFallback)
            })
        }
    }

    /** Edit one logged set's fields in place. The core deletes the
     *  matched set and re-pushes it carrying the SAME [entryId]. */
    data class AmendSet(
        val entryId: Long,
        val exercise: String,
        val weightKg: Double,
        val reps: Int,
        val rpe: Double,
        val observedAt: Long,
        // The row's ORIGINAL observed_at (before any date change), so a legacy
        // (entry_id == 0) row is matched and REPLACED rather than duplicated.
        val observedAtFallback: Long = 0,
        // Timed-hold seconds (see LogSet.holdSec); null for a reps×load set.
        val holdSec: Int? = null,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("AmendSet", buildJsonObject {
                put("entry_id", entryId)
                put("exercise", exercise)
                put("weight_kg", weightKg)
                put("reps", reps)
                put("rpe", rpe)
                put("observed_at", observedAt)
                put("observed_at_fallback", observedAtFallback)
                if (holdSec != null) put("hold_sec", holdSec)
            })
        }
    }

    /** Edit one hand-entered run's fields. GPS-tracked runs are delete-only for
     *  field edits, so amending one replaces it with a manual run; the sole
     *  exception is [TrimRunTrack], which cuts a GPS run's head/tail in place. */
    data class AmendRun(
        val entryId: Long,
        val distanceKm: Double,
        val durationMin: Double,
        val hrPctMax: Double,
        val observedAt: Long,
        // The row's ORIGINAL observed_at (before any date change), so a legacy
        // (entry_id == 0) row is matched and REPLACED rather than duplicated.
        val observedAtFallback: Long = 0,
        // User-declared run-intent label; null = untagged, omitted from wire.
        val workoutType: WorkoutType? = null,
        // Whole-session effort, CR-10 (0–10); null = unrated, omitted from wire.
        val sessionRpe: Int? = null,
        // Average heart rate in bpm (see [LogRun.avgHrBpm]); null = omitted.
        val avgHrBpm: Double? = null,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("AmendRun", buildJsonObject {
                put("entry_id", entryId)
                putManualRun(distanceKm, durationMin, hrPctMax, observedAt, workoutType, sessionRpe, avgHrBpm)
                put("observed_at_fallback", observedAtFallback)
            })
        }
    }

    /** Trim a saved GPS run's recorded head/tail: keep only the fixes whose
     *  per-point observed_at lies in [keepFromSec, keepUntilSec] (inclusive,
     *  absolute unix seconds). The run's identity (entry_id, observed_at) is
     *  UNCHANGED and its track is edited in place; every derived figure
     *  re-derives in the core. [observedAtFallback] is the run's observed_at, so
     *  a legacy (entry_id == 0) run is located by timestamp. Matches like
     *  DeleteEntry/AmendRun. The one field-edit allowed on a GPS run. */
    data class TrimRunTrack(
        val entryId: Long,
        val observedAtFallback: Long,
        val keepFromSec: Long,
        val keepUntilSec: Long,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("TrimRunTrack", buildJsonObject {
                put("entry_id", entryId)
                put("observed_at_fallback", observedAtFallback)
                put("keep_from_sec", keepFromSec)
                put("keep_until_sec", keepUntilSec)
            })
        }
    }

    /** Rate or tag one logged run IN PLACE (GPS runs included): its whole-session
     *  CR-10 effort and its workout tag. BOTH fields are replaced on every send,
     *  so a null clears that field: callers pass the current value of the one they
     *  aren't changing. [observedAtFallback] locates a legacy (entry_id 0) run. */
    data class AmendRunMeta(
        val entryId: Long,
        val observedAtFallback: Long,
        val sessionRpe: Int?,
        val workoutType: WorkoutType?,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("AmendRunMeta", buildJsonObject {
                put("entry_id", entryId)
                put("observed_at_fallback", observedAtFallback)
                put("session_rpe", sessionRpe)
                put("workout_type", workoutType?.name)
            })
        }
    }

    data class SubmitReview(
        val bonePainRedFlag: Boolean = false,
        val compulsiveFlag: Boolean = false,
        val overtrainingSignalCount: Int = 0,
        val decoupling: Decouple? = null,
        val easyFracTimeAboveVt1: Double? = null,
        val positiveSplitPct: Double? = null,
        val rpeLoadGapSessions: Int? = null,
        val weeklyVelocityDropMs: Double? = null,
        val failedKeySessions: Int? = null,
        val badDay: Boolean = false,
        // How long the overtraining signs have co-occurred, in weeks (<1 / 1–2 / >2
        // answered as 0.5 / 1.5 / 3.0). Null = not asked.
        val overtrainingSignalWeeks: Double? = null,
        // Which of the four listed overtraining signs were ticked; when present
        // the core counts the ticked signs instead of [overtrainingSignalCount].
        val overtrainingSigns: OvertrainingSigns? = null,
        // Weekly RED-S screen answered yes (REDS-SCREEN-001 referral).
        val redsSignalReported: Boolean = false,
        // When the review was submitted, unix seconds (backdatable). Baked into
        // the persisted line so replay keeps the original stamp.
        val observedAt: Long = AppClock.nowSec(),
    ) : Event {
        /** The SessionReview payload shared by SubmitReview and SubmitWeeklyCheckin. */
        fun payload() = buildJsonObject {
            put("bone_pain_red_flag", bonePainRedFlag)
            put("compulsive_flag", compulsiveFlag)
            put("overtraining_signal_count", overtrainingSignalCount)
            if (overtrainingSignalWeeks != null)
                put("overtraining_signal_weeks", overtrainingSignalWeeks)
            if (overtrainingSigns != null) put("overtraining_signs", buildJsonObject {
                put("worse_performance", overtrainingSigns.worsePerformance)
                put("low_mood", overtrainingSigns.lowMood)
                put("higher_resting_hr", overtrainingSigns.higherRestingHr)
                put("worse_sleep", overtrainingSigns.worseSleep)
            })
            if (redsSignalReported) put("reds_signal_reported", true)
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
        }

        override fun toJson() = buildJsonObject { put("SubmitReview", payload()) }
    }

    /** The weekly check-in (week-level counts + clinical screens): the core reads
     *  only the weekly part of [review] and keeps every one as history. */
    data class SubmitWeeklyCheckin(val review: SubmitReview) : Event {
        override fun toJson() = buildJsonObject { put("SubmitWeeklyCheckin", review.payload()) }
    }

    data object ClearWeeklyCheckin : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearWeeklyCheckin")
    }

    data object ClearReview : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearReview")
    }

    data class PredictRace(
        val recentDistanceM: Double,
        val recentTimeSec: Double,
        val goalDistanceM: Double,
        val weeklyKm: Double,
        // Weeks since the input race (running-041 freshness). null keeps the wire
        // pre-freshness-identical; a value lets the core flag a stale result.
        val weeksSinceRace: Int? = null,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("PredictRace", buildJsonObject {
                put("recent_distance_m", recentDistanceM)
                put("recent_time_sec", recentTimeSec)
                put("goal_distance_m", goalDistanceM)
                put("weekly_km", weeklyKm)
                if (weeksSinceRace != null) put("weeks_since_race", weeksSinceRace)
            })
        }
    }

    data object ClearRacePrediction : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearRacePrediction")
    }

    data class PlanHypertrophyMeso(
        val muscle: String,
        val weeks: Int,
        // hypertrophy-008 next-meso decision inputs (mirror the core's
        // PlanHypertrophyMeso event): the muscle is not growing on the current
        // volume, and recovery from it is easy. Both #[serde(default)] on the
        // core, so each is emitted only when true and a plain plan keeps the
        // same two-field wire.
        val notGrowing: Boolean = false,
        val recoveringEasily: Boolean = false,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("PlanHypertrophyMeso", buildJsonObject {
                put("muscle", muscle)
                put("weeks", weeks)
                if (notGrowing) put("not_growing", true)
                if (recoveringEasily) put("recovering_easily", true)
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

    data class ComputeHrZones(
        val ageYears: Double,
        // Resting HR (bpm) so the core can run Karvonen heart-rate-reserve zones
        // (running-005) instead of plain %HRmax. null keeps the age-only wire.
        val restingHrBpm: Double? = null,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("ComputeHrZones", buildJsonObject {
                put("age_years", ageYears)
                if (restingHrBpm != null) put("resting_hr_bpm", restingHrBpm)
            })
        }
    }

    data object ClearHrZones : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearHrZones")
    }

    /** Accept a synthesized plan: "Plan my training". */
    data class GeneratePlan(val startEpochDay: Long) : Event {
        override fun toJson(): JsonElement = buildJsonObject {
            put("GeneratePlan", buildJsonObject { put("start_epoch_day", startEpochDay) })
        }
    }

    /** Drop the plan (Coach returns to no-plan state). */
    data object ClearPlan : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearPlan")
    }

    /** Move a planned session to another day of its week (one of its
     *  `move_targets`). Kept with the plan request; ClearPlan sweeps it. */
    data class MoveSession(val fromEpochDay: Long, val toEpochDay: Long) : Event {
        override fun toJson(): JsonElement = buildJsonObject {
            put("MoveSession", buildJsonObject {
                put("from_epoch_day", fromEpochDay)
                put("to_epoch_day", toEpochDay)
            })
        }
    }

    /** "I'm pain-free now" / "I've recovered": retracts every active report of
     *  [signal] (history kept) and the coach restarts with an easier session. */
    data class ResolveHold(
        val signal: ReadinessSignal,
        val observedAt: Long = AppClock.nowSec(),
    ) : Event {
        override fun toJson(): JsonElement = buildJsonObject {
            put("ResolveHold", buildJsonObject {
                put("signal", signal.name)
                put("observed_at", observedAt)
            })
        }
    }

    /** "Relearn your HRV normal" (e.g. after a device change): HRV readings before [observedAt]
     *  stop counting toward the baseline, which starts collecting again. */
    data class RestartHrvBaseline(val observedAt: Long = AppClock.nowSec()) : Event {
        override fun toJson(): JsonElement = buildJsonObject {
            put("RestartHrvBaseline", buildJsonObject { put("observed_at", observedAt) })
        }
    }

    /** The shell's clock as event data: today's epoch-day, sent on
     *  foreground so the core dates the week + picks the next session. Also carries
     *  the device's current UTC offset in seconds EAST of UTC so the core can
     *  match a logged session's UTC `observed_at` to the correct LOCAL day. */
    data class SetToday(val epochDay: Long, val utcOffsetSec: Int = 0) : Event {
        // The shell's clock, re-sent every foreground; kept as one last-write-wins
        // line, so it is applied in-memory but never appended to the durable log.
        override val persist: Boolean get() = false
        override fun toJson(): JsonElement = buildJsonObject {
            put("SetToday", buildJsonObject {
                put("epoch_day", epochDay)
                put("utc_offset_sec", utcOffsetSec)
            })
        }
    }
}

/** The fields a hand-entered run's [Event.LogRun] and [Event.AmendRun] share. */
private fun JsonObjectBuilder.putManualRun(
    distanceKm: Double,
    durationMin: Double,
    hrPctMax: Double,
    observedAt: Long,
    workoutType: WorkoutType?,
    sessionRpe: Int?,
    avgHrBpm: Double?,
) {
    put("distance_km", distanceKm)
    put("duration_min", durationMin)
    put("hr_pct_max", hrPctMax)
    // Always 0: the core derives the baseline from logged runs.
    put("longest_recent_km", 0.0)
    put("observed_at", observedAt)
    if (workoutType != null) put("workout_type", workoutType.name)
    if (sessionRpe != null) put("session_rpe", sessionRpe)
    if (avgHrBpm != null) put("avg_hr_bpm", avgHrBpm)
}

/** The four listed overtraining signs on the weekly check-in (model.rs OvertrainingSigns). */
data class OvertrainingSigns(
    val worsePerformance: Boolean = false,
    val lowMood: Boolean = false,
    val higherRestingHr: Boolean = false,
    val worseSleep: Boolean = false,
) {
    val any: Boolean get() = worsePerformance || lowMood || higherRestingHr || worseSleep
}

/** Aerobic-decoupling context for a run review. Mirrors shared/src/app/mod.rs::Decouple. */
data class Decouple(val driftPct: Double, val coolSteadyContext: Boolean)

/** One GPS fix. Mirrors shared/src/running.rs::GpsPoint. */
data class GpsPoint(
    val lat: Double,
    val lon: Double,
    val observedAt: Long,
    val accuracyM: Double,
)
