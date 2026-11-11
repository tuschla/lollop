package app.milestone

import app.milestone.widget.IosRestLive
import app.milestone.widget.IosRunLive
import app.milestone.widget.iosSurfaceJson
import app.milestone.widget.restLiveOf
import app.milestone.widget.runLiveOf

// The app's surfaces outside its window on iOS, drawn by the Swift widget
// extension (iosApp/LollopWidgets) from the JSON the common code resolves
// (widget/IosSurfacePayload.kt). The Swift side (`SurfaceHost` in the `Lollop`
// framework) stores the widget payload in the App Group container and starts,
// updates and ends the activities.

/** The Swift side of the system surfaces, installed by [IosSurfaces.install]. Main thread. */
public interface IosSurfaceHost {
    /** Store the widget payload ([app.milestone.widget.IosWidgetPayload] JSON) where the widget
     *  extension reads it, and reload the widgets and controls. */
    public fun publishWidget(json: String)

    /** Start or update the run's Live Activity ([IosRunLive] JSON). */
    public fun updateRunActivity(json: String)

    /** End the run's Live Activity. */
    public fun endRunActivity()

    /** Start or update the rest's Live Activity ([IosRestLive] JSON). */
    public fun updateRestActivity(json: String)

    /** End the rest's Live Activity. */
    public fun endRestActivity()
}

/** Where the app's surfaces outside its window go. Main thread. */
public object IosSurfaces {
    private var host: IosSurfaceHost? = null

    // The last widget payload handed over, so an unchanged one is not sent again
    // and a new host gets it at once.
    private class State {
        var widget: String? = null
    }

    private val state by ProcessLocal { State() }

    /**
     * Hand the surfaces to [host]: the current widget payload, and the running
     * run's and rest's activities, at once; every change after.
     */
    public fun install(host: IosSurfaceHost) {
        this.host = host
        RunActivity.sink = HostRunSink
        state.widget?.let(host::publishWidget)
        RestAlarm.showCountdownAgain(PlatformContext.INSTANCE)
    }

    /** Hand the widget payload [json] over unless it is the last one. */
    internal fun publishWidget(json: String) {
        if (json == state.widget) return
        state.widget = json
        host?.publishWidget(json)
    }

    /** Show [run] (and the [next] set) in the rest's activity. */
    internal fun showRest(run: RestRun, next: RestNextSet?) {
        val host = host ?: return
        val live = restLiveOf(run, next, AppSettings.theme.value, PlatformContext.INSTANCE.appResources())
        host.updateRestActivity(iosSurfaceJson.encodeToString(IosRestLive.serializer(), live))
    }

    internal fun endRest() {
        host?.endRestActivity()
    }

    private object HostRunSink : RunActivitySink {
        override fun update(state: RunSurfaceState) {
            val host = host ?: return
            val live = runLiveOf(state, AppSettings.theme.value, PlatformContext.INSTANCE.appResources())
            host.updateRunActivity(iosSurfaceJson.encodeToString(IosRunLive.serializer(), live))
        }

        override fun end() {
            host?.endRunActivity()
        }
    }
}
