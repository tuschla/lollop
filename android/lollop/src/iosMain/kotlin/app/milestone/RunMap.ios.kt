package app.milestone

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.UIKitInteropProperties
import androidx.compose.ui.viewinterop.UIKitView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import app.milestone.resources.Res
import app.milestone.resources.track_map_attribution
import kotlin.math.pow
import platform.UIKit.accessibilityElementsHidden
import org.jetbrains.compose.resources.stringResource

/** The deepest view of the MapKit route map, in MapKit zoom levels (256 pt tiles):
 *  past it MapKit may refuse the framing and its tiles would part from the route. */
private const val MaxMapKitZoom = 20.0

/** Opacity of the tile attribution's ink over the map (as the tracker's). */
private const val AttributionAlpha = 0.7f

/** Edge to edge: CMP's dialog drawn over the safe areas; the chrome pads itself. */
@Composable
internal actual fun FullScreenMapDialog(onDismissRequest: () -> Unit, content: @Composable () -> Unit) {
    Dialog(
        onDismissRequest = onDismissRequest,
        properties = DialogProperties(usePlatformDefaultWidth = false, usePlatformInsets = false),
        content = content,
    )
}

@Composable
internal actual fun RouteLayer(
    track: RouteTrack,
    options: RouteDrawOptions,
    tiles: Boolean,
    interactive: Boolean,
    insetPx: Float,
    modifier: Modifier,
    corner: Dp,
    topPx: Int,
    bottomPx: Int,
    fitRequests: Int,
) {
    val painter = rememberRoutePainter()
    if (tiles) {
        MapKitRouteLayer(track, painter, options, interactive, insetPx, corner, topPx, bottomPx, fitRequests, modifier)
    } else {
        PlainRouteLayer(track, painter, options, interactive, insetPx, topPx, bottomPx, fitRequests, modifier)
    }
}

/**
 * The route over OSM tiles in a passive MKMapView. The app owns the camera, as on
 * the tile-less canvas: [MercatorFit] frames the route (and the user's pan and pinch
 * when [interactive]) and the map is set to the same world rectangle on every frame
 * the route draws, so the tiles and the Compose-drawn route never part. MapKit's
 * view takes no touches; a static map lets a drag scroll the sheet around it.
 */
@Composable
private fun MapKitRouteLayer(
    track: RouteTrack,
    painter: RoutePainter,
    options: RouteDrawOptions,
    interactive: Boolean,
    insetPx: Float,
    corner: Dp,
    topPx: Int,
    bottomPx: Int,
    fitRequests: Int,
    modifier: Modifier,
) {
    val palette = LocalPalette.current
    val density = LocalDensity.current
    val camera = rememberRouteCamera(track, fitRequests)
    val fit = remember { MercatorFit() }
    val bounds = remember { RouteRect() }
    val map = remember { RouteMapKit() }
    // Past MaxMapKitZoom (in points, so density-scaled px) the pinch stops.
    val maxWorldPx = 256.0 * 2.0.pow(MaxMapKitZoom) * density.density
    Box(modifier) {
        UIKitView(
            factory = { map.view },
            modifier = Modifier.matchParentSize(),
            update = { view ->
                map.tiles.apply(palette)
                view.layer.cornerRadius = corner.value.toDouble()
                view.layer.masksToBounds = corner.value > 0f
            },
            onRelease = { map.release() },
            properties = UIKitInteropProperties(interactionMode = null, isNativeAccessibilityEnabled = false),
        )
        Canvas(
            Modifier
                .matchParentSize()
                .routeGestures(camera, interactive, track, topPx, bottomPx) {
                    (maxWorldPx / fit.fittedScale).toFloat().coerceAtMost(MaxUserScale)
                },
        ) {
            val pan = camera.pan
            fit.fit(track, size.width, size.height, insetPx, topPx.toFloat(), bottomPx.toFloat(), camera.scale, pan.x, pan.y)
            map.frame(fit, size.width, size.height)
            bounds.set(0f, topPx.toFloat(), size.width, size.height - bottomPx)
            painter.draw(this, track, fit, bounds, options)
        }
        // The OSM attribution the tile policy asks for, in the free area's top corner
        // (MapKit keeps its own legal link at the bottom).
        Text(
            stringResource(Res.string.track_map_attribution),
            color = (if (palette.isDark) Color.White else Color.Black).copy(alpha = AttributionAlpha),
            style = Type.Caption,
            modifier = Modifier
                .align(Alignment.TopEnd)
                .padding(top = with(density) { topPx.toDp() } + Space.Sm.dp, end = Space.Md.dp),
        )
    }
}

private class RouteMapKit {
    val view = LollopMapView().apply {
        userInteractionEnabled = false
        accessibilityElementsHidden = true
    }
    private val delegate = LollopMapDelegate()
    val tiles = OsmTileLayer(view)
    private val world = DoubleArray(4) { Double.NaN }

    init {
        view.delegate = delegate
        // The view learns its size after the first frame set it: show that framing now.
        view.onLayout = { if (!world[0].isNaN()) view.showWorld(world[0], world[1], world[2], world[3]) }
    }

    fun frame(fit: MercatorFit, width: Float, height: Float) {
        val left = fit.worldXAt(0f)
        val top = fit.worldYAt(0f)
        val right = fit.worldXAt(width)
        val bottom = fit.worldYAt(height)
        if (left == world[0] && top == world[1] && right == world[2] && bottom == world[3]) return
        world[0] = left
        world[1] = top
        world[2] = right
        world[3] = bottom
        if (view.hasSize) view.showWorld(left, top, right, bottom)
    }

    fun release() {
        tiles.release()
        view.delegate = null
        view.onLayout = null
    }
}
