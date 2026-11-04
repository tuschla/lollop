package app.milestone

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * [prescriptionPrefill] turns a coach [PrescriptionView] into a NEW-entry
 * [SetPrefill] the set editor seeds from. The display text is kg without
 * trailing zeros or lb to the half pound, while the exact wire kg rides along in `weightKgVerbatim` for a
 * lossless untouched Save; an RIR-prescribed lift with no load invents none
 * (HARD RULE 1). Reps and hold seconds take the band midpoint (rounded).
 */
class PrescriptionPrefillTest {

    @Test
    fun anchoredLiftSeedsLoadRepsAndNoFabricatedRpe() {
        val p = PrescriptionView(
            exercise = "Back Squat",
            load_kg = 87.5,
            reps_low = 3,
            reps_high = 3,
            target_rpe = null,
        )
        val rx = prescriptionPrefill(p, WeightUnit.Kg)
        assertEquals("Back Squat", rx.exercise)
        assertEquals("87.5", rx.weightText)
        assertEquals(87.5, rx.weightKgVerbatim!!, 0.0)
        assertEquals(3, rx.reps)
        // No target_rpe on the wire → no effort hint either.
        assertNull(rx.targetRpe)
    }

    @Test
    fun rirLiftInventsNoLoadAndCarriesTheTargetAsAHint() {
        val p = PrescriptionView(
            exercise = "Bench",
            load_kg = null,
            reps_low = 5,
            reps_high = 5,
            target_rpe = 7.0,
        )
        val rx = prescriptionPrefill(p, WeightUnit.Kg)
        assertNull("an RIR lift has no load to prefill", rx.weightText)
        assertNull(rx.weightKgVerbatim)
        // The target rides along as a HINT; the seeded editor value stays unrated.
        assertEquals(7.0, rx.targetRpe!!, 0.0)
        assertNull(freshSetSeed(initial = null, rx = rx, weightUnit = WeightUnit.Kg).rpe)
    }

    @Test
    fun timedHoldSeedsHoldMidpointAndBodyweightAddedLoad() {
        val p = PrescriptionView(
            exercise = "Front plank",
            timed_hold = true,
            hold_low_sec = 5,
            hold_high_sec = 15,
            added_kg = 0.0,
            reps_low = 0,
        )
        val rx = prescriptionPrefill(p, WeightUnit.Kg)
        assertEquals(10, rx.holdSec) // band midpoint of 5–15
        // Bodyweight added load renders "0", never null and never "×0".
        assertEquals("0", rx.weightText)
        assertEquals(0.0, rx.weightKgVerbatim!!, 0.0)
    }

    @Test
    fun weightedDynamicCalSeedsAddedLoadOverBandMidpointReps() {
        val p = PrescriptionView(
            exercise = "Full pull-up",
            added_kg = 10.0,
            reps_low = 5,
            reps_high = 8,
            target_rpe = 8.0,
        )
        val rx = prescriptionPrefill(p, WeightUnit.Kg)
        assertEquals("10", rx.weightText)
        assertEquals(10.0, rx.weightKgVerbatim!!, 0.0)
        assertEquals(7, rx.reps) // band midpoint of 5–8, rounded (6.5 → 7)
        assertEquals(8.0, rx.targetRpe!!, 0.0)
    }

    @Test
    fun lbUnitConvertsDisplayButKeepsTheWireKgVerbatim() {
        val p = PrescriptionView(exercise = "Deadlift", load_kg = 100.0)
        val rx = prescriptionPrefill(p, WeightUnit.Lb)
        // Display converts kg→lb to the half pound (220.46 → 220.5); the exact wire kg stays 100.0.
        assertEquals("220.5", rx.weightText)
        assertEquals(100.0, rx.weightKgVerbatim!!, 0.0)
    }

    @Test
    fun kgPrefillKeepsAQuarterKiloLoad() {
        // A 101.25 kg load is loadable; the field must not round it to 101.3.
        val rx = prescriptionPrefill(PrescriptionView(exercise = "Bench", load_kg = 101.25), WeightUnit.Kg)
        assertEquals("101.25", rx.weightText)
    }

    @Test
    fun aReopenedLoopKeepsTodaysLoadOverThePlan() {
        val warmUp = ItemSetView(entry_id = 1, observed_at = 100, weight_kg = 60.0, reps = 5, qualifies = false)
        val work = ItemSetView(entry_id = 2, observed_at = 200, weight_kg = 136.0, reps = 5, qualifies = true)
        val p = PrescriptionView(exercise = "Back Squat", reps_low = 5, reps_high = 5, load_kg = 125.0, logged_sets = listOf(warmUp, work))
        assertEquals(136.0, prescriptionPrefill(p, WeightUnit.Kg).weightKgVerbatim!!, 0.0)
        // Reps stay the plan's target.
        assertEquals(5, prescriptionPrefill(p, WeightUnit.Kg).reps)
        // A cue load applied after that set wins; one applied before it does not.
        assertEquals(140.0, prescriptionPrefill(p, WeightUnit.Kg, AppliedCue(140.0, 300)).weightKgVerbatim!!, 0.0)
        assertEquals(136.0, prescriptionPrefill(p, WeightUnit.Kg, AppliedCue(140.0, 150)).weightKgVerbatim!!, 0.0)
        // Warm-ups alone never replace the plan's load.
        assertEquals(125.0, prescriptionPrefill(p.copy(logged_sets = listOf(warmUp)), WeightUnit.Kg).weightKgVerbatim!!, 0.0)
    }
}
