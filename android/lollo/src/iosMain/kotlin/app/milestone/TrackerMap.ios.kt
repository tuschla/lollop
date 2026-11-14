package app.milestone

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.viewinterop.UIKitInteropInteractionMode
import androidx.compose.ui.viewinterop.UIKitInteropProperties
import androidx.compose.ui.viewinterop.UIKitView
import kotlinx.cinterop.BetaInteropApi
import kotlinx.cinterop.ExperimentalForeignApi
import kotlinx.cinterop.ObjCAction
import kotlinx.cinterop.ObjCSignatureOverride
import kotlinx.cinterop.allocArray
import kotlinx.cinterop.get
import kotlinx.cinterop.memScoped
import kotlinx.cinterop.useContents
import platform.CoreGraphics.CGLineCap
import platform.CoreGraphics.CGLineJoin
import platform.CoreGraphics.CGRectMake
import platform.CoreGraphics.CGSizeMake
import platform.CoreLocation.CLLocationCoordinate2D
import platform.CoreLocation.CLLocationCoordinate2DMake
import platform.MapKit.MKAnnotationProtocol
import platform.MapKit.MKAnnotationView
import platform.MapKit.MKAnnotationViewZPriorityDefaultUnselected
import platform.MapKit.MKAnnotationViewZPriorityMax
import platform.MapKit.MKOverlayLevelAboveLabels
import platform.MapKit.MKOverlayProtocol
import platform.MapKit.MKOverlayRenderer
import platform.MapKit.MKPointAnnotation
import platform.MapKit.MKPolyline
import platform.MapKit.MKPolylineRenderer
import platform.MapKit.addOverlay
import platform.MapKit.insertOverlay
import platform.MapKit.removeOverlays
import platform.MapKit.rendererForOverlay
import platform.UIKit.UIBezierPath
import platform.UIKit.UIColor
import platform.UIKit.UIEdgeInsetsMake
import platform.UIKit.UIGestureRecognizer
import platform.UIKit.UIGestureRecognizerDelegateProtocol
import platform.UIKit.UIGestureRecognizerStateBegan
import platform.UIKit.UIGestureRecognizerStateChanged
import platform.UIKit.UIGraphicsImageRenderer
import platform.UIKit.UIImage
import platform.UIKit.UIPanGestureRecognizer
import platform.UIKit.UIPinchGestureRecognizer
import platform.UIKit.UIScreen
import platform.UIKit.UIView
import platform.UIKit.UIViewAutoresizingFlexibleHeight
import platform.UIKit.UIViewAutoresizingFlexibleWidth
import platform.darwin.NSObject
import platform.objc.sel_registerName
import kotlin.math.pow

/** Fixes per sealed route polyline: the tail polyline is rebuilt on every fix, so a
 *  long run costs at most this many coordinates per update. */
private const val TRACK_CHUNK = 256

/** Room under MapKit's legal link for the OSM attribution caption the frame draws. */
private const val LEGAL_LINK_BOTTOM_PT = 28.0

internal actual fun newLiveMapSurface(ctx: PlatformContext, palette: Palette): LiveMapSurface = MapKitLiveMap(palette)

private fun uiColor(color: Color): UIColor =
    UIColor.colorWithRed(color.red.toDouble(), color.green.toDouble(), color.blue.toDouble(), color.alpha.toDouble())

/** Concentric discs, outermost first: (colour, diameter in points). */
@OptIn(ExperimentalForeignApi::class)
private fun discs(vararg layers: Pair<Color, Float>): UIImage {
    val size = layers.first().second.toDouble()
    return UIGraphicsImageRenderer(size = CGSizeMake(size, size)).imageWithActions { _ ->
        for ((color, diameter) in layers) {
            val d = diameter.toDouble()
            uiColor(color).setFill()
            UIBezierPath.bezierPathWithOvalInRect(CGRectMake((size - d) / 2, (size - d) / 2, d, d)).fill()
        }
    }
}

/** The location blip: `Accent` inside a [MARKER_RING_DP] `BgTop` casing. */
private fun locationDotImage(palette: Palette) = discs(palette.bgTop to DOT_OUTER_DP, palette.accent to DOT_INNER_DP)

/** [RouteMarker] start: the `BgTop` halo, the `Accent` ring disc, the `BgElevated` centre. */
private fun startRingImage(palette: Palette): UIImage {
    val halo = RouteMarker.Halo.value
    val ring = RouteMarker.StartRing.value
    val disc = RouteMarker.StartSize.value
    return discs(palette.bgTop to disc + 2 * halo, palette.accent to disc, palette.bgElevated to disc - 2 * ring)
}

/** Reports the start of each user pan or pinch on the map (never a programmatic move). */
@OptIn(BetaInteropApi::class)
private class UserGestures(private val onGesture: () -> Unit) : NSObject(), UIGestureRecognizerDelegateProtocol {
    @ObjCAction
    fun handleGesture(recognizer: UIGestureRecognizer) {
        val state = recognizer.state
        if (state == UIGestureRecognizerStateBegan || state == UIGestureRecognizerStateChanged) onGesture()
    }

    // MapKit's own recognizers keep panning and zooming alongside these.
    @ObjCSignatureOverride
    override fun gestureRecognizer(
        gestureRecognizer: UIGestureRecognizer,
        shouldRecognizeSimultaneouslyWithGestureRecognizer: UIGestureRecognizer,
    ): Boolean = true
}

/**
 * The live tracker's MapKit map: OSM tiles ([OsmTileLayer]), the route as MKPolyline
 * pairs (a [CASING_WIDTH_DP] `BgTop` casing under a [TRACK_WIDTH_DP] `Accent` line,
 * round caps and joins) sealed every [TRACK_CHUNK] fixes, and the location blip
 * over the start ring as annotations. Opens at Android's street-level zoom
 * ([TRACK_ZOOM] in device pixels) once the view has a size.
 */
@OptIn(ExperimentalForeignApi::class)
private class MapKitLiveMap(private var palette: Palette) : LiveMapSurface {
    private val casings = ArrayList<MKPolyline>()
    private val lines = ArrayList<MKPolyline>()
    /** The unsealed tail: fixes since the last sealed chunk, from its last fix on. */
    private val tail = ArrayList<GpsPoint>()
    private var tailCasing: MKPolyline? = null
    private var tailLine: MKPolyline? = null
    private var dotAnnotation: MKPointAnnotation? = null
    private var startAnnotation: MKPointAnnotation? = null
    private var gestures: UserGestures? = null
    private var zoomed = false

    private val delegate = LolloMapDelegate(
        overlayRenderer = { overlay -> renderer(overlay) },
        annotationView = { annotation -> annotationView(annotation) },
    )

    val view = LolloMapView().apply {
        delegate = this@MapKitLiveMap.delegate
        layoutMargins = UIEdgeInsetsMake(0.0, 0.0, LEGAL_LINK_BOTTOM_PT, 0.0)
        onLayout = { openAtTrackZoom() }
    }
    private val tiles = OsmTileLayer(view).also { it.apply(palette) }

    @Composable
    override fun View(modifier: Modifier) {
        // A fresh host per placement: the map moves between layouts (beside the
        // stats, then above the Finish review) and a released host only takes
        // itself out of the hierarchy, never the map now living in the next one.
        UIKitView(
            factory = {
                UIView().apply {
                    view.setFrame(bounds)
                    view.autoresizingMask = UIViewAutoresizingFlexibleWidth or UIViewAutoresizingFlexibleHeight
                    addSubview(view)
                }
            },
            modifier = modifier,
            properties = UIKitInteropProperties(
                interactionMode = UIKitInteropInteractionMode.NonCooperative,
                isNativeAccessibilityEnabled = false,
            ),
        )
    }

    /** The first time the view has a size: Android's [TRACK_ZOOM] (256 px tiles,
     *  device pixels) around the current centre. */
    private fun openAtTrackZoom() {
        if (zoomed || !view.hasSize) return
        zoomed = true
        val worldPx = 256.0 * 2.0.pow(TRACK_ZOOM)
        val (w, h) = view.bounds.useContents { size.width to size.height }
        val scale = UIScreen.mainScreen.scale
        val centre = view.centerCoordinate.useContents { latitude to longitude }
        val fit = MercatorFit()
        val cx = fit.worldX(centre.second)
        val cy = fit.worldY(centre.first)
        val halfW = w * scale / worldPx / 2
        val halfH = h * scale / worldPx / 2
        view.showWorld(cx - halfW, cy - halfH, cx + halfW, cy + halfH)
    }

    private fun renderer(overlay: MKOverlayProtocol): MKOverlayRenderer? {
        val polyline = overlay as? MKPolyline ?: return null
        val casing = isCasing(polyline)
        return MKPolylineRenderer(polyline = polyline).apply {
            strokeColor = uiColor(if (casing) palette.bgTop else palette.accent)
            lineWidth = (if (casing) CASING_WIDTH_DP else TRACK_WIDTH_DP).toDouble()
            lineCap = CGLineCap.kCGLineCapRound
            lineJoin = CGLineJoin.kCGLineJoinRound
        }
    }

    private fun annotationView(annotation: MKAnnotationProtocol): MKAnnotationView? {
        val dot = annotation === dotAnnotation
        if (!dot && annotation !== startAnnotation) return null
        return MKAnnotationView(annotation = annotation, reuseIdentifier = null).apply {
            image = if (dot) locationDotImage(palette) else startRingImage(palette)
            enabled = false
            // The start ring stays under the location dot.
            zPriority = if (dot) MKAnnotationViewZPriorityMax else MKAnnotationViewZPriorityDefaultUnselected
        }
    }

    override fun attach(onUserDrag: () -> Unit) {
        tiles.apply(palette)
        val handler = UserGestures(onUserDrag)
        gestures = handler
        val action = sel_registerName("handleGesture:")
        view.addGestureRecognizer(UIPanGestureRecognizer(target = handler, action = action).also { it.delegate = handler })
        view.addGestureRecognizer(UIPinchGestureRecognizer(target = handler, action = action).also { it.delegate = handler })
    }

    /** Stops reporting gestures and loading tiles; the route and markers stay for a re-attach. */
    override fun detach() {
        view.gestureRecognizers?.forEach { r ->
            val recognizer = r as UIGestureRecognizer
            if (recognizer.delegate === gestures) view.removeGestureRecognizer(recognizer)
        }
        gestures = null
        tiles.release()
    }

    // MapKit suspends its own rendering and tile loads while the app is in the background.
    override fun resume() = Unit
    override fun pause() = Unit

    override fun applyPalette(palette: Palette) {
        this.palette = palette
        tiles.apply(palette)
        for (overlay in casings + lines + listOfNotNull(tailCasing, tailLine)) {
            (view.rendererForOverlay(overlay) as? MKPolylineRenderer)?.let { r ->
                val casing = isCasing(overlay)
                r.strokeColor = uiColor(if (casing) palette.bgTop else palette.accent)
                r.setNeedsDisplay()
            }
        }
        dotAnnotation?.let { view.viewForAnnotation(it)?.image = locationDotImage(palette) }
        startAnnotation?.let { view.viewForAnnotation(it)?.image = startRingImage(palette) }
    }

    override fun setCenter(lat: Double, lon: Double) = view.setCenterCoordinate(CLLocationCoordinate2DMake(lat, lon), animated = false)

    override fun animateTo(lat: Double, lon: Double) = view.setCenterCoordinate(CLLocationCoordinate2DMake(lat, lon), animated = true)

    override fun showLocation(lat: Double, lon: Double) {
        dotAnnotation = place(dotAnnotation, lat, lon)
    }

    override fun showStart(lat: Double, lon: Double) {
        startAnnotation = place(startAnnotation, lat, lon)
    }

    private fun place(annotation: MKPointAnnotation?, lat: Double, lon: Double): MKPointAnnotation {
        val coordinate = CLLocationCoordinate2DMake(lat, lon)
        if (annotation != null) {
            annotation.setCoordinate(coordinate)
            return annotation
        }
        return MKPointAnnotation().also {
            it.setCoordinate(coordinate)
            view.addAnnotation(it)
        }
    }

    override fun clearTrack() {
        view.removeOverlays(casings + lines + listOfNotNull(tailCasing, tailLine))
        casings.clear()
        lines.clear()
        tail.clear()
        tailCasing = null
        tailLine = null
    }

    override fun appendTrack(points: List<GpsPoint>, from: Int) {
        // The current tail pair goes once its replacement (or the sealed chunk) is on.
        val stale = listOfNotNull(tailCasing, tailLine)
        tailCasing = null
        tailLine = null
        for (i in from until points.size) {
            tail += points[i]
            if (tail.size >= TRACK_CHUNK) {
                // Seal the full chunk; the next one starts at its last fix, so the line is unbroken.
                val (casing, line) = drawTail()
                casings += casing
                lines += line
                tailCasing = null
                tailLine = null
                val last = tail.last()
                tail.clear()
                tail += last
            }
        }
        if (tail.size >= 2) drawTail()
        view.removeOverlays(stale)
    }

    private fun isCasing(polyline: MKPolyline) = polyline === tailCasing || casings.any { it === polyline }

    /** Adds the tail as a casing (under every line) and line polyline pair. */
    private fun drawTail(): Pair<MKPolyline, MKPolyline> {
        val casing = polyline(tail)
        val line = polyline(tail)
        // Registered before MapKit asks for their renderers.
        tailCasing = casing
        tailLine = line
        view.insertOverlay(casing, atIndex = 0uL, level = MKOverlayLevelAboveLabels)
        view.addOverlay(line, level = MKOverlayLevelAboveLabels)
        return casing to line
    }

    private fun polyline(points: List<GpsPoint>): MKPolyline = memScoped {
        val coords = allocArray<CLLocationCoordinate2D>(points.size)
        points.forEachIndexed { i, p ->
            coords[i].latitude = p.lat
            coords[i].longitude = p.lon
        }
        MKPolyline.polylineWithCoordinates(coords, points.size.toULong())
    }

    // MapKit redraws overlays and annotations as they change.
    override fun invalidate() = Unit
}
