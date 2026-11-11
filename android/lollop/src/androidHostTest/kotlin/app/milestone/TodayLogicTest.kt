package app.milestone

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

/** Pure Today decisions: the banner's way back per hold reason, one coaching
 *  card per exercise, the win that carries its next-time advice, the newcomer
 *  guide's reps-in-reserve and face line, when the session review asks runs,
 *  and the plan face's plate figure, run head and display-unit distances. */
class TodayLogicTest {

    @Test
    fun holdAction_followsTheHoldReasonBeforeTheTier() {
        // Profile illness and under-18 both arrive as a MedicalReferral tier; the
        // reason decides the way back, never "a doctor's OK" for both.
        assertEquals(HoldAction.EndIllness, holdAction(HoldReason.AcuteIllnessProfile, SafetyTier.MedicalReferral))
        assertEquals(HoldAction.CheckAge, holdAction(HoldReason.Youth, SafetyTier.MedicalReferral))
        assertEquals(HoldAction.ReviewHealth, holdAction(HoldReason.Parq, SafetyTier.MedicalReferral))
        assertEquals(HoldAction.ReviewHealth, holdAction(HoldReason.Pregnancy, SafetyTier.MedicalReferral))
        // A reported illness removes only that report, not every readiness input.
        assertEquals(HoldAction.RemoveIllness, holdAction(HoldReason.Illness, SafetyTier.Illness))
        assertEquals(HoldAction.RemovePain, holdAction(HoldReason.Pain, SafetyTier.Pain))
        assertEquals(HoldAction.ClearReadiness, holdAction(HoldReason.MedicalSignal, SafetyTier.MedicalReferral))
    }

    @Test
    fun holdAction_withoutAReasonFallsBackToTheTier() {
        assertEquals(HoldAction.RemovePain, holdAction(null, SafetyTier.Pain))
        assertEquals(HoldAction.RemoveIllness, holdAction(null, SafetyTier.Illness))
        assertEquals(HoldAction.ReviewHealth, holdAction(null, SafetyTier.MedicalReferral))
        assertEquals(HoldAction.ClearReadiness, holdAction(null, SafetyTier.HrvTrend))
    }

    @Test
    fun painSubline_fallbackUsesTheTriageWordsAndMatchesTheCore() {
        val noCore = ViewModel()
        // The triage choices' words, not "sharp/joint" / "tendon-like".
        assertEquals(
            "Left knee · sharp or joint pain · 6/10",
            painSubline(noCore, PainDetail(PainKind.SharpJoint, severity = 6, trend = PainTrend.Stable, location = "Left knee")),
        )
        assertEquals("dull ache · 3/10", painSubline(noCore, PainDetail(PainKind.TendonLoadRelated, severity = 3, trend = PainTrend.Stable)))
        // "Not sure" adds nothing to the line (never a stray "not sure" fragment).
        assertEquals("Left knee · 4/10", painSubline(noCore, PainDetail(PainKind.Other, severity = 4, trend = PainTrend.Stable, location = "Left knee")))
        assertNull(painSubline(noCore, PainDetail(PainKind.Other, trend = PainTrend.Stable)))
        // Worse the morning after reads "worsening" too, as the core's pain_context does.
        assertEquals(
            "muscle soreness · worsening",
            painSubline(noCore, PainDetail(PainKind.Doms, trend = PainTrend.WorseningAfter24h)),
        )
    }

    @Test
    fun illnessResolution_endsTheReportAndTheProfileAnswerInOneConfirm() {
        // Owner 2026-10-17: "I'm better" left the profile's "I'm ill right now" on.
        val ill = Profile(age_years = 30.0, health = HealthScreen(acute_illness = true, parq_yes = 0b10))
        val both = HoldResolutionView(signal = ReadinessSignal.Illness, action_label = "I'm better", ends_profile_illness = true)
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
        val reportOnly = HoldResolutionView(signal = ReadinessSignal.Illness, action_label = "I've recovered")
        assertEquals(
            listOf<Event>(Event.ResolveHold(ReadinessSignal.Illness, 5L)),
            holdResolutionEvents(reportOnly, Profile(), today = 1L, nowSec = 5L),
        )
    }

    @Test
    fun holdProfileRow_landsOnTheAnswerThatHoldsTraining() {
        // Owner check 2026-10-17: a pregnancy hold landed on "Health questions ·
        // None apply" with the pregnancy switch off-screen below it.
        assertEquals(ProfileRowId.Pregnant, holdProfileRow(HoldReason.Pregnancy, HealthScreen(pregnant = true)))
        assertEquals(
            ProfileRowId.PregnancyWarning,
            holdProfileRow(HoldReason.Pregnancy, HealthScreen(pregnant = true, pregnancy_warning_sign = true)),
        )
        assertEquals(ProfileRowId.Age, holdProfileRow(HoldReason.Youth, null))
        assertEquals(ProfileRowId.ParqSymptom, holdProfileRow(HoldReason.Symptomatic, HealthScreen()))
        assertEquals(ProfileRowId.ParqInjury, holdProfileRow(HoldReason.InjuryRehab, HealthScreen()))
        assertEquals(ProfileRowId.ParqClearance, holdProfileRow(HoldReason.Parq, HealthScreen()))
        assertEquals(ProfileRowId.Reds, holdProfileRow(HoldReason.Reds, HealthScreen(reds_signal = true)))
        assertEquals("a weekly-check-in RED-S answer has no profile switch", ProfileRowId.HealthQuestions, holdProfileRow(HoldReason.Reds, HealthScreen()))
        assertEquals(ProfileRowId.Ill, holdProfileRow(HoldReason.AcuteIllnessProfile, HealthScreen(acute_illness = true)))
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
        assertTrue(bannerStatesHold(HoldReason.AcuteIllnessProfile, SafetyTier.MedicalReferral, pregnancyWarning = false))
        assertTrue(bannerStatesHold(HoldReason.Pain, SafetyTier.Pain, pregnancyWarning = false))
        assertTrue(bannerStatesHold(HoldReason.Illness, SafetyTier.Illness, pregnancyWarning = false))
        assertTrue(bannerStatesHold(HoldReason.Pregnancy, SafetyTier.MedicalReferral, pregnancyWarning = true))
        // The under-18 card already carries the hold's summary behind its "?".
        assertTrue(bannerStatesHold(HoldReason.Youth, SafetyTier.MedicalReferral, pregnancyWarning = false))
        // A plain pregnancy's card carries the activity reference; the other
        // health answers' cards name the answer behind the generic banner line.
        assertFalse(bannerStatesHold(HoldReason.Pregnancy, SafetyTier.MedicalReferral, pregnancyWarning = false))
        for (reason in listOf(HoldReason.Parq, HoldReason.Symptomatic, HoldReason.InjuryRehab, HoldReason.Reds, HoldReason.MedicalSignal)) {
            assertFalse(reason.wire, bannerStatesHold(reason, SafetyTier.MedicalReferral, pregnancyWarning = false))
        }
    }

    @Test
    fun returnCheckAdjustmentKey_onlyForTheReturnCheckSession() {
        val adj = AdjustmentView(summary = "Return check after illness: …", citation = "Schwellnus 2022", safety_critical = true)
        val check = SessionPlanView(
            status = SessionStatus.Adjusted,
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
        val zone = kotlinx.datetime.TimeZone.UTC
        val lifts = SessionPlanView(epoch_day = day, session_type = SessionType.LiftMaxEffort, items = listOf(PrescriptionView(exercise = "Back Squat")))
        val liftDay = ViewModel(today_epoch_day = day, week_plan = listOf(lifts))
        assertFalse("a lift-only day skips the run questions", reviewAsksRun(liftDay, zone))
        val ranToo = liftDay.copy(runs = listOf(RunResultView(observed_at = day * 86_400L + 3_600L)))
        assertTrue("a run logged today brings them back", reviewAsksRun(ranToo, zone))
        val runDay = ViewModel(
            today_epoch_day = day,
            week_plan = listOf(SessionPlanView(epoch_day = day, session_type = SessionType.RunEasy, items = listOf(PrescriptionView()))),
        )
        assertTrue("a planned run asks them", reviewAsksRun(runDay, zone))
    }

    private fun squat(day: java.time.LocalDate?, e1rm: Double) = LiftResultView(
        exercise = "Back squat",
        e1rm_kg = e1rm,
        observed_at = day?.atTime(18, 0)?.atZone(java.time.ZoneId.systemDefault())?.toEpochSecond() ?: 0L,
    )

    @Test
    fun e1rmTrend_countsOnlyDatedSessions() {
        val d = java.time.LocalDate.of(2026, 10, 1)
        assertNull(
            "an undated set is no session: two dated ones are not a trend",
            e1rmTrend(ViewModel(lifts = listOf(squat(null, 90.0), squat(d, 100.0), squat(d.plusDays(3), 102.0)))),
        )
        val trend = e1rmTrend(
            ViewModel(lifts = listOf(squat(null, 90.0), squat(d, 95.0), squat(d.plusDays(2), 100.0), squat(d.plusDays(4), 102.0))),
        )!!
        assertEquals(listOf(95.0, 100.0, 102.0), trend.series)
        assertEquals(kotlinx.datetime.LocalDate(2026, 10, 1), trend.firstDay)
    }

    @Test
    fun todayCards_aDoneSessionSpendsItsCoachingButKeepsTheSafetyCall() {
        val cue = AdjustmentView(summary = "Stop a rep earlier", grade = EvidenceGrade.Moderate, citation = "c1", exercise = "Back squat")
        val safety = AdjustmentView(summary = "Head cold: fewer sets", grade = EvidenceGrade.Moderate, citation = "c2", safety_critical = true)
        val underWay = ViewModel(today_epoch_day = 100, adjustments = listOf(cue, safety))
        val open = todayCards(underWay, hasSignal = true, DistanceUnit.Km)
        assertSame(safety, open.hostAdjustment)
        assertEquals(listOf(cue), open.coachingCards)
        assertTrue("both are cards above the session", ("Stop a rep earlier" to "c1") in open.renderedKeys)
        val done = underWay.copy(
            session_done = SessionDoneView(epoch_day = 100, items = listOf(SessionDoneItemView(exercise = "Back squat"))),
        )
        val after = todayCards(done, hasSignal = true, DistanceUnit.Km)
        assertSame("a safety call is never spent", safety, after.hostAdjustment)
        assertTrue("the session's coaching is spent once it is done", after.coachingCards.isEmpty())
    }

    @Test
    fun plateFigureHelper() {
        assertEquals("20", plateFigure(20.0))
        assertEquals("2.5", plateFigure(2.5))
        assertEquals("1.25", plateFigure(1.25))
    }

    /** A run face leads with the core's volume head only when the item carries a
     *  run target, whose pace / heart-rate / effort lines then render under it. */
    @Test
    fun runFaceHeadIsTheCoreVolumeHeadUnderATarget() {
        val item = coreJson.decodeFromString(
            PrescriptionView.serializer(),
            """
            {"summary":"6 × 800 m · 3:40–3:50 /km (interval) · effort 8 of 10","run_head":"6 × 800 m","face_lead":"6 × 800 m",
             "run_target":{"band":"interval","pace_label_km":"3:40–3:50 /km","effort_cue":"effort 8 of 10","effort_detail":"8 of 10"}}
            """.trimIndent(),
        )
        assertEquals("6 × 800 m", runFaceHead(item, DistanceUnit.Km))
        // A rep head has no plain distance: it reads the same in miles.
        assertEquals("6 × 800 m", runFaceHead(item, DistanceUnit.Mi))
        assertEquals("8 of 10", item.run_target?.effort_detail)
        // No target, or no volume head (a pain stop): the full summary stays.
        assertEquals(null, runFaceHead(item.copy(run_target = null), DistanceUnit.Km))
        assertEquals(null, runFaceHead(item.copy(run_head = ""), DistanceUnit.Km))
    }

    /** A distance head and distance prose read in the display unit from the core's
     *  structured km figure and its miles twins, never by rewriting the km text. */
    @Test
    fun distancesFollowTheDisplayUnit() {
        val item = PrescriptionView(
            run_head = "8 km", run_head_km = 8.0, run_target = RunTargetView(band = "easy"),
            safety_reason = "Capped at 8 km: at most 10% over your longest recent run",
            safety_reason_mi = "Capped at 5 mi: at most 10% over your longest recent run",
        )
        assertEquals("8 km", runFaceHead(item, DistanceUnit.Km))
        assertEquals("5 mi", runFaceHead(item, DistanceUnit.Mi))
        assertEquals(item.safety_reason, safetyReasonText(item, DistanceUnit.Km))
        assertEquals(item.safety_reason_mi, safetyReasonText(item, DistanceUnit.Mi))
        // A reason stating no distance has no twin: the km text stands.
        val kept = item.copy(safety_reason = "Kept short and easy", safety_reason_mi = "")
        assertEquals("Kept short and easy", safetyReasonText(kept, DistanceUnit.Mi))
        assertEquals(null, safetyReasonText(PrescriptionView(), DistanceUnit.Mi))
        val fb = FeedbackView(message = "Run done: 10.2 km in 47 min, as planned.", message_mi = "Run done: 6.3 mi in 47 min, as planned.")
        assertEquals(fb.message, feedbackMessage(fb, DistanceUnit.Km))
        assertEquals(fb.message_mi, feedbackMessage(fb, DistanceUnit.Mi))
        assertEquals("Even split.", feedbackMessage(FeedbackView(message = "Even split."), DistanceUnit.Mi))
    }
}
