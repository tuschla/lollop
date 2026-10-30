package app.milestone.qa

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.click
import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import app.milestone.AppTheme
import app.milestone.ContributionHeatmap
import app.milestone.DayActivity
import app.milestone.MilestoneTheme
import app.milestone.R
import app.milestone.isAchromatic
import app.milestone.spacedMonthTicks
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * History's activity heatmap: square cells sized from the available width, tints,
 * month ticks, and a tap that opens the logged day under the finger.
 */
@RunWith(RobolectricTestRunner::class)
class ContributionHeatmapTest {

    @get:Rule
    val rule = createComposeRule()

    /** The "Both" cell splits on the diagonal only under an ink accent (Paper without
     *  the system accent), where the lift grey and the run ink sit too close. */
    @Test
    fun diagonalBoth_onlyUnderTheInkAccent() {
        AppTheme.Paper.let { assertTrue(it.light.accent.isAchromatic() && it.dark.accent.isAchromatic()) }
        listOf(AppTheme.Rose, AppTheme.Meadow, AppTheme.Iris).forEach {
            assertFalse("${it.name} keeps the side-by-side split", it.light.accent.isAchromatic() || it.dark.accent.isAchromatic())
        }
    }

    @Test
    fun grid_fillsTheContentWidth() {
        java.util.Locale.setDefault(java.util.Locale.US)
        val today = 20_000L
        rule.setContent {
            MilestoneTheme {
                Box(Modifier.fillMaxWidth().testTag("host")) {
                    ContributionHeatmap(
                        mapOf(today to DayActivity(lifted = false, runs = 1), today - 3 to DayActivity(lifted = true, runs = 1)),
                        today,
                        weeks = 12,
                    )
                }
            }
        }
        val host = rule.onNodeWithTag("host").getBoundsInRoot()
        val grid = rule.onNode(hasContentDescription(strPrefix(R.string.history_heatmap_desc, 12), substring = true)).getBoundsInRoot()
        val slack = (host.right - host.left) - (grid.right - grid.left)
        // Only the whole-pixel flooring of 12 equal columns (< 1px each) may be
        // left over.
        val maxSlack = with(rule.density) { 12.toDp() }
        assertTrue("heatmap leaves $slack unused at the right (max $maxSlack)", slack <= maxSlack)
    }

    @Test
    fun aLeadingPartialMonthGivesWayWhenItsLabelWouldTouchTheNext() {
        val ticks = listOf("Jul", "Aug", "", "", "", "Sep", "", "", "", "Oct", "", "")
        // Large font: "Jul" (61 px) + a word space (21 px) overruns one 79 px column.
        assertEquals(
            listOf("", "Aug", "", "", "", "Sep", "", "", "", "Oct", "", ""),
            spacedMonthTicks(ticks, listOf(61, 64, 0, 0, 0, 62, 0, 0, 0, 60, 0, 0), pitchPx = 79, minGapPx = 21),
        )
        // Default font: it fits, so every month keeps its tick.
        assertEquals(ticks, spacedMonthTicks(ticks, listOf(47, 50, 0, 0, 0, 48, 0, 0, 0, 46, 0, 0), pitchPx = 79, minGapPx = 21))
    }

    // 2024-10-06 is a Sunday; with weeks = 2 the grid covers Mon 2024-09-23 .. Sun 2024-10-06.
    private val tapToday = 20_002L
    private val oldestMonday = 19_989L

    /** Taps the one logged cell (the oldest Monday) under [direction]; returns the opened day. */
    private fun tapOnlyLoggedCell(direction: LayoutDirection): Long? {
        java.util.Locale.setDefault(java.util.Locale.US)
        var clicked: Long? = null
        rule.setContent {
            MilestoneTheme {
                CompositionLocalProvider(LocalLayoutDirection provides direction) {
                    Box(Modifier.width(300.dp)) {
                        ContributionHeatmap(
                            days = mapOf(oldestMonday to DayActivity(lifted = true, runs = 0)),
                            todayLocalDay = tapToday,
                            weeks = 2,
                            onDayClick = { clicked = it },
                        )
                    }
                }
            }
        }
        // The logged cell is the only node carrying the "Open this day" action.
        rule.onNode(SemanticsMatcher.keyIsDefined(SemanticsActions.CustomActions)).performTouchInput { click() }
        rule.waitForIdle()
        return clicked
    }

    @Test
    fun tap_opensTheDayUnderTheFinger() {
        assertEquals(oldestMonday, tapOnlyLoggedCell(LayoutDirection.Ltr))
    }

    /** The grid mirrors in RTL (oldest week at the right); the tap maps the same way. */
    @Test
    fun tap_opensTheDayUnderTheFinger_underRtl() {
        assertEquals(oldestMonday, tapOnlyLoggedCell(LayoutDirection.Rtl))
    }
}
