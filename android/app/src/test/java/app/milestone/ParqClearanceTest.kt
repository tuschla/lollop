package app.milestone

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** A doctor's clearance covers only the PAR-Q+ follow-up answers it was given for
 *  (safety-044); a later new Yes needs a new clearance. */
class ParqClearanceTest {

    @Test
    fun newFollowUpYesAfterClearanceDropsTheClearance() {
        val cleared = HealthScreen()
            .withParqAnswer(ParqItem.HeartOrBloodPressure, true)
            .withClearance(true)
        assertTrue(cleared.medically_cleared)
        assertEquals(1, cleared.cleared_parq_mask)

        val added = cleared.withParqAnswer(ParqItem.Medication, true)
        assertTrue(added.parq_positive)
        assertFalse(added.medically_cleared)
        assertEquals(0, added.cleared_parq_mask)
    }

    @Test
    fun symptomOrRemovedAnswerKeepsTheClearanceForCoveredAnswers() {
        val cleared = HealthScreen()
            .withParqAnswer(ParqItem.HeartOrBloodPressure, true)
            .withParqAnswer(ParqItem.Medication, true)
            .withClearance(true)
        val fewer = cleared.withParqAnswer(ParqItem.Medication, false)
        assertTrue(fewer.medically_cleared)
        assertEquals(1, fewer.cleared_parq_mask)

        val symptom = fewer.withParqAnswer(ParqItem.ChestPain, true)
        assertTrue(symptom.medically_cleared)
        assertTrue(symptom.symptomatic)
    }

    @Test
    fun clearanceSavedBeforeTheMaskStillDropsOnANewAnswer() {
        val old = HealthScreen()
            .withParqAnswer(ParqItem.OtherCondition, true)
            .copy(medically_cleared = true)
        assertEquals(0, old.cleared_parq_mask)
        assertFalse(old.withParqAnswer(ParqItem.SupervisedOnly, true).medically_cleared)
        assertTrue(old.withParqAnswer(ParqItem.ChestPain, true).medically_cleared)
    }
}
