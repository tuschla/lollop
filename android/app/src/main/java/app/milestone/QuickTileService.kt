package app.milestone

import android.annotation.SuppressLint
import android.content.ComponentName
import android.content.Context
import android.graphics.drawable.Icon
import android.os.Build
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import android.util.Log
import androidx.core.content.edit
import app.milestone.widget.WidgetDayKind
import app.milestone.widget.WidgetMidnight
import app.milestone.widget.WidgetSnapshot
import app.milestone.widget.WidgetSnapshotStore
import kotlinx.coroutines.flow.withIndex
import kotlinx.coroutines.launch

/** What the tile shows and opens. */
internal enum class TileFace(val label: Int, val icon: Int, val dest: OpenDest, val active: Boolean) {
    /** A GPS run is recording: the tile is lit and reopens the tracker. */
    Recording(R.string.tile_run_recording, R.drawable.ic_content_track_run, OpenDest.Track, true),

    /** Today's open session is a run: the tracker's pre-start screen. */
    StartRun(R.string.tile_start_run, R.drawable.ic_content_track_run, OpenDest.Track, false),

    /** Today's open session is a lift: the set editor at the next plan item. */
    LogSet(R.string.tile_log_set, R.drawable.ic_content_set_dumbbell, OpenDest.LogSet, false),

    /** Anything else (rest day, session done, hold, no plan, no snapshot): the Log chooser. */
    Log(R.string.tile_log, R.drawable.ic_action_log_plus, OpenDest.Log, false),
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

/**
 * Quick Settings tile: one tap to today's next action. It reads only the widget's
 * day snapshot file and the in-process [RunSession] flags, so showing it never
 * boots the core.
 *
 * An active tile (manifest meta-data `ACTIVE_TILE`): System UI redraws it only
 * when the app asks ([requestTileRefresh]) or on a tap, so the app asks whenever
 * the face can change: a run starts or stops ([RecordingWatch]), the day snapshot is
 * written or marked stale ([WidgetSnapshotStore.write]), and the day changes
 * (local midnight, clock or time zone change in
 * [app.milestone.widget.TodayWidgetReceiver]).
 *
 * A tap opens the app ([openIntent]); it never starts [RunTrackingService] itself,
 * since a location foreground service may not start from the background. "Start
 * run" opens the tracker, where the user starts the run. The tap goes through
 * [onClick], not [Tile.setActivityLaunchForClick]: the click must bind the service
 * so the face is redrawn.
 */
class QuickTileService : TileService() {

    override fun onTileAdded() {
        super.onTileAdded()
        setAdded(this, true)
        // The face rolls over at local midnight like the widget's.
        WidgetMidnight.schedule(this)
        // Draw now (a listening request from here can be ignored), or System UI
        // shows the manifest label until the next trigger.
        draw()
    }

    override fun onTileRemoved() {
        super.onTileRemoved()
        setAdded(this, false)
        if (!WidgetMidnight.hasWidgets(this)) WidgetMidnight.cancel(this)
    }

    override fun onStartListening() {
        super.onStartListening()
        draw()
        // Cleared app data forgets onTileAdded; a tile System UI draws is on the panel.
        if (!isAdded(this)) setAdded(this, true)
        // A reboot clears alarms; re-arm the midnight tick (replaces any earlier one).
        WidgetMidnight.schedule(this)
    }

    private fun draw() {
        val tile = qsTile ?: return
        val face = currentFace(this)
        tile.label = getString(face.label)
        tile.contentDescription = getString(face.label)
        tile.icon = Icon.createWithResource(this, face.icon)
        tile.state = if (face.active) Tile.STATE_ACTIVE else Tile.STATE_INACTIVE
        tile.updateTile()
    }

    override fun onClick() {
        super.onClick()
        // Read again: the run may have stopped since the tile was drawn.
        val dest = currentFace(this).dest
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            // System UI starts the PendingIntent itself and asks for the PIN on the lock
            // screen (startPendingIntentDismissingKeyguard). An unlockAndRun around it asks
            // twice: the launch arrives while the keyguard goes away and is dropped.
            startActivityAndCollapse(openPendingIntent(this, dest))
        } else if (isLocked) {
            // Below 34 the service starts the Intent itself, which would stay under the keyguard.
            unlockAndRun { openBelow34(dest) }
        } else {
            openBelow34(dest)
        }
    }

    // The Intent overload; API 34+ only accepts the PendingIntent one (this one throws
    // for apps targeting 34+).
    @SuppressLint("StartActivityAndCollapseDeprecated")
    private fun openBelow34(dest: OpenDest) {
        @Suppress("DEPRECATION")
        startActivityAndCollapse(openIntent(this, dest))
    }

    companion object {
        private const val PREFS = "quick_tile"
        private const val KEY_ADDED = "added"

        /** The face for the current state: a recording run first (no file read), then the snapshot. */
        internal fun currentFace(context: Context): TileFace =
            if (RunSession.tracking.value) TileFace.Recording else tileFace(WidgetSnapshotStore.today(context))

        /** Whether the tile is on the user's Quick Settings panel (it then needs the midnight tick). */
        internal fun isAdded(context: Context): Boolean =
            context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getBoolean(KEY_ADDED, false)

        private fun setAdded(context: Context, added: Boolean) {
            context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit { putBoolean(KEY_ADDED, added) }
        }
    }
}

/**
 * Ask System UI to bind the tile and call [QuickTileService.onStartListening], which
 * redraws it. Does nothing visible when the tile is not on the panel. A failure is
 * logged and dropped: a missed redraw must never break the run or the snapshot write
 * that asked for it, and the next trigger or tap redraws the tile.
 */
internal fun requestTileRefresh(ctx: Context) {
    val app = ctx.applicationContext
    try {
        TileService.requestListeningState(app, ComponentName(app, QuickTileService::class.java))
    } catch (e: RuntimeException) {
        Log.w("QuickTile", "tile refresh not requested", e)
    }
}

/**
 * Follows [RunSession.tracking], whoever changes it: redraws the tile on every
 * change ([TileFace.Recording]) and takes a shown run reminder down when a run
 * starts recording ([SessionReminder.onRecording]). Process-scoped, started from
 * MainActivity and [RunTrackingService]; a later call only replaces the context
 * (Robolectric gives each test a new Application under the same collector).
 */
internal object RecordingWatch {
    @Volatile private var started = false
    @Volatile private var app: Context? = null

    fun start(context: Context) {
        app = context.applicationContext
        if (started) return
        started = true
        EventDispatcher.scope.launch {
            RunSession.tracking.withIndex().collect { (i, recording) ->
                val app = app ?: return@collect
                if (i > 0) requestTileRefresh(app)
                if (recording) SessionReminder.onRecording(app)
            }
        }
    }
}
