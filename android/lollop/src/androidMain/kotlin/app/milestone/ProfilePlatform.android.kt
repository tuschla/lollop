package app.milestone

import android.content.Context
import android.content.Intent
import android.os.Build
import android.provider.Settings
import androidx.core.app.NotificationManagerCompat

/** Dynamic (wallpaper) colour exists from Android 12 (API 31). */
internal actual val platformHasDynamicAccent: Boolean
    get() = Build.VERSION.SDK_INT >= Build.VERSION_CODES.S

internal actual val liveUpdatesSwitch: LiveUpdatesSwitch? = AndroidLiveUpdates

/** Android 16 (API 36) Live Updates (see LiveUpdates.kt); below API 36 there is
 *  nothing to turn off. */
internal object AndroidLiveUpdates : LiveUpdatesSwitch {
    override fun isOff(ctx: Context): Boolean =
        Build.VERSION.SDK_INT >= Build.VERSION_CODES.BAKLAVA &&
            !NotificationManagerCompat.from(ctx).canPostPromotedNotifications()

    /** Opens [promotedNotificationSettingsIntent]. The platform documents that the
     *  promotion screen may be missing on some builds, so that case falls back to the
     *  app's notification settings. */
    override fun open(ctx: Context) =
        ctx.openFirst(promotedNotificationSettingsIntent(ctx), appNotificationSettingsIntent(ctx))
}

/** The system screen that turns Live Updates on or off for this app (API 36+), else
 *  the app's notification settings. */
internal fun promotedNotificationSettingsIntent(ctx: Context): Intent = when {
    Build.VERSION.SDK_INT >= Build.VERSION_CODES.BAKLAVA ->
        Intent(Settings.ACTION_APP_NOTIFICATION_PROMOTION_SETTINGS)
            .putExtra(Settings.EXTRA_APP_PACKAGE, ctx.packageName)
    else -> appNotificationSettingsIntent(ctx)
}
