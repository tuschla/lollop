package de.tuschla.fitnessanlage

import android.Manifest
import android.annotation.SuppressLint
import android.content.pm.PackageManager
import android.graphics.Paint
import android.os.Build
import android.view.MotionEvent
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
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
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import org.osmdroid.tileprovider.tilesource.TileSourceFactory
import org.osmdroid.util.GeoPoint
import org.osmdroid.views.MapView
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
fun RunTrackingScreen(onFinish: (ViewModel?) -> Unit) {
    val ctx = LocalContext.current

    // Follow mode keeps the map centred on the latest fix. A manual drag drops it
    // so the runner can inspect the route; the Recenter button re-arms it.
    var follow by remember { mutableStateOf(true) }

    var hasPermission by remember {
        mutableStateOf(
            ContextCompat.checkSelfPermission(ctx, Manifest.permission.ACCESS_FINE_LOCATION) ==
                PackageManager.PERMISSION_GRANTED
        )
    }
    val permLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions()
    ) { result ->
        hasPermission = result[Manifest.permission.ACCESS_FINE_LOCATION] == true ||
            result[Manifest.permission.ACCESS_COARSE_LOCATION] == true
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
    // outside it and cannot touch the composition-local tokens.
    val palette = LocalPalette.current
    val casing = remember {
        Polyline().apply {
            outlinePaint.color = palette.bgTop.toArgb()
            outlinePaint.strokeWidth = 18f
            outlinePaint.strokeCap = Paint.Cap.ROUND
            outlinePaint.strokeJoin = Paint.Join.ROUND
        }
    }
    val track = remember {
        Polyline().apply {
            outlinePaint.color = palette.accent.toArgb()
            outlinePaint.strokeWidth = 10f
            outlinePaint.strokeCap = Paint.Cap.ROUND
            outlinePaint.strokeJoin = Paint.Join.ROUND
        }
    }
    DisposableEffect(Unit) {
        mapView.overlays.add(casing)
        mapView.overlays.add(track)
        // A user drag (not a programmatic setCenter, which dispatches no touch)
        // means they want to look around: stop yanking the map back.
        mapView.setOnTouchListener { _, ev ->
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
        if (follow) {
            points.lastOrNull()?.let { mapView.controller.setCenter(GeoPoint(it.lat, it.lon)) }
        }
        mapView.invalidate()
    }

    fun startTracking() {
        if (!hasPermission) {
            requestPermissions()
            return
        }
        RunSession.reset()
        RunTrackingService.start(ctx)
    }

    fun stopAndLog() {
        RunTrackingService.stop(ctx)
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
            Box(Modifier.weight(1f).fillMaxWidth()) {
                AndroidView(factory = { mapView }, modifier = Modifier.fillMaxSize())
            }
            Column(Modifier.padding(Space.Screen.dp), verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                Text(
                    when {
                        !hasPermission -> "Location permission needed to track."
                        !tracking && points.isEmpty() -> "Press Start to begin recording."
                        else -> {
                            // Elapsed spans the first-to-last raw fix. The core
                            // derives the logged duration from accuracy-filtered
                            // fixes, so this live readout can run slightly long when
                            // a start/end fix is GPS noise (dropped there, not here);
                            // it is a progress indicator, not the of-record figure.
                            // Distance/pace stay core-derived (post-Stop).
                            val secs =
                                if (points.size >= 2)
                                    (points.last().observedAt - points.first().observedAt)
                                        .coerceAtLeast(0L)
                                else 0L
                            val fixLabel = if (points.size == 1) "fix" else "fixes"
                            "${formatElapsed(secs)} · ${points.size} $fixLabel"
                        }
                    },
                    color = OnBgMuted,
                    style = Type.Body.merge(TabularFigures),
                )
                Row(horizontalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                    if (!tracking) {
                        Button(onClick = { startTracking() }) { Text("Start") }
                    } else {
                        Button(
                            onClick = { stopAndLog() },
                            colors = ButtonDefaults.buttonColors(
                                containerColor = LocalStatusColors.current.dangerStrong,
                            ),
                        ) { Text("Stop & log", color = Color.White) }
                    }
                    if (!follow) {
                        OutlinedButton(onClick = {
                            follow = true
                            points.lastOrNull()?.let {
                                mapView.controller.animateTo(GeoPoint(it.lat, it.lon))
                            }
                        }) { Text("Recenter") }
                    }
                    OutlinedButton(onClick = {
                        RunTrackingService.stop(ctx)
                        RunSession.reset()
                        onFinish(null)
                    }) { Text("Back") }
                }
            }
        }
    }
}
