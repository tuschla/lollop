package app.milestone

import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

// The few UI calls that reach the platform's window or system: back navigation,
// a dialog window's soft-input and nav-bar handling, the wallpaper accent.

/** Calls [onBack] on the system back gesture or button while [enabled]; the
 *  innermost enabled handler wins. */
@Composable
internal expect fun BackHandler(enabled: Boolean = true, onBack: () -> Unit)

/** Inside a dialog: the dialog's window shrinks for the soft keyboard, so content
 *  at its bottom (a field's error line) stays above the keyboard. */
@Composable
internal expect fun DialogResizesForKeyboard()

/** Inside a sheet's dialog window: no system scrim under the navigation bar, so the
 *  sheet's own surface is the band behind it ([AppSheet]). */
@Composable
internal expect fun SheetNavBarScrim()

/** The wallpaper accent for a [dark] or light ground (Material You), or null where
 *  the platform has none. */
@Composable
internal expect fun platformAccent(dark: Boolean): Color?

/** [appLocale] for the configuration the composition reads, so a language change
 *  recomposes the dates that use it. */
@Composable
internal expect fun currentAppLocale(): PlatformLocale

/** The app window's height in dp, as the platform reports it for layout
 *  (Android: `Configuration.screenHeightDp`, without the system bars). */
@Composable
internal expect fun screenHeightDp(): Int

/** The context platform services hang off, for the composition (Android: the
 *  Activity's `LocalContext`). */
@Composable
internal expect fun currentPlatformContext(): PlatformContext
