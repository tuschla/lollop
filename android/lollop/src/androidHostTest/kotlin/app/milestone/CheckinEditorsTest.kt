package app.milestone

import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.ZoneId

/**
 * The check-in editors never submit an answer the user didn't give: an earlier
 * day's morning check-in only hints, and a quiet week is sent only as the
 * explicit all-clear.
 */
class CheckinEditorsTest {

    private val utc = ZoneId.of("UTC")
    private val day = 86_400L

    @Test
    fun onlyTodaysEchoRehydratesTheMorningCheckin() {
        val now = 20_000 * day + 9 * 3_600L
        assertTrue(checkinEchoIsToday(20_000 * day + 6 * 3_600L, now, utc))
        // Yesterday evening's answers are a hint, never today's values.
        assertFalse(checkinEchoIsToday(20_000 * day - 3_600L, now, utc))
    }

    @Test
    fun theWeeklyAllClearStatesEveryAnswer() {
        val payload = (Event.SubmitWeeklyCheckin(weeklyAllClear(1_700_000_000L)).toJson() as JsonObject)
            .getValue("SubmitWeeklyCheckin").jsonObject
        assertEquals(0, payload.getValue("failed_key_sessions").jsonPrimitive.content.toInt())
        assertEquals(0, payload.getValue("rpe_load_gap_sessions").jsonPrimitive.content.toInt())
        val signs = payload.getValue("overtraining_signs").jsonObject
        assertTrue(signs.values.none { it.jsonPrimitive.boolean })
        assertFalse(payload.getValue("bone_pain_red_flag").jsonPrimitive.boolean)
        assertFalse(payload.getValue("compulsive_flag").jsonPrimitive.boolean)
        assertFalse(payload.containsKey("reds_signal_reported"))
        assertEquals(1_700_000_000L, payload.getValue("observed_at").jsonPrimitive.content.toLong())
    }
}
