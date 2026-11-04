package app.milestone

import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextReplacement
import app.milestone.AppClock
import app.milestone.Event
import app.milestone.LiftResultView
import app.milestone.LocalEditorPinnedActions
import app.milestone.LogSetEditor
import app.milestone.MilestoneTheme
import app.milestone.ObservedAtRow
import app.milestone.R
import app.milestone.SessionPlanView
import app.milestone.ThisWeekCard
import app.milestone.WeightUnit
import app.milestone.qa.str
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import java.util.Locale

/** The week strip's spoken day status, an edited hold set's hold mode, and the
 *  logged-at date dialog's traversal group. */
@RunWith(RobolectricTestRunner::class)
class TodayEditorA11yTest {

    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    /** writing.md: "paused (not "on hold")". The strip's chip and hero say PAUSED,
     *  and so does the day's spoken description. */
    @Test
    fun aHeldDayInTheWeekStripIsSpokenPaused() {
        Locale.setDefault(Locale.US)
        // Mon 2026-09-21: Thursday is held.
        val monday = java.time.LocalDate.of(2026, 9, 21).toEpochDay()
        val week = (0 until 7).map {
            SessionPlanView(epoch_day = monday + it, title = "Day $it", session_type = "Lift(MaxEffort)", status = if (it == 3) "blocked" else "planned")
        }
        rule.setContent {
            MilestoneTheme {
                ThisWeekCard(
                    consistency = null, week = week, today = monday + 3, weightUnit = WeightUnit.Kg, program = null,
                    footnote = null, painArea = null, onRemovePlan = {}, actions = null,
                    heroDay = -1L, selectedDay = -1L, onSelectDay = {},
                )
            }
        }
        val spoken = rule.onAllNodes(SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Tab)).fetchSemanticsNodes()
            .map { it.config.getOrNull(SemanticsProperties.ContentDescription).orEmpty().joinToString(" ") }
            .single { it.contains("Thursday") }
        assertFalse(spoken, spoken.contains("on hold"))
        assertTrue(spoken, spoken.contains("paused"))
    }

    /** Editing a logged 30 s hold and retyping its name in another case keeps it a
     *  hold: the name saves as the same "Plank" ([sameExercise]), so the hold
     *  seconds must survive. */
    @Test
    fun recasingAnEditedHoldSetKeepsItAHold() {
        Locale.setDefault(Locale.US)
        var saved: Event? = null
        rule.setContent {
            MilestoneTheme {
                Column(Modifier.fillMaxSize()) {
                    CompositionLocalProvider(LocalEditorPinnedActions provides true) {
                        LogSetEditor(
                            recentExercises = listOf("Plank"),
                            initial = LiftResultView(
                                exercise = "Plank", weight_kg = 0.0, reps = 1, hold_sec = 30,
                                observed_at = AppClock.nowSec() - 3_600, entry_id = 42L,
                            ),
                            weightUnit = WeightUnit.Kg,
                            onSave = { saved = it },
                        )
                    }
                }
            }
        }
        rule.onNodeWithContentDescription(str(R.string.log_change_exercise_cd, "Plank")).performClick()
        rule.onNodeWithTag("exercise-search").performTextReplacement("plank")
        rule.waitForIdle()
        rule.onNodeWithTag("editor-primary").performClick()
        rule.waitForIdle()
        val ev = saved as Event.AmendSet
        assertEquals("saved under the known spelling", "Plank", ev.exercise)
        assertEquals("still a 30 s hold", 30, ev.holdSec)
    }

    /** Every dialog window is a traversal group so RTL reads its buttons start to
     *  end, the logged-at date dialog like its time dialog. The modifier lands on
     *  Material's dialog pane node (the one with a pane title), so the time dialog
     *  is the control. */
    @Test
    fun theDatePickerDialogIsATraversalGroupLikeTheTimeDialog() {
        rule.setContent {
            MilestoneTheme { ObservedAtRow(AppClock.nowSec() - 86_400, withTime = true) {} }
        }
        fun paneIsGroup(): Boolean {
            val panes = rule.onAllNodes(SemanticsMatcher.keyIsDefined(SemanticsProperties.PaneTitle), useUnmergedTree = true)
                .fetchSemanticsNodes()
            assertTrue("a dialog is open", panes.isNotEmpty())
            return panes.any { it.config.getOrNull(SemanticsProperties.IsTraversalGroup) == true }
        }
        rule.onNode(hasClickAction()).performClick()
        rule.waitForIdle()
        val dateGroup = paneIsGroup()
        rule.onNodeWithText(str(R.string.log_ok)).performClick()
        rule.waitForIdle()
        val timeGroup = paneIsGroup()
        assertTrue("control: the time dialog is a traversal group", timeGroup)
        assertTrue("the date dialog is a traversal group", dateGroup)
    }
}
