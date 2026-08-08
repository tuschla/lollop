package app.milestone

import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Pins the serde wire form the Rust core expects: unit variants encode as a bare
 * string, struct variants as a single-key object with snake_case fields. A drift
 * here means the FFI silently rejects (or mis-reads) the event, so these guard
 * the Kotlin→Rust contract without needing the native library.
 */
class EventJsonTest {

    private fun obj(e: Event) = (e.toJson() as JsonObject)

    @Test
    fun unitVariantsAreBareStrings() {
        assertEquals("\"ClearSets\"", Event.ClearSets.toJson().toString())
        assertEquals("\"ClearRuns\"", Event.ClearRuns.toJson().toString())
        assertEquals("\"ClearProtein\"", Event.ClearProtein.toJson().toString())
    }

    @Test
    fun logSetHasSnakeCaseFields() {
        val fields = obj(Event.LogSet("Back Squat", 100.0, 5, 8.0, 1_700_000_000L))["LogSet"]!!.jsonObject
        assertEquals("Back Squat", fields["exercise"]!!.jsonPrimitive.content)
        assertEquals(100.0, fields["weight_kg"]!!.jsonPrimitive.content.toDouble(), 1e-9)
        assertEquals(5, fields["reps"]!!.jsonPrimitive.content.toInt())
        assertEquals(8.0, fields["rpe"]!!.jsonPrimitive.content.toDouble(), 1e-9)
        // Log time rides on the wire as snake_case unix seconds so the core can
        // carry it back into the history view (LiftResultView.observed_at).
        assertEquals(1_700_000_000L, fields["observed_at"]!!.jsonPrimitive.content.toLong())
    }

    @Test
    fun setProfileEmitsAllNineFields() {
        val fields = obj(
            Event.SetProfile(
                ProgressionCadence.WeekToWeek, LiftGoal.MaxStrength, GoalDistance.TenK,
                ConcurrentGoal.Strength, 12, 4, 45.0, true, 75.0,
            )
        )["SetProfile"]!!.jsonObject
        assertEquals(
            setOf(
                "progression_cadence", "lift_goal", "goal_distance", "concurrent_goal",
                "weekly_sets", "running_days_per_week", "running_km_per_week", "advanced",
                "endurance_intensity_pct_vo2max",
            ),
            fields.keys,
        )
        // Enum values are the variant name verbatim (serde reads these).
        assertEquals("WeekToWeek", fields["progression_cadence"]!!.jsonPrimitive.content)
        assertEquals("TenK", fields["goal_distance"]!!.jsonPrimitive.content)
        // Person fields (Phase 5 / M5) absent by default → wire stays the old
        // nine-field shape, so old cores replay it identically.
        assertFalse(fields.containsKey("female"))
        assertFalse(fields.containsKey("bodyweight_kg"))
        assertFalse(fields.containsKey("age_years"))
    }

    @Test
    fun setProfileEmitsPersonFieldsWhenSet() {
        val fields = obj(
            Event.SetProfile(
                ProgressionCadence.EverySession, LiftGoal.Hypertrophy, GoalDistance.General,
                ConcurrentGoal.Hypertrophy, 10, 0, 0.0, false, 75.0,
                female = true, bodyweightKg = 62.5, ageYears = 34.0,
                restingHrBpm = 54.0, measuredHrMax = 188.0,
            )
        )["SetProfile"]!!.jsonObject
        // snake_case names the core parses back; values verbatim.
        assertTrue(fields["female"]!!.jsonPrimitive.content.toBoolean())
        assertEquals(62.5, fields["bodyweight_kg"]!!.jsonPrimitive.content.toDouble(), 1e-9)
        assertEquals(34.0, fields["age_years"]!!.jsonPrimitive.content.toDouble(), 1e-9)
        assertEquals(54.0, fields["resting_hr_bpm"]!!.jsonPrimitive.content.toDouble(), 1e-9)
        assertEquals(188.0, fields["measured_hr_max"]!!.jsonPrimitive.content.toDouble(), 1e-9)
    }

    @Test
    fun setProfileOmitsFemaleWhenFalse() {
        // female is emitted only when true (keeps the all-absent line byte-identical
        // to the pre-Phase-5 form and lets the core's serde default handle absence).
        val fields = obj(
            Event.SetProfile(
                ProgressionCadence.WeekToWeek, LiftGoal.MaxStrength, GoalDistance.TenK,
                ConcurrentGoal.Strength, 12, 4, 45.0, false, 75.0,
                female = false, bodyweightKg = 80.0,
            )
        )["SetProfile"]!!.jsonObject
        assertFalse(fields.containsKey("female"))
        assertTrue(fields.containsKey("bodyweight_kg"))
    }

    @Test
    fun submitReviewOmitsNullOptionalsButKeepsRequiredFlags() {
        val fields = obj(Event.SubmitReview(overtrainingSignalCount = 2))["SubmitReview"]!!.jsonObject
        // Required always present.
        assertEquals(2, fields["overtraining_signal_count"]!!.jsonPrimitive.content.toInt())
        assertTrue(fields.containsKey("bone_pain_red_flag"))
        assertTrue(fields.containsKey("bad_day"))
        // Null optionals are omitted (serde skips them on the Rust side too).
        assertFalse(fields.containsKey("single_session_spike_frac"))
        assertFalse(fields.containsKey("lift"))
        assertFalse(fields.containsKey("decoupling"))
    }

    @Test
    fun submitReviewIncludesNestedLiftWhenPresent() {
        val fields = obj(
            Event.SubmitReview(lift = LiftExec(repsMet = true, rirActual = 1, rirTarget = 2))
        )["SubmitReview"]!!.jsonObject
        val lift = fields["lift"]!!.jsonObject
        assertEquals(true, lift["reps_met"]!!.jsonPrimitive.content.toBoolean())
        assertEquals(1, lift["rir_actual"]!!.jsonPrimitive.content.toInt())
        assertEquals(2, lift["rir_target"]!!.jsonPrimitive.content.toInt())
    }

    @Test
    fun readinessSignalEnumCoversMedicalReferralTiers() {
        // These top-of-ladder signals must exist or the safety tier is unreachable.
        val names = ReadinessSignal.entries.map { it.name }
        assertTrue(names.containsAll(listOf("Pain", "Illness", "RedS", "CardiacRedFlag", "BoneStress")))
        val fields = obj(Event.SubmitReadiness(ReadinessSignal.Pain, 1.0, 42L))["SubmitReadiness"]!!.jsonObject
        assertEquals("Pain", fields["signal"]!!.jsonPrimitive.content)
        assertEquals(42L, fields["observed_at"]!!.jsonPrimitive.content.toLong())
        // Phase 1 wire contract: a bare readiness report still sends streak (0)
        // and OMITS the pain object entirely (serde reads it as None).
        assertEquals(0, fields["streak"]!!.jsonPrimitive.content.toInt())
        assertFalse(fields.containsKey("pain"))
    }

    @Test
    fun submitReadinessCarriesFullPainDetail() {
        // The B2 fix: a characterized pain report reaches the core's graded pain
        // gate. The nested PainDetail must match the Rust serde shape exactly
        // (snake_case, enum variant names verbatim, streak alongside).
        val fields = obj(
            Event.SubmitReadiness(
                signal = ReadinessSignal.Pain,
                value = 1.0,
                observedAt = 100L,
                streak = 2,
                pain = PainDetail(
                    kind = PainKind.TendonLoadRelated,
                    severity = 4,
                    trend = PainTrend.Rising,
                    persists = false,
                    location = "Left knee",
                ),
            )
        )["SubmitReadiness"]!!.jsonObject
        assertEquals(2, fields["streak"]!!.jsonPrimitive.content.toInt())
        val pain = fields["pain"]!!.jsonObject
        assertEquals("TendonLoadRelated", pain["kind"]!!.jsonPrimitive.content)
        assertEquals(4, pain["severity"]!!.jsonPrimitive.content.toInt())
        assertEquals("Rising", pain["trend"]!!.jsonPrimitive.content)
        assertEquals(false, pain["persists"]!!.jsonPrimitive.content.toBoolean())
        assertEquals("Left knee", pain["location"]!!.jsonPrimitive.content)
    }

    @Test
    fun painDetailOmitsLocationWhenAbsent() {
        // location is optional (serde default None on the Rust side): omitted
        // from the wire when the user didn't pick a body area.
        val pain = obj(
            Event.SubmitReadiness(
                ReadinessSignal.Pain, 1.0, 5L,
                pain = PainDetail(PainKind.SharpJoint, 7, PainTrend.Stable),
            )
        )["SubmitReadiness"]!!.jsonObject["pain"]!!.jsonObject
        assertEquals("SharpJoint", pain["kind"]!!.jsonPrimitive.content)
        assertFalse(pain.containsKey("location"))
    }

    @Test
    fun submitReadinessEmitsEffortMinOnlyWhenPresent() {
        // A6 fix: AerobicDecoupling is duration-gated (valid only >20 min); the
        // editor now sends the run length so the core can validate it instead of
        // silently discarding a None-duration reading. effort_min is snake_case,
        // matching ReadinessInput's serde field, and OMITTED when not supplied.
        val withDuration = obj(
            Event.SubmitReadiness(
                signal = ReadinessSignal.AerobicDecoupling,
                value = 12.0,
                observedAt = 7L,
                effortMin = 30.0,
            )
        )["SubmitReadiness"]!!.jsonObject
        assertEquals(30.0, withDuration["effort_min"]!!.jsonPrimitive.content.toDouble(), 0.0)
        // A signal without a duration omits the field (serde default None).
        val bare = obj(
            Event.SubmitReadiness(ReadinessSignal.WellnessZ, -1.5, 7L)
        )["SubmitReadiness"]!!.jsonObject
        assertFalse(bare.containsKey("effort_min"))
    }

    @Test
    fun viewModelDecodesCoreSplitVerdictAndE1rmDelta() {
        // Rust→Kotlin side of the contract: the additive history fields
        // (runs[i].split, lifts[i].e1rm_delta_kg/e1rm_direction) decode, and
        // their absence (hand-entered run / first set) stays null: the shell
        // renders these purely from the wire, no local thresholds/arithmetic.
        val json = kotlinx.serialization.json.Json { ignoreUnknownKeys = true }
        val vm = json.decodeFromString<ViewModel>(
            """{"safety_tier":null,"train_blocked":false,"adjustments":[],
                "review_adjustments":[],"input_count":2,
                "lifts":[
                  {"exercise":"Squat","weight_kg":100.0,"reps":5,"rpe":8.0,
                   "e1rm_kg":116.7,"pct_1rm":86.0,"rir":2.0,"summary":"","observed_at":1},
                  {"exercise":"Squat","weight_kg":102.5,"reps":5,"rpe":8.0,
                   "e1rm_kg":119.6,"pct_1rm":86.0,"rir":2.0,
                   "e1rm_delta_kg":2.9,"e1rm_direction":"up","summary":"","observed_at":2}],
                "runs":[
                  {"zone":"Z1","pace":"6:00/km","distance_km":8.0,"spike_flag":false,
                   "spike_note":"","split_pct":5.2,
                   "split":{"verdict":"fade","label":"FADE +5%",
                     "message":"Start easier next time.","grade":"Moderate",
                     "citation":"feedback-016","confidence":0.7,
                     "safety_critical":false,"contested":false},
                   "summary":"","citation":"","gpx":"","observed_at":3},
                  {"zone":"Z2","pace":"5:30/km","distance_km":5.0,"spike_flag":false,
                   "spike_note":"","split_pct":null,"split":null,
                   "summary":"","citation":"","gpx":"","observed_at":4}],
                "guidance":[],"feedback":null,"reference":[]}"""
        )
        assertEquals(null, vm.lifts[0].e1rm_delta_kg)
        assertEquals(null, vm.lifts[0].e1rm_direction)
        assertEquals(2.9, vm.lifts[1].e1rm_delta_kg!!, 1e-9)
        assertEquals("up", vm.lifts[1].e1rm_direction)
        val split = vm.runs[0].split!!
        assertEquals("fade", split.verdict)
        assertEquals("FADE +5%", split.label)
        assertEquals("Moderate", split.grade)
        assertEquals("feedback-016", split.citation)
        assertEquals(null, vm.runs[1].split)
    }

    @Test
    fun submitReviewCarriesObservedAt() {
        // Backdating: the review's log stamp always rides the wire (serde
        // default on the Rust side keeps old logs replayable without it).
        val fields = obj(
            Event.SubmitReview(overtrainingSignalCount = 0, observedAt = 1_700_000_777L)
        )["SubmitReview"]!!.jsonObject
        assertEquals(1_700_000_777L, fields["observed_at"]!!.jsonPrimitive.content.toLong())
    }

    @Test
    fun viewModelDecodesReadinessSummaryHeadlineAndSignalGroups() {
        // Rust→Kotlin contract for the KB-honest readiness rework: per-signal
        // states (+ their judging rule's evidence), the core-owned today
        // headline, and the static signal→group fence metadata all decode.
        val json = kotlinx.serialization.json.Json { ignoreUnknownKeys = true }
        val vm = json.decodeFromString<ViewModel>(
            """{"safety_tier":null,"train_blocked":false,"adjustments":[],
                "review_adjustments":[],"input_count":1,"lifts":[],"runs":[],
                "guidance":[],"feedback":null,"reference":[],
                "readiness_summary":[
                  {"signal":"HrvLnRmssd","group":"metric","value":-1.0,"streak":0,
                   "state":"suppressed","grade":"Moderate","citation":"Kiviniemi 2007",
                   "confidence":0.65,"safety_critical":false,"contested":false},
                  {"signal":"Pain","group":"red_flag","value":1.0,"streak":0,
                   "state":"red flag - stop","grade":"Strong","citation":"safety",
                   "confidence":0.9,"safety_critical":true,"contested":false}],
                "today_headline":{"kind":"safety_hold","summary":"Stop - do not train",
                  "grade":"Strong","citation":"safety","confidence":0.9,
                  "safety_critical":true,"contested":false},
                "signal_groups":[
                  {"signal":"Rpe","group":"metric"},
                  {"signal":"Pain","group":"red_flag"}]}"""
        )
        assertEquals("suppressed", vm.readiness_summary[0].state)
        assertEquals("metric", vm.readiness_summary[0].group)
        assertTrue(vm.readiness_summary[1].safety_critical)
        assertEquals("safety_hold", vm.today_headline!!.kind)
        assertEquals("red_flag", vm.signal_groups.first { it.signal == "Pain" }.group)
        // Pre-headline core (fields absent) must still decode: null/empty.
        val old = json.decodeFromString<ViewModel>(
            """{"safety_tier":null,"train_blocked":false,"adjustments":[],
                "review_adjustments":[],"input_count":0,"lifts":[],"runs":[],
                "guidance":[],"feedback":null,"reference":[]}"""
        )
        assertEquals(null, old.today_headline)
        assertTrue(old.readiness_summary.isEmpty())
        assertTrue(old.signal_groups.isEmpty())
    }

    @Test
    fun viewModelDecodesWhyDisclosureAndGradeDefinitions() {
        // Phase 3 / M2: the three-part why? block on action cards and the
        // core-provided evidence-grade legend decode from the wire.
        val json = kotlinx.serialization.json.Json { ignoreUnknownKeys = true }
        val vm = json.decodeFromString<ViewModel>(
            """{"safety_tier":null,"train_blocked":false,
                "adjustments":[
                  {"summary":"Downgrade to an easier session","grade":"Moderate",
                   "citation":"Plews 2013","confidence":0.65,"safety_critical":false,
                   "contested":false,
                   "why":{"basis":"Your 7-day HRV average is below your normal band.",
                     "grade_note":"Moderate evidence - mixed or limited randomized trials.",
                     "improves":"Keep logging morning check-ins to tighten the baseline."}}],
                "review_adjustments":[],"input_count":1,"lifts":[],"runs":[],
                "guidance":[
                  {"section":"Heart-rate zones","summary":"Estimated HRmax: 187 bpm",
                   "grade":"Weak","citation":"Tanaka","confidence":0.4,
                   "safety_critical":false,"contested":false,
                   "why":{"basis":"Estimated from your age (30) with the Tanaka formula.",
                     "grade_note":"Weak evidence - from mechanism or observation.",
                     "improves":"Log a measured max HR from an all-out effort to replace the age-based estimate."}}],
                "feedback":null,"reference":[],
                "today_headline":{"kind":"adjustment","summary":"Downgrade to an easier session",
                  "grade":"Moderate","citation":"Plews 2013","confidence":0.65,
                  "safety_critical":false,"contested":false,
                  "why":{"basis":"HRV below band.","grade_note":"Moderate evidence.","improves":"More check-ins."}},
                "grade_definitions":[
                  {"grade":"Strong","label":"Strong",
                   "definition":"Well-replicated meta-analyses or randomized controlled trials.","confidence":0.9},
                  {"grade":"Weak","label":"Weak",
                   "definition":"Mechanistic or observational evidence only.","confidence":0.4}]}"""
        )
        // why? triad decodes on adjustment, guidance, and headline.
        assertEquals("Your 7-day HRV average is below your normal band.", vm.adjustments[0].why.basis)
        assertTrue(vm.adjustments[0].why.improves.contains("check-ins"))
        assertTrue(vm.guidance[0].why.basis.contains("Tanaka"))
        assertTrue(vm.guidance[0].why.improves.contains("measured max HR"))
        assertEquals("adjustment", vm.today_headline!!.kind)
        assertTrue(vm.today_headline!!.why.grade_note.contains("Moderate"))
        // Grade legend from core data.
        assertEquals(2, vm.grade_definitions.size)
        val strong = vm.grade_definitions.first { it.grade == "Strong" }
        assertTrue(strong.definition.contains("meta-analyses"))
        assertEquals(0.9f, strong.confidence, 1e-6f)

        // Old core (no why?, no grade_definitions) decodes to empty defaults -
        // the shell falls back to the legacy restatement / hardcoded legend.
        val old = json.decodeFromString<ViewModel>(
            """{"safety_tier":null,"train_blocked":false,
                "adjustments":[
                  {"summary":"Take a rest day","grade":"Strong","citation":"x",
                   "confidence":0.9,"safety_critical":true,"contested":false}],
                "review_adjustments":[],"input_count":0,"lifts":[],"runs":[],
                "guidance":[],"feedback":null,"reference":[]}"""
        )
        assertEquals("", old.adjustments[0].why.basis)
        assertEquals("", old.adjustments[0].why.grade_note)
        assertTrue(old.grade_definitions.isEmpty())
    }

    @Test
    fun submitCheckinEmitsSnakeCaseAndOmitsAbsentOptionals() {
        // Phase 2 / B1: the morning check-in wire form. Answered items ride the
        // wire as snake_case; an unset watch number is OMITTED (serde reads None),
        // so the core never fabricates a channel the user didn't provide.
        val fields = obj(
            Event.SubmitCheckin(
                observedAt = 1_700_000_000L,
                sleepQuality = 2,
                soreness = 4,
                mood = 3,
                restingHrBpm = 52.0,
                // hrvRmssdMs left null → omitted.
            )
        )["SubmitCheckin"]!!.jsonObject
        assertEquals(1_700_000_000L, fields["observed_at"]!!.jsonPrimitive.content.toLong())
        assertEquals(2, fields["sleep_quality"]!!.jsonPrimitive.content.toInt())
        assertEquals(4, fields["soreness"]!!.jsonPrimitive.content.toInt())
        assertEquals(3, fields["mood"]!!.jsonPrimitive.content.toInt())
        assertEquals(52.0, fields["resting_hr_bpm"]!!.jsonPrimitive.content.toDouble(), 1e-9)
        assertFalse(fields.containsKey("hrv_rmssd_ms"))
    }

    @Test
    fun clearCheckinsIsABareStringUnitVariant() {
        assertEquals("\"ClearCheckins\"", Event.ClearCheckins.toJson().toString())
    }

    @Test
    fun viewModelDecodesCheckinEchoAndBaselineStatus() {
        // Rust→Kotlin contract for the check-in view surface: the echoed check-in
        // (rehydration) and the honest "collecting baseline" rows decode, and
        // their absence on an old core stays null/empty.
        val json = kotlinx.serialization.json.Json { ignoreUnknownKeys = true }
        val vm = json.decodeFromString<ViewModel>(
            """{"safety_tier":null,"train_blocked":false,"adjustments":[],
                "review_adjustments":[],"input_count":0,"lifts":[],"runs":[],
                "guidance":[],"feedback":null,"reference":[],
                "checkin_today":{"observed_at":1700000000,"sleep_quality":2,
                  "soreness":4,"mood":3,"resting_hr_bpm":52.0,"hrv_rmssd_ms":null},
                "baseline_status":[
                  {"signal":"WellnessZ","label":"Sleep, soreness & mood","have":1,
                   "need":7,"note":"Collecting your baseline - 1 of 7 check-ins"}]}"""
        )
        assertEquals(2, vm.checkin_today!!.sleep_quality)
        assertEquals(4, vm.checkin_today!!.soreness)
        assertEquals(null, vm.checkin_today!!.hrv_rmssd_ms)
        assertEquals("WellnessZ", vm.baseline_status[0].signal)
        assertEquals(1, vm.baseline_status[0].have)
        assertTrue(vm.baseline_status[0].note.contains("Collecting your baseline"))

        // Old core (fields absent) still decodes to null/empty.
        val old = json.decodeFromString<ViewModel>(
            """{"safety_tier":null,"train_blocked":false,"adjustments":[],
                "review_adjustments":[],"input_count":0,"lifts":[],"runs":[],
                "guidance":[],"feedback":null,"reference":[]}"""
        )
        assertEquals(null, old.checkin_today)
        assertTrue(old.baseline_status.isEmpty())
    }

    @Test
    fun logRunTrackNestsPointsWithSnakeCaseFields() {
        val fields = obj(
            Event.LogRunTrack(listOf(GpsPoint(52.5, 13.4, 1000L, 4.5)), 80.0, 10.0, 1_700_000_500L)
        )["LogRunTrack"]!!.jsonObject
        val pt = fields["points"]!!.let { (it as kotlinx.serialization.json.JsonArray)[0] }.jsonObject
        assertEquals(52.5, pt["lat"]!!.jsonPrimitive.content.toDouble(), 1e-9)
        assertEquals(1000L, pt["observed_at"]!!.jsonPrimitive.content.toLong())
        assertEquals(4.5, pt["accuracy_m"]!!.jsonPrimitive.content.toDouble(), 1e-9)
        assertEquals(80.0, fields["hr_pct_max"]!!.jsonPrimitive.content.toDouble(), 1e-9)
        // Session log time is the run-level stamp, distinct from the per-fix one.
        assertEquals(1_700_000_500L, fields["observed_at"]!!.jsonPrimitive.content.toLong())
        // I15/B2: an un-paused save carries NO segment_starts key → matches serde's
        // `#[serde(default)]` empty Vec, so old logs / this common case are byte-
        // identical on the wire (back-compat).
        assertFalse("un-paused run omits segment_starts", fields.containsKey("segment_starts"))
    }

    @Test
    fun logRunTrackEmitsSegmentStartsArrayWhenPaused() {
        // A pause + relocation carries the boundary indices (into `points`) so the
        // segment-aware core skips each pause-bridge leg and breaks the GPX <trkseg>.
        val fields = obj(
            Event.LogRunTrack(
                points = listOf(
                    GpsPoint(0.0, 0.000, 0L, 5.0),
                    GpsPoint(0.0, 0.001, 10L, 5.0),
                    GpsPoint(0.0, 1.000, 70L, 5.0),
                    GpsPoint(0.0, 1.001, 80L, 5.0),
                ),
                hrPctMax = 0.0,
                longestRecentKm = 0.0,
                observedAt = 0L,
                segmentStarts = listOf(2),
            )
        )["LogRunTrack"]!!.jsonObject
        val starts = fields["segment_starts"]!! as kotlinx.serialization.json.JsonArray
        assertEquals(1, starts.size)
        assertEquals(2, starts[0].jsonPrimitive.content.toInt())
    }

    // ── I16: user-declared workout-type tag (USER DATA, no evidence) ─────────

    @Test
    fun workoutTypeIsOmittedWhenUntaggedAndEmittedAsBareVariantWhenSet() {
        // Untagged (null) → the key is ABSENT on the wire, so the event matches
        // serde's `#[serde(default)]` shape and old logs/replay are unaffected.
        val untagged = obj(
            Event.LogRun(8.0, 48.0, 70.0, 12.0, 1_700_000_000L)
        )["LogRun"]!!.jsonObject
        assertFalse(untagged.containsKey("workout_type"))

        // Tagged → the field carries the EXACT serde variant string (the Rust
        // `WorkoutType` deserializes external-form, so it must be the bare name).
        val tagged = obj(
            Event.LogRun(8.0, 48.0, 70.0, 12.0, 1_700_000_000L, workoutType = WorkoutType.Interval)
        )["LogRun"]!!.jsonObject
        assertEquals("Interval", tagged["workout_type"]!!.jsonPrimitive.content)

        // Same contract on the GPS + amend events.
        val track = obj(
            Event.LogRunTrack(listOf(GpsPoint(0.0, 0.0, 0L, 5.0)), 0.0, 0.0, 0L, workoutType = WorkoutType.LongRun)
        )["LogRunTrack"]!!.jsonObject
        assertEquals("LongRun", track["workout_type"]!!.jsonPrimitive.content)
        val amend = obj(
            Event.AmendRun(9L, 5.0, 30.0, 0.0, 0.0, 1_700_000_000L, workoutType = WorkoutType.Recovery)
        )["AmendRun"]!!.jsonObject
        assertEquals("Recovery", amend["workout_type"]!!.jsonPrimitive.content)

        // Every enum name is the wire string the Rust side matches, incl. the
        // multi-word variant (a rename either side is a silent drop → pin it).
        assertEquals("Steady", WorkoutType.Steady.name)
        assertEquals("LongRun", WorkoutType.LongRun.name)
    }

    @Test
    fun runResultViewDecodesWorkoutTypeAndBackCompatOldShape() {
        val json = kotlinx.serialization.json.Json { ignoreUnknownKeys = true }
        // Back-compat: a pre-I16 run row (no workout_type key) still decodes → null.
        val old = json.decodeFromString<RunResultView>(
            """{"zone":"Z2","pace":"5:00/km","distance_km":10.0,"summary":"","citation":"",
                "gpx":"","observed_at":0}"""
        )
        assertEquals(null, old.workout_type)
        assertEquals(null, WorkoutType.fromWire(old.workout_type))

        // A tagged run row echoes the label, and fromWire maps it back to the enum.
        val tagged = json.decodeFromString<RunResultView>(
            """{"zone":"Z2","pace":"5:00/km","distance_km":10.0,"summary":"","citation":"",
                "gpx":"","observed_at":0,"workout_type":"Tempo"}"""
        )
        assertEquals("Tempo", tagged.workout_type)
        assertEquals(WorkoutType.Tempo, WorkoutType.fromWire(tagged.workout_type))

        // Decode-safe: an unknown/future variant string still decodes (raw String)
        // and maps to null rather than throwing: the run just renders untagged.
        val future = json.decodeFromString<RunResultView>(
            """{"zone":"Z2","pace":"5:00/km","distance_km":10.0,"summary":"","citation":"",
                "gpx":"","observed_at":0,"workout_type":"Fartlek"}"""
        )
        assertEquals("Fartlek", future.workout_type)
        assertEquals(null, WorkoutType.fromWire(future.workout_type))
    }

    // ── Phase 6 / B3: Coach-as-planner events ────────────────────────────────

    @Test
    fun generatePlanHasSnakeCaseStartDay() {
        val fields = obj(Event.GeneratePlan(20_300L))["GeneratePlan"]!!.jsonObject
        assertEquals(20_300L, fields["start_epoch_day"]!!.jsonPrimitive.content.toLong())
    }

    @Test
    fun setTodayHasSnakeCaseEpochDay() {
        val fields = obj(Event.SetToday(20_301L))["SetToday"]!!.jsonObject
        assertEquals(20_301L, fields["epoch_day"]!!.jsonPrimitive.content.toLong())
    }

    @Test
    fun clearPlanIsABareString() {
        assertEquals("\"ClearPlan\"", Event.ClearPlan.toJson().toString())
    }

    // ── Wave 2 / #6: structured HRmax / protein / spike-baseline fields ───────

    @Test
    fun oldShapeViewModelDecodesWithoutWave2Fields() {
        // Back-compat: a ViewModel blob from an OLD core (before hr_max,
        // protein_figures, runs[i].spike_has_baseline existed) must still decode,
        // with the new fields falling to their defaults (null / empty / false) -
        // no crash, so a stale replay or a mixed core/shell build stays safe.
        val json = kotlinx.serialization.json.Json { ignoreUnknownKeys = true }
        val vm = json.decodeFromString<ViewModel>(
            """{"safety_tier":null,"train_blocked":false,"adjustments":[],
                "review_adjustments":[],"input_count":1,
                "runs":[
                  {"zone":"Z2","pace":"5:30/km","distance_km":5.0,"spike_flag":true,
                   "spike_note":"First run logged - no prior run to gauge a spike against.",
                   "summary":"","citation":"","gpx":"","observed_at":4}],
                "guidance":[],"feedback":null,"reference":[],
                "protein_targets":[],"hr_zones":[]}"""
        )
        assertEquals(null, vm.hr_max)
        assertTrue(vm.protein_figures.isEmpty())
        // The run decodes and spike_has_baseline defaults to false (a first run
        // with no baseline), the shell reads this instead of scraping spike_note.
        assertFalse(vm.runs[0].spike_has_baseline)
        assertTrue(vm.runs[0].spike_flag)
    }

    @Test
    fun wave2ViewModelDecodesStructuredFields() {
        // Rust→Kotlin: the Wave 2 structured fields decode with the core's exact
        // serde names, replacing the deleted prose scrapes (hrMaxRegex,
        // proteinPerDayRegex, spike_note.contains).
        val json = kotlinx.serialization.json.Json { ignoreUnknownKeys = true }
        val vm = json.decodeFromString<ViewModel>(
            """{"safety_tier":null,"train_blocked":false,"adjustments":[],
                "review_adjustments":[],"input_count":1,
                "runs":[
                  {"zone":"Z2","pace":"5:30/km","distance_km":5.0,"spike_flag":true,
                   "spike_note":"","summary":"","citation":"","gpx":"",
                   "observed_at":4,"spike_has_baseline":true}],
                "guidance":[],"feedback":null,"reference":[],
                "protein_figures":[
                  {"kind":"masters","low_g_per_day":120.0,"high_g_per_day":140.0,"refused":false},
                  {"kind":"deficit","low_g_per_day":0.0,"high_g_per_day":0.0,"refused":true}],
                "hr_max":{"bpm":187.0,"measured":false,"age_years":30.0,
                   "tanaka_intercept":208.0,"tanaka_slope":0.7}}"""
        )
        val hm = vm.hr_max!!
        assertEquals(187, hm.bpm.toInt())
        assertFalse(hm.measured)
        assertEquals(30, hm.age_years.toInt())
        assertEquals(208.0, hm.tanaka_intercept, 1e-9)
        assertEquals(0.7, hm.tanaka_slope, 1e-9)
        // The tile reads the first non-refused figure as "120–140".
        val figure = vm.protein_figures.first { !it.refused }
        assertEquals("masters", figure.kind)
        assertEquals(120, figure.low_g_per_day.toInt())
        assertEquals(140, figure.high_g_per_day.toInt())
        assertTrue(vm.protein_figures.last().refused)
        assertTrue(vm.runs[0].spike_has_baseline)
    }
}
