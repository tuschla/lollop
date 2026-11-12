package app.milestone

import app.milestone.resources.Res
import app.milestone.resources.track_notif_armed_text
import app.milestone.resources.track_notif_locate_text
import app.milestone.resources.track_notif_progress
import app.milestone.resources.track_notif_recording_hint
import app.milestone.resources.track_notif_signal_lost
import app.milestone.resources.track_state_paused
import app.milestone.resources.track_gates_caption_both
import app.milestone.resources.track_gates_caption_moving
import app.milestone.resources.track_gates_caption_none
import app.milestone.resources.track_gates_caption_precise
import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.sqrt
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * JVM unit tests (no device) for the run-tracking shell: the incremental
 * live-distance accumulator must match a full haversine, and the recorded track,
 * moving time, pace slices and notification state stay consistent.
 *
 * Robolectric, not plain JUnit: the save's and recovery's thinning is the core's
 * (`ThinTrack`), and the JVM loads `libshared.so` into one classloader only (see
 * [EventLogTest]).
 */
@RunWith(RobolectricTestRunner::class)
class RunTrackingUnitTest {

    @Before fun isolate() = AppRuntime.resetForTest()
    @After fun cleanup() = RunSession.reset()

    @Test
    fun runSessionAccumulatesDistanceLikeFullHaversine() {
        RunSession.reset()
        assertEquals(0.0, RunSession.distanceKm.value, 1e-12)
        // Realistic running legs: 30 m north every 10 s = 3 m/s, well under the
        // jitter guard, so every leg is accepted and the incremental sum equals a
        // full haversine.
        val pts = straightTrack(4, firstAt = 0L, stepM = 30.0, stepSec = 10L, lat = 52.52, lon = 13.405)
        // add() takes the fix's monotonic timestamp (elapsedRealtimeNanos); drive it
        // from observedAt so the per-leg dt is a plausible 10 s.
        pts.forEach { RunSession.add(it, it.observedAt * 1_000_000_000L) }
        // The incrementally accumulated distance is the path's true length: a straight
        // run north is the meridian arc between its ends.
        assertEquals(pathKm(pts), RunSession.distanceKm.value, 1e-9)
        assertEquals(pts, RunSession.points.value)

        RunSession.reset()
        assertEquals(0.0, RunSession.distanceKm.value, 1e-12)
        assertTrue(RunSession.points.value.isEmpty())
    }

    @Test
    fun nearAntipodalSegmentIsFiniteNotNaN() {
        // On a near-antipodal leg the haversine `h` can round a hair past
        // 1.0, and Math.asin(x > 1) is NaN — which would poison live distance. The
        // asin-input clamp (h.coerceAtMost(1.0)) keeps the result finite.
        // An exactly antipodal pair (~half the Earth's circumference apart) is the edge.
        val a = GpsPoint(0.0, 0.0, 0, 5.0)
        val b = GpsPoint(0.0, 180.0, 10, 5.0)
        val seg = segmentKm(a, b)
        assertTrue("near-antipodal segment is finite (no NaN)", seg.isFinite())
        // ~π·6371 km with the core's Earth radius (gps.rs EARTH_RADIUS_M).
        assertTrue("near-antipodal distance ≈ half the Earth's circumference", seg in 20_000.0..20_100.0)
    }

    @Test
    fun runSessionCapsGpsJitterSpikeInLiveDistanceLikeTheCore() {
        RunSession.reset()
        // Two clean ~30 m / 10 s legs, then a teleport (~800 km in 1 s). The jitter
        // guard (CoreConstants.MAX_PLAUSIBLE_SPEED_MPS) CAPS the spike leg's distance
        // at that speed·dt rather than dropping it, mirroring the core's `moving_legs`
        // (gps.rs) so live pace matches the of-record moving pace instead of
        // reading slightly slow (the leg's time still elapsed and is counted).
        val clean = listOf(
            GpsPoint(52.52000, 13.4050, 0, 5.0),
            GpsPoint(52.52027, 13.4050, 10, 5.0),
            GpsPoint(52.52054, 13.4050, 20, 5.0),
        )
        clean.forEach { RunSession.add(it, it.observedAt * 1_000_000_000L) }
        val cleanKm = RunSession.distanceKm.value
        // Spike: huge jump in 1 s (monotonic dt = 1 s) → capped at the guard speed
        // × 1 s, never the ~800 km haversine.
        RunSession.add(GpsPoint(57.0, 18.0, 21, 5.0), 21_000_000_000L)
        assertEquals(cleanKm + CoreConstants.MAX_PLAUSIBLE_SPEED_MPS * 1.0 / 1000.0, RunSession.distanceKm.value, 1e-9)
        RunSession.reset()
    }

    @Test
    fun restoreOpensANewSegmentForTheFirstPostResumeFix() {
        // A crash-resume splice must be a SEGMENT BOUNDARY. The app can be dead
        // for hours of wall time while the runner relocates, but the first live fix
        // after restore() arrives only a few MONOTONIC seconds later, so the gap rule
        // (which measures monotonically) would NOT fire: restore() must arm the
        // boundary itself so that displacement is not counted.
        RunSession.reset()
        // Recovered pre-crash track: two clean ~33 m / 10 s legs.
        val recovered = listOf(
            GpsPoint(52.5200, 13.4050, 0, 5.0),
            GpsPoint(52.5203, 13.4050, 10, 5.0),
            GpsPoint(52.5206, 13.4050, 20, 5.0),
        )
        RunSession.restore(recovered)
        val afterRestoreKm = RunSession.distanceKm.value
        val afterRestoreSec = RunSession.elapsedSec.value
        // First live fix after resume: displaced ~44 m from the last recovered fix
        // (observedAt jumps ~3 h, the wall gap while the app was dead) but only 10 s
        // after the restore on the monotonic clock (a plausible 4.4 m/s, WOULD count
        // as a normal leg without the boundary). It must open a new segment: no
        // distance, no moving time added, and the boundary reported to the core.
        val displaced = GpsPoint(52.5210, 13.4050, 10_820, 5.0)
        RunSession.add(displaced, (AppClock.monotonicMillis() + 10_000L) * 1_000_000L)
        assertEquals("splice leg adds no distance", afterRestoreKm, RunSession.distanceKm.value, 1e-9)
        assertEquals("splice leg adds no moving time", afterRestoreSec, RunSession.elapsedSec.value)
        assertEquals("boundary reported at the displaced fix", listOf(3), RunSession.segmentStartIndices())
        RunSession.reset()
    }

    @Test
    fun restoreRebuildsDistanceForARecoveredRun() {
        RunSession.reset()
        // Realistic ~3.3 m/s legs (0.0003° lat ≈ 33 m / 10 s): under the jitter
        // clamp restore() applies, so the recovered distance is the track's length.
        val pts = listOf(
            GpsPoint(48.8566, 2.3522, 0, 5.0),
            GpsPoint(48.8569, 2.3522, 10, 5.0),
            GpsPoint(48.8572, 2.3522, 20, 5.0),
        )
        RunSession.restore(pts)
        assertTrue(RunSession.tracking.value)
        assertEquals(pts, RunSession.points.value)
        assertEquals(pathKm(pts), RunSession.distanceKm.value, 1e-9)
        RunSession.reset()
    }

    @Test
    fun resetClearsArmedAndLastFixElapsed() {
        // The armed-start flows are part of the session's reset contract: a new run
        // must never inherit a stale armed flag or fix stamp.
        RunRecorder.armRequested()
        RunSession.previewFix(GpsPoint(52.52, 13.405, 0, 5.0), 12_345L)
        RunSession.claimSaving()
        RunSession.reset()
        assertFalse("armed cleared on reset", RunSession.armed.value)
        assertEquals("last-fix stamp cleared on reset", 0L, RunSession.lastFixElapsedMs.value)
        assertFalse("saving cleared on reset", RunSession.saving.value)
    }

    @Test
    fun resetBumpsSessionGenerationButAddAndRestoreDoNot() {
        // The armed-teardown race guard: reset() (Cancel/Back/discard) must advance the
        // session token so the service's captured generation diverges and a location
        // result queued on the main looper stands down instead of resurrecting a ghost
        // run + sidecar. Recording a fix (add) and recovering a run (restore) keep the
        // SAME session identity — only a reset ends a session — so the service's stream,
        // whose captured token equals the value at start, keeps recording across them.
        RunSession.reset()
        val g0 = RunSession.generation
        RunSession.reset()
        assertTrue("reset advances the session generation", RunSession.generation > g0)
        val g1 = RunSession.generation
        // A recorded fix does not end the session: the token is stable, so the live
        // stream (captured == g1) keeps recording.
        RunSession.add(GpsPoint(52.52, 13.405, 0, 5.0), 0L)
        RunSession.add(GpsPoint(52.52027, 13.405, 10, 5.0), 10_000_000_000L)
        assertEquals("add() keeps the session generation", g1, RunSession.generation)
        // A crash-recovery restore is a fresh recording under the same identity token;
        // the service started for it captures whatever the current value is.
        RunSession.restore(
            listOf(
                GpsPoint(52.52, 13.405, 0, 5.0),
                GpsPoint(52.5203, 13.405, 10, 5.0),
            ),
        )
        assertEquals("restore() keeps the session generation", g1, RunSession.generation)
        // The next teardown advances it again (monotonic), invalidating that stream.
        RunSession.reset()
        assertTrue("the following reset advances it again", RunSession.generation > g1)
    }

    @Test
    fun resetClearsPausedState() {
        // The notification Pause/Resume actions and the in-app Pause button share the
        // one RunSession.paused flag; a new run must never inherit a stale paused state.
        RunSession.setPaused(true)
        RunSession.reset()
        assertFalse("paused cleared on reset", RunSession.paused.value)
    }

    @Test
    fun notifTitleResPinsStateToResourceId() {
        // The state→resource mapping lives in the pure function, so a swapped id
        // fails here against the actual R.string constants (plain ints on the JVM
        // test classpath). A paused recording selects the paused title; armed/locate
        // ignore the paused flag.
        fun trackingNotifTitleRes(recording: Boolean, armed: Boolean, paused: Boolean) =
            androidStringId(runSurfacePhase(recording, armed, paused, signalLost = false).titleRes.key)
        assertEquals(
            R.string.track_notif_recording_title,
            trackingNotifTitleRes(recording = true, armed = false, paused = false),
        )
        assertEquals(
            R.string.track_notif_paused_title,
            trackingNotifTitleRes(recording = true, armed = false, paused = true),
        )
        assertEquals(
            R.string.track_notif_armed_title,
            trackingNotifTitleRes(recording = false, armed = true, paused = true),
        )
        assertEquals(
            R.string.track_notif_locate_title,
            trackingNotifTitleRes(recording = false, armed = false, paused = true),
        )
    }

    @Test
    fun notifContentSpecPinsStateToResourceIdOrProgress() {
        // The content spec (pure): recording+unpaused shows the raw progress line plus
        // the current pace (resolved via track_notif_progress_pace); paused keeps the
        // "Moving …" line without the pace (track_notif_progress with the progress as
        // its arg): the title "Run paused" and the chip say the pause, the line does not
        // repeat it; a lost signal while recording shows the signal-lost line;
        // armed/locate pin their fixed resources. Asserting the real resources pins
        // the wiring itself, not just the branch logic.
        val progress = "2:00 · 1.00 km"
        val pace = "5:45 /km"
        fun spec(recording: Boolean, armed: Boolean, paused: Boolean, signalLost: Boolean = false, progress: String? = "2:00 · 1.00 km", paceNow: String? = pace) =
            runSurfaceLine(runSurfacePhase(recording, armed, paused, signalLost), progress, paceNow)
        assertEquals(
            RunSurfaceLine.Progress(progress, pace),
            spec(recording = true, armed = false, paused = false),
        )
        // No current pace yet (standing still / fresh segment): progress without pace.
        assertEquals(
            RunSurfaceLine.Progress(progress, null),
            spec(recording = true, armed = false, paused = false, paceNow = null),
        )
        assertEquals(
            RunSurfaceLine.PausedProgress(Res.string.track_notif_progress, progress),
            spec(recording = true, armed = false, paused = true),
        )
        // A lost GPS signal while actively recording replaces the progress line, so the
        // lock screen shows it; a pause outranks it (nothing is expected to record).
        assertEquals(
            RunSurfaceLine.Literal(Res.string.track_notif_signal_lost),
            spec(recording = true, armed = false, paused = false, signalLost = true),
        )
        assertEquals(
            RunSurfaceLine.PausedProgress(Res.string.track_notif_progress, progress),
            spec(recording = true, armed = false, paused = true, signalLost = true),
        )
        // Paused / signal loss only fold while recording; the armed/locate copy is fixed.
        assertEquals(
            RunSurfaceLine.Literal(Res.string.track_notif_armed_text),
            spec(recording = false, armed = true, paused = true, signalLost = true),
        )
        assertEquals(
            RunSurfaceLine.Literal(Res.string.track_notif_locate_text),
            spec(recording = false, armed = false, paused = true, signalLost = true),
        )
        // Null progress (no two-fix track yet) falls back to a state-appropriate
        // literal instead of an empty/absent progress line.
        assertEquals(
            RunSurfaceLine.Literal(Res.string.track_state_paused),
            spec(recording = true, armed = false, paused = true, progress = null),
        )
        assertEquals(
            RunSurfaceLine.Literal(Res.string.track_notif_recording_hint),
            spec(recording = true, armed = false, paused = false, progress = null),
        )
    }

    @Test
    fun signalLossIsDueOnlyPastTheThresholdAndAfterAFix() {
        // Shared by the in-app pill and the service's screen-off watchdog.
        assertFalse("no fix yet → nothing lost", signalLossDue(lastFixElapsedMs = 0L, nowElapsedMs = 60_000L))
        assertFalse("at the threshold is not yet lost", signalLossDue(10_000L, 10_000L + SIGNAL_LOSS_MS))
        assertTrue("past the threshold is lost", signalLossDue(10_000L, 10_000L + SIGNAL_LOSS_MS + 1))
        assertTrue("warned before the segment break", SIGNAL_LOSS_MS < CoreConstants.MAX_FIX_GAP_SEC * 1000L)
    }

    /** Feed [seconds] of 1 Hz fixes moving north at [mps] (0 = standing still), starting
     *  from [state] = (lat, tSec); returns the new state. */
    private fun runFor(state: Pair<Double, Long>, seconds: Int, mps: Double): Pair<Double, Long> {
        var (lat, t) = state
        repeat(seconds) {
            lat += mps / M_PER_DEG
            t += 1
            RunSession.add(GpsPoint(lat, 13.4, t, 5.0), t * 1_000_000_000L)
        }
        return lat to t
    }

    @Test
    fun currentPaceFollowsASurgeWhileTheAverageBarelyMoves() {
        RunSession.reset()
        RunSession.add(GpsPoint(52.5, 13.4, 0, 5.0), 0L)
        // 10 min steady at 5:00/km (3.333 m/s), then a 60 s surge at 4:00/km (4.1667 m/s).
        var s = runFor(52.5 to 0L, 600, 1000.0 / 300.0)
        assertEquals("steady: now ≈ 5:00/km", 5.0, RunSession.currentPaceMinPerKm.value, 0.05)
        s = runFor(s, 60, 1000.0 / 240.0)
        assertEquals("surge: now ≈ 4:00/km within one window", 4.0, RunSession.currentPaceMinPerKm.value, 0.05)
        val avg = averagePaceMinPerKm(RunSession.elapsedSec.value, RunSession.distanceKm.value)
        assertTrue("average barely moves (still ~4:53/km, not 4:00): $avg", avg > 4.8)
        // Standing still for a full window: nothing is moving "now" → unknown, rendered "-".
        runFor(s, 61, 0.0)
        assertFalse(
            "standing still → no current pace",
            RunSession.currentPaceMinPerKm.value.isFinite(),
        )
        RunSession.reset()
        assertFalse("reset clears the current pace", RunSession.currentPaceMinPerKm.value.isFinite())
    }

    @Test
    fun currentPaceRestartsAfterAPauseBoundary() {
        RunSession.reset()
        RunSession.add(GpsPoint(52.5, 13.4, 0, 5.0), 0L)
        val s = runFor(52.5 to 0L, 120, 1000.0 / 300.0)
        assertTrue(RunSession.currentPaceMinPerKm.value.isFinite())
        // Pause: the next accepted fix opens a new segment, so pre-pause legs are not "now".
        RunSession.pauseAndResume()
        runFor(s, 1, 1000.0 / 300.0)
        assertFalse(
            "a fresh segment has no current pace until a window of new legs builds",
            RunSession.currentPaceMinPerKm.value.isFinite(),
        )
        RunSession.reset()
    }

    @Test
    fun restoreForcesArmedFalse() {
        // A recovered run is already recording, never armed: restore() must clear the
        // arm flag even if a crash left it set.
        RunSession.reset()
        RunRecorder.armRequested()
        val pts = listOf(
            GpsPoint(52.5200, 13.4050, 0, 5.0),
            GpsPoint(52.5203, 13.4050, 10, 5.0),
        )
        RunSession.restore(pts)
        assertFalse("armed forced off on restore", RunSession.armed.value)
        assertTrue("restored run is recording", RunSession.tracking.value)
        RunSession.reset()
    }

    @Test
    fun paceBucketsSliceSteadyRunByMovingTime() {
        RunSession.reset()
        RunSession.setPaceBucketMinutes(1)
        // 150 s steady at 3 m/s, 1 Hz along the equator.
        var lon = 0.0
        RunSession.add(GpsPoint(0.0, 0.0, 0, 5.0), 0L)
        for (t in 1..150) {
            lon += 3.0 / M_PER_DEG
            RunSession.add(GpsPoint(0.0, lon, t.toLong(), 5.0), t * 1_000_000_000L)
        }
        // Two full one-minute slices of 180 m (5:33 /km) and a 30 s tail of 90 m.
        assertBuckets(
            listOf(PaceBucket(1 / 0.180, 1.0, true), PaceBucket(1 / 0.180, 1.0, true), PaceBucket(0.5 / 0.090, 0.5, false)),
            RunSession.paceBucketsFlow.value,
        )
        RunSession.reset()
    }

    @Test
    fun standingStillWithGpsWanderAccruesNoDistanceOrElapsed() {
        RunSession.reset()
        // 60 s standing still at 1 Hz: the fix drifts within a ~±2 m envelope but each
        // consecutive-fix step is ≈0.2 m/s peak: every leg is below the stop floor
        // (CoreConstants.MIN_MOVING_SPEED_MPS, mirroring the core's gps.rs `is_stopped`),
        // so NONE accrues live distance or moving time. (A real stationary GPS wanders
        // slowly, not by whole metres per second.) Standing at a red light must not
        // inflate the run.
        RunSession.add(GpsPoint(52.52, 13.405, 0, 5.0), 0L)
        for (t in 1..60) {
            val latOff = 2.0 / M_PER_DEG * kotlin.math.sin(2.0 * Math.PI * t / 60.0)
            RunSession.add(GpsPoint(52.52 + latOff, 13.405, t.toLong(), 5.0), t * 1_000_000_000L)
        }
        assertEquals(0.0, RunSession.distanceKm.value, 1e-12)
        assertEquals(0L, RunSession.elapsedSec.value)
        RunSession.reset()
    }

    @Test
    fun pauseGapAdvancesMovingElapsedByZero() {
        RunSession.reset()
        var lon = 0.0
        // Three 3 m/s legs (t=0..3): 3 s of moving time.
        RunSession.add(GpsPoint(0.0, lon, 0, 5.0), 0L)
        for (t in 1..3) {
            lon += 3.0 / M_PER_DEG
            RunSession.add(GpsPoint(0.0, lon, t.toLong(), 5.0), t * 1_000_000_000L)
        }
        assertEquals(3L, RunSession.elapsedSec.value)
        // A 37 s fix gap (> MAX_FIX_GAP_SEC): a pause bridge, its span adds ZERO
        // moving time (the runner may have stood/relocated). Elapsed stays at 3 s.
        lon += 3.0 / M_PER_DEG
        RunSession.add(GpsPoint(0.0, lon, 40, 5.0), 40_000_000_000L)
        assertEquals(3L, RunSession.elapsedSec.value)
        // Three more 3 m/s legs (t=41..43): moving time resumes, +3 s (never +40).
        for (t in 41..43) {
            lon += 3.0 / M_PER_DEG
            RunSession.add(GpsPoint(0.0, lon, t.toLong(), 5.0), t * 1_000_000_000L)
        }
        assertEquals(6L, RunSession.elapsedSec.value)
        RunSession.reset()
    }

    @Test
    fun theSaveEventCarriesTheDecimatedTrackItsPausesAndItsLastFix() {
        RunSession.reset()
        var lon = 0.0
        RunSession.add(GpsPoint(0.0, lon, 1_000, 5.0), 0L)
        for (t in 1..80) {
            if (t == 41) RunSession.pauseAndResume()
            lon += 3.0 / M_PER_DEG
            RunSession.add(GpsPoint(0.0, lon, 1_000L + t, 5.0), t * 995_000_000L)
        }
        // The Finish review previews this very event's figures (Core.savedFigures),
        // and Save sends it, so both read the same track, pauses and date.
        val event = RunSession.trackEventForCore(entryId = 7L, workoutType = WorkoutType.Easy, sessionRpe = 4)
        assertEquals(RunSession.decimatedTrackForCore(), event.points)
        assertEquals(listOf(41), event.segmentStarts)
        assertEquals("dated at the last fix", 1_080L, event.observedAt)
        assertEquals(7L, event.entryId)
        assertEquals(WorkoutType.Easy, event.workoutType)
        assertEquals(4, event.sessionRpe)
        RunSession.reset()
    }

    @Test
    fun theReviewRouteSplitsAtPausesAndTheGateCaptionExplainsOnlyWhatIsOn() {
        val pts = (0..4).map { GpsPoint(52.0 + it * 0.0003, 13.0, it * 10L, 5.0) }
        // A pause opened a new segment at index 3: no bridging line across it.
        val segs = trackSegments(pts, listOf(3))
        assertEquals(listOf(3, 2), segs.map { it.size })
        assertEquals(pts[3].lat, segs[1].first().lat, 0.0)
        assertEquals(30L, segs[1].first().observedAt)
        assertEquals(listOf(5), trackSegments(pts, emptyList()).map { it.size })
        assertTrue(trackSegments(emptyList(), listOf(0)).isEmpty())

        assertEquals(Res.string.track_gates_caption_none, startGatesCaptionRes(precise = false, moving = false))
        assertEquals(Res.string.track_gates_caption_precise, startGatesCaptionRes(precise = true, moving = false))
        assertEquals(Res.string.track_gates_caption_moving, startGatesCaptionRes(precise = false, moving = true))
        assertEquals(Res.string.track_gates_caption_both, startGatesCaptionRes(precise = true, moving = true))
    }

    @Test
    fun stationaryGapDoesNotSlowTheCurrentPaceBucket() {
        RunSession.reset()
        RunSession.setPaceBucketMinutes(1)
        var lon = 0.0
        // 30 s of moving at 3 m/s → an OPEN (partial) 30 s slice at ~5.56 min/km.
        RunSession.add(GpsPoint(0.0, lon, 0, 5.0), 0L)
        for (t in 1..30) {
            lon += 3.0 / M_PER_DEG
            RunSession.add(GpsPoint(0.0, lon, t.toLong(), 5.0), t * 1_000_000_000L)
        }
        val before = RunSession.paceBucketsFlow.value.last()
        assertFalse("open trailing slice", before.complete)
        // 120 s standing still at 1 Hz: the fix drifts within a ~±2 m envelope but every
        // consecutive-fix step is well below the stop floor (a real stationary GPS
        // wanders slowly, not by whole metres/s), and each 1 s dt is under the
        // segment-break gap, so no boundary opens either. Every such leg must be DROPPED
        // from the pace accumulator: standing at a red light must not slow the slice.
        for (t in 31..150) {
            val latOff = 2.0 / M_PER_DEG * kotlin.math.sin(2.0 * Math.PI * (t - 30) / 60.0)
            RunSession.add(GpsPoint(0.0 + latOff, lon, t.toLong(), 5.0), t * 1_000_000_000L)
        }
        val after = RunSession.paceBucketsFlow.value.last()
        assertFalse("still the same open slice, never a fabricated full one", after.complete)
        assertEquals("stationary gap adds no moving time to the slice", before.minutes, after.minutes, 1e-9)
        assertEquals("stationary gap does not slow the pace", before.paceMinPerKm, after.paceMinPerKm, 1e-9)
        assertTrue("no completed slice from an idle stretch", RunSession.paceBucketsFlow.value.none { it.complete })
        // A rebuild from the stored track (a size change and back) agrees with the
        // live feed: neither counts the standing legs.
        val live = RunSession.paceBucketsFlow.value
        RunSession.setPaceBucketMinutes(2)
        RunSession.setPaceBucketMinutes(1)
        assertEquals(live, RunSession.paceBucketsFlow.value)
        RunSession.reset()
    }

    @Test
    fun steadyRunElapsedEqualsWallSpanAndDistanceUnchanged() {
        RunSession.reset()
        var lon = 0.0
        val pts = ArrayList<GpsPoint>()
        pts.add(GpsPoint(0.0, 0.0, 0, 5.0))
        for (t in 1..150) {
            lon += 3.0 / M_PER_DEG
            pts.add(GpsPoint(0.0, lon, t.toLong(), 5.0))
        }
        pts.forEach { RunSession.add(it, it.observedAt * 1_000_000_000L) }
        // Clean run, no pauses/stops/jitter: every leg is moving, so live moving time
        // equals the wall-clock span, the parity the of-record moving clock holds too.
        val wallSpan = pts.last().observedAt - pts.first().observedAt
        assertEquals(wallSpan, RunSession.elapsedSec.value)
        // Distance is still the whole path: the stop floor changed nothing here.
        assertEquals(pathKm(pts), RunSession.distanceKm.value, 1e-9)
        RunSession.reset()
    }

    @Test
    fun restoreMatchesLivePathForACleanRun() {
        var lon = 0.0
        val pts = ArrayList<GpsPoint>()
        pts.add(GpsPoint(0.0, 0.0, 0, 5.0))
        for (t in 1..120) {
            lon += 3.0 / M_PER_DEG
            pts.add(GpsPoint(0.0, lon, t.toLong(), 5.0))
        }
        // Live path (monotonic clock driven from observedAt).
        RunSession.reset()
        pts.forEach { RunSession.add(it, it.observedAt * 1_000_000_000L) }
        val liveKm = RunSession.distanceKm.value
        val liveElapsed = RunSession.elapsedSec.value
        // Crash-recovered path: the same fixes replayed through restore() must rebuild
        // the identical distance AND moving elapsed (both gated by the same stop floor
        // and jitter ceiling).
        RunSession.reset()
        RunSession.restore(pts)
        assertEquals(liveKm, RunSession.distanceKm.value, 1e-9)
        assertEquals(liveElapsed, RunSession.elapsedSec.value)
        RunSession.reset()
    }

    @Test
    fun decimatedTrackReturnsAShortTrackUnthinned() {
        RunSession.reset()
        val pts = listOf(
            GpsPoint(0.0, 0.0000, 0, 5.0),
            GpsPoint(0.0, 0.0003, 1, 5.0),
            GpsPoint(0.0, 0.0006, 2, 5.0),
        )
        pts.forEach { RunSession.add(it, it.observedAt * 1_000_000_000L) }
        // Below the cap → identical to the un-decimated of-record track.
        assertEquals(RunSession.points.value, RunSession.decimatedTrackForCore())
        RunSession.reset()
    }

    @Test
    fun decimatedTrackKeepsEndpointsAndRespectsCap() {
        RunSession.reset()
        // 1200 fixes at ~3 m/s, 1 Hz, no pauses.
        var lon = 0.0
        val pts = ArrayList<GpsPoint>()
        pts.add(GpsPoint(0.0, 0.0, 0, 5.0))
        for (t in 1..1199) {
            lon += 3.0 / M_PER_DEG
            pts.add(GpsPoint(0.0, lon, t.toLong(), 5.0))
        }
        pts.forEach { RunSession.add(it, it.observedAt * 1_000_000_000L) }
        val full = RunSession.points.value
        val cap = 300
        val dec = RunSession.decimatedTrackForCore(cap)
        assertTrue("must thin below the source: ${dec.size}", dec.size < full.size)
        // Strided count never exceeds the cap; with no pause boundaries only the
        // last fix can push it one past cap.
        assertTrue("cap respected: ${dec.size}", dec.size <= cap + 1)
        assertEquals("first fix always kept", full.first(), dec.first())
        assertEquals("last fix always kept", full.last(), dec.last())
        RunSession.reset()
    }

    @Test
    fun decimatedTrackAlwaysKeepsPauseBoundaries() {
        RunSession.reset()
        var lon = 0.0
        val pts = ArrayList<GpsPoint>()
        pts.add(GpsPoint(0.0, 0.0, 0, 5.0)) // index 0
        // Segment A: indices 1..200.
        for (t in 1..200) {
            lon += 3.0 / M_PER_DEG
            pts.add(GpsPoint(0.0, lon, t.toLong(), 5.0))
        }
        // A 61 s gap (> MAX_FIX_GAP_SEC) opens a new segment at index 201.
        var t = 261L
        for (k in 1..199) {
            lon += 3.0 / M_PER_DEG
            pts.add(GpsPoint(0.0, lon, t, 5.0))
            t += 1
        }
        pts.forEach { RunSession.add(it, it.observedAt * 1_000_000_000L) }
        val full = RunSession.points.value // 401 fixes; boundary at index 201
        val boundaryFix = full[201]
        // Force a coarse stride (401/40 → stride 11) so index 201 is NOT on the
        // uniform grid; only the mandatory-boundary rule can retain it.
        val dec = RunSession.decimatedTrackForCore(40)
        assertTrue(
            "the pause-boundary fix (index 201, observedAt 261) must survive decimation",
            dec.contains(boundaryFix),
        )
        RunSession.reset()
    }

    @Test
    fun decimationPreservesDistanceWithinOnePercent() {
        RunSession.reset()
        // 900 fixes at ~3 m/s with realistic sub-metre lateral jitter (~0.44 m).
        var lon = 0.0
        val pts = ArrayList<GpsPoint>()
        pts.add(GpsPoint(0.0, 0.0, 0, 5.0))
        for (t in 1..900) {
            lon += 3.0 / M_PER_DEG
            val lat = 0.000_004 * Math.sin(t * 1.3)
            pts.add(GpsPoint(lat, lon, t.toLong(), 5.0))
        }
        pts.forEach { RunSession.add(it, it.observedAt * 1_000_000_000L) }
        val full = RunSession.points.value
        val fullKm = pathKm(full)
        // Cap 300 over 900 fixes → stride 3 (a ~3 s cadence).
        val decKm = pathKm(RunSession.decimatedTrackForCore(300))
        assertEquals(fullKm, decKm, fullKm * 0.01)
        RunSession.reset()
    }

    @Test
    fun trackForCoreKeepsTrueCoordsAndReportsSegmentStarts() {
        RunSession.reset()
        // Segment 1 (indices 0–2), then a pause + ~111 km relocation, then
        // segment 2 (indices 3–4). The boundary is registered at index 3.
        val seg1 = listOf(
            GpsPoint(0.0, 0.000, 0, 5.0),
            GpsPoint(0.0, 0.001, 10, 5.0),
            GpsPoint(0.0, 0.002, 20, 5.0),
        )
        seg1.forEach { RunSession.add(it, it.observedAt * 1_000_000_000L) }
        RunSession.pauseAndResume()
        val seg2 = listOf(
            GpsPoint(0.0, 1.000, 80, 5.0),
            GpsPoint(0.0, 1.001, 90, 5.0),
        )
        seg2.forEach { RunSession.add(it, it.observedAt * 1_000_000_000L) }
        // trackForCore returns the TRUE captured coordinates, UN-shifted (older
        // builds re-anchored segment 2 onto segment 1: now the core is
        // segment-aware, so the shell keeps the real geometry).
        assertEquals(seg1 + seg2, RunSession.points.value)
        assertEquals(listOf(3), RunSession.segmentStartIndices())
        RunSession.reset()
    }

    @Test
    fun decimatedSegmentStartsRemapToTheThinnedBoundaryFix() {
        RunSession.reset()
        var lon = 0.0
        val pts = ArrayList<GpsPoint>()
        pts.add(GpsPoint(0.0, 0.0, 0, 5.0))
        for (t in 1..200) {
            lon += 3.0 / M_PER_DEG
            pts.add(GpsPoint(0.0, lon, t.toLong(), 5.0))
        }
        // A 61 s gap (> MAX_FIX_GAP_SEC) opens a new segment at index 201.
        var t = 261L
        for (k in 1..199) {
            lon += 3.0 / M_PER_DEG
            pts.add(GpsPoint(0.0, lon, t, 5.0))
            t += 1
        }
        pts.forEach { RunSession.add(it, it.observedAt * 1_000_000_000L) }
        val full = RunSession.points.value
        val boundaryFix = full[201]
        // Coarse cap so index 201 is NOT on the uniform grid: only the boundary
        // rule retains it, and its remapped index must point back at the same fix.
        val dec = RunSession.decimatedTrackForCore(40)
        val starts = RunSession.decimatedSegmentStarts(40)
        assertEquals("one remapped boundary", 1, starts.size)
        assertEquals("remapped start points at the true boundary fix", boundaryFix, dec[starts[0]])
        RunSession.reset()
    }

    @Test
    fun shortRunGateUsesMovingTimeNotWallSpan() {
        // A run over the distance bar with under 3 min MOVING, padded by a pause to
        // a longer wall span, is still short: the gate reads RunSession.elapsedSec
        // (moving time only), never the first→last span.
        RunSession.reset()
        var lon = 0.0
        RunSession.add(GpsPoint(0.0, lon, 0, 5.0), 0L)
        // 170 legs of 5 m/s: 0.85 km in 170 s moving (< MIN_RUN_SEC).
        for (t in 1..170) {
            lon += 5.0 / M_PER_DEG
            RunSession.add(GpsPoint(0.0, lon, t.toLong(), 5.0), t * 1_000_000_000L)
        }
        // A fix gap over MAX_FIX_GAP_SEC is a pause bridge: no moving time, but it
        // stretches the wall span to 400 s.
        lon += 5.0 / M_PER_DEG
        RunSession.add(GpsPoint(0.0, lon, 400, 5.0), 400_000_000_000L)
        val captured = RunSession.points.value
        assertTrue("wall span passes the bar", captured.last().observedAt - captured.first().observedAt >= CoreConstants.MIN_RUN_SEC)
        assertTrue("distance passes the bar", RunSession.distanceKm.value >= CoreConstants.MIN_RUN_KM)
        assertTrue("moving time does not", isShortRun(RunSession.distanceKm.value, RunSession.elapsedSec.value))
        RunSession.reset()
    }

    @Test
    fun addDropsNonMonotonicFix() {
        // A fix whose MONOTONIC stamp is EARLIER than the previous one (dtMs < 0),
        // e.g. an out-of-order fused batch on an NTP step, is dropped whole: never
        // appended out of order, where it would open a spurious segment.
        RunSession.reset()
        RunSession.add(GpsPoint(0.0, 0.0000, 0, 5.0), 0L)
        RunSession.add(GpsPoint(0.0, 0.0003, 10, 5.0), 10_000_000_000L)
        val pointsBefore = RunSession.points.value
        val distBefore = RunSession.distanceKm.value
        val elapsedBefore = RunSession.elapsedSec.value
        // Monotonic stamp 5 s < the previous 10 s → dropped, and add reports it so
        // the crash sidecar (which mirrors only accepted fixes) never persists it.
        assertFalse(RunSession.add(GpsPoint(0.0, 0.0006, 20, 5.0), 5_000_000_000L))
        assertEquals("non-monotonic fix not appended", pointsBefore, RunSession.points.value)
        assertEquals("no distance from a dropped fix", distBefore, RunSession.distanceKm.value, 1e-12)
        assertEquals("no moving time from a dropped fix", elapsedBefore, RunSession.elapsedSec.value)
        assertTrue("no spurious segment opened", RunSession.segmentStartIndices().isEmpty())
        RunSession.reset()
    }

    @Test
    fun paceBucketsDropThePauseBridgeCapAJitterLegAndRebuildOnASizeChange() {
        // One-minute slices of MOVING time, worked by hand: 100 s at 3 m/s, a 40 s fix
        // gap (> MAX_FIX_GAP_SEC: a pause bridge) over which the runner moved 200 m,
        // 30 s more at 3 m/s, then a 1 s teleport capped at MAX_PLAUSIBLE_SPEED_MPS.
        // Moving: 131 s; the bridge's 40 s and 200 m count nowhere.
        RunSession.reset()
        RunSession.setPaceBucketMinutes(1)
        var lon = 0.0
        RunSession.add(GpsPoint(0.0, lon, 0, 5.0), 0L)
        for (t in 1..100) {
            lon += 3.0 / M_PER_DEG
            RunSession.add(GpsPoint(0.0, lon, t.toLong(), 5.0), t * 1_000_000_000L)
        }
        lon += 200.0 / M_PER_DEG
        RunSession.add(GpsPoint(0.0, lon, 140, 5.0), 140_000_000_000L)
        for (t in 141..170) {
            lon += 3.0 / M_PER_DEG
            RunSession.add(GpsPoint(0.0, lon, t.toLong(), 5.0), t * 1_000_000_000L)
        }
        RunSession.add(GpsPoint(0.0, lon + 5.0, 171, 5.0), 171_000_000_000L)

        val capKm = CoreConstants.MAX_PLAUSIBLE_SPEED_MPS * 1.0 / 1000.0
        // 0:00-1:00 is 180 m; 1:00-2:00 is the last 40 s before the bridge and the
        // first 20 s after it, 180 m again; the 11 s tail is 30 m plus the capped leg.
        val tail = PaceBucket(paceMinPerKm = (11 / 60.0) / (0.030 + capKm), minutes = 11 / 60.0, complete = false)
        val oneMinute = RunSession.paceBucketsFlow.value
        assertBuckets(listOf(PaceBucket(1 / 0.180, 1.0, true), PaceBucket(1 / 0.180, 1.0, true), tail), oneMinute)
        // A size change rebuilds from the stored track: one 2-minute slice of 360 m
        // and the same tail.
        RunSession.setPaceBucketMinutes(2)
        assertBuckets(listOf(PaceBucket(2 / 0.360, 2.0, true), tail), RunSession.paceBucketsFlow.value)
        // Back to one minute, the rebuild equals what the live feed folded fix by fix.
        RunSession.setPaceBucketMinutes(1)
        assertEquals(oneMinute, RunSession.paceBucketsFlow.value)
        RunSession.reset()
    }

    @Test
    fun savedTrackIsByteIdenticalToTheAppendedFixes() {
        // The append-only backing store exposes EXACTLY the surviving fixes (a
        // dropped non-monotonic fix never appears; a pause boundary is reported),
        // and the saved forms (decimatedTrackForCore / segmentStartIndices) are pure
        // functions of that list.
        RunSession.reset()
        val expected = ArrayList<GpsPoint>()
        var lon = 0.0
        fun feed(p: GpsPoint, nanos: Long, survives: Boolean) {
            RunSession.add(p, nanos)
            if (survives) expected.add(p)
        }
        feed(GpsPoint(0.0, lon, 0, 5.0), 0L, true) // index 0
        for (t in 1..50) {
            lon += 3.0 / M_PER_DEG
            feed(GpsPoint(0.0, lon, t.toLong(), 5.0), t * 1_000_000_000L, true) // 1..50
        }
        // A non-monotonic fix (monotonic stamp EARLIER than the last) is DROPPED -
        // never appended, so it must not appear in the saved track.
        feed(GpsPoint(0.0, lon + 1.0 / M_PER_DEG, 51, 5.0), 25_000_000_000L, false)
        // Pause boundary at index 51 (50 s monotonic gap), then more legs.
        lon += 3.0 / M_PER_DEG
        feed(GpsPoint(0.0, lon, 100, 5.0), 100_000_000_000L, true) // index 51 (boundary)
        for (t in 101..150) {
            lon += 3.0 / M_PER_DEG
            feed(GpsPoint(0.0, lon, t.toLong(), 5.0), t * 1_000_000_000L, true) // 52..101
        }
        assertEquals("points expose the surviving fixes", expected, RunSession.points.value)
        // Decimation with a cap above the size is the identity, so it equals the
        // oracle list, confirming the of-record path reads the same content.
        assertEquals(expected, RunSession.decimatedTrackForCore(10_000))
        assertEquals("boundary reported at index 51", listOf(51), RunSession.segmentStartIndices())
        RunSession.reset()
    }

    @Test
    fun thinForRecoveryCapsLongTrackAndKeepsEndpoints() {
        // A very long interrupted run must thin to the save-time cap on recovery so it
        // doesn't reload unbounded points into memory. Endpoints are always kept.
        val pts = (0 until 20_000).map { GpsPoint(0.0, it * 0.00001, it.toLong(), 5.0) }
        val thinned = ActiveRunStore.thinForRecovery(pts, 5400)
        assertTrue("thins below the source: ${thinned.size}", thinned.size < pts.size)
        assertTrue("respects the cap: ${thinned.size}", thinned.size <= 5400 + 1)
        assertEquals("first fix kept", pts.first(), thinned.first())
        assertEquals("last fix kept", pts.last(), thinned.last())
        // Below the cap → returned unchanged.
        val small = pts.take(100)
        assertEquals(small, ActiveRunStore.thinForRecovery(small, 5400))
    }

    @Test
    fun trackSnapshotEqualsHonoursTheListContract() {
        // Equal content compares equal whether the other side is a snapshot on a
        // different backing array or a plain List, both directions, so equality
        // stays transitive.
        val p1 = GpsPoint(52.5200, 13.4050, 0, 5.0)
        val p2 = GpsPoint(52.5203, 13.4050, 10, 5.0)
        // Each run resets to a FRESH backing array, so these two snapshots hold the
        // same fixes on distinct arrays — the transitivity-breaking case.
        RunSession.reset()
        RunSession.add(p1, 0L)
        RunSession.add(p2, 10_000_000_000L)
        val snapA = RunSession.points.value
        RunSession.reset()
        RunSession.add(p1, 0L)
        RunSession.add(p2, 10_000_000_000L)
        val snapB = RunSession.points.value
        val plain = listOf(p1, p2)

        assertEquals(snapA, snapB)
        assertEquals(snapB, snapA)
        assertEquals(plain, snapA)
        assertEquals(snapA, plain)
        // hashCode consistent with equals, and with a plain List's element-wise hash.
        assertEquals(snapA.hashCode(), snapB.hashCode())
        assertEquals(plain.hashCode(), snapA.hashCode())
        RunSession.reset()
        RunSession.add(p1, 0L)
        val snapShort = RunSession.points.value
        assertNotEquals(snapA, snapShort)
    }

    @Test
    fun resetClearsTheLocateFailureLatch() {
        // A failed location start latches so the screen stops re-launching the
        // service into the same failure; leaving the tracker must not carry the latch
        // into the next session.
        RunSession.markLocationFailed()
        RunSession.reset()
        assertFalse(RunSession.locationFailed.value)
    }

    @Test
    fun paceCueComparesTheRollingPaceWithThePlanRange() {
        // Plan range 5:00–6:00 /km (300–360 s/km).
        assertEquals(PaceCue.OnTarget, paceCue(5.5, 300, 360))
        assertEquals("bounds are on target", PaceCue.OnTarget, paceCue(5.0, 300, 360))
        assertEquals(PaceCue.OnTarget, paceCue(6.0, 300, 360))
        assertEquals("slower than the range → speed up", PaceCue.SpeedUp, paceCue(6.5, 300, 360))
        assertEquals("faster than the range → ease off", PaceCue.EaseOff, paceCue(4.5, 300, 360))
        // Bound order doesn't matter.
        assertEquals(PaceCue.SpeedUp, paceCue(6.5, 360, 300))
        // Nothing to judge: no current pace, or no pace range on the plan.
        assertEquals(null, paceCue(Double.NaN, 300, 360))
        assertEquals(null, paceCue(5.5, 0, 0))
    }

    @Test
    fun singleStepPlanDistanceComesFromTheRunHead() {
        val easy = PrescriptionView(
            summary = "7 km · 6:03–7:14 /km (easy) · HR 120–146 bpm",
            exercise = "Easy run",
            run_head = "7 km",
            face_lead = "7 km",
            run_head_km = 7.0,
        )
        assertEquals(7.0, singleStepPlanKm(easy)!!, 1e-9)
        // A warm-up + reps + cool-down guide carries its own per-step countdown.
        val reps = easy.copy(
            summary = "6 × 800 m · 3:40–3:50 /km",
            run_head = "6 × 800 m",
            face_lead = "6 × 800 m",
            run_head_km = null,
            run_structure = RunStructureView(reps = 6, rep_m = 800, recovery_sec = 120, warmup_min = 15, cooldown_min = 10),
        )
        assertEquals(null, singleStepPlanKm(reps))
    }

    @Test
    fun planHeadlineReexpressesTheLeadDistanceInTheUserUnit() {
        val easy = PrescriptionView(summary = "7 km · 6:03–7:14 /km (easy)", exercise = "Easy run", face_lead = "7 km", run_head = "7 km", run_head_km = 7.0)
        assertEquals("Easy run · 7 km", runPlanHeadline(easy, DistanceUnit.Km))
        assertEquals("Easy run · 4.3 mi", runPlanHeadline(easy, DistanceUnit.Mi))
        // A structural head stays as the core wrote it.
        val reps = easy.copy(face_lead = "6 × 800 m", run_head = "6 × 800 m", run_head_km = null)
        assertEquals("Easy run · 6 × 800 m", runPlanHeadline(reps, DistanceUnit.Mi))
    }

    @Test
    fun theUntrimmedWindowReadsTheRunsOfRecordDistance() {
        // The raw parsed track (14.0 km) runs a little long of the core's QC'd moving
        // distance (13.97 km): the whole window reads the History figure exactly, so
        // "Keeps 8.68 mi" matches the list, not a raw 8.70.
        val record = 13.97
        assertEquals(record, trimPreviewShare(14.0, 14.0, record), 0.0)
        assertEquals(runDistLabel(record, DistanceUnit.Mi), runDistLabel(trimPreviewShare(14.0, 14.0, record), DistanceUnit.Mi))
        // Half the raw track keeps half the of-record distance.
        assertEquals(record / 2.0, trimPreviewShare(7.0, 14.0, record), 1e-12)
        // Unknown totals fall back to the raw window.
        assertEquals(3.0, trimPreviewShare(3.0, 0.0, record), 0.0)
        assertEquals(3.0, trimPreviewShare(3.0, 14.0, 0.0), 0.0)
    }

    @Test
    fun trimKeepsReadsMovingTimeNotTheWallClockSpanOfAPausedRun() {
        // 60 s of running (30 m every 10 s), 20 s standing still, a pause, then
        // another 60 s of running: 4:20 on the wall clock, 2:00 moving.
        val seg1 = straightTrack(7, firstAt = 0L, stepM = 30.0, stepSec = 10L) +
            GpsPoint(47.0 + 180.0 / M_PER_DEG, 8.0, 80L, 5.0)
        val seg2 = straightTrack(7, firstAt = 200L, stepM = 30.0, stepSec = 10L, lat = 47.01)
        val points = timedTrackFromSegments(listOf(seg1, seg2))
        val last = points.lastIndex
        assertEquals(260L, points[last].timeSec - points[0].timeSec)
        assertEquals(120.0, points[last].movingSec, 0.0)

        // The untouched window reads the run detail's moving time, not "4:20".
        assertEquals("2:00", formatElapsed(trimKeptMovingSec(points, 0, last, recordSec = 120.0).toLong()))
        // The core's QC'd figure wins over the raw track's, as for distance.
        assertEquals(118, trimKeptMovingSec(points, 0, last, recordSec = 118.0))
        // Trimming the first segment away keeps only the second one's minute.
        assertEquals(60, trimKeptMovingSec(points, seg1.size, last, recordSec = 120.0))
        // Keeping the first segment drops the standing tail and the pause with the second.
        assertEquals(60, trimKeptMovingSec(points, 0, seg1.size - 1, recordSec = 120.0))
        // Unknown of-record time falls back to the raw moving seconds.
        assertEquals(120, trimKeptMovingSec(points, 0, last, recordSec = 0.0))
        // Fewer than two kept fixes keep no time.
        assertEquals(0, trimKeptMovingSec(points, 3, 3, recordSec = 120.0))
        assertEquals(0, trimKeptMovingSec(points, -1, -1, recordSec = 120.0))
    }
}

/** Path length (km) over [points] by the flat-earth (equirectangular) formula on the
 *  core's Earth radius: a measure independent of the app's haversine, exact along the
 *  equator or a meridian and within ~1e-12 of it on these few-metre legs. */
private fun pathKm(points: List<GpsPoint>): Double = points.zipWithNext { a, b ->
    val dLat = (b.lat - a.lat) * PI / 180.0
    val dLon = (b.lon - a.lon) * PI / 180.0 * cos((a.lat + b.lat) / 2 * PI / 180.0)
    CoreConstants.EARTH_RADIUS_M * sqrt(dLat * dLat + dLon * dLon) / 1000.0
}.sum()

/** [actual] slices equal [expected] up to floating-point rounding. */
private fun assertBuckets(expected: List<PaceBucket>, actual: List<PaceBucket>) {
    assertEquals("slices $actual", expected.size, actual.size)
    expected.zip(actual).forEachIndexed { i, (e, a) ->
        assertEquals("slice $i complete", e.complete, a.complete)
        assertEquals("slice $i minutes", e.minutes, a.minutes, 1e-9)
        assertEquals("slice $i pace", e.paceMinPerKm, a.paceMinPerKm, 1e-9)
    }
}

/** The pause-boundary indices of the live track, sorted (no decimation). */
private fun RunSession.segmentStartIndices(): List<Int> = decimatedSegmentStarts(Int.MAX_VALUE)

/** A Pause then Resume with no fix in between: the next fix opens a new segment. */
private fun RunSession.pauseAndResume() {
    setPaused(true)
    setPaused(false)
}

