package app.milestone

import android.os.Build
import android.view.WindowManager
import androidx.activity.compose.BackHandler as ActivityBackHandler
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.SideEffect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.window.DialogWindowProvider

@Composable
internal actual fun BackHandler(enabled: Boolean, onBack: () -> Unit) = ActivityBackHandler(enabled, onBack)

/** adjustResize on the dialog's own window (the platform otherwise pans it, OK
 *  under the keyboard). */
@Composable
internal actual fun DialogResizesForKeyboard() {
    val window = (LocalView.current.parent as? DialogWindowProvider)?.window
    SideEffect { window?.setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE) }
}

/**
 * The sheet runs in its own dialog window, which on gesture-less (3-button)
 * navigation gets the system's contrast scrim, a near-white band under a light
 * palette. Turning it off lets the sheet's own surface (already drawn under the
 * nav inset) be the band, the same theme-derived ground the activity window gets
 * in MainActivity. API 35+ ignores the nav bar colour (edge-to-edge), so only the
 * scrim switch applies there.
 */
@Composable
internal actual fun SheetNavBarScrim() {
    val window = (LocalView.current.parent as? DialogWindowProvider)?.window ?: return
    SideEffect {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.VANILLA_ICE_CREAM) {
            @Suppress("DEPRECATION")
            window.navigationBarColor = android.graphics.Color.TRANSPARENT
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) window.isNavigationBarContrastEnforced = false
    }
}

@Composable
internal actual fun platformAccent(dark: Boolean): Color? {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return null
    val ctx = LocalContext.current
    return (if (dark) dynamicDarkColorScheme(ctx) else dynamicLightColorScheme(ctx)).primary
}

@Composable
internal actual fun currentAppLocale(): PlatformLocale = appLocale(LocalConfiguration.current.locales)

@Composable
internal actual fun screenHeightDp(): Int = LocalConfiguration.current.screenHeightDp

@Composable
internal actual fun currentPlatformContext(): PlatformContext = LocalContext.current
