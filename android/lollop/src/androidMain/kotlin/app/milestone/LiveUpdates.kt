package app.milestone

import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Context
import android.content.Intent
import android.content.res.Configuration
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.ui.graphics.toArgb
import androidx.core.app.NotificationManagerCompat

// Android 16 (API 36) Live Updates: an ongoing notification that requests promotion
// (`NotificationCompat.Builder.setRequestPromotedOngoing`) is shown at the top of the
// shade, on the lock screen and as a status-bar chip. Below API 36 the request is an
// ignored extra, so the same notification posts as an ordinary ongoing one.

/** True only where Live Updates exist (API 36+) and the user turned them off for
 *  this app: the one case worth explaining in Profile (the ProfilePlatform.kt seam). */
internal actual fun liveUpdatesTurnedOff(ctx: Context): Boolean =
    Build.VERSION.SDK_INT >= Build.VERSION_CODES.BAKLAVA &&
        !NotificationManagerCompat.from(ctx).canPostPromotedNotifications()

/** The system screen that turns Live Updates on or off for this app (API 36+), else
 *  the app's notification settings (its details page before API 26). */
internal fun promotedNotificationSettingsIntent(ctx: Context): Intent = when {
    Build.VERSION.SDK_INT >= Build.VERSION_CODES.BAKLAVA ->
        Intent(Settings.ACTION_APP_NOTIFICATION_PROMOTION_SETTINGS)
            .putExtra(Settings.EXTRA_APP_PACKAGE, ctx.packageName)
    else -> appNotificationSettingsIntent(ctx)
}

/** This app's notification settings (its details page before API 26). */
internal fun appNotificationSettingsIntent(ctx: Context): Intent =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
        Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(Settings.EXTRA_APP_PACKAGE, ctx.packageName)
    } else {
        appDetailsIntent(ctx)
    }

/** This app's details page in the system settings. */
internal fun appDetailsIntent(ctx: Context): Intent =
    Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", ctx.packageName, null))

/** Open [promotedNotificationSettingsIntent]. The platform documents that the
 *  promotion screen may be missing on some builds, so that case falls back to the
 *  app's notification settings. */
internal actual fun openPromotedNotificationSettings(ctx: Context) =
    ctx.openFirst(promotedNotificationSettingsIntent(ctx), appNotificationSettingsIntent(ctx))

/** Start the first of [intents] that opens; none does: nothing happens (a device
 *  without the Settings screen must not crash the caller). */
internal fun Context.openFirst(vararg intents: Intent) {
    intents.any { runCatching { startActivity(it) }.isSuccess }
}

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

/**
 * The colour every lollop notification carries ([NotificationCompat.Builder.setColor]:
 * the small icon's circle, the app name, action labels): the app theme's accent
 * for the system's light or dark shade, the wallpaper accent where the app uses it
 * (Android 12+ with the system accent on), through the app's contrast guard. Never
 * colorized, which a Live Update must not be. A process started for a notification
 * loads [AppSettings] first.
 */
internal fun notificationAccent(ctx: Context): Int {
    val night = (ctx.resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) == Configuration.UI_MODE_NIGHT_YES
    val theme = AppSettings.theme.value
    val base = if (night) theme.dark else theme.light
    val palette = if (AppSettings.dynamicAccent.value && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        base.withDynamicAccent((if (night) dynamicDarkColorScheme(ctx) else dynamicLightColorScheme(ctx)).primary)
    } else {
        base
    }
    return palette.accent.toArgb()
}
