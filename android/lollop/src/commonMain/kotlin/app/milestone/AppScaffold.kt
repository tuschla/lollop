package app.milestone

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.displayCutoutPadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.NavigationRailDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.dp
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import app.milestone.resources.Res
import app.milestone.resources.ic_action_log_plus
import app.milestone.resources.main_log
import kotlinx.coroutines.launch
import org.jetbrains.compose.resources.painterResource
import org.jetbrains.compose.resources.stringResource

/** The app's root: decides the window's [WindowLayout] once and hands it down. */
@Composable
internal fun CoachScaffold(session: CoachSession) {
    CompositionLocalProvider(
        LocalWindowLayout provides rememberWindowLayout(),
        LocalRestRunning provides RestAlarm.running,
    ) { CoachScaffoldBody(session) }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun CoachScaffoldBody(session: CoachSession) {
    val ctx = currentPlatformContext()
    val liveTracking by RunSession.tracking.collectAsState()

    // Hoisted above the full-screen setup/tracker returns so it survives them.
    var selected by rememberSaveable(stateSaver = DestSaver) { mutableStateOf(Dest.Today) }
    // A Profile Focus switch that needs the new side's answers opens the setup at
    // those questions (null = a plain re-run or first run).
    var setupSwitchTo by rememberSaveable { mutableStateOf<Modality?>(null) }
    // A run "View" action (import confirmation) asks History to open that run:
    // switch tabs while it is pending; History opens the sheet and consumes it.
    val historyFocus by HistoryFocus.pending.collectAsState()
    LaunchedEffect(historyFocus) {
        if (historyFocus != null) selected = Dest.History
    }

    // Log bottom-sheet state. `sheetOpen` drives whether the ModalBottomSheet is
    // composed; `sheetMode` is which content it shows (the chooser first, then one
    // of the editors when picked). RememberSaveable so a rotation mid-entry
    // keeps the sheet OPEN on the same editor (the editors' own fields are already
    // rememberSaveable) instead of dropping the in-progress entry. LogMode is a
    // (Java-Serializable) enum, so the autoSaver bundles it like the calc sheet.
    // Declared above the full-screen returns so the tracker can hand over to the
    // run editor ("Log run by hand").
    var sheetOpen by rememberSaveable { mutableStateOf(false) }
    var sheetMode by rememberSaveable { mutableStateOf(LogMode.Chooser) }
    // Which coach-prescribed exercise the Set editor should prefill from, or null
    // for a blank entry. A KEY (not the object): re-resolved against the live model
    // each recomposition, so it survives rotation without a custom Saver and can't
    // pin a stale prescription.
    var sheetPlanExercise by rememberSaveable { mutableStateOf<String?>(null) }
    // The plan DAY a past-session Log/Track targets: drives the editor's seed
    // stamp + the missed-day prescription lookup. null = today's flow.
    var sheetPlanDay by rememberSaveable { mutableStateOf<Long?>(null) }
    val logGuard = remember { EditorGuard() }
    fun resetSheet() {
        sheetPlanExercise = null
        sheetPlanDay = null
    }
    fun openSheet(mode: LogMode, exercise: String? = null, day: Long? = null) {
        sheetPlanExercise = exercise
        sheetPlanDay = day
        sheetMode = mode
        sheetOpen = true
    }
    // The tracker where the platform records runs; elsewhere (null) a run is logged
    // by hand (the run editor), so no entry leads to a tracker that isn't there.
    val canTrack = session.tracker != null
    val trackRun: (() -> Unit)? = if (canTrack) ({ session.showTracker = true }) else null

    // Crash-recovery consent, ahead of the full-screen returns below so neither
    // the setup nor a tracker restored after process death can hide it.
    ResumeRunDialog(session)

    // An open intent (widget, Quick Settings tile, Live Update tap) asks for a
    // destination: apply it once and clear it. While the guided setup shows, the
    // setup stays and the request waits for it to be completed or skipped;
    // [routeOpen] keeps unsaved input and a tracker that holds a service.
    val openDest by session.openRequest.collectAsState()
    LaunchedEffect(openDest, session.showSetup) {
        val dest = openDest ?: return@LaunchedEffect
        if (session.showSetup) return@LaunchedEffect
        session.openRequest.value = null
        val nav = OpenNav(session.showTracker, selected, sheetOpen, sheetMode, sheetPlanExercise, sheetPlanDay)
        val next = routeOpen(
            dest,
            nav,
            session.model,
            sheetBusy = logGuard.busy,
            trackerPinned = !RunSession.tracking.value && (RunSession.armed.value || RunSession.locating.value),
            canTrack = canTrack,
        )
        session.showTracker = next.showTracker
        selected = next.selected
        sheetMode = next.sheetMode
        sheetPlanExercise = next.sheetPlanExercise
        sheetPlanDay = next.sheetPlanDay
        sheetOpen = next.sheetOpen
    }

    if (session.showSetup) {
        // Guided setup replaces the whole scaffold on first run. Completing it
        // writes ONE SetProfile (same wire as the full editor); skipping leaves the
        // profile empty and drops into the normal empty Today. Either way the
        // onboarding flag is set so it never re-prompts.
        // GuidedSetup is a full-screen replacement (outside the Scaffold that
        // owns the system-bar insets), so under edge-to-edge its header would sit
        // under the status bar and, in landscape, its edge under the camera
        // cutout. The insets belong on the full-screen container, which lives
        // here at the call site, not inside GuidedSetup's own content.
        Box(Modifier.fillMaxSize().background(BgTop).statusBarsPadding().navigationBarsPadding().displayCutoutPadding()) {
            // A re-run (setupInitial != null) must NOT touch the onboarding pref -
            // it's a returning user reviewing their answers, not first-run onboarding.
            val firstRun = session.setupInitial == null
            GuidedSetup(
                initial = session.setupInitial,
                switchTo = setupSwitchTo,
                calisLadders = session.model.calisthenics_ladders,
                onComplete = { draft, rungs ->
                    val today = session.model.today()
                    EventDispatcher.send(draft.effectiveFrom(today))
                    // Calisthenics self-placement: each answered ladder starts at
                    // its rung (after the profile, before the plan is generated).
                    rungs.forEach(EventDispatcher::send)
                    // Zero-tap after setup: now that a profile exists, generate the
                    // plan immediately so the user lands on Today's prescription in
                    // THIS session (not only after a relaunch). The core preserves
                    // this anchor on later boots.
                    EventDispatcher.send(Event.GeneratePlan(today))
                    if (firstRun) session.markOnboardingDone()
                    session.showSetup = false
                    session.setupInitial = null
                    setupSwitchTo = null
                },
                onSkip = {
                    if (firstRun) session.markOnboardingDone()
                    session.showSetup = false
                    session.setupInitial = null
                    setupSwitchTo = null
                },
            )
        }
        return
    }

    if (session.showTracker && canTrack) {
        // The tracking screen replaces this whole scaffold, so it receives the
        // live model and re-pins the SafetyBanner itself (chrome §5: the banner is
        // on EVERY screen, never scrollable or dismissable). onEvent keeps the
        // banner's undo/Add-details paths live during a run.
        RunTrackingScreen(
            model = session.model,
            onEvent = EventDispatcher::send,
            onFinish = {
                // The run save routes through EventDispatcher.exclusive,
                // so the consumer already published the saved model in queue order and
                // the model collector re-seeds session.model — nothing to publish here.
                session.showTracker = false
            },
            onLogRunManually = {
                session.showTracker = false
                openSheet(LogMode.Run)
            },
        )
        return
    }

    // Clear all data, gated behind a confirm dialog (Profile's Danger zone opens
    // it). The banner's own confirms live in SafetyBannerHost.
    var confirmClearAll by rememberSaveable { mutableStateOf(false) }
    // The Profile row a banner action ("Check age", "Review health answers")
    // opens: Profile scrolls to and expands it, then clears this.
    var profileFocus by rememberSaveable { mutableStateOf<ProfileRowId?>(null) }
    // Profile's "Your plan answers" fold, kept here so leaving the tab and coming
    // back finds it as the user left it.
    var planAnswersOpen by rememberSaveable { mutableStateOf(false) }
    // Re-run guided setup (Profile's row, Today's "nothing to plan" card): seed the
    // wizard from the current profile (or SEED when none) and open it pre-filled; a
    // Focus switch opens it at the new side's questions. A re-run leaves the
    // onboarding pref untouched (handled at the call site).
    val rerunSetup: (Modality?) -> Unit = { switchTo ->
        session.setupInitial = session.model.profile?.let { Event.SetProfile.from(it) } ?: Event.SetProfile.SEED
        setupSwitchTo = switchTo
        session.showSetup = true
    }
    // "How evidence grading works" legend sheet, opened from the "?" on grade
    // badges. Static copy; no coaching logic.
    var showLegend by rememberSaveable { mutableStateOf(false) }
    // Glossary bottom-sheet: the term key to open it scrolled to, or null
    // when closed. Opened from tappable term chips app-wide. Static UI copy;
    // definitions of jargon, NOT KB training claims.
    var glossaryTerm by rememberSaveable { mutableStateOf<String?>(null) }

    // Today's scroll position, hoisted so the Log FAB can collapse to its icon
    // once the list leaves the top.
    val todayListState = rememberLazyListState()
    val fabExpanded by remember { derivedStateOf { todayListState.firstVisibleItemIndex == 0 } }
    val scope = rememberCoroutineScope()

    // The Today Log FAB. Reopening always resets to the chooser so the sheet
    // never reappears mid-editor.
    val openLog: () -> Unit = { openSheet(LogMode.Chooser) }

    // The bottom bar's measured height: the top chrome keeps the app bar off it.
    var bottomBarPx by remember { mutableIntStateOf(0) }
    // The nav bar's own measured height, kept while the rail replaces it, so the
    // rail decision below is judged against the bar it would bring back.
    var navBarPx by remember { mutableIntStateOf(0) }
    // The chrome gives way to the pinned safety banner, never the reverse: when the
    // bottom nav and the Log button no longer fit under the banner (a landscape
    // phone under a hold), a side rail carries the destinations and Log, beside the
    // banner instead of over it. A wide or a short window always takes the rail
    // (visual-design-system §5 "Adaptive layout").
    var bannerRail by remember { mutableStateOf(false) }
    val windowLayout = LocalWindowLayout.current
    val navRail = bannerRail || windowLayout.navRail
    val logRoomPx = with(LocalDensity.current) { LogButtonRoom.roundToPx() }
    Row(Modifier.fillMaxSize().background(BgTop)) {
        if (navRail) {
            MilestoneNavRail(
                selected,
                onSelect = { selected = it },
                onLog = if (selected == Dest.Today) openLog else null,
                showBrand = windowLayout.shortHeight,
            )
        }
        Scaffold(
            // Beside the rail the scaffold no longer touches the start edge: the
            // rail took the start inset (a landscape camera cutout, a side
            // navigation bar), so the content does not pad for it a second time.
            modifier = Modifier.weight(1f).then(
                if (navRail) Modifier.consumeWindowInsets(NavigationRailDefaults.windowInsets.only(WindowInsetsSides.Start)) else Modifier,
            ),
            containerColor = BgTop,
            // M3 lays the snackbar above the FAB on Today and above the nav bar elsewhere.
            snackbarHost = { UiMessageHost() },
            topBar = {
                // Chrome §5 stacking order: status bar → SAFETY BANNER → app bar.
                // The DO-NOT-TRAIN banner is the topmost element below the status
                // bar on EVERY destination: it renders ABOVE the brand app bar,
                // never scrolls, never dismisses (HARD RULE 3 / INVARIANT 3). The
                // under-18 plan pause is no training stop: it is Today's first card
                // (YouthHoldCard), below the app bar, and the host renders nothing.
                TopChrome(
                    bottomBarPx = bottomBarPx,
                    navBarPx = navBarPx,
                    logRoomPx = logRoomPx,
                    onNeedsRail = { if (bannerRail != it) bannerRail = it },
                    banner = {
                        SafetyBannerHost(
                            session.model,
                            onEvent = EventDispatcher::send,
                            modifier = Modifier
                                .padding(horizontal = Space.Screen.dp)
                                .padding(top = Space.Md.dp),
                            onAddDetails = { openSheet(LogMode.Readiness) },
                            onOpenProfile = { row -> profileFocus = row; selected = Dest.Profile },
                        )
                    },
                    // Brand lockup IS the title on destinations (chrome §2): no
                    // secondary title string anywhere in content. No overflow menu:
                    // "Clear all data" lives only in Profile's Danger zone, since a
                    // destructive action a tap away on every screen is too easy to hit.
                    // A short window gives the bar's height to the content; the
                    // route mark tops the rail instead.
                    appBar = { if (!windowLayout.shortHeight) BrandTopBar() },
                )
            },
            bottomBar = {
                Column(Modifier.onSizeChanged { bottomBarPx = it.height }) {
                    // Run-in-progress chip pinned just above the nav bar (chrome §4),
                    // below content, never above the safety banner (which is topmost).
                    if (liveTracking && !session.showTracker) {
                        RunInProgressChip { session.showTracker = true }
                    }
                    if (!navRail) {
                        Box(Modifier.onSizeChanged { navBarPx = it.height }) {
                            MilestoneNavBar(selected) { selected = it }
                        }
                    }
                }
            },
            floatingActionButton = {
                // The Log FAB belongs to Today only (chrome §1). It floats over the
                // list; Today's rows keep their full width and the list's bottom
                // padding lets its last row scroll clear of it. With the rail, Log is
                // the rail's header instead.
                if (selected == Dest.Today && !navRail) {
                    val logLabel = stringResource(Res.string.main_log)
                    ExtendedFloatingActionButton(
                        onClick = openLog,
                        expanded = fabExpanded,
                        // The extended FAB clears its content's semantics, so the
                        // visible "Log" text never reaches TalkBack; name the button.
                        modifier = Modifier.testTag("log-fab").semantics { contentDescription = logLabel },
                        containerColor = Accent,
                        // Dark OnAccent on every accent fill: white (#fff) text fails
                        // AA contrast against the accent here.
                        contentColor = OnAccent,
                        shape = RoundedCornerShape(Space.Card.dp),
                        text = { Text(logLabel, style = Type.Body.copy(fontWeight = FontWeight.ExtraBold)) },
                        icon = {
                            Icon(
                                painterResource(Res.drawable.ic_action_log_plus),
                                contentDescription = null,
                                modifier = Modifier.size(20.dp),
                            )
                        },
                    )
                }
            },
        ) { pad ->
            ClearAllDataDialog(
                visible = confirmClearAll,
                onDismiss = { confirmClearAll = false },
                onClear = {
                    // Every logged family + derived coaching output; the profile
                    // (training configuration) stays. ONE batch: a single core turn with
                    // one view decode and one published model, never a run of partial
                    // models. The logged ClearPlan keeps the plan removed on the next
                    // boot.
                    EventDispatcher.sendAll(
                        listOf(
                            Event.ClearSets, Event.ClearRuns, Event.ClearReadiness,
                            Event.ClearCheckins, Event.ClearReview, Event.ClearWeeklyCheckin, Event.ClearRacePrediction,
                            Event.ClearHypertrophyPlan, Event.ClearProtein, Event.ClearHrZones,
                            Event.ClearPlan, Event.ClearActivities,
                        ),
                    )
                },
            )
            CompositionLocalProvider(
                LocalEvidenceLegend provides { showLegend = true },
                LocalGlossary provides { term -> glossaryTerm = term },
            ) {
                // Destinations are pure projections of the one hoisted `model`.
                Box(Modifier.fillMaxSize().padding(pad)) {
                    when (selected) {
                        Dest.Today -> {
                            TodayDestination(
                                model = session.model,
                                listState = todayListState,
                                actions = TodayActions(
                                    onEvent = EventDispatcher::send,
                                    onOpenProfile = { row -> profileFocus = row; selected = Dest.Profile },
                                    onStartSetup = {
                                        // First-run entry point (no seed): keep the onboarding
                                        // pref semantics at the call site above.
                                        session.setupInitial = null
                                        session.showSetup = true
                                    },
                                    onRerunSetup = { rerunSetup(null) },
                                    onLogPlanItem = { openSheet(LogMode.Set, exercise = it.exercise) },
                                    onLogPastItem = { item, day -> openSheet(LogMode.Set, exercise = item.exercise, day = day) },
                                    onLogPastRun = { day -> openSheet(LogMode.Run, day = day) },
                                    onTrackRun = trackRun,
                                    onOpenSheet = { mode -> openSheet(mode) },
                                ),
                            )
                        }
                        Dest.History -> HistoryDestination(
                            model = session.model,
                            onEvent = EventDispatcher::send,
                            onTrackRun = trackRun,
                            onLogSet = { openSheet(LogMode.Set) },
                            onLogRun = { openSheet(LogMode.Run) },
                        )
                        Dest.Profile -> ProfileDestination(
                            ctx = ctx,
                            model = session.model,
                            onEvent = EventDispatcher::send,
                            onClearAll = { confirmClearAll = true },
                            onRerunSetup = rerunSetup,
                            focusRow = profileFocus,
                            onFocusHandled = { profileFocus = null },
                            planAnswersOpen = planAnswersOpen,
                            onPlanAnswersOpenChange = { planAnswersOpen = it },
                        )
                    }
                }

                // Re-resolve the tapped plan item against the LIVE model each recomposition
                // (by key, not object): a stale key (plan cleared) or a day under a training
                // hold (its row carries no items) resolves to null and the editor opens
                // blank, no error path. Mirrors History's resolveHistoryEntry.
                val planPrefill = sheetPlanExercise?.let { key ->
                    // The day's own row keeps its items after they complete (the hero moves
                    // on once the whole day is done), so the loop editor still sees the item
                    // it is logging and can show "Exercise complete". Prefer an open item
                    // with this name; fall back to the finished one.
                    val day = sheetPlanDay ?: session.model.today()
                    val items = session.model.week_plan.firstOrNull { it.epoch_day == day }?.items.orEmpty()
                    items.firstOrNull { it.exercise == key && !it.done } ?: items.firstOrNull { it.exercise == key }
                }
                val planObservedAt = sheetPlanDay?.takeIf { it != session.model.today() }?.let(::planDayObservedAt)

                if (sheetOpen) {
                    EditorSheet(logGuard, onClosed = { resetSheet(); sheetOpen = false }) { close ->
                        val dismissSheet: () -> Unit = { resetSheet(); close() }
                        LogSheetContent(
                            mode = sheetMode,
                            model = session.model,
                            onMode = {
                                if (it == LogMode.Chooser) resetSheet()
                                sheetMode = it
                            },
                            onEvent = EventDispatcher::send,
                            // No tracker: the chooser leaves its "Track run" row out.
                            onTrackRun = trackRun,
                            onDismiss = dismissSheet,
                            planPrefill = planPrefill,
                            onPlanPrefill = { sheetPlanExercise = it },
                            planObservedAt = planObservedAt,
                            // Finishing a session lands on Today's top, where its done state is.
                            onFinishSession = {
                                dismissSheet()
                                scope.launch { todayListState.animateScrollToItem(0) }
                            },
                        )
                    }
                }
                if (showLegend) {
                    AppSheet(onDismiss = { showLegend = false }) {
                        EvidenceLegendSheet(session.model.grade_definitions)
                    }
                }
                glossaryTerm?.let { term ->
                    AppSheet(onDismiss = { glossaryTerm = null }) { GlossarySheet(term) }
                }
            }
        }
    }
}

/** Room the Log button needs under the banner: the extended FAB (56dp) and
 *  the Scaffold's 16dp FAB margin. */
private val LogButtonRoom = 72.dp

/**
 * The destinations' top chrome under the status bar: the safety [banner], then
 * the [appBar] (the wordmark). The banner always shows whole and is never folded
 * or overlapped (INVARIANT 3; its own Layout in SafetyBanner.kt caps only its
 * extras). The app bar is brand chrome only, so when the banner leaves it no room
 * above the bottom bar ([bottomBarPx], the Scaffold places the two from opposite
 * edges) it is left out instead of being laid over the navigation labels.
 * [onNeedsRail] reports whether a banner leaves less than [logRoomPx] between
 * it and the nav bar ([navBarPx]): the nav bar and the Log button would then be
 * laid over it, so the host swaps them for a side rail (a landscape phone under
 * a hold).
 */
@Composable
private fun TopChrome(
    bottomBarPx: Int,
    navBarPx: Int,
    logRoomPx: Int,
    onNeedsRail: (Boolean) -> Unit,
    banner: @Composable () -> Unit,
    appBar: @Composable () -> Unit,
) {
    Layout(listOf(banner, appBar), Modifier.background(BgTop).statusBarsPadding()) { (bannerSlot, appBarSlot), c ->
        val loose = c.copy(minHeight = 0, maxHeight = Constraints.Infinity)
        val banners = bannerSlot.map { it.measure(loose) }
        val bars = appBarSlot.map { it.measure(loose) }
        val bannerHeight = banners.sumOf { it.height }
        val barHeight = bars.sumOf { it.height }
        onNeedsRail(bannerHeight > 0 && c.hasBoundedHeight && c.maxHeight - bannerHeight - navBarPx < logRoomPx)
        val showBar = !c.hasBoundedHeight || bannerHeight + barHeight <= c.maxHeight - bottomBarPx
        val width = if (c.hasBoundedWidth) c.maxWidth else (banners + bars).maxOfOrNull { it.width } ?: 0
        layout(width, bannerHeight + if (showBar) barHeight else 0) {
            var y = 0
            for (p in if (showBar) banners + bars else banners) {
                p.placeRelative(0, y)
                y += p.height
            }
        }
    }
}

/** Decode-safe saver for the selected bottom-nav tab: stores the enum NAME and
 *  falls back to Today when the saved name no longer exists (e.g. "Coach"
 *  from a build before the Today+Coach merge). The default saver Java-serializes
 *  the enum, and a removed constant would throw inside readObject. */
private val DestSaver = androidx.compose.runtime.saveable.Saver<Dest, String>(
    save = { it.name },
    restore = { name -> Dest.entries.firstOrNull { it.name == name } ?: Dest.Today },
)

