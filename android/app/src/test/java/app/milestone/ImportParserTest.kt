package app.milestone

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Pure (no-XML) unit tests for the run-import parser logic that the JVM unit
 * suite can exercise directly:
 *
 *  - [mergeTcxTracks]: a standard Garmin/Strava TCX opens one
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

    // --- TCX lap merge ------------------------------------------------------

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

    // --- Imported-run accuracy ("Imported-run accuracy assumption") -----

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
    // at a running pace (0.0054° ≈ 600 m per 200 s, 3 m/s) so they survive the
    // core-facing QC, the import's plausibility gate and its minimum run length
    // (0.5 km and 3 minutes).
    private fun fixAt(t: Long, lon: Double, accuracyM: Double?) =
        GpxFix(lat = 0.0, lon = lon, timeSec = t, hrBpm = null, accuracyM = accuracyM)

    @Test
    fun unknownAccuracyGetsQcPassingSentinelNotFabricatedFive() {
        // No source accuracy (TCX / GPX-without-hdop) → the 30 m sentinel, which
        // must clear the core's 30 m QC gate (never a fabricated 5.0 that reads
        // as a great fix).
        val ev = importedRunEvent(
            listOf(listOf(fixAt(1000, 0.000, null), fixAt(1200, 0.0054, null))),
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
            listOf(listOf(fixAt(1000, 0.000, 8.0), fixAt(1200, 0.0054, 12.5))),
            measuredHrMax = null,
        )
        assertEquals(8.0, ev.points[0].accuracyM, 1e-9)
        assertEquals(12.5, ev.points[1].accuracyM, 1e-9)
    }

    // --- Time-weighted %HRmax ---------------------------------------------------

    @Test
    fun hrPctMaxIsTimeWeightedSoALongHeldSampleDominates() {
        // The first sample is held ~300 s (the gap to the next fix), the second only
        // ~5 s. A time-weighted mean must land within 1 bpm of the long-held sample;
        // a plain mean (142.5 bpm) would sit ~27 bpm below it.
        val measuredHrMax = 190.0
        val ev = importedRunEvent(
            listOf(
                listOf(
                    GpxFix(0.0, 0.000, 1000, 170), // held 300 s (~556 m: moving)
                    GpxFix(0.0, 0.005, 1300, 115), // held 5 s (~2 m: standing)
                    GpxFix(0.0, 0.00502, 1305, null),
                ),
            ),
            measuredHrMax = measuredHrMax,
        )
        val weightedBpm = ev.hrPctMax / 100.0 * measuredHrMax
        assertEquals("the 300-s sample dominates the 5-s spike", 170.0, weightedBpm, 1.0)
        assertNotEquals("a plain mean would not satisfy this", 142.5, weightedBpm, 1.0)
    }

    @Test
    fun evenlySpacedHrSamplesMatchThePlainMean() {
        // Constant 100 s spacing (the final fix takes the median gap, also 100 s) →
        // all weights are equal, so the time-weighted result is the old plain mean.
        val measuredHrMax = 200.0
        val ev = importedRunEvent(
            listOf(
                listOf(
                    GpxFix(0.0, 0.0000, 1000, 100),
                    GpxFix(0.0, 0.0030, 1100, 120),
                    GpxFix(0.0, 0.0060, 1200, 140),
                ),
            ),
            measuredHrMax = measuredHrMax,
        )
        assertEquals(120.0 / measuredHrMax * 100.0, ev.hrPctMax, 1e-9)
    }

    // --- import <time> parsing (GPX + TCX share parseImportTimeSec) ---------
    // The XML→segment path uses android.util.Xml (a non-functional JVM stub), so the
    // timestamp decoder is verified on its pure helper.

    @Test
    fun specUtcTimestampsParseToTheirInstant() {
        assertEquals(1_788_084_000L, parseImportTimeSec("2026-08-30T10:00:00Z"))
        // An explicit offset resolves to the same instant (10:00+02:00 == 08:00Z).
        assertEquals(1_788_076_800L, parseImportTimeSec("2026-08-30T10:00:00+02:00"))
    }

    @Test
    fun zoneNaiveTimestampParsesAsUtcNotDropped() {
        // A zone-naive stamp (no Z / offset) is read as UTC per the GPX/TCX spec, NOT
        // dropped to 0 — a dropped time made a genuinely recorded run get misdiagnosed
        // downstream as "a planned route, not a recorded run".
        val naive = parseImportTimeSec("2026-08-30T10:00:00")
        assertEquals(1_788_084_000L, naive)
        assertEquals(parseImportTimeSec("2026-08-30T10:00:00Z"), naive)
        assertTrue("a naive timestamp must not read as time-less", naive > 0L)
    }

    @Test
    fun missingOrGarbageTimestampIsZero() {
        assertEquals(0L, parseImportTimeSec(""))
        assertEquals(0L, parseImportTimeSec("   "))
        assertEquals(0L, parseImportTimeSec("not-a-timestamp"))
    }

    // --- distinct entry ids per imported activity --------------------------

    @Test
    fun two_imported_runs_in_one_ms_get_distinct_ids() {
        // A multi-activity TCX import builds several runs back-to-back; the default
        // System.currentTimeMillis() id would collide within a millisecond, making a
        // later DeleteEntry/AmendRun target the wrong run. EntryIds.next() guarantees
        // a unique id per built run even in a tight loop.
        val seg = listOf(listOf(GpxFix(0.0, 0.000, 1000, null), GpxFix(0.0, 0.0054, 1200, null)))
        val a = importedRunEvent(seg, measuredHrMax = null)
        val b = importedRunEvent(seg, measuredHrMax = null)
        assertNotEquals(a.entryId, b.entryId)
    }
}
