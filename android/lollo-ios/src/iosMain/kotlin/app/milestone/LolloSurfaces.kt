package app.milestone

/**
 * The Swift side of the app's surfaces outside its window (iosApp/iosApp/Surfaces.swift):
 * it stores the widget payload in the App Group container the widget extension
 * reads, reloads the widgets and the control, and runs the Live Activities. Every
 * argument is JSON the extension decodes (the payloads in :lollo's widget/IosSurfacePayload.kt).
 * Called on the main thread.
 */
public interface SurfaceHost {
    /** Store the widget payload and reload the widgets and controls. */
    public fun publishWidget(json: String)

    /** Start or update the run's Live Activity. */
    public fun updateRunActivity(json: String)

    public fun endRunActivity()

    /** Start or update the rest's Live Activity. */
    public fun updateRestActivity(json: String)

    public fun endRestActivity()
}

/** The surfaces' entry points for the Swift host. Main thread. */
public object LolloSurfaces {
    /** Hand the widgets, the control and the Live Activities to [host]: the
     *  current state at once, every change after. Call once, at launch. */
    public fun install(host: SurfaceHost) {
        IosSurfaces.install(
            object : IosSurfaceHost {
                override fun publishWidget(json: String) = host.publishWidget(json)

                override fun updateRunActivity(json: String) = host.updateRunActivity(json)

                override fun endRunActivity() = host.endRunActivity()

                override fun updateRestActivity(json: String) = host.updateRestActivity(json)

                override fun endRestActivity() = host.endRestActivity()
            },
        )
    }

    /** The run activity's Pause/Resume (an App Intent the system runs in the app):
     *  pause or resume the recording run, a no-op unless one records. */
    public fun setRunPaused(paused: Boolean) = RunActivity.setPaused(paused)
}
