package app.milestone

import android.graphics.Canvas
import android.graphics.DashPathEffect
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF
import android.graphics.Typeface
import android.graphics.drawable.Drawable
import kotlin.math.PI
import kotlin.math.asin
import kotlin.math.cos
import kotlin.math.floor
import kotlin.math.hypot
import kotlin.math.ln
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt
import kotlin.math.sin
import kotlin.math.sqrt
import kotlin.math.tan

// Route geometry and drawing shared by every route surface (run detail card,
// full-screen map, trim preview, finish review): one renderer onto an
// android.graphics.Canvas behind a [RouteProjector], so the osmdroid overlay and the
// tile-less Compose canvas draw identical routes, markers and labels.

/** Mean Earth radius (m), matching the core's EARTH_RADIUS_M = 6_371_000 (running.rs)
 *  so live, route and of-record distances agree. */
private const val EarthRadiusM = 6_371_000.0

/** Start and finish closer than this (metres) are one spot: a loop. */
internal const val LoopCoincidenceM = 30.0

/** Cap on coloured track chunks across the whole route, so a long track draws few paths. */
private const val MaxTrackChunks = 64

/** Steps of the precomputed progression ramp. */
internal const val RouteRampSteps = 32

/** Past this many whole units only every [DenseMarkerStep]th gets a distance marker. */
private const val DenseMarkerUnits = 20
private const val DenseMarkerStep = 5

// dp sizes of the drawn route (multiplied by density in [RouteStyle]).
private const val CasingWidthDp = 10f
private const val TrackWidthDp = 6f
private const val CutWidthDp = 6f
private const val CutDashDp = 7f
private const val CutGapDp = 6f
/** Two marker centres closer than this drop the later one (spec §2.3 thinning). */
private const val MarkerSpacingDp = 28f
/** Gap between a marker's halo and its label pill. */
private const val LabelGapDp = 4f
private const val PillPadXDp = 8f
private const val PillPadYDp = 4f
/** Gap between the loop marker's finish disc and the start ring around it. */
private const val LoopRingGapDp = 2f

/** Great-circle metres between two fixes (haversine on the core's radius): the one
 *  place the segment maths lives, shared by the route renderer, the trim preview
 *  ([timedTrackFromSegments]) and the live tracker ([segmentKm]). */
internal fun haversineM(lat1: Double, lon1: Double, lat2: Double, lon2: Double): Double {
    val dLat = Math.toRadians(lat2 - lat1)
    val dLon = Math.toRadians(lon2 - lon1)
    val a = sin(dLat / 2).let { it * it } +
        cos(Math.toRadians(lat1)) * cos(Math.toRadians(lat2)) * sin(dLon / 2).let { it * it }
    // Clamp asin's input to [0,1]: it can drift a hair past 1.0 on a near-antipodal
    // leg through FP rounding → asin(NaN).
    return 2 * EarthRadiusM * asin(sqrt(a.coerceIn(0.0, 1.0)))
}

/**
 * A run's route ready to draw: the segments with ≥2 fixes, the cumulative distance
 * in metres at every fix (a segment gap, a GPS pause, adds none) and the framing box.
 */
internal class RouteTrack(segments: List<List<GpxFix>>) {
    val segs: List<List<GpxFix>> = segments.filter { it.size >= 2 }
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

/** The theme's route colours and sizes, resolved once per palette and density. */
internal class RouteStyle(
    /** [RouteRampSteps] + 1 progression colours, `AccentFaint` → `Accent` (§0.2). */
    val ramp: IntArray,
    val casing: Int,
    val accent: Int,
    val elevated: Int,
    val body: Int,
    val cut: Int,
    val pillFill: Int,
    val pillBorder: Int,
    val density: Float,
    val textPx: Float,
    /** The finish flag, already tinted OnAccent. */
    val flag: Drawable,
    val startLabel: String,
    val finishLabel: String,
    val loopLabel: String,
    val startDp: Float,
    val startRingDp: Float,
    val finishDp: Float,
    val flagDp: Float,
    val distanceDp: Float,
    val distanceRingDp: Float,
    val haloDp: Float,
)

/** What to draw on top of the geometry. */
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

/**
 * Draws a [RouteTrack]: the ground casing, the progression ramp (or kept vs dashed
 * trimmed legs), the numbered distance markers, the start ring and finish flag
 * disc (one combined marker on a loop) and their "Start" / "Finish" label pills,
 * placed to stay inside [draw]'s bounds and off each other. Holds its own paints
 * and scratch buffers; not thread-safe (one per surface).
 */
internal class RouteRenderer(private val s: RouteStyle) {
    private val d = s.density
    private val line = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeCap = Paint.Cap.ROUND
        strokeJoin = Paint.Join.ROUND
    }
    private val cutPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeCap = Paint.Cap.BUTT
        strokeJoin = Paint.Join.ROUND
        strokeWidth = CutWidthDp * d
        color = s.cut
        pathEffect = DashPathEffect(floatArrayOf(CutDashDp * d, CutGapDp * d), 0f)
    }
    private val fill = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.FILL }
    private val ring = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.STROKE }
    private val text = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        typeface = Typeface.DEFAULT_BOLD
        textSize = s.textPx
        textAlign = Paint.Align.CENTER
        fontFeatureSettings = "tnum"
        color = s.body
    }
    private val path = Path()
    private val pt = FloatArray(2)
    private val geo = DoubleArray(2)
    private var xs: Array<FloatArray> = emptyArray()
    private var ys: Array<FloatArray> = emptyArray()
    private var marks = FloatArray(0)
    private val pillA = RectF()
    private val pillB = RectF()
    private val scratch = RectF()

    fun draw(c: Canvas, track: RouteTrack, projector: RouteProjector, bounds: RectF, o: RouteDrawOptions) {
        if (track.isEmpty) return
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

        // The kept window (whole track unless trimming): first/last kept fix and their
        // cumulative distances, for the markers and the ramp's 0..1.
        var firstS = -1
        var firstI = -1
        var lastS = -1
        var lastI = -1
        for (si in segs.indices) for (i in segs[si].indices) {
            if (kept(segs[si][i].timeSec, o)) {
                if (firstS < 0) { firstS = si; firstI = i }
                lastS = si
                lastI = i
            }
        }
        val fromM = if (firstS >= 0) track.cumM[firstS][firstI] else 0.0
        val untilM = if (lastS >= 0) track.cumM[lastS][lastI] else track.totalM
        val spanM = (untilM - fromM).coerceAtLeast(1.0)

        // Casing under every kept leg.
        line.color = s.casing
        line.strokeWidth = CasingWidthDp * d
        for (si in segs.indices) strokeLegs(c, si, segs[si], o, keptLegs = true, paint = line)
        // Trimmed legs: dashed, no casing.
        if (o.trim) for (si in segs.indices) strokeLegs(c, si, segs[si], o, keptLegs = false, paint = cutPaint)
        // The progression ramp, in chunks of consecutive kept legs.
        val totalPoints = segs.sumOf { it.size }
        val chunk = (totalPoints + MaxTrackChunks - 1) / MaxTrackChunks
        val chunkSize = chunk.coerceAtLeast(2)
        line.strokeWidth = TrackWidthDp * d
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
                line.color = s.ramp[(f * RouteRampSteps).roundToInt()]
                path.reset()
                path.moveTo(xs[si][i], ys[si][i])
                for (k in i + 1..end) path.lineTo(xs[si][k], ys[si][k])
                c.drawPath(path, line)
                i = end
            }
        }
        if (firstS < 0) return

        // Markers: finish first, then start, then the numbers in order; a later one
        // whose centre falls within MarkerSpacingDp of a placed one is dropped.
        val fx = xs[lastS][lastI]
        val fy = ys[lastS][lastI]
        val sx = xs[firstS][firstI]
        val sy = ys[firstS][firstI]
        val spacing = MarkerSpacingDp * d
        val f0 = segs[firstS][firstI]
        val f1 = segs[lastS][lastI]
        val loop = haversineM(f0.lat, f0.lon, f1.lat, f1.lon) <= LoopCoincidenceM ||
            hypot(fx - sx, fy - sy) < spacing
        val units = if (o.distanceMarkers && !o.trim && o.unitM > 0) {
            floor(track.totalM * o.distanceScale / o.unitM + 1e-6).toInt()
        } else {
            0
        }
        val step = if (units > DenseMarkerUnits) DenseMarkerStep else 1
        val maxMarks = 2 + units / step
        if (marks.size < maxMarks * 3) marks = FloatArray(maxMarks * 3)
        var placed = 0
        fun place(x: Float, y: Float, label: Int): Boolean {
            for (k in 0 until placed) {
                if (hypot(marks[k * 3] - x, marks[k * 3 + 1] - y) < spacing) return false
            }
            marks[placed * 3] = x
            marks[placed * 3 + 1] = y
            marks[placed * 3 + 2] = label.toFloat()
            placed++
            return true
        }
        place(fx, fy, -1)
        if (!loop) place(sx, sy, 0)
        var k = step
        while (k <= units) {
            track.pointAt(k * o.unitM / o.distanceScale, geo)
            projector.project(geo[0], geo[1], pt)
            place(pt[0], pt[1], k)
            k += step
        }
        placedCount = placed

        val halo = s.haloDp * d
        for (m in 0 until placed) {
            val label = marks[m * 3 + 2].toInt()
            if (label > 0) drawDistance(c, marks[m * 3], marks[m * 3 + 1], label, halo)
        }
        val finishR = s.finishDp * d / 2
        if (loop) {
            // The start ring drawn around the finish flag disc, a hairline gap apart.
            val ringOuter = finishR + LoopRingGapDp * d + s.startRingDp * d
            drawStart(c, fx, fy, ringOuter, halo)
            drawFinish(c, fx, fy, finishR, halo = 0f)
        } else {
            drawStart(c, sx, sy, s.startDp * d / 2, halo)
            drawFinish(c, fx, fy, finishR, halo)
        }

        // Label pills: finish (or the one loop label) first, then start avoiding it.
        val startR = (if (loop) finishR + LoopRingGapDp * d + s.startRingDp * d else s.startDp * d / 2) + halo
        if (loop) {
            placeLabel(s.loopLabel, fx, fy, startR, bounds, avoid = null, avoidX = Float.NaN, avoidY = 0f, avoidR = 0f, out = pillA)
            drawPill(c, s.loopLabel, pillA)
        } else {
            placeLabel(s.finishLabel, fx, fy, finishR + halo, bounds, avoid = null, avoidX = sx, avoidY = sy, avoidR = startR, out = pillA)
            placeLabel(s.startLabel, sx, sy, startR, bounds, avoid = pillA, avoidX = fx, avoidY = fy, avoidR = finishR + halo, out = pillB)
            drawPill(c, s.finishLabel, pillA)
            drawPill(c, s.startLabel, pillB)
        }
    }

    private fun kept(t: Long, o: RouteDrawOptions) = !o.trim || t in o.keepFromSec..o.keepUntilSec

    private fun legKept(seg: List<GpxFix>, i: Int, o: RouteDrawOptions) =
        kept(seg[i].timeSec, o) && kept(seg[i + 1].timeSec, o)

    /** Strokes the runs of consecutive legs of segment [si] that are kept ([keptLegs]) or cut. */
    private fun strokeLegs(c: Canvas, si: Int, seg: List<GpxFix>, o: RouteDrawOptions, keptLegs: Boolean, paint: Paint) {
        val n = seg.size
        path.reset()
        var open = false
        for (i in 0 until n - 1) {
            val isKept = legKept(seg, i, o)
            if (isKept == keptLegs) {
                if (!open) {
                    path.moveTo(xs[si][i], ys[si][i])
                    open = true
                }
                path.lineTo(xs[si][i + 1], ys[si][i + 1])
            } else {
                open = false
            }
        }
        c.drawPath(path, paint)
    }

    private fun drawStart(c: Canvas, x: Float, y: Float, r: Float, halo: Float) {
        val ringW = s.startRingDp * d
        if (halo > 0f) {
            fill.color = s.casing
            c.drawCircle(x, y, r + halo, fill)
        }
        fill.color = s.elevated
        c.drawCircle(x, y, r, fill)
        ring.color = s.accent
        ring.strokeWidth = ringW
        c.drawCircle(x, y, r - ringW / 2, ring)
    }

    private fun drawFinish(c: Canvas, x: Float, y: Float, r: Float, halo: Float) {
        if (halo > 0f) {
            fill.color = s.casing
            c.drawCircle(x, y, r + halo, fill)
        }
        fill.color = s.accent
        c.drawCircle(x, y, r, fill)
        val g = (s.flagDp * d / 2).roundToInt()
        s.flag.setBounds(x.roundToInt() - g, y.roundToInt() - g, x.roundToInt() + g, y.roundToInt() + g)
        s.flag.draw(c)
    }

    private fun drawDistance(c: Canvas, x: Float, y: Float, n: Int, halo: Float) {
        val r = s.distanceDp * d / 2
        val ringW = s.distanceRingDp * d
        fill.color = s.casing
        c.drawCircle(x, y, r + halo, fill)
        fill.color = s.elevated
        c.drawCircle(x, y, r, fill)
        ring.color = s.accent
        ring.strokeWidth = ringW
        c.drawCircle(x, y, r - ringW / 2, ring)
        val label = n.toString()
        // A numeral wider than the disc's inside shrinks to fit (a 100 km ultra).
        val room = (r - ringW) * 2 - d * 2
        val w = text.measureText(label)
        val size = text.textSize
        if (w > room) text.textSize = size * room / w
        val fm = text.fontMetrics
        c.drawText(label, x, y - (fm.ascent + fm.descent) / 2, text)
        text.textSize = size
    }

    /** Marker centres placed in the current [draw] (finish, start, numbers), for label avoidance. */
    private var placedCount = 0

    /** Whether [rect] covers any numbered distance marker placed in this draw. */
    private fun hitsNumbered(rect: RectF): Boolean {
        val r = (s.distanceDp / 2 + s.haloDp) * d
        for (m in 0 until placedCount) {
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
        label: String, x: Float, y: Float, r: Float, bounds: RectF,
        avoid: RectF?, avoidX: Float, avoidY: Float, avoidR: Float, out: RectF,
    ) {
        val w = text.measureText(label) + PillPadXDp * d * 2
        val fm = text.fontMetrics
        val h = (fm.descent - fm.ascent) + PillPadYDp * d * 2
        val gap = LabelGapDp * d
        // Slide a pill along its side so it stays inside the bounds when it can.
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
                    if (avoid != null && RectF.intersects(scratch, avoid)) continue
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

    private fun circleHits(rect: RectF, cx: Float, cy: Float, r: Float): Boolean {
        val nx = cx.coerceIn(rect.left, rect.right)
        val ny = cy.coerceIn(rect.top, rect.bottom)
        return hypot(cx - nx, cy - ny) < r
    }

    private fun drawPill(c: Canvas, label: String, rect: RectF) {
        val radius = Space.Md * d
        fill.color = s.pillFill
        c.drawRoundRect(rect, radius, radius, fill)
        ring.color = s.pillBorder
        ring.strokeWidth = d
        scratch.set(rect.left + d / 2, rect.top + d / 2, rect.right - d / 2, rect.bottom - d / 2)
        c.drawRoundRect(scratch, radius, radius, ring)
        val fm = text.fontMetrics
        c.drawText(label, rect.centerX(), rect.centerY() - (fm.ascent + fm.descent) / 2, text)
    }
}

/**
 * Web-Mercator framing for a route drawn without tiles: fits the route's box into
 * the canvas between [topPx] and [bottomPx] with [insetPx] on every side (zoom capped
 * at street level), then applies the user's pan and pinch ([userScale], [panX],
 * [panY]). Mirrors osmdroid's projection so a route looks the same with or without
 * tiles.
 */
internal class MercatorFit : RouteProjector {
    private var k = 1.0
    private var wcx = 0.0
    private var wcy = 0.0
    private var cx = 0f
    private var cy = 0f
    private var shift = false
    private var west = 0.0

    fun fit(
        track: RouteTrack, width: Float, height: Float, insetPx: Float, topPx: Float, bottomPx: Float,
        maxWorldPx: Double, userScale: Float = 1f, panX: Float = 0f, panY: Float = 0f,
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
        k = min(fitK, maxWorldPx) * userScale
        wcx = (x0 + x1) / 2
        wcy = (y0 + y1) / 2
        cx = width / 2 + panX
        cy = topPx + (height - topPx - bottomPx) / 2 + panY
    }

    private fun worldX(lon: Double): Double {
        val l = if (shift && lon < west) lon + 360.0 else lon
        return (l + 180.0) / 360.0
    }

    private fun worldY(lat: Double): Double {
        val r = Math.toRadians(lat.coerceIn(-85.0, 85.0))
        return (1.0 - ln(tan(r) + 1.0 / cos(r)) / PI) / 2.0
    }

    override fun project(lat: Double, lon: Double, out: FloatArray) {
        out[0] = (cx + (worldX(lon) - wcx) * k).toFloat()
        out[1] = (cy + (worldY(lat) - wcy) * k).toFloat()
    }
}
