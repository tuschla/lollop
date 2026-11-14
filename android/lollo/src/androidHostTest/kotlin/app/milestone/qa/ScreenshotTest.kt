package app.milestone.qa

import android.content.Context
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.test.core.app.ApplicationProvider
import app.milestone.DarkMode
import app.milestone.R
import org.junit.Before
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * Screenshot-regression baselines in light + dark, captured
 * from the REAL app on the Robolectric host JVM.
 *
 *   ./gradlew :lollo:recordRoborazziAndroidHostTest   # (re)record baselines after an
 *                                                      # intentional UI change
 *   ./gradlew :lollo:verifyRoborazziAndroidHostTest   # compare against the baselines
 *
 * Baselines live under src/test/screenshots/ (local, gitignored); diffs/actuals
 * land in build/outputs/roborazzi.
 */
@RunWith(RobolectricTestRunner::class)
class ScreenshotTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @Before
    fun setUp() {
        QaHarness.reset(ctx)
    }

    @After
    fun tearDown() = QaHarness.teardown()

    private fun openToday() {
        QaHarness.seedLog(
            ctx,
            listOf(
                QaHarness.strengthProfile(),
                QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(1)),
                QaHarness.loggedRun(QaHarness.daysAgoAtMidday(2)),
            ),
        )
        rule.launchApp()
    }

    // Today, History and the Log chooser in the default theme are the Rose captures
    // of [ScreenshotThemesTest]; the guided setup is [ScreenshotSetupTest].

    @Test fun profileLight() { useDarkMode(DarkMode.Light); openToday(); rule.selectTab(str(R.string.main_dest_profile)); rule.shot("profile-light") }

    @Test fun profileDark() { useDarkMode(DarkMode.Dark); openToday(); rule.selectTab(str(R.string.main_dest_profile)); rule.shot("profile-dark") }

    @Test fun setEditorLight() { useDarkMode(DarkMode.Light); openToday(); openEditor(); rule.shot("set-editor-light") }

    @Test fun setEditorDark() { useDarkMode(DarkMode.Dark); openToday(); openEditor(); rule.shot("set-editor-dark") }

    @Test fun painTriageLight() {
        useDarkMode(DarkMode.Light); openToday(); rule.openLogChooser()
        rule.onNodeWithText(str(R.string.main_report_pain)).performScrollTo().performClick()
        rule.waitForIdle()
        rule.pickRating("pain-severity", 6)
        rule.shot("pain-triage-light")
    }

    @Test fun setLoopLight() { useDarkMode(DarkMode.Light); openToday(); openLoop(); rule.shot("set-loop-light") }

    @Test fun setLoopDark() { useDarkMode(DarkMode.Dark); openToday(); openLoop(); rule.shot("set-loop-dark") }

    private fun openLoop() {
        rule.onAllNodesWithText(strPrefix(R.string.plan_log_set_n_of_n, 1), substring = true)[0].performClick()
        rule.waitUntil(5_000) {
            rule.onAllNodesWithText(strPrefix(R.string.log_set_header_n_of_n, 1), substring = true).fetchSemanticsNodes().isNotEmpty()
        }
    }

    private fun openEditor() {
        rule.openLogChooser()
        rule.onNodeWithText(str(R.string.main_log_set_title)).performClick()
        rule.waitForIdle()
        rule.waitUntil(5_000) {
            rule.onAllNodesWithText(str(R.string.log_field_exercise)).fetchSemanticsNodes().isNotEmpty()
        }
    }
}
