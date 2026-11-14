package app.milestone

import androidx.compose.runtime.Composable
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.backhandler.BackHandler as ComposeBackHandler

/** The UIKit back gesture as Compose Multiplatform delivers it. CMP 1.12 marks its
 *  BackHandler deprecated in favour of the navigationevent handlers it is now
 *  built on; it keeps Android's `BackHandler(enabled, onBack)` contract. */
@OptIn(ExperimentalComposeUiApi::class)
@Suppress("DEPRECATION")
@Composable
internal actual fun BackHandler(enabled: Boolean, onBack: () -> Unit) = ComposeBackHandler(enabled, onBack)

/** Nothing to set: a Compose Multiplatform dialog on iOS already keeps its
 *  content above the keyboard (`DialogProperties.useSoftwareKeyboardInset`). */
@Composable
internal actual fun DialogResizesForKeyboard() = Unit

/** Nothing to set: iOS draws no navigation bar or scrim under a sheet. */
@Composable
internal actual fun SheetNavBarScrim() = Unit

/** iOS has no wallpaper accent; the theme's own accent stays. */
@Composable
internal actual fun platformAccent(dark: Boolean): Color? = null

/** iOS restarts the app on a language change, so the launch-time locale holds. */
@Composable
internal actual fun currentAppLocale(): PlatformLocale = appLocale()

/** The Compose view's height (the whole screen, safe areas included). */
@Composable
internal actual fun screenHeightDp(): Int =
    with(LocalDensity.current) { LocalWindowInfo.current.containerSize.height.toDp().value.toInt() }

@Composable
internal actual fun currentPlatformContext(): PlatformContext = PlatformContext.INSTANCE
