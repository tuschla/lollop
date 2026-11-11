package app.milestone

import android.graphics.Canvas
import android.graphics.DashPathEffect
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF
import android.graphics.Typeface
import android.graphics.drawable.Drawable
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.graphics.nativeCanvas
import kotlin.math.roundToInt

/** The theme's route colours, density and labels, resolved once per palette and
 *  density; the marker sizes come from [RouteMarker]. */
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
    /** The finish flag, already tinted (the route's casing colour). */
    val flag: Drawable,
    val startLabel: String,
    val finishLabel: String,
    val loopLabel: String,
)

/**
 * Draws a [RouteTrack] onto an android.graphics [Canvas] as [RouteLayout] places it:
 * the ground casing, the progression ramp (or kept vs dashed trimmed legs), the
 * numbered distance markers, the start ring and finish flag disc (one combined
 * marker on a loop) and their "Start" / "Finish" label pills. The osmdroid overlay
 * and the tile-less Compose canvas both draw through it. Holds its own paints and
 * scratch buffers; not thread-safe (one per surface).
 */
internal class RouteRenderer(private val s: RouteStyle) : RoutePainter {
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
    private val layout = RouteLayout(
        density = d,
        text = object : RouteTextMetrics {
            override fun width(label: String) = text.measureText(label)
            override val ascent: Float get() = text.fontMetrics.ascent
            override val descent: Float get() = text.fontMetrics.descent
        },
        startLabel = s.startLabel,
        finishLabel = s.finishLabel,
        loopLabel = s.loopLabel,
    )
    private val path = Path()
    private val rect = RectF()

    override fun draw(scope: DrawScope, track: RouteTrack, projector: RouteProjector, bounds: RouteRect, options: RouteDrawOptions) {
        scope.drawIntoCanvas { draw(it.nativeCanvas, track, projector, bounds, options) }
    }

    fun draw(c: Canvas, track: RouteTrack, projector: RouteProjector, bounds: RouteRect, o: RouteDrawOptions) {
        if (track.isEmpty) return
        val l = layout
        l.layout(track, projector, bounds, o)
        val segs = track.segs

        // Casing under every kept leg.
        line.color = s.casing
        line.strokeWidth = CasingWidthDp * d
        for (si in segs.indices) strokeLegs(c, si, segs[si], o, keptLegs = true, paint = line)
        // Trimmed legs: dashed, no casing.
        if (o.trim) for (si in segs.indices) strokeLegs(c, si, segs[si], o, keptLegs = false, paint = cutPaint)
        // The progression ramp, in chunks of consecutive kept legs.
        line.strokeWidth = TrackWidthDp * d
        val xs = l.xs
        val ys = l.ys
        l.forEachRampChunk(track, o) { si, from, to, ramp ->
            line.color = s.ramp[ramp]
            path.reset()
            path.moveTo(xs[si][from], ys[si][from])
            for (k in from + 1..to) path.lineTo(xs[si][k], ys[si][k])
            c.drawPath(path, line)
        }
        if (!l.hasKept) return

        val halo = RouteMarker.Halo.value * d
        val marks = l.marks
        for (m in 0 until l.placed) {
            val label = marks[m * 3 + 2].toInt()
            if (label > 0) drawDistance(c, marks[m * 3], marks[m * 3 + 1], label, halo)
        }
        val finishR = RouteMarker.FinishSize.value * d / 2
        if (l.loop) {
            // The start ring drawn around the finish flag disc, a hairline gap apart.
            val ringOuter = finishR + LoopRingGapDp * d + RouteMarker.StartRing.value * d
            drawStart(c, l.fx, l.fy, ringOuter, halo)
            drawFinish(c, l.fx, l.fy, finishR, halo = 0f)
            drawPill(c, l.pillALabel, l.pillA)
        } else {
            drawStart(c, l.sx, l.sy, RouteMarker.StartSize.value * d / 2, halo)
            drawFinish(c, l.fx, l.fy, finishR, halo)
            drawPill(c, l.pillALabel, l.pillA)
            drawPill(c, l.pillBLabel, l.pillB)
        }
    }

    /** Strokes the runs of consecutive legs of segment [si] that are kept ([keptLegs]) or cut. */
    private fun strokeLegs(c: Canvas, si: Int, seg: List<GpsPoint>, o: RouteDrawOptions, keptLegs: Boolean, paint: Paint) {
        val xs = layout.xs[si]
        val ys = layout.ys[si]
        val n = seg.size
        path.reset()
        var open = false
        for (i in 0 until n - 1) {
            val isKept = layout.legKept(seg, i, o)
            if (isKept == keptLegs) {
                if (!open) {
                    path.moveTo(xs[i], ys[i])
                    open = true
                }
                path.lineTo(xs[i + 1], ys[i + 1])
            } else {
                open = false
            }
        }
        c.drawPath(path, paint)
    }

    private fun drawStart(c: Canvas, x: Float, y: Float, r: Float, halo: Float) {
        val ringW = RouteMarker.StartRing.value * d
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
        val g = (RouteMarker.FinishGlyph.value * d / 2).roundToInt()
        s.flag.setBounds(x.roundToInt() - g, y.roundToInt() - g, x.roundToInt() + g, y.roundToInt() + g)
        s.flag.draw(c)
    }

    private fun drawDistance(c: Canvas, x: Float, y: Float, n: Int, halo: Float) {
        val r = RouteMarker.DistanceSize.value * d / 2
        val ringW = RouteMarker.DistanceRing.value * d
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

    private fun drawPill(c: Canvas, label: String, pill: RouteRect) {
        val radius = Space.Md * d
        fill.color = s.pillFill
        rect.set(pill.left, pill.top, pill.right, pill.bottom)
        c.drawRoundRect(rect, radius, radius, fill)
        ring.color = s.pillBorder
        ring.strokeWidth = d
        rect.set(pill.left + d / 2, pill.top + d / 2, pill.right - d / 2, pill.bottom - d / 2)
        c.drawRoundRect(rect, radius, radius, ring)
        val fm = text.fontMetrics
        c.drawText(label, pill.centerX, pill.centerY - (fm.ascent + fm.descent) / 2, text)
    }
}
