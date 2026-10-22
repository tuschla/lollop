package app.milestone

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle

/**
 * Discarding a captured-but-unsaved run deletes the whole recording with no undo and
 * logs nothing, so it confirms first like every other destructive action.
 */
@Composable
internal fun CaptureDiscardDialog(visible: Boolean, onDismiss: () -> Unit, onDiscard: () -> Unit) {
    ClearConfirmDialog(
        visible = visible,
        title = stringResource(R.string.track_discard_title),
        message = stringResource(R.string.track_discard_body),
        confirmLabel = stringResource(R.string.log_discard_confirm),
        onDismiss = onDismiss,
        onClear = onDiscard,
    )
}

/**
 * The Finish review's Discard confirm. Its own dialog (not [ClearConfirmDialog],
 * whose confirm fires onDismiss before onClear) so each button maps to exactly one
 * [FinishAction]: [onCancel] returns to the review, [onConfirm] deletes the run.
 * [enabled] is false while a save is in flight.
 */
@Composable
internal fun FinishDiscardDialog(enabled: Boolean, onCancel: () -> Unit, onConfirm: () -> Unit) {
    AlertDialog(
        onDismissRequest = onCancel,
        shape = RoundedCornerShape(Space.Card.dp),
        title = { Text(stringResource(R.string.track_discard_title)) },
        text = { Text(stringResource(R.string.track_discard_body)) },
        confirmButton = {
            TextButton(onClick = onConfirm, enabled = enabled) {
                Text(stringResource(R.string.log_discard_confirm), color = DangerText)
            }
        },
        dismissButton = {
            TextButton(onClick = onCancel) { Text(stringResource(R.string.main_cancel)) }
        },
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
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        containerColor = BgElevated,
    ) {
        SheetNavBarScrim()
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = Space.Sheet.dp)
                .padding(bottom = Space.Lg.dp),
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
 * The Finish review shown in place of the control row after Stop: the title, then a
 * scrolling summary (a route thumbnail with start and finish markers, the paused
 * run's distance · moving time · average pace as three large figures, the moving
 * time taken from the recorded track as the core will derive it so History repeats
 * these figures, a short-run notice when the run is under the keep threshold
 * ([isShortRun])), then the optional whole-session effort and run type. The action
 * row stays pinned: Discard · Resume · Save run, with the primary action outermost
 * bottom-right and Resume a text action, so no tonal slab sits beside the primary.
 * [enabled] gates the actions while a save is in flight. Discard only opens the
 * confirm (see [finishTransition]).
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
    val liveKm by RunSession.distanceKm.collectAsStateWithLifecycle()
    // Moving time from the very track Save hands the core, on its whole-second basis,
    // so the review's time and pace are the figures History then lists.
    val points by RunSession.points.collectAsStateWithLifecycle()
    val elapsedSec = remember(points) {
        recordedMovingSec(RunSession.decimatedTrackForCore(), RunSession.decimatedSegmentStarts())
    }
    val route = remember(points) {
        trackSegments(RunSession.decimatedTrackForCore(), RunSession.decimatedSegmentStarts())
    }
    val avgPace = averagePaceMinPerKm(elapsedSec, liveKm)
    val paceFigure = if (avgPace.isFinite()) formatPaceMinutes(paceInUnit(avgPace, unit)) else NO_VALUE
    val spoken = stringResource(
        R.string.track_finish_summary,
        runDistLabel(liveKm, unit),
        formatElapsed(elapsedSec),
        if (avgPace.isFinite()) "$paceFigure ${unit.paceSuffix}" else stringResource(R.string.track_cd_not_available),
    )
    Column(
        modifier = modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        Text(
            stringResource(R.string.track_finish_title),
            color = OnBgBody,
            style = Type.Title.copy(fontWeight = FontWeight.ExtraBold),
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
                    RouteMapCard(route, entryId = 0L, height = FinishRouteHeight, distanceMarkers = false)
                }
                Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .clearAndSetSemantics { contentDescription = spoken },
                        horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
                    ) {
                        LiveFigure(
                            label = stringResource(R.string.track_stat_distance),
                            value = distanceFigure(liveKm, unit),
                            suffix = unit.distanceLabel,
                            known = liveKm > 0.0,
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
                            label = stringResource(R.string.track_finish_avg_pace),
                            value = paceFigure,
                            suffix = unit.paceSuffix,
                            known = avgPace.isFinite(),
                            maxSp = LIVE_FIGURE_MAX_SP,
                            modifier = Modifier.weight(1f),
                        )
                    }
                    // A run under the keep threshold says so here, inline: Save keeps
                    // it, Discard drops it, and the effort/type picks are never lost to
                    // a dialog. The threshold reads as a round figure ("0.3 mi").
                    if (isShortRun(liveKm, elapsedSec)) {
                        InlineNotice(
                            stringResource(
                                R.string.track_short_run_note,
                                runDistLabel(liveKm, unit),
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
            ControlCircle(
                R.drawable.ic_ui_close,
                stringResource(R.string.track_cd_discard_run),
                danger = true,
                enabled = enabled,
                iconSize = 20.dp,
                onClick = onDiscard,
            )
            TextAction(
                stringResource(R.string.track_finish_resume),
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
