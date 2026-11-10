package app.milestone

import kotlinx.coroutines.CloseableCoroutineDispatcher
import kotlinx.coroutines.DelicateCoroutinesApi
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.newSingleThreadContext

/**
 * The Rust core's bridge calls, the set [DurableCore] makes. Bytes and strings are
 * the core's JSON; the shell decodes them. Android implements it over the JNI
 * exports (`Java_app_milestone_Core_*`, [Core]'s natives); the iOS app over the
 * `lollop_*` C ABI (`android/core-bridge`'s `NativeCore`, the same signatures).
 * Each call wraps one `shared::ffi` function.
 *
 * Threading: every call is safe from any thread, but the core keeps its
 * derived-view memos per thread, so [update] and [view] run on [CoreDispatcher].
 */
interface CoreBridge {
    /** One event's wire JSON in; the effect-request array (`[...]`) out, no
     *  bytes for a dropped (malformed or unknown) event, or a
     *  `{"error":{"kind":"panic_reset",...}}` object after a caught core panic
     *  (the core was replaced: replay the log). Null when the answer couldn't
     *  be allocated. */
    fun update(event: ByteArray): ByteArray?

    /** The current `ViewModel` JSON, or a non-resetting `{"error":{"kind":"panic",...}}`. */
    fun view(): ByteArray?

    /** A `.fit` file's bytes in; the tracker-import JSON or an `{"error":...}` object out. */
    fun parseFit(bytes: ByteArray): String?

    /** The GPX document for one logged run by its `(entry_id, observed_at)`
     *  identity; empty when the run has no exportable route. Reads the run
     *  registry the last [view] filled. */
    fun exportGpx(entryId: Long, observedAt: Long): String?

    /** A `LogRunTrack` event's wire JSON in; `{"distance_km":D,"duration_min":M}`
     *  or `{"error":...}` out. */
    fun trackFigures(event: ByteArray): String?

    /** The event log's lines in; the compaction verdict JSON out, or null when
     *  the core can't answer (keep every line). */
    fun compactLog(lines: Array<String>): String?

    /** Replace the core with an empty one, as a fresh process starts; false where
     *  the bridge can't (Android's JNI exports: a data import restarts the process
     *  there instead, [DurableCore.resetForReboot]). */
    fun reset(): Boolean = false
}

/**
 * The one thread every view()-producing core call runs on ([CoreEngine.send],
 * the boot restore, the view read-back). ONE OS thread on purpose, not
 * `Dispatchers.IO` or `limitedParallelism(1)`, which may hop threads: the core's
 * derived-view memos (RUN_VIEW_CACHE, QC_TRACK_CACHE, the list caches) are
 * thread_local in Rust, so every thread hop lands on cold caches and redoes the
 * full O(history) rebuild. Top-level rather than an engine member: callers
 * evaluate it on the main thread, and touching the Android engine ([Core]) runs
 * its initializer (`System.loadLibrary`), which must happen on this thread, not
 * the UI thread at cold start.
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

/** The platform's core engine: Android's JNI [Core]; on iOS the [BridgeCore] the
 *  app installs at startup. Read on [CoreDispatcher] (the Android engine loads its
 *  library on first touch). */
internal expect fun platformCoreEngine(): DurableCore
