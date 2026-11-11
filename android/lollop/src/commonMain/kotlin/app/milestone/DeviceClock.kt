package app.milestone

/** The device's wall clock, milliseconds since the epoch (System.currentTimeMillis). */
internal expect fun wallClockMillis(): Long

/** Milliseconds on a clock that never jumps and keeps counting in deep sleep
 *  (Android `SystemClock.elapsedRealtime`): for durations, never for dates. */
internal expect fun deviceMonotonicMillis(): Long

/** The app's clocks: wall time for every "now" the app stamps or shows, monotonic
 *  time (never jumps, keeps counting in deep sleep) for durations. */
internal interface WallClock {
    fun nowMillis(): Long

    fun nowSec(): Long = nowMillis() / 1000L

    fun monotonicMillis(): Long
}

/** The device's clocks. */
internal object DeviceClock : WallClock {
    override fun nowMillis(): Long = wallClockMillis()

    override fun monotonicMillis(): Long = deviceMonotonicMillis()
}
