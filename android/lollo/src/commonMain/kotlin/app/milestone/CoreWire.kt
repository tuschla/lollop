package app.milestone

// The core's JSON wire as the shell reads it: the bridge's error envelope, the
// replay with reset recovery, and what one send reports. Platform-free; the
// engine that drives the native core ([DurableCore]) builds on it. The wire
// types and `coreJson` are generated (WireTypes.kt).

/** How many log lines a replay may skip for resetting the core before it gives up
 *  and leaves the core with what replayed. */
internal const val MAX_REPLAY_RESETS = 16

/** How an [ErrorEnvelope] starts: its one key opens the object, so a view or an op's
 *  payload is told apart without tree-parsing it. */
private val ENVELOPE_PREFIX = "{\"${ErrorEnvelope.serializer().descriptor.getElementName(0)}\""

/** The error an op answered, or null when [text] is not an [ErrorEnvelope]. */
internal fun coreErrorOf(text: String): CoreError? {
    if (!text.trimStart().startsWith(ENVELOPE_PREFIX)) return null
    return runCatching { coreJson.decodeFromString(ErrorEnvelope.serializer(), text).error }.getOrNull()
}

internal fun isCoreErrorEnvelope(text: String): Boolean = coreErrorOf(text) != null

/** Whether [text] is a [ErrorKind.PanicReset] envelope: the core panicked AND was
 *  replaced with an empty one, so the shell must rebuild it from the persisted
 *  log. Any other kind leaves the core intact; the shell keeps its last good view. */
internal fun isCorePanicReset(text: String): Boolean = coreErrorOf(text)?.kind == ErrorKind.PanicReset

/** Whether a raw `update` response is a [ErrorKind.PanicReset] envelope. An accepted
 *  event's response is an effect array (`[`), so only a response opening with `{`
 *  is decoded to text. */
internal fun isPanicResetResponse(response: ByteArray?): Boolean =
    response != null && response.firstNonBlank() == '{'.code.toByte() && isCorePanicReset(response.decodeToString())

/** The first non-whitespace byte of a core response; null for a blank or missing one. */
internal fun ByteArray?.firstNonBlank(): Byte? = this?.firstOrNull { !it.toInt().toChar().isWhitespace() }

/** Outcome of [replayLog]: the indices of lines skipped because applying them reset
 *  the core, and whether the final pass replayed every other line ([complete] is
 *  false only when the reset budget ran out, leaving the core with what replayed). */
internal data class ReplayOutcome(val skipped: Set<Int>, val complete: Boolean)

/**
 * Replay [lines] in order through [apply] (one raw JNI `update` each). A line whose
 * response is a `panic_reset` envelope has swapped in an EMPTY core, discarding
 * everything replayed before it (a DO-NOT-TRAIN hold included), so that line is
 * skipped and the replay restarts from the first line, at most [maxResets] times.
 * Top-level so the recovery is JVM-testable without the native library.
 */
internal fun replayLog(lines: List<String>, maxResets: Int, apply: (String) -> ByteArray?): ReplayOutcome {
    val skipped = LinkedHashSet<Int>()
    while (true) {
        val reset = lines.indices.firstOrNull { i -> i !in skipped && isPanicResetResponse(apply(lines[i])) }
            ?: return ReplayOutcome(skipped, complete = true)
        skipped += reset
        if (skipped.size > maxResets) return ReplayOutcome(skipped, complete = false)
    }
}

/**
 * The outcome of [CoreEngine.send]: the fresh [view]; whether the core [accepted] the
 * event (false for a serde-rejected event or a panic-firewall envelope); whether
 * an accepted event reached the durable log ([persisted] false: applied in memory
 * but the append failed, so it is gone after the next restart); and whether [view]
 * is [stale] (a stand-in for a view that couldn't be read, or a model missing part
 * of the saved log). The dispatcher surfaces each so a lost entry can't look saved.
 */
data class SendResult(
    val view: ViewModel,
    val accepted: Boolean,
    val persisted: Boolean = true,
    val stale: Boolean = false,
)
