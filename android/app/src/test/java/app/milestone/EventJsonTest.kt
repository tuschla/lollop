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
    }
}
