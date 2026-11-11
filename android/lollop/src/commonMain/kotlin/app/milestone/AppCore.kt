package app.milestone

import kotlin.concurrent.Volatile
import kotlinx.coroutines.withContext

/** Where the event queue's units go: the core engine on its thread
 *  ([EngineSink]), or a test's own ([AppRuntime.resetForTest]). */
internal interface EventSink {
    suspend fun send(e: Event): SendResult

    suspend fun sendAll(events: List<Event>): SendResult
}

/** [AppCore.engine] on [CoreDispatcher]. */
internal object EngineSink : EventSink {
    override suspend fun send(e: Event): SendResult = withContext(CoreDispatcher) { AppCore.engine.send(e) }

    override suspend fun sendAll(events: List<Event>): SendResult = withContext(CoreDispatcher) { AppCore.engine.sendAll(events) }
}

/** The platform's core engine, which [AppCore.engine] runs on: Android's JNI
 *  [Core], iOS's engine over the C ABI. Read on [CoreDispatcher] (the Android
 *  engine loads its library on first touch). */
internal expect fun platformCoreEngine(): DurableCore

/** The process's core engine and where its event queue sends ([sink]); a test may
 *  run the process on its own ([AppRuntime.resetForTest]). */
internal object AppCore {
    private class Slot(engine: DurableCore?, val sink: EventSink?) {
        val engine: Lazy<DurableCore> = lazy { engine ?: platformCoreEngine() }
    }

    @Volatile private var slot = Slot(null, null)

    /** The core engine: Android's JNI [Core], iOS's engine over the C ABI. Its
     *  first touch belongs on [CoreDispatcher] (Android loads the library then). */
    val engine: DurableCore get() = slot.engine.value

    /** Where the event queue sends: [engine] on its thread. */
    internal val sink: EventSink get() = slot.sink ?: EngineSink

    /** Run on [engine] and [sink] from now on. An engine the process used starts
     *  over ([DurableCore.forgetProcess]); one it never touched is left alone (a
     *  plain-JVM test loads the native library only when it asks the core). */
    internal fun reset(engine: DurableCore?, sink: EventSink?) {
        val old = slot
        if (old.engine.isInitialized()) old.engine.value.forgetProcess()
        slot = Slot(engine, sink)
    }
}
