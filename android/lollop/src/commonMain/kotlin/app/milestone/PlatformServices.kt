package app.milestone

import androidx.compose.runtime.Composable

// The platform services common screens call outside composition's own APIs: a
// transient notice, holding the screen awake, the app's page in the system settings.

/** A short notice that outlives the screen that raised it. Android: a Toast
 *  (`LENGTH_LONG` when [long]); iOS: the app's [UiMessages] snackbar. */
internal expect fun showNotice(ctx: PlatformContext, text: String, long: Boolean = false)

/** While composed, the screen stays on (no dimming, no auto-lock); leaving
 *  composition restores what was there before. Android: the host view's
 *  `keepScreenOn`; iOS: `UIApplication.idleTimerDisabled`. */
@Composable
internal expect fun KeepScreenOn()

/** Open the app's own page in the system settings (permissions, notifications).
 *  Nothing happens where the platform has no such page. */
internal expect fun openAppSettings(ctx: PlatformContext)
