package de.tuschla.fitnessanlage

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
        val fields = obj(Event.LogSet("Back Squat", 100.0, 5, 8.0))["LogSet"]!!.jsonObject
        assertEquals("Back Squat", fields["exercise"]!!.jsonPrimitive.content)
        assertEquals(100.0, fields["weight_kg"]!!.jsonPrimitive.content.toDouble(), 1e-9)
        assertEquals(5, fields["reps"]!!.jsonPrimitive.content.toInt())
        assertEquals(8.0, fields["rpe"]!!.jsonPrimitive.content.toDouble(), 1e-9)
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
    fun logRunTrackNestsPointsWithSnakeCaseFields() {
        val fields = obj(
            Event.LogRunTrack(listOf(GpsPoint(52.5, 13.4, 1000L, 4.5)), 80.0, 10.0)
        )["LogRunTrack"]!!.jsonObject
        val pt = fields["points"]!!.let { (it as kotlinx.serialization.json.JsonArray)[0] }.jsonObject
        assertEquals(52.5, pt["lat"]!!.jsonPrimitive.content.toDouble(), 1e-9)
        assertEquals(1000L, pt["observed_at"]!!.jsonPrimitive.content.toLong())
        assertEquals(4.5, pt["accuracy_m"]!!.jsonPrimitive.content.toDouble(), 1e-9)
        assertEquals(80.0, fields["hr_pct_max"]!!.jsonPrimitive.content.toDouble(), 1e-9)
    }
}
