package app.milestone

import android.annotation.SuppressLint
import android.content.ComponentName
import android.content.Context
import android.graphics.drawable.Icon
import android.os.Build
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import app.milestone.widget.AndroidWidgets
import app.milestone.widget.MidnightTick
import app.milestone.widget.TileFace
import app.milestone.widget.WidgetSnapshotStore
import app.milestone.widget.drawable
import app.milestone.widget.tileFace
import app.milestone.widget.tileSubtitle

/**
 * Quick Settings tile: one tap to today's next action. It reads only the widget's
 * day snapshot file and the in-process [RunSession] flags, so showing it never
 * boots the core.
 *
 * An active tile (manifest meta-data `ACTIVE_TILE`): System UI redraws it only
 * when the app asks ([requestTileRefresh]) or on a tap, so the app asks whenever
 * the face can change ([AndroidWidgets]): a run starts or stops, the day snapshot
 * is written or marked stale, the day changes (local midnight, clock or time zone
 * change) and the language changes.
 *
 * A tap opens the app ([openIntent]); it never starts [RunTrackingService] itself,
 * since a location foreground service may not start from the background. "Start
 * run" opens the tracker, where the user starts the run. The tap goes through
 * [onClick], not [Tile.setActivityLaunchForClick]: the click must bind the service
 * so the face is redrawn.
 */
class QuickTileService : TileService() {
    override fun onCreate() {
        super.onCreate()
        AppRuntime.start(this)
    }

    override fun onTileAdded() {
        super.onTileAdded()
        MidnightTick.setTilePlaced(this, true)
        // Draw now (a listening request from here can be ignored), or System UI
        // shows the manifest label until the next trigger.
        draw()
    }

    override fun onTileRemoved() {
        super.onTileRemoved()
        MidnightTick.setTilePlaced(this, false)
    }

    override fun onStartListening() {
        super.onStartListening()
        draw()
        // A tile System UI draws is on the panel, though cleared app data forgets
        // onTileAdded; this also sets the midnight tick again after a reboot.
        MidnightTick.setTilePlaced(this, true)
    }

    private fun draw() {
        val tile = qsTile ?: return
        val snapshot = WidgetSnapshotStore.today(this)
        val face = tileFace(snapshot, RunSession.tracking.value)
        val res = appResources()
        val label = res.getString(face.label)
        val subtitle = tileSubtitle(snapshot, res)
        tile.label = label
        tile.contentDescription = listOfNotNull(label, subtitle).joinToString(", ")
        // The widget's title under the label (Android 10+), so tile and widget say
        // the same; without one System UI writes the state ("On", "Off") there.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) tile.subtitle = subtitle
        tile.icon = Icon.createWithResource(this, face.icon.drawable)
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
        /** The face for the current state. */
        internal fun currentFace(context: Context): TileFace =
            tileFace(WidgetSnapshotStore.today(context), RunSession.tracking.value)
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
        PlatformLog.w("QuickTile", "tile refresh not requested", e)
    }
}
