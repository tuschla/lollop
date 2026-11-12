package app.milestone

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Text
import androidx.compose.material3.HorizontalDivider
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import app.milestone.resources.Res
import app.milestone.resources.history_delete_set
import app.milestone.resources.history_delete_set_message
import app.milestone.resources.history_detail_with_date
import app.milestone.resources.history_e1rm_range
import app.milestone.resources.history_edit_set
import app.milestone.resources.history_pct_e1rm
import app.milestone.resources.history_run_deleted
import app.milestone.resources.history_run_updated
import app.milestone.resources.history_set_deleted
import app.milestone.resources.history_set_e1rm_label
import app.milestone.resources.history_set_load_label
import app.milestone.resources.history_set_updated
import app.milestone.resources.ic_ui_delete
import app.milestone.resources.ic_ui_edit
import app.milestone.resources.log_undo
import app.milestone.resources.log_value_with_unit
import app.milestone.resources.main_delete
import app.milestone.resources.main_delete_set_title
import app.milestone.resources.main_set
import org.jetbrains.compose.resources.painterResource
import org.jetbrains.compose.resources.stringResource


/**
 * The row an edit [saved] from the edit sheet replaces, for its "Undo"
 * ([sendUndoable]); null for anything that isn't an amend.
 */
internal fun undoEditQuery(entry: HistoryEntry, saved: Event): RelogQuery? = when (saved) {
    is Event.AmendSet -> relogQueryOf(entry, saved.observedAt)
    is Event.AmendRun -> relogQueryOf(entry, saved.observedAt)
    else -> null
}


/**
 * The entry detail → edit/delete flow. Tapping a History card sets
 * [selected]; this hosts the detail bottom sheet, the pre-filled keypad editor
 * (emitting AmendSet/AmendRun) and the run's trim sheet. Each detail sheet confirms
 * a delete itself; the delete then lands with a "deleted · Undo" message, and an
 * edit save says "updated · Undo".
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun EntryActionSheets(
    selected: HistoryEntry?,
    editorInputs: SetEditorInputs,
    weightUnit: WeightUnit,
    // Whether the core converts a run's average bpm ([ViewModel.hr_max_known]).
    hrMaxKnown: Boolean,
    // The caller's ticking wall clock, so a sheet left open across midnight
    // re-labels "Today" instead of freezing at its composition time.
    nowMillis: Long,
    onDismiss: () -> Unit,
    onEvent: (Event) -> Unit,
    // The weekly session-effort load, for the evidence behind a rated run's "?"
    // (grade + citation). Null until a run is rated.
    srpeLoad: SrpeLoadView?,
) {
    if (selected == null) return
    val undoLabel = stringResource(Res.string.log_undo)
    // Saveable so a rotation mid-edit keeps the editor open (keyed on the
    // selected entry, so picking a different entry resets these).
    var editing by rememberSaveable(selected) { mutableStateOf(false) }
    var trimming by rememberSaveable(selected) { mutableStateOf(false) }
    val route = (selected as? HistoryEntry.Run)?.let { rememberRunRoute(it.v) }
    val isRun = selected is HistoryEntry.Run
    val deletedText = stringResource(if (isRun) Res.string.history_run_deleted else Res.string.history_set_deleted)
    val updatedText = stringResource(if (isRun) Res.string.history_run_updated else Res.string.history_set_updated)
    val saveEdit: (Event) -> Unit = { ev ->
        sendUndoable(onEvent, listOf(ev), listOfNotNull(undoEditQuery(selected, ev)), updatedText, undoLabel)
        onDismiss()
    }

    val editGuard = remember(selected) { EditorGuard() }

    if (editing) {
        EditorSheet(editGuard, onClosed = { editing = false }) {
            // The editor scrolls its own form and pins Save in thumb reach, as in the
            // Log sheet, so Save sits in one place for a new and an edited entry.
            Column(
                modifier = Modifier.fillMaxWidth().sheetPadding(),
                verticalArrangement = Arrangement.spacedBy(Space.Md.dp + Space.Xs.dp),
            ) {
              CompositionLocalProvider(LocalEditorPinnedActions provides true) {
                when (selected) {
                    is HistoryEntry.Lift -> LogSetEditor(
                        inputs = editorInputs,
                        initial = selected.v,
                        weightUnit = weightUnit,
                        onClose = { editing = false },
                        onSave = saveEdit,
                    )
                    is HistoryEntry.Run -> LogRunEditor(
                        initial = selected.v,
                        hrMaxKnown = hrMaxKnown,
                        onClose = { editing = false },
                        onSave = saveEdit,
                    )
                }
              }
            }
        }
    } else if (trimming && selected is HistoryEntry.Run) {
        val runView = selected.v
        // Opens only on a loaded route (the detail sheet's, else once a recreated
        // screen has reloaded it), so its body is final in its first frame.
        val loaded = route
        // Fully expanded: the route map + slider + Trim shouldn't strand below a
        // partial detent.
        if (loaded != null) AppSheet(onDismiss = { trimming = false }) {
            TrimRunSheetContent(
                run = runView,
                route = loaded,
                onCancel = { trimming = false },
                onConfirm = { keepFromSec, keepUntilSec ->
                    onEvent(
                        Event.TrimRunTrack(
                            entryId = runView.entry_id,
                            observedAtFallback = runView.observed_at,
                            keepFromSec = keepFromSec,
                            keepUntilSec = keepUntilSec,
                        ),
                    )
                    // The run identity (entry_id, observed_at) is unchanged by a
                    // trim, so `selected` re-resolves to the SAME key with refreshed
                    // data: drop the trim sheet back to the (now refreshed) detail.
                    trimming = false
                },
            )
        }
    } else if (selected !is HistoryEntry.Run || !selected.v.has_gpx || route != null) {
        // A GPS run's sheet opens once its route has loaded (map, Trim), so its body
        // is final in its first frame. Beside the list on a two-pane window.
        DetailSheet(onDismiss = onDismiss) {
            val onDelete: () -> Unit = {
                sendUndoable(onEvent, listOf(deleteEventFor(selected)), listOf(relogQueryOf(selected)), deletedText, undoLabel)
                onDismiss()
            }
            when (selected) {
                is HistoryEntry.Run -> RunDetailContent(
                    run = selected.v,
                    route = route,
                    srpeLoad = srpeLoad,
                    onEdit = { editing = true },
                    onTrim = { trimming = true },
                    onDelete = onDelete,
                    onEvent = onEvent,
                )
                is HistoryEntry.Lift -> EntryDetailContent(
                    entry = selected,
                    weightUnit = weightUnit,
                    nowMillis = nowMillis,
                    onEdit = { editing = true },
                    onDelete = onDelete,
                )
            }
        }
    }
}

@Composable
private fun EntryDetailContent(
    entry: HistoryEntry.Lift,
    weightUnit: WeightUnit,
    nowMillis: Long,
    onEdit: () -> Unit,
    onDelete: () -> Unit,
) {
    val v = entry.v
    val res = currentResources()
    Column(
        modifier = Modifier.sheetBody(rememberScrollState()),
        verticalArrangement = Arrangement.spacedBy(Space.Card.dp),
    ) {
        val exercise = v.exercise.ifBlank { stringResource(Res.string.main_set) }
        val date = formatLogDate(v.observed_at, nowMillis)
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
        ) {
            Box(Modifier.weight(1f)) {
                TileOverline(if (date.isBlank()) exercise else stringResource(Res.string.history_detail_with_date, exercise, date))
            }
            if (v.e1rm_pr) NewBestChip(v.exercise)
        }
        Column(verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
            Text(
                liftSetLine(res, v.hold_sec, v.weight_kg, v.reps, v.rpe, rpeRecorded = false, weightUnit = weightUnit),
                color = OnBgBody,
                style = Type.Display.merge(TabularFigures),
            )
            if (v.hold_sec == null && v.rpe_recorded) {
                Text(repsLeftText(res, v.rpe), color = OnBgMuted, style = Type.Caption.merge(TabularFigures))
            }
        }
        // A timed hold carries no e1RM or load share (the core zeroes them): no card,
        // never a fabricated "0 kg e1RM · 0% of e1RM".
        if (v.hold_sec == null && v.e1rm_kg > 0.0) {
            PlainCard(contentPadding = PaddingValues(horizontal = Space.Card.dp), spacing = 0.dp) {
                KeyValueRow(
                    stringResource(Res.string.history_set_e1rm_label),
                    stringResource(Res.string.log_value_with_unit, weightFigure(v.e1rm_kg, weightUnit), weightSuffix(weightUnit)),
                )
                if (v.pct_1rm > 0.0) {
                    HorizontalDivider(color = Hairline)
                    KeyValueRow(
                        stringResource(Res.string.history_set_load_label),
                        stringResource(Res.string.history_pct_e1rm, v.pct_1rm.roundOrZero().toInt()),
                    )
                }
            }
        }
        // strength-006 e1RM cross-check for THIS set: the multi-formula range
        // (Epley/Brzycki/Lombardi), widened to take in the e1RM shown above, so the one
        // figure on the sheet always sits inside its own range. Absent when the
        // estimate is unreliable (0 or >10 reps, or an isolation lift): the core omits
        // it rather than show a false range.
        v.cross_check?.let { cc ->
            val low = if (v.e1rm_kg > 0.0) minOf(cc.low_kg, v.e1rm_kg) else cc.low_kg
            val high = if (v.e1rm_kg > 0.0) maxOf(cc.high_kg, v.e1rm_kg) else cc.high_kg
            EvidenceCard(
                stringResource(
                    Res.string.history_e1rm_range,
                    weightFigure(low, weightUnit), weightFigure(high, weightUnit),
                    weightSuffix(weightUnit), cc.formulas,
                ),
                cc.evidence,
                border = if (cc.safety_critical) null else SheetCardBorder,
            )
        }
        // Session-level audit + longitudinal trend live on the lift SESSION sheet
        // (LiftSessionSheet), the newest session's own detail, not on single sets.
        ActionRows(
            rows = listOf(
                ActionRowItem(
                    stringResource(Res.string.history_edit_set),
                    painterResource(Res.drawable.ic_ui_edit),
                    onClick = onEdit,
                    testTag = "set-detail-edit",
                ),
            ),
            destructive = DestructiveAction(
                label = stringResource(Res.string.history_delete_set),
                icon = painterResource(Res.drawable.ic_ui_delete),
                confirmTitle = stringResource(Res.string.main_delete_set_title),
                confirmMessage = stringResource(Res.string.history_delete_set_message),
                confirmLabel = stringResource(Res.string.main_delete),
                onConfirm = onDelete,
                testTag = "set-detail-delete",
            ),
        )
    }
}
