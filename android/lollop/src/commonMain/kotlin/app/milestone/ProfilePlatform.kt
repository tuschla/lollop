package app.milestone

// The platform facts Profile's rows depend on.

/** Whether the platform has a wallpaper accent the app can follow ("Use system
 *  accent"): Android 12+ (Material You); iOS has none, so the row is hidden. */
internal expect val platformHasDynamicAccent: Boolean

/** True only where Live Updates exist and the user turned them off for this app:
 *  the one case worth explaining in Profile. Android 16+; never on iOS, whose run
 *  and rest timer post no Live Update. */
internal expect fun liveUpdatesTurnedOff(ctx: PlatformContext): Boolean

/** Open the system screen that turns Live Updates on or off for this app. */
internal expect fun openPromotedNotificationSettings(ctx: PlatformContext)
