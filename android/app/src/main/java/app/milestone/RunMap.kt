package app.milestone

import android.annotation.SuppressLint
import android.content.res.Resources
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.drawable.BitmapDrawable
import android.graphics.drawable.Drawable
import android.view.MotionEvent
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.graphics.ColorUtils
import org.osmdroid.tileprovider.tilesource.TileSourceFactory
import org.osmdroid.util.BoundingBox
import org.osmdroid.util.GeoPoint
import org.osmdroid.views.CustomZoomButtonsController
import org.osmdroid.views.MapView
import org.osmdroid.views.overlay.Marker
import org.osmdroid.views.overlay.Polyline
import kotlin.math.ceil

// dp tokens (owner rule: no raw px, every size below is `token * density`).
private const val CasingWidthDp = 10f     // ground casing, drawn once per segment
private const val TrackWidthDp = 6f        // colored progression track
private const val MarkerDiameterDp = 16f   // start / finish marker circle
private const val MarkerRingDp = 3f        // contrasting ring around a marker
private const val LoopMarkerDiameterDp = 22f // combined start-ring + finish-dot marker
// Saturation multiplier for the muted start tone of the progression track: the
// run begins in a quiet, low-chroma version of the accent hue and eases to the
// full accent at the finish (owner: "muted start tone → full accent").
private const val StartToneSaturation = 0.4f
// Two points closer than this (metres) are treated as the same spot (loop run):
// one combined marker is drawn instead of an overlapping start + finish pair.
private const val LoopCoincidenceM = 25.0
// Cap on colored track polylines across the whole route so a very long GPX
// track doesn't spawn thousands of overlays; chunk size grows with point count.
private const val MaxTrackChunks = 64

/**
 * Static-ish route map for a SAVED run (entry detail sheet): the core-produced
 * GPX parsed back into per-segment polylines, auto-framed to the track's
 * bounding box. Same two-layer stroke idea as the live tracker (06 spec), at
 * detail-card weights, a [CasingWidthDp] ground casing under a [TrackWidthDp]
 * colored track, dp tokens density-scaled (owner rule: no raw px). Pinch/drag
 * work; touches are kept from the host bottom sheet so panning the map never
 * drags the sheet.
 *
 * Legibility layer (owner ask): the flat accent track is replaced by a calm
 * progression along cumulative distance, a muted, low-chroma tone of the accent
 * hue at the start easing to the full accent at the finish, so self-crossing
 * routes read directionally without any overlaid marker clutter; and
 * [makeMarkerDrawable]/[makeLoopMarkerDrawable] start + finish markers show where
 * the run began and ended (a single combined marker when it is a loop back to the
 * start).
 */
@SuppressLint("ClickableViewAccessibility")
@Composable
fun RunRouteMap(gpx: String, modifier: Modifier = Modifier) {
    val ctx = LocalContext.current
    val palette = LocalPalette.current
    val segments = remember(gpx) {
        runCatching { parseGpx(gpx) }.getOrDefault(emptyList())
            .filter { it.size >= 2 }
    }
    if (segments.isEmpty()) return

    val density = ctx.resources.displayMetrics.density
    val res = ctx.resources
    val accentArgb = palette.accent.toArgb()
    val casingArgb = palette.bgTop.toArgb()
    val finishArgb = palette.onBgBody.toArgb()
    // Progression start tone: the same accent hue and lightness at a fraction of
    // its saturation, so the run opens in a quiet, muted colour and eases up to
    // the full accent at the finish. Keeping hue + lightness fixed (only chroma
    // moves) reads calm in BOTH themes, the accent is already theme-legible, and
    // a low-chroma variant of it stays legible over either tile style.
    val startTone = run {
        val hsl = FloatArray(3)
        ColorUtils.colorToHSL(accentArgb, hsl)
        hsl[1] *= StartToneSaturation
        ColorUtils.HSLToColor(hsl)
    }

    // The AndroidView `update` lambda runs on EVERY recomposition, but the overlay
    // rebuild + initial `zoomToBoundingBox` framing must run only when the rendered
    // content actually changes: re-framing on an unrelated recompose would discard
    // the user's pinch/pan, and re-adding the first-layout listener each pass would
    // stack listeners. Key on the track's stable identity plus the theme colors (so
    // a theme switch still recolors) and skip the work when the key is unchanged.
    val lastKey = remember { mutableStateOf<List<Int>?>(null) }

    AndroidView(
        modifier = modifier
            .fillMaxWidth()
            .height(220.dp)
            .clip(RoundedCornerShape(Space.Card.dp)),
        factory = { c ->
            MapView(c).apply {
                setTileSource(TileSourceFactory.MAPNIK)
                setMultiTouchControls(true)
                zoomController.setVisibility(CustomZoomButtonsController.Visibility.NEVER)
                // The map lives inside a ModalBottomSheet: claim the gesture on
                // touch-down so a pan moves the MAP, not the sheet.
                setOnTouchListener { v, e ->
                    if (e.actionMasked == MotionEvent.ACTION_DOWN) {
                        v.parent?.requestDisallowInterceptTouchEvent(true)
                    }
                    false
                }
            }
        },
        update = { map ->
            val key = listOf(
                System.identityHashCode(segments),
                accentArgb, casingArgb, finishArgb, startTone,
            )
            // Same track + same theme as last render → nothing to rebuild; leaving
            // the map untouched preserves the user's current pinch/pan.
            if (lastKey.value == key) return@AndroidView
            lastKey.value = key

            map.overlays.clear()
            val allGeo = mutableListOf<GeoPoint>()
            val geoSegments = segments.map { seg ->
                seg.map { GeoPoint(it.lat, it.lon) }.also { allGeo += it }
            }

            // Cumulative screen-agnostic (metres) distance per point across the whole
            // run, segment gaps (GPS pauses) add no distance, so the progression
            // color reflects how far into the run each chunk sits.
            var running = 0.0
            val cum = geoSegments.map { geo ->
                val arr = DoubleArray(geo.size)
                for (k in geo.indices) {
                    if (k > 0) running += geo[k - 1].distanceToAsDouble(geo[k])
                    arr[k] = running
                }
                arr
            }
            val total = running.coerceAtLeast(1.0)

            // Adaptive chunk size: keep the total colored-polyline count under
            // MaxTrackChunks for long tracks, but never chunk finer than a few points.
            val totalPoints = geoSegments.sumOf { it.size }
            val chunkSize = ceil(totalPoints.toDouble() / MaxTrackChunks).toInt().coerceAtLeast(2)

            // Casings first so every colored track chunk draws above every casing.
            // Per-segment: no casing bridges a segment boundary (pauses stay gaps).
            geoSegments.forEach { geo ->
                map.overlays.add(Polyline().apply {
                    setPoints(geo)
                    outlinePaint.color = casingArgb
                    outlinePaint.strokeWidth = CasingWidthDp * density
                    outlinePaint.strokeCap = Paint.Cap.ROUND
                    outlinePaint.strokeJoin = Paint.Join.ROUND
                })
            }

            // Colored progression track: one polyline per chunk of consecutive
            // points, colored by the chunk midpoint's cumulative-distance fraction.
            // Chunks share their boundary point so the line stays continuous within
            // a segment; chunks never span a segment boundary.
            geoSegments.forEachIndexed { s, geo ->
                val n = geo.size
                var i = 0
                while (i < n - 1) {
                    val end = (i + chunkSize).coerceAtMost(n - 1)
                    val chunk = geo.subList(i, end + 1)
                    val midCum = cum[s][(i + end) / 2]
                    val frac = (midCum / total).toFloat().coerceIn(0f, 1f)
                    val color = ColorUtils.blendARGB(startTone, accentArgb, frac)
                    map.overlays.add(Polyline().apply {
                        setPoints(chunk)
                        outlinePaint.color = color
                        outlinePaint.strokeWidth = TrackWidthDp * density
                        outlinePaint.strokeCap = Paint.Cap.ROUND
                        outlinePaint.strokeJoin = Paint.Join.ROUND
                    })
                    i = end
                }
            }

            // Start / finish markers on top of everything.
            val start = geoSegments.first().first()
            val finish = geoSegments.last().last()
            if (start.distanceToAsDouble(finish) <= LoopCoincidenceM) {
                // Loop run: a single combined marker (accent start ring + finish dot)
                // so both remain visible where they'd otherwise overlap.
                map.overlays.add(marker(map, start, makeLoopMarkerDrawable(res, accentArgb, casingArgb, finishArgb, density)))
            } else {
                map.overlays.add(marker(map, start, makeMarkerDrawable(res, accentArgb, finishArgb, density)))
                map.overlays.add(marker(map, finish, makeMarkerDrawable(res, finishArgb, accentArgb, density)))
            }

            // NOT BoundingBox.fromGeoPointsSafe: its naive min/max longitude frames
            // the whole globe for a track crossing ±180° (e.g. Fiji, Chukotka), since
            // −179 and +179 look 358° apart. frameRoute normalizes across the dateline
            // so the box stays tight (osmdroid reads west>east as a crossing box).
            val bbox = frameRoute(allGeo.map { it.latitude }, allGeo.map { it.longitude })
            val frame = { map.zoomToBoundingBox(bbox, false, (12 * density).toInt()) }
            if (map.width > 0) frame() else map.addOnFirstLayoutListener { _, _, _, _, _ -> frame() }
            map.invalidate()
        },
    )
}

/** Centered, non-interactive [Marker] at [at] carrying [icon]. */
private fun marker(map: MapView, at: GeoPoint, icon: Drawable): Marker =
    Marker(map).apply {
        position = at
        setAnchor(Marker.ANCHOR_CENTER, Marker.ANCHOR_CENTER)
        this.icon = icon
        infoWindow = null
        setOnMarkerClickListener { _, _ -> true } // no popup; keep it a static pin
    }

/**
 * A filled circle with a contrasting ring, built programmatically (owner rule:
 * no raw px, sized from dp tokens × [density]). Used for the plain start and
 * finish pins ([fillArgb] / [ringArgb] swapped between the two so they read as
 * inverses of each other).
 */
private fun makeMarkerDrawable(res: Resources, fillArgb: Int, ringArgb: Int, density: Float): Drawable {
    val size = (MarkerDiameterDp * density).toInt().coerceAtLeast(1)
    val ring = MarkerRingDp * density
    val bmp = Bitmap.createBitmap(size, size, Bitmap.Config.ARGB_8888)
    val c = Canvas(bmp)
    val cx = size / 2f
    val r = size / 2f - ring / 2f
    val fill = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.FILL; color = fillArgb }
    val stroke = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE; strokeWidth = ring; color = ringArgb
    }
    c.drawCircle(cx, cx, r, fill)
    c.drawCircle(cx, cx, r, stroke)
    return BitmapDrawable(res, bmp)
}

/**
 * Combined marker for a loop run whose start and finish coincide: an accent
 * start ring with a contrasting [ringArgb] outline, plus a centered finish dot
 * ([dotArgb]) so both endpoints stay visible in one pin.
 */
private fun makeLoopMarkerDrawable(
    res: Resources, accentArgb: Int, ringArgb: Int, dotArgb: Int, density: Float,
): Drawable {
    val size = (LoopMarkerDiameterDp * density).toInt().coerceAtLeast(1)
    val ring = MarkerRingDp * density
    val bmp = Bitmap.createBitmap(size, size, Bitmap.Config.ARGB_8888)
    val c = Canvas(bmp)
    val cx = size / 2f
    val rOuter = size / 2f - ring / 2f
    val fill = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.FILL; color = accentArgb }
    val stroke = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE; strokeWidth = ring; color = ringArgb
    }
    val dot = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.FILL; color = dotArgb }
    c.drawCircle(cx, cx, rOuter, fill)   // accent start ring body
    c.drawCircle(cx, cx, rOuter, stroke) // contrasting outline
    c.drawCircle(cx, cx, rOuter * 0.42f, dot) // finish dot
    return BitmapDrawable(res, bmp)
}

/** West/east longitude pair for a framed route. `west > east` is legal and means
 *  the box crosses the antimeridian, osmdroid's zoom/center math adds 360 to a
 *  negative span, so it frames such a box tightly (verified against 6.1.20). */
internal data class LonBounds(val west: Double, val east: Double)

/**
 * Antimeridian-safe longitude framing (pure, so it is unit-testable without a
 * MapView). Naive min/max is wrong for a track straddling ±180°: −179° and +179°
 * are one degree apart on the ground but look 358° apart, so a plain bounding box
 * stretches the "long way" around the globe and zooms out to the whole world.
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
    // Shift the western (negative) hemisphere up by 360 so a dateline-straddling
    // track becomes contiguous, then re-measure.
    val shifted = lons.map { if (it < 0.0) it + 360.0 else it }
    val minShift = shifted.min()
    val maxShift = shifted.max()
    // Shifting didn't tighten it → the run really is that wide; keep the honest box.
    if (maxShift - minShift >= rawSpan) return LonBounds(minRaw, maxRaw)
    // Wrap each bound back into range. west = smaller normalized value, east =
    // larger; the larger one is the one that can exceed 180 and wraps to negative,
    // yielding the west>east pair osmdroid reads as a crossing box.
    fun wrap(v: Double) = if (v > 180.0) v - 360.0 else v
    return LonBounds(west = wrap(minShift), east = wrap(maxShift))
}

/**
 * Build the auto-frame [BoundingBox] for a route. Latitude has no wrap hazard for
 * a ground track (a run never spans a pole), so plain min/max; longitude goes
 * through [frameLongitudes] for the antimeridian case. Constructor order is
 * `(north, east, south, west)` (osmdroid 6.1.20).
 */
internal fun frameRoute(lats: List<Double>, lons: List<Double>): BoundingBox {
    val lon = frameLongitudes(lons)
    return BoundingBox(lats.max(), lon.east, lats.min(), lon.west)
}
