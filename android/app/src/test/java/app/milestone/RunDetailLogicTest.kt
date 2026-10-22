package app.milestone

import java.time.ZoneId
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** Pure rules of the run detail redesign (ui-sweep §0.2, §2.5, §2.7). */
class RunDetailLogicTest {

    private fun split(i: Int, sec: Int, pace: String, partial: Boolean = false, km: Double = i.toDouble()) =
        RunSplitView(index = i, pace = pace, seconds_per_unit = sec, distance_km = km, partial = partial)

    @Test
    fun splitBars_fastestIsFullAndTheOnlyAccentWithThreeFullSplits() {
        val bars = splitBars(listOf(split(1, 502, "8:22"), split(2, 504, "8:24"), split(3, 504, "8:24"), split(4, 504, "8:24", partial = true)))
        assertEquals(1f, bars[0].fraction, 1e-6f)
        assertEquals(1, bars.count { it.tone == SplitTone.Accent })
        assertEquals(SplitTone.Accent, bars[0].tone)
        assertEquals(SplitTone.Faint, bars[1].tone)
        assertEquals(SplitTone.Partial, bars[3].tone)
        // Delta against the average full split (503.3 s): −1 / +1 / +1; none for the partial.
        assertEquals(listOf(-1, 1, 1, null), bars.map { it.deltaSec })
        assertEquals(SplitTag.Fastest, bars[0].tag)
        assertEquals(SplitTag.Slowest, bars[1].tag)
        assertNull(bars[2].tag)
    }

    @Test
    fun splitBars_floorSitsAtEightyFivePercentOfTheSlowestSpeed() {
        // A split 5% slower renders at about 75% of the fastest.
        val bars = splitBars(listOf(split(1, 300, "5:00"), split(2, 316, "5:16")))
        assertEquals(0.75f, bars[1].fraction, 0.03f)
        // Fewer than 3 full splits: every full bar is Accent, no tags.
        assertTrue(bars.all { it.tone == SplitTone.Accent && it.tag == null })
    }

    @Test
    fun splitBars_tagsNeedDifferentFastestAndSlowestPaces() {
        val even = splitBars(listOf(split(1, 300, "5:00"), split(2, 300, "5:00"), split(3, 300, "5:00")))
        assertTrue(even.all { it.tag == null })
        assertEquals(listOf(0, 0, 0), even.map { it.deltaSec })
    }

    @Test
    fun splitDeltaLabel_signsAndPads() {
        assertEquals("\u22120:04", splitDeltaLabel(-4))
        assertEquals("+0:07", splitDeltaLabel(7))
        assertEquals("±0:00", splitDeltaLabel(0))
        assertEquals("+1:05", splitDeltaLabel(65))
    }

    @Test
    fun partialIndexLabel_quartersElseDecimal() {
        assertEquals("¼", partialIndexLabel(0.24))
        assertEquals("½", partialIndexLabel(0.5))
        assertEquals("¾", partialIndexLabel(0.8))
        assertEquals("0.08", partialIndexLabel(0.08))
        assertEquals("0.93", partialIndexLabel(0.93))
    }

    @Test
    fun gpxFileName_usesTheRunsLocalStart() {
        // 2026-10-14 07:42 in Berlin (UTC+2).
        val start = java.time.ZonedDateTime.of(2026, 10, 14, 7, 42, 30, 0, ZoneId.of("Europe/Berlin")).toEpochSecond()
        assertEquals("run-2026-10-14-0742.gpx", gpxFileName(start, ZoneId.of("Europe/Berlin")))
    }

    @Test
    fun routeTrack_pointAtWalksCumulativeDistanceAcrossSegments() {
        val metresPerDegLat = 111_195.0
        val seg1 = (0..10).map { GpxFix(47.0 + it * 100.0 / metresPerDegLat, 8.0, it * 30L, null) }
        val seg2 = (0..10).map { GpxFix(47.01 + it * 100.0 / metresPerDegLat, 8.0, 400L + it * 30L, null) }
        val track = RouteTrack(listOf(seg1, seg2, listOf(GpxFix(0.0, 0.0, 0L, null))))
        assertEquals(2, track.segs.size)
        // Two 1 km legs; the gap between segments adds nothing.
        assertEquals(2000.0, track.totalM, 5.0)
        val out = DoubleArray(2)
        track.pointAt(500.0, out)
        assertEquals(47.0 + 500.0 / metresPerDegLat, out[0], 2e-5)
        track.pointAt(1500.0, out)
        assertEquals(47.01 + 500.0 / metresPerDegLat, out[0], 2e-5)
    }

    @Test
    fun routeRamp_runsFromAccentFaintToAccent() {
        for (p in listOf(PaperLightPalette, PaperDarkPalette, RosePalette, IrisLightPalette)) {
            assertEquals(p.accentFaint(), p.routeRamp(0f))
            val end = p.routeRamp(1f)
            assertEquals(p.accent.red, end.red, 0.01f)
            assertEquals(p.accent.green, end.green, 0.01f)
            assertEquals(p.accent.blue, end.blue, 0.01f)
        }
        // Paper's chroma-free accent still changes along the route (light → dark).
        assertTrue(relativeLuminance(PaperLightPalette.routeRamp(0f)) > relativeLuminance(PaperLightPalette.routeRamp(1f)) + 0.1)
    }
}
