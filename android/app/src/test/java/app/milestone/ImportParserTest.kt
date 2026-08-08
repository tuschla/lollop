package app.milestone

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Pure (no-XML) unit tests for the run-import parser logic that the JVM unit
 * suite can exercise directly:
 *
 *  - [mergeTcxTracks]: the M3 fix: a standard Garmin/Strava TCX opens one
 *    `<Track>` per `<Lap>` (autolaps on continuous running included), so a Track
 *    boundary is NOT inherently a pause. Consecutive Tracks with a small
 *    inter-track gap must merge into one segment (otherwise the core drops each
 *    lap's entering leg from distance AND moving time); a genuine pause gap must
 *    still split. (The full XML→segment path uses `android.util.Xml`, which is a
 *    non-functional stub in JVM unit tests, so the merge policy is verified on
 *    its pure helper; the XML wiring is covered by the instrumented parser test.)
 *  - [parseHrBpm]: the HR-validation fix: accept only finite 20..=250 bpm, drop
 *    everything else (NaN, `Infinity`, absurd magnitudes, out-of-range), shared
 *    by the GPX and TCX readers.
 */
class ImportParserTest {

    private fun fix(t: Long) = GpxFix(lat = 0.0, lon = 0.0, timeSec = t, hrBpm = null)

    // --- M3: TCX lap merge ------------------------------------------------------

    @Test
    fun consecutiveAutolapTracksMergeIntoOneSegment() {
        // Two "laps" of a continuous run: lap 2's first fix is 1 s after lap 1's
        // last fix (autolap boundary, not a pause) → one merged segment.
        val lap1 = listOf(fix(1000), fix(1001), fix(1002))
        val lap2 = listOf(fix(1003), fix(1004), fix(1005))
        val merged = mergeTcxTracks(listOf(lap1, lap2))
        assertEquals("autolap boundary must not split", 1, merged.size)
        assertEquals("no fixes lost across the merge", 6, merged[0].size)
    }

    @Test
    fun aRealPauseStillSplitsTheSegment() {
        // A 5-minute stop between Tracks (gap ≫ MAX_TRACK_MERGE_GAP_SEC) is a
        // genuine recording pause → two segments, so the bridging leg is dropped.
        val before = listOf(fix(1000), fix(1001), fix(1002))
        val after = listOf(fix(1302), fix(1303)) // 300 s later
        val merged = mergeTcxTracks(listOf(before, after))
        assertEquals("a real pause must stay a boundary", 2, merged.size)
        assertEquals(3, merged[0].size)
        assertEquals(2, merged[1].size)
    }

    @Test
    fun backwardsClockStepBetweenTracksSplits() {
        // A non-positive (overlapping/backwards) inter-track step is not a
        // trustworthy continuation → keep the boundary rather than merge.
        val a = listOf(fix(2000), fix(2001))
        val b = listOf(fix(1500), fix(1501))
        assertEquals(2, mergeTcxTracks(listOf(a, b)).size)
    }

    @Test
    fun emptyTracksAreSkipped() {
        val a = listOf(fix(10), fix(11))
        val merged = mergeTcxTracks(listOf(emptyList(), a, emptyList()))
        assertEquals(1, merged.size)
        assertEquals(2, merged[0].size)
    }

    // --- HR validation (shared GPX/TCX guard) -----------------------------------

    @Test
    fun plausibleHrIsKept() {
        assertEquals(120, parseHrBpm("120"))
        assertEquals(155, parseHrBpm(" 155 "))
        assertEquals(155, parseHrBpm("155.7")) // truncates toward the bpm
    }

    @Test
    fun boundaryHrValuesAreInclusive() {
        assertEquals(20, parseHrBpm("20"))
        assertEquals(250, parseHrBpm("250"))
    }

    @Test
    fun outOfRangeHrIsDropped() {
        assertNull("below-floor bpm rejected", parseHrBpm("19"))
        assertNull("above-ceiling bpm rejected", parseHrBpm("251"))
        assertNull("absurd magnitude rejected", parseHrBpm("1000000000"))
        assertNull("zero rejected", parseHrBpm("0"))
        assertNull("negative rejected", parseHrBpm("-40"))
    }

    @Test
    fun nonFiniteAndGarbageHrIsDropped() {
        // The wire-path bug: `Infinity` used to parse to a Double then flow on as
        // a bare `Infinity` JSON token → serde-rejected event → silent no-op.
        assertNull("Infinity rejected", parseHrBpm("Infinity"))
        assertNull("-Infinity rejected", parseHrBpm("-Infinity"))
        assertNull("NaN rejected", parseHrBpm("NaN"))
        assertNull("empty rejected", parseHrBpm(""))
        assertNull("non-numeric rejected", parseHrBpm("abc"))
    }

    // --- Imported-run accuracy (BUGS.md "Imported-run accuracy assumption") -----

    @Test
    fun hdopConvertsToMetresViaNominalUere() {
        // accuracy ≈ hdop × 5 m (nominal 1-sigma UERE). An HONEST estimate, not
        // a device measurement, but a real signal, unlike the old fixed 5.0.
        assertEquals(5.0, hdopToAccuracyM("1.0")!!, 1e-9)
        assertEquals(12.5, hdopToAccuracyM("2.5")!!, 1e-9)
        assertEquals(4.0, hdopToAccuracyM(" 0.8 ")!!, 1e-9)
    }

    @Test
    fun garbageOrNonPositiveHdopIsUnknownNotZero() {
        assertNull("missing/blank hdop is unknown", hdopToAccuracyM(""))
        assertNull("non-numeric hdop is unknown", hdopToAccuracyM("abc"))
        assertNull("zero hdop is not a real fix quality", hdopToAccuracyM("0"))
        assertNull("negative hdop rejected", hdopToAccuracyM("-1"))
        assertNull("NaN rejected", hdopToAccuracyM("NaN"))
        assertNull("Infinity rejected", hdopToAccuracyM("Infinity"))
    }

    // importedRunEvent is a pure function (no android.util.Xml), so its accuracy
    // handling is unit-testable directly. Build fixes with distinct positions/times
    // so they survive the core-facing QC the event feeds.
    private fun fixAt(t: Long, lon: Double, accuracyM: Double?) =
        GpxFix(lat = 0.0, lon = lon, timeSec = t, hrBpm = null, accuracyM = accuracyM)

    @Test
    fun unknownAccuracyGetsQcPassingSentinelNotFabricatedFive() {
        // No source accuracy (TCX / GPX-without-hdop) → the 30 m sentinel, which
        // must clear the core's 30 m QC gate (never a fabricated 5.0 that reads
        // as a great fix).
        val ev = importedRunEvent(
            listOf(listOf(fixAt(1000, 0.000, null), fixAt(1010, 0.001, null))),
            measuredHrMax = null,
        )
        assertEquals(2, ev.points.size)
        ev.points.forEach { assertEquals(30.0, it.accuracyM, 1e-9) }
        // Whatever the sentinel is, it must not be rejected by the 30 m gate.
        assertTrue("sentinel must pass the 30 m QC gate", ev.points.all { it.accuracyM <= 30.0 })
    }

    @Test
    fun realAccuracyPassesThroughUnchanged() {
        // A real recorded/derived figure (FIT gps_accuracy or GPX hdop) survives
        // to the core untouched, not overwritten by the sentinel.
        val ev = importedRunEvent(
            listOf(listOf(fixAt(1000, 0.000, 8.0), fixAt(1010, 0.001, 12.5))),
            measuredHrMax = null,
        )
        assertEquals(8.0, ev.points[0].accuracyM, 1e-9)
        assertEquals(12.5, ev.points[1].accuracyM, 1e-9)
    }
}
