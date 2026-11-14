package app.milestone

import kotlinx.cinterop.ExperimentalForeignApi
import kotlinx.cinterop.ObjCSignatureOverride
import kotlinx.cinterop.cValue
import kotlinx.cinterop.useContents
import platform.CoreGraphics.CGRectMake
import platform.MapKit.MKAnnotationProtocol
import platform.MapKit.MKAnnotationView
import platform.MapKit.MKMapRect
import platform.MapKit.MKMapView
import platform.MapKit.MKMapViewDelegateProtocol
import platform.MapKit.MKOverlayLevelAboveRoads
import platform.MapKit.MKOverlayProtocol
import platform.MapKit.MKOverlayRenderer
import platform.MapKit.insertOverlay
import platform.MapKit.removeOverlay
import platform.darwin.NSObject

/**
 * The MKMapView every map in the app uses: no rotation, pitch, compass or scale, so
 * north stays up as on Android, and a callback after each layout pass (the view
 * knows its size only then, so a framing set earlier is applied again).
 */
@OptIn(ExperimentalForeignApi::class)
internal class LolloMapView : MKMapView(frame = CGRectMake(0.0, 0.0, 0.0, 0.0)) {
    var onLayout: (() -> Unit)? = null
    private var laidOutWidth = 0.0
    private var laidOutHeight = 0.0

    init {
        rotateEnabled = false
        pitchEnabled = false
        showsCompass = false
        showsScale = false
        showsUserLocation = false
    }

    val hasSize: Boolean get() = bounds.useContents { size.width > 0.0 && size.height > 0.0 }

    override fun layoutSubviews() {
        super.layoutSubviews()
        val (w, h) = bounds.useContents { size.width to size.height }
        if (w != laidOutWidth || h != laidOutHeight) {
            laidOutWidth = w
            laidOutHeight = h
            onLayout?.invoke()
        }
    }

    /** Shows the world rectangle [left]..[right] × [top]..[bottom] (Web-Mercator 0..1
     *  units, as [MercatorFit] uses), wrapped into the first world copy. */
    fun showWorld(left: Double, top: Double, right: Double, bottom: Double) {
        var l = left
        var r = right
        while (l >= 1.0) { l -= 1.0; r -= 1.0 }
        while (l < 0.0) { l += 1.0; r += 1.0 }
        val rect = cValue<MKMapRect> {
            origin.x = l * MapWorldPoints
            origin.y = top * MapWorldPoints
            size.width = (r - l) * MapWorldPoints
            size.height = (bottom - top) * MapWorldPoints
        }
        setVisibleMapRect(rect, animated = false)
    }
}

/**
 * The OSM tile layer of a map, swapped for a light or dark ([OsmTileOverlay]) one
 * when the palette's ground changes.
 */
internal class OsmTileLayer(private val map: MKMapView) {
    private var overlay: OsmTileOverlay? = null
    private var dark: Boolean? = null

    fun apply(palette: Palette) {
        val isDark = palette.isDark
        if (dark == isDark) return
        dark = isDark
        overlay?.let { map.removeOverlay(it) }
        overlay = OsmTileOverlay(isDark).also { map.insertOverlay(it, atIndex = 0uL, level = MKOverlayLevelAboveRoads) }
    }

    fun release() {
        overlay?.let { map.removeOverlay(it) }
        overlay = null
        dark = null
    }
}

/**
 * A map's delegate: OSM tiles render through [OsmTileRenderer], every other
 * overlay through [overlayRenderer], annotations through [annotationView].
 * MKMapView holds its delegate weakly, so its owner keeps a reference.
 */
@OptIn(ExperimentalForeignApi::class)
internal class LolloMapDelegate(
    private val overlayRenderer: (MKOverlayProtocol) -> MKOverlayRenderer? = { null },
    private val annotationView: (MKAnnotationProtocol) -> MKAnnotationView? = { null },
) : NSObject(), MKMapViewDelegateProtocol {
    @ObjCSignatureOverride
    override fun mapView(mapView: MKMapView, rendererForOverlay: MKOverlayProtocol): MKOverlayRenderer =
        if (rendererForOverlay is OsmTileOverlay) {
            OsmTileRenderer(rendererForOverlay)
        } else {
            overlayRenderer(rendererForOverlay) ?: MKOverlayRenderer(overlay = rendererForOverlay)
        }

    @ObjCSignatureOverride
    override fun mapView(mapView: MKMapView, viewForAnnotation: MKAnnotationProtocol): MKAnnotationView? =
        annotationView(viewForAnnotation)
}
