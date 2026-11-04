package app.milestone

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertSame
import org.junit.Before
import org.junit.Test

/**
 * The process-lifetime [EventDispatcher] consumer: it must survive a throwing
 * send, publish every result in order, and surface every core outcome the user
 * must hear about through [EventDispatcher.notices] — including one raised while
 * no Activity is collecting.
 *
 * The [EventDispatcher.sender] / [EventDispatcher.batchSender] seams make this
 * testable off-device: the real send paths cross the JNI boundary (`Core.send`,
 * unavailable in a plain JVM unit test), so fakes stand in for them here.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class EventDispatcherTest {
    private lateinit var realSender: suspend (Event) -> SendResult
    private lateinit var realBatchSender: suspend (List<Event>) -> SendResult

    @Before
    fun setUp() {
        // The consumer runs on EventDispatcher.scope (Dispatchers.Main.immediate).
        // Install the test Main BEFORE the object first initializes so its scope
        // dispatches through it; runTest then shares this scheduler.
        Dispatchers.setMain(StandardTestDispatcher())
        realSender = EventDispatcher.sender
        realBatchSender = EventDispatcher.batchSender
    }

    @After
    fun tearDown() {
        EventDispatcher.sender = realSender
        EventDispatcher.batchSender = realBatchSender
        Dispatchers.resetMain()
    }

    @Test
    fun aThrowingSendDropsOneEventButTheConsumerKeepsProcessing() = runTest {
        val goodView = ViewModel(input_count = 42)
        val good = SendResult(goodView, accepted = true)
        var calls = 0
        // First send throws (a malformed view / Core.send blow-up); the second
        // returns a recognizable accepted result.
        EventDispatcher.sender = { _ ->
            calls++
            if (calls == 1) throw RuntimeException("first send blows up")
            good
        }

        EventDispatcher.send(Event.SetToday(0L, 0))
        EventDispatcher.send(Event.SetToday(1L, 0))
        advanceUntilIdle()

        // Both events reached the consumer: the throw did not stop the loop.
        assertEquals(2, calls)
        // The surviving second event's view landed in the model.
        assertSame(goodView, EventDispatcher.model.value)
    }

    @Test
    fun aRejectedSendPublishesTheViewAndRaisesNotSaved() = runTest {
        // The core did NOT accept the event (a serde-rejected event / panic
        // envelope): the consumer still publishes the view but must flag it.
        val rejectedView = ViewModel(input_count = 7)
        EventDispatcher.sender = { _ -> SendResult(rejectedView, accepted = false) }

        val notices = mutableListOf<CoreNotice>()
        val job = launch { EventDispatcher.notices.collect { notices += it } }
        runCurrent()

        EventDispatcher.send(Event.SetToday(0L, 0))
        advanceUntilIdle()

        assertSame(rejectedView, EventDispatcher.model.value)
        assertEquals(listOf(CoreNotice.NotSaved), notices)
        job.cancel()
    }

    @Test
    fun aNoticeRaisedWithNoCollectorReachesTheNextCollector() = runTest {
        // Activity recreation: the event lands while nothing collects. The notice
        // must wait for the next collector instead of being dropped.
        EventDispatcher.sender = { _ -> SendResult(ViewModel(), accepted = false) }
        EventDispatcher.send(Event.SetToday(0L, 0))
        advanceUntilIdle()

        val notices = mutableListOf<CoreNotice>()
        val job = launch { EventDispatcher.notices.collect { notices += it } }
        advanceUntilIdle()

        assertEquals(listOf(CoreNotice.NotSaved), notices)
        job.cancel()
    }

    @Test
    fun anAcceptedButUnpersistedEventRaisesNotSaved() = runTest {
        // Disk full: the core applied the entry in memory, but the log append
        // failed, so it is gone after the next restart and must not look saved.
        EventDispatcher.sender = { _ -> SendResult(ViewModel(), accepted = true, persisted = false) }
        val notices = mutableListOf<CoreNotice>()
        val job = launch { EventDispatcher.notices.collect { notices += it } }
        runCurrent()

        EventDispatcher.send(Event.SetToday(0L, 0))
        advanceUntilIdle()

        assertEquals(listOf(CoreNotice.NotSaved), notices)
        job.cancel()
    }

    @Test
    fun aRunOfStaleResultsRaisesOneUnreadablePerRun() = runTest {
        val stale = listOf(false, true, true, true, false, true)
        var i = 0
        EventDispatcher.sender = { _ -> SendResult(ViewModel(), accepted = true, stale = stale[i++]) }
        val notices = mutableListOf<CoreNotice>()
        val job = launch { EventDispatcher.notices.collect { notices += it } }
        runCurrent()

        repeat(stale.size) { EventDispatcher.send(Event.SetToday(it.toLong(), 0)) }
        advanceUntilIdle()

        // Two stale runs (events 2-4 and 6) → exactly two notices.
        assertEquals(listOf(CoreNotice.Unreadable, CoreNotice.Unreadable), notices)
        job.cancel()
    }

    @Test
    fun aBatchIsSentAsOneUnitAndPublishesOneModel() = runTest {
        val batchView = ViewModel(input_count = 3)
        val batches = mutableListOf<List<Event>>()
        var published = 0
        EventDispatcher.batchSender = { events ->
            batches += events
            SendResult(batchView, accepted = true)
        }
        val job = launch { EventDispatcher.model.collect { if (it === batchView) published++ } }
        runCurrent()

        EventDispatcher.sendAll(listOf(Event.ClearSets, Event.ClearRuns, Event.ClearPlan))
        advanceUntilIdle()

        assertEquals(listOf(listOf(Event.ClearSets, Event.ClearRuns, Event.ClearPlan)), batches)
        assertEquals(1, published)
        assertSame(batchView, EventDispatcher.model.value)
        job.cancel()
    }
}
