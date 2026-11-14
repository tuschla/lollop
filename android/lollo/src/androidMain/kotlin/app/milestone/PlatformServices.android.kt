package app.milestone

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.platform.LocalView

@Composable
internal actual fun KeepScreenOn() {
    val view = LocalView.current
    DisposableEffect(view) {
        val prev = view.keepScreenOn
        view.keepScreenOn = true
        onDispose { view.keepScreenOn = prev }
    }
}

internal actual fun openAppSettings(ctx: PlatformContext) = ctx.openFirst(appDetailsIntent(ctx))

/** This app's notification settings (its details page before API 26). */
internal fun appNotificationSettingsIntent(ctx: Context): Intent =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
        Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(Settings.EXTRA_APP_PACKAGE, ctx.packageName)
    } else {
        appDetailsIntent(ctx)
    }

internal fun appDetailsIntent(ctx: Context): Intent =
    Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", ctx.packageName, null))

/** Start the first of [intents] that opens; none does: nothing happens (a device
 *  without the Settings screen must not crash the caller). */
internal fun Context.openFirst(vararg intents: Intent) {
    intents.any { intent ->
        runCatching { startActivity(intent) }
            .onFailure { PlatformLog.w("Settings", "couldn't open ${intent.action}", it) }
            .isSuccess
    }
}
