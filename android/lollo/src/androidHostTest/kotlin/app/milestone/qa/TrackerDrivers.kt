package app.milestone.qa

import android.content.Context
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.ComposeTestRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.core.app.ApplicationProvider
import app.milestone.AppRuntime
import app.milestone.OpenDest
import app.milestone.R
import app.milestone.RunSession
import app.milestone.openIntent
import kotlinx.serialization.json.JsonObject

/** Open the tracker on the run being recorded (as its notification does, e.g. after
 *  [QaHarness.seedRecordingRun]) and wait for its Stop button and the boot. */
internal fun ComposeTestRule.launchOnTracker() {
    lastScenario = launchMain(openIntent(ApplicationProvider.getApplicationContext<Context>(), OpenDest.Track))
    waitForNode(hasText(str(R.string.track_btn_stop)), 30_000)
    waitUntil(30_000) { AppRuntime.bootHandled }
}

/** On the recording tracker: Stop, and wait for the Finish review. */
internal fun ComposeTestRule.stopToFinishReview() {
    onNodeWithText(str(R.string.track_btn_stop)).performClick()
    waitForNode(hasText(str(R.string.track_finish_title)))
}

/**
 * On the Finish review: tap Save run and stand in for the foreground service's stop
 * confirmation (no service runs under Robolectric). The clock is held while the tap
 * lands so the screen's stop-confirm timeout can't be auto-advanced past before
 * `tracking` flips; then wait for the WHOLE save (event-log append + session reset),
 * so no save turn is left queued on the process-wide serial consumer for a later
 * test to trip over. Returns the saved `LogRunTrack`'s payload.
 */
internal fun ComposeTestRule.saveRunAndAwait(): JsonObject {
    mainClock.autoAdvance = false
    try {
        onNodeWithText(str(R.string.track_btn_save_run)).performClick()
        RunSession.markStopped()
    } finally {
        mainClock.autoAdvance = true
    }
    waitForState {
        journeyLog().any { it.variant == "LogRunTrack" } && !RunSession.saving.value && RunSession.points.value.isEmpty()
    }
    return journeyLog().last { it.variant == "LogRunTrack" }.payload
}
