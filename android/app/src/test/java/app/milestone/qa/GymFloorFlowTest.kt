package app.milestone.qa

import android.content.Context
import androidx.compose.ui.semantics.SemanticsProperties
import android.os.Looper
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasAnyChild
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isDialog
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.isSelectable
import androidx.compose.ui.test.isSelected
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performScrollTo
import androidx.lifecycle.Lifecycle
import androidx.test.core.app.ApplicationProvider
import app.milestone.AppClock
import app.milestone.Event
import app.milestone.EventDispatcher
import app.milestone.LiftResultView
import app.milestone.R
import app.milestone.repsLeftText
import java.io.File
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.double
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.util.Locale
import java.time.Duration
import org.robolectric.Shadows.shadowOf
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * The gym-floor loop driven end to end through the UI: repeating a set, a
 * double tap, unrated effort, undo, warm-ups, the rest countdown, an off-plan
 * exercise, finishing an exercise and moving to the next, a calisthenics hold,
 * a session that runs past midnight, the last-time prefill, and the morning and
 * weekly check-ins that feed the coach.
 */
@RunWith(RobolectricTestRunner::class)
class GymFloorFlowTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @Before
    fun setUp() {
        Locale.setDefault(Locale.US)
        QaHarness.reset(ctx)
    }

    @After
    fun tearDown() = QaHarness.teardown()

    private val todayStartSec get() = QaHarness.daysAgoAtMidday(0) - 43_200L

    private fun openLoopFor(exercise: String) {
        // The current item's filled pill and every other open item's compact row
        // both carry "Log set 1 of N, <exercise>" as their tap target's label.
        rule.onNode(
            hasClickAction() and hasContentDescription(strPrefix(R.string.plan_log_set_n_of_n, 1), substring = true) and
                hasAnyAncestor(hasTestTag("plan-item:$exercise")),
        ).performScrollTo().performClick()
        rule.waitUntil(5_000) {
            rule.onAllNodes(hasText(strPrefix(R.string.log_set_header_n_of_n, 1), substring = true)).fetchSemanticsNodes().isNotEmpty()
        }
    }

    /** The loop editor's "Set k of N" header. */
    private fun setHeader(k: Int, n: Int) = str(R.string.log_set_header_n_of_n, k, n)

    /** An effort-scale cell, spoken as "k reps left". */
    private fun effortCell(repsLeft: Int) = hasContentDescription(str(R.string.log_effort_cd, "$repsLeft")) and isSelectable()

    /** The weight well, spoken with its value ("Weight, 100 kg"). */
    private fun weightWell(shown: String) = hasContentDescription("${str(R.string.log_field_weight)}, $shown kg")

    /** Wait for app state the screen doesn't show (the model, the event log).
     *  The semantics fetch is what idles the main looper under Robolectric (see
     *  [launchApp]), so the core's reply can land while we poll. */
    private fun waitForState(timeoutMs: Long = 10_000, cond: () -> Boolean) =
        rule.waitUntil(timeoutMs) {
            rule.onAllNodes(isRoot()).fetchSemanticsNodes()
            cond()
        }

    /** The loop's pinned primary action ("Log set k of N"). */
    private fun logSet() = rule.onNodeWithTag("editor-primary").performClick()

    /** The lifter rests before the next set (also clears the double-tap guard). */
    private fun rest(sec: Long = 90) {
        AppClock.offsetMillis += sec * 1000L
    }

    private fun todaysSets(exercise: String): List<LiftResultView> =
        EventDispatcher.model.value!!.lifts.filter { it.exercise == exercise && it.observed_at >= todayStartSec }

    @Test
    fun anUntouchedSaveRepeatsTheLoadJustLogged() {
        // An anchored squat prefills its prescribed load; the lifter bumps it
        // +2.5 kg for set 1, then repeats with an untouched Save.
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(1))))
        rule.launchApp()
        openLoopFor("Back Squat")
        // The ± chip's "+2.5" is a figure, not a string resource.
        rule.onNodeWithText("+2.5").performClick()
        logSet()
        rule.assertTextCountEventually(setHeader(2, 4), 1)
        rule.onNodeWithTag("editor-primary").assert(hasText(str(R.string.log_primary_set_n_of_n, 2, 4)))
        rest()
        logSet()
        rule.assertTextCountEventually(setHeader(3, 4), 1)
        assertEquals("both sets carry the load the lifter chose", listOf(120.0, 120.0), todaysSets("Back Squat").map { it.weight_kg })
    }

    @Test
    fun aDoubleTapOnSaveLogsOneSet() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(1))))
        rule.launchApp()
        openLoopFor("Back Squat")
        logSet()
        logSet()
        rule.assertTextCountEventually(setHeader(2, 4), 1)
        // A later, deliberate set still logs; it lands as set 2, so the double tap
        // added nothing (the serial core applied every earlier event first).
        rest()
        logSet()
        rule.assertTextCountEventually(setHeader(3, 4), 1)
        assertEquals(2, todaysSets("Back Squat").size)
    }

    @Test
    fun effortIsRatedFreshEverySetAndUnratedStaysUnrated() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        openLoopFor("Back squat")
        // The coach's target shows as a hint in reps left ("target ~2 reps left"),
        // nothing is pre-picked. Any reps-left wording the target may resolve to.
        val target = strPrefix(R.string.log_rpe_target_hint)
        val repsLeft = ((12..20).map { it / 2.0 }).map { repsLeftText(ctx.resources, it) }
        val texts = rule.onAllNodes(hasText(target, substring = true)).fetchSemanticsNodes()
            .flatMap { n -> n.config.getOrNull(SemanticsProperties.Text).orEmpty().map { it.text } }
        assertTrue("the effort target is a hint: $texts", texts.any { t -> repsLeft.any { t == str(R.string.log_rpe_target_hint, it) } })
        rule.onAllNodes(isSelectable() and isSelected() and hasAnyAncestor(hasTestTag("effort-scale"))).assertCountEquals(0)
        rule.typeWeight("60")
        // The whole effort scale is on screen in the loop: one tap, no scrolling.
        val rpeEight = effortCell(2)
        rule.onNode(rpeEight).assertIsDisplayed().performClick()
        logSet()
        rule.assertTextCountEventually(setHeader(2, 3), 1)
        rest()
        logSet()
        rule.assertTextCountEventually(setHeader(3, 3), 1)
        assertEquals(
            "set 1 carries the athlete's rating; set 2 was not rated and stays unrated",
            listOf(8.0, 0.0),
            todaysSets("Back squat").sortedBy { it.observed_at }.map { it.rpe },
        )
    }

    @Test
    fun undoRemovesTheLoggedSet() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(1))))
        rule.launchApp()
        openLoopFor("Back Squat")
        logSet()
        rule.assertTextCountEventually(setHeader(2, 4), 1)
        rule.onNodeWithText(str(R.string.log_undo)).performClick()
        rule.assertTextCountEventually(setHeader(1, 4), 1)
        assertTrue("the undone set is gone", todaysSets("Back Squat").isEmpty())
        val squat = EventDispatcher.model.value!!.week_plan
            .first { it.epoch_day == app.milestone.todayEpochDay() }.items.first { it.exercise == "Back Squat" }
        assertEquals(0, squat.sets_done)
    }

    @Test
    fun theLastSetCanBeEditedFromTheLoop() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(1))))
        rule.launchApp()
        openLoopFor("Back Squat")
        logSet()
        rule.assertTextCountEventually(setHeader(2, 4), 1)
        // While the undo row names the set just logged, the header's "Last: … · Edit"
        // line stays out (one statement of the set, not two); it returns once the
        // undo window closes.
        val editLast = hasContentDescription(strPrefix(R.string.log_edit_set_cd), substring = true)
        rule.onAllNodesWithText(str(R.string.log_undo)).assertCountEquals(1)
        rule.onAllNodes(editLast).assertCountEquals(0)
        rule.mainClock.advanceTimeBy(9_000L)
        shadowOf(Looper.getMainLooper()).idleFor(Duration.ofSeconds(9))
        rule.assertCountEventually(editLast, 1)
        rule.onNode(editLast).performClick()
        rule.assertTextCountEventually(str(R.string.log_set_title_edit), 1)
    }

    @Test
    fun aWarmupDoesNotBecomeTheNextPrefill() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(1))))
        rule.launchApp()
        openLoopFor("Back Squat")
        val prescribed = EventDispatcher.model.value!!.week_plan
            .first { it.epoch_day == app.milestone.todayEpochDay() }.items.first { it.exercise == "Back Squat" }.load_kg!!
        val shown = app.milestone.weightFieldText(prescribed, app.milestone.WeightUnit.Kg)
        // A light ramp set typed on the keypad (not the reps/effort chips); the
        // keypad opens under the weight well once the well is tapped.
        rule.onNode(weightWell(shown)).performClick()
        rule.onNode(hasText("6") and hasAnyAncestor(hasTestTag("numeric-keypad"))).performScrollTo().performClick()
        rule.onNode(hasText("0") and hasAnyAncestor(hasTestTag("numeric-keypad"))).performScrollTo().performClick()
        logSet()
        // Not counted as set 1, and the working load comes back for the next set.
        rule.assertCountEventually(weightWell(shown), 1)
        rule.onAllNodesWithText(setHeader(1, 4)).assertCountEquals(1)
        assertEquals(listOf(60.0), todaysSets("Back Squat").map { it.weight_kg })
    }

    @Test
    fun theRestCountsDownToThePrescribedRest() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(1))))
        rule.launchApp()
        openLoopFor("Back Squat")
        val restSec = EventDispatcher.model.value!!.week_plan
            .first { it.epoch_day == app.milestone.todayEpochDay() }.items.first { it.exercise == "Back Squat" }.rest_sec
        logSet()
        rule.assertCountEventually(hasContentDescription(strPrefix(R.string.log_rest_left_cd), substring = true), 1)
        rest(restSec + 5L)
        rule.assertCountEventually(hasContentDescription(str(R.string.plan_rest_done_cd), substring = true), 1)
    }

    @Test
    fun anOffPlanExerciseLoopsToo() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        rule.onNodeWithContentDescription(str(R.string.main_log)).performClick()
        rule.onNodeWithText(str(R.string.main_log_set_title)).performClick()
        rule.onNode(hasSetTextAction()).performTextInput("Curl")
        // A new name is confirmed with "+ Add"; then weight, reps and effort show.
        rule.onNodeWithText(str(R.string.log_add_exercise, "Curl")).performScrollTo().performClick()
        rule.typeWeight("10")
        // The unset reps row opens centred on the common 8–10 band, so 5 is scrolled to.
        rule.onNode(hasText("5") and hasAnyAncestor(hasTestTag("reps-scale"))).performScrollTo().performClick()
        logSet()
        // The sheet stays open on the next set of the same exercise.
        rule.assertTextCountEventually(str(R.string.log_set_header_n, 2), 1)
        rule.onAllNodes(hasSetTextAction() and hasText("Curl")).assertCountEquals(1)
        assertEquals(listOf(10.0), todaysSets("Curl").map { it.weight_kg })
    }

    @Test
    fun finishingAnExerciseInTheLoopOffersTheNextOne() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        openLoopFor("Back squat")
        rule.typeWeight("60")
        repeat(3) { i ->
            logSet()
            if (i < 2) rule.assertTextCountEventually(setHeader(i + 2, 3), 1)
            rest()
        }
        rule.assertTextCountEventually(str(R.string.plan_item_complete), 1)
        // A strength goal's hinge is the competition deadlift (STRENGTH-10).
        rule.onNodeWithText(str(R.string.plan_next_exercise, "Deadlift")).performClick()
        rule.assertCountEventually(hasTestTag("loop-exercise") and hasText("Deadlift"), 1)
        rule.onAllNodesWithText(setHeader(1, 3)).assertCountEquals(1)
        rule.onNodeWithText(str(R.string.log_set_done)).performClick()
        // Back on Today: the squat is a collapsed ✓ row and the other three are open.
        // ("3/3" is a bare figure pair, not a string resource.)
        rule.assertTextCountEventually("3/3", 1)
        rule.onAllNodes(
            hasClickAction() and hasContentDescription(str(R.string.plan_log_set_n_of_n, 1, 3), substring = true),
        ).assertCountEquals(3)
    }

    @Test
    fun setsWithoutRpeNeverShowAFabricatedRpe() {
        val at = QaHarness.daysAgoAtMidday(0) - 3_600
        QaHarness.seedLog(
            ctx,
            listOf(
                QaHarness.strengthProfile(),
                Event.LogSet("Back Squat", 100.0, 5, 8.0, at, at * 1000),
                // A lighter set logged without RPE: listed under the item as not counted.
                Event.LogSet("Back Squat", 40.0, 5, 0.0, at + 60, at * 1000 + 1),
            ),
        )
        rule.launchApp()
        rule.onAllNodesWithText(str(R.string.plan_set_not_counted), substring = true).assertCountEquals(1)
        assertTrue(UxInvariants.check(rule, rule.density).none { it.kind == "forbidden-text" })
        rule.selectTab(str(R.string.main_dest_history))
        // The seeded 40 kg set's line.
        rule.scrollToText("40 kg")
        val history = UxInvariants.check(rule, rule.density).filter { it.kind == "forbidden-text" }
        assertTrue("History shows no fabricated RPE: $history", history.isEmpty())
    }

    @Test
    fun aHoldRungLogsSecondsNotReps() {
        QaHarness.seedLog(ctx, listOf(QaHarness.calisthenicsProfile()))
        rule.launchApp()
        openLoopFor("Dead hang")
        // The compact header states the seconds target beside the set count
        // ("Set 1 of 3 · 30 s").
        rule.onAllNodesWithText("\u00b7 " + str(R.string.log_band_hold, 30), substring = true).assertCountEquals(1)
        // A hold rung asks for seconds, not reps.
        rule.onAllNodesWithText(str(R.string.log_field_hold), substring = true).assertCountEquals(1)
        rule.onAllNodesWithText(str(R.string.log_field_reps)).assertCountEquals(0)
    }

    @Test
    fun theEffortScaleIsOnScreenInTheLoopWithoutScrolling() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(1))))
        rule.launchApp()
        openLoopFor("Back Squat")
        // Both ends of the one-row scale, with no performScrollTo.
        rule.onNode(effortCell(2)).assertIsDisplayed()
        rule.onNode(effortCell(0)).assertIsDisplayed()
        rule.onNodeWithTag("editor-primary").assertIsDisplayed()
        rule.onNode(effortCell(2)).performClick()
        logSet()
        rule.assertTextCountEventually(setHeader(2, 4), 1)
        assertEquals(listOf(8.0), todaysSets("Back Squat").map { it.rpe })
    }

    @Test
    fun aLiftWithAPriorSetPrefillsThatWeightAndShowsLastTime() {
        // Accessory sets from Saturday. The core gives each of today's matching
        // items the athlete's last top set; an effort-prescribed one (no load of
        // its own) starts from that weight (or the core's one-step-up suggestion),
        // an anchored one from its prescribed load.
        val prior = QaHarness.daysAgoAtMidday(2)
        QaHarness.seedLog(
            ctx,
            listOf(
                QaHarness.strengthProfile(),
                Event.LogSet("Row", 60.0, 8, 7.0, prior, prior * 1000),
                Event.LogSet("Bench press", 60.0, 8, 7.0, prior + 600, prior * 1000 + 1),
                Event.LogSet("Romanian deadlift", 80.0, 8, 7.0, prior + 1200, prior * 1000 + 2),
            ),
        )
        rule.launchApp()
        val today = EventDispatcher.model.value!!.week_plan.first { it.epoch_day == app.milestone.todayEpochDay() }
        val withLast = today.items.filter { it.last_set != null && !it.done }
        assertTrue("today's items carry last time: ${today.items}", withLast.isNotEmpty())
        val item = withLast.firstOrNull { it.load_kg == null } ?: withLast.first()
        val last = item.last_set!!
        val expectedKg = item.load_kg ?: item.suggested_load_kg ?: last.weight_kg
        openLoopFor(item.exercise)
        rule.assertCountEventually(weightWell(app.milestone.weightFieldText(expectedKg, app.milestone.WeightUnit.Kg)), 1)
        // The note rides in the weight field's hint slot (joined with Per side), below
        // the well inside the editor's own scroll body.
        rule.onNode(hasText(str(R.string.log_last_time, last.summary), substring = true))
            .performScrollTo().assertIsDisplayed()
        // An untouched Save (reps prefilled from the band) logs exactly that weight.
        logSet()
        waitForState { todaysSets(item.exercise).isNotEmpty() }
        assertEquals(expectedKg, todaysSets(item.exercise).first().weight_kg, 1e-9)
    }

    @Test
    fun thePrefillFallsBackFromPrescribedToSuggestedToLastSet() {
        val last = app.milestone.ItemSetView(weight_kg = 12.0, reps = 12, rpe = 8.0, summary = "12 kg × 12 @ RPE 8")
        val rx = { p: app.milestone.PrescriptionView ->
            app.milestone.prescriptionPrefill(p, app.milestone.WeightUnit.Kg)
        }
        // An isolation lift with only a prior set starts from that set's weight.
        assertEquals(12.0, rx(app.milestone.PrescriptionView(exercise = "Lateral raise", last_set = last)).weightKgVerbatim!!, 0.0)
        // The core's suggestion wins over the raw last set, a prescribed load over both.
        assertEquals(
            14.0,
            rx(app.milestone.PrescriptionView(exercise = "Lateral raise", last_set = last, suggested_load_kg = 14.0)).weightKgVerbatim!!,
            0.0,
        )
        assertEquals(
            100.0,
            rx(app.milestone.PrescriptionView(exercise = "Squat", load_kg = 100.0, suggested_load_kg = 14.0, last_set = last)).weightKgVerbatim!!,
            0.0,
        )
        // The effort target stays a hint, and a ballistic item carries none.
        val jump = app.milestone.PrescriptionView(
            exercise = "Box jump",
            target_rpe = 8.0,
            calis = app.milestone.CalisItemView(ballistic = true),
        )
        assertEquals(null, rx(jump).targetRpe)
    }

    private fun eventLogLines(): List<String> =
        File(ctx.filesDir, "event-log.ndjson").takeIf { it.exists() }?.readLines().orEmpty()

    @Test
    fun theWeeklyCheckinSendsTheListedSignsAndCanBeCleared() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        rule.onNodeWithContentDescription(str(R.string.main_log)).performClick()
        rule.onNodeWithText(str(R.string.main_weekly_checkin_title)).performScrollTo().performClick()
        rule.onNodeWithText(str(R.string.log_weekly_sign_sleep)).performScrollTo().performClick()
        rule.onNodeWithText(str(R.string.log_weekly_sign_mood)).performScrollTo().performClick()
        rule.onNodeWithText(str(R.string.log_weekly_signs_1to2)).performScrollTo().performClick()
        rule.onNodeWithTag("editor-primary").performClick()
        waitForState { eventLogLines().any { it.contains("SubmitWeeklyCheckin") } }
        val payload = Json.parseToJsonElement(eventLogLines().last { it.contains("SubmitWeeklyCheckin") })
            .jsonObject["SubmitWeeklyCheckin"]!!.jsonObject
        val signs = payload["overtraining_signs"]!!.jsonObject
        assertTrue(signs["worse_sleep"]!!.jsonPrimitive.boolean)
        assertTrue(signs["low_mood"]!!.jsonPrimitive.boolean)
        assertTrue(!signs["worse_performance"]!!.jsonPrimitive.boolean)
        assertEquals(1.5, payload["overtraining_signal_weeks"]!!.jsonPrimitive.double, 0.0)
        waitForState { EventDispatcher.model.value?.weekly_checkin_present == true }

        // Reopened, the sheet offers to clear the check-in in force.
        rule.onNodeWithContentDescription(str(R.string.main_log)).performClick()
        rule.onNodeWithText(str(R.string.main_weekly_checkin_title)).performScrollTo().performClick()
        rule.onNodeWithText(str(R.string.log_weekly_clear)).performScrollTo().performClick()
        rule.onNode(hasText(str(R.string.log_weekly_clear_confirm)) and hasAnyAncestor(isDialog())).performClick()
        waitForState { EventDispatcher.model.value?.weekly_checkin_present == false }
    }

    /** Open the morning check-in and answer sleep, soreness and mood with 3. */
    private fun openCheckinWithRequiredAnswers() {
        rule.onNodeWithContentDescription(str(R.string.main_log)).performClick()
        rule.onNodeWithText(str(R.string.main_morning_checkin_title)).performScrollTo().performClick()
        // Each scale is one RatingScale tagged "checkin-<key>".
        rule.waitUntil(5_000) { rule.onAllNodes(hasTestTag("checkin-sleep")).fetchSemanticsNodes().isNotEmpty() }
        // Any answer opens Save; answer 3 on the first three scales.
        for (key in listOf("sleep", "soreness", "mood")) {
            rule.pickRating("checkin-$key", 3)
        }
    }

    /** The value of the last Illness report the check-in wrote to the log. */
    private fun lastIllnessValue(): Double {
        val line = eventLogLines().last { it.contains("\"Illness\"") }
        return Json.parseToJsonElement(line).jsonObject["SubmitReadiness"]!!
            .jsonObject["value"]!!.jsonPrimitive.double
    }

    @Test
    fun fluOnTheCheckinHoldsTraining() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        assertTrue(!EventDispatcher.model.value!!.train_blocked)
        openCheckinWithRequiredAnswers()
        // The helper line says what each answer means for training.
        rule.onNodeWithText(str(R.string.log_illness_help), substring = true).performScrollTo().assertIsDisplayed()
        rule.onNodeWithText(str(R.string.log_illness_below_neck), substring = true).performScrollTo().performClick()
        rule.onNodeWithTag("editor-primary").performClick()
        waitForState { EventDispatcher.model.value?.train_blocked == true }
        assertTrue(eventLogLines().any { it.contains("SubmitCheckin") })
        // Flu / fever / body aches is the below-neck-or-fever tier.
        assertEquals(2.0, lastIllnessValue(), 0.0)
    }

    @Test
    fun headColdOnTheCheckinDowngradesWithoutHoldingTraining() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        openCheckinWithRequiredAnswers()
        rule.onNodeWithText(str(R.string.log_illness_above_neck), substring = true).performScrollTo().performClick()
        rule.onNodeWithTag("editor-primary").performClick()
        waitForState { eventLogLines().any { it.contains("\"Illness\"") } }
        // A head cold is the above-neck tier: easier sessions, training not held.
        assertEquals(1.0, lastIllnessValue(), 0.0)
        rule.waitForIdle()
        assertTrue(!EventDispatcher.model.value!!.train_blocked)
    }

    @Test
    fun crossingMidnightMovesTheLogButtonsWithTheDay() {
        // Monday 23:50: today's session is actionable.
        AppClock.offsetMillis += (13 * 60 + 50) * 60_000L
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        val logFirstSet = strPrefix(R.string.plan_log_set_n_of_n, 1)
        rule.onAllNodesWithText(str(R.string.main_todays_session).uppercase(Locale.US)).assertCountEquals(1)
        assertTrue(rule.onAllNodesWithText(logFirstSet, substring = true).fetchSemanticsNodes().isNotEmpty())
        // 00:10 Tuesday (a rest day); the app comes back to the foreground.
        AppClock.offsetMillis += 20 * 60_000L
        lastScenario!!.moveToState(Lifecycle.State.STARTED)
        lastScenario!!.moveToState(Lifecycle.State.RESUMED)
        rule.assertTextCountEventually(str(R.string.main_rest_day_next_overline).uppercase(Locale.US), 1)
        rule.onAllNodesWithText(logFirstSet, substring = true).assertCountEquals(0)
    }
}
