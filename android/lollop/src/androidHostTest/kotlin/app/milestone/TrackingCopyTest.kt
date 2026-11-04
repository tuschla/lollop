package app.milestone

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/** Tracker copy whose wording carries a fact (nothing left to keep, which denial,
 *  the short-run threshold, spoken units), pinned against the real string resources. */
@RunWith(RobolectricTestRunner::class)
class TrackingCopyTest {

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @Test
    fun nothingRecordedToastDoesNotOfferToKeepADiscardedRun() {
        // Shown AFTER the empty capture was cleared: nothing is left to keep.
        val toast = ctx.getString(R.string.track_toast_not_enough_gps)
        assertTrue(toast, !toast.contains("keep", ignoreCase = true))
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

    @Test
    fun finishSummarySpellsOutTimeAndPaceUnits() {
        val spoken = finishSummarySpoken(ctx.resources, km = 0.6, elapsedSec = 200L, unit = DistanceUnit.Km)
        assertEquals("0.60 kilometers · 3 minutes 20 seconds moving · 5 minutes 33 seconds per kilometer average", spoken)
        assertTrue(spoken, !spoken.contains("km") && !spoken.contains("3:20"))
    }

    /** writing.md Terms: never "on hold"; and not "paused" either, which is the
     *  user's own Pause: the location-loss captions say what recording waits for. */
    @Test
    fun resumeCaptionsNeverSayOnHoldOrPaused() {
        for (id in listOf(R.string.track_resume_perm_caption, R.string.track_resume_perm_blocked_caption)) {
            val s = ctx.getString(id)
            assertTrue(s, !s.contains("on hold", ignoreCase = true) && !s.contains("paused", ignoreCase = true))
        }
    }
}
