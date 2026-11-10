package app.milestone

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.MutableState
import androidx.compose.runtime.Stable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import app.milestone.resources.Res
import app.milestone.resources.main_core_unavailable_message
import app.milestone.resources.main_core_unavailable_title
import app.milestone.resources.main_data_unreadable
import app.milestone.resources.main_discard
import app.milestone.resources.main_restored_entries
import app.milestone.resources.main_restoring_data
import app.milestone.resources.main_resume_run_message
import app.milestone.resources.main_resume_run_title
import app.milestone.resources.main_save_failed
import app.milestone.resources.profile_export_data
import app.milestone.resources.track_action_resume
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.IO
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.jetbrains.compose.resources.stringResource

/**
 * Boot + session state for the Coach screen: a state holder
 * ([rememberCoachSession]) for [CoachScaffold]. Every field delegates to the
 * MutableState the factory created, so each keeps its remember/rememberSaveable
 * tier (see the factory).
 */
@Stable
internal class CoachSession(
    loadedState: MutableState<Boolean>,
    modelState: MutableState<ViewModel>,
    showSetupState: MutableState<Boolean>,
    setupInitialState: MutableState<Event.SetProfile?>,
    showTrackerState: MutableState<Boolean>,
    showResumePromptState: MutableState<Boolean>,
    lastPainState: MutableState<PainDetail?>,
    val dispatch: (Event) -> Unit,
    val dispatchAll: (List<Event>) -> Unit,
    val markOnboardingDone: () -> Unit,
    /** A pending [OpenDest] from an open intent or URL; [CoachScaffold] applies and clears it. */
    val openRequest: OpenRequests,
    /** The platform's run recorder, or null where the app has none (iOS for now):
     *  then every tracker entry is hidden and runs are logged by hand. */
    val tracker: RunTracker?,
) {
    var loaded by loadedState
    var model by modelState
    var showSetup by showSetupState
    var setupInitial by setupInitialState
    var showTracker by showTrackerState
    var showResumePrompt by showResumePromptState
    var lastPain by lastPainState

    /** The interrupted run's fixes boot read for the resume offer, so Resume reuses
     *  them instead of re-reading the file; null once consumed. */
    var recoveredRun: RecoveredTrack? = null
}

@Composable
private fun rememberCoachSession(openRequest: OpenRequests): CoachSession {
    val ctx = currentPlatformContext()
    val res = currentResources()
    // Seed from the process-level EventDispatcher.model so a rotation renders the
    // last content instantly; a cold start (flow still null) shows the splash while
    // the boot effect below restores the log off the main thread and hydrates.
    val loadedState = remember { mutableStateOf(EventDispatcher.model.value != null) }
    val modelState = remember { mutableStateOf(EventDispatcher.model.value ?: ViewModel()) }
    // Every post-event model the serial consumer publishes, including those drained
    // while a previous Activity was torn down. `loaded` stays owned by the boot
    // effect, so the UI is gated until the once-per-process setup decision has run.
    LaunchedEffect(Unit) {
        EventDispatcher.model.collect { vm -> if (vm != null) modelState.value = vm }
    }

    // Surface core notices (an entry not saved, data that couldn't be read).
    // Collected at composition scope; the process-scoped channel holds a notice
    // that arrives while no Activity is collecting (mid-recreation).
    LaunchedEffect(Unit) {
        EventDispatcher.notices.collect { notice ->
            val text = when (notice) {
                CoreNotice.NotSaved -> Res.string.main_save_failed
                CoreNotice.Unreadable -> Res.string.main_data_unreadable
            }
            showNotice(ctx, res.getString(text), long = true)
        }
    }

    // Guided setup: shown once on a true fresh install instead of auto-seeding
    // an opinionated profile. Saveable so it survives rotation mid-setup; gated
    // additionally by a persisted "onboarding_done" flag so a skip doesn't re-prompt
    // on the next launch (first-run only, per the plan).
    val showSetupState = rememberSaveable { mutableStateOf(false) }
    // When non-null the guided setup opens PRE-FILLED from this draft (the Profile
    // "Re-run guided setup" row seeds it from the current profile). null = a genuine
    // first-run pass. Drives the onboarding-flag behaviour at the call site: only a
    // first-run pass (null) touches the onboarding pref on complete/skip.
    val setupInitialState = rememberSaveable { mutableStateOf<Event.SetProfile?>(null) }
    val setupStore = remember { ctx.setupStore() }
    val markOnboardingDone = { setupStore.edit { putBoolean(KEY_ONBOARDING_DONE, true) } }

    // rememberSaveable, not remember: a config change (rotation) recreates this
    // Activity, and a plain remember would reset this to false, bouncing the user
    // off the live tracking screen mid-run even though the foreground service keeps
    // recording. The saveable flag keeps them on the map across recreation.
    val showTrackerState = rememberSaveable { mutableStateOf(false) }

    // A crash-recovery sidecar is offered for resume, never resumed unasked; a
    // stale sidecar (newest fix older than RESUME_STALE_SEC) is discarded outright.
    val showResumePromptState = rememberSaveable { mutableStateOf(false) }

    // Shell echo of the most recent pain report, so the DO-NOT-TRAIN banner can
    // name the body part immediately after triage even before the core surfaces
    // its own `detail` string. Cleared when the hold is removed/readiness cleared.
    // Not persisted across process death (the core's persisted detail covers that).
    val lastPainState = remember { mutableStateOf<PainDetail?>(null) }

    // One dispatch path so shell-side echoes (lastPain, the plan-cleared marker) stay
    // in lockstep with the core. The echoes run on the caller (main) thread in call
    // order before the event is enqueued on the process-scoped [EventDispatcher],
    // whose single consumer applies events in order off the main thread.
    val echo: (Event) -> Unit = { e ->
        when {
            e is Event.SubmitReadiness && e.pain != null -> lastPainState.value = e.pain
            e is Event.RemoveReadiness && e.signal == ReadinessSignal.Pain -> lastPainState.value = null
            // "I'm pain-free now" retracts every pain report: the banner's echo goes too.
            e is Event.ResolveHold && e.signal == ReadinessSignal.Pain -> lastPainState.value = null
            e is Event.ClearReadiness -> lastPainState.value = null
            // Plan-cleared marker (persisted): a "Remove plan" must STICK across
            // rotation AND relaunch, so the boot auto-GeneratePlan is gated on it
            // ([bootTurn]). Generating any plan (the plan prompt card, or a guided-setup
            // completion) clears the marker so auto-generation resumes normally.
            e is Event.ClearPlan -> setupStore.edit { putBoolean(KEY_PLAN_CLEARED, true) }
            e is Event.GeneratePlan -> setupStore.edit { putBoolean(KEY_PLAN_CLEARED, false) }
        }
    }
    val dispatch: (Event) -> Unit = { e ->
        echo(e)
        EventDispatcher.send(e)
    }
    val dispatchAll: (List<Event>) -> Unit = { events ->
        events.forEach(echo)
        EventDispatcher.sendAll(events)
    }

    val session = remember {
        CoachSession(
            loadedState = loadedState,
            modelState = modelState,
            showSetupState = showSetupState,
            setupInitialState = setupInitialState,
            showTrackerState = showTrackerState,
            showResumePromptState = showResumePromptState,
            lastPainState = lastPainState,
            dispatch = dispatch,
            dispatchAll = dispatchAll,
            markOnboardingDone = markOnboardingDone,
            openRequest = openRequest,
            tracker = platformRunTracker(),
        )
    }

    // Boot hydration as ONE exclusive turn of the serial consumer ([bootTurn]): no
    // queued event interleaves with restore/SetToday/GeneratePlan. A re-run
    // (rotation) queues after it, skips the probe and the one-time block, and resends
    // SetToday only when the day or offset changed since the last send. A reboot
    // ([requestReboot], iOS after a data import) runs it again as a fresh process would.
    val generation by AppRuntime.bootGeneration.collectAsState()
    LaunchedEffect(generation) {
        // A reboot replaces the shown model: the loading state until it hydrates.
        if (generation > 0 && !AppRuntime.bootHandled) session.loaded = false
        var fresh = false
        val vm = EventDispatcher.exclusive {
            // First touch of the engine on its own thread: Android's loads the native
            // library in its initializer, which must not run on the UI thread.
            val engine = withContext(CoreDispatcher) { AppRuntime.engine }
            val beforeRestore = AppRuntime.takeBeforeRestore()
            bootTurn(engine, setupStore, AppRuntime, restore = {
                beforeRestore?.invoke()
                engine.restore(ctx)
            })?.also { fresh = it.fresh }?.view
        }
        // No model: the core is unavailable. Show the error state, dispatch nothing.
        if (vm == null) {
            EventDispatcher.coreUnavailable.value = true
            return@LaunchedEffect
        }
        // The consumer already published vm in queue order.
        session.model = vm
        // Once per process: a re-run must not recompute showSetup (mid-setup answers)
        // or re-offer run recovery.
        if (!AppRuntime.bootHandled) {
            AppRuntime.bootHandled = true
            // A new process restored with the tracker flag set records nothing (the
            // service is NOT_STICKY): leave it so the resume prompt is the way back.
            if (!RunSession.tracking.value && !RunSession.armed.value && !RunSession.locating.value) {
                session.showTracker = false
            }
            // The guided setup only on a first run not yet completed or skipped.
            session.showSetup = showSetupOnBoot(fresh, vm, setupStore)
            // A data import just restarted the process: confirm what it restored.
            takeRestoredEntries(setupStore)?.let { restored ->
                UiMessages.post(UiMessage(res.getQuantityString(Res.plurals.main_restored_entries, restored, restored)))
            }
            // An interrupted run's sidecar is offered, never auto-resumed; skipped when
            // a run is live in memory.
            val tracker = session.tracker
            if (tracker != null && !RunSession.tracking.value && RunSession.points.value.isEmpty() &&
                tracker.hasInterruptedRun(ctx)
            ) {
                val recovered = withContext(Dispatchers.IO) { tracker.recoverInterruptedRun(ctx) }
                if (resumable(recovered, wallClockMillis() / 1000)) {
                    // Ask; Resume reuses these fixes instead of re-reading the file.
                    session.recoveredRun = recovered
                    session.showResumePrompt = true
                } else {
                    // Under 2 fixes, or a dead run that must not become a multi-hour run.
                    tracker.discardInterruptedRun()
                }
            }
        }
        session.loaded = true
    }

    // Live only once the boot gate has passed, never during the loading spinner.
    if (session.loaded) {
        // An app left alive across local midnight (or a timezone / DST change) keeps
        // the core dating the plan to the last day it was told. Re-send SetToday on
        // every ON_RESUME whose day or offset differs from the last send; never
        // before boot's first send (`lastToday == null`), which owns the fresh-install path.
        val lifecycleOwner = LocalLifecycleOwner.current
        DisposableEffect(lifecycleOwner) {
            val observer = LifecycleEventObserver { _, event ->
                if (event == Lifecycle.Event.ON_RESUME) {
                    val now = todayEpochDay() to utcOffsetSec()
                    val last = AppRuntime.lastToday
                    if (last != null && last != now) {
                        AppRuntime.lastToday = now
                        session.dispatch(Event.SetToday(now.first, now.second))
                    }
                }
            }
            lifecycleOwner.lifecycle.addObserver(observer)
            onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
        }
    }

    return session
}

/**
 * Crash-recovery consent: an interrupted run's sidecar is offered, never silently
 * resumed. Discard leaves nothing behind; Resume repopulates the track and
 * restarts the recording service, or, without a precise-location grant (revoked
 * while the app was gone), holds the run for the tracker, which asks for the grant
 * and restarts recording once it is given. Composed at the very top of [CoachScaffold],
 * ahead of its full-screen setup/tracker returns, so a tracker screen restored
 * after process death can never hide it. Only offered where a [RunTracker] exists.
 */
@Composable
internal fun ResumeRunDialog(session: CoachSession) {
    val ctx = currentPlatformContext()
    // Remembered before the visibility return so the Resume coroutine outlives the
    // dialog leaving composition.
    val scope = rememberCoroutineScope()
    val tracker = session.tracker
    if (!session.showResumePrompt || tracker == null) return
    // A run started since boot owns the sidecar now (Start truncated the
    // interrupted one): resuming would replace its live track and discarding would
    // delete its crash-recovery file, so both just drop the stale offer.
    val liveRun = { RunSession.tracking.value || RunSession.armed.value }
    AppDialog(
        onDismiss = { /* deliberate choice required */ },
        title = { Text(stringResource(Res.string.main_resume_run_title)) },
        text = { Text(stringResource(Res.string.main_resume_run_message)) },
        confirmButton = {
            TextButton(onClick = {
                session.showResumePrompt = false
                if (liveRun()) {
                    session.recoveredRun = null
                    return@TextButton
                }
                scope.launch {
                    val recovered = session.recoveredRun
                        ?: withContext(Dispatchers.IO) { tracker.recoverInterruptedRun(ctx) }
                    session.recoveredRun = null
                    if (recovered.size >= 2) {
                        RunSession.restore(recovered)
                        if (tracker.hasLocationPermission(ctx)) {
                            tracker.startRecording(ctx)
                        } else {
                            // No grant (revoked mid-run, or a lapsed one-time grant): a
                            // start now would only fail the location service and strand the
                            // run. Hold it as a capture; the tracker asks for location and
                            // restarts recording once it is granted.
                            RunSession.awaitLocationForResume()
                        }
                        session.showTracker = true
                    } else {
                        tracker.discardInterruptedRun()
                    }
                }
            }) { Text(stringResource(Res.string.track_action_resume)) }
        },
        dismissButton = {
            TextButton(onClick = {
                session.showResumePrompt = false
                session.recoveredRun = null
                if (!liveRun()) tracker.discardInterruptedRun()
            }) { Text(stringResource(Res.string.main_discard), color = DangerText) }
        },
    )
}

/**
 * The app: boot (the loading state until the first model, or the core-unavailable
 * state), then the scaffold with its tabs, sheets and snackbars. Each platform's host
 * calls it under [MilestoneTheme]: Android's `MainActivity`, iOS's `LollopIosRoot`.
 * [openRequest] is the host's deep-link state (an open intent, a URL, a reminder tap).
 */
@Composable
internal fun LollopApp(openRequest: OpenRequests) {
    val session = rememberCoachSession(openRequest)

    val coreUnavailable by EventDispatcher.coreUnavailable.collectAsState()
    if (coreUnavailable) {
        CoreUnavailableScreen()
        return
    }

    if (!session.loaded) {
        val ctx = currentPlatformContext()
        // An import restart is still replaying the restored log (the key goes once boot runs).
        val restoring = remember { restoringImport(ctx.setupStore()) }
        Box(Modifier.fillMaxSize().background(BgTop), contentAlignment = Alignment.Center) {
            Column(
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(Space.Card.dp),
            ) {
                CircularProgressIndicator(color = Accent)
                if (restoring) Text(stringResource(Res.string.main_restoring_data), color = OnBgMuted, style = Type.Body)
            }
        }
        return
    }

    CoachScaffold(session)
}

/**
 * Boot found the core unusable ([DurableCore.probe]): say so instead of rendering an
 * empty model as a fresh install. Nothing here can dispatch an event (the core
 * refuses them anyway, [DurableCore.unavailable]); the one action is the raw-log
 * export, which reads the durable log in pure Kotlin ([exportDataTo]), so the
 * user's data can leave the phone before an update or reinstall.
 */
@Composable
internal fun CoreUnavailableScreen() {
    val exportData = rememberDataExport()
    Box(
        Modifier.fillMaxSize().background(BgTop).safeDrawingPadding().padding(Space.Screen.dp),
        contentAlignment = Alignment.Center,
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(Space.Card.dp)) {
            SheetTitle(stringResource(Res.string.main_core_unavailable_title))
            Text(stringResource(Res.string.main_core_unavailable_message), color = OnBgBody, style = Type.Body)
            PrimaryButton(stringResource(Res.string.profile_export_data), onClick = exportData, modifier = Modifier.fillMaxWidth())
        }
    }
}
