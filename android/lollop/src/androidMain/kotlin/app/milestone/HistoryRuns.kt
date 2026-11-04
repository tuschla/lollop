package app.milestone

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.unit.dp
import java.time.Instant
import java.time.ZoneId

// ── Run cards (History "Runs" stream) ───────────────────────────────────────

/**
 * The run's title: the user's own run-type tag when set ("Tempo run"), so the title
 * always matches the RUN TYPE chip in its detail; else the local weekday it started
 * ("Tuesday run"), so an untagged list doesn't repeat one name; else "Run" for an
 * undated row. User data and a clock reading only: no coaching judgment.
 */
@Composable
internal fun runTitle(r: RunResultView): String {
    val tagged = when (WorkoutType.fromWire(r.workout_type)) {
        WorkoutType.Steady -> R.string.history_run_steady
        WorkoutType.Interval -> R.string.history_run_interval
        WorkoutType.Tempo -> R.string.history_run_tempo
        WorkoutType.LongRun -> R.string.history_run_long
        WorkoutType.Recovery -> R.string.history_run_recovery
        WorkoutType.Easy -> R.string.history_run_easy
        null -> null
    }
    if (tagged != null) return stringResource(tagged)
    if (r.started_at <= 0L) return stringResource(R.string.main_run)
    val weekday = Instant.ofEpochSecond(r.started_at).atZone(ZoneId.systemDefault()).dayOfWeek
        .getDisplayName(java.time.format.TextStyle.FULL, appLocale(LocalConfiguration.current.locales))
    return stringResource(R.string.history_run_weekday, weekday)
}

/**
 * The date chip beside a run's title: "Today", "Yesterday", "3 days ago", then the
 * absolute date, on every run card alike (an imported or untagged run included), so
 * the list reads at one rhythm. Dated by the day the run STARTED, the day
 * [runTitle] names, so a run across midnight never reads "Sunday run · <a Monday>".
 */
internal fun runDateChip(r: RunResultView, nowMillis: Long): String = formatLogDate(runDaySec(r), nowMillis)

/** The stamp a run is dated by everywhere the shell names or groups its day (title,
 *  date chip, History's month headers and newest-first order, heatmap, daily
 *  distance, records, coach summary): its start, or its logged stamp for a row
 *  without one. One rule: a run belongs to the local day it STARTED, so a run from
 *  Sun 23:40 to Mon 00:20 is Sunday's everywhere. 0 = undated. */
internal fun runDaySec(r: RunResultView): Long = r.started_at.takeIf { it > 0L } ?: r.observed_at

/** The local calendar day a run belongs to ([runDaySec] in [zone]); null when undated. */
internal fun runLocalDay(r: RunResultView, zone: ZoneId): Long? =
    runDaySec(r).takeIf { it > 0L }?.let { localEpochDay(it, zone) }

/** History's run stream, newest first by [runDaySec] (the core lists runs by their
 *  logged stamp): the month headers bucket by start, so the stream must be in start
 *  order or two overlapping runs could split a month and repeat its header key.
 *  Stable, so equal starts keep the core's newest-first order; undated rows last. */
internal fun historyRunStream(runs: List<RunResultView>): List<RunResultView> =
    runs.asReversed().sortedByDescending { runDaySec(it) }

/** The latest-logged run that started on local [day] (a heatmap cell opens it), or null. */
internal fun latestRunOnDay(runs: List<RunResultView>, day: Long, zone: ZoneId): RunResultView? =
    runs.lastOrNull { runLocalDay(it, zone) == day }

/** Total distance per local day the runs started on (undated and zero-distance rows
 *  left out): History's daily distance chart. */
internal fun runKmByDay(runs: List<RunResultView>, zone: ZoneId): Map<Long, Double> {
    val m = HashMap<Long, Double>()
    for (r in runs) {
        if (r.distance_km <= 0.0) continue
        val day = runLocalDay(r, zone) ?: continue
        m[day] = (m[day] ?: 0.0) + r.distance_km
    }
    return m
}

/** The run a record cell names (the core keys a record by the run's entry_id, null
 *  for the longest run, and `observed_at`), or null if it is no longer listed. */
internal fun recordRun(runs: List<RunResultView>, entryId: Long?, observedAt: Long): RunResultView? =
    runs.firstOrNull { (entryId == null || it.entry_id == entryId) && it.observed_at == observedAt }

/** The run's face line in the display unit: "8.05 km · 5:34 /km · 44:12". The time is
 *  the same clock Today and the Finish review show for a run. */
@Composable
internal fun runFaceLine(r: RunResultView, unit: DistanceUnit): String {
    val base = "${runDistLabel(r.distance_km, unit)} · ${runPaceLabel(r.distance_km, r.duration_min, unit)}"
    return if (r.duration_min > 0.0) "$base · ${formatElapsed(Math.round(r.duration_min * 60.0))}" else base
}

// ── Records, run rating and run progress ────────────────────────────────────

/** Filled Accent pill for a record ("Fastest 5K", "New best"): OnAccent text holds
 *  AA on every theme's accent. Not an action, so no touch target. A [description]
 *  replaces the visible text for TalkBack ("New best" is read once, as "New best
 *  estimated max for …"). */
@Composable
internal fun RecordChip(text: String, description: String? = null, modifier: Modifier = Modifier) {
    Text(
        text,
        color = OnAccent,
        style = Type.Chip,
        modifier = modifier
            .background(Accent, SegmentShape)
            .chipPadding()
            .then(if (description != null) Modifier.clearAndSetSemantics { contentDescription = description } else Modifier),
    )
}

/** "New best" on a lift set / exercise whose e1RM beat every prior one. */
@Composable
internal fun NewBestChip(exercise: String) {
    RecordChip(
        stringResource(R.string.history_new_best),
        stringResource(R.string.history_new_best_cd, exercise.ifBlank { stringResource(R.string.main_set) }),
    )
}

/** "Effort 7/10" on the run card face once the run is rated; nothing when unrated. */
@Composable
internal fun RunEffortLine(r: RunResultView) {
    val rpe = r.session_rpe ?: return
    Text(
        stringResource(R.string.history_effort_value, rpe),
        color = OnBgMuted,
        style = Type.Caption.merge(TabularFigures),
    )
}

/**
 * A History segment (Lifts or Runs) with nothing logged yet: one line saying it
 * is empty, then the action that fills it ([primaryLabel]) and an optional
 * second way in ([secondaryLabel], e.g. importing past runs) as a start-aligned
 * [TextAction], never a second slab beside the button. No empty
 * chart or zero tiles.
 */
@Composable
internal fun HistoryEmptyState(
    text: String,
    primaryLabel: String,
    onPrimary: () -> Unit,
    secondaryLabel: String? = null,
    onSecondary: () -> Unit = {},
) {
    Column(
        modifier = Modifier.fillMaxWidth().padding(top = Space.Md.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        Text(text, color = OnBgMuted, style = Type.Body)
        PrimaryButton(primaryLabel, onClick = onPrimary, modifier = Modifier.fillMaxWidth())
        if (secondaryLabel != null) {
            TextAction(secondaryLabel, onClick = onSecondary, modifier = Modifier.offset(x = -TextActionInset), chevron = true)
        }
    }
}

/** Which distance chart the Runs progress block shows. */
internal enum class RunProgressWindow { Weeks, Days }

/** Height of each Runs progress chart: one compact chart, not two stacked tall ones. */
private val PROGRESS_CHART_HEIGHT = 96.dp

/** Window of the daily-distance chart. */
private const val PROGRESS_DAYS = 14

/**
 * The Runs segment's progress block (self-monitoring): a PROGRESS overline over one
 * card holding ONE distance chart, switched by a compact two-option segment (a chart
 * range, never as heavy as the Lifts | Runs page filter) between the core's 16-week
 * weekly distance (with the planned weekly distance as a reference line) and the last
 * [PROGRESS_DAYS] days of daily distance ([kmByDay], km per local day, aggregated by
 * the caller). All figures in the user's distance [unit].
 */
@Composable
internal fun RunProgressSection(
    progress: RunProgressView,
    kmByDay: Map<Long, Double>,
    todayLocalDay: Long,
    unit: DistanceUnit,
) {
    val weekly = progress.weekly
    var window by rememberSaveable { mutableStateOf(RunProgressWindow.Weeks) }
    Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
        SectionOverline(stringResource(R.string.history_progress_overline))
        PlainCard {
            Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.CenterEnd) {
                TwoSegmentRow(
                    stringResource(R.string.history_progress_weeks, weekly.size),
                    stringResource(R.string.history_progress_days, PROGRESS_DAYS),
                    selectedIndex = window.ordinal,
                    compact = true,
                ) { window = RunProgressWindow.entries[it] }
            }
            when (window) {
                RunProgressWindow.Weeks -> WeeklyDistanceChart(weekly, progress.planned_weekly_km, unit, chartHeight = PROGRESS_CHART_HEIGHT)
                RunProgressWindow.Days -> RunDistanceBars(
                    kmByDay, todayLocalDay, unit = unit, days = PROGRESS_DAYS, chartHeight = PROGRESS_CHART_HEIGHT,
                )
            }
        }
    }
}

/**
 * The Runs records: a RECORDS overline over one card holding a two-column grid of
 * record cells (longest run, then the best efforts 1K/5K/10K/half), split by a
 * hairline cross. Each cell names the record, states its figure in the user's
 * distance [unit] and the day it was set (the day its run STARTED, [recordDaySec]),
 * and opens the run holding it ([onOpenRun], resolved in [runs]).
 * An odd last cell spans both columns. Always open (it is compact).
 */
@Composable
internal fun RunRecordsSection(
    progress: RunProgressView,
    runs: List<RunResultView>,
    unit: DistanceUnit,
    nowMillis: Long,
    onOpenRun: (RunResultView) -> Unit,
) {
    val cells = buildList {
        if (progress.longest_run_km > 0.0) {
            // A one-decimal summary figure ("13.1 mi"), distinct from the run card's
            // two-decimal measured distance.
            val run = recordRun(runs, null, progress.longest_run_observed_at)
            add(
                RecordCell(
                    label = stringResource(R.string.history_longest_run),
                    value = "${trimDecimal(metersToDisplay(progress.longest_run_km * 1000.0, unit))} ${unit.distanceLabel}",
                    date = formatLogDate(recordDaySec(run, progress.longest_run_observed_at), nowMillis),
                    run = run,
                ),
            )
        }
        // Time plus the day it was set; pace lives in the run sheet the cell opens.
        progress.best_efforts.forEach { b ->
            val run = recordRun(runs, b.entry_id, b.observed_at)
            add(RecordCell(b.label, b.time_label, formatLogDate(recordDaySec(run, b.observed_at), nowMillis), run))
        }
    }
    Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
        SectionOverline(stringResource(R.string.history_records))
        PlainCard(contentPadding = PaddingValues(0.dp), spacing = 0.dp) {
            cells.chunked(2).forEachIndexed { row, pair ->
                if (row > 0) Box(Modifier.fillMaxWidth().height(1.dp).background(Hairline))
                Row(Modifier.fillMaxWidth().height(IntrinsicSize.Min)) {
                    pair.forEachIndexed { col, cell ->
                        if (col > 0) Box(Modifier.width(1.dp).fillMaxHeight().background(Hairline))
                        RecordGridCell(cell, Modifier.weight(1f)) { cell.run?.let(onOpenRun) }
                    }
                }
            }
        }
    }
}

/** One record cell; [run] is null once its run is no longer listed (the tap does nothing). */
private data class RecordCell(
    val label: String,
    val value: String,
    val date: String,
    val run: RunResultView?,
)

/** The stamp a record cell dates itself by: its [run]'s start ([runDaySec]), else the
 *  record's own logged stamp [observedAt] when the run is not listed. */
internal fun recordDaySec(run: RunResultView?, observedAt: Long): Long = run?.let(::runDaySec) ?: observedAt

/** One record cell: overline name, tabular figure, the day it was set; opens its run. */
@Composable
private fun RecordGridCell(cell: RecordCell, modifier: Modifier, onClick: () -> Unit) {
    Column(
        modifier = modifier
            .fillMaxHeight()
            .heightIn(min = ActionMinHeight)
            .clickable(
                role = Role.Button,
                onClickLabel = stringResource(R.string.history_best_effort_open),
                onClick = onClick,
            )
            .testTag("history-record")
            .padding(Space.Card.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Xs.dp),
    ) {
        TileOverline(cell.label)
        Text(cell.value, color = OnBgBody, style = Type.Title.merge(TabularFigures))
        if (cell.date.isNotBlank()) {
            Text(cell.date, color = OnBgMuted, style = Type.Caption.merge(TabularFigures))
        }
    }
}
