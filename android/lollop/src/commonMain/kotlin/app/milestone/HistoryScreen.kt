package app.milestone

import androidx.compose.foundation.ScrollState
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
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
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.runtime.CompositionLocalProvider
import app.milestone.resources.Res
import app.milestone.resources.history_import_runs_long
import app.milestone.resources.history_last_7_days_empty
import app.milestone.resources.history_lifts_empty
import app.milestone.resources.history_log_set
import app.milestone.resources.history_runs_empty
import app.milestone.resources.history_stat_sessions
import app.milestone.resources.history_track_run
import app.milestone.resources.ic_ui_calendar
import app.milestone.resources.main_log_run_title
import app.milestone.resources.main_jump_to_month
import app.milestone.resources.main_overline_last_12_weeks
import app.milestone.resources.main_overline_last_7_days
import app.milestone.resources.main_segment_lifts
import app.milestone.resources.main_segment_runs
import app.milestone.resources.main_stat_running
import app.milestone.resources.main_stat_tonnage
import app.milestone.resources.main_unit_klb
import app.milestone.resources.main_unit_t
import app.milestone.resources.ui_cd_collapsed
import app.milestone.resources.ui_cd_expanded
import kotlin.time.Instant
import kotlinx.coroutines.launch
import kotlinx.datetime.TimeZone
import kotlinx.datetime.toLocalDateTime
import org.jetbrains.compose.resources.painterResource
import org.jetbrains.compose.resources.pluralStringResource
import org.jetbrains.compose.resources.stringResource

/** Which logged type History shows: two options, no "All". */
private enum class HistoryFilter { Lifts, Runs }

/** Precomputed History month scaffolding: [jumpTargets] maps each month
 *  label to its LazyColumn item index for the jump control; [headerBefore] maps a
 *  stream-entry index to the month header to emit before that card, so the stream
 *  loop never recomputes month strings per card. */
internal data class MonthNav(
    val jumpTargets: List<Pair<String, Int>>,
    val headerBefore: Map<Int, String>,
)

/** Local-month header label ("August 2026", in the locale's own order) for a history
 *  card's day stamp (a lift session's first set, a run's start: [runDaySec]),
 *  bucketed in the device zone (the shell owns the wall clock; the core stays
 *  clock-free). Blank for an undated entry so no header is emitted. */
internal fun monthLabel(epochSec: Long, zone: TimeZone, fmt: DayFormatter): String {
    if (epochSec <= 0L) return ""
    return fmt.format(Instant.fromEpochSeconds(epochSec).toLocalDateTime(zone).date)
}


/** The month headers and jump targets for a newest-first stream with one day stamp
 *  per card ([stamps]), after [base] non-month items. MIRRORS the stream's emission
 *  order (a header slot before each month's first card, then the card): a wrong
 *  offset would scroll to the wrong row. Pure. */
internal fun buildMonthNav(
    stamps: List<Long>,
    base: Int,
    zone: TimeZone,
    fmt: DayFormatter,
): MonthNav {
    val headerBefore = HashMap<Int, String>()
    val labels = ArrayList<Pair<String, Int>>()
    var streamOffset = 0
    var prevMonth: String? = null
    stamps.forEachIndexed { i, stamp ->
        val month = monthLabel(stamp, zone, fmt)
        if (month.isNotBlank() && month != prevMonth) {
            headerBefore[i] = month
            labels += month to streamOffset
            streamOffset++ // header slot
        }
        streamOffset++ // card slot
        prevMonth = month
    }
    val jumpTargets = if (labels.size < 2) {
        emptyList()
    } else {
        labels.map { (label, off) -> label to (base + off) }
    }
    return MonthNav(jumpTargets = jumpTargets, headerBefore = headerBefore)
}

/** History (03-history): 7-day strip → LAST 12 WEEKS heatmap → Lifts|Runs segment → (Runs: progress, records) → entry cards, or the segment's empty state. */
@Composable
internal fun HistoryDestination(
    model: ViewModel,
    onEvent: (Event) -> Unit = {},
    // Opens the run tracker; null where the platform records no runs (iOS for now):
    // the Runs empty state then offers "Log run" ([onLogRun]) instead.
    onTrackRun: (() -> Unit)? = {},
    onLogSet: () -> Unit = {},
    onLogRun: () -> Unit = {},
) {
    // The segment tracks whichever type was logged most recently (newest
    // observed_at), falling back to Lifts when empty. A factual shell-side
    // default from the ViewModel, no coaching logic. Once the user picks a
    // segment that pick wins and stays put, including across rotation
    // (rememberSaveable); until then a freshly logged run/lift moves the
    // segment to that type.
    val defaultFilter = remember(model.lifts, model.runs) {
        val lastLift = model.lifts.maxOfOrNull { it.observed_at } ?: Long.MIN_VALUE
        val lastRun = model.runs.maxOfOrNull { it.observed_at } ?: Long.MIN_VALUE
        if (lastRun > lastLift) HistoryFilter.Runs else HistoryFilter.Lifts
    }
    var manualFilter by rememberSaveable { mutableStateOf<HistoryFilter?>(null) }
    val filter = manualFilter ?: defaultFilter
    // Modality filtering: show the Lifts|Runs segment only when BOTH sides
    // are relevant: either the profile programs that modality, or the user has
    // logged that type. Otherwise render the single available list with no
    // segment, defaulting to whichever side has data / is programmed.
    val liftsPresent = model.lifts.isNotEmpty()
    val runsPresent = model.runs.isNotEmpty()
    val hasAny = liftsPresent || runsPresent
    val showLifting = model.showLifting()
    val showRunning = model.showRunning()
    // Weight display unit for every kg figure on this screen (lift faces, e1RM,
    // tonnage tile, the edit editor).
    val weightUnit = model.weightUnit()
    // Distance display unit (locale + Profile override) for the running tile and
    // the daily-distance chart; run cards resolve the same unit themselves.
    val distanceUnit = rememberDistanceUnit()
    // Re-emits once a minute so "2 hours ago" labels and the rolling-7-day window refresh.
    val nowMillis = rememberTicker(60_000L)
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
    // Tapping a card opens its detail/edit/delete sheet. Only a selection KEY
    // (kind + entry_id + observed_at) is saved, never the view itself (the view
    // classes are not Bundle-saveable). The selected entry is re-resolved from
    // the CURRENT model each recomposition, so a rotation-while-selected re-reads
    // fresh data; a delete-while-rotated resolves to null and the sheet closes.
    var selectedKey by rememberSaveable { mutableStateOf<String?>(null) }
    val selected: HistoryEntry? = remember(selectedKey, model.lifts, model.runs) {
        resolveHistoryEntry(selectedKey, model)
    }
    // Lifts render as SESSIONS (one card per local day), grouped once per model
    // change; local-day bucketing stays in the shell (device zone) so the core is
    // clock-free. The zone is re-read on every composition (the minute ticker
    // recomposes), so a time-zone change rebuckets everything together. The open
    // session sheet is keyed by its day (a saveable string).
    val zone = TimeZone.currentSystemDefault()
    val liftSessions = remember(model.lifts, zone) { groupLiftSessions(model.lifts, zone) }
    // Per-exercise best-e1RM-per-session series for the session sheet's trend lines.
    val e1rmSeries = remember(liftSessions) { bestE1rmByExerciseDay(liftSessions) }
    var selectedSessionKey by rememberSaveable { mutableStateOf<String?>(null) }
    val selectedSession = selectedSessionKey?.let { k -> liftSessions.firstOrNull { it.key == k } }
    // A session that no longer exists (its last set deleted) drops its stale key,
    // so a set logged later that day can't reopen the sheet on its own.
    SideEffect {
        if (selectedSessionKey != null && selectedSession == null) selectedSessionKey = null
    }
    // A run another screen asked to open (an import's "View"): open its sheet on the
    // Runs segment, then mark the request handled.
    val focus by HistoryFocus.pending.collectAsState()
    LaunchedEffect(focus, model.runs) {
        val (id, at) = focus ?: return@LaunchedEffect
        model.runs.firstOrNull { it.entry_id == id && it.observed_at == at }?.let { r ->
            manualFilter = HistoryFilter.Runs
            selectedKey = historyEntryKey(HistoryEntry.Run(r))
        }
        HistoryFocus.consume()
    }
    // The Runs empty state offers import as its second way in; otherwise import
    // lives in the Log chooser (Log → More → Import runs).
    val launchImport = rememberRunImport()
    val todayLocalDay = localEpochDay(nowMillis / 1000, zone)
    // On a two-pane window the list stands at the start and the overview (7-day
    // strip, heatmap, run progress, records) at the end; an opened entry takes the
    // end pane in place of the overview instead of a sheet ([DetailSheet]).
    val twoPane = LocalWindowLayout.current.twoPane && hasAny
    val detailDock = remember { SlotDock() }
    CompositionLocalProvider(LocalDetailDock provides detailDock.takeIf { twoPane }) {
    EntryActionSheets(
        selected = selected,
        editorInputs = remember(model) { SetEditorInputs.from(model) },
        weightUnit = weightUnit,
        hrMaxKnown = model.hr_max_known,
        nowMillis = nowMillis,
        onDismiss = { selectedKey = null },
        onEvent = onEvent,
        srpeLoad = model.srpe_load,
    )
    // The session sheet steps aside while one of its sets is open in the per-set
    // sheet above, and returns once that closes (edit saved, set deleted, dismissed).
    // Kept across the set-editor round trip (the sheet leaves composition while a set
    // is open), so it returns to the edited set; a different session starts at the top.
    val sessionScroll = rememberSaveable(selectedSessionKey, saver = ScrollState.Saver) { ScrollState(0) }
    if (selected == null && selectedSession != null) {
        LiftSessionSheet(
            session = selectedSession,
            model = model,
            e1rmSeries = e1rmSeries,
            weightUnit = weightUnit,
            nowMillis = nowMillis,
            scrollState = sessionScroll,
            onOpenSet = { s -> selectedKey = historyEntryKey(HistoryEntry.Lift(s)) },
            onDismiss = { selectedSessionKey = null },
            onEvent = onEvent,
        )
    }
    }
    // The list state is hoisted so the jump control can scroll it.
    val listState = rememberLazyListState()
    val scope = rememberCoroutineScope()
    // The core sends run progress only once a dated run has a distance; without it
    // both charts would be empty, so the empty state below carries the actions instead.
    val runProgress = model.run_progress?.takeIf { activeFilter == HistoryFilter.Runs }
    val showRunProgress = runProgress != null
    val runRecords = model.run_progress?.takeIf { it.longest_run_km > 0.0 || it.best_efforts.isNotEmpty() }
    val showRunRecords = activeFilter == HistoryFilter.Runs && runRecords != null
    val monthFmt = remember { localizedDayFormatter("yMMMM") }
    // The Runs stream, newest first by START (a run belongs to the day it started,
    // the day its title and date chip name), shared by the month headers and the cards.
    val runStream = remember(model.runs) { historyRunStream(model.runs) }
    val monthNav = remember(activeFilter, liftSessions, runStream, showSegment, showRunProgress, showRunRecords, zone, twoPane) {
        // One stamp per stream card: a lift SESSION (its first set) or a run's start.
        val stamps: List<Long> = when (activeFilter) {
            HistoryFilter.Runs -> runStream.map(::runDaySec)
            else -> liftSessions.map { it.firstObservedAt }
        }
        // Non-month items emitted before the entry stream: the 7-day strip and the
        // heatmap (whenever anything is logged), the segment row (when shown), the run
        // progress chart and the records grid (Runs). The jump control rides on the
        // first month header, so it adds no slot of its own.
        var base = 0
        if (hasAny && !twoPane) base += 2              // 7-day strip + 12-week heatmap
        if (showSegment) base++                        // Lifts | Runs segment
        if (showRunProgress && !twoPane) base++        // run progress chart
        if (showRunRecords && !twoPane) base++         // run records
        buildMonthNav(stamps, base, zone, monthFmt)
    }
    // The screen's parts, in one-column order: the overview's top (7-day strip,
    // heatmap), the segment, the run overview (progress, records), the entries.
    val overviewTop: LazyListScope.() -> Unit = {
        // 1. Stat strip over the rolling last 7 days (now − 7 days) of ALL activity,
        // so it sits above the Lifts | Runs filter. Nothing logged: only the empty line below.
        if (hasAny) {
            item {
                val nowSec = nowMillis / 1000
                val windowStart = nowSec - 7L * 86400L
                val weekLifts = model.lifts.filter { it.observed_at >= windowStart }
                val weekRuns = model.runs.filter { it.observed_at >= windowStart }
                // Count SESSIONS, not individual sets: a day of lifting is one
                // session (group sets by local calendar day) + each run is its own
                // session. `weekLifts` is a set list, so its raw size over-counts.
                val sessionCount = weekLifts.map { localEpochDay(it.observed_at, zone) }.toSet().size + weekRuns.size
                if (sessionCount == 0) {
                    // A quiet week is one line, never a row of zero tiles.
                    Text(stringResource(Res.string.history_last_7_days_empty), color = OnBgMuted, style = Type.Body)
                    return@item
                }
                // Tonnage honours the weight-unit choice: metric tonnes ("t") for kg,
                // thousands of pounds ("klb") for lb (a bare "lb" total runs to five/six
                // digits over a week, so the k-scale keeps the tile readable).
                val totalLiftKg = weekLifts.sumOf { it.tonnage_kg }
                val tonnageValue = if (weightUnit == WeightUnit.Lb) {
                    kgToLb(totalLiftKg) / 1000.0
                } else {
                    totalLiftKg / 1000.0
                }
                val tonnageUnit = if (weightUnit == WeightUnit.Lb) stringResource(Res.string.main_unit_klb) else stringResource(Res.string.main_unit_t)
                // Running distance in the user's distance unit (km or mi).
                val runningKm = weekRuns.sumOf { it.distance_km }
                val runningDistance = metersToDisplay(runningKm * 1000.0, distanceUnit)
                // A modality's tile shows when it is programmed or has logged data
                // (logged data is never hidden); a zero reads "—", never "0.0".
                Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                    SectionOverline(stringResource(Res.string.main_overline_last_7_days))
                    // Equal-height tiles: a glossary "?" in one label must not make that
                    // tile taller than its neighbours.
                    Row(
                        modifier = Modifier.height(IntrinsicSize.Min),
                        horizontalArrangement = Arrangement.spacedBy(Space.Md.dp + Space.Xs.dp),
                    ) {
                        StatTile("$sessionCount", null, pluralStringResource(Res.plurals.history_stat_sessions, sessionCount))
                        if (showLifting || liftsPresent) {
                            if (totalLiftKg > 0.0) {
                                StatTile(formatFixed(tonnageValue, 1), tonnageUnit, stringResource(Res.string.main_stat_tonnage), glossaryKey = "tonnage")
                            } else {
                                StatTile(EmptyValuePlaceholder, null, stringResource(Res.string.main_stat_tonnage), glossaryKey = "tonnage")
                            }
                        }
                        if (showRunning || runsPresent) {
                            if (runningKm > 0.0) {
                                StatTile(formatFixed(runningDistance, 1), distanceUnit.distanceLabel, stringResource(Res.string.main_stat_running))
                            } else {
                                StatTile(EmptyValuePlaceholder, null, stringResource(Res.string.main_stat_running))
                            }
                        }
                    }
                }
            }
            // 2. Heatmap, LAST 12 WEEKS, tinted by discipline (lift / run / both),
            // weeks starting Monday. Local-day bucketing happens here in the shell
            // (device timezone) so the core stays clock-free. A logged day opens what
            // was done that day: its lift session, else its latest run.
            item {
                Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                    SectionOverline(stringResource(Res.string.main_overline_last_12_weeks))
                    val days = remember(model.lifts, model.runs, zone) {
                        historyActivityDays(model.lifts, model.runs, zone)
                    }
                    ContributionHeatmap(days, todayLocalDay, weeks = 12) { day ->
                        val session = liftSessions.firstOrNull { it.epochDay == day }
                        val run = latestRunOnDay(model.runs, day, zone)
                        if (session != null && (run == null || activeFilter == HistoryFilter.Lifts)) {
                            manualFilter = HistoryFilter.Lifts
                            selectedSessionKey = session.key
                        } else if (run != null) {
                            manualFilter = HistoryFilter.Runs
                            selectedKey = historyEntryKey(HistoryEntry.Run(run))
                        }
                    }
                }
            }
        }
    }
    val segmentItem: LazyListScope.() -> Unit = {
        // 3. Filter: two-option segmented Lifts | Runs, no "All", no counts.
        // Hidden when only one modality is relevant: the single available
        // list renders on its own.
        if (showSegment) {
            item {
                TwoSegmentRow(
                    stringResource(Res.string.main_segment_lifts), stringResource(Res.string.main_segment_runs),
                    selectedIndex = if (filter == HistoryFilter.Lifts) 0 else 1,
                ) { manualFilter = if (it == 0) HistoryFilter.Lifts else HistoryFilter.Runs }
            }
        }
    }
    val runOverview: LazyListScope.() -> Unit = {
        // 2b. Runs: progress at the top (visualizations first). ONE compact distance
        // chart (16-week weekly vs the plan, or the last 14 days), then the records
        // grid (longest run + best efforts); a record opens its run.
        if (runProgress != null) {
            item(key = "run-progress") {
                // Garmin-style per-day distance, a factual aggregation (sum of
                // distance_km per local day the run started) over the same local-day
                // bucketing as the heatmap, shown in the user's distance unit.
                val kmByDay = remember(model.runs, zone) { runKmByDay(model.runs, zone) }
                RunProgressSection(
                    progress = runProgress,
                    kmByDay = kmByDay,
                    todayLocalDay = todayLocalDay,
                    unit = distanceUnit,
                )
            }
        }
        if (showRunRecords) {
            item(key = "run-records") {
                RunRecordsSection(
                    progress = runRecords,
                    runs = model.runs,
                    unit = distanceUnit,
                    nowMillis = nowMillis,
                    onOpenRun = { selectedKey = historyEntryKey(HistoryEntry.Run(it)) },
                )
            }
        }
    }
    val entryItems: LazyListScope.() -> Unit = {
        // Jump to month rides on the FIRST month header (a calendar icon button that
        // opens a whole-row option list, never an anchored dropdown) when the active
        // stream spans >=2 months; a pick animate-scrolls to that month's header (item
        // index precomputed in `monthNav`).
        val jumpMonths = monthNav.jumpTargets.map { it.first }.takeIf { it.isNotEmpty() }
        val onJump: (String) -> Unit = { label ->
            monthNav.jumpTargets.firstOrNull { it.first == label }?.let { target ->
                scope.launch { listState.animateScrollToItem(target.second) }
            }
        }

        // 4. Entry cards, filtered to the active segment, most recent on top.
        if (activeFilter == HistoryFilter.Lifts && liftSessions.isNotEmpty()) {
            // One card per SESSION (local day), not per set: a 20-set day is one
            // card. Keyed by the day (stable across inserts, unique per stream). A
            // local-month overline header ("August 2026") precedes each month's
            // first session (newest-first stream), grouping the unbounded list.
            liftSessions.forEachIndexed { i, s ->
                monthNav.headerBefore[i]?.let { m ->
                    item(key = "lift-month-$m") { MonthHeader(m, jumpMonths.takeIf { i == 0 }, onJump) }
                }
                item(key = "lift-session-${s.key}") {
                    LiftSessionCard(
                        session = s,
                        nowMillis = nowMillis,
                        todayLocalDay = todayLocalDay,
                        weightUnit = weightUnit,
                        // Beside the detail pane the open session's card is marked.
                        selected = twoPane && s.key == selectedSessionKey,
                        onClick = { selectedSessionKey = s.key },
                    )
                }
            }
        } else if (activeFilter == HistoryFilter.Lifts) {
            item(key = "lifts-empty") {
                HistoryEmptyState(
                    text = stringResource(Res.string.history_lifts_empty),
                    primaryLabel = stringResource(Res.string.history_log_set),
                    onPrimary = onLogSet,
                )
            }
        }

        if (activeFilter == HistoryFilter.Runs && model.runs.isNotEmpty()) {
            // Same month-header grouping as the Lifts segment, by the day each run started.
            // Runs an old build logged without an id in one second share their id;
            // the occurrence keeps their keys apart.
            val seen = HashMap<String, Int>()
            runStream.forEachIndexed { i, r ->
                monthNav.headerBefore[i]?.let { m ->
                    item(key = "run-month-$m") { MonthHeader(m, jumpMonths.takeIf { i == 0 }, onJump) }
                }
                val id = "${r.entry_id}|${r.observed_at}"
                val n = (seen[id] ?: 0).also { seen[id] = it + 1 }
                item(key = "$id#$n") {
                    val key = historyEntryKey(HistoryEntry.Run(r))
                    RunCard(r, nowMillis, selected = twoPane && key == selectedKey, onClick = { selectedKey = key })
                }
            }
        } else if (activeFilter == HistoryFilter.Runs) {
            item(key = "runs-empty") {
                HistoryEmptyState(
                    text = stringResource(Res.string.history_runs_empty),
                    primaryLabel = stringResource(if (onTrackRun != null) Res.string.history_track_run else Res.string.main_log_run_title),
                    onPrimary = onTrackRun ?: onLogRun,
                    secondaryLabel = stringResource(Res.string.history_import_runs_long),
                    onSecondary = launchImport,
                )
            }
        }
    }
    val spacing = Arrangement.spacedBy(Space.Card.dp)
    if (twoPane) {
        val overviewState = rememberLazyListState()
        TwoPaneRow(
            first = {
                LazyColumn(
                    state = listState,
                    modifier = Modifier.weight(1f).fillMaxSize().testTag("history-list-pane"),
                    contentPadding = PaddingValues(start = Space.Screen.dp, end = Space.Md.dp, top = Space.Screen.dp, bottom = Space.Screen.dp),
                    verticalArrangement = spacing,
                ) {
                    segmentItem()
                    entryItems()
                }
            },
            second = {
                Box(Modifier.weight(1f).fillMaxSize().padding(start = Space.Md.dp, end = Space.Screen.dp, top = Space.Screen.dp)) {
                    val detail = detailDock.content
                    if (detail != null) {
                        detail()
                    } else {
                        LazyColumn(
                            state = overviewState,
                            modifier = Modifier.fillMaxSize().testTag("history-overview-pane"),
                            contentPadding = PaddingValues(bottom = Space.Screen.dp),
                            verticalArrangement = spacing,
                        ) {
                            overviewTop()
                            runOverview()
                        }
                    }
                }
            },
        )
    } else {
        ReadableWidth { gutter ->
            LazyColumn(
                state = listState,
                modifier = Modifier.fillMaxSize(),
                contentPadding = PaddingValues(horizontal = gutter, vertical = Space.Screen.dp),
                verticalArrangement = spacing,
            ) {
                overviewTop()
                segmentItem()
                runOverview()
                entryItems()
            }
        }
    }
}

/**
 * A month header in the History stream ("SEPTEMBER 2026", accent section overline).
 * The first one also carries the jump-to-month control when [jumpMonths] is given:
 * a trailing 48dp calendar icon button ("Jump to month") expands a full-width
 * [OptionList] of every month the active stream spans; picking one collapses it and
 * [onJump] animate-scrolls the list to that month's header. Never an anchored
 * dropdown, and the month is never echoed in a separate row above its header.
 */
@Composable
private fun MonthHeader(month: String, jumpMonths: List<String>?, onJump: (String) -> Unit) {
    if (jumpMonths == null) {
        SectionOverline(month)
        return
    }
    var expanded by rememberSaveable { mutableStateOf(false) }
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth().heightIn(min = ActionMinHeight),
            horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(Modifier.weight(1f)) { SectionOverline(month) }
            val expandedState = stringResource(if (expanded) Res.string.ui_cd_expanded else Res.string.ui_cd_collapsed)
            // End-aligned: the glyph (not its touch padding) meets the content edge.
            IconButton(
                onClick = { expanded = !expanded },
                modifier = Modifier
                    .offset(x = (ActionMinHeight - IconSize) / 2)
                    .size(ActionMinHeight)
                    .testTag("history-jump-to-month")
                    .semantics { stateDescription = expandedState },
            ) {
                Icon(
                    painterResource(Res.drawable.ic_ui_calendar),
                    contentDescription = stringResource(Res.string.main_jump_to_month),
                    tint = OnBgMuted,
                    modifier = Modifier.size(IconSize),
                )
            }
        }
        if (expanded) {
            OptionList(
                options = jumpMonths,
                current = month,
                display = { it },
            ) {
                onJump(it)
                expanded = false
            }
        }
    }
}

/** The jump-to-month glyph size (VDS icon size). */
private val IconSize = 24.dp


/** The saveable selection key of a tapped entry, "<kind><entry_id>|<observed_at>";
 *  [resolveHistoryEntry] turns it back into the live entry (null once deleted). */
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
    // Match on entry_id AND observed_at, the pair the list keys carry.
    return when (kind) {
        'L' -> model.lifts.firstOrNull { it.entry_id == id && it.observed_at == obs }
            ?.let { HistoryEntry.Lift(it) }
        'R' -> model.runs.firstOrNull { it.entry_id == id && it.observed_at == obs }
            ?.let { HistoryEntry.Run(it) }
        else -> null
    }
}

/** A [StatTile]'s value: 19sp ExtraBold, tabular. */
internal val StatValueStyle = androidx.compose.ui.text.TextStyle(
    fontSize = 19.sp, fontWeight = androidx.compose.ui.text.font.FontWeight.ExtraBold,
    fontFeatureSettings = "tnum", textDirection = AppTextDirection,
)

/**
 * One compact stat cell for a horizontal summary strip (History week header),
 * using the app-wide tile anatomy: big tabular value (with optional small unit)
 * over its label. A [RowScope] extension so callers place several across a row
 * with `weight`. A [value] of [EmptyValuePlaceholder] ("nothing logged") renders
 * as the faint dash with no unit, never a "0.0 t" that reads as a measurement.
 */
@Composable
internal fun RowScope.StatTile(value: String, unit: String?, label: String, glossaryKey: String? = null) {
    // History week-strip anatomy (spec 03-history §1): value ABOVE (19sp
    // ExtraBold, tabular) with the unit inline muted, label below-left.
    // fillMaxHeight: in a `height(IntrinsicSize.Min)` row every tile takes the
    // tallest tile's height, so a glossary ⓘ never leaves a ragged bottom edge.
    // A traversal group: TalkBack reads each tile's value and label together, tile
    // by tile, even when a large font wraps one label to more lines than another.
    Column(
        modifier = Modifier
            .weight(1f)
            .fillMaxHeight()
            .traversalGroup()
            .clip(RoundedCornerShape(12.dp))
            .background(BgElevated)
            .padding(horizontal = Space.Card.dp, vertical = Space.Md.dp + Space.Xs.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Xs.dp),
    ) {
        val empty = value == EmptyValuePlaceholder
        Row(verticalAlignment = Alignment.Bottom) {
            Text(
                value,
                color = if (empty) OnBgFaint else OnBgBody,
                style = StatValueStyle,
                maxLines = 1,
            )
            if (unit != null && !empty) {
                Text(" $unit", color = OnBgFaint, style = Type.Caption.merge(TabularFigures))
            }
        }
        // Label, with an optional glossary affordance for jargon tiles.
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
            Text(label, color = OnBgFaint, style = Type.Caption)
            glossaryKey?.let { GlossaryInfo(it) }
        }
    }
}
