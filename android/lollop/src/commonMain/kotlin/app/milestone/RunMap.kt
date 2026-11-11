package app.milestone

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
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
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
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
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.traversalIndex
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.milestone.resources.Res
import app.milestone.resources.ic_ui_close
import app.milestone.resources.ic_ui_expand
import app.milestone.resources.ic_ui_fit_bounds
import app.milestone.resources.runmap_cd
import app.milestone.resources.runmap_cd_loop
import app.milestone.resources.runmap_close
import app.milestone.resources.runmap_fit_route
import app.milestone.resources.runmap_open_full
import app.milestone.resources.runmap_open_full_action
import org.jetbrains.compose.resources.DrawableResource
import org.jetbrains.compose.resources.painterResource
import org.jetbrains.compose.resources.stringResource

/** Route inset from the card edge inside the run detail card and trim preview. */
private const val CardFrameInsetDp = 24f
/** Route inset from the free rectangle (between the chrome) on the full-screen map. */
private const val FullScreenFrameInsetDp = 48f
/** Pinch range on the app-drawn full-screen map, relative to the fitted route. */
internal const val MinUserScale = 0.5f
internal const val MaxUserScale = 32f
/** Height of the full-screen map's top chrome row (below the status bar). */
private val MapTopRowHeight = 64.dp
/** The sheet map's "open in full" control: visual square and glyph. */
private val ExpandVisual = 40.dp
private val ExpandGlyph = 20.dp
/** The full-screen map's control glyphs (close, Fit route). */
private val FitGlyph = 24.dp

/** Opacity of a trimmed-off leg's dashed `OnBgFaint` stroke (spec §4). */
internal const val TrimCutAlpha = 0.6f

/** Metres in one display distance unit. */
internal val DistanceUnit.metres: Double get() = if (this == DistanceUnit.Mi) CoreConstants.MILE_M else 1000.0

/** The header and stats the full-screen route map shows over the map. */
internal data class RouteFullScreen(val title: String, val subtitle: String, val stats: List<RunStat>)

/** "Route map, 3.18 miles, loop.": the computed description of a route map (§2 Accessibility). */
@Composable
private fun routeDescription(track: RouteTrack, recordKm: Double, unit: DistanceUnit): String {
    val km = if (recordKm > 0.0) recordKm else track.totalM / 1000.0
    val distance = spokenDistance(currentResources(), distanceFigure(km, unit), unit)
    val first = track.segs.first().first()
    val last = track.segs.last().last()
    val loop = haversineM(first.lat, first.lon, last.lat, last.lon) <= LoopCoincidenceM
    return stringResource(if (loop) Res.string.runmap_cd_loop else Res.string.runmap_cd, distance)
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
    segments: List<List<GpsPoint>>,
    entryId: Long,
    height: Dp = RouteMapHeight,
    distanceMarkers: Boolean = true,
    fullScreen: RouteFullScreen? = null,
    recordKm: Double = 0.0,
) {
    val track = remember(segments) { RouteTrack(segments) }
    if (track.isEmpty) return
    val unit = rememberDistanceUnit()
    val options = remember(track, unit, distanceMarkers, recordKm) {
        RouteDrawOptions(
            distanceMarkers = distanceMarkers,
            unitM = unit.metres,
            distanceScale = if (recordKm > 0.0 && track.totalM > 0.0) recordKm * 1000.0 / track.totalM else 1.0,
        )
    }
    val description = routeDescription(track, recordKm, unit)
    val openAction = stringResource(Res.string.runmap_open_full_action)
    var open by rememberSaveable(entryId) { mutableStateOf(false) }
    val appear = remember(track) { Animatable(0f) }
    LaunchedEffect(track) { appear.animateTo(1f, tween(MotionMedium)) }
    RouteCard(track, options, height, Modifier.fillMaxSize().graphicsLayer { alpha = appear.value }) {
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
        FullScreenRouteMap(track, options, description, fullScreen) { open = false }
    }
}

/** The §2.3 card both sheet route maps draw [track] in: [height] tall, over map tiles
 *  or (maps off) on the plain `BgElevated` card with a hairline, framed with a 24dp
 *  inset and static. [layer] styles the route layer; [overlay] sits over it. */
@Composable
private fun RouteCard(
    track: RouteTrack,
    options: RouteDrawOptions,
    height: Dp,
    layer: Modifier,
    overlay: @Composable BoxScope.() -> Unit,
) {
    val tiles by AppSettings.showRouteMaps.collectAsStateWithLifecycle()
    val shape = RoundedCornerShape(Space.Card.dp)
    val inset = with(LocalDensity.current) { CardFrameInsetDp.dp.toPx() }
    Box(
        Modifier
            .fillMaxWidth()
            .height(height)
            .clip(shape)
            .background(BgElevated)
            .then(if (tiles) Modifier else Modifier.border(1.dp, OnBgBody.copy(alpha = HairlineAlpha.Card), shape)),
    ) {
        RouteLayer(track, options, tiles, interactive = false, insetPx = inset, corner = Space.Card.dp, modifier = layer)
        overlay()
    }
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
                painterResource(Res.drawable.ic_ui_expand),
                contentDescription = stringResource(Res.string.runmap_open_full),
                tint = OnBgBody,
                modifier = Modifier.size(ExpandGlyph),
            )
        }
    }
}

/**
 * A full-screen dialog drawn edge to edge behind the system bars, whose bars take
 * the palette's light or dark icons: the full-screen route map's window.
 */
@Composable
internal expect fun FullScreenMapDialog(onDismissRequest: () -> Unit, content: @Composable () -> Unit)

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
    options: RouteDrawOptions,
    description: String,
    info: RouteFullScreen,
    onClose: () -> Unit,
) {
    val tiles by AppSettings.showRouteMaps.collectAsStateWithLifecycle()
    var fitRequests by remember { mutableIntStateOf(0) }
    var topPx by remember { mutableIntStateOf(0) }
    var bottomPx by remember { mutableIntStateOf(0) }
    val inset = with(LocalDensity.current) { FullScreenFrameInsetDp.dp.toPx() }
    FullScreenMapDialog(onDismissRequest = onClose) {
        Box(
            Modifier
                .fillMaxSize()
                .background(if (tiles) BgTop else BgSunken)
                .traversalGroup(),
        ) {
            if (topPx > 0 && bottomPx > 0) {
                RouteLayer(
                    track, options, tiles, interactive = true, insetPx = inset,
                    modifier = Modifier.fillMaxSize().semantics { contentDescription = description },
                    topPx = topPx, bottomPx = bottomPx, fitRequests = fitRequests,
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
                            .clip(PillShape)
                            .clickable(role = Role.Button, onClick = onClose)
                            .semantics { traversalIndex = -1f },
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            painterResource(Res.drawable.ic_ui_close),
                            contentDescription = stringResource(Res.string.runmap_close),
                            tint = OnBgBody,
                            modifier = Modifier.size(FitGlyph),
                        )
                    }
                    Spacer(Modifier.width(Space.Md.dp))
                    Column(Modifier.weight(1f)) {
                        Text(
                            info.title,
                            color = OnBgBody,
                            style = Type.BodyBold,
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
                    MapOverlayButton(
                        Res.drawable.ic_ui_fit_bounds,
                        stringResource(Res.string.runmap_fit_route),
                        size = LargeControlHeight,
                        glyph = FitGlyph,
                        alpha = MapChromeAlpha,
                    ) { fitRequests++ }
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

/**
 * The route over OSM map tiles ([tiles]) or on a plain canvas, drawn by the
 * platform's route renderer in the theme's colours. Framed into the area between
 * [topPx] and [bottomPx] with [insetPx] all round on first layout, on every resize
 * and on each bump of [fitRequests]; [interactive] maps pan and pinch, a static one
 * ignores touches. [corner] is the radius of the card the layer fills (where the
 * platform's map view isn't clipped by Compose).
 */
@Composable
internal expect fun RouteLayer(
    track: RouteTrack,
    options: RouteDrawOptions,
    tiles: Boolean,
    interactive: Boolean,
    insetPx: Float,
    modifier: Modifier = Modifier,
    corner: Dp = 0.dp,
    topPx: Int = 0,
    bottomPx: Int = 0,
    fitRequests: Int = 0,
)

/** The user's pan and pinch over an app-framed route: [scale] relative to the
 *  fitted zoom, [pan] in px from the fitted centre. */
@Stable
internal class RouteCamera {
    var scale by mutableFloatStateOf(1f)
    var pan by mutableStateOf(Offset.Zero)
}

/** A [RouteCamera] for [track], back at the fitted framing on each bump of [fitRequests]. */
@Composable
internal fun rememberRouteCamera(track: RouteTrack, fitRequests: Int): RouteCamera {
    val camera = remember(track) { RouteCamera() }
    LaunchedEffect(fitRequests) {
        camera.scale = 1f
        camera.pan = Offset.Zero
    }
    return camera
}

/**
 * Pan and pinch of [camera] when [enabled]: zooms about the gesture centroid within
 * [MinUserScale]..[maxScale] (read per gesture step), the fitted centre being the
 * middle of the area between [topPx] and [bottomPx].
 */
internal fun Modifier.routeGestures(
    camera: RouteCamera,
    enabled: Boolean,
    track: RouteTrack,
    topPx: Int,
    bottomPx: Int,
    maxScale: () -> Float = { MaxUserScale },
): Modifier = if (!enabled) this else pointerInput(track, topPx, bottomPx) {
    detectTransformGestures { centroid, panChange, zoom, _ ->
        val scale = camera.scale
        val next = (scale * zoom).coerceIn(MinUserScale, maxScale().coerceAtLeast(MinUserScale))
        val z = next / scale
        // Zoom about the gesture centroid: the fitted centre plus pan scales with it.
        val centre = Offset(size.width / 2f, topPx + (size.height - topPx - bottomPx) / 2f)
        camera.pan = (centre + camera.pan - centroid) * z + centroid - centre + panChange
        camera.scale = next
    }
}

/** The route on a plain canvas (maps off): Web-Mercator fitted, with pan and pinch when [interactive]. */
@Composable
internal fun PlainRouteLayer(
    track: RouteTrack,
    painter: RoutePainter,
    options: RouteDrawOptions,
    interactive: Boolean,
    insetPx: Float,
    topPx: Int,
    bottomPx: Int,
    fitRequests: Int,
    modifier: Modifier,
) {
    val camera = rememberRouteCamera(track, fitRequests)
    val fit = remember { MercatorFit() }
    val bounds = remember { RouteRect() }
    Canvas(modifier.routeGestures(camera, interactive, track, topPx, bottomPx)) {
        val pan = camera.pan
        fit.fit(track, size.width, size.height, insetPx, topPx.toFloat(), bottomPx.toFloat(), camera.scale, pan.x, pan.y)
        bounds.set(0f, topPx.toFloat(), size.width, size.height - bottomPx)
        painter.draw(this, track, fit, bounds, options)
    }
}

/**
 * Trim-preview route map (the trim sheet, spec §4): the §2.3 card with the kept
 * window `[keepFromSec, keepUntilSec]` in the progression ramp, the trimmed head and
 * tail as a dashed `OnBgFaint` stroke without casing, and the start and finish
 * markers at the kept window's ends, moving live with the slider. No distance
 * markers. Framed to the FULL track, so dragging never re-zooms; static, so a drag
 * over it scrolls the sheet. Described with [recordKm], the run's of-record
 * distance, as [RouteMapCard] is.
 */
@Composable
fun TrimRouteMap(
    segments: List<List<GpsPoint>>,
    keepFromSec: Long,
    keepUntilSec: Long,
    recordKm: Double,
) {
    val track = remember(segments) { RouteTrack(segments) }
    if (track.isEmpty) return
    val options = remember(keepFromSec, keepUntilSec) {
        RouteDrawOptions(distanceMarkers = false, trim = true, keepFromSec = keepFromSec, keepUntilSec = keepUntilSec)
    }
    val description = routeDescription(track, recordKm, rememberDistanceUnit())
    RouteCard(track, options, RouteMapHeight, Modifier.fillMaxSize()) {
        // Touches stop here, so the map never pans under the slider's thumb and a
        // vertical drag still scrolls the sheet.
        Box(Modifier.matchParentSize().pointerInput(Unit) {}.semantics { contentDescription = description })
    }
}

/** A round `BgElevated` button with a hairline floating over a map: the full-screen
 *  route's 56dp "Fit route" (spec §3.3, ground at [MapChromeAlpha]) and the tracker's
 *  48dp Recenter (opaque ground). */
@Composable
internal fun MapOverlayButton(
    icon: DrawableResource,
    description: String,
    size: Dp,
    glyph: Dp,
    alpha: Float,
    modifier: Modifier = Modifier,
    onClick: () -> Unit,
) {
    Box(
        modifier = modifier
            .size(size)
            .clip(PillShape)
            .background(BgElevated.copy(alpha = alpha))
            .border(1.dp, OnBgBody.copy(alpha = HairlineAlpha.Control), PillShape)
            .clickable(role = Role.Button, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Icon(painterResource(icon), contentDescription = description, tint = OnBgBody, modifier = Modifier.size(glyph))
    }
}

/** Height of an embedded route map (run detail, trim sheet). */
val RouteMapHeight = 220.dp

/** Top-corner radius of the tracker's floating stat sheet. */
val SheetCornerRadius = 26.dp

/** Opacity of the `BgTop` ground behind the full-screen map's top bar and stat sheet. */
const val MapChromeAlpha = 0.94f

/** Medium motion, in ms: a content fade-in (the route map). */
const val MotionMedium = 200

