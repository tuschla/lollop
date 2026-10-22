package app.milestone

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
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
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.dp
import java.time.Instant
import java.time.ZoneId
import java.util.Locale

// ── Run cards (History "Runs" stream) ───────────────────────────────────────

/** Whether the run carries a heart-rate reading: the core's [RunResultView.hr_label]
 *  ("148 bpm · Z2", "HR 80% · Z2") is empty for a run without HR, and the HR row
 *  is hidden then rather than printing a dead line. */
internal fun RunResultView.hasHrReading(): Boolean = hr_label.isNotBlank()

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
    if (r.observed_at <= 0L) return stringResource(R.string.main_run)
    val weekday = Instant.ofEpochSecond(r.observed_at).atZone(ZoneId.systemDefault()).dayOfWeek
        .getDisplayName(java.time.format.TextStyle.FULL, Locale.getDefault())
    return stringResource(R.string.history_run_weekday, weekday)
}

/** Local calendar days from [epochSec] to [nowMillis] (0 = today), or null when undated. */
internal fun localDaysAgo(epochSec: Long, nowMillis: Long): Long? {
    if (epochSec <= 0L) return null
    val zone = ZoneId.systemDefault()
    return localEpochDay(nowMillis / 1000, zone) - localEpochDay(epochSec, zone)
}

/**
 * The date chip beside a run's title: "Today", "Yesterday", "3 days ago", then the
 * absolute date, on every run card alike (an imported or untagged run included), so
 * the list reads at one rhythm.
 */
internal fun runDateChip(r: RunResultView, nowMillis: Long): String = formatLogDate(r.observed_at, nowMillis)

/** The run detail overline: title and date with the day named once ("Tempo run ·
 *  Today", "Today", "Saturday run", "Saturday run · September 12"). */
@Composable
internal fun runOverline(r: RunResultView, nowMillis: Long): String {
    val chip = runDateChip(r, nowMillis)
    val untaggedRecent = WorkoutType.fromWire(r.workout_type) == null &&
        localDaysAgo(r.observed_at, nowMillis)?.let { it in 0L..1L } == true
    return when {
        untaggedRecent -> chip
        chip.isBlank() -> runTitle(r)
        else -> stringResource(R.string.history_run_overline, runTitle(r), chip)
    }
}

/** The run's face line in the display unit: "8.05 km · 5:34 /km · 44:12". The time is
 *  the same clock Today and the Finish review show for a run. */
@Composable
internal fun runFaceLine(r: RunResultView, unit: DistanceUnit): String {
    val base = "${runDistLabel(r.distance_km, unit)} · ${runPaceLabel(r.distance_km, r.duration_min, unit)}"
    return if (r.duration_min > 0.0) "$base · ${fmtRunTime(Math.round(r.duration_min * 60.0).toInt())}" else base
}

// ── Records, run rating and run progress ────────────────────────────────────

/** Filled Accent pill for a record ("Fastest 5K", "New best"): OnAccent text holds
 *  AA on every theme's accent. Not an action, so no touch target. */
@Composable
internal fun RecordChip(text: String, description: String? = null, modifier: Modifier = Modifier) {
    Text(
        text,
        color = OnAccent,
        style = Type.Chip,
        modifier = modifier
            .background(Accent, RoundedCornerShape(Space.Md.dp - Space.Xs.dp))
            .padding(horizontal = Space.Md.dp, vertical = Space.Sm.dp - 1.dp)
            .then(if (description != null) Modifier.semantics { contentDescription = description } else Modifier),
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

/** The core's best-effort records this run holds ("Fastest 5K"), as chips. The card
 *  [face] carries only the longest-distance record (the list is ordered by distance);
 *  the detail sheet lists them all. Renders nothing otherwise. */
@Composable
@OptIn(ExperimentalLayoutApi::class)
internal fun RunRecordChips(r: RunResultView, face: Boolean) {
    if (r.best_effort_labels.isEmpty()) return
    if (face) {
        RecordChip(r.best_effort_labels.last())
        return
    }
    FlowRow(
        horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
    ) {
        r.best_effort_labels.forEach { RecordChip(it) }
    }
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
 * A History segment (Lifts or Runs) with nothing logged yet: one line saying what
 * will show up here, then the action that fills it ([primaryLabel]) and an optional
 * second way in ([secondaryLabel], e.g. importing past runs) as a start-aligned
 * [TextAction], never a second slab beside the button (ui-sweep §14). No empty
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
 * the caller). All figures in the user's distance [unit]. Without a weekly series (an
 * older core) only the daily chart renders, with no segment.
 */
@Composable
internal fun RunProgressSection(
    progress: RunProgressView?,
    kmByDay: Map<Long, Double>,
    todayLocalDay: Long,
    unit: DistanceUnit,
    // The local day the active plan began (null = no plan, or older than the core
    // reports): the weekly chart's plan line starts there.
    planStartEpochDay: Long? = null,
) {
    val weekly = progress?.weekly.orEmpty()
    var window by rememberSaveable { mutableStateOf(RunProgressWindow.Weeks) }
    val shown = if (weekly.isEmpty()) RunProgressWindow.Days else window
    Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
        SectionOverline(stringResource(R.string.history_progress_overline))
        PlainCard {
            if (weekly.isNotEmpty()) {
                Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.CenterEnd) {
                    TwoSegmentRow(
                        stringResource(R.string.history_progress_weeks, weekly.size),
                        stringResource(R.string.history_progress_days, PROGRESS_DAYS),
                        selectedIndex = window.ordinal,
                        compact = true,
                    ) { window = RunProgressWindow.entries[it] }
                }
            }
            when (shown) {
                RunProgressWindow.Weeks -> WeeklyDistanceChart(
                    weekly, progress?.planned_weekly_km ?: 0.0, unit, chartHeight = PROGRESS_CHART_HEIGHT,
                    planStartEpochDay = planStartEpochDay,
                )
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
 * distance [unit] and the day it was set, and opens the run holding it. An odd last
 * cell spans both columns. Always open (it is compact). [onOpenRun] gets the run's
 * entry_id (null when only its timestamp is known) and observed_at.
 */
@Composable
internal fun RunRecordsSection(
    progress: RunProgressView,
    unit: DistanceUnit,
    nowMillis: Long,
    onOpenRun: (entryId: Long?, observedAt: Long) -> Unit,
) {
    val cells = buildList {
        if (progress.longest_run_km > 0.0) {
            // A one-decimal summary figure ("13.1 mi"), distinct from the run card's
            // two-decimal measured distance.
            add(
                RecordCell(
                    label = stringResource(R.string.history_longest_run),
                    value = "${trimDecimal(metersToDisplay(progress.longest_run_km * 1000.0, unit))} ${unit.distanceLabel}",
                    date = formatLogDate(progress.longest_run_observed_at, nowMillis),
                    entryId = null,
                    observedAt = progress.longest_run_observed_at,
                ),
            )
        }
        // Time plus the day it was set; pace lives in the run sheet the cell opens.
        progress.best_efforts.forEach { b ->
            add(RecordCell(b.label, b.time_label, formatLogDate(b.observed_at, nowMillis), b.entry_id, b.observed_at))
        }
    }
    if (cells.isEmpty()) return
    Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
        SectionOverline(stringResource(R.string.history_records))
        PlainCard(contentPadding = PaddingValues(0.dp), spacing = 0.dp) {
            cells.chunked(2).forEachIndexed { row, pair ->
                if (row > 0) Box(Modifier.fillMaxWidth().height(1.dp).background(Hairline))
                Row(Modifier.fillMaxWidth().height(IntrinsicSize.Min)) {
                    pair.forEachIndexed { col, cell ->
                        if (col > 0) Box(Modifier.width(1.dp).fillMaxHeight().background(Hairline))
                        RecordGridCell(cell, Modifier.weight(1f)) { onOpenRun(cell.entryId, cell.observedAt) }
                    }
                }
            }
        }
    }
}

private data class RecordCell(
    val label: String,
    val value: String,
    val date: String,
    val entryId: Long?,
    val observedAt: Long,
)

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
