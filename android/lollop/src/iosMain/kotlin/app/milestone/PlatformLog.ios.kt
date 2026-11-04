package app.milestone

import platform.Foundation.NSLog

internal actual object PlatformLog {
    actual fun e(tag: String, msg: String, tr: Throwable?) = log("E", tag, msg, tr)

    actual fun w(tag: String, msg: String, tr: Throwable?) = log("W", tag, msg, tr)

    // Logcat's "level/tag: message" shape; the text goes as an argument, never as
    // the format, so a '%' in it prints as written.
    private fun log(level: String, tag: String, msg: String, tr: Throwable?) {
        val text = if (tr == null) msg else msg + "\n" + tr.stackTraceToString()
        NSLog("%@/%@: %@", level, tag, text)
    }
}
