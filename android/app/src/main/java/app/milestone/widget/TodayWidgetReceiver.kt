package app.milestone.widget

import android.app.AlarmManager
import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.util.Log
import androidx.glance.appwidget.GlanceAppWidget
import androidx.glance.appwidget.GlanceAppWidgetReceiver
import androidx.glance.appwidget.updateAll
import app.milestone.AppClock
import app.milestone.todayEpochDay
import app.milestone.QuickTileService
import app.milestone.requestTileRefresh
import java.time.Instant
import java.time.ZoneId
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

/**
 * The widget's receiver. Besides Glance's own updates it handles the day
 * changing under a stored snapshot: the local-midnight tick ([ACTION_MIDNIGHT])
 * and a clock change mark a snapshot from another day stale; a time zone change
 * marks it stale outright. Either way the widget then reads "Open lollop to
 * update" until the app publishes a model for the new day; after a zone change
 * within the day [WidgetSync] rewrites it at once while the app process holds a
 * model. A clock change within the same day (network time corrects the clock by
 * seconds) leaves the snapshot alone.
 * Each of these, and a locale change, also redraws the Quick Settings tile
 * ([requestTileRefresh]), whose face depends on the day and the language; the
 * midnight tick runs while a widget or the tile is placed.
 */
class TodayWidgetReceiver : GlanceAppWidgetReceiver() {
    override val glanceAppWidget: GlanceAppWidget = TodayWidget()

    override fun onUpdate(context: Context, appWidgetManager: AppWidgetManager, appWidgetIds: IntArray) {
        super.onUpdate(context, appWidgetManager, appWidgetIds)
        WidgetMidnight.schedule(context)
    }

    override fun onDisabled(context: Context) {
        super.onDisabled(context)
        if (!QuickTileService.isAdded(context)) WidgetMidnight.cancel(context)
    }

    override fun onReceive(context: Context, intent: Intent) {
        val force = when (intent.action) {
            Intent.ACTION_TIMEZONE_CHANGED -> true
            ACTION_MIDNIGHT, Intent.ACTION_TIME_CHANGED -> false
            Intent.ACTION_LOCALE_CHANGED -> null
            else -> return super.onReceive(context, intent)
        }
        val pending = goAsync()
        CoroutineScope(Dispatchers.IO).launch {
            try {
                if (force != null) WidgetSnapshotStore.markStale(context, force = force, today = todayEpochDay())
                requestTileRefresh(context)
                val widgets = WidgetMidnight.hasWidgets(context)
                if (widgets || QuickTileService.isAdded(context)) WidgetMidnight.schedule(context)
                if (widgets) TodayWidget().updateAll(context)
            } catch (t: Throwable) {
                Log.w("TodayWidget", "day change not applied", t)
            } finally {
                pending.finish()
            }
        }
    }

    companion object {
        /** The receiver's own local-midnight alarm. */
        internal const val ACTION_MIDNIGHT = "app.milestone.widget.action.MIDNIGHT"
    }
}

/** The local-midnight alarm that rolls the widget over to "Open lollop to update". */
internal object WidgetMidnight {
    private const val REQUEST_CODE = 0x57440001

    fun hasWidgets(ctx: Context): Boolean =
        AppWidgetManager.getInstance(ctx)
            ?.getAppWidgetIds(ComponentName(ctx, TodayWidgetReceiver::class.java))
            ?.isNotEmpty() == true

    private fun pendingIntent(ctx: Context): PendingIntent = PendingIntent.getBroadcast(
        ctx,
        REQUEST_CODE,
        Intent(ctx, TodayWidgetReceiver::class.java).setAction(TodayWidgetReceiver.ACTION_MIDNIGHT),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

    /** The next local midnight after [nowMillis] in [zone], in epoch millis. */
    fun nextMidnightMillis(nowMillis: Long, zone: ZoneId = ZoneId.systemDefault()): Long =
        Instant.ofEpochMilli(nowMillis).atZone(zone).toLocalDate().plusDays(1)
            .atStartOfDay(zone).toInstant().toEpochMilli()

    /**
     * Inexact and non-waking (RTC): it never needs the exact-alarm permission, and
     * a sleeping phone shows no widget, so the tick lands when the screen comes on.
     * Replaces any earlier one (same PendingIntent).
     */
    fun schedule(ctx: Context) {
        val alarms = ctx.getSystemService(AlarmManager::class.java) ?: return
        alarms.set(AlarmManager.RTC, nextMidnightMillis(AppClock.nowMillis()), pendingIntent(ctx))
    }

    fun cancel(ctx: Context) {
        ctx.getSystemService(AlarmManager::class.java)?.cancel(pendingIntent(ctx))
    }
}
