package app.milestone

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** The set loop's pure field reducers ([SetEditorState]). */
class SetEditorStateTest {

    private val noCue: (PrescriptionView) -> AppliedCue? = { null }

    private val typed = SetEditorState(
        exercise = "Bench",
        weightText = "61",
        weightFresh = false,
        weightVerbatim = null,
        reps = 3,
        holdSec = 20,
        rpe = 8.0,
    )

    @Test
    fun reseedFromAnOpenPlanItemLoadsItsPrescription() {
        val squat = PrescriptionView(exercise = "Back squat", reps_low = 5, reps_high = 5, load_kg = 100.0)
        val s = typed.reseededTo("back squat", listOf(squat), emptyList(), WeightUnit.Kg, noCue)
        assertEquals("Back squat", s.exercise)
        assertEquals("100", s.weightText)
        assertEquals(100.0, s.weightVerbatim!!, 0.0)
        assertEquals(5, s.reps)
        assertNull(s.rpe)
        assertEquals(true, s.weightFresh)
    }

    @Test
    fun reseedWithoutAPlanItemRepeatsTheLastLoggedSet() {
        val last = LiftResultView(exercise = "Curl", weight_kg = 12.5, reps = 10)
        val s = typed.reseededTo("curl", emptyList(), listOf(last), WeightUnit.Kg, noCue)
        assertEquals("Curl", s.exercise)
        assertEquals("12.5", s.weightText)
        assertEquals(10, s.reps)
    }

    @Test
    fun reseedOfAnUnknownNameOnlyRenames() {
        val s = typed.reseededTo("Sled push", emptyList(), emptyList(), WeightUnit.Kg, noCue)
        assertEquals(typed.copy(exercise = "Sled push", rpe = null, weightFresh = true), s)
    }

    @Test
    fun aLoggedSetKeepsTheLoadAndResetsRepsToTheTarget() {
        val item = PrescriptionView(exercise = "Bench", reps_low = 5, reps_high = 5)
        val s = typed.afterLoggedSet("Bench", 61.0, item, holdMode = false, WeightUnit.Kg)
        assertEquals(61.0, s.weightVerbatim!!, 0.0)
        assertEquals(true, s.weightFresh)
        assertEquals(5, s.reps)
        assertNull(s.rpe)
    }

    @Test
    fun theWorkingLoadReturnsAfterAWarmUpOnlyWhenPrescribed() {
        val planned = PrescriptionView(exercise = "Bench", reps_low = 5, reps_high = 5, load_kg = 80.0)
        assertEquals("80", typed.withWorkingLoad(planned, holdMode = false, WeightUnit.Kg, noCue).weightText)
        val unloaded = PrescriptionView(exercise = "Bench", reps_low = 5, reps_high = 5)
        assertEquals(typed, typed.withWorkingLoad(unloaded, holdMode = false, WeightUnit.Kg, noCue))
    }

    /** [nameKey] keys every row of the fixture the core's `names::exercise_key`
     *  test reads, so the shell and the core agree on which spellings are one
     *  exercise (a Turkish keyboard's "İncline", "Straße" / "STRASSE"). */
    @Test
    fun nameKeyMatchesTheCoresExerciseKey() {
        val rows = checkNotNull(javaClass.classLoader!!.getResourceAsStream("names/fold.tsv")) { "names/fold.tsv not on the test classpath" }
            .bufferedReader().readLines()
            .filter { it.isNotEmpty() && !it.startsWith("#") }
        assertTrue("${rows.size} rows", rows.size >= 10)
        for (row in rows) {
            val (name, key) = row.split('\t', limit = 2)
            assertEquals(name, key, nameKey(name))
        }
    }

    /** Spelled with escapes, as the fixture's rows are invisible: a supplementary
     *  letter folds per code point, and the trim strips U+0085 but keeps
     *  U+001C–U+001F, as the core's `str::trim`. */
    @Test
    fun nameKeyFoldsCodePointsAndTrimsLikeTheCore() {
        assertEquals("\uD801\uDC28 curl", nameKey("\uD801\uDC00 curl"))
        assertEquals("squat", nameKey("\u0085squat"))
        assertEquals("\u001Fsquat", nameKey("\u001Fsquat"))
    }
}
