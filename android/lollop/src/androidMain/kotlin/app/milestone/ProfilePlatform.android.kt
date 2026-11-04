package app.milestone

import android.os.Build

// liveUpdatesTurnedOff / openPromotedNotificationSettings: LiveUpdates.kt.

/** Dynamic (wallpaper) colour exists from Android 12 (API 31). */
internal actual val platformHasDynamicAccent: Boolean
    get() = Build.VERSION.SDK_INT >= Build.VERSION_CODES.S
