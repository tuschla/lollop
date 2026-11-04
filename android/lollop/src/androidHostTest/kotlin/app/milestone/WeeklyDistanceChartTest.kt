package app.milestone

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.click
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.unit.dp
import app.milestone.DistanceUnit
import app.milestone.MilestoneTheme
import app.milestone.RunWeekView
import app.milestone.WeeklyDistanceChart
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * [WeeklyDistanceChart] keeps the tapped week in `remember(weeks)`, so its tap
 * handler is keyed on the weeks too: when the weekly series changes with the same
 * week count (a run edited or deleted from History's sheet while the chart stays
 * composed), a tap still shows the "Week of …" readout.
 */
@RunWith(RobolectricTestRunner::class)
class WeeklyDistanceChartTest {

    @get:Rule
    val rule = createComposeRule()

    /** 16 Monday-start weeks (epoch day 20_004 is a Monday), each [kmAt] km. */
    private fun weeks(kmAt: (Int) -> Double) = (0 until 16).map { i ->
        RunWeekView(week_start_epoch_day = 20_004L + i * 7L, km = kmAt(i), runs = 1)
    }

    /** Taps the middle week's bar (its per-week node sits over the bar). */
    private fun tapMiddle() {
        rule.onAllNodes(hasContentDescription("Week of", substring = true))[8].performTouchInput { click(center) }
        rule.waitForIdle()
    }

    private fun readouts() = rule.onAllNodes(hasText("Week of", substring = true)).fetchSemanticsNodes().size

    /** Shows the chart for [first]; with [second], first taps the middle bar once
     *  (the gesture coroutine starts lazily on the first pointer event, so without
     *  this tap it would start after the swap, on the fresh state) and swaps in
     *  [second]. Then taps the middle bar and returns how many "Week of …" readouts
     *  are on screen (as text, not the per-week TalkBack nodes, which are content
     *  descriptions). */
    private fun tapReadouts(first: List<RunWeekView>, second: List<RunWeekView>?): Int {
        java.util.Locale.setDefault(java.util.Locale.US)
        val shown = mutableStateOf(first)
        rule.setContent {
            MilestoneTheme {
                Box(Modifier.width(320.dp)) {
                    WeeklyDistanceChart(shown.value, plannedKm = 0.0, unit = DistanceUnit.Km, chartHeight = 96.dp)
                }
            }
        }
        rule.waitForIdle()
        if (second != null) {
            tapMiddle()
            assertEquals("the first tap shows the readout", 1, readouts())
            rule.runOnIdle { shown.value = second }
            rule.waitForIdle()
            // remember(weeks) reset the selection: no readout until the next tap.
            assertEquals("the swap clears the selection", 0, readouts())
        }
        tapMiddle()
        return readouts()
    }

    @Test
    fun aTapShowsTheWeeksReadout() {
        assertEquals(1, tapReadouts(weeks { 10.0 + it }, second = null))
    }

    @Test
    fun aTapStillShowsTheReadoutAfterTheWeeksChange() {
        val before = weeks { 10.0 + it }
        val after = weeks { if (it == 15) 30.0 else 10.0 + it }
        assertEquals("the tapped week's \"Week of …\" readout", 1, tapReadouts(before, after))
    }
}
