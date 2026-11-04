package app.milestone

import kotlinx.serialization.json.Json

// The core's JSON wire as the shell reads it: the ViewModel decoder, the
// panic-firewall envelopes, the replay with reset recovery, and what one send
// reports. Platform-free; the engine that drives the native core ([DurableCore])
// builds on it.

/** The production ViewModel decoder (decode only). Top-level so the JVM wire tests
 *  decode with exactly this configuration without loading the native library. */
internal val coreJson = Json { ignoreUnknownKeys = true }

/** How many log lines a replay may skip for resetting the core before it gives up
 *  and leaves the core with what replayed. */
internal const val MAX_REPLAY_RESETS = 16

/** A stand-in for the native core's two round trips ([Core.faultForTest]): the raw
 *  `update` response bytes for one event's wire JSON, and the raw `view` bytes. */
internal interface CoreFault {
    fun update(event: ByteArray): ByteArray?
    fun view(): ByteArray?
}

/** Whether [text] is the FFI panic-firewall error envelope (`{"error":…}`). A cheap
 *  prefix check avoids tree-parsing a possibly large view blob on every read.
 *  Top-level (not a [Core] member) so it is JVM-unit-testable without loading the
 *  native library. */
internal fun isCoreErrorEnvelope(text: String): Boolean =
    text.trimStart().startsWith("{\"error\"")

/** Whether [text] is specifically a `panic_reset` envelope: the FFI caught a panic
 *  AND swapped in a fresh (empty) core bridge, so the shell must rebuild the
 *  core from the persisted log. The plain `panic` kind (bridge NOT reset) is a
 *  generic error envelope handled by keeping the last good view. */
internal fun isCorePanicReset(text: String): Boolean =
    isCoreErrorEnvelope(text) && text.contains("\"kind\":\"panic_reset\"")

/** Whether a raw JNI `update` response is a `panic_reset` envelope. An accepted
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
 * The outcome of [Core.send]: the fresh [view]; whether the core [accepted] the
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
