package app.milestone

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/** Tracker copy that a past bug made untruthful or sloppy (URH-6), pinned against the
 *  real string resources. */
@RunWith(RobolectricTestRunner::class)
class TrackingCopyTest {

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @Test
    fun nothingRecordedToastDoesNotOfferToKeepADiscardedRun() {
        // Shown AFTER the empty capture was cleared: nothing is left to keep.
        val toast = ctx.getString(R.string.track_toast_not_enough_gps)
        assertTrue(toast, !toast.contains("keep", ignoreCase = true))
        assertTrue(toast, !toast.contains("—"))
    }

    @Test
    fun signalLostLinesUseNoEmDash() {
        for (id in listOf(R.string.track_pill_signal_lost, R.string.track_notif_signal_lost)) {
            val s = ctx.getString(id)
            assertTrue(s, !s.contains("—"))
        }
    }

    @Test
    fun fullDenialCopyDoesNotBlameApproximateLocation() {
        // Denying location entirely is not an approximate-only grant; only the
        // coarse caption may talk about approximate location.
        for (id in listOf(R.string.track_perm_rationale, R.string.track_perm_denied_caption, R.string.track_perm_blocked_caption)) {
            val s = ctx.getString(id)
            assertTrue(s, !s.contains("approximate", ignoreCase = true))
        }
        assertTrue(ctx.getString(R.string.track_perm_coarse_caption).contains("approximate", ignoreCase = true))
    }

    @Test
    fun shortRunNoteShowsTheRunAndARoundThreshold() {
        assertEquals("0.3 mi", shortRunThresholdLabel(DistanceUnit.Mi))
        assertEquals("0.5 km", shortRunThresholdLabel(DistanceUnit.Km))
        val note = ctx.getString(R.string.track_short_run_note, "0.13 mi", shortRunThresholdLabel(DistanceUnit.Mi))
        assertTrue(note, note.contains("0.13 mi") && note.contains("under 0.3 mi"))
    }
}
