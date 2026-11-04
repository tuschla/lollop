package app.milestone

import platform.Foundation.NSLog

internal actual object PlatformLog {
    actual fun e(tag: String, msg: String, tr: Throwable?) = log("E", tag, msg, tr)

    actual fun w(tag: String, msg: String, tr: Throwable?) = log("W", tag, msg, tr)

    // Logcat's "level/tag: message" shape. The text is the format itself with
    // every '%' doubled, so it prints as written: Kotlin/Native passes a String
    // vararg to a C variadic function as a C string, which `%@` would read as an
    // object pointer (a crash), and `%s` would decode in the legacy C encoding.
    private fun log(level: String, tag: String, msg: String, tr: Throwable?) {
        val text = if (tr == null) msg else msg + "\n" + tr.stackTraceToString()
        NSLog("$level/$tag: $text".replace("%", "%%"))
    }
}
