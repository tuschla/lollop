package app.milestone

import android.app.AlarmManager
import android.app.Notification
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.service.quicksettings.TileService
import app.milestone.widget.WidgetDayKind
import app.milestone.widget.WidgetSnapshot
import java.lang.reflect.Proxy
import org.junit.Assert.assertTrue
import org.robolectric.Robolectric
import org.robolectric.Shadows.shadowOf

/**
 * Run a seeded walk. [body] gets a `note` that keeps the last [keep] steps; a
 * failure rethrows with [label], [repro] (the env line that replays it) and those
 * steps.
 */
internal fun seededWalk(label: String, repro: String, keep: Int = 25, body: (note: (String) -> Unit) -> Unit) {
    val trace = ArrayDeque<String>()
    try {
        body { s ->
            trace.addLast(s)
            if (trace.size > keep) trace.removeFirst()
        }
    } catch (e: Throwable) {
        val msg = "$label ($repro)\n  ${trace.joinToString("\n  ")}\n${e.message}"
        println("FAILED $msg")
        throw AssertionError(msg, e)
    }
}

/** Tap the Quick Settings tile on API 34+ with a stand-in for System UI's QS
 *  binder; returns the PendingIntent the tile handed to startActivityAndCollapse. */
internal fun clickTile(): PendingIntent {
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

/** A stored day snapshot with only what the tile reads. */
internal fun daySnapshot(kind: WidgetDayKind, sessionDone: Boolean = false, trackable: Boolean = false, day: Long = todayEpochDay()) =
    WidgetSnapshot(epochDay = day, kind = kind, sessionDone = sessionDone, trackable = trackable)

/** What the rest shows: the rest-done alert, else the countdown; never both. */
internal fun postedRest(ctx: Context): Notification? {
    val nm = shadowOf(ctx.getSystemService(NotificationManager::class.java))
    val done = nm.getNotification(RestAlarm.DONE_NOTIF_ID)
    val countdown = nm.getNotification(RestAlarm.COUNTDOWN_NOTIF_ID)
    assertTrue("countdown and rest-done alert both posted", done == null || countdown == null)
    return done ?: countdown
}

/** The broadcast the system sends when the one rest-end alarm goes off, as it was
 *  set (a copy: a later rest updates the PendingIntent's extras in place). */
internal fun restEndBroadcast(ctx: Context): Intent =
    Intent(shadowOf(shadowOf(ctx.getSystemService(AlarmManager::class.java)).scheduledAlarms.single().operation).savedIntent)

/** The system delivers the rest-end alarm to [RestEndReceiver]. */
internal fun fireRestEnd(ctx: Context) = RestEndReceiver().onReceive(ctx, restEndBroadcast(ctx))
