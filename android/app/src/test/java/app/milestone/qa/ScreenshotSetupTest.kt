package app.milestone.qa

import android.content.Context
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasScrollToNodeAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.unit.dp
import androidx.test.core.app.ApplicationProvider
import app.milestone.CalisEquipment
import app.milestone.ConcurrentGoal
import app.milestone.DarkMode
import app.milestone.Event
import app.milestone.GoalDistance
import app.milestone.HealthScreen
import app.milestone.LiftGoal
import app.milestone.ParqItem
import app.milestone.ProgressionCadence
import app.milestone.R
import app.milestone.RecentRace
import app.milestone.ResistanceModality
import app.milestone.AppSettings
import app.milestone.todayEpochDay
import com.github.takahirom.roborazzi.captureScreenRoboImage
import java.util.Locale
import org.junit.After
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment

/**
 * Screenshot baselines for every guided-setup screen on the longest walk (Both +
 * calisthenics + a race goal, answered as [GuidedSetupWalk] answers it) and for
 * the Profile editor's coaching-input rows (hybrid hypertrophy runner,
 * calisthenics strength, pure runner), each in light + dark, plus Profile's
 * CALCULATORS card.
 *
 * Tall screens are captured at the top and again scrolled (`-mid` / `-bottom`)
 * so every element appears in at least one image. Recorded with
 * `./gradlew :app:recordRoborazziDebug` like [ScreenshotTest].
 */
@RunWith(RobolectricTestRunner::class)
class ScreenshotSetupTest {

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

    private fun theme(mode: DarkMode) = AppSettings.setDarkMode(ctx, mode)

    private val DarkMode.suffix get() = if (this == DarkMode.Dark) "dark" else "light"

    @OptIn(com.github.takahirom.roborazzi.ExperimentalRoborazziApi::class)
    private fun shot(name: String) =
        rule.captureWithoutRipples { captureScreenRoboImage("src/test/screenshots/$name.png") }

    // ── Scrolling ──────────────────────────────────────────────────────────

    private fun SemanticsNode.isVerticalScroller() =
        SemanticsActions.ScrollBy in config && SemanticsProperties.VerticalScrollAxisRange in config

    private fun byId(id: Int) = SemanticsMatcher("semantics id $id") { it.id == id }

    /** Scroll the vertical scroller holding the first [matcher] node so that node
     *  sits just under the scroller's top edge (layout position, not the clipped
     *  bounds, so a below-the-fold node measures its real offset). */
    private fun scrollToTop(matcher: SemanticsMatcher) {
        val target = rule.onAllNodes(matcher).onFirst().fetchSemanticsNode()
        var container = target.parent
        while (container != null && !container.isVerticalScroller()) container = container.parent
        checkNotNull(container) { "no vertical scroller above $matcher" }
        val margin = with(rule.density) { 16.dp.toPx() }
        val dy = target.positionInRoot.y - container.positionInRoot.y - margin
        rule.onNode(byId(container.id), useUnmergedTree = true)
            .performSemanticsAction(SemanticsActions.ScrollBy) { it(0f, dy) }
        rule.waitForIdle()
    }

    /** Scroll the screen's (single) vertical scroller to its start or its end. */
    private fun scrollScreen(toEnd: Boolean) {
        val scroller = rule.onAllNodes(SemanticsMatcher("vertical scroller") { it.isVerticalScroller() })
            .onFirst().fetchSemanticsNode()
        val range = scroller.config[SemanticsProperties.VerticalScrollAxisRange]
        val dy = (if (toEnd) range.maxValue() else 0f) - range.value()
        rule.onNode(byId(scroller.id), useUnmergedTree = true)
            .performSemanticsAction(SemanticsActions.ScrollBy) { it(0f, dy) }
        rule.waitForIdle()
    }

    // ── Guided setup (fresh install), Both + calisthenics + race ───────────

    /** Walk to [name], answer it as the walk does (so the captured screen shows its
     *  selected state and the follow-ups an answer reveals), then capture it: once
     *  for a short screen, or top / [mid] / bottom for a tall one. */
    private fun setupStep(mode: DarkMode, name: String, tall: Boolean, mid: SemanticsMatcher? = null) {
        theme(mode)
        rule.launchAppExpectingSetup()
        GuidedSetupWalk.walk(rule, stopAt = name)
        GuidedSetupWalk.bothPath.single { it.name == name }.answer(rule)
        rule.waitForIdle()
        // Each answer scrolls its option into view; start every capture set at the top.
        scrollScreen(toEnd = false)
        if (!tall) {
            shot("setup-step-$name-${mode.suffix}")
            return
        }
        shot("setup-step-$name-top-${mode.suffix}")
        if (mid != null) {
            scrollToTop(mid)
            shot("setup-step-$name-mid-${mode.suffix}")
        }
        scrollScreen(toEnd = true)
        shot("setup-step-$name-bottom-${mode.suffix}")
    }

    private val healthMid get() = hasText(str(R.string.setup_parq_q4))

    @Test fun setupTrainLight() = setupStep(DarkMode.Light, "train", tall = false)
    @Test fun setupTrainDark() = setupStep(DarkMode.Dark, "train", tall = false)

    @Test fun setupLiftGoalLight() = setupStep(DarkMode.Light, "lift-goal", tall = true)
    @Test fun setupLiftGoalDark() = setupStep(DarkMode.Dark, "lift-goal", tall = true)

    @Test fun setupPlacementLight() = setupStep(DarkMode.Light, "placement", tall = true)
    @Test fun setupPlacementDark() = setupStep(DarkMode.Dark, "placement", tall = true)

    @Test fun setupRunGoalLight() = setupStep(DarkMode.Light, "run-goal", tall = true)
    @Test fun setupRunGoalDark() = setupStep(DarkMode.Dark, "run-goal", tall = true)

    @Test fun setupRaceLight() = setupStep(DarkMode.Light, "race", tall = true)
    @Test fun setupRaceDark() = setupStep(DarkMode.Dark, "race", tall = true)

    @Test fun setupLeadLight() = setupStep(DarkMode.Light, "lead", tall = false)
    @Test fun setupLeadDark() = setupStep(DarkMode.Dark, "lead", tall = false)

    @Test fun setupDaysLight() = setupStep(DarkMode.Light, "days", tall = true)
    @Test fun setupDaysDark() = setupStep(DarkMode.Dark, "days", tall = true)

    @Test fun setupLevelLight() = setupStep(DarkMode.Light, "level", tall = true)
    @Test fun setupLevelDark() = setupStep(DarkMode.Dark, "level", tall = true)

    @Test fun setupRunNowLight() = setupStep(DarkMode.Light, "run-now", tall = true)
    @Test fun setupRunNowDark() = setupStep(DarkMode.Dark, "run-now", tall = true)

    @Test fun setupAboutLight() = setupStep(DarkMode.Light, "about", tall = true)
    @Test fun setupAboutDark() = setupStep(DarkMode.Dark, "about", tall = true)

    @Test fun setupHealthLight() = setupStep(DarkMode.Light, "health", tall = true, mid = healthMid)
    @Test fun setupHealthDark() = setupStep(DarkMode.Dark, "health", tall = true, mid = healthMid)

    @Test fun setupReviewLight() = setupStep(DarkMode.Light, "review", tall = true)
    @Test fun setupReviewDark() = setupStep(DarkMode.Dark, "review", tall = true)

    /** The densest wizard screen (the review) at 130 % font. */
    @Test
    fun setupReviewFontScale130() {
        RuntimeEnvironment.setFontScale(1.3f)
        theme(DarkMode.Light)
        rule.launchAppExpectingSetup()
        GuidedSetupWalk.walk(rule, stopAt = "review")
        shot("setup-step-review-fontscale130-light")
    }

    // ── Profile: coaching-input rows ───────────────────────────────────────

    private val today get() = todayEpochDay()

    /** Mon, Tue, Thu, Sat, Sun (bit0 = Monday). */
    private val fiveDays = 0b1101011

    /** Mon, Wed, Fri. */
    private val threeDays = 0b0010101

    /** Both, hypertrophy-led lifting + a half-marathon: lift days, priority muscle,
     *  training days, race date + recent race, longest run, one cleared PAR-Q+ Yes
     *  and a recent break with its restart date. */
    private fun hybridProfile(): Event = Event.SetProfile(
        progressionCadence = ProgressionCadence.WeekToWeek,
        liftGoal = LiftGoal.Hypertrophy,
        goalDistance = GoalDistance.HalfMarathon,
        concurrentGoal = ConcurrentGoal.Hypertrophy,
        weeklySets = 12,
        runningDaysPerWeek = 3,
        runningKmPerWeek = 30.0,
        advanced = false,
        bodyweightKg = 78.0,
        ageYears = 34.0,
        health = HealthScreen().withParqAnswer(ParqItem.Medication, true).copy(medically_cleared = true),
        weeksOff = 6.0,
        liftDaysPerWeek = 2,
        trainingWeekdays = fiveDays,
        goalRaceEpochDay = today + 84,
        recentRace = RecentRace(distance_km = 5.0, time_sec = 26 * 60 + 15),
        statedLongestRecentKm = 16.0,
        layoffReturnedEpochDay = today - 10,
        specializationMuscle = "glutes",
    )

    /** Lift-only calisthenics strength: meet date + equipment rows. */
    private fun calisProfile(): Event = Event.SetProfile(
        progressionCadence = ProgressionCadence.WeekToWeek,
        liftGoal = LiftGoal.MaxStrength,
        goalDistance = GoalDistance.General,
        concurrentGoal = ConcurrentGoal.Strength,
        weeklySets = 12,
        runningDaysPerWeek = 0,
        runningKmPerWeek = 0.0,
        advanced = false,
        bodyweightKg = 72.0,
        ageYears = 30.0,
        resistanceModality = ResistanceModality.Calisthenics,
        liftDaysPerWeek = 3,
        trainingWeekdays = threeDays,
        meetEpochDay = today + 56,
        calisEquipment = CalisEquipment(pull_up_bar = true, rings = true),
    )

    /** A pure 10K runner who opted into the two short strength sessions. */
    private fun runnerProfile(): Event = Event.SetProfile(
        progressionCadence = ProgressionCadence.WeekToWeek,
        liftGoal = LiftGoal.MaxStrength,
        goalDistance = GoalDistance.TenK,
        concurrentGoal = ConcurrentGoal.EndurancePriority,
        weeklySets = 0,
        runningDaysPerWeek = 4,
        runningKmPerWeek = 32.0,
        advanced = false,
        ageYears = 29.0,
        trainingWeekdays = 0b1010101,
        goalRaceEpochDay = today + 70,
        statedLongestRecentKm = 12.0,
        runnerStrength = true,
    )

    /** Profile for [profile], with "Your plan answers" unfolded and its header
     *  pinned under the top edge, so the coaching-input rows are on screen. */
    private fun openProfile(mode: DarkMode, profile: Event) {
        theme(mode)
        QaHarness.seedLog(ctx, listOf(profile))
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_profile))
        rule.openPlanAnswers()
        scrollToTop(hasText(str(R.string.main_overline_plan_answers).uppercase(Locale.US)))
    }

    /** A Profile tap-row by its label. */
    private fun row(labelId: Int) = hasText(str(labelId)) and hasClickAction()

    /** Bring a tap-row to the top and toggle its inline picker. */
    private fun toggleRow(labelId: Int) {
        scrollToTop(row(labelId))
        rule.onAllNodes(row(labelId)).onFirst().performClick()
        rule.waitForIdle()
    }

    /** Open [labelId]'s picker at the top of the screen, capture, close it again. */
    private fun openRowShot(labelId: Int, name: String) {
        toggleRow(labelId)
        shot(name)
        toggleRow(labelId)
    }

    private fun profileHybrid(mode: DarkMode) {
        val s = mode.suffix
        openProfile(mode, hybridProfile())
        // The plan answers header, the PLAN card (Focus Both, which leads, training
        // days, experience, re-run setup) and the top of the LIFTING card.
        shot("profile-hybrid-top-$s")
        // The one Training days control open: lift / run day scales + weekdays.
        openRowShot(R.string.setup_row_weekdays, "profile-hybrid-days-open-$s")
        // The LIFTING card (goal, modality, priority muscle, sets per muscle) and the
        // start of RUNNING (goal distance, race date, recent race, weekly distance).
        // Each group label renders as an uppercase accent overline.
        scrollToTop(hasText(str(R.string.setup_section_lifting).uppercase(Locale.US)))
        shot("profile-hybrid-mid-$s")
        // Longest run, YOU, HEALTH & SAFETY (health questions "1 applies").
        scrollToTop(row(R.string.setup_row_longest_run))
        shot("profile-hybrid-lower-$s")
        // TRAINING CONTEXT (environment, back from a break "Since …", high-load).
        scrollToTop(hasText(str(R.string.setup_section_context).uppercase(Locale.US)))
        shot("profile-hybrid-context-$s")
        openRowShot(R.string.setup_row_priority_muscle, "profile-hybrid-priority-muscle-open-$s")
        openRowShot(R.string.setup_row_recent_race, "profile-hybrid-recent-race-open-$s")
        // The seven PAR-Q+ questions (Q5 Yes + cleared), then the rest of them below.
        toggleRow(R.string.setup_row_health_questions)
        shot("profile-hybrid-health-open-top-$s")
        scrollToTop(hasText(str(R.string.setup_parq_q5)))
        shot("profile-hybrid-health-open-bottom-$s")
        toggleRow(R.string.setup_row_health_questions)
        // Weeks off scale + restart date.
        openRowShot(R.string.setup_row_break, "profile-hybrid-break-open-$s")
    }

    private fun profileCalis(mode: DarkMode) {
        val s = mode.suffix
        openProfile(mode, calisProfile())
        // Bodyweight modality, lift days, meet date, equipment, training days.
        shot("profile-calis-top-$s")
        openRowShot(R.string.setup_row_meet, "profile-calis-meet-open-$s")
        openRowShot(R.string.setup_row_equipment, "profile-calis-equipment-open-$s")
        openRowShot(R.string.setup_row_weekdays, "profile-calis-weekdays-open-$s")
    }

    private fun profileRunner(mode: DarkMode) {
        val s = mode.suffix
        openProfile(mode, runnerProfile())
        // Goal distance, race date, recent race (not set), running rows, longest
        // run, the runner-strength switch.
        scrollToTop(row(R.string.setup_row_goal_distance))
        shot("profile-runner-mid-$s")
    }

    @Test fun profileHybridLight() = profileHybrid(DarkMode.Light)
    @Test fun profileHybridDark() = profileHybrid(DarkMode.Dark)

    @Test fun profileCalisLight() = profileCalis(DarkMode.Light)
    @Test fun profileCalisDark() = profileCalis(DarkMode.Dark)

    @Test fun profileRunnerLight() = profileRunner(DarkMode.Light)
    @Test fun profileRunnerDark() = profileRunner(DarkMode.Dark)

    /** The CALCULATORS card (hybrid: all four tools), the first Profile section.
     *  [openProfile] leaves the list at the plan answers, so bring the section back
     *  into composition first, then pin its overline under the top edge. */
    private fun profileCalculators(mode: DarkMode) {
        openProfile(mode, hybridProfile())
        val overline = hasText(str(R.string.main_overline_calculators).uppercase(Locale.US))
        rule.onAllNodes(hasScrollToNodeAction()).onFirst().performScrollToNode(overline)
        rule.waitForIdle()
        scrollToTop(overline)
        shot("profile-calculators-${mode.suffix}")
    }

    @Test fun profileCalculatorsLight() = profileCalculators(DarkMode.Light)
    @Test fun profileCalculatorsDark() = profileCalculators(DarkMode.Dark)

    /** The densest Profile state (the expanded health questions) at 130 % font. */
    @Test
    fun profileHealthFontScale130() {
        RuntimeEnvironment.setFontScale(1.3f)
        openProfile(DarkMode.Light, hybridProfile())
        toggleRow(R.string.setup_row_health_questions)
        shot("profile-hybrid-health-open-fontscale130-light")
    }
}
