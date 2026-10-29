package app.milestone.qa

import android.content.Context
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.test.core.app.ApplicationProvider
import app.milestone.R
import org.junit.After
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * The fresh-install entry point over the REAL app (real Compose UI over the real
 * Rust core, offline via Robolectric + `jni-host`); the daily journeys live in
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
}
