package app.milestone.qa

import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isSelectable
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.unit.dp
import app.milestone.EffortScale
import app.milestone.EventDispatcher
import app.milestone.LocalEditorPinnedActions
import app.milestone.LogSetEditor
import app.milestone.MilestoneTheme
import app.milestone.PrescriptionView
import app.milestone.MorningCheckinSheet
import app.milestone.R
import app.milestone.RunTargetView
import app.milestone.RunTypeChips
import app.milestone.SessionPlanView
import app.milestone.ViewModel
import app.milestone.WeightUnit
import app.milestone.WorkoutType
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * UI sweep §0.4 / §8 / §9: every 0–N rating is ONE segmented track in one row
 * (one adjustable TalkBack node, no gap cell), run type is a chip row, and a
 * free set opened from the Log chooser on a run day shows nothing of the run.
 */
@RunWith(RobolectricTestRunner::class)
class RatingScaleTest {

    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    @After
    fun tearDown() {
        EventDispatcher.model.value = null
    }

    private var picked: Int? = null

    private fun showEffort() {
        rule.setContent {
            MilestoneTheme {
                var value by remember { mutableStateOf<Int?>(null) }
                Column(Modifier.fillMaxSize().padding(16.dp)) {
                    EffortScale(value, { value = it; picked = it })
                }
            }
        }
    }

    @Test
    fun aTapPicksAValueAndNamesIt_aSecondTapClearsIt() {
        showEffort()
        rule.pickRating("effort-scale", 5)
        assertEquals(5, picked)
        rule.onNodeWithText(str(R.string.history_effort_selected, 5, str(R.string.history_effort_anchor_5))).assertExists()
        rule.pickRating("effort-scale", 5)
        assertNull(picked)
    }

    @Test
    fun theTrackIsOneAdjustableNode() {
        showEffort()
        val track = rule.onNodeWithTag("effort-scale")
        track.assert(hasContentDescription(str(R.string.history_effort_overline)))
        track.assert(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, str(R.string.rating_unrated)))
        track.performSemanticsAction(SemanticsActions.SetProgress) { it(7f) }
        rule.waitForIdle()
        assertEquals(7, picked)
        track.assert(
            SemanticsMatcher.expectValue(
                SemanticsProperties.StateDescription,
                str(R.string.rating_effort_spoken, 7, str(R.string.history_effort_anchor_7)),
            ),
        )
        // The eleven values are not eleven focus targets.
        assertEquals(0, rule.onAllNodes(isSelectable()).fetchSemanticsNodes().size)
    }

    @Test
    fun allElevenValuesSitInOneRowWithNoGapCell() {
        showEffort()
        val bounds = (0..10).map { rule.onNodeWithTag("effort-scale-$it", useUnmergedTree = true).fetchSemanticsNode().boundsInRoot }
        assertEquals("one row", 1, bounds.map { it.top }.distinct().size)
        val gapPx = with(rule.density) { 5.dp.toPx() }
        bounds.zipWithNext().forEach { (a, b) -> assertTrue("contiguous segments: ${a.right} → ${b.left}", b.left - a.right < gapPx) }
        val widths = bounds.map { it.width }
        assertTrue("equal segments: $widths", widths.max() - widths.min() <= 1f)
    }

    @Test
    fun runTypeIsOneChipRowInRunnerOrder() {
        var type: WorkoutType? = null
        rule.setContent {
            MilestoneTheme {
                Column(Modifier.fillMaxSize().padding(16.dp)) {
                    RunTypeChips(type, { type = it })
                }
            }
        }
        val labels = listOf(WorkoutType.Easy, WorkoutType.Recovery, WorkoutType.Steady, WorkoutType.LongRun, WorkoutType.Tempo, WorkoutType.Interval)
        val lefts = labels.map { rule.onNodeWithTag("run-type-${it.name}").fetchSemanticsNode().boundsInRoot.left }
        assertTrue("chips flow left to right in runner order: $lefts", lefts.first() < lefts[1])
        rule.onNodeWithText(WorkoutType.Tempo.label).performClick()
        assertEquals(WorkoutType.Tempo, type)
    }

    @Test
    fun aFreeSetOnARunDayShowsNothingOfTheRun() {
        java.util.Locale.setDefault(java.util.Locale.US)
        EventDispatcher.model.value = ViewModel()
        val run = PrescriptionView(
            summary = "5:36–6:06 /km (easy) · ≤ 140 bpm",
            exercise = "",
            sets = 1,
            intensity_label = "5:36–6:06 /km (easy) · ≤ 140 bpm · effort 3–5 of 10",
            run_target = RunTargetView(),
        )
        rule.setContent {
            MilestoneTheme {
                Column(Modifier.fillMaxSize()) {
                    CompositionLocalProvider(LocalEditorPinnedActions provides true) {
                        LogSetEditor(
                            recentExercises = listOf("Back Squat"),
                            planSession = SessionPlanView(items = listOf(run)),
                            weightUnit = WeightUnit.Lb,
                            onSave = {},
                        )
                    }
                }
            }
        }
        for (runText in listOf("/km", "/mi", "bpm", " km", "effort 3")) {
            assertEquals("no \"$runText\" on a free set", 0, rule.onAllNodes(hasText(runText, substring = true)).fetchSemanticsNodes().size)
        }
        assertEquals(0, rule.onAllNodes(hasText(str(R.string.log_set_header_n_of_n, 1, 1))).fetchSemanticsNodes().size)
        rule.onNodeWithTag("exercise-search").assertExists()
        // Weight, reps and effort wait for an exercise.
        val weight = hasContentDescription(str(R.string.log_field_weight), substring = true)
        assertEquals(0, rule.onAllNodes(weight).fetchSemanticsNodes().size)
        rule.onNodeWithText("Back Squat").performScrollTo().performClick()
        rule.waitForIdle()
        assertTrue("the weight well shows once Back Squat is picked", rule.onAllNodes(weight).fetchSemanticsNodes().isNotEmpty())
    }

    @Test
    fun everyCheckinScaleIsOneRowAndTheNoteSaysWhichWayItReads() {
        rule.setContent {
            MilestoneTheme {
                Column(Modifier.fillMaxSize()) {
                    CompositionLocalProvider(LocalEditorPinnedActions provides true) {
                        MorningCheckinSheet(onSubmit = { _, _ -> })
                    }
                }
            }
        }
        rule.onNodeWithText(str(R.string.log_checkin_scale_note)).assertExists()
        val scales = listOf(
            "sleep" to (1..5),
            "soreness" to (5 downTo 1),
            "mood" to (1..5),
            "fatigue" to (5 downTo 1),
            "stress" to (5 downTo 1),
        )
        for ((key, values) in scales) {
            val tag = "checkin-$key"
            val bounds = values.map { rule.onNodeWithTag("$tag-$it", useUnmergedTree = true).fetchSemanticsNode().boundsInRoot }
            assertEquals("$tag is one row", 1, bounds.map { it.top }.distinct().size)
            assertTrue("$tag keeps the core's order, left to right", bounds.zipWithNext().all { (a, b) -> a.left < b.left })
        }
        // The worse end sits on the left of a reversed scale.
        rule.pickRating("checkin-soreness", 5)
        val words = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
            .resources.getStringArray(R.array.rating_soreness_words)
        rule.onNodeWithText(str(R.string.rating_value_word, 5, words[4])).assertExists()
    }
}
