package app.milestone.qa

import android.content.Context
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.test.core.app.ApplicationProvider
import app.milestone.AppSettings
import app.milestone.AppTheme
import app.milestone.DarkMode
import app.milestone.DistanceUnitOverride
import app.milestone.Event
import app.milestone.GpsPoint
import app.milestone.R
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
 * The run detail sheet's route surfaces in every theme, light and dark, with map
 * tiles off (the plain-card route; Robolectric has no tiles): the top of the sheet
 * (stats + route card with its start/finish markers, labels and numbered miles on a
 * 3.18 mi loop) and the trim sheet (token slider, dashed trimmed ends).
 */
@RunWith(RobolectricTestRunner::class)
class ScreenshotRouteThemesTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @Before
    fun setUp() {
        Locale.setDefault(Locale.US)
        QaHarness.reset(ctx)
        AppSettings.setShowRouteMaps(ctx, false)
        AppSettings.setDistanceUnitOverride(ctx, DistanceUnitOverride.Mi)
    }

    @After
    fun tearDown() {
        QaHarness.teardown()
        AppSettings.showRouteMaps.value = true
        AppSettings.distanceUnitOverride.value = DistanceUnitOverride.System
    }

    @OptIn(com.github.takahirom.roborazzi.ExperimentalRoborazziApi::class)
    private fun shot(name: String) =
        rule.captureWithoutRipples { captureScreenRoboImage("src/test/screenshots/$name.png") }

    private fun waitFor(matcher: SemanticsMatcher) {
        rule.waitUntil(10_000) { rule.onAllNodes(matcher).fetchSemanticsNodes().isNotEmpty() }
        rule.waitForIdle()
    }

    private fun capture(theme: AppTheme, mode: DarkMode) {
        AppSettings.setTheme(ctx, theme)
        AppSettings.setDarkMode(ctx, mode)
        // Each theme's own accent (picking Paper turns the system accent on).
        AppSettings.setDynamicAccent(ctx, false)
        val at = QaHarness.daysAgoAtMidday(1)
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), loopRun(at)))
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_history))
        val title = str(R.string.history_run_weekday, "Sunday")
        rule.scrollToText(title)
        rule.onNodeWithText(title).performClick()
        waitFor(hasTestTag("run-trim"))
        val name = "route-${theme.name.lowercase()}"
        val s = if (mode == DarkMode.Dark) "dark" else "light"
        shot("$name-detail-$s")
        rule.onNode(hasContentDescription(str(R.string.runmap_open_full)) and hasClickAction()).performClick()
        waitFor(hasContentDescription(str(R.string.runmap_fit_route)))
        shot("$name-full-$s")
        rule.onNode(hasContentDescription(str(R.string.runmap_close)) and hasClickAction()).performClick()
        rule.waitForIdle()
        rule.onNodeWithTag("run-trim").performScrollTo().performClick()
        waitFor(hasText(str(R.string.trim_hint)))
        shot("$name-trim-$s")
    }

    @Test fun roseLight() = capture(AppTheme.Rose, DarkMode.Light)

    @Test fun roseDark() = capture(AppTheme.Rose, DarkMode.Dark)

    @Test fun meadowLight() = capture(AppTheme.Meadow, DarkMode.Light)

    @Test fun meadowDark() = capture(AppTheme.Meadow, DarkMode.Dark)

    @Test fun irisLight() = capture(AppTheme.Iris, DarkMode.Light)

    @Test fun irisDark() = capture(AppTheme.Iris, DarkMode.Dark)

    @Test fun paperLight() = capture(AppTheme.Paper, DarkMode.Light)

    @Test fun paperDark() = capture(AppTheme.Paper, DarkMode.Dark)

    @Test fun coversEveryTheme() {
        assertEquals(AppTheme.entries.toSet(), setOf(AppTheme.Rose, AppTheme.Meadow, AppTheme.Iris, AppTheme.Paper))
    }

    companion object {
        /** A 5.12 km (3.18 mi) rectangular loop, 1600 m east-west by 960 m north-south,
         *  run clockwise from the south-west corner: one clean fix every 3 s, 10 m apart. */
        fun loopRun(observedAt: Long): Event {
            val mLat = 111_195.0
            val mLon = mLat * Math.cos(Math.toRadians(52.5))
            val legs = listOf(0.0 to 96, 1.0 to 160, 2.0 to 96, 3.0 to 160)
            var north = 0.0
            var east = 0.0
            val points = ArrayList<GpsPoint>()
            var t = observedAt
            points += GpsPoint(52.5 + north / mLat, 13.35 + east / mLon, t, 5.0)
            for ((dir, steps) in legs) {
                repeat(steps) {
                    when (dir) {
                        0.0 -> north += 10.0
                        1.0 -> east += 10.0
                        2.0 -> north -= 10.0
                        else -> east -= 10.0
                    }
                    t += 3
                    points += GpsPoint(52.5 + north / mLat, 13.35 + east / mLon, t, 5.0)
                }
            }
            return Event.LogRunTrack(
                points = points,
                hrPctMax = 0.0,
                longestRecentKm = 0.0,
                observedAt = observedAt,
                entryId = observedAt * 1000,
            )
        }
    }
}
