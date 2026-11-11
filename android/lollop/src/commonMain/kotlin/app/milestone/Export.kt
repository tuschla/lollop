package app.milestone

import androidx.compose.runtime.Composable
import kotlin.time.Instant
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.IO
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.withContext
import kotlinx.datetime.LocalDate
import kotlinx.datetime.TimeZone
import kotlinx.datetime.toLocalDateTime
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull

/**
 * Write a core-produced GPX document to a share-temp file and open the platform
 * share sheet so the user can hand it to Strava/Garmin/Komoot/Drive/etc. The GPX
 * string itself is built on demand in the Rust core ([DurableCore.runGpx]); the
 * shell only persists + shares.
 *
 * The filename the user sees in the share target / saves to Drive is the run's own
 * local start time ([gpxFileName]: `run-2026-10-14-0742.gpx`), not the export time,
 * so the file names the run it holds. Two runs never start in the same minute;
 * re-exporting a run rewrites its one file with the same document. Android: a
 * FileProvider uri in the share chooser; iOS: a temporary file in the share sheet.
 */
internal expect suspend fun shareGpx(ctx: PlatformContext, gpx: String, runStartEpochSec: Long)

/** The GPX share file name for a run that started at [runStartEpochSec] (local time). */
internal fun gpxFileName(runStartEpochSec: Long, zone: TimeZone = TimeZone.currentSystemDefault()): String {
    val t = Instant.fromEpochSeconds(runStartEpochSec).toLocalDateTime(zone)
    return "run-" + t.date + "-" + t.hour.twoDigits() + t.minute.twoDigits() + ".gpx"
}

private fun Int.twoDigits(): String = toString().padStart(2, '0')

// ---------------------------------------------------------------------------
// Full-data export / import: the whole event log is the source of truth, so a
// backup is just the compacted NDJSON, and a restore replaces the log wholesale
// and lets the core replay it ([AppRuntime.requestReboot]). Kept out of the OS auto-backup
// (see res/xml/backup_rules.xml) so the user moves it deliberately.
// ---------------------------------------------------------------------------

/** Cap on a whole import candidate, from what the app really writes. A backup is
 *  the compacted log, and every GPS run is one LogRunTrack line of up to
 *  [CoreConstants.TRACK_DECIMATION_CAP] fixes: one year of a hybrid athlete recording at the
 *  tracker's screen-off cadence (1 fix / 4 s) exports to about 11.7 MiB (146 GPS
 *  runs, PersistenceTortureTest.aOneYearBackupWithGpsRunsIsImportable), five years
 *  to about 50 MiB. 256 MiB is two decades of that, so any backup the app produced
 *  re-imports, while a pick past it is not one of ours. The reader STREAMS within
 *  this bound, and a non-log file fails on its first line ([MAX_IMPORT_LINE_BYTES]
 *  bounds a newline-less one), so this is a sanity limit; the valid lines are then
 *  held whole, as boot holds the log. */
internal const val MAX_IMPORT_BYTES = 256L * 1024 * 1024

/** Cap on one import line. The longest line the app writes is a LogRunTrack: at
 *  most [CoreConstants.TRACK_DECIMATION_CAP] fixes (plus kept pause boundaries) of at most ~140
 *  wire bytes each (four full-precision numbers and their keys), so under 1 MiB.
 *  16 MiB leaves room for an undecimated track from an older build; a longer
 *  "line" (a newline-less binary pick) is refused before it can fill the line
 *  buffer. */
internal const val MAX_IMPORT_LINE_BYTES = 16L * 1024 * 1024

/** Default filename of a data export: lollop-YYYY-MM-DD.ndjson. Import sniffs
 *  content, never the name, so older milestone-*.ndjson exports still load. */
internal fun dataExportFilename(): String = "lollop-" + LocalDate.fromEpochDays(todayEpochDay()) + ".ndjson"

/** A "save as" for the full data export ([readDataExport]) that reports the outcome
 *  in a notice; [onExported] follows a successful save. Invoke the returned
 *  function to start it. The export runs at process scope, so closing the screen
 *  doesn't cancel it. Android: the system "create document" picker; iOS: the share
 *  sheet with the backup as a file (Save to Files, AirDrop), saved once the sheet
 *  completes. */
@Composable
internal expect fun rememberDataExport(onExported: () -> Unit = {}): () -> Unit

/** The compacted event log as a backup holds it, or null when it couldn't be read.
 *  The log is read with the read-only [EventLog.read] inside an exclusive
 *  dispatcher turn, so no append interleaves with the read and the export never
 *  rewrites the live log. A removed plan is the log's own final `ClearPlan` line,
 *  which compaction keeps, so the backup restores with the plan removed. */
internal suspend fun readDataExport(ctx: PlatformContext): List<String>? = runCatching {
    var read = emptyList<String>()
    EventDispatcher.exclusive {
        read = withContext(Dispatchers.IO) {
            EventLog.read(ctx.eventLogPath(), AppRuntime.engine).lines
        }
        null
    }
    read
}.getOrElse {
    PlatformLog.e("Export", "couldn't read the event log for export", it)
    null
}

/** Outcome of reading + validating an import candidate before it can replace the log. */
sealed interface ImportCheck {
    data class Valid(val lines: List<String>) : ImportCheck
    object Invalid : ImportCheck
}

/** The backup-file picker: invoke the returned function to open it; [onPicked] gets
 *  the picked file as a reference [readPickedBackup] reads (Android: its content
 *  uri; iOS: the file URL of the picked copy), nothing on cancel. A string, so the
 *  pick survives rotation in saveable state. */
@Composable
internal expect fun rememberBackupPicker(onPicked: (String) -> Unit): () -> Unit

/** Read the picked backup [file] (a [rememberBackupPicker] reference, capped at
 *  [MAX_IMPORT_BYTES]) and validate every non-blank line as a known event variant
 *  ([streamValidateImport]), STREAMING so a large non-log pick can't exhaust memory.
 *  Disk/IO + parse only: the live log is untouched. */
internal expect suspend fun readPickedBackup(ctx: PlatformContext, file: String): ImportCheck

/**
 * Stream a candidate through [read] (InputStream's contract: fill the buffer's
 * start, return the count, -1 at the end), validating each non-blank line's
 * variant as it is read and returning the survivors, or [ImportCheck.Invalid] on
 * the first unknown-variant / non-JSON line, a line longer than [maxLine], OR once
 * the running byte count crosses [max].
 *
 * Peak memory stays one buffered line (at most [maxLine]) plus the accumulated
 * valid lines: the whole file is NEVER materialized as bytes, so a mistaken large
 * non-log pick fails gracefully instead of transiently holding ~3× its size and
 * running out of memory. A content provider may report an unknown size (statSize
 * == -1), so the running byte bound — not a declared-size pre-check — is what stops
 * an oversized or endless stream; a binary file trips the variant check on its first
 * garbage line (or the line cap), well before [max].
 */
internal fun streamValidateImport(read: (ByteArray) -> Int, max: Long, maxLine: Long = MAX_IMPORT_LINE_BYTES): ImportCheck {
    val lines = ArrayList<String>()
    var line = ByteArray(256)
    var lineLen = 0
    val buf = ByteArray(64 * 1024)
    var total = 0L
    fun append(from: Int, count: Int) {
        val needed = lineLen + count
        if (needed > line.size) line = line.copyOf(maxOf(needed, minOf(line.size * 2L, maxLine).toInt()))
        buf.copyInto(line, lineLen, from, from + count)
        lineLen = needed
    }
    fun flushLine(): Boolean {
        val s = line.decodeToString(0, lineLen).trim()
        lineLen = 0
        if (s.isEmpty()) return true
        if (EventLog.variantOf(s) == null) return false
        lines.add(s)
        return true
    }
    while (true) {
        val n = read(buf)
        if (n < 0) break
        if (total + n > max) return ImportCheck.Invalid
        total += n
        var start = 0
        for (i in 0 until n) {
            if (buf[i] == '\n'.code.toByte()) {
                if (lineLen + (i - start) > maxLine) return ImportCheck.Invalid
                append(start, i - start)
                if (!flushLine()) return ImportCheck.Invalid
                start = i + 1
            }
        }
        if (lineLen + (n - start) > maxLine) return ImportCheck.Invalid
        if (start < n) append(start, n - start)
    }
    if (!flushLine()) return ImportCheck.Invalid
    return if (lines.isEmpty()) ImportCheck.Invalid else ImportCheck.Valid(lines)
}

/** What an import candidate holds, for the replace confirmation: how many logged
 *  [entries] (sets, runs, activities, check-ins, readiness reports) survive compaction, and the
 *  local epoch days of the oldest/newest of them (null when none carries a date). */
data class ImportSummary(val entries: Int, val firstDay: Long?, val lastDay: Long?)

private val importEntryVariants = setOf("LogSet", "LogRun", "LogRunTrack", "LogActivity", "SubmitCheckin", "SubmitReadiness")

/** Summarize validated import [lines] as the restored log will hold them (compacted
 *  first, so cleared or superseded lines don't count), dating entries in [zone]. */
internal fun summarizeImport(lines: List<String>, zone: TimeZone): ImportSummary {
    var entries = 0
    var first: Long? = null
    var last: Long? = null
    for (line in EventLog.compact(lines, AppRuntime.engine)) {
        val variant = EventLog.variantOf(line) ?: continue
        if (variant !in importEntryVariants) continue
        entries++
        val payload = runCatching { coreJson.parseToJsonElement(line) as JsonObject }.getOrNull()
            ?.get(variant) as? JsonObject
        val at = payload?.get("observed_at")?.let { runCatching { it.jsonPrimitive.longOrNull }.getOrNull() }
        if (at == null || at <= 0L) continue
        val day = localEpochDay(at, zone)
        first = minOf(first ?: day, day)
        last = maxOf(last ?: day, day)
    }
    return ImportSummary(entries, first, last)
}

/** How [replaceEventLog] ended. */
enum class DataImportResult { Replaced, RunInProgress, Failed }

/**
 * Replace the live event log with validated [lines] so the next boot replays them.
 * Runs as ONE exclusive dispatcher turn, non-cancellable, with the core's log
 * detached first: no queued event, run save or resume `SetToday` can append to the
 * imported file (it would replay on top of it), and a cancel can't strand the old
 * in-memory model over the new file. A failed write (an OutOfMemoryError
 * included: see [isFatalToVm]) re-attaches the original log and reports
 * [DataImportResult.Failed]. Refused while a run is recording or armed: the
 * reboot that must follow replaces the model the run would be saved into. On
 * [DataImportResult.Replaced] the caller reboots with [AppRuntime.requestReboot].
 */
internal suspend fun replaceEventLog(ctx: PlatformContext, lines: List<String>): DataImportResult {
    if (RunSession.runActive) return DataImportResult.RunInProgress
    return try {
        EventDispatcher.exclusive {
            withContext(CoreDispatcher + NonCancellable) {
                val engine = AppRuntime.engine
                val previous = engine.detachLog()
                try {
                    swapLogFile(ctx.eventLogPath(), lines)
                } catch (t: Throwable) {
                    engine.reattachLog(previous)
                    throw t
                }
            }
            null
        }
        DataImportResult.Replaced
    } catch (t: Throwable) {
        if (t.isFatalToVm()) throw t
        PlatformLog.e("Export", "data import failed", t)
        DataImportResult.Failed
    }
}

/** A throwable after which the runtime itself can't be trusted to go on, so it
 *  must not be reported as a failed import. Android: a VirtualMachineError other
 *  than an OutOfMemoryError (one allocation the heap couldn't serve) or a
 *  StackOverflowError, whose throwing frames unwind and free what they held, so
 *  the user keeps their old log. Kotlin/Native has no such error (it aborts). */
internal expect fun Throwable.isFatalToVm(): Boolean

/** Atomically replace the event log [log] with [lines] (tmp + fsync + rename), so a
 *  crash mid-write can never leave a half-written log. The lines are STREAMED to
 *  the temporary ([AppFiles.writeLinesSynced]): a 5-year backup (~49 MiB) joined
 *  into one string OOM-killed the app at a 192 MB heap. Throws when the log could
 *  not be replaced (an OutOfMemoryError included); the temporary is then deleted
 *  and the original is still in place (or set aside as `.bak`, which the next
 *  boot's [EventLog.load] restores). */
internal fun swapLogFile(log: AppPath, lines: List<String>) {
    val tmp = AppFiles.sibling(log, AppFileNames.IMPORT_SUFFIX)
    try {
        AppFiles.writeLinesSynced(tmp, lines)
        if (!AppFiles.rename(tmp, log) && !renameOverViaBackup(tmp, log)) error("couldn't replace the event log")
    } finally {
        // A no-op once renamed into place; otherwise no partial temporary survives.
        AppFiles.delete(tmp)
    }
}

/** Rename [tmp] over [log] where the rename over it failed: some filesystems refuse
 *  an atomic rename over an existing file. The live log is moved aside to a `.bak`
 *  (excluded from backup via res/xml/backup_rules.xml), not deleted, so a crash
 *  mid-swap can restore it; a delete then a failed rename would lose the log.
 *  False, with the log back in place, when [tmp] still can't take its name. */
private fun renameOverViaBackup(tmp: AppPath, log: AppPath): Boolean {
    val bak = AppFiles.sibling(log, AppFileNames.BAK_SUFFIX)
    AppFiles.delete(bak)
    val moved = AppFiles.rename(log, bak)
    if (AppFiles.rename(tmp, log)) {
        if (moved) AppFiles.delete(bak)
        return true
    }
    if (moved) AppFiles.rename(bak, log)
    return false
}
