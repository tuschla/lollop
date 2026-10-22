package app.milestone

import android.content.Context
import android.content.Intent
import android.content.res.Configuration as UiConfiguration
import android.graphics.drawable.ColorDrawable
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.graphics.toArgb
import androidx.core.splashscreen.SplashScreen.Companion.installSplashScreen
import java.io.File
import kotlinx.coroutines.flow.MutableStateFlow
import org.osmdroid.config.Configuration

class MainActivity : ComponentActivity() {
    // Set true when the run-tracking notification's contentIntent (or any intent
    // carrying EXTRA_OPEN_TRACKING) delivers, CoachScreen observes it and, if a
    // run is genuinely live, reopens the tracking screen. A flow rather than a
    // plain field so an already-running Activity reacts to onNewIntent too.
    private val openTrackingRequest = MutableStateFlow(false)

    override fun onCreate(savedInstanceState: Bundle?) {
        // Theme-aware launch/splash frame. The manifest theme (Theme.Milestone)
        // already follows the SYSTEM light/dark via the values / values-night
        // split. If the user picked an explicit in-app DarkMode (Light/Dark),
        // force the matching splash theme here, BEFORE installSplashScreen(),
        // so the pre-Compose frame honors that choice too. AppSettings is
        // loaded first for exactly that (it only reads SharedPreferences).
        //
        // PLATFORM LIMITATION (accepted): on API 31+ the COLD-START splash is
        // drawn by SystemUI from the MANIFEST theme before this process even
        // runs, so an explicit Light/Dark override can NOT tint that very first
        // frame: it follows the SYSTEM setting for the whole system-drawn
        // splash, until our own window takes over. setTheme() here still
        // governs everything the app itself draws: the pre-31 splash, warm
        // starts, and the post-splash windowBackground (no opposite-color flash
        // once Compose loads). The only platform lever over the system splash,
        // UiModeManager.setApplicationNightMode(), is deliberately NOT used: it
        // recreates the app like a config change on every toggle and has no
        // documented follow-system reset for our third "System" choice.
        AppSettings.load(this)
        when (AppSettings.darkMode.value) {
            DarkMode.Light -> setTheme(R.style.Theme_Milestone_Forced_Light)
            DarkMode.Dark -> setTheme(R.style.Theme_Milestone_Forced_Dark)
            DarkMode.System -> {} // keep the night-qualified manifest theme
        }
        // Hold the splash until boot has hydrated the first model, so a cold start
        // goes straight from the splash to content instead of to a second spinner.
        // Not after an import restart: that in-app start gets the icon-less solid
        // splash, so it lets go at once for the "Restoring your data…" loading screen.
        val restoringImport = setupPrefs().getInt(KEY_RESTORED_ENTRIES, -1) >= 0
        installSplashScreen().setKeepOnScreenCondition { !restoringImport && EventDispatcher.model.value == null }
        super.onCreate(savedInstanceState)
        // osmdroid needs a config + user agent set before any MapView inflates.
        Configuration.getInstance().load(this, getSharedPreferences("osmdroid", Context.MODE_PRIVATE))
        Configuration.getInstance().osmdroidBasePath = File(cacheDir, "osmdroid")
        Configuration.getInstance().osmdroidTileCache = File(cacheDir, "osmdroid/tiles")
        Configuration.getInstance().userAgentValue = packageName
        if (intent?.getBooleanExtra(EXTRA_OPEN_TRACKING, false) == true) {
            openTrackingRequest.value = true
            // Consume it once: without this, every config change (rotation) re-reads
            // getIntent() and force-reopens the tracker mid-run.
            intent.removeExtra(EXTRA_OPEN_TRACKING)
            setIntent(intent)
        }
        // "Open with" / "Share to" run import (GPX/TCX/FIT). Only on a FRESH create
        // that the share itself started: a rotation or process-restore re-delivers
        // the same getIntent(), and reopening the task from recents replays the
        // original VIEW intent (its URI grant possibly expired); neither may import
        // the files again.
        val fromRecents = (intent?.flags ?: 0) and Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY != 0
        if (savedInstanceState == null && !fromRecents) RunImport.importRuns(this, RunImport.urisFrom(intent))
        // The window ground under the first Compose frame (and the import restart's
        // "Restoring your data…" screen) is the resolved palette's, not the theme's
        // plain white/black windowBackground.
        val nightUi = when (AppSettings.darkMode.value) {
            DarkMode.System -> (resources.configuration.uiMode and UiConfiguration.UI_MODE_NIGHT_MASK) == UiConfiguration.UI_MODE_NIGHT_YES
            DarkMode.Light -> false
            DarkMode.Dark -> true
        }
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
                val dark = bgTop.luminance() < 0.5f
                DisposableEffect(bgTop) {
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
                    onDispose {}
                }
                Surface(modifier = Modifier.fillMaxSize(), color = BgTop) {
                    CoachScreen(openTrackingRequest)
                }
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        if (intent.getBooleanExtra(EXTRA_OPEN_TRACKING, false)) {
            openTrackingRequest.value = true
            // Consume it so a later rotation doesn't re-force the tracker open.
            intent.removeExtra(EXTRA_OPEN_TRACKING)
            setIntent(intent)
        }
        // A share/open into an already-running Activity imports too.
        RunImport.importRuns(this, RunImport.urisFrom(intent))
    }

    companion object {
        /** Intent extra asking the shell to reopen the live tracking screen. */
        const val EXTRA_OPEN_TRACKING = "app.milestone.extra.OPEN_TRACKING"

        // Theme-ground alpha for the nav-bar scrim, and the dark scrim androidx's
        // default enableEdgeToEdge() uses where light icons are the only option.
        private const val NAV_SCRIM_ALPHA = 0.9f
        private val NAV_SCRIM_DARK = android.graphics.Color.argb(0x80, 0x1b, 0x1b, 0x1b)
    }
}
