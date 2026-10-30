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
import app.milestone.widget.WidgetSync
import java.io.File
import kotlinx.coroutines.flow.MutableStateFlow
import org.osmdroid.config.Configuration

class MainActivity : ComponentActivity() {
    // The latest [ACTION_OPEN] destination (widget, Quick Settings tile, a run or
    // rest notification) not yet applied. CoachScaffold applies and clears it once
    // boot has run, so a cold start waits for the model before it navigates.
    private val openRequest = MutableStateFlow<OpenDest?>(null)

    override fun onCreate(savedInstanceState: Bundle?) {
        // AppSettings first: an explicit Light/Dark picks the splash theme before
        // installSplashScreen(). On API 31+ the system-drawn cold-start splash still
        // follows the system mode (UiModeManager.setApplicationNightMode would
        // recreate the app on every toggle and has no "follow system" reset).
        AppSettings.load(this)
        // Home screen widget: every published model refreshes its snapshot; the
        // Quick Settings tile also redraws when a run starts or stops.
        WidgetSync.start(applicationContext)
        TileSync.start(applicationContext)
        // A force stop clears the session reminder's alarm; any start sets it again.
        SessionReminder.reschedule(applicationContext)
        // A shown "Start run" reminder goes once a run records.
        SessionReminder.watchRecording(applicationContext)
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
        val restoringImport = setupPrefs().getInt(KEY_RESTORED_ENTRIES, -1) >= 0
        installSplashScreen().setKeepOnScreenCondition {
            !restoringImport && EventDispatcher.model.value == null && !EventDispatcher.coreUnavailable.value
        }
        super.onCreate(savedInstanceState)
        // osmdroid needs a config + user agent set before any MapView inflates.
        Configuration.getInstance().load(this, getSharedPreferences("osmdroid", Context.MODE_PRIVATE))
        Configuration.getInstance().osmdroidBasePath = File(cacheDir, "osmdroid")
        Configuration.getInstance().osmdroidTileCache = File(cacheDir, "osmdroid/tiles")
        Configuration.getInstance().userAgentValue = packageName
        // "Open with" / "Share to" run import (GPX/TCX/FIT). Only on a FRESH create
        // that the share itself started: a rotation or process-restore re-delivers
        // the same getIntent(), and reopening the task from recents replays the
        // original VIEW intent (its URI grant possibly expired); neither may import
        // the files again.
        val fromRecents = (intent?.flags ?: 0) and Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY != 0
        if (savedInstanceState == null && !fromRecents) RunImport.importRuns(this, RunImport.urisFrom(intent))
        // An open intent (widget, tile, notification) navigates on the same terms: a
        // rotation or process-restore re-delivers getIntent() with saved state, and a
        // relaunch from recents replays it; neither navigates again.
        if (savedInstanceState == null && !fromRecents) {
            openDestOf(intent)?.let { openRequest.value = it }
            SessionReminder.onOpenIntent(this, intent)
        }
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
                    CoachScreen(openRequest)
                }
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        // A widget / tile / notification tap while the Activity is alive. It only
        // reaches here as a new intent, so a rotation never replays it.
        openDestOf(intent)?.let { openRequest.value = it }
        SessionReminder.onOpenIntent(this, intent)
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
