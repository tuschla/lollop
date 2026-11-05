package app.milestone

import app.milestone.resources.Res
import app.milestone.resources.tile_log
import app.milestone.resources.tile_log_set
import app.milestone.resources.tile_run_recording
import app.milestone.resources.tile_start_run
import app.milestone.widget.WidgetDayKind
import app.milestone.widget.WidgetSnapshot
import app.milestone.widget.widgetFace
import org.jetbrains.compose.resources.StringResource

/**
 * What the one-tap system action shows and opens: Android's Quick Settings tile
 * and the widget's primary button, iOS's Control Center control and the widget's
 * button. Each platform draws its own icon for a face (Android: `TileFace.icon`,
 * QuickTileService.kt; iOS: the widget extension's symbols).
 */
internal enum class TileFace(val label: StringResource, val dest: OpenDest, val active: Boolean) {
    /** A GPS run is recording: the tile is lit and reopens the tracker. */
    Recording(Res.string.tile_run_recording, OpenDest.Track, true),

    /** Today's open session is a run: the tracker's pre-start screen. */
    StartRun(Res.string.tile_start_run, OpenDest.Track, false),

    /** Today's open session is a lift: the set editor at the next plan item. */
    LogSet(Res.string.tile_log_set, OpenDest.LogSet, false),

    /** Anything else (rest day, session done, hold, no plan, no snapshot): the Log chooser. */
    Log(Res.string.tile_log, OpenDest.Log, false),
}

/**
 * The tile for today's widget [snapshot] (null when there is none for today, or
 * it is stale). Start run needs an open run item today ([WidgetSnapshot.trackable]);
 * Log set needs today's lift session still open. A finished session, a hold
 * ([WidgetDayKind.Paused]), a rest day and no plan all fall back to Log.
 */
internal fun tileFace(snapshot: WidgetSnapshot?): TileFace = when {
    snapshot == null -> TileFace.Log
    snapshot.kind == WidgetDayKind.Run && snapshot.trackable -> TileFace.StartRun
    snapshot.kind == WidgetDayKind.Lift && !snapshot.sessionDone -> TileFace.LogSet
    else -> TileFace.Log
}

/** The line under the tile's label for [today]'s snapshot: the widget's title
 *  (the session, "Rest day", "Training paused"), "Session done" once today's
 *  session is; nothing without a snapshot for today. */
internal fun tileSubtitle(today: WidgetSnapshot?, res: PlatformResources): String? {
    val snapshot = today ?: return null
    val face = widgetFace(snapshot, snapshot.epochDay, res)
    return (if (snapshot.sessionDone) face.status else face.title)?.takeIf { it.isNotBlank() }
}
