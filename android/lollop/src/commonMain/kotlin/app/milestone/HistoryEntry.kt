package app.milestone

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

/**
 * The event that logs a deleted entry again, for "Undo": the same entry_id,
 * observed_at and figures the view shows (the core re-derives everything else:
 * a typed bpm's %HRmax against the profile in force on the run's day, the spike
 * baseline from the runs around it). A GPS run is rebuilt from its [segments]
 * (the core's route fixes, [DurableCore.runTrack]: the route, or for a run QC
 * left unmeasurable its recorded fixes), so it is null for a GPS run whose fixes
 * haven't loaded; the caller then confirms the delete instead of offering Undo.
 */
internal fun restoreEventFor(entry: HistoryEntry, segments: List<List<GpsPoint>>): Event? = when (entry) {
    is HistoryEntry.Lift -> entry.v.let { v ->
        Event.LogSet(
            exercise = v.exercise, weightKg = v.weight_kg, reps = v.reps, rpe = v.rpe,
            observedAt = v.observed_at, entryId = v.entry_id, holdSec = v.hold_sec,
        )
    }
    is HistoryEntry.Run -> entry.v.let { v ->
        if (!v.has_gpx && !v.gps_unmeasurable) {
            Event.LogRun(
                distanceKm = v.distance_km, durationMin = v.duration_min, hrPctMax = loggedHrPctMax(v),
                observedAt = v.observed_at, entryId = v.entry_id,
                workoutType = v.workout_type, sessionRpe = v.session_rpe,
                avgHrBpm = v.avg_hr_bpm, utcOffsetSec = v.utc_offset_sec,
            )
        } else {
            val points = ArrayList<GpsPoint>()
            val starts = ArrayList<Int>()
            for (seg in segments) {
                if (seg.isEmpty()) continue
                if (points.isNotEmpty()) starts += points.size
                points += seg
            }
            if (points.size < 2) {
                null
            } else {
                Event.LogRunTrack(
                    points = points, hrPctMax = loggedHrPctMax(v),
                    observedAt = v.observed_at, entryId = v.entry_id,
                    workoutType = v.workout_type, segmentStarts = starts,
                    sessionRpe = v.session_rpe, avgHrBpm = v.avg_hr_bpm, utcOffsetSec = v.utc_offset_sec,
                )
            }
        }
    }
}

/** The `hr_pct_max` a run was logged with: a bpm entry sends 0 and lets the core
 *  derive the percentage, so re-sending the derived figure would read as typed. */
internal fun loggedHrPctMax(v: RunResultView): Double = if (v.avg_hr_bpm != null) 0.0 else v.hr_pct_max
