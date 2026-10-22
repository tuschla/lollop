package app.milestone.qa

import android.content.Context
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasScrollToIndexAction
import androidx.compose.ui.test.hasScrollToNodeAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeUp
import androidx.test.core.app.ApplicationProvider
import app.milestone.R
import org.junit.After
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * The calculators live on Profile, not Today: Today never renders the calculator
 * rows (checked at every scroll position, since the lazy list only composes what
 * is near the viewport), and Profile's CALCULATORS card opens the tool's sheet.
 */
@RunWith(RobolectricTestRunner::class)
class ProfileCalculatorsTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @Before
    fun setUp() {
        java.util.Locale.setDefault(java.util.Locale.US)
        QaHarness.reset(ctx)
    }

    @After
    fun tearDown() = QaHarness.teardown()

    @Test
    fun calculatorsLiveOnProfileNotToday() {
        // A lifting profile, so the Volume planner row is gated on.
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        val volume = str(R.string.main_tool_volume_title)

        // Today: no Volume planner row anywhere down the list.
        repeat(10) {
            rule.onAllNodes(hasText(volume)).assertCountEquals(0)
            rule.onAllNodes(hasScrollToIndexAction())[0].performTouchInput { swipeUp() }
            rule.waitForIdle()
        }
        rule.onAllNodes(hasText(volume)).assertCountEquals(0)

        // Profile: the row is there, and tapping it opens the Volume planner sheet
        // (its form's "Not growing" switch is a node only the sheet renders).
        rule.selectTab(str(R.string.main_dest_profile))
        val row = hasText(volume) and hasClickAction()
        rule.onAllNodes(hasScrollToNodeAction()).onFirst().performScrollToNode(row)
        rule.waitForIdle()
        val formNode = hasText(str(R.string.log_switch_not_growing))
        rule.onAllNodes(formNode).assertCountEquals(0)
        rule.onAllNodes(row).onFirst().performClick()
        rule.waitUntil(10_000) { rule.onAllNodes(formNode).fetchSemanticsNodes().isNotEmpty() }
    }
}
