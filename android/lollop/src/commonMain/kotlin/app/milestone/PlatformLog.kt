package app.milestone

/** The platform log: logcat on Android, the unified system log (NSLog) on iOS. */
internal expect object PlatformLog {
    /** An error worth a look in a bug report: [tr]'s stack trace follows [msg]. */
    fun e(tag: String, msg: String, tr: Throwable? = null)

    /** A recoverable problem: [tr]'s stack trace follows [msg]. */
    fun w(tag: String, msg: String, tr: Throwable? = null)
}
