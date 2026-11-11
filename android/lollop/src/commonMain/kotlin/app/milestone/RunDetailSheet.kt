package app.milestone

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import app.milestone.resources.Res
import app.milestone.resources.history_effort_overline
import app.milestone.resources.history_training_load_explain
import app.milestone.resources.history_training_load_subject
import app.milestone.resources.history_training_load_value
import app.milestone.resources.history_workout_type
import app.milestone.resources.ic_ui_delete
import app.milestone.resources.ic_ui_share
import app.milestone.resources.ic_ui_trim
import app.milestone.resources.main_delete
import app.milestone.resources.main_delete_run_title
import app.milestone.resources.main_export_gpx
import app.milestone.resources.main_export_gpx_failed
import app.milestone.resources.main_run
import app.milestone.resources.main_save
import app.milestone.resources.run_delete_message
import app.milestone.resources.run_delete_row
import app.milestone.resources.run_details
import app.milestone.resources.run_details_value
import app.milestone.resources.run_effort_value
import app.milestone.resources.run_rate_title
import app.milestone.resources.run_start_caption
import app.milestone.resources.run_trim_row
import app.milestone.resources.run_type_unset
import app.milestone.resources.run_your_log
import kotlin.time.Instant
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.IO
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.datetime.TimeZone
import kotlinx.datetime.toLocalDateTime
import org.jetbrains.compose.resources.painterResource
import org.jetbrains.compose.resources.stringResource

/** "Wed, Oct 14 · 7:42 AM": the run's local start date and time ([startedAt], the
 *  core's `started_at`), localized; blank when undated. */
@Composable
internal fun runStartCaption(startedAt: Long): String {
    if (startedAt <= 0L) return ""
    val clock = if (is24HourClock(currentPlatformContext())) "Hm" else "hm"
    val at = Instant.fromEpochSeconds(startedAt).toLocalDateTime(TimeZone.currentSystemDefault())
    return stringResource(Res.string.run_start_caption, localizedDayFormatter("EEEdMMM").format(at), localizedDayFormatter(clock).format(at))
}

/** A run's route, shared by its detail and trim sheets: its segments of fixes (map,
 *  Undo, the trim map), whether the track can be trimmed (≥2 fixes over a nonzero
 *  span) and, when it can, the timed track the trim slider and readout work on. */
internal data class RunRoute(
    val segments: List<List<GpsPoint>>,
    val trimmable: Boolean,
    val track: List<TimedTrackPoint>,
)

/** Fetches a run's route fixes for [rememberRunRoute] (off the main thread). A var
 *  only so a JVM test can hold the fetch and see what the sheets do while it loads. */
internal var runTrackFetch: (entryId: Long, observedAt: Long) -> List<List<GpsPoint>> =
    { id, at -> AppCore.engine.runTrack(id, at) }

/**
 * The run's route fixes ([DurableCore.runTrack]), fetched ON DEMAND (off the main
 * thread) for the one run being inspected, keyed on the fields that change the
 * route, so rating or tagging never re-runs the fetch. Null while loading. A GPS run
 * QC left unmeasurable has no route, but its recorded fixes are fetched too, for Undo
 * only (never mapped, shared or trimmed). Hoisted above the detail and trim sheets:
 * neither opens before it has loaded, so their height never changes while they slide
 * in. (Material3 1.4's sheet drops a height change that lands as its open animation
 * ends and rests at the old height, its bottom rows below the screen.)
 */
@Composable
internal fun rememberRunRoute(run: RunResultView): RunRoute? {
    val route: RunRoute? by produceState<RunRoute?>(
        null,
        run.entry_id, run.observed_at, run.has_gpx, run.gps_unmeasurable, run.distance_km, run.duration_min,
    ) {
        value = if (run.has_gpx || run.gps_unmeasurable) {
            withContext(Dispatchers.IO) {
                val segs = runTrackFetch(run.entry_id, run.observed_at)
                val flat = segs.flatten()
                val trimmable = run.has_gpx && flat.size >= 2 && flat.last().observedAt > flat.first().observedAt
                RunRoute(segs, trimmable, if (trimmable) timedTrackFromSegments(segs) else emptyList())
            }
        } else {
            RunRoute(emptyList(), false, emptyList())
        }
    }
    return route
}

/**
 * Past-run detail sheet body (spec §2): the run's name with its record badge and
 * start date, the 3-up headline stats and HR-zone bar, any findings, the route map
 * card, the splits, then YOUR LOG (effort, run type, details: each opens its
 * editor) and the action rows (Export GPX, Trim start or end, Delete run, which
 * confirms first). [route] is [rememberRunRoute]'s, null while it loads. [onDelete]
 * gets the event that re-logs the run for Undo ([restoreEventFor]), null when the
 * route hasn't loaded yet.
 */
@Composable
@OptIn(ExperimentalLayoutApi::class)
internal fun RunDetailContent(
    run: RunResultView,
    route: RunRoute?,
    srpeLoad: SrpeLoadView?,
    onEdit: () -> Unit,
    onTrim: () -> Unit,
    onDelete: (restore: Event?) -> Unit,
    onEvent: (Event) -> Unit,
) {
    val unit = rememberDistanceUnit()
    val title = runTitle(run)
    val caption = runStartCaption(run.started_at)
    val stats = runStats(run, unit)
    val splits = run.splits
    var rating by rememberSaveable(run.entry_id, run.observed_at) { mutableStateOf(false) }

    Column(
        modifier = Modifier.sheetBody(rememberScrollState()),
        verticalArrangement = Arrangement.spacedBy(Space.Lg.dp),
    ) {
        // 2.1 Header: name + top record badge on one baseline, then the start date.
        Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
            Row(horizontalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                SheetTitle(title, Modifier.weight(1f).alignByBaseline())
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
                Text(run.summary.ifBlank { stringResource(Res.string.main_run) }, color = OnBgBody, style = Type.Body)
            }
            RunHrRow(run, explain = true)
        }
        // Spike / baseline / interval findings (the split verdict sits under SPLITS).
        RunFindings(run, face = false, showSplit = splits.isEmpty())
        // 2.3 Route map card (the sheet opens once a GPS run's route has loaded);
        // nothing for a manual run.
        val segments = route?.segments.orEmpty()
        if (run.has_gpx && segments.isNotEmpty()) {
            RouteMapCard(
                segments, run.entry_id,
                fullScreen = RouteFullScreen(title, caption, stats),
                recordKm = run.distance_km,
            )
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
    val type = run.workout_type
    Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
        TileOverline(stringResource(Res.string.run_your_log))
        PlainCard(contentPadding = PaddingValues(0.dp), spacing = 0.dp) {
            LogRow(
                label = stringResource(Res.string.history_effort_overline),
                value = if (rpe != null) stringResource(Res.string.run_effort_value, rpe, effortWord(rpe)) else stringResource(Res.string.run_rate_title),
                prompt = rpe == null,
                tag = "run-log-effort",
                onClick = onRate,
                trailing = if (rpe != null) {
                    { DisclosureButton(loadOpen, subject = stringResource(Res.string.history_training_load_subject)) { loadOpen = !loadOpen } }
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
                            stringResource(Res.string.history_training_load_value, formatFixed(it, 0)),
                            color = OnBgBody,
                            style = Type.Body.merge(TabularFigures),
                        )
                    }
                    Text(stringResource(Res.string.history_training_load_explain), color = OnBgMuted, style = Type.Caption)
                    srpeLoad?.let { sl ->
                        ClaimMarkers(
                            grades = listOfNotNull(sl.grade),
                            contested = false,
                            citations = listOfNotNull(sl.citation.takeIf { it.isNotBlank() }),
                        )
                    }
                }
            }
            HorizontalDivider(color = Hairline)
            LogRow(
                label = stringResource(Res.string.history_workout_type),
                value = stringResource(type?.labels?.label ?: Res.string.run_type_unset),
                prompt = type == null,
                tag = "run-log-type",
                onClick = onRate,
            )
            if (editable) {
                HorizontalDivider(color = Hairline)
                LogRow(
                    label = stringResource(Res.string.run_details),
                    value = stringResource(Res.string.run_details_value),
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
            style = (if (bold || prompt) Type.BodyBold else Type.Body).merge(TabularFigures),
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
    val ctx = currentPlatformContext()
    val scope = rememberCoroutineScope()
    val exportFailed = stringResource(Res.string.main_export_gpx_failed)
    val rows = buildList {
        if (run.has_gpx) {
            add(
                ActionRowItem(
                    label = stringResource(Res.string.main_export_gpx),
                    icon = painterResource(Res.drawable.ic_ui_share),
                    testTag = "run-export",
                    onClick = {
                        scope.launch {
                            val g = withContext(Dispatchers.IO) { AppCore.engine.runGpx(run.entry_id, run.observed_at) }
                            if (!g.isNullOrBlank()) {
                                shareGpx(ctx, g, run.started_at)
                            } else {
                                UiMessages.post(UiMessage(exportFailed, long = true))
                            }
                        }
                    },
                ),
            )
        }
        if (route?.trimmable == true) {
            add(
                ActionRowItem(
                    label = stringResource(Res.string.run_trim_row),
                    icon = painterResource(Res.drawable.ic_ui_trim),
                    testTag = "run-trim",
                    onClick = onTrim,
                ),
            )
        }
    }
    ActionRows(
        rows = rows,
        destructive = DestructiveAction(
            label = stringResource(Res.string.run_delete_row),
            icon = painterResource(Res.drawable.ic_ui_delete),
            confirmTitle = stringResource(Res.string.main_delete_run_title),
            confirmMessage = stringResource(Res.string.run_delete_message),
            confirmLabel = stringResource(Res.string.main_delete),
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
    var type by rememberSaveable(run.entry_id, run.observed_at) { mutableStateOf(run.workout_type) }
    AppSheet(onDismiss = onDismiss) {
        Column(
            modifier = Modifier.sheetBody(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(Space.Lg.dp),
        ) {
            SheetTitle(stringResource(Res.string.run_rate_title))
            EffortScale(rpe, onSelect = { rpe = it })
            RunTypeChips(type, onSelect = { wt -> type = if (type == wt) null else wt })
            PrimaryButton(
                stringResource(Res.string.main_save),
                onClick = { onSave(rpe, type) },
                modifier = Modifier.fillMaxWidth().testTag("run-rate-save"),
                minHeight = LargeControlHeight,
            )
        }
    }
}
