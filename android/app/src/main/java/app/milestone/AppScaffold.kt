package app.milestone

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SheetValue
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import kotlinx.coroutines.launch

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun CoachScaffold(session: CoachSession) {
    val ctx = LocalContext.current
    // The setup prefs the boot effect + dispatch use; read here to make the
    // clear-all ↔ KEY_PLAN_CLEARED coupling explicit (see the onClear below).
    val setupPrefs = remember { ctx.setupPrefs() }
    // Live tracking state drives the "run in progress" reentry chip below.
    val liveTracking by RunSession.tracking.collectAsState()

    // The bottom-nav destination, hoisted ABOVE the full-screen setup/tracker
    // early-returns below so its rememberSaveable slot stays in composition across
    // a tracker/setup round-trip (declared after them, it would be disposed while
    // the tracker replaces the scaffold and come back as Today). It also
    // survives a config change (rotation): saved as the enum NAME with an
    // unknown-name fallback, since the default autoSaver Java-serializes the enum
    // and restoring an icicle that still holds a REMOVED constant (a pre-merge
    // build's "Coach") would throw inside readObject; the same decode-safe pattern
    // as WorkoutType.fromWire.
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

    // Crash-recovery consent, ahead of the full-screen returns below so neither
    // the setup nor a tracker restored after process death can hide it.
    ResumeRunDialog(session)

    if (session.showSetup) {
        // Guided setup replaces the whole scaffold on first run. Completing it
        // writes ONE SetProfile (same wire as the full editor); skipping leaves the
        // profile empty and drops into the normal empty Today. Either way the
        // onboarding flag is set so it never re-prompts.
        // GuidedSetup is a full-screen replacement (outside the Scaffold that
        // owns the status-bar inset), so under edge-to-edge its header would sit
        // under the status bar. The inset belongs on the full-screen container,
        // which lives here at the call site, not inside GuidedSetup's own content.
        Box(Modifier.fillMaxSize().background(BgTop).statusBarsPadding().navigationBarsPadding()) {
            // A re-run (setupInitial != null) must NOT touch the onboarding pref -
            // it's a returning user reviewing their answers, not first-run onboarding.
            val firstRun = session.setupInitial == null
            GuidedSetup(
                initial = session.setupInitial,
                switchTo = setupSwitchTo,
                onComplete = { draft, rungs ->
                    session.dispatch(draft.toEvent(session.model.today()))
                    // Calisthenics self-placement: each answered ladder starts at
                    // its rung (after the profile, before the plan is generated).
                    rungs.forEach { session.dispatch(it) }
                    // Zero-tap after setup: now that a profile exists, generate the
                    // plan immediately so the user lands on the prescription-led
                    // Coach/Today in THIS session (not only after a relaunch). The
                    // core preserves this anchor on later boots.
                    session.dispatch(Event.GeneratePlan(todayEpochDay()))
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

    if (session.showTracker) {
        // The tracking screen replaces this whole scaffold, so it receives the
        // live model and re-pins the SafetyBanner itself (chrome §5: the banner is
        // on EVERY screen, never scrollable or dismissable). onEvent keeps the
        // banner's undo/Add-details paths live during a run.
        RunTrackingScreen(
            model = session.model,
            onEvent = { session.dispatch(it) },
            onFinish = { _ ->
                // The run save routes its Core.send through EventDispatcher.exclusive,
                // so the consumer already published the saved model in queue order and
                // the model collector re-seeds session.model — nothing to publish here.
                session.showTracker = false
            },
            onLogRunManually = {
                session.showTracker = false
                sheetPlanExercise = null
                sheetPlanDay = null
                sheetMode = LogMode.Run
                sheetOpen = true
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
        session.setupInitial = session.model.profile?.let { ProfileDraft.from(it) } ?: ProfileDraft.SEED
        setupSwitchTo = switchTo
        session.showSetup = true
    }
    // "How evidence grading works" legend sheet, opened from the "?" on grade
    // badges. Static copy; no coaching logic.
    var showLegend by remember { mutableStateOf(false) }
    // Glossary bottom-sheet: the term key to open it scrolled to, or null
    // when closed. Opened from tappable term chips app-wide. Static UI copy;
    // definitions of jargon, NOT KB training claims.
    var glossaryTerm by remember { mutableStateOf<String?>(null) }

    // Today's scroll position, hoisted so the Log FAB can collapse to its icon
    // once the list leaves the top.
    val todayListState = rememberLazyListState()
    val fabExpanded by remember { derivedStateOf { todayListState.firstVisibleItemIndex == 0 } }
    // Whether the currently-shown editor has unsaved edits, hoisted here so the
    // sheet's SWIPE-DOWN dismissal can honor the same "Discard this entry?" guard
    // the editor's X already enforces. The Set/Run editors report their dirty state
    // up; other quick forms leave it false (swipe-down just closes them).
    var sheetDirty by rememberSaveable { mutableStateOf(false) }
    var confirmDiscardSheet by rememberSaveable { mutableStateOf(false) }
    // A set loop in progress (reported by the Set editor): the sheet refuses to be
    // swiped or scrim-tapped away; X or "Done for now" close it.
    var sheetLocked by rememberSaveable { mutableStateOf(false) }
    val sheetState = rememberModalBottomSheetState(
        // Open fully expanded: the chooser list and the editors (with their
        // in-sheet keypad) must be visible without a second drag; the partial
        // detent left them below the fold.
        skipPartiallyExpanded = true,
        // A locked set loop refuses to hide. A dirty editor blocks the
        // swipe-to-hide and raises the discard confirm INSTEAD, so the sheet stays
        // VISIBLE under the dialog. That way "Keep editing" (and an outside-tap on
        // the dialog) leave the sheet in place; no stranded invisible sheet / dead
        // Log FAB). Discard and the programmatic close clear both flags first.
        confirmValueChange = { target ->
            when {
                target != SheetValue.Hidden -> true
                sheetLocked && sheetMode == LogMode.Set -> false
                sheetDirty && sheetMode != LogMode.Chooser -> {
                    confirmDiscardSheet = true
                    false
                }
                else -> true
            }
        },
    )
    val scope = rememberCoroutineScope()
    // Animate the sheet closed, then drop it from composition once hidden.
    val dismissSheet: () -> Unit = {
        sheetDirty = false
        sheetLocked = false
        sheetPlanExercise = null
        sheetPlanDay = null
        scope.launch { sheetState.hide() }.invokeOnCompletion {
            if (!sheetState.isVisible) sheetOpen = false
        }
    }

    // The Today Log FAB. Reopening always resets to the chooser so the sheet
    // never reappears mid-editor.
    val openLog: () -> Unit = {
        sheetMode = LogMode.Chooser
        sheetPlanExercise = null
        sheetPlanDay = null
        sheetOpen = true
    }

    Scaffold(
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
            Column(Modifier.background(BgTop).statusBarsPadding()) {
                SafetyBannerHost(
                    session.model,
                    onEvent = { session.dispatch(it) },
                    modifier = Modifier
                        .padding(horizontal = Space.Screen.dp)
                        .padding(top = Space.Md.dp),
                    lastPain = session.lastPain,
                    onAddDetails = { sheetMode = LogMode.Readiness; sheetOpen = true },
                    onOpenProfile = { row -> profileFocus = row; selected = Dest.Profile },
                )
                // Brand lockup IS the title on destinations (chrome §2): no
                // secondary title string anywhere in content. No overflow menu:
                // "Clear all data" lives only in Profile's Danger zone, since a
                // destructive action a tap away on every screen is too easy to hit.
                BrandTopBar()
            }
        },
        bottomBar = {
            Column {
                // Run-in-progress chip pinned just above the nav bar (chrome §4),
                // below content, never above the safety banner (which is topmost).
                if (liveTracking && !session.showTracker) {
                    RunInProgressChip { session.showTracker = true }
                }
                MilestoneNavBar(selected) { selected = it }
            }
        },
        floatingActionButton = {
            // The Log FAB belongs to Today only (chrome §1). It floats over the
            // list; Today's rows keep their full width and the list's bottom
            // padding lets its last row scroll clear of it.
            if (selected == Dest.Today) {
                val logLabel = stringResource(R.string.main_log)
                ExtendedFloatingActionButton(
                    onClick = openLog,
                    // Extended ("+ Log") at the top of Today; icon-only once the list scrolls.
                    expanded = fabExpanded,
                    // The extended FAB clears its content's semantics, so the
                    // visible "Log" text never reaches TalkBack; name the button.
                    modifier = Modifier.semantics { contentDescription = logLabel },
                    containerColor = Accent,
                    // Dark OnAccent on every accent fill: white (#fff) text fails
                    // AA contrast against the accent here.
                    contentColor = OnAccent,
                    shape = RoundedCornerShape(Space.Card.dp),
                    text = { Text(logLabel, style = Type.Body.copy(fontWeight = FontWeight.ExtraBold)) },
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
        ClearAllDataDialog(
            visible = confirmClearAll,
            onDismiss = { confirmClearAll = false },
            onClear = {
                // Every logged family + derived coaching output; the profile
                // (training configuration) stays. ONE batch: a single core turn with
                // one view decode and one published model, never a run of partial
                // models. (There are no Cooper/CriticalSpeed/APRE encoders in the
                // shell: those calculators have no UI, so nothing to clear for them.)
                session.dispatchAll(
                    listOf(
                        Event.ClearSets, Event.ClearRuns, Event.ClearReadiness,
                        Event.ClearCheckins, Event.ClearReview, Event.ClearWeeklyCheckin, Event.ClearRacePrediction,
                        Event.ClearHypertrophyPlan, Event.ClearProtein, Event.ClearHrZones,
                        Event.ClearPlan, Event.ClearActivities,
                    ),
                )
                // Explicit decision: after "Clear all data" the plan is NOT
                // auto-regenerated on the next boot; the user re-creates it from the
                // PlanPromptCard. The dispatch echo already sets this marker for the
                // ClearPlan above, but writing it here makes the durable-disable an
                // explicit part of clear-all rather than an invisible side effect.
                setupPrefs.edit().putBoolean(KEY_PLAN_CLEARED, true).apply()
            },
        )
        CompositionLocalProvider(
            LocalEvidenceLegend provides { showLegend = true },
            LocalGlossary provides { term -> glossaryTerm = term },
        ) {
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(pad),
            ) {
                // Destinations are pure projections of the one hoisted `model`.
                Box(Modifier.weight(1f)) {
                    when (selected) {
                        Dest.Today -> {
                            TodayDestination(
                                model = session.model,
                                listState = todayListState,
                                onEvent = { session.dispatch(it) },
                                lastPain = session.lastPain,
                                onOpenProfile = { row -> profileFocus = row; selected = Dest.Profile },
                                onStartSetup = {
                                    // First-run entry point (no seed): keep the onboarding
                                    // pref semantics at the call site above.
                                    session.setupInitial = null
                                    session.showSetup = true
                                },
                                onRerunSetup = { rerunSetup(null) },
                                onLogPlanItem = {
                                    sheetPlanExercise = it.exercise
                                    sheetPlanDay = null
                                    sheetMode = LogMode.Set
                                    sheetOpen = true
                                },
                                onLogPastItem = { item, day ->
                                    sheetPlanExercise = item.exercise
                                    sheetPlanDay = day
                                    sheetMode = LogMode.Set
                                    sheetOpen = true
                                },
                                onLogPastRun = { day ->
                                    sheetPlanExercise = null
                                    sheetPlanDay = day
                                    sheetMode = LogMode.Run
                                    sheetOpen = true
                                },
                                onTrackRun = { session.showTracker = true },
                                onOpenSheet = { mode ->
                                    sheetPlanExercise = null
                                    sheetPlanDay = null
                                    sheetMode = mode
                                    sheetOpen = true
                                },
                            )
                        }
                        Dest.History -> HistoryDestination(
                            model = session.model,
                            onEvent = { session.dispatch(it) },
                            onTrackRun = { session.showTracker = true },
                            onLogSet = {
                                sheetPlanExercise = null
                                sheetPlanDay = null
                                sheetMode = LogMode.Set
                                sheetOpen = true
                            },
                        )
                        Dest.Profile -> ProfileDestination(
                            ctx = ctx,
                            model = session.model,
                            onEvent = { session.dispatch(it) },
                            onClearAll = { confirmClearAll = true },
                            onRerunSetup = rerunSetup,
                            focusRow = profileFocus,
                            onFocusHandled = { profileFocus = null },
                            planAnswersOpen = planAnswersOpen,
                            onPlanAnswersOpenChange = { planAnswersOpen = it },
                        )
                    }
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
            // Seed the editor's stamp to a past plan day's local noon (null = today's flow).
            val planObservedAt = sheetPlanDay?.takeIf { it != session.model.today() }?.let(::planDayObservedAt)

            if (sheetOpen) {
                // A set loop in progress can't be swiped or scrim-tapped away (a scroll
                // back up to the timer closed the whole loop); it closes via X or Finish.
                val loopLocked = sheetLocked && sheetMode == LogMode.Set
                ModalBottomSheet(
                    // The dirty-editor and loop guards live in sheetState.confirmValueChange
                    // (which keeps the sheet visible while confirming). By the time
                    // onDismissRequest fires the hide is already committed on a clean,
                    // unlocked sheet, so this just finishes the close.
                    onDismissRequest = { sheetDirty = false; sheetLocked = false; sheetPlanExercise = null; sheetPlanDay = null; sheetOpen = false },
                    sheetState = sheetState,
                    sheetGesturesEnabled = !loopLocked,
                    // Sheet ground a hair off BgElevated for separation (05-log §1);
                    // on paper the elevated surface already separates.
                    containerColor = BgSunken,
                ) {
                    SheetNavBarScrim()
                    LogSheetContent(
                        mode = sheetMode,
                        model = session.model,
                        initialReadinessSignal = ReadinessSignal.WellnessZ,
                        // Returning to the chooser clears the dirty flag (the entry was
                        // handled or discarded via the editor's own guard).
                        onMode = {
                            if (it == LogMode.Chooser) {
                                sheetDirty = false; sheetLocked = false; sheetPlanExercise = null; sheetPlanDay = null
                            }
                            sheetMode = it
                        },
                        onEvent = { session.dispatch(it) },
                        onTrackRun = { session.showTracker = true },
                        onDismiss = dismissSheet,
                        onDirtyChange = { sheetDirty = it },
                        planPrefill = planPrefill,
                        onPlanPrefill = { sheetPlanExercise = it },
                        planObservedAt = planObservedAt,
                        onSheetLockChange = { sheetLocked = it },
                        // Finishing a session lands on Today's top, where its done state is.
                        onFinishSession = {
                            dismissSheet()
                            scope.launch { todayListState.animateScrollToItem(0) }
                        },
                    )
                }
            }
            // The swipe-down discard confirmation for a dirty Log editor.
            if (confirmDiscardSheet) {
                AlertDialog(
                    onDismissRequest = { confirmDiscardSheet = false },
                    shape = RoundedCornerShape(Space.Card.dp),
                    title = { Text(stringResource(R.string.main_discard_entry_title)) },
                    confirmButton = {
                        TextButton(onClick = {
                            confirmDiscardSheet = false
                            sheetDirty = false
                            sheetLocked = false
                            sheetOpen = false
                        }) { Text(stringResource(R.string.main_discard), color = DangerText) }
                    },
                    dismissButton = {
                        TextButton(onClick = { confirmDiscardSheet = false }) { Text(stringResource(R.string.main_keep_editing)) }
                    },
                )
            }
            // "How evidence grading works" legend, reached from the "?" on any
            // grade badge. Static reference copy; no coaching logic.
            if (showLegend) {
                ModalBottomSheet(
                    onDismissRequest = { showLegend = false },
                    // Fully expanded only: Back and swipe close the sheet in one step
                    // (a half-height detent made Back collapse it first).
                    sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
                    containerColor = BgSunken,
                ) {
                    SheetNavBarScrim()
                    EvidenceLegendSheet(session.model.grade_definitions)
                }
            }
            // Glossary: one app-wide sheet defining the jargon, opened from any
            // term chip. Static UI copy; no coaching logic, no KB claim.
            glossaryTerm?.let { term ->
                ModalBottomSheet(
                    onDismissRequest = { glossaryTerm = null },
                    sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
                    containerColor = BgSunken,
                ) {
                    SheetNavBarScrim()
                    GlossarySheet(term)
                }
            }
        }
    }
}

/** Decode-safe saver for the selected bottom-nav tab: stores the enum NAME and
 *  falls back to Today when the saved name no longer exists (e.g. "Coach"
 *  from a build before the Today+Coach merge). */
private val DestSaver = androidx.compose.runtime.saveable.Saver<Dest, String>(
    save = { it.name },
    restore = { name -> Dest.entries.firstOrNull { it.name == name } ?: Dest.Today },
)

/** Which content the Log bottom sheet shows: the chooser, or one editor. */
internal enum class LogMode { Chooser, Set, Run, Checkin, Readiness, Review, WeeklyCheckin, Pain, Activity }
