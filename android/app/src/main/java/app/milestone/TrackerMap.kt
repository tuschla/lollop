package app.milestone

import android.annotation.SuppressLint
import android.content.Context
import android.graphics.drawable.LayerDrawable
import android.graphics.drawable.ShapeDrawable
import android.graphics.drawable.shapes.OvalShape
import android.view.MotionEvent
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import kotlinx.coroutines.flow.combine
import org.osmdroid.util.GeoPoint
import org.osmdroid.views.MapView
import org.osmdroid.views.overlay.Marker

/** Street-level zoom the live map opens at. */
private const val TRACK_ZOOM = 17.0

// Route stroke (06 spec): `Accent` 10dp over an 18dp ground casing, so the route
// stays legible on any tile. osmdroid Paint widths are raw px, so every dp size here
// is density-scaled.
private const val CASING_WIDTH_DP = 18f
private const val TRACK_WIDTH_DP = 10f

// Current-location blip (spec: `Accent` fill, 3dp `BgTop` casing, r9): outer disc,
// inner disc, and the inset between them. The start ring is the shared
// [RouteMarker] start marker on its halo, so it matches every other route map.
private const val DOT_OUTER_DP = 24f
private const val DOT_INNER_DP = 18f
private const val MARKER_RING_DP = 3f

/** Opacity of the tile attribution's ink over the map. */
private const val ATTRIBUTION_ALPHA = 0.7f

/** Test tag of the tracker's map frame (the area only the status pill and Recenter
 *  may float over). */
internal const val TRACKER_MAP_FRAME_TAG = "tracker-map-frame"

/**
 * The live tracker's osmdroid map and its overlays: the two-layer route stroke, the
 * current-location blip pinned to the LATEST fix and the start ring at the first.
 * Both markers are added lazily on the first fix, so no dot ever renders at the
 * (0,0) default position. Retained across recompositions by [rememberTrackerMap].
 */
internal class TrackerMap(ctx: Context, palette: Palette) {
    val view: MapView = newOsmMapView(ctx).apply { controller.setZoom(TRACK_ZOOM) }
    private val density = ctx.resources.displayMetrics.density
    private val casing = routeLine(palette.bgTop.toArgb(), CASING_WIDTH_DP * density)
    private val track = routeLine(palette.accent.toArgb(), TRACK_WIDTH_DP * density)
    private val locationDot = centeredMarker(locationDotIcon(palette))
    private val startRing = centeredMarker(startRingIcon(palette))

    private fun circle(color: Color, sizeDp: Float): ShapeDrawable = ShapeDrawable(OvalShape()).apply {
        paint.color = color.toArgb()
        intrinsicWidth = (sizeDp * density).toInt()
        intrinsicHeight = (sizeDp * density).toInt()
    }

    /** An [outer] disc with an [inner] disc inset [MARKER_RING_DP] inside it. */
    private fun ringIcon(outer: Color, outerDp: Float, inner: Color, innerDp: Float): LayerDrawable {
        val inset = (MARKER_RING_DP * density).toInt()
        return LayerDrawable(arrayOf(circle(outer, outerDp), circle(inner, innerDp)))
            .apply { setLayerInset(1, inset, inset, inset, inset) }
    }

    private fun locationDotIcon(palette: Palette) = ringIcon(palette.bgTop, DOT_OUTER_DP, palette.accent, DOT_INNER_DP)

    /** [RouteMarker] start: the halo in the route's casing colour, the `Accent` ring
     *  disc, then the `BgElevated` centre, each layer inset inside the one before. */
    private fun startRingIcon(palette: Palette): LayerDrawable {
        val halo = RouteMarker.Halo.value
        val ring = RouteMarker.StartRing.value
        val disc = RouteMarker.StartSize.value
        val centre = disc - 2 * ring
        val outer = disc + 2 * halo
        return LayerDrawable(
            arrayOf(circle(palette.bgTop, outer), circle(palette.accent, disc), circle(palette.bgElevated, centre)),
        ).apply {
            val haloPx = (halo * density).toInt()
            val centrePx = ((halo + ring) * density).toInt()
            setLayerInset(1, haloPx, haloPx, haloPx, haloPx)
            setLayerInset(2, centrePx, centrePx, centrePx, centrePx)
        }
    }

    private fun centeredMarker(icon: LayerDrawable): Marker = Marker(view).apply {
        setAnchor(Marker.ANCHOR_CENTER, Marker.ANCHOR_CENTER)
        this.icon = icon
        setInfoWindow(null)
    }

    private fun show(marker: Marker, at: GeoPoint) {
        marker.position = at
        if (!view.overlays.contains(marker)) view.overlays.add(marker)
    }

    /**
     * Repaint for [palette]: the overlays are built once, so a mid-run light/dark flip
     * recolours the strokes and rebuilds the marker icons instead of leaving stale
     * ones, and re-themes the tiles ([applyMapTheme]).
     */
    fun applyPalette(palette: Palette) {
        casing.outlinePaint.color = palette.bgTop.toArgb()
        track.outlinePaint.color = palette.accent.toArgb()
        locationDot.icon = locationDotIcon(palette)
        startRing.icon = startRingIcon(palette)
        view.applyMapTheme(palette)
        view.invalidate()
    }

    /** Add the route layers and report a user drag (not a programmatic setCenter),
     *  which means the runner wants to look around. */
    @SuppressLint("ClickableViewAccessibility")
    fun attach(onUserDrag: () -> Unit) {
        view.overlays.add(casing)
        view.overlays.add(track)
        view.setOnTouchListener { v, ev ->
            v.parent?.requestDisallowInterceptTouchEvent(true)
            if (ev.actionMasked == MotionEvent.ACTION_MOVE) onUserDrag()
            false
        }
    }

    fun detach() {
        view.setOnTouchListener(null)
        view.onPause()
        view.onDetach()
    }

    fun centerOn(lat: Double, lon: Double) = view.controller.setCenter(GeoPoint(lat, lon))

    /** Animate to the latest recorded fix, else the locate preview's. */
    fun recenter() {
        val at = RunSession.points.value.lastOrNull()?.let { GeoPoint(it.lat, it.lon) }
            ?: RunSession.lastFix.value?.let { GeoPoint(it.lat, it.lon) }
        at?.let { view.controller.animateTo(it) }
    }

    /**
     * Before recording (no recorded points yet) the dot follows the locate preview's
     * fix, so the runner sees where they are before pressing Start. A flow collector,
     * so per-fix updates move only this overlay and never recompose the screen.
     */
    suspend fun followPreviewFix(follow: () -> Boolean) {
        combine(RunSession.lastFix, RunSession.tracking, snapshotFlow { follow() }) { fix, trk, f ->
            Triple(fix, trk, f)
        }.collect { (fix, trk, f) ->
            if (!trk && RunSession.points.value.isEmpty()) {
                fix?.let {
                    val gp = GeoPoint(it.lat, it.lon)
                    show(locationDot, gp)
                    if (f) view.controller.setCenter(gp)
                    view.invalidate()
                }
            }
        }
    }

    /**
     * Extend the route with only the NEW fixes on each emission, so a long run stays
     * O(1) per fix. `drawn` counts the points already on the overlay; a shrink (a new
     * run after a reset) clears and redraws from scratch, and a recovered track is
     * drawn in one O(n) pass. A flow collector, so growth never recomposes the screen.
     */
    suspend fun followTrack(follow: () -> Boolean) {
        var drawn = 0
        combine(RunSession.points, snapshotFlow { follow() }) { pts, f -> pts to f }
            .collect { (pts, f) ->
                if (pts.size < drawn) {
                    casing.setPoints(emptyList())
                    track.setPoints(emptyList())
                    drawn = 0
                }
                for (i in drawn until pts.size) {
                    val gp = GeoPoint(pts[i].lat, pts[i].lon)
                    casing.addPoint(gp)
                    track.addPoint(gp)
                }
                drawn = pts.size
                pts.firstOrNull()?.let { first -> show(startRing, GeoPoint(first.lat, first.lon)) }
                pts.lastOrNull()?.let { last ->
                    val gp = GeoPoint(last.lat, last.lon)
                    show(locationDot, gp)
                    if (f) view.controller.setCenter(gp)
                }
                view.invalidate()
            }
    }
}

/** The tracker's [TrackerMap], built once and repainted whenever the palette changes. */
@Composable
internal fun rememberTrackerMap(): TrackerMap {
    val ctx = LocalContext.current
    val palette = LocalPalette.current
    val map = remember { TrackerMap(ctx, palette) }
    LaunchedEffect(palette) { map.applyPalette(palette) }
    return map
}

/** Pitch of the placeholder grid, raw px: osmdroid's not-yet-loaded tile draws its
 *  grid lines every 16 px of a 256 px tile, unscaled (tiles aren't scaled to dpi). */
private const val MAP_GRID_PITCH_PX = 16f

/**
 * The neutral map ground shown until the tracker knows where the runner is: the
 * same ground and grid as osmdroid's not-yet-loaded tiles ([applyMapTheme]), so the
 * frame doesn't open on the ocean tiles at the map's (0,0) default.
 */
@Composable
private fun MapGridPlaceholder(modifier: Modifier = Modifier) {
    val palette = LocalPalette.current
    val ground = palette.bgTop
    val grid = palette.bgElevated
    Canvas(modifier) {
        drawRect(ground)
        var x = 0f
        while (x < size.width) {
            drawLine(grid, Offset(x, 0f), Offset(x, size.height), strokeWidth = Stroke.HairlineWidth)
            x += MAP_GRID_PITCH_PX
        }
        var y = 0f
        while (y < size.height) {
            drawLine(grid, Offset(0f, y), Offset(size.width, y), strokeWidth = Stroke.HairlineWidth)
            y += MAP_GRID_PITCH_PX
        }
    }
}

/**
 * The map frame above the stat sheet: the live map (when [showMap]) with the one
 * status pill top-left, the OSM attribution bottom-left and Recenter bottom-right;
 * nothing else floats over it. Without a map, only the status pill. Until the runner
 * is [located] (no permission yet, no fix) the map is covered by the neutral grid
 * ground and carries no attribution.
 * [showOverlays] is false while the Finish review has collapsed the frame.
 * clipToBounds is load-bearing: osmdroid paints tiles OUTSIDE its interop slot
 * (Compose's view holder doesn't clip), which would over-paint the safety banner's
 * rows above the map.
 */
@Composable
internal fun TrackerMapFrame(
    map: TrackerMap,
    showMap: Boolean,
    showOverlays: Boolean,
    located: Boolean,
    canRecenter: Boolean,
    onRecenter: () -> Unit,
    modifier: Modifier = Modifier,
    statusPill: @Composable (Modifier) -> Unit,
) {
    val overlayInset = Space.Md.dp + Space.Xs.dp
    Box(
        modifier
            .fillMaxWidth()
            .clipToBounds()
            .testTag(TRACKER_MAP_FRAME_TAG),
    ) {
        if (showMap) {
            // The map view stays attached throughout (osmdroid is never torn down); the
            // placeholder only covers it.
            AndroidView(factory = { map.view }, modifier = Modifier.fillMaxSize().clipToBounds())
            if (!located) MapGridPlaceholder(Modifier.fillMaxSize())
            if (showOverlays) statusPill(Modifier.align(Alignment.TopStart).padding(overlayInset))
        } else if (showOverlays) {
            statusPill(Modifier.padding(overlayInset))
        }
        if (showMap && showOverlays && located) {
            // The tile attribution: a bare legal caption on the tiles, only while tiles
            // actually render; its ink follows the map's own ground (dark under the dark
            // tile filter and placeholder), not the screen chrome.
            Text(
                stringResource(R.string.track_map_attribution),
                color = (if (mapIsDark(LocalPalette.current)) Color.White else Color.Black).copy(alpha = ATTRIBUTION_ALPHA),
                style = Type.Caption,
                modifier = Modifier
                    .align(Alignment.BottomStart)
                    .padding(overlayInset),
            )
            // Recenter: separate from the sheet's control row, only once there is a
            // position to centre on.
            if (canRecenter) {
                MapOverlayButton(
                    R.drawable.ic_track_recenter,
                    stringResource(R.string.track_cd_recenter),
                    modifier = Modifier
                        .align(Alignment.BottomEnd)
                        .padding(Space.Md.dp),
                    onClick = onRecenter,
                )
            }
        }
    }
}
