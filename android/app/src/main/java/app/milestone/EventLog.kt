package app.milestone

import java.io.File
import java.io.FileOutputStream
import java.io.RandomAccessFile
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

/**
 * Result of [EventLog.load] / [EventLog.read]: the surviving replayable lines plus
 * whether this is a genuinely fresh install. [freshInstall] is true ONLY when the
 * log file never existed, a log that exists but compacted to zero surviving lines
 * (the user cleared all their data) is a RETURNING user, and the caller must not
 * re-seed the onboarding profile over their deliberate empty state. [maxEntryId]
 * is the largest `entry_id` any intact line carries (0 when none). [planRemoved]
 * is true when the surviving log's last plan line is a `ClearPlan` (the user
 * removed their plan): compaction keeps that line, so boot can honour the
 * removal even when the setup prefs' marker was lost.
 */
data class RestoredLog(
    val lines: List<String>,
    val freshInstall: Boolean,
    val maxEntryId: Long = 0L,
    val planRemoved: Boolean = false,
)

/** The app's event log in [dir] (the app's `filesDir`). */
internal fun eventLogFile(dir: File) = File(dir, "event-log.ndjson")

/** The core's compaction verdict on a log's lines (log.rs `Compaction`), by line
 *  index: the lines compaction drops, the lines that are not JSON (a tail torn by
 *  a kill mid-append), the largest `entry_id` and whether the plan was removed. */
@Serializable
internal class LogCompaction(
    val dropped: List<Int> = emptyList(),
    val torn: List<Int> = emptyList(),
    val max_entry_id: Long = 0L,
    val plan_removed: Boolean = false,
)

/**
 * Reads, repairs and compacts the persisted event log (one event's serde wire JSON
 * per line). The compaction rules live in the core (`shared/src/log.rs`, reached
 * through [Core.compaction]). Without a working core the log is read as it is,
 * minus torn lines, so the raw-log export still works.
 */
object EventLog {
    private val json = Json { ignoreUnknownKeys = true }

    /** Event variants that serialize as a BARE JSON string (Rust unit variants). A
     *  named object such as {"ClearSets":{}} is not one: the core rejects a unit
     *  variant carrying a payload. */
    private val unitVariants = setOf(
        "ClearReadiness", "ClearSets", "ClearRuns", "ClearCheckins", "ClearProfile",
        "ClearReview", "ClearWeeklyCheckin", "ClearRacePrediction", "ClearHypertrophyPlan", "ClearProtein",
        "ClearHrZones", "ClearPlan", "ClearActivities",
    )

    /** Event variants that serialize as a single-key object ({"LogSet":{…}}). A known
     *  name with a non-object payload ({"LogSet":5}) is a shape the core rejects. */
    private val structVariants = setOf(
        "SubmitReadiness", "RemoveReadiness", "ResolveHold", "SubmitCheckin", "RestartHrvBaseline", "LogSet", "AmendSet",
        "LogRun", "LogRunTrack", "AmendRun", "TrimRunTrack", "AmendRunMeta", "DeleteEntry",
        "SetProfile", "SubmitReview", "SubmitWeeklyCheckin", "PredictRace", "PlanHypertrophyMeso",
        "ComputeProtein", "ComputeHrZones", "GeneratePlan", "SetToday", "SetExerciseEnded", "SetCalisthenicsRung",
        "MoveSession", "LogActivity",
    )

    /** [lines] minus those the core's compaction drops (replay-equivalent, order
     *  kept); unchanged when the core can't answer. A line that is not JSON is kept. */
    fun compact(lines: List<String>): List<String> {
        val dropped = Core.compaction(lines)?.dropped?.toHashSet() ?: return lines
        return lines.filterIndexed { i, _ -> i !in dropped }
    }

    /**
     * Read + compact the persisted event log, rewriting [file] in place when
     * compaction dropped enough lines to be worth the fsync
     * ([compactionWorthRewriting]) OR the durable tail is torn: an unparseable line
     * or a newline-less last line left by a process kill mid-append. The rewrite is
     * atomic (tmp-file + fsync + rename) and always ends the file with a newline, so
     * a torn tail heals instead of the next append gluing its JSON onto the partial
     * bytes (which would lose that event on the next boot). [Core.restore] replays
     * the returned survivors whether or not the file was rewritten.
     *
     * Mutates the log directory, so it runs only at boot, before anything appends.
     * Anything reading the log while the core may be appending uses [read].
     */
    fun load(file: File): RestoredLog {
        // A crash between the two renames of an import swap can leave the live log
        // set aside as .bak with the destination missing: put it back (a copy when
        // the rename fails) before deciding the install is fresh, and delete .bak
        // only once the log is in place. Leftover .tmp/.import files are stale the
        // moment no operation is in flight.
        val bak = File(file.path + ".bak")
        if (!file.exists() && bak.exists() && !bak.renameTo(file)) bak.copyTo(file)
        File(file.path + ".tmp").delete()
        File(file.path + ".import").delete()
        if (file.exists()) bak.delete()
        if (!file.exists()) return RestoredLog(emptyList(), freshInstall = true)
        val parsed = parse(file)
        val kept = parsed.restored.lines
        // A launch that shaves a line or two replays the result in memory and leaves
        // the disk alone; a torn tail always forces the rewrite.
        val torn = parsed.unparseable || !endsWithNewline(file)
        if (torn || compactionWorthRewriting(parsed.total - kept.size, parsed.total)) {
            val tmp = File(file.path + ".tmp")
            runCatching {
                // fsync before the rename: a rename can reach the disk before the
                // data, and a power loss then truncates the whole log. Streamed line
                // by line so no joined copy of a multi-year log doubles the heap.
                FileOutputStream(tmp).use { out ->
                    val w = out.bufferedWriter()
                    if (kept.isEmpty()) w.write("\n")
                    for (line in kept) {
                        w.write(line)
                        w.write("\n")
                    }
                    w.flush()
                    out.fd.sync()
                }
                if (!tmp.renameTo(file)) tmp.delete()
            }
        }
        return parsed.restored
    }

    /** A compaction drop at least this many lines long is always worth a rewrite. */
    internal const val REWRITE_MIN_DROPPED_LINES = 32

    /** …as is one of at least 1/[REWRITE_MIN_DROPPED_SHARE_INVERSE] (5%) of the log. */
    internal const val REWRITE_MIN_DROPPED_SHARE_INVERSE = 20

    /** Whether dropping [dropped] of [total] lines justifies rewriting (and fsyncing)
     *  the durable log: a large absolute drop, or a large share of it. */
    internal fun compactionWorthRewriting(dropped: Int, total: Int): Boolean =
        dropped > 0 &&
            (dropped >= REWRITE_MIN_DROPPED_LINES || dropped * REWRITE_MIN_DROPPED_SHARE_INVERSE >= total)

    /**
     * Read + compact the event log WITHOUT touching the disk: no temporaries are
     * removed and nothing is rewritten, so it is safe while the core may append (a
     * half-written tail line is torn and left out of the result). The data export
     * and the mid-session panic-reset replay read through this.
     */
    fun read(file: File): RestoredLog {
        if (!file.exists()) return RestoredLog(emptyList(), freshInstall = true)
        return parse(file).restored
    }

    /** [read]'s result plus what [load] needs to decide on a rewrite: the count of
     *  non-blank lines on disk and whether any of them was torn. */
    private class Parsed(val restored: RestoredLog, val total: Int, val unparseable: Boolean)

    private fun parse(file: File): Parsed {
        val lines = ArrayList<String>()
        file.bufferedReader().useLines { all -> all.filterTo(lines) { it.isNotBlank() } }
        val verdict = Core.compaction(lines)
            ?: LogCompaction(torn = lines.indices.filter { runCatching { json.parseToJsonElement(lines[it]) }.isFailure })
        val skip = BooleanArray(lines.size)
        verdict.dropped.forEach { skip[it] = true }
        verdict.torn.forEach { skip[it] = true }
        return Parsed(
            RestoredLog(
                lines.filterIndexed { i, _ -> !skip[i] },
                freshInstall = false,
                maxEntryId = verdict.max_entry_id,
                planRemoved = verdict.plan_removed,
            ),
            total = lines.size,
            unparseable = verdict.torn.isNotEmpty(),
        )
    }

    /** Whether [file]'s durable bytes end in '\n'. An empty file counts as
     *  terminated (nothing to glue onto). */
    internal fun endsWithNewline(file: File): Boolean =
        RandomAccessFile(file, "r").use {
            if (it.length() == 0L) true else { it.seek(it.length() - 1); it.read() == '\n'.code }
        }

    /** The serde variant tag of one wire line, or null when it names no event the
     *  core accepts in that shape: unit variants are a bare JSON string
     *  (`"ClearRuns"`), struct variants a single-key object (`{"LogRun": {…}}`). */
    fun variantOf(line: String): String? =
        when (val el = runCatching { json.parseToJsonElement(line) }.getOrNull()) {
            is JsonPrimitive -> if (el.isString && el.content in unitVariants) el.content else null
            is JsonObject -> el.keys.singleOrNull()?.takeIf { it in structVariants && el[it] is JsonObject }
            else -> null
        }
}
