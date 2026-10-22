package app.milestone.qa

import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import app.milestone.Event
import app.milestone.EventDispatcher
import app.milestone.LocalEditorPinnedActions
import app.milestone.LogRunEditor
import app.milestone.MilestoneTheme
import app.milestone.ProfileView
import app.milestone.RunResultView
import app.milestone.R
import app.milestone.ViewModel
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * The manual run editor takes the average heart rate in bpm (what a watch
 * shows; the core converts it to % HRmax) and the whole-session effort (CR-10),
 * and sends both on the LogRun.
 */
@RunWith(RobolectricTestRunner::class)
class RunEditorHrEffortTest {

    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    @After
    fun tearDown() {
        EventDispatcher.model.value = null
    }

    private fun key(k: String) =
        rule.onNode(hasText(k) and hasAnyAncestor(hasTestTag("numeric-keypad"))).performScrollTo().performClick()

    private fun show(profile: ProfileView?, initial: RunResultView? = null, onSave: (Event) -> Unit) {
        java.util.Locale.setDefault(java.util.Locale.US)
        EventDispatcher.model.value = ViewModel(profile = profile)
        // Hosted the way the Log sheet hosts it: a bounded column with the pinned
        // primary action (tag `editor-primary`) and the form scrolling above it.
        rule.setContent {
            MilestoneTheme {
                Column(Modifier.fillMaxSize()) {
                    CompositionLocalProvider(LocalEditorPinnedActions provides true) {
                        LogRunEditor(initial = initial, onSave = onSave)
                    }
                }
            }
        }
    }

    @Test
    fun bpmAndEffort_areSentOnTheLogRun() {
        var sent: Event? = null
        show(ProfileView(age_years = 30.0)) { sent = it }
        key("5")
        rule.onNode(hasContentDescription(str(R.string.log_field_duration))).performScrollTo().performClick()
        listOf("3", "0", "0", "0").forEach { key(it) }
        // One "Average heart rate" field, always shown (empty = not recorded); a
        // known age lets the core convert bpm, so bpm is offered first and the unit
        // in the field header is the swap to % of max (no extra row).
        assertEquals(1, rule.onAllNodes(hasContentDescription(str(R.string.log_hr_use_pct))).fetchSemanticsNodes().size)
        rule.onNode(hasContentDescription(str(R.string.track_log_hr_avg))).performScrollTo().performClick()
        listOf("1", "5", "2").forEach { key(it) }
        rule.pickRating("effort-scale", 6)
        // The pinned primary sits below the scrolling form, always on screen.
        rule.onNodeWithTag("editor-primary").performClick()
        rule.waitForIdle()

        val run = sent as Event.LogRun
        // Locale.US → the editor takes miles; the wire carries km (2-decimal grid).
        assertEquals(5 * 1.609344, run.distanceKm, 0.005)
        assertEquals(30.0, run.durationMin, 1e-9)
        assertEquals(152.0, run.avgHrBpm!!, 1e-9)
        assertEquals("the core derives the % from the bpm", 0.0, run.hrPctMax, 1e-9)
        assertEquals(6, run.sessionRpe)
        val json = run.toJson().toString()
        assertTrue(json, json.contains("\"avg_hr_bpm\":152.0") && json.contains("\"session_rpe\":6"))
    }

    @Test
    fun withoutAnAgeOrMeasuredMax_onlyPercentOfMaxIsOffered() {
        var sent: Event? = null
        show(ProfileView()) { sent = it }
        rule.waitForIdle()
        // One average-HR field, in % of max: no bpm unit and no unit swap offered.
        rule.onNode(hasContentDescription(str(R.string.track_log_hr_avg))).performScrollTo()
        assertEquals(1, rule.onAllNodesWithText(str(R.string.log_hr_pct_max)).fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodes(hasContentDescription(str(R.string.log_hr_use_bpm))).fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodesWithText(str(R.string.track_log_hr_bpm)).fetchSemanticsNodes().size)
        assertEquals(null, sent)
    }

    @Test
    fun anEmptyHeartRateIsSentAsNotRecorded() {
        var sent: Event? = null
        show(ProfileView(age_years = 30.0)) { sent = it }
        key("5")
        rule.onNode(hasContentDescription(str(R.string.log_field_duration))).performScrollTo().performClick()
        listOf("3", "0", "0", "0").forEach { key(it) }
        rule.onNodeWithTag("editor-primary").performClick()
        rule.waitForIdle()

        val run = sent as Event.LogRun
        assertEquals("no HR typed → no HR sample", 0.0, run.hrPctMax, 1e-9)
        assertEquals(null, run.avgHrBpm)
    }

    @Test
    fun editingABpmRun_keepsTheEnteredBpm() {
        // The view echoes the core's DERIVED %max beside the entered bpm; the editor
        // must reopen on the bpm and save it, never re-send 82.5 as a typed %max.
        val logged = RunResultView(
            entry_id = 7L, observed_at = 1_700_000_000L, distance_km = 8.0, duration_min = 45.0,
            hr_pct_max = 82.5, avg_hr_bpm = 152.0,
        )
        var sent: Event? = null
        show(ProfileView(age_years = 30.0), initial = logged) { sent = it }
        // Reopens on the entered bpm, not the derived 82.5 %.
        rule.onNode(
            hasContentDescription("${str(R.string.track_log_hr_avg)}, 152 ${str(R.string.track_log_hr_bpm)}"),
        ).performScrollTo()
        // Change only the distance.
        rule.onNode(hasContentDescription("${str(R.string.log_field_distance)}, ", substring = true)).performScrollTo().performClick()
        key("6")
        rule.onNodeWithTag("editor-primary").performClick()
        rule.waitForIdle()

        val run = sent as Event.AmendRun
        assertEquals(6 * 1.609344, run.distanceKm, 0.005)
        assertEquals(152.0, run.avgHrBpm!!, 1e-9)
        assertEquals("the core re-derives the % from the bpm", 0.0, run.hrPctMax, 1e-9)
        val json = run.toJson().toString()
        assertTrue(json, json.contains("\"avg_hr_bpm\":152.0"))
    }

    @Test
    fun editingAPercentRun_keepsThePercent() {
        val logged = RunResultView(
            entry_id = 8L, observed_at = 1_700_000_000L, distance_km = 8.0, duration_min = 45.0,
            hr_pct_max = 78.0,
        )
        var sent: Event? = null
        show(ProfileView(age_years = 30.0), initial = logged) { sent = it }
        rule.onNode(
            hasContentDescription("${str(R.string.track_log_hr_avg)}, 78 ${str(R.string.log_hr_pct_max)}"),
        ).performScrollTo()
        rule.onNode(hasContentDescription("${str(R.string.log_field_distance)}, ", substring = true)).performScrollTo().performClick()
        key("6")
        rule.onNodeWithTag("editor-primary").performClick()
        rule.waitForIdle()

        val run = sent as Event.AmendRun
        assertEquals(78.0, run.hrPctMax, 1e-9)
        assertEquals(null, run.avgHrBpm)
    }

    @Test
    fun systemBack_foldsTheKeypadBeforeAskingToDiscard() {
        val logged = RunResultView(entry_id = 9L, observed_at = 1_700_000_000L, distance_km = 8.0, duration_min = 45.0)
        show(ProfileView(age_years = 30.0), initial = logged) {}
        rule.onNode(hasContentDescription("${str(R.string.log_field_distance)}, ", substring = true)).performScrollTo().performClick()
        key("6")
        val keypad = hasTestTag("numeric-keypad")
        val discard = str(R.string.log_discard_title)

        rule.runOnUiThread { rule.activity.onBackPressedDispatcher.onBackPressed() }
        rule.waitForIdle()
        assertEquals("Back folds the open keypad", 0, rule.onAllNodes(keypad).fetchSemanticsNodes().size)
        assertEquals("and asks nothing yet", 0, rule.onAllNodesWithText(discard).fetchSemanticsNodes().size)

        rule.runOnUiThread { rule.activity.onBackPressedDispatcher.onBackPressed() }
        rule.waitForIdle()
        assertEquals("the next Back asks to discard the edit", 1, rule.onAllNodesWithText(discard).fetchSemanticsNodes().size)

        rule.onNodeWithText(str(R.string.log_keep_editing)).performClick()
        rule.onNode(hasContentDescription("${str(R.string.log_field_distance)}, ", substring = true)).performScrollTo().performClick()
        assertEquals("tapping a field opens the keypad again", 1, rule.onAllNodes(keypad).fetchSemanticsNodes().size)
    }
}
