package app.milestone

import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.unit.dp
import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.floor
import kotlin.math.hypot
import kotlin.math.ln
import kotlin.math.min
import kotlin.math.max
import kotlin.math.pow
import kotlin.math.roundToInt
import kotlin.math.tan

// Route geometry shared by every route surface (run detail card, full-screen map,
// trim preview, finish review) on both platforms: the track ([RouteTrack]), its
// Web-Mercator framing ([MercatorFit]) and where the drawn parts go in canvas pixels
// ([RouteLayout]: kept window, progression chunks, thinned markers, label pills).
// Each platform's [RoutePainter] draws a layout: Android onto an android.graphics
// Canvas (the osmdroid overlay and the tile-less canvas), iOS in Compose (over the
// MapKit tiles and on the tile-less canvas), so every surface places the same
// routes, markers and labels.

/** Start and finish closer than this (metres) are one spot: a loop. */
internal const val LoopCoincidenceM = 30.0

/** Cap on coloured track chunks across the whole route, so a long track draws few paths. */
internal const val MaxTrackChunks = 64

internal const val RouteRampSteps = 32

/** Framing never zooms past street level, so a short route keeps its surroundings. */
internal const val MaxFrameZoom = 17.0

/** Past this many whole units only every [DenseMarkerStep]th gets a distance marker. */
private const val DenseMarkerUnits = 20
private const val DenseMarkerStep = 5

// dp sizes of the drawn route (multiplied by density by each painter).
internal const val CasingWidthDp = 10f
internal const val TrackWidthDp = 6f
internal const val CutWidthDp = 6f
internal const val CutDashDp = 7f
internal const val CutGapDp = 6f
/** Two marker centres closer than this drop the later one (spec §2.3 thinning). */
internal const val MarkerSpacingDp = 28f
/** Gap between a marker's halo and its label pill. */
private const val LabelGapDp = 4f
internal const val PillPadXDp = 8f
internal const val PillPadYDp = 4f
/** Gap between the loop marker's finish disc and the start ring around it. */
internal const val LoopRingGapDp = 2f

/**
 * A run's route ready to draw: the segments with ≥2 fixes, the cumulative distance
 * in metres at every fix (a segment gap, a GPS pause, adds none) and the framing box.
 */
internal class RouteTrack(segments: List<List<GpsPoint>>) {
    val segs: List<List<GpsPoint>> = segments.filter { it.size >= 2 }
    val cumM: List<DoubleArray>
    val totalM: Double
    val north: Double
    val south: Double
    /** West/east longitudes, antimeridian-safe ([frameLongitudes]); `west > east` crosses it. */
    val west: Double
    val east: Double
    val isEmpty: Boolean get() = segs.isEmpty()

    init {
        var running = 0.0
        cumM = segs.map { seg ->
            DoubleArray(seg.size).also { arr ->
                for (k in seg.indices) {
                    if (k > 0) running += haversineM(seg[k - 1].lat, seg[k - 1].lon, seg[k].lat, seg[k].lon)
                    arr[k] = running
                }
            }
        }
        totalM = running
        var n = -90.0
        var s = 90.0
        val lons = ArrayList<Double>()
        for (seg in segs) for (f in seg) {
            n = max(n, f.lat)
            s = min(s, f.lat)
            lons += f.lon
        }
        val lon = frameLongitudes(lons)
        north = if (segs.isEmpty()) 0.0 else n
        south = if (segs.isEmpty()) 0.0 else s
        west = lon.west
        east = lon.east
    }

    /** Whether the route crosses the antimeridian: longitudes west of [west] then sit at +360. */
    val crossesDateLine: Boolean get() = west > east

    /** The fix position [m] metres along the route, interpolated, into [out] (lat, lon). */
    fun pointAt(m: Double, out: DoubleArray) {
        for (s in segs.indices) {
            val cum = cumM[s]
            if (m > cum[cum.size - 1] && s < segs.size - 1) continue
            val seg = segs[s]
            var lo = 0
            var hi = cum.size - 1
            if (m <= cum[0]) {
                out[0] = seg[0].lat
                out[1] = seg[0].lon
                return
            }
            if (m >= cum[hi]) {
                out[0] = seg[hi].lat
                out[1] = seg[hi].lon
                return
            }
            while (hi - lo > 1) {
                val mid = (lo + hi) ushr 1
                if (cum[mid] <= m) lo = mid else hi = mid
            }
            val span = cum[hi] - cum[lo]
            val t = if (span > 0) (m - cum[lo]) / span else 0.0
            out[0] = seg[lo].lat + (seg[hi].lat - seg[lo].lat) * t
            out[1] = seg[lo].lon + (seg[hi].lon - seg[lo].lon) * t
            return
        }
    }
}

/** Maps a fix to canvas pixels, writing (x, y) into `out`; allocation-free. */
internal fun interface RouteProjector {
    fun project(lat: Double, lon: Double, out: FloatArray)
}

internal class RouteDrawOptions(
    /** Number every whole unit ([unitM] metres) along the route. */
    val distanceMarkers: Boolean = true,
    val unitM: Double = 1000.0,
    /** Of-record distance ÷ track distance, so the numbers agree with the stated distance. */
    val distanceScale: Double = 1.0,
    /** Trim preview: fixes outside `[keepFromSec, keepUntilSec]` draw dashed and the
     *  markers sit at the kept window's ends. */
    val trim: Boolean = false,
    val keepFromSec: Long = Long.MIN_VALUE,
    val keepUntilSec: Long = Long.MAX_VALUE,
)

/** A mutable pixel rectangle (android.graphics.RectF semantics), reused across draws. */
internal class RouteRect(
    var left: Float = 0f,
    var top: Float = 0f,
    var right: Float = 0f,
    var bottom: Float = 0f,
) {
    val centerX: Float get() = (left + right) * 0.5f
    val centerY: Float get() = (top + bottom) * 0.5f

    fun set(left: Float, top: Float, right: Float, bottom: Float) {
        this.left = left
        this.top = top
        this.right = right
        this.bottom = bottom
    }

    fun set(other: RouteRect) = set(other.left, other.top, other.right, other.bottom)

    /** Whether the two rectangles overlap with a non-empty area (RectF.intersects). */
    fun intersects(other: RouteRect): Boolean =
        left < other.right && other.left < right && top < other.bottom && other.top < bottom
}

/** The label font's measurements, in px, as the painter draws the labels. */
internal interface RouteTextMetrics {
    /** Advance width of [label]. */
    fun width(label: String): Float
    /** Distance from the baseline to the top of the font's box (negative). */
    val ascent: Float
    /** Distance from the baseline to the bottom of the font's box. */
    val descent: Float
}

/** Draws a [RouteTrack] into a Compose [DrawScope] through [projector], keeping its
 *  labels inside [bounds]: the platform's route renderer behind the shared layout. */
internal interface RoutePainter {
    fun draw(scope: DrawScope, track: RouteTrack, projector: RouteProjector, bounds: RouteRect, options: RouteDrawOptions)
}

/**
 * Where a draw of a [RouteTrack] puts things, in canvas pixels: every fix projected
 * ([xs], [ys]), the kept window (whole track unless trimming) with its first/last
 * kept fix and the distances the progression ramp runs between, the start and
 * finish (one combined marker on a [loop]), the numbered distance markers thinned so
 * no two centres sit closer than [MarkerSpacingDp] ([marks]: x, y, label; -1 the
 * finish, 0 the start, n the whole unit), and the "Start" / "Finish" label pills ([pillA] the
 * finish or loop label, [pillB] the start label) placed inside the bounds and off
 * each other and the markers. Holds its own buffers; one per surface.
 */
internal class RouteLayout(
    private val density: Float,
    private val text: RouteTextMetrics,
    private val startLabel: String,
    private val finishLabel: String,
    private val loopLabel: String,
) {
    var xs: Array<FloatArray> = emptyArray()
        private set
    var ys: Array<FloatArray> = emptyArray()
        private set
    var firstS = -1
        private set
    var firstI = -1
        private set
    var lastS = -1
        private set
    var lastI = -1
        private set
    /** Cumulative distance at the first kept fix, and the kept span (≥ 1 m). */
    var fromM = 0.0
        private set
    var spanM = 1.0
        private set
    var loop = false
        private set
    /** Start and finish marker centres. */
    var sx = 0f
        private set
    var sy = 0f
        private set
    var fx = 0f
        private set
    var fy = 0f
        private set
    var marks = FloatArray(0)
        private set
    var placed = 0
        private set
    val pillA = RouteRect()
    val pillB = RouteRect()
    private val scratch = RouteRect()
    private val pt = FloatArray(2)
    private val geo = DoubleArray(2)

    /** Whether any fix is kept: without one there are no markers or labels. */
    val hasKept: Boolean get() = firstS >= 0

    val pillALabel: String get() = if (loop) loopLabel else finishLabel
    val pillBLabel: String get() = startLabel

    fun kept(t: Long, o: RouteDrawOptions) = !o.trim || t in o.keepFromSec..o.keepUntilSec

    fun legKept(seg: List<GpsPoint>, i: Int, o: RouteDrawOptions) =
        kept(seg[i].observedAt, o) && kept(seg[i + 1].observedAt, o)

    /** Lays out a non-empty [track] projected through [projector], labels kept inside [bounds]. */
    fun layout(track: RouteTrack, projector: RouteProjector, bounds: RouteRect, o: RouteDrawOptions) {
        val segs = track.segs
        if (xs.size != segs.size || segs.indices.any { xs[it].size != segs[it].size }) {
            xs = Array(segs.size) { FloatArray(segs[it].size) }
            ys = Array(segs.size) { FloatArray(segs[it].size) }
        }
        for (si in segs.indices) {
            val seg = segs[si]
            for (i in seg.indices) {
                projector.project(seg[i].lat, seg[i].lon, pt)
                xs[si][i] = pt[0]
                ys[si][i] = pt[1]
            }
        }

        firstS = -1
        firstI = -1
        lastS = -1
        lastI = -1
        for (si in segs.indices) for (i in segs[si].indices) {
            if (kept(segs[si][i].observedAt, o)) {
                if (firstS < 0) { firstS = si; firstI = i }
                lastS = si
                lastI = i
            }
        }
        fromM = if (firstS >= 0) track.cumM[firstS][firstI] else 0.0
        val untilM = if (lastS >= 0) track.cumM[lastS][lastI] else track.totalM
        spanM = (untilM - fromM).coerceAtLeast(1.0)
        placed = 0
        if (firstS < 0) return

        val d = density
        fx = xs[lastS][lastI]
        fy = ys[lastS][lastI]
        sx = xs[firstS][firstI]
        sy = ys[firstS][firstI]
        val spacing = MarkerSpacingDp * d
        val f0 = segs[firstS][firstI]
        val f1 = segs[lastS][lastI]
        loop = haversineM(f0.lat, f0.lon, f1.lat, f1.lon) <= LoopCoincidenceM ||
            hypot(fx - sx, fy - sy) < spacing
        val units = if (o.distanceMarkers && !o.trim && o.unitM > 0) {
            floor(track.totalM * o.distanceScale / o.unitM + 1e-6).toInt()
        } else {
            0
        }
        val step = if (units > DenseMarkerUnits) DenseMarkerStep else 1
        val maxMarks = 2 + units / step
        if (marks.size < maxMarks * 3) marks = FloatArray(maxMarks * 3)
        place(fx, fy, -1, spacing)
        if (!loop) place(sx, sy, 0, spacing)
        var k = step
        while (k <= units) {
            track.pointAt(k * o.unitM / o.distanceScale, geo)
            projector.project(geo[0], geo[1], pt)
            place(pt[0], pt[1], k, spacing)
            k += step
        }

        val halo = RouteMarker.Halo.value * d
        val finishR = RouteMarker.FinishSize.value * d / 2
        val startR = (if (loop) finishR + LoopRingGapDp * d + RouteMarker.StartRing.value * d else RouteMarker.StartSize.value * d / 2) + halo
        if (loop) {
            placeLabel(loopLabel, fx, fy, startR, bounds, avoid = null, avoidX = Float.NaN, avoidY = 0f, avoidR = 0f, out = pillA)
        } else {
            placeLabel(finishLabel, fx, fy, finishR + halo, bounds, avoid = null, avoidX = sx, avoidY = sy, avoidR = startR, out = pillA)
            placeLabel(startLabel, sx, sy, startR, bounds, avoid = pillA, avoidX = fx, avoidY = fy, avoidR = finishR + halo, out = pillB)
        }
    }

    private fun place(x: Float, y: Float, label: Int, spacing: Float): Boolean {
        for (k in 0 until placed) {
            if (hypot(marks[k * 3] - x, marks[k * 3 + 1] - y) < spacing) return false
        }
        marks[placed * 3] = x
        marks[placed * 3 + 1] = y
        marks[placed * 3 + 2] = label.toFloat()
        placed++
        return true
    }

    /**
     * The progression ramp in chunks of consecutive kept legs of at most
     * `ceil(points / MaxTrackChunks)` (≥ 2) legs: each [chunk] strokes fixes
     * `from..to` of segment `si` in ramp colour index `ramp` (0..[RouteRampSteps],
     * the chunk's middle fix's place in the kept window).
     */
    inline fun forEachRampChunk(track: RouteTrack, o: RouteDrawOptions, chunk: (si: Int, from: Int, to: Int, ramp: Int) -> Unit) {
        val segs = track.segs
        val totalPoints = segs.sumOf { it.size }
        val chunkSize = ((totalPoints + MaxTrackChunks - 1) / MaxTrackChunks).coerceAtLeast(2)
        for (si in segs.indices) {
            val seg = segs[si]
            val n = seg.size
            var i = 0
            while (i < n - 1) {
                if (!legKept(seg, i, o)) { i++; continue }
                var end = i + 1
                while (end < n - 1 && end - i < chunkSize && legKept(seg, end, o)) end++
                val mid = track.cumM[si][(i + end) / 2]
                val f = ((mid - fromM) / spanM).toFloat().coerceIn(0f, 1f)
                chunk(si, i, end, (f * RouteRampSteps).roundToInt())
                i = end
            }
        }
    }

    private fun hitsNumbered(rect: RouteRect): Boolean {
        val r = (RouteMarker.DistanceSize.value / 2 + RouteMarker.Halo.value) * density
        for (m in 0 until placed) {
            if (marks[m * 3 + 2] > 0f && circleHits(rect, marks[m * 3], marks[m * 3 + 1], r)) return true
        }
        return false
    }

    /**
     * Puts the [label] pill beside the marker at ([x], [y]) of radius [r]: above, else
     * below, left, right (slid along its side to stay inside [bounds]), the first that
     * fits [bounds] and keeps off the [avoid] pill, the other endpoint marker and the
     * numbered markers; failing that, the first that fits [bounds].
     */
    private fun placeLabel(
        label: String, x: Float, y: Float, r: Float, bounds: RouteRect,
        avoid: RouteRect?, avoidX: Float, avoidY: Float, avoidR: Float, out: RouteRect,
    ) {
        val d = density
        val w = text.width(label) + PillPadXDp * d * 2
        val h = (text.descent - text.ascent) + PillPadYDp * d * 2
        val gap = LabelGapDp * d
        fun slideX(cx: Float) = cx.coerceIn(bounds.left + w / 2, (bounds.right - w / 2).coerceAtLeast(bounds.left + w / 2))
        fun slideY(cy: Float) = cy.coerceIn(bounds.top + h / 2, (bounds.bottom - h / 2).coerceAtLeast(bounds.top + h / 2))
        var fallback = false
        for (pass in 0..1) {
            for (side in 0..3) {
                when (side) {
                    0 -> slideX(x).let { cx -> scratch.set(cx - w / 2, y - r - gap - h, cx + w / 2, y - r - gap) }
                    1 -> slideX(x).let { cx -> scratch.set(cx - w / 2, y + r + gap, cx + w / 2, y + r + gap + h) }
                    2 -> slideY(y).let { cy -> scratch.set(x - r - gap - w, cy - h / 2, x - r - gap, cy + h / 2) }
                    else -> slideY(y).let { cy -> scratch.set(x + r + gap, cy - h / 2, x + r + gap + w, cy + h / 2) }
                }
                val inside = scratch.left >= bounds.left && scratch.right <= bounds.right &&
                    scratch.top >= bounds.top && scratch.bottom <= bounds.bottom
                if (!inside) continue
                if (!fallback) {
                    if (avoid != null && scratch.intersects(avoid)) continue
                    if (!avoidX.isNaN() && circleHits(scratch, avoidX, avoidY, avoidR)) continue
                    if (circleHits(scratch, x, y, r)) continue
                    if (hitsNumbered(scratch)) continue
                }
                out.set(scratch)
                return
            }
            fallback = true
        }
        out.set(x - w / 2, y - r - gap - h, x + w / 2, y - r - gap)
    }

    private fun circleHits(rect: RouteRect, cx: Float, cy: Float, r: Float): Boolean {
        val nx = cx.coerceIn(rect.left, rect.right)
        val ny = cy.coerceIn(rect.top, rect.bottom)
        return hypot(cx - nx, cy - ny) < r
    }
}

/** World size in px at [MaxFrameZoom] (256 px tiles): the tile-less canvas's zoom cap. */
internal val MaxWorldPx = 256.0 * 2.0.pow(MaxFrameZoom)

/**
 * Web-Mercator framing for a route drawn by the app (no osmdroid projection): fits
 * the route's box into the canvas between [topPx] and [bottomPx] with [insetPx] on
 * every side (zoom capped at street level), then applies the user's pan and pinch
 * ([userScale], [panX], [panY]). Mirrors osmdroid's projection so a route looks the
 * same with or without tiles. World coordinates run 0..1 across the Web-Mercator
 * square (x east from −180°, y south from 85.05°N).
 */
internal class MercatorFit : RouteProjector {
    private var k = 1.0
    private var wcx = 0.0
    private var wcy = 0.0
    private var cx = 0f
    private var cy = 0f
    private var shift = false
    private var west = 0.0

    /** Canvas px per world width before the user's pinch (the fitted, capped zoom). */
    var fittedScale = 1.0
        private set

    /** Canvas px per world width, the user's pinch included. */
    val scale: Double get() = k

    fun fit(
        track: RouteTrack, width: Float, height: Float, insetPx: Float, topPx: Float, bottomPx: Float,
        userScale: Float, panX: Float, panY: Float,
    ) {
        shift = track.crossesDateLine
        west = track.west
        val x0 = worldX(track.west)
        val x1 = worldX(track.east)
        val y0 = worldY(track.north)
        val y1 = worldY(track.south)
        val freeW = (width - insetPx * 2).coerceAtLeast(1f)
        val freeH = (height - topPx - bottomPx - insetPx * 2).coerceAtLeast(1f)
        val dx = x1 - x0
        val dy = y1 - y0
        val fitK = min(if (dx > 0) freeW / dx else Double.MAX_VALUE, if (dy > 0) freeH / dy else Double.MAX_VALUE)
        fittedScale = min(fitK, MaxWorldPx)
        k = fittedScale * userScale
        wcx = (x0 + x1) / 2
        wcy = (y0 + y1) / 2
        cx = width / 2 + panX
        cy = topPx + (height - topPx - bottomPx) / 2 + panY
    }

    /** World x of [lon]; with a dateline-crossing route, longitudes west of its west
     *  edge continue past 1 so the route stays contiguous. */
    fun worldX(lon: Double): Double {
        val l = if (shift && lon < west) lon + 360.0 else lon
        return (l + 180.0) / 360.0
    }

    /** World y of [lat] (clamped to ±85°, the Web-Mercator square). */
    fun worldY(lat: Double): Double {
        val r = toRadians(lat.coerceIn(-85.0, 85.0))
        return (1.0 - ln(tan(r) + 1.0 / cos(r)) / PI) / 2.0
    }

    /** The world point under canvas pixel ([px], [py]). */
    fun worldXAt(px: Float): Double = wcx + (px - cx) / k
    fun worldYAt(py: Float): Double = wcy + (py - cy) / k

    override fun project(lat: Double, lon: Double, out: FloatArray) {
        out[0] = (cx + (worldX(lon) - wcx) * k).toFloat()
        out[1] = (cy + (worldY(lat) - wcy) * k).toFloat()
    }
}

/** West/east longitude pair for a framed route. `west > east` is legal and means
 *  the box crosses the antimeridian, osmdroid's zoom/center math adds 360 to a
 *  negative span, so it frames such a box tightly (verified against 6.1.20). */
internal data class LonBounds(val west: Double, val east: Double)

/**
 * Antimeridian-safe longitude framing. Naive min/max is wrong for a track
 * straddling ±180°: −179° and +179° are one degree apart on the ground but look
 * 358° apart, so a plain bounding box stretches the "long way" around the globe
 * and zooms out to the whole world.
 *
 * A run is a local effort, so its true longitude span is small. When the raw span
 * exceeds 180° the box has almost certainly wrapped the long way, so re-measure
 * with every negative longitude shifted +360 (−179 → 181, one degree from 179) and
 * keep whichever framing is tighter. The tighter shifted bounds are then wrapped
 * back into [−180, 180); if the shifted span isn't actually smaller the track
 * genuinely covers a huge range (not a dateline artefact) and the honest naive box
 * is kept, never a broken zoom.
 */
internal fun frameLongitudes(lons: List<Double>): LonBounds {
    if (lons.isEmpty()) return LonBounds(0.0, 0.0)
    val minRaw = lons.min()
    val maxRaw = lons.max()
    val rawSpan = maxRaw - minRaw
    if (rawSpan <= 180.0) return LonBounds(minRaw, maxRaw)
    val shifted = lons.map { if (it < 0.0) it + 360.0 else it }
    val minShift = shifted.min()
    val maxShift = shifted.max()
    if (maxShift - minShift >= rawSpan) return LonBounds(minRaw, maxRaw)
    fun wrap(v: Double) = if (v > 180.0) v - 360.0 else v
    return LonBounds(west = wrap(minShift), east = wrap(maxShift))
}

/**
 * Route markers (map, trim, finish review). Start: an [StartSize] disc with an
 * [StartRing] `Accent` ring and a `BgElevated` centre. Finish: a solid `Accent`
 * [FinishSize] disc holding a [FinishGlyph] OnAccent flag
 * (`R.drawable.ic_route_finish_flag`). Distance: a [DistanceSize] `BgElevated`
 * disc with a [DistanceRing] `Accent` ring and a `Type.Chip` tnum `OnBgBody`
 * numeral at every whole km/mi. Every marker sits on a [Halo] casing in the
 * polyline's ground-casing colour so it reads on any tile.
 */
object RouteMarker {
    val StartSize = 18.dp
    val StartRing = 4.dp
    val FinishSize = 26.dp
    val FinishGlyph = 16.dp
    val DistanceSize = 22.dp
    val DistanceRing = 1.5.dp
    val Halo = 3.dp
}

