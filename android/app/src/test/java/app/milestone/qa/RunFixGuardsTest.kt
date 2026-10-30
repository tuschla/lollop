package app.milestone.qa

import android.Manifest
import android.app.Application
import android.content.Context
import android.content.Intent
import android.location.Location
import android.location.LocationManager
import android.os.Looper
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isDialog
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.test.core.app.ApplicationProvider
import app.milestone.ActiveRunStore
import app.milestone.AppClock
import app.milestone.AppSettings
import app.milestone.DistanceUnit
import app.milestone.DistanceUnitOverride
import app.milestone.Event
import app.milestone.EventDispatcher
import app.milestone.GpsPoint
import app.milestone.HistoryFocus
import app.milestone.OpenDest
import app.milestone.R
import app.milestone.RunSession
import app.milestone.RunTrackingService
import app.milestone.openIntent
import app.milestone.spokenDuration
import app.milestone.spokenRunDistance
import kotlinx.serialization.json.jsonArray
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf

/**
 * Run tracking and recovery guards: the tracker's Finish review shows the CORE's
 * figures for the track Save sends, never
 * the live Kotlin counters; a crash-recovered run resumed from the boot prompt keeps
 * its pause boundaries, and recovery thinning keeps every segment's first fix; a GPS
 * run QC left unmeasurable can still be undone after a delete, because its detail
 * sheet fetches its recorded fixes. Also: a pause breaks the segment even when no
 * fix passes the live accuracy gate while paused.
 */
@RunWith(RobolectricTestRunner::class)
class RunFixGuardsTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @Before
    fun setUp() {
        QaHarness.reset(ctx)
        QaHarness.clearSidecar(ctx)
        shadowOf(ctx as Application).grantPermissions(
            Manifest.permission.ACCESS_FINE_LOCATION,
            Manifest.permission.ACCESS_COARSE_LOCATION,
            Manifest.permission.POST_NOTIFICATIONS,
        )
        AppSettings.setShowRouteMaps(ctx, false)
        AppSettings.setDistanceUnitOverride(ctx, DistanceUnitOverride.Km)
    }

    @After
    fun tearDown() {
        QaHarness.teardown()
        ProcessDeath.teardown()
        RunSession.reset()
        QaHarness.clearSidecar(ctx)
        AppSettings.distanceUnitOverride.value = DistanceUnitOverride.System
        AppSettings.showRouteMaps.value = true
    }

    private val model get() = checkNotNull(EventDispatcher.model.value)

    /** A fix [northM] metres north of (52.52, 13.405) at [t], 5 m accuracy. */
    private fun fix(northM: Double, t: Long, accuracyM: Double = 5.0) =
        GpsPoint(52.52 + northM / 111_195.0, 13.405, t, accuracyM)

    // ── The Finish review shows the core's figures ───────────────────────────

    @Test
    fun theFinishReviewShowsTheCoresFiguresNotTheLiveCounters() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        // 21 fixes, 30 m every 10 s: the core saves 600 m in 200 s moving.
        QaHarness.seedRecordingRun()
        lastScenario = launchMain(openIntent(ctx, OpenDest.Track))
        rule.waitForNode(hasText(str(R.string.track_btn_stop)), 30_000)
        // The live counters disagree with the saved figures (a live leg the QC
        // drops, a stop the live floor missed): the review must not show them.
        RunSession.distanceKm.value = 9.99
        RunSession.elapsedSec.value = 4_321L
        rule.onNodeWithText(str(R.string.track_btn_stop)).performClick()
        rule.waitForNode(hasText(str(R.string.track_finish_title)))
        val core = "${spokenRunDistance(ctx.resources, 0.6, DistanceUnit.Km)} · ${spokenDuration(ctx.resources, 200)} moving"
        runCatching { rule.waitForNode(hasContentDescription(core, substring = true)) }
        val spoken = rule.onAllNodes(androidx.compose.ui.test.SemanticsMatcher.keyIsDefined(androidx.compose.ui.semantics.SemanticsProperties.ContentDescription))
            .fetchSemanticsNodes().flatMap { it.config[androidx.compose.ui.semantics.SemanticsProperties.ContentDescription] }
        assertTrue("the review speaks the core's \"$core\": $spoken", spoken.any { core in it })
        assertEquals(0, rule.onAllNodes(hasContentDescription(spokenRunDistance(ctx.resources, 9.99, DistanceUnit.Km), substring = true)).fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodes(hasContentDescription(spokenDuration(ctx.resources, 4_321L), substring = true)).fetchSemanticsNodes().size)
    }

    // ── A recovered run keeps its pauses ─────────────────────────────────────

    @Test
    fun recoveryThinningKeepsEverySegmentsFirstFix() {
        val pts = (0 until 100).map { fix(3.0 * it, 1_790_000_000L + it) }
        // Stride 10: index 37 is off the stride, yet it opened a segment.
        val thinned = ActiveRunStore.thinForRecovery(pts, maxPoints = 10, segmentStarts = listOf(37))
        assertTrue("thinned: ${thinned.size}", thinned.size < pts.size)
        assertEquals(1, thinned.segmentStarts.size)
        assertEquals("the segment's first fix opens it", pts[37], thinned[thinned.segmentStarts.single()])
    }

    @Test
    fun aRecoveredRunResumedFromTheBootPromptKeepsItsPause() {
        // The resume gate ages the sidecar against the wall clock, so run on it.
        AppClock.offsetMillis = 0
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        // 3 m/s, paused at fix 100 for 20 s (under the fix-gap rule) while the
        // runner walked 30 m: only the recorded pause boundary keeps that walk out.
        val t0 = System.currentTimeMillis() / 1000 - 900
        val pts = (0 until 200).map { i ->
            if (i < 100) fix(3.0 * i, t0 + i) else fix(3.0 * i + 30.0, t0 + i + 19)
        }
        ActiveRunStore.begin(ctx)
        pts.forEachIndexed { i, p ->
            if (i == 100) {
                RunSession.setPaused(true)
                RunSession.setPaused(false)
            }
            if (RunSession.add(p, (5_000L + (p.observedAt - t0) * 1000L) * 1_000_000L)) ActiveRunStore.append(p)
        }
        RunSession.tracking.value = true
        assertEquals(listOf(100), RunSession.decimatedSegmentStarts(Int.MAX_VALUE))
        val liveKm = RunSession.distanceKm.value

        ProcessDeath.kill(ctx)
        rule.launchApp()
        rule.assertTextCountEventually(str(R.string.main_resume_run_title), 1)
        rule.onNode(hasText(str(R.string.track_action_resume)) and hasClickAction() and hasAnyAncestor(isDialog())).performClick()
        rule.waitForState { RunSession.tracking.value && RunSession.points.value.size == pts.size }
        assertEquals("the pause is still a boundary", listOf(100), RunSession.decimatedSegmentStarts(Int.MAX_VALUE))
        assertEquals("the walk during the pause is not distance", liveKm, RunSession.distanceKm.value, 1e-9)
    }

    // ── A pause breaks the segment without an accepted fix ───────────────────

    /** A platform fix [eastM] metres east of (0, 0) at [tSec], [accuracyM] accuracy. */
    private fun location(eastM: Double, tSec: Long, accuracyM: Float): Location =
        Location(LocationManager.GPS_PROVIDER).apply {
            latitude = 0.0
            longitude = eastM / 111_195.0
            time = 1_700_000_000_000L + tSec * 1000L
            elapsedRealtimeNanos = (10_000L + tSec * 1000L) * 1_000_000L
            accuracy = accuracyM
        }

    @Test
    fun aPauseWithoutAnAcceptedFixStillBreaksTheSegment() {
        val lm = ctx.getSystemService(LocationManager::class.java)
        shadowOf(lm).setProviderEnabled(LocationManager.GPS_PROVIDER, true)
        val start = Intent(ctx, RunTrackingService::class.java).setAction("app.milestone.action.START")
        val controller = Robolectric.buildService(RunTrackingService::class.java, start).create().startCommand(0, 1)
        fun deliver(l: Location) {
            shadowOf(lm).simulateLocation(l)
            shadowOf(Looper.getMainLooper()).idle()
        }
        deliver(location(0.0, 0, 5f))
        deliver(location(15.0, 5, 5f)) // a 3 m/s run leg
        assertEquals(5L, RunSession.elapsedSec.value)

        RunSession.setPaused(true)
        // Paused indoors: the only fix is too coarse (50 m > 30 m) for the live gate.
        deliver(location(25.0, 15, 50f))
        RunSession.setPaused(false)
        // First fix after Resume: 20 m from the pre-pause fix, 20 s later (1 m/s).
        deliver(location(35.0, 25, 5f))
        assertEquals(3, RunSession.points.value.size)
        assertEquals("the paused span adds no moving time", 5L, RunSession.elapsedSec.value)
        assertEquals("the post-resume fix opens a new segment", listOf(2), RunSession.decimatedSegmentStarts())
        controller.destroy()
    }

    // ── Undo of an unmeasurable GPS run ──────────────────────────────────────

    @Test
    fun anUnmeasurableGpsRunComesBackWithItsFixesAfterDeleteAndUndo() {
        val end = QaHarness.daysAgoAtMidday(1)
        // Five fixes, every one past the 30 m accuracy gate: QC leaves nothing to
        // measure, but the run and its recorded fixes are the runner's data.
        val points = (0 until 5).map { fix(3.0 * it, end - 4 + it, accuracyM = 50.0) }
        val run = Event.LogRunTrack(points = points, hrPctMax = 0.0, observedAt = end, entryId = end * 1000)
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), run))
        rule.launchApp()
        assertTrue(model.runs.single().gps_unmeasurable)
        HistoryFocus.request(run.entryId, run.observedAt)
        rule.waitForNode(hasTestTag("run-delete"))
        // The fixes load off the main thread; give that fetch time to land.
        repeat(20) {
            Thread.sleep(50)
            rule.waitForIdle()
        }
        rule.onNodeWithTag("run-delete").performScrollTo().performClick()
        val confirm = hasText(str(R.string.main_delete)) and hasClickAction() and hasAnyAncestor(isDialog())
        rule.waitForNode(confirm)
        rule.onNode(confirm).performClick()
        rule.waitForState { model.runs.isEmpty() }
        val undo = hasText(str(R.string.log_undo)) and hasClickAction()
        rule.waitForNode(undo)
        rule.onNode(undo).performClick()
        rule.waitForState { model.runs.isNotEmpty() }
        val back = model.runs.single()
        assertEquals(run.entryId, back.entry_id)
        assertTrue("still the unmeasurable GPS run", back.gps_unmeasurable)
        val relogged = journeyLog().filter { it.variant == "LogRunTrack" }
        assertEquals("seeded + undone", 2, relogged.size)
        val fixes = relogged.last().payload["points"]!!.jsonArray
        assertEquals("every recorded fix comes back", points.size, fixes.size)
    }
}
