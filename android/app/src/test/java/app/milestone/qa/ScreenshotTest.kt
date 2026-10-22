package app.milestone.qa

import android.content.Context
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.test.core.app.ApplicationProvider
import app.milestone.DarkMode
import app.milestone.R
import app.milestone.AppSettings
import com.github.takahirom.roborazzi.captureScreenRoboImage
import java.util.Locale
import org.junit.Before
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * Screenshot-regression baselines for the key screens in light + dark, captured
 * from the REAL app on the Robolectric host JVM.
 *
 *   ./gradlew :app:recordRoborazziDebug   # (re)record baselines after an
 *                                         # intentional UI change
 *   ./gradlew :app:verifyRoborazziDebug   # fail CI on any unexpected pixel diff
 *
 * Baselines live under src/test/screenshots/ (committed); diffs/actuals land in
 * build/outputs/roborazzi.
 */
@RunWith(RobolectricTestRunner::class)
class ScreenshotTest {

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

    @OptIn(com.github.takahirom.roborazzi.ExperimentalRoborazziApi::class)
    private fun shot(name: String) =
        rule.captureWithoutRipples { captureScreenRoboImage("src/test/screenshots/$name.png") }

    private val logButton get() = hasClickAction() and hasContentDescription(str(R.string.main_log))

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

    // ── Today ──────────────────────────────────────────────────────────────
    @Test fun todayLight() { theme(DarkMode.Light); openToday(); shot("today-light") }

    @Test fun todayDark() { theme(DarkMode.Dark); openToday(); shot("today-dark") }

    // ── History ────────────────────────────────────────────────────────────
    @Test fun historyLight() { theme(DarkMode.Light); openToday(); rule.selectTab(str(R.string.main_dest_history)); shot("history-light") }

    @Test fun historyDark() { theme(DarkMode.Dark); openToday(); rule.selectTab(str(R.string.main_dest_history)); shot("history-dark") }

    // ── Profile ────────────────────────────────────────────────────────────
    @Test fun profileLight() { theme(DarkMode.Light); openToday(); rule.selectTab(str(R.string.main_dest_profile)); shot("profile-light") }

    @Test fun profileDark() { theme(DarkMode.Dark); openToday(); rule.selectTab(str(R.string.main_dest_profile)); shot("profile-dark") }

    // ── Log chooser ────────────────────────────────────────────────────────
    @Test fun logChooserLight() { theme(DarkMode.Light); openToday(); openChooser(); shot("log-chooser-light") }

    @Test fun logChooserDark() { theme(DarkMode.Dark); openToday(); openChooser(); shot("log-chooser-dark") }

    // ── Set editor ─────────────────────────────────────────────────────────
    @Test fun setEditorLight() { theme(DarkMode.Light); openToday(); openEditor(); shot("set-editor-light") }

    @Test fun setEditorDark() { theme(DarkMode.Dark); openToday(); openEditor(); shot("set-editor-dark") }

    // A free set once its exercise is picked: weight, reps and effort appear together.
    @Test fun setEditorPickedLight() {
        theme(DarkMode.Light); openToday(); openEditor()
        rule.onNode(androidx.compose.ui.test.hasText("Back Squat") and androidx.compose.ui.test.isSelectable()).performClick()
        rule.waitForIdle()
        shot("set-editor-picked-light")
    }

    // ── Pain triage: severity as one 0–10 rating row ─────────────────────────
    @Test fun painTriageLight() {
        theme(DarkMode.Light); openToday(); openChooser()
        rule.onNodeWithText(str(R.string.main_report_pain)).performScrollTo().performClick()
        rule.waitForIdle()
        rule.pickRating("pain-severity", 6)
        shot("pain-triage-light")
    }

    // ── Per-set loop editor (opened from a plan item) ──────────────────────
    @Test fun setLoopLight() { theme(DarkMode.Light); openToday(); openLoop(); shot("set-loop-light") }

    @Test fun setLoopDark() { theme(DarkMode.Dark); openToday(); openLoop(); shot("set-loop-dark") }

    // ── Guided setup (fresh install), Both path ────────────────────────────
    @Test fun setupTrainLight() { theme(DarkMode.Light); openSetup("train"); shot("setup-train-light") }

    @Test fun setupTrainDark() { theme(DarkMode.Dark); openSetup("train"); shot("setup-train-dark") }

    @Test fun setupAboutLight() { theme(DarkMode.Light); openSetup("about"); shot("setup-about-light") }

    @Test fun setupAboutDark() { theme(DarkMode.Dark); openSetup("about"); shot("setup-about-dark") }

    @Test fun setupReviewLight() { theme(DarkMode.Light); openSetup("review"); shot("setup-review-light") }

    @Test fun setupReviewDark() { theme(DarkMode.Dark); openSetup("review"); shot("setup-review-dark") }

    private fun openSetup(screen: String) {
        rule.launchAppExpectingSetup()
        GuidedSetupWalk.walk(rule, stopAt = screen)
    }

    private fun openChooser() {
        rule.onNode(logButton).performClick()
        rule.waitForIdle()
        // Wait until the chooser's "Log set" tile is composed in the sheet.
        rule.waitUntil(5_000) {
            rule.onAllNodesWithText(str(R.string.main_log_set_title)).fetchSemanticsNodes().isNotEmpty()
        }
    }

    private fun openLoop() {
        rule.onAllNodesWithText(strPrefix(R.string.plan_log_set_n_of_n, 1), substring = true)[0].performClick()
        rule.waitUntil(5_000) {
            rule.onAllNodesWithText(strPrefix(R.string.log_set_header_n_of_n, 1), substring = true).fetchSemanticsNodes().isNotEmpty()
        }
    }

    private fun openEditor() {
        openChooser()
        rule.onNodeWithText(str(R.string.main_log_set_title)).performClick()
        rule.waitForIdle()
        rule.waitUntil(5_000) {
            rule.onAllNodesWithText(str(R.string.log_field_exercise)).fetchSemanticsNodes().isNotEmpty()
        }
    }
}
