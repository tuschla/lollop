package app.milestone

import android.Manifest
import android.annotation.SuppressLint
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.graphics.Paint
import android.graphics.drawable.LayerDrawable
import android.graphics.drawable.ShapeDrawable
import android.graphics.drawable.shapes.OvalShape
import android.location.LocationManager
import android.os.Build
import android.view.MotionEvent
import android.widget.Toast
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.wrapContentWidth
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.core.location.LocationManagerCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import java.util.Locale
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import org.osmdroid.tileprovider.tilesource.TileSourceFactory
import org.osmdroid.util.GeoPoint
import org.osmdroid.views.CustomZoomButtonsController
import org.osmdroid.views.MapView
import org.osmdroid.views.overlay.Marker
import org.osmdroid.views.overlay.Polyline

/**
 * GPS run-tracking overlay (06-run-tracking): map fills the frame, stat sheet
 * pins to the bottom, safety banner (when active) pins topmost. The location
 * stream lives in [RunTrackingService] (foreground service) so recording
 * survives screen-off/backgrounding; on Stop & save the raw fix list goes to
 * the Rust core via [Event.LogRunTrack], the OF-RECORD distance / pace /
 * zone / spike are all derived there. The live elapsed / distance / pace shown
 * while recording are factual progress indicators computed from the raw fixes
 * (no coaching, no thresholds); they are superseded by the core's derivation
 * on save.
 */
@SuppressLint("ClickableViewAccessibility")
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RunTrackingScreen(
    model: ViewModel,
    onEvent: (Event) -> Unit = {},
    onFinish: (ViewModel?) -> Unit,
) {
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    val status = LocalStatusColors.current

    // Follow mode keeps the map centred on the latest fix. A manual drag drops it
    // so the runner can inspect the route; the Recenter button re-arms it.
    var follow by remember { mutableStateOf(true) }

    // FINE only: GPS tracking is unusable on a coarse-only grant.
    var hasPermission by remember { mutableStateOf(hasFineLocation(ctx)) }
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
    // C5 rotation escape hatch: a captured-but-unsaved run is DERIVED from
    // RunSession (the service is stopped yet raw fixes are still held), not kept in
    // a volatile `remember` that dies on rotation / Back+reopen. The locate-only
    // preview records no points, so `!tracking && points.isNotEmpty()` is precise:
    // it can only be a track awaiting save (failed Core.send / dismissed short-run
    // prompt). While true the control row shows Save/Discard (never Start), the
    // locate-restart effect and BackHandler stand down, and Start refuses, so the
    // captured run is never silently cleared (Back) or merged into the next (Start).
    val unsavedCapture = hasUnsavedCapture(tracking, points.size)
    val paused by RunSession.paused.collectAsState()
    // Live distance is accumulated one segment at a time in RunSession, so
    // reading it here is O(1), no re-haversine of the whole track every fix.
    val liveKm by RunSession.distanceKm.collectAsState()
    // Elapsed comes from RunSession's MONOTONIC clock (elapsedRealtime), not a
    // first→last wall-clock span, so an NTP correction mid-run can't make it jump or
    // go negative.
    val elapsedSec by RunSession.elapsedSec.collectAsState()
    // Locate-only preview (Phase 4 / M4): the service acquires GPS on open but
    // records nothing until Start. `lastFix` drives the lock readout + preview
    // dot; a fix within `GPS_LOCK_ACCURACY_M` counts as a usable lock that
    // enables Start.
    val locating by RunSession.locating.collectAsState()
    val lastFix by RunSession.lastFix.collectAsState()
    // Live-reactive display prefs (shell chrome, not coaching state): the pace-bucket
    // size and the distance/pace unit. Both drive the readouts below and update the
    // moment the user changes them in Profile.
    val paceBucketN by ThemeSettings.paceBucketMinutes.collectAsState()
    val distanceUnitOverride by ThemeSettings.distanceUnitOverride.collectAsState()
    val unit = remember(distanceUnitOverride) { resolveDistanceUnit(distanceUnitOverride) }
    val gpsLocked = lastFix?.let { it.accuracyM in 0.0..GPS_LOCK_ACCURACY_M } == true
    // Set the moment the user leaves (Discard / Back / after save) so the
    // locate-restart effect can't re-launch the service into a disposing screen.
    var exiting by remember { mutableStateOf(false) }
    // Short-run keep/discard confirmation (Phase 4 / M4) before an implausible run
    // is logged. `rememberSaveable` so a rotation while the prompt is up doesn't
    // strand the captured run (the `Double?` is Bundle-safe). Dismissing it just
    // clears this: the derived `unsavedCapture` keeps the Save/Discard row up, no
    // separate held-track state needed.
    var shortRunKm by rememberSaveable { mutableStateOf<Double?>(null) }

    // The map view + its track overlay, retained across recompositions.
    val mapView = remember {
        MapView(ctx).apply {
            setTileSource(TileSourceFactory.MAPNIK)
            setMultiTouchControls(true)
            // No built-in +/− zoom buttons (they aren't in the 06 spec's
            // overlay set); pinch-to-zoom stays via multi-touch.
            zoomController.setVisibility(CustomZoomButtonsController.Visibility.NEVER)
            controller.setZoom(17.0)
        }
    }
    // Two-layer stroke (06 spec: `Accent` 10dp over an 18dp ground casing) so
    // the route stays legible on any tile. osmdroid Paint widths are raw px, so
    // the dp values are density-scaled here (owner rule: dp tokens, no raw px).
    val palette = LocalPalette.current
    val strokeDensity = ctx.resources.displayMetrics.density
    val casing = remember {
        Polyline().apply {
            outlinePaint.color = palette.bgTop.toArgb()
            outlinePaint.strokeWidth = 18f * strokeDensity
            outlinePaint.strokeCap = Paint.Cap.ROUND
            outlinePaint.strokeJoin = Paint.Join.ROUND
        }
    }
    val track = remember {
        Polyline().apply {
            outlinePaint.color = palette.accent.toArgb()
            outlinePaint.strokeWidth = 10f * strokeDensity
            outlinePaint.strokeCap = Paint.Cap.ROUND
            outlinePaint.strokeJoin = Paint.Join.ROUND
        }
    }
    fun circle(color: Int, sizeDp: Float): ShapeDrawable = ShapeDrawable(OvalShape()).apply {
        paint.color = color
        intrinsicWidth = (sizeDp * strokeDensity).toInt()
        intrinsicHeight = (sizeDp * strokeDensity).toInt()
    }
    // Current-location blip (spec: `Accent` fill, 3dp `BgTop` casing, r9),
    // pinned to the LATEST fix. Added lazily on the first fix so no dot ever
    // renders at the (0,0) default position.
    val locationDot = remember {
        val inset = (3 * strokeDensity).toInt()
        val icon = LayerDrawable(
            arrayOf(circle(palette.bgTop.toArgb(), 24f), circle(palette.accent.toArgb(), 18f)),
        ).apply { setLayerInset(1, inset, inset, inset, inset) }
        Marker(mapView).apply {
            setAnchor(Marker.ANCHOR_CENTER, Marker.ANCHOR_CENTER)
            this.icon = icon
            setInfoWindow(null)
        }
    }
    // Start marker (spec: ring, `BgTop` fill, 3dp `Accent` stroke, r7), at the
    // first fix.
    val startRing = remember {
        val inset = (3 * strokeDensity).toInt()
        val icon = LayerDrawable(
            arrayOf(circle(palette.accent.toArgb(), 20f), circle(palette.bgTop.toArgb(), 14f)),
        ).apply { setLayerInset(1, inset, inset, inset, inset) }
        Marker(mapView).apply {
            setAnchor(Marker.ANCHOR_CENTER, Marker.ANCHOR_CENTER)
            this.icon = icon
            setInfoWindow(null)
        }
    }
    // React to a mid-run light/dark theme flip: the polylines + markers were
    // captured in keyless remembers, so repaint their colours (and rebuild the
    // marker icons) when the palette changes instead of leaving stale ones.
    LaunchedEffect(palette) {
        casing.outlinePaint.color = palette.bgTop.toArgb()
        track.outlinePaint.color = palette.accent.toArgb()
        val dotInset = (3 * strokeDensity).toInt()
        locationDot.icon = LayerDrawable(
            arrayOf(circle(palette.bgTop.toArgb(), 24f), circle(palette.accent.toArgb(), 18f)),
        ).apply { setLayerInset(1, dotInset, dotInset, dotInset, dotInset) }
        startRing.icon = LayerDrawable(
            arrayOf(circle(palette.accent.toArgb(), 20f), circle(palette.bgTop.toArgb(), 14f)),
        ).apply { setLayerInset(1, dotInset, dotInset, dotInset, dotInset) }
        mapView.invalidate()
    }

    // Bind the osmdroid MapView to the ACTIVITY lifecycle (not just composition):
    // resume/pause its tile threads with the app so it stops draining battery in
    // the background, and re-sample the location/permission/notification states on
    // every ON_RESUME. Mid-run the system location toggle can be flipped off or
    // the fine-location grant revoked in Settings, without this the "GPS · N
    // fixes" pill keeps reassuring the runner while fixes have silently stopped.
    val lifecycleOwner = LocalLifecycleOwner.current
    DisposableEffect(lifecycleOwner) {
        mapView.overlays.add(casing)
        mapView.overlays.add(track)
        // A user drag (not a programmatic setCenter) means they want to look
        // around: stop yanking the map back.
        mapView.setOnTouchListener { v, ev ->
            v.parent?.requestDisallowInterceptTouchEvent(true)
            if (ev.actionMasked == MotionEvent.ACTION_MOVE) follow = false
            false
        }
        val observer = LifecycleEventObserver { _, event ->
            when (event) {
                Lifecycle.Event.ON_RESUME -> {
                    mapView.onResume()
                    hasPermission = hasFineLocation(ctx)
                    notifGranted = notificationsAllowed(ctx)
                    locationOn = isLocationEnabled(ctx)
                }
                Lifecycle.Event.ON_PAUSE -> mapView.onPause()
                else -> {}
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        // Live detection of the system location toggle changing mid-run.
        val providerReceiver = object : BroadcastReceiver() {
            override fun onReceive(c: Context?, i: Intent?) {
                locationOn = isLocationEnabled(ctx)
                hasPermission = hasFineLocation(ctx)
            }
        }
        ContextCompat.registerReceiver(
            ctx,
            providerReceiver,
            IntentFilter(LocationManager.PROVIDERS_CHANGED_ACTION),
            ContextCompat.RECEIVER_NOT_EXPORTED,
        )
        onDispose {
            lifecycleOwner.lifecycle.removeObserver(observer)
            runCatching { ctx.unregisterReceiver(providerReceiver) }
            mapView.setOnTouchListener(null)
            mapView.onPause()
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

    // Phase 4 / M4 explicit Start: opening the tracker starts the service in
    // LOCATE mode; it acquires GPS to preview fix quality but records NOTHING
    // until Start. Recording (tracking) or a pending exit suppresses it.
    LaunchedEffect(hasPermission, locationOn, tracking, locating, exiting, unsavedCapture) {
        if (hasPermission && locationOn && !tracking && !locating && !exiting && !unsavedCapture) {
            RunTrackingService.locate(ctx)
        }
    }

    // Self-location dot before recording: the live route dot follows recorded
    // points; while only locating (no recorded points yet) it follows the preview
    // fix so the runner sees where they are before pressing Start.
    LaunchedEffect(lastFix, tracking, follow) {
        if (!tracking && points.isEmpty()) {
            lastFix?.let { fix ->
                val gp = GeoPoint(fix.lat, fix.lon)
                locationDot.position = gp
                if (!mapView.overlays.contains(locationDot)) mapView.overlays.add(locationDot)
                if (follow) mapView.controller.setCenter(gp)
                mapView.invalidate()
            }
        }
    }

    // Extend the polyline with only the NEW fixes each time instead of remapping
    // + rebuilding the whole track every second (that was O(n) per fix → O(n²)
    // over a long run). `drawn` tracks how many points are already on the
    // overlay; a reset (points shrank, e.g. Start pressed again) clears and
    // redraws from scratch. A one-shot recovery repopulates a full track: that
    // single O(n) pass is fine.
    var drawn by remember { mutableStateOf(0) }
    LaunchedEffect(points, follow) {
        if (points.size < drawn) {
            casing.setPoints(emptyList())
            track.setPoints(emptyList())
            drawn = 0
        }
        for (i in drawn until points.size) {
            val gp = GeoPoint(points[i].lat, points[i].lon)
            casing.addPoint(gp)
            track.addPoint(gp)
        }
        drawn = points.size
        points.firstOrNull()?.let { first ->
            startRing.position = GeoPoint(first.lat, first.lon)
            if (!mapView.overlays.contains(startRing)) mapView.overlays.add(startRing)
        }
        points.lastOrNull()?.let { last ->
            val gp = GeoPoint(last.lat, last.lon)
            locationDot.position = gp
            if (!mapView.overlays.contains(locationDot)) mapView.overlays.add(locationDot)
            if (follow) mapView.controller.setCenter(gp)
        }
        mapView.invalidate()
    }

    fun startTracking() {
        if (!hasPermission) {
            requestPermissions()
            return
        }
        // Refuse-guard (C5, defense in depth): never begin recording while an
        // unsaved capture is held. `ActiveRunStore.begin()` truncates the crash
        // sidecar and this fn deliberately doesn't reset(), so appending onto an old
        // stopped track would merge two runs and lose the sidecar. The derived
        // `unsavedCapture` already keeps Start off the control row: this backstops a
        // future UI regression so it can still never happen.
        if (RunSession.points.value.isNotEmpty() && !RunSession.tracking.value) return
        locationOn = isLocationEnabled(ctx)
        // The service was already LOCATE-ing (no recorded points yet), so DON'T
        // reset() here: that would clear the preview lock and race the locate
        // restart effect. Just clear any stale pause and open a fresh crash-durable
        // sidecar BEFORE recording starts, so every accepted fix is persisted from
        // the first one, a process/service kill is then recoverable on next launch.
        RunSession.paused.value = false
        ActiveRunStore.begin(ctx)
        RunTrackingService.start(ctx)
    }

    // Discard the current session (Phase 4 / M4): stop the service, delete the
    // crash-recovery sidecar, and leave without logging anything.
    fun discardRun() {
        exiting = true
        RunTrackingService.stop(ctx)
        ActiveRunStore.clear()
        RunSession.reset()
        onFinish(null)
    }

    // Guards a double Stop tap while the coroutine below awaits the service's
    // confirmation, a second pass would log the same run twice.
    var stopping by remember { mutableStateOf(false) }

    // Log the currently-captured track (RunSession.trackForCore) to the core, drop
    // the sidecar, and leave. Off the main thread, LogRunTrack pushes thousands of
    // points through JNI + view(). Always sources the live RunSession track, so no
    // remembered copy can go stale across a rotation between capture and retry.
    fun logCaptured() {
        // Guard against a double-log / discard-mid-save race: the Save/Discard
        // control buttons are gated on `!stopping`, so flipping it true up-front
        // (for EVERY caller, the Keep button, the auto-log-on-stop path, and the
        // Save-retry) disables them while this send is in flight (a big track
        // through JNI takes tens–hundreds of ms). A second "Save" tap can't
        // double-log, and a "Discard" tap can't race the in-flight send into a
        // degenerate empty-track row. Idempotent: stopAndLog already sets it true
        // before calling here. Reset to false only on failure; success navigates
        // away via onFinish.
        stopping = true
        scope.launch {
            try {
                // M1: send→clearSync→reset is a CRITICAL SECTION that must run to
                // completion once the persist begins, even if this job's scope
                // (`rememberCoroutineScope`, tied to composition) is cancelled by a
                // rotation mid-save. The JNI `Core.send` is non-cancellable and has
                // already APPENDED the run to the event log by the time a plain
                // withContext would resume; a rotation there would rethrow
                // CancellationException, skip clear/reset, and (via the catch below)
                // flip `stopping` back and re-show the Save/Discard row for an
                // already-logged run → duplicate on retry / phantom on discard.
                // NonCancellable defers the cancellation until AFTER send+clear+reset
                // (and the navigation) complete. clearSync orders the sidecar delete
                // against the just-returned event-log append (LOW: a crash in that
                // gap would otherwise resurrect the saved run via the B4 prompt).
                val vm = withContext(Dispatchers.IO + NonCancellable) {
                    // Date the saved run at its LAST GPS fix, NOT "now": a
                    // crash-recovery save can run hours after the run actually
                    // happened, and the of-record History day / weekly-km window /
                    // acute-load spike window must land on WHEN the run occurred, not
                    // when it was persisted. Mirrors the GPX import path, which stamps
                    // observedAt = last fix (Gpx.kt). Falls back to now only for the
                    // impossible-here empty track (this send is gated on ≥2 points).
                    val runObservedAt = RunSession.points.value.lastOrNull()?.observedAt
                        ?: (System.currentTimeMillis() / 1000)
                    val result = Core.send(
                        // No paired HR sensor yet, and the spike baseline is derived
                        // in-core from prior runs; the shell sends neither figure.
                        // decimatedTrackForCore() sends the TRUE coordinates thinned
                        // to ~TRACK_DECIMATION_CAP fixes (endpoints + every pause
                        // boundary always kept) so one saved run can't bloat the
                        // append-only log; the paired decimatedSegmentStarts() tells
                        // the core where the pauses are (B2/I15) so it excludes each
                        // pause-bridge leg itself and breaks the GPX <trkseg> there -
                        // the of-record figures move negligibly (running.rs tests).
                        Event.LogRunTrack(
                            points = RunSession.decimatedTrackForCore(),
                            hrPctMax = 0.0,
                            longestRecentKm = 0.0,
                            observedAt = runObservedAt,
                            // I16 v1: the live tracker collects no run-type label, so
                            // a GPS save is untagged. A tracked run also derives its
                            // measured INTERVAL verdict, which the RunCard prefers over
                            // a user tag anyway, never a fabricated label here.
                            workoutType = null,
                            segmentStarts = RunSession.decimatedSegmentStarts(),
                        )
                    )
                    // Ordered against the append above (see clearSync), then clear the
                    // in-memory session: both inside NonCancellable so a cancellation
                    // can't tear them off a persisted run.
                    ActiveRunStore.clearSync()
                    RunSession.reset()
                    result
                }
                // Navigate away non-cancellably too, so the happy path always leaves
                // the tracker after a successful save; reset() already emptied the
                // session so even a skipped onFinish leaves a clean (non-unsaved) screen.
                withContext(NonCancellable) { onFinish(vm) }
            } catch (e: CancellationException) {
                // NEVER treat a cancellation as a failed save: the NonCancellable
                // block above already persisted, cleared, and reset. Re-throw so it
                // is not swallowed by the catch below (which would flip `stopping`
                // and re-show Save/Discard for a run that is already in the log).
                throw e
            } catch (e: Exception) {
                // Save failed: KEEP the captured run + its crash-recovery sidecar and
                // surface a Save/Discard affordance (C5): never drop to Start (which
                // would truncate the sidecar) and never lose the run. The captured
                // fixes stay in RunSession.points, so the derived `unsavedCapture`
                // holds the Save/Discard row up without any remembered state.
                stopping = false
                Toast.makeText(ctx, "Couldn't save run - tap Save to retry", Toast.LENGTH_SHORT).show()
            }
        }
    }

    fun stopAndLog() {
        if (stopping) return
        stopping = true
        // Committing to end the session: suppress the locate-restart effect so the
        // service doesn't re-acquire while we finish / prompt.
        exiting = true
        scope.launch {
            try {
                RunTrackingService.stop(ctx)
                // Snapshot the track only AFTER the service confirms the engine is
                // unregistered: the stop intent is asynchronous, so a snapshot taken
                // immediately would drop tail fixes still in flight.
                withTimeoutOrNull(2_000L) { RunSession.tracking.first { !it } }
                val captured = RunSession.points.value
                if (captured.size < 2) {
                    Toast.makeText(ctx, "Run too short to log", Toast.LENGTH_SHORT).show()
                    ActiveRunStore.clear()
                    RunSession.reset()
                    onFinish(null)
                    return@launch
                }
                // Plausibility gate (Phase 4 / M4): a sub-threshold run (drift, an
                // accidental record) prompts keep/discard before it pollutes the
                // log. `distanceKm` is the shell-side live haversine; the service is
                // stopped so it won't change while the dialog is up. Both a too-short
                // DISTANCE and a too-short DURATION trigger the prompt (A/C prongs).
                val km = RunSession.distanceKm.value
                // Gate on the MONOTONIC MOVING-time clock ([RunSession.elapsedSec]),
                // NOT a first→last wall-clock span: the span counts paused/standing
                // time too, so a sub-3-min MOVING run padded by a ≥3-min pause would
                // slip past this keep/discard prompt and silently pollute weekly km /
                // spikes. elapsedSec is exactly the of-record moving duration the core
                // will store (pause-bridge + sub-floor legs excluded), so it is the
                // honest measure of "too short to keep".
                val durationSec = RunSession.elapsedSec.value
                if (km < MIN_RUN_KM || durationSec < MIN_RUN_SEC) {
                    stopping = false
                    shortRunKm = km
                    return@launch
                }
                logCaptured()
            } catch (e: CancellationException) {
                // A rotation cancelling the snapshot wait is not a save failure -
                // nothing was persisted here (Core.send lives in logCaptured). The
                // captured fixes stay in RunSession.points, so on recomposition the
                // derived `unsavedCapture` re-shows Save/Discard (C5). Re-throw rather
                // than fall into the failure toast below.
                throw e
            } catch (e: Exception) {
                // Stop/snapshot failed before logging: don't brick or lose the run.
                // The service is already stopped and the captured fixes stay in
                // RunSession.points, so the derived `unsavedCapture` shows a
                // Save/Discard affordance (C5): Start (which truncates the sidecar)
                // is never what appears. Nothing to remember.
                stopping = false
                Toast.makeText(ctx, "Couldn't save run - tap Save to retry", Toast.LENGTH_SHORT).show()
            }
        }
    }

    // Back gesture keeps recording and returns to the app (06 spec §Behavior);
    // only a non-tracking exit (incl. locate-only preview) tears the session down.
    BackHandler {
        when {
            // A captured-but-unsaved run (failed save / dismissed short-run prompt):
            // leave WITHOUT clearing the sidecar so the run stays recoverable on next
            // launch: never silently drop the track the save path promised to keep (C5).
            unsavedCapture -> {}
            !tracking -> {
                exiting = true
                RunTrackingService.stop(ctx)
                ActiveRunStore.clear()
                RunSession.reset()
            }
        }
        onFinish(null)
    }

    // Live factual progress from the raw fixes (allowed shell-side): elapsed is the
    // monotonic recording span (RunSession.elapsedSec, collected above). Distance is
    // the incrementally-accumulated liveKm (RunSession.distanceKm); pace is the
    // average over that span. Of-record values still come from the core on save.
    // Canonical average pace (min/km); converted to the resolved unit at render.
    val avgPaceMinPerKm = if (liveKm >= 0.05 && elapsedSec > 0) elapsedSec / 60.0 / liveKm else Double.NaN
    val paceText = if (avgPaceMinPerKm.isFinite()) formatPaceMinutes(paceInUnit(avgPaceMinPerKm, unit)) else "-"

    // Per-N-minute pace slices (RunSession.paceBuckets is O(n); recomputed on each
    // new fix, keyed on the fix list + the live bucket size). Canonical min/km -
    // the unit conversion is applied per-cell at render, so a unit flip needs no
    // recompute. Only the most recent few are shown (see the primary row below).
    val paceBuckets = remember(points, paceBucketN) { RunSession.paceBuckets(paceBucketN) }

    // Banner action state: the fallback clear-readiness confirm (chrome §6)
    // and the Add-details readiness sheet, both reachable DURING a run so the
    // banner carries its full content here too (chrome §5 / INVARIANT 3).
    var confirmReadiness by remember { mutableStateOf(false) }
    var confirmRemovePain by remember { mutableStateOf(false) }
    var showReadinessSheet by remember { mutableStateOf(false) }
    ClearConfirmDialog(
        visible = confirmReadiness,
        title = "Clear readiness inputs?",
        message = "This clears today's readiness inputs and every adjustment they produced - including any safety hold that blocks training. Re-log your readiness to restore it.",
        confirmLabel = "Clear",
        onDismiss = { confirmReadiness = false },
        onClear = { onEvent(Event.ClearReadiness) },
    )
    // Phase 1: removing a pain hold confirms first, symmetric with triage.
    ClearConfirmDialog(
        visible = confirmRemovePain,
        title = "Remove the pain report?",
        message = "Only do this if it was logged by mistake. Removing it lifts the training hold.",
        confirmLabel = "Remove",
        onDismiss = { confirmRemovePain = false },
        onClear = { onEvent(Event.RemoveReadiness(ReadinessSignal.Pain)) },
    )
    // Phase 4 / M4: a sub-threshold run prompts keep/discard before it is logged.
    shortRunKm?.let { km ->
        val status = LocalStatusColors.current
        androidx.compose.material3.AlertDialog(
            // Outside-tap / back dismiss must NOT drop to the Start UI (which would
            // truncate the sidecar). The service is already stopped and the fixes stay
            // in RunSession.points, so clearing the prompt leaves the derived
            // `unsavedCapture` true: the Save/Discard affordance stays (C5).
            onDismissRequest = {
                shortRunKm = null
            },
            shape = RoundedCornerShape(Space.Card.dp),
            containerColor = BgElevated,
            title = { Text("Only ${String.format(Locale.US, "%.2f", km)} km") },
            text = {
                Text(
                    "That's a very short run - keep it in your history, or discard it? Discarding logs nothing.",
                    color = OnBgMuted,
                    style = Type.Body,
                )
            },
            confirmButton = {
                androidx.compose.material3.TextButton(onClick = {
                    shortRunKm = null
                    // The service is stopped, so the captured track is still in
                    // RunSession.points: log it.
                    logCaptured()
                }) { Text("Keep") }
            },
            dismissButton = {
                androidx.compose.material3.TextButton(onClick = {
                    shortRunKm = null
                    discardRun()
                }) { Text("Discard", color = status.danger) }
            },
        )
    }

    Column(Modifier.fillMaxSize().background(BgTop).statusBarsPadding()) {
        // Safety banner pinned above the map (INVARIANT 3): during tracking it
        // never yields to the map: the banner keeps its intrinsic height (all
        // content + the undo line fully visible) and the map takes what's left.
        // Renders nothing when no tier is active.
        SafetyBanner(
            model,
            Modifier
                .padding(horizontal = Space.Screen.dp)
                .padding(top = Space.Md.dp, bottom = Space.Md.dp),
            holdDetail = painSubline(model, null),
            onClearReadiness = { confirmReadiness = true },
            onRemovePain = { confirmRemovePain = true },
            onAddDetails = { showReadinessSheet = true },
        )
        // Map fills the frame; overlays: attribution/GPS pill bottom-left,
        // Recenter bottom-right. clipToBounds is load-bearing: osmdroid's
        // MapView paints tiles OUTSIDE its interop slot (Compose's view holder
        // doesn't clip), which over-painted the safety banner's action/undo
        // rows above the map: the banner must keep its intrinsic height with
        // every line visible while the map takes only the remaining space.
        Box(Modifier.weight(1f).fillMaxWidth().clipToBounds()) {
            AndroidView(factory = { mapView }, modifier = Modifier.fillMaxSize().clipToBounds())
            // Attribution + GPS/fix status. Before Start: "Acquiring GPS…" (warn)
            // until a usable fix lands, then "GPS lock · ±N m". While recording:
            // the live fix count. Never a fake position.
            val acquiring = (tracking && points.isEmpty()) || (!tracking && !gpsLocked)
            Row(
                modifier = Modifier
                    .align(Alignment.BottomStart)
                    .padding(Space.Md.dp)
                    .clip(RoundedCornerShape(100))
                    .background(if (acquiring) status.warn else Color.Black.copy(alpha = 0.4f))
                    .padding(horizontal = Space.Md.dp + Space.Xs.dp, vertical = Space.Sm.dp),
                horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
            ) {
                Text("© OpenStreetMap", color = Color.White.copy(alpha = 0.85f), style = Type.Caption)
                when {
                    tracking -> Text(
                        if (paused) "Paused · ${points.size} fixes" else "GPS · ${points.size} fixes",
                        color = Color.White,
                        style = Type.Caption.merge(TabularFigures),
                    )
                    gpsLocked -> Text(
                        "GPS lock · ±${lastFix?.accuracyM?.let { Math.round(it) } ?: 0} m",
                        color = Color.White,
                        style = Type.Caption.merge(TabularFigures),
                    )
                    else -> Text("Acquiring GPS…", color = Color.White, style = Type.Caption)
                }
            }
            // Recenter, 44dp circular button overlaying the map bottom-right,
            // above the sheet edge (separate from the sheet's control row).
            Box(
                modifier = Modifier
                    .align(Alignment.BottomEnd)
                    .padding(Space.Md.dp)
                    .size(44.dp)
                    .clip(RoundedCornerShape(100))
                    .background(BgElevated)
                    .border(1.dp, OnBgBody.copy(alpha = 0.07f), RoundedCornerShape(100))
                    .clickable {
                        follow = true
                        points.lastOrNull()?.let {
                            mapView.controller.animateTo(GeoPoint(it.lat, it.lon))
                        }
                    },
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    painterResource(R.drawable.ic_track_recenter),
                    contentDescription = "Recenter",
                    tint = OnBgBody,
                    modifier = Modifier.size(20.dp),
                )
            }
        }
        // Stat sheet, pinned bottom, BgTop, rounded top corners, the one other
        // sanctioned shadow besides the FAB. navigationBarsPadding keeps the
        // control row clear of the system gesture strip (edge-to-edge window).
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .shadow(16.dp, RoundedCornerShape(topStart = 26.dp, topEnd = 26.dp))
                .background(BgTop, RoundedCornerShape(topStart = 26.dp, topEnd = 26.dp))
                .navigationBarsPadding()
                .padding(horizontal = 18.dp)
                .padding(top = Space.Md.dp, bottom = 20.dp),
            verticalArrangement = Arrangement.spacedBy(Space.Card.dp),
        ) {
            // Drag handle.
            Box(
                Modifier
                    .align(Alignment.CenterHorizontally)
                    .width(38.dp)
                    .height(5.dp)
                    .clip(RoundedCornerShape(100))
                    .background(OnBgBody.copy(alpha = 0.22f)),
            )
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
                else -> {
                    // Primary row: ELAPSED (40sp) left · [pace buckets] · PACE right.
                    // ELAPSED and PACE are UNWEIGHTED (measured at intrinsic size, so
                    // both stay fully readable); the buckets live in the WEIGHTED middle
                    // that takes only leftover width and clips. Older buckets can never
                    // reach ELAPSED: the middle box starts to its right and clips its
                    // own overflow (aligned to the right, nearest PACE).
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
                        verticalAlignment = Alignment.Bottom,
                    ) {
                        Column(verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
                            TileOverline("Elapsed")
                            Text(
                                formatElapsed(elapsedSec),
                                color = OnBgBody,
                                style = Type.Display.copy(fontSize = 40.sp, fontWeight = FontWeight.ExtraBold)
                                    .merge(TabularFigures),
                            )
                        }
                        // Pace buckets: the LATEST slices, chronological (oldest→newest,
                        // newest rightmost adjacent to the overall PACE: the run's recent
                        // pace history flowing into its running average). Bounded to the
                        // last few and clipped so the region never grows into ELAPSED.
                        PaceBucketStrip(
                            buckets = paceBuckets,
                            bucketMinutes = paceBucketN,
                            unit = unit,
                            modifier = Modifier.weight(1f),
                        )
                        Column(
                            horizontalAlignment = Alignment.End,
                            verticalArrangement = Arrangement.spacedBy(Space.Xs.dp),
                        ) {
                            TileOverline("Pace")
                            Row(verticalAlignment = Alignment.Bottom) {
                                Text(
                                    paceText,
                                    color = OnBgBody,
                                    style = Type.Display.copy(fontSize = 26.sp, fontWeight = FontWeight.ExtraBold)
                                        .merge(TabularFigures),
                                )
                                Text(" ${unit.paceSuffix}", color = OnBgFaint, style = Type.Caption)
                            }
                        }
                    }
                    // Secondary row: Distance tile + HR/zone tile (wider).
                    Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                        Column(
                            modifier = Modifier
                                .weight(1f)
                                .clip(RoundedCornerShape(12.dp))
                                .background(BgElevated)
                                .padding(Space.Card.dp),
                            verticalArrangement = Arrangement.spacedBy(Space.Xs.dp),
                        ) {
                            Text(
                                if (liveKm > 0.0) {
                                    String.format(Locale.US, "%.2f", metersToDisplay(liveKm * 1000.0, unit))
                                } else {
                                    "-"
                                },
                                color = OnBgBody,
                                style = Type.Title.copy(fontWeight = FontWeight.ExtraBold).merge(TabularFigures),
                            )
                            Text(unit.distanceLabel, color = OnBgFaint, style = Type.Caption)
                        }
                        Column(
                            modifier = Modifier
                                .weight(1.4f)
                                .clip(RoundedCornerShape(12.dp))
                                .background(BgElevated)
                                .padding(Space.Card.dp),
                            verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
                        ) {
                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                horizontalArrangement = Arrangement.SpaceBetween,
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                // No paired HR sensor: the unmeasured value is
                                // "-", never fabricated (honesty rule).
                                Text(
                                    "- bpm",
                                    color = OnBgFaint,
                                    style = Type.Title.copy(fontWeight = FontWeight.ExtraBold).merge(TabularFigures),
                                )
                                Text("Z -", color = OnBgFaint, style = Type.Chip)
                            }
                            Row(horizontalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
                                listOf(status.hrZone1, status.hrZone2, status.hrZone3).forEach { c ->
                                    Box(
                                        Modifier
                                            .weight(1f)
                                            .height(6.dp)
                                            .clip(RoundedCornerShape(2.dp))
                                            .background(c.copy(alpha = 0.25f)),
                                    )
                                }
                            }
                        }
                    }
                    // Degraded-state surfacing: location off = loud warn row;
                    // denied notifications = muted caption.
                    if (!locationOn) {
                        Text(
                            "Location services are off - no GPS fixes will be recorded. Enable location in system settings.",
                            color = Color.White,
                            style = Type.Body,
                            modifier = Modifier
                                .fillMaxWidth()
                                .clip(RoundedCornerShape(Space.Card.dp))
                                .background(status.warn)
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
                    // Control row, primary action outermost bottom-right.
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(12.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        if (unsavedCapture) {
                            // Captured-but-unsaved run (C5): the save failed or the
                            // short-run prompt was dismissed. Offer Discard + Save
                            // (retry), NEVER Start, which would truncate the sidecar.
                            Box(
                                modifier = Modifier
                                    .size(56.dp)
                                    .clip(RoundedCornerShape(100))
                                    .background(BgElevated)
                                    .border(1.dp, status.danger.copy(alpha = 0.5f), RoundedCornerShape(100))
                                    .clickable(enabled = !stopping) { discardRun() },
                                contentAlignment = Alignment.Center,
                            ) {
                                Icon(
                                    painterResource(R.drawable.ic_ui_close),
                                    contentDescription = "Discard run",
                                    tint = status.danger,
                                    modifier = Modifier.size(20.dp),
                                )
                            }
                            Row(
                                modifier = Modifier
                                    .weight(1f)
                                    .height(56.dp)
                                    .clip(RoundedCornerShape(100))
                                    .background(Accent)
                                    .clickable(enabled = !stopping) {
                                        // logCaptured() sets `stopping` itself now (guards
                                        // every caller against the double-log race).
                                        logCaptured()
                                    },
                                horizontalArrangement = Arrangement.Center,
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Text(
                                    "Save run",
                                    color = OnAccent,
                                    style = Type.Body.copy(fontWeight = FontWeight.ExtraBold),
                                )
                            }
                        } else if (!tracking) {
                            // Not recording (locate preview): Back circle + Start
                            // pill. Start is disabled until a usable GPS fix lands
                            // (Phase 4 / M4 explicit-Start-after-lock).
                            ControlCircle(R.drawable.ic_ui_close, "Back") {
                                exiting = true
                                RunTrackingService.stop(ctx)
                                ActiveRunStore.clear()
                                RunSession.reset()
                                onFinish(null)
                            }
                            Row(
                                modifier = Modifier
                                    .weight(1f)
                                    .height(56.dp)
                                    .clip(RoundedCornerShape(100))
                                    .background(if (gpsLocked) Accent else Accent.copy(alpha = 0.4f))
                                    .clickable(enabled = gpsLocked) { startTracking() },
                                horizontalArrangement = Arrangement.Center,
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Text(
                                    if (gpsLocked) "Start" else "Acquiring GPS…",
                                    color = OnAccent,
                                    style = Type.Body.copy(fontWeight = FontWeight.ExtraBold),
                                )
                            }
                        } else {
                            // Recording: Pause + Discard circles + Stop & save.
                            Box(
                                modifier = Modifier
                                    .size(56.dp)
                                    .clip(RoundedCornerShape(100))
                                    .background(if (paused) Accent else BgElevated)
                                    .border(1.dp, OnBgBody.copy(alpha = 0.07f), RoundedCornerShape(100))
                                    .clickable { RunSession.paused.value = !paused },
                                contentAlignment = Alignment.Center,
                            ) {
                                Icon(
                                    painterResource(R.drawable.ic_track_pause),
                                    contentDescription = if (paused) "Resume" else "Pause",
                                    tint = if (paused) OnAccent else OnBgBody,
                                    modifier = Modifier.size(22.dp),
                                )
                            }
                            // Discard: drop the run, delete the sidecar, log nothing.
                            Box(
                                modifier = Modifier
                                    .size(56.dp)
                                    .clip(RoundedCornerShape(100))
                                    .background(BgElevated)
                                    .border(1.dp, status.danger.copy(alpha = 0.5f), RoundedCornerShape(100))
                                    .clickable(enabled = !stopping) { discardRun() },
                                contentAlignment = Alignment.Center,
                            ) {
                                Icon(
                                    painterResource(R.drawable.ic_ui_close),
                                    contentDescription = "Discard run",
                                    tint = status.danger,
                                    modifier = Modifier.size(20.dp),
                                )
                            }
                            Row(
                                modifier = Modifier
                                    .weight(1f)
                                    .height(56.dp)
                                    .clip(RoundedCornerShape(100))
                                    .background(status.danger)
                                    .clickable(enabled = !stopping) { stopAndLog() },
                                horizontalArrangement = Arrangement.spacedBy(Space.Md.dp, Alignment.CenterHorizontally),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Icon(
                                    painterResource(R.drawable.ic_track_stop),
                                    contentDescription = null,
                                    tint = Color.White,
                                    modifier = Modifier.size(15.dp),
                                )
                                Text(
                                    "Stop & save",
                                    color = Color.White,
                                    style = Type.Body.copy(fontWeight = FontWeight.ExtraBold),
                                )
                            }
                        }
                    }
                }
            }
        }
    }

    // "Add details" during a run: the readiness editor in a bottom sheet, so
    // the banner's detail path works without leaving the live tracking screen.
    if (showReadinessSheet) {
        ModalBottomSheet(
            onDismissRequest = { showReadinessSheet = false },
            containerColor = BgElevated,
        ) {
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .verticalScroll(rememberScrollState())
                    .padding(horizontal = 18.dp)
                    .padding(bottom = Space.Lg.dp),
                verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
            ) {
                ReadinessEditor(
                    signalGroups = model.signal_groups.associate { it.signal to it.group },
                    onClose = { showReadinessSheet = false },
                ) { r ->
                    onEvent(r)
                    showReadinessSheet = false
                }
            }
        }
    }
}

/** A preview fix at or under this accuracy (m) counts as a usable GPS lock that
 *  enables Start (Phase 4 / M4). The live-fix plausibility gate already rejects
 *  fixes worse than ~50 m, so this is the "good enough to begin" line. */
private const val GPS_LOCK_ACCURACY_M = 50.0

/**
 * A captured-but-not-yet-saved run exists (C5) when the foreground service is no
 * longer recording yet raw fixes are still held in [RunSession]. The locate-only
 * preview records NO points: it only reads `lastFix`, so a stopped session with
 * a non-empty point list can ONLY be a captured track awaiting save (after a failed
 * `Core.send` or a dismissed short-run prompt), never a live locate preview.
 *
 * DERIVED (not `remember`ed) precisely so it survives configuration changes:
 * rotation or system-Back + reopen recreate the Composable and drop every
 * `remember`, but [RunSession] is a process-lifetime singleton whose `points`/
 * `tracking` persist. A volatile flag would read null after rotation → the control
 * row would fall to Back/Start, and Back silently clears the crash sidecar while
 * Start truncates it and merges the two tracks. Keying off this fn instead keeps
 * Save/Discard on screen no matter how many times the screen is recreated.
 */
internal fun hasUnsavedCapture(tracking: Boolean, pointCount: Int): Boolean =
    !tracking && pointCount > 0

/** Below this measured distance (km) a Stop & save prompts keep/discard, so a
 *  drift/accidental "run" doesn't silently pollute weekly km / spikes (M4). */
internal const val MIN_RUN_KM = 0.5

/** Below this measured duration (seconds) a Stop & save also prompts keep/discard
 *  - the migration-plan A/C duration prong: a ≥0.5 km blip logged over ~90 s is
 *  as likely to be noise as a sub-0.5 km one, so a run under ~3 min is gated too. */
internal const val MIN_RUN_SEC = 180L

/** 56dp circular secondary control on the stat sheet. */
@Composable
private fun ControlCircle(iconRes: Int, description: String, onClick: () -> Unit) {
    Box(
        modifier = Modifier
            .size(56.dp)
            .clip(RoundedCornerShape(100))
            .background(BgElevated)
            .border(1.dp, OnBgBody.copy(alpha = 0.07f), RoundedCornerShape(100))
            .clickable { onClick() },
        contentAlignment = Alignment.Center,
    ) {
        Icon(
            painterResource(iconRes),
            contentDescription = description,
            tint = OnBgBody,
            modifier = Modifier.size(22.dp),
        )
    }
}

/** At most this many pace buckets render at once, the rest clip off the left of
 *  the bucket strip so the strip can never grow into the ELAPSED number. */
private const val MAX_VISIBLE_BUCKETS = 4

/**
 * The recent per-N-minute pace slices, shown between ELAPSED and the overall PACE.
 * Rendered chronologically (oldest→newest, newest rightmost, next to PACE) so it
 * reads as the run's recent pace history flowing into its running average. Lives in
 * a weighted, right-aligned, CLIPPED box; the content is measured UNBOUNDED and
 * End-aligned (`wrapContentWidth(End, unbounded = true)`), so under width pressure
 * the NEWEST bucket stays fully visible next to PACE and it is the OLDEST that
 * spills off the LEFT edge and clips. (Without the unbounded measure, the plain Row
 * would squeeze its last-measured, newest, child first.) The wrapper still
 * reports at most the weighted leftover width, so the strip can never overrun the
 * ELAPSED readout. Only the latest [MAX_VISIBLE_BUCKETS] render at all. The
 * in-progress tail slice is dimmed so it never reads as a completed N-minute split.
 */
@Composable
private fun PaceBucketStrip(
    buckets: List<PaceBucket>,
    bucketMinutes: Int,
    unit: DistanceUnit,
    modifier: Modifier = Modifier,
) {
    Box(modifier.clipToBounds(), contentAlignment = Alignment.BottomEnd) {
        if (buckets.isEmpty()) return@Box // <N min of moving time yet → nothing to show
        val shown = buckets.takeLast(MAX_VISIBLE_BUCKETS)
        Column(
            // Measure at full intrinsic width (no squeeze), then pin the RIGHT
            // edge: the overflow, the oldest buckets, leaves off the left,
            // where clipToBounds cuts it. Reported size stays within the
            // weighted bounds, so ELAPSED is never encroached on.
            modifier = Modifier.wrapContentWidth(align = Alignment.End, unbounded = true),
            horizontalAlignment = Alignment.End,
            verticalArrangement = Arrangement.spacedBy(Space.Xs.dp),
        ) {
            TileOverline("$bucketMinutes-min")
            Row(
                horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
                verticalAlignment = Alignment.Bottom,
            ) {
                shown.forEach { b ->
                    Text(
                        formatPaceMinutes(paceInUnit(b.paceMinPerKm, unit)),
                        // Completed slice = full N minutes (muted); the in-progress
                        // tail is fainter so it never reads as a finished split.
                        color = if (b.complete) OnBgMuted else OnBgFaint,
                        maxLines = 1,
                        style = Type.Body.copy(fontWeight = FontWeight.Bold).merge(TabularFigures),
                    )
                }
            }
        }
    }
}

/** Great-circle distance between two consecutive fixes, km (haversine). The one
 *  place the segment maths lives, so incremental accumulation ([RunSession.add])
 *  and a full recompute ([haversineKm]) can never diverge. */
internal fun segmentKm(a: GpsPoint, b: GpsPoint): Double {
    val dLat = Math.toRadians(b.lat - a.lat)
    val dLon = Math.toRadians(b.lon - a.lon)
    val h = Math.sin(dLat / 2) * Math.sin(dLat / 2) +
        Math.cos(Math.toRadians(a.lat)) * Math.cos(Math.toRadians(b.lat)) *
        Math.sin(dLon / 2) * Math.sin(dLon / 2)
    return 2 * 6371.0088 * Math.asin(Math.sqrt(h))
}

/** Path length over the raw fixes, km (haversine). A factual live progress
 *  figure only, the of-record distance is derived in the core on save. Used for
 *  one-shot recomputes (recovery); the live path accumulates incrementally. */
internal fun haversineKm(points: List<GpsPoint>): Double {
    if (points.size < 2) return 0.0
    var total = 0.0
    for (i in 1 until points.size) total += segmentKm(points[i - 1], points[i])
    return total
}

/** `m:ss` per-km pace from a min/km figure; clamps degenerate values. */
internal fun formatPace(minPerKm: Double): String {
    if (minPerKm.isNaN() || minPerKm.isInfinite() || minPerKm <= 0.0 || minPerKm >= 60.0) return "-"
    val totalSec = Math.round(minPerKm * 60.0)
    return "%d:%02d".format(Locale.US, totalSec / 60, totalSec % 60)
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
