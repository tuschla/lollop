package app.milestone

import kotlin.math.abs
import kotlin.math.hypot
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class RouteGeometryTest {
    private val metresPerDegLat = 111_195.0

    private fun north(lat: Double, lon: Double, n: Int, stepM: Double, t0: Long = 0L, stepSec: Long = 10L) =
        (0 until n).map { GpsPoint(lat + it * stepM / metresPerDegLat, lon, t0 + it * stepSec, 5.0) }

    /** Label metrics with a fixed advance per character, so pill sizes are exact. */
    private val metrics = object : RouteTextMetrics {
        override fun width(label: String) = label.length * 7f
        override val ascent = -12f
        override val descent = 4f
    }

    private fun layout() = RouteLayout(1f, metrics, startLabel = "Start", finishLabel = "Finish", loopLabel = "Start · Finish")

    private fun fitted(track: RouteTrack, w: Float = 400f, h: Float = 300f, inset: Float = 24f) =
        MercatorFit().also { it.fit(track, w, h, inset, 0f, 0f, 1f, 0f, 0f) }

    @Test
    fun trackKeepsRealSegmentsAndCountsNoGap() {
        val a = north(47.0, 8.0, 11, 100.0)
        val b = north(47.02, 8.0, 11, 100.0, t0 = 1000L)
        val track = RouteTrack(listOf(a, listOf(GpsPoint(0.0, 0.0, 0L, 5.0)), b))
        assertEquals(2, track.segs.size, "a one-fix segment draws nothing")
        assertEquals(2000.0, track.totalM, 5.0)
        // Cumulative distance carries over the gap unchanged.
        assertEquals(track.cumM[0].last(), track.cumM[1].first(), 1e-9)
        assertEquals(47.02 + 1000.0 / metresPerDegLat, track.north, 1e-9)
        assertEquals(47.0, track.south, 1e-9)
        assertFalse(track.crossesDateLine)
    }

    @Test
    fun pointAtInterpolatesAndClamps() {
        val track = RouteTrack(listOf(north(47.0, 8.0, 3, 100.0)))
        val out = DoubleArray(2)
        track.pointAt(150.0, out)
        assertEquals(47.0 + 150.0 / metresPerDegLat, out[0], 1e-6)
        track.pointAt(-5.0, out)
        assertEquals(47.0, out[0], 1e-12)
        track.pointAt(1e9, out)
        assertEquals(47.0 + 200.0 / metresPerDegLat, out[0], 1e-12)
    }

    @Test
    fun fitCentresTheBoxInsideTheInset() {
        val track = RouteTrack(listOf(north(47.0, 8.0, 2, 1000.0), north(47.0, 8.01, 2, 1000.0)))
        val fit = fitted(track)
        val sw = FloatArray(2).also { fit.project(track.south, track.west, it) }
        val ne = FloatArray(2).also { fit.project(track.north, track.east, it) }
        // The binding axis (height here: 1 km tall, ~760 m wide) fills the free height exactly.
        assertEquals(24f, ne[1], 0.01f)
        assertEquals(276f, sw[1], 0.01f)
        assertTrue(sw[0] >= 24f && ne[0] <= 376f, "the box stays inside the side insets")
        assertEquals(200f, (sw[0] + ne[0]) / 2, 0.01f)
        assertEquals(150f, (sw[1] + ne[1]) / 2, 0.01f)
    }

    @Test
    fun fitNeverZoomsPastStreetLevel() {
        val track = RouteTrack(listOf(north(47.0, 8.0, 2, 5.0)))
        val fit = fitted(track)
        assertEquals(MaxWorldPx, fit.fittedScale, 1e-6)
        val a = FloatArray(2).also { fit.project(track.south, 8.0, it) }
        val b = FloatArray(2).also { fit.project(track.north, 8.0, it) }
        // 5 m at 47° N at zoom 17 is a handful of pixels, not the whole canvas.
        assertTrue(abs(a[1] - b[1]) < 20f)
    }

    @Test
    fun panAndPinchMoveTheProjectionAboutTheFittedCentre() {
        val track = RouteTrack(listOf(north(47.0, 8.0, 2, 1000.0)))
        val fit = MercatorFit()
        fit.fit(track, 400f, 300f, 24f, 0f, 0f, 1f, 0f, 0f)
        val base = FloatArray(2).also { fit.project(track.north, 8.0, it) }
        fit.fit(track, 400f, 300f, 24f, 0f, 0f, 2f, 10f, -5f)
        val moved = FloatArray(2).also { fit.project(track.north, 8.0, it) }
        assertEquals(200f + (base[0] - 200f) * 2 + 10f, moved[0], 0.01f)
        assertEquals(150f + (base[1] - 150f) * 2 - 5f, moved[1], 0.01f)
        assertEquals(fit.fittedScale * 2, fit.scale, 1e-6)
    }

    @Test
    fun chromeShiftsTheFittedCentreIntoTheFreeArea() {
        val track = RouteTrack(listOf(north(47.0, 8.0, 2, 1000.0)))
        val fit = MercatorFit()
        fit.fit(track, 400f, 600f, 48f, 100f, 200f, 1f, 0f, 0f)
        val top = FloatArray(2).also { fit.project(track.north, 8.0, it) }
        val bottom = FloatArray(2).also { fit.project(track.south, 8.0, it) }
        assertEquals(100f + 48f, top[1], 0.01f)
        assertEquals(600f - 200f - 48f, bottom[1], 0.01f)
    }

    @Test
    fun worldRectRoundTripsThroughTheProjection() {
        val track = RouteTrack(listOf(north(52.5, 13.4, 2, 2000.0)))
        val fit = fitted(track)
        val p = FloatArray(2).also { fit.project(52.51, 13.4, it) }
        assertEquals(fit.worldX(13.4), fit.worldXAt(p[0]), 1e-9)
        assertEquals(fit.worldY(52.51), fit.worldYAt(p[1]), 1e-9)
        // The canvas spans width / scale of the world.
        assertEquals(400.0 / fit.scale, fit.worldXAt(400f) - fit.worldXAt(0f), 1e-12)
        // Web-Mercator: the equator sits at the middle, x runs from −180°.
        assertEquals(0.5, fit.worldY(0.0), 1e-12)
        assertEquals(0.0, fit.worldX(-180.0), 1e-12)
    }

    @Test
    fun datelineRouteStaysContiguous() {
        val seg = listOf(
            GpsPoint(-17.0, 179.99, 0L, 5.0),
            GpsPoint(-17.0, -179.99, 10L, 5.0),
        )
        val track = RouteTrack(listOf(seg))
        assertTrue(track.crossesDateLine)
        val fit = fitted(track)
        val a = FloatArray(2).also { fit.project(-17.0, 179.99, it) }
        val b = FloatArray(2).also { fit.project(-17.0, -179.99, it) }
        // East of the line draws just right of west of it, not a world away.
        assertTrue(b[0] > a[0])
        assertTrue(b[0] - a[0] <= 352f + 0.01f)
    }

    @Test
    fun layoutPlacesStartFinishAndThinsMarkers() {
        // 3.5 km due north: start, finish and the 1, 2, 3 km markers.
        val track = RouteTrack(listOf(north(47.0, 8.0, 36, 100.0)))
        val fit = MercatorFit().also { it.fit(track, 300f, 1200f, 24f, 0f, 0f, 1f, 0f, 0f) }
        val l = layout()
        val bounds = RouteRect(0f, 0f, 300f, 1200f)
        l.layout(track, fit, bounds, RouteDrawOptions())
        assertTrue(l.hasKept)
        assertFalse(l.loop)
        val labels = (0 until l.placed).map { l.marks[it * 3 + 2].toInt() }
        assertEquals(listOf(-1, 0, 1, 2, 3), labels)
        assertEquals(l.ys[0].last(), l.fy, 0f)
        assertEquals(l.ys[0].first(), l.sy, 0f)
        // No two placed centres closer than the marker spacing.
        for (i in 0 until l.placed) for (j in i + 1 until l.placed) {
            assertTrue(hypot(l.marks[i * 3] - l.marks[j * 3], l.marks[i * 3 + 1] - l.marks[j * 3 + 1]) >= MarkerSpacingDp)
        }
        // Pills: inside the bounds, apart, sized by the font.
        for (pill in listOf(l.pillA, l.pillB)) {
            assertTrue(pill.left >= 0f && pill.right <= 300f && pill.top >= 0f && pill.bottom <= 1200f)
            assertEquals((metrics.descent - metrics.ascent) + 2 * PillPadYDp, pill.bottom - pill.top, 1e-4f)
        }
        assertEquals(metrics.width("Finish") + 2 * PillPadXDp, l.pillA.right - l.pillA.left, 1e-4f)
        assertFalse(l.pillA.intersects(l.pillB))
    }

    @Test
    fun denseMarkersThinOutOnASmallCanvas() {
        // 10 km squeezed into 100 px: the km markers would overlap, so most drop.
        val track = RouteTrack(listOf(north(47.0, 8.0, 101, 100.0)))
        val fit = MercatorFit().also { it.fit(track, 100f, 100f, 0f, 0f, 0f, 1f, 0f, 0f) }
        val l = layout()
        l.layout(track, fit, RouteRect(0f, 0f, 100f, 100f), RouteDrawOptions())
        assertTrue(l.placed < 12)
        assertEquals(-1, l.marks[2].toInt(), "the finish is placed first")
        // What stays keeps the marker spacing.
        for (i in 0 until l.placed) for (j in i + 1 until l.placed) {
            assertTrue(hypot(l.marks[i * 3] - l.marks[j * 3], l.marks[i * 3 + 1] - l.marks[j * 3 + 1]) >= MarkerSpacingDp)
        }
    }

    @Test
    fun loopGetsOneCombinedLabel() {
        val out = north(47.0, 8.0, 11, 100.0)
        val back = out.reversed().mapIndexed { i, f -> f.copy(lon = 8.002, observedAt = 200L + i * 10L) }
        val home = GpsPoint(47.0, 8.0, 400L, 5.0)
        val track = RouteTrack(listOf(out + back + home))
        val l = layout()
        l.layout(track, fitted(track), RouteRect(0f, 0f, 400f, 300f), RouteDrawOptions(distanceMarkers = false))
        assertTrue(l.loop)
        assertEquals("Start · Finish", l.pillALabel)
        assertEquals(1, l.placed, "only the combined marker")
    }

    @Test
    fun trimKeepsTheWindowAndColoursItsRamp() {
        val track = RouteTrack(listOf(north(47.0, 8.0, 11, 100.0, stepSec = 10L)))
        val l = layout()
        val o = RouteDrawOptions(distanceMarkers = false, trim = true, keepFromSec = 20L, keepUntilSec = 70L)
        l.layout(track, fitted(track), RouteRect(0f, 0f, 400f, 300f), o)
        assertEquals(2, l.firstI)
        assertEquals(7, l.lastI)
        assertEquals(200.0, l.fromM, 1.0)
        assertEquals(500.0, l.spanM, 1.0)
        val chunks = ArrayList<IntArray>()
        l.forEachRampChunk(track, o) { si, from, to, ramp -> chunks += intArrayOf(si, from, to, ramp) }
        // Only kept legs (2..7) are coloured, in order, from the faint end to the accent.
        assertEquals(2, chunks.first()[1])
        assertEquals(7, chunks.last()[2])
        assertTrue(chunks.zipWithNext().all { (a, b) -> a[2] == b[1] && a[3] <= b[3] })
        assertTrue(chunks.all { it[3] in 0..RouteRampSteps })
    }

    @Test
    fun trimWithNothingKeptHasNoMarkers() {
        val track = RouteTrack(listOf(north(47.0, 8.0, 5, 100.0)))
        val l = layout()
        val o = RouteDrawOptions(trim = true, keepFromSec = 1000L, keepUntilSec = 2000L)
        l.layout(track, fitted(track), RouteRect(0f, 0f, 400f, 300f), o)
        assertFalse(l.hasKept)
        assertEquals(0, l.placed)
        var chunks = 0
        l.forEachRampChunk(track, o) { _, _, _, _ -> chunks++ }
        assertEquals(0, chunks)
    }

    @Test
    fun longTracksDrawInAtMostSixtyFourishChunks() {
        val track = RouteTrack(listOf(north(47.0, 8.0, 5000, 2.0)))
        val l = layout()
        val o = RouteDrawOptions(distanceMarkers = false)
        l.layout(track, fitted(track), RouteRect(0f, 0f, 400f, 300f), o)
        var chunks = 0
        l.forEachRampChunk(track, o) { _, _, _, _ -> chunks++ }
        assertTrue(chunks in 60..MaxTrackChunks + 1, "chunks=$chunks")
    }

    @Test
    fun labelSlidesInsideTheBoundsNearAnEdge() {
        // The finish sits at the top edge: its pill can't go above, so it goes below or beside.
        val track = RouteTrack(listOf(north(47.0, 8.0, 11, 100.0)))
        val fit = MercatorFit().also { it.fit(track, 400f, 300f, 0f, 0f, 0f, 1f, 0f, 0f) }
        val l = layout()
        l.layout(track, fit, RouteRect(0f, 0f, 400f, 300f), RouteDrawOptions(distanceMarkers = false))
        assertTrue(l.pillA.top >= 0f)
        assertTrue(l.pillA.top >= l.fy || l.pillA.left >= l.fx || l.pillA.right <= l.fx)
    }

    @Test
    fun rectIntersectionIsStrict() {
        val a = RouteRect(0f, 0f, 10f, 10f)
        assertTrue(a.intersects(RouteRect(9f, 9f, 20f, 20f)))
        assertFalse(a.intersects(RouteRect(10f, 0f, 20f, 10f)), "touching edges don't overlap")
        assertEquals(5f, a.centerX)
    }
}
