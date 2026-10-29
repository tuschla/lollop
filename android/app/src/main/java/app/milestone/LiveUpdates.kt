package app.milestone

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.core.app.NotificationManagerCompat

/**
 * Android 16 (API 36) Live Updates: an ongoing notification that requests promotion
 * (`NotificationCompat.Builder.setRequestPromotedOngoing`) is shown at the top of the
 * shade, on the lock screen and as a status-bar chip. Below API 36 the request is an
 * ignored extra, so the same notification posts as an ordinary ongoing one.
 */

/** The status chip shows its text whole up to this many characters
 *  (developer.android.com, "Live Updates", "Status chip appearance"). */
internal const val LIVE_CHIP_MAX_CHARS = 7

/** True when the system will promote this app's notifications. False below API 36
 *  (no Live Updates) and when the user turned Live Updates off for the app. */
internal fun canPostPromotedNotifications(ctx: Context): Boolean =
    NotificationManagerCompat.from(ctx).canPostPromotedNotifications()

/** True only where Live Updates exist (API 36+) and the user turned them off for
 *  this app: the one case worth explaining in Profile. */
internal fun liveUpdatesTurnedOff(ctx: Context): Boolean =
    Build.VERSION.SDK_INT >= Build.VERSION_CODES.BAKLAVA && !canPostPromotedNotifications(ctx)

/** The system screen that turns Live Updates on or off for this app (API 36+), else
 *  the app's notification settings (its details page before API 26). */
internal fun promotedNotificationSettingsIntent(ctx: Context): Intent = when {
    Build.VERSION.SDK_INT >= Build.VERSION_CODES.BAKLAVA ->
        Intent(Settings.ACTION_APP_NOTIFICATION_PROMOTION_SETTINGS)
            .putExtra(Settings.EXTRA_APP_PACKAGE, ctx.packageName)
    else -> appNotificationSettingsIntent(ctx)
}

private fun appNotificationSettingsIntent(ctx: Context): Intent =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
        Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(Settings.EXTRA_APP_PACKAGE, ctx.packageName)
    } else {
        Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", ctx.packageName, null))
    }

/** Open [promotedNotificationSettingsIntent]. The platform documents that the
 *  promotion screen may be missing on some builds, so that case falls back to the
 *  app's notification settings. */
internal fun openPromotedNotificationSettings(ctx: Context) {
    try {
        ctx.startActivity(promotedNotificationSettingsIntent(ctx))
    } catch (_: ActivityNotFoundException) {
        runCatching { ctx.startActivity(appNotificationSettingsIntent(ctx)) }
    }
}
