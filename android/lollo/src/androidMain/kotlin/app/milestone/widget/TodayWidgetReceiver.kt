package app.milestone.widget

import android.app.AlarmManager
import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import androidx.glance.appwidget.GlanceAppWidget
import androidx.glance.appwidget.GlanceAppWidgetReceiver
import app.milestone.AndroidIds
import app.milestone.AppClock
import app.milestone.AppRuntime
import app.milestone.keyValueStore
import app.milestone.nextLocalMidnightMillis

/**
 * The widget's receiver. Besides Glance's own updates it gets the local-midnight
 * tick ([ACTION_MIDNIGHT]): the local day changed, which every feature hears as a
 * time change ([AppRuntime.onTimeChanged]). Clock, time zone and language changes
 * reach the widget through app.milestone.SystemChangeReceiver.
 */
class TodayWidgetReceiver : GlanceAppWidgetReceiver() {
    override val glanceAppWidget: GlanceAppWidget = TodayWidget()

    override fun onUpdate(context: Context, appWidgetManager: AppWidgetManager, appWidgetIds: IntArray) {
        super.onUpdate(context, appWidgetManager, appWidgetIds)
        MidnightTick.update(context, widgetPlaced = true)
    }

    override fun onDisabled(context: Context) {
        super.onDisabled(context)
        MidnightTick.update(context, widgetPlaced = false)
    }

    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action == ACTION_MIDNIGHT) {
            AppRuntime.onTimeChanged(context, zoneChanged = false)
        } else {
            super.onReceive(context, intent)
        }
    }

    companion object {
        /** The receiver's own local-midnight alarm. */
        internal const val ACTION_MIDNIGHT = "app.milestone.widget.action.MIDNIGHT"
    }
}

/**
 * The local-midnight alarm that rolls the widget and the Quick Settings tile over
 * to the new day ([TodayWidgetReceiver.ACTION_MIDNIGHT]). It is set while a widget
 * or the tile is placed, and only then: [update] decides from both.
 */
internal object MidnightTick {
    private const val TILE_PREFS = "quick_tile"
    private const val KEY_TILE_PLACED = "added"

    fun widgetPlaced(ctx: Context): Boolean =
        AppWidgetManager.getInstance(ctx)
            ?.getAppWidgetIds(ComponentName(ctx, TodayWidgetReceiver::class.java))
            ?.isNotEmpty() == true

    /** Whether the tile is on the user's Quick Settings panel. */
    fun tilePlaced(ctx: Context): Boolean = ctx.keyValueStore(TILE_PREFS).getBoolean(KEY_TILE_PLACED, false)

    /** The tile was put on the panel ([placed]) or taken off it; then [update]. */
    fun setTilePlaced(ctx: Context, placed: Boolean) {
        val prefs = ctx.keyValueStore(TILE_PREFS)
        if (prefs.getBoolean(KEY_TILE_PLACED, false) != placed) prefs.edit { putBoolean(KEY_TILE_PLACED, placed) }
        update(ctx)
    }

    /** Set the tick for the next local midnight while a widget ([widgetPlaced]) or
     *  the tile is placed (replacing any earlier one: a reboot clears alarms), else
     *  cancel it. */
    fun update(ctx: Context, widgetPlaced: Boolean = widgetPlaced(ctx)) {
        if (widgetPlaced || tilePlaced(ctx)) schedule(ctx) else cancel(ctx)
    }

    private fun pendingIntent(ctx: Context): PendingIntent = PendingIntent.getBroadcast(
        ctx,
        AndroidIds.MIDNIGHT_ALARM_REQUEST,
        Intent(ctx, TodayWidgetReceiver::class.java).setAction(TodayWidgetReceiver.ACTION_MIDNIGHT),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

    /**
     * Inexact and non-waking (RTC): it never needs the exact-alarm permission, and
     * a sleeping phone shows no widget, so the tick lands when the screen comes on.
     */
    private fun schedule(ctx: Context) {
        val alarms = ctx.getSystemService(AlarmManager::class.java) ?: return
        alarms.set(AlarmManager.RTC, nextLocalMidnightMillis(AppClock.nowMillis()), pendingIntent(ctx))
    }

    fun cancel(ctx: Context) {
        ctx.getSystemService(AlarmManager::class.java)?.cancel(pendingIntent(ctx))
    }
}
