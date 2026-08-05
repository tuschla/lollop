package app.milestone

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * C5 rotation escape hatch: the captured-but-unsaved-run condition is DERIVED
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
    // | false    | >0         | true              | captured, awaiting save (C5)  |
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
        // fixes are still held: the C5 case rotation used to strand.
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
}
