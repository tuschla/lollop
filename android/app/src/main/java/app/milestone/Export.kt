package app.milestone

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.util.Log
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

/** Cap on an import candidate: a real (decimated) log is kilobytes-to-a-few-MiB, so
 *  anything past this is not one of ours. The reader STREAMS within this bound
 *  (never materializing the whole file), so this is a sanity limit, not the memory
 *  ceiling. */
internal const val MAX_IMPORT_BYTES = 8L * 1024 * 1024

private fun eventLogFile(ctx: Context) = File(ctx.filesDir, "event-log.ndjson")

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
 *  the export never rewrites the live log. */
suspend fun exportDataTo(ctx: Context, uri: Uri): Boolean {
    val lines = runCatching {
        var read = emptyList<String>()
        EventDispatcher.exclusive {
            read = withContext(Dispatchers.IO) { EventLog.read(eventLogFile(ctx)).lines }
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

/** Outcome of reading + validating an import candidate before it can replace the log. */
sealed interface ImportCheck {
    data class Valid(val lines: List<String>) : ImportCheck
    object Invalid : ImportCheck
}

/**
 * Stream [input] line by line, validating each non-blank line's variant as it is
 * read and returning the survivors, or [ImportCheck.Invalid] on the first
 * unknown-variant / non-JSON line OR once the running byte count crosses [max].
 *
 * Peak memory stays one buffered line plus the accumulated valid lines (a real log
 * is kilobytes): the whole file is NEVER materialized, so a mistaken large non-log
 * pick fails gracefully instead of transiently holding ~3× its size and OOM-ing.
 * A content provider may report an unknown size (statSize
 * == -1), so the running byte bound — not a declared-size pre-check — is what stops
 * an oversized or endless stream; a binary file trips the variant check on its first
 * garbage line, well before [max].
 */
internal fun streamValidateImport(input: InputStream, max: Long): ImportCheck {
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
                lineBuf.write(buf, start, i - start)
                if (!flushLine()) return ImportCheck.Invalid
                start = i + 1
            }
        }
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
 * in-memory model over the new file. A failed write re-attaches the original log
 * and reports [DataImportResult.Failed]. Refused while a run is recording or armed,
 * because the restart that must follow ends the tracking service. On
 * [DataImportResult.Replaced] the caller restarts with [restartForImport].
 */
suspend fun replaceEventLog(ctx: Context, lines: List<String>): DataImportResult {
    if (RunSession.tracking.value || RunSession.armed.value) return DataImportResult.RunInProgress
    return try {
        EventDispatcher.exclusive {
            withContext(CoreJniDispatcher + NonCancellable) {
                val previous = Core.detachLog()
                try {
                    swapLogFile(ctx.filesDir, lines)
                } catch (e: Exception) {
                    Core.reattachLog(previous)
                    throw e
                }
            }
            null
        }
        DataImportResult.Replaced
    } catch (e: Exception) {
        Log.e("Export", "data import failed", e)
        DataImportResult.Failed
    }
}

/** Atomically replace `event-log.ndjson` in [dir] with [lines] (tmp + fsync +
 *  rename), so a crash mid-write can never leave a half-written log. Throws when
 *  the log could not be replaced; the original is then still in place (or set
 *  aside as `.bak`, which the next boot's [EventLog.load] restores). */
internal fun swapLogFile(dir: File, lines: List<String>) {
    val dst = File(dir, "event-log.ndjson")
    val tmp = File(dir, "event-log.ndjson.import")
    // fsync the bytes to disk BEFORE the rename: a rename-replace without fsync
    // can commit the directory metadata (the new name) before the file's data
    // reaches disk, so a power loss right after the rename can truncate the WHOLE
    // log to zero/garbage.
    FileOutputStream(tmp).use {
        it.write(lines.joinToString("\n", postfix = "\n").toByteArray())
        it.fd.sync()
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
    tmp.delete()
    throw IOException("couldn't replace the event log")
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
