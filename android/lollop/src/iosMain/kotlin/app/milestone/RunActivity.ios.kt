package app.milestone

/** Where a run is, as a Live Activity would title it. */
public enum class RunActivityPhase { Locating, Armed, Recording, Paused, SignalLost }

/**
 * One frame of a run for the system's run surface (the Android notification's
 * figures): the phase, moving time, distance, the current pace (canonical min/km,
 * NaN while there is none) and the user's distance unit. Equal frames are not
 * pushed twice.
 */
public data class RunActivityState(
    val phase: RunActivityPhase,
    val movingSec: Long,
    val distanceKm: Double,
    val paceMinPerKm: Double,
    val miles: Boolean,
)

/** Receives the run's frames while the tracker's location stream is up. Main thread. */
public interface RunActivitySink {
    /** The run changed what it shows. */
    public fun update(state: RunActivityState)

    /** The stream is down: take the surface away. */
    public fun end()
}

/**
 * The hook for a Live Activity / Dynamic Island (a Swift extension, not part of the
 * Kotlin app): the tracker pushes [RunActivityState] frames to [sink], and the
 * activity's buttons call [setPaused]. With no sink the tracker shows no system
 * surface; iOS still shows its blue location indicator while a run records in the
 * background. Main thread only.
 */
public object RunActivity {
    /** Where frames go; a new sink gets the current frame at once. */
    public var sink: RunActivitySink? = null
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
