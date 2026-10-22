package app.milestone.qa

import android.content.Context
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.hasScrollToNodeAction
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.unit.dp
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performScrollToNode
import androidx.test.core.app.ApplicationProvider
import app.milestone.DistanceUnitOverride
import app.milestone.Event
import app.milestone.GpsPoint
import app.milestone.R
import app.milestone.AppSettings
import java.io.File
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment

/**
 * The Runs segment shows progress over time and lets a run be rated in place:
 * the 16-week weekly-distance chart (spoken in the user's unit), best efforts
 * with the run card's top record chip ("Fastest 10K"), the CR-10 rating that sends
 * AmendRunMeta, and the lift "New best" marker. Real app over the real core.
 */
@RunWith(RobolectricTestRunner::class)
class HistoryProgressTest {

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

    /** A 10.1 km GPS run at an even 5:00 /km: one clean fix every 3 s, 10 m apart, heading north. */
    private fun gpsTenK(observedAt: Long): Event {
        val metresPerDegLat = 111_195.0
        val fixes = 1011
        val points = (0 until fixes).map { i ->
            GpsPoint(lat = 47.0 + i * 10.0 / metresPerDegLat, lon = 8.0, observedAt = observedAt + i * 3L, accuracyM = 5.0)
        }
        return Event.LogRunTrack(
            points = points,
            hrPctMax = 0.0,
            longestRecentKm = 0.0,
            observedAt = observedAt,
            entryId = observedAt * 1000,
        )
    }

    private fun violations(vararg kinds: String, clipping: Boolean = false): List<UxInvariants.Violation> =
        UxInvariants.check(rule, rule.density, checkClipping = clipping).filter { it.kind in kinds }

    @Test
    fun gpsTenK_showsTopRecordChipAndBestEffortsRows() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), gpsTenK(QaHarness.daysAgoAtMidday(1))))
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_history))

        // Progress section (Runs is the default: the run is the newest entry): one
        // record cell per standard distance the run covered (1K and 5K measured inside
        // the longer run, the 10K as the whole run) beside the longest run, with no
        // engine sub-line. The distance labels and the record chip ("Fastest 10K")
        // are the core's own copy.
        // The records are always open: a carded 2×2 grid, no fold to tap.
        val records = str(R.string.history_records).uppercase(java.util.Locale.US)
        rule.scrollToText(records)
        rule.onAllNodesWithText("1K").assertCountEquals(1)
        rule.onAllNodesWithText("5K").assertCountEquals(1)
        rule.onAllNodesWithText("10K").assertCountEquals(1)
        rule.onAllNodesWithText("Part of a longer run").assertCountEquals(0)
        // Measured with the whole grid on screen (bounds are clipped to the viewport).
        rule.onAllNodesWithTag("history-record")[3].performScrollTo()
        rule.waitForIdle()
        val cells = rule.onAllNodesWithTag("history-record").fetchSemanticsNodes().map { it.boundsInRoot }
        assertEquals("longest run + 1K/5K/10K", 4, cells.size)
        // Two columns: cells pair up side by side, the second row below the first.
        assertEquals(cells[0].top, cells[1].top, 0.5f)
        assertTrue("column 2 sits right of column 1", cells[1].left >= cells[0].right - 0.5f)
        assertEquals(cells[0].left, cells[2].left, 0.5f)
        assertTrue("row 2 sits below row 1", cells[2].top >= cells[0].bottom - 0.5f)
        // Both columns are equally wide.
        assertEquals(cells[0].width, cells[1].width, 1.5f)
        // Every cell is a tappable target of at least 48dp.
        cells.forEach { assertTrue("record cell ≥48dp", it.height >= with(rule.density) { 48.dp.toPx() } - 0.5f) }

        // The run card carries only its most significant record as a chip.
        rule.scrollToText("Fastest 10K")
        rule.onAllNodesWithText("Fastest 10K").assertCountEquals(1)
        rule.onAllNodesWithText("Fastest 5K").assertCountEquals(0)
    }

    @Test
    fun ratingARunSeven_sendsAmendRunMetaAndCardShowsEffort() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), QaHarness.loggedRun(QaHarness.daysAgoAtMidday(1))))
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_history))
        // Unrated: no effort line on the face.
        rule.onAllNodesWithText(strPrefix(R.string.history_effort_value), substring = true).assertCountEquals(0)

        // Yesterday (Sunday) at midday, untagged → titled by its weekday.
        val title = str(R.string.history_run_weekday, "Sunday")
        rule.scrollToText(title)
        rule.onNodeWithText(title).performClick()
        rule.waitForIdle()
        // Unrated: YOUR LOG offers "Rate this run"; no rating grid lives open on the sheet.
        rule.onAllNodesWithTag("effort-scale").assertCountEquals(0)
        rule.onNodeWithTag("run-log-effort").performScrollTo().assert(hasText(str(R.string.run_rate_title), substring = true))
        rule.onNodeWithTag("run-log-effort").performClick()
        rule.waitForIdle()
        // The Rate run sheet: one effort scale, the run type chips and Save.
        rule.pickRating("effort-scale", 7)
        rule.onNodeWithTag("run-rate-save").performScrollTo().performClick()
        rule.waitForIdle()

        // The card face reads the rating back from the core.
        rule.assertTextCountEventually(str(R.string.history_effort_value, 7), 1)
        // The rating is one durable in-place edit, the wire form the core replays.
        val log = File(ctx.filesDir, "event-log.ndjson").readText()
        assertTrue(log, log.contains("\"AmendRunMeta\""))
        assertTrue(log, log.contains("\"session_rpe\":7"))
        // Rated, the Effort row states the rating with its CR-10 word.
        rule.assertTextCountEventually(str(R.string.run_effort_value, 7, str(R.string.history_effort_anchor_7)), 1)
        // The derived session-effort load lives behind the rating's "?", not on the face.
        val loadLine = strPrefix(R.string.history_training_load_value)
        rule.onAllNodesWithText(loadLine, substring = true).assertCountEquals(0)
        rule.onNode(hasContentDescription(str(R.string.history_training_load_subject), substring = true) and hasClickAction()).performClick()
        rule.waitForIdle()
        rule.onAllNodesWithText(loadLine, substring = true).fetchSemanticsNodes().let {
            assertTrue("training load line", it.isNotEmpty())
        }
    }

    @Test
    fun runDetailWithRating_meetsEveryInvariantAtFontScale130() {
        RuntimeEnvironment.setFontScale(1.3f)
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), gpsTenK(QaHarness.daysAgoAtMidday(1))))
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_history))
        rule.scrollToText("Fastest 10K")
        rule.onNodeWithText("Fastest 10K").performClick()
        rule.waitForIdle()
        // The route loads off the main thread; Trim appears once it has.
        rule.waitUntil(10_000) { rule.onAllNodesWithTag("run-trim").fetchSemanticsNodes().isNotEmpty() }
        val kinds = arrayOf("forbidden-text", "unlabeled-action", "small-touch-target", "duplicate-action-in-card", "clipped-action")
        rule.onNodeWithTag("run-delete").performScrollTo()
        val sheet = violations(*kinds, clipping = true)
        assertTrue("UX invariant(s) violated on the run sheet:\n" + sheet.joinToString("\n"), sheet.isEmpty())
        // The Rate run sheet at 130%: the effort scale and the type chips.
        rule.onNodeWithTag("run-log-effort").performScrollTo().performClick()
        rule.waitForIdle()
        rule.onNodeWithTag("run-rate-save").performScrollTo()
        val rate = violations(*kinds, clipping = true)
        assertTrue("UX invariant(s) violated on Rate run:\n" + rate.joinToString("\n"), rate.isEmpty())
    }

    @Test
    fun progressChart_rendersSixteenWeeksSpokenInMiles() {
        AppSettings.setDistanceUnitOverride(ctx, DistanceUnitOverride.Mi)
        QaHarness.seedLog(
            ctx,
            listOf(
                QaHarness.strengthProfile(),
                QaHarness.loggedRun(QaHarness.daysAgoAtMidday(9)),
                QaHarness.loggedRun(QaHarness.daysAgoAtMidday(1)),
            ),
        )
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_history))

        val chart = hasContentDescription(strPrefix(R.string.history_weekly_desc, 16), substring = true)
        rule.onNode(hasScrollToNodeAction()).performScrollToNode(chart)
        // The weekly chart is the default of the progress block's 16 weeks | 14 days segment.
        rule.onAllNodesWithText(str(R.string.history_progress_weeks, 16)).assertCountEquals(1)
        val spoken = rule.onNode(chart).fetchSemanticsNode()
            .config[SemanticsProperties.ContentDescription].joinToString(" ")
        // 2 × 5 km = 6.2 mi, every figure in miles. (Fragments of the spoken
        // summary, whose peak-week date argument the test can't rebuild.)
        assertTrue(spoken, spoken.contains("6.2 mi in total"))
        assertTrue(spoken, spoken.contains("this week 0 mi"))
        assertFalse(spoken, spoken.contains("km"))
    }

    @Test
    fun liftSessionThatBeatsEveryPriorE1rm_showsNewBest() {
        val earlier = QaHarness.daysAgoAtMidday(3)
        val later = QaHarness.daysAgoAtMidday(1)
        QaHarness.seedLog(
            ctx,
            listOf(
                QaHarness.strengthProfile(),
                Event.LogSet("Back Squat", 100.0, 5, 8.0, earlier, earlier * 1000),
                Event.LogSet("Back Squat", 110.0, 5, 8.0, later, later * 1000),
            ),
        )
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_history))
        rule.scrollToText("110 kg")

        // Only the session that beat the earlier e1RM is marked; a first set never is.
        rule.onAllNodesWithText(str(R.string.history_new_best)).assertCountEquals(1)
    }
}
