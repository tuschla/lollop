package app.milestone

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** The pure rules behind the set/run editors: the loop's double-tap guard, quick
 *  picks, timer-style run duration, the rest countdown, and which sets count as
 *  "today's" for the loop. */
class SetLoggingLogicTest {

    @Test
    fun aRepeatTapInsideTheGuardIsNotANewSet() {
        assertTrue(loopSaveAccepted(null, 1_000))
        assertFalse("a double tap", loopSaveAccepted(1_000, 1_120))
        assertTrue("the next real set", loopSaveAccepted(1_000, 1_000 + LoopRepeatGuardMs))
        assertTrue("a clock that stepped back never blocks saving", loopSaveAccepted(5_000, 1_000))
    }

    @Test
    fun recentLiftsSurviveABusyPlanWeek() {
        val week = (1..10).map { "Plan lift $it" }
        val picks = quickPickCandidates("", todayOpen = listOf("Back Squat"), recents = listOf("Curl", "Lateral raise"), weekPlanned = week)
        assertEquals(listOf("Back Squat", "Curl", "Lateral raise", "Plan lift 1", "Plan lift 2", "Plan lift 3"), picks)
    }

    @Test
    fun typingFiltersTheQuickPicks() {
        val picks = quickPickCandidates(
            "dead",
            todayOpen = listOf("Back Squat"),
            recents = listOf("Curl"),
            weekPlanned = listOf("Romanian deadlift", "Deadlift", "Bench"),
        )
        assertEquals(listOf("Romanian deadlift", "Deadlift"), picks)
        // A name that already matches a pick shows the normal list (so the user can switch).
        assertEquals(
            listOf("Back Squat", "Curl", "Romanian deadlift"),
            quickPickCandidates("back squat", listOf("Back Squat"), listOf("Curl", "back squat"), listOf("Romanian deadlift")),
        )
    }

    @Test
    fun aFreeSetNeverMatchesTheDaysRunItem() {
        // A run item has no exercise name; a blank editor must not adopt it.
        val run = PrescriptionView(exercise = "", intensity_label = "5:36–6:06 /km (easy)", run_target = RunTargetView())
        val squat = PrescriptionView(exercise = "Back Squat")
        assertNull(planItemFor(listOf(run, squat), ""))
        assertNull(planItemFor(listOf(run, squat), "   "))
        assertEquals(squat, planItemFor(listOf(run, squat), "back squat"))
        // "Next" after a lift never points at the run.
        assertNull(nextOpenItem(listOf(run, squat.copy(done = true)), "Back Squat"))
    }

    @Test
    fun theSetFieldsWaitForAChosenExercise() {
        val known = listOf("Back Squat", "Curl")
        assertFalse("nothing typed", exerciseChosen("", known, added = null, preset = null, editing = false))
        assertFalse("a half-typed search", exerciseChosen("Back Sq", known, added = null, preset = null, editing = false))
        assertTrue("a known name, any case", exerciseChosen("back squat", known, added = null, preset = null, editing = false))
        assertTrue("a confirmed new name", exerciseChosen("Dumbbell fly", known, added = "Dumbbell fly", preset = null, editing = false))
        assertTrue("the rung the editor opened on", exerciseChosen("Ring row", known, added = null, preset = "Ring row", editing = false))
        assertTrue("an edited set", exerciseChosen("Old lift", known, added = null, preset = null, editing = true))
    }

    @Test
    fun runDurationIsTypedLikeATimer() {
        assertEquals(105.5, durationDigitsToMin("14530")!!, 1e-9)
        assertEquals("1:45:30", fmtDurationDigits("14530"))
        assertEquals("45:30", fmtDurationDigits("4530"))
        assertEquals("0:30", fmtDurationDigits("30"))
        assertEquals("14530", minutesToDurationDigits(105.5))
        assertEquals("4530", minutesToDurationDigits(45.5))
        // Seconds (and minutes under an hours field) must stay under 60.
        assertNull(durationDigitsToMin("160"))
        assertNull(durationDigitsToMin("16000"))
        assertEquals(90.0, durationDigitsToMin("9000")!!, 1e-9)
        assertNull(durationDigitsToMin(""))
        // Typing: no leading zero, decimal point ignored, at most hh:mm:ss.
        assertEquals("", editDurationDigits("", '0', replaceAll = false))
        assertEquals("14", editDurationDigits("14", '.', replaceAll = false))
        assertEquals("5", editDurationDigits("14530", '5', replaceAll = true))
        assertEquals("100000", editDurationDigits("100000", '1', replaceAll = false))
    }

    @Test
    fun theRestReadoutFollowsTheRunningRestWhateverTheExercise() {
        // A scheduled 3:00 rest, 100 s in: 80 s left, whatever item is on screen.
        assertEquals(RestFace.Counting(80, 180), restFace(1_000_000, 1_180_000, lastSetSec = null, itemRestSec = 0, nowMillis = 1_100_000))
        // Rounded up, so "0:01" shows until the rest is really over.
        assertEquals(RestFace.Counting(1, 180), restFace(1_000_000, 1_180_000, null, 120, 1_179_500))
        assertEquals(RestFace.Over(20), restFace(1_000_000, 1_180_000, null, 120, 1_200_000))
        // Past the window the rest belongs to an earlier bout.
        assertNull(restFace(1_000_000, 1_180_000, null, 120, 1_000_000 + (RestReadoutWindowSec + 1) * 1000L))
        // No running rest: an exercise without a prescribed rest counts up from its last set.
        assertEquals(RestFace.Since(30), restFace(null, null, lastSetSec = 1_000, itemRestSec = 0, nowMillis = 1_030_000))
        // ...but one with a prescribed rest shows nothing once its rest was undone.
        assertNull(restFace(null, null, lastSetSec = 1_000, itemRestSec = 120, nowMillis = 1_030_000))
    }

    @Test
    fun aCompletedExerciseMovesOnInsteadOfLoggingAJunkSet() {
        val open = PrescriptionView(exercise = "Back Squat", sets = 3, sets_done = 1)
        val done = open.copy(sets_done = 3, done = true)
        val next = PrescriptionView(exercise = "Deadlift", sets = 3)
        assertEquals(LoopPrimary.Save, loopPrimaryAction(false, open, null, true, false))
        assertEquals(LoopPrimary.LogPlannedSet, loopPrimaryAction(true, open, null, true, false))
        assertEquals(LoopPrimary.Next, loopPrimaryAction(true, done, next, true, false))
        assertEquals(LoopPrimary.FinishSession, loopPrimaryAction(true, done, null, true, false))
        // Only a set the user started changing logs an extra set.
        assertEquals(LoopPrimary.LogExtraSet, loopPrimaryAction(true, done, next, true, true))
        assertEquals(LoopPrimary.LogOffPlanSet, loopPrimaryAction(true, null, null, true, false))
        assertEquals(LoopPrimary.LogBlank, loopPrimaryAction(true, null, null, false, false))
    }

    @Test
    fun theLoopShowsOnlyThisExercisesActionableCue() {
        val nextTime = AdjustmentView(summary = "Add 4% next time", exercise = "Back Squat")
        val easier = AdjustmentView(summary = "Felt easier: add 8%", exercise = "Back Squat", suggested_load_kg = 142.5)
        val other = AdjustmentView(summary = "Stop here", exercise = "Bench", suggests_end_exercise = true)
        assertEquals(easier, inSessionCue("back squat", listOf(nextTime, other, easier), emptyList()))
        assertNull(inSessionCue("Back Squat", listOf(nextTime, other), emptyList()))
        // A session-review "stop here" names no exercise and applies to the lift in progress.
        val stop = AdjustmentView(summary = "Stop here", suggests_end_exercise = true)
        assertEquals(stop, inSessionCue("Back Squat", listOf(nextTime), listOf(stop)))
    }

    @Test
    fun warmUpsRampFortySixtyEightyWithATaperAfterTheFirstStep() {
        assertEquals(
            listOf(WarmUpStep(20.0, 5, bar = true), WarmUpStep(40.0, 5), WarmUpStep(60.0, 3), WarmUpStep(80.0, 2)),
            warmUpSteps(100.0, WeightUnit.Kg, "Back squat"),
        )
        // lb: 45 lb bar, then 115, 175 and 230 lb on the 5 lb grid for a 290 lb work set.
        assertEquals(
            listOf(lbToKgWire(45.0), lbToKgWire(115.0), lbToKgWire(175.0), lbToKgWire(230.0)),
            warmUpSteps(lbToKgWire(290.0), WeightUnit.Lb, "Back squat").map { it.kg },
        )
        // A light work set keeps only the ramp steps above the bar.
        assertEquals(listOf(20.0, 25.0, 32.5), warmUpSteps(40.0, WeightUnit.Kg, "Bench press").map { it.kg })
        assertEquals(emptyList<WarmUpStep>(), warmUpSteps(20.0, WeightUnit.Kg, "Bench press"))
    }

    @Test
    fun aFloorPullStartsPlateLoadedNotOnTheEmptyBar() {
        assertEquals(
            listOf(WarmUpStep(60.0, 5), WarmUpStep(85.0, 3), WarmUpStep(112.5, 2)),
            warmUpSteps(140.0, WeightUnit.Kg, "Deadlift"),
        )
        assertEquals(lbToKgWire(135.0), warmUpSteps(lbToKgWire(390.0), WeightUnit.Lb, "Sumo deadlift").first().kg, 1e-9)
        assertEquals(emptyList<WarmUpStep>(), warmUpSteps(60.0, WeightUnit.Kg, "Deadlift"))
        assertTrue(isFloorLift("Power clean"))
        assertFalse(isFloorLift("Romanian deadlift"))
        assertFalse(isFloorLift("Hang clean"))
    }

    @Test
    fun aNewExerciseIsOfferedAndSavedInSentenceCase() {
        val known = listOf("Back Squat", "Romanian deadlift")
        assertEquals("Dumbbell curl", newExerciseToAdd("dumbbell  curl", known))
        assertNull(newExerciseToAdd("back squat", known))
        assertNull(newExerciseToAdd("  ", known))
        assertEquals("Dumbbell curl", resolveExerciseName("dumbbell curl", known))
        assertEquals("Back Squat", resolveExerciseName("back squat", known))
        // A name the user cased themselves is kept.
        assertEquals("RDL", exerciseDisplayName("RDL"))
    }

    @Test
    fun weightTextIsWholeOrHalfPoundsAndTrimmedKilograms() {
        assertEquals("290", weightFieldText(lbToKgWire(290.0), WeightUnit.Lb))
        assertEquals("292.5", weightFieldText(lbToKgWire(292.5), WeightUnit.Lb))
        // kg is lossless: a 101.25 kg load is not rounded to a non-loadable 101.3.
        assertEquals("101.25", weightFieldText(101.25, WeightUnit.Kg))
        assertEquals("100", weightFieldText(100.0, WeightUnit.Kg))
        assertEquals("295", trimWeightText("295.0"))
        assertEquals("292.5", trimWeightText("292.5"))
    }

    @Test
    fun theLatestLoggableDayIsTodaysLocalDate() {
        val zone = java.time.ZoneId.of("America/New_York")
        // 23:30 local on Oct 16 is already Oct 17 in UTC; tomorrow must stay unselectable.
        val now = java.time.ZonedDateTime.of(2026, 10, 16, 23, 30, 0, 0, zone).toInstant().toEpochMilli()
        val oct16 = java.time.LocalDate.of(2026, 10, 16).atStartOfDay(java.time.ZoneOffset.UTC).toInstant().toEpochMilli()
        assertEquals(oct16, latestSelectableDayUtcMillis(now, zone))
    }

    @Test
    fun todaysSetsMatchTheItemOrTheName() {
        val dayStart = 10_000L
        val item = PrescriptionView(exercise = "Back squat", logged_sets = listOf(ItemSetView(entry_id = 7)))
        val lifts = listOf(
            LiftResultView(exercise = "Barbell back squat", observed_at = dayStart + 50, entry_id = 7),
            LiftResultView(exercise = "Back squat", observed_at = dayStart + 10, entry_id = 8),
            LiftResultView(exercise = "Back squat", observed_at = dayStart - 10, entry_id = 9),
            LiftResultView(exercise = "Bench", observed_at = dayStart + 20, entry_id = 10),
        )
        val today = todaySetsFor("Back squat", item, lifts, dayStart)
        assertEquals(listOf(8L, 7L), today.map { it.entry_id })
    }
}
