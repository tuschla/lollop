package app.milestone

import android.app.AlarmManager
import android.app.StatusBarManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Looper
import android.service.quicksettings.TileService
import androidx.test.core.app.ApplicationProvider
import app.milestone.widget.TodayWidgetReceiver
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
import org.robolectric.Shadows.shadowOf

/**
 * The Quick Settings tile is an active tile: System UI redraws it only when the app
 * asks ([requestTileRefresh] → StatusBarManager.requestTileServiceListeningState).
 * Each event that can change its face asks; a stand-in for the status bar service
 * records the requests.
 */
@RunWith(RobolectricTestRunner::class)
class QuickTileRefreshTest {

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()
    private val tile get() = ComponentName(ctx, QuickTileService::class.java)
    private val requests = mutableListOf<ComponentName>()

    @Before
    fun setUp() {
        RunSession.reset()
        File(ctx.filesDir, WidgetSnapshotStore.FILE_NAME).delete()
        val statusBar = checkNotNull(ctx.getSystemService(StatusBarManager::class.java))
        val iface = Class.forName("com.android.internal.statusbar.IStatusBarService")
        val fake = Proxy.newProxyInstance(iface.classLoader, arrayOf(iface)) { _, method, args ->
            if (method.name == "requestTileServiceListeningState") {
                synchronized(requests) { requests += args[0] as ComponentName }
            }
            when (method.returnType) {
                java.lang.Boolean.TYPE -> false
                Integer.TYPE -> 0
                java.lang.Long.TYPE -> 0L
                else -> null
            }
        }
        StatusBarManager::class.java.getDeclaredField("mService").apply { isAccessible = true }.set(statusBar, fake)
    }

    @After
    fun tearDown() {
        RunSession.reset()
        File(ctx.filesDir, WidgetSnapshotStore.FILE_NAME).delete()
    }

    private fun taken(): List<ComponentName> = synchronized(requests) { requests.toList().also { requests.clear() } }

    /** Requests made within [timeoutMs] (the receiver works on a background thread). */
    private fun awaitRequests(timeoutMs: Long = 5_000): List<ComponentName> {
        val end = System.currentTimeMillis() + timeoutMs
        while (System.currentTimeMillis() < end) {
            shadowOf(Looper.getMainLooper()).idle()
            if (synchronized(requests) { requests.isNotEmpty() }) return taken()
            Thread.sleep(20)
        }
        return taken()
    }

    private fun snapshot(kind: WidgetDayKind, day: Long = todayEpochDay()) =
        WidgetSnapshot(epochDay = day, kind = kind, trackable = kind == WidgetDayKind.Run)

    @Test
    fun manifestDeclaresAnActiveTile() {
        val info = ctx.packageManager.getServiceInfo(tile, PackageManager.GET_META_DATA)
        assertTrue(info.metaData.getBoolean(TileService.META_DATA_ACTIVE_TILE, false))
    }

    @Test
    fun snapshotWriteAndStaleMarkRequestARedraw() {
        WidgetSnapshotStore.write(ctx, snapshot(WidgetDayKind.Run))
        assertEquals(listOf(tile), taken())
        // The same snapshot again writes nothing and asks nothing.
        WidgetSnapshotStore.write(ctx, snapshot(WidgetDayKind.Run))
        assertEquals(emptyList<ComponentName>(), taken())
        WidgetSnapshotStore.markStale(ctx, force = true)
        assertEquals(listOf(tile), taken())
    }

    @Test
    fun runStartAndStopRequestARedraw() {
        TileSync.start(ctx)
        shadowOf(Looper.getMainLooper()).idle()
        taken()
        RunSession.tracking.value = true
        assertEquals(listOf(tile), awaitRequests())
        RunSession.tracking.value = false
        assertEquals(listOf(tile), awaitRequests())
    }

    @Test
    fun dayChangesAndLocaleRequestARedraw() {
        // Yesterday's run day: every day change makes it read Log today.
        WidgetSnapshotStore.write(ctx, snapshot(WidgetDayKind.Run, day = todayEpochDay() - 1))
        taken()
        for (action in listOf(
            TodayWidgetReceiver.ACTION_MIDNIGHT,
            Intent.ACTION_DATE_CHANGED,
            Intent.ACTION_TIME_CHANGED,
            Intent.ACTION_TIMEZONE_CHANGED,
            Intent.ACTION_LOCALE_CHANGED,
        )) {
            ctx.sendBroadcast(Intent(action).setClass(ctx, TodayWidgetReceiver::class.java))
            val got = awaitRequests()
            assertTrue("$action: $got", got.isNotEmpty() && got.all { it == tile })
        }
        assertEquals(TileFace.Log, QuickTileService.currentFace(ctx))
    }

    @Test
    fun addedTileKeepsTheMidnightTickWithoutAWidget() {
        val alarms = shadowOf(ctx.getSystemService(AlarmManager::class.java))
        WidgetSnapshotStore.write(ctx, snapshot(WidgetDayKind.Lift))
        val service = Robolectric.buildService(QuickTileService::class.java).create().get()
        service.onTileAdded()
        assertTrue(QuickTileService.isAdded(ctx))
        // A new tile is drawn at once, not left on the manifest label.
        assertEquals(ctx.getString(R.string.tile_log_set), service.qsTile.label.toString())
        assertNotNull(alarms.nextScheduledAlarm)
        service.onTileRemoved()
        assertEquals(false, QuickTileService.isAdded(ctx))
        assertNull(alarms.nextScheduledAlarm)
        // After cleared app data (the flag gone), the next draw restores it and the tick.
        service.onStartListening()
        assertTrue(QuickTileService.isAdded(ctx))
        assertNotNull(alarms.nextScheduledAlarm)
    }
}
