package app.milestone

import androidx.compose.runtime.Composable

/** While composed, the screen stays on (no dimming, no auto-lock); leaving
 *  composition restores what was there before. Android: the host view's
 *  `keepScreenOn`; iOS: `UIApplication.idleTimerDisabled`. */
@Composable
internal expect fun KeepScreenOn()

/** Open the app's own page in the system settings (permissions, notifications).
 *  Nothing happens where the platform has no such page. */
internal expect fun openAppSettings(ctx: PlatformContext)
