package app.milestone

/**
 * Result of [EventLog.load] / [EventLog.read]: the surviving replayable lines plus
 * whether this is a fresh install. [freshInstall] is true ONLY when the log
 * file never existed: a log that exists but compacted to zero surviving lines
 * (the user cleared all their data) is a RETURNING user, and the caller must not
 * re-seed the onboarding profile over their deliberate empty state. [maxEntryId]
 * is the largest `entry_id` any intact line carries (0 when none).
 */
data class RestoredLog(
    val lines: List<String>,
    val freshInstall: Boolean,
    val maxEntryId: Long = 0L,
)

/** The core's compaction of a log's [lines] ([DurableCore]), null when it can't answer. */
fun interface LogCompaction {
    fun compaction(lines: List<String>): Compaction?
}

/**
 * Reads, repairs and compacts the persisted event log (one event's serde wire JSON
 * per line). The compaction rules live in the core (`shared/src/log.rs`, reached
 * through the `core` argument). Without a working core the log is read as it
 * is, minus torn lines, so the raw-log export still works.
 */
object EventLog {

    /** [lines] minus those [core]'s compaction drops (replay-equivalent, order
     *  kept); unchanged when the core can't answer. A line that is not JSON is kept. */
    fun compact(lines: List<String>, core: LogCompaction): List<String> {
        val dropped = core.compaction(lines)?.dropped?.toHashSet() ?: return lines
        return lines.filterIndexed { i, _ -> i !in dropped }
    }

    /**
     * Read + compact the persisted event log, rewriting [file] in place when
     * compaction dropped enough lines to be worth the fsync
     * ([compactionWorthRewriting]) OR the durable tail is torn: an unparseable line
     * or a newline-less last line left by a process kill mid-append. The rewrite is
     * atomic (tmp-file + fsync + rename) and always ends the file with a newline, so
     * a torn tail heals instead of the next append gluing its JSON onto the partial
     * bytes (which would lose that event on the next boot). [DurableCore.restore] replays
     * the returned survivors whether or not the file was rewritten.
     *
     * Mutates the log directory, so it runs only at boot, before anything appends.
     * Anything reading the log while the core may be appending uses [read].
     */
    fun load(file: AppPath, core: LogCompaction): RestoredLog {
        // A crash between the two renames of an import swap can leave the live log
        // set aside as .bak with the destination missing: put it back (a copy when
        // the rename fails) before deciding the install is fresh, and delete .bak
        // only once the log is in place. Leftover .tmp/.import files are stale the
        // moment no operation is in flight.
        val bak = AppFiles.sibling(file, AppFileNames.BAK_SUFFIX)
        if (!AppFiles.exists(file) && AppFiles.exists(bak) && !AppFiles.rename(bak, file)) AppFiles.copy(bak, file)
        AppFiles.delete(AppFiles.sibling(file, AppFileNames.TMP_SUFFIX))
        AppFiles.delete(AppFiles.sibling(file, AppFileNames.IMPORT_SUFFIX))
        if (AppFiles.exists(file)) AppFiles.delete(bak)
        if (!AppFiles.exists(file)) return RestoredLog(emptyList(), freshInstall = true)
        val parsed = parse(file, core)
        val kept = parsed.restored.lines
        val torn = parsed.unparseable || !AppFiles.endsWithNewline(file)
        if (torn || compactionWorthRewriting(parsed.total - kept.size, parsed.total)) {
            val tmp = AppFiles.sibling(file, AppFileNames.TMP_SUFFIX)
            runCatching {
                AppFiles.writeLinesSynced(tmp, kept.ifEmpty { listOf("") })
                if (!AppFiles.rename(tmp, file)) AppFiles.delete(tmp)
            }.onFailure { PlatformLog.w("EventLog", "compacted log not rewritten, the old file stays", it) }
        }
        return parsed.restored
    }

    internal const val REWRITE_MIN_DROPPED_LINES = 32
    internal const val REWRITE_MIN_DROPPED_SHARE_INVERSE = 20

    internal fun compactionWorthRewriting(dropped: Int, total: Int): Boolean =
        dropped > 0 &&
            (dropped >= REWRITE_MIN_DROPPED_LINES || dropped * REWRITE_MIN_DROPPED_SHARE_INVERSE >= total)

    /**
     * Read + compact the event log WITHOUT touching the disk: no temporaries are
     * removed and nothing is rewritten, so it is safe while the core may append (a
     * half-written tail line is torn and left out of the result). The data export
     * and the mid-session panic-reset replay read through this.
     */
    fun read(file: AppPath, core: LogCompaction): RestoredLog {
        if (!AppFiles.exists(file)) return RestoredLog(emptyList(), freshInstall = true)
        return parse(file, core).restored
    }

    /** [read]'s result plus what [load] needs to decide on a rewrite: the count of
     *  non-blank lines on disk and whether any of them was torn. */
    private class Parsed(val restored: RestoredLog, val total: Int, val unparseable: Boolean)

    private fun parse(file: AppPath, core: LogCompaction): Parsed {
        val lines = ArrayList<String>()
        AppFiles.forEachLine(file) { if (it.isNotBlank()) lines += it }
        val verdict = core.compaction(lines)
            ?: Compaction(torn = lines.indices.filter { runCatching { coreJson.parseToJsonElement(lines[it]) }.isFailure })
        val skip = BooleanArray(lines.size)
        verdict.dropped.forEach { skip[it] = true }
        verdict.torn.forEach { skip[it] = true }
        return Parsed(
            RestoredLog(
                lines.filterIndexed { i, _ -> !skip[i] },
                freshInstall = false,
                maxEntryId = verdict.max_entry_id,
            ),
            total = lines.size,
            unparseable = verdict.torn.isNotEmpty(),
        )
    }

    /** The serde variant tag of one wire line, or null when it names no event the
     *  core accepts in that shape. */
    fun variantOf(line: String): String? =
        runCatching { coreJson.parseToJsonElement(line) }.getOrNull()?.let { Event.tagOf(it) }
}
