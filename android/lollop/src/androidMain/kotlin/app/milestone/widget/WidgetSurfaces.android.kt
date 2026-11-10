package app.milestone.widget

import android.content.Context
import android.os.Build
import androidx.annotation.RequiresApi
import androidx.glance.appwidget.GlanceAppWidgetManager
import androidx.glance.appwidget.updateAll
import app.milestone.AndroidReminder
import app.milestone.AppRuntime
import app.milestone.PlatformLog
import app.milestone.keyValueStore
import app.milestone.requestTileRefresh
import app.milestone.todayEpochDay
import kotlin.coroutines.cancellation.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

internal actual val widgetSurfacePort: WidgetSurfacePort = AndroidWidgets

/**
 * The widget surfaces on Android: the Glance widget ([TodayWidget]) and the Quick
 * Settings tile (app.milestone.QuickTileService), both drawn from the day snapshot
 * file ([WidgetSnapshotStore]) so neither boots the core. A model, distance unit
 * or stale-mark change writes the snapshot; a written snapshot, a run starting or
 * stopping and a theme, dark-mode or accent change redraw the widget; the tile is
 * redrawn when its face can change ([requestTileRefresh]). On Android 15+ the
 * widget picker's preview follows the theme.
 */
internal object AndroidWidgets : WidgetSurfacePort {
    private const val TAG = "WidgetSurfaces"
    private const val PREVIEW_PREFS = "widget_preview"
    private const val KEY_PREVIEW = "published"

    override val ticks: Flow<Long> get() = WidgetSnapshotStore.staleMarks

    override fun start(ctx: Context) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.VANILLA_ICE_CREAM) launchPreviewSync(ctx.applicationContext)
    }

    override suspend fun show(ctx: Context, inputs: WidgetInputs, previous: WidgetInputs?) {
        val app = ctx.applicationContext
        val recordingChanged = previous != null && inputs.recording != previous.recording
        if (recordingChanged) requestTileRefresh(app)
        if (inputs.recording && previous?.recording != true) AndroidReminder.onRecording(app)
        var redraw = recordingChanged || (previous != null && inputs.look != previous.look)
        if (inputs.model != null && !inputs.sameSnapshotAs(previous)) {
            try {
                val snapshot = withContext(Dispatchers.Default) { inputs.snapshot() }
                if (snapshot != null && withContext(Dispatchers.IO) { store(app, snapshot) }) redraw = true
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                PlatformLog.w(TAG, "widget snapshot not written", e)
            }
        }
        if (redraw) redraw(app)
    }

    override fun onTimeChanged(ctx: Context, zoneChanged: Boolean) {
        val app = ctx.applicationContext
        dayChanged(app, force = zoneChanged)
        AppRuntime.scope.launch { redraw(app) }
    }

    /**
     * Store [snapshot]. A changed one redraws the Quick Settings tile, which shows
     * the same day, and updates a shown session reminder
     * ([AndroidReminder.onSnapshot]). True when it changed.
     */
    fun store(ctx: Context, snapshot: WidgetSnapshot): Boolean {
        if (!WidgetSnapshotStore.write(ctx, snapshot)) return false
        requestTileRefresh(ctx)
        AndroidReminder.onSnapshot(ctx, snapshot)
        return true
    }

    /** The local-midnight tick ([MidnightTick]): a shown reminder for the day that
     *  ended goes ([AndroidReminder.onDayChanged]), then [dayChanged]. */
    fun onMidnight(ctx: Context, today: Long = todayEpochDay()) {
        AndroidReminder.onDayChanged(ctx, today)
        dayChanged(ctx, force = false, today)
    }

    /**
     * The local day may have changed under the stored snapshot (the midnight tick,
     * a clock change) or, when [force], the time zone did: the snapshot is marked
     * stale ([WidgetSnapshotStore.markStale]) and reads "Open lollop to update" until
     * the app publishes a model for the new day. The tile, whose face depends on the
     * day, is redrawn, and the midnight tick set for the next local midnight.
     */
    fun dayChanged(ctx: Context, force: Boolean, today: Long = todayEpochDay()) {
        WidgetSnapshotStore.markStale(ctx, force, today)
        requestTileRefresh(ctx)
        MidnightTick.update(ctx)
    }

    /** Redraw every placed widget. */
    suspend fun redraw(ctx: Context) {
        try {
            if (MidnightTick.widgetPlaced(ctx)) TodayWidget().updateAll(ctx)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            PlatformLog.w(TAG, "widget not updated", e)
        }
    }

    /**
     * The picker's generated preview (Android 15+) in the current theme: published
     * once per theme, dark-mode and accent choice and app version. The platform
     * rate-limits it; a refused call is tried again on the next change or start.
     */
    @RequiresApi(Build.VERSION_CODES.VANILLA_ICE_CREAM)
    private fun launchPreviewSync(ctx: Context) =
        AppRuntime.scope.launch {
            // The library module has no BuildConfig: the installed package's version.
            val version = ctx.packageManager.getPackageInfo(ctx.packageName, 0).longVersionCode
            surfaceLooks()
                .map { "$version/${it.theme}/${it.darkMode}/${it.dynamicAccent}" }
                .distinctUntilChanged()
                .collect { key ->
                    val prefs = ctx.keyValueStore(PREVIEW_PREFS)
                    if (prefs.getString(KEY_PREVIEW, null) == key) return@collect
                    try {
                        val result = GlanceAppWidgetManager(ctx).setWidgetPreviews(TodayWidgetReceiver::class)
                        if (result == GlanceAppWidgetManager.SET_WIDGET_PREVIEWS_RESULT_SUCCESS) prefs.edit { putString(KEY_PREVIEW, key) }
                    } catch (e: CancellationException) {
                        throw e
                    } catch (e: Exception) {
                        PlatformLog.w(TAG, "widget preview not published", e)
                    }
                }
        }
}
