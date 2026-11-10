package app.milestone

/** iOS has no wallpaper accent ([platformAccent] is null). */
internal actual val platformHasDynamicAccent: Boolean get() = false

/** The iOS run and rest timer post no Live Update, so there is no switch to explain. */
internal actual val liveUpdatesSwitch: LiveUpdatesSwitch? = null
