package app.milestone

/** iOS has no wallpaper accent ([platformAccent] is null). */
internal actual val platformHasDynamicAccent: Boolean get() = false

/** The iOS run and rest timer post no Live Update, so there is nothing to turn off. */
internal actual fun liveUpdatesTurnedOff(ctx: PlatformContext): Boolean = false

/** iOS has no per-app Live Updates switch: the app's page in Settings, where its
 *  notifications are. */
internal actual fun openPromotedNotificationSettings(ctx: PlatformContext) = openAppSettings(ctx)
