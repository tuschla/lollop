package app.milestone

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.core.app.NotificationManagerCompat

// Android 16 (API 36) Live Updates: an ongoing notification that requests promotion
// (`NotificationCompat.Builder.setRequestPromotedOngoing`) is shown at the top of the
// shade, on the lock screen and as a status-bar chip. Below API 36 the request is an
// ignored extra, so the same notification posts as an ordinary ongoing one.

/** True only where Live Updates exist (API 36+) and the user turned them off for
 *  this app: the one case worth explaining in Profile. */
internal fun liveUpdatesTurnedOff(ctx: Context): Boolean =
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
internal fun openPromotedNotificationSettings(ctx: Context) {
    try {
        ctx.startActivity(promotedNotificationSettingsIntent(ctx))
    } catch (_: ActivityNotFoundException) {
        runCatching { ctx.startActivity(appNotificationSettingsIntent(ctx)) }
    }
}
