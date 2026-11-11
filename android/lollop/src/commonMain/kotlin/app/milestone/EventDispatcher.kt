package app.milestone

import kotlin.concurrent.Volatile
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.channels.onFailure
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

// Process-scoped serial event dispatcher: the queue and its one consumer outlive
// composition, so a rotation never drops a queued event. Exactly one unit (an
// event, a batch, or an [exclusive] body) is in flight at a time, on
// CoreDispatcher, and its ViewModel is published to [model] in enqueue order.
// That serial order is the whole ordering guarantee: the core's log and state are
// order-sensitive.
internal object EventDispatcher {
    private sealed interface Msg
    private class Ev(val e: Event) : Msg
    private class Batch(val events: List<Event>) : Msg
    private class Block(val body: suspend () -> ViewModel?, val done: CompletableDeferred<ViewModel?>) : Msg

    private class Queue : AutoCloseable {
        // A queued [exclusive] block the channel drops undelivered (the process
        // ended) cancels its awaiter instead of leaving it suspended forever.
        val channel = Channel<Msg>(Channel.UNLIMITED) { (it as? Block)?.done?.cancel() }
        val model = MutableStateFlow<ViewModel?>(null)
        val coreUnavailable = MutableStateFlow(false)

        // A Channel, not a SharedFlow: a notice sent while nothing collects (an
        // Activity mid-recreation) waits in the buffer for the next collector.
        val notices = Channel<CoreNotice>(Channel.BUFFERED)
        val noticeFlow: Flow<CoreNotice> = notices.receiveAsFlow()

        // Whether the last reported result was already stale, so a run of stale
        // results raises ONE notice rather than one per event.
        @Volatile var staleReported = false

        override fun close() {
            channel.cancel()
        }
    }

    private val queue by ProcessLocal { Queue().also(::startConsumer) }

    // The freshest hydrated ViewModel, or null before boot hydrates it. The
    // composition seeds `loaded`/`model` from this (rotation → last content
    // instantly, cold start → the splash holds while boot hydrates) and collects it
    // so every later event's model reaches the UI.
    val model: MutableStateFlow<ViewModel?> get() = queue.model

    // Boot found the native core unusable (Android: Core.probe / Core.markUnavailable):
    // the shell shows [CoreUnavailableScreen] instead of any screen that could write,
    // and the splash lets go although [model] stays null.
    val coreUnavailable: MutableStateFlow<Boolean> get() = queue.coreUnavailable

    /** Core outcomes the user must hear about ([report]). */
    val notices: Flow<CoreNotice> get() = queue.noticeFlow

    // One serial consumer per process on the app scope; it drains the queue
    // strictly in order.
    private fun startConsumer(q: Queue) {
        AppProcess.scope.launch {
            for (m in q.channel) when (m) {
                // A malformed view (or any send throw) must drop THIS unit, never
                // kill the process-lifetime consumer loop.
                is Ev -> {
                    val r = runCatching { AppCore.sink.send(m.e) }
                    if (!isActive) break
                    r.onSuccess { publish(q, it) }.onFailure { PlatformLog.e("EventDispatcher", "event dropped", it) }
                }
                is Batch -> {
                    val r = runCatching { AppCore.sink.sendAll(m.events) }
                    if (!isActive) break
                    r.onSuccess { publish(q, it) }.onFailure { PlatformLog.e("EventDispatcher", "event batch dropped", it) }
                }
                is Block -> {
                    val r = runCatching { m.body() }
                    if (isActive) r.getOrNull()?.let { q.model.value = it }
                    // Complete the awaiter with the body's value (null included) or its
                    // exception; a thrown body reaches the awaiter, never kills the consumer.
                    r.fold({ m.done.complete(it) }, { m.done.completeExceptionally(it) })
                }
            }
        }
    }

    private fun publish(q: Queue, r: SendResult) {
        q.model.value = r.view
        report(r)
    }

    /** Surface what the user must know about [r]: an entry that is not durably
     *  saved (rejected, or accepted but the log append failed) and, once per stale
     *  run, a view that is a stand-in or missing saved data. The queue paths call
     *  this themselves; an [exclusive] body calls it for each [SendResult] it gets. */
    fun report(r: SendResult) {
        val q = queue
        if (!r.accepted || !r.persisted) notice(q, CoreNotice.NotSaved)
        if (r.stale && !q.staleReported) notice(q, CoreNotice.Unreadable)
        q.staleReported = r.stale
    }

    private fun notice(q: Queue, n: CoreNotice) {
        q.notices.trySend(n).onFailure { PlatformLog.w("EventDispatcher", "notice $n dropped", it) }
    }

    /** Enqueue an event for the serial consumer (an UNLIMITED channel: it only
     *  refuses once its process ended). */
    fun send(e: Event) {
        enqueue(Ev(e))
    }

    /** Enqueue [events] as ONE unit: applied under one core monitor turn with a
     *  single view decode and a single published model (Clear all data). */
    fun sendAll(events: List<Event>) {
        enqueue(Batch(events))
    }

    private fun enqueue(m: Msg) {
        queue.channel.trySend(m).onFailure { PlatformLog.w("EventDispatcher", "queue closed, unit dropped", it) }
    }

    /** Run [body] as ONE exclusive turn of the serial consumer — no queued event
     *  interleaves with its sends — and publish its non-null result in queue order. */
    suspend fun exclusive(body: suspend () -> ViewModel?): ViewModel? {
        val done = CompletableDeferred<ViewModel?>()
        val sent = queue.channel.trySend(Block(body, done))
        if (sent.isFailure) done.cancel()
        return done.await()
    }
}

/** A core outcome the user must hear about, posted by [EventDispatcher.report]. */
internal enum class CoreNotice {
    /** An entry the core rejected, or accepted in memory but couldn't write to the log. */
    NotSaved,

    /** What is shown stands in for a view that couldn't be read, or is missing part
     *  of the saved data. */
    Unreadable,
}
