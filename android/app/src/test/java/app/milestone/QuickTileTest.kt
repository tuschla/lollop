package app.milestone

import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import androidx.test.core.app.ApplicationProvider
import app.milestone.widget.WidgetDayKind
import app.milestone.widget.WidgetSnapshot
import app.milestone.widget.WidgetSnapshotStore
import java.io.File
import java.lang.reflect.Proxy
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.Shadows.shadowOf

/** The Quick Settings tile: its face per day snapshot and run state, and what a tap opens. */
@RunWith(RobolectricTestRunner::class)
class QuickTileTest {

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    private fun snapshot(kind: WidgetDayKind, sessionDone: Boolean = false, trackable: Boolean = false, day: Long = todayEpochDay()) =
        WidgetSnapshot(epochDay = day, kind = kind, sessionDone = sessionDone, trackable = trackable)

    @Before
    fun setUp() {
        RunSession.reset()
        File(ctx.filesDir, WidgetSnapshotStore.FILE_NAME).delete()
    }

    @After
    fun tearDown() {
        RunSession.reset()
        File(ctx.filesDir, WidgetSnapshotStore.FILE_NAME).delete()
    }

    @Test
    fun faceFollowsTodayAndTheRecordingRun() {
        assertEquals(TileFace.Log, tileFace(null))
        assertEquals(TileFace.StartRun, tileFace(snapshot(WidgetDayKind.Run, trackable = true)))
        // A run day whose run is logged has nothing to track.
        assertEquals(TileFace.Log, tileFace(snapshot(WidgetDayKind.Run, sessionDone = true)))
        assertEquals(TileFace.LogSet, tileFace(snapshot(WidgetDayKind.Lift)))
        assertEquals(TileFace.Log, tileFace(snapshot(WidgetDayKind.Lift, sessionDone = true)))
        for (kind in listOf(WidgetDayKind.Rest, WidgetDayKind.Paused, WidgetDayKind.NoPlan, WidgetDayKind.Setup)) {
            assertEquals(kind.name, TileFace.Log, tileFace(snapshot(kind, trackable = true)))
        }
        // A recording run wins over any day.
        RunSession.tracking.value = true
        for (s in listOf(null, snapshot(WidgetDayKind.Lift), snapshot(WidgetDayKind.Paused))) {
            if (s == null) File(ctx.filesDir, WidgetSnapshotStore.FILE_NAME).delete() else WidgetSnapshotStore.write(ctx, s)
            assertEquals(TileFace.Recording, QuickTileService.currentFace(ctx))
        }
        assertEquals(OpenDest.Track, TileFace.Recording.dest)
        assertEquals(OpenDest.Track, TileFace.StartRun.dest)
        assertEquals(OpenDest.LogSet, TileFace.LogSet.dest)
        assertEquals(OpenDest.Log, TileFace.Log.dest)
        assertEquals(listOf(TileFace.Recording), TileFace.entries.filter { it.active })
    }

    private fun listen(): Tile {
        val service = Robolectric.buildService(QuickTileService::class.java).create().get()
        service.onStartListening()
        return service.qsTile
    }

    private fun assertTile(tile: Tile, label: Int, icon: Int, state: Int) {
        assertEquals(ctx.getString(label), tile.label.toString())
        assertEquals(ctx.getString(label), tile.contentDescription.toString())
        assertEquals(icon, tile.icon.resId)
        assertEquals(state, tile.state)
    }

    @Test
    fun tileShowsTheStoredSnapshotForToday() {
        // No snapshot yet (the app never ran): Log.
        assertTile(listen(), R.string.tile_log, R.drawable.ic_action_log_plus, Tile.STATE_INACTIVE)
        WidgetSnapshotStore.write(ctx, snapshot(WidgetDayKind.Run, trackable = true))
        assertTile(listen(), R.string.tile_start_run, R.drawable.ic_content_track_run, Tile.STATE_INACTIVE)
        WidgetSnapshotStore.write(ctx, snapshot(WidgetDayKind.Lift))
        assertTile(listen(), R.string.tile_log_set, R.drawable.ic_content_set_dumbbell, Tile.STATE_INACTIVE)
        // Yesterday's snapshot, or one a clock change marked stale, says nothing about today.
        WidgetSnapshotStore.write(ctx, snapshot(WidgetDayKind.Lift, day = todayEpochDay() - 1))
        assertTile(listen(), R.string.tile_log, R.drawable.ic_action_log_plus, Tile.STATE_INACTIVE)
        WidgetSnapshotStore.write(ctx, snapshot(WidgetDayKind.Lift))
        WidgetSnapshotStore.markStale(ctx, force = true)
        assertTile(listen(), R.string.tile_log, R.drawable.ic_action_log_plus, Tile.STATE_INACTIVE)
    }

    /** API 34: the recording face. The tile hands System UI no launch of its own, so a
     *  tap reaches [QuickTileService.onClick] (System UI binds the service for it). */
    @Test
    fun recordingRunLightsTheTile() {
        WidgetSnapshotStore.write(ctx, snapshot(WidgetDayKind.Lift))
        RunSession.tracking.value = true
        val recording = listen()
        assertTile(recording, R.string.tile_run_recording, R.drawable.ic_content_track_run, Tile.STATE_ACTIVE)
        assertNull(recording.activityLaunchForClick)
        RunSession.tracking.value = false
        val lift = listen()
        assertTile(lift, R.string.tile_log_set, R.drawable.ic_content_set_dumbbell, Tile.STATE_INACTIVE)
        assertNull(lift.activityLaunchForClick)
    }

    /** API 34: a tap on a face drawn before the day changed or a run started opens
     *  what the tile would show now. */
    @Test
    fun clickOnAStaleFaceOpensTheCurrentDestination() {
        WidgetSnapshotStore.write(ctx, snapshot(WidgetDayKind.Run, trackable = true))
        assertTile(listen(), R.string.tile_start_run, R.drawable.ic_content_track_run, Tile.STATE_INACTIVE)
        // The day moves on (the stored snapshot is now yesterday's); the tile is not redrawn.
        WidgetSnapshotStore.write(ctx, snapshot(WidgetDayKind.Run, trackable = true, day = todayEpochDay() - 1))
        assertOpens(click(), OpenDest.Log)
        RunSession.tracking.value = true
        assertOpens(click(), OpenDest.Track)
    }

    /** Below API 34 a tap starts [openIntent] from the service: it needs NEW_TASK (a
     *  service is no activity context) and opens the face's destination, locked or not. */
    @Test
    @Config(sdk = [33])
    fun clickOnApi33StartsTheOpenIntent() {
        val app = ctx as android.app.Application
        fun clickAndTake(locked: Boolean = false): Intent {
            val service = Robolectric.buildService(QuickTileService::class.java).create().get()
            shadowOf(service).setLocked(locked)
            service.onClick()
            return checkNotNull(shadowOf(app).nextStartedActivity) { "the tile started nothing" }
        }
        fun assertOpensIntent(intent: Intent, dest: OpenDest) {
            assertEquals(MainActivity::class.java.name, intent.component?.className)
            assertEquals(ACTION_OPEN, intent.action)
            assertEquals(dest.wire, intent.getStringExtra(EXTRA_DEST))
            assertEquals(Intent.FLAG_ACTIVITY_NEW_TASK, intent.flags and Intent.FLAG_ACTIVITY_NEW_TASK)
        }
        assertOpensIntent(clickAndTake(), OpenDest.Log)
        WidgetSnapshotStore.write(ctx, snapshot(WidgetDayKind.Lift))
        assertOpensIntent(clickAndTake(), OpenDest.LogSet)
        RunSession.tracking.value = true
        assertOpensIntent(clickAndTake(), OpenDest.Track)
        // On the lock screen the tap unlocks first, then opens the same destination.
        assertOpensIntent(clickAndTake(locked = true), OpenDest.Track)
        assertEquals(null, shadowOf(app).nextStartedService)
    }

    /** Click the tile on API 34 with a stand-in for the system's QS binder; returns the
     *  PendingIntent the tile handed to startActivityAndCollapse. */
    private fun click(): PendingIntent {
        val service = Robolectric.buildService(QuickTileService::class.java).create().get()
        val iface = Class.forName("android.service.quicksettings.IQSService")
        var started: PendingIntent? = null
        val qs = Proxy.newProxyInstance(iface.classLoader, arrayOf(iface)) { _, method, args ->
            if (method.name == "startActivity") started = args[1] as PendingIntent
            if (method.returnType == java.lang.Boolean.TYPE) false else null
        }
        TileService::class.java.getDeclaredField("mService").apply { isAccessible = true }.set(service, qs)
        service.onClick()
        return checkNotNull(started) { "the tile started nothing" }
    }

    private fun assertOpens(pending: PendingIntent, dest: OpenDest) {
        val shadow = shadowOf(pending)
        assertTrue(shadow.isActivityIntent)
        assertNotNull(shadow.savedIntent.component)
        assertEquals(MainActivity::class.java.name, shadow.savedIntent.component!!.className)
        assertEquals(ACTION_OPEN, shadow.savedIntent.action)
        assertEquals(dest.wire, shadow.savedIntent.getStringExtra(EXTRA_DEST))
        assertEquals(PendingIntent.FLAG_IMMUTABLE, shadow.flags and PendingIntent.FLAG_IMMUTABLE)
    }

    @Test
    fun clickOpensTheAppNeverTheTrackingService() {
        assertOpens(click(), OpenDest.Log)
        WidgetSnapshotStore.write(ctx, snapshot(WidgetDayKind.Lift))
        assertOpens(click(), OpenDest.LogSet)
        WidgetSnapshotStore.write(ctx, snapshot(WidgetDayKind.Run, trackable = true))
        assertOpens(click(), OpenDest.Track)
        RunSession.tracking.value = true
        assertOpens(click(), OpenDest.Track)
        // Start run opens the tracker screen; the tile itself starts no service.
        assertEquals(null, shadowOf(ctx as android.app.Application).nextStartedService)
    }
}
