package app.milestone

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
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import app.milestone.resources.Res
import app.milestone.resources.ic_track_recenter
import app.milestone.resources.track_cd_recenter
import app.milestone.resources.track_map_attribution
import kotlinx.coroutines.flow.combine
import org.jetbrains.compose.resources.stringResource

/** Street-level zoom the live map opens at (256 px tiles, unscaled to density). */
internal const val TRACK_ZOOM = 17.0

// Route stroke (06 spec): `Accent` 10dp over an 18dp ground casing, so the route
// stays legible on any tile.
internal const val CASING_WIDTH_DP = 18f
internal const val TRACK_WIDTH_DP = 10f

// Current-location blip (spec: `Accent` fill, 3dp `BgTop` casing, r9): outer disc,
// inner disc, and the inset between them. The start ring is the shared
// [RouteMarker] start marker on its halo, so it matches every other route map.
internal const val DOT_OUTER_DP = 24f
internal const val DOT_INNER_DP = 18f
internal const val MARKER_RING_DP = 3f

/** Opacity of the tile attribution's ink over the map. */
private const val ATTRIBUTION_ALPHA = 0.7f

/** Test tag of the tracker's map frame (the area only the status pill and Recenter
 *  may float over). */
internal const val TRACKER_MAP_FRAME_TAG = "tracker-map-frame"

/**
 * The platform map the live tracker draws on (osmdroid on Android, MapKit on iOS):
 * OSM tiles themed for the palette, the two-layer route stroke ([CASING_WIDTH_DP]
 * `BgTop` casing under [TRACK_WIDTH_DP] `Accent`), the current-location blip and
 * the [RouteMarker] start ring under it. Markers appear on their first placement,
 * never at a (0,0) default. Retained across recompositions and layout moves; built
 * by [newLiveMapSurface].
 */
internal interface LiveMapSurface {
    /** Embeds the map view; the same view moves between layouts without being torn down. */
    @Composable
    fun View(modifier: Modifier)

    /** Starts reporting a user drag (not a programmatic move) to [onUserDrag]. */
    fun attach(onUserDrag: () -> Unit)

    /** Releases the map when the screen goes. */
    fun detach()

    /** The app came to the foreground / left it: resume or pause tile loading. */
    fun resume()
    fun pause()

    /** Recolours the strokes, markers and tiles for [palette]. */
    fun applyPalette(palette: Palette)

    fun setCenter(lat: Double, lon: Double)
    fun animateTo(lat: Double, lon: Double)

    /** Places (first call: adds) the location blip / start ring at a fix. */
    fun showLocation(lat: Double, lon: Double)
    fun showStart(lat: Double, lon: Double)

    /** Empties the route stroke. */
    fun clearTrack()

    /** Extends the route stroke with `points[from until points.size]`. */
    fun appendTrack(points: List<GpsPoint>, from: Int)

    /** Redraws after a batch of changes. */
    fun invalidate()
}

/** The platform's live map, themed for [palette]. */
internal expect fun newLiveMapSurface(ctx: PlatformContext, palette: Palette): LiveMapSurface

/**
 * The live tracker's map: the route grows with each recorded fix, the location blip
 * pinned to the LATEST fix and the start ring at the first; before recording the blip
 * follows the locate preview. Retained across recompositions by [rememberTrackerMap].
 */
internal class TrackerMap(private val surface: LiveMapSurface) {
    /** Embeds the map. */
    @Composable
    fun View(modifier: Modifier) = surface.View(modifier)

    fun applyPalette(palette: Palette) = surface.applyPalette(palette)

    /** Add the route layers and report a user drag (not a programmatic setCenter),
     *  which means the runner wants to look around. */
    fun attach(onUserDrag: () -> Unit) = surface.attach(onUserDrag)

    fun detach() = surface.detach()

    /** Resume / pause the map's tile loading with the app (bound to the activity's
     *  lifecycle, so it stops draining battery in the background). */
    fun resume() = surface.resume()
    fun pause() = surface.pause()

    fun centerOn(lat: Double, lon: Double) = surface.setCenter(lat, lon)

    /** Animate to the latest recorded fix, else the locate preview's. */
    fun recenter() {
        val last = RunSession.points.value.lastOrNull()
        if (last != null) {
            surface.animateTo(last.lat, last.lon)
        } else {
            RunSession.lastFix.value?.let { surface.animateTo(it.lat, it.lon) }
        }
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
                    surface.showLocation(it.lat, it.lon)
                    if (f) surface.setCenter(it.lat, it.lon)
                    surface.invalidate()
                }
            }
        }
    }

    /**
     * Extend the route with only the NEW fixes on each emission, so a long run stays
     * O(1) per fix. `drawn` counts the points already on the map; a shrink (a new
     * run after a reset) clears and redraws from scratch, and a recovered track is
     * drawn in one O(n) pass. A flow collector, so growth never recomposes the screen.
     */
    suspend fun followTrack(follow: () -> Boolean) {
        var drawn = 0
        combine(RunSession.points, snapshotFlow { follow() }) { pts, f -> pts to f }
            .collect { (pts, f) ->
                if (pts.size < drawn) {
                    surface.clearTrack()
                    drawn = 0
                }
                if (drawn < pts.size) surface.appendTrack(pts, drawn)
                drawn = pts.size
                pts.firstOrNull()?.let { first -> surface.showStart(first.lat, first.lon) }
                pts.lastOrNull()?.let { last ->
                    surface.showLocation(last.lat, last.lon)
                    if (f) surface.setCenter(last.lat, last.lon)
                }
                surface.invalidate()
            }
    }
}

/** The tracker's [TrackerMap], built once and repainted whenever the palette changes. */
@Composable
internal fun rememberTrackerMap(): TrackerMap {
    val ctx = currentPlatformContext()
    val palette = LocalPalette.current
    val map = remember { TrackerMap(newLiveMapSurface(ctx, palette)) }
    LaunchedEffect(palette) { map.applyPalette(palette) }
    return map
}

/** Pitch of the placeholder grid, raw px: osmdroid's not-yet-loaded tile draws its
 *  grid lines every 16 px of a 256 px tile, unscaled (tiles aren't scaled to dpi). */
private const val MAP_GRID_PITCH_PX = 16f

/**
 * The neutral map ground shown until the tracker knows where the runner is: the
 * same ground and grid as osmdroid's not-yet-loaded tiles, so the frame doesn't
 * open on the ocean tiles at the map's (0,0) default.
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
 * The map frame above the stat sheet: the live map (when [showMap]) with the status
 * pill, OSM attribution and Recenter, covered by the neutral grid until [located].
 * [showOverlays] is false while the Finish review has collapsed the frame.
 * clipToBounds is load-bearing: osmdroid paints tiles OUTSIDE its interop slot,
 * which would over-paint the safety banner's rows above the map.
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
            // The map view stays attached throughout (never torn down); the
            // placeholder only covers it.
            map.View(Modifier.fillMaxSize().clipToBounds())
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
                stringResource(Res.string.track_map_attribution),
                color = (if (LocalPalette.current.isDark) Color.White else Color.Black).copy(alpha = ATTRIBUTION_ALPHA),
                style = Type.Caption,
                modifier = Modifier
                    .align(Alignment.BottomStart)
                    .padding(overlayInset),
            )
            // Recenter: separate from the sheet's control row, only once there is a
            // position to centre on.
            if (canRecenter) {
                MapOverlayButton(
                    Res.drawable.ic_track_recenter,
                    stringResource(Res.string.track_cd_recenter),
                    size = ActionMinHeight,
                    glyph = 20.dp,
                    alpha = 1f,
                    modifier = Modifier
                        .align(Alignment.BottomEnd)
                        .padding(Space.Md.dp),
                    onClick = onRecenter,
                )
            }
        }
    }
}
