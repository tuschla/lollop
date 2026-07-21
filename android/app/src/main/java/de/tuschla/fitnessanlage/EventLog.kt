package de.tuschla.fitnessanlage

import java.io.File
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

/**
 * Result of [EventLog.load]: the surviving replayable lines plus whether this is
 * a genuinely fresh install. [freshInstall] is true ONLY when the log file never
 * existed, a log that exists but compacted to zero surviving lines (the user
 * cleared all their data) is a RETURNING user, and the caller must not re-seed
 * the onboarding profile over their deliberate empty state.
 */
data class RestoredLog(val lines: List<String>, val freshInstall: Boolean)

/**
 * Event-log compaction over raw serde wire lines. Lives outside [Core] (which
 * loads `libshared.so` in its initializer) so it stays pure Kotlin and is unit
 * testable on the JVM without a device or the native library.
 *
 * Mirrors the authoritative Rust implementation `compact_event_log`
 * (shared/src/log.rs), which the web shell uses directly and whose
 * replay-equivalence tests pin the rules. That function isn't exposed over the
 * JSON FFI, so this reimplements it over raw wire lines; the two must stay in
 * lockstep: a new `Event` family added there must be added here.
 */
object EventLog {
    private val json = Json { ignoreUnknownKeys = true }

    /** Clear-variant → the member variants its clear supersedes. */
    private val families = mapOf(
        "ClearReadiness" to setOf("SubmitReadiness"),
        "ClearSets" to setOf("LogSet"),
        "ClearRuns" to setOf("LogRun", "LogRunTrack"),
        "ClearProfile" to setOf("SetProfile"),
        "ClearReview" to setOf("SubmitReview"),
        "ClearRacePrediction" to setOf("PredictRace"),
        "ClearHypertrophyPlan" to setOf("PlanHypertrophyMeso"),
        "ClearProtein" to setOf("ComputeProtein"),
        "ClearHrZones" to setOf("ComputeHrZones"),
        "ClearCooper" to setOf("ComputeCooper"),
        "ClearCriticalSpeed" to setOf("ComputeCriticalSpeed"),
        "ClearApre" to setOf("ComputeApre"),
    )

    /** Last-write-wins singleton variants (assign a scalar model field outright). */
    private val singletons = listOf(
        "SetProfile", "SubmitReview", "PredictRace", "PlanHypertrophyMeso", "ComputeProtein",
        "ComputeHrZones", "ComputeCooper", "ComputeCriticalSpeed", "ComputeApre",
    )

    /**
     * Drop log lines whose effect on the replayed model is provably nil, returning
     * a shorter but replay-equivalent line list (same relative order of survivors).
     *
     * Two rules, both grounded in the core's `update` (app.rs): the model stores
     * only raw inputs and derives everything in `view`, and no event's `update`
     * reads another family's state.
     *  1. **Clear supersedes its family.** A `Clear<F>` empties family F's vec, so
     *     every F event (and the clear) at or before the *last* `Clear<F>` leaves
     *     no residue, F events after it replay against an already-empty vec.
     *  2. **Last write wins for singletons.** `SetProfile`/`SubmitReview` assign a
     *     scalar model field outright, and nothing else reads it at update time, so
     *     only the last surviving one matters.
     */
    fun compact(lines: List<String>): List<String> {
        val variant = lines.map(::variantOf)
        val remove = BooleanArray(lines.size)

        for ((clear, members) in families) {
            val lastClear = variant.indexOfLast { it == clear }
            if (lastClear < 0) continue
            for (i in 0..lastClear) {
                if (variant[i] == clear || variant[i] in members) remove[i] = true
            }
        }
        for (singleton in singletons) {
            val survivors = lines.indices.filter { !remove[it] && variant[it] == singleton }
            survivors.dropLast(1).forEach { remove[it] = true }
        }
        return lines.filterIndexed { i, _ -> !remove[i] }
    }

    /**
     * Read + compact the persisted event log, rewriting [file] in place when
     * compaction dropped lines (atomic tmp-file + rename, so a crash mid-write
     * can never truncate the durable log). Pure file/JSON logic, no native
     * library, so the fresh-install-vs-compacted-empty distinction is unit
     * testable on the JVM (see EventLogTest); [Core.restore] replays the result.
     */
    fun load(file: File): RestoredLog {
        if (!file.exists()) return RestoredLog(emptyList(), freshInstall = true)
        val lines = file.readLines().filter { it.isNotBlank() }
        val kept = compact(lines)
        if (kept.size < lines.size) {
            val tmp = File(file.parentFile, file.name + ".tmp")
            runCatching {
                tmp.writeText(kept.joinToString("\n", postfix = "\n"))
                if (!tmp.renameTo(file)) tmp.delete()
            }
        }
        return RestoredLog(kept, freshInstall = false)
    }

    /**
     * The serde variant tag of one wire line, or null if it doesn't parse (a
     * hand-edited or forward-incompatible line is then never removable). Unit
     * variants serialize as a bare JSON string (`"ClearRuns"`); struct/newtype
     * variants as a single-key object (`{"LogRun": …}`).
     */
    fun variantOf(line: String): String? = runCatching {
        when (val el = json.parseToJsonElement(line)) {
            is JsonPrimitive -> if (el.isString) el.content else null
            is JsonObject -> el.keys.singleOrNull()
            else -> null
        }
    }.getOrNull()
}
