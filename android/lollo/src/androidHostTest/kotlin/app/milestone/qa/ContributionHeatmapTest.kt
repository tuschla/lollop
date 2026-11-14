package app.milestone.qa

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.captureToImage
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
import app.milestone.DarkMode
import app.milestone.DayActivity
import app.milestone.JvmDefaultsRule
import app.milestone.LolloTheme
import app.milestone.MilestoneTheme
import app.milestone.R
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

@RunWith(RobolectricTestRunner::class)
class ContributionHeatmapTest {

    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val jvmDefaults = JvmDefaultsRule()

    /** The "Both" cell splits on the diagonal only under an ink accent (Paper without
     *  the system accent), where the lift grey and the run ink sit too close; under a
     *  hued accent it splits side by side. Read off the drawn cell, light and dark. */
    @Test
    fun diagonalBoth_onlyUnderTheInkAccent() {
        var look by mutableStateOf(AppTheme.Paper to DarkMode.Light)
        rule.setContent {
            LolloTheme(look.first, look.second, dynamicColor = false) {
                Box(Modifier.width(300.dp)) {
                    ContributionHeatmap(
                        days = mapOf(oldestMonday to DayActivity(lifted = true, runs = 1)),
                        todayLocalDay = tapToday,
                        weeks = 2,
                        onDayClick = {},
                    )
                }
            }
        }
        // Down the cell's left third: side by side that is the lift half from top to
        // bottom; on the diagonal the lift triangle (upper left) gives way to the run
        // one (lower right).
        fun splitsOnTheDiagonal(): Boolean {
            rule.waitForIdle()
            val cell = rule.onNode(SemanticsMatcher.keyIsDefined(SemanticsActions.CustomActions)).captureToImage().toPixelMap()
            val x = cell.width * 3 / 10
            return cell[x, cell.height / 10] != cell[x, cell.height * 9 / 10]
        }
        for (mode in listOf(DarkMode.Light, DarkMode.Dark)) {
            for (theme in AppTheme.entries) {
                look = theme to mode
                assertEquals("$theme ${mode.suffix}: the Both cell splits on the diagonal", theme == AppTheme.Paper, splitsOnTheDiagonal())
            }
        }
    }

    @Test
    fun grid_fillsTheContentWidth() {
        Locale.setDefault(Locale.US)
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

    // 2024-10-06 is a Sunday; with weeks = 2 the grid covers Mon 2024-09-23 .. Sun 2024-10-06.
    private val tapToday = 20_002L
    private val oldestMonday = 19_989L

    /** Taps the one logged cell (the oldest Monday) under [direction]; returns the opened day. */
    private fun tapOnlyLoggedCell(direction: LayoutDirection): Long? {
        Locale.setDefault(Locale.US)
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
