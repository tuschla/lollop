package app.milestone

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Rotation escape hatch: the captured-but-unsaved-run condition is DERIVED
 * from [RunSession] (`hasUnsavedCapture`) rather than kept in a composition-
 * volatile `remember`, so it survives rotation / Back+reopen. That derivation is
 * the single load-bearing predicate for the tracker control row, the
 * locate-restart effect, the BackHandler keep-sidecar branch, and the
 * startTracking refuse-guard, so its truth table is pinned here.
 *
 * The predicate: a capture is unsaved exactly when the service is NOT recording
 * (`!tracking`) AND raw fixes are still held (`pointCount > 0`). The locate-only
 * preview records no points, so a stopped-with-points state can only be a track
 * awaiting save, never a live locate preview.
 */
class TrackerStateTest {

    // --- Truth table over (tracking, pointCount) --------------------------------
    //
    // | tracking | pointCount | hasUnsavedCapture | meaning                       |
    // |----------|------------|-------------------|-------------------------------|
    // | false    | 0          | false             | fresh open / locate preview   |
    // | false    | >0         | true              | captured, awaiting save       |
    // | true     | 0          | false             | recording, first fix pending  |
    // | true     | >0         | false             | recording in progress         |

    @Test
    fun stoppedWithNoPointsIsLocatePreviewNotAnUnsavedCapture() {
        // Fresh tracker open: service is LOCATE-ing, nothing recorded yet.
        assertFalse(hasUnsavedCapture(tracking = false, pointCount = 0))
    }

    @Test
    fun stoppedWithPointsIsAnUnsavedCapture() {
        // Service stopped after a failed save / dismissed short-run prompt but the
        // fixes are still held: the case rotation used to strand.
        assertTrue(hasUnsavedCapture(tracking = false, pointCount = 1))
        assertTrue(hasUnsavedCapture(tracking = false, pointCount = 4200))
    }

    @Test
    fun recordingWithNoPointsIsNotAnUnsavedCapture() {
        // Just pressed Start; the first accepted fix hasn't landed yet.
        assertFalse(hasUnsavedCapture(tracking = true, pointCount = 0))
    }

    @Test
    fun recordingWithPointsIsNotAnUnsavedCapture() {
        // A run in progress is not "awaiting save": Stop & save hasn't run.
        assertFalse(hasUnsavedCapture(tracking = true, pointCount = 1))
        assertFalse(hasUnsavedCapture(tracking = true, pointCount = 9000))
    }

    @Test
    fun fresh_lock_lights_for_a_fix_newer_than_the_clock_sample() {
        // A fix stamped at the current clock sample (age 0) is a fresh lock: the pill
        // resamples the clock on every new fix stamp, so a just-landed lock lights within
        // its window instead of reading stale against an older clock sample.
        assertTrue(isFreshLock(accuracyM = 10.0, fixElapsedMs = 5_000L, nowElapsedMs = 5_000L))
    }
}
