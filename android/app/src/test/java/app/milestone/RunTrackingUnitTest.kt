package app.milestone

import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * JVM unit tests (no device) for the run-tracking shell fixes: the incremental
 * live-distance accumulator must match a full haversine, and the readiness
 * `Soreness` signal must be present, correctly ordered vs the Rust schema, and
 * submittable on the wire.
 */
class RunTrackingUnitTest {

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
        // M2: a crash-resume splice must be a SEGMENT BOUNDARY. The app can be dead
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
    fun readinessSignalEnumIncludesSorenessInSchemaOrder() {
        val names = ReadinessSignal.entries.map { it.name }
        assertTrue(names.contains("Soreness"))
        // schema.rs orders Soreness between Pain and Illness: the manual picker
        // iterates entries, so wrong ordering would misplace it in the UI.
        assertEquals(names.indexOf("Pain") + 1, names.indexOf("Soreness"))
        assertEquals(names.indexOf("Soreness") + 1, names.indexOf("Illness"))
    }

    @Test
    fun sorenessIsSubmittableOnTheWire() {
        val fields = obj(
            Event.SubmitReadiness(ReadinessSignal.Soreness, 6.0, 1_600_000_000L),
        )["SubmitReadiness"]!!.jsonObject
        assertEquals("Soreness", fields["signal"]!!.jsonPrimitive.content)
        assertEquals(6.0, fields["value"]!!.jsonPrimitive.content.toDouble(), 1e-9)
        assertEquals(1_600_000_000L, fields["observed_at"]!!.jsonPrimitive.content.toLong())
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

    // ── I15/B2: trackForCore sends TRUE coords + segment starts (no re-anchor) ──

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

    // ── External review (2026-08-04): tracker save-date, short-run gate, ──────────
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
}
