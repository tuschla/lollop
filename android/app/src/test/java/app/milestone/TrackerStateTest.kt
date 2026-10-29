package app.milestone

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * The captured-but-unsaved-run condition is derived from [RunSession]
 * (`hasUnsavedCapture`), not kept in a composition `remember`, so it survives
 * rotation and Back+reopen. It drives the tracker control row, the locate-restart
 * effect, the BackHandler keep-sidecar branch and the startTracking refuse-guard.
 */
class TrackerStateTest {

    @Test
    fun aCaptureIsUnsavedOnlyWhenStoppedWithFixesHeld() {
        // The locate-only preview records no points, so stopped-with-points can only
        // be a track awaiting save.
        val rows = listOf(
            // tracking, hasPoints, unsaved: meaning
            Triple(false, false, false), // fresh open / locate preview
            Triple(false, true, true), // stopped after a failed save or dismissed prompt
            Triple(true, false, false), // recording, first fix pending
            Triple(true, true, false), // recording in progress
        )
        for ((tracking, hasPoints, unsaved) in rows) {
            assertEquals("tracking=$tracking hasPoints=$hasPoints", unsaved, hasUnsavedCapture(tracking = tracking, hasPoints = hasPoints))
        }
    }

    @Test
    fun startRingDrawsBelowALocationDotAddedBeforeIt() {
        // A preview fix placed the dot before Start; the first recorded point adds the ring.
        val overlays = mutableListOf("casing", "track", "dot")
        overlays.addBelow("ring", "dot")
        assertEquals(listOf("casing", "track", "ring", "dot"), overlays)
        // A recovered track has no preview dot: the ring goes on top, the dot after it.
        val recovered = mutableListOf("casing", "track")
        recovered.addBelow("ring", "dot")
        assertEquals(listOf("casing", "track", "ring"), recovered)
    }
}
