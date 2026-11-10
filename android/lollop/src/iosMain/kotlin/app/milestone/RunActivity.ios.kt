package app.milestone

/** Receives the run's frames while the tracker's location stream is up. Main thread. */
internal interface RunActivitySink {
    /** The run changed what it shows. */
    fun update(state: RunSurfaceState)

    /** The stream is down: take the surface away. */
    fun end()
}

/**
 * The run's hook for its Live Activity: the tracker pushes [RunSurfaceState]
 * frames to [sink] ([IosSurfaces] installs one that hands them to the Swift
 * side), and the activity's Pause/Resume buttons call [setPaused] (through
 * `LollopSurfaces` in the `Lollop` framework). With no sink the tracker shows no
 * system surface; iOS still shows its blue location indicator while a run records
 * in the background. Main thread only.
 */
public object RunActivity {
    /** Where frames go; a new sink gets the current frame at once. */
    internal var sink: RunActivitySink? = null
        set(value) {
            field = value
            IosRunTracker.resendSurface()
        }

    /** Pause or resume the recording run (a no-op unless one records), like the
     *  Android notification's Pause/Resume action. */
    public fun setPaused(paused: Boolean) {
        IosRunTracker.setPaused(paused)
    }
}
