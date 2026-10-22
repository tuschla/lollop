package app.milestone

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The weight/distance keypad editors submit exactly what the prefill buffer
 * displays (the display-committed invariant), so a lossy prefill silently
 * rewrites an untouched Save. [fmtLosslessPrefill] must render the exact stored
 * value with trailing zeros trimmed, never rounded.
 */
class PrefillFormatTest {

    @Test
    fun wholeValuesDropTheDecimal() {
        assertEquals("100", fmtLosslessPrefill(100.0))
        assertEquals("10", fmtLosslessPrefill(10.0))
        assertEquals("0", fmtLosslessPrefill(0.0))
    }

    @Test
    fun fractionsArePreservedNotRounded() {
        // The old `%.1f` weight prefill rounded 100.25 → "100.3"; the old `%.2f`
        // distance prefill rounded 42.195 → "42.20". Both must now survive intact.
        assertEquals("100.25", fmtLosslessPrefill(100.25))
        assertEquals("42.195", fmtLosslessPrefill(42.195))
        assertEquals("50.25", fmtLosslessPrefill(50.25))
    }

    @Test
    fun trailingZerosAreTrimmed() {
        assertEquals("12.5", fmtLosslessPrefill(12.50))
        assertEquals("5.5", fmtLosslessPrefill(5.5))
    }

    @Test
    fun outputRoundTripsThroughTheBufferParser() {
        // The buffer is parsed back with `.toDoubleOrNull()` at submit; the prefill
        // must reparse to the exact stored double so an untouched Save is a no-op.
        for (v in listOf(100.0, 42.195, 12.5, 0.0, 7.25)) {
            assertEquals(v, fmtLosslessPrefill(v).toDouble(), 0.0)
        }
    }

    /**
     * No-fabrication: opening the set editor with neither an edited row nor a
     * coach prescription must seed EMPTY exercise/weight and leave reps/RPE unset,
     * so Save stays gated until the user actually enters a set (no phantom
     * "Back Squat 100 kg × 5 @ RPE 8" the user never chose).
     */
    @Test
    fun absentPrescriptionSeedsNothing() {
        val seed = freshSetSeed(initial = null, rx = null, weightUnit = WeightUnit.Kg)
        assertEquals("", seed.exercise)
        assertEquals("", seed.weightText)
        assertNull(seed.reps)
        assertNull(seed.rpe)
    }

    /**
     * A coach prescription DOES seed the editor: exercise, the display weight text
     * and the rep-band midpoint flow through so the prescribed set is one tap from
     * Save. Effort does NOT: it is the athlete's own per-set rating, so the target
     * stays a hint and an untouched Save logs the set unrated.
     */
    @Test
    fun prescriptionSeedsFieldsButNeverTheEffort() {
        val rx = SetPrefill(
            exercise = "Back Squat",
            weightText = "100.0",
            weightKgVerbatim = 100.0,
            reps = 5,
            holdSec = 20,
            targetRpe = 8.0,
        )
        val seed = freshSetSeed(initial = null, rx = rx, weightUnit = WeightUnit.Kg)
        assertEquals("Back Squat", seed.exercise)
        assertEquals("100.0", seed.weightText)
        assertEquals(5, seed.reps)
        assertNull(seed.rpe)
    }

    @Test
    fun anUnratedLoggedSetEditsAsUnrated() {
        // 0.0 on the wire means "not recorded"; editing must not show it as a rating.
        val seed = freshSetSeed(LiftResultView(exercise = "Curl", weight_kg = 12.0, reps = 12, rpe = 0.0), null, WeightUnit.Kg)
        assertNull(seed.rpe)
        val rated = freshSetSeed(LiftResultView(exercise = "Curl", weight_kg = 12.0, reps = 12, rpe = 8.5), null, WeightUnit.Kg)
        assertEquals(8.5, rated.rpe!!, 0.0)
    }

    @Test
    fun untouched_mi_prefill_round_trips_exact_km() {
        // A mi prefill the user never edits must re-emit the stored km verbatim: the
        // km→mi→km round-trip rounds to 2 dp (miToKmWire), which would drift 42.195.
        val stored = RunResultView(distance_km = 42.195)
        val displayedMi = kmToMi(stored.distance_km)
        assertEquals(
            42.195,
            runSaveDistanceKm(stored, distFresh = true, unit = DistanceUnit.Mi, distParsed = displayedMi),
            0.0,
        )
        // The drift the guard avoids: converting the displayed mi back rounds away
        // from the stored km, so an untouched Save must NOT take this path.
        assertTrue(miToKmWire(displayedMi) != 42.195)
    }
}
