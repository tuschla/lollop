package app.milestone.qa

import android.content.Context
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasScrollToNodeAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.performScrollToNode
import androidx.test.core.app.ApplicationProvider
import app.milestone.EmptyValuePlaceholder
import app.milestone.Event
import app.milestone.EventDispatcher
import app.milestone.R
import app.milestone.SessionDiscipline
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * Rows the shell shows from what the core says, not from a rule of its own: the
 * sides a profile trains (`trains`) and the race prediction it has (`predicted`).
 */
@RunWith(RobolectricTestRunner::class)
class CoreGatedRowsTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @Before
    fun setUp() {
        QaHarness.reset(ctx)
    }

    @After
    fun tearDown() = QaHarness.teardown()

    /** A runner who opted into strength sessions has lift days in the plan, so the
     *  log chooser offers "Log set". */
    @Test
    fun runnerStrength_logChooserOffersLogSet() {
        QaHarness.seedLog(ctx, listOf(QaHarness.tenKRunnerProfile().copy(runnerStrength = true)))
        rule.launchApp()
        val week = EventDispatcher.model.value!!.week_plan
        assertTrue(
            "scenario: the plan holds lift sessions: ${week.map { it.session_type }}",
            week.any { it.session_type.discipline == SessionDiscipline.Lift },
        )
        rule.openLogChooser()
        assertEquals(1, rule.countOf(hasText(str(R.string.main_log_set_title)) and hasClickAction()))
    }

    /** A race query that gives no prediction reads as the result card's placeholder
     *  on the calculator row, never as a bare "-". */
    @Test
    fun degenerateRacePrediction_rowShowsThePlaceholder() {
        QaHarness.seedLog(
            ctx,
            listOf(QaHarness.hybridFiveKProfile(), Event.PredictRace(5000.0, 0.0, 42_195.0, 40.0, null)),
        )
        rule.launchApp()
        val goal = EventDispatcher.model.value!!.race_prediction!!.goal_label
        rule.selectTab(str(R.string.main_dest_profile))
        val row = hasText(str(R.string.main_tool_race_title)) and hasClickAction()
        rule.onAllNodes(hasScrollToNodeAction()).onFirst().performScrollToNode(row)
        assertEquals(0, rule.countOf(hasText("- · $goal")))
        assertEquals(1, rule.countOf(hasText("$EmptyValuePlaceholder · $goal")))
    }
}
