package app.milestone.widget

import app.milestone.OpenDest
import app.milestone.PlatformResources
import app.milestone.resources.Res
import app.milestone.resources.tile_log
import app.milestone.resources.tile_log_set
import app.milestone.resources.tile_run_recording
import app.milestone.resources.tile_start_run
import kotlinx.serialization.Serializable
import org.jetbrains.compose.resources.StringResource

/**
 * The icons the system surfaces draw: Android's drawables (`SurfaceIcon.drawable`,
 * androidMain) and the iOS widget extension's image sets `Icon<name>`, which
 * iosApp/make-assets.py renders from those drawables.
 */
@Serializable
internal enum class SurfaceIcon { TrackRun, LogSet, Log, Mark, Pause, Play, Stop }

/**
 * What the one-tap system action shows and opens: Android's Quick Settings tile
 * and the widget's primary button, iOS's Control Center control and the widget's
 * button, with its [icon]. A Control Center control shows SF Symbols only:
 * [iosSymbol].
 */
internal enum class TileFace(
    val label: StringResource,
    val dest: OpenDest,
    val active: Boolean,
    val icon: SurfaceIcon,
    val iosSymbol: String,
) {
    /** A GPS run is recording: the tile is lit and reopens the tracker. */
    Recording(Res.string.tile_run_recording, OpenDest.Track, true, SurfaceIcon.TrackRun, "location.fill"),

    /** Today's open session is a run: the tracker's pre-start screen. */
    StartRun(Res.string.tile_start_run, OpenDest.Track, false, SurfaceIcon.TrackRun, "location"),

    /** Today's open session is a lift: the set editor at the next plan item. */
    LogSet(Res.string.tile_log_set, OpenDest.LogSet, false, SurfaceIcon.LogSet, "dumbbell"),

    /** Anything else (rest day, session done, hold, no plan, no snapshot): the Log chooser. */
    Log(Res.string.tile_log, OpenDest.Log, false, SurfaceIcon.Log, "plus"),
}

/**
 * The tile for today's widget snapshot [today] (null when there is none for today,
 * or it is stale) while a GPS run records in this process ([recording]) or not. A
 * recording run wins. Start run needs an open run item today
 * ([WidgetSnapshot.trackable]); Log set needs today's lift session still open. A
 * finished session, a hold ([WidgetDayKind.Paused]), a rest day and no plan all
 * fall back to Log.
 */
internal fun tileFace(today: WidgetSnapshot?, recording: Boolean = false): TileFace = when {
    recording -> TileFace.Recording
    today == null -> TileFace.Log
    today.kind == WidgetDayKind.Run && today.trackable -> TileFace.StartRun
    today.kind == WidgetDayKind.Lift && !today.sessionDone -> TileFace.LogSet
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
