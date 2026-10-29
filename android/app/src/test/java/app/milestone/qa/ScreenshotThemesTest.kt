package app.milestone.qa

import android.content.Context
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.performClick
import androidx.test.core.app.ApplicationProvider
import app.milestone.AppSettings
import app.milestone.AppTheme
import app.milestone.DarkMode
import app.milestone.Event
import app.milestone.R
import java.util.Locale
import org.junit.After
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.ParameterizedRobolectricTestRunner

/**
 * Screenshot baselines of every [AppTheme] in light and dark, from the REAL app
 * over the real core (same recording flow as [ScreenshotTest]): Today, History
 * and the Log sheet's chooser, for a strength lifter with two weeks of lifts and
 * a couple of runs so every screen carries content. Named
 * `theme-<theme>-<today|history|logsheet>-<light|dark>`.
 */
@RunWith(ParameterizedRobolectricTestRunner::class)
class ScreenshotThemesTest(private val theme: AppTheme, private val mode: DarkMode) {

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

    private fun seedHistory() {
        val lifts = listOf(1, 3, 5, 8, 10, 12).flatMap { day ->
            val at = QaHarness.daysAgoAtMidday(day)
            listOf(
                Event.LogSet("Back squat", 115.0 + (12 - day), 3, 8.0, at, at * 1000),
                Event.LogSet("Bench press", 70.0, 5, 7.5, at + 600, at * 1000 + 1),
            )
        }
        val runs = listOf(2, 6, 9).map { QaHarness.loggedRun(QaHarness.daysAgoAtMidday(it)) }
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()) + lifts + runs)
    }

    /** Today → History → back to Today with the Log chooser open, one capture each. */
    @Test
    fun capture() {
        AppSettings.setTheme(ctx, theme)
        useDarkMode(mode)
        seedHistory()
        rule.launchApp()
        val name = "theme-${theme.name.lowercase()}"
        val s = mode.suffix

        rule.shot("$name-today-$s")

        rule.selectTab(str(R.string.main_dest_history))
        rule.shot("$name-history-$s")

        // The bottom-nav item (a Tab), not any "Today" text on the History screen.
        rule.onNode(SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Tab) and hasText(str(R.string.main_dest_today)))
            .performClick()
        rule.waitForIdle()
        rule.onNode(hasClickAction() and hasContentDescription(str(R.string.main_log))).performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText(str(R.string.main_log_set_title)).fetchSemanticsNodes().isNotEmpty() }
        rule.shot("$name-logsheet-$s")
    }

    companion object {
        /** Every theme × light and dark. */
        @JvmStatic
        @ParameterizedRobolectricTestRunner.Parameters(name = "{0}-{1}")
        fun themes(): List<Array<Any>> =
            AppTheme.entries.flatMap { t -> listOf(DarkMode.Light, DarkMode.Dark).map { arrayOf<Any>(t, it) } }
    }
}
