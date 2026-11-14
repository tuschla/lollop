package app.milestone

/**
 * Every notification id and PendingIntent request code the app hands the system,
 * allocated here so no two collide. The values are what installed versions used:
 * a changed request code would orphan an alarm set before an update.
 */
internal object AndroidIds {
    const val RUN_NOTIFICATION = 1
    const val REST_COUNTDOWN_NOTIFICATION = 4207
    const val REST_DONE_NOTIFICATION = 4209
    const val REMINDER_NOTIFICATION = 4301

    /** The run notification's actions; each differs by its Intent action. */
    const val RUN_ACTION_REQUEST = 0
    const val REST_DISMISS_REQUEST = 4207
    const val REST_END_ALARM_REQUEST = 4208
    const val REMINDER_ALARM_REQUEST = 4302
    const val REMINDER_ACTION_REQUEST = 4303
    const val MIDNIGHT_ALARM_REQUEST = 0x57440001

    /** The open-the-app intents: this base plus the [OpenDest] ordinal. */
    const val OPEN_REQUEST_BASE = 0x4F500000
}
