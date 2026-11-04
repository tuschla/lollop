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

    /** A legacy `symptomatic` flag (no symptom bit) is a hold no clearance lifts
     *  (onboard-052): an unrelated answer keeps it, only re-answering a symptom
     *  question replaces it. */
    @Test
    fun legacySymptomHoldSurvivesAnUnrelatedFollowUpToggle() {
        val legacy = HealthScreen(symptomatic = true)

        val yes = legacy.withParqAnswer(ParqItem.HeartOrBloodPressure, true)
        assertTrue("a follow-up Yes is not an answer to Q2/Q3", yes.symptomatic)

        val backToNo = yes.withParqAnswer(ParqItem.HeartOrBloodPressure, false)
        assertTrue("the symptom hold is still raised", backToNo.symptomatic)
        assertTrue("the SetProfile still carries the health screen", backToNo.anyRaised())

        val chest = backToNo.withParqAnswer(ParqItem.ChestPain, true)
        assertTrue(chest.symptomatic)
        assertFalse("re-answering the symptom question replaces the legacy flag",
            chest.withParqAnswer(ParqItem.ChestPain, false).symptomatic)
    }

    /** A legacy doctor flag (`parq_positive`, no follow-up bit) stays until a
     *  follow-up question is answered, even when a legacy injury flag folded
     *  into Q6 makes the mask non-zero. */
    @Test
    fun legacyDoctorFlagSurvivesAnUnrelatedNoWithALegacyInjury() {
        val control = HealthScreen(parq_positive = true).withParqAnswer(ParqItem.ChestPain, false)
        assertTrue("control: without the injury flag the doctor flag stays", control.parq_positive)

        val withInjury = HealthScreen(parq_positive = true, injury_or_rehab = true)
            .withParqAnswer(ParqItem.ChestPain, false)
        assertTrue("the injury flag stays", withInjury.injury_or_rehab)
        assertTrue("the legacy doctor flag stays raised", withInjury.parq_positive)
        assertTrue("the legacy note still explains it", withInjury.legacyParq)

        val symptomYes = control.withParqAnswer(ParqItem.DizzyOrFainting, true)
        assertTrue("a symptom Yes is not a follow-up answer", symptomYes.parq_positive)

        val followUpNo = withInjury.withParqAnswer(ParqItem.Medication, false)
        assertFalse("answering a follow-up question replaces the legacy flag", followUpNo.parq_positive)
    }
}
