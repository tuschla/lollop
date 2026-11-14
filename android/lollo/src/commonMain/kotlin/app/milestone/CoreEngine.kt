package app.milestone

import kotlinx.coroutines.CloseableCoroutineDispatcher
import kotlinx.coroutines.DelicateCoroutinesApi
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.newSingleThreadContext

/**
 * The thread the event queue's core turns run on ([CoreEngine.send], the boot
 * restore, the view read-back): one thread, so the turns run in order and never
 * on the UI thread. Top-level rather than an engine member: callers evaluate it
 * on the main thread, and touching the Android engine ([Core]) runs its
 * initializer (`System.loadLibrary`), which must happen here, not on the UI
 * thread at cold start.
 */
@OptIn(DelicateCoroutinesApi::class, ExperimentalCoroutinesApi::class)
internal val CoreDispatcher: CloseableCoroutineDispatcher = newSingleThreadContext("core-jni")

/** What the event queue ([EventDispatcher]) needs from the core engine: apply
 *  events, persist the accepted ones, and answer with the fresh view. Called on
 *  [CoreDispatcher] only. */
interface CoreEngine {
    /** Apply [event] and read the view once. */
    fun send(event: Event): SendResult

    /** Apply [events] as one unit and read the view once. */
    fun sendAll(events: List<Event>): SendResult
}
