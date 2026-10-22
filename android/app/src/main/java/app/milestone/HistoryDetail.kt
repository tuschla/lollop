package app.milestone

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.SheetValue
import androidx.compose.material3.Text
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp

/** The [Event.DeleteEntry] for a logged entry: by entry_id, with observed_at as
 *  the legacy fallback the core uses when the id is 0. */
internal fun deleteEventFor(entry: HistoryEntry): Event = when (entry) {
    is HistoryEntry.Lift -> Event.DeleteEntry(
        Event.EntryKind.Set, entry.v.entry_id, observedAtFallback = entry.v.observed_at,
    )
    is HistoryEntry.Run -> Event.DeleteEntry(
        Event.EntryKind.Run, entry.v.entry_id, observedAtFallback = entry.v.observed_at,
    )
}

/**
 * The event that logs a deleted entry again, for "Undo": the same entry_id,
 * observed_at and figures the view shows (the core re-derives everything else). A
 * GPS run is rebuilt from its route [segments] (parsed from the core's own GPX), so
 * it is null for a GPS run whose route hasn't loaded; the caller then confirms the
 * delete instead of offering Undo.
 */
internal fun restoreEventFor(entry: HistoryEntry, segments: List<List<GpxFix>>): Event? = when (entry) {
    is HistoryEntry.Lift -> entry.v.let { v ->
        Event.LogSet(
            exercise = v.exercise, weightKg = v.weight_kg, reps = v.reps, rpe = v.rpe,
            observedAt = v.observed_at, entryId = v.entry_id, holdSec = v.hold_sec,
        )
    }
    is HistoryEntry.Run -> entry.v.let { v ->
        if (!v.has_gpx) {
            Event.LogRun(
                distanceKm = v.distance_km, durationMin = v.duration_min, hrPctMax = loggedHrPctMax(v),
                longestRecentKm = 0.0, observedAt = v.observed_at, entryId = v.entry_id,
                workoutType = WorkoutType.fromWire(v.workout_type), sessionRpe = v.session_rpe,
                avgHrBpm = v.avg_hr_bpm,
            )
        } else {
            val points = ArrayList<GpsPoint>()
            val starts = ArrayList<Int>()
            for (seg in segments) {
                if (seg.isEmpty()) continue
                if (points.isNotEmpty()) starts += points.size
                // The exported route already passed the core's QC, so a fix without
                // a recorded accuracy takes the QC-passing import sentinel.
                seg.mapTo(points) { GpsPoint(it.lat, it.lon, it.timeSec, it.accuracyM ?: IMPORT_UNKNOWN_ACCURACY_M) }
            }
            if (points.size < 2) {
                null
            } else {
                Event.LogRunTrack(
                    points = points, hrPctMax = loggedHrPctMax(v), longestRecentKm = 0.0,
                    observedAt = v.observed_at, entryId = v.entry_id,
                    workoutType = WorkoutType.fromWire(v.workout_type), segmentStarts = starts,
                    sessionRpe = v.session_rpe, avgHrBpm = v.avg_hr_bpm,
                )
            }
        }
    }
}

/** The `hr_pct_max` a run was logged with: a bpm entry sends 0 and lets the core
 *  derive the percentage, so re-sending the derived figure would read as typed. */
internal fun loggedHrPctMax(v: RunResultView): Double = if (v.avg_hr_bpm != null) 0.0 else v.hr_pct_max

/**
 * The amend that puts an edited entry back as it was, for "Undo" after an edit
 * [saved] from the edit sheet; null for anything that isn't an amend. A legacy
 * (entry_id 0) row is matched on the stamp the edit gave it.
 */
internal fun undoEditFor(entry: HistoryEntry, saved: Event): Event? = when {
    entry is HistoryEntry.Lift && saved is Event.AmendSet -> entry.v.let { v ->
        Event.AmendSet(
            entryId = v.entry_id, exercise = v.exercise, weightKg = v.weight_kg, reps = v.reps, rpe = v.rpe,
            observedAt = v.observed_at, observedAtFallback = saved.observedAt, holdSec = v.hold_sec,
        )
    }
    entry is HistoryEntry.Run && saved is Event.AmendRun -> entry.v.let { v ->
        Event.AmendRun(
            entryId = v.entry_id, distanceKm = v.distance_km, durationMin = v.duration_min,
            hrPctMax = loggedHrPctMax(v), observedAt = v.observed_at, observedAtFallback = saved.observedAt,
            workoutType = WorkoutType.fromWire(v.workout_type), sessionRpe = v.session_rpe,
            avgHrBpm = v.avg_hr_bpm,
        )
    }
    else -> null
}

/** "Run deleted · Undo" (or "Set updated · Undo"…): the confirmation for a change made
 *  in History; Undo re-sends [undo] in order through the process-scoped dispatcher.
 *  Nothing to undo → the plain confirmation. */
internal fun postUndoable(text: String, undoLabel: String, undo: List<Event>) {
    UiMessages.post(
        if (undo.isEmpty()) {
            UiMessage(text)
        } else {
            UiMessage(text, actionLabel = undoLabel, onAction = { undo.forEach(EventDispatcher::send) })
        },
    )
}

/**
 * The entry detail → edit/delete flow. Tapping a History or Today card sets
 * [selected]; this hosts the detail bottom sheet, the pre-filled keypad editor
 * (emitting AmendSet/AmendRun) and the run's trim sheet. Each detail sheet confirms
 * a delete itself; the delete then lands with a "deleted · Undo" message (Undo is
 * left off only for a GPS run whose route never loaded, so it can't be rebuilt), and
 * an edit save says "updated · Undo".
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun EntryActionSheets(
    selected: HistoryEntry?,
    recentExercises: List<String>,
    holdExercises: Set<String> = emptySet(),
    weightUnit: WeightUnit = WeightUnit.Kg,
    // The caller's ticking wall clock, so a sheet left open across midnight
    // re-labels "Today" instead of freezing at its composition time.
    nowMillis: Long,
    onDismiss: () -> Unit,
    onEvent: (Event) -> Unit,
    // The weekly session-effort load, for the evidence behind a rated run's "?"
    // (grade + citation). Null until a run is rated.
    srpeLoad: SrpeLoadView? = null,
) {
    if (selected == null) return
    val ctx = LocalContext.current
    val undoLabel = stringResource(R.string.log_undo)
    // Saveable so a rotation mid-edit keeps the editor open (keyed on the
    // selected entry, so picking a different entry resets these).
    var editing by rememberSaveable(selected) { mutableStateOf(false) }
    var trimming by rememberSaveable(selected) { mutableStateOf(false) }
    val deletedText = ctx.getString(
        if (selected is HistoryEntry.Run) R.string.history_run_deleted else R.string.history_set_deleted,
    )
    val saveEdit: (Event) -> Unit = { ev ->
        onEvent(ev)
        onDismiss()
        postUndoable(
            ctx.getString(if (selected is HistoryEntry.Run) R.string.history_run_updated else R.string.history_set_updated),
            undoLabel,
            listOfNotNull(undoEditFor(selected, ev)),
        )
    }

    // Edit-sheet hardening, mirroring the Log sheet host: hoist the editor's dirty
    // state so the sheet's swipe-down honors the same discard guard the editor's
    // X enforces. Keyed on `selected` so switching entries resets it.
    var editDirty by rememberSaveable(selected) { mutableStateOf(false) }
    var confirmDiscardEdit by rememberSaveable { mutableStateOf(false) }
    val editSheetState = rememberModalBottomSheetState(
        // Open fully expanded so Save isn't stranded below the partial detent.
        skipPartiallyExpanded = true,
        // A dirty editor blocks the swipe-to-hide and raises the discard confirm
        // INSTEAD, keeping the sheet VISIBLE under the dialog; "Keep editing"
        // leaves it in place. Discard/Save drop it via `editing = false`.
        confirmValueChange = { target ->
            if (target == SheetValue.Hidden && editDirty) {
                confirmDiscardEdit = true
                false
            } else {
                true
            }
        },
    )

    if (editing) {
        ModalBottomSheet(
            // The dirty-editor guard lives in editSheetState.confirmValueChange
            // (which keeps the sheet visible while confirming); by the time this
            // fires the hide is committed on a clean sheet, so just finish closing.
            onDismissRequest = { editDirty = false; editing = false },
            sheetState = editSheetState,
            // The Log sheet's ground, since the same editors render on it.
            containerColor = BgSunken,
        ) {
            SheetNavBarScrim()
            // The editor scrolls its own form and pins Save in thumb reach, as in the
            // Log sheet, so Save sits in one place for a new and an edited entry.
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    // Grow the bottom inset with the keyboard so the exercise-search
                    // IME can't cover the form (mirrors LogSheetContent).
                    .imePadding()
                    .padding(horizontal = Space.Sheet.dp)
                    .padding(bottom = Space.Lg.dp),
                verticalArrangement = Arrangement.spacedBy(Space.Md.dp + Space.Xs.dp),
            ) {
              CompositionLocalProvider(LocalEditorPinnedActions provides true) {
                when (selected) {
                    is HistoryEntry.Lift -> LogSetEditor(
                        recentExercises = recentExercises,
                        holdExercises = holdExercises,
                        initial = selected.v,
                        weightUnit = weightUnit,
                        onClose = { editDirty = false; editing = false },
                        onDirtyChange = { editDirty = it },
                        onSave = saveEdit,
                    )
                    is HistoryEntry.Run -> LogRunEditor(
                        initial = selected.v,
                        onClose = { editDirty = false; editing = false },
                        onDirtyChange = { editDirty = it },
                        onSave = saveEdit,
                    )
                }
              }
            }
        }
    } else if (trimming && selected is HistoryEntry.Run) {
        val runView = selected.v
        ModalBottomSheet(
            onDismissRequest = { trimming = false },
            containerColor = BgSunken,
            // Open fully expanded: the route map + slider + Trim shouldn't strand below a
            // partial detent (mirrors the edit sheet above).
            sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
        ) {
            SheetNavBarScrim()
            TrimRunSheetContent(
                run = runView,
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
    } else {
        // Fully expanded only: Back (and a swipe) close the sheet in one step instead
        // of first collapsing it to a half-height detent.
        ModalBottomSheet(
            onDismissRequest = onDismiss,
            containerColor = if (selected is HistoryEntry.Run) BgSunken else BgElevated,
            sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
        ) {
            SheetNavBarScrim()
            val onDelete: (Event?) -> Unit = { restore ->
                onEvent(deleteEventFor(selected))
                onDismiss()
                postUndoable(deletedText, undoLabel, listOfNotNull(restore))
            }
            when (selected) {
                is HistoryEntry.Run -> RunDetailContent(
                    run = selected.v,
                    nowMillis = nowMillis,
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

    // The swipe-down discard confirmation for a dirty edit sheet (the editor's
    // own X routes through EditorScaffold's identical guard).
    if (confirmDiscardEdit) {
        AlertDialog(
            onDismissRequest = { confirmDiscardEdit = false },
            shape = RoundedCornerShape(Space.Card.dp),
            title = { Text(stringResource(R.string.log_discard_title)) },
            confirmButton = {
                TextButton(onClick = {
                    confirmDiscardEdit = false
                    editDirty = false
                    editing = false
                }) { Text(stringResource(R.string.log_discard_confirm), color = DangerText) }
            },
            dismissButton = {
                TextButton(onClick = { confirmDiscardEdit = false }) {
                    Text(stringResource(R.string.log_keep_editing))
                }
            },
        )
    }
}

/**
 * Detail-sheet body for one logged set: an "EXERCISE · DATE" overline (with "New
 * best" end-aligned), the set as the sheet's largest figure with its reps left below,
 * a key/value card (e1RM, load as a share of it), the e1RM cross-check, then the
 * [ActionRows] footer: Edit set, and Delete set behind a confirm. [onDelete] gets the
 * event that would re-log the set ([restoreEventFor]), for the Undo after it.
 */
@Composable
private fun EntryDetailContent(
    entry: HistoryEntry.Lift,
    weightUnit: WeightUnit = WeightUnit.Kg,
    nowMillis: Long,
    onEdit: () -> Unit,
    onDelete: (restore: Event?) -> Unit,
) {
    val v = entry.v
    val res = LocalContext.current.resources
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .verticalScroll(rememberScrollState())
            .navigationBarsPadding()
            .padding(horizontal = Space.Sheet.dp)
            .padding(bottom = Space.Lg.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Card.dp),
    ) {
        val exercise = v.exercise.ifBlank { stringResource(R.string.main_set) }
        val date = formatLogDate(v.observed_at, nowMillis)
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
        ) {
            Box(Modifier.weight(1f)) {
                TileOverline(if (date.isBlank()) exercise else stringResource(R.string.history_detail_with_date, exercise, date))
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
                    stringResource(R.string.history_set_e1rm_label),
                    stringResource(R.string.log_value_with_unit, weightFigure(v.e1rm_kg, weightUnit), weightSuffix(weightUnit)),
                )
                if (v.pct_1rm > 0.0) {
                    HorizontalDivider(color = Hairline)
                    KeyValueRow(
                        stringResource(R.string.history_set_load_label),
                        stringResource(R.string.history_pct_e1rm, Math.round(v.pct_1rm).toInt()),
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
                    R.string.history_e1rm_range,
                    weightFigure(low, weightUnit), weightFigure(high, weightUnit),
                    weightSuffix(weightUnit), cc.formulas,
                ),
                cc.grade, cc.citation, cc.confidence, cc.safety_critical, cc.contested,
                // On the sheet's own BgElevated ground: a hairline keeps it a card.
                border = if (cc.safety_critical) null else OnBgFaint.copy(alpha = 0.25f),
            )
        }
        // Session-level audit + longitudinal trend live on the lift SESSION sheet
        // (LiftSessionSheet), the newest session's own detail, not on single sets.
        ActionRows(
            rows = listOf(
                ActionRowItem(
                    stringResource(R.string.history_edit_set),
                    painterResource(R.drawable.ic_ui_edit),
                    onClick = onEdit,
                    chevron = true,
                    testTag = "set-detail-edit",
                ),
            ),
            destructive = DestructiveAction(
                label = stringResource(R.string.history_delete_set),
                icon = painterResource(R.drawable.ic_ui_delete),
                confirmTitle = stringResource(R.string.main_delete_set_title),
                confirmMessage = stringResource(R.string.history_delete_set_message),
                confirmLabel = stringResource(R.string.main_delete),
                onConfirm = { onDelete(restoreEventFor(entry, emptyList())) },
                testTag = "set-detail-delete",
            ),
        )
    }
}
