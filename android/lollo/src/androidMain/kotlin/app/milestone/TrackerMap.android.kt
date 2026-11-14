package app.milestone

import android.annotation.SuppressLint
import android.content.Context
import android.graphics.drawable.LayerDrawable
import android.graphics.drawable.ShapeDrawable
import android.graphics.drawable.shapes.OvalShape
import android.view.MotionEvent
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.viewinterop.AndroidView
import org.osmdroid.util.GeoPoint
import org.osmdroid.views.MapView
import org.osmdroid.views.overlay.Marker

internal actual fun newLiveMapSurface(ctx: PlatformContext, palette: Palette): LiveMapSurface = OsmLiveMap(ctx, palette)

/**
 * The live tracker's osmdroid map and its overlays: the two-layer route stroke, the
 * current-location blip and the start ring. osmdroid Paint widths are raw px, so
 * every dp size is density-scaled.
 */
private class OsmLiveMap(ctx: Context, palette: Palette) : LiveMapSurface {
    // Destroy mode off: osmdroid's default tears the overlays down (Polyline
    // outline nulled) whenever the view leaves the window, and the tracker moves
    // this one view between layouts (beside the stats, then above the Finish
    // review); the next track update would write into the dead overlays. [detach]
    // releases it when the screen goes.
    val view: MapView = newOsmMapView(ctx).apply {
        controller.setZoom(TRACK_ZOOM)
        setDestroyMode(false)
    }
    private val density = ctx.resources.displayMetrics.density
    private val casing = routeLine(palette.bgTop.toArgb(), CASING_WIDTH_DP * density)
    private val track = routeLine(palette.accent.toArgb(), TRACK_WIDTH_DP * density)
    private val locationDot = centeredMarker(locationDotIcon(palette))
    private val startRing = centeredMarker(startRingIcon(palette))

    @Composable
    override fun View(modifier: Modifier) {
        AndroidView(factory = { view }, modifier = modifier)
    }

    private fun circle(color: Color, sizeDp: Float): ShapeDrawable = ShapeDrawable(OvalShape()).apply {
        paint.color = color.toArgb()
        intrinsicWidth = (sizeDp * density).toInt()
        intrinsicHeight = (sizeDp * density).toInt()
    }

    private fun ringIcon(outer: Color, outerDp: Float, inner: Color, innerDp: Float): LayerDrawable {
        val inset = (MARKER_RING_DP * density).toInt()
        return LayerDrawable(arrayOf(circle(outer, outerDp), circle(inner, innerDp)))
            .apply { setLayerInset(1, inset, inset, inset, inset) }
    }

    private fun locationDotIcon(palette: Palette) = ringIcon(palette.bgTop, DOT_OUTER_DP, palette.accent, DOT_INNER_DP)

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
        if (view.overlays.contains(marker)) return
        // osmdroid draws in list order: the start ring stays under the location dot.
        if (marker === startRing) view.overlays.addBelow(startRing, locationDot) else view.overlays.add(marker)
    }

    /**
     * The overlays are built once, so a mid-run light/dark flip recolours the strokes
     * and rebuilds the marker icons instead of leaving stale ones, and re-themes the
     * tiles ([applyMapTheme]).
     */
    override fun applyPalette(palette: Palette) {
        casing.outlinePaint.color = palette.bgTop.toArgb()
        track.outlinePaint.color = palette.accent.toArgb()
        locationDot.icon = locationDotIcon(palette)
        startRing.icon = startRingIcon(palette)
        view.applyMapTheme(palette)
        view.invalidate()
    }

    @SuppressLint("ClickableViewAccessibility")
    override fun attach(onUserDrag: () -> Unit) {
        view.overlays.add(casing)
        view.overlays.add(track)
        view.setOnTouchListener { v, ev ->
            v.parent?.requestDisallowInterceptTouchEvent(true)
            if (ev.actionMasked == MotionEvent.ACTION_MOVE) onUserDrag()
            false
        }
    }

    override fun detach() {
        view.setOnTouchListener(null)
        view.onPause()
        view.onDetach()
    }

    override fun resume() = view.onResume()

    override fun pause() = view.onPause()

    override fun setCenter(lat: Double, lon: Double) = view.controller.setCenter(GeoPoint(lat, lon))

    override fun animateTo(lat: Double, lon: Double) = view.controller.animateTo(GeoPoint(lat, lon))

    override fun showLocation(lat: Double, lon: Double) = show(locationDot, GeoPoint(lat, lon))

    override fun showStart(lat: Double, lon: Double) = show(startRing, GeoPoint(lat, lon))

    override fun clearTrack() {
        casing.setPoints(emptyList())
        track.setPoints(emptyList())
    }

    override fun appendTrack(points: List<GpsPoint>, from: Int) {
        for (i in from until points.size) {
            val gp = GeoPoint(points[i].lat, points[i].lon)
            casing.addPoint(gp)
            track.addPoint(gp)
        }
    }

    override fun invalidate() = view.invalidate()
}

/** Insert [item] just below [above] in draw order, or on top when [above] is absent. */
internal fun <T> MutableList<T>.addBelow(item: T, above: T) {
    val i = indexOf(above)
    if (i >= 0) add(i, item) else add(item)
}
