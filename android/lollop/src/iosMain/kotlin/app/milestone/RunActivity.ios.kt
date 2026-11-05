package app.milestone

/** Where a run is, as its Live Activity titles it. */
internal enum class RunActivityPhase { Locating, Armed, Recording, Paused, SignalLost }

/**
 * One frame of a run for its Live Activity (the Android notification's figures):
 * the phase, moving time, distance, the current pace (canonical min/km, NaN while
 * there is none), the user's distance unit, whether the track has the two fixes a
 * distance needs ([hasTrack]) and a guided run's step. Equal frames are not pushed
 * twice.
 */
internal data class RunActivityState(
    val phase: RunActivityPhase,
    val movingSec: Long,
    val distanceKm: Double,
    val paceMinPerKm: Double,
    val miles: Boolean,
    val hasTrack: Boolean = true,
    val step: StepProgress? = null,
)

/** Receives the run's frames while the tracker's location stream is up. Main thread. */
internal interface RunActivitySink {
    /** The run changed what it shows. */
    fun update(state: RunActivityState)

    /** The stream is down: take the surface away. */
    fun end()
}

/**
 * The run's hook for its Live Activity: the tracker pushes [RunActivityState]
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
            IosRunTracker.recording?.resendSurface()
        }

    /** Pause or resume the recording run (a no-op unless one records), like the
     *  Android notification's Pause/Resume action. */
    public fun setPaused(paused: Boolean) {
        IosRunTracker.recording?.recorder?.setPaused(paused)
    }
}
