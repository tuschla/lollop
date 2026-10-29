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
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * Keeps the widget snapshot in step with the app: every published model
 * ([EventDispatcher.model]) is turned into a [WidgetSnapshot] and, when it
 * differs from the stored one, written and the widget updated. A theme or
 * dark-mode change re-renders the widget. Process-scoped, started once from
 * MainActivity; a second call is a no-op.
 */
internal object WidgetSync {
    @Volatile private var started = false

    fun start(context: Context) {
        if (started) return
        started = true
        val ctx = context.applicationContext
        EventDispatcher.scope.launch {
            combine(EventDispatcher.model.filterNotNull(), AppSettings.distanceUnitOverride) { model, unit ->
                widgetSnapshotOf(model, resolveDistanceUnit(unit))
            }
                .flowOn(Dispatchers.Default)
                .conflate()
                .collect { snapshot ->
                    runCatching {
                        val changed = withContext(Dispatchers.IO) { WidgetSnapshotStore.write(ctx, snapshot) }
                        if (changed && WidgetMidnight.hasWidgets(ctx)) TodayWidget().updateAll(ctx)
                    }.onFailure { Log.w("WidgetSync", "widget snapshot not written", it) }
                }
        }
        EventDispatcher.scope.launch {
            combine(AppSettings.theme, AppSettings.darkMode, AppSettings.dynamicAccent) { _, _, _ -> }
                .drop(1)
                .conflate()
                .collect { runCatching { if (WidgetMidnight.hasWidgets(ctx)) TodayWidget().updateAll(ctx) } }
        }
    }
}
