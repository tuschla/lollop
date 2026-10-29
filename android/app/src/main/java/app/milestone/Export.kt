package app.milestone

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.util.Log
import androidx.core.content.edit
import androidx.core.content.FileProvider
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.FileNotFoundException
import java.io.FileOutputStream
import java.io.IOException
import java.io.InputStream
import java.time.Instant
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.ZoneId
import java.time.format.DateTimeFormatter

/** Cap on retained GPX share-temp files in the cache; older ones are pruned. */
private const val MAX_KEPT_EXPORTS = 20

/**
 * Write a core-produced GPX document to app cache and fire a share sheet so the
 * user can hand it to Strava/Garmin/Komoot/Drive/etc. The GPX string itself is
 * built on demand in the Rust core ([Core.exportGpx]); the shell only persists + shares.
 *
 * The filename the user sees in the share target / saves to Drive is the run's own
 * local start time (`run-2026-10-14-0742.gpx`), not the export time, so the file
 * names the run it holds. Two runs never start in the same minute; re-exporting a
 * run rewrites its one file with the same document.
 */
suspend fun shareGpx(ctx: Context, gpx: String, runStartEpochSec: Long) {
    // The cache prune + file write are disk IO: do them off the main thread
    // (mirrors the already-IO import path); only the share-sheet launch returns
    // to the caller's (main) dispatcher.
    val uri = withContext(Dispatchers.IO) { writeGpxToCache(ctx, gpx, runStartEpochSec) }
    val send = Intent(Intent.ACTION_SEND).apply {
        type = "application/gpx+xml"
        putExtra(Intent.EXTRA_STREAM, uri)
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    }
    ctx.startActivity(Intent.createChooser(send, ctx.getString(R.string.ui_export_chooser_title)).apply {
        addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    })
}

/** The GPX share file name for a run that started at [runStartEpochSec] (local time). */
internal fun gpxFileName(runStartEpochSec: Long, zone: ZoneId = ZoneId.systemDefault()): String =
    "run-" + Instant.ofEpochSecond(runStartEpochSec).atZone(zone)
        .format(DateTimeFormatter.ofPattern("yyyy-MM-dd-HHmm")) + ".gpx"

/** Persist the GPX to app cache and return a shareable FileProvider URI. Disk IO. */
private fun writeGpxToCache(ctx: Context, gpx: String, runStartEpochSec: Long): Uri {
    val dir = File(ctx.cacheDir, "exports").apply { mkdirs() }
    // These are share-temp files the OS only evicts under cache pressure, so a
    // heavy exporter would otherwise let them pile up indefinitely. Keep the most
    // recently written handful and drop the rest before writing the new one, so a
    // just-shared file is never the victim.
    val name = gpxFileName(runStartEpochSec)
    dir.listFiles { f -> f.name.endsWith(".gpx") && f.name != name }
        ?.sortedByDescending { it.lastModified() }
        ?.drop(MAX_KEPT_EXPORTS - 1)
        ?.forEach { it.delete() }
    val file = File(dir, name)
    file.writeText(gpx)
    return FileProvider.getUriForFile(ctx, "${ctx.packageName}.fileprovider", file)
}

// ---------------------------------------------------------------------------
// Full-data export / import: the whole event log is the source of truth, so a
// backup is just the compacted NDJSON, and a restore replaces the log wholesale
// and lets the core replay it on next boot. Kept out of the OS auto-backup (see
// res/xml/backup_rules.xml) so the user moves it deliberately.
// ---------------------------------------------------------------------------

/** Cap on a whole import candidate, from what the app really writes. A backup is
 *  the compacted log, and every GPS run is one LogRunTrack line of up to
 *  [TRACK_DECIMATION_CAP] fixes: one year of a hybrid athlete recording at the
 *  tracker's screen-off cadence (1 fix / 4 s) exports to about 11.7 MiB (146 GPS
 *  runs, PersistenceTortureTest.aOneYearBackupWithGpsRunsIsImportable), five years
 *  to about 50 MiB. 256 MiB is two decades of that, so any backup the app produced
 *  re-imports, while a pick past it is not one of ours. The reader STREAMS within
 *  this bound, and a non-log file fails on its first line ([MAX_IMPORT_LINE_BYTES]
 *  bounds a newline-less one), so this is a sanity limit; the valid lines are then
 *  held whole, as boot holds the log. */
internal const val MAX_IMPORT_BYTES = 256L * 1024 * 1024

/** Cap on one import line. The longest line the app writes is a LogRunTrack: at
 *  most [TRACK_DECIMATION_CAP] fixes (plus kept pause boundaries) of at most ~140
 *  wire bytes each (four full-precision numbers and their keys), so under 1 MiB.
 *  16 MiB leaves room for an undecimated track from an older build; a longer
 *  "line" (a newline-less binary pick) is refused before it can fill the line
 *  buffer. */
internal const val MAX_IMPORT_LINE_BYTES = 16L * 1024 * 1024

/** [SETUP_PREFS] key: how many entries a data import restored, written
 *  just before the restart so the next boot can confirm it, then removed. */
internal const val KEY_RESTORED_ENTRIES = "restored_entries"

/** Default filename shown in the "save as" chooser: lollop-YYYY-MM-DD.ndjson. Import
 *  sniffs content, never the name, so older milestone-*.ndjson exports still load. */
fun dataExportFilename(): String =
    "lollop-" + LocalDate.ofEpochDay(todayEpochDay()).format(DateTimeFormatter.ofPattern("yyyy-MM-dd")) + ".ndjson"

/** Write the compacted event log to the user-chosen document uri; true only when
 *  every line was written. The log is read with the read-only [EventLog.read]
 *  inside an exclusive dispatcher turn, so no append interleaves with the read and
 *  the export never rewrites the live log.
 *
 *  A removed plan ([KEY_PLAN_CLEARED]) is normally the log's own final `ClearPlan`
 *  line, which compaction keeps ([RestoredLog.planRemoved]). A log rewritten by an
 *  older build dropped it with the plan it cleared, so the backup ends with a
 *  `ClearPlan` line when the log holds no plan line: restoring it
 *  ([replaceEventLog]) then keeps the plan removed instead of generating one. */
suspend fun exportDataTo(ctx: Context, uri: Uri): Boolean {
    val lines = runCatching {
        var read = emptyList<String>()
        EventDispatcher.exclusive {
            read = withContext(Dispatchers.IO) {
                val log = EventLog.read(eventLogFile(ctx.filesDir)).lines
                val planRemoved = ctx.setupPrefs().getBoolean(KEY_PLAN_CLEARED, false)
                if (planRemoved && lastPlanEvent(log) == null) log + CLEAR_PLAN_LINE else log
            }
            null
        }
        read
    }.getOrElse {
        Log.e("Export", "couldn't read the event log for export", it)
        return false
    }
    return withContext(Dispatchers.IO) {
        runCatching {
            val out = try {
                ctx.contentResolver.openOutputStream(uri, "wt")
            } catch (e: IllegalArgumentException) {
                // Some document providers reject the "wt" truncate mode; retry with "w".
                ctx.contentResolver.openOutputStream(uri, "w")
            } catch (e: FileNotFoundException) {
                ctx.contentResolver.openOutputStream(uri, "w")
            } ?: return@runCatching false
            out.bufferedWriter().use { w ->
                lines.forEach { w.append(it); w.append('\n') }
            }
            true
        }.getOrDefault(false)
    }
}

/** [Event.ClearPlan] as a log line. */
private val CLEAR_PLAN_LINE = Event.ClearPlan.toJson().toString()

/** The variant of the last `GeneratePlan` / `ClearPlan` line in [lines], or null
 *  when there is none: "ClearPlan" means the log ends with the plan removed. */
internal fun lastPlanEvent(lines: List<String>): String? =
    lines.asReversed().firstNotNullOfOrNull { line ->
        // Substring pre-check so the scan never JSON-parses a large track line.
        if ("GeneratePlan" !in line && "ClearPlan" !in line) return@firstNotNullOfOrNull null
        EventLog.variantOf(line)?.takeIf { it == "GeneratePlan" || it == "ClearPlan" }
    }

/** Outcome of reading + validating an import candidate before it can replace the log. */
sealed interface ImportCheck {
    data class Valid(val lines: List<String>) : ImportCheck
    object Invalid : ImportCheck
}

/**
 * Stream [input] line by line, validating each non-blank line's variant as it is
 * read and returning the survivors, or [ImportCheck.Invalid] on the first
 * unknown-variant / non-JSON line, a line longer than [maxLine], OR once the
 * running byte count crosses [max].
 *
 * Peak memory stays one buffered line (at most [maxLine]) plus the accumulated
 * valid lines: the whole file is NEVER materialized as bytes, so a mistaken large
 * non-log pick fails gracefully instead of transiently holding ~3× its size and
 * OOM-ing. A content provider may report an unknown size (statSize
 * == -1), so the running byte bound — not a declared-size pre-check — is what stops
 * an oversized or endless stream; a binary file trips the variant check on its first
 * garbage line (or the line cap), well before [max].
 */
internal fun streamValidateImport(input: InputStream, max: Long, maxLine: Long = MAX_IMPORT_LINE_BYTES): ImportCheck {
    val lines = ArrayList<String>()
    val lineBuf = ByteArrayOutputStream()
    val buf = ByteArray(64 * 1024)
    var total = 0L
    fun flushLine(): Boolean {
        val s = lineBuf.toByteArray().toString(Charsets.UTF_8).trim()
        lineBuf.reset()
        if (s.isEmpty()) return true
        if (EventLog.variantOf(s) == null) return false
        lines.add(s)
        return true
    }
    while (true) {
        val n = input.read(buf)
        if (n < 0) break
        if (total + n > max) return ImportCheck.Invalid
        total += n
        var start = 0
        for (i in 0 until n) {
            if (buf[i] == '\n'.code.toByte()) {
                if (lineBuf.size() + (i - start) > maxLine) return ImportCheck.Invalid
                lineBuf.write(buf, start, i - start)
                if (!flushLine()) return ImportCheck.Invalid
                start = i + 1
            }
        }
        if (lineBuf.size() + (n - start) > maxLine) return ImportCheck.Invalid
        if (start < n) lineBuf.write(buf, start, n - start)
    }
    if (!flushLine()) return ImportCheck.Invalid
    return if (lines.isEmpty()) ImportCheck.Invalid else ImportCheck.Valid(lines)
}

/** Read a candidate uri (capped at [MAX_IMPORT_BYTES]) and validate every non-blank
 *  line as a known event variant, STREAMING so a large non-log pick can't OOM the
 *  process. An unknown variant or a non-JSON line rejects the whole file. Disk/IO +
 *  parse only: the live log is untouched. */
suspend fun readImportCandidate(ctx: Context, uri: Uri): ImportCheck = withContext(Dispatchers.IO) {
    runCatching {
        val declared = ctx.contentResolver.openFileDescriptor(uri, "r")?.use { it.statSize } ?: -1L
        if (declared > MAX_IMPORT_BYTES) return@runCatching ImportCheck.Invalid
        ctx.contentResolver.openInputStream(uri)?.use { streamValidateImport(it, MAX_IMPORT_BYTES) }
            ?: ImportCheck.Invalid
    }.getOrDefault(ImportCheck.Invalid)
}

/** What an import candidate holds, for the replace confirmation: how many logged
 *  [entries] (sets, runs, activities, check-ins, readiness reports) survive compaction, and the
 *  local epoch days of the oldest/newest of them (null when none carries a date). */
data class ImportSummary(val entries: Int, val firstDay: Long?, val lastDay: Long?)

private val importEntryVariants = setOf("LogSet", "LogRun", "LogRunTrack", "LogActivity", "SubmitCheckin", "SubmitReadiness")

private val summaryJson = Json { ignoreUnknownKeys = true }

/** Summarize validated import [lines] as the restored log will hold them (compacted
 *  first, so cleared or superseded lines don't count), dating entries in [zone]. */
internal fun summarizeImport(lines: List<String>, zone: ZoneId): ImportSummary {
    var entries = 0
    var first: Long? = null
    var last: Long? = null
    for (line in EventLog.compact(lines)) {
        val variant = EventLog.variantOf(line) ?: continue
        if (variant !in importEntryVariants) continue
        entries++
        val payload = runCatching { summaryJson.parseToJsonElement(line) as JsonObject }.getOrNull()
            ?.get(variant) as? JsonObject
        val at = payload?.get("observed_at")?.let { runCatching { it.jsonPrimitive.longOrNull }.getOrNull() }
        if (at == null || at <= 0L) continue
        val day = Instant.ofEpochSecond(at).atZone(zone).toLocalDate().toEpochDay()
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
 * [DataImportResult.Failed]. Refused while a run is recording or armed,
 * because the restart that must follow ends the tracking service. On
 * [DataImportResult.Replaced] the caller restarts with [restartForImport].
 *
 * The "plan removed" marker ([KEY_PLAN_CLEARED]) describes the log, so it is set
 * from the imported one (its last plan line is a `ClearPlan`, see [exportDataTo]):
 * a backup of a removed plan restores without one, and this device's own marker
 * never outlives its replaced log.
 */
suspend fun replaceEventLog(ctx: Context, lines: List<String>): DataImportResult {
    if (RunSession.tracking.value || RunSession.armed.value) return DataImportResult.RunInProgress
    return try {
        EventDispatcher.exclusive {
            withContext(CoreJniDispatcher + NonCancellable) {
                val previous = Core.detachLog()
                try {
                    swapLogFile(ctx.filesDir, lines)
                } catch (t: Throwable) {
                    Core.reattachLog(previous)
                    throw t
                }
                ctx.setupPrefs().edit(commit = true) { putBoolean(KEY_PLAN_CLEARED, lastPlanEvent(lines) == "ClearPlan") }
            }
            null
        }
        DataImportResult.Replaced
    } catch (t: Throwable) {
        if (t.isFatalToVm()) throw t
        Log.e("Export", "data import failed", t)
        DataImportResult.Failed
    }
}

/** A throwable after which the VM itself can't be trusted to go on, so it must
 *  not be reported as a failed import. An OutOfMemoryError (one allocation the
 *  heap couldn't serve) or a StackOverflowError is not: the throwing frames
 *  unwind and free what they held, and the user keeps their old log. */
private fun Throwable.isFatalToVm(): Boolean =
    this is VirtualMachineError && this !is OutOfMemoryError && this !is StackOverflowError

/** Atomically replace the event log in [dir] with [lines] (tmp + fsync +
 *  rename), so a crash mid-write can never leave a half-written log. The lines are
 *  STREAMED to the temporary through one small buffer: a 5-year backup (~49 MiB)
 *  joined into one string OOM-killed the app at a 192 MB heap. Throws when the log
 *  could not be replaced (an OutOfMemoryError included); the temporary is then
 *  deleted and the original is still in place (or set aside as `.bak`, which the
 *  next boot's [EventLog.load] restores). */
internal fun swapLogFile(dir: File, lines: List<String>) {
    val dst = eventLogFile(dir)
    val tmp = File(dst.path + ".import")
    try {
        // fsync the bytes to disk BEFORE the rename: a rename-replace without fsync
        // can commit the directory metadata (the new name) before the file's data
        // reaches disk, so a power loss right after the rename can truncate the WHOLE
        // log to zero/garbage.
        FileOutputStream(tmp).use { out ->
            val w = out.bufferedWriter(Charsets.UTF_8)
            for (line in lines) {
                w.write(line)
                w.write('\n'.code)
            }
            w.flush()
            out.fd.sync()
        }
        if (tmp.renameTo(dst)) return
        // Some filesystems refuse an atomic rename over an existing file. Move the live
        // log aside to a .bak before overwriting so a crash mid-swap can restore it,
        // rather than deleting it outright (a delete then a failed rename would lose the
        // log). The .bak is excluded from backup via res/xml/backup_rules.xml.
        val bak = File(dst.path + ".bak")
        bak.delete()
        val moved = dst.renameTo(bak)
        if (tmp.renameTo(dst)) {
            if (moved) bak.delete()
            return
        }
        if (moved) bak.renameTo(dst)
        throw IOException("couldn't replace the event log")
    } finally {
        // A no-op once renamed into place; otherwise no partial temporary survives.
        tmp.delete()
    }
}

/** Restart into the replaced log: start the launcher activity as a fresh task from
 *  this FOREGROUND [ctx] (a start from a backgrounded, exiting process can be
 *  blocked), then end the process so the core rebuilds from the imported log. */
fun restartForImport(ctx: Context) {
    val component = ctx.packageManager.getLaunchIntentForPackage(ctx.packageName)?.component
        ?: ComponentName(ctx, MainActivity::class.java)
    ctx.startActivity(Intent.makeRestartActivityTask(component))
    Runtime.getRuntime().exit(0)
}
