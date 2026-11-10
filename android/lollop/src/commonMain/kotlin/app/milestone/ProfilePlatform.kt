package app.milestone

// The platform facts Profile's rows depend on.

/** Whether the platform has a wallpaper accent the app can follow ("Use system
 *  accent"): Android 12+ (Material You); iOS has none, so the row is hidden. */
internal expect val platformHasDynamicAccent: Boolean

/** The per-app system switch for Live Updates (the run and rest timer at the top of
 *  the shade, on the lock screen and as a status-bar chip), or null where the
 *  platform has none: iOS's run and rest timer post no Live Update. */
internal expect val liveUpdatesSwitch: LiveUpdatesSwitch?

internal interface LiveUpdatesSwitch {
    /** True only where Live Updates exist and the user turned them off for this
     *  app: the one case worth explaining in Profile. */
    fun isOff(ctx: PlatformContext): Boolean

    /** Open the system screen that turns Live Updates on or off for this app. */
    fun open(ctx: PlatformContext)
}
