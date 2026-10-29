package app.milestone

import android.Manifest
import android.annotation.SuppressLint
import android.app.Activity
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.content.res.Resources
import android.location.LocationManager
import android.net.Uri
import android.os.Build
import android.os.PowerManager
import android.os.SystemClock
import android.provider.Settings
import android.widget.Toast
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import androidx.core.content.edit
import androidx.core.location.LocationManagerCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.DelicateCoroutinesApi
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull

/** How long Save waits for the service to confirm the location stream stopped before
 *  it snapshots the track; past it the run is kept unsaved (Save/Discard). */
private const val STOP_CONFIRM_TIMEOUT_MS = 2_000L

/** Elevation of the stat sheet, a sanctioned shadow (06-run-tracking §5). */
private val SheetElevation = 16.dp
private val SheetBottomPadding = 20.dp

// The stat sheet's drag handle.
private val DragHandleWidth = 38.dp
private val DragHandleHeight = 5.dp
private const val DRAG_HANDLE_ALPHA = 0.22f

/**
 * GPS run-tracking overlay (06-run-tracking): map fills the frame, stat sheet
 * pins to the bottom, safety banner (when active) pins topmost. The location
 * stream lives in [RunTrackingService] (foreground service) so recording
 * survives screen-off/backgrounding; on Save (Stop → Finish review → Save) the raw
 * fix list goes to the Rust core via [Event.LogRunTrack], where the OF-RECORD
 * distance / pace / zone / spike are all derived. The live elapsed / distance / pace
 * shown while recording are factual progress indicators computed from the raw fixes;
 * the only judgement is PACE NOW against the core's own planned pace range for a
 * followed plan. They are superseded by the core's derivation on save.
 *
 * [onLogRunManually] opens the manual run log; the location-denied panel offers it
 * as the way out when GPS can't be used (hidden when null).
 */
@OptIn(DelicateCoroutinesApi::class)
@Composable
fun RunTrackingScreen(
    model: ViewModel,
    onEvent: (Event) -> Unit = {},
    onFinish: (ViewModel?) -> Unit,
    onLogRunManually: (() -> Unit)? = null,
) {
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()

    // Follow mode keeps the map centred on the latest fix. A manual drag drops it
    // so the runner can inspect the route; the Recenter button re-arms it.
    var follow by remember { mutableStateOf(true) }

    // FINE only: GPS tracking is unusable on a coarse-only grant.
    var hasPermission by remember { mutableStateOf(hasFineLocation(ctx)) }
    var notifGranted by remember { mutableStateOf(notificationsAllowed(ctx)) }
    var locationOn by remember { mutableStateOf(isLocationEnabled(ctx)) }
    // Battery-optimization exemption: a non-exempt app can be Dozed/frozen on a long
    // background run, silently starving the location stream. Tracked so the tracker
    // can nudge the user to allow it; re-sampled on ON_RESUME so the caption clears
    // itself the moment the exemption is granted.
    var batteryUnrestricted by remember { mutableStateOf(isIgnoringBatteryOptimizations(ctx)) }
    val setupPrefs = remember { ctx.setupPrefs() }
    // Whether the system location prompt was ever shown, persisted: after a permanent
    // denial shouldShowRequestPermissionRationale is false exactly as before the first
    // ask, and only this tells the two apart. The rationale's Continue sets it and
    // opens the system prompt; saveable, so a rotation never re-fires the prompt.
    var permAsked by rememberSaveable { mutableStateOf(setupPrefs.getBoolean(KEY_LOCATION_ASKED, false)) }
    // Permanently denied ("don't ask again"): the system dialog won't show, so
    // re-launching it silently no-ops; the panel's allow button deep-links to app
    // settings instead, the only place the grant can then be restored. Asked before
    // and no rationale due means exactly that, so a later visit opens straight on it.
    // Saveable so a rotation while the permanent-denial state is showing keeps the
    // settings-deep-link guidance instead of flashing back to a plain allow button
    // that can't surface the system dialog.
    var permaDenied by rememberSaveable {
        mutableStateOf(permAsked && !hasFineLocation(ctx) && locationPromptBlocked(ctx))
    }
    // Notifications are asked once, at Start, after location is granted.
    var notifAsked by rememberSaveable { mutableStateOf(false) }
    val permLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions()
    ) { result ->
        hasPermission = result[Manifest.permission.ACCESS_FINE_LOCATION] == true
        permaDenied = !hasPermission && locationPromptBlocked(ctx)
        locationOn = isLocationEnabled(ctx)
    }
    val notifLauncher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {
        notifGranted = notificationsAllowed(ctx)
    }

    fun requestPermissions() {
        permAsked = true
        setupPrefs.edit { putBoolean(KEY_LOCATION_ASKED, true) }
        permLauncher.launch(
            arrayOf(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION),
        )
    }

    // Asked at the moment of Start, once location is granted, never up front: the
    // ongoing notification is the lock-screen view of a run in progress.
    fun requestNotificationsOnce() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU || notifGranted || notifAsked) return
        notifAsked = true
        notifLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
    }

    val tracking by RunSession.tracking.collectAsStateWithLifecycle()
    // Coarse "has any captured fix" signal: flips only empty↔non-empty, so a
    // recording run doesn't recompose this screen every fix. The map's collectors and
    // the save/recenter callbacks read RunSession.points.value where they need it.
    val hasPoints by RunSession.hasPoints.collectAsStateWithLifecycle()
    // Same coarse signal for the locate preview's fix, so Recenter only shows once
    // there is a position to centre on.
    val hasPreviewFix by remember {
        RunSession.lastFix.map { it != null }.distinctUntilChanged()
    }.collectAsStateWithLifecycle(initialValue = false)
    // While true the control row shows Save/Discard (never Start), and the
    // locate-restart effect and BackHandler stand down ([hasUnsavedCapture]).
    val unsavedCapture = hasUnsavedCapture(tracking, hasPoints)
    val paused by RunSession.paused.collectAsStateWithLifecycle()
    // Locate-only preview: the service acquires GPS on open but records nothing until
    // Start. The lock readout + preview dot are driven from RunSession.lastFix in the
    // pill leaf ([TrackerStatusPill]) and the map's preview collector.
    val locating by RunSession.locating.collectAsStateWithLifecycle()
    // Armed: Start was tapped before a lock; recording begins automatically at the first
    // qualifying fix (the service owns the transition). The freshness ticker that ages
    // the last fix (fresh-lock decay / signal-loss) lives in [TrackerStatusPill] so it
    // recomposes only the pill, not this whole screen.
    val armed by RunSession.armed.collectAsStateWithLifecycle()
    // The service could not start the location stream; the locate preview is not
    // re-launched until ON_RESUME, Try again or Start clears it.
    val locateFailed by RunSession.locationFailed.collectAsStateWithLifecycle()
    // A crash-recovered run resumed without a location grant: held as a capture until
    // precise location is allowed, then recording restarts (the effect below).
    val resumeWaiting by RunSession.resumeAwaitingLocation.collectAsStateWithLifecycle()
    // Profile "Show route maps" toggle: when off, no MapView is drawn (no OSM tile
    // egress); GPS recording is unaffected.
    val showRouteMaps by AppSettings.showRouteMaps.collectAsStateWithLifecycle()
    // Optional, stackable start gates, persisted in the [SETUP_PREFS] prefs (default
    // off). The UI holds the toggles so both the gate chips and the armed pill/control
    // text stay consistent with what the just-tapped Start requested.
    var preciseGate by remember { mutableStateOf(setupPrefs.getBoolean(KEY_START_PRECISE, false)) }
    var movingGate by remember { mutableStateOf(setupPrefs.getBoolean(KEY_START_MOVING, false)) }
    // Live-reactive display prefs (shell chrome, not coaching state): the pace-bucket
    // size and the distance/pace unit. Both drive the readouts below and update the
    // moment the user changes them in Profile.
    val paceBucketN by AppSettings.paceBucketMinutes.collectAsStateWithLifecycle()
    val unit = rememberDistanceUnit()
    // The armed → recording confirmation buzz lives in [RunTrackingService]
    // (beginRecording): it must reach a runner whose screen is already off, which a
    // View-bound haptic from this composable cannot.
    // Set the moment the user leaves (Discard / Back / after save) so the
    // locate-restart effect can't re-launch the service into a disposing screen.
    var exiting by remember { mutableStateOf(false) }
    // Whether the tracker is in the foreground. The locate-preview effect only
    // acquires GPS while true; ON_STOP flips it false and tears a locate-only
    // preview down so the GPS radio isn't left on in the background, ON_START
    // restores it and the effect re-acquires.
    var appForeground by remember { mutableStateOf(true) }
    // Finish review: Stop never saves in one tap. It pauses recording and opens a
    // review (summary + Resume / Save / Discard-with-confirm); the pure
    // [finishTransition] decides each step + side effect. Saveable so a rotation
    // mid-review keeps the sheet (the run stays paused underneath either way).
    var finishStep by rememberSaveable { mutableStateOf(FinishStep.Closed) }
    // Effort (CR-10) and run type picked on the Finish review; both optional and
    // threaded into the saved LogRunTrack. Saveable so a rotation mid-review keeps
    // the picks.
    var finishRpe by rememberSaveable { mutableStateOf<Int?>(null) }
    var finishType by rememberSaveable { mutableStateOf<WorkoutType?>(null) }

    // Today's open planned run. Before Start the runner sees its targets and chooses
    // whether to follow it (default on); Start hands it to [RunSession], where the
    // service's step guide and the live target line read it for the whole run.
    val planItem = remember(model) { plannedRunToday(model) }
    var followPlan by rememberSaveable { mutableStateOf(true) }
    val plannedRun by RunSession.plannedRun.collectAsStateWithLifecycle()
    val stepProgress by RunSession.stepProgress.collectAsStateWithLifecycle()
    val paceTarget = plannedRun?.run_target?.takeIf { paceTargetApplies(stepProgress) }
    // A single-step planned run's distance, for the live "X to go" line.
    val planKm = remember(plannedRun) { plannedRun?.let { singleStepPlanKm(it) } }

    // Profile "Keep screen on" (default off): hold the screen awake while a run is
    // armed or recording, for a mounted or handheld phone. Restored on leave.
    val keepScreenOn by AppSettings.keepScreenOn.collectAsStateWithLifecycle()
    val hostView = LocalView.current
    DisposableEffect(hostView, keepScreenOn, tracking, armed) {
        val hold = keepScreenOn && (tracking || armed)
        val prev = hostView.keepScreenOn
        if (hold) hostView.keepScreenOn = true
        onDispose { if (hold) hostView.keepScreenOn = prev }
    }

    val map = rememberTrackerMap()

    // Bind the osmdroid MapView to the ACTIVITY lifecycle (not just composition):
    // resume/pause its tile threads with the app so it stops draining battery in
    // the background, and re-sample the location/permission/notification states on
    // every ON_RESUME. Mid-run the system location toggle can be flipped off or
    // the fine-location grant revoked in Settings; without this the "GPS · N
    // fixes" pill keeps reassuring the runner while fixes have silently stopped.
    val lifecycleOwner = LocalLifecycleOwner.current
    DisposableEffect(lifecycleOwner) {
        map.attach(onUserDrag = { follow = false })
        val observer = LifecycleEventObserver { _, event ->
            when (event) {
                Lifecycle.Event.ON_RESUME -> {
                    map.view.onResume()
                    hasPermission = hasFineLocation(ctx)
                    notifGranted = notificationsAllowed(ctx)
                    locationOn = isLocationEnabled(ctx)
                    // Re-sample after returning from the battery-optimization dialog /
                    // settings so the guidance caption disappears once exempted.
                    batteryUnrestricted = isIgnoringBatteryOptimizations(ctx)
                    // Back from settings (or the app switcher): one fresh locate attempt.
                    RunSession.locationFailed.value = false
                }
                Lifecycle.Event.ON_PAUSE -> map.view.onPause()
                Lifecycle.Event.ON_START -> appForeground = true
                Lifecycle.Event.ON_STOP -> {
                    appForeground = false
                    // Locate-only preview burns the GPS radio with no run in progress.
                    // Leaving the tracker's foreground (Home, backgrounding) with no
                    // recording/armed run and no captured track awaiting save stops the
                    // service so the radio isn't left on indefinitely; a live/armed run
                    // keeps its foreground service. Live RunSession reads, not the
                    // captured composition values the long-lived observer would stale.
                    if (!RunSession.tracking.value && !RunSession.armed.value &&
                        RunSession.points.value.isEmpty()
                    ) {
                        RunTrackingService.stop(ctx)
                    }
                }
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
            map.detach()
        }
    }

    // Before the first fix arrives, center on last-known location so the map
    // doesn't open on (0,0) in the Gulf of Guinea.
    LaunchedEffect(hasPermission) {
        if (hasPermission && RunSession.points.value.isEmpty()) {
            RunTrackingService.lastKnownLocation(ctx) { lat, lon ->
                if (RunSession.points.value.isEmpty()) map.centerOn(lat, lon)
            }
        }
    }

    // Explicit Start: opening the tracker starts the service in
    // LOCATE mode; it acquires GPS to preview fix quality but records NOTHING
    // until Start. Recording (tracking) or a pending exit suppresses it, and so does a
    // failed start ([locateFailed]) so a device that can't provide GPS isn't restarted
    // in a tight loop.
    LaunchedEffect(hasPermission, locationOn, tracking, locating, exiting, unsavedCapture, armed, appForeground, locateFailed) {
        if (hasPermission && locationOn && !tracking && !locating && !exiting && !unsavedCapture && !armed &&
            appForeground && !locateFailed
        ) {
            RunTrackingService.locate(ctx)
        }
    }

    // Resume after a location loss: the run was restored without recording because
    // the grant was missing. The moment it is back (the system prompt's result, or
    // ON_RESUME after app settings), re-issue the crash-resume recording start. The
    // flag stays set until the service reports recording, so a failed start
    // ([locateFailed]) is retried by Try again / the next ON_RESUME, and a Save or
    // Discard (`exiting`) stands it down.
    LaunchedEffect(resumeWaiting, hasPermission, locateFailed, exiting) {
        if (resumeWaiting && hasPermission && !locateFailed && !exiting) {
            RunTrackingService.start(ctx)
        }
    }
    LaunchedEffect(tracking) {
        if (tracking) RunSession.resumeAwaitingLocation.value = false
    }

    LaunchedEffect(map) { map.followPreviewFix { follow } }
    LaunchedEffect(map) { map.followTrack { follow } }

    fun armTracking() {
        if (!hasPermission) {
            requestPermissions()
            return
        }
        // Refuse-guard (defense in depth), two EXPLICIT cases. The service's
        // beginRecording() opens the sidecar and this fn deliberately doesn't
        // reset(), so arming onto a live run OR an earlier stopped track would risk
        // merging two runs. The derived `unsavedCapture` already keeps Start off the
        // control row; these backstop a future UI regression so it can still never
        // happen. Two checks, not one `isNotEmpty() && !tracking`: that would pass
        // while tracking, so a one-frame-stale click could demote a live run.
        if (RunSession.tracking.value) return          // never arm over a live recording
        if (RunSession.points.value.isNotEmpty()) return // never arm over an unsaved capture
        locationOn = isLocationEnabled(ctx)
        // A user Start is a fresh attempt even after a failed locate preview.
        RunSession.locationFailed.value = false
        // Armed start: Start is tappable before a lock. Recording begins automatically
        // at the first fix clearing the chosen gates; the SIDECAR is opened by the
        // service's beginRecording() at that first real fix (NOT here), so a crash while
        // merely armed leaves no sidecar and no false recovery prompt. Clear any stale
        // pause and optimistically publish `armed` so the control row flips instantly.
        RunSession.plannedRun.value = if (followPlan) planItem else null
        RunSession.setPaused(false)
        RunSession.armed.value = true
        RunTrackingService.arm(ctx, preciseGate, movingGate)
        requestNotificationsOnce()
    }

    // Leave the tracker with nothing to keep: stop the service, delete the
    // crash-recovery sidecar, clear the session and close the screen. The one
    // teardown every no-capture exit shares (Back, ✕ before Start, ✕ while armed,
    // a denied location, a confirmed discard). `exiting` first, so the locate-restart
    // effect can't re-launch the service into the disposing screen.
    fun leaveTracker() {
        exiting = true
        RunTrackingService.stop(ctx)
        ActiveRunStore.clear()
        RunSession.reset()
        onFinish(null)
    }

    // Discard the current session and leave without logging anything. Refused while
    // a save holds [RunSession.saving] (see [claimSaving]): the confirm dialog can
    // still fire after a save started.
    fun discardRun() {
        if (RunSession.saving.value) return
        leaveTracker()
    }

    // Save/Discard/Stop are disabled while a save holds [RunSession.saving]
    // (process-scoped, so a rotation mid-save neither re-enables nor strands them).
    val stopping by RunSession.saving.collectAsStateWithLifecycle()
    val runSavedText = stringResource(R.string.log_run_saved)
    val viewLabel = stringResource(R.string.import_view)
    val saveFailedText = stringResource(R.string.track_toast_save_failed)
    val notEnoughGpsText = stringResource(R.string.track_toast_not_enough_gps)

    // Log the captured track ([RunSession.trackEventForCore]) to the core, drop the
    // sidecar, and leave. Always reads the live RunSession track, so no remembered
    // copy can go stale across a rotation. [ownsSaving]: stopAndLog already holds the
    // [claimSaving] claim and hands it over.
    fun logCaptured(ownsSaving: Boolean = false, sessionRpe: Int? = finishRpe, workoutType: WorkoutType? = finishType) {
        if (!claimSaving(RunSession.saving, ownsSaving)) return
        // Committed to leaving: RunSession.reset() lands before onFinish, and the
        // locate-restart effect must not start a LOCATE service in that gap.
        exiting = true
        // Pinned to the main thread (a no-op on device, where the composition scope
        // already is): every resume after the IO persist (toast, navigation, the
        // saving flag) must land on main. A host whose effect dispatcher doesn't
        // dispatch (Compose's test environment) would otherwise continue on the IO
        // thread, where Toast throws and the save never finishes.
        scope.launch(Dispatchers.Main.immediate, start = CoroutineStart.ATOMIC) {
            // The saved run's identity, for the confirmation's "View".
            val entryId = EntryIds.next()
            var runObservedAt = 0L
            // The send→clearSync→reset(→onFinish) orchestration lives in the
            // pure-ish [runSaveCriticalSection] so it can be unit-tested against a
            // real coroutine cancellation (the JVM harness can drive that, the UI
            // can't). This call site only supplies the real effects; the exact
            // NonCancellable / re-throw semantics are enforced there.
            runSaveCriticalSection(
                // The non-cancellable JNI persist of [RunSession.trackEventForCore],
                // the very event the Finish review previewed its figures from.
                send = {
                    // Route the persist through the serial consumer as ONE exclusive
                    // turn (no queued event interleaves with it); the JNI itself still
                    // runs on CoreJniDispatcher. runSaveCriticalSection's NonCancellable
                    // wrapper protects the await, so a rotation can't cancel the save.
                    EventDispatcher.exclusive {
                        withContext(CoreJniDispatcher) {
                            // The run type and whole-session effort the runner picked on
                            // the Finish review (both optional, null = not given; never
                            // fabricated). A tracked run also derives its measured
                            // INTERVAL verdict on its own.
                            val event = RunSession.trackEventForCore(entryId, workoutType, sessionRpe)
                            runObservedAt = event.observedAt
                            val result = Core.send(event)
                            // A rejected/panicked send, or one accepted in memory whose
                            // log append failed (disk full), must not silently drop the
                            // captured run: throw so the critical section's onFailure
                            // keeps the track + crash sidecar and surfaces the
                            // save-failed toast, instead of resetting the session.
                            if (!result.accepted || !result.persisted) error("run save not persisted")
                            result.view
                        }
                    }!!
                },
                // clearSync orders the sidecar delete against the just-returned
                // event-log append (a crash in that gap would otherwise
                // resurrect the saved run via the recovery prompt).
                clearSidecar = { ActiveRunStore.clearSync() },
                resetSession = { RunSession.reset() },
                finish = { vm ->
                    // Confirm the save landed the way a manual save does, with a way to
                    // the run (it now has a route worth opening); the screen navigates away.
                    val savedAt = runObservedAt
                    UiMessages.post(
                        UiMessage(
                            text = runSavedText,
                            actionLabel = viewLabel,
                            onAction = { HistoryFocus.request(entryId, savedAt) },
                        ),
                    )
                    onFinish(vm)
                },
                onFailure = {
                    // Keep the captured run and its sidecar: the fixes stay in
                    // RunSession.points, so `unsavedCapture` re-shows Save/Discard.
                    RunSession.saving.value = false
                    exiting = false
                    Toast.makeText(ctx, saveFailedText, Toast.LENGTH_SHORT).show()
                },
            )
        }
    }

    fun stopAndLog(sessionRpe: Int?, workoutType: WorkoutType?) {
        // Claims [RunSession.saving] (see [claimSaving]) and hands it to
        // logCaptured(ownsSaving = true); every early exit below releases it.
        if (!claimSaving(RunSession.saving)) return
        // Committing to end the session: suppress the locate-restart effect so the
        // service doesn't re-acquire while we finish.
        exiting = true
        // ATOMIC start: a rotation can cancel `scope` before the first dispatch, and
        // a dropped body would strand the claim. Main-pinned like [logCaptured].
        scope.launch(Dispatchers.Main.immediate, start = CoroutineStart.ATOMIC) {
            try {
                RunTrackingService.stop(ctx)
                // Snapshot the track only AFTER the service confirms the engine is
                // unregistered: the stop intent is asynchronous, so a snapshot taken
                // immediately would drop tail fixes still in flight.
                val stopConfirmed = withTimeoutOrNull(STOP_CONFIRM_TIMEOUT_MS) { RunSession.tracking.first { !it } }
                if (stopConfirmed == null) {
                    // The engine may still be registered, so the track may be
                    // incomplete: keep the fixes, release the claim, save nothing.
                    RunSession.saving.value = false
                    Toast.makeText(ctx, saveFailedText, Toast.LENGTH_SHORT).show()
                    return@launch
                }
                val captured = RunSession.points.value
                if (captured.size < 2) {
                    Toast.makeText(ctx, notEnoughGpsText, Toast.LENGTH_LONG).show()
                    ActiveRunStore.clear()
                    RunSession.reset()
                    onFinish(null)
                    return@launch
                }
                // A short run was already flagged inline on the Finish review, so
                // Save here means keep it.
                logCaptured(ownsSaving = true, sessionRpe = sessionRpe, workoutType = workoutType)
            } catch (e: CancellationException) {
                // A rotation cancelled the stop-confirm wait: nothing was persisted, so
                // release the claim (else Save/Discard stay disabled) and re-throw.
                RunSession.saving.value = false
                throw e
            } catch (e: Exception) {
                // Stop/snapshot failed before logging: keep the fixes, so
                // `unsavedCapture` shows Save/Discard rather than Start.
                RunSession.saving.value = false
                Toast.makeText(ctx, saveFailedText, Toast.LENGTH_SHORT).show()
            }
        }
    }

    // Back gesture keeps recording and returns to the app (06 spec §Behavior). A
    // captured-but-unsaved run (failed save) leaves WITHOUT clearing the sidecar so it
    // stays recoverable on next launch; only an exit with nothing captured (incl. the
    // locate-only preview) tears the session down.
    BackHandler {
        if (tracking || unsavedCapture) onFinish(null) else leaveTracker()
    }

    // Perform one Finish-review action: the pure [finishTransition] picks the next step
    // and at most one side effect; this only applies it. Resume just unpauses the
    // still-running service (same sidecar, same run), Save hands off to the existing
    // stop → save path, Discard happens only from the confirm step.
    fun onFinishAction(action: FinishAction) {
        val t = finishTransition(finishStep, action)
        finishStep = t.step
        when (t.effect) {
            FinishEffect.None -> {}
            FinishEffect.PauseRecording -> {
                RunSession.setPaused(true)
                // Preselect the run type from the plan being followed (visible and
                // one tap to change or clear); a free run starts untagged.
                val followed = RunSession.plannedRun.value
                if (finishType == null && followed != null) {
                    finishType = workoutTypeForPlan(model.next_session?.session_type, followed.run_target?.band)
                }
            }
            FinishEffect.ResumeRecording -> RunSession.setPaused(false)
            FinishEffect.SaveRun -> stopAndLog(finishRpe, finishType)
            FinishEffect.DiscardRun -> discardRun()
        }
    }
    // No live recording behind the review (the save stopped it, or the service died):
    // nothing left to review, so close it.
    LaunchedEffect(tracking) { if (!tracking) finishStep = FinishStep.Closed }
    // While reviewing, Back cancels the Stop (Resume); it never strands a paused run.
    // Declared after the screen-level handler above so it takes precedence.
    BackHandler(enabled = tracking && finishStep == FinishStep.Review) {
        onFinishAction(FinishAction.Resume)
    }

    // Apply the user's chosen pace-bucket size to the RunSession accumulator (the live
    // slices themselves are read + rendered in [TrackerStats]). Changing the size
    // rebuilds the accumulator once for the new size.
    LaunchedEffect(paceBucketN) { RunSession.setPaceBucketMinutes(paceBucketN) }

    // The Add-details readiness sheet, reachable DURING a run from the banner so it
    // carries its full content here too (chrome §5 / INVARIANT 3).
    var showReadinessSheet by remember { mutableStateOf(false) }
    // Only the unsaved-capture discard routes through this confirm: the armed/locate
    // Cancel circles fire before ANY fix is recorded (points are still empty there),
    // so those stay single-tap, and a live recording discards from the Finish review
    // (its own confirm). Saveable so a rotation while the confirm is up doesn't
    // dismiss it silently: it gates a no-undo delete of a captured run.
    var confirmDiscard by rememberSaveable { mutableStateOf(false) }
    if (confirmDiscard) {
        DiscardRunDialog(
            enabled = !stopping,
            onCancel = { confirmDiscard = false },
            onConfirm = {
                confirmDiscard = false
                discardRun()
            },
        )
    }
    if (finishStep == FinishStep.ConfirmDiscard) {
        DiscardRunDialog(
            enabled = !stopping,
            onCancel = { onFinishAction(FinishAction.CancelDiscard) },
            onConfirm = { onFinishAction(FinishAction.ConfirmDiscard) },
        )
    }

    // The Finish review is open over a live (paused) recording.
    val reviewing = tracking && finishStep != FinishStep.Closed

    Column(Modifier.fillMaxSize().background(BgTop).statusBarsPadding()) {
        // Safety banner pinned above the map (INVARIANT 3): during tracking it
        // never yields to the map: the banner keeps its intrinsic height (all
        // content + the undo line fully visible) and the map takes what's left.
        // Renders nothing when no tier is active.
        SafetyBannerHost(
            model,
            onEvent = onEvent,
            modifier = Modifier
                .padding(horizontal = Space.Screen.dp)
                .padding(top = Space.Md.dp, bottom = Space.Md.dp),
            onAddDetails = { showReadinessSheet = true },
        )
        // Map fills the frame. During the Finish review the map collapses to zero
        // height (kept attached so osmdroid isn't torn down) and the review takes the
        // whole sheet, so its effort and type chips scroll above Save/Resume instead of
        // pushing them off screen. With route maps off the frame wraps just the status
        // pill and the stat sheet rises to take the freed height (the Profile toggle is
        // the one place that says maps are off).
        val mapFrame = when {
            reviewing -> Modifier.height(0.dp)
            showRouteMaps -> Modifier.weight(1f)
            else -> Modifier
        }
        // The map carries ONE human status a runner can act on; every other notice
        // sits in the sheet.
        TrackerMapFrame(
            map = map,
            showMap = showRouteMaps,
            showOverlays = !reviewing,
            located = hasPoints || hasPreviewFix,
            canRecenter = hasPermission && (hasPoints || hasPreviewFix),
            onRecenter = {
                follow = true
                map.recenter()
            },
            modifier = mapFrame,
        ) { pillModifier ->
            TrackerStatusPill(
                hasPermission = hasPermission,
                locationOn = locationOn,
                tracking = tracking,
                paused = paused,
                armed = armed,
                locating = locating,
                requirePrecise = preciseGate,
                requireMoving = movingGate,
                onRecheckPermission = { hasPermission = hasFineLocation(ctx) },
                permissionAsked = permAsked,
                modifier = pillModifier,
            )
        }
        // Stat sheet, pinned bottom, BgTop, rounded top corners. navigationBarsPadding
        // keeps the control row clear of the system gesture strip (edge-to-edge window).
        // It takes the leftover height whenever the map frame doesn't (Finish review,
        // route maps off); the control row then sits at its bottom edge.
        val sheetFills = reviewing || !showRouteMaps
        val sheetShape = RoundedCornerShape(topStart = SheetCornerRadius, topEnd = SheetCornerRadius)
        Column(
            modifier = (if (sheetFills) Modifier.weight(1f) else Modifier)
                .fillMaxWidth()
                .shadow(SheetElevation, sheetShape)
                .background(BgTop, sheetShape)
                .navigationBarsPadding()
                .padding(horizontal = Space.Sheet.dp)
                .padding(top = Space.Md.dp, bottom = SheetBottomPadding),
            verticalArrangement = Arrangement.spacedBy(Space.Card.dp),
        ) {
            DragHandle()
            if (!hasPermission) {
                val prompt = locationPrompt(permAsked, permaDenied, hasCoarseLocation(ctx))
                LocationPermissionPanel(
                    prompt = prompt,
                    // Once permanently denied ("don't ask again") the system dialog won't
                    // appear, so re-requesting silently no-ops; deep-link to app
                    // settings instead, the only place the grant can then be restored.
                    onAllow = { if (prompt == LocationPrompt.Blocked) openAppDetailsSettings(ctx) else requestPermissions() },
                    // A resumed run waiting on the grant holds recorded fixes: ✕ is a
                    // confirmed discard and Save keeps what was recorded, never a
                    // silent leave that deletes the run.
                    onClose = { if (resumeWaiting && unsavedCapture) confirmDiscard = true else leaveTracker() },
                    onLogManually = onLogRunManually?.let { open -> { leaveTracker(); open() } },
                    onSaveRun = if (resumeWaiting && unsavedCapture) ({ logCaptured() }) else null,
                )
            } else {
                // With the sheet filling the frame (route maps off), its content sits
                // on the control row in thumb reach, not stacked under the handle
                // above a half-empty screen.
                if (sheetFills && !reviewing) Spacer(Modifier.weight(1f))
                // Battery reliability (pre-recording only), at the top of the sheet. It
                // clears itself (ON_RESUME re-check) once the exemption is granted.
                if (!batteryUnrestricted && !tracking && !unsavedCapture) {
                    InlineNotice(
                        stringResource(R.string.track_battery_caption),
                        actionLabel = stringResource(R.string.track_battery_allow),
                        onAction = { requestIgnoreBatteryOptimizations(ctx) },
                    )
                }
                // Live stat readouts (MOVING · DISTANCE · PACE NOW, then Avg + last
                // slice), a leaf so the per-fix elapsed/distance/pace updates recompose
                // only those tiles, not the whole tracking screen. Only once there is
                // something to show: before recording (locate/armed) the figures would
                // be a frozen row of zeros and dashes. The Finish review carries its own
                // distance · time · pace summary, so the live tiles step aside while it
                // is open. Without a map they are the screen's hero, so they may grow.
                if (!reviewing && (tracking || unsavedCapture)) {
                    TrackerStats(
                        unit = unit,
                        paceBucketN = paceBucketN,
                        paused = paused,
                        large = !showRouteMaps,
                        paceTarget = if (tracking) paceTarget else null,
                        planKm = if (tracking) planKm else null,
                    )
                }
                // Following today's plan: the targets and the current step (with
                // Skip step) sit right under the live figures for the whole run.
                if (tracking && !reviewing) {
                    plannedRun?.let { item ->
                        RunGuideCard(
                            item = item,
                            progress = stepProgress,
                            unit = unit,
                            onSkip = { RunSession.skipStep(SystemClock.elapsedRealtime()) },
                        )
                    }
                }
                // Degraded-state surfacing: location off = loud warn row; a GPS
                // start failure / denied notifications = an inline notice. Each carries
                // an inline action so the state is actionable, not a dead end.
                if (!locationOn) LocationOffNotice(onOpenSettings = { openLocationSettings(ctx) })
                if (locateFailed && locationOn && !tracking && !armed && (!unsavedCapture || resumeWaiting)) {
                    InlineNotice(
                        stringResource(R.string.track_location_failed_caption),
                        actionLabel = stringResource(R.string.track_btn_try_again),
                        onAction = { RunSession.locationFailed.value = false },
                    )
                }
                // Only once a run is armed or recording: notifications are asked at
                // Start, so before it there is nothing to warn about.
                if (!notifGranted && (tracking || armed)) {
                    InlineNotice(
                        stringResource(R.string.track_notifications_off_caption),
                        actionLabel = stringResource(R.string.track_open_settings),
                        onAction = { openNotificationSettings(ctx) },
                    )
                }
                // Today's plan + optional start gates, ONLY in the locate-preview state
                // (not while armed/recording or with an unsaved capture).
                if (!tracking && !armed && !unsavedCapture) {
                    TrackerStartOptions(
                        planItem = planItem,
                        followPlan = followPlan,
                        onFollowPlan = { followPlan = it },
                        unit = unit,
                        preciseGate = preciseGate,
                        onTogglePrecise = {
                            preciseGate = !preciseGate
                            setupPrefs.edit { putBoolean(KEY_START_PRECISE, preciseGate) }
                        },
                        movingGate = movingGate,
                        onToggleMoving = {
                            movingGate = !movingGate
                            setupPrefs.edit { putBoolean(KEY_START_MOVING, movingGate) }
                        },
                    )
                }
                // Finish review (Stop was tapped; recording is paused underneath):
                // replaces the control row until the runner resumes, saves or
                // discards. Only over a live recording; a stopped capture keeps its
                // Save/Discard row below.
                if (reviewing) {
                    RunFinishPanel(
                        unit = unit,
                        enabled = !stopping,
                        sessionRpe = finishRpe,
                        onSessionRpe = { finishRpe = it },
                        workoutType = finishType,
                        onWorkoutType = { picked -> finishType = if (picked == finishType) null else picked },
                        onResume = { onFinishAction(FinishAction.Resume) },
                        onSave = { onFinishAction(FinishAction.Save) },
                        onDiscard = { onFinishAction(FinishAction.Discard) },
                        modifier = Modifier.weight(1f),
                    )
                } else {
                    TrackerControlRow(
                        unsavedCapture = unsavedCapture,
                        armed = armed,
                        tracking = tracking,
                        paused = paused,
                        stopping = stopping,
                        onDiscardCapture = { confirmDiscard = true },
                        // logCaptured() CAS-claims `saving` itself (guards every caller
                        // vs double-log).
                        onSaveCapture = { logCaptured() },
                        onLeave = { leaveTracker() },
                        onStartNow = { RunTrackingService.startNow(ctx) },
                        onStart = { armTracking() },
                        onPause = { RunSession.setPaused(true) },
                        onResume = { RunSession.setPaused(false) },
                        onStop = { onFinishAction(FinishAction.Stop) },
                    )
                }
            }
        }
    }

    if (showReadinessSheet) {
        TrackerReadinessSheet(model, onEvent, onDismiss = { showReadinessSheet = false })
    }
}

/** The stat sheet's drag handle. */
@Composable
private fun ColumnScope.DragHandle() {
    Box(
        Modifier
            .align(Alignment.CenterHorizontally)
            .width(DragHandleWidth)
            .height(DragHandleHeight)
            .clip(PillShape)
            .background(OnBgBody.copy(alpha = DRAG_HANDLE_ALPHA)),
    )
}

// Deep-links for the sheet's degraded-state actions. All wrapped so a device with no
// matching Settings activity can't crash the tracker.

/** The system location settings. */
private fun openLocationSettings(ctx: Context) {
    runCatching { ctx.startActivity(Intent(Settings.ACTION_LOCATION_SOURCE_SETTINGS)) }
}

/** This app's details page: the only place a permanently-denied grant is restored. */
private fun openAppDetailsSettings(ctx: Context) {
    runCatching { ctx.startActivity(appDetailsIntent(ctx)) }
}

/** This app's notification settings (its details page before API 26). */
private fun openNotificationSettings(ctx: Context) {
    runCatching { ctx.startActivity(appNotificationSettingsIntent(ctx)) }
}

/**
 * Ask the system to exempt this app from battery optimization. The direct
 * REQUEST_IGNORE_BATTERY_OPTIMIZATIONS dialog (needs the same-named permission,
 * declared in the manifest, fine for a sideloaded app) is preferred; if the OEM
 * rejects/omits it, fall back to the general battery-optimization settings list.
 */
@SuppressLint("BatteryLife")
private fun requestIgnoreBatteryOptimizations(ctx: Context) {
    val direct = runCatching {
        ctx.startActivity(
            Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS)
                .setData(Uri.fromParts("package", ctx.packageName, null)),
        )
    }.isSuccess
    if (!direct) {
        runCatching {
            ctx.startActivity(Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS))
        }
    }
}

/** The "Precise start" gate's accuracy bound (m): recording only begins on a fix at
 *  or under this. Typical post-lock consumer GPS is 5–15 m open sky, so 20 m is a
 *  trustworthy start anchor without being unachievable under light tree/building
 *  cover. Consumed by [ArmTrigger]'s precise gate (RunSession.kt). */
internal const val GPS_PRECISE_ACCURACY_M = 20.0

/** SharedPreferences keys (the [SETUP_PREFS] prefs, [KEY_PLAN_CLEARED] idiom) for
 *  the two optional, stackable start gates, both default false. */
private const val KEY_START_PRECISE = "run_start_precise"
private const val KEY_START_MOVING = "run_start_moving"
/** Set once the system location prompt has been shown (same prefs, default false). */
private const val KEY_LOCATION_ASKED = "run_location_asked"

/**
 * A captured run awaits save when the service no longer records yet fixes are still
 * held (a failed save or an unconfirmed stop); the locate preview records none.
 * Derived from the process-scoped [RunSession], not `remember`ed, so Save/Discard
 * survive rotation and Back+reopen instead of falling to Back (clears the sidecar)
 * or Start (truncates it).
 */
internal fun hasUnsavedCapture(tracking: Boolean, hasPoints: Boolean): Boolean =
    !tracking && hasPoints

/**
 * Claim [saving] for one save: the false→true compareAndSet admits exactly one caller
 * among a double Save tap, Save vs stopAndLog, and blocks a Discard under an in-flight
 * send. [ownsSaving]: the caller already holds the claim (stopAndLog → logCaptured).
 * Released by [RunSession.reset] or a save-failure path.
 */
internal fun claimSaving(saving: MutableStateFlow<Boolean>, ownsSaving: Boolean = false): Boolean =
    ownsSaving || saving.compareAndSet(expect = false, update = true)

/**
 * Save a captured run as one unit that survives the caller's scope being cancelled
 * (rotation): [send] persists (appends to the event log), then [clearSidecar] and
 * [resetSession] run in the same `NonCancellable` IO block, so a persisted run is
 * never left re-savable. [finish] (navigation) may be skipped on cancel, harmlessly.
 * A [CancellationException] is re-thrown, never sent to [onFailure]; any other
 * exception means [send] failed before clear/reset, with the run intact for a retry.
 */
internal suspend fun <T> runSaveCriticalSection(
    send: suspend () -> T,
    clearSidecar: () -> Unit,
    resetSession: () -> Unit,
    finish: (T) -> Unit,
    onFailure: () -> Unit,
) {
    try {
        val result = withContext(Dispatchers.IO + NonCancellable) {
            val r = send()
            clearSidecar()
            resetSession()
            r
        }
        withContext(NonCancellable) { finish(result) }
    } catch (e: CancellationException) {
        throw e
    } catch (e: Exception) {
        onFailure()
    }
}

/** Below this measured distance (km) the Finish review notes a short run, so a
 *  drift/accidental "run" is a conscious keep rather than silent weekly-km / spike
 *  pollution. */
internal const val MIN_RUN_KM = 0.5

/** Below this measured MOVING duration (seconds) the review notes a short run too:
 *  a ≥0.5 km blip logged over ~90 s is as likely to be noise as a sub-0.5 km one. */
internal const val MIN_RUN_SEC = 180L

/** True when a captured run is under the keep threshold on distance OR moving time
 *  ([RunSession.elapsedSec], which excludes pauses and standing still, so a short
 *  run padded by a long pause still counts as short). */
internal fun isShortRun(km: Double, movingSec: Long): Boolean =
    km < MIN_RUN_KM || movingSec < MIN_RUN_SEC

/** The keep threshold as the short-run note states it: a round figure in the user's
 *  unit ("0.3 mi", "0.5 km"), not a converted 500 m ("0.31 mi"); the check itself
 *  stays at [MIN_RUN_KM]. */
internal fun shortRunThresholdLabel(unit: DistanceUnit): String =
    "${trimDecimal(metersToDisplay(MIN_RUN_KM * 1000.0, unit))} ${unit.distanceLabel}"

/** Where the Finish review is: not shown, reviewing the paused run, or confirming a
 *  discard. An enum so `rememberSaveable` keeps it across rotation. */
internal enum class FinishStep { Closed, Review, ConfirmDiscard }

/** What the runner did on the recording controls / Finish review. */
internal enum class FinishAction { Stop, Resume, Save, Discard, CancelDiscard, ConfirmDiscard }

/** The single side effect a Finish transition asks the screen to perform. */
internal enum class FinishEffect { None, PauseRecording, ResumeRecording, SaveRun, DiscardRun }

internal data class FinishTransition(val step: FinishStep, val effect: FinishEffect)

/**
 * The Finish review as a pure state machine, so its guarantees are JVM-testable
 * without Compose:
 *  - Stop never saves: it pauses recording and opens the review.
 *  - Resume closes the review and un-pauses the SAME live run (no restart, no new
 *    crash-recovery sidecar; the service never stopped).
 *  - Save is the only path to the of-record save (the existing stop → persist
 *    pipeline; a short run is flagged inline on the review, never a second prompt).
 *  - Discard only ASKS; the run is deleted solely by ConfirmDiscard from the confirm
 *    step, and CancelDiscard returns to the review.
 * Any action that doesn't apply to the current step is a no-op (a stale tap after a
 * recomposition can't skip the confirm or double-fire an effect).
 */
internal fun finishTransition(step: FinishStep, action: FinishAction): FinishTransition = when {
    step == FinishStep.Closed && action == FinishAction.Stop ->
        FinishTransition(FinishStep.Review, FinishEffect.PauseRecording)
    step == FinishStep.Review && action == FinishAction.Resume ->
        FinishTransition(FinishStep.Closed, FinishEffect.ResumeRecording)
    step == FinishStep.Review && action == FinishAction.Save ->
        FinishTransition(FinishStep.Closed, FinishEffect.SaveRun)
    step == FinishStep.Review && action == FinishAction.Discard ->
        FinishTransition(FinishStep.ConfirmDiscard, FinishEffect.None)
    step == FinishStep.ConfirmDiscard && action == FinishAction.CancelDiscard ->
        FinishTransition(FinishStep.Review, FinishEffect.None)
    step == FinishStep.ConfirmDiscard && action == FinishAction.ConfirmDiscard ->
        FinishTransition(FinishStep.Closed, FinishEffect.DiscardRun)
    else -> FinishTransition(step, FinishEffect.None)
}

/**
 * Today's open planned run, or null: the next session is dated today, still
 * trainable (next/adjusted), a run session, and has an item not yet done. Prefers
 * the item carrying run targets/structure. Pure so the tracker's plan hand-off is
 * testable.
 */
internal fun plannedRunToday(model: ViewModel): PrescriptionView? {
    val ns = model.next_session ?: return null
    if (ns.epoch_day != model.today()) return null
    if (!ns.actionable) return null
    if (sessionDiscipline(ns.session_type) != "Run") return null
    val open = ns.items.filter { !it.done }
    return open.firstOrNull { it.run_target != null || it.run_structure != null } ?: open.firstOrNull()
}

/** The run-type chip preselected on the Finish review for a followed plan's
 *  training band (view_types.rs RunTargetView.band); null for a free run. */
internal fun workoutTypeForBand(band: String?): WorkoutType? = when (band) {
    null, "" -> null
    "recovery" -> WorkoutType.Recovery
    "easy" -> WorkoutType.Easy
    "threshold" -> WorkoutType.Tempo
    "interval", "repetition", "hill effort" -> WorkoutType.Interval
    else -> WorkoutType.Steady
}

/** The run-type chip preselected for a followed plan: the planned session's type
 *  when it names one of the chips (a long run's band is "easy", so the band alone
 *  would read Steady), else [workoutTypeForBand]. [sessionType] is the session's
 *  wire name (plan_views.rs `format!("{:?}")`, e.g. "Run(LongRun)"). */
internal fun workoutTypeForPlan(sessionType: String?, band: String?): WorkoutType? = when (sessionType) {
    "Run(LongRun)" -> WorkoutType.LongRun
    "Run(Recovery)" -> WorkoutType.Recovery
    "Run(Easy)" -> WorkoutType.Easy
    "Run(Tempo)" -> WorkoutType.Tempo
    "Run(Interval)", "Run(Repetition)", "Run(Hills)" -> WorkoutType.Interval
    else -> workoutTypeForBand(band)
}

/** The plan's targets as short lines: pace in the user's unit, heart rate, and the
 *  effort, each labelled like the Today run item ("Effort: 3–4 of 10"). Empty when
 *  the item carries no targets. */
internal fun runTargetLines(res: Resources, t: RunTargetView?, unit: DistanceUnit): List<String> {
    if (t == null) return emptyList()
    val out = ArrayList<String>(3)
    val pace = paceLabel(t, unit)
    if (pace.isNotBlank()) out += res.getString(R.string.track_target_pace, pace)
    if (t.hr_label.isNotBlank()) out += res.getString(R.string.track_target_hr, t.hr_label)
    if (t.effort_detail.isNotBlank()) out += res.getString(R.string.plan_face_effort, t.effort_detail)
    return out
}

/** The plan's targets folded into the one line shown while recording ("Plan
 *  9:00–9:50 /mi · ≤ 140 bpm · effort 3–5 of 10"), or null without targets. */
internal fun runTargetSummary(res: Resources, t: RunTargetView?, unit: DistanceUnit): String? {
    if (t == null) return null
    val parts = listOf(paceLabel(t, unit), t.hr_label, t.effort_cue).filter { it.isNotBlank() }
    if (parts.isEmpty()) return null
    return res.getString(R.string.track_plan_targets_line, parts.joinToString(" · "))
}

/** The target pace label in [unit], falling back to the other unit's label. */
private fun paceLabel(t: RunTargetView, unit: DistanceUnit): String =
    if (unit == DistanceUnit.Mi) t.pace_label_mi.ifBlank { t.pace_label_km } else t.pace_label_km.ifBlank { t.pace_label_mi }

/** The planned distance (km) of a SINGLE-STEP planned run (a continuous run: no
 *  warm-up / reps / strides guide), for the live "X to go" line; null for a guided
 *  run (its steps carry their own countdown) or a plan without a distance. */
internal fun singleStepPlanKm(item: PrescriptionView): Double? {
    val steps = item.run_structure?.let { buildRunSteps(it) }.orEmpty()
    if (steps.size > 1) return null
    return item.run_head_km?.takeIf { it > 0.0 }
}

/** The plan preview's headline: the item's name plus its leading distance, never the
 *  targets (those are the lines below it). A plain distance lead (`run_head_km`) is
 *  re-expressed in the user's unit; any other lead ("6 × 800 m") is kept verbatim. */
internal fun runPlanHeadline(item: PrescriptionView, unit: DistanceUnit): String {
    val km = item.run_head_km
    val leadShown = if (km != null) {
        "${trimDecimal(Math.round(metersToDisplay(km * 1000.0, unit) * 10.0) / 10.0)} ${unit.distanceLabel}"
    } else {
        item.face_lead
    }
    return listOf(item.exercise.trim(), leadShown)
        .filter { it.isNotBlank() }
        .distinct()
        .joinToString(" · ")
        .ifBlank { item.summary }
}

/** Where the live pace sits against a followed plan's pace range. */
internal enum class PaceCue { OnTarget, SpeedUp, EaseOff }

/**
 * [paceMinPerKm] (the "Pace now" figure, already smoothed over a ≥30 s / ≥200 m
 * rolling window by [RollingPace], so it doesn't flicker leg to leg) against the
 * plan's range in seconds per km (RunTargetView pace_fast/slow_sec_per_km, either
 * order). Null when the plan has no pace range or there is no current pace.
 */
internal fun paceCue(paceMinPerKm: Double, fastSecPerKm: Int, slowSecPerKm: Int): PaceCue? {
    if (!paceMinPerKm.isFinite() || paceMinPerKm <= 0.0) return null
    if (fastSecPerKm <= 0 || slowSecPerKm <= 0) return null
    val secPerKm = paceMinPerKm * 60.0
    return when {
        secPerKm > maxOf(fastSecPerKm, slowSecPerKm) -> PaceCue.SpeedUp
        secPerKm < minOf(fastSecPerKm, slowSecPerKm) -> PaceCue.EaseOff
        else -> PaceCue.OnTarget
    }
}

/** The plan's pace range describes the WORK: it is judged on a continuous run (no
 *  step guide) and on its main-run / rep steps, never during a warm-up, recovery,
 *  cool-down, strides, run/walk bouts, or once the steps are done. */
internal fun paceTargetApplies(progress: StepProgress?): Boolean {
    if (progress == null) return true
    val kind = progress.step?.kind ?: return false
    return kind == RunStepKind.Main || kind == RunStepKind.Rep
}

/** What the location-permission panel says, matching what the user actually allowed. */
internal enum class LocationPrompt {
    /** Never asked: explain, then Continue opens the system prompt. */
    Rationale,
    /** Asked and refused (the system can still ask again). */
    Denied,
    /** Only approximate location was allowed, which can't measure a route. */
    CoarseOnly,
    /** "Don't ask again": only app settings can restore it. */
    Blocked,
}

internal fun locationPrompt(asked: Boolean, permaDenied: Boolean, coarseGranted: Boolean): LocationPrompt = when {
    permaDenied -> LocationPrompt.Blocked
    coarseGranted -> LocationPrompt.CoarseOnly
    !asked -> LocationPrompt.Rationale
    else -> LocationPrompt.Denied
}

/** Covered distance (km) below which the whole-run average pace is withheld: an
 *  average over a few metres is noise. */
private const val MIN_AVG_PACE_KM = 0.05

/** Whole-run average pace (canonical min/km) over MOVING time, or NaN before
 *  [MIN_AVG_PACE_KM] is covered. Shared by the stat tiles and the Finish review so
 *  both show the same figure. */
internal fun averagePaceMinPerKm(elapsedSec: Long, km: Double): Double =
    if (km >= MIN_AVG_PACE_KM && elapsedSec > 0) elapsedSec / 60.0 / km else Double.NaN

/** The recorded fixes as route segments (each [segmentStarts] index opens a new
 *  one, so a pause never draws a bridging line), for the Finish review's route
 *  thumbnail. Pure. */
internal fun trackSegments(points: List<GpsPoint>, segmentStarts: Collection<Int>): List<List<GpxFix>> {
    val starts = segmentStarts as? Set<Int> ?: segmentStarts.toHashSet()
    val out = ArrayList<List<GpxFix>>()
    var seg = ArrayList<GpxFix>()
    points.forEachIndexed { i, p ->
        if (i in starts && seg.isNotEmpty()) {
            out += seg
            seg = ArrayList()
        }
        seg += GpxFix(p.lat, p.lon, p.observedAt, hrBpm = null, accuracyM = p.accuracyM)
    }
    if (seg.isNotEmpty()) out += seg
    return out
}

/** Great-circle distance between two consecutive fixes, km ([haversineM]), for the
 *  live accumulator ([RunSession.add]), recovery ([RunSession.restore]), the pace
 *  slices and the start-on-move gate. */
internal fun segmentKm(a: GpsPoint, b: GpsPoint): Double = haversineM(a.lat, a.lon, b.lat, b.lon) / 1000.0

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

/** True when this app is exempt from battery optimization (Doze/App Standby won't
 *  freeze it), so a long background run can keep its location stream alive. When
 *  false the tracker surfaces a quiet reliability nudge. */
private fun isIgnoringBatteryOptimizations(ctx: Context): Boolean {
    val pm = ctx.getSystemService(Context.POWER_SERVICE) as PowerManager
    return pm.isIgnoringBatteryOptimizations(ctx.packageName)
}

/** True when at least approximate location is allowed (with or without precise). */
private fun hasCoarseLocation(ctx: Context): Boolean =
    ContextCompat.checkSelfPermission(ctx, Manifest.permission.ACCESS_COARSE_LOCATION) ==
        PackageManager.PERMISSION_GRANTED

/** True when the system would no longer show its location prompt ("don't ask again"
 *  after a denial). Before the first ask the same check reads true, so callers pair it
 *  with whether the prompt was shown. */
private fun locationPromptBlocked(ctx: Context): Boolean {
    val act = ctx as? Activity ?: return false
    return !ActivityCompat.shouldShowRequestPermissionRationale(act, Manifest.permission.ACCESS_FINE_LOCATION)
}
