package app.milestone.qa

import android.content.Context
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasScrollToNodeAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.unit.dp
import androidx.test.core.app.ApplicationProvider
import app.milestone.DistanceUnitOverride
import app.milestone.Event
import app.milestone.EventDispatcher
import app.milestone.R
import app.milestone.AppSettings
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * History renders logged training the way it was trained: one card per lifting
 * SESSION (a local day), not one per set, and every distance in the user's own
 * unit. Drives the real app over the real core (see [ScenarioBaselineTest]).
 */
@RunWith(RobolectricTestRunner::class)
class HistorySessionTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @Before
    fun setUp() {
        java.util.Locale.setDefault(java.util.Locale.US)
        QaHarness.reset(ctx)
    }

    @After
    fun tearDown() {
        AppSettings.distanceUnitOverride.value = DistanceUnitOverride.System
        QaHarness.teardown()
    }

    @Test
    fun sixSetsOfTwoExercisesOnOneDay_renderAsOneSessionCard() {
        val at = QaHarness.daysAgoAtMidday(1)
        val squats = (0 until 3).map { i ->
            val t = at + i * 180L
            Event.LogSet("Back Squat", 100.0, 5, 8.0, t, t * 1000)
        }
        val bench = (0 until 3).map { i ->
            val t = at + 900L + i * 180L
            Event.LogSet("Bench Press", 70.0, 8, 0.0, t, t * 1000)
        }
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()) + squats + bench)
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_history))
        rule.scrollToText(plural(R.plurals.history_set_count, 6, 6))

        // ONE session card for the day: one header, each exercise named once with
        // its sets folded into a single line.
        val header = plural(R.plurals.history_exercise_count, 2, 2) + " · " + plural(R.plurals.history_set_count, 6, 6)
        rule.onAllNodesWithText(header).assertCountEquals(1)
        rule.onAllNodesWithText("Back Squat").assertCountEquals(1)
        rule.onAllNodesWithText("Bench Press").assertCountEquals(1)
        fun folded(sets: Int, reps: Int, kg: String) =
            str(R.string.history_sets_reps_load, sets, reps, str(R.string.history_load, kg, "kg"))
        rule.onAllNodesWithText(folded(3, 5, "100")).assertCountEquals(1)
        rule.onAllNodesWithText(folded(3, 8, "70")).assertCountEquals(1)

        // Tapping the card opens the session sheet: the day's volume in the subtitle,
        // one card per exercise (named in sentence case, never as an overline) with a
        // set table of 48dp rows, each openable for edit/delete, and the ActionRows
        // footer's single "Delete session" row.
        rule.onNodeWithText(header).performClick()
        rule.waitForIdle()
        val volume = str(R.string.history_session_volume, "3,180", "kg")
        rule.onAllNodesWithText("$header · $volume").assertCountEquals(1)
        rule.onAllNodesWithText("BACK SQUAT").assertCountEquals(0)
        rule.onAllNodesWithText("BENCH PRESS").assertCountEquals(0)
        rule.onAllNodes(hasContentDescription(str(R.string.history_set_n, 3) + ",", substring = true)).assertCountEquals(2)
        val rows = rule.onAllNodesWithTag("lift-set-row").fetchSemanticsNodes().map { it.boundsInRoot }
        assertEquals(6, rows.size)
        val rowPx = with(rule.density) { 48.dp.toPx() }
        rows.forEach { assertEquals("a set row is 48dp tall", rowPx, it.height, 1.5f) }
        // One column grid across both cards: every row starts and ends at the same x.
        assertTrue("set rows align across cards", rows.all { it.left == rows[0].left && it.right == rows[0].right })
        // Squats carry their logged effort as reps left; bench sets logged none.
        rule.onAllNodes(hasContentDescription(plural(R.plurals.log_reps_left, 2, 2), substring = true)).assertCountEquals(3)
        rule.onAllNodesWithText(str(R.string.history_delete_session)).assertCountEquals(1)
        // Deleting the session always confirms first.
        rule.onNodeWithTag("lift-session-delete").performScrollTo().performClick()
        rule.waitForIdle()
        rule.onAllNodesWithText(str(R.string.history_delete_session_title)).assertCountEquals(1)
    }

    @Test
    fun setDetail_showsTheSetLargeWithItsFactsAndConfirmsDelete() {
        val t = QaHarness.daysAgoAtMidday(1)
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), Event.LogSet("Deadlift", 100.0, 5, 8.0, t, t * 1000)))
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_history))
        val header = plural(R.plurals.history_exercise_count, 1, 1) + " · " + plural(R.plurals.history_set_count, 1, 1)
        rule.scrollToText(header)
        rule.onNodeWithText(header).performClick()
        rule.waitForIdle()
        rule.onAllNodesWithTag("lift-set-row")[0].performClick()
        rule.waitForIdle()

        // The set is the sheet's one large figure, its reps left a caption below it.
        val load = str(R.string.log_set_line_reps, str(R.string.log_value_with_unit, "100", "kg"), 5)
        rule.onAllNodesWithText(load).assertCountEquals(1)
        rule.onAllNodesWithText(plural(R.plurals.log_reps_left, 2, 2)).assertCountEquals(1)
        // Key/value facts: the load is a share of the ESTIMATED max.
        rule.onAllNodesWithText(str(R.string.history_set_e1rm_label)).assertCountEquals(1)
        rule.onAllNodesWithText("of e1RM", substring = true).assertCountEquals(1)
        // No filled slab: Edit and Delete are ActionRows, and Delete confirms first.
        rule.onAllNodesWithText(str(R.string.main_edit)).assertCountEquals(0)
        rule.onAllNodesWithText(str(R.string.history_edit_set)).assertCountEquals(1)
        rule.onNodeWithTag("set-detail-delete").performScrollTo().performClick()
        rule.waitForIdle()
        rule.onAllNodesWithText(str(R.string.main_delete_set_title)).assertCountEquals(1)
        assertEquals(1, EventDispatcher.model.value!!.lifts.size)
    }

    @Test
    fun jumpToMonth_isOneCalendarIconOnTheFirstMonthHeader() {
        val recent = QaHarness.daysAgoAtMidday(1)
        val older = QaHarness.daysAgoAtMidday(45)
        QaHarness.seedLog(
            ctx,
            listOf(
                QaHarness.strengthProfile(),
                Event.LogSet("Back Squat", 100.0, 5, 8.0, older, older * 1000),
                Event.LogSet("Back Squat", 100.0, 5, 8.0, recent, recent * 1000),
            ),
        )
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_history))
        rule.scrollToText(plural(R.plurals.history_set_count, 1, 1))
        // A 48dp icon button named "Jump to month", never a bold text link; one only.
        val jump = rule.onAllNodesWithTag("history-jump-to-month").fetchSemanticsNodes()
        assertEquals(1, jump.size)
        val px48 = with(rule.density) { 48.dp.toPx() }
        assertEquals(px48, jump[0].boundsInRoot.width, 1.5f)
        assertEquals(px48, jump[0].boundsInRoot.height, 1.5f)
        rule.onAllNodes(hasContentDescription(str(R.string.main_jump_to_month))).assertCountEquals(1)
        rule.onAllNodesWithText(str(R.string.main_jump_to_month)).assertCountEquals(0)
        // It opens the month list in place.
        rule.onNodeWithTag("history-jump-to-month").performClick()
        rule.waitForIdle()
        val months = rule.onAllNodes(hasClickAction() and hasText(" 20", substring = true)).fetchSemanticsNodes()
        assertTrue("the picker lists both months", months.size >= 2)
    }

    @Test
    fun milesUser_seesMilesInTheHistoryTileAndChart() {
        AppSettings.setDistanceUnitOverride(ctx, DistanceUnitOverride.Mi)
        QaHarness.seedLog(
            ctx,
            listOf(QaHarness.strengthProfile(), QaHarness.loggedRun(QaHarness.daysAgoAtMidday(1))),
        )
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_history))

        // Rolling-7-day running tile: 5 km reads as 3.1 mi, never "5.0 km" (the
        // figure and its unit suffix are separate, resource-free texts).
        rule.onAllNodesWithText("3.1").assertCountEquals(1)
        rule.onAllNodesWithText(" mi").assertCountEquals(1)
        rule.onAllNodesWithText(" km").assertCountEquals(0)

        // The progress block opens on the 16-week chart (Runs is the default
        // segment: the run is the newest entry); it speaks miles, never km.
        val weekly = hasContentDescription(strPrefix(R.string.history_weekly_desc, 16), substring = true)
        rule.onNode(hasScrollToNodeAction()).performScrollToNode(weekly)
        val spoken = rule.onNode(weekly).fetchSemanticsNode()
            .config[SemanticsProperties.ContentDescription].joinToString(" ")
        assertTrue(spoken, spoken.contains("3.1 mi"))
        assertFalse(spoken, spoken.contains("km"))

        // Switched to "14 days", the daily chart totals in miles too.
        rule.onNode(hasText(str(R.string.history_progress_days, 14)) and hasClickAction()).performClick()
        rule.waitForIdle()
        rule.onAllNodesWithText(str(R.string.history_bars_total, "3.1", "mi")).assertCountEquals(1)

        // A run without HR shows no dead "HR -" row on its card (a retired row, so
        // there is no resource to resolve it from).
        rule.scrollToText(QaHarness.expectedRunDistanceLabel(5.0))
        rule.onAllNodesWithText("HR -", substring = true).assertCountEquals(0)
    }
}
