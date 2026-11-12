package app.milestone

import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** A tapped History entry, routed to the detail sheet. */
internal sealed interface HistoryEntry {
    data class Lift(val v: LiftResultView) : HistoryEntry
    data class Run(val v: RunResultView) : HistoryEntry
}

internal fun deleteEventFor(entry: HistoryEntry): Event = when (entry) {
    is HistoryEntry.Lift -> Event.DeleteEntry(
        EntryKind.Set, entry.v.entry_id, observedAtFallback = entry.v.observed_at,
    )
    is HistoryEntry.Run -> Event.DeleteEntry(
        EntryKind.Run, entry.v.entry_id, observedAtFallback = entry.v.observed_at,
    )
}

/** The row [entry] names, for the core's re-log of it ([DurableCore.relogEvent]);
 *  [amendedAt] for an edit's Undo: the `observed_at` the edit saves it under. */
internal fun relogQueryOf(entry: HistoryEntry, amendedAt: Long? = null): RelogQuery = when (entry) {
    is HistoryEntry.Lift -> RelogQuery(EntryKind.Set, entry.v.entry_id, entry.v.observed_at, amendedAt)
    is HistoryEntry.Run -> RelogQuery(EntryKind.Run, entry.v.entry_id, entry.v.observed_at, amendedAt)
}

/**
 * Send [edits] (a delete, or an edit's amend) and say [text] with an "Undo" that
 * puts [rows] back as they are now. The core builds each Undo event from its own
 * row ([DurableCore.relogEvent]), read in one turn of the event queue before the
 * edits land; a row it has no event for gets no Undo.
 */
internal fun sendUndoable(
    onEvent: (Event) -> Unit,
    edits: List<Event>,
    rows: List<RelogQuery>,
    text: String,
    undoLabel: String,
) {
    if (rows.isEmpty()) {
        edits.forEach(onEvent)
        postUndoable(text, undoLabel, emptyList())
        return
    }
    AppProcess.scope.launch {
        var undo = emptyList<Event>()
        EventDispatcher.exclusive {
            undo = withContext(CoreDispatcher) { rows.mapNotNull(AppCore.engine::relogEvent) }
            null
        }
        edits.forEach(onEvent)
        postUndoable(text, undoLabel, undo)
    }
}

/** The `hr_pct_max` a run was logged with: a bpm entry sends 0 and lets the core
 *  derive the percentage, so re-sending the derived figure would read as typed. */
internal fun loggedHrPctMax(v: RunResultView): Double = if (v.avg_hr_bpm != null) 0.0 else v.hr_pct_max
