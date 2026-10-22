package app.milestone

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

/** Pure Today decisions: the banner's way back per hold reason, one coaching
 *  card per exercise, the win that carries its next-time advice, the newcomer
 *  guide's reps-in-reserve and face line, and when the session review asks runs. */
class TodayLogicTest {

    @Test
    fun holdAction_followsTheHoldReasonBeforeTheTier() {
        // Profile illness and under-18 both arrive as a MedicalReferral tier; the
        // reason decides the way back, never "a professional's OK" for both.
        assertEquals(HoldAction.EndIllness, holdAction("acute_illness_profile", "MedicalReferral"))
        assertEquals(HoldAction.CheckAge, holdAction("youth", "MedicalReferral"))
        assertEquals(HoldAction.ReviewHealth, holdAction("parq", "MedicalReferral"))
        assertEquals(HoldAction.ReviewHealth, holdAction("pregnancy", "MedicalReferral"))
        // A reported illness removes only that report, not every readiness input.
        assertEquals(HoldAction.RemoveIllness, holdAction("illness", "Illness"))
        assertEquals(HoldAction.RemovePain, holdAction("pain", "Pain"))
        assertEquals(HoldAction.ClearReadiness, holdAction("medical_signal", "MedicalReferral"))
    }

    @Test
    fun holdAction_withoutAReasonFallsBackToTheTier() {
        assertEquals(HoldAction.RemovePain, holdAction(null, "Pain"))
        assertEquals(HoldAction.RemoveIllness, holdAction(null, "Illness"))
        assertEquals(HoldAction.ReviewHealth, holdAction(null, "MedicalReferral"))
        assertEquals(HoldAction.ClearReadiness, holdAction(null, "HrvTrend"))
    }

    @Test
    fun illnessResolution_endsTheReportAndTheProfileAnswerInOneConfirm() {
        // Owner 2026-10-17: "I'm better" left the profile's "I'm ill right now" on.
        val ill = ProfileView(age_years = 30.0, health = HealthScreen(acute_illness = true, parq_yes = 0b10))
        val both = HoldResolutionView(signal = "Illness", action_label = "I'm better", ends_profile_illness = true)
        val sent = holdResolutionEvents(both, ill, today = 20_740L, nowSec = 1_792_000_000L)
        assertEquals(Event.ResolveHold(ReadinessSignal.Illness, 1_792_000_000L), sent[0])
        val profile = sent[1] as Event.SetProfile
        assertFalse(profile.health.acute_illness)
        assertEquals("other health answers are kept", 0b10, profile.health.parq_yes)
        assertEquals(30.0, profile.ageYears)
        assertEquals("switched off from today only", 20_740L, profile.effectiveFromEpochDay)
        assertEquals(2, sent.size)
    }

    @Test
    fun illnessResolution_withoutTheProfileAnswerSendsOnlyTheResolve() {
        val reportOnly = HoldResolutionView(signal = "Illness", action_label = "I've recovered")
        assertEquals(
            listOf<Event>(Event.ResolveHold(ReadinessSignal.Illness, 5L)),
            holdResolutionEvents(reportOnly, ProfileView(), today = 1L, nowSec = 5L),
        )
        assertTrue(holdResolutionEvents(HoldResolutionView(signal = "Bogus"), null, 1L, 5L).isEmpty())
    }

    @Test
    fun holdProfileRow_landsOnTheAnswerThatHoldsTraining() {
        // Owner check 2026-10-17: a pregnancy hold landed on "Health questions ·
        // None apply" with the pregnancy switch off-screen below it.
        assertEquals(ProfileRowId.Pregnant, holdProfileRow("pregnancy", HealthScreen(pregnant = true)))
        assertEquals(
            ProfileRowId.PregnancyWarning,
            holdProfileRow("pregnancy", HealthScreen(pregnant = true, pregnancy_warning_sign = true)),
        )
        assertEquals(ProfileRowId.Age, holdProfileRow("youth", null))
        assertEquals(ProfileRowId.ParqSymptom, holdProfileRow("symptomatic", HealthScreen()))
        assertEquals(ProfileRowId.ParqInjury, holdProfileRow("injury_rehab", HealthScreen()))
        assertEquals(ProfileRowId.ParqClearance, holdProfileRow("parq", HealthScreen()))
        assertEquals(ProfileRowId.Reds, holdProfileRow("reds", HealthScreen(reds_signal = true)))
        assertEquals("a weekly-check-in RED-S answer has no profile switch", ProfileRowId.HealthQuestions, holdProfileRow("reds", HealthScreen()))
        assertEquals(ProfileRowId.Ill, holdProfileRow("acute_illness_profile", HealthScreen(acute_illness = true)))
        assertEquals(ProfileRowId.HealthQuestions, holdProfileRow(null, null))
    }

    @Test
    fun aHealthFocusOpensOnlyThePickerItsAnswerSitsIn() {
        assertEquals(ProfileRowId.HealthQuestions, ProfileRowId.ParqSymptom.pickerRow)
        assertEquals(ProfileRowId.HealthQuestions, ProfileRowId.ParqClearance.pickerRow)
        assertNull(ProfileRowId.Pregnant.pickerRow)
        assertNull(ProfileRowId.Reds.pickerRow)
        assertEquals(ProfileRowId.Age, ProfileRowId.Age.pickerRow)
        val chestPain = HealthScreen().withParqAnswer(ParqItem.ChestPain, true)
        assertEquals(ParqItem.ChestPain, parqRevealItem(ProfileRowId.ParqSymptom, chestPain))
        assertNull("a legacy flag has no question to land on", parqRevealItem(ProfileRowId.ParqSymptom, HealthScreen(symptomatic = true)))
        assertEquals(ParqItem.BoneOrJoint, parqRevealItem(ProfileRowId.ParqInjury, HealthScreen(injury_or_rehab = true)))
    }

    @Test
    fun bannerStatesHold_coversReportsTheProfileIllnessAPregnancyWarningAndTheYouthPause() {
        // The profile-ill and pregnancy-warning SAFETY cards repeated the banner's own line.
        assertTrue(bannerStatesHold("acute_illness_profile", "MedicalReferral", pregnancyWarning = false))
        assertTrue(bannerStatesHold("pain", "Pain", pregnancyWarning = false))
        assertTrue(bannerStatesHold("illness", "Illness", pregnancyWarning = false))
        assertTrue(bannerStatesHold("pregnancy", "MedicalReferral", pregnancyWarning = true))
        // The under-18 card already carries the hold's summary behind its "?".
        assertTrue(bannerStatesHold("youth", "MedicalReferral", pregnancyWarning = false))
        // A plain pregnancy's card carries the activity reference; the other
        // health answers' cards name the answer behind the generic banner line.
        assertFalse(bannerStatesHold("pregnancy", "MedicalReferral", pregnancyWarning = false))
        for (reason in listOf("parq", "symptomatic", "injury_rehab", "reds", "medical_signal")) {
            assertFalse(reason, bannerStatesHold(reason, "MedicalReferral", pregnancyWarning = false))
        }
    }

    @Test
    fun returnCheckAdjustmentKey_onlyForTheReturnCheckSession() {
        val adj = AdjustmentView(summary = "Return check after illness: …", citation = "Schwellnus 2022", safety_critical = true)
        val check = SessionPlanView(
            status = "adjusted",
            items = listOf(PrescriptionView(summary = "Return check: 10–20 min easy", return_check = true)),
            adjustment = adj,
        )
        assertEquals(adj.summary to adj.citation, returnCheckAdjustmentKey(check))
        assertNull(returnCheckAdjustmentKey(check.copy(items = listOf(PrescriptionView(summary = "Run: 20 min easy")))))
        assertNull(returnCheckAdjustmentKey(null))
    }

    @Test
    fun coachingPerExercise_keepsOneCardPerExerciseAndPrefersTheInSessionCue() {
        val nextTime = AdjustmentView(summary = "Add about 4% next time", exercise = "Back Squat")
        val inSession = AdjustmentView(summary = "Add about 8% now", exercise = "Back Squat", suggested_load_kg = 140.0)
        val bench = AdjustmentView(summary = "Hold the load", exercise = "Bench Press")
        val general = AdjustmentView(summary = "Sleep was short")
        val shown = coachingPerExercise(null, listOf(nextTime, inSession, bench, general))
        assertEquals(listOf(inSession, bench, general), shown)
    }

    @Test
    fun coachingPerExercise_dropsCardsForTheExerciseTheHeadlineAlreadyStates() {
        val squat = AdjustmentView(summary = "Add about 4% next time", exercise = "back squat")
        val bench = AdjustmentView(summary = "Hold the load", exercise = "Bench Press")
        assertEquals(listOf(bench), coachingPerExercise("Back Squat", listOf(squat, bench)))
    }

    @Test
    fun prescribedRir_isWholeAndBelowFourOnly() {
        assertEquals(3, prescribedRir(7.0))
        assertEquals(0, prescribedRir(10.0))
        // Half points and the open-ended "4+" are not stated as a plain number.
        assertNull(prescribedRir(7.5))
        assertNull(prescribedRir(6.0))
    }

    @Test
    fun winAdviceFor_pairsTheWinWithItsLiftsNextTimeNote() {
        val win = "New best on Back squat: 295 lb × 5."
        val cue = AdjustmentView(summary = "Deadlift: remaining sets drop 5%", exercise = "Deadlift", suggested_load_kg = 140.0)
        val nextTime = AdjustmentView(summary = "Back squat: add about 4% next time", exercise = "Back squat")
        assertSame(nextTime, winAdviceFor(win, listOf(cue, nextTime)))
        // An in-session cue for the same lift is not next-time advice.
        val squatCue = AdjustmentView(summary = "Squat: drop 5%", exercise = "Back squat", suggested_load_kg = 120.0)
        assertNull(winAdviceFor(win, listOf(squatCue)))
        // Advice for another lift stays its own card.
        assertNull(winAdviceFor(win, listOf(AdjustmentView(summary = "Add 2%", exercise = "Bench press"))))
    }

    @Test
    fun withoutEffortClause_dropsOnlyTheRepsLeftClause() {
        assertEquals("3×5 @ 60 kg", withoutEffortClause("3×5 @ 60 kg · ~3 reps left"))
        assertEquals("3×5 @ 60 kg · move fast", withoutEffortClause("3×5 @ 60 kg · 1–2 reps left · move fast"))
        assertEquals("3×5", withoutEffortClause("3×5 · to your last good rep"))
        assertEquals("3×5 @ 60 kg", withoutEffortClause("3×5 @ 60 kg"))
    }

    @Test
    fun reviewAsksRun_onlyOnADayWithARun() {
        val day = 20_000L
        val zone = java.time.ZoneOffset.UTC
        val lifts = SessionPlanView(epoch_day = day, session_type = "Lift(Strength)", items = listOf(PrescriptionView(exercise = "Back Squat")))
        val liftDay = ViewModel(today_epoch_day = day, week_plan = listOf(lifts))
        assertFalse("a lift-only day skips the run questions", reviewAsksRun(liftDay, zone))
        val ranToo = liftDay.copy(runs = listOf(RunResultView(observed_at = day * 86_400L + 3_600L)))
        assertTrue("a run logged today brings them back", reviewAsksRun(ranToo, zone))
        val runDay = ViewModel(
            today_epoch_day = day,
            week_plan = listOf(SessionPlanView(epoch_day = day, session_type = "Run(Easy)", items = listOf(PrescriptionView()))),
        )
        assertTrue("a planned run asks them", reviewAsksRun(runDay, zone))
    }
}
