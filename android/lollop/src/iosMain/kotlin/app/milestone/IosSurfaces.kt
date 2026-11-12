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

    public fun endRunActivity()

    /** Start or update the rest's Live Activity ([IosRestLive] JSON). */
    public fun updateRestActivity(json: String)

    public fun endRestActivity()
}

/** Where the app's surfaces outside its window go. Main thread. */
public object IosSurfaces {
    private class State {
        var host: IosSurfaceHost? = null
        var widget: String? = null

        // The recording run's last frame; null while no run records.
        var run: RunSurfaceState? = null
    }

    private val state by ProcessLocal { State() }

    /**
     * Hand the surfaces to [host]: the current widget payload, and the running
     * run's and rest's activities, at once; every change after.
     */
    public fun install(host: IosSurfaceHost) {
        state.host = host
        state.widget?.let(host::publishWidget)
        state.run?.let(::showRun)
        RestAlarm.showCountdownAgain(PlatformContext.INSTANCE)
    }

    /** Hand the widget payload [json] over unless it is the last one. */
    internal fun publishWidget(json: String) {
        if (json == state.widget) return
        state.widget = json
        state.host?.publishWidget(json)
    }

    /** Show the recording run's frame [run] in its activity. */
    internal fun showRun(run: RunSurfaceState) {
        state.run = run
        val host = state.host ?: return
        val live = runLiveOf(run, AppSettings.theme.value, PlatformContext.INSTANCE.appResources())
        host.updateRunActivity(iosSurfaceJson.encodeToString(IosRunLive.serializer(), live))
    }

    /** The run's location stream is down: take its activity away. */
    internal fun endRun() {
        state.run = null
        state.host?.endRunActivity()
    }

    /** Show [run] (and the [next] set) in the rest's activity. */
    internal fun showRest(run: RestRun, next: RestNextSet?) {
        val host = state.host ?: return
        val live = restLiveOf(run, next, AppSettings.theme.value, PlatformContext.INSTANCE.appResources())
        host.updateRestActivity(iosSurfaceJson.encodeToString(IosRestLive.serializer(), live))
    }

    internal fun endRest() {
        state.host?.endRestActivity()
    }
}
