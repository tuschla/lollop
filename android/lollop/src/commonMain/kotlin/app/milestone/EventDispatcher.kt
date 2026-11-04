package app.milestone

import kotlin.concurrent.Volatile
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

// Process-scoped serial event dispatcher: the queue and its one consumer outlive
// composition, so a rotation never drops a queued event. Exactly one unit (an
// event, a batch, or an [exclusive] body) is in flight at a time, on
// CoreDispatcher, and its ViewModel is published to [model] in enqueue order.
// That serial order is the whole ordering guarantee: the core's log and state are
// order-sensitive.
internal object EventDispatcher : BootLedger {
    private sealed interface Msg
    private class Ev(val e: Event) : Msg
    private class Batch(val events: List<Event>) : Msg
    private class Block(val body: suspend () -> ViewModel?, val done: CompletableDeferred<ViewModel?>) : Msg

    // A var only for [resetForTest]; the consumer drains the channel it was started on.
    @Volatile private var channel = newQueue()

    // A queued [exclusive] block the channel drops undelivered (only [resetForTest]
    // cancels a queue) cancels its awaiter instead of leaving it suspended forever.
    private fun newQueue() = Channel<Msg>(Channel.UNLIMITED) { (it as? Block)?.done?.cancel() }

    // The freshest hydrated ViewModel, or null before boot hydrates it. The
    // composition seeds `loaded`/`model` from this (rotation → last content
    // instantly, cold start → the splash holds while boot hydrates) and collects it
    // so every later event's model reaches the UI.
    val model = MutableStateFlow<ViewModel?>(null)

    // Boot found the native core unusable (Android: Core.probe / Core.markUnavailable):
    // the shell shows [CoreUnavailableScreen] instead of any screen that could write,
    // and the splash lets go although [model] stays null. Process-scoped like
    // [model], so a rotation renders the error at once.
    val coreUnavailable = MutableStateFlow(false)

    // Core outcomes the user must hear about ([report]). A Channel, not a
    // SharedFlow: a notice sent while nothing collects (an Activity mid-recreation)
    // waits in the buffer for the next collector instead of being dropped.
    private val noticeChannel = Channel<CoreNotice>(Channel.BUFFERED)
    val notices: Flow<CoreNotice> = noticeChannel.receiveAsFlow()

    // Whether the last reported result was already stale, so a run of stale
    // results raises ONE notice rather than one per event.
    @Volatile private var staleReported = false

    // The once-per-process boot gate (guided-setup decision + run recovery).
    // Survives rotation so the boot effect doesn't clobber mid-setup answers, and
    // resets on process death, when the ActiveRunStore recovery check must re-run.
    override var bootHandled = false

    // The (local epoch-day, utc-offset-sec) the core was last told is "today"; null
    // until boot sends it. Process-scoped, so a rotation's boot re-run can tell the
    // core already has this clock and skip the resend.
    @Volatile override var lastToday: Pair<Long, Int>? = null

    override val published: ViewModel? get() = model.value

    // The crash-recovery sidecar fixes read ONCE at boot when a resume prompt is
    // offered, so the Resume action reuses them instead of re-reading the sidecar
    // file off disk. Null when no interrupted run was found / after it is consumed.
    var recoveredSidecar: RecoveredTrack? = null

    // Process-level scope for work that must outlive a sheet/screen (imports,
    // export, the data restore).
    val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    // The real event→view send paths: the platform's core engine, on its thread.
    private val coreSender: suspend (Event) -> SendResult =
        { e -> withContext(CoreDispatcher) { platformCoreEngine().send(e) } }
    private val coreBatchSender: suspend (List<Event>) -> SendResult =
        { es -> withContext(CoreDispatcher) { platformCoreEngine().sendAll(es) } }

    // The send paths in use, injectable so a JVM test can substitute fakes: the
    // real ones are unavailable off device. A var so the consumer's drop-one-unit
    // runCatching still wraps whatever send path is installed.
    var sender: suspend (Event) -> SendResult = coreSender
    var batchSender: suspend (List<Event>) -> SendResult = coreBatchSender

    private var consumer: Job = startConsumer(channel)

    // One serial consumer per process on the main dispatcher; it drains the queue
    // strictly in order. Only [resetForTest] cancels it, after cancelling its queue.
    private fun startConsumer(queue: Channel<Msg>): Job = scope.launch {
        for (m in queue) when (m) {
            // A malformed view (or any send throw) must drop THIS unit, never
            // kill the process-lifetime consumer loop.
            is Ev -> {
                val r = runCatching { sender(m.e) }
                if (!isActive) break
                r.onSuccess(::publish).onFailure { PlatformLog.e("EventDispatcher", "event dropped", it) }
            }
            is Batch -> {
                val r = runCatching { batchSender(m.events) }
                if (!isActive) break
                r.onSuccess(::publish).onFailure { PlatformLog.e("EventDispatcher", "event batch dropped", it) }
            }
            is Block -> {
                val r = runCatching { m.body() }
                if (isActive) r.getOrNull()?.let { model.value = it }
                // Complete the awaiter with the body's value (null included) or its
                // exception; a thrown body reaches the awaiter, never kills the consumer.
                r.fold({ m.done.complete(it) }, { m.done.completeExceptionally(it) })
            }
        }
    }

    /** Test isolation: fresh process state (new queue and consumer, real send
     *  paths, no model or notice, boot not handled). Cancels the old queue so no
     *  [exclusive] awaiter hangs. */
    fun resetForTest() {
        val old = channel
        channel = newQueue()
        old.cancel()
        consumer.cancel()
        consumer = startConsumer(channel)
        sender = coreSender
        batchSender = coreBatchSender
        while (noticeChannel.tryReceive().isSuccess) {}
        model.value = null
        coreUnavailable.value = false
        bootHandled = false
        lastToday = null
        recoveredSidecar = null
        staleReported = false
    }

    private fun publish(r: SendResult) {
        model.value = r.view
        report(r)
    }

    /** Surface what the user must know about [r]: an entry that is not durably
     *  saved (rejected, or accepted but the log append failed) and, once per stale
     *  run, a view that is a stand-in or missing saved data. The queue paths call
     *  this themselves; an [exclusive] body calls it for each [SendResult] it gets. */
    override fun report(r: SendResult) {
        if (!r.accepted || !r.persisted) noticeChannel.trySend(CoreNotice.NotSaved)
        if (r.stale && !staleReported) noticeChannel.trySend(CoreNotice.Unreadable)
        staleReported = r.stale
    }

    /** Enqueue an event for the serial consumer. UNLIMITED channel, so trySend
     *  never drops (it only fails on close, which never happens for this process
     *  singleton). */
    fun send(e: Event) {
        channel.trySend(Ev(e))
    }

    /** Enqueue [events] as ONE unit: applied under one core monitor turn with a
     *  single view decode and a single published model (Clear all data). */
    fun sendAll(events: List<Event>) {
        channel.trySend(Batch(events))
    }

    /** Run [body] as ONE exclusive turn of the serial consumer — no queued event
     *  interleaves with its sends — and publish its non-null result in queue order. */
    suspend fun exclusive(body: suspend () -> ViewModel?): ViewModel? {
        val done = CompletableDeferred<ViewModel?>()
        channel.trySend(Block(body, done))
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
