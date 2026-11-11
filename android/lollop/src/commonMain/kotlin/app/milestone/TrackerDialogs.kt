package app.milestone

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextMeasurer
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.milestone.resources.Res
import app.milestone.resources.log_discard_confirm
import app.milestone.resources.run_stat_avg_pace
import app.milestone.resources.run_stat_distance
import app.milestone.resources.track_action_resume
import app.milestone.resources.track_btn_save_run
import app.milestone.resources.track_discard_body
import app.milestone.resources.track_discard_title
import app.milestone.resources.track_finish_summary
import app.milestone.resources.track_finish_title
import app.milestone.resources.track_short_run_note
import app.milestone.resources.track_stat_elapsed
import org.jetbrains.compose.resources.stringResource

/**
 * Confirm before deleting a recorded run (no undo, nothing logged). Each button maps
 * to exactly one action: [onCancel] keeps the run, [onConfirm] deletes it. [enabled]
 * is false while a save is in flight.
 */
@Composable
internal fun DiscardRunDialog(enabled: Boolean, onCancel: () -> Unit, onConfirm: () -> Unit) {
    ConfirmDialog(
        title = stringResource(Res.string.track_discard_title),
        body = stringResource(Res.string.track_discard_body),
        confirmLabel = stringResource(Res.string.log_discard_confirm),
        onDismiss = onCancel,
        onConfirm = onConfirm,
        confirmColor = DangerText,
        confirmEnabled = enabled,
    )
}

/**
 * "Add details" during a run: the readiness editor in a bottom sheet, so the safety
 * banner's detail path keeps its full content without leaving the live tracker
 * (chrome §5 / INVARIANT 3).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun TrackerReadinessSheet(model: ViewModel, onEvent: (Event) -> Unit, onDismiss: () -> Unit) {
    AppSheet(onDismiss = onDismiss) {
        Column(
            modifier = Modifier.sheetBody(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
        ) {
            ReadinessEditor(
                signalGroups = model.signal_groups.associate { it.signal to it.group },
                onClose = onDismiss,
            ) { r ->
                onEvent(r)
                onDismiss()
            }
        }
    }
}

/**
 * The Finish review shown in place of the control row after Stop: route thumbnail,
 * the core's distance · moving time · average pace for the very track Save sends (so
 * History repeats these figures), then effort and run type above a pinned Discard ·
 * Resume · Save run row. Discard only opens the confirm ([finishTransition]).
 * [enabled] gates the actions while a save is in flight. One column of at most
 * [ContentMaxWidth]; on a landscape window ([WindowLayout.sideControls]) the route
 * takes the start column at full height and the review the end column, so the
 * figures sit whole above Save on a phone's short landscape height. That end
 * column must fit the figures with their units and the action row with "Save run"
 * on one line at the current font scale (measured); when it does not, the review
 * takes the full width and the route is left out, as a route above the figures
 * would push them out of a short landscape window.
 */
@Composable
internal fun RunFinishPanel(
    unit: DistanceUnit,
    enabled: Boolean,
    sessionRpe: Int?,
    onSessionRpe: (Int?) -> Unit,
    workoutType: WorkoutType?,
    onWorkoutType: (WorkoutType) -> Unit,
    onResume: () -> Unit,
    onSave: () -> Unit,
    onDiscard: () -> Unit,
    modifier: Modifier = Modifier,
) {
    // Distance and moving time are the core's figures for the very event Save sends
    // ([RunSession.trackEventForCore]): the same QC and derivation as the saved run,
    // so the review shows exactly what History then lists.
    val points by RunSession.points.collectAsStateWithLifecycle()
    val figures = remember(points) {
        AppCore.engine.savedFigures(RunSession.trackEventForCore(entryId = 0L, workoutType = null, sessionRpe = null))
    }
    val km = figures?.distance_km ?: 0.0
    val elapsedSec = figures?.let { (it.duration_min * 60.0).roundOrZero() } ?: 0L
    val route = remember(points) {
        trackSegments(RunSession.decimatedTrackForCore(), RunSession.decimatedSegmentStarts())
    }
    val avgPace = averagePaceMinPerKm(elapsedSec, km)
    val spoken = finishSummarySpoken(currentResources(), km, elapsedSec, unit)
    val routeShown = route.sumOf { it.size } >= 2
    val sideControls = LocalWindowLayout.current.sideControls
    val distance = distanceFigure(km, unit)
    val elapsed = formatElapsed(elapsedSec)
    val pace = paceFigure(avgPace, unit)
    val resumeLabel = stringResource(Res.string.track_action_resume)
    val saveLabel = stringResource(Res.string.track_btn_save_run)
    val measurer = rememberTextMeasurer()
    val density = LocalDensity.current
    val routeAbove = routeShown && !sideControls
    val review: @Composable ColumnScope.() -> Unit = {
        Text(
            stringResource(Res.string.track_finish_title),
            color = OnBgBody,
            style = Type.Title.copy(fontWeight = FontWeight.ExtraBold),
            modifier = Modifier.semantics { heading() },
        )
        // The summary and pickers scroll when they outgrow the height above the
        // pinned action row; any spare height sits above that row, never as a gap
        // between the summary and the pickers.
        Box(Modifier.weight(1f).fillMaxWidth()) {
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(Space.Lg.dp),
            ) {
                if (routeAbove) {
                    RouteMapCard(route, entryId = 0L, height = FinishRouteHeight, distanceMarkers = false, recordKm = km)
                }
                Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .clearAndSetSemantics { contentDescription = spoken },
                        horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
                    ) {
                        LiveFigure(
                            label = stringResource(Res.string.run_stat_distance),
                            value = distance,
                            suffix = unit.distanceLabel,
                            known = km > 0.0,
                            maxSp = LIVE_FIGURE_MAX_SP,
                            modifier = Modifier.weight(1f),
                        )
                        LiveFigure(
                            label = stringResource(Res.string.track_stat_elapsed),
                            value = elapsed,
                            suffix = null,
                            known = true,
                            maxSp = LIVE_FIGURE_MAX_SP,
                            modifier = Modifier.weight(1f),
                        )
                        LiveFigure(
                            label = stringResource(Res.string.run_stat_avg_pace),
                            value = pace,
                            suffix = unit.paceSuffix,
                            known = avgPace.isFinite(),
                            maxSp = LIVE_FIGURE_MAX_SP,
                            modifier = Modifier.weight(1f),
                        )
                    }
                    // A run under the keep threshold says so here, inline: Save keeps
                    // it, Discard drops it, and the effort/type picks are never lost to
                    // a dialog. The threshold reads as a round figure ("0.3 mi").
                    if (isShortRun(km, elapsedSec)) {
                        InlineNotice(
                            stringResource(
                                Res.string.track_short_run_note,
                                runDistLabel(km, unit),
                                shortRunThresholdLabel(unit),
                                shortRunMinutesLabel(),
                            ),
                        )
                    }
                }
                // Whole-session effort (CR-10, LOAD-SRPE-001): skippable; tap the picked
                // segment again to clear it. Drives the session-effort load series.
                EffortScale(selected = sessionRpe, onSelect = onSessionRpe)
                // Run type: the user's label, preselected from the plan being followed.
                RunTypeChips(workoutType, onSelect = onWorkoutType)
            }
        }
        ControlRow {
            DiscardRunCircle(enabled = enabled, onClick = onDiscard)
            TextAction(
                resumeLabel,
                onClick = onResume,
                enabled = enabled,
            )
            ControlPill(
                saveLabel,
                onClick = onSave,
                modifier = Modifier.weight(1f),
                enabled = enabled,
            )
        }
    }
    BoxWithConstraints(modifier.fillMaxWidth()) {
        val columnPx = (constraints.maxWidth - with(density) { Space.Lg.dp.roundToPx() }) / 2
        val columnFits = remember(columnPx, distance, elapsed, pace, unit, resumeLabel, saveLabel, measurer, density) {
            val figures = listOf(distance to unit.distanceLabel, elapsed to null, pace to unit.paceSuffix)
            finishColumnFits(measurer, density, columnPx, figures, resumeLabel, saveLabel)
        }
        if (routeShown && sideControls && columnFits) {
            Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Space.Lg.dp)) {
                BoxWithConstraints(Modifier.weight(1f).fillMaxHeight().testTag("finish-route")) {
                    RouteMapCard(route, entryId = 0L, height = maxHeight, distanceMarkers = false, recordKm = km)
                }
                Column(
                    modifier = Modifier.weight(1f).fillMaxHeight(),
                    verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
                    content = review,
                )
            }
        } else {
            Box(modifier = Modifier.fillMaxWidth(), contentAlignment = Alignment.TopCenter) {
                Column(
                    modifier = Modifier.widthIn(max = ContentMaxWidth).fillMaxWidth().fillMaxHeight(),
                    verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
                    content = review,
                )
            }
        }
    }
}

/** The Finish review's end column, [columnPx] wide, fits each of the three
 *  [figures] (value, unit) whole in its third of the figures row and the Discard ·
 *  [resume] · [save] row with both labels on one line. */
private fun finishColumnFits(
    measurer: TextMeasurer,
    density: Density,
    columnPx: Int,
    figures: List<Pair<String, String?>>,
    resume: String,
    save: String,
): Boolean {
    val thirdPx = (columnPx - with(density) { Space.Md.dp.roundToPx() } * (figures.size - 1)) / figures.size
    return figures.all { (value, suffix) -> liveFigureMinWidthPx(measurer, value, suffix) <= thirdPx } &&
        circleTextPillRowMinWidthPx(measurer, density, resume, save) <= columnPx
}

/** Widest the Finish review's route + review pair grows on a landscape window;
 *  the pair centres beyond it, like [TwoPaneRow]. */
internal val FinishPairMaxWidth = PaneMaxWidth * 2 + Space.Lg.dp

/** Height of the Finish review's route thumbnail. */
private val FinishRouteHeight = 160.dp

/** The Finish review's figures read aloud with units spelled out, e.g. "0.60
 *  kilometers · 3 minutes 20 seconds moving · 5 minutes 33 seconds per kilometer
 *  average". */
internal fun finishSummarySpoken(res: PlatformResources, km: Double, elapsedSec: Long, unit: DistanceUnit): String =
    res.getString(
        Res.string.track_finish_summary,
        spokenRunDistance(res, km, unit),
        spokenDuration(res, elapsedSec),
        spokenPace(res, averagePaceMinPerKm(elapsedSec, km), unit),
    )
