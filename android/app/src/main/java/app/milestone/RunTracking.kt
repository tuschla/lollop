package app.milestone

import android.Manifest
import android.annotation.SuppressLint
import android.content.Context
import android.content.pm.PackageManager
import android.graphics.Paint
import android.graphics.drawable.LayerDrawable
import android.graphics.drawable.ShapeDrawable
import android.graphics.drawable.shapes.OvalShape
import android.location.LocationManager
import android.os.Build
import android.view.MotionEvent
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.ui.draw.clip
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.core.location.LocationManagerCompat
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import org.osmdroid.tileprovider.tilesource.TileSourceFactory
import org.osmdroid.util.GeoPoint
import org.osmdroid.views.MapView
import org.osmdroid.views.overlay.Marker
import org.osmdroid.views.overlay.Polyline

/**
 * GPS run-tracking screen. The actual location stream lives in
 * [RunTrackingService] (a foreground service) so recording survives screen-off
 * and backgrounding; this screen only observes [RunSession] and, on Stop, hands
 * the raw fix list to the Rust core via [Event.LogRunTrack], distance / pace /
 * zone / spike are all derived there.
 */
@SuppressLint("ClickableViewAccessibility")
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RunTrackingScreen(model: ViewModel, onFinish: (ViewModel?) -> Unit) {
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()

    // Follow mode keeps the map centred on the latest fix. A manual drag drops it
    // so the runner can inspect the route; the Recenter button re-arms it.
    var follow by remember { mutableStateOf(true) }

    // FINE only: GPS tracking is unusable on a coarse-only grant (the platform
    // GPS provider throws SecurityException without FINE), so a coarse-only
    // result must NOT enable Start: it reads as "needs precise location" with a
    // re-request path instead.
    var hasPermission by remember { mutableStateOf(hasFineLocation(ctx)) }
    // Degraded-but-not-blocking states the runner should know about before a
    // long run silently loses its lockscreen notification or records no fixes.
    var notifGranted by remember { mutableStateOf(notificationsAllowed(ctx)) }
    var locationOn by remember { mutableStateOf(isLocationEnabled(ctx)) }
    val permLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions()
    ) { result ->
        hasPermission = result[Manifest.permission.ACCESS_FINE_LOCATION] == true
        notifGranted = notificationsAllowed(ctx)
        locationOn = isLocationEnabled(ctx)
    }

    fun requestPermissions() {
        val perms = mutableListOf(
            Manifest.permission.ACCESS_FINE_LOCATION,
            Manifest.permission.ACCESS_COARSE_LOCATION,
        )
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            perms += Manifest.permission.POST_NOTIFICATIONS
        }
        permLauncher.launch(perms.toTypedArray())
    }

    LaunchedEffect(Unit) { if (!hasPermission) requestPermissions() }

    val points by RunSession.points.collectAsState()
    val tracking by RunSession.tracking.collectAsState()

    // The map view + its track overlay, retained across recompositions.
    val mapView = remember {
        MapView(ctx).apply {
            setTileSource(TileSourceFactory.MAPNIK)
            setMultiTouchControls(true)
            controller.setZoom(17.0)
        }
    }
    // Two-layer stroke (design spec §4): a dark casing under the accent core so the
    // route stays legible on both light and dark tiles and never blends into the
    // basemap. Casing is added first so the accent core draws on top of it. The
    // palette is read here (in composition) because the remember lambdas below run
    // outside it and cannot touch the composition-local tokens. osmdroid Paint
    // widths are raw px, so the token dp values (Space.Sm accent / Space.Md
    // casing) are density-scaled here, a fixed px width would render thinner
    // on denser screens.
    val palette = LocalPalette.current
    val strokeDensity = ctx.resources.displayMetrics.density
    val casing = remember {
        Polyline().apply {
            outlinePaint.color = palette.bgTop.toArgb()
            outlinePaint.strokeWidth = Space.Md * strokeDensity
            outlinePaint.strokeCap = Paint.Cap.ROUND
            outlinePaint.strokeJoin = Paint.Join.ROUND
        }
    }
    val track = remember {
        Polyline().apply {
            outlinePaint.color = palette.accent.toArgb()
            outlinePaint.strokeWidth = Space.Sm * strokeDensity
            outlinePaint.strokeCap = Paint.Cap.ROUND
            outlinePaint.strokeJoin = Paint.Join.ROUND
        }
    }
    // Self-location blip: an accent dot with a ground-toned casing ring, pinned
    // to the LATEST fix (the same stream the route is drawn from, no second
    // location client). Added to the overlays lazily on the first fix so no dot
    // ever renders at the (0,0) default position.
    val locationDot = remember {
        val den = ctx.resources.displayMetrics.density
        fun circle(color: Int, sizeDp: Float) = ShapeDrawable(OvalShape()).apply {
            paint.color = color
            intrinsicWidth = (sizeDp * den).toInt()
            intrinsicHeight = (sizeDp * den).toInt()
        }
        val inset = (4 * den).toInt()
        val icon = LayerDrawable(
            arrayOf(circle(palette.bgTop.toArgb(), 20f), circle(palette.accent.toArgb(), 12f)),
        ).apply { setLayerInset(1, inset, inset, inset, inset) }
        Marker(mapView).apply {
            setAnchor(Marker.ANCHOR_CENTER, Marker.ANCHOR_CENTER)
            this.icon = icon
            setInfoWindow(null)
        }
    }
    DisposableEffect(Unit) {
        mapView.overlays.add(casing)
        mapView.overlays.add(track)
        // A user drag (not a programmatic setCenter, which dispatches no touch)
        // means they want to look around: stop yanking the map back.
        mapView.setOnTouchListener { v, ev ->
            // The map owns its gestures: without this, a parent can intercept
            // mid-pinch and the surrounding chrome judders in and out.
            v.parent?.requestDisallowInterceptTouchEvent(true)
            if (ev.actionMasked == MotionEvent.ACTION_MOVE) follow = false
            false
        }
        mapView.onResume()
        onDispose {
            mapView.setOnTouchListener(null)
            mapView.onPause()
            // Release osmdroid's tile provider + overlay manager (and its writer
            // thread) instead of leaking them until GC each time this screen closes.
            mapView.onDetach()
        }
    }

    // Before the first fix arrives, center on last-known location so the map
    // doesn't open on (0,0) in the Gulf of Guinea.
    LaunchedEffect(hasPermission) {
        if (hasPermission && points.isEmpty()) {
            RunTrackingService.lastKnownLocation(ctx) { lat, lon ->
                if (points.isEmpty()) mapView.controller.setCenter(GeoPoint(lat, lon))
            }
        }
    }

    // Redraw the polyline from the observed fix list whenever it grows.
    LaunchedEffect(points, follow) {
        val geo = points.map { GeoPoint(it.lat, it.lon) }
        casing.setPoints(geo)
        track.setPoints(geo)
        geo.lastOrNull()?.let { last ->
            locationDot.position = last
            if (!mapView.overlays.contains(locationDot)) mapView.overlays.add(locationDot)
            if (follow) mapView.controller.setCenter(last)
        }
        mapView.invalidate()
    }

    fun startTracking() {
        if (!hasPermission) {
            requestPermissions()
            return
        }
        locationOn = isLocationEnabled(ctx)
        RunSession.reset()
        RunTrackingService.start(ctx)
    }

    // Guards a double Stop tap while the coroutine below awaits the service's
    // confirmation, a second pass would log the same run twice.
    var stopping by remember { mutableStateOf(false) }

    fun stopAndLog() {
        if (stopping) return
        stopping = true
        scope.launch {
            RunTrackingService.stop(ctx)
            // Snapshot the track only AFTER the service confirms the engine is
            // unregistered (stopTracking flips `tracking` false once the last fix
            // is delivered): the stop intent is asynchronous, so a snapshot taken
            // immediately would drop tail fixes still in flight. The timeout
            // covers a service that died without confirming.
            withTimeoutOrNull(2_000L) { RunSession.tracking.first { !it } }
            val captured = RunSession.points.value
            val vm = if (captured.size >= 2) {
                Core.send(
                    Event.LogRunTrack(
                        // No paired HR sensor yet, and the spike baseline is derived
                        // in-core from prior runs; the shell sends neither figure.
                        points = captured,
                        hrPctMax = 0.0,
                        longestRecentKm = 0.0,
                    )
                )
            } else {
                // Fewer than two fixes is not a route the core can derive pace/distance
                // from: say so instead of silently dropping the tap.
                Toast.makeText(ctx, "Run too short to log", Toast.LENGTH_SHORT).show()
                null
            }
            RunSession.reset()
            onFinish(vm)
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Track run", style = Type.Title) },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = BgTop,
                    titleContentColor = Accent,
                ),
            )
        },
    ) { pad ->
        Column(Modifier.fillMaxSize().padding(pad)) {
            // Pinned safety banner (usability spec §3: on EVERY screen, never
            // scrollable/dismissable). This screen replaces the root scaffold, so
            // it must re-pin the banner itself: an active DO-NOT-TRAIN hold has
            // to stay visible mid-run too. Renders nothing when no tier is active.
            SafetyBanner(
                model,
                Modifier
                    .padding(horizontal = Space.Screen.dp)
                    .padding(top = Space.Sm.dp, bottom = Space.Md.dp),
            )
            // The map sits in its own rounded, clipped inset: the banner ends,
            // gutter, map begins: nothing folds into or under the map, and the
            // rounded clip keeps map tiles from butting against screen chrome.
            Box(
                Modifier
                    .weight(1f)
                    .fillMaxWidth()
                    .padding(horizontal = Space.Screen.dp)
                    .clip(RoundedCornerShape(Space.Card.dp)),
            ) {
                AndroidView(factory = { mapView }, modifier = Modifier.fillMaxSize())
                // Map-view control pinned BOTTOM-RIGHT as an overlay: appearing /
                // disappearing never resizes the map (the old inline button row
                // grew and shrank the layout mid-pinch, glitching the chrome).
                if (!follow) {
                    Button(
                        onClick = {
                            follow = true
                            points.lastOrNull()?.let {
                                mapView.controller.animateTo(GeoPoint(it.lat, it.lon))
                            }
                        },
                        colors = ButtonDefaults.buttonColors(
                            containerColor = BgElevated,
                            contentColor = Accent,
                        ),
                        modifier = Modifier
                            .align(Alignment.BottomEnd)
                            .padding(Space.Md.dp),
                    ) { Text("Recenter") }
                }
            }
            Column(Modifier.padding(Space.Screen.dp), verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                when {
                    !hasPermission -> Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                        Text(
                            "Precise location (GPS) is needed to track a run - approximate-only location can't record a route.",
                            color = OnBgMuted,
                            style = Type.Body,
                        )
                        OutlinedButton(onClick = { requestPermissions() }) {
                            Text("Grant precise location")
                        }
                    }
                    !tracking && points.isEmpty() -> Text(
                        "Press Start to begin recording.",
                        color = OnBgMuted,
                        style = Type.Body,
                    )
                    else -> {
                        // Elapsed spans the first-to-last raw fix: a live progress
                        // indicator, not the of-record figure. The core derives the
                        // logged duration from accuracy-filtered fixes, so this can run
                        // slightly long when a start/end fix is GPS noise. Distance /
                        // pace / zone stay core-derived (post-Stop): no shell-side
                        // haversine, so they are deliberately not shown live.
                        val secs =
                            if (points.size >= 2)
                                (points.last().observedAt - points.first().observedAt)
                                    .coerceAtLeast(0L)
                            else 0L
                        val fixLabel = if (points.size == 1) "fix" else "fixes"
                        Column(
                            modifier = Modifier
                                .fillMaxWidth()
                                .clip(RoundedCornerShape(Space.Card.dp))
                                .background(BgElevated)
                                .padding(Space.Card.dp),
                            verticalArrangement = Arrangement.spacedBy(Space.Xs.dp),
                        ) {
                            Text("ELAPSED", color = OnBgMuted, style = Type.Section)
                            Text(
                                formatElapsed(secs),
                                color = OnBgBody,
                                style = Type.Display.merge(TabularFigures),
                            )
                            Text(
                                "${points.size} $fixLabel",
                                color = OnBgFaint,
                                style = Type.Caption.merge(TabularFigures),
                            )
                        }
                    }
                }
                // Degraded-state surfacing (design-spec §9 run-tracking polish):
                // location services off means zero fixes will ever arrive: loud,
                // warn-ground row; a denied notification permission (API 33+) only
                // hides the ongoing notification, so a muted caption suffices.
                if (hasPermission && !locationOn) {
                    Text(
                        "Location services are off - no GPS fixes will be recorded. Enable location in system settings.",
                        color = Color.White,
                        style = Type.Body,
                        modifier = Modifier
                            .fillMaxWidth()
                            .clip(RoundedCornerShape(Space.Card.dp))
                            .background(LocalStatusColors.current.warn)
                            .padding(Space.Card.dp),
                    )
                }
                if (!notifGranted) {
                    Text(
                        "Notifications are off - tracking still works, but the ongoing lockscreen notification won't show.",
                        color = OnBgMuted,
                        style = Type.Caption,
                    )
                }
                // Actions bottom-RIGHT (user feedback #19), primary outermost.
                // Back never kills a live run: the foreground service keeps
                // recording and the Today "run in progress" chip (or the
                // notification tap) returns here with full live state: only a
                // non-tracking exit tears the session down.
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(Space.Md.dp, Alignment.End),
                ) {
                    OutlinedButton(onClick = {
                        if (!tracking) {
                            RunTrackingService.stop(ctx)
                            RunSession.reset()
                        }
                        onFinish(null)
                    }) { Text(if (tracking) "Back · keeps recording" else "Back") }
                    if (!tracking) {
                        Button(onClick = { startTracking() }) { Text("Start") }
                    } else {
                        Button(
                            onClick = { stopAndLog() },
                            enabled = !stopping,
                            colors = ButtonDefaults.buttonColors(
                                containerColor = LocalStatusColors.current.dangerStrong,
                            ),
                        ) { Text("Stop & log", color = Color.White) }
                    }
                }
            }
        }
    }
}

/** True when the app may post notifications: below API 33 there is no runtime
 *  gate; from 33 on, the POST_NOTIFICATIONS grant decides. */
private fun notificationsAllowed(ctx: Context): Boolean =
    Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
        ContextCompat.checkSelfPermission(ctx, Manifest.permission.POST_NOTIFICATIONS) ==
            PackageManager.PERMISSION_GRANTED

/** True when the system-wide location toggle is on. A fine-location grant alone
 *  delivers no fixes while location services are disabled, so the tracking UI
 *  surfaces this state instead of recording silence. */
private fun isLocationEnabled(ctx: Context): Boolean {
    val lm = ctx.getSystemService(Context.LOCATION_SERVICE) as LocationManager
    return LocationManagerCompat.isLocationEnabled(lm)
}
