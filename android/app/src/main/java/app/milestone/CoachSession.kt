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
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
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
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
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
internal const val KEY_PLAN_CLEARED = "plan_cleared"

// Process-scoped serial event dispatcher. The queue AND its single consumer live
// here, at PROCESS scope, not in composition: a config change (rotation) cancels
// the Composable's coroutines, so a composition-scoped consumer would drop any
// event still queued-but-undrained at that instant (a half-applied Clear-all, a
// GPX import toasted as done before it ran, a guided-setup completion without its
// GeneratePlan).
//
// Strict serial order is the WHOLE ordering guarantee: `Core.send` is not
// reentrant-safe and the core's event log + state are order-sensitive, so exactly
// ONE send is ever in flight, on CoreJniDispatcher (single thread, keeps the
// core's thread_local view memos hot). Each resulting ViewModel is published to
// [model]; the composition collects that flow into its own `model` state, so an
// Activity recreation seeds the freshest snapshot with no spinner. Work that must
// run a Core.send sequence OFF the plain event queue as ONE atomic turn (boot
// hydration, a run save, a GPX import, the data import's log swap) goes through
// [exclusive]: the body runs as a single exclusive turn of the SAME serial
// consumer, so no queued event interleaves with its sends, and its non-null result
// is published in queue order — a rotation right after it is fresh too.
internal object EventDispatcher {
    private sealed interface Msg
    private class Ev(val e: Event) : Msg
    private class Batch(val events: List<Event>) : Msg
    private class Block(val body: suspend () -> ViewModel?, val done: CompletableDeferred<ViewModel?>) : Msg

    private val channel = Channel<Msg>(Channel.UNLIMITED)

    // The freshest hydrated ViewModel, or null before boot hydrates it. The
    // composition seeds `loaded`/`model` from this (rotation → last content
    // instantly, cold start → the splash holds while boot hydrates) and collects it
    // so every later event's model reaches the UI.
    val model = MutableStateFlow<ViewModel?>(null)

    // Core outcomes the user must hear about ([report]). A Channel, not a
    // SharedFlow: a notice sent while nothing collects (an Activity mid-recreation)
    // waits in the buffer for the next collector instead of being dropped.
    private val noticeChannel = Channel<CoreNotice>(Channel.BUFFERED)
    val notices: Flow<CoreNotice> = noticeChannel.receiveAsFlow()

    // Whether the last reported result was already stale, so a run of stale
    // results raises ONE notice rather than one per event.
    @Volatile private var staleReported = false

    // The once-per-process boot gate (guided-setup decision + run recovery),
    // hoisted here from a rememberSaveable local. Process-scoped: it survives
    // rotation (same process) so the boot effect doesn't re-run and clobber
    // mid-setup answers, and resets on process death — which is exactly when
    // the ActiveRunStore recovery check in the boot effect must re-run.
    var bootHandled = false

    // The crash-recovery sidecar fixes read ONCE at boot when a resume prompt is
    // offered, so the Resume action reuses them instead of re-reading the sidecar
    // file off disk. Null when no interrupted run was found / after it is consumed.
    var recoveredSidecar: List<GpsPoint>? = null

    // Process-level scope for work that must outlive a sheet/screen (imports,
    // export, the data restore).
    val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    // The event→view send paths, injectable so a JVM test can substitute fakes:
    // the real path crosses the JNI boundary (Core.send), which is unavailable off
    // device. A var so the consumer's drop-one-unit runCatching still wraps
    // whatever send path is installed.
    var sender: suspend (Event) -> SendResult = { e -> withContext(CoreJniDispatcher) { Core.send(e) } }
    var batchSender: suspend (List<Event>) -> SendResult =
        { es -> withContext(CoreJniDispatcher) { Core.sendAll(es) } }

    init {
        // One serial consumer, started once per process and never cancelled (a
        // SupervisorJob on the immediate main dispatcher). It drains the queue
        // STRICTLY IN ORDER: an [Ev] runs one Core.send, a [Batch] one Core.sendAll,
        // a [Block] an exclusive body as ONE turn. Either way exactly one unit of
        // work is ever in flight and results reach [model] in enqueue order.
        scope.launch {
            for (m in channel) when (m) {
                // A malformed view (or any send throw) must drop THIS unit, never
                // kill the process-lifetime consumer loop.
                is Ev -> runCatching { sender(m.e) }
                    .onSuccess(::publish)
                    .onFailure { Log.e("EventDispatcher", "event dropped", it) }
                is Batch -> runCatching { batchSender(m.events) }
                    .onSuccess(::publish)
                    .onFailure { Log.e("EventDispatcher", "event batch dropped", it) }
                is Block -> {
                    val r = runCatching { m.body() }
                    r.getOrNull()?.let { model.value = it }
                    // Complete the awaiter with the body's value (null included) or its
                    // exception; a thrown body reaches the awaiter, never kills the consumer.
                    r.fold({ m.done.complete(it) }, { m.done.completeExceptionally(it) })
                }
            }
        }
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
 * Boot + session state for the Coach screen, hoisted out of [CoachScreen] so the
 * screen splits into a state holder ([rememberCoachSession]) + chrome/router
 * ([CoachScaffold]). Every field DELEGATES to the exact MutableState the factory
 * created with remember / rememberSaveable, so each symbol keeps its original
 * persistence tier unchanged: the holder aliases the state, it never re-tiers it.
 */
@Stable
internal class CoachSession(
    loadedState: MutableState<Boolean>,
    modelState: MutableState<ViewModel>,
    showSetupState: MutableState<Boolean>,
    setupInitialState: MutableState<ProfileDraft?>,
    showTrackerState: MutableState<Boolean>,
    showResumePromptState: MutableState<Boolean>,
    lastPainState: MutableState<PainDetail?>,
    val dispatch: (Event) -> Unit,
    val dispatchAll: (List<Event>) -> Unit,
    val markOnboardingDone: () -> Unit,
) {
    var loaded by loadedState                     // remember (composition-local)
    var model by modelState                       // remember (composition-local)
    var showSetup by showSetupState               // rememberSaveable
    var setupInitial by setupInitialState         // rememberSaveable
    var showTracker by showTrackerState           // rememberSaveable
    var showResumePrompt by showResumePromptState // rememberSaveable
    var lastPain by lastPainState                 // remember (composition-local)
}

@Composable
private fun rememberCoachSession(openTrackingRequest: MutableStateFlow<Boolean>): CoachSession {
    val ctx = LocalContext.current
    // Replay the persisted event log; on a fresh install (the log file never
    // existed) seed a representative profile so the engine still renders content
    // on first frame. Core.restore distinguishes that from a log that exists but
    // compacted to empty (a returning user who cleared everything), the latter
    // must NOT be re-seeded through onboarding.
    // Core.restore replays the whole compacted log through JNI (a GPS run is
    // thousands of points), heavy enough to jank the first frame if done inline
    // during composition. Do it on a background dispatcher with a brief loading
    // state, then hydrate the model. `loaded` gates the UI until it's ready.
    // Seed from the process-level EventDispatcher.model so a rotation renders the
    // last content instantly instead of the blocking spinner; a genuine cold start
    // (flow still null) still shows the spinner once while the boot effect hydrates.
    val loadedState = remember { mutableStateOf(EventDispatcher.model.value != null) }
    val modelState = remember { mutableStateOf(EventDispatcher.model.value ?: ViewModel()) }
    // The process-level serial consumer publishes each post-event model to that
    // flow; collect it into the local state. On recreation this re-seeds `model`
    // from the freshest snapshot the process-level consumer holds (subsumes the old
    // bootCache), so no event drained while the previous Activity was being torn
    // down is ever lost. `loaded` stays owned by the boot effect below so the UI is
    // still gated until the once-per-process setup/recovery decision has run.
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
            Toast.makeText(ctx, ctx.getString(text), Toast.LENGTH_LONG).show()
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
    val setupInitialState = rememberSaveable { mutableStateOf<ProfileDraft?>(null) }
    val setupPrefs = remember { ctx.setupPrefs() }
    val markOnboardingDone = { setupPrefs.edit().putBoolean("onboarding_done", true).apply() }

    // rememberSaveable, not remember: a config change (rotation) recreates this
    // Activity, and a plain remember would reset this to false, bouncing the user
    // off the live tracking screen mid-run even though the foreground service keeps
    // recording. The saveable flag keeps them on the map across recreation.
    val showTrackerState = rememberSaveable { mutableStateOf(false) }

    // A crash-recovery sidecar is never auto-resumed anymore; the user is asked
    // first (consent), and a stale sidecar (newest fix older than RESUME_STALE_SEC)
    // is discarded outright so it can't silently become a garbage multi-hour run.
    val showResumePromptState = rememberSaveable { mutableStateOf(false) }

    // The (local epoch-day, utc-offset-sec) the core was last told is "today".
    // Seeded by the boot effect's SetToday below and re-sent on ON_RESUME whenever
    // the day rolls over or the offset changes (app left alive across midnight, or
    // travel). null until the boot effect's first send owns it, so the resume
    // observer can never fire before boot (which decides the fresh-install path).
    var lastToday by remember { mutableStateOf<Pair<Long, Int>?>(null) }

    // Shell echo of the most recent pain report, so the DO-NOT-TRAIN banner can
    // name the body part immediately after triage even before the core surfaces
    // its own `detail` string. Cleared when the hold is removed/readiness cleared.
    // Not persisted across process death (the core's persisted detail covers that).
    val lastPainState = remember { mutableStateOf<PainDetail?>(null) }

    // One dispatch path so shell-side echoes (lastPain) stay in lockstep with the
    // core. Every onEvent below routes through this instead of Core.send directly.
    //
    // `Core.send` (JNI update + view() + full kotlinx.serialization decode) is
    // moved OFF the main thread so a tap never blocks the UI as history grows. Events
    // are enqueued on the PROCESS-scoped [EventDispatcher], whose single consumer
    // drains them STRICTLY IN ORDER on CoreJniDispatcher (see its comment). Living
    // at process scope is what makes a mid-drain rotation lossless: the composition
    // is torn down, the queue + consumer are not. The synchronous shell-side echoes
    // below still run on the CALLER (main) thread, in call order, BEFORE the event
    // is enqueued, so their semantics are unchanged.
    val echo: (Event) -> Unit = { e ->
        when {
            e is Event.SubmitReadiness && e.pain != null -> lastPainState.value = e.pain
            e is Event.RemoveReadiness && e.signal == ReadinessSignal.Pain -> lastPainState.value = null
            // "I'm pain-free now" retracts every pain report: the banner's echo goes too.
            e is Event.ResolveHold && e.signal == ReadinessSignal.Pain -> lastPainState.value = null
            e is Event.ClearReadiness -> lastPainState.value = null
            // Plan-cleared marker (persisted): a "Remove plan" must STICK across
            // rotation AND relaunch, so the boot auto-GeneratePlan is gated on it
            // below. Generating any plan (the PlanPromptCard, or a guided-setup
            // completion) clears the marker so auto-generation resumes normally.
            e is Event.ClearPlan -> setupPrefs.edit().putBoolean(KEY_PLAN_CLEARED, true).apply()
            e is Event.GeneratePlan -> setupPrefs.edit().putBoolean(KEY_PLAN_CLEARED, false).apply()
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
        )
    }

    LaunchedEffect(Unit) {
        // The whole boot hydration runs as ONE exclusive turn of the serial
        // consumer: no queued event interleaves with restore/SetToday/GeneratePlan,
        // and the GeneratePlan gate below reads core state that no later event can
        // outrun. It always completes on the consumer, and a re-run boot block
        // (rotation) queues AFTER it.
        var fresh = false
        val vm = EventDispatcher.exclusive {
            fresh = withContext(CoreJniDispatcher) { Core.restore(ctx) }
            // A fresh install renders the empty Today and routes first-run into the
            // guided setup, which writes user-asserted profile values.
            // The shell's clock enters the core as event data so it can date the
            // plan week and pick today's next session (determinism; no clock
            // in-core). Sent on launch; the last-write-wins singleton keeps one
            // line. Its result view reflects the freshly restored state (SetToday
            // creates/destroys no lift/run/plan), so the boot gates below read it
            // directly instead of a separate currentView() round-trip.
            val bootDay = todayEpochDay()
            val bootOffset = utcOffsetSec()
            val today = withContext(CoreJniDispatcher) { Core.send(Event.SetToday(bootDay, bootOffset)) }
            // An unreadable log or a replay that had to skip lines surfaces here.
            EventDispatcher.report(today)
            var v = today.view
            // Remember what we told the core so the ON_RESUME observer below can
            // detect a midnight rollover / offset change and re-send (see [lastToday]).
            lastToday = bootDay to bootOffset
            // KEY_PLAN_CLEARED reconcile: [dispatch] writes the marker BEFORE the
            // ClearPlan event durably persists, so a process death in that gap can
            // leave marker=true while the plan is still alive in the replayed core.
            // The replayed core is the source of truth: a surviving program means
            // the ClearPlan never landed, so clear the stale marker to re-arm boot
            // auto-generation to match the visible state.
            if (v.program != null && setupPrefs.getBoolean(KEY_PLAN_CLEARED, false)) {
                setupPrefs.edit().putBoolean(KEY_PLAN_CLEARED, false).apply()
            }
            // Auto-generate the plan on boot for a set-up user who has NO plan
            // yet, so they land on the real prescription-led Coach/Today (next-session
            // hero + week strip + prescription headline) with ZERO manual taps. Gated on
            // `program == null` so an EXISTING plan is never touched here: re-firing
            // would otherwise re-anchor the week to today (stuck "week 1", wiped
            // done/missed history). The core also preserves an existing anchor as a
            // backstop; this gate additionally lets a session ClearPlan hold until the
            // next launch. Guided-setup completion fires its own GeneratePlan (below)
            // so the zero-tap promise also holds in the very first session.
            // The KEY_PLAN_CLEARED marker makes a "Remove plan" durable: once the user
            // removes their plan we do NOT auto-regenerate it here (on rotation OR a
            // later relaunch): they regenerate from the PlanPromptCard, which clears
            // the marker. Because a re-run boot block queues AFTER this exclusive turn,
            // it observes program != null and cannot double-fire GeneratePlan.
            if (v.profile != null && v.program == null &&
                !setupPrefs.getBoolean(KEY_PLAN_CLEARED, false)
            ) {
                val plan = withContext(CoreJniDispatcher) { Core.send(Event.GeneratePlan(todayEpochDay())) }
                EventDispatcher.report(plan)
                v = plan.view
            }
            v
        }!!
        session.model = vm
        // The consumer already published this hydrated model IN QUEUE ORDER (no
        // separate publish needed), so a later Activity recreation (rotation) renders
        // it instantly with no spinner and the model collector re-seeds `model`.
        // Everything below is one-time-per-process. On a rotation the effect
        // re-runs, but EventDispatcher.bootHandled is already true (process-scoped),
        // so we DON'T recompute showSetup (which would clobber mid-setup answers)
        // and DON'T re-offer run recovery. model + loaded are still refreshed above.
        if (!EventDispatcher.bootHandled) {
            EventDispatcher.bootHandled = true
            // A new process restored with the saved tracker flag set (Android killed
            // it mid-run) has nothing recording: RunTrackingService is NOT_STICKY.
            // Leave that idle tracker so the resume prompt below is the way back, not
            // a Start that would truncate the interrupted run's sidecar.
            if (!RunSession.tracking.value && !RunSession.armed.value && !RunSession.locating.value) {
                session.showTracker = false
            }
            // Offer the guided setup only on a genuine first run that hasn't already
            // completed/skipped it (persisted flag), and only while no profile exists.
            session.showSetup = fresh && !setupPrefs.getBoolean("onboarding_done", false) && vm.profile == null
            // A data import just restarted the process: confirm what it restored.
            val restoredEntries = setupPrefs.getInt(KEY_RESTORED_ENTRIES, -1)
            if (restoredEntries >= 0) {
                setupPrefs.edit().remove(KEY_RESTORED_ENTRIES).apply()
                UiMessages.post(
                    UiMessage(ctx.resources.getQuantityString(R.plurals.main_restored_entries, restoredEntries, restoredEntries)),
                )
            }
            // An interrupted GPS run's crash-durable sidecar. Do NOT auto-resume;
            // a sidecar left by a process/service kill can be hours stale and one
            // Stop&save would then write a garbage multi-hour run. Skipped when a run
            // is already live in memory (a mere config change, not a fresh process).
            if (!RunSession.tracking.value && RunSession.points.value.isEmpty() &&
                ActiveRunStore.hasActiveRun(ctx)
            ) {
                val recovered = withContext(Dispatchers.IO) { ActiveRunStore.recover(ctx) }
                val newestSec = recovered.maxOfOrNull { it.observedAt } ?: 0L
                val ageSec = System.currentTimeMillis() / 1000 - newestSec
                when {
                    // <2 fixes (crashed before a real route): nothing to resume.
                    recovered.size < 2 -> ActiveRunStore.clear()
                    // Newest fix is old: the app was gone long enough that this is
                    // almost certainly a dead run, not a genuine continuation. Drop it
                    // so it can't become a multi-hour logged run.
                    ageSec > RESUME_STALE_SEC -> ActiveRunStore.clear()
                    // Recent enough to plausibly resume: ASK the user (consent).
                    else -> {
                        // Cache the fixes read here so the Resume action reuses them
                        // instead of re-reading the sidecar file off disk.
                        EventDispatcher.recoveredSidecar = recovered
                        session.showResumePrompt = true
                    }
                }
            }
        }
        session.loaded = true
    }

    // These two effects belong to the session but must only be live once the boot
    // gate has passed (in the original CoachScreen they sat AFTER the `!loaded`
    // early-return). The `if (session.loaded)` guard reproduces that exactly: they
    // enter composition only when loaded, and never during the loading spinner.
    if (session.loaded) {
        // Live tracking state: the notification-tap reentry. The request flag is
        // consumed exactly once; it only opens the tracker when a run is genuinely
        // live (a stale notification tap after the run ended must not open a dead
        // screen).
        val openTracking by openTrackingRequest.collectAsState()
        LaunchedEffect(openTracking) {
            if (openTracking) {
                // Open the tracker whenever the service is live in ANY state:
                // recording, merely ARMED, or a locate-only preview. A Notification
                // tap or "Stop" while armed/locating (recording not yet started, so
                // `tracking` is still false) must land on the tracker so the user can
                // cancel it — consuming and dropping the request here would strand the
                // service (armed → auto-starts a run the user meant to cancel; locate →
                // GPS radio left on with its one visible affordance leading nowhere). A
                // stale tap after the service stopped opens nothing (all flags false).
                if (RunSession.tracking.value || RunSession.armed.value || RunSession.locating.value) {
                    session.showTracker = true
                }
                openTrackingRequest.value = false
            }
        }

        // An app process left alive across local midnight (or moved across a
        // timezone / DST boundary) keeps the core dating the plan to the day the boot
        // effect sent; "Next"/week-number/missed-status freeze on yesterday while the
        // shell-side week-strip today-ring has already advanced, so the two disagree.
        // Re-send SetToday on every ON_RESUME whose day or offset differs from the last
        // value we sent. Cheap no-op when nothing changed. Deliberately does NOT touch
        // the one-time boot logic (guided-setup decision + run recovery, gated on
        // bootHandled), it only re-dates the core clock; `lastToday == null` (boot
        // hasn't sent yet) is skipped so this can never pre-empt the fresh-install path.
        val lifecycleOwner = LocalLifecycleOwner.current
        DisposableEffect(lifecycleOwner) {
            val observer = LifecycleEventObserver { _, event ->
                if (event == Lifecycle.Event.ON_RESUME) {
                    val day = todayEpochDay()
                    val offset = utcOffsetSec()
                    val last = lastToday
                    if (last != null && (last.first != day || last.second != offset)) {
                        lastToday = day to offset
                        session.dispatch(Event.SetToday(day, offset))
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
 * restarts the foreground service. Composed at the very top of [CoachScaffold],
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
    AlertDialog(
        onDismissRequest = { /* deliberate choice required */ },
        shape = RoundedCornerShape(Space.Card.dp),
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
                        RunTrackingService.start(ctx)
                        session.showTracker = true
                    } else {
                        ActiveRunStore.clear()
                    }
                }
            }) { Text(stringResource(R.string.main_resume)) }
        },
        dismissButton = {
            TextButton(onClick = {
                session.showResumePrompt = false
                EventDispatcher.recoveredSidecar = null
                if (!liveRun()) ActiveRunStore.clear()
            }) { Text(stringResource(R.string.main_discard), color = LocalStatusColors.current.danger) }
        },
    )
}

@Composable
internal fun CoachScreen(openTrackingRequest: MutableStateFlow<Boolean>) {
    val session = rememberCoachSession(openTrackingRequest)

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

