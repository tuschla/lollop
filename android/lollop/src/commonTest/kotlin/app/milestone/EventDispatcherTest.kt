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
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertSame

/**
 * The process-lifetime [EventDispatcher] consumer: it must survive a throwing
 * send, publish every result in order, and surface every core outcome the user
 * must hear about through [EventDispatcher.notices] — including one raised while
 * no Activity is collecting.
 *
 * The process's send path is a [Sink] of the test's: the dispatcher's units reach
 * it as they would the core engine ([EngineSink]).
 */
@OptIn(ExperimentalCoroutinesApi::class)
class EventDispatcherTest : PlatformTest() {
    /** The event queue's send path, answering what the test scripts. */
    private class Sink : EventSink {
        var sender: suspend (Event) -> SendResult = { error("unexpected send of $it") }
        var batchSender: suspend (List<Event>) -> SendResult = { error("unexpected batch of $it") }

        override suspend fun send(e: Event): SendResult = sender(e)

        override suspend fun sendAll(events: List<Event>): SendResult = batchSender(events)
    }

    private val sink = Sink()

    @BeforeTest
    fun setUp() {
        // The consumer runs on AppProcess.scope (Dispatchers.Main.immediate).
        // Install the test Main BEFORE the process starts so its scope dispatches
        // through it; runTest then shares this scheduler.
        Dispatchers.setMain(StandardTestDispatcher())
        AppRuntime.resetForTest(sink = sink)
    }

    @AfterTest
    fun tearDown() {
        AppRuntime.resetForTest()
        Dispatchers.resetMain()
    }

    @Test
    fun aThrowingSendDropsOneEventButTheConsumerKeepsProcessing() = runTest {
        val goodView = ViewModel(input_count = 42)
        val good = SendResult(goodView, accepted = true)
        var calls = 0
        // First send throws (a malformed view / Core.send blow-up); the second
        // returns a recognizable accepted result.
        sink.sender = { _ ->
            calls++
            if (calls == 1) throw RuntimeException("first send blows up")
            good
        }

        EventDispatcher.send(Event.SetToday(0L, 0))
        EventDispatcher.send(Event.SetToday(1L, 0))
        advanceUntilIdle()

        assertEquals(2, calls)
        assertSame(goodView, EventDispatcher.model.value)
    }

    @Test
    fun aRejectedSendPublishesTheViewAndRaisesNotSaved() = runTest {
        // The core did NOT accept the event (a serde-rejected event / panic
        // envelope): the consumer still publishes the view but must flag it.
        val rejectedView = ViewModel(input_count = 7)
        sink.sender = { _ -> SendResult(rejectedView, accepted = false) }

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
        sink.sender = { _ -> SendResult(ViewModel(), accepted = false) }
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
        sink.sender = { _ -> SendResult(ViewModel(), accepted = true, persisted = false) }
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
        sink.sender = { _ -> SendResult(ViewModel(), accepted = true, stale = stale[i++]) }
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
        sink.batchSender = { events ->
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
