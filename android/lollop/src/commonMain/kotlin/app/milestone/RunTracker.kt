package app.milestone

import androidx.compose.runtime.Composable

/**
 * The platform side of GPS run recording: the location stream, the run's system
 * surface, the location / notification grants and the settings pages the tracker
 * sends the runner to. Fixes and the recording state machine are common
 * ([RunRecorder] into [RunSession], the crash-safe [ActiveRunStore] file); this
 * only drives the platform. Android: the RunTrackingService foreground service;
 * iOS: CoreLocation (RunTracker.ios.kt). [platformRunTracker] is null where the
 * platform has no tracker; every tracker entry is hidden then.
 */
internal interface RunTracker {
    /** A precise location grant. Coarse-only is not enough for GPS tracking. */
    fun hasLocationPermission(ctx: PlatformContext): Boolean

    /** At least approximate location is allowed (with or without precise). */
    fun hasApproximateLocation(ctx: PlatformContext): Boolean

    /** The system would no longer show its location prompt ("don't ask again" after
     *  a denial). Before the first ask the same check reads true, so callers pair it
     *  with whether the prompt was shown. */
    fun locationPromptBlocked(ctx: PlatformContext): Boolean

    /** The app may post notifications (the run's ongoing notification). */
    fun canPostNotifications(ctx: PlatformContext): Boolean

    /** The system-wide location toggle is on. A grant alone delivers no fixes while
     *  location services are off, so the tracker surfaces this state. */
    fun isLocationEnabled(ctx: PlatformContext): Boolean

    /** The app is exempt from the system's battery optimisation, so a long background
     *  run keeps its location stream alive. When false the tracker nudges. */
    fun isBatteryUnrestricted(ctx: PlatformContext): Boolean

    /** Ask the system to exempt the app from battery optimisation. */
    fun requestBatteryUnrestricted(ctx: PlatformContext)

    /** Open the system's location settings (the location toggle). */
    fun openLocationSettings(ctx: PlatformContext)

    /** Open the app's notification settings. */
    fun openNotificationSettings(ctx: PlatformContext)

    /** Call [onChange] whenever the location toggle or grant may have changed, until
     *  the returned function is called. */
    fun observeLocationSettings(ctx: PlatformContext, onChange: () -> Unit): () -> Unit

    /** The permission prompts, bound to the composition: [onLocationResult] gets
     *  whether precise location was granted, [onNotificationResult] fires when the
     *  notification prompt closes. */
    @Composable
    fun rememberPermissionPrompts(
        onLocationResult: (preciseGranted: Boolean) -> Unit,
        onNotificationResult: () -> Unit,
    ): PermissionPrompts

    /** Best-effort last-known position for centring the map before the first fix;
     *  needs the location grant, silently does nothing without a cached fix.
     *  [onResult] fires on the main thread. */
    fun lastKnownLocation(ctx: PlatformContext, onResult: (lat: Double, lon: Double) -> Unit)

    /** Locate-only: acquire GPS to preview fix quality; records nothing. */
    fun locate(ctx: PlatformContext)

    /** Armed start: acquire GPS and begin recording automatically at the first fix
     *  clearing the requested gates. */
    fun arm(ctx: PlatformContext, requirePrecise: Boolean, requireMoving: Boolean)

    /** Armed → recording now, skipping the start gates. */
    fun startNow(ctx: PlatformContext)

    /** Start (or resume) recording now: the crash-resume start, appending to the
     *  interrupted run's file. */
    fun startRecording(ctx: PlatformContext)

    /** Stop the recording service; [RunSession.tracking] turns false once the location
     *  stream is down. */
    fun stop(ctx: PlatformContext)

    /** A run's crash-safe file exists: the app died while recording. */
    fun hasInterruptedRun(ctx: PlatformContext): Boolean

    /** Read the interrupted run back. Blocking file I/O: call it off the main thread. */
    fun recoverInterruptedRun(ctx: PlatformContext): RecoveredTrack

    /** Delete the run's crash-safe file (asynchronously). */
    fun discardInterruptedRun()

    /** Delete the run's crash-safe file before returning, ordered after the saved
     *  run's event-log append. */
    fun discardInterruptedRunSync()
}

/** The tracker's system permission prompts, launched from a click. */
internal interface PermissionPrompts {
    /** Ask for precise (and approximate) location. */
    fun requestLocation()

    /** Ask to post notifications. */
    fun requestNotifications()
}

/** This platform's run tracker, or null where GPS run recording isn't available. */
internal expect fun platformRunTracker(): RunTracker?
