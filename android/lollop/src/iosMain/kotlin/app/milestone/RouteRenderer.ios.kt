package app.milestone

import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.painter.Painter
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextMeasurer
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.sp
import app.milestone.resources.Res
import app.milestone.resources.ic_route_finish_flag
import app.milestone.resources.runmap_finish
import app.milestone.resources.runmap_start
import app.milestone.resources.runmap_start_finish
import kotlin.math.roundToInt
import org.jetbrains.compose.resources.painterResource
import org.jetbrains.compose.resources.stringResource

/** The theme's route painter (colours, label text and font), resolved once per palette and density. */
@Composable
internal fun rememberComposeRoutePainter(): ComposeRoutePainter {
    val palette = LocalPalette.current
    val density = LocalDensity.current
    val measurer = rememberTextMeasurer(cacheSize = 16)
    val flag = painterResource(Res.drawable.ic_route_finish_flag)
    val start = stringResource(Res.string.runmap_start)
    val finish = stringResource(Res.string.runmap_finish)
    val loop = stringResource(Res.string.runmap_start_finish)
    return remember(palette, density, measurer, flag, start, finish, loop) {
        ComposeRoutePainter(
            ramp = Array(RouteRampSteps + 1) { palette.routeRamp(it / RouteRampSteps.toFloat()) },
            casing = palette.bgTop,
            accent = palette.accent,
            elevated = palette.bgElevated,
            cut = palette.onBgFaint.copy(alpha = TrimCutAlpha),
            pillFill = palette.bgElevated.copy(alpha = MapChromeAlpha),
            pillBorder = palette.onBgFaint.copy(alpha = HairlineAlpha.Strong),
            density = density,
            measurer = measurer,
            // Android's label paint: the bold default face at the chip size, tabular figures.
            label = Type.Chip.copy(color = palette.onBgBody, fontFeatureSettings = "tnum", lineHeight = TextUnit.Unspecified),
            flag = flag,
            startLabel = start,
            finishLabel = finish,
            loopLabel = loop,
        )
    }
}

/**
 * Draws a [RouteTrack] in Compose as [RouteLayout] places it, with Android's
 * renderer's look (RouteRenderer.android.kt): the `BgTop` casing, the progression
 * ramp (or kept vs dashed trimmed legs), the numbered distance markers, the start
 * ring and finish flag disc (one combined marker on a loop) and the label pills.
 * Holds its own paths and layout; one per surface.
 */
internal class ComposeRoutePainter(
    private val ramp: Array<Color>,
    private val casing: Color,
    private val accent: Color,
    private val elevated: Color,
    private val cut: Color,
    private val pillFill: Color,
    private val pillBorder: Color,
    density: Density,
    private val measurer: TextMeasurer,
    private val label: TextStyle,
    private val flag: Painter,
    startLabel: String,
    finishLabel: String,
    loopLabel: String,
) : RoutePainter {
    private val d = density.density
    private val sample: TextLayoutResult = measurer.measure("0", label, density = density)
    private val layout = RouteLayout(
        density = d,
        text = object : RouteTextMetrics {
            override fun width(label: String) = measure(label).size.width.toFloat()
            override val ascent: Float get() = -sample.firstBaseline
            override val descent: Float get() = sample.getLineBottom(0) - sample.firstBaseline
        },
        startLabel = startLabel,
        finishLabel = finishLabel,
        loopLabel = loopLabel,
    )
    private val textDensity = density
    private val path = Path()
    private val casingStroke = Stroke(width = CasingWidthDp * d, cap = StrokeCap.Round, join = StrokeJoin.Round)
    private val trackStroke = Stroke(width = TrackWidthDp * d, cap = StrokeCap.Round, join = StrokeJoin.Round)
    private val cutStroke = Stroke(
        width = CutWidthDp * d,
        cap = StrokeCap.Butt,
        join = StrokeJoin.Round,
        pathEffect = PathEffect.dashPathEffect(floatArrayOf(CutDashDp * d, CutGapDp * d), 0f),
    )
    private val startRing = Stroke(width = RouteMarker.StartRing.value * d)
    private val distanceRing = Stroke(width = RouteMarker.DistanceRing.value * d)
    private val pillStroke = Stroke(width = d)
    private val flagFilter = ColorFilter.tint(casing)

    private fun measure(text: String, style: TextStyle = label) = measurer.measure(text, style, density = textDensity)

    override fun draw(scope: DrawScope, track: RouteTrack, projector: RouteProjector, bounds: RouteRect, options: RouteDrawOptions) {
        if (track.isEmpty) return
        val l = layout
        l.layout(track, projector, bounds, options)
        val segs = track.segs
        // Casing under every kept leg; trimmed legs dashed, no casing.
        for (si in segs.indices) strokeLegs(scope, si, segs[si], options, keptLegs = true, color = casing, stroke = casingStroke)
        if (options.trim) for (si in segs.indices) strokeLegs(scope, si, segs[si], options, keptLegs = false, color = cut, stroke = cutStroke)
        // The progression ramp, in chunks of consecutive kept legs.
        val xs = l.xs
        val ys = l.ys
        l.forEachRampChunk(track, options) { si, from, to, step ->
            path.reset()
            path.moveTo(xs[si][from], ys[si][from])
            for (k in from + 1..to) path.lineTo(xs[si][k], ys[si][k])
            scope.drawPath(path, ramp[step], style = trackStroke)
        }
        if (!l.hasKept) return

        val halo = RouteMarker.Halo.value * d
        val marks = l.marks
        for (m in 0 until l.placed) {
            val n = marks[m * 3 + 2].toInt()
            if (n > 0) drawDistance(scope, marks[m * 3], marks[m * 3 + 1], n, halo)
        }
        val finishR = RouteMarker.FinishSize.value * d / 2
        if (l.loop) {
            // The start ring drawn around the finish flag disc, a hairline gap apart.
            val ringOuter = finishR + LoopRingGapDp * d + RouteMarker.StartRing.value * d
            drawStart(scope, l.fx, l.fy, ringOuter, halo)
            drawFinish(scope, l.fx, l.fy, finishR, halo = 0f)
            drawPill(scope, l.pillALabel, l.pillA)
        } else {
            drawStart(scope, l.sx, l.sy, RouteMarker.StartSize.value * d / 2, halo)
            drawFinish(scope, l.fx, l.fy, finishR, halo)
            drawPill(scope, l.pillALabel, l.pillA)
            drawPill(scope, l.pillBLabel, l.pillB)
        }
    }

    /** Strokes the runs of consecutive legs of segment [si] that are kept ([keptLegs]) or cut. */
    private fun strokeLegs(
        scope: DrawScope, si: Int, seg: List<GpxFix>, o: RouteDrawOptions, keptLegs: Boolean, color: Color, stroke: Stroke,
    ) {
        val xs = layout.xs[si]
        val ys = layout.ys[si]
        path.reset()
        var open = false
        for (i in 0 until seg.size - 1) {
            if (layout.legKept(seg, i, o) == keptLegs) {
                if (!open) {
                    path.moveTo(xs[i], ys[i])
                    open = true
                }
                path.lineTo(xs[i + 1], ys[i + 1])
            } else {
                open = false
            }
        }
        scope.drawPath(path, color, style = stroke)
    }

    private fun drawStart(scope: DrawScope, x: Float, y: Float, r: Float, halo: Float) {
        val c = Offset(x, y)
        if (halo > 0f) scope.drawCircle(casing, r + halo, c)
        scope.drawCircle(elevated, r, c)
        scope.drawCircle(accent, r - startRing.width / 2, c, style = startRing)
    }

    private fun drawFinish(scope: DrawScope, x: Float, y: Float, r: Float, halo: Float) {
        val c = Offset(x, y)
        if (halo > 0f) scope.drawCircle(casing, r + halo, c)
        scope.drawCircle(accent, r, c)
        val g = (RouteMarker.FinishGlyph.value * d / 2).roundToInt()
        val left = (x.roundToInt() - g).toFloat()
        val top = (y.roundToInt() - g).toFloat()
        scope.translate(left, top) {
            with(flag) { draw(Size(2f * g, 2f * g), colorFilter = flagFilter) }
        }
    }

    private fun drawDistance(scope: DrawScope, x: Float, y: Float, n: Int, halo: Float) {
        val r = RouteMarker.DistanceSize.value * d / 2
        val ringW = distanceRing.width
        val c = Offset(x, y)
        scope.drawCircle(casing, r + halo, c)
        scope.drawCircle(elevated, r, c)
        scope.drawCircle(accent, r - ringW / 2, c, style = distanceRing)
        val text = n.toString()
        // A numeral wider than the disc's inside shrinks to fit (a 100 km ultra).
        val room = (r - ringW) * 2 - d * 2
        var result = measure(text)
        val w = result.size.width
        if (w > room) {
            result = measure(text, label.copy(fontSize = (label.fontSize.value * room / w).sp))
        }
        drawCentred(scope, result, x, y)
    }

    private fun drawPill(scope: DrawScope, text: String, pill: RouteRect) {
        val radius = CornerRadius(Space.Md * d)
        scope.drawRoundRect(pillFill, Offset(pill.left, pill.top), Size(pill.right - pill.left, pill.bottom - pill.top), radius)
        val inset = d / 2
        scope.drawRoundRect(
            pillBorder,
            Offset(pill.left + inset, pill.top + inset),
            Size(pill.right - pill.left - d, pill.bottom - pill.top - d),
            radius,
            style = pillStroke,
        )
        drawCentred(scope, measure(text), pill.centerX, pill.centerY)
    }

    /** [text] centred on ([x], [y]): its line box's middle on [y], as Android's
     *  `y - (ascent + descent) / 2` baseline does. */
    private fun drawCentred(scope: DrawScope, text: TextLayoutResult, x: Float, y: Float) {
        val top = y - text.getLineBottom(0) / 2
        scope.drawText(text, topLeft = Offset(x - text.size.width / 2f, top))
    }
}
