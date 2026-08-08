package app.milestone

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.text.format.DateUtils
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.core.splashscreen.SplashScreen.Companion.installSplashScreen
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.SheetValue
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.util.Locale
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import androidx.core.view.WindowCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import org.osmdroid.config.Configuration

class MainActivity : ComponentActivity() {
    // Set true when the run-tracking notification's contentIntent (or any intent
    // carrying EXTRA_OPEN_TRACKING) delivers, CoachScreen observes it and, if a
    // run is genuinely live, reopens the tracking screen. A flow rather than a
    // plain field so an already-running Activity reacts to onNewIntent too.
    private val openTrackingRequest = MutableStateFlow(false)

    override fun onCreate(savedInstanceState: Bundle?) {
        // Theme-aware launch/splash frame. The manifest theme (Theme.Milestone)
        // already follows the SYSTEM light/dark via the values / values-night
        // split. If the user picked an explicit in-app DarkMode (Light/Dark),
        // force the matching splash theme here, BEFORE installSplashScreen(),
        // so the pre-Compose frame honors that choice too. Read straight from the
        // "theme"/"dark_mode" SharedPreferences (same keys ThemeSettings uses),
        // since the ThemeSettings flow isn't loaded this early.
        //
        // PLATFORM LIMITATION (accepted): on API 31+ the COLD-START splash is
        // drawn by SystemUI from the MANIFEST theme before this process even
        // runs, so an explicit Light/Dark override can NOT tint that very first
        // frame: it follows the SYSTEM setting for the whole system-drawn
        // splash, until our own window takes over. setTheme() here still
        // governs everything the app itself draws: the pre-31 splash, warm
        // starts, and the post-splash windowBackground (no opposite-color flash
        // once Compose loads). The only platform lever over the system splash,
        // UiModeManager.setApplicationNightMode(), is deliberately NOT used: it
        // recreates the app like a config change on every toggle and has no
        // documented follow-system reset for our third "System" choice.
        when (getSharedPreferences("theme", Context.MODE_PRIVATE).getString("dark_mode", null)) {
            "Light" -> setTheme(R.style.Theme_Milestone_Forced_Light)
            "Dark" -> setTheme(R.style.Theme_Milestone_Forced_Dark)
            else -> {} // System / unset → keep the night-qualified manifest theme
        }
        installSplashScreen()
        super.onCreate(savedInstanceState)
        // Draw edge-to-edge so the system bars are transparent and we control
        // their contrast per THEME (not the hardcoded navy in themes.xml, which
        // API 35+ ignores). Content is inset via statusBarsPadding / the nav-bar
        // inset in the chrome.
        enableEdgeToEdge()
        // osmdroid needs a config + user agent set before any MapView inflates.
        Configuration.getInstance().load(this, getSharedPreferences("osmdroid", Context.MODE_PRIVATE))
        Configuration.getInstance().userAgentValue = packageName
        ThemeSettings.load(this)
        if (intent?.getBooleanExtra(EXTRA_OPEN_TRACKING, false) == true) {
            openTrackingRequest.value = true
            // Consume it once: without this, every config change (rotation) re-reads
            // getIntent() and force-reopens the tracker mid-run.
            intent.removeExtra(EXTRA_OPEN_TRACKING)
            setIntent(intent)
        }
        setContent {
            MilestoneTheme {
                // Match the system-bar icon tint to the RESOLVED theme (which may be
                // forced Light on a dark OS or vice-versa) so the status/nav icons
                // never vanish into a same-tone bar. Dark ground → light icons.
                val view = LocalView.current
                val dark = LocalPalette.current.bgTop.luminance() < 0.5f
                SideEffect {
                    val window = (view.context as Activity).window
                    val controller = WindowCompat.getInsetsController(window, view)
                    controller.isAppearanceLightStatusBars = !dark
                    controller.isAppearanceLightNavigationBars = !dark
                }
                Surface(modifier = Modifier.fillMaxSize(), color = BgTop) {
                    CoachScreen(openTrackingRequest)
                }
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        if (intent.getBooleanExtra(EXTRA_OPEN_TRACKING, false)) {
            openTrackingRequest.value = true
            // Consume it so a later rotation doesn't re-force the tracker open.
            intent.removeExtra(EXTRA_OPEN_TRACKING)
            setIntent(intent)
        }
    }

    companion object {
        /** Intent extra asking the shell to reopen the live tracking screen. */
        const val EXTRA_OPEN_TRACKING = "app.milestone.extra.OPEN_TRACKING"
    }
}

// How old a crash-recovery sidecar's newest fix may be and still be offered
// for resume. Beyond this the app was gone long enough that the run is almost
// certainly dead, so the sidecar is discarded rather than resumed (a stale resume
// would let one Stop&save write a garbage multi-hour run). 3 hours.
private const val RESUME_STALE_SEC = 3L * 60L * 60L

// Persisted flag (in the "milestone_setup" prefs) recording that the user removed
// their plan. It gates the boot auto-GeneratePlan so a "Remove plan" survives both
// rotation and relaunch; generating any plan clears it. See the boot effect + dispatch.
private const val KEY_PLAN_CLEARED = "plan_cleared"

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun CoachScreen(openTrackingRequest: MutableStateFlow<Boolean>) {
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
    var loaded by remember { mutableStateOf(false) }
    var model by remember { mutableStateOf(ViewModel()) }

    // Guided setup: shown once on a true fresh install instead of auto-seeding
    // an opinionated profile. Saveable so it survives rotation mid-setup; gated
    // additionally by a persisted "onboarding_done" flag so a skip doesn't re-prompt
    // on the next launch (first-run only, per the plan).
    var showSetup by rememberSaveable { mutableStateOf(false) }
    // When non-null the guided setup opens PRE-FILLED from this draft (the Profile
    // "Re-run guided setup" row seeds it from the current profile). null = a genuine
    // first-run pass. Drives the onboarding-flag behaviour at the call site: only a
    // first-run pass (null) touches the onboarding pref on complete/skip.
    var setupInitial by rememberSaveable { mutableStateOf<ProfileDraft?>(null) }
    val setupPrefs = remember { ctx.getSharedPreferences("milestone_setup", Context.MODE_PRIVATE) }
    val markOnboardingDone = { setupPrefs.edit().putBoolean("onboarding_done", true).apply() }

    // rememberSaveable, not remember: a config change (rotation) recreates this
    // Activity, and a plain remember would reset this to false, bouncing the user
    // off the live tracking screen mid-run even though the foreground service keeps
    // recording. The saveable flag keeps them on the map across recreation.
    var showTracker by rememberSaveable { mutableStateOf(false) }

    // The one-time boot work (deciding showSetup, offering run recovery) must
    // run ONCE PER PROCESS, not on every Activity recreation. A rotation re-runs the
    // boot LaunchedEffect; without this guard it would recompute showSetup=false
    // (line "SetToday persisted → freshInstall=false") and clobber the rememberSaveable
    // guided-setup answers. Saveable so it survives the recreation the guard exists for.
    var bootHandled by rememberSaveable { mutableStateOf(false) }
    // A crash-recovery sidecar is never auto-resumed anymore; the user is asked
    // first (consent), and a stale sidecar (newest fix older than RESUME_STALE_SEC)
    // is discarded outright so it can't silently become a garbage multi-hour run.
    var showResumePrompt by rememberSaveable { mutableStateOf(false) }

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
    var lastPain by remember { mutableStateOf<PainDetail?>(null) }
    // "How evidence grading works" legend sheet, opened from the "?" on grade
    // badges. Static copy; no coaching logic.
    var showLegend by remember { mutableStateOf(false) }
    // Glossary bottom-sheet: the term key to open it scrolled to, or null
    // when closed. Opened from tappable term chips app-wide. Static UI copy;
    // definitions of jargon, NOT KB training claims.
    var glossaryTerm by remember { mutableStateOf<String?>(null) }
    // Which signal the readiness editor opens pre-selected when reached from a
    // Today "+ Add" chip (deep-link). Reset to the advanced default otherwise.
    var readinessInitial by remember { mutableStateOf(ReadinessSignal.WellnessZ) }

    // One dispatch path so shell-side echoes (lastPain) stay in lockstep with the
    // core. Every onEvent below routes through this instead of Core.send directly.
    val dispatch: (Event) -> Unit = { e ->
        when {
            e is Event.SubmitReadiness && e.pain != null -> lastPain = e.pain
            e is Event.RemoveReadiness && e.signal == ReadinessSignal.Pain -> lastPain = null
            e is Event.ClearReadiness -> lastPain = null
            // Plan-cleared marker (persisted): a "Remove plan" must STICK across
            // rotation AND relaunch, so the boot auto-GeneratePlan is gated on it
            // below. Generating any plan (the PlanPromptCard, or a guided-setup
            // completion) clears the marker so auto-generation resumes normally.
            e is Event.ClearPlan -> setupPrefs.edit().putBoolean(KEY_PLAN_CLEARED, true).apply()
            e is Event.GeneratePlan -> setupPrefs.edit().putBoolean(KEY_PLAN_CLEARED, false).apply()
        }
        model = Core.send(e)
    }

    LaunchedEffect(Unit) {
        val fresh = withContext(Dispatchers.IO) { Core.restore(ctx) }
        // No auto-seed anymore: a fresh install renders the empty Today and
        // routes first-run into the guided setup, which writes user-asserted
        // profile values rather than the old 45 km/wk-looking beginner defaults.
        var vm = withContext(Dispatchers.IO) { Core.currentView() }
        // The shell's clock enters the core as event data so it can
        // date the plan week and pick today's next session (determinism; no clock
        // in-core). Sent on launch; the last-write-wins singleton keeps one line.
        val bootDay = todayEpochDay()
        val bootOffset = utcOffsetSec()
        vm = withContext(Dispatchers.IO) { Core.send(Event.SetToday(bootDay, bootOffset)) }
        // Remember what we told the core so the ON_RESUME observer below can
        // detect a midnight rollover / offset change and re-send (see [lastToday]).
        lastToday = bootDay to bootOffset
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
        // the marker. Without this the effect (keyed on Unit) re-fires on every
        // rotation and re-anchors the week, wiping done/missed.
        if (vm.profile != null && vm.program == null &&
            !setupPrefs.getBoolean(KEY_PLAN_CLEARED, false)
        ) {
            vm = withContext(Dispatchers.IO) { Core.send(Event.GeneratePlan(todayEpochDay())) }
        }
        model = vm
        // Everything below is one-time-per-process. On a rotation the effect
        // re-runs, but bootHandled is already true (restored from saveable state),
        // so we DON'T recompute showSetup (which would clobber mid-setup answers)
        // and DON'T re-offer run recovery. model + loaded are still refreshed above.
        if (!bootHandled) {
            bootHandled = true
            // Offer the guided setup only on a genuine first run that hasn't already
            // completed/skipped it (persisted flag), and only while no profile exists.
            showSetup = fresh && !setupPrefs.getBoolean("onboarding_done", false) && vm.profile == null
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
                    else -> showResumePrompt = true
                }
            }
        }
        loaded = true
    }

    if (!loaded) {
        Box(Modifier.fillMaxSize().background(BgTop), contentAlignment = Alignment.Center) {
            CircularProgressIndicator(color = Accent)
        }
        return
    }

    // Live tracking state: drives the "run in progress" reentry chip below and
    // the notification-tap reentry here. The request flag is consumed exactly
    // once; it only opens the tracker when a run is genuinely live (a stale
    // notification tap after the run ended must not open a dead screen).
    val liveTracking by RunSession.tracking.collectAsState()
    val openTracking by openTrackingRequest.collectAsState()
    LaunchedEffect(openTracking) {
        if (openTracking) {
            if (RunSession.tracking.value) showTracker = true
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
                    dispatch(Event.SetToday(day, offset))
                }
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
    }

    if (showSetup) {
        // Guided setup replaces the whole scaffold on first run. Completing it
        // writes ONE SetProfile (same wire as the full editor); skipping leaves the
        // profile empty and drops into the normal empty Today. Either way the
        // onboarding flag is set so it never re-prompts.
        // GuidedSetup is a full-screen replacement (outside the Scaffold that
        // owns the status-bar inset), so under edge-to-edge its header would sit
        // under the status bar. Inset it here; GuidedSetup.kt itself is owned by
        // another agent, so the fix lives at the call site.
        Box(Modifier.fillMaxSize().background(BgTop).statusBarsPadding().navigationBarsPadding()) {
            // A re-run (setupInitial != null) must NOT touch the onboarding pref -
            // it's a returning user reviewing their answers, not first-run onboarding.
            val firstRun = setupInitial == null
            GuidedSetup(
                initial = setupInitial,
                onComplete = { draft ->
                    dispatch(draft.toEvent())
                    // Zero-tap after setup: now that a profile exists, generate the
                    // plan immediately so the user lands on the prescription-led
                    // Coach/Today in THIS session (not only after a relaunch). The
                    // core preserves this anchor on later boots.
                    dispatch(Event.GeneratePlan(todayEpochDay()))
                    if (firstRun) markOnboardingDone()
                    showSetup = false
                    setupInitial = null
                },
                onSkip = {
                    if (firstRun) markOnboardingDone()
                    showSetup = false
                    setupInitial = null
                },
            )
        }
        return
    }

    if (showTracker) {
        // The tracking screen replaces this whole scaffold, so it receives the
        // live model and re-pins the SafetyBanner itself (chrome §5: the banner is
        // on EVERY screen, never scrollable or dismissable). onEvent keeps the
        // banner's undo/Add-details paths live during a run.
        RunTrackingScreen(
            model = model,
            onEvent = { dispatch(it) },
            onFinish = { vm ->
                if (vm != null) model = vm
                showTracker = false
            },
        )
        return
    }

    // The bottom-nav destination. rememberSaveable so the selected tab survives a
    // config change (rotation). Saved as the enum NAME with an unknown-name
    // fallback: the default autoSaver Java-serializes the
    // enum, and restoring an icicle that still holds a REMOVED constant (a
    // pre-merge build's "Coach") would throw inside readObject; the same
    // decode-safe pattern as WorkoutType.fromWire.
    var selected by rememberSaveable(stateSaver = DestSaver) { mutableStateOf(Dest.Today) }

    // The one destructive action in the top-bar overflow (chrome §2): Clear all
    // data, gated behind a confirm dialog. Also mirrored at the bottom of
    // Profile (04-profile §4). The readiness clear keeps its own guarded
    // confirm because the safety banner's fallback undo path opens it.
    var confirmClearAll by remember { mutableStateOf(false) }
    var confirmReadiness by remember { mutableStateOf(false) }
    // Removing a pain hold now confirms first, symmetric with setting
    // it via triage; an accidental "remove" shouldn't silently drop a red flag.
    var confirmRemovePain by remember { mutableStateOf(false) }

    // Log bottom-sheet state. `sheetOpen` drives whether the ModalBottomSheet is
    // composed; `sheetMode` is which content it shows (the chooser first, then one
    // of the editors when picked). RememberSaveable so a rotation mid-entry
    // keeps the sheet OPEN on the same editor (the editors' own fields are already
    // rememberSaveable) instead of dropping the in-progress entry. LogMode is a
    // (Java-Serializable) enum, so the autoSaver bundles it like the calc sheet.
    var sheetOpen by rememberSaveable { mutableStateOf(false) }
    var sheetMode by rememberSaveable { mutableStateOf(LogMode.Chooser) }
    // Whether the currently-shown editor has unsaved edits, hoisted here so the
    // sheet's SWIPE-DOWN dismissal can honor the same "Discard this entry?" guard
    // the editor's X already enforces. The Set/Run editors report their dirty state
    // up; other quick forms leave it false (swipe-down just closes them).
    var sheetDirty by rememberSaveable { mutableStateOf(false) }
    var confirmDiscardSheet by remember { mutableStateOf(false) }
    val sheetState = rememberModalBottomSheetState(
        // A dirty editor blocks the swipe-to-hide and raises the discard
        // confirm INSTEAD, so the sheet stays VISIBLE under the dialog. That way
        // "Keep editing" (and an outside-tap on the dialog) leave the sheet in
        // place; no stranded invisible sheet / dead Log FAB. Discard closes it
        // via `sheetOpen=false` (removes the composable, bypassing this guard).
        confirmValueChange = { target ->
            if (target == SheetValue.Hidden && sheetDirty && sheetMode != LogMode.Chooser) {
                confirmDiscardSheet = true
                false
            } else {
                true
            }
        },
    )
    val scope = rememberCoroutineScope()
    // Animate the sheet closed, then drop it from composition once hidden.
    val dismissSheet: () -> Unit = {
        sheetDirty = false
        scope.launch { sheetState.hide() }.invokeOnCompletion {
            if (!sheetState.isVisible) sheetOpen = false
        }
    }

    Scaffold(
        containerColor = BgTop,
        topBar = {
            // Chrome §5 stacking order: status bar → SAFETY BANNER → app bar.
            // The DO-NOT-TRAIN banner is the topmost element below the status
            // bar on EVERY destination: it renders ABOVE the brand app bar,
            // never scrolls, never dismisses (HARD RULE 3 / INVARIANT 3).
            Column(Modifier.background(BgTop).statusBarsPadding()) {
                SafetyBanner(
                    model,
                    Modifier
                        .padding(horizontal = Space.Screen.dp)
                        .padding(top = Space.Md.dp),
                    holdDetail = painSubline(model, lastPain),
                    onClearReadiness = { confirmReadiness = true },
                    onRemovePain = { confirmRemovePain = true },
                    onAddDetails = { sheetMode = LogMode.Readiness; sheetOpen = true },
                )
                // Brand lockup IS the title on destinations (chrome §2): no
                // secondary title string anywhere in content.
                // "Clear all data" no longer lives in a global overflow; a
                // destructive action a tap away on every screen is too easy to hit.
                // It now lives ONLY at the bottom of Profile under a "Danger zone"
                // section (behind the confirm dialog). The overflow held nothing
                // else, so it's gone entirely.
                BrandTopBar()
            }
        },
        bottomBar = {
            Column {
                // Run-in-progress chip pinned just above the nav bar (chrome §4),
                // below content, never above the safety banner (which is topmost).
                if (liveTracking && !showTracker) {
                    RunInProgressChip { showTracker = true }
                }
                MilestoneNavBar(selected) { selected = it }
            }
        },
        floatingActionButton = {
            // The Log FAB belongs to Today only (chrome §1). Reopening always
            // resets to the chooser so the sheet never reappears mid-editor.
            if (selected == Dest.Today) {
                ExtendedFloatingActionButton(
                    onClick = {
                        sheetMode = LogMode.Chooser
                        sheetOpen = true
                    },
                    containerColor = Accent,
                    // Owner ruling (design/user-decisions.md): dark OnAccent on
                    // every accent fill: the board's #fff fails AA here.
                    contentColor = OnAccent,
                    shape = RoundedCornerShape(Space.Card.dp),
                    text = { Text("Log", style = Type.Body.copy(fontWeight = FontWeight.ExtraBold)) },
                    icon = {
                        Icon(
                            painterResource(R.drawable.ic_action_log_plus),
                            contentDescription = null,
                            modifier = Modifier.size(20.dp),
                        )
                    },
                )
            }
        },
    ) { pad ->
        ClearConfirmDialog(
            visible = confirmClearAll,
            title = "Clear all data?",
            message = "This permanently deletes all logged sets, runs, readiness entries and coaching history. This can't be undone.",
            confirmLabel = "Clear all",
            onDismiss = { confirmClearAll = false },
            onClear = {
                // Every logged family + derived coaching output; the profile
                // (training configuration) stays. ClearPlan was missing, so the
                // generated coaching plan (next_session / week / program) survived a
                // "Clear all data" and Coach still showed it. (There are no
                // Cooper/CriticalSpeed/APRE encoders in the shell yet: those
                // calculators have no UI, so nothing to clear for them here.)
                listOf(
                    Event.ClearSets, Event.ClearRuns, Event.ClearReadiness,
                    Event.ClearCheckins, Event.ClearReview, Event.ClearRacePrediction,
                    Event.ClearHypertrophyPlan, Event.ClearProtein, Event.ClearHrZones,
                    Event.ClearPlan,
                ).forEach { dispatch(it) }
            },
        )
        ClearConfirmDialog(
            visible = confirmReadiness,
            title = "Clear readiness inputs?",
            message = "This clears today's readiness inputs and every adjustment they produced, including any safety hold that blocks training. Re-log your readiness to restore it.",
            confirmLabel = "Clear",
            onDismiss = { confirmReadiness = false },
            onClear = { dispatch(Event.ClearReadiness) },
        )
        ClearConfirmDialog(
            visible = confirmRemovePain,
            title = "Remove the pain report?",
            message = "Only do this if it was logged by mistake. Removing it lifts the training hold.",
            confirmLabel = "Remove",
            onDismiss = { confirmRemovePain = false },
            onClear = { dispatch(Event.RemoveReadiness(ReadinessSignal.Pain)) },
        )
        // Crash-recovery consent. An interrupted run's sidecar is offered, never
        // silently resumed. Discard leaves nothing behind; Resume repopulates the
        // track and restarts the foreground service.
        if (showResumePrompt) {
            AlertDialog(
                onDismissRequest = { /* deliberate choice required */ },
                shape = RoundedCornerShape(Space.Card.dp),
                title = { Text("Resume your run?") },
                text = {
                    Text(
                        "An unfinished run was found from before the app closed. Resume tracking it, or discard it if it's not a run you want to keep.",
                    )
                },
                confirmButton = {
                    TextButton(onClick = {
                        showResumePrompt = false
                        scope.launch {
                            val recovered = withContext(Dispatchers.IO) { ActiveRunStore.recover(ctx) }
                            if (recovered.size >= 2) {
                                RunSession.restore(recovered)
                                RunTrackingService.start(ctx)
                                showTracker = true
                            } else {
                                ActiveRunStore.clear()
                            }
                        }
                    }) { Text("Resume") }
                },
                dismissButton = {
                    TextButton(onClick = {
                        showResumePrompt = false
                        ActiveRunStore.clear()
                    }) { Text("Discard", color = LocalStatusColors.current.danger) }
                },
            )
        }
        CompositionLocalProvider(
            LocalEvidenceLegend provides { showLegend = true },
            LocalGlossary provides { term -> glossaryTerm = term },
        ) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(pad),
        ) {
            // The safety banner lives in the top-bar slot ABOVE the app bar
            // (chrome §5 stacking) so a DO-NOT-TRAIN hold is the topmost element
            // below the status bar on every tab and can never scroll away.
            // Destinations are pure projections of the one hoisted `model`.
            Box(Modifier.weight(1f)) {
                when (selected) {
                    Dest.Today -> TodayDestination(
                        model = model,
                        onEvent = { dispatch(it) },
                        onStartSetup = {
                            // First-run entry point (no seed): keep the onboarding
                            // pref semantics at the call site above.
                            setupInitial = null
                            showSetup = true
                        },
                    )
                    Dest.History -> HistoryDestination(model = model, onEvent = { dispatch(it) })
                    Dest.Profile -> ProfileDestination(
                        ctx = ctx,
                        model = model,
                        onEvent = { dispatch(it) },
                        onClearAll = { confirmClearAll = true },
                        onRerunSetup = {
                            // Seed the wizard from the current profile (or SEED when
                            // none) and open it pre-filled. A re-run leaves the
                            // onboarding pref untouched (handled at the call site).
                            setupInitial = model.profile?.let { ProfileDraft.from(it) } ?: ProfileDraft.SEED
                            showSetup = true
                        },
                    )
                }
            }
        }

        if (sheetOpen) {
            ModalBottomSheet(
                // The dirty-editor guard lives in sheetState.confirmValueChange
                // (which keeps the sheet visible while confirming). By the time
                // onDismissRequest fires the hide is already committed on a clean
                // sheet, so this just finishes the close.
                onDismissRequest = { sheetDirty = false; sheetOpen = false },
                sheetState = sheetState,
                // Sheet ground a hair off BgElevated for separation (05-log §1);
                // on paper the elevated surface already separates.
                containerColor = if (LocalPalette.current.bgTop.luminance() < 0.5f) Color(0xFF1E1B18) else BgElevated,
            ) {
                LogSheetContent(
                    mode = sheetMode,
                    model = model,
                    initialReadinessSignal = readinessInitial,
                    // Returning to the chooser clears the dirty flag (the entry was
                    // handled or discarded via the editor's own guard).
                    onMode = { if (it == LogMode.Chooser) sheetDirty = false; sheetMode = it },
                    onEvent = { dispatch(it) },
                    onTrackRun = { showTracker = true },
                    onDismiss = dismissSheet,
                    onDirtyChange = { sheetDirty = it },
                )
            }
        }
        // The swipe-down discard confirmation for a dirty Log editor.
        if (confirmDiscardSheet) {
            AlertDialog(
                onDismissRequest = { confirmDiscardSheet = false },
                shape = RoundedCornerShape(Space.Card.dp),
                title = { Text("Discard this entry?") },
                confirmButton = {
                    TextButton(onClick = {
                        confirmDiscardSheet = false
                        sheetDirty = false
                        sheetOpen = false
                    }) { Text("Discard", color = LocalStatusColors.current.danger) }
                },
                dismissButton = {
                    TextButton(onClick = { confirmDiscardSheet = false }) { Text("Keep editing") }
                },
            )
        }
        // "How evidence grading works" legend, reached from the "?" on any
        // grade badge. Static reference copy; no coaching logic.
        if (showLegend) {
            ModalBottomSheet(
                onDismissRequest = { showLegend = false },
                sheetState = rememberModalBottomSheetState(),
                containerColor = if (LocalPalette.current.bgTop.luminance() < 0.5f) Color(0xFF1E1B18) else BgElevated,
            ) {
                EvidenceLegendSheet(model.grade_definitions)
            }
        }
        // Glossary: one app-wide sheet defining the jargon, opened from any
        // term chip. Static UI copy; no coaching logic, no KB claim.
        glossaryTerm?.let { term ->
            ModalBottomSheet(
                onDismissRequest = { glossaryTerm = null },
                sheetState = rememberModalBottomSheetState(),
                containerColor = if (LocalPalette.current.bgTop.luminance() < 0.5f) Color(0xFF1E1B18) else BgElevated,
            ) {
                GlossarySheet(term)
            }
        }
        }
    }
}

/** Decode-safe saver for the selected bottom-nav tab: stores the enum NAME and
 *  falls back to Today when the saved name no longer exists (e.g. "Coach"
 *  from a build before the 2026-08-03 Today+Coach merge). */
private val DestSaver = androidx.compose.runtime.saveable.Saver<Dest, String>(
    save = { it.name },
    restore = { name -> Dest.entries.firstOrNull { it.name == name } ?: Dest.Today },
)

/** Which content the Log bottom sheet shows: the chooser, or one editor. */
enum class LogMode { Chooser, Set, Run, Checkin, Readiness, Review, WeeklyCheckin, Pain }

/** Import cap: real GPX/TCX/FIT exports are well under this; the cap keeps a
 *  pathological or wrong-type file from OOMing the process or blowing up JSON
 *  allocation across the parseFit JNI boundary. */
private const val MaxImportBytes = 32 * 1024 * 1024 // 32 MB

/**
 * Read at most [MaxImportBytes] from [stream], throwing a [GpxImportException]
 * (surfaced as a toast) if the source is larger, so the bytes are size-checked
 * BEFORE they reach the parsers / JNI. minSdk-24-safe (a manual bounded copy, not
 * InputStream.readNBytes) and never buffers more than the cap.
 */
private fun readCappedBytes(stream: java.io.InputStream): ByteArray {
    val out = java.io.ByteArrayOutputStream()
    val buf = ByteArray(64 * 1024)
    var total = 0L
    while (true) {
        val n = stream.read(buf)
        if (n < 0) break
        total += n
        if (total > MaxImportBytes) {
            throw GpxImportException("That file is too large to import (max 32 MB)")
        }
        out.write(buf, 0, n)
    }
    return out.toByteArray()
}

/** Modality derivation from the echoed profile: a side is "shown" when it carries
 *  volume. Callers OR in `profile == null` wherever a not-yet-set-up user should
 *  still see everything (the chooser, calculators, history segment). */
private fun ViewModel.showLifting(): Boolean = (profile?.weekly_sets ?: 0) > 0
private fun ViewModel.showRunning(): Boolean = (profile?.running_days_per_week ?: 0) > 0

/**
 * Body of the Log [ModalBottomSheet] (05-log): the chooser list first, Report
 * pain pinned on top as the danger fast-path, then the picked editor. Each
 * editor renders the shared editor header (close · title · Save); closing an
 * editor returns to the chooser (with a discard confirm when dirty).
 */
@Composable
private fun LogSheetContent(
    mode: LogMode,
    model: ViewModel,
    initialReadinessSignal: ReadinessSignal,
    onMode: (LogMode) -> Unit,
    onEvent: (Event) -> Unit,
    onTrackRun: () -> Unit,
    onDismiss: () -> Unit,
    // The shown editor reports whether it has unsaved edits so the host's
    // swipe-down dismissal can guard it. Default no-op keeps other callers simple.
    onDirtyChange: (Boolean) -> Unit = {},
) {
    val status = LocalStatusColors.current
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    // GPX import (2026-08-03): pick a file, parse shell-side, hand the raw
    // points to the core via the SAME LogRunTrack event a live-tracked save
    // uses, distance/pace/splits/zone all derive in-core.
    val importGpx = rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.GetContent(),
    ) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        scope.launch {
            val result = withContext(Dispatchers.IO) {
                runCatching {
                    val bytes = ctx.contentResolver.openInputStream(uri)
                        ?.use { readCappedBytes(it) }
                        ?: throw GpxImportException("Couldn't read that file")
                    // Sniff the CONTENT, not the extension/mime (providers lie):
                    // the FIT header magic first (binary), then the XML root to
                    // tell GPX from TCX; anything else is rejected with a
                    // plain-language reason.
                    val events: List<Event.LogRunTrack> = if (isFitFile(bytes)) {
                        listOf(importedRunEvent(fitSegments(Core.parseFit(bytes)), model.profile?.measured_hr_max))
                    } else {
                        // BOM stripped before the pull-parser: it throws
                        // "content not allowed in prolog" on a BOM'd export.
                        val text = bytes.decodeToString().removePrefix("\uFEFF")
                        when {
                            text.contains("<TrainingCenterDatabase") -> {
                                // A TCX export can bundle several <Activity> blocks in
                                // one file (e.g. a multi-sport day), each Activity is
                                // its own run, not a pause inside one, so build one
                                // event per Activity instead of flattening them all
                                // into a single import. An Activity that fails to
                                // import (most commonly a route-only one with no
                                // timestamps) is skipped as long as at least one other
                                // Activity imports; if all of them fail, surface the
                                // first failure's message.
                                val activities = parseTcxActivities(text).filter { it.isNotEmpty() }
                                if (activities.isEmpty()) {
                                    throw GpxImportException("No GPS track found in this file")
                                }
                                var firstFailure: GpxImportException? = null
                                val built = activities.mapNotNull { segments ->
                                    runCatching { importedRunEvent(segments, model.profile?.measured_hr_max) }
                                        .onFailure { e ->
                                            if (firstFailure == null) firstFailure = e as? GpxImportException
                                            // A non-import exception here is a shell bug, not a
                                            // bad file; leave a diagnostic trail even when the
                                            // partial-import UX carries on.
                                            if (e !is GpxImportException) {
                                                android.util.Log.w("milestone", "TCX activity skipped on import", e)
                                            }
                                        }
                                        .getOrNull()
                                }
                                if (built.isEmpty()) {
                                    throw firstFailure ?: GpxImportException("Couldn't import this file")
                                }
                                built
                            }
                            text.contains("<gpx") -> listOf(importedRunEvent(parseGpx(text), model.profile?.measured_hr_max))
                            else -> throw GpxImportException("Not a GPX, TCX or FIT file")
                        }
                    }
                    // Re-importing the same file would silently double-count
                    // distance in weekly volume + the spike baseline (entry_id
                    // differs per import, so the core can't pair them). A run
                    // already ending at the exact same second IS the same run.
                    // Applied per event so one already-logged Activity in a
                    // multi-activity TCX doesn't block the rest from importing.
                    // distinctBy first: two Activities in the SAME file ending
                    // the same second (duplicated block from a buggy exporter)
                    // would both pass the model.runs check; the snapshot
                    // doesn't grow between dispatches.
                    val fresh = events
                        .distinctBy { it.observedAt }
                        .filter { ev -> model.runs.none { it.observed_at == ev.observedAt } }
                    if (fresh.isEmpty()) {
                        throw GpxImportException("Already imported: a logged run ends at the same time")
                    }
                    fresh
                }
            }
            result
                .onSuccess { events ->
                    // Each onEvent runs a full Core.send (JNI + event-log append +
                    // view decode): keep that off the main thread (Core.send is
                    // @Synchronized, so IO-thread dispatch is safe). Only the UI
                    // feedback (sheet dismiss) returns to main.
                    withContext(Dispatchers.IO) { events.forEach { onEvent(it) } }
                    onDismiss()
                }
                .onFailure { e ->
                    android.widget.Toast.makeText(
                        ctx,
                        (e as? GpxImportException)?.message ?: "Couldn't import this file",
                        android.widget.Toast.LENGTH_LONG,
                    ).show()
                }
        }
    }
    Column(
        modifier = Modifier
            .fillMaxWidth()
            // The tallest editor exceeds a ModalBottomSheet's max height; without
            // a scroll the fields below the fold would be unreachable.
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 18.dp)
            .padding(bottom = Space.Lg.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Md.dp + Space.Xs.dp),
    ) {
        when (mode) {
            LogMode.Chooser -> {
                // Report pain FIRST and visually distinct (05-log §1): the
                // safety fast-path must never be buried.
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(Space.Card.dp))
                        .background(status.danger)
                        // Open the triage sheet; nothing is
                        // submitted until "Report pain" inside it, so an accidental
                        // tap can't freeze the app.
                        .clickable { onMode(LogMode.Pain) }
                        .padding(Space.Card.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(Space.Card.dp),
                ) {
                    Icon(
                        painterResource(R.drawable.ic_safety_warning_triangle),
                        contentDescription = null,
                        tint = Color.White,
                        modifier = Modifier.size(24.dp),
                    )
                    Column(verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
                        Text(
                            "Report pain",
                            color = Color.White,
                            style = Type.Title.copy(fontSize = 17.sp, fontWeight = FontWeight.ExtraBold),
                        )
                        Text(
                            "Quick triage, then a safety hold",
                            color = DangerOn,
                            style = Type.Caption,
                        )
                    }
                }
                // "Today's plan" fast-path (owner 2026-08-08): the highlighted way
                // to start today's prescribed session, replacing the removed hero
                // action button. Only a trainable session dated today, with items,
                // reaches here; once it's logged the core advances next_session off
                // today (D1) so the tile disappears. Placed AFTER Report pain (the
                // safety fast-path stays first, 05-log §1) and before "Log set".
                val ns = model.next_session
                if (ns != null && ns.epoch_day == todayEpochDay() && ns.items.isNotEmpty() &&
                    (ns.status == "next" || ns.status == "adjusted")
                ) {
                    val nsDiscipline = sessionDiscipline(ns.session_type)
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .clip(RoundedCornerShape(Space.Card.dp))
                            .background(Accent)
                            .clickable {
                                if (nsDiscipline == "Run") {
                                    onTrackRun(); onDismiss()
                                } else {
                                    onMode(LogMode.Set)
                                }
                            }
                            .padding(Space.Card.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(Space.Card.dp),
                    ) {
                        Icon(
                            painterResource(
                                if (nsDiscipline == "Run") R.drawable.ic_content_run
                                else R.drawable.ic_content_set_dumbbell,
                            ),
                            contentDescription = null,
                            tint = OnAccent,
                            modifier = Modifier.size(24.dp),
                        )
                        Column(verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
                            Text(
                                "Today's plan: ${ns.title}",
                                color = OnAccent,
                                style = Type.Title.copy(fontSize = 17.sp, fontWeight = FontWeight.ExtraBold),
                            )
                            Text(
                                ns.items.first().summary,
                                color = OnAccent.copy(alpha = 0.75f),
                                style = Type.Caption,
                            )
                        }
                    }
                }
                // Modality-gate the lift/run entries so a run-only profile isn't
                // offered "Log set" (and vice-versa). A not-yet-set-up user
                // (profile == null) still sees everything. Pain / check-ins /
                // review / raw signal are modality-agnostic and always shown.
                val liftVisible = model.profile == null || model.showLifting()
                val runVisible = model.profile == null || model.showRunning()
                if (liftVisible) {
                    LogOptionRow(
                        "Log set", "Exercise, weight, reps, RPE",
                        R.drawable.ic_content_set_dumbbell, Accent, Accent.copy(alpha = 0.14f),
                    ) { onMode(LogMode.Set) }
                }
                if (runVisible) {
                    // Owner UX ruling (2026-07-28): every Log tile icon uses the theme
                    // Accent for consistency (Report pain stays danger). The two run
                    // tiles keep their distinct icons (person vs pin), both in Accent.
                    LogOptionRow(
                        "Log run", "Distance, duration, HR",
                        R.drawable.ic_content_run, Accent, Accent.copy(alpha = 0.14f),
                    ) { onMode(LogMode.Run) }
                    // Run import (2026-08-03): a run recorded on another device/app.
                    // Mime is "*/*": providers commonly report .gpx/.tcx/.fit as
                    // octet-stream; the content sniffer rejects anything else with a
                    // clear message.
                    LogOptionRow(
                        "Import run", "GPX, TCX or FIT file from Garmin, Strava, …",
                        R.drawable.ic_content_track_run, Accent, Accent.copy(alpha = 0.14f),
                    ) { importGpx.launch("*/*") }
                }
                LogOptionRow(
                    "Morning check-in", "How you slept, soreness, mood",
                    R.drawable.ic_content_readiness_heart, Accent, Accent.copy(alpha = 0.14f),
                ) { onMode(LogMode.Checkin) }
                LogOptionRow(
                    "Session review", "How this one workout went",
                    R.drawable.ic_content_review_list, Accent, Accent.copy(alpha = 0.14f),
                ) { onMode(LogMode.Review) }
                LogOptionRow(
                    "Weekly check-in", "A look back at the whole week",
                    R.drawable.ic_content_review_list, Accent, Accent.copy(alpha = 0.14f),
                ) { onMode(LogMode.WeeklyCheckin) }
                // Advanced / lab-data path: the raw-signal editor (enter a
                // z-score / bpm delta directly). Secondary to the human check-in.
                LogOptionRow(
                    "Advanced: log a raw signal", "z-score, bar velocity, lab data",
                    R.drawable.ic_content_readiness_heart, Accent, Accent.copy(alpha = 0.14f),
                ) { onMode(LogMode.Readiness) }
                if (runVisible) {
                    // Owner decision: GPS tracking reachable from the Log flow too
                    // (06-run-tracking launches from here).
                    LogOptionRow(
                        // Distinct location-pin icon, same Accent as every other Log
                        // tile (owner ruling): the two run actions read as related,
                        // told apart by icon (person vs pin), not by colour.
                        "Track run (GPS)", "Live map + route",
                        R.drawable.ic_content_track_run, Accent, Accent.copy(alpha = 0.14f),
                    ) {
                        onTrackRun()
                        onDismiss()
                    }
                }
            }
            LogMode.Set -> LogSetEditor(
                // Quick-picks: the user's own most recent exercises, newest
                // first (History list is oldest-first on the wire).
                recentExercises = model.lifts.asReversed().map { it.exercise }.distinct(),
                onClose = { onMode(LogMode.Chooser) },
                onDirtyChange = onDirtyChange,
            ) { set -> onEvent(set); onDismiss() }
            LogMode.Run -> LogRunEditor(
                onClose = { onMode(LogMode.Chooser) },
                onDirtyChange = onDirtyChange,
            ) { run -> onEvent(run); onDismiss() }
            // The primary human morning check-in. The core derives
            // the z-scores; the user never enters one.
            LogMode.Checkin -> MorningCheckinSheet(
                echo = model.checkin_today,
                onClose = { onMode(LogMode.Chooser) },
            ) { checkin -> onEvent(checkin); onDismiss() }
            LogMode.Readiness -> ReadinessEditor(
                // Red-flag fence position comes from the core's signal_groups.
                signalGroups = model.signal_groups.associate { it.signal to it.group },
                initialSignal = initialReadinessSignal,
                onClose = { onMode(LogMode.Chooser) },
            ) { r -> onEvent(r); onDismiss() }
            // Per-session review vs weekly check-in are now separate, correctly
            // titled sheets; both still emit SubmitReview (UI decomposition).
            LogMode.Review -> SessionReviewSheet(
                onClose = { onMode(LogMode.Chooser) },
            ) { review -> onEvent(review); onDismiss() }
            LogMode.WeeklyCheckin -> WeeklyCheckinSheet(
                onClose = { onMode(LogMode.Chooser) },
            ) { review -> onEvent(review); onDismiss() }
            // Pain triage → full PainDetail, then the hold.
            LogMode.Pain -> PainTriageSheet(
                onClose = { onMode(LogMode.Chooser) },
            ) { r -> onEvent(r); onDismiss() }
        }
    }
}

/**
 * A log-chooser row (05-log §1 rows 2–5): 38dp icon tile, title + subtitle,
 * trailing chevron. Full-width ≥48dp tap target on `BgElevated`.
 */
@Composable
private fun LogOptionRow(
    title: String,
    subtitle: String,
    iconRes: Int,
    iconTint: Color,
    iconGround: Color,
    onClick: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 48.dp)
            .clip(RoundedCornerShape(Space.Card.dp))
            .background(BgElevated)
            .border(1.dp, OnBgBody.copy(alpha = 0.07f), RoundedCornerShape(Space.Card.dp))
            .clickable { onClick() }
            .padding(Space.Card.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.Card.dp),
    ) {
        IconTile(painterResource(iconRes), iconTint, iconGround, size = 38.dp)
        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
            Text(title, color = OnBgBody, style = Type.Body.copy(fontWeight = FontWeight.Bold))
            Text(subtitle, color = OnBgFaint, style = Type.Caption)
        }
        RowChevron()
    }
}

/**
 * Today (01-today + 02-coach, merged, owner ruling 2026-08-03: the Coach tab
 * is folded into Today so one screen owns the day). Order: readiness strip →
 * today's call → adjustments & feedback (directly below the call, they can
 * carry safety-relevant guidance and must never sit below the fold) → plan
 * (next-session hero, week strip) → calculators → e1RM trend → recent
 * activity → quick tiles. States: B. empty "Get started" (then plan prompt +
 * calculators); C. safety override (the global banner dominates; latest
 * guidance + dimmed recent activity).
 *
 * "Today's call" renders the CORE-owned `today_headline` (safety hold >
 * adjustment > feedback > all-clear), prioritization is coaching logic and
 * lives in the core; the shell never computes it.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun TodayDestination(
    model: ViewModel,
    onEvent: (Event) -> Unit,
    onStartSetup: () -> Unit,
) {
    val status = LocalStatusColors.current
    val headline = model.today_headline
    // Last logged activity: the view lists are chronological oldest-first -
    // lastOrNull() is the most recent of each; newest-first on screen. Kept for
    // the e1RM-trend exercise pick + the hasActivity signal (the Recent-activity
    // list itself was removed 2026-08-04, it duplicated History).
    val lastLift = model.lifts.lastOrNull()
    val lastRun = model.runs.lastOrNull()
    // Removing the generated plan is confirm-gated (owner ruling 2026-08-04): the
    // old ProgramCard fired Event.ClearPlan instantly with no idea what it did.
    var confirmRemovePlan by rememberSaveable { mutableStateOf(false) }

    // Which calculator's form + results are shown in the bottom sheet. null =
    // sheet closed. Owner ruling (2026-07-28): calculator forms and their result
    // cards live in a ModalBottomSheet opened from the tile, never inline in the
    // main scroll, otherwise computing e.g. HR zones dumps ~7 EvidenceCards into
    // the list and re-creates the wall of text.
    var activeTool by rememberSaveable { mutableStateOf<CoachTool?>(null) }
    val toolSheetState = rememberModalBottomSheetState()

    val hasCoachSignal = model.feedback != null ||
        model.adjustments.isNotEmpty() ||
        model.review_adjustments.isNotEmpty() ||
        model.input_count > 0 ||
        // A morning check-in is coaching signal too; even pre-baseline
        // (the "collecting baseline" honesty state), and once derived signals land.
        model.checkin_today != null ||
        model.readiness_summary.isNotEmpty() ||
        model.baseline_status.isNotEmpty()
    val hasActivity = lastLift != null || lastRun != null
    val hasAnything = hasCoachSignal || hasActivity

    // e1RM trend for the most-recently-logged exercise: a factual series of the
    // core-derived e1RMs for that lift, oldest→newest.
    val trendExercise = lastLift?.exercise
    val trendSeries = model.lifts.filter { it.exercise == trendExercise }.map { it.e1rm_kg }

    // Modality gating: which calculators / trends are relevant. A
    // not-yet-set-up user (profile == null) sees every calculator.
    val showLifting = model.showLifting()
    val showRunning = model.showRunning()
    val calcAll = model.profile == null

    // Memoized so the Today list doesn't re-run the priority sort every
    // recomposition (mirrors the SafetyBanner lookup).
    val safetyCard = remember(model.adjustments) { dominantSafetyAdjustment(model) }

    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        // Extra bottom inset so the last row (empty-state caption, activity
        // card) clears the Log FAB.
        contentPadding = PaddingValues(
            // Extra bottom inset so the last row (tile labels / e1RM value /
            // empty-state caption) always clears the Log FAB, never occluded.
            start = Space.Screen.dp, end = Space.Screen.dp,
            top = Space.Screen.dp, bottom = 112.dp,
        ),
        verticalArrangement = Arrangement.spacedBy(Space.Screen.dp),
    ) {
        if (model.train_blocked) {
            // C. Safety override: the banner (global, pinned above) dominates;
            // content reduces to the latest safety guidance + dimmed activity.
            // Prefer the core's OWN hold resolution: build_headline's
            // "safety_hold" rung already picks the dominant source (onboarding
            // gate > readiness stop > review deferral) WITH its evidence -
            // including a gates-only medical-referral hold, which produces NO
            // adjustment row at all (BUGS.md 2026-08-03). The adjustment search
            // stays as the fallback.
            // Highest-PRIORITY safety row, not first-emitted.
            val hold = headline?.takeIf { it.kind == "safety_hold" && it.summary.isNotBlank() }
            if (hold != null || safetyCard != null) {
                item { SectionOverline("Latest guidance") }
                item {
                    // Safety hold card: no confidence meter; safety is a
                    // rule, not a probability. Grade + citation stay behind why?.
                    if (hold != null) {
                        EvidenceCard(
                            hold.summary, hold.grade, hold.citation,
                            hold.confidence, hold.safety_critical, hold.contested,
                            showConfidence = false,
                            why = hold.why,
                        )
                    } else if (safetyCard != null) {
                        EvidenceCard(
                            safetyCard.summary, safetyCard.grade, safetyCard.citation,
                            safetyCard.confidence, safetyCard.safety_critical, safetyCard.contested,
                            showConfidence = false,
                            why = safetyCard.why,
                        )
                    }
                }
            }
            // The train_blocked path keeps ONLY its safety card now: the dimmed
            // "Recent activity" copy was removed (owner ruling 2026-08-04: recent
            // activity duplicates the History tab).
            return@LazyColumn
        }

        // 1. Readiness strip: passive per-signal qualitative pills only (the
        // "+ Add" chips are gone; every signal is logged through the + FAB /
        // check-in). Deliberately NO ring and NO 0–100 number (INVARIANT 1).
        // Owner ruling 2026-08-04: render NOTHING when there is no readiness data
        // at all: gate on actual pills (measured signals or building baselines),
        // not `hasAnything`, so an empty strip never mounts (no caption either).
        val hasReadinessPills =
            model.readiness_summary.any { it.signal !in hardwareOnlySignals } ||
                model.baseline_status.isNotEmpty()
        if (hasReadinessPills) {
            item { ReadinessStrip(model, status) }
        }

        // 2. Today hero: one card merging Today's call + feedback +
        // adjustments + the next-session/plan-state card. The call/adjustments
        // portion only renders when there's signal; the plan-state card always
        // renders (a set-up user sees their next session; a profile-less user
        // sees the setup prompt). Evidence chrome is unchanged.
        item {
            TodayHeroCard(
                model = model,
                status = status,
                hasSignal = hasAnything,
                onEvent = onEvent,
                onStartSetup = onStartSetup,
            )
        }

        // Minimal empty state: no tiles; everything logs via the + FAB.
        // Shown only when there's genuinely nothing yet.
        if (!hasAnything) {
            item {
                Text(
                    "Nothing logged yet. Tap + to log a lift, a run, or how you feel.",
                    color = OnBgFaint,
                    style = Type.Caption,
                    modifier = Modifier.widthIn(max = 260.dp),
                )
            }
        }

        // 3. This week + plan footer, separate full-width items under the hero,
        // only when a plan exists (the hero already renders the next-session card
        // itself). Owner ruling 2026-08-04: the week overview sits ABOVE the plan
        // footer, and the plan summary is demoted to a compact plain-language
        // footer with a confirm-gated "Remove plan".
        val nextSession = model.next_session
        if (nextSession != null) {
            if (model.week_plan.isNotEmpty()) {
                item { SectionOverline("This week") }
                item { WeekStrip(model.week_plan) }
            }
            model.program?.let { prog -> item { ProgramCard(prog) { confirmRemovePlan = true } } }
        }

        // 4. Calculators: modality-gated tiles. Race predictor + HR zones
        // are running tools; Volume planner is a lifting tool; Protein is always
        // relevant. A not-yet-set-up user (profile == null) sees them all. Rows
        // are built from the visible tiles so a lone tile goes full-width: no
        // orphan half-width cell.
        run {
            val tiles = buildList<@Composable RowScope.() -> Unit> {
                if (calcAll || showRunning) {
                    add {
                        CoachToolTile(
                            "Race predictor",
                            cta = "Set a recent race",
                            value = model.race_prediction?.predicted,
                            unit = model.race_prediction?.goal_label,
                            selected = activeTool == CoachTool.Race,
                        ) { activeTool = CoachTool.Race }
                    }
                }
                if (calcAll || showLifting) {
                    add {
                        CoachToolTile(
                            "Volume planner",
                            cta = "Set a goal",
                            value = if (model.hypertrophy_plan.isNotEmpty()) "Planned" else null,
                            unit = null,
                            selected = activeTool == CoachTool.Volume,
                        ) { activeTool = CoachTool.Volume }
                    }
                }
                add {
                    // Owner ruling (2026-07-28): the tile face shows the COMPUTED
                    // value ("120–140 g/day"), read from the core's structured
                    // protein_figures (no prose scrape). A RED-S
                    // deficit refusal carries no g/day figure (refused), so it
                    // falls back to "Set g/day" (a result exists to reopen).
                    val proteinGrams = model.protein_figures
                        .firstOrNull { !it.refused }
                        ?.let { "${it.low_g_per_day.toInt()}–${it.high_g_per_day.toInt()}" }
                    CoachToolTile(
                        "Protein target",
                        cta = if (model.profile?.bodyweight_kg != null) "Compute protein" else "Add bodyweight",
                        value = proteinGrams ?: if (model.protein_targets.isNotEmpty()) "Set" else null,
                        unit = if (model.protein_targets.isNotEmpty()) "g/day" else null,
                        selected = activeTool == CoachTool.Protein,
                    ) { activeTool = CoachTool.Protein }
                }
                if (calcAll || showRunning) {
                    add {
                        // After compute, show the actual HRmax ("187 HRmax bpm"),
                        // read from the core's structured hr_max (no
                        // prose scrape, never recomputed). bpm is core-rounded.
                        val hrMax = model.hr_max?.bpm?.toInt()
                        CoachToolTile(
                            "HR zones",
                            cta = if (model.profile?.age_years != null) "Compute HR zones" else "Add your age",
                            value = when {
                                hrMax != null -> "$hrMax"
                                model.hr_zones.isNotEmpty() -> "Set"
                                else -> null
                            },
                            unit = when {
                                hrMax != null -> "HRmax bpm"
                                model.hr_zones.isNotEmpty() -> "zones"
                                else -> null
                            },
                            selected = activeTool == CoachTool.HrZones,
                        ) { activeTool = CoachTool.HrZones }
                    }
                }
            }
            if (tiles.isNotEmpty()) {
                item { SectionOverline("Calculators") }
                item {
                    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                        tiles.chunked(2).forEach { rowTiles ->
                            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                                rowTiles.forEach { it() }
                            }
                        }
                    }
                }
            }
        }

        // 5. e1RM trend card (01-today §A.3): a lift-progression viz, so
        // additionally gated on showLifting. Renders from the FIRST point.
        // Last section on Today (owner ruling 2026-08-04): the "Recent activity"
        // list was removed; it duplicated the History tab.
        if (trendExercise != null && trendSeries.isNotEmpty() && (calcAll || showLifting)) {
            item {
                E1rmTrendCard(trendExercise, trendSeries, lastLift?.e1rm_delta_kg, lastLift?.e1rm_direction)
            }
        }
    }

    // Remove-plan confirm (owner ruling 2026-08-04): honest copy, reuses the
    // ClearConfirmDialog pattern. Confirm dispatches Event.ClearPlan.
    ClearConfirmDialog(
        visible = confirmRemovePlan,
        title = "Remove this plan?",
        message = "Deletes the current plan. Your logged workouts are kept. It won't come back on its own. Generate a new plan whenever you're ready.",
        confirmLabel = "Remove plan",
        onDismiss = { confirmRemovePlan = false },
        onClear = { onEvent(Event.ClearPlan) },
    )

    // Calculator sheet (owner ruling 2026-07-28): the selected tool's form and its
    // result EvidenceCards live HERE, opened from the tile, never inline in the
    // main scroll. Full evidence chrome (grade chip, SAFETY/CONTESTED, why? with
    // confidence behind why?) is preserved; this only relocates where it renders.
    if (activeTool != null) {
        ModalBottomSheet(
            onDismissRequest = { activeTool = null },
            sheetState = toolSheetState,
            containerColor = if (LocalPalette.current.bgTop.luminance() < 0.5f) Color(0xFF1E1B18) else BgElevated,
        ) {
            CoachToolSheet(activeTool!!, model, onEvent)
        }
    }
}

/**
 * Today hero (owner declutter 2026-08-04): one card stack merging the former
 * separate "Today's call" + "Adjustments" + "Next session" cards. Top = the
 * core-owned headline (EvidenceCard, amber for an adjustment; PlainCard for the
 * ungraded all-clear) + the feedback card; then the adjustment list attached
 * directly under it (the isHeadline dedupe keeps the headline card from repeating);
 * then the plan-state card, the next session inline, else the plan prompt (profile
 * set) or the setup prompt (no profile). Evidence chrome is unchanged (summary +
 * SAFETY on the face, everything else behind the "?").
 *
 * [hasSignal] gates the call/feedback/adjustments block (matching the old
 * hasAnything gate) so a brand-new user sees only the setup prompt; the plan-state
 * card always renders. The train_blocked safety branch is handled by the caller and
 * never reaches here.
 */
@Composable
private fun TodayHeroCard(
    model: ViewModel,
    status: StatusColors,
    hasSignal: Boolean,
    onEvent: (Event) -> Unit,
    onStartSetup: () -> Unit,
) {
    val headline = model.today_headline
    val fb = model.feedback
    val showFeedback = fb != null && headline?.kind != "feedback"
    // Dedup key is (summary, citation): several rules emit the same summary
    // string, so matching on text alone would vanish the second rule's evidence.
    val isHeadline = { a: AdjustmentView ->
        headline?.kind == "adjustment" &&
            a.summary == headline.summary && a.citation == headline.citation
    }
    // Priority sort + headline dedupe is pure work over the wire lists, memoize
    // it so it doesn't re-run on every unrelated recomposition (only when the
    // adjustment lists or the headline actually change).
    val listedAdjustments = remember(model.adjustments, headline) {
        model.adjustments.byAdjustmentPriority().filterNot(isHeadline)
    }
    val listedReviewAdjustments = remember(model.review_adjustments, headline) {
        model.review_adjustments.byAdjustmentPriority().filterNot(isHeadline)
    }
    val nextSession = model.next_session

    Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp + Space.Xs.dp)) {
        if (hasSignal) {
            // Today's call, the core-owned headline. A downgrade-class ADJUSTMENT
            // renders AMBER (headline IS the call, no confidence meter on the
            // face); a graded feedback headline keeps the full EvidenceCard; the
            // ungraded all-clear is a plain card with no chrome.
            //
            // Hero dedupe (owner ruling 2026-08-04): a "prescription" headline
            // ("Next: Recovery run - 30 min · 76% HRmax") restates the very session
            // NextSessionCard renders below, so it was shown twice. Skip the
            // separate headline card for that kind; NextSessionCard IS the hero
            // face (its title serves as the headline). Every other kind
            // (safety_hold / adjustment / feedback / all_clear) is unchanged.
            if (headline != null && headline.kind != "prescription") {
                if (headline.grade.isNotEmpty()) {
                    val isAdjustment = headline.kind == "adjustment"
                    EvidenceCard(
                        headline.summary, headline.grade, headline.citation,
                        headline.confidence, headline.safety_critical, headline.contested,
                        // All chrome (grade + confidence) lives behind the "?"
                        // (owner ruling 2026-07-31); confidenceInWhy keeps the
                        // figure in-panel for the amber adjustment / graded feedback.
                        showConfidence = !isAdjustment,
                        confidenceInWhy = true,
                        container = if (isAdjustment) status.warn.copy(alpha = 0.12f) else BgElevated,
                        border = if (isAdjustment) status.warn.copy(alpha = 0.55f) else null,
                        why = headline.why,
                    )
                } else {
                    PlainCard {
                        Text(
                            headline.summary,
                            color = OnBgBody,
                            style = Type.Body.copy(fontWeight = FontWeight.Bold),
                        )
                    }
                }
            }
            if (showFeedback && fb != null) {
                EvidenceCard(fb.message, fb.grade, fb.citation, fb.confidence, fb.safety_critical, fb.contested, fb.category_label.ifBlank { null }, why = fb.why)
            }
            // Adjustments & feedback attached directly under the call: never below
            // the fold (owner ruling 2026-08-03: they can carry safety-relevant
            // guidance). The feedback card is NOT repeated (rendered once above); a
            // headline that IS an adjustment is skipped here (same card, one render).
            listedAdjustments.forEach {
                EvidenceCard(
                    it.summary, it.grade, it.citation, it.confidence, it.safety_critical, it.contested,
                    showConfidence = false, confidenceInWhy = true, why = it.why,
                )
            }
            listedReviewAdjustments.forEach {
                EvidenceCard(
                    it.summary, it.grade, it.citation, it.confidence, it.safety_critical, it.contested,
                    showConfidence = false, confidenceInWhy = true, why = it.why,
                )
            }
        }
        // Plan-state card, always. Lead with the concrete next session; else offer
        // to generate a plan (profile set) or route into guided setup (no profile -
        // the one setup entry point on this screen).
        when {
            nextSession != null -> NextSessionCard(nextSession)
            model.profile != null -> PlanPromptCard { onEvent(Event.GeneratePlan(todayEpochDay())) }
            else -> SetupPromptCard(onStartSetup)
        }
    }
}

// Signals that need hardware the app can't read yet: never shown as pills
// until an integration exists.
private val hardwareOnlySignals = setOf(
    "BarVelocity", "VelocityLoss", "AerobicDecoupling", "HrvCv",
)

/**
 * Readiness strip (01-today §A.1, amended for M6/m7 and the 2026-08-04 declutter):
 * overline + one qualitative pill per MEASURED signal (from `readiness_summary`),
 * plus honest "building baseline" pills for channels mid-collection. PASSIVE status
 * only; the "+ Add" chips are gone: every signal is logged through the + FAB
 * / check-in, not from the strip. No headline sentence here (it lives on the hero
 * card; dedupe), no "-" placeholders, hardware-only signals stay hidden until
 * an integration exists. No composite score (INVARIANT 1).
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun ReadinessStrip(model: ViewModel, status: StatusColors) {
    val measuredRows = model.readiness_summary.filter { it.signal !in hardwareOnlySignals }
    // Channels still collecting a baseline: shown honestly as a
    // "building baseline" pill instead of a fabricated number.
    val collecting = model.baseline_status
    // No empty-state here (owner ruling 2026-08-04, superseding the earlier "quiet
    // caption" decision): with zero readiness data the strip renders NOTHING, no
    // caption, no "Readiness -" line. The Today mount site skips this composable
    // entirely when [hasReadinessPills] is false (see the mount gate), so this only
    // ever runs with at least one pill to show.
    PlainCard {
        TileOverline("Readiness")
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp + Space.Xs.dp),
            verticalArrangement = Arrangement.spacedBy(Space.Sm.dp + Space.Xs.dp),
        ) {
            // Only signals that actually have data, states verbatim from the core.
            measuredRows.forEach { row ->
                SignalPill(
                    label = readinessPillLabel(row.signal),
                    state = if (row.group == "metric") "${row.state} · ${trimDecimal(row.value)}" else row.state,
                    color = if (row.safety_critical) status.dangerStrong else OnBgMuted,
                )
            }
            // Honest "building baseline - N of 7" pills for channels mid-collection.
            collecting.forEach { b ->
                SignalPill(
                    label = b.label,
                    state = "building baseline · ${b.have}/${b.need}",
                    color = OnBgFaint,
                )
            }
        }
    }
}

@Composable
private fun SignalPill(label: String, state: String, color: Color) {
    Row(
        modifier = Modifier
            .clip(RoundedCornerShape(100))
            .background(BgTop)
            .padding(horizontal = Space.Md.dp + Space.Xs.dp, vertical = Space.Sm.dp),
        horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
    ) {
        Text(label, color = OnBgFaint, style = Type.Caption)
        Text(state, color = color, style = Type.Caption.merge(TabularFigures))
    }
}

/**
 * A Coach calculator tile (02-coach §1): overline top-left; before use, a
 * one-line CTA in `OnBgFaint` with a trailing `→`; after use, the computed
 * value with its unit inline muted. Accent-outlined while its form is open.
 */
@Composable
private fun RowScope.CoachToolTile(
    label: String,
    cta: String,
    value: String?,
    unit: String?,
    selected: Boolean,
    onClick: () -> Unit,
) {
    Column(
        modifier = Modifier
            .weight(1f)
            .height(TileHeight)
            .clip(RoundedCornerShape(Space.Card.dp))
            .background(BgElevated)
            .then(
                if (selected) {
                    Modifier.border(1.5.dp, Accent, RoundedCornerShape(Space.Card.dp))
                } else {
                    Modifier.border(1.dp, OnBgBody.copy(alpha = 0.06f), RoundedCornerShape(Space.Card.dp))
                },
            )
            .clickable { onClick() }
            .padding(horizontal = Space.Card.dp, vertical = Space.Md.dp + Space.Xs.dp),
        verticalArrangement = Arrangement.SpaceBetween,
    ) {
        TileOverline(label)
        if (value == null) {
            Text("$cta →", color = OnBgFaint, style = Type.Body, maxLines = 1)
        } else {
            Row(verticalAlignment = Alignment.Bottom) {
                Text(
                    value,
                    color = OnBgBody,
                    style = Type.Title.copy(fontWeight = FontWeight.ExtraBold).merge(TabularFigures),
                    maxLines = 1,
                )
                if (unit != null) {
                    Text(" $unit", color = OnBgFaint, style = Type.Caption, maxLines = 1)
                }
            }
        }
    }
}

/**
 * e1RM trend card (01-today §A.3): overline `<exercise> · e1RM`, delta pill
 * top-right (`evidenceStrong`-tinted, neutral meaning, the direction judgment
 * stays the core's), 32sp value + inline unit, 140×40dp Sparkline with end dot.
 * Every number is core-derived and arrives on the wire.
 */
@Composable
private fun E1rmTrendCard(exercise: String, series: List<Double>, deltaKg: Double?, direction: String?) {
    val status = LocalStatusColors.current
    val latest = series.last()
    PlainCard {
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            TileOverline("$exercise · e1RM")
            Spacer(Modifier.width(Space.Xs.dp))
            GlossaryInfo("e1rm")
            Spacer(Modifier.weight(1f))
            if (deltaKg != null && direction != null) {
                val arrow = when (direction) {
                    "up" -> "▲"
                    "down" -> "▼"
                    else -> "–"
                }
                Text(
                    "$arrow ${trimDecimal(Math.abs(deltaKg))} kg",
                    color = OnBgBody,
                    style = Type.Chip.merge(TabularFigures),
                    modifier = Modifier
                        .clip(RoundedCornerShape(6.dp))
                        .background(status.evidenceStrong.copy(alpha = 0.22f))
                        .padding(horizontal = Space.Md.dp, vertical = Space.Sm.dp),
                )
            }
        }
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.Bottom,
        ) {
            Row(verticalAlignment = Alignment.Bottom) {
                Text(
                    trimDecimal(latest),
                    color = OnBgBody,
                    style = Type.Display.copy(fontWeight = FontWeight.ExtraBold).merge(TabularFigures),
                )
                Text(" kg", color = OnBgFaint, style = Type.Caption)
            }
            Sparkline(
                series.map { it.toFloat() },
                color = Accent,
                modifier = Modifier
                    .width(140.dp)
                    .height(40.dp),
                endDot = true,
            )
        }
    }
}

/**
 * Coach priority sort: safety-critical first, then strongest grade, then
 * confidence descending. Applied WITHIN each Coach group only.
 */
private fun List<AdjustmentView>.byAdjustmentPriority(): List<AdjustmentView> =
    sortedWith(
        compareByDescending<AdjustmentView> { it.safety_critical }
            .thenByDescending { gradeRank(it.grade) }
            .thenByDescending { it.confidence },
    )

/** The single highest-priority safety-critical adjustment, or null. Shared by the
 *  Today safety card and the global SafetyBanner so the lookup is defined once
 *  (wrap in `remember(model.adjustments)` at each call site). */
private fun dominantSafetyAdjustment(model: ViewModel): AdjustmentView? =
    model.adjustments.byAdjustmentPriority().firstOrNull { it.safety_critical }

private fun List<GuidanceView>.byGuidancePriority(): List<GuidanceView> =
    sortedWith(
        compareByDescending<GuidanceView> { it.safety_critical }
            .thenByDescending { gradeRank(it.grade) }
            .thenByDescending { it.confidence },
    )

/** The four on-demand Coach calculators, launched from the tile grid. */
private enum class CoachTool { Race, Volume, Protein, HrZones }

/**
 * Body of the Coach calculator [ModalBottomSheet] (owner ruling 2026-07-28): a
 * title, the tool's FORM (unchanged, same events/prefill), then its RESULT
 * EvidenceCards (identical to what used to render inline in the Coach scroll,
 * same Tanaka-split/extraDetail/why handling), then the "Clear …" action. Moving
 * these OUT of the Coach LazyColumn is what keeps the main scroll lean.
 */
@Composable
private fun CoachToolSheet(
    tool: CoachTool,
    model: ViewModel,
    onEvent: (Event) -> Unit,
) {
    val title = when (tool) {
        CoachTool.Race -> "Race predictor"
        CoachTool.Volume -> "Volume planner"
        CoachTool.Protein -> "Protein target"
        CoachTool.HrZones -> "HR zones"
    }
    Column(
        modifier = Modifier
            .fillMaxWidth()
            // The forms + result cards can exceed the sheet's max height; scroll so
            // everything below the fold stays reachable (mirrors LogSheetContent).
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 20.dp)
            .padding(bottom = Space.Lg.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Card.dp),
    ) {
        Text(title, color = OnBgBody, style = Type.Title)
        // The tool's FORM, same events/prefill as before, seeded from the core's
        // echoed query so it rehydrates after a cold start. Bodyweight is entered
        // HERE in the protein calculator, not on Profile (DEVIATIONS #4).
        when (tool) {
            CoachTool.Race -> RacePredictorForm(model.race_prediction) { onEvent(it) }
            CoachTool.Volume -> HypertrophyPlannerForm(model.hypertrophy_input) { onEvent(it) }
            // Person data lives on the profile now; prefill bodyweight/age from
            // it (override still allowed in the form).
            CoachTool.Protein -> ProteinForm(
                model.protein_input,
                profileBodyweightKg = model.profile?.bodyweight_kg,
            ) { bodyweight, masters, deficit ->
                onEvent(Event.ComputeProtein(bodyweight, masters, deficit))
            }
            CoachTool.HrZones -> HrZonesForm(
                model.hr_zone_input,
                profileAgeYears = model.profile?.age_years,
                profileRestingHrBpm = model.profile?.resting_hr_bpm,
            ) { age, rhr -> onEvent(Event.ComputeHrZones(age, rhr)) }
        }
        // The tool's RESULTS, the same graded EvidenceCards that used to render
        // inline in the Coach scroll, unchanged, plus the "Clear …" action.
        when (tool) {
            CoachTool.Race -> model.race_prediction?.let { rp ->
                EvidenceCard(
                    summary = rp.summary.ifBlank { "${rp.goal_label}: ${rp.predicted}" },
                    grade = rp.grade,
                    citation = rp.citation,
                    confidence = rp.confidence,
                    safetyCritical = rp.safety_critical,
                    contested = rp.contested,
                )
                // D2: the core's evidence-graded caveats (staleness re-test /
                // marathon under-mileage optimism): previously dropped on the wire.
                rp.notes.forEach { n ->
                    EvidenceCard(
                        n.summary, n.grade, n.citation, n.confidence,
                        n.safety_critical, n.contested, why = n.why,
                    )
                }
                TextButton(onClick = { onEvent(Event.ClearRacePrediction) }) {
                    Text("Clear prediction", style = Type.Body)
                }
            }
            CoachTool.Volume -> if (model.hypertrophy_plan.isNotEmpty()) {
                model.hypertrophy_plan.forEach {
                    EvidenceCard(it.summary, it.grade, it.citation, it.confidence, it.safety_critical, it.contested, it.section, why = it.why)
                }
                TextButton(onClick = { onEvent(Event.ClearHypertrophyPlan) }) {
                    Text("Clear plan", style = Type.Body)
                }
            }
            CoachTool.Protein -> if (model.protein_targets.isNotEmpty()) {
                model.protein_targets.forEach {
                    EvidenceCard(it.summary, it.grade, it.citation, it.confidence, it.safety_critical, it.contested, it.section, why = it.why)
                }
                TextButton(onClick = { onEvent(Event.ClearProtein) }) {
                    Text("Clear protein", style = Type.Body)
                }
            }
            CoachTool.HrZones -> if (model.hr_zones.isNotEmpty()) {
                // The HRmax row's "(Tanaka 208 − 0.7 × age)" formula lives in
                // why?; the card leads with the number, derivation behind it. Built
                // from the core's structured hr_max, not scraped: the
                // exact parenthetical is reconstructed from the same fields the core
                // rendered, then stripped off the estimate row's face. Only the
                // age-based estimate carries it (measured maxima carry no Tanaka).
                val hm = model.hr_max
                val tanakaParen = hm
                    ?.takeIf { !it.measured && it.tanaka_intercept > 0.0 }
                    ?.let {
                        "(Tanaka ${it.tanaka_intercept.toInt()} − ${trimDecimal(it.tanaka_slope)} × ${it.age_years.toInt()})"
                    }
                model.hr_zones.forEach { row ->
                    val paren = tanakaParen?.takeIf { row.summary.contains(it) }
                    val summary = if (paren != null) row.summary.replace(" $paren", "").trim() else row.summary
                    EvidenceCard(
                        summary, row.grade, row.citation, row.confidence,
                        row.safety_critical, row.contested, row.section,
                        extraDetail = paren?.removePrefix("(")?.removeSuffix(")"),
                        why = row.why,
                    )
                }
                TextButton(onClick = { onEvent(Event.ClearHrZones) }) {
                    Text("Clear zones", style = Type.Body)
                }
            }
        }
    }
}

/** The guidance rows that belong in the Reference library (everything the core
 *  emits under `guidance` that is NOT the one Profile-context block). Shared by
 *  the Profile "Evidence & references" screen. */
private fun referenceGuidance(model: ViewModel): List<GuidanceView> =
    model.guidance.filterNot { it.section == "Profile" }

/** How many cards the Reference library holds (guidance groups + reference defaults). */
private fun referenceLibraryCount(model: ViewModel): Int =
    referenceGuidance(model).size + model.reference.size

/**
 * The Reference library, moved out of Coach into a Profile-opened sheet (owner
 * ruling 2026-07-28). Renders the same evidence-graded programming-guidance
 * groups (STRENGTH/HYPERTROPHY/RUNNING/HYBRID/INDIVIDUALIZATION) and the old
 * Reference source list, full chrome intact (grade chip, SAFETY/CONTESTED,
 * why?). No coaching logic here; it only lays out core-emitted cards.
 */
@Composable
private fun ReferenceLibrarySheet(model: ViewModel) {
    val otherGuidance = referenceGuidance(model)
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 20.dp)
            .padding(bottom = Space.Lg.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Card.dp),
    ) {
        Text("Evidence & references", color = OnBgBody, style = Type.Title)
        Text(
            "Every programming rule the coach can apply, with its grade and sources. Read-only.",
            color = OnBgMuted,
            style = Type.Body,
        )
        if (referenceLibraryCount(model) == 0) {
            Text(
                "Nothing to show yet. Set a profile and log some training and the applicable rules appear here.",
                color = OnBgFaint,
                style = Type.Caption,
            )
        }
        // Collapsible per section (default collapsed): the library holds 60+
        // full evidence cards, so rendering every group at once was a wall. Each
        // section is now a tappable header (name + count); tapping one reveals
        // just that group's cards. Evidence chrome inside is untouched.
        otherGuidance.groupBy { it.section }.forEach { (section, rows) ->
            ReferenceSection(section, rows.size) {
                rows.byGuidancePriority().forEach {
                    EvidenceCard(it.summary, it.grade, it.citation, it.confidence, it.safety_critical, it.contested, why = it.why)
                }
            }
        }
        if (model.reference.isNotEmpty()) {
            ReferenceSection("Reference", model.reference.size) {
                model.reference.groupBy { it.section }.values.forEach { rows ->
                    rows.byGuidancePriority().forEach { ReferenceRow(it) }
                }
            }
        }
    }
}

/**
 * One collapsible group in the Reference library (default collapsed). Header =
 * accent overline + item count + a chevron that rotates when open; the body
 * (the section's evidence cards) mounts only while expanded, so the sheet opens
 * as a short list of section headers instead of a wall of every graded rule.
 */
@Composable
private fun ReferenceSection(title: String, count: Int, content: @Composable () -> Unit) {
    var open by rememberSaveable(title) { mutableStateOf(false) }
    Column(verticalArrangement = Arrangement.spacedBy(Space.Card.dp)) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(Space.Md.dp))
                .clickable { open = !open }
                .padding(vertical = Space.Sm.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
        ) {
            TileOverline(title)
            Text("$count", color = OnBgFaint, style = Type.Caption)
            Spacer(Modifier.weight(1f))
            Icon(
                painterResource(R.drawable.ic_ui_chevron_right),
                contentDescription = if (open) "Collapse" else "Expand",
                tint = OnBgFaint,
                modifier = Modifier
                    .size(16.dp)
                    .rotate(if (open) 90f else 0f),
            )
        }
        if (open) content()
    }
}

// ── Coach-as-planner composables ────────────────

/** Today's local epoch-day (days since 1970-01-01), for GeneratePlan/SetToday. */
private fun todayEpochDay(): Long = java.time.LocalDate.now().toEpochDay()

/** The device's current UTC offset in SECONDS east of UTC, so the core can
 *  bucket a logged session's UTC `observed_at` into the correct local day. Uses
 *  the offset at "now" (DST-correct for the current instant). */
private fun utcOffsetSec(): Int {
    val now = System.currentTimeMillis()
    return java.util.TimeZone.getDefault().getOffset(now) / 1000
}

/** "Mon 5 Jan" label for a plan day. */
private fun planDateLabel(epochDay: Long): String {
    val d = java.time.LocalDate.ofEpochDay(epochDay)
    val dow = d.dayOfWeek.getDisplayName(java.time.format.TextStyle.SHORT, java.util.Locale.US)
    val mon = d.month.getDisplayName(java.time.format.TextStyle.SHORT, java.util.Locale.US)
    return "$dow ${d.dayOfMonth} $mon"
}

/** Two-letter weekday for the week strip (Mo Tu We Th Fr Sa Su), unambiguous
 *  where a single narrow initial collides (M/T/T, S/S). */
private fun weekdayShort(epochDay: Long): String = when (
    java.time.LocalDate.ofEpochDay(epochDay).dayOfWeek
) {
    java.time.DayOfWeek.MONDAY -> "Mo"
    java.time.DayOfWeek.TUESDAY -> "Tu"
    java.time.DayOfWeek.WEDNESDAY -> "We"
    java.time.DayOfWeek.THURSDAY -> "Th"
    java.time.DayOfWeek.FRIDAY -> "Fr"
    java.time.DayOfWeek.SATURDAY -> "Sa"
    java.time.DayOfWeek.SUNDAY -> "Su"
}

/**
 * The Coach hero: today's concrete next session, made self-describing. Leads
 * with a "TODAY'S SESSION" / "NEXT SESSION" overline and a discipline icon
 * beside the title so a newcomer knows what the card IS. Each prescribed
 * exercise is a full EvidenceCard (grade chip, SAFETY/CONTESTED, confidence +
 * citation behind why?) rendered one surface step down (BgTop) so the nested
 * cards read as nested. A readiness-adjusted or blocked session wears a status
 * chip; a hold empties the items (the plan never renders load numbers through a
 * hold). Today's session is started from the "Today's plan" tile in the + Log
 * chooser, not from the hero.
 */
@Composable
private fun NextSessionCard(
    ns: SessionPlanView,
) {
    val status = LocalStatusColors.current
    val isToday = ns.epoch_day == todayEpochDay()
    val discipline = sessionDiscipline(ns.session_type)
    PlainCard {
        // Self-describing overline naming the card for a newcomer. FieldLabel's
        // quiet uppercase-chip style (Type.Chip, 1.2sp tracking, OnBgFaint).
        Text(
            if (isToday) "TODAY'S SESSION" else "NEXT SESSION",
            color = OnBgFaint,
            style = Type.Chip.copy(letterSpacing = 1.2.sp),
        )
        Row(verticalAlignment = Alignment.CenterVertically) {
            // Discipline icon tile: Run / Lift get a symbol; Rest gets none.
            val iconRes = when (discipline) {
                "Run" -> R.drawable.ic_content_run
                "Lift" -> R.drawable.ic_content_set_dumbbell
                else -> null
            }
            if (iconRes != null) {
                IconTile(
                    painterResource(iconRes),
                    Accent, Accent.copy(alpha = 0.14f), size = 36.dp,
                )
                Spacer(Modifier.size(Space.Md.dp))
            }
            Text(ns.title, color = OnBgBody, style = Type.Title, modifier = Modifier.weight(1f))
            // Today is already triple-marked in the week strip below; only a
            // future-dated session keeps its date label.
            if (!isToday) {
                Text(planDateLabel(ns.epoch_day), color = OnBgMuted, style = Type.Caption)
            }
        }
        when (ns.status) {
            "adjusted" -> Chip("ADJUSTED", status.warn)
            "blocked" -> Chip("ON HOLD", status.danger)
            else -> {}
        }
        // D2: the ADJUSTED chip used to appear unexplained. Surface the readiness
        // adjustment that reshaped the session, its own evidence card (grade chip,
        // SAFETY/CONTESTED, confidence behind why?), matching the amber Today card.
        if (ns.status == "adjusted") {
            ns.adjustment?.let { adj ->
                EvidenceCard(
                    adj.summary, adj.grade, adj.citation, adj.confidence,
                    adj.safety_critical, adj.contested,
                    showConfidence = false, confidenceInWhy = true, why = adj.why,
                    // One surface step down so the nested card reads as nested.
                    container = BgTop,
                )
            }
        }
        if (ns.items.isEmpty()) {
            Text(
                if (ns.status == "blocked") {
                    "Training is on hold. See Today's call."
                } else {
                    "Rest day."
                },
                color = OnBgMuted,
                style = Type.Body,
            )
        } else {
            ns.items.forEach { it ->
                val detail = listOf(it.anchored_on, it.adjusted_note)
                    .filter { s -> s.isNotBlank() }
                    .joinToString(" · ")
                val hasHrmax = it.summary.contains("HRmax")
                EvidenceCard(
                    summary = it.summary,
                    grade = it.grade,
                    citation = it.citation,
                    confidence = it.confidence,
                    safetyCritical = it.safety_critical,
                    contested = it.contested,
                    // Owner ruling (2026-07-28): prescription card FACES carry no
                    // confidence meter; the figure lives behind why?.
                    showConfidence = false,
                    confidenceInWhy = true,
                    extraDetail = detail.ifBlank { null },
                    why = it.why,
                    // One surface step down (BgTop) so the nested prescription card
                    // reads as nested inside this BgElevated PlainCard.
                    container = BgTop,
                    // Surface the HRmax glossary where the prescription mentions it.
                    glossaryKey = "hrmax".takeIf { hasHrmax },
                )
            }
        }
    }
}

/**
 * Plan footer (owner ruling 2026-08-04): the plan summary demoted from a hero
 * EvidenceCard to a compact plain-language line, human words, never the raw
 * "name · phase - week X of Y" internals. The "?" evidence disclosure is kept
 * (grade + citation + why? behind it, unchanged mechanism). "Remove plan" is a
 * quiet text action wired to a confirm dialog (never an instant ClearPlan).
 */
@Composable
private fun ProgramCard(prog: ProgramSummaryView, onRemove: () -> Unit) {
    // Keyed on the plan's identity so an open "?" panel resets when the plan
    // changes (mirrors EvidenceCard's saveable keying).
    var expanded by rememberSaveable(prog.name, prog.citation) { mutableStateOf(false) }
    Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
        ) {
            Text(
                planPlainLabel(prog),
                color = OnBgMuted,
                style = Type.Body,
                modifier = Modifier.weight(1f),
            )
            DisclosureButton(expanded) { expanded = !expanded }
        }
        if (expanded) {
            // Full evidence chrome behind the "?": grade badge, CONTESTED,
            // confidence, citation, why-lines (the same panel EvidenceCard opens).
            WhyDetail(
                why = prog.why,
                grade = prog.grade,
                citation = prog.citation,
                confidence = prog.confidence,
                contested = prog.contested,
                showConfidence = true,
                extraDetail = null,
            )
            // Remove plan, a destructive action, kept OFF the main face and
            // revealed only inside the "?" disclosure (owner ruling 2026-08-04).
            // Restyled OnBgMuted now that it's opt-in; confirm-dialog wiring
            // (onRemove → confirmRemovePlan) is unchanged.
            Text(
                "Remove plan",
                color = OnBgMuted,
                style = Type.Caption.copy(fontWeight = FontWeight.Bold),
                modifier = Modifier
                    .clip(RoundedCornerShape(Space.Sm.dp))
                    .clickable { onRemove() }
                    .padding(vertical = Space.Sm.dp, horizontal = Space.Xs.dp),
            )
        }
    }
}

/**
 * A plain-language one-liner for the plan footer: "Your plan: strength + running
 * · building phase, week 1 of 4". The core's `name` raw-concats "Hybrid -
 * strength + running"; take the descriptive tail after the em dash, and turn the
 * enum phase word into human copy. Never touches wire values, display only.
 */
private fun planPlainLabel(prog: ProgramSummaryView): String {
    val focus = prog.name.substringAfter(" - ", prog.name)
    val phaseWord = when (prog.phase) {
        "Base" -> "base phase"
        "Build" -> "building phase"
        "Peak" -> "peak phase"
        "Taper" -> "tapering"
        "Deload" -> "recovery week"
        else -> "${prog.phase.lowercase(Locale.US)} phase"
    }
    return "Your plan: $focus · $phaseWord, week ${prog.week} of ${prog.weeks_total}"
}

/** The "Plan my training" call-to-action shown when a profile exists but no plan
 *  has been generated yet. */
@Composable
private fun PlanPromptCard(onGenerate: () -> Unit) {
    PlainCard {
        Text("Get your next workout", color = OnBgBody, style = Type.Title)
        Text(
            "Build a dated week from your profile and logged training.",
            color = OnBgMuted,
            style = Type.Body,
        )
        Box(
            modifier = Modifier
                .fillMaxWidth()
                // Clip before clickable so the ripple stays within the rounded pill.
                .clip(RoundedCornerShape(Space.Md.dp))
                .background(Accent, RoundedCornerShape(Space.Md.dp))
                .clickable { onGenerate() }
                .padding(vertical = Space.Card.dp),
            contentAlignment = Alignment.Center,
        ) {
            Text("Plan my training", color = OnAccent, style = Type.Section)
        }
    }
}

/** Shown on Coach when no profile exists yet: the plan is built from the profile,
 *  so route the user into guided setup rather than leaving the plan section blank.
 *  (Profile-less users otherwise never saw the prescription flagship.) */
@Composable
private fun SetupPromptCard(onStartSetup: () -> Unit) {
    PlainCard {
        Text("Set up your training", color = OnBgBody, style = Type.Title)
        Text(
            "Answer a few questions and milestone builds a dated week of workouts.",
            color = OnBgMuted,
            style = Type.Body,
        )
        Box(
            modifier = Modifier
                .fillMaxWidth()
                // Clip before clickable so the ripple stays within the rounded pill.
                .clip(RoundedCornerShape(Space.Md.dp))
                .background(Accent, RoundedCornerShape(Space.Md.dp))
                .clickable { onStartSetup() }
                .padding(vertical = Space.Card.dp),
            contentAlignment = Alignment.Center,
        ) {
            Text("Start guided setup", color = OnAccent, style = Type.Section)
        }
    }
}

/** The week strip: 7 tappable day columns, each showing its weekday initial, the
 *  day-type token (Lift/Run/Rest, from the core's session_type), and a per-day
 *  status dot (filled = done/scheduled, hollow ring = planned-but-not-logged /
 *  missed). TODAY is ringed + accent-bold. Tapping a day reveals its sessions
 *  inline below. Every field is core-emitted; the "today" mark is the shell's
 *  own clock (already sent to the core via SetToday), not a fabricated value. */
@Composable
private fun WeekStrip(week: List<SessionPlanView>) {
    val status = LocalStatusColors.current
    val today = todayEpochDay()
    // The expanded day is keyed to its stable epoch-day, NOT a positional index:
    // a plan regeneration / week rollover reorders the list, and an unkeyed index
    // would silently re-attach the open detail to a different day. -1L = none open
    // (no real plan day is before 1970).
    var selectedDay by rememberSaveable { mutableStateOf(-1L) }
    Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
        Row(
            horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
            modifier = Modifier.fillMaxWidth(),
        ) {
            week.forEachIndexed { i, s ->
                // Per-day status → dot appearance. Filled = a settled call
                // (next/done/adjusted/blocked); a hollow ring = still open
                // (planned) or open-and-past (missed). GREEN (evidenceStrong)
                // marks a completed day, NOT hrZone1's teal, which means "easy
                // aerobic zone" everywhere else. Rest days render no dot at all
                // (handled below). "missed" wears a neutral hollow ring
                // (OnBgMuted), NOT danger red: a skipped planned day is not a
                // safety event, and red is reserved for real safety states (owner
                // ruling: red = safety). Only "blocked" (a safety hold) keeps danger.
                val (dotColor, dotFilled) = when (s.status) {
                    "next" -> Accent to true
                    "adjusted" -> status.warn to true
                    "blocked" -> status.danger to true
                    "done" -> status.evidenceStrong to true
                    "missed" -> OnBgMuted to false
                    "rest" -> OnBgFaint to false
                    else -> OnBgFaint to false // planned
                }
                val isToday = s.epoch_day == today
                val sel = selectedDay == s.epoch_day
                Column(
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(Space.Xs.dp),
                    modifier = Modifier
                        .weight(1f)
                        // Clip before clickable so the ripple stays rounded.
                        .clip(RoundedCornerShape(Space.Md.dp))
                        .background(
                            if (sel) Accent.copy(alpha = 0.12f) else Color.Transparent,
                            RoundedCornerShape(Space.Md.dp),
                        )
                        // TODAY is marked distinctly with an accent ring.
                        .then(
                            if (isToday) {
                                Modifier.border(1.dp, Accent, RoundedCornerShape(Space.Md.dp))
                            } else {
                                Modifier
                            },
                        )
                        .clickable { selectedDay = if (sel) -1L else s.epoch_day }
                        .padding(vertical = Space.Sm.dp, horizontal = Space.Xs.dp),
                ) {
                    Text(
                        weekdayShort(s.epoch_day),
                        color = if (isToday || sel) Accent else OnBgMuted,
                        style = if (isToday) {
                            Type.Caption.copy(fontWeight = FontWeight.Bold)
                        } else {
                            Type.Caption
                        },
                    )
                    // Day-type/label token, from the core's session_type.
                    Text(
                        sessionDiscipline(s.session_type),
                        color = OnBgFaint,
                        style = Type.Chip,
                        maxLines = 1,
                    )
                    if (s.status != "rest") {
                        Box(
                            Modifier
                                .size(8.dp)
                                .clip(RoundedCornerShape(4.dp))
                                .then(
                                    if (dotFilled) {
                                        Modifier.background(dotColor)
                                    } else {
                                        Modifier.border(1.5.dp, dotColor, RoundedCornerShape(4.dp))
                                    },
                                ),
                        )
                    } else {
                        // Rest days carry no dot: a hollow ring here read as an
                        // unsettled session where none exists. Keep an 8.dp
                        // placeholder so every column's height stays aligned.
                        Spacer(Modifier.size(8.dp))
                    }
                }
            }
        }
        val expanded = week.firstOrNull { it.epoch_day == selectedDay }
        if (expanded != null) {
            val s = expanded
            PlainCard {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(s.title, color = OnBgBody, style = Type.Body, modifier = Modifier.weight(1f))
                    Text(planDateLabel(s.epoch_day), color = OnBgMuted, style = Type.Caption)
                }
                if (s.items.isEmpty()) {
                    Text(
                        if (s.status == "rest") "Rest day." else "-",
                        color = OnBgMuted,
                        style = Type.Body,
                    )
                } else {
                    s.items.forEach {
                        Text("• ${it.summary}", color = OnBgMuted, style = Type.Body)
                    }
                }
            }
        }
    }
}

/** Which logged type History shows (INVARIANT 4: two options, no "All"). */
private enum class HistoryFilter { Lifts, Runs }

/** History (03-history): week stat strip → Lifts|Runs segments → LAST 12 WEEKS heatmap → entry cards. */
@Composable
private fun HistoryDestination(
    model: ViewModel,
    onEvent: (Event) -> Unit = {},
) {
    // Owner ruling (2026-07-28): default to whichever type was logged most
    // recently (newest observed_at). Falls back to Lifts when empty. A factual
    // shell-side default from the ViewModel, no coaching logic. rememberSaveable
    // preserves a manual segment pick across rotation.
    val defaultFilter = remember(model.lifts, model.runs) {
        val lastLift = model.lifts.maxOfOrNull { it.observed_at } ?: Long.MIN_VALUE
        val lastRun = model.runs.maxOfOrNull { it.observed_at } ?: Long.MIN_VALUE
        if (lastRun > lastLift) HistoryFilter.Runs else HistoryFilter.Lifts
    }
    var filter by rememberSaveable { mutableStateOf(defaultFilter) }
    val hasAny = model.lifts.isNotEmpty() || model.runs.isNotEmpty()
    // Modality filtering: show the Lifts|Runs segment only when BOTH sides
    // are relevant: either the profile programs that modality, or the user has
    // logged that type. Otherwise render the single available list with no
    // segment, defaulting to whichever side has data / is programmed.
    val liftsPresent = model.lifts.isNotEmpty()
    val runsPresent = model.runs.isNotEmpty()
    val showLifting = model.showLifting()
    val showRunning = model.showRunning()
    val showSegment = (showLifting || liftsPresent) && (showRunning || runsPresent)
    val activeFilter = if (showSegment) {
        filter
    } else {
        when {
            runsPresent && !liftsPresent -> HistoryFilter.Runs
            liftsPresent && !runsPresent -> HistoryFilter.Lifts
            showRunning && !showLifting -> HistoryFilter.Runs
            showLifting && !showRunning -> HistoryFilter.Lifts
            else -> defaultFilter
        }
    }
    // Tapping a card opens its detail/edit/delete sheet. We persist
    // only a lightweight selection KEY (kind + entry_id + observed_at), never the
    // whole view; a run's RunResultView carries ~0.5 MB of GPX, and bundling that
    // into the saved-state parcel risks TransactionTooLargeException on rotation.
    // The selected entry is re-resolved from the CURRENT model each recomposition,
    // so a rotation-while-selected re-reads fresh data; a delete-while-rotated
    // resolves to null and the sheet simply closes.
    var selectedKey by rememberSaveable { mutableStateOf<String?>(null) }
    val selected: HistoryEntry? = remember(selectedKey, model.lifts, model.runs) {
        resolveHistoryEntry(selectedKey, model)
    }
    EntryActionSheets(
        selected = selected,
        recentExercises = model.lifts.asReversed().map { it.exercise }.distinct(),
        onDismiss = { selectedKey = null },
        onEvent = onEvent,
    )
    // Per-exercise grouping computed ONCE per model change (not per LiftCard) so the
    // sparkline series lookup below is per-exercise, not a whole-model re-filter.
    val liftsByExercise = remember(model.lifts) { model.lifts.groupBy { it.exercise } }
    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(Space.Screen.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        // The specced structure (03-history: stat strip → filter → heatmap)
        // renders in EVERY state: empty just means zeroed tiles, an empty
        // heatmap track and the empty line below, never a bare string screen.
        // 1. Rolling-7-day stat strip: factual aggregates of the LAST 7 DAYS.
        // The strip is a "how's it going" glance (the historical views, 12-week
        // heatmap + full list: sit below); an all-time tonnage only ever grows
        // and stops meaning anything at a glance. A calendar-week window (owner
        // ruling 2026-08-04, SUPERSEDED) read 0 all week for a weekend runner
        // whose runs land in the prior locale week; a ROLLING 7 days (now − 7 days)
        // always reflects recent work. Reading a clock in the shell is fine: the
        // determinism HARD RULE governs `shared/`, not Kotlin (the heatmap below
        // already reads the wall clock for "today").
        item {
            val zone = java.time.ZoneId.systemDefault()
            val nowSec = System.currentTimeMillis() / 1000
            val windowStart = nowSec - 7L * 86400L
            val weekLifts = model.lifts.filter { it.observed_at >= windowStart }
            val weekRuns = model.runs.filter { it.observed_at >= windowStart }
            val tonnage = weekLifts.sumOf { it.weight_kg * it.reps } / 1000.0
            val runningKm = weekRuns.sumOf { it.distance_km }
            // Count SESSIONS, not individual sets: a day of lifting is one
            // session (group sets by local calendar day) + each run is its own
            // session. `weekLifts` is a set list, so its raw size over-counts.
            val sessionCount = run {
                val liftDays = weekLifts
                    .map { java.time.Instant.ofEpochSecond(it.observed_at).atZone(zone).toLocalDate() }
                    .toSet().size
                liftDays + weekRuns.size
            }
            Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                SectionOverline("Last 7 days")
                Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    StatTile("$sessionCount", null, "sessions")
                    StatTile(String.format(Locale.US, "%.1f", tonnage), "t", "tonnage", glossaryKey = "tonnage")
                    StatTile(String.format(Locale.US, "%.1f", runningKm), "km", "running")
                }
            }
        }
        // 2. Filter: two-option segmented Lifts | Runs, no "All", no counts.
        // Hidden when only one modality is relevant: the single available
        // list renders on its own.
        if (showSegment) {
            item {
                TwoSegmentRow(
                    "Lifts", "Runs",
                    selectedIndex = if (filter == HistoryFilter.Lifts) 0 else 1,
                ) { filter = if (it == 0) HistoryFilter.Lifts else HistoryFilter.Runs }
            }
        }
        // 3. Heatmap, LAST 12 WEEKS, 2dp cells, Accent intensity ramp.
        // Local-day bucketing happens here in the shell (device timezone)
        // so the core stays clock-free.
        item {
            Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                SectionOverline("Last 12 weeks")
                val tz = java.util.TimeZone.getDefault()
                val counts = remember(model.lifts, model.runs) {
                    val m = HashMap<Long, Int>()
                    (model.lifts.map { it.observed_at } + model.runs.map { it.observed_at })
                        .filter { it > 0 }
                        .forEach { sec ->
                            val day = Math.floorDiv(sec + tz.getOffset(sec * 1000) / 1000, 86400L)
                            m[day] = (m[day] ?: 0) + 1
                        }
                    m
                }
                val nowSec = System.currentTimeMillis() / 1000
                val today = Math.floorDiv(nowSec + tz.getOffset(nowSec * 1000) / 1000, 86400L)
                ContributionHeatmap(counts, today, weeks = 12)
            }
        }

        // 3b. Runs segment only, Garmin-style per-day distance bar chart, a
        // factual aggregation (sum of distance_km per local day) computed here
        // in the shell over the same local-day bucketing as the heatmap.
        if (activeFilter == HistoryFilter.Runs) {
            item {
                Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                    SectionOverline("Daily distance · last 14 days")
                    val tz = java.util.TimeZone.getDefault()
                    val kmByDay = remember(model.runs) {
                        val m = HashMap<Long, Double>()
                        model.runs
                            .filter { it.observed_at > 0 && it.distance_km > 0.0 }
                            .forEach { r ->
                                val day = Math.floorDiv(
                                    r.observed_at + tz.getOffset(r.observed_at * 1000) / 1000,
                                    86400L,
                                )
                                m[day] = (m[day] ?: 0.0) + r.distance_km
                            }
                        m
                    }
                    val nowSec = System.currentTimeMillis() / 1000
                    val today = Math.floorDiv(nowSec + tz.getOffset(nowSec * 1000) / 1000, 86400L)
                    RunDistanceBars(kmByDay, today, days = 14)
                }
            }
        }

        // 4. Entry cards, filtered to the active segment, most recent on top.
        if (activeFilter == HistoryFilter.Lifts && model.lifts.isNotEmpty()) {
            items(model.lifts.asReversed()) { l ->
                // The e1RM series for THIS exercise up to this entry: the card's
                // small sparkline is a factual sequence of core-derived numbers.
                // Read from the once-grouped map (precomputed above) instead of
                // re-scanning the WHOLE model.lifts per card (was O(N²)).
                val series = liftsByExercise[l.exercise].orEmpty()
                    .filter { it.observed_at <= l.observed_at }
                    .map { it.e1rm_kg }
                LiftCard(l, series, onClick = { selectedKey = historyEntryKey(HistoryEntry.Lift(l)) })
            }
        } else if (activeFilter == HistoryFilter.Lifts && hasAny) {
            item { Text("No lifts logged yet.", color = OnBgMuted, style = Type.Body) }
        }

        if (activeFilter == HistoryFilter.Runs && model.runs.isNotEmpty()) {
            items(model.runs.asReversed()) { RunCard(it, onClick = { selectedKey = historyEntryKey(HistoryEntry.Run(it)) }) }
        } else if (activeFilter == HistoryFilter.Runs && hasAny) {
            item { Text("No runs logged yet.", color = OnBgMuted, style = Type.Body) }
        }

        if (!hasAny) {
            item {
                Text(
                    "No sessions logged yet. Log a lift or run from the Today tab and it shows up here.",
                    color = OnBgMuted,
                    style = Type.Body,
                )
            }
        }
    }
}

/** Profile (04-profile): grouped training-profile rows, Appearance · Theme, settings,
 *  Evidence & references (the moved Reference library), Clear all data. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ProfileDestination(
    ctx: Context,
    model: ViewModel,
    onEvent: (Event) -> Unit,
    onClearAll: () -> Unit,
    onRerunSetup: () -> Unit,
) {
    val status = LocalStatusColors.current
    // Reference library, opened from the row below (owner ruling 2026-07-28:
    // moved off Coach). A ModalBottomSheet keeps it a read-only overlay.
    var showReferences by remember { mutableStateOf(false) }
    if (showReferences) {
        ModalBottomSheet(
            onDismissRequest = { showReferences = false },
            sheetState = rememberModalBottomSheetState(),
            containerColor = if (LocalPalette.current.bgTop.luminance() < 0.5f) Color(0xFF1E1B18) else BgElevated,
        ) {
            ReferenceLibrarySheet(model)
        }
    }
    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(Space.Screen.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Screen.dp),
    ) {
        // 1. Training profile, grouped rows; tapping a row opens its inline
        // editor (OptionList / scale rows, no dropdowns, no steppers as primary).
        // NOTE: no Bodyweight row, bodyweight lives in the Coach protein
        // calculator (owner defect note, DEVIATIONS #4).
        item {
            Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                SectionOverline("Profile")
                val initialProfile = model.profile?.let { ProfileDraft.from(it) } ?: ProfileDraft.SEED
                ProfileEditor(initial = initialProfile) { draft ->
                    onEvent(draft.toEvent())
                }
            }
        }

        // 1b. The core's evidence-cited profile-context rows ("Profile" guidance
        // section, training age from cadence etc.). Folded into ONE collapsed
        // section so they don't wall the Profile tab; expanding reveals the
        // same full-chrome EvidenceCards. No "Training age · …" until there's
        // logged history to base it on; on a zero-data day-1 user it derives from
        // a profile default, not from evidence about them.
        if (model.profile != null) {
            val hasHistory = model.lifts.isNotEmpty() || model.runs.isNotEmpty()
            val profileGuidance = model.guidance
                .filter { it.section == "Profile" }
                .filterNot { !hasHistory && it.summary.startsWith("Training age") }
                .byGuidancePriority()
            if (profileGuidance.isNotEmpty()) {
                item {
                    ExpandableSection("How the coach reads you", count = profileGuidance.size) {
                        Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                            profileGuidance.forEach {
                                EvidenceCard(deckSeparators(it.summary), it.grade, it.citation, it.confidence, it.safety_critical, it.contested, why = it.why)
                            }
                        }
                    }
                }
            }
        }

        // 2. Re-run guided setup: a whole-row ≥48dp entry that reopens the guided
        // wizard PRE-FILLED from the current profile. Moved up directly under the
        // Profile rows: it edits the same answers. The seed + onboarding
        // semantics live at the call site (setupInitial != null → a re-run that
        // leaves the onboarding pref untouched on skip).
        item {
            Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                SectionOverline("Setup")
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = 48.dp)
                        .clip(RoundedCornerShape(Space.Card.dp))
                        .background(BgElevated)
                        .border(1.dp, OnBgBody.copy(alpha = 0.07f), RoundedCornerShape(Space.Card.dp))
                        .clickable { onRerunSetup() }
                        .padding(Space.Card.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(Space.Card.dp),
                ) {
                    Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
                        Text("Re-run guided setup", color = OnBgBody, style = Type.Body.copy(fontWeight = FontWeight.Bold))
                        Text(
                            "Answer the setup questions again. Your current answers are pre-filled.",
                            color = OnBgFaint,
                            style = Type.Caption,
                        )
                    }
                    RowChevron()
                }
            }
        }

        // 3. Appearance, every appearance control under one overline: theme
        // swatch cards, the system-dark-mode override, and (API 31+) the
        // system-accent switch. By default light/dark follows the OS.
        item {
            Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                SectionOverline("Appearance")
                val currentTheme by ThemeSettings.theme.collectAsState()
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(9.dp),
                ) {
                    AppTheme.entries.forEach { t ->
                        ThemeSwatchCard(t, selected = t == currentTheme) {
                            ThemeSettings.setTheme(ctx, t)
                        }
                    }
                }

                // Dark-mode override (owner 2026-07-28). ON = follow the OS; OFF
                // reveals a Light|Dark segmented choice. Applies immediately, no
                // Apply button (user-decisions.md: Profile changes apply at once).
                val darkMode by ThemeSettings.darkMode.collectAsState()
                val sysDark = isSystemInDarkTheme()
                PlainCard {
                    Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.SpaceBetween,
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Column(
                                modifier = Modifier.weight(1f).padding(end = Space.Md.dp),
                                verticalArrangement = Arrangement.spacedBy(Space.Xs.dp),
                            ) {
                                Text("Respect system dark mode", color = OnBgBody, style = Type.Body)
                                Text(
                                    "Follow your phone's light/dark setting. Turn off to pin the app to one.",
                                    color = OnBgFaint,
                                    style = Type.Caption,
                                )
                            }
                            Switch(
                                checked = darkMode == DarkMode.System,
                                onCheckedChange = { on ->
                                    // Seed the manual choice with whatever the OS
                                    // currently shows so the appearance doesn't jump.
                                    ThemeSettings.setDarkMode(
                                        ctx,
                                        if (on) DarkMode.System
                                        else if (sysDark) DarkMode.Dark else DarkMode.Light,
                                    )
                                },
                            )
                        }
                        if (darkMode != DarkMode.System) {
                            SegmentedEnumRow(
                                label = "Appearance",
                                values = listOf(DarkMode.Light, DarkMode.Dark),
                                current = darkMode,
                                onSelect = { ThemeSettings.setDarkMode(ctx, it) },
                            )
                        }
                    }
                }

                // System accent, the third appearance control, under the same
                // overline. Gated to API 31+ where dynamic (wallpaper) colour
                // exists; safety colours never follow it.
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                    PlainCard {
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.SpaceBetween,
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Column(
                                modifier = Modifier.weight(1f).padding(end = Space.Md.dp),
                                verticalArrangement = Arrangement.spacedBy(Space.Xs.dp),
                            ) {
                                Text("Use system accent color", color = OnBgBody, style = Type.Body)
                                Text(
                                    "Match the app's accent to your phone's wallpaper colour. Safety colours never change.",
                                    color = OnBgFaint,
                                    style = Type.Caption,
                                )
                            }
                            val dynamicAccent by ThemeSettings.dynamicAccent.collectAsState()
                            Switch(
                                checked = dynamicAccent,
                                onCheckedChange = { ThemeSettings.setDynamicAccent(ctx, it) },
                            )
                        }
                    }
                }
            }
        }

        // 4. Running · Units, the distance unit + the live pace-bucket size.
        // Both apply immediately (user-decisions.md: Profile changes apply at once).
        item {
            Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                SectionOverline("Running · Units")
                PlainCard {
                    Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                        // Distance units: Auto follows the phone locale (mi for
                        // US/UK/Liberia/Myanmar, else km); Km/Mi pin it.
                        val unitOverride by ThemeSettings.distanceUnitOverride.collectAsState()
                        SegmentedEnumRow(
                            label = "Distance units",
                            values = listOf(
                                DistanceUnitOverride.System,
                                DistanceUnitOverride.Km,
                                DistanceUnitOverride.Mi,
                            ),
                            current = unitOverride,
                            display = {
                                when (it) {
                                    DistanceUnitOverride.System -> "Auto"
                                    DistanceUnitOverride.Km -> "km"
                                    DistanceUnitOverride.Mi -> "mi"
                                }
                            },
                            onSelect = { ThemeSettings.setDistanceUnitOverride(ctx, it) },
                        )
                        // Live pace-bucket size (minutes): the run screen groups
                        // pace into buckets of this many minutes of moving time.
                        val bucketMin by ThemeSettings.paceBucketMinutes.collectAsState()
                        Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
                            Text("Live pace split", color = OnBgBody, style = Type.Body)
                            Text(
                                "On the run screen, recent pace is grouped into buckets of this many minutes.",
                                color = OnBgFaint,
                                style = Type.Caption,
                            )
                            ScrollableScaleRow(
                                options = listOf(1, 2, 3, 4, 5, 6, 8, 10),
                                current = bucketMin,
                                render = { "$it min" },
                                onSelect = { ThemeSettings.setPaceBucketMinutes(ctx, it) },
                            )
                        }
                    }
                }
            }
        }

        // 5. Evidence & references, the Reference library, moved here off Coach
        // (owner ruling 2026-07-28). A whole-row ≥48dp tap target opening the
        // read-only study wall (grade chips + why? intact) as a sheet.
        item {
            Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                SectionOverline("Evidence")
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = 48.dp)
                        .clip(RoundedCornerShape(Space.Card.dp))
                        .background(BgElevated)
                        .border(1.dp, OnBgBody.copy(alpha = 0.07f), RoundedCornerShape(Space.Card.dp))
                        .clickable { showReferences = true }
                        .padding(Space.Card.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(Space.Card.dp),
                ) {
                    Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
                        Text("Evidence & references", color = OnBgBody, style = Type.Body.copy(fontWeight = FontWeight.Bold))
                        Text(
                            "The programming rules the coach can apply, with grades and sources.",
                            color = OnBgFaint,
                            style = Type.Caption,
                        )
                    }
                    val count = referenceLibraryCount(model)
                    if (count > 0) {
                        Text(
                            "$count",
                            color = OnBgFaint,
                            style = Type.Chip.merge(TabularFigures),
                            modifier = Modifier
                                .clip(RoundedCornerShape(6.dp))
                                .background(BgTop)
                                .padding(horizontal = Space.Md.dp, vertical = Space.Xs.dp),
                        )
                    }
                    RowChevron()
                }
            }
        }

        // 6. Danger zone: the ONE destructive action, at the very bottom of Profile
        // It used to also sit a tap away in a global top-bar overflow; that's
        // gone. A "Danger zone" overline + the confirm dialog keep it from being hit
        // by accident.
        item {
            Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                SectionOverline("Danger zone")
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = 48.dp)
                        .clip(RoundedCornerShape(Space.Card.dp))
                        .border(1.dp, status.danger.copy(alpha = 0.4f), RoundedCornerShape(Space.Card.dp))
                        .clickable { onClearAll() }
                        .padding(Space.Card.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(Space.Xs.dp),
                ) {
                    Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
                        Text(
                            "Clear all data",
                            color = status.danger,
                            style = Type.Body.copy(fontWeight = FontWeight.SemiBold),
                        )
                        Text(
                            "Permanently deletes every logged set, run, readiness entry, check-in and the coaching plan. Your profile settings stay. Can't be undone.",
                            color = OnBgFaint,
                            style = Type.Caption,
                        )
                    }
                }
            }
        }
    }
}

/**
 * A [SectionOverline] whose body collapses. Reference and the per-section
 * guidance groups fold away to a header row; [count] shows how many items a
 * collapsed group holds (board intent "Coach - de-densified"), and the
 * rotating chevron signals the toggle.
 */
@Composable
private fun ExpandableSection(
    title: String,
    count: Int? = null,
    initiallyExpanded: Boolean = false,
    content: @Composable () -> Unit,
) {
    // Saveable so an opened section stays open when scrolled out of the
    // LazyColumn (item disposal) and across rotation.
    var expanded by rememberSaveable { mutableStateOf(initiallyExpanded) }
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(Space.Md.dp))
            .clickable { expanded = !expanded }
            .padding(top = Space.Sm.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
        ) {
            Text(title.uppercase(Locale.US), color = Accent, style = Type.Section)
            if (count != null) {
                Text(
                    "$count",
                    color = OnBgFaint,
                    style = Type.Chip.merge(TabularFigures),
                    modifier = Modifier
                        .clip(RoundedCornerShape(6.dp))
                        .background(BgElevated)
                        .padding(horizontal = Space.Md.dp, vertical = Space.Xs.dp),
                )
            }
        }
        RowChevron(expanded)
    }
    if (expanded) content()
}

/**
 * One compact Reference row (02-coach §6): the claim title + its citation
 * line. Deliberately NOT an EvidenceCard, the full graded card already
 * rendered wherever the claim was recommended; Reference only lists sources.
 */
@Composable
private fun ReferenceRow(row: GuidanceView) {
    Column(verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
        Text(
            deckSeparators(row.summary),
            color = OnBgBody,
            style = Type.Body.copy(fontWeight = FontWeight.Bold),
        )
        Text(citationLabel(row.citation), color = OnBgFaint, style = Type.Caption)
    }
}

/**
 * Deck-conform separators for core-factual strings, display-only: the wire's
 * "Training age: Intermediate" renders with the copy deck's middot
 * ("Training age · Intermediate"). Never touches the wire value.
 */
private fun deckSeparators(summary: String): String =
    summary.replaceFirst("Training age: ", "Training age · ")

/** The irreversible-clear confirmation [AlertDialog] (chrome §6). Internal:
 *  the run-tracking banner's fallback undo path reuses it. */
@Composable
internal fun ClearConfirmDialog(
    visible: Boolean,
    title: String = "Clear all?",
    message: String = "This permanently removes every logged entry in this list and can't be undone.",
    confirmLabel: String = "Clear",
    onDismiss: () -> Unit,
    onClear: () -> Unit,
) {
    val status = LocalStatusColors.current
    if (visible) {
        AlertDialog(
            onDismissRequest = onDismiss,
            shape = RoundedCornerShape(Space.Card.dp),
            title = { Text(title) },
            text = { Text(message) },
            confirmButton = {
                TextButton(onClick = {
                    onDismiss()
                    onClear()
                }) { Text(confirmLabel, color = status.danger) }
            },
            dismissButton = {
                TextButton(onClick = onDismiss) { Text("Cancel") }
            },
        )
    }
}

/**
 * DO-NOT-TRAIN banner (01-today §C, INVARIANT 3). Pinned above content on every
 * screen; never scrollable, never dismissable. Renders ONLY when the core
 * actually blocks training (`train_blocked`), a downgrade-class adjustment
 * (HRV/wellness dip) is NOT a hold and renders as an amber inline card on Today,
 * never here. Safety is a rule, not a probability: the "?" disclosure
 * shows grade + citation but NO confidence meter. `holdDetail` is the
 * body-part/character sub-line for a characterized Pain report. A Pain hold
 * shows the "Remove the pain report" inline undo (the only clear path, now
 * confirm-gated). "Add details" opens the readiness editor.
 */
@Composable
internal fun SafetyBanner(
    model: ViewModel,
    modifier: Modifier = Modifier,
    holdDetail: String? = null,
    onClearReadiness: (() -> Unit)? = null,
    onRemovePain: (() -> Unit)? = null,
    onAddDetails: (() -> Unit)? = null,
) {
    // Proportional safety: the pinned red banner is reserved for a genuine
    // training block. A non-blocking safety_tier (e.g. HrvTrend) is handled by
    // the amber Today card, not here.
    if (!model.train_blocked) return
    val tier = model.safety_tier
    val status = LocalStatusColors.current
    val bg = status.danger
    // A slow, gentle pulse ring so the hold reads as the most urgent state.
    val pulse = run {
        val transition = rememberInfiniteTransition(label = "safety")
        transition.animateFloat(
            initialValue = 0f,
            targetValue = 1f,
            animationSpec = infiniteRepeatable(tween(1200), RepeatMode.Reverse),
            label = "pulse",
        ).value
    }
    val pulseModifier = Modifier.border(
        2.dp,
        Color.White.copy(alpha = 0.15f + pulse * 0.4f),
        RoundedCornerShape(Space.Card.dp),
    )
    var whyOpen by rememberSaveable { mutableStateOf(false) }
    Card(
        colors = CardDefaults.cardColors(containerColor = bg),
        elevation = CardDefaults.cardElevation(defaultElevation = 0.dp),
        shape = RoundedCornerShape(Space.Card.dp),
        modifier = modifier.fillMaxWidth().then(pulseModifier),
    ) {
        Column(Modifier.padding(Space.Card.dp), verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Space.Md.dp + Space.Xs.dp),
            ) {
                Icon(
                    painterResource(R.drawable.ic_safety_warning_triangle),
                    contentDescription = null,
                    tint = Color.White,
                    modifier = Modifier.size(26.dp),
                )
                Text(
                    "DO NOT TRAIN",
                    color = Color.White,
                    style = Type.Title.copy(fontSize = 23.sp, fontWeight = FontWeight.Black),
                )
            }
            // Sub-line: the characterized pain detail when we have it ("right
            // knee · sharp · 6/10"), else the tier that triggered the hold.
            val subLine = holdDetail?.let { "Pain: $it" }
                ?: tier?.let { "Triggered by ${safetyTierLabel(it)}." }
            if (subLine != null) {
                Text(subLine, color = DangerOn, style = Type.Body)
                Text(
                    "Programming is paused until cleared.",
                    color = Color.White.copy(alpha = 0.8f),
                    style = Type.Caption,
                )
            }
            // Action row: Add details (solid white, danger text) + the unified
            // "?" disclosure (owner ruling 2026-07-31: one "?" replaces every
            // "why?" text link: white-tinted here per the danger-surface colour
            // rule). Only the disclosure TRIGGER changed; all SAFETY copy on the
            // face stays fully visible. Hidden where no flow exists (tracking
            // screen).
            Row(
                horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (onAddDetails != null) {
                    Text(
                        "Add details",
                        color = bg,
                        style = Type.Body.copy(fontWeight = FontWeight.Bold),
                        modifier = Modifier
                            .clip(RoundedCornerShape(Space.Md.dp))
                            .background(Color.White)
                            .clickable { onAddDetails() }
                            .padding(horizontal = Space.Card.dp, vertical = Space.Md.dp),
                    )
                }
                DisclosureButton(whyOpen, tint = Color.White, onTint = bg) { whyOpen = !whyOpen }
            }
            if (whyOpen) {
                // The evidence behind the hold, verbatim from the core (never
                // invented here). Prefer the headline's "safety_hold" rung -
                // build_headline resolves the DOMINANT hold source including
                // gates-only medical referrals, which emit NO adjustment row
                // (BUGS.md 2026-08-03); fall back to the safety-critical
                // adjustment. Safety is a rule, not a probability; grade
                // + citation only, NO confidence meter.
                val hold = model.today_headline?.takeIf {
                    it.kind == "safety_hold" && it.summary.isNotBlank()
                }
                val adj = remember(model.adjustments) { dominantSafetyAdjustment(model) }
                val holdSummary = hold?.summary ?: adj?.summary
                val holdGrade = hold?.grade ?: adj?.grade
                val holdCitation = hold?.citation ?: adj?.citation
                if (holdSummary != null) {
                    Column(verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
                        Text(holdSummary, color = Color.White, style = Type.Caption)
                        gradeLabel(holdGrade ?: "")?.let { gradeText ->
                            Text(
                                "Evidence: $gradeText",
                                color = Color.White.copy(alpha = 0.85f),
                                style = Type.Caption,
                            )
                        }
                        if (!holdCitation.isNullOrBlank()) {
                            Text(
                                citationLabel(holdCitation),
                                color = Color.White.copy(alpha = 0.7f),
                                style = Type.Caption,
                            )
                        }
                    }
                } else {
                    Text(
                        // A MedicalReferral hold is derived from a profile health
                        // flag (or an NFOR/OTS review), not a "reported" readiness
                        // signal, so don't call it one. Other tiers are genuine
                        // reported signals.
                        if (tier == "MedicalReferral") {
                            "A health & safety flag pauses programming until it's resolved."
                        } else {
                            "A reported red-flag signal pauses programming until it clears."
                        },
                        color = Color.White.copy(alpha = 0.85f),
                        style = Type.Caption,
                    )
                }
            }
            // Undo path for a mis-logged signal, surfaced where the hold shows.
            // A Pain hold gets the surgical per-signal undo (the ONLY clear
            // path, INVARIANT 3), now confirm-gated; other readiness
            // holds keep the guarded clear-all confirm.
            val painHold = tier == "Pain" && onRemovePain != null
            // A MedicalReferral hold comes from a profile health flag
            // (youth/PARQ/pregnancy/injury) or an NFOR/OTS review: NOT a
            // readiness input, so "Clear readiness inputs" would not lift it and
            // is misleading. Point to the real resolution instead (no action).
            val medicalHold = tier == "MedicalReferral"
            when {
                painHold -> Text(
                    "Remove the pain report",
                    color = Color.White.copy(alpha = 0.9f),
                    style = Type.Caption,
                    modifier = Modifier
                        .clip(RoundedCornerShape(Space.Md.dp))
                        .clickable { onRemovePain!!() }
                        .padding(vertical = Space.Md.dp),
                )
                medicalHold -> Text(
                    "Resolve in Profile › Health & safety, or consult a professional.",
                    color = DangerOn,
                    style = Type.Caption,
                    modifier = Modifier.padding(top = Space.Sm.dp),
                )
                onClearReadiness != null -> Text(
                    "Logged by mistake? Clear readiness inputs…",
                    color = Color.White.copy(alpha = 0.9f),
                    style = Type.Caption,
                    modifier = Modifier
                        .clip(RoundedCornerShape(Space.Md.dp))
                        .clickable { onClearReadiness() }
                        .padding(vertical = Space.Md.dp),
                )
            }
        }
    }
}

/**
 * Human-readable label for the core's safety tier. The wire value is the raw
 * `SafetyTier` Debug name; an unknown tier falls through to its raw name.
 */
private fun safetyTierLabel(tier: String): String = when (tier) {
    "MedicalReferral" -> "a medical-referral red flag"
    "Pain" -> "pain reported today, a red flag"
    "Illness" -> "illness"
    "ObjectivePerformance" -> "an objective performance drop"
    "SubjectiveMultiDay" -> "subjective signals (multi-day)"
    "HrvTrend" -> "an HRV trend"
    "SingleDayMarker" -> "a single-day marker"
    else -> tier
}

/**
 * Sub-line for a pain hold's banner: prefer the core's own `detail` on the Pain
 * readiness row (populated when the core surfaces the characterized report),
 * else fall back to the shell echo of what the user just entered in triage.
 * Returns null when there's no characterized pain (bare/legacy report): the
 * banner then shows the generic tier line.
 */
internal fun painSubline(model: ViewModel, echo: PainDetail?): String? {
    val coreDetail = model.readiness_summary.firstOrNull { it.signal == "Pain" }?.detail
    if (!coreDetail.isNullOrBlank()) return coreDetail
    if (echo == null) return null
    val parts = buildList {
        echo.location?.let { add(it) }
        add(painKindLabel(echo.kind))
        add("${echo.severity}/10")
        if (echo.trend == PainTrend.Rising) add("worsening")
    }
    return parts.joinToString(" · ")
}

/** Plain-language label for a [PainKind]. Display-only. */
private fun painKindLabel(kind: PainKind): String = when (kind) {
    PainKind.SharpJoint -> "sharp / joint"
    PainKind.TendonLoadRelated -> "tendon-like"
    PainKind.Doms -> "muscle soreness"
    PainKind.Other -> "unspecified"
}

/**
 * Lift entry card (03-history §4): 34dp dumbbell tile + exercise title + date
 * badge; the set line; footer with the e1RM block left and a small factual
 * sparkline of that exercise's core-derived e1RMs right.
 */
@Composable
private fun LiftCard(l: LiftResultView, series: List<Double> = emptyList(), onClick: (() -> Unit)? = null) {
    PlainCard(onClick = onClick) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Space.Md.dp + Space.Xs.dp),
        ) {
            IconTile(
                painterResource(R.drawable.ic_content_set_dumbbell),
                Accent, Accent.copy(alpha = 0.14f), size = 34.dp,
            )
            Text(
                l.exercise,
                style = Type.Title.copy(fontSize = 17.sp, fontWeight = FontWeight.ExtraBold),
                color = OnBgBody,
                modifier = Modifier.weight(1f),
            )
            DateBadge(formatLogDate(l.observed_at))
        }
        Row {
            Text(
                "${trimDecimal(l.weight_kg)} kg × ${l.reps}",
                color = OnBgBody,
                style = Type.Body.merge(TabularFigures),
            )
            Text(
                " @ RPE ${trimDecimal(l.rpe)}",
                color = OnBgFaint,
                style = Type.Body.merge(TabularFigures),
            )
        }
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.Bottom,
        ) {
            Column(verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
                Row(verticalAlignment = Alignment.Bottom) {
                    Text(
                        trimDecimal(l.e1rm_kg),
                        color = OnBgBody,
                        style = Type.Title.merge(TabularFigures),
                    )
                    Text(" kg e1RM", color = OnBgFaint, style = Type.Caption)
                }
                Text(
                    "${Math.round(l.pct_1rm)}% 1RM · RIR ${trimDecimal(l.rir)}",
                    color = OnBgFaint,
                    style = Type.Caption.merge(TabularFigures),
                )
            }
            Sparkline(
                series.map { it.toFloat() },
                color = Accent,
                modifier = Modifier
                    .width(80.dp)
                    .height(28.dp),
            )
        }
    }
}

/**
 * Friendly log date for a history card from a unix-seconds stamp: "2 hours ago",
 * "Yesterday", "Jul 15". Empty for an undated entry.
 */
private fun formatLogDate(epochSec: Long): String =
    if (epochSec <= 0L) {
        ""
    } else {
        DateUtils.getRelativeTimeSpanString(
            epochSec * 1000L,
            System.currentTimeMillis(),
            DateUtils.DAY_IN_MILLIS,
            DateUtils.FORMAT_ABBREV_RELATIVE,
        ).toString()
    }

// Neutral informational chip ground: a plain slate that carries NO semantic
// meaning. Used where a chip states a fact without valence (even/negative
// split). Deliberately not a `StatusColors` token.
private val ChipNeutral = Color(0xFF334155)

/**
 * Chip ground for the core's split verdict. Only a fade gets the semantic warn;
 * even/negative splits are neutral facts. An unknown future verdict falls back
 * to neutral rather than inventing a valence.
 */
private fun splitVerdictColor(verdict: String, status: StatusColors): Color = when (verdict) {
    "fade" -> status.warn
    else -> ChipNeutral // "even", "negative", unknown
}

/**
 * Run entry card (03-history §4): 34dp run tile (hrZone1 family) + title +
 * date badge; distance · pace line; zone row with a three-segment zone
 * legend highlighting the run's core-judged zone; the split-verdict chip
 * renders ONLY when the core sends one.
 */
/** Distance in the display unit, e.g. "8.05 km" / "5.00 mi". */
private fun runDistLabel(distanceKm: Double, unit: DistanceUnit): String =
    "${String.format(Locale.US, "%.2f", metersToDisplay(distanceKm * 1000.0, unit))} ${unit.distanceLabel}"

/** Pace in the display unit, e.g. "5:34 /km" / "8:57 /mi". Recomputed from the
 *  run's distance+duration (not the core's always-/km `pace` string) so every run
 *  readout honours the chosen unit. "-" for a degenerate run. */
private fun runPaceLabel(distanceKm: Double, durationMin: Double, unit: DistanceUnit): String {
    if (distanceKm <= 0.0 || durationMin <= 0.0) return "-"
    return formatPaceMinutes(paceInUnit(durationMin / distanceKm, unit)) + " " + unit.paceSuffix
}

/** The resolved distance unit (locale + user override), collected reactively. */
@Composable
private fun rememberDistanceUnit(): DistanceUnit =
    resolveDistanceUnit(ThemeSettings.distanceUnitOverride.collectAsState().value)

@Composable
@OptIn(ExperimentalLayoutApi::class)
private fun RunCard(r: RunResultView, onClick: (() -> Unit)? = null) {
    val status = LocalStatusColors.current
    val unit = rememberDistanceUnit()
    PlainCard(onClick = onClick) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Space.Md.dp + Space.Xs.dp),
        ) {
            IconTile(
                painterResource(R.drawable.ic_content_run),
                Accent, Accent.copy(alpha = 0.18f), size = 34.dp,
            )
            Text(
                "Run",
                style = Type.Title.copy(fontSize = 17.sp, fontWeight = FontWeight.ExtraBold),
                color = OnBgBody,
                modifier = Modifier.weight(1f),
            )
            DateBadge(formatLogDate(r.observed_at))
        }
        // A GPS run with no usable fixes has no measurable zone/pace/distance -
        // surface the core's plain-language reason instead of an empty structure.
        if (r.distance_km <= 0.0) {
            if (r.summary.isNotBlank()) Text(r.summary, color = OnBgBody, style = Type.Body)
            if (r.citation.isNotBlank()) Text(citationLabel(r.citation), color = OnBgFaint, style = Type.Caption)
            return@PlainCard
        }
        Text(
            "${runDistLabel(r.distance_km, unit)} · ${runPaceLabel(r.distance_km, r.duration_min, unit)}",
            color = OnBgBody,
            style = Type.Body.merge(TabularFigures),
        )
        // Zone row: the core-judged zone ("-" when no HR was measured) + a
        // three-zone legend bar with the active zone at full strength.
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
        ) {
            Text(
                // Surface the run's HR (% of HRmax) when the core has it; a
                // hand-entered run with no HR stays "HR -". Never invented.
                "HR ${if (r.hr_pct_max > 0.0) "${trimDecimal(r.hr_pct_max)}%" else "-"} · ${r.zone}",
                color = if (r.zone.startsWith("Z")) OnBgMuted else OnBgFaint,
                style = Type.Caption.merge(TabularFigures),
            )
            Row(
                modifier = Modifier.weight(1f),
                horizontalArrangement = Arrangement.spacedBy(Space.Xs.dp),
            ) {
                listOf("Z1" to status.hrZone1, "Z2" to status.hrZone2, "Z3" to status.hrZone3)
                    .forEach { (z, c) ->
                        Box(
                            Modifier
                                .weight(1f)
                                .height(6.dp)
                                .clip(RoundedCornerShape(2.dp))
                                .background(if (z == r.zone) c else c.copy(alpha = 0.25f)),
                        )
                    }
            }
        }
        val split = r.split
        val interval = r.interval
        // Only chip the interval-LIKE case; a steady run is the default
        // expectation, so labelling it adds noise. VI still rode in on the wire.
        val intervalNotable = interval != null && interval.kind == "interval"
        // The user's own run-type label (USER DATA: no evidence, no coaching
        // reads it). Show it as a NEUTRAL chip, but NEVER when the measured INTERVAL
        // VI chip is already up; a second label there would duplicate or contradict
        // the derived verdict. Unknown/future wire strings map to null (untagged).
        val userType = WorkoutType.fromWire(r.workout_type)
        val showUserType = userType != null && !intervalNotable
        // A first-ever run has no prior distance to gauge a spike against;
        // that's baseline-building, not a danger. Frame it neutrally; a REAL
        // >10% jump keeps the red SPIKE alarm. spike_has_baseline is the core's
        // structured provenance; no spike_note scrape.
        val isBaseline = r.spike_flag && !r.spike_has_baseline
        if (r.spike_flag || split != null || intervalNotable || showUserType) {
            FlowRow(horizontalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                if (r.spike_flag) {
                    if (isBaseline) {
                        Chip("BASELINE", status.hrZone2)
                    } else {
                        Chip("SPIKE", status.danger)
                        GlossaryInfo("spike")
                    }
                }
                if (split != null) {
                    // FACE: the finding label + SAFETY only. The grade badge AND
                    // the CONTESTED marker moved behind the "?" (owner ruling
                    // 2026-07-31); SAFETY stays visible (HARD RULE).
                    Chip(split.label, splitVerdictColor(split.verdict, status))
                    if (split.safety_critical) Chip("SAFETY", status.danger)
                }
                if (intervalNotable) {
                    // hrZone2 (a non-danger zone colour, white-text-safe like the
                    // BASELINE chip), NOT the danger-red hrZone3; INTERVAL is a
                    // descriptive measurement, not a safety alarm; red misreads as one.
                    Chip(interval!!.label, status.hrZone2)
                }
                if (showUserType) {
                    // Neutral slate (evidenceUnknown): this is the user's own tag,
                    // it carries NO evidence grade and drives NO coaching, so it must
                    // NOT borrow a zone/verdict/danger colour that reads as measured.
                    Chip(userType!!.label.uppercase(Locale.US), status.evidenceUnknown)
                }
            }
        }
        if (r.spike_note.isNotBlank()) {
            // Render the core's honest spike_note verbatim: including the
            // first-run "no prior run" line, rather than a gamified "unlock"
            // paraphrase (the BASELINE chip already carries the neutral framing).
            Text(r.spike_note, color = OnBgMuted, style = Type.Caption)
        }
        // The core's evidence-cited pacing copy (fade cue or discipline praise).
        // The copy stays visible; the raw citation(s) move behind why? (m3,
        // the spec's own two-tier rule).
        if (split != null && split.message.isNotBlank()) {
            Text(split.message, color = OnBgMuted, style = Type.Caption)
        }
        // The interval-vs-steady explanation (its grade lives in the "?" panel).
        if (intervalNotable && interval!!.message.isNotBlank()) {
            Text(interval.message, color = OnBgMuted, style = Type.Caption)
        }
        val citations = listOfNotNull(
            split?.citation?.takeIf { it.isNotBlank() },
            interval?.citation?.takeIf { intervalNotable && it.isNotBlank() },
            r.citation.takeIf { it.isNotBlank() },
        )
        // Grade badges + CONTESTED marker + citations ALL live behind the single
        // "?" (owner ruling 2026-07-31): never on the collapsed face.
        val gradeChips = listOfNotNull(
            split?.grade?.takeIf { it.isNotBlank() },
            interval?.grade?.takeIf { intervalNotable && it.isNotBlank() },
        )
        val anyContested = (split?.contested == true) || (intervalNotable && interval?.contested == true)
        if (citations.isNotEmpty() || gradeChips.isNotEmpty() || anyContested) {
            var whyOpen by rememberSaveable(r.observed_at) { mutableStateOf(false) }
            DisclosureButton(whyOpen) { whyOpen = !whyOpen }
            if (whyOpen) {
                Column(verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
                    if (gradeChips.isNotEmpty() || anyContested) {
                        FlowRow(horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
                            gradeChips.forEach { GradeChip(it) }
                            if (anyContested) Chip("CONTESTED", status.warn)
                        }
                    }
                    citations.forEach {
                        Text(citationLabel(it), color = OnBgFaint, style = Type.Caption)
                    }
                }
            }
        }
        // Export GPX moved OFF the card face into the tap-open detail sheet
        // (2026-08-03 declutter ruling), one button per run list, not per row.
    }
}

/** A tapped History/Today entry, routed to the detail sheet. */
private sealed interface HistoryEntry {
    data class Lift(val v: LiftResultView) : HistoryEntry
    data class Run(val v: RunResultView) : HistoryEntry
}

// C3 (revised): the detail/edit sheet a user is mid-way through survives rotation,
// but we persist ONLY a lightweight selection KEY: "<kind><entry_id>|<observed_at>"
// never the whole view object. A run's RunResultView carries the full GPX
// (~0.5 MB for a long run); bundling that into the saved-state parcel risked a
// TransactionTooLargeException. The key re-resolves against the current model
// (resolveHistoryEntry); a since-deleted entry resolves to null → the sheet closes.
private fun historyEntryKey(e: HistoryEntry): String = when (e) {
    is HistoryEntry.Lift -> "L${e.v.entry_id}|${e.v.observed_at}"
    is HistoryEntry.Run -> "R${e.v.entry_id}|${e.v.observed_at}"
}

private fun resolveHistoryEntry(key: String?, model: ViewModel): HistoryEntry? {
    if (key.isNullOrEmpty()) return null
    val kind = key[0]
    val parts = key.substring(1).split("|")
    val id = parts.getOrNull(0)?.toLongOrNull() ?: return null
    val obs = parts.getOrNull(1)?.toLongOrNull() ?: return null
    // Match on entry_id AND observed_at so legacy rows (entry_id 0) can't collide.
    return when (kind) {
        'L' -> model.lifts.firstOrNull { it.entry_id == id && it.observed_at == obs }
            ?.let { HistoryEntry.Lift(it) }
        'R' -> model.runs.firstOrNull { it.entry_id == id && it.observed_at == obs }
            ?.let { HistoryEntry.Run(it) }
        else -> null
    }
}

/** The [Event.DeleteEntry] for a logged entry: by entry_id, with observed_at as
 *  the legacy fallback the core uses when the id is 0. */
private fun deleteEventFor(entry: HistoryEntry): Event = when (entry) {
    is HistoryEntry.Lift -> Event.DeleteEntry(
        Event.EntryKind.Set, entry.v.entry_id, observedAtFallback = entry.v.observed_at,
    )
    is HistoryEntry.Run -> Event.DeleteEntry(
        Event.EntryKind.Run, entry.v.entry_id, observedAtFallback = entry.v.observed_at,
    )
}

/**
 * The entry detail → edit/delete flow. Tapping a History or Today
 * card sets [selected]; this hosts the detail bottom sheet (Edit + Delete), the
 * pre-filled keypad editor (emitting AmendSet/AmendRun), and the delete confirm.
 * A GPS-tracked run (has a GPX track) is delete-only; its measured route is not
 * field-editable, so Edit is offered only for lifts and hand-entered runs.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun EntryActionSheets(
    selected: HistoryEntry?,
    recentExercises: List<String>,
    onDismiss: () -> Unit,
    onEvent: (Event) -> Unit,
) {
    if (selected == null) return
    // Saveable so a rotation mid-edit keeps the editor open (keyed on the
    // selected entry, so picking a different entry resets these).
    var editing by rememberSaveable(selected) { mutableStateOf(false) }
    var confirmDelete by rememberSaveable(selected) { mutableStateOf(false) }

    if (editing) {
        ModalBottomSheet(onDismissRequest = { editing = false }, containerColor = BgElevated) {
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .verticalScroll(rememberScrollState())
                    .padding(horizontal = 18.dp)
                    .padding(bottom = Space.Lg.dp),
                verticalArrangement = Arrangement.spacedBy(Space.Md.dp + Space.Xs.dp),
            ) {
                when (selected) {
                    is HistoryEntry.Lift -> LogSetEditor(
                        recentExercises = recentExercises,
                        initial = selected.v,
                        onClose = { editing = false },
                    ) { ev -> onEvent(ev); onDismiss() }
                    is HistoryEntry.Run -> LogRunEditor(
                        initial = selected.v,
                        onClose = { editing = false },
                    ) { ev -> onEvent(ev); onDismiss() }
                }
            }
        }
    } else {
        ModalBottomSheet(onDismissRequest = onDismiss, containerColor = BgElevated) {
            EntryDetailContent(
                entry = selected,
                onEdit = { editing = true },
                onDelete = { confirmDelete = true },
            )
        }
    }

    ClearConfirmDialog(
        visible = confirmDelete,
        title = if (selected is HistoryEntry.Run) "Delete this run?" else "Delete this set?",
        message = "This permanently removes the entry from your history and every metric derived from it. This can't be undone.",
        confirmLabel = "Delete",
        onDismiss = { confirmDelete = false },
        onClear = {
            onEvent(deleteEventFor(selected))
            onDismiss()
        },
    )
}

/** Detail-sheet body for one logged entry: a summary line + Edit / Delete. */
@Composable
private fun EntryDetailContent(entry: HistoryEntry, onEdit: () -> Unit, onDelete: () -> Unit) {
    val status = LocalStatusColors.current
    // A GPS run (has a GPX export) is delete-only.
    val editable = when (entry) {
        is HistoryEntry.Lift -> true
        is HistoryEntry.Run -> entry.v.gpx.isBlank() && entry.v.distance_km > 0.0
    }
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .navigationBarsPadding()
            .padding(horizontal = 18.dp)
            .padding(bottom = Space.Lg.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Card.dp),
    ) {
        when (entry) {
            is HistoryEntry.Lift -> {
                TileOverline(entry.v.exercise.ifBlank { "Set" })
                Text(
                    "${trimDecimal(entry.v.weight_kg)} kg × ${entry.v.reps} @ RPE ${trimDecimal(entry.v.rpe)}",
                    color = OnBgBody,
                    style = Type.Title.merge(TabularFigures),
                )
                Text(
                    "e1RM ${trimDecimal(entry.v.e1rm_kg)} kg${formatLogDate(entry.v.observed_at).let { if (it.isBlank()) "" else " · $it" }}",
                    color = OnBgFaint,
                    style = Type.Caption.merge(TabularFigures),
                )
            }
            is HistoryEntry.Run -> {
                val unit = rememberDistanceUnit()
                TileOverline(if (entry.v.gpx.isNotBlank()) "GPS run" else "Run")
                Text(
                    if (entry.v.distance_km > 0.0) {
                        "${runDistLabel(entry.v.distance_km, unit)} · " +
                            runPaceLabel(entry.v.distance_km, entry.v.duration_min, unit)
                    } else {
                        entry.v.summary.ifBlank { "Run" }
                    },
                    color = OnBgBody,
                    style = Type.Title.merge(TabularFigures),
                )
                val date = formatLogDate(entry.v.observed_at)
                if (date.isNotBlank()) Text(date, color = OnBgFaint, style = Type.Caption)
                // Route map (2026-08-03): a GPS run's track, parsed back from the
                // core-produced GPX and framed to its bounding box. Renders only
                // when a track exists: a hand-entered run has no gpx.
                if (entry.v.gpx.isNotBlank()) {
                    RunRouteMap(entry.v.gpx)
                }
                // Per-km / per-mi splits from the GPS track. Pick the list
                // matching the user's distance-unit override (`unit` resolved above);
                // pace is pre-formatted by the core (render verbatim). A hand-entered
                // run carries no track, so both lists are empty → no split section.
                val splits = if (unit == DistanceUnit.Mi) entry.v.splits_mi else entry.v.splits_km
                if (splits.isNotEmpty()) {
                    TileOverline("Splits")
                    Column(verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
                        splits.forEach { s ->
                            Text(
                                "Split ${s.index} · ${s.pace} ${unit.paceSuffix}" +
                                    if (s.partial) "  (partial)" else "",
                                color = if (s.partial) OnBgFaint else OnBgMuted,
                                style = Type.Caption.merge(TabularFigures),
                            )
                        }
                    }
                }
            }
        }
        // Export GPX lives here (2026-08-03: moved off every RunCard face -
        // it's a per-run action, offered once where the run is inspected).
        if (entry is HistoryEntry.Run && entry.v.gpx.isNotBlank()) {
            val ctx = LocalContext.current
            val shareScope = rememberCoroutineScope()
            OutlinedButton(
                onClick = { shareScope.launch { shareGpx(ctx, entry.v.gpx) } },
                modifier = Modifier.fillMaxWidth(),
            ) { Text("Export GPX") }
        }
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            if (editable) {
                Row(
                    modifier = Modifier
                        .weight(1f)
                        .height(52.dp)
                        .clip(RoundedCornerShape(100))
                        .background(Accent)
                        .clickable { onEdit() },
                    horizontalArrangement = Arrangement.Center,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text("Edit", color = OnAccent, style = Type.Body.copy(fontWeight = FontWeight.ExtraBold))
                }
            }
            Row(
                modifier = Modifier
                    .weight(1f)
                    .height(52.dp)
                    .clip(RoundedCornerShape(100))
                    .background(status.danger)
                    .clickable { onDelete() },
                horizontalArrangement = Arrangement.Center,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Delete", color = Color.White, style = Type.Body.copy(fontWeight = FontWeight.ExtraBold))
            }
        }
    }
}

/**
 * EvidenceCard (02-coach §EvidenceCard, INVARIANTS 2 & 5). Owner ruling
 * 2026-07-31 (supersedes the earlier "grade badge on the face" wording): the
 * collapsed FACE carries ONLY the summary, the SAFETY/CONTESTED chips (honesty
 * invariant, safety must stay visible), and the single unified "?" disclosure.
 * The grade badge, confidence meter, citation and grade-note ALL live behind the
 * "?" so no card leads with evidence chrome / a confidence meter unprompted. The
 * evidence MECHANISM is unchanged, every recommendation still carries its grade +
 * confidence + citation; this only changes where they render.
 *
 * `showConfidence` no longer drives any face meter (there is none); it survives
 * only as the default source for `confidenceInWhy`, which gates the confidence
 * figure INSIDE the "?" panel. A safety hold (both false) shows no confidence;
 * safety is a rule, not a probability. Adjustment/prescription pass
 * `showConfidence = false` + `confidenceInWhy = true`; feedback/calculator/
 * reference/profile-guidance cards default to confidence-in-panel.
 * `container`/`border` let a caller tint the card (the amber Today adjustment
 * card). `extraDetail` is an optional line rendered inside the "?" panel (the
 * HRmax card's Tanaka formula moves here off the collapsed face).
 */
/** The single unified evidence-disclosure affordance (owner ruling 2026-07-31): a
 *  small circular "?" that toggles a card's evidence panel (grade badge,
 *  confidence, citation, why). Replaces the old "why?"/"less" text link everywhere
 *  a disclosure exists, EvidenceCard, RunCard, prescription/adjustment/calculator/
 *  reference cards, so the collapsed face never leads with evidence chrome; only
 *  the summary + the SAFETY chip sit on the face (CONTESTED moved behind the "?"
 *  with the rest of the chrome, owner ruling 2026-07-31; SAFETY stays, HARD RULE).
 *  Filled when open.
 *  `tint`/`onTint` recolor the SAME affordance for non-default surfaces (the
 *  danger SafetyBanner uses white-on-danger, mid-tone Accent fails on red and
 *  the owner's colour ruling keeps white on danger surfaces); shape/behaviour
 *  never vary per call site. */
@Composable
private fun DisclosureButton(
    expanded: Boolean,
    tint: Color = Accent,
    onTint: Color = OnAccent,
    onClick: () -> Unit,
) {
    Box(
        modifier = Modifier
            .size(26.dp)
            .clip(RoundedCornerShape(100))
            .background(if (expanded) tint else tint.copy(alpha = 0.14f))
            .clickable { onClick() },
        contentAlignment = Alignment.Center,
    ) {
        Text(
            "?",
            color = if (expanded) onTint else tint,
            style = Type.Chip.copy(fontWeight = FontWeight.Bold),
        )
    }
}

@Composable
internal fun EvidenceCard(
    summary: String,
    grade: String,
    citation: String,
    confidence: Float,
    safetyCritical: Boolean,
    contested: Boolean,
    section: String? = null,
    showConfidence: Boolean = true,
    confidenceInWhy: Boolean = showConfidence,
    container: Color = BgElevated,
    border: Color? = null,
    extraDetail: String? = null,
    why: WhyView? = null,
    glossaryKey: String? = null,
) {
    val status = LocalStatusColors.current
    // Keyed on the card's identity (summary+citation), not composition position:
    // in an unkeyed/reordered list a positional saveable can re-attach an open
    // "?" panel to a DIFFERENT card after reorder (BUGS.md minor cluster). Keying
    // also resets the panel when a slot's content changes, correct, since the
    // disclosure belongs to the claim, not the slot.
    var expanded by rememberSaveable(summary, citation) { mutableStateOf(false) }
    // A safety-critical card carries the danger border unless the caller tinted
    // it (01-today §C.2); an explicit `border` (amber adjustment) wins.
    val effectiveBorder = border
        ?: if (safetyCritical) status.danger.copy(alpha = 0.35f) else null
    // The card itself is NOT clickable: the single "?" DisclosureButton toggles
    // expansion. This is the real fix for the recurring "weird shadow on an
    // unfolded tile" bug: a whole-card onClick draws a ripple that, on a tall
    // (expanded) card, is an inherently circular/partial highlight: it can never
    // fill a large rounded rectangle, so it reads as a soft-edged shadow that
    // doesn't cover the tile. Removing the card-sized click removes that ripple
    // entirely; the only ripple left is the small, self-contained "?" button.
    Card(
        colors = CardDefaults.cardColors(containerColor = container),
        elevation = CardDefaults.cardElevation(defaultElevation = 0.dp),
        shape = RoundedCornerShape(Space.Card.dp),
        modifier = Modifier
            .fillMaxWidth()
            .then(
                if (effectiveBorder != null) {
                    Modifier.border(1.dp, effectiveBorder, RoundedCornerShape(Space.Card.dp))
                } else {
                    Modifier
                },
            ),
    ) {
        Column(
            Modifier.padding(Space.Card.dp),
            verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
        ) {
            // A bare face (no SAFETY chip, no section overline) would collapse the
            // header to a lone right-aligned "?" floating above the summary, which
            // reads as an empty gap (the hero's nested prescription cards). Fold
            // the summary, glossary affordance, and "?" into ONE row instead. The
            // chip/section variant keeps the two-row face.
            if (!safetyCritical && section == null) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        summary,
                        modifier = Modifier.weight(1f),
                        color = OnBgBody,
                        style = Type.Body.copy(fontWeight = FontWeight.Bold),
                    )
                    glossaryKey?.let { GlossaryInfo(it) }
                    DisclosureButton(expanded) { expanded = !expanded }
                }
            } else {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp + Space.Xs.dp),
                ) {
                    // Collapsed FACE (owner ruling 2026-07-31, extended 2026-07-31 to
                    // hide CONTESTED too): ONLY the SAFETY chip stays on the face -
                    // safety visibility is a HARD RULE, not a declutter tradeoff. The
                    // CONTESTED marker (an evidence-quality tag, not a safety signal),
                    // the grade badge, confidence, citation and grade-note ALL live
                    // behind the "?" now, so a card leads with just its summary.
                    if (safetyCritical) Chip("SAFETY", status.danger)
                    section?.let { Text(it, color = Accent, style = Type.Chip) }
                    Spacer(Modifier.weight(1f))
                    DisclosureButton(expanded) { expanded = !expanded }
                }
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
                ) {
                    Text(
                        summary,
                        modifier = Modifier.weight(1f),
                        color = OnBgBody,
                        style = Type.Body.copy(fontWeight = FontWeight.Bold),
                    )
                    glossaryKey?.let { GlossaryInfo(it) }
                }
            }
            if (expanded) {
                WhyDetail(
                    why = why,
                    grade = grade,
                    citation = citation,
                    confidence = confidence,
                    contested = contested,
                    // Confidence appears inside the "?" for every card EXCEPT a
                    // safety hold (safety is a rule, not a probability). Callers
                    // that pass showConfidence=false without confidenceInWhy (the
                    // safety card) suppress it; adjustment/prescription/feedback/
                    // calculator/reference cards keep it in the panel.
                    showConfidence = confidenceInWhy,
                    extraDetail = extraDetail,
                )
            }
        }
    }
}

/**
 * The three-part "why?" disclosure body. Renders
 * the core-provided WhyView as: basis (what it's based on) → why THIS grade →
 * what data would improve it, then the citation. This replaces the old circular
 * restatement ("Evidence: Weak - 40% confidence / <citation>"). When the core
 * carries no why? block (old core), it falls back to that legacy restatement so
 * the sheet is never empty. The confidence figure only appears when the card
 * shows a meter at all (never on a safety hold, safety is a rule).
 */
@Composable
private fun WhyDetail(
    why: WhyView?,
    grade: String,
    citation: String,
    confidence: Float,
    contested: Boolean,
    showConfidence: Boolean,
    extraDetail: String?,
) {
    val hasWhy = why != null &&
        (why.basis.isNotBlank() || why.grade_note.isNotBlank() || why.improves.isNotBlank())
    Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
        // Grade badge + the CONTESTED marker now live behind the "?" (owner ruling
        // 2026-07-31): the grade chip, its legend "?", and the contested tag open
        // here inside the disclosure panel, not on the collapsed face. Availability
        // is unchanged, only placement. (SAFETY stays on the face, HARD RULE.)
        Row(
            horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            GradeChip(grade)
            if (contested) Chip("CONTESTED", LocalStatusColors.current.warn)
        }
        if (hasWhy) {
            // 1. What this is based on.
            if (why!!.basis.isNotBlank()) {
                WhyLine("Based on", why.basis)
            }
            // 2. Why THIS grade (+ confidence figure when the card shows one).
            val gradeLine = why.grade_note.ifBlank {
                gradeLabel(grade)?.let { "Evidence grade: $it." } ?: ""
            }
            if (gradeLine.isNotBlank()) {
                val gradeLineFull = if (showConfidence) {
                    "$gradeLine (${Math.round(confidence * 100)}% confidence)"
                } else {
                    gradeLine
                }
                WhyLine("Why this grade", gradeLineFull)
            }
            // 3. What would improve it, the engagement loop. Skipped when the
            //    core reports nothing would ("-").
            val improves = why.improves.trim()
            if (improves.isNotBlank() && improves != "-") {
                WhyLine("To improve", improves)
            }
        } else {
            // Legacy fallback: the pre-Phase-3 evidence restatement. Hidden for an
            // unmapped grade rather than leaking a raw Debug string.
            gradeLabel(grade)?.let { label ->
                Text(
                    if (showConfidence) {
                        "Evidence: $label, ${Math.round(confidence * 100)}% confidence"
                    } else {
                        "Evidence: $label"
                    },
                    color = OnBgMuted,
                    style = Type.Caption.merge(TabularFigures),
                )
            }
        }
        Text(citationLabel(citation), color = OnBgFaint, style = Type.Caption)
        extraDetail?.let { Text(it, color = OnBgMuted, style = Type.Caption) }
        // Only add the generic contested explainer when the grade note did not
        // already name the contested question (avoid a duplicate line).
        if (contested && (why == null || !why.grade_note.contains("contested", ignoreCase = true))) {
            Text(
                "Experts disagree on this. Here's both sides. Treated as provisional.",
                color = OnBgMuted,
                style = Type.Caption,
            )
        }
    }
}

/** One labelled line of the why? disclosure: a small accent label + body copy. */
@Composable
private fun WhyLine(label: String, body: String) {
    Column(verticalArrangement = Arrangement.spacedBy(1.dp)) {
        Text(label, color = Accent, style = Type.Chip)
        Text(body, color = OnBgMuted, style = Type.Caption)
    }
}

/** The evidence-grade legend, opened from the "?" on any grade badge. */
val LocalEvidenceLegend = staticCompositionLocalOf<() -> Unit> { {} }

/**
 * Grade badge + a small "?" that opens the "How evidence grading works" legend.
 * The badge color/label still key off the raw wire grade.
 */
@Composable
internal fun GradeChip(grade: String) {
    // Unmapped/future grades have no human label; hide the whole badge rather
    // than leak a raw Debug string.
    val label = gradeChipLabel(grade) ?: return
    val status = LocalStatusColors.current
    val openLegend = LocalEvidenceLegend.current
    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.Xs.dp),
    ) {
        Chip(label, status.gradeColor(grade))
        Text(
            "?",
            color = OnBgFaint,
            style = Type.Chip.copy(fontWeight = FontWeight.Bold),
            modifier = Modifier
                .clip(RoundedCornerShape(100))
                .background(OnBgBody.copy(alpha = 0.08f))
                .clickable { openLegend() }
                .padding(horizontal = Space.Sm.dp, vertical = 1.dp),
        )
    }
}

/**
 * Static "How evidence grading works" reference sheet. Explains the five
 * grades and the SAFETY / CONTESTED markers. No coaching logic, no wire data.
 */
@Composable
private fun EvidenceLegendSheet(definitions: List<GradeDefView> = emptyList()) {
    val status = LocalStatusColors.current
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 20.dp)
            .padding(bottom = Space.Lg.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        Text("How evidence grading works", color = OnBgBody, style = Type.Title)
        Text(
            "Every coaching recommendation carries the strength of the evidence behind it. Higher grades mean more, better-controlled research agrees.",
            color = OnBgMuted,
            style = Type.Body,
        )
        // Definitions come from the core (File 09) so the legend and the cards
        // agree by construction. Fall back to shell copy only against an old
        // core that doesn't export them.
        if (definitions.isNotEmpty()) {
            definitions.forEach { d ->
                val label = d.label.ifBlank { gradeLabel(d.grade) }
                if (label != null) {
                    LegendRow(
                        d.grade,
                        label,
                        status,
                        d.definition + "  (${Math.round(d.confidence * 100)}% confidence)",
                    )
                }
            }
        } else {
            LegendRow("Strong", "Strong", status, "Consistent findings from randomized trials or meta-analyses.")
            LegendRow("Moderate", "Moderate", status, "Mixed or limited randomized trials. The effect is likely but less certain.")
            LegendRow("Weak", "Weak", status, "Mostly observational or small studies; treat as a working hypothesis.")
            LegendRow("ExpertOpinion", "Expert opinion", status, "No direct trials. Expert consensus or mechanism-based reasoning.")
            LegendRow("MarketingMyth", "Marketing myth", status, "A popular claim the evidence does not support. Never programmed.")
        }
        HorizontalDivider(color = OnBgBody.copy(alpha = 0.08f))
        Row(horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp), verticalAlignment = Alignment.CenterVertically) {
            Chip("SAFETY", status.danger)
            Text(
                "A safety call. It comes before your goals, so there's no confidence score to show.",
                color = OnBgMuted,
                style = Type.Caption,
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp), verticalAlignment = Alignment.CenterVertically) {
            Chip("CONTESTED", status.warn)
            Text(
                "Experts genuinely disagree. The app shows both sides and treats it as provisional.",
                color = OnBgMuted,
                style = Type.Caption,
            )
        }
    }
}

// ── Glossary (m2) ────────────────────────────────────────────────────────────
// One app-wide bottom sheet defining the app's jargon, reachable from tappable
// term chips wherever those terms appear. These are UI copy, plain-language
// definitions of terminology, NOT knowledge-base training claims, so they live
// as a static shell resource (HARD RULE 1 concerns training claims; a
// definition of "RPE" asserts none).

/** One glossary entry: a stable key, the display term, and its plain definition. */
private data class GlossaryEntry(val key: String, val term: String, val definition: String)

private val GLOSSARY: List<GlossaryEntry> = listOf(
    GlossaryEntry(
        "e1rm", "e1RM",
        "Estimated 1-rep max: the heaviest single rep you could likely do, calculated from a set you logged (from the weight and reps). You never have to test a true max.",
    ),
    GlossaryEntry(
        "1rm", "1RM / %1RM",
        "1RM is your one-rep maximum. %1RM expresses a working load as a share of it, e.g. 80% 1RM is 80% of your best single.",
    ),
    GlossaryEntry(
        "rpe", "RPE",
        "Rate of Perceived Exertion: how hard a set felt, 1–10. RPE 10 means no reps left in the tank; RPE 8 means about two left.",
    ),
    GlossaryEntry(
        "rir", "RIR",
        "Reps In Reserve: how many more reps you could have done before failure. It mirrors RPE: RIR 2 ≈ RPE 8.",
    ),
    GlossaryEntry(
        "zscore", "z-score / your normal band",
        "How far a reading sits from your own normal, measured in standard deviations. 0 is typical for you; −1 is below your usual band. The app computes it from your logged history. You never enter it.",
    ),
    GlossaryEntry(
        "vdot", "VDOT",
        "A running-fitness number derived from a recent race or time trial (Daniels). It sets your training paces and zones.",
    ),
    GlossaryEntry(
        "decoupling", "decoupling",
        "Aerobic decoupling: the drift between your pace and heart rate across a steady run. A low drift signals a sound aerobic base; a high one means the effort outran it.",
    ),
    GlossaryEntry(
        "tonnage", "tonnage",
        "Total weight lifted in the last 7 days: the sum of weight × reps across every set you logged in that window. A simple volume tally.",
    ),
    GlossaryEntry(
        "spike", "SPIKE",
        "A sudden jump in training load: here, a run much longer than your recent normal. Big jumps can raise injury risk, so the app flags them.",
    ),
    GlossaryEntry(
        "hrmax", "HRmax",
        "Your maximum heart rate: the highest your heart can beat. Estimated from your age (Tanaka formula) until you log a measured value from an all-out effort.",
    ),
    GlossaryEntry(
        "deload", "deload",
        "A planned lighter week, less volume and/or load, to shed accumulated fatigue and let your body catch up on adaptation.",
    ),
    GlossaryEntry(
        "mesocycle", "mesocycle",
        "A training block of several weeks (often 4–6) built around one focus, usually ending in a deload.",
    ),
    GlossaryEntry(
        "ctl", "CTL / ATL / TSB",
        "Training-load bookkeeping: CTL is your rolling ~6-week fitness, ATL your rolling ~1-week fatigue, and TSB (CTL − ATL) a rough 'freshness'. Bookkeeping only, not a performance predictor.",
    ),
    GlossaryEntry(
        "trimp", "TRIMP",
        "Training Impulse: a single number summarising one session's load from its duration and heart-rate intensity. It feeds the CTL/ATL/TSB bookkeeping.",
    ),
    GlossaryEntry(
        "hrzones", "HR zones (Z1 / Z2 / Z3)",
        "Effort bands set from your thresholds or HRmax. Z1 is very easy recovery; Z2 is an easy conversational pace you can talk through; Z3 is a harder tempo above easy.",
    ),
    GlossaryEntry(
        "volume", "MEV / MAV / MRV",
        "Weekly training-volume landmarks, counted as sets per muscle: MEV is the minimum effective volume, MAV the productive middle range, and MRV the maximum you can recover from. They shift with your training age and recovery.",
    ),
    GlossaryEntry(
        "maf", "MAF",
        "Maximum Aerobic Function (Maffetone): an easy aerobic heart-rate cap, often estimated as about 180 minus your age. One method for pacing base work; measured lactate thresholds are an alternative.",
    ),
    GlossaryEntry(
        "pap", "PAP / PAPE",
        "Post-Activation Potentiation (Enhancement): a heavy 'primer' set can briefly sharpen a following explosive effort, so the two are paired with a short rest between.",
    ),
    GlossaryEntry(
        "reds", "RED-S / LEA",
        "LEA (Low Energy Availability) is not eating enough to cover training plus daily needs; RED-S (Relative Energy Deficiency in Sport) is the health fallout from sustained LEA. A safety concern. The app defers to a professional.",
    ),
)

/** Opens the glossary sheet at a given term key. Provided by the app scaffold. */
val LocalGlossary = staticCompositionLocalOf<(String) -> Unit> { {} }

/**
 * A tappable term chip (m2): the term rendered as an accent link + info glyph
 * that opens the glossary at that term. For a standalone term (e.g. making a
 * label tappable). `label` overrides the visible text (defaults to the term).
 */
@Composable
internal fun GlossaryChip(key: String, label: String? = null) {
    val open = LocalGlossary.current
    val text = label ?: GLOSSARY.firstOrNull { it.key == key }?.term ?: key
    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(2.dp),
        modifier = Modifier
            .clip(RoundedCornerShape(100))
            .clickable { open(key) }
            .padding(horizontal = Space.Xs.dp, vertical = 1.dp),
    ) {
        Text(text, color = Accent, style = Type.Caption)
        Text("ⓘ", color = OnBgFaint, style = Type.Chip)
    }
}

/**
 * An icon-only glossary affordance (m2): a small tappable "ⓘ" for placing next
 * to an already-labelled value (the e1RM overline, the tonnage tile) without
 * repeating the term. Opens the glossary at `key`.
 */
@Composable
internal fun GlossaryInfo(key: String) {
    val open = LocalGlossary.current
    Text(
        "ⓘ",
        color = OnBgFaint,
        style = Type.Chip.copy(fontWeight = FontWeight.Bold),
        modifier = Modifier
            .clip(RoundedCornerShape(100))
            .background(OnBgBody.copy(alpha = 0.08f))
            .clickable { open(key) }
            .padding(horizontal = Space.Sm.dp, vertical = 1.dp),
    )
}

/** The glossary bottom sheet, scrolled/anchored to `initialKey` and highlighting it. */
@Composable
private fun GlossarySheet(initialKey: String) {
    val listState = rememberLazyListState()
    val index = GLOSSARY.indexOfFirst { it.key == initialKey }.coerceAtLeast(0)
    LaunchedEffect(initialKey) {
        if (index > 0) listState.scrollToItem(index)
    }
    LazyColumn(
        state = listState,
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 20.dp)
            .padding(bottom = Space.Lg.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        item {
            Text("Glossary", color = OnBgBody, style = Type.Title)
        }
        items(GLOSSARY, key = { it.key }) { e ->
            val highlighted = e.key == initialKey
            Column(
                verticalArrangement = Arrangement.spacedBy(Space.Xs.dp),
                modifier = Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(Space.Card.dp))
                    .then(
                        if (highlighted) {
                            Modifier.border(1.dp, Accent.copy(alpha = 0.5f), RoundedCornerShape(Space.Card.dp))
                        } else {
                            Modifier
                        },
                    )
                    .background(if (highlighted) Accent.copy(alpha = 0.08f) else Color.Transparent)
                    .padding(Space.Sm.dp),
            ) {
                Text(e.term, color = OnBgBody, style = Type.Body.copy(fontWeight = FontWeight.Bold))
                Text(e.definition, color = OnBgMuted, style = Type.Caption)
            }
        }
    }
}

@Composable
private fun LegendRow(wireGrade: String, label: String, status: StatusColors, body: String) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        Box(modifier = Modifier.width(96.dp)) {
            Chip(label.uppercase(Locale.US), status.gradeColor(wireGrade))
        }
        Text(body, color = OnBgMuted, style = Type.Caption, modifier = Modifier.weight(1f))
    }
}

/** Grade-badge text: the human label, uppercased to match the other chips
 *  (SAFETY / CONTESTED / SPIKE). Display-only. */
internal fun gradeChipLabel(grade: String): String? = gradeLabel(grade)?.uppercase(Locale.US)

/**
 * Human label for an evidence grade's wire name, or null for an unmapped grade
 * (callers hide the chip/row rather than leak a raw Debug string). Display-only -
 * badges, sorting, and colors still key off the raw wire value.
 */
internal fun gradeLabel(grade: String): String? = when (grade) {
    "Strong" -> "Strong"
    "Moderate" -> "Moderate"
    "Weak" -> "Weak"
    "ExpertOpinion" -> "Expert opinion"
    "MarketingMyth" -> "Marketing myth"
    else -> null
}

/**
 * Human-readable citation line. Verbatim-render contract (design/user-decisions
 * "Evidence display"): the shell renders the CORE's citation string exactly as
 * emitted, it never paraphrases, renumbers, or collapses a distinction the core
 * drew (an earlier version rewrote "File 02 consensus" → "Knowledge base
 * synthesis (File 2)", which both dropped the consensus-vs-synthesis distinction
 * and renumbered the file, a lossy rewrite that is now removed). The registry
 * emits real published references (evidence.rs `primary_citations`); a blank
 * citation is the only thing we touch, rendering it as an em dash.
 */
internal fun citationLabel(citation: String): String =
    citation.ifBlank { "-" }

/** Compact day-type token for the week strip, from the core's debug session_type
 *  ("Lift(MaxEffort)" / "Run(LongRun)" / "Rest"). Discipline-level so it fits a
 *  7-column strip; the full session title shows in the expanded day card. */
private fun sessionDiscipline(sessionType: String): String = when {
    sessionType.startsWith("Lift") -> "Lift"
    sessionType.startsWith("Run") -> "Run"
    else -> "Rest"
}

/**
 * Grade rank for the Coach priority sort: higher = stronger evidence. Mirrors
 * `EvidenceGrade` ordering in `schema.rs`; unknown grades sort last.
 */
private fun gradeRank(grade: String): Int = when (grade) {
    "Strong" -> 4
    "Moderate" -> 3
    "Weak" -> 2
    "ExpertOpinion" -> 1
    "MarketingMyth" -> 0
    else -> -1
}

/**
 * Display label for a wire readiness-signal name. Falls back to the raw wire
 * name for a signal the shell hasn't caught up to.
 */
private fun readinessSignalLabel(wire: String): String =
    runCatching { ReadinessSignal.valueOf(wire).label }.getOrDefault(wire)

/**
 * Readiness-strip pill name: the copy deck's friendly name where one exists
 * (deck says "HRV", not the wire's "HRV (ln rMSSD)"); every other signal keeps
 * its full label. Display-only, the wire value never changes.
 */
private fun readinessPillLabel(wire: String): String = when (wire) {
    "HrvLnRmssd" -> "HRV"
    "HrvCv" -> "HRV variability"
    "WellnessZ" -> "Wellness"
    "EstimatedOneRm" -> "Strength"
    "Rpe" -> "Effort"
    "RestingHr" -> "Resting HR"
    "BarVelocity" -> "Bar speed"
    "VelocityLoss" -> "Velocity loss"
    "AerobicDecoupling" -> "HR drift"
    else -> readinessSignalLabel(wire)
}

@Composable
private fun PlainCard(onClick: (() -> Unit)? = null, content: @Composable () -> Unit) {
    val border = BorderStroke(1.dp, OnBgBody.copy(alpha = 0.06f))
    val shape = RoundedCornerShape(Space.Card.dp)
    val colors = CardDefaults.cardColors(containerColor = BgElevated)
    val elevation = CardDefaults.cardElevation(defaultElevation = 0.dp)
    val body: @Composable () -> Unit = {
        Column(
            Modifier.padding(Space.Card.dp),
            verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
        ) { content() }
    }
    // Clickable variant uses the M3 `Card(onClick=…)` overload so the tap ripple
    // is clipped by the card's own rounded surface at any height (no soft-edge
    // leak on tall/expanded cards); the static variant is a plain Card.
    if (onClick != null) {
        Card(
            onClick = onClick,
            colors = colors,
            elevation = elevation,
            shape = shape,
            border = border,
            modifier = Modifier.fillMaxWidth(),
        ) { body() }
    } else {
        Card(
            colors = colors,
            elevation = elevation,
            shape = shape,
            border = border,
            modifier = Modifier.fillMaxWidth(),
        ) { body() }
    }
}

@Composable
private fun Chip(text: String, bg: Color) {
    Text(
        text,
        color = Color.White,
        style = Type.Chip,
        modifier = Modifier
            .background(bg, RoundedCornerShape(Space.Md.dp - Space.Xs.dp))
            .padding(horizontal = Space.Md.dp, vertical = Space.Sm.dp - 1.dp),
    )
}
