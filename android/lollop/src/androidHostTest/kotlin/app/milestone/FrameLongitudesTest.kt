package app.milestone

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * [frameLongitudes] frames a route's longitude span for auto-zoom. The naive
 * min/max it replaces stretches a bounding box the "long way" around the globe
 * for any track crossing ±180°, zooming out to the whole world. These cases pin
 * the antimeridian handling and the honest fallbacks (osmdroid reads a `west >
 * east` pair as a dateline-crossing box, so a tight crossing box is valid).
 */
class FrameLongitudesTest {

    @Test
    fun ordinaryTrackKeepsNaiveBounds() {
        // A normal local run well clear of the dateline: plain min/max, west < east.
        val b = frameLongitudes(listOf(13.40, 13.38, 13.42, 13.39))
        assertEquals(13.38, b.west, 1e-9)
        assertEquals(13.42, b.east, 1e-9)
    }

    @Test
    fun datelineCrossingTrackFramesTight() {
        // A run straddling 180°: points at 179.9 and −179.9 are 0.2° apart on the
        // ground. Naive min/max would report a 359.8° span; the fix keeps it tight,
        // as a west>east crossing box (west near +180, east near −180).
        val b = frameLongitudes(listOf(179.9, 179.95, -179.95, -179.9))
        assertEquals(179.9, b.west, 1e-9)
        assertEquals(-179.9, b.east, 1e-9)
        assertTrue("crossing box has west > east", b.west > b.east)
        // Dateline-aware span (osmdroid: east − west, +360 if negative) is the true
        // 0.2°, not the bogus 359.8°.
        val span = (b.east - b.west).let { if (it < 0) it + 360.0 else it }
        assertEquals(0.2, span, 1e-9)
    }

    @Test
    fun genuinelyWideTrackKeepsHonestBox() {
        // Points spread across a real >180° range that shifting does NOT tighten
        // (−100, 0, +100 → raw span 200; shifting the −100 to 260 gives span 260,
        // wider still): the run really is that wide, so keep the honest naive box
        // rather than fabricating a bogus tight crossing frame.
        val b = frameLongitudes(listOf(-100.0, 0.0, 100.0))
        assertEquals(-100.0, b.west, 1e-9)
        assertEquals(100.0, b.east, 1e-9)
    }

    @Test
    fun emptyIsSafe() {
        val b = frameLongitudes(emptyList())
        assertEquals(0.0, b.west, 1e-9)
        assertEquals(0.0, b.east, 1e-9)
    }

    @Test
    fun exactly180SpanIsNotTreatedAsCrossing() {
        // The >180 trigger is strict: an exactly-180 span stays a naive box.
        val b = frameLongitudes(listOf(-90.0, 90.0))
        assertEquals(-90.0, b.west, 1e-9)
        assertEquals(90.0, b.east, 1e-9)
    }
}
