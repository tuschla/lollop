package app.milestone

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Context
import android.content.pm.PackageManager
import android.content.res.Configuration
import android.os.Build
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.ui.graphics.toArgb
import androidx.core.content.ContextCompat

// Android 16 (API 36) Live Updates: an ongoing notification that requests promotion
// (`NotificationCompat.Builder.setRequestPromotedOngoing`) is shown at the top of the
// shade, on the lock screen and as a status-bar chip. Below API 36 the request is an
// ignored extra, so the same notification posts as an ordinary ongoing one.

/**
 * Create the channel of a Live Update (the run, the rest countdown): IMPORTANCE_DEFAULT
 * without sound or vibration, so it never makes a noise and still shows on the lock
 * screen. On an IMPORTANCE_LOW ("silent") channel System UI's default "hide silent
 * notifications" left the run and the rest off the lock screen (Android 17 emulator), where a
 * runner or a resting lifter glances at them. Above IMPORTANCE_MIN, which a Live Update
 * can't use. A channel's importance can't be raised once it exists, so the channel an
 * earlier install created as LOW ([retired]) is deleted.
 */
internal fun ensureLiveChannel(ctx: Context, id: String, name: String, retired: String) {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
    val nm = ctx.getSystemService(NotificationManager::class.java) ?: return
    nm.deleteNotificationChannel(retired)
    nm.createNotificationChannel(
        NotificationChannel(id, name, NotificationManager.IMPORTANCE_DEFAULT).apply {
            setSound(null, null)
            enableVibration(false)
        },
    )
}

/** True when the app may post notifications: below API 33 there is no runtime
 *  gate; from 33 on, the POST_NOTIFICATIONS grant decides. */
internal fun canPostNotifications(ctx: Context): Boolean =
    Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
        ContextCompat.checkSelfPermission(ctx, Manifest.permission.POST_NOTIFICATIONS) ==
        PackageManager.PERMISSION_GRANTED

/**
 * The colour every lollop notification carries ([NotificationCompat.Builder.setColor]:
 * the small icon's circle, the app name, action labels): the app theme's accent
 * for the system's light or dark shade ([resolveSurfaceAccent]). Never colorized,
 * which a Live Update must not be. A process started for a notification loads
 * [AppSettings] first.
 */
internal fun notificationAccent(ctx: Context): Int {
    val night = (ctx.resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) == Configuration.UI_MODE_NIGHT_YES
    val theme = AppSettings.theme.value
    return resolveSurfaceAccent(ctx, if (night) theme.dark else theme.light, night, AppSettings.dynamicAccent.value).accent.toArgb()
}

/**
 * [base] (a theme's light or [night] palette) as the surfaces outside the app's
 * window draw it (notifications, the widget): with the system accent on ([dynamic],
 * Android 12+) its accent is the wallpaper colour through the app's contrast guard;
 * grounds and text stay the theme's.
 */
internal fun resolveSurfaceAccent(ctx: Context, base: Palette, night: Boolean, dynamic: Boolean): Palette =
    if (dynamic && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        base.withDynamicAccent((if (night) dynamicDarkColorScheme(ctx) else dynamicLightColorScheme(ctx)).primary)
    } else {
        base
    }
