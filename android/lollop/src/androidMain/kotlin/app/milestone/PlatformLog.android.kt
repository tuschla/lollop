package app.milestone

import android.util.Log

internal actual object PlatformLog {
    // Log.e(tag, msg, null) would end the line with an empty stack trace.
    actual fun e(tag: String, msg: String, tr: Throwable?) {
        if (tr == null) Log.e(tag, msg) else Log.e(tag, msg, tr)
    }

    actual fun w(tag: String, msg: String, tr: Throwable?) {
        if (tr == null) Log.w(tag, msg) else Log.w(tag, msg, tr)
    }
}
