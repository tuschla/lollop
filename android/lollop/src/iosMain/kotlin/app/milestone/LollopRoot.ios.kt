package app.milestone

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import kotlinx.coroutines.flow.MutableStateFlow
import platform.Foundation.NSURL

// The iOS app's entry points into the shared app: the root composable the
// `ComposeUIViewController` hosts, and the URLs the system opens the app with.

// The iOS host's deep-link state: a `lollop://open/<dest>` URL or a reminder tap.
private val openRequests: OpenRequests = MutableStateFlow(null)

// Once per process: the settings the theme reads, the reminder schedule.
private var started = false

private fun startOnce() {
    if (started) return
    started = true
    AppSettings.load(PlatformContext.INSTANCE)
    startIosReminders()
}

/**
 * The whole app: boot (probe, restore the event log, SetToday, compaction, the
 * auto plan), then the tabs, sheets and snackbars, under the app's theme. Draws
 * edge to edge and pads itself for the safe area. [installCoreEngine] (the iOS
 * framework's `LollopApp.start()`) must have run before the first composition.
 */
@Composable
public fun LollopIosRoot() {
    remember { startOnce() }
    MilestoneTheme {
        Surface(modifier = Modifier.fillMaxSize().traversalGroup(), color = BgTop) {
            LollopApp(openRequests)
        }
    }
}

/**
 * A URL the system opened the app with (SwiftUI `onOpenURL`): `lollop://open/<dest>`
 * opens that destination once boot has run; a `file://` URL of a GPX, TCX or FIT file
 * (Files "Open in", AirDrop, a share) is imported like Android's open intent, with
 * the summary in a snackbar. Main thread; safe before or after the root composes.
 */
public fun LollopIosOpen(url: String) {
    openDestOfUrl(url)?.let {
        requestOpen(it)
        return
    }
    val file = NSURL.URLWithString(url)?.takeIf { it.isFileURL() } ?: return
    importRunFiles(listOf(file))
}

/** Open the app at [dest] once boot has run (a reminder tap, an open URL). Main thread. */
internal fun requestOpen(dest: OpenDest) {
    openRequests.value = dest
}
