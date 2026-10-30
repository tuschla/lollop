package app.milestone

import android.content.res.Resources
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle

/**
 * Confirm before deleting a recorded run (no undo, nothing logged). Each button maps
 * to exactly one action: [onCancel] keeps the run, [onConfirm] deletes it. [enabled]
 * is false while a save is in flight.
 */
@Composable
internal fun DiscardRunDialog(enabled: Boolean, onCancel: () -> Unit, onConfirm: () -> Unit) {
    ConfirmDialog(
        title = stringResource(R.string.track_discard_title),
        body = stringResource(R.string.track_discard_body),
        confirmLabel = stringResource(R.string.log_discard_confirm),
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
 * [enabled] gates the actions while a save is in flight.
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
        Core.savedFigures(RunSession.trackEventForCore(entryId = 0L, workoutType = null, sessionRpe = null))
    }
    val km = figures?.distance_km ?: 0.0
    val elapsedSec = figures?.let { Math.round(it.duration_min * 60.0) } ?: 0L
    val route = remember(points) {
        trackSegments(RunSession.decimatedTrackForCore(), RunSession.decimatedSegmentStarts())
    }
    val avgPace = averagePaceMinPerKm(elapsedSec, km)
    val spoken = finishSummarySpoken(LocalResources.current, km, elapsedSec, unit)
    Column(
        modifier = modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        Text(
            stringResource(R.string.track_finish_title),
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
                if (route.sumOf { it.size } >= 2) {
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
                            label = stringResource(R.string.run_stat_distance),
                            value = distanceFigure(km, unit),
                            suffix = unit.distanceLabel,
                            known = km > 0.0,
                            maxSp = LIVE_FIGURE_MAX_SP,
                            modifier = Modifier.weight(1f),
                        )
                        LiveFigure(
                            label = stringResource(R.string.track_stat_elapsed),
                            value = formatElapsed(elapsedSec),
                            suffix = null,
                            known = true,
                            maxSp = LIVE_FIGURE_MAX_SP,
                            modifier = Modifier.weight(1f),
                        )
                        LiveFigure(
                            label = stringResource(R.string.run_stat_avg_pace),
                            value = paceFigure(avgPace, unit),
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
                                R.string.track_short_run_note,
                                runDistLabel(km, unit),
                                shortRunThresholdLabel(unit),
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
                stringResource(R.string.track_action_resume),
                onClick = onResume,
                enabled = enabled,
            )
            ControlPill(
                stringResource(R.string.track_btn_save_run),
                onClick = onSave,
                modifier = Modifier.weight(1f),
                enabled = enabled,
            )
        }
    }
}

/** Height of the Finish review's route thumbnail. */
private val FinishRouteHeight = 160.dp

/** The Finish review's figures read aloud with units spelled out, e.g. "0.60
 *  kilometers · 3 minutes 20 seconds moving · 5 minutes 33 seconds per kilometer
 *  average". */
internal fun finishSummarySpoken(res: Resources, km: Double, elapsedSec: Long, unit: DistanceUnit): String =
    res.getString(
        R.string.track_finish_summary,
        spokenRunDistance(res, km, unit),
        spokenDuration(res, elapsedSec),
        spokenPace(res, averagePaceMinPerKm(elapsedSec, km), unit),
    )
