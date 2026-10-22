package app.milestone.qa

import android.Manifest
import android.app.Application
import android.content.Context
import android.content.Intent
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.test.core.app.ActivityScenario
import androidx.test.core.app.ApplicationProvider
import app.milestone.AppClock
import app.milestone.GpsPoint
import app.milestone.MainActivity
import app.milestone.R
import app.milestone.RunSession
import app.milestone.AppSettings
import app.milestone.DistanceUnitOverride
import app.milestone.PrescriptionView
import app.milestone.RunStructureView
import app.milestone.RunTargetView
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isRoot
import app.milestone.TRACKER_MAP_FRAME_TAG
import org.junit.After
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import java.io.File

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
        java.util.Locale.setDefault(java.util.Locale.US)
        QaHarness.reset(ctx)
        RunSession.reset()
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
        AppSettings.distanceUnitOverride.value = DistanceUnitOverride.System
        AppSettings.showRouteMaps.value = true
    }

    /**
     * Tap Save run and stand in for the foreground service's stop confirmation
     * (the service never runs under Robolectric). The clock is held while the tap
     * lands so the screen's 2 s stop-confirm timeout can't be auto-advanced past
     * before `tracking` flips; then wait for the WHOLE save (event-log append +
     * session reset) to finish, so no save turn is left queued on the process-wide
     * serial consumer for a later test to trip over. Returns the saved line.
     */
    private fun saveAndAwaitLoggedRun(): String {
        rule.mainClock.autoAdvance = false
        try {
            rule.onNodeWithText(str(R.string.track_btn_save_run)).performClick()
            RunSession.tracking.value = false
        } finally {
            rule.mainClock.autoAdvance = true
        }
        val log = File(ctx.filesDir, "event-log.ndjson")
        rule.waitUntil(15_000) {
            // The semantics fetch idles the main looper, where the save's serial
            // consumer turn runs; a bare file check would never let it progress.
            rule.onAllNodes(isRoot()).fetchSemanticsNodes()
            log.exists() && log.readText().contains("LogRunTrack") &&
                !RunSession.saving.value && RunSession.points.value.isEmpty()
        }
        return log.readLines().last { it.contains("LogRunTrack") }
    }

    /** A live recording: 21 fixes, ~30 m every 10 s (600 m, 200 s moving), so the
     *  save clears the short-run gate. */
    private fun seedRecordingRun() {
        val t0 = AppClock.nowSec() - 400
        for (i in 0..20) {
            RunSession.add(
                GpsPoint(52.5200 + i * 0.00027, 13.4050, t0 + i * 10L, 5.0),
                (1_000L + i * 10L) * 1_000_000_000L,
            )
        }
        RunSession.tracking.value = true
    }

    private fun openTracker() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        seedRecordingRun()
        lastScenario = ActivityScenario.launch(
            Intent(ctx, MainActivity::class.java).putExtra(MainActivity.EXTRA_OPEN_TRACKING, true),
        )
        val stop = str(R.string.track_btn_stop)
        rule.waitUntil(30_000) { rule.onAllNodesWithText(stop).fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText(stop).performClick()
        rule.waitUntil(10_000) { rule.onAllNodesWithText(str(R.string.track_finish_title)).fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun finishReview_effort7_saveSendsLogRunTrackWithSessionRpe7() {
        openTracker()
        rule.pickRating("effort-scale", 7)
        // The run-type chip shows WorkoutType's own label (no string resource).
        rule.onNodeWithText("Interval").performScrollTo().performClick()
        val line = saveAndAwaitLoggedRun()
        assertTrue(line, line.contains("\"session_rpe\":7"))
        assertTrue(line, line.contains("\"workout_type\":\"Interval\""))
    }

    @Test
    fun finishReview_meetsEveryInvariant_atFontScale130() {
        RuntimeEnvironment.setFontScale(1.3f)
        openTracker()
        val v = UxInvariants.check(rule, rule.density, checkClipping = true)
        val kinds = setOf(
            "forbidden-text", "unlabeled-action", "small-touch-target",
            "duplicate-action-in-card", "clipped-action", "duplicate-summary-on-screen",
        )
        val hits = v.filter { it.kind in kinds }
        assertTrue("UX invariant(s) violated:\n" + hits.joinToString("\n"), hits.isEmpty())
        // Save stays reachable without scrolling: the chips scroll above the pinned row.
        val line = saveAndAwaitLoggedRun()
        assertTrue("an unrated run sends no rating: $line", !line.contains("session_rpe"))
    }

    /**
     * A run started from today's plan shows its targets and walks its steps
     * on the tracker: the current step, what is left of it, what comes next, and
     * Skip step.
     */
    @Test
    fun followedPlan_showsTargetsAndCurrentStep_andSkipAdvances() {
        AppSettings.setDistanceUnitOverride(ctx, DistanceUnitOverride.Km)
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        RunSession.plannedRun.value = PrescriptionView(
            exercise = "Intervals",
            summary = "6 × 800 m · 400 m jog between",
            run_head = "6 × 800 m",
            face_lead = "6 × 800 m",
            run_target = RunTargetView(
                band = "interval",
                pace_fast_sec_per_km = 250,
                pace_slow_sec_per_km = 260,
                pace_label_km = "4:10–4:20 /km",
                pace_label_mi = "6:43–6:59 /mi",
                effort_cue = "effort 8–9 of 10",
                effort_detail = "8–9 of 10",
            ),
            run_structure = RunStructureView(reps = 6, rep_m = 800, recovery_m = 400),
        )
        seedRecordingRun()
        RunSession.startSteps(android.os.SystemClock.elapsedRealtime())
        lastScenario = ActivityScenario.launch(
            Intent(ctx, MainActivity::class.java).putExtra(MainActivity.EXTRA_OPEN_TRACKING, true),
        )
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
        rule.onNodeWithText(str(R.string.track_btn_stop)).performClick()
        rule.waitUntil(10_000) { rule.onAllNodesWithText(str(R.string.track_finish_title)).fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Interval").performScrollTo().assertIsSelected()
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
        QaHarness.seedLog(
            ctx,
            listOf(
                app.milestone.Event.SetProfile(
                    progressionCadence = app.milestone.ProgressionCadence.WeekToWeek,
                    liftGoal = app.milestone.LiftGoal.MaxStrength,
                    goalDistance = app.milestone.GoalDistance.FiveK,
                    concurrentGoal = app.milestone.ConcurrentGoal.Strength,
                    weeklySets = 12,
                    runningDaysPerWeek = 3,
                    runningKmPerWeek = 20.0,
                    advanced = false,
                    bodyweightKg = 80.0,
                    ageYears = 30.0,
                ),
            ),
        )
        rule.launchApp()
        rule.onNode(hasClickAction() and hasContentDescription(str(R.string.main_log))).performClick()
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
            R.string.track_pill_permission_needed, R.string.track_pill_location_off, R.string.track_pill_signal_lost,
            R.string.track_pill_armed_moving, R.string.track_pill_armed_precise, R.string.track_pill_armed_waiting,
            R.string.track_pill_acquiring, R.string.track_cd_pill_fresh_lock, R.string.track_map_attribution,
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
