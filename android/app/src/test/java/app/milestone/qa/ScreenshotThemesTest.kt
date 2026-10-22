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
import app.milestone.AppTheme
import app.milestone.DarkMode
import app.milestone.Event
import app.milestone.R
import app.milestone.AppSettings
import com.github.takahirom.roborazzi.captureScreenRoboImage
import java.util.Locale
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * Screenshot baselines of every [AppTheme] in light and dark, from the REAL app
 * over the real core (same recording flow as [ScreenshotTest]): Today, History
 * and the Log sheet's chooser, for a strength lifter with two weeks of lifts and
 * a couple of runs so every screen carries content. Named
 * `theme-<theme>-<today|history|logsheet>-<light|dark>`.
 */
@RunWith(RobolectricTestRunner::class)
class ScreenshotThemesTest {

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

    @OptIn(com.github.takahirom.roborazzi.ExperimentalRoborazziApi::class)
    private fun shot(name: String) =
        rule.captureWithoutRipples { captureScreenRoboImage("src/test/screenshots/$name.png") }

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
    private fun capture(theme: AppTheme, mode: DarkMode) {
        AppSettings.setTheme(ctx, theme)
        AppSettings.setDarkMode(ctx, mode)
        seedHistory()
        rule.launchApp()
        val name = "theme-${theme.name.lowercase()}"
        val s = if (mode == DarkMode.Dark) "dark" else "light"

        shot("$name-today-$s")

        rule.selectTab(str(R.string.main_dest_history))
        shot("$name-history-$s")

        // The bottom-nav item (a Tab), not any "Today" text on the History screen.
        rule.onNode(SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Tab) and hasText(str(R.string.main_dest_today)))
            .performClick()
        rule.waitForIdle()
        rule.onNode(hasClickAction() and hasContentDescription(str(R.string.main_log))).performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText(str(R.string.main_log_set_title)).fetchSemanticsNodes().isNotEmpty() }
        shot("$name-logsheet-$s")
    }

    @Test fun roseLight() = capture(AppTheme.Rose, DarkMode.Light)

    @Test fun roseDark() = capture(AppTheme.Rose, DarkMode.Dark)

    @Test fun meadowLight() = capture(AppTheme.Meadow, DarkMode.Light)

    @Test fun meadowDark() = capture(AppTheme.Meadow, DarkMode.Dark)

    @Test fun irisLight() = capture(AppTheme.Iris, DarkMode.Light)

    @Test fun irisDark() = capture(AppTheme.Iris, DarkMode.Dark)

    @Test fun paperLight() = capture(AppTheme.Paper, DarkMode.Light)

    @Test fun paperDark() = capture(AppTheme.Paper, DarkMode.Dark)

    /** Every theme has a light and a dark capture above; fails when a theme is
     *  added to [AppTheme] without its baselines. */
    @Test fun coversEveryTheme() {
        val covered = setOf(AppTheme.Rose, AppTheme.Meadow, AppTheme.Iris, AppTheme.Paper)
        assertEquals(AppTheme.entries.toSet(), covered)
    }
}
