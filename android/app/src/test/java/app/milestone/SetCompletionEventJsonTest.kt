package app.milestone

import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Wire form for the completion events: SetExerciseEnded
 * and SetProfile's optional effective-from day. A drift here means the FFI silently
 * mis-reads them, so these pin the exact serde shape without the native library.
 */
class SetCompletionEventJsonTest {

    private fun obj(e: Event) = (e.toJson() as JsonObject)

    @Test
    fun setExerciseEndedHasSnakeCaseFields() {
        val fields = obj(
            Event.SetExerciseEnded("Back Squat", 20354L, ended = true, observedAt = 1_758_600_000L),
        )["SetExerciseEnded"]!!.jsonObject
        assertEquals("Back Squat", fields["exercise"]!!.jsonPrimitive.content)
        assertEquals(20354L, fields["epoch_day"]!!.jsonPrimitive.content.toLong())
        assertTrue(fields["ended"]!!.jsonPrimitive.content.toBoolean())
        assertEquals(1_758_600_000L, fields["observed_at"]!!.jsonPrimitive.content.toLong())
    }

    @Test
    fun setExerciseEndedReopenCarriesEndedFalse() {
        val fields = obj(
            Event.SetExerciseEnded("Bench", 20354L, ended = false, observedAt = 1L),
        )["SetExerciseEnded"]!!.jsonObject
        assertFalse(fields["ended"]!!.jsonPrimitive.content.toBoolean())
    }

    @Test
    fun setProfileEmitsEffectiveDayOnlyWhenSet() {
        val dated = obj(
            Event.SetProfile(
                ProgressionCadence.WeekToWeek, LiftGoal.MaxStrength, GoalDistance.TenK,
                ConcurrentGoal.Strength, 12, 4, 45.0, false,
                effectiveFromEpochDay = 20354L,
            ),
        )["SetProfile"]!!.jsonObject
        assertEquals(20354L, dated["effective_from_epoch_day"]!!.jsonPrimitive.content.toLong())

        // A null effective day keeps the pre-history wire (an old core replays it).
        val undated = obj(
            Event.SetProfile(
                ProgressionCadence.WeekToWeek, LiftGoal.MaxStrength, GoalDistance.TenK,
                ConcurrentGoal.Strength, 12, 4, 45.0, false,
            ),
        )["SetProfile"]!!.jsonObject
        assertFalse(undated.containsKey("effective_from_epoch_day"))
    }
}
