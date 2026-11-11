package app.milestone

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import app.milestone.resources.Res
import app.milestone.resources.main_core_unavailable_message
import app.milestone.resources.main_core_unavailable_title
import app.milestone.resources.main_restoring_data
import app.milestone.resources.profile_export_data
import org.jetbrains.compose.resources.stringResource

/**
 * The app: boot (the loading state until the first model, or the core-unavailable
 * state), then the scaffold with its tabs, sheets and snackbars. Each platform's host
 * calls it under [MilestoneTheme]: Android's `MainActivity`, iOS's `LollopIosRoot`.
 * [openRequest] is the host's deep-link state (an open intent, a URL, a reminder tap).
 */
@Composable
internal fun LollopApp(openRequest: OpenRequests) {
    val session = rememberCoachSession(openRequest)

    val coreUnavailable by EventDispatcher.coreUnavailable.collectAsState()
    if (coreUnavailable) {
        CoreUnavailableScreen()
        return
    }

    if (!session.loaded) {
        val ctx = currentPlatformContext()
        // An import restart is still replaying the restored log (the key goes once boot runs).
        val restoring = remember { restoringImport(ctx.setupStore()) }
        Box(Modifier.fillMaxSize().background(BgTop), contentAlignment = Alignment.Center) {
            Column(
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(Space.Card.dp),
            ) {
                CircularProgressIndicator(color = Accent)
                if (restoring) Text(stringResource(Res.string.main_restoring_data), color = OnBgMuted, style = Type.Body)
            }
        }
        return
    }

    CoachScaffold(session)
}

/**
 * Boot found the core unusable ([DurableCore.probe]): say so instead of rendering an
 * empty model as a fresh install. Nothing here can dispatch an event (the core
 * refuses them anyway, [DurableCore.unavailable]); the one action is the raw-log
 * export, which reads the durable log in pure Kotlin ([exportDataTo]), so the
 * user's data can leave the phone before an update or reinstall.
 */
@Composable
internal fun CoreUnavailableScreen() {
    val exportData = rememberDataExport()
    Box(
        Modifier.fillMaxSize().background(BgTop).safeDrawingPadding().padding(Space.Screen.dp),
        contentAlignment = Alignment.Center,
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(Space.Card.dp)) {
            SheetTitle(stringResource(Res.string.main_core_unavailable_title))
            Text(stringResource(Res.string.main_core_unavailable_message), color = OnBgBody, style = Type.Body)
            PrimaryButton(stringResource(Res.string.profile_export_data), onClick = exportData, modifier = Modifier.fillMaxWidth())
        }
    }
}
