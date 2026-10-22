package app.milestone

import android.content.Context
import android.content.Intent
import android.content.res.Resources
import android.net.Uri
import android.provider.OpenableColumns
import android.util.Log
import androidx.core.content.IntentCompat
import java.io.ByteArrayOutputStream
import java.io.InputStream
import java.time.Instant
import java.time.ZoneId
import java.util.Locale
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** Import cap: real GPX/TCX/FIT exports are well under this; the cap keeps a
 *  pathological or wrong-type file from OOMing the process or blowing up JSON
 *  allocation across the parseFit JNI boundary. */
private const val MAX_RUN_IMPORT_BYTES = 32 * 1024 * 1024 // 32 MB

/** How much of a file the XML sniff decodes to find its root element. */
private const val SNIFF_BYTES = 4 * 1024

/** Chunk size of the bounded import read. */
private const val READ_BUFFER_BYTES = 64 * 1024

/** Above this many skipped files the result names none inline; "Details" lists them. */
private const val INLINE_SKIPPED_MAX = 2

/**
 * Read at most [cap] bytes from [stream], throwing [ImportFailure.TooLarge] if the
 * source is larger, so the bytes are size-checked BEFORE they reach the parsers /
 * JNI. minSdk-24-safe (a manual bounded copy, not InputStream.readNBytes) and never
 * buffers more than the cap.
 */
internal fun readCappedBytes(stream: InputStream, cap: Int = MAX_RUN_IMPORT_BYTES): ByteArray {
    val out = ByteArrayOutputStream()
    val buf = ByteArray(READ_BUFFER_BYTES)
    var total = 0L
    while (true) {
        val n = stream.read(buf)
        if (n < 0) break
        total += n
        if (total > cap) throw GpxImportException(ImportFailure.TooLarge)
        out.write(buf, 0, n)
    }
    return out.toByteArray()
}

/**
 * One file → the segments of each run it holds (one per TCX `<Activity>`, else one).
 * Sniffs the CONTENT, not the extension/MIME (providers lie): the FIT header magic
 * first (binary), then the XML root within the first [SNIFF_BYTES] to tell GPX from
 * TCX, so a wrong-type file is refused without decoding all of it.
 */
internal fun parseRunFileSegments(bytes: ByteArray, parseFit: (ByteArray) -> String?): List<List<List<GpxFix>>> {
    if (isFitFile(bytes)) return listOf(fitSegments(parseFit(bytes)))
    val head = bytes.decodeToString(0, minOf(bytes.size, SNIFF_BYTES))
    val tcx = head.contains("<TrainingCenterDatabase")
    if (!tcx && !head.contains("<gpx")) throw GpxImportException(ImportFailure.WrongType)
    // BOM stripped before the pull-parser: it throws "content not allowed in
    // prolog" on a BOM'd export.
    val text = bytes.decodeToString().removePrefix("\uFEFF")
    return if (tcx) parseTcxActivities(text) else listOf(parseGpx(text))
}

/** One picked/shared file, already read and parsed: its display [name] and the
 *  segments of each run it holds, or why it couldn't be read. */
internal data class ParsedRunFile(val name: String, val runs: Result<List<List<List<GpxFix>>>>)

/** One file that produced no run, and why ([detail] as in [GpxImportException]). */
internal data class RunImportFailure(val fileName: String, val reason: ImportFailure, val detail: String? = null)

/** A run to send, with the file it came from (named again if the core refuses it). */
internal data class PlannedRun(val fileName: String, val event: Event.LogRunTrack)

/** What a batch of picked/shared files turns into: the NEW runs to send, how many
 *  runs were skipped as already logged, and one failure per file that gave nothing. */
internal data class RunImportPlan(
    val runs: List<PlannedRun>,
    val duplicates: Int,
    val failures: List<RunImportFailure>,
)

/**
 * Turn a batch of parsed files into the runs to import. Pure ([build] is injected,
 * normally [importedRunEvent]) so the batch rules are JVM-testable:
 *  - each file stands alone, so one unreadable / wrong-type / route-only / cycling /
 *    implausibly fast file is reported by name and never blocks the others;
 *  - inside a multi-activity file, an activity that fails is skipped as long as
 *    another imports; when none does, the first activity's reason is the file's;
 *  - a run ending at the same second as an already-logged run ([existingObservedAt])
 *    or as an earlier run in this batch IS that run (entry ids differ per import, so
 *    the core can't pair them): it is skipped and counted, never double-counted into
 *    weekly volume or the spike baseline.
 */
internal fun planRunImport(
    files: List<ParsedRunFile>,
    existingObservedAt: Set<Long>,
    build: (List<List<GpxFix>>) -> Event.LogRunTrack,
): RunImportPlan {
    val seen = HashSet(existingObservedAt)
    val runs = ArrayList<PlannedRun>()
    val failures = ArrayList<RunImportFailure>()
    var duplicates = 0
    for (file in files) {
        val unreadable = file.runs.exceptionOrNull()
        if (unreadable != null) {
            failures += failureOf(file.name, unreadable)
            continue
        }
        val activities = file.runs.getOrThrow().filter { it.isNotEmpty() }
        if (activities.isEmpty()) {
            failures += RunImportFailure(file.name, ImportFailure.NoTrack)
            continue
        }
        var firstFailure: Throwable? = null
        val built = activities.mapNotNull { segments ->
            runCatching { build(segments) }.onFailure { if (firstFailure == null) firstFailure = it }.getOrNull()
        }
        if (built.isEmpty()) {
            failures += failureOf(file.name, firstFailure)
            continue
        }
        for (ev in built) {
            if (seen.add(ev.observedAt)) runs += PlannedRun(file.name, ev) else duplicates++
        }
    }
    return RunImportPlan(runs, duplicates, failures)
}

private fun failureOf(fileName: String, e: Throwable?): RunImportFailure =
    if (e is GpxImportException) {
        RunImportFailure(fileName, e.reason, e.detail)
    } else {
        RunImportFailure(fileName, ImportFailure.Unknown)
    }

/** The plain-language reason for one skipped file ("no GPS points", "a cycling
 *  activity, not a run"). */
internal fun importFailureReason(res: Resources, failure: RunImportFailure): String = when (failure.reason) {
    ImportFailure.ReadFailed -> res.getString(R.string.import_reason_read_failed)
    ImportFailure.TooLarge -> res.getString(R.string.import_reason_too_large)
    ImportFailure.WrongType -> res.getString(R.string.import_reason_wrong_type)
    ImportFailure.NoTrack -> res.getString(R.string.import_reason_no_track)
    ImportFailure.TooShort -> res.getString(
        R.string.import_reason_too_short,
        shortRunThresholdLabel(resolveDistanceUnit(AppSettings.distanceUnitOverride.value)),
    )
    ImportFailure.NoTimestamps -> res.getString(R.string.import_reason_no_timestamps)
    ImportFailure.Damaged -> res.getString(R.string.import_reason_damaged)
    ImportFailure.NotARun -> failure.detail
        ?.let { res.getString(R.string.import_reason_not_a_run, it) }
        ?: res.getString(R.string.import_reason_not_a_run_unknown)
    ImportFailure.ImplausiblePace -> res.getString(R.string.import_reason_implausible_pace)
    ImportFailure.Rejected -> res.getString(R.string.import_reason_rejected)
    ImportFailure.Unknown -> res.getString(R.string.import_reason_unknown)
}

/** "bike.fit (a cycling activity, not a run)": one skipped file, named, with why. */
internal fun importFailureLine(res: Resources, failure: RunImportFailure): String =
    res.getString(R.string.import_skip_item, failure.fileName, importFailureReason(res, failure))

/** A run's local date for the import result ("Jun 12, 2017"), device locale. */
internal fun importDateLabel(epochSec: Long, locale: Locale = Locale.getDefault(), zone: ZoneId = ZoneId.systemDefault()): String =
    Instant.ofEpochSecond(epochSec).atZone(zone).format(localizedDayFormatter("yMMMd", locale))

/** The import result: one line, plus every skipped file when there are too many to
 *  name inline (shown behind "Details"). */
internal data class RunImportSummary(val text: String, val details: List<String>)

/**
 * The result line for an import batch: "Imported a run from Jun 12, 2017 · 1 already
 * in history. Skipped: broken.gpx (no GPS points), bike.fit (a cycling activity, not a
 * run)". Up to [INLINE_SKIPPED_MAX] skipped files are named inline; more read
 * "Skipped 5 files" with every name and reason in [RunImportSummary.details]. A batch
 * that only hit duplicates keeps the specific duplicate message.
 */
internal fun runImportSummary(
    res: Resources,
    imported: Int,
    newestImportedAt: Long?,
    duplicates: Int,
    failures: List<RunImportFailure>,
): RunImportSummary {
    if (imported == 0 && failures.isEmpty() && duplicates > 0) {
        return RunImportSummary(res.getString(R.string.main_import_duplicate), emptyList())
    }
    val parts = ArrayList<String>(2)
    when {
        imported == 1 && newestImportedAt != null && newestImportedAt > 0L ->
            parts += res.getString(R.string.import_result_one, importDateLabel(newestImportedAt))
        imported >= 1 -> parts += res.getQuantityString(R.plurals.import_result_many, imported, imported)
        else -> parts += res.getString(R.string.import_result_none)
    }
    if (duplicates > 0) {
        parts += res.getQuantityString(R.plurals.main_import_summary_duplicates, duplicates, duplicates)
    }
    val head = parts.joinToString(" · ")
    if (failures.isEmpty()) return RunImportSummary(head, emptyList())
    val lines = failures.map { importFailureLine(res, it) }
    return if (failures.size <= INLINE_SKIPPED_MAX) {
        RunImportSummary(res.getString(R.string.import_result_with_skipped, head, lines.joinToString(", ")), emptyList())
    } else {
        val skipped = res.getQuantityString(R.plurals.import_result_skipped_count, failures.size, failures.size)
        RunImportSummary(res.getString(R.string.import_result_with_skipped_count, head, skipped), lines)
    }
}

/**
 * Run import from files: the Log sheet's multi-file picker, the History Runs import
 * button and the system "Open with" / "Share to" entry points (MainActivity) share
 * this one pipeline. Shell-side parse (content sniff → GPX/TCX/FIT), then the SAME
 * `LogRunTrack` event a live-tracked save uses, so distance/pace/splits/zone all
 * derive in-core.
 */
internal object RunImport {

    /** The file URIs an incoming VIEW / SEND / SEND_MULTIPLE intent carries, or empty
     *  for any other intent (launcher, notification deep link). */
    fun urisFrom(intent: Intent?): List<Uri> {
        if (intent == null) return emptyList()
        val direct = when (intent.action) {
            Intent.ACTION_VIEW -> listOfNotNull(intent.data)
            Intent.ACTION_SEND ->
                listOfNotNull(IntentCompat.getParcelableExtra(intent, Intent.EXTRA_STREAM, Uri::class.java))
            Intent.ACTION_SEND_MULTIPLE ->
                IntentCompat.getParcelableArrayListExtra(intent, Intent.EXTRA_STREAM, Uri::class.java)
                    ?.filterNotNull()
                    .orEmpty()
            else -> return emptyList()
        }
        if (direct.isNotEmpty()) return direct
        // Some senders only populate ClipData (which also carries the read grant).
        val clip = intent.clipData ?: return emptyList()
        return (0 until clip.itemCount).mapNotNull { clip.getItemAt(it).uri }
    }

    /**
     * Import every file in [uris] as one batch and post the result through
     * [UiMessages]. Process-scoped (outlives the sheet / Activity that started it).
     * Each file is read AND parsed one at a time, while the picker's or sharer's read
     * grant is certainly live, so only one file's bytes are ever held; the batch then
     * waits for boot hydration (a Share-to can cold-start the app) so duplicate
     * detection and the measured-HRmax lookup see the real history, and is sent as
     * ONE exclusive turn of the serial event consumer. The result counts what the core
     * ACTUALLY accepted (post-state membership), not what was enqueued.
     */
    fun importRuns(ctx: Context, uris: List<Uri>) {
        if (uris.isEmpty()) return
        val app = ctx.applicationContext
        EventDispatcher.scope.launch {
            val files = withContext(Dispatchers.IO) {
                uris.map { uri ->
                    ParsedRunFile(
                        name = displayName(app, uri),
                        runs = runCatching { parseRunFileSegments(readUri(app, uri), Core::parseFit) }
                            .onFailure { e ->
                                // A non-import exception is a shell bug or an IO error, not a
                                // bad file; leave a diagnostic trail while the batch carries on.
                                if (e !is GpxImportException) Log.w("lollop", "run import file failed", e)
                            },
                    )
                }
            }
            val hydrated = EventDispatcher.model.filterNotNull().first()
            val hrMax = hydrated.profile?.measured_hr_max
            val plan = withContext(Dispatchers.Default) {
                planRunImport(
                    files = files,
                    existingObservedAt = hydrated.runs.mapTo(HashSet()) { it.observed_at },
                    build = { segments -> importedRunEvent(segments, hrMax) },
                )
            }
            var appliedRuns: List<PlannedRun> = emptyList()
            if (plan.runs.isNotEmpty()) {
                val vm = runCatching {
                    EventDispatcher.exclusive {
                        withContext(CoreJniDispatcher) { Core.sendAll(plan.runs.map { it.event }).view }
                    }
                }.getOrNull()
                val logged = vm?.runs?.mapTo(HashSet()) { it.observed_at }.orEmpty()
                appliedRuns = plan.runs.filter { it.event.observedAt in logged }
            }
            // A run the core refused is a failed import, reported by its file's name.
            val rejected = plan.runs.filter { it !in appliedRuns }
                .map { RunImportFailure(it.fileName, ImportFailure.Rejected) }
            val newest = appliedRuns.maxByOrNull { it.event.observedAt }?.event
            val skipped = plan.failures + rejected
            val summary = runImportSummary(
                app.resources, appliedRuns.size, newest?.observedAt, plan.duplicates, skipped,
            )
            val long = skipped.isNotEmpty()
            UiMessages.post(
                when {
                    summary.details.isNotEmpty() -> UiMessage(
                        summary.text, app.getString(R.string.main_details), details = summary.details,
                        detailsTitle = app.resources.getQuantityString(
                            R.plurals.import_details_title, skipped.size, skipped.size,
                        ),
                        long = long,
                    )
                    newest != null -> UiMessage(summary.text, app.getString(R.string.import_view), long = long) {
                        HistoryFocus.request(newest.entryId, newest.observedAt)
                    }
                    else -> UiMessage(summary.text, long = long)
                },
            )
        }
    }

    private fun readUri(ctx: Context, uri: Uri): ByteArray =
        ctx.contentResolver.openInputStream(uri)?.use { readCappedBytes(it) }
            ?: throw GpxImportException(ImportFailure.ReadFailed)

    /** The file's name as the user sees it in their file picker; the URI's last
     *  path segment when the provider won't say. */
    private fun displayName(ctx: Context, uri: Uri): String {
        val fromProvider = runCatching {
            ctx.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
                if (c.moveToFirst() && !c.isNull(0)) c.getString(0) else null
            }
        }.getOrNull()
        return fromProvider?.takeIf { it.isNotBlank() }
            ?: uri.lastPathSegment?.substringAfterLast('/')?.takeIf { it.isNotBlank() }
            ?: ctx.getString(R.string.import_unnamed_file)
    }
}
