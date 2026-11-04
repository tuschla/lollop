package app.milestone

import android.content.Context
import android.content.SharedPreferences
import android.util.Log
import android.widget.Toast
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.core.content.edit
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

// How old a crash-recovery sidecar's newest fix may be and still be offered
// for resume. Beyond this the app was gone long enough that the run is almost
// certainly dead, so the sidecar is discarded rather than resumed (a stale resume
// would let one Stop&save write a garbage multi-hour run). 3 hours.
private const val RESUME_STALE_SEC = 3L * 60L * 60L

/** Name of the shell's setup SharedPreferences: the onboarding flag,
 *  [KEY_PLAN_CLEARED], [KEY_RESTORED_ENTRIES] and the tracker's start gates.
 *  Excluded from Auto Backup (it describes the event log, which is excluded too). */
internal const val SETUP_PREFS = "milestone_setup"

/** The [SETUP_PREFS] SharedPreferences. */
internal fun Context.setupPrefs(): SharedPreferences = getSharedPreferences(SETUP_PREFS, Context.MODE_PRIVATE)

// Persisted flag (in the [SETUP_PREFS] prefs) recording that the user removed
// their plan. It gates the boot auto-GeneratePlan so a "Remove plan" survives both
// rotation and relaunch; generating any plan clears it. See the boot effect + dispatch.
// A data backup carries it as a trailing ClearPlan line ([exportDataTo]) and a
// restore sets it from the imported log ([replaceEventLog]).
internal const val KEY_PLAN_CLEARED = "plan_cleared"

// Process-scoped serial event dispatcher: the queue and its one consumer outlive
// composition, so a rotation never drops a queued event. Exactly one unit (an
// event, a batch, or an [exclusive] body) is in flight at a time, on
// CoreJniDispatcher, and its ViewModel is published to [model] in enqueue order.
// That serial order is the whole ordering guarantee: the core's log and state are
// order-sensitive.
internal object EventDispatcher {
    private sealed interface Msg
    private class Ev(val e: Event) : Msg
    private class Batch(val events: List<Event>) : Msg
    private class Block(val body: suspend () -> ViewModel?, val done: CompletableDeferred<ViewModel?>) : Msg

    // A var only for [resetForTest]; the consumer drains the channel it was started on.
    @Volatile private var channel = newQueue()

    // A queued [exclusive] block the channel drops undelivered (only [resetForTest]
    // cancels a queue) cancels its awaiter instead of leaving it suspended forever.
    private fun newQueue() = Channel<Msg>(Channel.UNLIMITED) { (it as? Block)?.done?.cancel() }

    // The freshest hydrated ViewModel, or null before boot hydrates it. The
    // composition seeds `loaded`/`model` from this (rotation → last content
    // instantly, cold start → the splash holds while boot hydrates) and collects it
    // so every later event's model reaches the UI.
    val model = MutableStateFlow<ViewModel?>(null)

    // Boot found the native core unusable ([Core.probe] / [Core.markUnavailable]):
    // the shell shows [CoreUnavailableScreen] instead of any screen that could write,
    // and the splash lets go although [model] stays null. Process-scoped like
    // [model], so a rotation renders the error at once.
    val coreUnavailable = MutableStateFlow(false)

    // Core outcomes the user must hear about ([report]). A Channel, not a
    // SharedFlow: a notice sent while nothing collects (an Activity mid-recreation)
    // waits in the buffer for the next collector instead of being dropped.
    private val noticeChannel = Channel<CoreNotice>(Channel.BUFFERED)
    val notices: Flow<CoreNotice> = noticeChannel.receiveAsFlow()

    // Whether the last reported result was already stale, so a run of stale
    // results raises ONE notice rather than one per event.
    @Volatile private var staleReported = false

    // The once-per-process boot gate (guided-setup decision + run recovery).
    // Survives rotation so the boot effect doesn't clobber mid-setup answers, and
    // resets on process death, when the ActiveRunStore recovery check must re-run.
    var bootHandled = false

    // The (local epoch-day, utc-offset-sec) the core was last told is "today"; null
    // until boot sends it. Process-scoped, so a rotation's boot re-run can tell the
    // core already has this clock and skip the resend.
    @Volatile var lastToday: Pair<Long, Int>? = null

    // The crash-recovery sidecar fixes read ONCE at boot when a resume prompt is
    // offered, so the Resume action reuses them instead of re-reading the sidecar
    // file off disk. Null when no interrupted run was found / after it is consumed.
    var recoveredSidecar: RecoveredTrack? = null

    // Process-level scope for work that must outlive a sheet/screen (imports,
    // export, the data restore).
    val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    // The real event→view send paths, across the JNI boundary (Core.send).
    private val coreSender: suspend (Event) -> SendResult = { e -> withContext(CoreJniDispatcher) { Core.send(e) } }
    private val coreBatchSender: suspend (List<Event>) -> SendResult =
        { es -> withContext(CoreJniDispatcher) { Core.sendAll(es) } }

    // The send paths in use, injectable so a JVM test can substitute fakes: the
    // real ones are unavailable off device. A var so the consumer's drop-one-unit
    // runCatching still wraps whatever send path is installed.
    var sender: suspend (Event) -> SendResult = coreSender
    var batchSender: suspend (List<Event>) -> SendResult = coreBatchSender

    private var consumer: Job = startConsumer(channel)

    // One serial consumer per process on the main dispatcher; it drains the queue
    // strictly in order. Only [resetForTest] cancels it, after cancelling its queue.
    private fun startConsumer(queue: Channel<Msg>): Job = scope.launch {
        for (m in queue) when (m) {
            // A malformed view (or any send throw) must drop THIS unit, never
            // kill the process-lifetime consumer loop.
            is Ev -> {
                val r = runCatching { sender(m.e) }
                if (!isActive) break
                r.onSuccess(::publish).onFailure { Log.e("EventDispatcher", "event dropped", it) }
            }
            is Batch -> {
                val r = runCatching { batchSender(m.events) }
                if (!isActive) break
                r.onSuccess(::publish).onFailure { Log.e("EventDispatcher", "event batch dropped", it) }
            }
            is Block -> {
                val r = runCatching { m.body() }
                if (isActive) r.getOrNull()?.let { model.value = it }
                // Complete the awaiter with the body's value (null included) or its
                // exception; a thrown body reaches the awaiter, never kills the consumer.
                r.fold({ m.done.complete(it) }, { m.done.completeExceptionally(it) })
            }
        }
    }

    /** Test isolation: fresh process state (new queue and consumer, real send
     *  paths, no model or notice, boot not handled). Cancels the old queue so no
     *  [exclusive] awaiter hangs. */
    fun resetForTest() {
        val old = channel
        channel = newQueue()
        old.cancel()
        consumer.cancel()
        consumer = startConsumer(channel)
        sender = coreSender
        batchSender = coreBatchSender
        while (noticeChannel.tryReceive().isSuccess) {}
        model.value = null
        coreUnavailable.value = false
        bootHandled = false
        lastToday = null
        recoveredSidecar = null
        staleReported = false
    }

    private fun publish(r: SendResult) {
        model.value = r.view
        report(r)
    }

    /** Surface what the user must know about [r]: an entry that is not durably
     *  saved (rejected, or accepted but the log append failed) and, once per stale
     *  run, a view that is a stand-in or missing saved data. The queue paths call
     *  this themselves; an [exclusive] body calls it for each [SendResult] it gets. */
    fun report(r: SendResult) {
        if (!r.accepted || !r.persisted) noticeChannel.trySend(CoreNotice.NotSaved)
        if (r.stale && !staleReported) noticeChannel.trySend(CoreNotice.Unreadable)
        staleReported = r.stale
    }

    /** Enqueue an event for the serial consumer. UNLIMITED channel, so trySend
     *  never drops (it only fails on close, which never happens for this process
     *  singleton). */
    fun send(e: Event) {
        channel.trySend(Ev(e))
    }

    /** Enqueue [events] as ONE unit: applied under one core monitor turn with a
     *  single view decode and a single published model (Clear all data). */
    fun sendAll(events: List<Event>) {
        channel.trySend(Batch(events))
    }

    /** Run [body] as ONE exclusive turn of the serial consumer — no queued event
     *  interleaves with its sends — and publish its non-null result in queue order. */
    suspend fun exclusive(body: suspend () -> ViewModel?): ViewModel? {
        val done = CompletableDeferred<ViewModel?>()
        channel.trySend(Block(body, done))
        return done.await()
    }
}

/** A core outcome the user must hear about, posted by [EventDispatcher.report]. */
internal enum class CoreNotice {
    /** An entry the core rejected, or accepted in memory but couldn't write to the log. */
    NotSaved,

    /** What is shown stands in for a view that couldn't be read, or is missing part
     *  of the saved data. */
    Unreadable,
}

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
    /** A pending [OpenDest] from an open intent; [CoachScaffold] applies and clears it. */
    val openRequest: MutableStateFlow<OpenDest?>,
) {
    var loaded by loadedState
    var model by modelState
    var showSetup by showSetupState
    var setupInitial by setupInitialState
    var showTracker by showTrackerState
    var showResumePrompt by showResumePromptState
    var lastPain by lastPainState
}

@Composable
private fun rememberCoachSession(openRequest: MutableStateFlow<OpenDest?>): CoachSession {
    val ctx = LocalContext.current
    val res = LocalResources.current
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
                CoreNotice.NotSaved -> R.string.main_save_failed
                CoreNotice.Unreadable -> R.string.main_data_unreadable
            }
            Toast.makeText(ctx, text, Toast.LENGTH_LONG).show()
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
    val setupPrefs = remember { ctx.setupPrefs() }
    val markOnboardingDone = { setupPrefs.edit { putBoolean("onboarding_done", true) } }

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
            // below. Generating any plan (the plan prompt card, or a guided-setup
            // completion) clears the marker so auto-generation resumes normally.
            e is Event.ClearPlan -> setupPrefs.edit { putBoolean(KEY_PLAN_CLEARED, true) }
            e is Event.GeneratePlan -> setupPrefs.edit { putBoolean(KEY_PLAN_CLEARED, false) }
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
        )
    }

    // Boot hydration as ONE exclusive turn of the serial consumer: no queued event
    // interleaves with restore/SetToday/GeneratePlan. A re-run (rotation) queues after
    // it, skips the probe and the one-time block, and resends SetToday only when the
    // day or offset changed since the last send.
    LaunchedEffect(Unit) {
        var fresh = false
        val vm = EventDispatcher.exclusive {
            // A core that can't answer would boot onto an empty model (the setup screen
            // over a user who has data): show the error state instead.
            if (!EventDispatcher.bootHandled && !withContext(CoreJniDispatcher) { Core.probe() }) return@exclusive null
            fresh = withContext(CoreJniDispatcher) { Core.restore(ctx) }
            // The shell's clock enters the core as event data (no clock in-core).
            val bootToday = todayEpochDay() to utcOffsetSec()
            val published = EventDispatcher.model.value
            var v = if (EventDispatcher.bootHandled && published != null && EventDispatcher.lastToday == bootToday) {
                published
            } else {
                val today = withContext(CoreJniDispatcher) { Core.send(Event.SetToday(bootToday.first, bootToday.second)) }
                // A core that reads a view but refuses the clock can't apply anything either.
                if (!today.accepted) {
                    Core.markUnavailable()
                    return@exclusive null
                }
                // An unreadable log or a replay that had to skip lines surfaces here.
                EventDispatcher.report(today)
                EventDispatcher.lastToday = bootToday
                today.view
            }
            // A surviving program means a ClearPlan never landed: drop the stale marker.
            if (v.program != null && setupPrefs.getBoolean(KEY_PLAN_CLEARED, false)) {
                setupPrefs.edit { putBoolean(KEY_PLAN_CLEARED, false) }
            }
            // A log that ends with the plan removed re-arms a lost marker (first turn only).
            if (!EventDispatcher.bootHandled && v.program == null && Core.restoredPlanRemoved &&
                !setupPrefs.getBoolean(KEY_PLAN_CLEARED, false)
            ) {
                setupPrefs.edit { putBoolean(KEY_PLAN_CLEARED, true) }
            }
            // A set-up user without a plan gets one unless they removed it; a re-run
            // then sees the program and cannot fire twice.
            if (v.profile != null && v.program == null &&
                !setupPrefs.getBoolean(KEY_PLAN_CLEARED, false)
            ) {
                val plan = withContext(CoreJniDispatcher) { Core.send(Event.GeneratePlan(todayEpochDay())) }
                EventDispatcher.report(plan)
                v = plan.view
            }
            v
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
        if (!EventDispatcher.bootHandled) {
            EventDispatcher.bootHandled = true
            // A new process restored with the tracker flag set records nothing (the
            // service is NOT_STICKY): leave it so the resume prompt is the way back.
            if (!RunSession.tracking.value && !RunSession.armed.value && !RunSession.locating.value) {
                session.showTracker = false
            }
            // The guided setup only on a first run not yet completed or skipped.
            session.showSetup = fresh && !setupPrefs.getBoolean("onboarding_done", false) && vm.profile == null
            // A data import just restarted the process: confirm what it restored.
            val restoredEntries = setupPrefs.getInt(KEY_RESTORED_ENTRIES, -1)
            if (restoredEntries >= 0) {
                setupPrefs.edit { remove(KEY_RESTORED_ENTRIES) }
                UiMessages.post(
                    UiMessage(res.getQuantityString(R.plurals.main_restored_entries, restoredEntries, restoredEntries)),
                )
            }
            // An interrupted run's sidecar is offered, never auto-resumed; skipped when
            // a run is live in memory.
            if (!RunSession.tracking.value && RunSession.points.value.isEmpty() &&
                ActiveRunStore.hasActiveRun(ctx)
            ) {
                val recovered = withContext(Dispatchers.IO) { ActiveRunStore.recover(ctx) }
                val newestSec = recovered.maxOfOrNull { it.observedAt } ?: 0L
                val ageSec = System.currentTimeMillis() / 1000 - newestSec
                when {
                    // Under 2 fixes: nothing to resume.
                    recovered.size < 2 -> ActiveRunStore.clear()
                    // An old newest fix is a dead run; it must not become a multi-hour run.
                    ageSec > RESUME_STALE_SEC -> ActiveRunStore.clear()
                    // Ask; Resume reuses these fixes instead of re-reading the file.
                    else -> {
                        EventDispatcher.recoveredSidecar = recovered
                        session.showResumePrompt = true
                    }
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
                    val last = EventDispatcher.lastToday
                    if (last != null && last != now) {
                        EventDispatcher.lastToday = now
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
 * restarts the foreground service, or, without a precise-location grant (revoked
 * while the app was gone), holds the run for the tracker, which asks for the grant
 * and restarts recording once it is given. Composed at the very top of [CoachScaffold],
 * ahead of its full-screen setup/tracker returns, so a tracker screen restored
 * after process death can never hide it.
 */
@Composable
internal fun ResumeRunDialog(session: CoachSession) {
    val ctx = LocalContext.current
    // Remembered before the visibility return so the Resume coroutine outlives the
    // dialog leaving composition.
    val scope = rememberCoroutineScope()
    if (!session.showResumePrompt) return
    // A run started since boot owns the sidecar now (Start truncated the
    // interrupted one): resuming would replace its live track and discarding would
    // delete its crash-recovery file, so both just drop the stale offer.
    val liveRun = { RunSession.tracking.value || RunSession.armed.value }
    AppDialog(
        onDismiss = { /* deliberate choice required */ },
        title = { Text(stringResource(R.string.main_resume_run_title)) },
        text = { Text(stringResource(R.string.main_resume_run_message)) },
        confirmButton = {
            TextButton(onClick = {
                session.showResumePrompt = false
                if (liveRun()) {
                    EventDispatcher.recoveredSidecar = null
                    return@TextButton
                }
                scope.launch {
                    // Reuse the sidecar fixes read once at boot instead of re-reading
                    // the file off disk here.
                    val recovered = EventDispatcher.recoveredSidecar
                        ?: withContext(Dispatchers.IO) { ActiveRunStore.recover(ctx) }
                    EventDispatcher.recoveredSidecar = null
                    if (recovered.size >= 2) {
                        RunSession.restore(recovered)
                        if (hasFineLocation(ctx)) {
                            RunTrackingService.start(ctx)
                        } else {
                            // No grant (revoked mid-run, or a lapsed one-time grant): a
                            // start now would only fail the location FGS and strand the
                            // run. Hold it as a capture; the tracker asks for location and
                            // restarts recording once it is granted.
                            RunSession.tracking.value = false
                            RunSession.resumeAwaitingLocation.value = true
                        }
                        session.showTracker = true
                    } else {
                        ActiveRunStore.clear()
                    }
                }
            }) { Text(stringResource(R.string.track_action_resume)) }
        },
        dismissButton = {
            TextButton(onClick = {
                session.showResumePrompt = false
                EventDispatcher.recoveredSidecar = null
                if (!liveRun()) ActiveRunStore.clear()
            }) { Text(stringResource(R.string.main_discard), color = DangerText) }
        },
    )
}

@Composable
internal fun CoachScreen(openRequest: MutableStateFlow<OpenDest?>) {
    val session = rememberCoachSession(openRequest)

    val coreUnavailable by EventDispatcher.coreUnavailable.collectAsState()
    if (coreUnavailable) {
        CoreUnavailableScreen()
        return
    }

    if (!session.loaded) {
        val ctx = LocalContext.current
        // An import restart is still replaying the restored log (the key goes once boot runs).
        val restoring = remember {
            ctx.setupPrefs().getInt(KEY_RESTORED_ENTRIES, -1) >= 0
        }
        Box(Modifier.fillMaxSize().background(BgTop), contentAlignment = Alignment.Center) {
            Column(
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(Space.Card.dp),
            ) {
                CircularProgressIndicator(color = Accent)
                if (restoring) Text(stringResource(R.string.main_restoring_data), color = OnBgMuted, style = Type.Body)
            }
        }
        return
    }

    CoachScaffold(session)
}

/**
 * Boot found the native core unusable ([Core.probe]): say so instead of rendering an
 * empty model as a fresh install. Nothing here can dispatch an event (the core
 * refuses them anyway, [Core.unavailable]); the one action is the raw-log export,
 * which reads the durable log in pure Kotlin ([exportDataTo]), so the user's data
 * can leave the phone before an update or reinstall.
 */
@Composable
internal fun CoreUnavailableScreen() {
    val exportData = rememberDataExport()
    Box(
        Modifier.fillMaxSize().background(BgTop).safeDrawingPadding().padding(Space.Screen.dp),
        contentAlignment = Alignment.Center,
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(Space.Card.dp)) {
            SheetTitle(stringResource(R.string.main_core_unavailable_title))
            Text(stringResource(R.string.main_core_unavailable_message), color = OnBgBody, style = Type.Body)
            PrimaryButton(stringResource(R.string.profile_export_data), onClick = exportData, modifier = Modifier.fillMaxWidth())
        }
    }
}

