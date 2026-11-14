package app.milestone

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

/**
 * The deep-link state both platforms share: the `lollo://open/<dest>` URLs iOS is
 * opened with, and where a request lands from the scaffold's navigation
 * ([routeOpen]; the Android open-intent cases are DeepLinkTest / DeepLinkPropertyTest).
 */
class OpenRequestsTest {
    private val today = OpenNav(
        showTracker = false,
        selected = Dest.Today,
        sheetOpen = false,
        sheetMode = LogMode.Chooser,
        sheetPlanExercise = null,
        sheetPlanDay = null,
    )

    @Test
    fun anOpenUrlNamesItsDestination() {
        assertEquals(OpenDest.Log, openDestOfUrl("lollo://open/log"))
        assertEquals(OpenDest.LogSet, openDestOfUrl("lollo://open/log_set?from=reminder"))
        assertEquals(OpenDest.Track, openDestOfUrl("LOLLO://open/track#x"))
        assertEquals(OpenDest.Today, openDestOfUrl("lollo://open"), "no dest: Today")
        assertEquals(OpenDest.Today, openDestOfUrl("lollo://open/somewhere"), "an unknown dest opens Today, as an intent's")
    }

    @Test
    fun anyOtherUrlIsNoOpenRequest() {
        assertNull(openDestOfUrl("file:///private/var/mobile/Inbox/run.gpx"))
        assertNull(openDestOfUrl("lollo://openings/log"))
        assertNull(openDestOfUrl("https://example.org/open/log"))
    }

    @Test
    fun aRequestOpensItsDestinationFromToday() {
        val model = ViewModel()
        assertEquals(today.copy(showTracker = true), routeOpen(OpenDest.Track, today, model, sheetBusy = false, trackerPinned = false))
        assertEquals(today.copy(sheetOpen = true), routeOpen(OpenDest.Log, today, model, sheetBusy = false, trackerPinned = false))
        assertEquals(
            today.copy(sheetOpen = true, sheetMode = LogMode.Set),
            routeOpen(OpenDest.LogSet, today, model, sheetBusy = false, trackerPinned = false),
            "no plan: a blank set editor",
        )
        val elsewhere = today.copy(selected = Dest.History, sheetOpen = true, sheetMode = LogMode.Run)
        assertEquals(today, routeOpen(OpenDest.Today, elsewhere, model, sheetBusy = false, trackerPinned = false))
    }

    @Test
    fun nothingTypedIsThrownAwayAndAPinnedTrackerStays() {
        val model = ViewModel()
        val editing = today.copy(selected = Dest.History, sheetOpen = true, sheetMode = LogMode.Run)
        assertEquals(
            editing.copy(selected = Dest.Today),
            routeOpen(OpenDest.Log, editing, model, sheetBusy = true, trackerPinned = false),
            "a dirty editor stays open on Today",
        )
        val armed = today.copy(showTracker = true)
        assertEquals(armed, routeOpen(OpenDest.Log, armed, model, sheetBusy = false, trackerPinned = true))
        assertEquals(today.copy(sheetOpen = true), routeOpen(OpenDest.Log, armed, model, sheetBusy = false, trackerPinned = false))
    }

    @Test
    fun trackAlwaysOpensTheRunTracker() {
        val model = ViewModel()
        val history = today.copy(selected = Dest.History)
        assertEquals(history.copy(showTracker = true), routeOpen(OpenDest.Track, history, model, sheetBusy = false, trackerPinned = false))
        val setLoop = today.copy(sheetOpen = true, sheetMode = LogMode.Set)
        assertEquals(
            setLoop.copy(showTracker = true),
            routeOpen(OpenDest.Track, setLoop, model, sheetBusy = true, trackerPinned = false),
            "the tracker opens over a set loop, which stays underneath",
        )
    }
}
