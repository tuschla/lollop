package app.milestone

import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

/**
 * JVM unit tests (no device) for the run-tracking shell: the incremental
 * live-distance accumulator must match a full haversine, and the recorded track,
 * moving time, pace slices and notification state stay consistent.
 */
class RunTrackingUnitTest {

    @Before fun isolate() = RunSession.reset()
    @After fun cleanup() = RunSession.reset()

    private fun obj(e: Event) = (e.toJson() as JsonObject)

    @Test
    fun runSessionAccumulatesDistanceLikeFullHaversine() {
        RunSession.reset()
        assertEquals(0.0, RunSession.distanceKm.value, 1e-12)
        // Realistic running legs: ~30 m north every 10 s ≈ 3 m/s, well under the
        // 12.5 m/s jitter guard, so every leg is accepted and the incremental sum
        // equals a full haversine. (0.00027° lat ≈ 30 m.)
        val pts = listOf(
            GpsPoint(52.52000, 13.4050, 0, 5.0),
            GpsPoint(52.52027, 13.4050, 10, 5.0),
            GpsPoint(52.52054, 13.4050, 20, 5.0),
            GpsPoint(52.52081, 13.4050, 30, 5.0),
        )
        // add() now takes the fix's monotonic timestamp (elapsedRealtimeNanos);
        // drive it from observedAt so the per-leg dt is a plausible 10 s.
        pts.forEach { RunSession.add(it, it.observedAt * 1_000_000_000L) }
        // Incrementally-accumulated distance is identical to a one-shot haversine
        // over the whole track: the O(1)-per-fix path can't drift from the truth.
        assertEquals(haversineKm(pts), RunSession.distanceKm.value, 1e-9)
        assertEquals(pts, RunSession.points.value)

        RunSession.reset()
        assertEquals(0.0, RunSession.distanceKm.value, 1e-12)
        assertTrue(RunSession.points.value.isEmpty())
    }

    @Test
    fun nearAntipodalSegmentIsFiniteNotNaN() {
        // On a near-antipodal leg the haversine `h` can round a hair past
        // 1.0, and Math.asin(x > 1) is NaN — which would poison live distance and the
        // oracle. The asin-input clamp (h.coerceAtMost(1.0)) keeps the result finite.
        // An exactly antipodal pair (~half the Earth's circumference apart) is the edge.
        val a = GpsPoint(0.0, 0.0, 0, 5.0)
        val b = GpsPoint(0.0, 180.0, 10, 5.0)
        val seg = segmentKm(a, b)
        assertTrue("near-antipodal segment is finite (no NaN)", seg.isFinite())
        // ~π·6371 km with the core-matching 6371.0 km radius (running.rs EARTH_RADIUS_M).
        assertTrue("near-antipodal distance ≈ half the Earth's circumference", seg in 20_000.0..20_100.0)
        assertTrue("oracle over a near-antipodal track is finite too", haversineKm(listOf(a, b)).isFinite())
    }

    @Test
    fun runSessionCapsGpsJitterSpikeInLiveDistanceLikeTheCore() {
        RunSession.reset()
        // Two clean ~30 m / 10 s legs, then a teleport (~800 km in 1 s). The jitter
        // guard (MAX_PLAUSIBLE_SPEED_MPS = 12.0 m/s) CAPS the spike leg's distance at
        // 12 m/s·dt rather than dropping it, mirroring the core's `moving_legs`
        // (running.rs) so live pace matches the of-record moving pace instead of
        // reading slightly slow (the leg's time still elapsed and is counted).
        val clean = listOf(
            GpsPoint(52.52000, 13.4050, 0, 5.0),
            GpsPoint(52.52027, 13.4050, 10, 5.0),
            GpsPoint(52.52054, 13.4050, 20, 5.0),
        )
        clean.forEach { RunSession.add(it, it.observedAt * 1_000_000_000L) }
        val cleanKm = RunSession.distanceKm.value
        // Spike: huge jump in 1 s (monotonic dt = 1 s) → capped at 12 m/s · 1 s =
        // 12 m = 0.012 km added, never the ~800 km haversine.
        RunSession.add(GpsPoint(57.0, 18.0, 21, 5.0), 21_000_000_000L)
        assertEquals(cleanKm + 0.012, RunSession.distanceKm.value, 1e-9)
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
        // later in the monotonic timebase (a plausible 4.4 m/s, WOULD count as a
        // normal leg without the boundary). It must open a new segment: no distance,
        // no moving time added, and the boundary reported to the core.
        val displaced = GpsPoint(52.5210, 13.4050, 10_820, 5.0)
        RunSession.add(displaced, 10_000_000_000L)
        assertEquals("splice leg adds no distance", afterRestoreKm, RunSession.distanceKm.value, 1e-9)
        assertEquals("splice leg adds no moving time", afterRestoreSec, RunSession.elapsedSec.value)
        assertEquals("boundary reported at the displaced fix", listOf(3), RunSession.segmentStartIndices())
        RunSession.reset()
    }

    @Test
    fun restoreRebuildsDistanceForARecoveredRun() {
        RunSession.reset()
        // Realistic ~3.3 m/s legs (0.0003° lat ≈ 33 m / 10 s): under the 12 m/s
        // jitter clamp restore() now applies, so the recovered distance equals a
        // full haversine over the track.
        val pts = listOf(
            GpsPoint(48.8566, 2.3522, 0, 5.0),
            GpsPoint(48.8569, 2.3522, 10, 5.0),
            GpsPoint(48.8572, 2.3522, 20, 5.0),
        )
        RunSession.restore(pts)
        assertTrue(RunSession.tracking.value)
        assertEquals(pts, RunSession.points.value)
        assertEquals(haversineKm(pts), RunSession.distanceKm.value, 1e-9)
        RunSession.reset()
    }

    @Test
    fun resetClearsArmedAndLastFixElapsed() {
        // The armed-start flows are part of the session's reset contract: a new run
        // must never inherit a stale armed flag or fix stamp.
        RunSession.armed.value = true
        RunSession.lastFixElapsedMs.value = 12_345L
        RunSession.saving.value = true
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
        RunSession.paused.value = true
        RunSession.reset()
        assertFalse("paused cleared on reset", RunSession.paused.value)
    }

    @Test
    fun notifTitleResPinsStateToResourceId() {
        // The state→resource mapping lives in the pure function, so a swapped id
        // fails here against the ACTUAL R.string constants (plain ints on the JVM
        // test classpath) — not against caller-supplied local literals, which is
        // what let a call-site swap slip past the old tests. A paused recording
        // selects the paused title; armed/locate ignore the paused flag.
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
        // the current pace (resolved via track_notif_progress_pace); paused selects the
        // parameterized "Paused · %1$s" resource with the progress as its arg (also the
        // notifyIfChanged throttle key, so a pause toggle re-posts); a lost signal while
        // recording shows the signal-lost line; armed/locate pin their fixed resources.
        // Asserting the real R.string ids pins the wiring itself, not just the branch logic.
        val progress = "2:00 · 1.00 km"
        val pace = "5:45 /km"
        fun spec(recording: Boolean, armed: Boolean, paused: Boolean, signalLost: Boolean = false, progress: String? = "2:00 · 1.00 km", paceNow: String? = pace) =
            trackingNotifContentSpec(recording, armed, paused, signalLost, progress, paceNow)
        assertEquals(
            NotifContentSpec.Progress(progress, pace),
            spec(recording = true, armed = false, paused = false),
        )
        // No current pace yet (standing still / fresh segment): progress without pace.
        assertEquals(
            NotifContentSpec.Progress(progress, null),
            spec(recording = true, armed = false, paused = false, paceNow = null),
        )
        assertEquals(
            NotifContentSpec.PausedProgress(R.string.track_notif_paused_content, progress),
            spec(recording = true, armed = false, paused = true),
        )
        // A lost GPS signal while actively recording replaces the progress line, so the
        // lock screen shows it; a pause outranks it (nothing is expected to record).
        assertEquals(
            NotifContentSpec.Literal(R.string.track_notif_signal_lost),
            spec(recording = true, armed = false, paused = false, signalLost = true),
        )
        assertEquals(
            NotifContentSpec.PausedProgress(R.string.track_notif_paused_content, progress),
            spec(recording = true, armed = false, paused = true, signalLost = true),
        )
        // Paused / signal loss only fold while recording; the armed/locate copy is fixed.
        assertEquals(
            NotifContentSpec.Literal(R.string.track_notif_armed_text),
            spec(recording = false, armed = true, paused = true, signalLost = true),
        )
        assertEquals(
            NotifContentSpec.Literal(R.string.track_notif_locate_text),
            spec(recording = false, armed = false, paused = true, signalLost = true),
        )
        // Null progress (no two-fix track yet) falls back to a state-appropriate
        // literal instead of an empty/absent progress line.
        assertEquals(
            NotifContentSpec.Literal(R.string.track_notif_paused_text),
            spec(recording = true, armed = false, paused = true, progress = null),
        )
        assertEquals(
            NotifContentSpec.Literal(R.string.track_notif_recording_hint),
            spec(recording = true, armed = false, paused = false, progress = null),
        )
    }

    @Test
    fun signalLossIsDueOnlyPastTheThresholdAndAfterAFix() {
        // Shared by the in-app pill and the service's screen-off watchdog.
        assertFalse("no fix yet → nothing lost", signalLossDue(lastFixElapsedMs = 0L, nowElapsedMs = 60_000L))
        assertFalse("at the threshold is not yet lost", signalLossDue(10_000L, 10_000L + SIGNAL_LOSS_MS))
        assertTrue("past the threshold is lost", signalLossDue(10_000L, 10_000L + SIGNAL_LOSS_MS + 1))
        assertTrue("warned before the 30 s segment break", SIGNAL_LOSS_MS < 30_000L)
    }

    /** Feed [seconds] of 1 Hz fixes moving north at [mps] (0 = standing still), starting
     *  from [state] = (lat, tSec); returns the new state. */
    private fun runFor(state: Pair<Double, Long>, seconds: Int, mps: Double): Pair<Double, Long> {
        val degPerM = 1.0 / 111_194.93 // metres → degrees of latitude at R = 6371 km
        var (lat, t) = state
        repeat(seconds) {
            lat += mps * degPerM
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
        RunSession.markResumeBoundary()
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
        RunSession.armed.value = true
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
        // 150 s steady at ~3 m/s, 1 Hz (lon advances 3 m/s at the equator).
        val degPerM = 1.0 / 111_320.0
        var lon = 0.0
        RunSession.add(GpsPoint(0.0, 0.0, 0, 5.0), 0L)
        for (t in 1..150) {
            lon += 3.0 * degPerM
            RunSession.add(GpsPoint(0.0, lon, t.toLong(), 5.0), t * 1_000_000_000L)
        }
        val buckets = RunSession.paceBuckets(1) // one-minute buckets
        val complete = buckets.filter { it.complete }
        // 150 s → two full 60 s slices + a 30 s in-progress tail.
        assertEquals(2, complete.size)
        assertTrue("trailing slice is partial", !buckets.last().complete)
        // ~3 m/s → 1000 m / 3 / 60 ≈ 5.56 min/km per completed slice.
        complete.forEach { assertEquals(5.56, it.paceMinPerKm, 0.1) }
        RunSession.reset()
    }

    @Test
    fun standingStillWithGpsWanderAccruesNoDistanceOrElapsed() {
        RunSession.reset()
        val degPerM = 1.0 / 111_320.0
        // 60 s standing still at 1 Hz: the fix drifts within a ~±2 m envelope but each
        // consecutive-fix step is ≈0.2 m/s peak: every leg is below the 0.5 m/s stop
        // floor (MIN_MOVING_SPEED_MPS, mirroring load::is_stopped), so NONE accrues
        // live distance or moving time. (A real stationary GPS wanders slowly, not by
        // whole metres per second.) Standing at a red light must not inflate the run.
        RunSession.add(GpsPoint(52.52, 13.405, 0, 5.0), 0L)
        for (t in 1..60) {
            val latOff = 2.0 * degPerM * kotlin.math.sin(2.0 * Math.PI * t / 60.0)
            RunSession.add(GpsPoint(52.52 + latOff, 13.405, t.toLong(), 5.0), t * 1_000_000_000L)
        }
        assertEquals(0.0, RunSession.distanceKm.value, 1e-12)
        assertEquals(0L, RunSession.elapsedSec.value)
        RunSession.reset()
    }

    @Test
    fun pauseGapAdvancesMovingElapsedByZero() {
        RunSession.reset()
        val degPerM = 1.0 / 111_320.0
        var lon = 0.0
        // Three 3 m/s legs (t=0..3): 3 s of moving time.
        RunSession.add(GpsPoint(0.0, lon, 0, 5.0), 0L)
        for (t in 1..3) {
            lon += 3.0 * degPerM
            RunSession.add(GpsPoint(0.0, lon, t.toLong(), 5.0), t * 1_000_000_000L)
        }
        assertEquals(3L, RunSession.elapsedSec.value)
        // A 37 s fix gap (> MAX_FIX_GAP_SEC): a pause bridge, its span adds ZERO
        // moving time (the runner may have stood/relocated). Elapsed stays at 3 s, it
        // does NOT jump the 37 s the way the old wall-clock span did.
        lon += 3.0 * degPerM
        RunSession.add(GpsPoint(0.0, lon, 40, 5.0), 40_000_000_000L)
        assertEquals(3L, RunSession.elapsedSec.value)
        // Three more 3 m/s legs (t=41..43): moving time resumes, +3 s (never +40).
        for (t in 41..43) {
            lon += 3.0 * degPerM
            RunSession.add(GpsPoint(0.0, lon, t.toLong(), 5.0), t * 1_000_000_000L)
        }
        assertEquals(6L, RunSession.elapsedSec.value)
        RunSession.reset()
    }

    @Test
    fun theReviewMovingTimeIsTheWholeSecondTrackTheCoreDerivesFrom() {
        RunSession.reset()
        val degPerM = 1.0 / 111_320.0
        var lon = 0.0
        // 80 legs, 1 s apart by their fix stamps but 995 ms apart on the monotonic
        // clock: the live clock floors 79.6 s to 79, the saved run lasts 80 s.
        RunSession.add(GpsPoint(0.0, lon, 1_000, 5.0), 0L)
        for (t in 1..80) {
            lon += 3.0 * degPerM
            RunSession.add(GpsPoint(0.0, lon, 1_000L + t, 5.0), t * 995_000_000L)
        }
        assertEquals(79L, RunSession.elapsedSec.value)
        assertEquals(80L, recordedMovingSec(RunSession.decimatedTrackForCore(), RunSession.decimatedSegmentStarts()))
        // A pause bridge into a new segment adds no moving time, nor does a standing leg.
        val pts = listOf(
            GpsPoint(0.0, 0.0, 0, 5.0),
            GpsPoint(0.0, 30.0 * degPerM, 10, 5.0),
            GpsPoint(0.0, 300.0 * degPerM, 100, 5.0),
            GpsPoint(0.0, 300.5 * degPerM, 110, 5.0),
            GpsPoint(0.0, 330.5 * degPerM, 120, 5.0),
        )
        assertEquals(20L, recordedMovingSec(pts, listOf(2)))
        RunSession.reset()
    }

    @Test
    fun theReviewRouteSplitsAtPausesAndTheGateCaptionExplainsOnlyWhatIsOn() {
        val pts = (0..4).map { GpsPoint(52.0 + it * 0.0003, 13.0, it * 10L, 5.0) }
        // A pause opened a new segment at index 3: no bridging line across it.
        val segs = trackSegments(pts, listOf(3))
        assertEquals(listOf(3, 2), segs.map { it.size })
        assertEquals(pts[3].lat, segs[1].first().lat, 0.0)
        assertEquals(30L, segs[1].first().timeSec)
        assertEquals(listOf(5), trackSegments(pts, emptyList()).map { it.size })
        assertTrue(trackSegments(emptyList(), listOf(0)).isEmpty())

        assertEquals(R.string.track_gates_caption_none, startGatesCaptionRes(precise = false, moving = false))
        assertEquals(R.string.track_gates_caption_precise, startGatesCaptionRes(precise = true, moving = false))
        assertEquals(R.string.track_gates_caption_moving, startGatesCaptionRes(precise = false, moving = true))
        assertEquals(R.string.track_gates_caption_both, startGatesCaptionRes(precise = true, moving = true))
    }

    @Test
    fun stationaryGapDoesNotSlowTheCurrentPaceBucket() {
        RunSession.reset()
        RunSession.setPaceBucketMinutes(1) // 1-min slices from the default 4
        val degPerM = 1.0 / 111_320.0
        var lon = 0.0
        // 30 s of moving at 3 m/s → an OPEN (partial) 30 s slice at ~5.56 min/km.
        RunSession.add(GpsPoint(0.0, lon, 0, 5.0), 0L)
        for (t in 1..30) {
            lon += 3.0 * degPerM
            RunSession.add(GpsPoint(0.0, lon, t.toLong(), 5.0), t * 1_000_000_000L)
        }
        val before = RunSession.paceBucketsFlow.value.last()
        assertFalse("open trailing slice", before.complete)
        // 120 s standing still at 1 Hz: the fix drifts within a ~±2 m envelope but every
        // consecutive-fix step is well below the 0.5 m/s stop floor (a real stationary
        // GPS wanders slowly, not by whole metres/s), and each 1 s dt is under the 30 s
        // segment-break gap, so no boundary opens either. Every such leg must be DROPPED
        // from the pace accumulator: standing at a red light must not slow the slice.
        for (t in 31..150) {
            val latOff = 2.0 * degPerM * kotlin.math.sin(2.0 * Math.PI * (t - 30) / 60.0)
            RunSession.add(GpsPoint(0.0 + latOff, lon, t.toLong(), 5.0), t * 1_000_000_000L)
        }
        val after = RunSession.paceBucketsFlow.value.last()
        assertFalse("still the same open slice, never a fabricated full one", after.complete)
        assertEquals("stationary gap adds no moving time to the slice", before.minutes, after.minutes, 1e-9)
        assertEquals("stationary gap does not slow the pace", before.paceMinPerKm, after.paceMinPerKm, 1e-9)
        // No slice ever completed from the idle stretch.
        assertTrue("no completed slice from an idle stretch", RunSession.paceBucketsFlow.value.none { it.complete })
        // The incremental output still equals a from-scratch recompute over the track.
        assertEquals(RunSession.paceBuckets(1), RunSession.paceBucketsFlow.value)
        RunSession.reset()
    }

    @Test
    fun steadyRunElapsedEqualsWallSpanAndDistanceUnchanged() {
        RunSession.reset()
        val degPerM = 1.0 / 111_320.0
        var lon = 0.0
        val pts = ArrayList<GpsPoint>()
        pts.add(GpsPoint(0.0, 0.0, 0, 5.0))
        for (t in 1..150) {
            lon += 3.0 * degPerM
            pts.add(GpsPoint(0.0, lon, t.toLong(), 5.0))
        }
        pts.forEach { RunSession.add(it, it.observedAt * 1_000_000_000L) }
        // Clean run, no pauses/stops/jitter: every leg is moving, so live moving time
        // equals the wall-clock span, the parity the of-record moving clock holds too.
        val wallSpan = pts.last().observedAt - pts.first().observedAt
        assertEquals(wallSpan, RunSession.elapsedSec.value)
        // Distance is still the full haversine: the stop floor changed nothing here.
        assertEquals(haversineKm(pts), RunSession.distanceKm.value, 1e-9)
        RunSession.reset()
    }

    @Test
    fun restoreMatchesLivePathForACleanRun() {
        val degPerM = 1.0 / 111_320.0
        var lon = 0.0
        val pts = ArrayList<GpsPoint>()
        pts.add(GpsPoint(0.0, 0.0, 0, 5.0))
        for (t in 1..120) {
            lon += 3.0 * degPerM
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

    // ── Save-time GPS-track decimation (event-log growth prong a) ──────────────

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
        assertEquals(RunSession.trackForCore(), RunSession.decimatedTrackForCore())
        RunSession.reset()
    }

    @Test
    fun decimatedTrackKeepsEndpointsAndRespectsCap() {
        RunSession.reset()
        // 1200 fixes at ~3 m/s, 1 Hz, no pauses.
        val degPerM = 1.0 / 111_320.0
        var lon = 0.0
        val pts = ArrayList<GpsPoint>()
        pts.add(GpsPoint(0.0, 0.0, 0, 5.0))
        for (t in 1..1199) {
            lon += 3.0 * degPerM
            pts.add(GpsPoint(0.0, lon, t.toLong(), 5.0))
        }
        pts.forEach { RunSession.add(it, it.observedAt * 1_000_000_000L) }
        val full = RunSession.trackForCore()
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
        val degPerM = 1.0 / 111_320.0
        var lon = 0.0
        val pts = ArrayList<GpsPoint>()
        pts.add(GpsPoint(0.0, 0.0, 0, 5.0)) // index 0
        // Segment A: indices 1..200.
        for (t in 1..200) {
            lon += 3.0 * degPerM
            pts.add(GpsPoint(0.0, lon, t.toLong(), 5.0))
        }
        // A 61 s gap (> MAX_FIX_GAP_SEC = 30) opens a new segment at index 201.
        var t = 261L
        for (k in 1..199) {
            lon += 3.0 * degPerM
            pts.add(GpsPoint(0.0, lon, t, 5.0))
            t += 1
        }
        pts.forEach { RunSession.add(it, it.observedAt * 1_000_000_000L) }
        val full = RunSession.trackForCore() // 401 fixes; boundary at index 201
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
        val degPerM = 1.0 / 111_320.0
        var lon = 0.0
        val pts = ArrayList<GpsPoint>()
        pts.add(GpsPoint(0.0, 0.0, 0, 5.0))
        for (t in 1..900) {
            lon += 3.0 * degPerM
            val lat = 0.000_004 * Math.sin(t * 1.3)
            pts.add(GpsPoint(lat, lon, t.toLong(), 5.0))
        }
        pts.forEach { RunSession.add(it, it.observedAt * 1_000_000_000L) }
        val full = RunSession.trackForCore()
        val fullKm = haversineKm(full)
        // Cap 300 over 900 fixes → stride 3 (a ~3 s cadence).
        val decKm = haversineKm(RunSession.decimatedTrackForCore(300))
        assertEquals(fullKm, decKm, fullKm * 0.01)
        RunSession.reset()
    }

    // ── trackForCore sends TRUE coords + segment starts (no re-anchor) ──

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
        RunSession.markResumeBoundary()
        val seg2 = listOf(
            GpsPoint(0.0, 1.000, 80, 5.0),
            GpsPoint(0.0, 1.001, 90, 5.0),
        )
        seg2.forEach { RunSession.add(it, it.observedAt * 1_000_000_000L) }
        // trackForCore returns the TRUE captured coordinates, UN-shifted (older
        // builds re-anchored segment 2 onto segment 1: now the core is
        // segment-aware, so the shell keeps the real geometry).
        assertEquals(seg1 + seg2, RunSession.trackForCore())
        // The boundary that begins segment 2 is reported for the core.
        assertEquals(listOf(3), RunSession.segmentStartIndices())
        RunSession.reset()
    }

    @Test
    fun decimatedSegmentStartsRemapToTheThinnedBoundaryFix() {
        RunSession.reset()
        val degPerM = 1.0 / 111_320.0
        var lon = 0.0
        val pts = ArrayList<GpsPoint>()
        pts.add(GpsPoint(0.0, 0.0, 0, 5.0))
        for (t in 1..200) {
            lon += 3.0 * degPerM
            pts.add(GpsPoint(0.0, lon, t.toLong(), 5.0))
        }
        // A 61 s gap (> MAX_FIX_GAP_SEC = 30) opens a new segment at index 201.
        var t = 261L
        for (k in 1..199) {
            lon += 3.0 * degPerM
            pts.add(GpsPoint(0.0, lon, t, 5.0))
            t += 1
        }
        pts.forEach { RunSession.add(it, it.observedAt * 1_000_000_000L) }
        val full = RunSession.trackForCore()
        val boundaryFix = full[201]
        // Coarse cap so index 201 is NOT on the uniform grid: only the boundary
        // rule retains it, and its remapped index must point back at the same fix.
        val dec = RunSession.decimatedTrackForCore(40)
        val starts = RunSession.decimatedSegmentStarts(40)
        assertEquals("one remapped boundary", 1, starts.size)
        assertEquals("remapped start points at the true boundary fix", boundaryFix, dec[starts[0]])
        RunSession.reset()
    }

    // ── tracker save-date, short-run gate, ──────────
    // ── non-monotonic-fix drop, sidecar recovery thinning ────────────────────────

    @Test
    fun logRunTrackDatesRunAtLastFixNotSaveTime() {
        // The tracker save path dates the run at its LAST GPS fix, not "now": a
        // crash-recovery save can run hours after the run happened, and History /
        // weekly-km / spike windows must land on when it occurred (mirrors GPX
        // import). This pins the value the shell derives + its wire encoding.
        RunSession.reset()
        val fixes = listOf(
            GpsPoint(52.5200, 13.405, 1_600_000_000L, 5.0),
            GpsPoint(52.5203, 13.405, 1_600_000_010L, 5.0),
        )
        fixes.forEach { RunSession.add(it, it.observedAt * 1_000_000_000L) }
        val runObservedAt = RunSession.points.value.lastOrNull()?.observedAt
            ?: (System.currentTimeMillis() / 1000)
        assertEquals("dates at the last fix, not now", 1_600_000_010L, runObservedAt)
        val wire = obj(
            Event.LogRunTrack(
                points = RunSession.decimatedTrackForCore(),
                hrPctMax = 0.0,
                longestRecentKm = 0.0,
                observedAt = runObservedAt,
                segmentStarts = RunSession.decimatedSegmentStarts(),
            ),
        )["LogRunTrack"]!!.jsonObject
        assertEquals(1_600_000_010L, wire["observed_at"]!!.jsonPrimitive.content.toLong())
        RunSession.reset()
    }

    @Test
    fun shortRunGateUsesMovingTimeNotWallSpan() {
        // A sub-3-min MOVING run padded by a long pause must still trip the
        // keep/discard gate. The old gate used the first→last wall-clock span (which
        // includes the pause) and would let it through; the fix gates on
        // RunSession.elapsedSec (moving time only).
        RunSession.reset()
        val degPerM = 1.0 / 111_320.0
        var lon = 0.0
        RunSession.add(GpsPoint(0.0, lon, 0, 5.0), 0L)
        // 59 legs of 3 m/s moving time = 59 s (< MIN_RUN_SEC).
        for (t in 1..59) {
            lon += 3.0 * degPerM
            RunSession.add(GpsPoint(0.0, lon, t.toLong(), 5.0), t * 1_000_000_000L)
        }
        // A 241 s fix gap (> MAX_FIX_GAP_SEC) → a pause bridge: adds ZERO moving time
        // but pushes the wall span to 300 s (≥ MIN_RUN_SEC).
        lon += 3.0 * degPerM
        RunSession.add(GpsPoint(0.0, lon, 300, 5.0), 300_000_000_000L)
        val captured = RunSession.points.value
        val wallSpan = captured.last().observedAt - captured.first().observedAt
        val movingSec = RunSession.elapsedSec.value
        assertTrue("wall span would have passed the old gate", wallSpan >= MIN_RUN_SEC)
        assertTrue("moving time correctly trips the new gate", movingSec < MIN_RUN_SEC)
        RunSession.reset()
    }

    @Test
    fun addDropsNonMonotonicFix() {
        // A fix whose MONOTONIC stamp is EARLIER than the previous one (dtMs < 0) -
        // e.g. an out-of-order fused batch on an NTP step is dropped whole, not
        // appended out of order (which used to open a spurious segment).
        RunSession.reset()
        RunSession.add(GpsPoint(0.0, 0.0000, 0, 5.0), 0L)
        RunSession.add(GpsPoint(0.0, 0.0003, 10, 5.0), 10_000_000_000L)
        val pointsBefore = RunSession.points.value
        val distBefore = RunSession.distanceKm.value
        val elapsedBefore = RunSession.elapsedSec.value
        // Monotonic stamp 5 s < the previous 10 s → dropped.
        RunSession.add(GpsPoint(0.0, 0.0006, 20, 5.0), 5_000_000_000L)
        assertEquals("non-monotonic fix not appended", pointsBefore, RunSession.points.value)
        assertEquals("no distance from a dropped fix", distBefore, RunSession.distanceKm.value, 1e-12)
        assertEquals("no moving time from a dropped fix", elapsedBefore, RunSession.elapsedSec.value)
        assertTrue("no spurious segment opened", RunSession.segmentStartIndices().isEmpty())
        RunSession.reset()
    }

    // ── append-only storage + incremental pace buckets. The saved track must ──
    // ── stay unchanged, and the incremental                                  ──
    // ── accumulator must equal a from-scratch recompute over the whole track.   ──

    @Test
    fun incrementalPaceBucketsMatchTheReferenceFunctionAcrossBoundaryAndJitter() {
        // The pace buckets are folded in one leg at a time as fixes arrive (O(1)
        // amortised). This proves the incremental output (paceBucketsFlow) is
        // bit-identical to the from-scratch reference at the bottom of this file over
        // a run that exercises every path: normal legs, a pause boundary (dropped), a
        // jitter spike (capped), multiple completed slices + a partial tail.
        RunSession.reset()
        RunSession.setPaceBucketMinutes(1) // 1-min slices from the default 4
        val degPerM = 1.0 / 111_320.0
        var lon = 0.0
        RunSession.add(GpsPoint(0.0, lon, 0, 5.0), 0L)
        for (t in 1..100) {
            lon += 3.0 * degPerM
            RunSession.add(GpsPoint(0.0, lon, t.toLong(), 5.0), t * 1_000_000_000L)
        }
        // Pause boundary: a 40 s gap (> MAX_FIX_GAP_SEC) in BOTH timebases, the
        // bridge leg is dropped from the buckets exactly as paceBuckets skips it.
        lon += 3.0 * degPerM
        RunSession.add(GpsPoint(0.0, lon, 140, 5.0), 140_000_000_000L)
        for (t in 141..170) {
            lon += 3.0 * degPerM
            RunSession.add(GpsPoint(0.0, lon, t.toLong(), 5.0), t * 1_000_000_000L)
        }
        // Jitter spike: a teleport in 1 s, distance capped at MAX_PLAUSIBLE·dt in
        // both the incremental feed and the reference, so the bucket pace agrees.
        RunSession.add(GpsPoint(0.0, lon + 5.0, 171, 5.0), 171_000_000_000L)

        assertEquals(
            "incremental buckets equal a from-scratch recompute",
            RunSession.paceBuckets(1),
            RunSession.paceBucketsFlow.value,
        )
        // A mid-run bucket-size change rebuilds the accumulator to match the
        // reference at the new size.
        RunSession.setPaceBucketMinutes(2)
        assertEquals(
            "rebuild after a size change equals the reference at the new size",
            RunSession.paceBuckets(2),
            RunSession.paceBucketsFlow.value,
        )
        RunSession.reset()
    }

    @Test
    fun savedTrackIsByteIdenticalToTheAppendedFixes() {
        // The append-only backing store must expose EXACTLY the surviving fixes (a
        // dropped non-monotonic fix never appears; a pause boundary is reported),
        // and the of-record saved forms (trackForCore / decimatedTrackForCore /
        // segmentStartIndices) are pure functions of that list, so a run built
        // through add() is identical to the old prev+p semantics.
        RunSession.reset()
        val expected = ArrayList<GpsPoint>()
        val degPerM = 1.0 / 111_320.0
        var lon = 0.0
        fun feed(p: GpsPoint, nanos: Long, survives: Boolean) {
            RunSession.add(p, nanos)
            if (survives) expected.add(p)
        }
        feed(GpsPoint(0.0, lon, 0, 5.0), 0L, true) // index 0
        for (t in 1..50) {
            lon += 3.0 * degPerM
            feed(GpsPoint(0.0, lon, t.toLong(), 5.0), t * 1_000_000_000L, true) // 1..50
        }
        // A non-monotonic fix (monotonic stamp EARLIER than the last) is DROPPED -
        // never appended, so it must not appear in the saved track.
        feed(GpsPoint(0.0, lon + degPerM, 51, 5.0), 25_000_000_000L, false)
        // Pause boundary at index 51 (50 s monotonic gap), then more legs.
        lon += 3.0 * degPerM
        feed(GpsPoint(0.0, lon, 100, 5.0), 100_000_000_000L, true) // index 51 (boundary)
        for (t in 101..150) {
            lon += 3.0 * degPerM
            feed(GpsPoint(0.0, lon, t.toLong(), 5.0), t * 1_000_000_000L, true) // 52..101
        }
        // The published track is exactly the surviving fixes.
        assertEquals("points expose the surviving fixes", expected, RunSession.points.value)
        assertEquals("trackForCore is the full surviving list", expected, RunSession.trackForCore())
        // Decimation with a cap above the size is the identity, so it equals the
        // oracle list, confirming the of-record path reads the same content.
        assertEquals(expected, RunSession.decimatedTrackForCore(10_000))
        // The pause boundary is reported at the true index.
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

    // ── TrackSnapshot.equals honours the List contract ──────────────────────────
    @Test
    fun trackSnapshotEqualsHonoursTheListContract() {
        // equals used to mix array-identity (two snapshots) with element-wise List
        // semantics (snapshot vs plain List), breaking transitivity: two equal-content
        // snapshots on DIFFERENT backing arrays compared UNEQUAL while each compared
        // EQUAL to a plain List of the same fixes.
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

        // Equal content on distinct arrays ⇒ equal (the fix; the old backing-identity
        // fast path short-circuited to false here).
        assertEquals(snapA, snapB)
        assertEquals(snapB, snapA)
        // Symmetric with a plain List, both directions.
        assertEquals(plain, snapA)
        assertEquals(snapA, plain)
        // hashCode consistent with equals, and with a plain List's element-wise hash.
        assertEquals(snapA.hashCode(), snapB.hashCode())
        assertEquals(plain.hashCode(), snapA.hashCode())
        // Different content stays unequal.
        RunSession.reset()
        RunSession.add(p1, 0L)
        val snapShort = RunSession.points.value
        assertNotEquals(snapA, snapShort)
    }

    @Test
    fun non_monotonic_fix_is_rejected_and_not_persisted() {
        RunSession.reset()
        // Seed the track with one fix (monotonic base at 10 s).
        assertTrue(RunSession.add(GpsPoint(52.5200, 13.4050, 0, 5.0), 10_000_000_000L))
        val before = RunSession.points.value.size
        // A later fix stamped BEFORE it on the monotonic elapsedRealtime timebase is a
        // batch-reorder artefact: add drops it and returns false, so the crash sidecar
        // (which mirrors only add's accepted fixes) never persists it.
        assertFalse(RunSession.add(GpsPoint(52.5300, 13.4050, 5, 5.0), 5_000_000_000L))
        assertEquals(before, RunSession.points.value.size)
        RunSession.reset()
    }

    // ── locate-failure latch ─────────────────────────────────────────────────────

    @Test
    fun resetClearsTheLocateFailureLatch() {
        // A failed location start latches so the screen stops re-launching the
        // service into the same failure; leaving the tracker must not carry the latch
        // into the next session.
        RunSession.locationFailed.value = true
        RunSession.reset()
        assertFalse(RunSession.locationFailed.value)
    }

    // ── live pace vs the plan's range ────────────────────────────────────────────

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

    // ── "X to go" on a single-step planned run ──────────────────────────────────

    @Test
    fun singleStepPlanDistanceComesFromTheRunHead() {
        val easy = PrescriptionView(
            summary = "7 km · 6:03–7:14 /km (easy) · HR 120–146 bpm",
            exercise = "Easy run",
            run_head = "7 km",
            face_lead = "7 km",
            run_head_km = 7.0,
        )
        assertEquals(7.0, plannedRunKm(easy)!!, 1e-9)
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
        // A head that is not a plain distance gives no planned distance.
        assertEquals(null, plannedRunKm(reps))
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
        val degPerM = 1.0 / 111_195.0
        val seg1 = (0..6).map { GpxFix(47.0 + it * 30.0 * degPerM, 8.0, it * 10L, null) } +
            GpxFix(47.0 + 180.0 * degPerM, 8.0, 80L, null)
        val seg2 = (0..6).map { GpxFix(47.01 + it * 30.0 * degPerM, 8.0, 200L + it * 10L, null) }
        val points = timedTrackFromSegments(listOf(seg1, seg2))
        val last = points.lastIndex
        assertEquals(260L, points[last].timeSec - points[0].timeSec)
        assertEquals(120.0, points[last].movingSec, 0.0)

        // The untouched window reads the run detail's moving time, not "4:20".
        assertEquals("2:00", fmtRunTime(trimKeptMovingSec(points, 0, last, recordSec = 120.0)))
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

// ── Test-only reference oracles (kept out of the APK) ───────────────────────────

/** Path length over the raw fixes, km (an unclamped, ungated haversine sum): the
 *  reference the live accumulators are pinned against. */
private fun haversineKm(points: List<GpsPoint>): Double {
    if (points.size < 2) return 0.0
    var total = 0.0
    for (i in 1 until points.size) total += segmentKm(points[i - 1], points[i])
    return total
}

/** The pause-boundary indices of the live track, sorted (no decimation). */
private fun RunSession.segmentStartIndices(): List<Int> = decimatedSegmentStarts(Int.MAX_VALUE)

/**
 * From-scratch per-N-minute pace slices over the whole track: the spec the
 * incremental [RunSession.paceBucketsFlow] must equal after any sequence. A slice is
 * [bucketMinutes] of MOVING time; pause-bridge legs are dropped whole, standing-still
 * legs (below [MIN_MOVING_SPEED_MPS]) accrue nothing, jitter legs are capped at
 * [MAX_PLAUSIBLE_SPEED_MPS]·dt, and a leg straddling a slice edge is split by time.
 */
private fun RunSession.paceBuckets(bucketMinutes: Int): List<PaceBucket> {
    val pts = points.value
    val starts = segmentStartIndices().toHashSet()
    val n = bucketMinutes.coerceAtLeast(1)
    val bucketSec = n * 60.0
    if (pts.size < 2) return emptyList()
    val out = ArrayList<PaceBucket>()
    var curSec = 0.0
    var curKm = 0.0
    for (i in 1 until pts.size) {
        if (i in starts) continue
        var legSec = (pts[i].observedAt - pts[i - 1].observedAt).toDouble()
        if (legSec <= 0.0) continue
        var legKm = segmentKm(pts[i - 1], pts[i])
        if (legKm * 1000.0 / legSec < MIN_MOVING_SPEED_MPS) continue
        val maxKm = MAX_PLAUSIBLE_SPEED_MPS * legSec / 1000.0
        if (legKm > maxKm) legKm = maxKm
        while (curSec + legSec >= bucketSec) {
            val take = bucketSec - curSec
            val frac = if (legSec > 0.0) take / legSec else 0.0
            val kmChunk = legKm * frac
            curKm += kmChunk
            out.add(PaceBucket(paceMinPerKm = n / curKm, minutes = n.toDouble(), complete = true))
            legSec -= take
            legKm -= kmChunk
            curSec = 0.0
            curKm = 0.0
        }
        curSec += legSec
        curKm += legKm
    }
    if (curSec > 0.0) {
        val minutes = curSec / 60.0
        out.add(PaceBucket(paceMinPerKm = minutes / curKm, minutes = minutes, complete = false))
    }
    return out
}
