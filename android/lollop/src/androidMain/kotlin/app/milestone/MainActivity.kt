package app.milestone

import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.res.Configuration
import android.graphics.drawable.ColorDrawable
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.runtime.SideEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.toArgb
import androidx.core.splashscreen.SplashScreen.Companion.installSplashScreen
import kotlinx.coroutines.flow.MutableStateFlow

class MainActivity : ComponentActivity() {
    // The latest [ACTION_OPEN] destination (widget, Quick Settings tile, a run or
    // rest notification) not yet applied: the app's [OpenRequests]. CoachScaffold
    // applies and clears it once boot has run, so a cold start waits for the model
    // before it navigates.
    private val openRequest: OpenRequests = MutableStateFlow(null)

    // Counts, times and dates on every screen in Latin digits (AppLocale.kt). A
    // language change recreates the Activity, so the base follows it.
    override fun attachBaseContext(newBase: Context) = super.attachBaseContext(newBase.withLatinDigits())

    override fun onCreate(savedInstanceState: Bundle?) {
        // The runtime (its settings) first: an explicit Light/Dark picks the splash
        // theme before installSplashScreen(). On API 31+ the system-drawn cold-start
        // splash still follows the system mode (UiModeManager.setApplicationNightMode
        // would recreate the app on every toggle and has no "follow system" reset).
        AppRuntime.start(applicationContext)
        when (AppSettings.darkMode.value) {
            DarkMode.Light -> setTheme(R.style.Theme_Milestone_Forced_Light)
            DarkMode.Dark -> setTheme(R.style.Theme_Milestone_Forced_Dark)
            DarkMode.System -> {} // keep the night-qualified manifest theme
        }
        // Hold the splash until boot has hydrated the first model, so a cold start
        // goes straight from the splash to content instead of to a second spinner.
        // Not after an import restart: that in-app start gets the icon-less solid
        // splash, so it lets go at once for the "Restoring your data…" loading screen.
        // A core found unusable at boot never hydrates a model: let go for its error
        // screen instead of holding the splash forever.
        val restoringImport = restoringImport(setupStore())
        installSplashScreen().setKeepOnScreenCondition {
            !restoringImport && EventDispatcher.model.value == null && !EventDispatcher.coreUnavailable.value
        }
        super.onCreate(savedInstanceState)
        // "Open with" / "Share to" run import (GPX/TCX/FIT) and an open intent
        // (widget, tile, notification) act only on a FRESH create that the intent
        // itself started: a rotation or process-restore re-delivers the same
        // getIntent(), and reopening the task from recents replays the original
        // intent (a VIEW's URI grant possibly expired); neither may act again.
        val fromRecents = (intent?.flags ?: 0) and Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY != 0
        if (savedInstanceState == null && !fromRecents) {
            RunImport.importRuns(this, RunImport.urisFrom(intent))
            openDestOf(intent)?.let { openRequest.value = it }
            cancelTappedNotification(this, intent)
        }
        // The window ground under the first Compose frame (and the import restart's
        // "Restoring your data…" screen) is the resolved palette's, not the theme's
        // plain white/black windowBackground.
        val systemNight = (resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) == Configuration.UI_MODE_NIGHT_YES
        val nightUi = AppSettings.darkMode.value.isDark(systemNight)
        val theme = AppSettings.theme.value
        window.setBackgroundDrawable(ColorDrawable((if (nightUi) theme.dark else theme.light).bgTop.toArgb()))
        setContent {
            MilestoneTheme {
                // Draw edge-to-edge with the system-bar icons and the nav-bar scrim
                // following the RESOLVED theme (which may be forced Light on a dark OS
                // or vice-versa), not the system uiMode, so bar icons never vanish into
                // a same-tone bar. The nav scrim is the theme ground itself: an explicit
                // light/dark style turns off the system's contrast scrim, which would
                // otherwise paint a near-white (or cool grey) band under the palette.
                val bgTop = LocalPalette.current.bgTop
                val dark = LocalPalette.current.isDark
                SideEffect {
                    window.setBackgroundDrawable(ColorDrawable(bgTop.toArgb()))
                    val navScrim = bgTop.copy(alpha = NAV_SCRIM_ALPHA).toArgb()
                    enableEdgeToEdge(
                        statusBarStyle = SystemBarStyle.auto(
                            android.graphics.Color.TRANSPARENT,
                            android.graphics.Color.TRANSPARENT,
                        ) { dark },
                        navigationBarStyle = if (dark) {
                            SystemBarStyle.dark(navScrim)
                        } else {
                            // API 24-25 cannot draw dark nav icons, so they get the dark scrim.
                            SystemBarStyle.light(navScrim, NAV_SCRIM_DARK)
                        },
                    )
                }
                Surface(modifier = Modifier.fillMaxSize().traversalGroup(), color = BgTop) {
                    LollopApp(openRequest)
                }
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        // A widget / tile / notification tap while the Activity is alive. It only
        // reaches here as a new intent, so a rotation never replays it.
        openDestOf(intent)?.let { openRequest.value = it }
        cancelTappedNotification(this, intent)
        // A share/open into an already-running Activity imports too.
        RunImport.importRuns(this, RunImport.urisFrom(intent))
    }

    companion object {
        // Theme-ground alpha for the nav-bar scrim, and the dark scrim androidx's
        // default enableEdgeToEdge() uses where light icons are the only option.
        private const val NAV_SCRIM_ALPHA = 0.9f
        private val NAV_SCRIM_DARK = android.graphics.Color.argb(0x80, 0x1b, 0x1b, 0x1b)
    }
}

// The open intents ([OpenDest] as an explicit activity intent): built by the widget,
// the Quick Settings tile and the notifications, read by [MainActivity].

internal const val ACTION_OPEN = "app.milestone.action.OPEN"
internal const val EXTRA_DEST = "dest"

/** The notification an open intent takes down: an action tap does not auto-cancel it. */
private const val EXTRA_CANCEL_NOTIFICATION = "app.milestone.cancel_notification"

/** The explicit intent that opens [MainActivity] at [dest]; with [cancels], opening
 *  it takes that notification down ([cancelTappedNotification]). */
internal fun openIntent(ctx: Context, dest: OpenDest, cancels: Int? = null): Intent =
    Intent(ACTION_OPEN)
        .setClass(ctx, MainActivity::class.java)
        .putExtra(EXTRA_DEST, dest.wire)
        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        .apply { if (cancels != null) putExtra(EXTRA_CANCEL_NOTIFICATION, cancels) }

/**
 * Immutable activity PendingIntent for [openIntent]. PendingIntents match on the
 * intent without its extras, so each destination gets its own request code.
 */
internal fun openPendingIntent(ctx: Context, dest: OpenDest): PendingIntent =
    PendingIntent.getActivity(
        ctx,
        AndroidIds.OPEN_REQUEST_BASE + dest.ordinal,
        openIntent(ctx, dest),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

/** The destination an incoming intent asks for, or null when it isn't an open intent. */
internal fun openDestOf(intent: Intent?): OpenDest? =
    if (intent?.action == ACTION_OPEN) OpenDest.fromWire(intent.getStringExtra(EXTRA_DEST)) else null

/** [intent] came from a notification action that names its notification: take it down. */
internal fun cancelTappedNotification(ctx: Context, intent: Intent?) {
    if (intent?.hasExtra(EXTRA_CANCEL_NOTIFICATION) != true) return
    ctx.getSystemService(NotificationManager::class.java)?.cancel(intent.getIntExtra(EXTRA_CANCEL_NOTIFICATION, 0))
}
