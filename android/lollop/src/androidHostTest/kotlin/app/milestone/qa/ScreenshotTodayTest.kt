package app.milestone.qa

import android.content.Context
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasScrollToIndexAction
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeUp
import androidx.test.core.app.ApplicationProvider
import app.milestone.AppClock
import app.milestone.AppFileNames
import app.milestone.ConcurrentGoal
import app.milestone.DarkMode
import app.milestone.Event
import app.milestone.EventDispatcher
import app.milestone.LiftGoal
import app.milestone.LiftResultView
import app.milestone.R
import app.milestone.ReadinessSignal
import app.milestone.SessionPlanView
import app.milestone.TestClock
import app.milestone.WorkoutType
import app.milestone.overtrainingWeekEdgeLabels
import java.io.File
import java.util.Locale
import org.junit.After
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment

/**
 * Screenshot baselines for the Today coaching surfaces and the logging flows the
 * UX + coaching overhaul added or changed, light + dark, from the REAL app over
 * the real core (same recording flow as [ScreenshotTest]). Tall screens are
 * captured in several scroll positions (`-top` / `-mid` / `-bottom`) so every new
 * element lands in at least one image; one 1.3 font-scale capture per area covers
 * its densest screen.
 */
@RunWith(RobolectricTestRunner::class)
class ScreenshotTodayTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @Before
    fun setUp() {
        QaHarness.reset(ctx)
    }

    @After
    fun tearDown() {
        releaseClock()
        QaHarness.teardown()
    }

    /** Swipe Today's lazy list to its end, the way a thumb does (extra swipes clamp,
     *  so the final position is the same every run; the baselines were recorded so). */
    private fun swipeTodayToEnd() {
        repeat(10) {
            rule.onAllNodes(hasScrollToIndexAction())[0].performTouchInput { swipeUp() }
            rule.waitForIdle()
        }
    }

    private val todayStartSec get() = QaHarness.daysAgoAtMidday(0) - 43_200L

    private fun squat(observedAt: Long): Event =
        Event.LogSet(TYPED_SQUAT, 120.0, 3, 8.0, observedAt, observedAt * 1000)

    private fun todaysSets(exercise: String): List<LiftResultView> =
        EventDispatcher.model.value!!.lifts.filter { it.exercise == exercise && it.observed_at >= todayStartSec }

    private fun hypertrophyProfile(): Event = QaHarness.strengthProfile().copy(
        liftGoal = LiftGoal.Hypertrophy,
        concurrentGoal = ConcurrentGoal.Hypertrophy,
        weeklySets = 14,
        bodyweightKg = 80.0,
    )

    private val thisMorning get() = todayStartSec + 8 * 3_600L

    private fun plan(mode: DarkMode, fontScale: Float? = null) {
        useDarkMode(mode)
        fontScale?.let { RuntimeEnvironment.setFontScale(it) }
        seedStrengthWithHistory(ctx)
        rule.launchApp()
        rule.waitForText(strPrefix(R.string.plan_face_per_side), substring = true)
        val tag = if (fontScale != null) "fs130-" else ""
        rule.shot("today-plan-top-$tag${mode.suffix}")
        if (fontScale != null) return
        val details = hasClickAction() and
            hasContentDescription(str(R.string.main_details_subject, EventDispatcher.model.value!!.next_session!!.title))
        rule.onNode(details).performClick()
        rule.shot("today-plan-evidence-${mode.suffix}")
        rule.onNode(hasClickAction() and hasContentDescription(str(R.string.main_hide_details), substring = true)).performClick()
        rule.waitForIdle()
        swipeTodayToEnd()
        rule.shot("today-plan-bottom-${mode.suffix}")
    }

    @Test fun planLight() = plan(DarkMode.Light)

    @Test fun planDark() = plan(DarkMode.Dark)

    @Test fun planFontScale130Light() = plan(DarkMode.Light, fontScale = 1.3f)

    private fun sessionDone(mode: DarkMode) {
        useDarkMode(mode)
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), squat(QaHarness.daysAgoAtMidday(2))))
        rule.launchApp()
        val session = EventDispatcher.model.value!!.next_session!!
        val now = AppClock.nowSec()
        var id = now * 1_000
        var sent = 0
        session.items.filter { it.exercise.isNotEmpty() }.forEachIndexed { i, item ->
            val reps = maxOf(item.reps_low, item.reps_high, 1)
            // Over its prescription: heavier than the prior best.
            val kg = (item.load_kg ?: item.suggested_load_kg ?: 60.0) + if (item.exercise == TYPED_SQUAT) 5.0 else 0.0
            repeat(maxOf(item.sets, 1)) { k ->
                EventDispatcher.send(
                    Event.LogSet(item.exercise, kg, reps, 8.0, now - 3_000 + i * 300 + k * 60, ++id, holdSec = null),
                )
                sent++
            }
        }
        rule.waitForState { EventDispatcher.model.value!!.lifts.count { it.observed_at >= todayStartSec } == sent }
        rule.waitForNode(hasTestTag("session-done"))
        rule.shot("today-sessiondone-top-${mode.suffix}")
        swipeTodayToEnd()
        rule.shot("today-sessiondone-bottom-${mode.suffix}")
    }

    @Test fun sessionDoneLight() = sessionDone(DarkMode.Light)

    @Test fun sessionDoneDark() = sessionDone(DarkMode.Dark)

    private fun consistency(mode: DarkMode) {
        useDarkMode(mode)
        val start = app.milestone.todayEpochDay() - 28
        val doneDays = listOf(28, 26, 24, 21, 19, 14, 12, 10, 7, 5)
        QaHarness.seedLog(
            ctx,
            listOf(QaHarness.strengthProfile(), Event.GeneratePlan(start)) +
                doneDays.mapIndexed { i, d ->
                    val at = QaHarness.daysAgoAtMidday(d)
                    Event.LogSet(TYPED_SQUAT, 100.0 + i, 5, 8.0, at, at * 1000)
                },
        )
        rule.launchApp()
        rule.scrollTodayTo(hasTestTag("this-week"))
        rule.shot("today-consistency-${mode.suffix}")
        // The Progress card: the squat's best set per session, captioned
        // "<exercise> · estimated max" (matched by its fixed tail).
        rule.scrollTodayTo(hasText(str(R.string.main_e1rm_title, "").trim(), substring = true))
        rule.shot("today-progress-${mode.suffix}")
    }

    @Test fun consistencyLight() = consistency(DarkMode.Light)

    @Test fun consistencyDark() = consistency(DarkMode.Dark)

    private fun painHold(mode: DarkMode) {
        useDarkMode(mode)
        QaHarness.seedLog(
            ctx,
            listOf(QaHarness.strengthProfile(), Event.SubmitReadiness(ReadinessSignal.Pain, 1.0, thisMorning)),
        )
        rule.launchApp()
        // The resolve action's label is the core's own copy.
        rule.waitForText("I'm pain-free now")
        rule.shot("today-painhold-${mode.suffix}")
        rule.onNodeWithText("I'm pain-free now").performClick()
        rule.waitForText(str(R.string.main_resolve_hold_confirm))
        rule.shot("today-painhold-confirm-${mode.suffix}")
    }

    @Test fun painHoldLight() = painHold(DarkMode.Light)

    @Test fun painHoldDark() = painHold(DarkMode.Dark)

    /** A head cold is a non-blocking hold: a card, not the banner. */
    private fun holdCard(mode: DarkMode) {
        useDarkMode(mode)
        QaHarness.seedLog(
            ctx,
            listOf(QaHarness.strengthProfile(), Event.SubmitReadiness(ReadinessSignal.Illness, 1.0, thisMorning)),
        )
        rule.launchApp()
        // The resolve action's label is the core's own copy.
        rule.waitForText("I've recovered")
        rule.scrollTodayTo(hasText("I've recovered"))
        rule.shot("today-holdcard-${mode.suffix}")
    }

    @Test fun holdCardLight() = holdCard(DarkMode.Light)

    @Test fun holdCardDark() = holdCard(DarkMode.Dark)

    private fun calisthenics(mode: DarkMode) {
        useDarkMode(mode)
        QaHarness.seedLog(ctx, listOf(QaHarness.calisthenicsProfile()))
        rule.launchApp()
        val harder = hasContentDescription(strPrefix(R.string.plan_calis_chip_cd, str(R.string.plan_calis_harder)), substring = true)
        rule.waitForNode(harder)
        rule.shot("today-calis-top-${mode.suffix}")
        rule.onAllNodes(harder)[0].performClick()
        rule.waitForState { EventDispatcher.model.value!!.calisthenics_ladders.any { it.current_rung != null } }
        rule.shot("today-calis-harder-${mode.suffix}")
    }

    @Test fun calisthenicsLight() = calisthenics(DarkMode.Light)

    @Test fun calisthenicsDark() = calisthenics(DarkMode.Dark)

    /**
     * The runner's plan is anchored on the pinned Monday; the quality day (reps,
     * else tempo/threshold, else strides) is found in this week's plan, and the
     * app is relaunched with the clock on that day over the same log, so the run
     * is TODAY'S SESSION on the hero.
     */
    private fun runDay(mode: DarkMode) {
        useDarkMode(mode)
        QaHarness.seedLog(ctx, listOf(QaHarness.tenKRunnerProfile()))
        rule.launchApp()
        val vm = EventDispatcher.model.value!!
        val today = vm.today_epoch_day ?: app.milestone.todayEpochDay()
        val runDays = vm.week_plan.filter { d -> d.epoch_day >= today && d.items.any { it.run_target != null } }
        val pick: SessionPlanView = runDays.firstOrNull { d -> d.items.any { (it.run_structure?.reps ?: 0) > 0 } }
            ?: runDays.firstOrNull { d -> d.items.any { it.workout_type == WorkoutType.Tempo || it.workout_type == WorkoutType.Interval } }
            ?: runDays.firstOrNull { d -> d.items.any { (it.run_structure?.strides_reps ?: 0) > 0 } }
            ?: runDays.first()
        if (pick.epoch_day != today) {
            val log = File(ctx.filesDir, AppFileNames.EVENT_LOG).readText()
            QaHarness.teardown()
            QaHarness.reset(ctx)
            useDarkMode(mode)
            File(ctx.filesDir, AppFileNames.EVENT_LOG).writeText(log)
            TestClock.offsetMillis += (pick.epoch_day - today) * 86_400_000L
            rule.launchApp()
        }
        rule.waitForText(str(R.string.main_todays_session).uppercase(Locale.US))
        rule.waitForNode(hasTestTag("plan-item:run"))
        rule.shot("today-run-top-${mode.suffix}")
    }

    @Test fun runDayLight() = runDay(DarkMode.Light)

    @Test fun runDayDark() = runDay(DarkMode.Dark)

    private fun hypertrophy(mode: DarkMode) {
        useDarkMode(mode)
        QaHarness.seedLog(ctx, listOf(hypertrophyProfile()))
        rule.launchApp()
        rule.waitForText(str(R.string.main_todays_session).uppercase(Locale.US))
        rule.shot("today-hypertrophy-top-${mode.suffix}")
        val section = hasText(str(R.string.main_muscle_volume_title).uppercase(Locale.US), substring = true) and hasClickAction()
        rule.scrollTodayTo(section)
        rule.onNode(section).performClick()
        rule.waitForIdle()
        // To the end of Today so the whole expanded table, its last rows included,
        // sits clear of the Log button.
        swipeTodayToEnd()
        rule.shot("today-hypertrophy-volume-${mode.suffix}")
    }

    @Test fun hypertrophyLight() = hypertrophy(DarkMode.Light)

    @Test fun hypertrophyDark() = hypertrophy(DarkMode.Dark)

    private fun moveDialog(mode: DarkMode) {
        useDarkMode(mode)
        seedStrengthWithHistory(ctx)
        rule.launchApp()
        rule.onNode(hasClickAction() and hasContentDescription(str(R.string.main_move_session), substring = true)).performClick()
        rule.waitForNode(SemanticsMatcher("testTag starts with move-target:") {
            it.config.getOrNull(SemanticsProperties.TestTag)?.startsWith("move-target:") == true
        })
        rule.shot("today-move-dialog-${mode.suffix}")
    }

    @Test fun moveDialogLight() = moveDialog(DarkMode.Light)

    @Test fun moveDialogDark() = moveDialog(DarkMode.Dark)

    private fun restDay(mode: DarkMode) {
        useDarkMode(mode)
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        rule.onNode(hasClickAction() and hasContentDescription(str(R.string.main_move_session), substring = true)).performClick()
        val target = SemanticsMatcher("testTag starts with move-target:") {
            it.config.getOrNull(SemanticsProperties.TestTag)?.startsWith("move-target:") == true
        }
        rule.waitForNode(target)
        rule.onAllNodes(target)[0].performClick()
        rule.waitForText(str(R.string.main_rest_day_next_overline).uppercase(Locale.US))
        rule.shot("today-restday-${mode.suffix}")
    }

    @Test fun restDayLight() = restDay(DarkMode.Light)

    @Test fun restDayDark() = restDay(DarkMode.Dark)

    /** Eight steady mornings, then two isolated spikes far off them, each refuted
     *  by the reading after it, and a normal reading today: 2 of the last 3 are
     *  unreliable artifacts, so the core suspends HRV gating. (A shift the next
     *  reading reproduces is a real change, not an artifact, and would not suspend
     *  anything.) */
    private fun hrvSuspended(mode: DarkMode) {
        useDarkMode(mode)
        val steady = listOf(60.0, 62.0, 58.0, 61.0, 59.0, 60.0, 63.0, 61.0)
        val mornings = steady.mapIndexed { i, ms -> (10 - i) to ms } + listOf(2 to 120.0, 1 to 25.0, 0 to 60.0)
        QaHarness.seedLog(
            ctx,
            listOf(QaHarness.strengthProfile()) + mornings.map { (daysAgo, ms) ->
                Event.SubmitCheckin(observedAt = QaHarness.daysAgoAtMidday(daysAgo) - 4 * 3_600L, hrvRmssdMs = ms)
            },
        )
        rule.launchApp()
        rule.waitForText(str(R.string.main_restart_hrv_baseline))
        rule.shot("today-hrv-suspended-${mode.suffix}")
    }

    @Test fun hrvSuspendedLight() = hrvSuspended(DarkMode.Light)

    @Test fun hrvSuspendedDark() = hrvSuspended(DarkMode.Dark)

    /** The running rest's start, else the newest squat set's stamp. */
    private fun restStartMillis(): Long = app.milestone.RestAlarm.running.value?.startMillis
        ?: (todaysSets(TYPED_SQUAT).maxOf { it.observed_at } * 1_000L)

    private fun setLoopRest(mode: DarkMode, fontScale: Float? = null) {
        useDarkMode(mode)
        fontScale?.let { RuntimeEnvironment.setFontScale(it) }
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), squat(QaHarness.daysAgoAtMidday(1))))
        rule.launchApp()
        rule.openLoopFor(TYPED_SQUAT)
        rule.tapEditorPrimary()
        rule.waitForText(strPrefix(R.string.log_set_header_n_of_n, 2), substring = true)
        rule.waitForNode(hasContentDescription(strPrefix(R.string.log_rest_left_cd), substring = true))
        rule.waitForNode(hasText(str(R.string.log_undo)))
        rule.waitForState { todaysSets(TYPED_SQUAT).isNotEmpty() }
        rule.holdClockInRest(restStartMillis(), secondsIn = 25)
        val tag = if (fontScale != null) "fs130-" else ""
        rule.shot("log-setloop-rest-$tag${mode.suffix}")
    }

    @Test fun setLoopRestLight() = setLoopRest(DarkMode.Light)

    @Test fun setLoopRestDark() = setLoopRest(DarkMode.Dark)

    @Test fun setLoopRestFontScale130Light() = setLoopRest(DarkMode.Light, fontScale = 1.3f)

    /** "Done for now" shows once a set is logged; editing after that save marks
     *  the editor dirty, which is what makes it ask. */
    private fun finishConfirm(mode: DarkMode) {
        useDarkMode(mode)
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), squat(QaHarness.daysAgoAtMidday(1))))
        rule.launchApp()
        rule.openLoopFor(TYPED_SQUAT)
        rule.tapEditorPrimary()
        rule.waitForText(strPrefix(R.string.log_set_header_n_of_n, 2), substring = true)
        rule.waitForState { todaysSets(TYPED_SQUAT).isNotEmpty() }
        // The ± chip's "+2.5" is a figure, not a string resource.
        rule.onNodeWithText("+2.5").performClick()
        rule.onNodeWithText(str(R.string.log_set_done)).performClick()
        rule.waitForText(str(R.string.log_finish_unsaved_title))
        // The loop's rest readout shows under the dialog: hold it in the rest's first
        // second, whatever time the steps above took.
        rule.holdClockInRest(restStartMillis(), secondsIn = 0)
        rule.waitForNode(hasContentDescription(str(R.string.log_rest_left_cd, "3:00")))
        rule.shot("log-finish-unsaved-${mode.suffix}")
    }

    @Test fun finishConfirmLight() = finishConfirm(DarkMode.Light)

    @Test fun finishConfirmDark() = finishConfirm(DarkMode.Dark)

    private fun weeklyCheckin(mode: DarkMode) {
        useDarkMode(mode)
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        rule.openLogChooser()
        rule.onNodeWithText(str(R.string.main_weekly_checkin_title)).performScrollTo().performClick()
        rule.waitForText(str(R.string.log_weekly_signs_q))
        rule.shot("log-weekly-top-${mode.suffix}")
        rule.onNodeWithText(str(R.string.log_weekly_sign_sleep)).performScrollTo().performClick()
        rule.onNodeWithText(str(R.string.log_weekly_sign_mood)).performScrollTo().performClick()
        val (minWeeks, nforWeeks) = overtrainingWeekEdgeLabels()
        rule.onNodeWithText(str(R.string.log_weekly_signs_1to2, minWeeks, nforWeeks)).performScrollTo().performClick()
        rule.onNodeWithText(str(R.string.log_weekly_signs_gt2, nforWeeks)).performScrollTo()
        rule.shot("log-weekly-mid-${mode.suffix}")
        rule.onNode(hasText(str(R.string.log_clinical_reds_ctx), substring = true)).performScrollTo()
        rule.shot("log-weekly-bottom-${mode.suffix}")
    }

    @Test fun weeklyCheckinLight() = weeklyCheckin(DarkMode.Light)

    @Test fun weeklyCheckinDark() = weeklyCheckin(DarkMode.Dark)

    /** An active head cold puts the check-in's Illness item at the top. */
    private fun morningCheckin(mode: DarkMode) {
        useDarkMode(mode)
        QaHarness.seedLog(
            ctx,
            listOf(QaHarness.strengthProfile(), Event.SubmitReadiness(ReadinessSignal.Illness, 1.0, thisMorning)),
        )
        rule.launchApp()
        rule.openMorningCheckin()
        rule.waitForText(str(R.string.log_illness_q))
        rule.shot("log-checkin-illness-${mode.suffix}")
    }

    @Test fun morningCheckinLight() = morningCheckin(DarkMode.Light)

    @Test fun morningCheckinDark() = morningCheckin(DarkMode.Dark)

    /** The chooser scrolls only a few dp past the viewport, so it has a top and a
     *  true-end capture, no separate mid. */
    private fun chooserCalis(mode: DarkMode) {
        useDarkMode(mode)
        QaHarness.seedLog(ctx, listOf(QaHarness.calisthenicsProfile()))
        rule.launchApp()
        rule.openLogChooser()
        rule.shot("log-chooser-calis-top-${mode.suffix}")
        rule.onNode(hasText(str(R.string.log_chooser_more)) and hasClickAction()).performScrollTo().performClick()
        rule.onNode(hasText(str(R.string.main_advanced_signal_title)) and hasClickAction()).performScrollTo()
        rule.shot("log-chooser-calis-bottom-${mode.suffix}")
    }

    @Test fun chooserCalisLight() = chooserCalis(DarkMode.Light)

    @Test fun chooserCalisDark() = chooserCalis(DarkMode.Dark)
}
