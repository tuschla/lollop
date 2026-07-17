package de.tuschla.fitnessanlage

import android.content.Context
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.automirrored.filled.List
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Person
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import java.util.Locale
import kotlinx.coroutines.launch
import org.osmdroid.config.Configuration

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // osmdroid needs a config + user agent set before any MapView inflates.
        Configuration.getInstance().load(this, getSharedPreferences("osmdroid", Context.MODE_PRIVATE))
        Configuration.getInstance().userAgentValue = packageName
        ThemeSettings.load(this)
        setContent {
            FitnessAnlageTheme {
                Surface(modifier = Modifier.fillMaxSize(), color = BgTop) {
                    CoachScreen()
                }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun CoachScreen() {
    val ctx = LocalContext.current
    // Replay the persisted event log; on a fresh install (nothing replayed) seed a
    // representative profile so the engine still renders content on first frame.
    // Remember whether this was a fresh install so onboarding can invite the user
    // to personalize the seeded profile (see the Profile section below), the seed
    // makes model.profile non-null, so that flag is the only first-run signal left.
    val freshInstall = remember { Core.restore(ctx) == 0 }
    var model by remember {
        mutableStateOf(
            if (freshInstall) {
                Core.send(ProfileDraft.SEED.toEvent())
            } else {
                Core.currentView()
            }
        )
    }

    // rememberSaveable, not remember: a config change (rotation) recreates this
    // Activity, and a plain remember would reset this to false, bouncing the user
    // off the live tracking screen mid-run even though the foreground service keeps
    // recording. The saveable flag keeps them on the map across recreation.
    var showTracker by rememberSaveable { mutableStateOf(false) }
    if (showTracker) {
        RunTrackingScreen(onFinish = { vm ->
            if (vm != null) model = vm
            showTracker = false
        })
        return
    }

    // The bottom-nav destination. rememberSaveable so the selected tab survives a
    // config change (rotation), a plain remember would bounce the user back to
    // Today on every rotation. Order matches the NavigationBar items below and the
    // `when (selected)` in the scaffold content.
    var selected by rememberSaveable { mutableStateOf(0) }

    // Log bottom-sheet state. `sheetOpen` drives whether the ModalBottomSheet is
    // composed; `sheetMode` is which content it shows (the chooser first, then one
    // of the four editors when picked). A plain remember is fine, a sheet dismisses
    // on rotation anyway, so there's nothing to preserve across recreation.
    var sheetOpen by remember { mutableStateOf(false) }
    var sheetMode by remember { mutableStateOf(LogMode.Chooser) }
    val sheetState = rememberModalBottomSheetState()
    val scope = rememberCoroutineScope()
    // Animate the sheet closed, then drop it from composition once hidden.
    val dismissSheet: () -> Unit = {
        scope.launch { sheetState.hide() }.invokeOnCompletion {
            if (!sheetState.isVisible) sheetOpen = false
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Text(stringResource(R.string.app_name), style = Type.Title)
                },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = BgTop,
                    titleContentColor = Accent,
                ),
            )
        },
        bottomBar = {
            NavigationBar(containerColor = BgTop) {
                val items = listOf(
                    Triple("Today", Icons.Default.Home, 0),
                    Triple("Coach", Icons.Default.Info, 1),
                    Triple("History", Icons.AutoMirrored.Filled.List, 2),
                    Triple("Profile", Icons.Default.Person, 3),
                )
                items.forEach { (label, icon, index) ->
                    NavigationBarItem(
                        selected = selected == index,
                        onClick = { selected = index },
                        icon = { Icon(icon, contentDescription = label) },
                        label = { Text(label) },
                        colors = NavigationBarItemDefaults.colors(
                            selectedIconColor = Accent,
                            selectedTextColor = Accent,
                            indicatorColor = BgElevated,
                            unselectedIconColor = OnBgMuted,
                            unselectedTextColor = OnBgMuted,
                        ),
                    )
                }
            }
        },
        floatingActionButton = {
            // The Log FAB belongs to Today only: the other destinations are
            // read-only (Coach/History) or config (Profile). Reopening always
            // resets to the chooser so the sheet never reappears mid-editor.
            if (selected == 0) {
                ExtendedFloatingActionButton(
                    onClick = {
                        sheetMode = LogMode.Chooser
                        sheetOpen = true
                    },
                    containerColor = Accent,
                    contentColor = Color.White,
                    text = { Text("Log") },
                    icon = { Text("+", style = Type.Title) },
                )
            }
        },
    ) { pad ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(pad),
        ) {
            // Pinned above the destination switch: a DO-NOT-TRAIN hold (HARD RULE 3)
            // must never scroll off-screen NOR be lost by switching tabs, so the
            // banner lives in the root scaffold outside/above the `when (selected)`.
            // It renders nothing when no tier/block is active, costing no space on a
            // normal screen; when active it takes the screen gutter itself (top +
            // sides) while each destination's own contentPadding supplies the gap
            // below it.
            SafetyBanner(
                model,
                Modifier
                    .padding(horizontal = Space.Screen.dp)
                    .padding(top = Space.Screen.dp),
            )
            // Destinations are pure projections of the one hoisted `model`; each is
            // handed the live view plus the dispatch/tracker callbacks so the core
            // stays the single source of truth.
            when (selected) {
                0 -> TodayDestination(
                    model = model,
                    freshInstall = freshInstall,
                    onEvent = { model = Core.send(it) },
                    onTrackRun = { showTracker = true },
                    onGoToCoach = { selected = 1 },
                    onGoToHistory = { selected = 2 },
                )
                1 -> CoachDestination(
                    model = model,
                    onEvent = { model = Core.send(it) },
                )
                2 -> HistoryDestination(
                    model = model,
                    onEvent = { model = Core.send(it) },
                )
                else -> ProfileDestination(
                    ctx = ctx,
                    model = model,
                    freshInstall = freshInstall,
                    onEvent = { model = Core.send(it) },
                )
            }
        }

        if (sheetOpen) {
            ModalBottomSheet(
                onDismissRequest = { sheetOpen = false },
                sheetState = sheetState,
                containerColor = BgElevated,
            ) {
                LogSheetContent(
                    mode = sheetMode,
                    onMode = { sheetMode = it },
                    onEvent = { model = Core.send(it) },
                    onTrackRun = { showTracker = true },
                    onDismiss = dismissSheet,
                )
            }
        }
    }
}

/** Which content the Log bottom sheet shows: the chooser, or one editor. */
private enum class LogMode { Chooser, Set, Run, Readiness, Review }

/**
 * Body of the Log [ModalBottomSheet]. First renders the chooser list; picking an
 * editor swaps [mode] to that editor (reused verbatim from LogEntry.kt). Each
 * editor's own submit button stays at the bottom of the sheet; its callback both
 * dispatches the Event and dismisses the sheet. The two fast-paths: Report pain
 * and Track run, act immediately from the chooser and dismiss.
 */
@Composable
private fun LogSheetContent(
    mode: LogMode,
    onMode: (LogMode) -> Unit,
    onEvent: (Event) -> Unit,
    onTrackRun: () -> Unit,
    onDismiss: () -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            // The tallest editor (ReviewEditor, with the week-fatigue + run-context
            // toggles expanded) exceeds a ModalBottomSheet's max height. Without a
            // scroll the Submit button is pushed off-screen and unreachable: the
            // user can't complete a full review, so make the sheet body scrollable.
            .verticalScroll(rememberScrollState())
            .padding(horizontal = Space.Screen.dp)
            .padding(bottom = Space.Screen.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Card.dp),
    ) {
        // From inside an editor the only exit used to be dismissing the whole sheet
        // and reopening: a mis-tap on the chooser cost a full round-trip. A Back row
        // returns to the chooser in place so switching editors is one tap.
        if (mode != LogMode.Chooser) {
            TextButton(onClick = { onMode(LogMode.Chooser) }) {
                Text("← Back", style = Type.Body)
            }
        }
        when (mode) {
            LogMode.Chooser -> {
                val status = LocalStatusColors.current
                // Pinned top + danger ground: the Pain fast-path must never be
                // buried. Same event the old Today "Pain flag" button emitted.
                Button(
                    onClick = {
                        onEvent(
                            Event.SubmitReadiness(
                                signal = ReadinessSignal.Pain,
                                value = 1.0,
                                observedAt = System.currentTimeMillis() / 1000,
                            )
                        )
                        onDismiss()
                    },
                    modifier = Modifier.fillMaxWidth(),
                    colors = ButtonDefaults.buttonColors(
                        containerColor = status.dangerStrong,
                    ),
                ) { Text("Report pain", color = Color.White) }

                LogChoice("Log set") { onMode(LogMode.Set) }
                LogChoice("Log run") { onMode(LogMode.Run) }
                LogChoice("Log readiness") { onMode(LogMode.Readiness) }
                LogChoice("Session review") { onMode(LogMode.Review) }
                LogChoice("Track run (GPS)") {
                    onTrackRun()
                    onDismiss()
                }
            }
            LogMode.Set -> LogSetEditor { set -> onEvent(set); onDismiss() }
            LogMode.Run -> LogRunEditor { run -> onEvent(run); onDismiss() }
            LogMode.Readiness -> ReadinessEditor { r -> onEvent(r); onDismiss() }
            LogMode.Review -> ReviewEditor { review -> onEvent(review); onDismiss() }
        }
    }
}

/** A large full-width tap target in the Log chooser list. */
@Composable
private fun LogChoice(label: String, onClick: () -> Unit) {
    OutlinedButton(
        onClick = onClick,
        modifier = Modifier.fillMaxWidth(),
    ) { Text(label, color = OnBgBody) }
}

/**
 * Today, deliberately sparse (spec §3-Today). Logging lives behind the root
 * scaffold's Log FAB → ModalBottomSheet (see [LogSheetContent]). This screen
 * answers "what did the coach last say, and what did I last do", with one-tap
 * links deeper into Coach / History.
 *
 * The "Latest" card is a SHELL-ONLY heuristic (no core change): it surfaces the
 * newest coaching signal: `model.feedback` if present, else the top readiness
 * adjustment by the same [byAdjustmentPriority] order Coach uses (falling back
 * to `review_adjustments` when `adjustments` is empty). It reuses [EvidenceCard]
 * verbatim so the grade / SAFETY / CONTESTED chips read identically to Coach.
 *
 * On a genuinely empty state (no feedback, no adjustments, no lifts, no runs)
 * nothing but a friendly hint renders, no empty cards.
 */
@Composable
private fun TodayDestination(
    model: ViewModel,
    freshInstall: Boolean,
    onEvent: (Event) -> Unit,
    onTrackRun: () -> Unit,
    onGoToCoach: () -> Unit,
    onGoToHistory: () -> Unit,
) {
    // Newest coaching signal: feedback wins; else the highest-priority readiness
    // adjustment (adjustments first, review_adjustments as the fallback source).
    val topAdjustment = model.adjustments.byAdjustmentPriority().firstOrNull()
        ?: model.review_adjustments.byAdjustmentPriority().firstOrNull()
    // Last logged activity: the core appends new entries, so the lists are
    // oldest-first (History reverses them for display), lastOrNull() is the most
    // recent of each. The two result views carry no timestamp, so the shell can't
    // tell whether the last lift or the last run happened more recently; showing
    // both (labelled) instead of picking one avoids hiding today's run behind an
    // older lift while staying sparse (at most two lines).
    val lastLift = model.lifts.lastOrNull()?.summary
    val lastRun = model.runs.lastOrNull()?.summary

    val hasCoachSignal = model.feedback != null || topAdjustment != null
    val hasActivity = lastLift != null || lastRun != null
    val hasAnything = hasCoachSignal || hasActivity

    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(Space.Screen.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Card.dp),
    ) {
        if (hasCoachSignal) {
            item { SectionTitle("Latest") }
            item {
                val fb = model.feedback
                if (fb != null) {
                    EvidenceCard(fb.message, fb.grade, fb.citation, fb.confidence, fb.safety_critical, fb.contested, fb.category)
                } else if (topAdjustment != null) {
                    EvidenceCard(topAdjustment.summary, topAdjustment.grade, topAdjustment.citation, topAdjustment.confidence, topAdjustment.safety_critical, topAdjustment.contested)
                }
            }
            item {
                TextButton(onClick = onGoToCoach) {
                    Text("See all in Coach →", style = Type.Body)
                }
            }
        }

        if (hasActivity) {
            item { SectionTitle("Recent activity") }
            item {
                PlainCard {
                    Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
                        lastLift?.let { Text("Lift · $it", color = OnBgBody, style = Type.Body) }
                        lastRun?.let { Text("Run · $it", color = OnBgBody, style = Type.Body) }
                    }
                }
            }
            item {
                TextButton(onClick = onGoToHistory) {
                    Text("History →", style = Type.Body)
                }
            }
        }

        if (!hasAnything) {
            item {
                Text(
                    "Log your first session with the ＋ Log button.",
                    color = OnBgMuted,
                    style = Type.Body,
                )
            }
        }
    }
}

/**
 * Coach priority sort (spec §5): safety-critical first (SAFETY-chipped cards to
 * the top of their group), then strongest grade, then confidence descending.
 * Each comparator negates the key so the "highest" sorts first under ascending
 * `sortedWith`. Applied WITHIN each Coach group only, never globally, never on
 * History/Today.
 */
private fun List<AdjustmentView>.byAdjustmentPriority(): List<AdjustmentView> =
    sortedWith(
        compareByDescending<AdjustmentView> { it.safety_critical }
            .thenByDescending { gradeRank(it.grade) }
            .thenByDescending { it.confidence },
    )

private fun List<GuidanceView>.byGuidancePriority(): List<GuidanceView> =
    sortedWith(
        compareByDescending<GuidanceView> { it.safety_critical }
            .thenByDescending { gradeRank(it.grade) }
            .thenByDescending { it.confidence },
    )

/**
 * Coach, read-only evidence-graded output: programming guidance, readiness
 * adjustments, session feedback + deloads, and the collapsed reference section.
 */
@Composable
private fun CoachDestination(
    model: ViewModel,
    onEvent: (Event) -> Unit,
) {
    // Destructive clears now live in this Coach-section overflow (⋮) menu instead
    // of inline section headers (spec §3 / roadmap #5). ClearReview and
    // ClearReadiness keep the exact confirm dialogs + Events they had inline; only
    // their trigger moved. Each owns a `confirming` flag its DropdownMenuItem sets
    // and its ClearConfirmDialog reads. The two clears' explanatory messages are
    // preserved verbatim from the former ConfirmClearHeader call sites.
    var confirmReview by remember { mutableStateOf(false) }
    var confirmReadiness by remember { mutableStateOf(false) }
    val hasReview = model.feedback != null || model.review_adjustments.isNotEmpty()
    val hasReadiness = model.adjustments.isNotEmpty() || model.input_count > 0
    val readinessMessage =
        if (model.adjustments.isEmpty() && model.input_count > 0) {
            "This clears today's ${model.input_count} readiness input(s)."
        } else {
            "This clears today's readiness inputs and every adjustment they produced - including any safety hold that blocks training. Re-log your readiness to restore it."
        }
    val overflowItems = buildList {
        if (hasReview) add("Clear session review" to { confirmReview = true })
        if (hasReadiness) add("Clear readiness signals" to { confirmReadiness = true })
    }

    Column(modifier = Modifier.fillMaxSize()) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = Space.Screen.dp),
        ) {
            SectionBarWithOverflow(
                title = "Coach",
                items = overflowItems,
            )
        }
        ClearConfirmDialog(
            visible = confirmReview,
            message = "This clears the session review behind this feedback and any session deloads it produced. Re-submit a review to restore it.",
            onDismiss = { confirmReview = false },
            onClear = { onEvent(Event.ClearReview) },
        )
        ClearConfirmDialog(
            visible = confirmReadiness,
            message = readinessMessage,
            onDismiss = { confirmReadiness = false },
            onClear = { onEvent(Event.ClearReadiness) },
        )
        LazyColumn(
            modifier = Modifier.fillMaxSize(),
            contentPadding = PaddingValues(Space.Screen.dp),
            verticalArrangement = Arrangement.spacedBy(Space.Card.dp),
        ) {
            // Goal-race predictor: an on-demand tool (not sticky coaching output),
            // so it lives collapsed at the top of Coach. Submitting drives
            // Event.PredictRace; the graded result renders just below from
            // model.race_prediction until ClearRacePrediction drops it.
            item {
                ExpandableSection("Race predictor") {
                    RacePredictorForm { onEvent(it) }
                }
            }
            model.race_prediction?.let { rp ->
                item {
                    val headline = "${rp.goal_label}: ${rp.predicted}"
                    EvidenceCard(
                        summary = if (rp.summary.isNotBlank()) "$headline\n${rp.summary}" else headline,
                        grade = rp.grade,
                        citation = rp.citation,
                        confidence = rp.confidence,
                        safetyCritical = rp.safety_critical,
                        contested = rp.contested,
                    )
                }
                item {
                    TextButton(onClick = { onEvent(Event.ClearRacePrediction) }) {
                        Text("Clear prediction", style = Type.Body)
                    }
                }
            }

            // Hypertrophy volume planner: another on-demand tool. Submitting drives
            // Event.PlanHypertrophyMeso; the graded per-week plan renders just below
            // from model.hypertrophy_plan until ClearHypertrophyPlan drops it.
            item {
                ExpandableSection("Volume planner") {
                    HypertrophyPlannerForm { onEvent(it) }
                }
            }
            if (model.hypertrophy_plan.isNotEmpty()) {
                items(model.hypertrophy_plan) {
                    EvidenceCard(it.summary, it.grade, it.citation, it.confidence, it.safety_critical, it.contested, it.section)
                }
                item {
                    TextButton(onClick = { onEvent(Event.ClearHypertrophyPlan) }) {
                        Text("Clear plan", style = Type.Body)
                    }
                }
            }

            // Protein target: an on-demand tool. Submitting drives
            // Event.ComputeProtein (bodyweight × graded g/kg → absolute g/day);
            // the graded row(s) render just below from model.protein_targets
            // until ClearProtein drops them.
            item {
                ExpandableSection("Protein target") {
                    ProteinForm { bodyweight, masters, deficit ->
                        onEvent(Event.ComputeProtein(bodyweight, masters, deficit))
                    }
                }
            }
            if (model.protein_targets.isNotEmpty()) {
                items(model.protein_targets) {
                    EvidenceCard(it.summary, it.grade, it.citation, it.confidence, it.safety_critical, it.contested, it.section)
                }
                item {
                    TextButton(onClick = { onEvent(Event.ClearProtein) }) {
                        Text("Clear protein", style = Type.Body)
                    }
                }
            }

            // HR-zone calculator: an on-demand tool. Submitting drives
            // Event.ComputeHrZones (age → Tanaka HRmax + five Daniels %HRmax band
            // bpm ranges); the graded rows render just below from model.hr_zones
            // until ClearHrZones drops them.
            item {
                ExpandableSection("Heart-rate zones") {
                    HrZonesForm { age -> onEvent(Event.ComputeHrZones(age)) }
                }
            }
            if (model.hr_zones.isNotEmpty()) {
                items(model.hr_zones) {
                    EvidenceCard(it.summary, it.grade, it.citation, it.confidence, it.safety_critical, it.contested, it.section)
                }
                item {
                    TextButton(onClick = { onEvent(Event.ClearHrZones) }) {
                        Text("Clear zones", style = Type.Body)
                    }
                }
            }

            model.feedback?.let { fb ->
                // Feedback comes from the last SubmitReview and is otherwise sticky
                // (it also survives a restart via event-log replay). Its Clear now
                // lives in the Coach overflow menu (ClearReview drops model.review
                // and the card with it).
                item { SectionTitle("Session feedback") }
                item { FeedbackCard(fb) }
            }

            if (model.review_adjustments.isNotEmpty()) {
                // Week-level deloads share the review's lifecycle, so their Clear is
                // the same ClearReview in the overflow, not the readiness adjustments
                // below, which ClearReadiness owns.
                item { SectionTitle("Session deloads") }
                items(model.review_adjustments.byAdjustmentPriority()) {
                    EvidenceCard(it.summary, it.grade, it.citation, it.confidence, it.safety_critical, it.contested)
                }
            }

            if (model.adjustments.isNotEmpty()) {
                // Clearing readiness wipes model.inputs, which recomputes to an empty
                // adjustment set and drops the safety tier, the only in-app path out
                // of a "DO NOT TRAIN" banner once a Pain/red-flag signal is resolved.
                // That clear now lives (guarded) in the Coach overflow menu.
                item { SectionTitle("Readiness adjustments") }
                items(model.adjustments.byAdjustmentPriority()) {
                    EvidenceCard(it.summary, it.grade, it.citation, it.confidence, it.safety_critical, it.contested)
                }
            } else if (model.input_count > 0) {
                // Readiness was logged but produced no adjustment (an all-clear
                // day). Without this the submission would leave no trace on
                // screen, the user could not tell it registered, so surface a
                // confirmation; its clear path is the Coach overflow menu.
                item {
                    Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
                        SectionTitle("Readiness: all clear (${model.input_count})")
                        Text(
                            "Logged - no adjustment needed today.",
                            color = OnBgMuted,
                            style = Type.Body,
                        )
                    }
                }
            }

        if (model.guidance.isNotEmpty()) {
            item { SectionTitle("Programming guidance") }
            // Group by engine section so each block (Strength, Running, …) is a
            // collapsible unit: a runner can fold away eight Strength cards
            // without losing today's running coaching. Expanded by default:
            // guidance is the live coaching output, so the screen still shows
            // everything on open (unlike Reference, which is background material
            // and starts collapsed). The section name now lives in the header,
            // so the cards inside drop their own per-card section chip.
            model.guidance.groupBy { it.section }.forEach { (section, rows) ->
                item(key = "guidance-$section") {
                    ExpandableSection(section, initiallyExpanded = true) {
                        Column(verticalArrangement = Arrangement.spacedBy(Space.Card.dp)) {
                            // Sort WITHIN this section bucket only: the grouping by
                            // section is preserved; priority orders the cards inside it.
                            rows.byGuidancePriority().forEach {
                                EvidenceCard(it.summary, it.grade, it.citation, it.confidence, it.safety_critical, it.contested)
                            }
                        }
                    }
                }
            }
        }

        if (model.reference.isNotEmpty()) {
            // Reference is background material, not today's action: collapse it
            // by default so it stops padding the scroll below the live coaching
            // output. One LazyColumn item wrapping the (short) list is fine; it
            // trades virtualization the handful of reference cards never needed.
            item {
                ExpandableSection("Reference") {
                    Column(verticalArrangement = Arrangement.spacedBy(Space.Card.dp)) {
                        // Each reference card keeps its own section chip, so the cards
                        // must stay grouped by section: a global priority sort would
                        // interleave sections (Strength/Nutrition/Hybrid…) by grade and
                        // scramble the chips. Group first (preserving the core's
                        // section-contiguous order) and priority-sort only WITHIN each
                        // section bucket, mirroring the guidance list above.
                        model.reference.groupBy { it.section }.values.forEach { rows ->
                            rows.byGuidancePriority().forEach {
                                EvidenceCard(it.summary, it.grade, it.citation, it.confidence, it.safety_critical, it.contested, it.section)
                            }
                        }
                    }
                }
            }
        }
        }
    }
}

/** History, the logged lifts and runs lists; bulk Clear lives in the ⋮ overflow. Per-item Export GPX stays on the RunCard. */
@Composable
private fun HistoryDestination(
    model: ViewModel,
    onEvent: (Event) -> Unit,
) {
    // Destructive bulk clears now live in this section's overflow (⋮) menu instead
    // of inline section headers (spec §3 / roadmap #5). The confirm dialogs + Events
    // are unchanged; only their trigger moved. Each clear owns a `confirming` flag
    // that its DropdownMenuItem sets and its ClearConfirmDialog reads.
    var confirmSets by remember { mutableStateOf(false) }
    var confirmRuns by remember { mutableStateOf(false) }
    val overflowItems = buildList {
        if (model.lifts.isNotEmpty()) add("Clear all sets" to { confirmSets = true })
        if (model.runs.isNotEmpty()) add("Clear all runs" to { confirmRuns = true })
    }

    Column(modifier = Modifier.fillMaxSize()) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = Space.Screen.dp),
        ) {
            SectionBarWithOverflow(
                title = "History",
                items = overflowItems,
            )
        }
        ClearConfirmDialog(
            visible = confirmSets,
            onDismiss = { confirmSets = false },
            onClear = { onEvent(Event.ClearSets) },
        )
        ClearConfirmDialog(
            visible = confirmRuns,
            onDismiss = { confirmRuns = false },
            onClear = { onEvent(Event.ClearRuns) },
        )
        LazyColumn(
            modifier = Modifier.fillMaxSize(),
            contentPadding = PaddingValues(Space.Screen.dp),
            verticalArrangement = Arrangement.spacedBy(Space.Card.dp),
        ) {
            if (model.lifts.isNotEmpty()) {
                item { SectionTitle("Lifts (${model.lifts.size})") }
                // Most recent on top so the latest set is visible without scrolling.
                items(model.lifts.asReversed()) { LiftCard(it) }
            }

            if (model.runs.isNotEmpty()) {
                item { SectionTitle("Runs (${model.runs.size})") }
                items(model.runs.asReversed()) { RunCard(it) }
            }
        }
    }
}

/** Profile, training-profile editor + the Appearance (theme / dynamic-accent) settings. */
@Composable
private fun ProfileDestination(
    ctx: Context,
    model: ViewModel,
    freshInstall: Boolean,
    onEvent: (Event) -> Unit,
) {
    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(Space.Screen.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Card.dp),
    ) {
        item {
            // Collapsed for a returning user (profile already set) so they land
            // on their data and guidance instead of scrolling past a full-screen
            // editor every launch. Expanded on a fresh install: the seeded
            // profile is a generic placeholder the user's guidance rides on, so
            // personalizing it is the intended first action, and also whenever a
            // returning user somehow has data but no profile of their own.
            ExpandableSection(
                "Profile",
                initiallyExpanded = freshInstall || model.profile == null,
            ) {
                val initialProfile = model.profile?.let { ProfileDraft.from(it) } ?: ProfileDraft.SEED
                ProfileEditor(initial = initialProfile) { draft ->
                    onEvent(draft.toEvent())
                }
            }
        }

        item {
            Column(verticalArrangement = Arrangement.spacedBy(Space.Card.dp)) {
                SectionTitle("Appearance")
                val currentTheme by ThemeSettings.theme.collectAsState()
                Text("Theme", color = OnBgBody, style = Type.Body)
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
                ) {
                    AppTheme.entries.forEach { t ->
                        if (t == currentTheme) {
                            Button(
                                onClick = { ThemeSettings.setTheme(ctx, t) },
                                modifier = Modifier.weight(1f),
                            ) {
                                Text(t.label)
                            }
                        } else {
                            OutlinedButton(
                                onClick = { ThemeSettings.setTheme(ctx, t) },
                                modifier = Modifier.weight(1f),
                            ) {
                                Text(t.label, color = OnBgBody)
                            }
                        }
                    }
                }
            }
        }

        // The Material You dynamic-accent Switch, relocated off the primary content
        // flow to the bottom of Profile under a divider (spec §3-Profile / roadmap
        // #5). Same state/callback wiring as before, only its placement moved.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            item {
                Column(verticalArrangement = Arrangement.spacedBy(Space.Card.dp)) {
                    HorizontalDivider()
                    val dynamicAccent by ThemeSettings.dynamicAccent.collectAsState()
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text("Use system accent color", color = OnBgBody, style = Type.Body)
                        Switch(
                            checked = dynamicAccent,
                            onCheckedChange = { ThemeSettings.setDynamicAccent(ctx, it) },
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun SectionTitle(text: String) {
    Text(
        text.uppercase(),
        color = Accent,
        style = Type.Section,
        modifier = Modifier.padding(top = Space.Sm.dp),
    )
}

/**
 * A [SectionTitle] whose body collapses. The four data-entry editors are long
 * and only one is used at a time, so the screen defaults every editor closed -
 * keeping the profile and any logged output within a short scroll. A trailing
 * +/− glyph signals the toggle; the whole header row is the tap target.
 */
@Composable
private fun ExpandableSection(
    title: String,
    initiallyExpanded: Boolean = false,
    content: @Composable () -> Unit,
) {
    // Saveable so an opened editor stays open when scrolled out of the LazyColumn
    // (item disposal) and across rotation, instead of silently collapsing.
    var expanded by rememberSaveable { mutableStateOf(initiallyExpanded) }
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable { expanded = !expanded }
            .padding(top = Space.Sm.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(title.uppercase(), color = Accent, style = Type.Section)
        Text(if (expanded) "−" else "+", color = Accent, style = Type.Section)
    }
    if (expanded) content()
}

/**
 * The irreversible-clear confirmation [AlertDialog], triggered from the
 * nav-structure overflow menus (History / Coach) so each destructive clear shares
 * the exact same guarded dialog + Event without duplicating its wiring.
 * The dialog is composed only while [visible]; [onDismiss] flips that state off,
 * [onClear] fires the destructive Event (and the caller closes the dialog).
 */
@Composable
private fun ClearConfirmDialog(
    visible: Boolean,
    message: String = "This permanently removes every logged entry in this list and can't be undone.",
    onDismiss: () -> Unit,
    onClear: () -> Unit,
) {
    if (visible) {
        AlertDialog(
            onDismissRequest = onDismiss,
            title = { Text("Clear all?") },
            text = { Text(message) },
            confirmButton = {
                TextButton(onClick = {
                    onDismiss()
                    onClear()
                }) { Text("Clear") }
            },
            dismissButton = {
                TextButton(onClick = onDismiss) { Text("Cancel") }
            },
        )
    }
}

/**
 * A reusable overflow (⋮) menu for a nav section's top row: an [IconButton] with
 * [Icons.Filled.MoreVert] opening a [DropdownMenu]. [items] are (label → action)
 * pairs; picking one closes the menu and runs its action. Purely relocates the
 * triggers that used to sit inline, the actions themselves (confirm dialogs +
 * Events) are unchanged and owned by the caller.
 */
@Composable
private fun OverflowMenu(items: List<Pair<String, () -> Unit>>) {
    if (items.isEmpty()) return
    var open by remember { mutableStateOf(false) }
    IconButton(onClick = { open = true }) {
        Icon(Icons.Filled.MoreVert, contentDescription = "More options", tint = Accent)
    }
    DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
        items.forEach { (label, action) ->
            DropdownMenuItem(
                text = { Text(label) },
                onClick = {
                    open = false
                    action()
                },
            )
        }
    }
}

/** A section top row: the section title left, an [OverflowMenu] pinned right. */
@Composable
private fun SectionBarWithOverflow(title: String, items: List<Pair<String, () -> Unit>>) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(top = Space.Sm.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(title.uppercase(), color = Accent, style = Type.Section)
        OverflowMenu(items)
    }
}

@Composable
private fun SafetyBanner(model: ViewModel, modifier: Modifier = Modifier) {
    val tier = model.safety_tier
    if (tier == null && !model.train_blocked) return
    val status = LocalStatusColors.current
    val bg = if (model.train_blocked) status.danger else status.warn
    Card(
        colors = CardDefaults.cardColors(containerColor = bg),
        elevation = CardDefaults.cardElevation(defaultElevation = 0.dp),
        shape = RoundedCornerShape(Space.Card.dp),
        modifier = modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(Space.Card.dp)) {
            Text(
                if (model.train_blocked) "DO NOT TRAIN" else "SAFETY CHECK",
                color = Color.White,
                style = Type.Title,
            )
            tier?.let {
                // DangerOn is the pink subtext tuned for the red danger surface;
                // on the amber warn surface (a non-blocking safety check) it reads
                // as an off-hue clash, so fall back to a dimmed white there.
                val subtext = if (model.train_blocked) DangerOn else Color.White.copy(alpha = 0.85f)
                Text("Triggered by: ${safetyTierLabel(it)}", color = subtext, style = Type.Body)
            }
        }
    }
}

/**
 * Human-readable label for the core's safety tier. The wire value is the raw
 * `SafetyTier` Debug name (app.rs formats it with `{t:?}`), so an unknown tier -
 * a future core variant the shell hasn't caught up to falls through to its raw
 * name rather than showing nothing. Display-only; nothing keys off this string.
 */
private fun safetyTierLabel(tier: String): String = when (tier) {
    "MedicalReferral" -> "Medical referral"
    "Pain" -> "Pain (red flag)"
    "Illness" -> "Illness"
    "ObjectivePerformance" -> "Objective performance drop"
    "SubjectiveMultiDay" -> "Subjective signals (multi-day)"
    "HrvTrend" -> "HRV trend"
    "SingleDayMarker" -> "Single-day marker"
    else -> tier
}

@Composable
private fun LiftCard(l: LiftResultView) {
    PlainCard {
        Text(l.exercise, style = Type.Title, color = Color.White)
        // Input as logged, then the core-derived metrics, each shown once. The
        // core's `summary` string folds both together (the web shell renders that);
        // here the structured fields let the card avoid repeating them.
        Text(
            "${trimNum(l.weight_kg)} kg × ${l.reps} @ RPE ${trimNum(l.rpe)}",
            color = OnBgBody,
            style = Type.Body.merge(TabularFigures),
        )
        Text(
            "e1RM ${trimNum(l.e1rm_kg)} kg · ~${trimNum(l.pct_1rm)}% 1RM · RIR ${trimNum(l.rir)}",
            color = OnBgMuted,
            style = Type.Body.merge(TabularFigures),
        )
    }
}

/** Drop a trailing `.0` (whole values), else keep one decimal (e.g. 2.5 RIR). */
private fun trimNum(d: Double): String =
    if (d % 1.0 == 0.0) "${d.toInt()}" else String.format(Locale.US, "%.1f", d)

// Mirrors `feedback::POSITIVE_SPLIT_FLAG_PCT` in the Rust core: the core's
// `positive_split_discipline` fires its coaching cue only for a split strictly
// beyond this percent, so the FADE / NEG SPLIT chips use the same bound -
// otherwise a run at exactly the threshold would show a chip with no matching
// coaching line. These two constants must stay in lockstep across the layers.
private const val POSITIVE_SPLIT_FLAG_PCT = 3.0

@Composable
@OptIn(ExperimentalLayoutApi::class)
private fun RunCard(r: RunResultView) {
    val status = LocalStatusColors.current
    PlainCard {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(r.zone, style = Type.Title, color = status.hrZoneColor(r.zone, Accent))
            Spacer(Modifier.width(Space.Md.dp))
            Text(r.pace, color = Color.White, style = Type.Body.merge(TabularFigures))
        }
        // Flags flow onto a second line when a narrow screen can't fit both a SPIKE
        // and a FADE chip beside each other: a plain Row would clip the trailing one.
        // NEG SPLIT (praise, UI-only) is the symmetric negative case.
        val split = r.split_pct
        val hasFlag = r.spike_flag ||
            (split != null && (split > POSITIVE_SPLIT_FLAG_PCT || split < -POSITIVE_SPLIT_FLAG_PCT))
        if (hasFlag) {
            FlowRow(horizontalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                if (r.spike_flag) Chip("SPIKE", status.danger)
                if (split != null && split > POSITIVE_SPLIT_FLAG_PCT) {
                    Chip("+${trimNum(split)}% FADE", status.warn)
                } else if (split != null && split < -POSITIVE_SPLIT_FLAG_PCT) {
                    Chip("NEG SPLIT", status.evidenceStrong)
                }
            }
        }
        if (r.summary.isNotBlank()) Text(r.summary, color = OnBgBody, style = Type.Body)
        if (r.citation.isNotBlank()) Text(r.citation, color = OnBgFaint, style = Type.Caption)
        if (r.gpx.isNotBlank()) {
            val ctx = LocalContext.current
            OutlinedButton(onClick = {
                shareGpx(ctx, r.gpx)
            }) { Text("Export GPX") }
        }
    }
}

@Composable
private fun FeedbackCard(f: FeedbackView) {
    EvidenceCard(f.message, f.grade, f.citation, f.confidence, f.safety_critical, f.contested, f.category)
}

/**
 * Two-tier evidence card (spec §5). Collapsed (default) always shows the
 * load-bearing scan signals: the summary, the grade chip, and any SAFETY /
 * CONTESTED chip. The reference detail, the citation string and the numeric
 * `conf 0.NN`, is disclosed on tap, keeping the honesty invariant intact
 * (grade + safety/contested are NEVER hidden, only citation + confidence).
 * Tapping anywhere on the card toggles [expanded]; a "why?" affordance hints
 * the extra detail is one tap away.
 */
@Composable
internal fun EvidenceCard(
    summary: String,
    grade: String,
    citation: String,
    confidence: Float,
    safetyCritical: Boolean,
    contested: Boolean,
    section: String? = null,
) {
    val status = LocalStatusColors.current
    var expanded by rememberSaveable { mutableStateOf(false) }
    Card(
        colors = CardDefaults.cardColors(containerColor = BgElevated),
        elevation = CardDefaults.cardElevation(defaultElevation = 0.dp),
        shape = RoundedCornerShape(Space.Card.dp),
        modifier = Modifier
            .fillMaxWidth()
            .clickable { expanded = !expanded },
    ) {
        Column(
            Modifier.padding(Space.Card.dp),
            verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                // Optional section label on the left, grade chip always pinned right via
                // the weighted spacer: otherwise a sectionless card (readiness/feedback)
                // would left-align its lone grade chip while a sectioned one right-aligns
                // it, so the chip would jump sides between card types.
                section?.let { Text(it, color = Accent, style = Type.Chip) }
                Spacer(Modifier.weight(1f))
                Chip(grade, status.gradeColor(grade))
            }
            Text(summary, color = Color.White, style = Type.Body)
            // ALWAYS-VISIBLE safety signals (honesty invariant): grade above,
            // SAFETY/CONTESTED here. Only citation + confidence hide behind the tap.
            Row(
                modifier = Modifier.padding(top = Space.Sm.dp),
                horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp + Space.Xs.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (safetyCritical) Chip("SAFETY", status.danger)
                if (contested) Chip("CONTESTED", status.warn)
                Spacer(Modifier.weight(1f))
                // The disclosure affordance: signals the reference detail is a tap away.
                Text(
                    if (expanded) "less" else "why?",
                    color = OnBgMuted,
                    style = Type.Caption,
                )
            }
            if (expanded) {
                Text(
                    "conf ${String.format(Locale.US, "%.2f", confidence)}",
                    color = OnBgMuted,
                    style = Type.Caption.merge(TabularFigures),
                    modifier = Modifier.padding(top = Space.Xs.dp),
                )
                Text(
                    if (citation.isNotBlank()) citation else "-",
                    color = OnBgFaint,
                    style = Type.Caption,
                )
            }
        }
    }
}

/**
 * Grade rank for the Coach priority sort (spec §5): higher = stronger evidence,
 * sorted first. Mirrors `EvidenceGrade` ordering in `schema.rs`
 * (Strong > Moderate > Weak > ExpertOpinion > MarketingMyth). The wire value is
 * the raw Debug name; an unknown/future grade falls to the bottom (-1) rather
 * than colliding with a real grade.
 */
private fun gradeRank(grade: String): Int = when (grade) {
    "Strong" -> 4
    "Moderate" -> 3
    "Weak" -> 2
    "ExpertOpinion" -> 1
    "MarketingMyth" -> 0
    else -> -1
}

@Composable
private fun PlainCard(content: @Composable () -> Unit) {
    Card(
        colors = CardDefaults.cardColors(containerColor = BgElevated),
        elevation = CardDefaults.cardElevation(defaultElevation = 0.dp),
        shape = RoundedCornerShape(Space.Card.dp),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(
            Modifier.padding(Space.Card.dp),
            verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
        ) { content() }
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
