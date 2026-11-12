package app.milestone.qa

import android.Manifest
import android.app.Application
import android.content.Context
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.test.core.app.ApplicationProvider
import app.milestone.OpenDest
import app.milestone.openIntent
import app.milestone.R
import app.milestone.RunSession
import app.milestone.AppSettings
import app.milestone.PrescriptionView
import app.milestone.RunStructureView
import app.milestone.RunTargetView
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import app.milestone.TRACKER_MAP_FRAME_TAG
import org.junit.After
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf

/**
 * The tracker's Finish review asks how hard the whole run was (CR-10) and
 * the run type, and Save carries both into the of-record LogRunTrack. Drives the
 * real app over the real core: a recording run is reopened from its notification,
 * Stop opens the review, effort 7 is picked, Save persists, and the durable event
 * log holds `"session_rpe":7`.
 */
@RunWith(RobolectricTestRunner::class)
class RunFinishEffortTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @Before
    fun setUp() {
        QaHarness.reset(ctx)
        shadowOf(ctx as Application).grantPermissions(
            Manifest.permission.ACCESS_FINE_LOCATION,
            Manifest.permission.ACCESS_COARSE_LOCATION,
            Manifest.permission.POST_NOTIFICATIONS,
        )
        // No map tiles under Robolectric; the run is recorded either way.
        AppSettings.setShowRouteMaps(ctx, false)
    }

    @After
    fun tearDown() {
        // Close the Activity first (its composition owns the tracker's scopes), then
        // drop every tracker singleton so no run state reaches the next test.
        QaHarness.teardown()
        RunSession.reset()
        QaHarness.freshProcess()
    }

    private fun openFinishReview() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        QaHarness.seedRecordingRun()
        rule.launchOnTracker()
        rule.stopToFinishReview()
    }

    @Test
    fun finishReview_effort7_saveSendsLogRunTrackWithSessionRpe7() {
        openFinishReview()
        rule.pickRating("effort-scale", 7)
        rule.onNodeWithText(str(R.string.run_type_interval)).performScrollTo().performClick()
        val saved = rule.saveRunAndAwait()
        assertEquals(7, saved["session_rpe"]?.jsonPrimitive?.int)
        assertEquals("Interval", saved["workout_type"]?.jsonPrimitive?.content)
    }

    @Test
    fun finishReview_meetsEveryInvariant_atFontScale130() {
        RuntimeEnvironment.setFontScale(1.3f)
        openFinishReview()
        val hits = UxInvariants.check(rule, rule.density)
        assertTrue("UX invariant(s) violated:\n" + hits.joinToString("\n"), hits.isEmpty())
        // Save stays reachable without scrolling: the chips scroll above the pinned row.
        val saved = rule.saveRunAndAwait()
        assertNull("an unrated run sends no rating: $saved", saved["session_rpe"])
    }

    /**
     * A run started from today's plan shows its targets and walks its steps
     * on the tracker: the current step, what is left of it, what comes next, and
     * Skip step.
     */
    @Test
    fun followedPlan_showsTargetsAndCurrentStep_andSkipAdvances() {
        QaHarness.legacyDistanceUnit(ctx, app.milestone.DistanceUnit.Km)
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        RunSession.followPlan(PrescriptionView(
            exercise = "Intervals",
            summary = "6 × 800 m · 400 m jog between",
            run_head = "6 × 800 m",
            face_lead = "6 × 800 m",
            run_target = RunTargetView(
                band = "interval",
                pace_fast_sec_per_km = 250,
                pace_slow_sec_per_km = 260,
                pace_label = "4:10–4:20 /km",
                effort_cue = "effort 8–9 of 10",
                effort_detail = "8–9 of 10",
            ),
            run_structure = RunStructureView(reps = 6, rep_m = 800, recovery_m = 400),
        ))
        QaHarness.seedRecordingRun()
        RunSession.startSteps(android.os.SystemClock.elapsedRealtime())
        lastScenario = launchMain(openIntent(ctx, OpenDest.Track))
        val rep = str(R.string.track_step_rep, 1, 6)
        rule.waitUntil(30_000) { rule.onAllNodesWithText(rep).fetchSemanticsNodes().isNotEmpty() }
        fun metresLeft(m: Int) = str(R.string.track_step_left, str(R.string.track_step_metres, m))
        val recovery = str(R.string.track_step_recovery)
        rule.onNodeWithText(metresLeft(800)).assertExists()
        rule.onNodeWithText(str(R.string.track_step_next, recovery)).assertExists()
        // While recording, the targets fold into one muted line under the stats; the
        // pre-start stack (one target per line) is gone.
        rule.onNodeWithText(str(R.string.track_plan_targets_line, "4:10–4:20 /km · effort 8–9 of 10")).assertExists()
        assertTrue(rule.onAllNodesWithText(str(R.string.track_target_pace, "4:10–4:20 /km")).fetchSemanticsNodes().isEmpty())
        // 3 m/s (5:33 /km) against 4:10–4:20 /km: the status says what to do, and it
        // agrees with the figure (slower than the range → Speed up).
        rule.onNode(hasContentDescription(str(R.string.track_cd_pace_status_speed_up), substring = true)).assertExists()

        rule.onNodeWithText(str(R.string.track_step_skip)).performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText(recovery).fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText(metresLeft(400)).assertExists()

        val v = UxInvariants.check(rule, rule.density)
        val hits = v.filter { it.kind in setOf("forbidden-text", "unlabeled-action", "small-touch-target") }
        assertTrue("UX invariant(s) violated:\n" + hits.joinToString("\n"), hits.isEmpty())

        // Stopping a guided run preselects its type from the plan (interval band).
        rule.stopToFinishReview()
        rule.onNodeWithText(str(R.string.run_type_interval)).performScrollTo().assertIsSelected()
    }

    /**
     * Pre-start: the battery notice sits at the top of the sheet, and nothing floats
     * over the map except the status pill (and Recenter / the attribution once there
     * is a position). The start-gate caption explains only what is on.
     */
    @Test
    fun preStart_onlyTheStatusPillFloatsOverTheMap() {
        AppSettings.setShowRouteMaps(ctx, true)
        // Lifts and runs, so the Log chooser offers Track run.
        QaHarness.seedLog(ctx, listOf(QaHarness.hybridFiveKProfile()))
        rule.launchApp()
        rule.openLogChooser()
        val trackRun = hasText(str(R.string.main_track_run_title)) and hasClickAction()
        rule.assertCountEventually(trackRun, 1)
        rule.waitForIdle()
        rule.onNode(trackRun).performClick()
        val start = hasText(str(R.string.track_btn_start)) and hasClickAction()
        rule.waitUntil(30_000) { rule.onAllNodes(start).fetchSemanticsNodes().isNotEmpty() }
        val battery = str(R.string.track_battery_caption)
        rule.onNodeWithText(battery).assertExists()
        val frame = rule.onNodeWithTag(TRACKER_MAP_FRAME_TAG).fetchSemanticsNode().boundsInRoot
        val notice = rule.onNodeWithText(battery).fetchSemanticsNode().boundsInRoot
        assertTrue("battery notice $notice floats over the map $frame", notice.top >= frame.bottom)
        val allowed = listOf(
            R.string.track_pill_permission_needed, R.string.track_pill_location_off, R.string.track_notif_signal_lost,
            R.string.track_pill_armed_moving, R.string.track_pill_armed_precise, R.string.track_pill_armed_waiting,
            R.string.track_notif_locate_title, R.string.track_cd_pill_fresh_lock, R.string.track_map_attribution,
            R.string.track_cd_recenter,
        ).map { str(it) }.toSet()
        val labelled = SemanticsMatcher("has text or description") {
            SemanticsProperties.Text in it.config || SemanticsProperties.ContentDescription in it.config
        }
        val overMap = rule.onAllNodes(labelled, useUnmergedTree = true).fetchSemanticsNodes()
            .filter { it.boundsInRoot.overlaps(frame) && it.boundsInRoot.intersect(frame).let { r -> r.width > 0f && r.height > 0f } }
            .flatMap { n ->
                n.config.getOrElse(SemanticsProperties.ContentDescription) { emptyList() } +
                    n.config.getOrElse(SemanticsProperties.Text) { emptyList() }.map { it.text }
            }
        assertTrue("over the map: $overMap", overMap.isNotEmpty() && overMap.all { it in allowed })
        // Both gates off: one line saying what Start does, not both explainers.
        rule.onNodeWithText(str(R.string.track_gates_caption_none)).assertExists()
        rule.onNodeWithText(str(R.string.track_gate_precise)).performClick()
        rule.onNodeWithText(str(R.string.track_gates_caption_precise)).assertExists()
        assertTrue(rule.onAllNodesWithText(str(R.string.track_gates_caption_moving)).fetchSemanticsNodes().isEmpty())
    }
}
