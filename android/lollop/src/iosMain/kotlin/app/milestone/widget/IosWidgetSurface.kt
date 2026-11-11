package app.milestone.widget

import app.milestone.IosSurfaces
import app.milestone.PlatformContext
import app.milestone.ProcessLocal
import app.milestone.appResources
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow

internal actual val widgetSurfacePort: WidgetSurfacePort = IosWidgetSurface

/**
 * The home and lock screen widgets and the Control Center control on iOS (Android:
 * the Glance widget and the Quick Settings tile): the [IosWidgetPayload] of every
 * change of the inputs, handed to the Swift side ([IosSurfaces.publishWidget]). A
 * time zone or clock change derives it again (the payload records the zone it was
 * written in). Built on the main thread, as the notifications build their text.
 */
internal object IosWidgetSurface : WidgetSurfacePort {
    private val tickCount by ProcessLocal { MutableStateFlow(0L) }

    override val ticks: Flow<Long> get() = tickCount

    override fun start(ctx: PlatformContext) {}

    override suspend fun show(ctx: PlatformContext, inputs: WidgetInputs, previous: WidgetInputs?) {
        val snapshot = inputs.snapshot() ?: return
        val payload = iosWidgetPayload(snapshot, inputs.recording, inputs.look.theme, inputs.look.darkMode, ctx.appResources())
        IosSurfaces.publishWidget(iosSurfaceJson.encodeToString(IosWidgetPayload.serializer(), payload))
    }

    override fun onTimeChanged(ctx: PlatformContext, zoneChanged: Boolean) {
        tickCount.value += 1
    }
}
