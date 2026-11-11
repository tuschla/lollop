package app.milestone.widget

import app.milestone.AppFeature
import app.milestone.AppRuntime
import app.milestone.AppSettings
import app.milestone.AppTheme
import app.milestone.DarkMode
import app.milestone.DistanceUnit
import app.milestone.EventDispatcher
import app.milestone.PlatformContext
import app.milestone.RunSession
import app.milestone.ViewModel
import app.milestone.distanceUnit
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.conflate
import kotlinx.coroutines.launch
import kotlinx.datetime.TimeZone

/** The look the app's surfaces outside its window draw in. */
internal data class SurfaceLook(val theme: AppTheme, val darkMode: DarkMode, val dynamicAccent: Boolean)

/** The app's [SurfaceLook], on every theme, dark-mode and system-accent change. */
internal fun surfaceLooks(): Flow<SurfaceLook> =
    combine(AppSettings.theme, AppSettings.darkMode, AppSettings.dynamicAccent) { t, d, a -> SurfaceLook(t, d, a) }

/**
 * What the day snapshot and every surface drawn from it (the widgets, the tile or
 * control, the session reminder) follow: the published [model] (null before the
 * first), the distance [unit], whether a GPS run records in this process
 * ([recording]), the [look], and [ticks], which count the times the snapshot must
 * be derived again from an unchanged model ([WidgetSurfacePort.ticks]).
 */
internal data class WidgetInputs(
    val model: ViewModel?,
    val unit: DistanceUnit,
    val recording: Boolean,
    val look: SurfaceLook,
    val ticks: Long,
) {
    /** The day snapshot of [model] in [zone]; null before the first model. */
    fun snapshot(zone: String = TimeZone.currentSystemDefault().id): WidgetSnapshot? =
        model?.let { widgetSnapshotOf(it, unit, zone) }

    /** [other] derives the same snapshot: the same model, unit and tick. */
    fun sameSnapshotAs(other: WidgetInputs?): Boolean =
        other != null && model === other.model && unit == other.unit && ticks == other.ticks
}

/** The [WidgetInputs], on every change of any of them; the unit is the model's
 *  ([distanceUnit]). */
internal fun widgetInputs(): Flow<WidgetInputs> =
    combine(
        EventDispatcher.model,
        RunSession.tracking,
        surfaceLooks(),
        widgetSurfacePort.ticks,
    ) { model, recording, look, ticks -> WidgetInputs(model, model.distanceUnit(), recording, look, ticks) }

/**
 * Keeps the surfaces outside the app's window in step with it: the home and lock
 * screen widgets, Android's Quick Settings tile and iOS's Control Center control.
 * Every change of the [widgetInputs] goes to the platform ([WidgetSurfacePort.show])
 * with the inputs it last showed; a clock or time zone change goes there too.
 */
internal object WidgetSurfaces : AppFeature {
    override fun start(ctx: PlatformContext) {
        widgetSurfacePort.start(ctx)
        AppRuntime.scope.launch {
            var shown: WidgetInputs? = null
            widgetInputs().conflate().collect { inputs ->
                widgetSurfacePort.show(ctx, inputs, shown)
                shown = inputs
            }
        }
    }

    override fun onTimeChanged(ctx: PlatformContext, zoneChanged: Boolean) =
        widgetSurfacePort.onTimeChanged(ctx, zoneChanged)
}

/** How a platform draws the widget surfaces ([WidgetSurfaces]). */
internal interface WidgetSurfacePort {
    /** Ticks when the day snapshot must be derived again although no new model
     *  came: the local day or zone changed under the one the process holds. */
    val ticks: Flow<Long>

    /** The process started ([AppRuntime.start]): what the platform follows besides the inputs. */
    fun start(ctx: PlatformContext)

    /** Show [inputs]; [previous] are the ones shown last (null: none yet in this process). */
    suspend fun show(ctx: PlatformContext, inputs: WidgetInputs, previous: WidgetInputs?)

    /** The wall clock was set or the time zone changed ([zoneChanged]): the local
     *  day may differ from the one the surfaces show. */
    fun onTimeChanged(ctx: PlatformContext, zoneChanged: Boolean)
}

internal expect val widgetSurfacePort: WidgetSurfacePort
