package app.milestone

import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import platform.Foundation.NSURL
import platform.UIKit.UIApplication
import platform.UIKit.UIApplicationOpenSettingsURLString

// Composables that hold the screen on; the idle timer comes back when the last
// one leaves. Composition runs on the main thread, so a plain counter is enough.
private var screenOnHolds = 0

@Composable
internal actual fun KeepScreenOn() {
    DisposableEffect(Unit) {
        screenOnHolds++
        UIApplication.sharedApplication.idleTimerDisabled = true
        onDispose {
            screenOnHolds--
            if (screenOnHolds == 0) UIApplication.sharedApplication.idleTimerDisabled = false
        }
    }
}

/** The app's page in the Settings app. */
internal actual fun openAppSettings(ctx: PlatformContext) {
    val url = NSURL.URLWithString(UIApplicationOpenSettingsURLString) ?: return
    UIApplication.sharedApplication.openURL(url, options = emptyMap<Any?, Any?>(), completionHandler = null)
}
