package app.milestone

import android.content.Context
import android.graphics.Canvas
import android.graphics.ColorMatrix
import android.graphics.ColorMatrixColorFilter
import android.graphics.Paint
import android.graphics.Point
import android.view.View
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.window.DialogWindowProvider
import androidx.core.content.ContextCompat
import androidx.core.view.WindowCompat
import app.milestone.resources.Res
import app.milestone.resources.runmap_finish
import app.milestone.resources.runmap_start
import app.milestone.resources.runmap_start_finish
import kotlin.math.roundToInt
import org.jetbrains.compose.resources.stringResource
import org.osmdroid.tileprovider.tilesource.TileSourceFactory
import org.osmdroid.util.BoundingBox
import org.osmdroid.util.GeoPoint
import org.osmdroid.views.CustomZoomButtonsController
import org.osmdroid.views.MapView
import org.osmdroid.views.Projection
import org.osmdroid.views.overlay.Overlay
import org.osmdroid.views.overlay.Polyline

/**
 * The one osmdroid [MapView] setup every map in the app shares (route detail,
 * full-screen route, trim preview, live tracker): OSM tiles, two-finger pinch zoom,
 * no +/− buttons. Callers add their own content description and gestures.
 */
internal fun newOsmMapView(context: Context): MapView = MapView(context).apply {
    setTileSource(TileSourceFactory.MAPNIK)
    setMultiTouchControls(true)
    zoomController.setVisibility(CustomZoomButtonsController.Visibility.NEVER)
}

/** A round-capped, round-joined route line of [widthPx] in [argb]. */
internal fun routeLine(argb: Int, widthPx: Float): Polyline = Polyline().apply {
    outlinePaint.color = argb
    outlinePaint.strokeWidth = widthPx
    outlinePaint.strokeCap = Paint.Cap.ROUND
    outlinePaint.strokeJoin = Paint.Join.ROUND
}

/**
 * Theme the osmdroid map for [palette]: in a dark theme ([Palette.isDark]) the
 * (always light) OSM tiles are desaturated then inverted so the map reads as a
 * dark ground instead of a bright block; the not-yet-loaded placeholder takes the
 * palette's ground and grid colours
 * in both themes. The tile filter also paints the placeholder, so in a dark theme the
 * placeholder is given the colours the filter maps onto the ground and grid. A light
 * theme clears the filter.
 */
internal fun MapView.applyMapTheme(palette: Palette) {
    val dark = palette.isDark
    val ground = palette.bgTop.toArgb()
    val grid = palette.bgElevated.toArgb()
    val tiles = overlayManager.tilesOverlay
    tiles.setColorFilter(if (dark) DarkTileFilter else null)
    tiles.loadingBackgroundColor = if (dark) preDarkFilter(ground) else ground
    tiles.loadingLineColor = if (dark) preDarkFilter(grid) else grid
}

private val DarkTileFilter: ColorMatrixColorFilter by lazy {
    val desaturate = ColorMatrix().apply { setSaturation(DARK_TILE_SATURATION) }
    val invert = ColorMatrix(
        floatArrayOf(
            -1f, 0f, 0f, 0f, 255f,
            0f, -1f, 0f, 0f, 255f,
            0f, 0f, -1f, 0f, 255f,
            0f, 0f, 0f, 1f, 0f,
        ),
    )
    // setConcat(a, b) applies b first: desaturate, then invert.
    ColorMatrixColorFilter(ColorMatrix().apply { setConcat(invert, desaturate) })
}

/**
 * The colour [DarkTileFilter] turns into [target]: undo the inversion, then the
 * desaturation (a saturation matrix's inverse is the one for the reciprocal factor,
 * since both keep luminance). Clamped to the gamut, so a target more saturated than
 * the filter can produce lands as close as it can.
 */
internal fun preDarkFilter(target: Int): Int {
    val inverse = ColorMatrix().apply { setSaturation(1f / DARK_TILE_SATURATION) }.array
    val r = 255f - android.graphics.Color.red(target)
    val g = 255f - android.graphics.Color.green(target)
    val b = 255f - android.graphics.Color.blue(target)
    fun channel(row: Int): Int =
        (inverse[row * 5] * r + inverse[row * 5 + 1] * g + inverse[row * 5 + 2] * b).roundToInt().coerceIn(0, 255)
    return android.graphics.Color.argb(android.graphics.Color.alpha(target), channel(0), channel(1), channel(2))
}

/** Edge to edge: transparent system bars whose icons follow the palette. */
@Composable
internal actual fun FullScreenMapDialog(onDismissRequest: () -> Unit, content: @Composable () -> Unit) {
    val palette = LocalPalette.current
    Dialog(
        onDismissRequest = onDismissRequest,
        properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false),
    ) {
        val view = LocalView.current
        val window = (view.parent as? DialogWindowProvider)?.window
        val lightBars = !palette.isDark
        DisposableEffect(window, lightBars) {
            if (window != null) {
                @Suppress("DEPRECATION")
                window.statusBarColor = android.graphics.Color.TRANSPARENT
                @Suppress("DEPRECATION")
                window.navigationBarColor = android.graphics.Color.TRANSPARENT
                WindowCompat.getInsetsController(window, window.decorView).apply {
                    isAppearanceLightStatusBars = lightBars
                    isAppearanceLightNavigationBars = lightBars
                }
            }
            onDispose {}
        }
        content()
    }
}

/** The theme's route renderer (colours, label text), resolved once per palette and density. */
@Composable
private fun rememberRouteRenderer(): RouteRenderer {
    val palette = LocalPalette.current
    val ctx = LocalContext.current
    val density = LocalDensity.current
    val textPx = with(density) { Type.Chip.fontSize.toPx() }
    val start = stringResource(Res.string.runmap_start)
    val finish = stringResource(Res.string.runmap_finish)
    val loop = stringResource(Res.string.runmap_start_finish)
    return remember(palette, density.density, textPx, start, finish, loop) {
        val flag = ContextCompat.getDrawable(ctx, R.drawable.ic_route_finish_flag)!!.mutate()
        flag.setTint(palette.bgTop.toArgb())
        RouteRenderer(RouteStyle(
            ramp = IntArray(RouteRampSteps + 1) { palette.routeRamp(it / RouteRampSteps.toFloat()).toArgb() },
            casing = palette.bgTop.toArgb(),
            accent = palette.accent.toArgb(),
            elevated = palette.bgElevated.toArgb(),
            body = palette.onBgBody.toArgb(),
            cut = palette.onBgFaint.copy(alpha = TrimCutAlpha).toArgb(),
            pillFill = palette.bgElevated.copy(alpha = MapChromeAlpha).toArgb(),
            pillBorder = palette.onBgFaint.copy(alpha = HairlineAlpha.Strong).toArgb(),
            density = density.density,
            textPx = textPx,
            flag = flag,
            startLabel = start,
            finishLabel = finish,
            loopLabel = loop,
        ))
    }
}

/** osmdroid tiles under the route overlay, or the plain canvas; Compose clips the view, so [corner] is unused. */
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
    val renderer = rememberRouteRenderer()
    if (tiles) {
        TileRouteLayer(track, renderer, options, interactive, insetPx, topPx, bottomPx, fitRequests, modifier)
    } else {
        PlainRouteLayer(track, renderer, options, interactive, insetPx, topPx, bottomPx, fitRequests, modifier)
    }
}

/** The route as an osmdroid overlay, re-projected (and its markers re-thinned) every frame. */
private class RouteOverlay : Overlay() {
    var track: RouteTrack? = null
    var renderer: RouteRenderer? = null
    var options = RouteDrawOptions()
    var topPx = 0
    var bottomPx = 0
    private var projection: Projection? = null
    private val geo = GeoPoint(0.0, 0.0)
    private val point = Point()
    private val bounds = RouteRect()
    private val projector = RouteProjector { lat, lon, out ->
        geo.setCoords(lat, lon)
        projection?.toPixels(geo, point)
        out[0] = point.x.toFloat()
        out[1] = point.y.toFloat()
    }

    override fun draw(c: Canvas, pj: Projection) {
        val t = track ?: return
        val r = renderer ?: return
        projection = pj
        val screen = pj.intrinsicScreenRect
        bounds.set(screen.left.toFloat(), (screen.top + topPx).toFloat(), screen.right.toFloat(), (screen.bottom - bottomPx).toFloat())
        r.draw(c, t, projector, bounds, options)
    }
}

/** Plain holder for the tile map's last applied state (written from AndroidView `update`). */
private class TileMapState {
    val overlay = RouteOverlay()
    var theme: Palette? = null
    var frameKey: List<Any?>? = null
    var fits = 0
    var insetPx = 0f

    /** Fits the route box into the free area (between the overlay's insets) with [insetPx]. */
    fun fit(map: MapView, animated: Boolean) {
        val t = overlay.track ?: return
        val w = map.width
        val h = map.height
        if (w <= 0 || h <= 0) return
        val top = overlay.topPx
        val bottom = overlay.bottomPx
        val freeW = (w - insetPx * 2).toInt()
        val freeH = (h - top - bottom - insetPx * 2).toInt()
        if (freeW <= 0 || freeH <= 0) return
        val box = BoundingBox(t.north, t.east, t.south, t.west)
        var zoom = MapView.getTileSystem().getBoundingBoxZoom(box, freeW, freeH)
        if (zoom.isNaN() || zoom > MaxFrameZoom) zoom = MaxFrameZoom
        zoom = zoom.coerceAtLeast(map.minZoomLevel)
        map.setMapCenterOffset(0, (top - bottom) / 2)
        val center = box.centerWithDateLine
        if (animated) {
            map.controller.animateTo(center, zoom, null)
        } else {
            map.controller.setZoom(zoom)
            map.controller.setCenter(center)
        }
    }
}

@Composable
private fun TileRouteLayer(
    track: RouteTrack,
    renderer: RouteRenderer,
    options: RouteDrawOptions,
    interactive: Boolean,
    insetPx: Float,
    topPx: Int,
    bottomPx: Int,
    fitRequests: Int,
    modifier: Modifier,
) {
    val palette = LocalPalette.current
    val state = remember { TileMapState() }
    AndroidView(
        modifier = modifier,
        factory = { c ->
            newOsmMapView(c).apply {
                setMultiTouchControls(interactive)
                if (!interactive) isClickable = false
                // Compose describes the map (the computed route description).
                importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
                overlays.add(state.overlay)
                addOnLayoutChangeListener { v, l, t, r, b, ol, ot, or, ob ->
                    if (r - l != or - ol || b - t != ob - ot) v.post { state.fit(this, animated = false) }
                }
            }
        },
        update = { map ->
            if (state.theme != palette) {
                state.theme = palette
                map.applyMapTheme(palette)
            }
            state.overlay.track = track
            state.overlay.renderer = renderer
            state.overlay.options = options
            state.overlay.topPx = topPx
            state.overlay.bottomPx = bottomPx
            state.insetPx = insetPx
            val key = listOf<Any?>(track, insetPx, topPx, bottomPx)
            if (state.frameKey != key) {
                state.frameKey = key
                state.fit(map, animated = false)
            }
            if (state.fits != fitRequests) {
                state.fits = fitRequests
                state.fit(map, animated = true)
            }
            map.invalidate()
        },
        onRelease = { it.onDetach() },
    )
}

/** osmdroid's process-wide configuration: a user agent and a tile cache in the
 *  app's cache dir, set before any MapView inflates. */
internal object OsmdroidTiles : AppFeature {
    override fun start(ctx: Context) {
        val config = org.osmdroid.config.Configuration.getInstance()
        config.load(ctx, ctx.getSharedPreferences("osmdroid", Context.MODE_PRIVATE))
        config.osmdroidBasePath = java.io.File(ctx.cacheDir, "osmdroid")
        config.osmdroidTileCache = java.io.File(ctx.cacheDir, "osmdroid/tiles")
        config.userAgentValue = ctx.packageName
    }
}
