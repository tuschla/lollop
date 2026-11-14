package app.milestone

import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

class SetEditorStateTest : PlatformTest() {

    // Exercise matching asks the real core: start from a fresh process, not an
    // engine another test class left installed.
    @BeforeTest
    fun setUp() = AppRuntime.resetForTest()

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

    /** The editor matches names through the core's `names::exercise_key` (its
     *  spelling rules are pinned in `names::tests`): the keys come back in order,
     *  and spellings the fold joins (a Turkish keyboard's "Inclıne", "Straße" /
     *  "STRASSE") match. */
    @Test
    fun nameMatchingAsksTheCore() {
        assertEquals(
            listOf("incline bench press", "strasse", "\uD801\uDC28 curl", "squat"),
            nameKeys(listOf("İNCLİNE BENCH PRESS", "Straße", "\uD801\uDC00 curl", "\u0085squat")),
        )
        assertTrue(sameExercise("Inclıne bench press", "incline bench press"))
        assertEquals(1, indexOfExercise(listOf("Back squat", "STRASSE"), "Straße"))
        assertEquals(-1, indexOfExercise(emptyList(), "Straße"))
        assertEquals(emptyList<String>(), nameKeys(emptyList()))
    }
}
