package app.milestone.qa

import android.content.Context
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.core.app.ApplicationProvider
import app.milestone.R
import org.junit.After
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * Baseline scenario tests that drive the REAL app (real Compose UI over the real
 * Rust core, offline via Robolectric + `jni-host`) through the journeys the owner
 * takes daily, as a regression net; the per-set completion flows live in
 * [CompletionScenarioTest] and the generic UX rules in [UxInvariantTest].
 */
@RunWith(RobolectricTestRunner::class)
class ScenarioBaselineTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @Before
    fun setUp() {
        // Pin the locale so locale-derived UI (distance unit, number formats) is
        // deterministic across host/CI environments.
        java.util.Locale.setDefault(java.util.Locale.US)
        QaHarness.reset(ctx)
    }

    @After
    fun tearDown() = QaHarness.teardown()

    @Test
    fun freshInstall_guidedSetupIsReachable() {
        // No seeded log = genuine fresh install → the guided setup replaces the
        // scaffold (the whole first-run onboarding entry point).
        rule.launchAppExpectingSetup()
        // The first step's question is the screen's one title.
        rule.onAllNodesWithText(str(R.string.setup_step_train_q)).assertCountEquals(1)
        // Step 1 says where it is in the path, and its bar is already partly filled
        // (GuidedSetupDraftTest); there is no Back to show yet, not even a hidden one
        // a screen reader could reach.
        val count = app.milestone.setupSteps(null).size
        rule.onAllNodes(hasContentDescription(str(R.string.setup_step_progress, 1, count))).assertCountEquals(1)
        rule.onAllNodesWithText(str(R.string.setup_btn_back)).assertCountEquals(0)
    }

    @Test
    fun seededStrengthPlan_todayShowsHeroWithExercises() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        // The prescription hero (NextSessionCard) and its concrete exercises.
        rule.onAllNodesWithText(str(R.string.main_todays_session).uppercase(java.util.Locale.US)).assertCountEquals(1)
        rule.onAllNodesWithText("Back squat", substring = true).assertCountEquals(1)
    }

    @Test
    fun logSet_throughLogSheet_updatesHistory() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()

        // Log the first back squat set through the plan item's loop editor: type the
        // RIR set a load on the keypad (the ± chips don't step from an empty field),
        // log it from the pinned action, then "Done for now" closes the loop editor.
        rule.onNode(
            hasText(str(R.string.plan_log_set_n_of_n, 1, 3)) and hasAnyAncestor(hasTestTag("plan-item:Back squat")),
        ).performClick()
        rule.waitForIdle()
        rule.typeWeight("60")
        rule.onNodeWithTag("editor-primary").performClick()
        rule.assertTextCountEventually(str(R.string.log_set_header_n_of_n, 2, 3), 1)
        rule.onNodeWithText(str(R.string.log_set_done)).performClick()
        rule.waitForIdle()

        // Only lifts exist, so History auto-selects the Lifts list (the Lifts|Runs
        // toggle only appears when a run modality is also relevant).
        rule.selectTab(str(R.string.main_dest_history))
        rule.scrollToText("Back squat")
        rule.onNodeWithText("Back squat", substring = true).assertIsDisplayed()
    }

    @Test
    fun history_rendersSeededHoldSet() {
        val at = QaHarness.daysAgoAtMidday(1)
        QaHarness.seedLog(
            ctx,
            listOf(QaHarness.calisthenicsProfile(), QaHarness.loggedHold(at)),
        )
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_history))
        // The hold set renders "1 × 30 s hold" in History (hold-specific, and never
        // a fabricated "@ RPE 0" / "0 kg e1RM").
        val hold = str(R.string.history_sets_hold, 1, 30)
        rule.scrollToText(hold)
        rule.onAllNodesWithText(hold, substring = true).assertCountEquals(1)
    }

    @Test
    fun runHistory_renders() {
        val at = QaHarness.daysAgoAtMidday(1)
        QaHarness.seedLog(
            ctx,
            listOf(QaHarness.strengthProfile(), QaHarness.loggedRun(at)),
        )
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_history))
        // The run modality is present, so the Runs list is reachable; RunCard
        // renders the distance in the locale's unit (mi under en-US).
        val label = QaHarness.expectedRunDistanceLabel(5.0)
        rule.scrollToText(label)
        rule.onNodeWithText(label, substring = true).assertIsDisplayed()
    }
}
