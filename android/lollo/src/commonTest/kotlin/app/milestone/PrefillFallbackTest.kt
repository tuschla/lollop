package app.milestone

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

/** Where a NEW set's weight comes from when the plan item carries no logged set
 *  today: the prescribed load, then the core's suggestion, then the last set; and
 *  the effort target stays a hint that a ballistic item never gets. */
class PrefillFallbackTest {

    private fun rx(p: PrescriptionView) = prescriptionPrefill(p, WeightUnit.Kg)

    @Test
    fun thePrefillFallsBackFromPrescribedToSuggestedToLastSet() {
        val last = ItemSetView(weight_kg = 12.0, reps = 12, rpe = 8.0, summary = "12 kg × 12 @ RPE 8")
        // An isolation lift with only a prior set starts from that set's weight.
        assertEquals(12.0, rx(PrescriptionView(exercise = "Lateral raise", last_set = last)).weightKgVerbatim!!, 0.0)
        // The core's suggestion wins over the raw last set, a prescribed load over both.
        assertEquals(
            14.0,
            rx(PrescriptionView(exercise = "Lateral raise", last_set = last, suggested_load_kg = 14.0)).weightKgVerbatim!!,
            0.0,
        )
        assertEquals(
            100.0,
            rx(PrescriptionView(exercise = "Squat", load_kg = 100.0, suggested_load_kg = 14.0, last_set = last)).weightKgVerbatim!!,
            0.0,
        )
        // The effort target stays a hint, and a ballistic item carries none.
        val jump = PrescriptionView(exercise = "Box jump", target_rpe = 8.0, calis = CalisItemView(ballistic = true))
        assertNull(rx(jump).targetRpe)
    }
}
