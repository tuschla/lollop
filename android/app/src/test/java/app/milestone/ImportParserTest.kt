package app.milestone

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
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
}
