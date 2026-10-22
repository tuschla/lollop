package app.milestone

import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * Pins the AmendRunMeta wire form (model.rs Event::AmendRunMeta): both fields are
 * always present because the core REPLACES them, so a null must travel as an
 * explicit JSON null (clearing the rating / tag), never be dropped.
 */
class AmendRunMetaJsonTest {

    private fun fields(e: Event) = (e.toJson() as JsonObject)["AmendRunMeta"]!!.jsonObject

    @Test
    fun ratingAndTagUseSnakeCaseWireNames() {
        val f = fields(Event.AmendRunMeta(42L, 1_700_000_000L, 7, WorkoutType.LongRun))
        assertEquals(42L, f["entry_id"]!!.jsonPrimitive.content.toLong())
        assertEquals(1_700_000_000L, f["observed_at_fallback"]!!.jsonPrimitive.content.toLong())
        assertEquals(7, f["session_rpe"]!!.jsonPrimitive.content.toInt())
        assertEquals("LongRun", f["workout_type"]!!.jsonPrimitive.content)
    }

    @Test
    fun nullClearsTravelAsExplicitJsonNull() {
        val f = fields(Event.AmendRunMeta(42L, 0L, null, null))
        assertEquals(JsonNull, f["session_rpe"])
        assertEquals(JsonNull, f["workout_type"])
    }
}
