package app.milestone

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.displayCutout
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
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
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.milestone.resources.Res
import app.milestone.resources.import_view
import app.milestone.resources.log_run_saved
import app.milestone.resources.track_battery_allow
import app.milestone.resources.track_battery_caption
import app.milestone.resources.track_btn_try_again
import app.milestone.resources.track_location_failed_caption
import app.milestone.resources.track_notifications_off_caption
import app.milestone.resources.track_open_settings
import app.milestone.resources.track_toast_not_enough_gps
import app.milestone.resources.track_toast_save_failed
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.DelicateCoroutinesApi
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import org.jetbrains.compose.resources.stringResource

/** How long Save waits for the service to confirm the location stream stopped before
 *  it snapshots the track; past it the run is kept unsaved (Save/Discard). */
private const val STOP_CONFIRM_TIMEOUT_MS = 2_000L

/** Elevation of the stat sheet, a sanctioned shadow (06-run-tracking §5). */
private val SheetElevation = 16.dp
private val SheetBottomPadding = 20.dp

private val DragHandleWidth = 38.dp
private val DragHandleHeight = 5.dp
private const val DRAG_HANDLE_ALPHA = 0.22f

/**
 * GPS run-tracking overlay (06-run-tracking): map fills the frame, stat sheet
 * pins to the bottom, safety banner (when active) pins topmost. The location
 * stream lives in the platform's [RunTracker] (Android: a foreground service) so recording
 * survives screen-off/backgrounding; on Save (Stop → Finish review → Save) the raw
 * fix list goes to the Rust core via [Event.LogRunTrack], where the OF-RECORD
 * distance / pace / zone / spike are all derived. The live elapsed / distance / pace
 * shown while recording are factual progress indicators computed from the raw fixes;
 * the only judgement is PACE NOW against the core's own planned pace range for a
 * followed plan. They are superseded by the core's derivation on save.
 *
 * [onLogRunManually] opens the manual run log; the location-denied panel offers it
 * as the way out when GPS can't be used.
 */
@OptIn(DelicateCoroutinesApi::class)
@Composable
fun RunTrackingScreen(
    model: ViewModel,
    onEvent: (Event) -> Unit,
    onFinish: () -> Unit,
    onLogRunManually: () -> Unit,
) {
    val ctx = currentPlatformContext()
    val tracker = remember { platformRunTracker() }
    val scope = rememberCoroutineScope()

    // Follow mode keeps the map centred on the latest fix. A manual drag drops it
    // so the runner can inspect the route; the Recenter button re-arms it.
    var follow by remember { mutableStateOf(true) }

    // FINE only: GPS tracking is unusable on a coarse-only grant.
    var hasPermission by remember { mutableStateOf(tracker.hasLocationPermission(ctx)) }
    // Null where a run has no foreground service: nothing to grant, nothing to nudge.
    val grants = tracker.serviceGrants
    var notifGranted by remember { mutableStateOf(grants?.canPostNotifications(ctx) ?: true) }
    var locationOn by remember { mutableStateOf(tracker.isLocationEnabled(ctx)) }
    // Battery-optimization exemption: a non-exempt app can be Dozed/frozen on a long
    // background run, silently starving the location stream. Tracked so the tracker
    // can nudge the user to allow it; re-sampled on ON_RESUME so the caption clears
    // itself the moment the exemption is granted.
    var batteryUnrestricted by remember { mutableStateOf(grants?.isBatteryUnrestricted(ctx) ?: true) }
    val setupPrefs = remember { ctx.setupStore() }
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
        mutableStateOf(permAsked && !tracker.hasLocationPermission(ctx) && tracker.locationPromptBlocked(ctx))
    }
    var notifAsked by rememberSaveable { mutableStateOf(false) }
    val requestLocation = tracker.rememberLocationPrompt { granted ->
        hasPermission = granted
        permaDenied = !hasPermission && tracker.locationPromptBlocked(ctx)
        locationOn = tracker.isLocationEnabled(ctx)
    }
    val requestNotifications = grants?.rememberNotificationPrompt { notifGranted = grants.canPostNotifications(ctx) }

    fun requestPermissions() {
        permAsked = true
        setupPrefs.edit { putBoolean(KEY_LOCATION_ASKED, true) }
        requestLocation()
    }

    // Asked at the moment of Start, once location is granted, never up front: the
    // ongoing notification is the lock-screen view of a run in progress.
    fun requestNotificationsOnce() {
        if (notifGranted || notifAsked) return
        notifAsked = true
        requestNotifications?.invoke()
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
    // Optional, stackable start gates, persisted in the [setupStore] (default
    // off). The UI holds the toggles so both the gate chips and the armed pill/control
    // text stay consistent with what the just-tapped Start requested.
    var preciseGate by remember { mutableStateOf(setupPrefs.getBoolean(KEY_START_PRECISE, false)) }
    var movingGate by remember { mutableStateOf(setupPrefs.getBoolean(KEY_START_MOVING, false)) }
    val paceBucketN by AppSettings.paceBucketMinutes.collectAsStateWithLifecycle()
    val unit = rememberDistanceUnit()
    // The armed → recording confirmation buzz is [RunRecorder]'s cue at its
    // beginRecording: it must reach a runner whose screen is already off, which a
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
    if (keepScreenOn && (tracking || armed)) KeepScreenOn()

    val map = rememberTrackerMap()

    // Bind the map to the ACTIVITY lifecycle (not just composition):
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
                    map.resume()
                    hasPermission = tracker.hasLocationPermission(ctx)
                    notifGranted = grants?.canPostNotifications(ctx) ?: true
                    locationOn = tracker.isLocationEnabled(ctx)
                    // Re-sample after returning from the battery-optimization dialog /
                    // settings so the guidance caption disappears once exempted.
                    batteryUnrestricted = grants?.isBatteryUnrestricted(ctx) ?: true
                    // Back from settings (or the app switcher): one fresh locate attempt.
                    RunSession.clearLocationFailed()
                }
                Lifecycle.Event.ON_PAUSE -> map.pause()
                Lifecycle.Event.ON_START -> appForeground = true
                Lifecycle.Event.ON_STOP -> {
                    appForeground = false
                    // Locate-only preview burns the GPS radio with no run in progress.
                    // Leaving the tracker's foreground (Home, backgrounding) with no
                    // recording/armed run and no captured track awaiting save stops the
                    // service so the radio isn't left on indefinitely; a live/armed run
                    // keeps its foreground service. Live RunSession reads, not the
                    // captured composition values the long-lived observer would stale.
                    if (!RunSession.runActive &&
                        RunSession.points.value.isEmpty()
                    ) {
                        tracker.stop(ctx)
                    }
                }
                else -> {}
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        val stopObservingLocation = tracker.observeLocationSettings(ctx) {
            locationOn = tracker.isLocationEnabled(ctx)
            hasPermission = tracker.hasLocationPermission(ctx)
        }
        onDispose {
            lifecycleOwner.lifecycle.removeObserver(observer)
            stopObservingLocation()
            map.detach()
        }
    }

    // Before the first fix arrives, center on last-known location so the map
    // doesn't open on (0,0) in the Gulf of Guinea.
    LaunchedEffect(hasPermission) {
        if (hasPermission && RunSession.points.value.isEmpty()) {
            tracker.lastKnownLocation(ctx) { lat, lon ->
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
            tracker.locate(ctx)
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
            tracker.startRecording(ctx)
        }
    }
    LaunchedEffect(tracking) {
        if (tracking) RunSession.clearResumeAwaitingLocation()
    }

    LaunchedEffect(map) { map.followPreviewFix { follow } }
    LaunchedEffect(map) { map.followTrack { follow } }

    fun armTracking() {
        if (!hasPermission) {
            requestPermissions()
            return
        }
        // Refuse-guard (defense in depth), two EXPLICIT cases. [RunRecorder]'s
        // beginRecording() opens the sidecar and this fn deliberately doesn't
        // reset(), so arming onto a live run OR an earlier stopped track would risk
        // merging two runs. The derived `unsavedCapture` already keeps Start off the
        // control row; these backstop a future UI regression so it can still never
        // happen. Two checks, not one `isNotEmpty() && !tracking`: that would pass
        // while tracking, so a one-frame-stale click could demote a live run.
        if (RunSession.tracking.value) return
        if (RunSession.points.value.isNotEmpty()) return
        locationOn = tracker.isLocationEnabled(ctx)
        // A user Start is a fresh attempt even after a failed locate preview.
        RunSession.clearLocationFailed()
        // Armed start: Start is tappable before a lock. Recording begins automatically
        // at the first fix clearing the chosen gates; the SIDECAR is opened by
        // [RunRecorder]'s beginRecording() at that first real fix (NOT here), so a crash while
        // merely armed leaves no sidecar and no false recovery prompt. Clear any stale
        // pause and publish `armed` now so the control row flips instantly.
        RunSession.followPlan(if (followPlan) planItem else null)
        RunSession.setPaused(false)
        RunRecorder.armRequested()
        tracker.arm(ctx, preciseGate, movingGate)
        requestNotificationsOnce()
    }

    // Leave the tracker with nothing to keep: stop the service, delete the
    // crash-recovery sidecar, clear the session and close the screen. The one
    // teardown every no-capture exit shares (Back, ✕ before Start, ✕ while armed,
    // a denied location, a confirmed discard). `exiting` first, so the locate-restart
    // effect can't re-launch the service into the disposing screen.
    fun leaveTracker() {
        exiting = true
        tracker.stop(ctx)
        tracker.discardInterruptedRun()
        RunSession.reset()
        onFinish()
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
    val runSavedText = stringResource(Res.string.log_run_saved)
    val viewLabel = stringResource(Res.string.import_view)
    val saveFailedText = stringResource(Res.string.track_toast_save_failed)
    val notEnoughGpsText = stringResource(Res.string.track_toast_not_enough_gps)

    // Log the captured track ([RunSession.trackEventForCore]) to the core, drop the
    // sidecar, and leave. Always reads the live RunSession track, so no remembered
    // copy can go stale across a rotation. [ownsSaving]: stopAndLog already holds the
    // [claimSaving] claim and hands it over.
    fun logCaptured(ownsSaving: Boolean = false, sessionRpe: Int? = finishRpe, workoutType: WorkoutType? = finishType) {
        if (!RunSession.claimSaving(ownsSaving)) return
        // Committed to leaving: RunSession.reset() lands before onFinish, and the
        // locate-restart effect must not start a LOCATE service in that gap.
        exiting = true
        // Pinned to the main thread (a no-op on device, where the composition scope
        // already is): every resume after the IO persist (toast, navigation, the
        // saving flag) must land on main. A host whose effect dispatcher doesn't
        // dispatch (Compose's test environment) would otherwise continue on the IO
        // thread, where the notice (an Android Toast) throws and the save never finishes.
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
                    // runs on CoreDispatcher. runSaveCriticalSection's NonCancellable
                    // wrapper protects the await, so a rotation can't cancel the save.
                    EventDispatcher.exclusive {
                        withContext(CoreDispatcher) {
                            // The run type and whole-session effort the runner picked on
                            // the Finish review (both optional, null = not given; never
                            // fabricated). A tracked run also derives its measured
                            // INTERVAL verdict on its own.
                            val event = RunSession.trackEventForCore(entryId, workoutType, sessionRpe)
                            runObservedAt = event.observedAt
                            val result = AppCore.engine.send(event)
                            // A rejected/panicked send, or one accepted in memory whose
                            // log append failed (disk full), must not silently drop the
                            // captured run: throw so the critical section's onFailure
                            // keeps the track + crash sidecar and surfaces the
                            // save-failed toast, instead of resetting the session.
                            if (!result.accepted || !result.persisted) error("run save not persisted")
                            result.view // the saved model, which exclusive publishes
                        }
                    }
                },
                // clearSync orders the sidecar delete against the just-returned
                // event-log append (a crash in that gap would otherwise
                // resurrect the saved run via the recovery prompt).
                clearSidecar = { tracker.discardInterruptedRunSync() },
                resetSession = { RunSession.reset() },
                finish = {
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
                    onFinish()
                },
                onFailure = {
                    // Keep the captured run and its sidecar: the fixes stay in
                    // RunSession.points, so `unsavedCapture` re-shows Save/Discard.
                    RunSession.releaseSaving()
                    exiting = false
                    UiMessages.post(UiMessage(saveFailedText))
                },
            )
        }
    }

    fun stopAndLog(sessionRpe: Int?, workoutType: WorkoutType?) {
        // Claims [RunSession.saving] (see [claimSaving]) and hands it to
        // logCaptured(ownsSaving = true); every early exit below releases it.
        if (!RunSession.claimSaving()) return
        // Committing to end the session: suppress the locate-restart effect so the
        // service doesn't re-acquire while the stop and save run.
        exiting = true
        // ATOMIC start: a rotation can cancel `scope` before the first dispatch, and
        // a dropped body would strand the claim. Main-pinned like [logCaptured].
        scope.launch(Dispatchers.Main.immediate, start = CoroutineStart.ATOMIC) {
            try {
                tracker.stop(ctx)
                // Snapshot the track only AFTER the service confirms the engine is
                // unregistered: the stop intent is asynchronous, so a snapshot taken
                // immediately would drop tail fixes still in flight.
                val stopConfirmed = withTimeoutOrNull(STOP_CONFIRM_TIMEOUT_MS) { RunSession.tracking.first { !it } }
                if (stopConfirmed == null) {
                    // The engine may still be registered, so the track may be
                    // incomplete: keep the fixes, release the claim, save nothing.
                    RunSession.releaseSaving()
                    UiMessages.post(UiMessage(saveFailedText))
                    return@launch
                }
                val captured = RunSession.points.value
                if (captured.size < 2) {
                    UiMessages.post(UiMessage(notEnoughGpsText, long = true))
                    tracker.discardInterruptedRun()
                    RunSession.reset()
                    onFinish()
                    return@launch
                }
                // A short run was already flagged inline on the Finish review, so
                // Save here means keep it.
                logCaptured(ownsSaving = true, sessionRpe = sessionRpe, workoutType = workoutType)
            } catch (e: CancellationException) {
                // A rotation cancelled the stop-confirm wait: nothing was persisted, so
                // release the claim (else Save/Discard stay disabled) and re-throw.
                RunSession.releaseSaving()
                throw e
            } catch (e: Exception) {
                PlatformLog.w("RunSave", "run stop before saving failed, fixes kept", e)
                // Stop/snapshot failed before logging: keep the fixes, so
                // `unsavedCapture` shows Save/Discard rather than Start.
                RunSession.releaseSaving()
                UiMessages.post(UiMessage(saveFailedText))
            }
        }
    }

    // Back gesture keeps recording and returns to the app (06 spec §Behavior). A
    // captured-but-unsaved run (failed save) leaves WITHOUT clearing the sidecar so it
    // stays recoverable on next launch; only an exit with nothing captured (incl. the
    // locate-only preview) tears the session down.
    BackHandler {
        if (tracking || unsavedCapture) onFinish() else leaveTracker()
    }

    // Perform one Finish-review action: the pure [finishTransition] picks the next step
    // and at most one side effect; this only applies it. Resume only unpauses the
    // still-running service (same sidecar, same run), Save hands off to the existing
    // stop → save path, Discard happens only from the confirm step.
    fun onFinishAction(action: FinishAction) {
        val t = finishTransition(finishStep, action)
        finishStep = t.step
        when (t.effect) {
            FinishEffect.None -> {}
            FinishEffect.PauseRecording -> {
                RunSession.setPaused(true)
                // Preselect the run type of the plan item being followed (visible and
                // one tap to change or clear); a free run starts untagged.
                if (finishType == null) finishType = RunSession.plannedRun.value?.workout_type
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
        // On a landscape window the map takes the start and the stat sheet stands
        // beside it as a full-height panel, so the map keeps a usable height
        // (visual-design-system §5 "Adaptive layout").
        val besideMap = LocalWindowLayout.current.sideControls && showRouteMaps && !reviewing
        val mapFrame: @Composable (Modifier) -> Unit = { frameModifier ->
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
                modifier = frameModifier,
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
                    onRecheckPermission = { hasPermission = tracker.hasLocationPermission(ctx) },
                    permissionAsked = permAsked,
                    modifier = pillModifier,
                )
            }
        }
        // The stat sheet's navigationBarsPadding keeps the control row clear of the
        // system gesture strip (edge-to-edge window).
        val sheetFills = reviewing || !showRouteMaps || besideMap
        val sheetShape = if (besideMap) {
            RoundedCornerShape(topStart = SheetCornerRadius, bottomStart = SheetCornerRadius)
        } else {
            RoundedCornerShape(topStart = SheetCornerRadius, topEnd = SheetCornerRadius)
        }
        // [gutter]: the sheet's side padding; the Finish review on a wide window
        // centres its route + review pair ([FinishPairMaxWidth]) instead of
        // stretching the chips and the Save row across a landscape screen. A
        // landscape camera cutout stays clear of the sheet's content (the map may
        // run under it); beside the map only the end edge can meet one.
        val cutoutSides = if (besideMap) WindowInsetsSides.End else WindowInsetsSides.Horizontal
        val statSheet: @Composable (Modifier, Dp) -> Unit = { sheetModifier, gutter -> Column(
            modifier = sheetModifier
                .shadow(SheetElevation, sheetShape)
                .background(BgTop, sheetShape)
                .navigationBarsPadding()
                .windowInsetsPadding(WindowInsets.displayCutout.only(cutoutSides))
                .padding(horizontal = gutter)
                .padding(top = Space.Md.dp, bottom = SheetBottomPadding),
            verticalArrangement = Arrangement.spacedBy(Space.Card.dp),
        ) {
            // A short window (a phone in landscape) gives the handle's height to
            // the content, as [AppSheet] does.
            if (!besideMap && !LocalWindowLayout.current.shortHeight) DragHandle()
            if (!hasPermission) {
                val prompt = locationPrompt(permAsked, permaDenied, tracker.hasApproximateLocation(ctx))
                LocationPermissionPanel(
                    prompt = prompt,
                    // Once permanently denied ("don't ask again") the system dialog won't
                    // appear, so re-requesting silently no-ops; deep-link to app
                    // settings instead, the only place the grant can then be restored.
                    onAllow = { if (prompt == LocationPrompt.Blocked) openAppSettings(ctx) else requestPermissions() },
                    // A resumed run waiting on the grant holds recorded fixes: ✕ is a
                    // confirmed discard and Save keeps what was recorded, never a
                    // silent leave that deletes the run.
                    onClose = { if (resumeWaiting && unsavedCapture) confirmDiscard = true else leaveTracker() },
                    onLogManually = { leaveTracker(); onLogRunManually() },
                    onSaveRun = if (resumeWaiting && unsavedCapture) ({ logCaptured() }) else null,
                )
            } else {
                // Everything above the control row. With the sheet filling the frame
                // (route maps off, the landscape side panel) it sits on the control row
                // in thumb reach, not stacked under the handle above a half-empty
                // screen, and scrolls when it is taller than the frame (a plan card
                // and the start options in a landscape window), so ✕ / Start stay on
                // screen; the `EditorScaffold` end-column pattern.
                val aboveControls: @Composable () -> Unit = {
                    // Battery reliability (pre-recording only), at the top of the sheet.
                    // It clears itself (ON_RESUME re-check) once the exemption is granted.
                    if (!batteryUnrestricted && !tracking && !unsavedCapture) {
                        InlineNotice(
                            stringResource(Res.string.track_battery_caption),
                            actionLabel = stringResource(Res.string.track_battery_allow),
                            onAction = { grants?.requestBatteryUnrestricted(ctx) },
                        )
                    }
                    // Live stat readouts (MOVING · DISTANCE · PACE NOW, then Avg + last
                    // slice), a leaf so the per-fix elapsed/distance/pace updates
                    // recompose only those tiles, not the whole tracking screen. Only
                    // once there is something to show: before recording (locate/armed)
                    // the figures would be a frozen row of zeros and dashes. The Finish
                    // review carries its own distance · time · pace summary, so the live
                    // tiles step aside while it is open. Without a map they are the
                    // screen's hero, so they may grow.
                    if (!reviewing && (tracking || unsavedCapture)) {
                        TrackerStats(
                            unit = unit,
                            paceBucketN = paceBucketN,
                            paused = paused,
                            large = !showRouteMaps || besideMap,
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
                                onSkip = { RunSession.skipStep(AppClock.monotonicMillis()) },
                            )
                        }
                    }
                    // Degraded-state surfacing: location off = loud warn row; a GPS
                    // start failure / denied notifications = an inline notice. Each
                    // carries its fix as an inline action (Open settings, Try again).
                    if (!locationOn) LocationOffNotice(onOpenSettings = { tracker.openLocationSettings(ctx) })
                    if (locateFailed && locationOn && !tracking && !armed && (!unsavedCapture || resumeWaiting)) {
                        InlineNotice(
                            stringResource(Res.string.track_location_failed_caption),
                            actionLabel = stringResource(Res.string.track_btn_try_again),
                            onAction = { RunSession.clearLocationFailed() },
                        )
                    }
                    // Only once a run is armed or recording: notifications are asked at
                    // Start, so before it there is nothing to warn about.
                    if (!notifGranted && (tracking || armed)) {
                        InlineNotice(
                            stringResource(Res.string.track_notifications_off_caption),
                            actionLabel = stringResource(Res.string.track_open_settings),
                            onAction = { grants?.openNotificationSettings(ctx) },
                        )
                    }
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
                }
                if (sheetFills && !reviewing) {
                    val scroll = rememberScrollState()
                    Box(Modifier.fillMaxWidth().weight(1f), contentAlignment = Alignment.BottomCenter) {
                        Column(
                            modifier = Modifier.fillMaxWidth().verticalScroll(scroll).testTag("tracker-sheet-scroll"),
                            verticalArrangement = Arrangement.spacedBy(Space.Card.dp),
                        ) { aboveControls() }
                        if (scroll.canScrollForward) {
                            Box(
                                Modifier
                                    .fillMaxWidth()
                                    .height(Space.Lg.dp)
                                    .background(Brush.verticalGradient(listOf(Color.Transparent, BgTop))),
                            )
                        }
                    }
                } else {
                    aboveControls()
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
                        onStartNow = { tracker.startNow(ctx) },
                        onStart = { armTracking() },
                        onPause = { RunSession.setPaused(true) },
                        onResume = { RunSession.setPaused(false) },
                        onStop = { onFinishAction(FinishAction.Stop) },
                    )
                }
            }
        } }
        if (besideMap) {
            BoxWithConstraints(Modifier.weight(1f).fillMaxWidth()) {
                val panelWidth = minOf(TrackerPanelWidth, maxWidth / 2)
                Row(Modifier.fillMaxSize()) {
                    mapFrame(Modifier.weight(1f).fillMaxHeight())
                    statSheet(Modifier.width(panelWidth).fillMaxHeight().testTag("tracker-side-panel"), Space.Sheet.dp)
                }
            }
        } else {
            mapFrame(
                when {
                    reviewing -> Modifier.height(0.dp)
                    showRouteMaps -> Modifier.weight(1f)
                    else -> Modifier
                },
            )
            if (reviewing) {
                BoxWithConstraints(Modifier.weight(1f).fillMaxWidth()) {
                    statSheet(Modifier.fillMaxSize().testTag("tracker-finish-review"), centringGutter(maxWidth, min = Space.Sheet.dp, max = FinishPairMaxWidth))
                }
            } else {
                statSheet((if (sheetFills) Modifier.weight(1f) else Modifier).fillMaxWidth(), Space.Sheet.dp)
            }
        }
        // The tracker replaces the app scaffold and its snackbars: a save failure
        // shows here, under the controls.
        UiMessageHost(Modifier.fillMaxWidth())
    }

    if (showReadinessSheet) {
        TrackerReadinessSheet(model, onEvent, onDismiss = { showReadinessSheet = false })
    }
}

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

/** Width of the side panel (stats and controls) beside the map on a landscape
 *  window: a portrait phone's stat sheet; at most half the window. */
private val TrackerPanelWidth = 400.dp

/** [KeyValueStore] keys (the [setupStore], [KEY_ONBOARDING_DONE] idiom) for
 *  the two optional, stackable start gates, both default false. */
private const val KEY_START_PRECISE = "run_start_precise"
private const val KEY_START_MOVING = "run_start_moving"
/** Set once the system location prompt has been shown (same prefs, default false). */
private const val KEY_LOCATION_ASKED = "run_location_asked"
