package app.milestone.core

/**
 * The Rust core's bridge calls, the same set and wire the app's core engine
 * (`DurableCore` in android/lollop) makes; Android's JNI exports are
 * `Java_app_milestone_Core_*`. Bytes and strings are the core's JSON, exactly
 * as the JNI exports return them; the shell decodes them. Each call wraps one
 * `shared::ffi` function, which Android's JNI exports wrap too.
 *
 * Threading: every call is safe from any thread, but the core keeps its
 * derived-view memos per thread, so a shell keeps its [update] / [view] calls
 * on one thread (Android: the single `core-jni` dispatcher).
 */
interface CoreBridge {
    /** One event's wire JSON in; the effect-request array (`[...]`) out, no
     *  bytes for a dropped (malformed or unknown) event, or a
     *  `{"error":{"kind":"panic_reset",...}}` object after a caught core panic
     *  (the core was replaced: replay the log). */
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

    /** Replace the core with an empty one, as a fresh process starts: the app
     *  then replays an imported event log in process. */
    fun reset()
}

/** The platform's core: the C ABI on Kotlin/Native (iOS). */
expect val rustCore: CoreBridge
