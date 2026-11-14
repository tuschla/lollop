package app.milestone

import android.os.SystemClock

internal actual fun wallClockMillis(): Long = System.currentTimeMillis()

internal actual fun deviceMonotonicMillis(): Long = SystemClock.elapsedRealtime()
