package app.milestone.qa

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasScrollToNodeAction
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.unit.dp
import app.milestone.MilestoneTheme
import app.milestone.R
import app.milestone.ConsistencyView
import app.milestone.GuidanceView
import app.milestone.MuscleVolumeView
import app.milestone.PrescriptionView
import app.milestone.ProgramSummaryView
import app.milestone.ReadinessSignalView
import app.milestone.SessionPlanView
import app.milestone.ThisWeekCard
import app.milestone.TodayDestination
import app.milestone.ViewModel
import app.milestone.WeekActions
import app.milestone.WeightUnit
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/** The THIS WEEK strip: seven single-glyph day columns; what one glyph leaves out
 *  (a partly done day) is stated in words in the day's preview. */
@RunWith(RobolectricTestRunner::class)
class ThisWeekCardTest {

    @get:Rule
    val rule = createComposeRule()

    @Test
    fun aPartlyDoneDaySaysSoInItsPreview() {
        java.util.Locale.setDefault(java.util.Locale.US)
        // Mon 2026-09-21 … Sun: done, partly done, missed, today, then planned and rest.
        val monday = java.time.LocalDate.of(2026, 9, 21).toEpochDay()
        val statuses = listOf("done", "partial", "missed", "next", "planned", "rest", "planned")
        val week = statuses.mapIndexed { i, status ->
            SessionPlanView(epoch_day = monday + i, title = "Day $i", session_type = "Strength", status = status)
        }
        var selected by mutableStateOf(-1L)
        rule.setContent {
            MilestoneTheme {
                ThisWeekCard(
                    consistency = null,
                    week = week,
                    today = monday + 3,
                    weightUnit = WeightUnit.Kg,
                    program = null,
                    footnote = null,
                    painArea = null,
                    onRemovePlan = {},
                    actions = noActions,
                    heroDay = monday + 3,
                    selectedDay = selected,
                    onSelectDay = { selected = it },
                )
            }
        }
        val word = str(R.string.main_week_day_partial)
        rule.onNodeWithTag("week-partial-mark", useUnmergedTree = true).assertIsDisplayed()
        assertEquals("the strip cell carries one glyph, no word", 0, rule.onAllNodesWithText(word, useUnmergedTree = true).fetchSemanticsNodes().size)
        rule.onNodeWithContentDescription("Tuesday", substring = true).performClick()
        rule.onNodeWithText(word, useUnmergedTree = true).assertIsDisplayed()
    }

    @Test
    fun theStripFillsTheCardInSevenEqualColumns() {
        val monday = java.time.LocalDate.of(2026, 9, 21).toEpochDay()
        val week = (0 until 7).map { SessionPlanView(epoch_day = monday + it, title = "Day $it", session_type = "Run", status = "planned") }
        rule.setContent {
            MilestoneTheme {
                ThisWeekCard(
                    consistency = null, week = week, today = monday, weightUnit = WeightUnit.Kg, program = null,
                    footnote = null, painArea = null, onRemovePlan = {}, actions = noActions,
                    heroDay = monday, selectedDay = -1L, onSelectDay = {},
                )
            }
        }
        val card = rule.onNodeWithTag("this-week").fetchSemanticsNode().boundsInRoot
        val days = rule.onAllNodes(SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Tab)).fetchSemanticsNodes()
            .map { it.boundsInRoot }
        assertEquals(7, days.size)
        val widths = days.map { it.width }
        assertTrue("equal columns: $widths", widths.max() - widths.min() <= 1f)
        // The last column ends at the card's content edge (14dp padding), not short of it.
        val inset = with(rule.density) { 14.dp.toPx() }
        assertEquals(card.right - inset, days.maxOf { it.right }, 2f)
        assertEquals(card.left + inset, days.minOf { it.left }, 2f)
    }

    /** A week row can read "missed" without the core's missed-session note (a hold
     *  is active, or the work was logged nearby); the repeating-week explanation
     *  still stays off the face. */
    @Test
    fun aMissedRowWithoutItsNoteKeepsTheRepeatingWeekNoteOffTheFace() {
        val monday = java.time.LocalDate.of(2026, 9, 21).toEpochDay()
        val week = listOf("missed", "blocked", "planned", "planned", "rest", "planned", "rest").mapIndexed { i, status ->
            SessionPlanView(epoch_day = monday + i, title = "Day $i", session_type = "Lift(MaxEffort)", status = status)
        }
        val consistency = ConsistencyView(
            this_week_done = 0,
            this_week_planned = 4,
            repeating_week = true,
            notes = listOf(
                GuidanceView(summary = "0 of 4 planned sessions done this week."),
                GuidanceView(summary = "Repeating this plan week: last week had 2 of 4 sessions."),
            ),
        )
        rule.setContent {
            MilestoneTheme {
                ThisWeekCard(
                    consistency = consistency, week = week, today = monday + 1, weightUnit = WeightUnit.Kg,
                    program = null, footnote = null, painArea = null, onRemovePlan = {}, actions = null,
                    heroDay = -1L, selectedDay = -1L, onSelectDay = {},
                )
            }
        }
        assertEquals(
            0,
            rule.onAllNodesWithText("Repeating this plan week", substring = true, useUnmergedTree = true)
                .fetchSemanticsNodes().size,
        )
    }

    /** An item a left-knee report stopped reads "Stopped · Left knee" in the week
     *  strip, as in the hero, never the bare "Stopped · pain". */
    @Test
    fun aPainStoppedItemReadsTheSameAreaInTheWeekStrip() {
        java.util.Locale.setDefault(java.util.Locale.US)
        val day = java.time.LocalDate.of(2026, 9, 24).toEpochDay()
        val stopped = PrescriptionView(
            summary = "Back Squat 3×5", exercise = "Back Squat", sets = 3, sets_done = 1,
            done = true, status = "stopped_pain",
        )
        val open = PrescriptionView(summary = "Bench Press 3×5", exercise = "Bench Press", sets = 3)
        val session = SessionPlanView(
            epoch_day = day, title = "Strength A", session_type = "Lift(MaxEffort)", status = "next",
            items = listOf(stopped, open),
        )
        val model = ViewModel(
            today_epoch_day = day,
            next_session = session,
            week_plan = listOf(session),
            readiness_summary = listOf(
                ReadinessSignalView(signal = "Pain", state = "reported", detail = "Left knee · sharp/joint · 6/10"),
            ),
        )
        rule.setContent {
            MilestoneTheme {
                TodayDestination(
                    model, rememberLazyListState(), onEvent = {}, onStartSetup = {}, onLogPlanItem = {},
                    onLogPastItem = { _, _ -> }, onLogPastRun = {}, onTrackRun = {}, onOpenSheet = {},
                    lastPain = null, onOpenProfile = {}, onRerunSetup = {},
                )
            }
        }
        val list = rule.onNode(hasScrollToNodeAction())
        list.performScrollToNode(hasTestTag("this-week"))
        rule.onNodeWithContentDescription("Thursday", substring = true).performClick()
        rule.waitForIdle()
        val inWeek = hasAnyAncestor(hasTestTag("this-week"))
        list.performScrollToNode(inWeek and hasTestTag("plan-item:Back Squat"))
        val bare = str(R.string.plan_stopped_pain)
        val area = str(R.string.plan_stopped_area, "Left knee")
        assertEquals(0, rule.onAllNodes(inWeek and hasText(bare), useUnmergedTree = true).fetchSemanticsNodes().size)
        assertTrue(rule.onAllNodes(inWeek and hasText(area), useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty())
    }

    /** The muscle-volume toggle tells TalkBack whether its table is open. */
    @Test
    fun theMuscleVolumeToggleSpeaksItsState() {
        val program = ProgramSummaryView(
            name = "Hypertrophy", phase = "Build", week = 1, weeks_total = 6,
            muscle_volume = listOf(MuscleVolumeView(muscle = "Chest", target_sets = 10, delivered_sets = 10f, label = "Chest 10 of 10")),
        )
        rule.setContent {
            MilestoneTheme {
                ThisWeekCard(
                    consistency = null, week = emptyList(), today = 0L, weightUnit = WeightUnit.Kg,
                    program = program, footnote = null, painArea = null, onRemovePlan = {}, actions = null,
                    heroDay = -1L, selectedDay = -1L, onSelectDay = {},
                )
            }
        }
        val toggle = rule.onNode(hasText(str(R.string.main_muscle_volume_all_on_target)) and hasClickAction())
        toggle.assert(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, str(R.string.ui_cd_collapsed)))
        toggle.performClick()
        toggle.assert(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, str(R.string.ui_cd_expanded)))
    }

    private val noActions = WeekActions(
        onLogItem = { _, _ -> },
        onLogRun = {},
        onEndExercise = { _, _ -> },
        onReopen = { _, _ -> },
        onMove = { _, _ -> },
    )
}
