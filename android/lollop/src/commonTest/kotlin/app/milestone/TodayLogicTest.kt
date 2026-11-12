package app.milestone

import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertSame
import kotlin.test.assertTrue
import kotlinx.datetime.DateTimeUnit
import kotlinx.datetime.LocalDate
import kotlinx.datetime.TimeZone
import kotlinx.datetime.atTime
import kotlinx.datetime.plus
import kotlinx.datetime.toInstant

/** Today decisions: the banner's way back per hold reason, one coaching
 *  card per exercise, the win that carries its next-time advice, the newcomer
 *  guide's reps-in-reserve and face line, when the session review asks runs,
 *  and the plan face's plate figure, run head and display-unit distances. */
class TodayLogicTest : PlatformTest() {

    // Exercise matching and draft facts ask the real core: start from a fresh
    // process, not an engine another test class left installed.
    @BeforeTest
    fun setUp() = AppRuntime.resetForTest()

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
    fun illnessResolution_endsTheReportAndTheProfileAnswerInOneConfirm() {
        // Owner 2026-10-17: "I'm better" left the profile's "I'm ill right now" on.
        val ill = Profile(age_years = 30.0, health = HealthScreen(acute_illness = true, parq_yes = 0b10))
        val both = HoldResolutionView(signal = ReadinessSignal.Illness, action_label = "I'm better", ends_profile_illness = true)
        val sent = holdResolutionEvents(both, ill, today = 20_740L, nowSec = 1_792_000_000L)
        assertEquals(Event.ResolveHold(ReadinessSignal.Illness, 1_792_000_000L), sent[0])
        val profile = sent[1] as Event.SetProfile
        assertFalse(profile.health.acute_illness)
        assertEquals(0b10, profile.health.parq_yes, "other health answers are kept")
        assertEquals(30.0, profile.ageYears)
        assertEquals(20_740L, profile.effectiveFromEpochDay, "switched off from today only")
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
        assertEquals(ProfileRowId.HealthQuestions, holdProfileRow(HoldReason.Reds, HealthScreen()), "a weekly-check-in RED-S answer has no profile switch")
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
        val chestPain = draftFacts(ProfileDraft(answer = HealthAnswer.Parq(ParqQuestion.ChestPain, true))).health
        assertEquals(ParqQuestion.ChestPain, parqRevealItem(ProfileRowId.ParqSymptom, chestPain))
        assertNull(parqRevealItem(ProfileRowId.ParqSymptom, HealthScreen(symptomatic = true)), "a legacy flag has no question to land on")
        assertEquals(ParqQuestion.BoneOrJoint, parqRevealItem(ProfileRowId.ParqInjury, HealthScreen(injury_or_rehab = true)))
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
            assertFalse(bannerStatesHold(reason, SafetyTier.MedicalReferral, pregnancyWarning = false), reason.wire)
        }
    }

    @Test
    fun aReturnCheckSessionOwnsItsAdjustment() {
        val adj = AdjustmentView(summary = "Return check after illness: …", citation = "Schwellnus 2022", safety_critical = true)
        val check = SessionPlanView(
            status = SessionStatus.Adjusted,
            items = listOf(PrescriptionView(summary = "Return check: 10–20 min easy", return_check = true)),
            adjustment = adj,
            adjustment_on_item = true,
        )
        val m = ViewModel(today_epoch_day = 100, next_session = check, adjustments = listOf(adj.copy(repeats_session = true)))
        val c = todayCards(m, hasSignal = true)
        assertNull(c.hostAdjustment, "the item states it: no card above")
        assertTrue(c.sessionAdjustmentAbove)
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
    fun reviewAsksRun_onlyOnADayWithARun() {
        val day = 20_000L
        val zone = kotlinx.datetime.TimeZone.UTC
        val lifts = SessionPlanView(epoch_day = day, session_type = SessionType.LiftMaxEffort, items = listOf(PrescriptionView(exercise = "Back Squat")))
        val liftDay = ViewModel(today_epoch_day = day, week_plan = listOf(lifts))
        assertFalse(reviewAsksRun(liftDay, zone), "a lift-only day skips the run questions")
        val ranToo = liftDay.copy(runs = listOf(RunResultView(observed_at = day * 86_400L + 3_600L)))
        assertTrue(reviewAsksRun(ranToo, zone), "a run logged today brings them back")
        val runDay = ViewModel(
            today_epoch_day = day,
            week_plan = listOf(SessionPlanView(epoch_day = day, session_type = SessionType.RunEasy, items = listOf(PrescriptionView()))),
        )
        assertTrue(reviewAsksRun(runDay, zone), "a planned run asks them")
    }

    /** A set at 18:00 on [day] in the zone [e1rmTrend] dates sets in (the device's),
     *  so the trend's days are the ones written here whatever zone the run uses. */
    private fun squat(day: LocalDate?, e1rm: Double) = LiftResultView(
        exercise = "Back squat",
        e1rm_kg = e1rm,
        observed_at = day?.atTime(18, 0)?.toInstant(TimeZone.currentSystemDefault())?.epochSeconds ?: 0L,
    )

    @Test
    fun e1rmTrend_countsOnlyDatedSessions() {
        val d = LocalDate(2026, 10, 1)
        fun plus(days: Int) = d.plus(days, DateTimeUnit.DAY)
        assertNull(
            e1rmTrend(ViewModel(lifts = listOf(squat(null, 90.0), squat(d, 100.0), squat(plus(3), 102.0)))),
            "an undated set is no session: two dated ones are not a trend",
        )
        val trend = e1rmTrend(
            ViewModel(lifts = listOf(squat(null, 90.0), squat(d, 95.0), squat(plus(2), 100.0), squat(plus(4), 102.0))),
        )!!
        assertEquals(listOf(95.0, 100.0, 102.0), trend.series)
        assertEquals(d, trend.firstDay)
    }

    @Test
    fun todayCards_aDoneSessionSpendsItsCoachingButKeepsTheSafetyCall() {
        val cue = AdjustmentView(summary = "Stop a rep earlier", grade = EvidenceGrade.Moderate, citation = "c1", exercise = "Back squat")
        val safety = AdjustmentView(summary = "Head cold: fewer sets", grade = EvidenceGrade.Moderate, citation = "c2", safety_critical = true)
        val underWay = ViewModel(today_epoch_day = 100, adjustments = listOf(cue, safety))
        val open = todayCards(underWay, hasSignal = true)
        assertSame(safety, open.hostAdjustment)
        assertEquals(listOf(cue), open.coachingCards)
        assertFalse(open.sessionAdjustmentAbove, "nothing above repeats the session's adjustment")
        val repeated = underWay.copy(adjustments = listOf(cue.copy(repeats_session = true), safety))
        assertTrue(todayCards(repeated, hasSignal = true).sessionAdjustmentAbove, "a card above states it")
        val done = underWay.copy(
            session_done = SessionDoneView(epoch_day = 100, items = listOf(SessionDoneItemView(exercise = "Back squat"))),
        )
        val after = todayCards(done, hasSignal = true)
        assertSame(safety, after.hostAdjustment, "a safety call is never spent")
        assertTrue(after.coachingCards.isEmpty(), "the session's coaching is spent once it is done")
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
        // No target, or no volume head (a pain stop): the full summary stays.
        assertEquals(null, runFaceHead(item.copy(run_target = null), DistanceUnit.Km))
        assertEquals(null, runFaceHead(item.copy(run_head = ""), DistanceUnit.Km))
    }

    /** A distance head reads in the display unit from the core's structured km
     *  figure, never by rewriting the km text. The safety reason and feedback are
     *  rendered in the profile's unit by the core (Rust tests
     *  `a_logged_shakeout_keeps_the_stated_long_run`,
     *  `session_done_carries_run_numbers_and_says_of_plan_when_short`). */
    @Test
    fun distancesFollowTheDisplayUnit() {
        val item = PrescriptionView(
            run_head = "8 km", run_head_km = 8.0, run_target = RunTargetView(band = "easy"),
            safety_reason = "Capped at 8 km: at most 10% over your longest recent run",
        )
        assertEquals("8 km", runFaceHead(item, DistanceUnit.Km))
        assertEquals("5\u00A0mi", runFaceHead(item, DistanceUnit.Mi))
        assertEquals(item.safety_reason, safetyReasonText(item))
        assertEquals(null, safetyReasonText(PrescriptionView()))
    }
}
