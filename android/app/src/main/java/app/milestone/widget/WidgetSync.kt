package app.milestone.widget

import android.content.Context
import android.util.Log
import app.milestone.AppSettings
import app.milestone.EventDispatcher
import app.milestone.resolveDistanceUnit
import androidx.glance.appwidget.updateAll
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.conflate
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.flowOn
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * Keeps the widget snapshot in step with the app: every published model
 * ([EventDispatcher.model]) is turned into a [WidgetSnapshot] and, when it
 * differs from the stored one, written and the widget updated. A stale mark
 * ([WidgetSnapshotStore.staleMarks]) derives it again from the current model, in
 * the zone now in force: after a time zone change that keeps the local day the
 * model is unchanged and nothing else would replace the stale snapshot. A theme
 * or dark-mode change re-renders the widget. Process-scoped, started once from
 * MainActivity; a second call is a no-op.
 */
internal object WidgetSync {
    @Volatile private var started = false
    // The collectors [start] launched: they hold the application context they
    // were started with.
    private var jobs: List<Job> = emptyList()

    fun start(context: Context) {
        if (started) return
        started = true
        val ctx = context.applicationContext
        jobs = listOf(launchSnapshotSync(ctx), launchThemeSync(ctx))
    }

    /**
     * Stop the collectors so the next [start] binds a new context. Tests only:
     * Robolectric gives every test a fresh Application (and files dir) in the
     * same process, while [EventDispatcher.scope] outlives it.
     */
    internal fun resetForTest() {
        jobs.forEach { it.cancel() }
        jobs = emptyList()
        started = false
    }

    private fun launchSnapshotSync(ctx: Context): Job =
        EventDispatcher.scope.launch {
            combine(EventDispatcher.model, AppSettings.distanceUnitOverride, WidgetSnapshotStore.staleMarks) { model, unit, _ ->
                model?.let { widgetSnapshotOf(it, resolveDistanceUnit(unit)) }
            }
                .filterNotNull()
                .flowOn(Dispatchers.Default)
                .conflate()
                .collect { snapshot ->
                    runCatching {
                        val changed = withContext(Dispatchers.IO) { WidgetSnapshotStore.write(ctx, snapshot) }
                        if (changed && WidgetMidnight.hasWidgets(ctx)) TodayWidget().updateAll(ctx)
                    }.onFailure { Log.w("WidgetSync", "widget snapshot not written", it) }
                }
        }

    private fun launchThemeSync(ctx: Context): Job =
        EventDispatcher.scope.launch {
            combine(AppSettings.theme, AppSettings.darkMode, AppSettings.dynamicAccent) { _, _, _ -> }
                .drop(1)
                .conflate()
                .collect { runCatching { if (WidgetMidnight.hasWidgets(ctx)) TodayWidget().updateAll(ctx) } }
        }
}
