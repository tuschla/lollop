package app.milestone

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.text.TextAutoSize
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** One headline stat column: overline [label], tabular [value], its [unit] (may be
 *  blank) and the merged [spoken] form ("Distance, 3.18 miles"). */
internal data class RunStat(val label: String, val value: String, val unit: String, val spoken: String)

/** Distance, moving time and average pace of [run] in [unit], each only when measured. */
@Composable
internal fun runStats(run: RunResultView, unit: DistanceUnit): List<RunStat> {
    val stats = ArrayList<RunStat>(3)
    if (run.distance_km > 0.0) {
        val dist = distanceFigure(run.distance_km, unit)
        val label = stringResource(R.string.run_stat_distance)
        val spoken = stringResource(if (unit == DistanceUnit.Mi) R.string.run_unit_miles else R.string.run_unit_km, dist)
        stats += RunStat(label, dist, unit.distanceLabel, stringResource(R.string.run_stat_cd, label, spoken))
    }
    if (run.duration_min > 0.0) {
        val time = fmtRunTime(Math.round(run.duration_min * 60.0).toInt())
        val label = stringResource(R.string.run_stat_time)
        stats += RunStat(label, time, "", stringResource(R.string.run_stat_cd, label, time))
    }
    if (run.distance_km > 0.0 && run.duration_min > 0.0) {
        val pace = formatPaceMinutes(paceInUnit(run.duration_min / run.distance_km, unit))
        val label = stringResource(R.string.run_stat_avg_pace)
        val spoken = stringResource(if (unit == DistanceUnit.Mi) R.string.run_unit_per_mile else R.string.run_unit_per_km, pace)
        stats += RunStat(label, pace, unit.paceSuffix, stringResource(R.string.run_stat_cd, label, spoken))
    }
    return stats
}

/** Smallest a headline stat value auto-sizes to before it would wrap (spec §2.2). */
private val StatValueMinSize = 24.sp

/**
 * The 3-up headline stats (spec §2.2, the tracker's stat typography a step
 * smaller): equal columns of a [TileOverline] label over a `Type.Display` value
 * (auto-sized down to 24sp before it would wrap) with its unit in `Type.Caption` on
 * the value's baseline. [compact] (the full-screen map's stat sheet) sets the
 * values in `Type.Title` and reads the whole row once.
 */
@Composable
internal fun RunStatColumns(stats: List<RunStat>, compact: Boolean, modifier: Modifier = Modifier) {
    val valueStyle = (if (compact) Type.Title.merge(TabularFigures) else Type.Display).copy(color = OnBgBody)
    Row(
        modifier
            .fillMaxWidth()
            .height(IntrinsicSize.Min)
            .then(if (compact) Modifier.semantics(mergeDescendants = true) {} else Modifier),
        horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        stats.forEach { stat ->
            Column(
                Modifier
                    .weight(1f)
                    .clearAndSetSemantics { contentDescription = stat.spoken },
                verticalArrangement = Arrangement.spacedBy(Space.Xs.dp),
            ) {
                TileOverline(stat.label)
                Row {
                    BasicText(
                        stat.value,
                        style = valueStyle,
                        maxLines = 1,
                        autoSize = if (compact) null else TextAutoSize.StepBased(StatValueMinSize, Type.Display.fontSize),
                        modifier = Modifier.alignByBaseline().weight(1f, fill = false),
                    )
                    if (stat.unit.isNotEmpty()) {
                        Spacer(Modifier.width(Space.Xs.dp))
                        Text(stat.unit, color = OnBgMuted, style = Type.Caption, maxLines = 1, modifier = Modifier.alignByBaseline())
                    }
                }
            }
        }
    }
}

/** "Wed, Oct 14 · 7:42 AM": the run's local start date and time, localized; blank when undated. */
@Composable
internal fun runStartCaption(observedAt: Long): String {
    if (observedAt <= 0L) return ""
    val ctx = LocalContext.current
    val locale = Locale.getDefault()
    val at = Instant.ofEpochSecond(observedAt).atZone(ZoneId.systemDefault())
    val date = DateTimeFormatter.ofPattern(android.text.format.DateFormat.getBestDateTimePattern(locale, "EEEdMMM"), locale)
    val time = DateTimeFormatter.ofPattern(
        android.text.format.DateFormat.getBestDateTimePattern(locale, if (android.text.format.DateFormat.is24HourFormat(ctx)) "Hm" else "hm"),
        locale,
    )
    return stringResource(R.string.history_run_overline, at.format(date), at.format(time))
}

/** The detail sheet's route: the core's GPX document (for Export), its parsed segments
 *  (map, Undo) and whether the track can be trimmed (≥2 fixes over a nonzero span). */
private data class RunRoute(val gpx: String?, val segments: List<List<GpxFix>>, val trimmable: Boolean)

/**
 * Past-run detail sheet body (spec §2): the run's name with its record badge and
 * start date, the 3-up headline stats and HR-zone bar, any findings, the route map
 * card, the splits, then YOUR LOG (effort, run type, details: each opens its
 * editor) and the action rows (Export GPX, Trim start or end, Delete run, which
 * confirms first). [onDelete] gets the event that re-logs the run for Undo
 * ([restoreEventFor]), null when the route hasn't loaded yet.
 */
@Composable
@OptIn(ExperimentalLayoutApi::class)
internal fun RunDetailContent(
    run: RunResultView,
    nowMillis: Long,
    srpeLoad: SrpeLoadView?,
    onEdit: () -> Unit,
    onTrim: () -> Unit,
    onDelete: (restore: Event?) -> Unit,
    onEvent: (Event) -> Unit,
) {
    val unit = rememberDistanceUnit()
    // The run's GPX is fetched ON DEMAND (off the main thread) for the one run being
    // inspected and parsed ONCE in the same block, keyed on the fields that change the
    // route, so rating or tagging never re-runs the export. Null while loading.
    val route: RunRoute? by produceState<RunRoute?>(
        null,
        run.entry_id, run.observed_at, run.has_gpx, run.distance_km, run.duration_min,
    ) {
        value = if (run.has_gpx) {
            withContext(Dispatchers.IO) {
                val gpx = Core.exportGpx(run.entry_id, run.observed_at)
                val segs = if (gpx.isNullOrBlank()) emptyList() else parseGpx(gpx)
                val flat = segs.flatten()
                RunRoute(gpx, segs, flat.size >= 2 && flat.last().timeSec > flat.first().timeSec)
            }
        } else {
            RunRoute(null, emptyList(), false)
        }
    }
    val title = runTitle(run)
    val caption = runStartCaption(run.observed_at)
    val stats = runStats(run, unit)
    val splits = if (unit == DistanceUnit.Mi) run.splits_mi else run.splits_km
    var rating by rememberSaveable(run.entry_id, run.observed_at) { mutableStateOf(false) }

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .verticalScroll(rememberScrollState())
            .navigationBarsPadding()
            .padding(horizontal = Space.Sheet.dp)
            .padding(bottom = Space.Lg.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Lg.dp),
    ) {
        // 2.1 Header: name + top record badge on one baseline, then the start date.
        Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
            Row(horizontalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                Text(title, color = OnBgBody, style = Type.Title, modifier = Modifier.weight(1f).alignByBaseline())
                run.best_effort_labels.lastOrNull()?.let { RecordChip(it, modifier = Modifier.alignByBaseline()) }
            }
            if (caption.isNotBlank()) Text(caption, color = OnBgMuted, style = Type.Caption.merge(TabularFigures))
            if (run.best_effort_labels.size > 1) {
                FlowRow(
                    horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
                    verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
                ) {
                    run.best_effort_labels.dropLast(1).forEach { RecordChip(it) }
                }
            }
        }
        // 2.2 Headline stats (+ the HR row only when HR was measured).
        Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
            if (stats.isNotEmpty()) {
                RunStatColumns(stats, compact = false)
            } else {
                Text(run.summary.ifBlank { stringResource(R.string.main_run) }, color = OnBgBody, style = Type.Body)
            }
            RunHrRow(run, explain = true)
        }
        // Spike / baseline / interval findings (the split verdict sits under SPLITS).
        RunFindings(run, face = false, showSplit = splits.isEmpty())
        // 2.3 Route map card: a placeholder while the GPX loads; nothing for a manual run.
        if (run.has_gpx) {
            val r = route
            if (r == null) {
                RouteMapPlaceholder()
            } else if (r.segments.isNotEmpty()) {
                RouteMapCard(
                    r.segments, run.entry_id,
                    fullScreen = RouteFullScreen(title, caption, stats),
                    recordKm = run.distance_km,
                )
            }
        }
        // 2.5 Splits.
        if (splits.isNotEmpty()) RunSplitsSection(run, splits, unit)
        // 2.6 Your log.
        RunLogSection(run, srpeLoad, editable = !run.has_gpx && run.distance_km > 0.0, onRate = { rating = true }, onEdit = onEdit)
        // 2.7 Actions.
        RunActions(run, route, onTrim, onDelete)
    }
    if (rating) {
        RateRunSheet(
            run,
            onDismiss = { rating = false },
            onSave = { rpe, type ->
                onEvent(Event.AmendRunMeta(run.entry_id, run.observed_at, rpe, type))
                rating = false
            },
        )
    }
}

/**
 * YOUR LOG (spec §2.6): the run's effort, type and (manual runs) details as rows in
 * one card. Effort reads "5 of 10 · Hard" with the training-load "?" (its panel
 * opens inside the card) or "Rate this run"; Run type reads the type or "Set type".
 * Effort and Run type open the Rate run sheet; Details opens the run editor.
 */
@Composable
private fun RunLogSection(
    run: RunResultView,
    srpeLoad: SrpeLoadView?,
    editable: Boolean,
    onRate: () -> Unit,
    onEdit: () -> Unit,
) {
    var loadOpen by rememberSaveable(run.entry_id, run.observed_at) { mutableStateOf(false) }
    val rpe = run.session_rpe
    val type = WorkoutType.fromWire(run.workout_type)
    Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
        TileOverline(stringResource(R.string.run_your_log))
        PlainCard(contentPadding = PaddingValues(0.dp), spacing = 0.dp) {
            LogRow(
                label = stringResource(R.string.history_effort_overline),
                value = if (rpe != null) stringResource(R.string.run_effort_value, rpe, effortWord(rpe)) else stringResource(R.string.run_rate_title),
                prompt = rpe == null,
                tag = "run-log-effort",
                onClick = onRate,
                trailing = if (rpe != null) {
                    { DisclosureButton(loadOpen, subject = stringResource(R.string.history_training_load_subject)) { loadOpen = !loadOpen } }
                } else {
                    null
                },
            )
            if (rpe != null && loadOpen) {
                // The load is the core's figure for the stored rating; a unitless figure
                // only means something next to its explanation, so it lives here.
                Column(
                    modifier = Modifier.padding(start = Space.Card.dp, end = Space.Card.dp, bottom = Space.Card.dp),
                    verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
                ) {
                    run.srpe_load_au?.let {
                        Text(
                            stringResource(R.string.history_training_load_value, String.format(Locale.US, "%.0f", it)),
                            color = OnBgBody,
                            style = Type.Body.merge(TabularFigures),
                        )
                    }
                    Text(stringResource(R.string.history_training_load_explain), color = OnBgMuted, style = Type.Caption)
                    srpeLoad?.let { sl ->
                        if (sl.grade.isNotBlank()) GradeChip(sl.grade)
                        if (sl.citation.isNotBlank()) Text(citationLabel(sl.citation), color = OnBgFaint, style = Type.Caption)
                    }
                }
            }
            HorizontalDivider(color = Hairline)
            LogRow(
                label = stringResource(R.string.history_workout_type),
                value = type?.label ?: stringResource(R.string.run_type_unset),
                prompt = type == null,
                tag = "run-log-type",
                onClick = onRate,
            )
            if (editable) {
                HorizontalDivider(color = Hairline)
                LogRow(
                    label = stringResource(R.string.run_details),
                    value = stringResource(R.string.run_details_value),
                    prompt = false,
                    bold = false,
                    tag = "run-log-details",
                    onClick = onEdit,
                )
            }
        }
    }
}

/** One YOUR LOG row: muted label, the value (an `Accent` [prompt] when unset), an
 *  optional [trailing] control in its own target, and a chevron. */
@Composable
private fun LogRow(
    label: String,
    value: String,
    prompt: Boolean,
    tag: String,
    onClick: () -> Unit,
    bold: Boolean = true,
    trailing: (@Composable () -> Unit)? = null,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = LargeControlHeight)
            .clickable(role = Role.Button, onClick = onClick)
            .testTag(tag)
            .padding(start = Space.Card.dp, end = Space.Card.dp, top = Space.Sm.dp, bottom = Space.Sm.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        Text(label, color = OnBgMuted, style = Type.Body)
        Text(
            value,
            color = if (prompt) Accent else OnBgBody,
            style = (if (bold || prompt) Type.Body.copy(fontWeight = FontWeight.Bold) else Type.Body).merge(TabularFigures),
            textAlign = TextAlign.End,
            modifier = Modifier.weight(1f),
        )
        trailing?.invoke()
        RowChevron()
    }
}

/**
 * The sheet's footer (spec §2.7): Export GPX (the system share sheet, the file named
 * by the run's start) and Trim start or end when the run has a track, then Delete
 * run, which confirms in a dialog.
 */
@Composable
private fun RunActions(run: RunResultView, route: RunRoute?, onTrim: () -> Unit, onDelete: (Event?) -> Unit) {
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    val rows = buildList {
        if (run.has_gpx) {
            add(
                ActionRowItem(
                    label = stringResource(R.string.main_export_gpx),
                    icon = painterResource(R.drawable.ic_ui_share),
                    chevron = true,
                    testTag = "run-export",
                    onClick = {
                        scope.launch {
                            val g = route?.gpx ?: withContext(Dispatchers.IO) { Core.exportGpx(run.entry_id, run.observed_at) }
                            if (!g.isNullOrBlank()) {
                                shareGpx(ctx, g, run.observed_at)
                            } else {
                                android.widget.Toast.makeText(
                                    ctx,
                                    ctx.getString(R.string.main_export_gpx_failed),
                                    android.widget.Toast.LENGTH_LONG,
                                ).show()
                            }
                        }
                    },
                ),
            )
        }
        if (route?.trimmable == true) {
            add(
                ActionRowItem(
                    label = stringResource(R.string.run_trim_row),
                    icon = painterResource(R.drawable.ic_ui_trim),
                    chevron = true,
                    testTag = "run-trim",
                    onClick = onTrim,
                ),
            )
        }
    }
    ActionRows(
        rows = rows,
        destructive = DestructiveAction(
            label = stringResource(R.string.run_delete_row),
            icon = painterResource(R.drawable.ic_ui_delete),
            confirmTitle = stringResource(R.string.main_delete_run_title),
            confirmMessage = stringResource(R.string.run_delete_message),
            confirmLabel = stringResource(R.string.main_delete),
            testTag = "run-delete",
            onConfirm = { onDelete(restoreEventFor(HistoryEntry.Run(run), route?.segments.orEmpty())) },
        ),
    )
}

/**
 * Rate run (spec §2.6), stacked over the detail sheet: the effort [EffortScale]
 * and the run type chips, saved together as one [Event.AmendRunMeta].
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun RateRunSheet(run: RunResultView, onDismiss: () -> Unit, onSave: (Int?, WorkoutType?) -> Unit) {
    var rpe by rememberSaveable(run.entry_id, run.observed_at) { mutableStateOf(run.session_rpe) }
    var type by rememberSaveable(run.entry_id, run.observed_at) { mutableStateOf(WorkoutType.fromWire(run.workout_type)) }
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        containerColor = BgSunken,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
    ) {
        SheetNavBarScrim()
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .navigationBarsPadding()
                .padding(horizontal = Space.Sheet.dp)
                .padding(bottom = Space.Lg.dp),
            verticalArrangement = Arrangement.spacedBy(Space.Lg.dp),
        ) {
            Text(stringResource(R.string.run_rate_title), color = OnBgBody, style = Type.Title)
            EffortScale(rpe, onSelect = { rpe = it })
            RunTypeChips(type, onSelect = { wt -> type = if (type == wt) null else wt })
            PrimaryButton(
                stringResource(R.string.main_save),
                onClick = { onSave(rpe, type) },
                modifier = Modifier.fillMaxWidth().testTag("run-rate-save"),
                minHeight = LargeControlHeight,
            )
        }
    }
}
