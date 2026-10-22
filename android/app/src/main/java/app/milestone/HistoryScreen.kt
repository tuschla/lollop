package app.milestone

import androidx.compose.foundation.ScrollState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import java.util.Locale
import kotlinx.coroutines.launch

/** Which logged type History shows (INVARIANT 4: two options, no "All"). */
private enum class HistoryFilter { Lifts, Runs }

/** Precomputed History month scaffolding: [jumpTargets] maps each month
 *  label to its LazyColumn item index for the jump control; [headerBefore] maps a
 *  reversed-entry index to the month header to emit before that card, so the stream
 *  loop never recomputes month strings per card. */
private data class MonthNav(
    val jumpTargets: List<Pair<String, Int>>,
    val headerBefore: Map<Int, String>,
)

/** Local-month header label ("August 2026", in the locale's own order) for a history
 *  entry's `observed_at`, bucketed in the device zone (the shell owns the wall clock;
 *  the core stays clock-free). Blank for an undated entry so no header is emitted. */
private fun monthLabel(epochSec: Long, zone: java.time.ZoneId, fmt: java.time.format.DateTimeFormatter): String {
    if (epochSec <= 0L) return ""
    return java.time.Instant.ofEpochSecond(epochSec).atZone(zone).toLocalDate().format(fmt)
}

/** ConsistencyView.weeks holds at most this many finished plan weeks (the core's
 *  `build_consistency`); fewer means the oldest one is the plan's first week. */
private const val CONSISTENCY_WEEKS_MAX = 8

/** History (03-history): 7-day strip → LAST 12 WEEKS heatmap → Lifts|Runs segment → (Runs: progress, records) → entry cards, or the segment's empty state. */
@Composable
internal fun HistoryDestination(
    model: ViewModel,
    onEvent: (Event) -> Unit = {},
    onTrackRun: () -> Unit = {},
    onLogSet: () -> Unit = {},
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
    val hasAny = model.lifts.isNotEmpty() || model.runs.isNotEmpty()
    // Modality filtering: show the Lifts|Runs segment only when BOTH sides
    // are relevant: either the profile programs that modality, or the user has
    // logged that type. Otherwise render the single available list with no
    // segment, defaulting to whichever side has data / is programmed.
    val liftsPresent = model.lifts.isNotEmpty()
    val runsPresent = model.runs.isNotEmpty()
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
    // clock-free. The open session sheet is keyed by its day (a saveable string).
    val zone = remember { java.time.ZoneId.systemDefault() }
    val liftSessions = remember(model.lifts) { groupLiftSessions(model.lifts, zone) }
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
    val ctx = LocalContext.current
    // The Runs empty state offers import as its second way in; otherwise import
    // lives in the Log chooser (Log → More → Import runs).
    val importRuns = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.GetMultipleContents(),
    ) { uris -> if (uris.isNotEmpty()) RunImport.importRuns(ctx, uris) }
    val launchImport = { importRuns.launch("*/*") }
    val todayLocalDay = localEpochDay(nowMillis / 1000, zone)
    // The plan's first week, so the weekly chart's plan line never covers the weeks
    // before the plan existed. When the core reports a full window of finished plan
    // weeks the start is older than it says, so the line spans the whole chart.
    val planStartEpochDay = model.consistency?.let { c ->
        if (c.weeks.size >= CONSISTENCY_WEEKS_MAX) {
            null
        } else {
            c.weeks.firstOrNull()?.week_monday_epoch_day ?: (todayLocalDay - Math.floorMod(todayLocalDay + 3, 7L))
        }
    }
    EntryActionSheets(
        selected = selected,
        // Memoized on model.lifts so the full-history scan isn't recomputed on
        // every recomposition.
        recentExercises = remember(model.lifts) {
            model.lifts.asReversed().map { it.exercise }.distinct()
        },
        holdExercises = remember(model.next_session, model.week_plan) {
            (model.next_session?.items.orEmpty() + model.week_plan.flatMap { it.items })
                .filter { it.timed_hold }
                .map { it.exercise }
                .toSet()
        },
        weightUnit = weightUnit,
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
    // Jump-to-month scaffolding: the list state is hoisted so the control below can
    // animate-scroll it, and `monthNav` maps each month label -> its LazyColumn item
    // index by MIRRORING the emission order (prefix of non-month items + a running
    // header/entry tally). A wrong offset would scroll to the wrong row.
    val listState = rememberLazyListState()
    val scope = rememberCoroutineScope()
    // No runs → no progress chart (an empty chart only reads as noise); the empty
    // state below carries the actions instead.
    val showRunProgress = activeFilter == HistoryFilter.Runs && runsPresent
    val runRecords = model.run_progress?.takeIf { it.longest_run_km > 0.0 || it.best_efforts.isNotEmpty() }
    val showRunRecords = activeFilter == HistoryFilter.Runs && runRecords != null
    val monthFmt = remember { localizedDayFormatter("yMMMM") }
    val monthNav = remember(activeFilter, liftSessions, model.runs, showSegment, showRunProgress, showRunRecords) {
        // One stamp per stream card: a lift SESSION (its first set) or a run.
        val stamps: List<Long> = when (activeFilter) {
            HistoryFilter.Runs -> model.runs.asReversed().map { it.observed_at }
            else -> liftSessions.map { it.firstObservedAt }
        }
        // Non-month items emitted before the entry stream: the 7-day strip and the
        // heatmap (whenever anything is logged), the segment row (when shown), the run
        // progress chart and the records grid (Runs). The jump control rides on the
        // first month header, so it adds no slot of its own.
        var base = 0
        if (hasAny) base += 2                          // 7-day strip + 12-week heatmap
        if (showSegment) base++                        // Lifts | Runs segment
        if (showRunProgress) base++                    // run progress chart
        if (showRunRecords) base++                     // run records
        // Precompute BOTH the header label to emit before each entry index (so the
        // stream loop never recomputes month strings per card) and the jump targets
        // (label -> LazyColumn item index), mirroring the stream's emission order.
        val headerBefore = HashMap<Int, String>()
        val labels = ArrayList<Pair<String, Int>>()
        var streamOffset = 0
        var prevMonth: String? = null
        stamps.forEachIndexed { i, stamp ->
            val month = monthLabel(stamp, zone, monthFmt)
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
        MonthNav(jumpTargets = jumpTargets, headerBefore = headerBefore)
    }
    LazyColumn(
        state = listState,
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(Space.Screen.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        // 1. Rolling-7-day stat strip: factual aggregates of the LAST 7 DAYS.
        // The strip is a "how's it going" glance (the historical views, 12-week
        // heatmap + full list: sit below); an all-time tonnage only ever grows
        // and stops meaning anything at a glance. A calendar-week window would read
        // 0 all week for a weekend runner whose runs land in the prior locale week;
        // a ROLLING 7 days (now − 7 days) always reflects recent work. Reading a
        // clock in the shell is fine: the determinism HARD RULE governs `shared/`,
        // not Kotlin (the heatmap below already reads the wall clock for "today").
        // The strip and the heatmap cover
        // ALL activity, so both sit above the Lifts | Runs filter. With nothing
        // logged at all, the empty line at the bottom stands alone.
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
                    Text(stringResource(R.string.history_last_7_days_empty), color = OnBgMuted, style = Type.Body)
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
                val tonnageUnit = if (weightUnit == WeightUnit.Lb) stringResource(R.string.main_unit_klb) else stringResource(R.string.main_unit_t)
                // Running distance in the user's distance unit (km or mi).
                val runningKm = weekRuns.sumOf { it.distance_km }
                val runningDistance = metersToDisplay(runningKm * 1000.0, distanceUnit)
                // A modality's tile shows when it is programmed or has logged data
                // (logged data is never hidden); a zero reads "—", never "0.0".
                Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                    SectionOverline(stringResource(R.string.main_overline_last_7_days))
                    // Equal-height tiles: a glossary "?" in one label must not make that
                    // tile taller than its neighbours.
                    Row(
                        modifier = Modifier.height(IntrinsicSize.Min),
                        horizontalArrangement = Arrangement.spacedBy(10.dp),
                    ) {
                        StatTile("$sessionCount", null, pluralStringResource(R.plurals.history_stat_sessions, sessionCount))
                        if (showLifting || liftsPresent) {
                            if (totalLiftKg > 0.0) {
                                StatTile(String.format(Locale.US, "%.1f", tonnageValue), tonnageUnit, stringResource(R.string.main_stat_tonnage), glossaryKey = "tonnage")
                            } else {
                                StatTile(EmptyValuePlaceholder, null, stringResource(R.string.main_stat_tonnage), glossaryKey = "tonnage")
                            }
                        }
                        if (showRunning || runsPresent) {
                            if (runningKm > 0.0) {
                                StatTile(String.format(Locale.US, "%.1f", runningDistance), distanceUnit.distanceLabel, stringResource(R.string.main_stat_running))
                            } else {
                                StatTile(EmptyValuePlaceholder, null, stringResource(R.string.main_stat_running))
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
                    SectionOverline(stringResource(R.string.main_overline_last_12_weeks))
                    val days = remember(model.lifts, model.runs) {
                        historyActivityDays(model.lifts, model.runs, zone)
                    }
                    ContributionHeatmap(days, todayLocalDay, weeks = 12) { day ->
                        val session = liftSessions.firstOrNull { it.epochDay == day }
                        val run = model.runs.lastOrNull { it.observed_at > 0L && localEpochDay(it.observed_at, zone) == day }
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
        // 3. Filter: two-option segmented Lifts | Runs, no "All", no counts.
        // Hidden when only one modality is relevant: the single available
        // list renders on its own.
        if (showSegment) {
            item {
                TwoSegmentRow(
                    stringResource(R.string.main_segment_lifts), stringResource(R.string.main_segment_runs),
                    selectedIndex = if (filter == HistoryFilter.Lifts) 0 else 1,
                ) { manualFilter = if (it == 0) HistoryFilter.Lifts else HistoryFilter.Runs }
            }
        }
        // 2b. Runs: progress at the top (visualizations first). ONE compact distance
        // chart (16-week weekly vs the plan, or the last 14 days), then the records
        // grid (longest run + best efforts); a record opens its run.
        if (showRunProgress) {
            item(key = "run-progress") {
                // Garmin-style per-day distance, a factual aggregation (sum of
                // distance_km per local day) over the same local-day bucketing as the
                // heatmap, shown in the user's distance unit.
                val kmByDay = remember(model.runs) {
                    val m = HashMap<Long, Double>()
                    model.runs
                        .filter { it.observed_at > 0 && it.distance_km > 0.0 }
                        .forEach { r ->
                            val day = localEpochDay(r.observed_at, zone)
                            m[day] = (m[day] ?: 0.0) + r.distance_km
                        }
                    m
                }
                RunProgressSection(
                    progress = model.run_progress,
                    kmByDay = kmByDay,
                    todayLocalDay = todayLocalDay,
                    unit = distanceUnit,
                    planStartEpochDay = planStartEpochDay,
                )
            }
        }
        if (showRunRecords) {
            item(key = "run-records") {
                RunRecordsSection(
                    progress = runRecords,
                    unit = distanceUnit,
                    nowMillis = nowMillis,
                    onOpenRun = { entryId, observedAt ->
                        model.runs.firstOrNull { (entryId == null || it.entry_id == entryId) && it.observed_at == observedAt }
                            ?.let { selectedKey = historyEntryKey(HistoryEntry.Run(it)) }
                    },
                )
            }
        }

        // Jump to month rides on the FIRST month header (a calendar icon button that
        // opens a whole-row option list, never an anchored dropdown) when the active
        // stream spans >=2 months; a pick animate-scrolls to that month's header (item
        // index precomputed in `monthNav`).
        val jumpMonths = monthNav.jumpTargets.map { it.first }.takeIf { it.size >= 2 }
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
                        weightUnit = weightUnit,
                        onClick = { selectedSessionKey = s.key },
                    )
                }
            }
        } else if (activeFilter == HistoryFilter.Lifts) {
            item(key = "lifts-empty") {
                HistoryEmptyState(
                    text = stringResource(R.string.history_lifts_empty),
                    primaryLabel = stringResource(R.string.history_log_set),
                    onPrimary = onLogSet,
                )
            }
        }

        if (activeFilter == HistoryFilter.Runs && model.runs.isNotEmpty()) {
            // Same month-header grouping as the Lifts segment.
            val reversed = model.runs.asReversed()
            reversed.forEachIndexed { i, r ->
                monthNav.headerBefore[i]?.let { m ->
                    item(key = "run-month-$m") { MonthHeader(m, jumpMonths.takeIf { i == 0 }, onJump) }
                }
                // Legacy rows share entry_id 0; suffix the index so two in the same
                // second can't collide.
                item(key = if (r.entry_id != 0L) "${r.entry_id}|${r.observed_at}" else "legacy-run-$i|${r.observed_at}") {
                    RunCard(r, nowMillis, onClick = { selectedKey = historyEntryKey(HistoryEntry.Run(r)) })
                }
            }
        } else if (activeFilter == HistoryFilter.Runs) {
            item(key = "runs-empty") {
                HistoryEmptyState(
                    text = stringResource(R.string.history_runs_empty),
                    primaryLabel = stringResource(R.string.history_track_run),
                    onPrimary = onTrackRun,
                    secondaryLabel = stringResource(R.string.history_import_runs_long),
                    onSecondary = launchImport,
                )
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
            val expandedState = stringResource(if (expanded) R.string.ui_cd_expanded else R.string.ui_cd_collapsed)
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
                    painterResource(R.drawable.ic_ui_calendar),
                    contentDescription = stringResource(R.string.main_jump_to_month),
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

/** A tapped History/Today entry, routed to the detail sheet. */
internal sealed interface HistoryEntry {
    data class Lift(val v: LiftResultView) : HistoryEntry
    data class Run(val v: RunResultView) : HistoryEntry
}

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
    // Match on entry_id AND observed_at so legacy rows (entry_id 0) can't collide.
    return when (kind) {
        'L' -> model.lifts.firstOrNull { it.entry_id == id && it.observed_at == obs }
            ?.let { HistoryEntry.Lift(it) }
        'R' -> model.runs.firstOrNull { it.entry_id == id && it.observed_at == obs }
            ?.let { HistoryEntry.Run(it) }
        else -> null
    }
}
