package app.milestone

import android.content.Context
import android.graphics.ColorMatrix
import android.graphics.ColorMatrixColorFilter
import android.graphics.Paint
import android.graphics.Canvas
import android.graphics.Point
import android.graphics.RectF
import android.view.View
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.isTraversalGroup
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.traversalIndex
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.window.DialogWindowProvider
import androidx.core.content.ContextCompat
import androidx.core.view.WindowCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlin.math.roundToInt
import org.osmdroid.tileprovider.tilesource.TileSourceFactory
import org.osmdroid.util.BoundingBox
import org.osmdroid.util.GeoPoint
import org.osmdroid.views.CustomZoomButtonsController
import org.osmdroid.views.MapView
import org.osmdroid.views.Projection
import org.osmdroid.views.overlay.Overlay
import org.osmdroid.views.overlay.Polyline

/** Route inset from the card edge inside the run detail card and trim preview. */
private const val CardFrameInsetDp = 24f
/** Route inset from the free rectangle (between the chrome) on the full-screen map. */
private const val FullScreenFrameInsetDp = 48f
/** Framing never zooms past street level, so a short route keeps its surroundings. */
private const val MaxFrameZoom = 17.0
/** World size in px at [MaxFrameZoom] (256px tiles), the tile-less canvas's zoom cap. */
private const val MaxWorldPx = 256.0 * (1 shl 17)
/** Pinch range on the tile-less full-screen canvas, relative to the fitted route. */
private const val MinUserScale = 0.5f
private const val MaxUserScale = 32f
/** Height of the full-screen map's top chrome row (below the status bar). */
private val MapTopRowHeight = 64.dp
/** The sheet map's "open in full" control: visual square and glyph. */
private val ExpandVisual = 40.dp
private val ExpandGlyph = 20.dp
/** The full-screen "Fit route" control glyph. */
private val FitGlyph = 24.dp
/** Desaturation the dark tile filter applies before inverting. */
private const val DARK_TILE_SATURATION = 0.3f

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

/** A round-capped, round-joined route line of [widthPx] in [argb] over [points]. */
internal fun routeLine(argb: Int, widthPx: Float, points: List<GeoPoint> = emptyList()): Polyline = Polyline().apply {
    if (points.isNotEmpty()) setPoints(points)
    outlinePaint.color = argb
    outlinePaint.strokeWidth = widthPx
    outlinePaint.strokeCap = Paint.Cap.ROUND
    outlinePaint.strokeJoin = Paint.Join.ROUND
}

/** Whether a map drawn in [palette] reads dark: its tiles are inverted and its ground
 *  is the palette's dark ground, so overlays on it (the attribution) take light ink. */
internal fun mapIsDark(palette: Palette): Boolean = palette.bgTop.luminance() < 0.5f

/**
 * Theme the osmdroid map for [palette]: in a dark theme the (always light) OSM tiles
 * are desaturated then inverted so the map reads as a dark ground instead of a bright
 * block; the not-yet-loaded placeholder takes the palette's ground and grid colours
 * in both themes. The tile filter also paints the placeholder, so in a dark theme the
 * placeholder is given the colours the filter maps onto the ground and grid. A light
 * theme clears the filter.
 */
internal fun MapView.applyMapTheme(palette: Palette) {
    val dark = mapIsDark(palette)
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


/** Metres in one display distance unit. */
internal val DistanceUnit.metres: Double get() = if (this == DistanceUnit.Mi) METERS_PER_MILE else 1000.0

/** The header and stats the full-screen route map shows over the map. */
internal data class RouteFullScreen(val title: String, val subtitle: String, val stats: List<RunStat>)

/** The theme's route style (colours, marker sizes, label text), resolved once per palette and density. */
@Composable
private fun rememberRouteStyle(): RouteStyle {
    val palette = LocalPalette.current
    val ctx = LocalContext.current
    val density = LocalDensity.current
    val textPx = with(density) { Type.Chip.fontSize.toPx() }
    val start = stringResource(R.string.runmap_start)
    val finish = stringResource(R.string.runmap_finish)
    val loop = stringResource(R.string.runmap_start_finish)
    return remember(palette, density.density, textPx, start, finish, loop) {
        val flag = ContextCompat.getDrawable(ctx, R.drawable.ic_route_finish_flag)!!.mutate()
        flag.setTint(palette.bgTop.toArgb())
        RouteStyle(
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
            startDp = RouteMarker.StartSize.value,
            startRingDp = RouteMarker.StartRing.value,
            finishDp = RouteMarker.FinishSize.value,
            flagDp = RouteMarker.FinishGlyph.value,
            distanceDp = RouteMarker.DistanceSize.value,
            distanceRingDp = RouteMarker.DistanceRing.value,
            haloDp = RouteMarker.Halo.value,
        )
    }
}

/** Opacity of a trimmed-off leg's dashed `OnBgFaint` stroke (spec §4). */
private const val TrimCutAlpha = 0.6f

/** "Route map, 3.18 miles, loop.": the computed description of a route map (§2 Accessibility). */
@Composable
private fun routeDescription(track: RouteTrack, recordKm: Double, unit: DistanceUnit): String {
    val km = if (recordKm > 0.0) recordKm else track.totalM / 1000.0
    val figure = distanceFigure(km, unit)
    val distance = stringResource(if (unit == DistanceUnit.Mi) R.string.run_unit_miles else R.string.run_unit_km, figure)
    val first = track.segs.first().first()
    val last = track.segs.last().last()
    val loop = haversineM(first.lat, first.lon, last.lat, last.lon) <= LoopCoincidenceM
    return stringResource(if (loop) R.string.runmap_cd_loop else R.string.runmap_cd, distance)
}

/**
 * The route map card (spec §2.3): the run's route over map tiles (or, with maps
 * off, on the plain `BgElevated` card), drawn in the §0.2 progression ramp with a
 * start ring, a finish flag disc, their label pills and a numbered marker at every
 * whole km/mi, framed with a 24dp inset. Static, so a drag over it scrolls the
 * sheet. With [fullScreen] the whole card and its corner control open
 * [FullScreenRouteMap]; without it the card is a picture only (finish review
 * thumbnail). [recordKm] (the run's of-record distance) keeps the numbered markers
 * and the description in step with the distance the sheet states.
 */
@Composable
internal fun RouteMapCard(
    segments: List<List<GpxFix>>,
    entryId: Long,
    modifier: Modifier = Modifier,
    height: Dp = RouteMapHeight,
    distanceMarkers: Boolean = true,
    fullScreen: RouteFullScreen? = null,
    recordKm: Double = 0.0,
) {
    val track = remember(segments) { RouteTrack(segments) }
    if (track.isEmpty) return
    val tiles by AppSettings.showRouteMaps.collectAsStateWithLifecycle()
    val unit = rememberDistanceUnit()
    val style = rememberRouteStyle()
    val renderer = remember(style) { RouteRenderer(style) }
    val options = remember(track, unit, distanceMarkers, recordKm) {
        RouteDrawOptions(
            distanceMarkers = distanceMarkers,
            unitM = unit.metres,
            distanceScale = if (recordKm > 0.0 && track.totalM > 0.0) recordKm * 1000.0 / track.totalM else 1.0,
        )
    }
    val description = routeDescription(track, recordKm, unit)
    val openAction = stringResource(R.string.runmap_open_full_action)
    var open by rememberSaveable(entryId) { mutableStateOf(false) }
    val appear = remember(track) { Animatable(0f) }
    LaunchedEffect(track) { appear.animateTo(1f, tween(MotionMedium)) }
    val shape = RoundedCornerShape(Space.Card.dp)
    val inset = with(LocalDensity.current) { CardFrameInsetDp.dp.toPx() }
    Box(
        modifier
            .fillMaxWidth()
            .height(height)
            .clip(shape)
            .background(BgElevated)
            .then(if (tiles) Modifier else Modifier.border(1.dp, OnBgBody.copy(alpha = HairlineAlpha.Card), shape)),
    ) {
        RouteLayer(
            track, renderer, options, tiles, interactive = false, insetPx = inset,
            modifier = Modifier.fillMaxSize().graphicsLayer { alpha = appear.value },
        )
        if (fullScreen != null) {
            // The tap target sits OVER the map, so the map never sees a touch and a
            // vertical drag falls through to the sheet's scroll.
            Box(
                Modifier
                    .matchParentSize()
                    .clickable(role = Role.Button, onClickLabel = openAction) { open = true }
                    .semantics { contentDescription = description },
            )
            ExpandButton(Modifier.align(Alignment.BottomEnd)) { open = true }
        } else {
            Box(Modifier.matchParentSize().semantics { contentDescription = description })
        }
    }
    if (open && fullScreen != null) {
        FullScreenRouteMap(track, renderer, options, description, fullScreen) { open = false }
    }
}

/** The route card's loading state (spec §2 States): the card at full height, no spinner. */
@Composable
internal fun RouteMapPlaceholder(modifier: Modifier = Modifier, height: Dp = RouteMapHeight) {
    Box(
        modifier
            .fillMaxWidth()
            .height(height)
            .clip(RoundedCornerShape(Space.Card.dp))
            .background(BgElevated),
    )
}

/** The sheet map's "open in full" corner control: a 40dp square in a 48dp target, 8dp in. */
@Composable
private fun ExpandButton(modifier: Modifier, onClick: () -> Unit) {
    Box(
        modifier
            .padding(Space.Sm.dp)
            .size(ActionMinHeight)
            .clickable(role = Role.Button, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Box(
            Modifier
                .size(ExpandVisual)
                .clip(ChoiceCellShape)
                .background(BgElevated.copy(alpha = MapChromeAlpha))
                .border(1.dp, OnBgBody.copy(alpha = HairlineAlpha.Control), ChoiceCellShape),
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                painterResource(R.drawable.ic_ui_expand),
                contentDescription = stringResource(R.string.runmap_open_full),
                tint = OnBgBody,
                modifier = Modifier.size(ExpandGlyph),
            )
        }
    }
}

/**
 * The full-screen route (spec §3), edge to edge behind both system bars: a
 * near-opaque `BgTop` top chrome under the status bar (close, run name, date), the
 * route fitted into the space between the chrome and the bottom stat sheet with a
 * 48dp inset, "Fit route" above the sheet, and the same 3-up stats as the detail
 * sheet. Pans and pinches; Back or ✕ closes.
 */
@Composable
private fun FullScreenRouteMap(
    track: RouteTrack,
    renderer: RouteRenderer,
    options: RouteDrawOptions,
    description: String,
    info: RouteFullScreen,
    onClose: () -> Unit,
) {
    val tiles by AppSettings.showRouteMaps.collectAsStateWithLifecycle()
    var fitRequests by remember { mutableIntStateOf(0) }
    var topPx by remember { mutableIntStateOf(0) }
    var bottomPx by remember { mutableIntStateOf(0) }
    val palette = LocalPalette.current
    val inset = with(LocalDensity.current) { FullScreenFrameInsetDp.dp.toPx() }
    Dialog(
        onDismissRequest = onClose,
        properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false),
    ) {
        val view = LocalView.current
        val window = (view.parent as? DialogWindowProvider)?.window
        val lightBars = !mapIsDark(palette)
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
        Box(
            Modifier
                .fillMaxSize()
                .background(if (tiles) BgTop else BgSunken)
                .semantics { isTraversalGroup = true },
        ) {
            if (topPx > 0 && bottomPx > 0) {
                RouteLayer(
                    track, renderer, options, tiles, interactive = true, insetPx = inset,
                    topPx = topPx, bottomPx = bottomPx, fitRequests = fitRequests,
                    modifier = Modifier.fillMaxSize().semantics { contentDescription = description },
                )
            }
            Column(
                Modifier
                    .fillMaxWidth()
                    .background(BgTop.copy(alpha = MapChromeAlpha))
                    .onSizeChanged { topPx = it.height }
                    .statusBarsPadding(),
            ) {
                Row(
                    Modifier
                        .fillMaxWidth()
                        .height(MapTopRowHeight)
                        .padding(start = Space.Sm.dp, end = Space.Screen.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Box(
                        Modifier
                            .size(ActionMinHeight)
                            .clip(RoundedCornerShape(100))
                            .clickable(role = Role.Button, onClick = onClose)
                            .semantics { traversalIndex = -1f },
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            painterResource(R.drawable.ic_ui_close),
                            contentDescription = stringResource(R.string.runmap_close),
                            tint = OnBgBody,
                            modifier = Modifier.size(FitGlyph),
                        )
                    }
                    Spacer(Modifier.width(Space.Md.dp))
                    Column(Modifier.weight(1f)) {
                        Text(
                            info.title,
                            color = OnBgBody,
                            style = Type.Body.copy(fontWeight = FontWeight.Bold),
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                        if (info.subtitle.isNotBlank()) {
                            Text(
                                info.subtitle,
                                color = OnBgMuted,
                                style = Type.Caption.merge(TabularFigures),
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                    }
                }
            }
            Column(Modifier.align(Alignment.BottomCenter).fillMaxWidth()) {
                Box(
                    Modifier
                        .fillMaxWidth()
                        .padding(Space.Screen.dp),
                    contentAlignment = Alignment.CenterEnd,
                ) {
                    FitRouteButton { fitRequests++ }
                }
                Box(
                    Modifier
                        .fillMaxWidth()
                        .onSizeChanged { bottomPx = it.height }
                        .clip(RoundedCornerShape(topStart = SheetCornerRadius, topEnd = SheetCornerRadius))
                        .background(BgElevated.copy(alpha = MapChromeAlpha))
                        .padding(horizontal = Space.Screen.dp)
                        .padding(top = Space.Card.dp)
                        .navigationBarsPadding()
                        .padding(bottom = Space.Card.dp),
                ) {
                    RunStatColumns(info.stats, compact = true)
                }
            }
        }
    }
}

/** "Fit route": a 56dp `BgElevated` @ [MapChromeAlpha] circle with a hairline (spec §3.3). */
@Composable
private fun FitRouteButton(onClick: () -> Unit) {
    val shape = RoundedCornerShape(100)
    Box(
        Modifier
            .size(LargeControlHeight)
            .clip(shape)
            .background(BgElevated.copy(alpha = MapChromeAlpha))
            .border(1.dp, OnBgBody.copy(alpha = HairlineAlpha.Control), shape)
            .clickable(role = Role.Button, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Icon(
            painterResource(R.drawable.ic_ui_fit_bounds),
            contentDescription = stringResource(R.string.runmap_fit_route),
            tint = OnBgBody,
            modifier = Modifier.size(FitGlyph),
        )
    }
}

/**
 * The route drawn by [RouteRenderer], over osmdroid tiles ([tiles]) or on a plain
 * canvas. Framed into the area between [topPx] and [bottomPx] with [insetPx] all
 * round on first layout, on every resize and on each bump of [fitRequests];
 * [interactive] maps pan and pinch, a static one ignores touches.
 */
@Composable
private fun RouteLayer(
    track: RouteTrack,
    renderer: RouteRenderer,
    options: RouteDrawOptions,
    tiles: Boolean,
    interactive: Boolean,
    insetPx: Float,
    modifier: Modifier = Modifier,
    topPx: Int = 0,
    bottomPx: Int = 0,
    fitRequests: Int = 0,
) {
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
    private val bounds = RectF()
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
    var track: RouteTrack? = null
    var insetPx = 0f

    /** Fits the route box into the free area (between the overlay's insets) with [insetPx]. */
    fun fit(map: MapView, animated: Boolean) {
        val t = track ?: return
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
            state.track = track
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

/** The route on a plain canvas (maps off): Web-Mercator fitted, with pan and pinch when [interactive]. */
@Composable
private fun PlainRouteLayer(
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
    var scale by remember(track) { mutableFloatStateOf(1f) }
    var pan by remember(track) { mutableStateOf(Offset.Zero) }
    LaunchedEffect(fitRequests) {
        scale = 1f
        pan = Offset.Zero
    }
    val fit = remember { MercatorFit() }
    val bounds = remember { RectF() }
    val gestures = if (interactive) {
        Modifier.pointerInput(track, topPx, bottomPx) {
            detectTransformGestures { centroid, panChange, zoom, _ ->
                val next = (scale * zoom).coerceIn(MinUserScale, MaxUserScale)
                val z = next / scale
                // Zoom about the gesture centroid: the fitted centre plus pan scales with it.
                val centre = Offset(size.width / 2f, topPx + (size.height - topPx - bottomPx) / 2f)
                pan = (centre + pan - centroid) * z + centroid - centre + panChange
                scale = next
            }
        }
    } else {
        Modifier
    }
    Canvas(modifier.then(gestures)) {
        fit.fit(track, size.width, size.height, insetPx, topPx.toFloat(), bottomPx.toFloat(), MaxWorldPx, scale, pan.x, pan.y)
        bounds.set(0f, topPx.toFloat(), size.width, size.height - bottomPx)
        drawIntoCanvas { renderer.draw(it.nativeCanvas, track, fit, bounds, options) }
    }
}

/**
 * Trim-preview route map (the trim sheet, spec §4): the §2.3 card with the kept
 * window `[keepFromSec, keepUntilSec]` in the progression ramp, the trimmed head and
 * tail as a dashed `OnBgFaint` stroke without casing, and the start and finish
 * markers at the kept window's ends, moving live with the slider. No distance
 * markers. Framed to the FULL track, so dragging never re-zooms; static, so a drag
 * over it scrolls the sheet.
 */
@Composable
fun TrimRouteMap(
    segments: List<List<GpxFix>>,
    entryId: Long,
    keepFromSec: Long,
    keepUntilSec: Long,
    modifier: Modifier = Modifier,
) {
    val track = remember(segments) { RouteTrack(segments) }
    if (track.isEmpty) return
    val tiles by AppSettings.showRouteMaps.collectAsStateWithLifecycle()
    val style = rememberRouteStyle()
    val renderer = remember(style, entryId) { RouteRenderer(style) }
    val options = remember(keepFromSec, keepUntilSec) {
        RouteDrawOptions(distanceMarkers = false, trim = true, keepFromSec = keepFromSec, keepUntilSec = keepUntilSec)
    }
    val description = routeDescription(track, 0.0, rememberDistanceUnit())
    val shape = RoundedCornerShape(Space.Card.dp)
    val inset = with(LocalDensity.current) { CardFrameInsetDp.dp.toPx() }
    Box(
        modifier
            .fillMaxWidth()
            .height(RouteMapHeight)
            .clip(shape)
            .background(BgElevated)
            .then(if (tiles) Modifier else Modifier.border(1.dp, OnBgBody.copy(alpha = HairlineAlpha.Card), shape)),
    ) {
        RouteLayer(track, renderer, options, tiles, interactive = false, insetPx = inset, modifier = Modifier.fillMaxSize())
        // Touches stop here, so the map never pans under the slider's thumb and a
        // vertical drag still scrolls the sheet.
        Box(Modifier.matchParentSize().pointerInput(Unit) {}.semantics { contentDescription = description })
    }
}

/** A 48dp round button floating over a map (full-screen route, tracker Recenter). */
@Composable
internal fun MapOverlayButton(icon: Int, description: String, modifier: Modifier = Modifier, onClick: () -> Unit) {
    val shape = RoundedCornerShape(100)
    Box(
        modifier = modifier
            .size(ActionMinHeight)
            .clip(shape)
            .background(BgElevated)
            .border(1.dp, OnBgBody.copy(alpha = HairlineAlpha.Control), shape)
            .clickable(role = Role.Button, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Icon(painterResource(icon), contentDescription = description, tint = OnBgBody, modifier = Modifier.size(20.dp))
    }
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

/**
 * One parsed GPX fix reduced to what the trim preview needs: its absolute
 * epoch-second timestamp, the cumulative haversine distance (km) and the cumulative
 * MOVING seconds from the track start up to this point. PREVIEW figures ONLY — the
 * of-record distance/duration are always re-derived in the core after a trim event.
 */
data class TimedTrackPoint(val timeSec: Long, val cumKm: Double, val movingSec: Double)

/**
 * Parsed route [segments] reshaped for the trim preview: every fix flattened into one
 * time-ordered list carrying its epoch-second time, the cumulative distance in km
 * (the same running haversine sum [RouteTrack] colours by) and the cumulative moving
 * time. A segment gap (a GPS pause between `<trkseg>`s) adds neither; within a
 * segment a leg adds its seconds only when it clears [STOPPED_SPEED_MPS], the
 * core's moving-leg rule (`running::leg`), so standing still counts no time either.
 */
fun timedTrackFromSegments(segments: List<List<GpxFix>>): List<TimedTrackPoint> {
    val out = ArrayList<TimedTrackPoint>()
    var runningM = 0.0
    var movingSec = 0.0
    for (seg in segments) {
        var prev: GpxFix? = null
        for (fix in seg) {
            if (prev != null) {
                val legM = haversineM(prev.lat, prev.lon, fix.lat, fix.lon)
                runningM += legM
                val dt = (fix.timeSec - prev.timeSec).toDouble()
                if (dt > 0.0 && legM / dt >= STOPPED_SPEED_MPS) movingSec += dt
            }
            out += TimedTrackPoint(fix.timeSec, runningM / 1000.0, movingSec)
            prev = fix
        }
    }
    return out
}
