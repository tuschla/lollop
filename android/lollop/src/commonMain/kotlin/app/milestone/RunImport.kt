package app.milestone

import app.milestone.resources.Res
import app.milestone.resources.import_details_title
import app.milestone.resources.import_reason_damaged
import app.milestone.resources.import_reason_implausible_pace
import app.milestone.resources.import_reason_no_timestamps
import app.milestone.resources.import_reason_no_track
import app.milestone.resources.import_reason_not_a_run
import app.milestone.resources.import_reason_not_a_run_unknown
import app.milestone.resources.import_reason_read_failed
import app.milestone.resources.import_reason_rejected
import app.milestone.resources.import_reason_too_large
import app.milestone.resources.import_reason_too_short
import app.milestone.resources.import_reason_unknown
import app.milestone.resources.import_reason_wrong_type
import app.milestone.resources.import_result_many
import app.milestone.resources.import_result_none
import app.milestone.resources.import_result_one
import app.milestone.resources.import_result_skipped_count
import app.milestone.resources.import_result_with_skipped
import app.milestone.resources.import_result_with_skipped_count
import app.milestone.resources.import_skip_item
import app.milestone.resources.import_view
import app.milestone.resources.main_details
import app.milestone.resources.main_import_duplicate
import app.milestone.resources.main_import_summary_duplicates
import kotlin.time.Instant
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.IO
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.datetime.TimeZone
import kotlinx.datetime.toLocalDateTime

/** Read cap for any import file: real GPX/TCX exports are well under it, and it keeps a
 *  wrong-type or endless source from exhausting memory. FIT has its own smaller cap. */
internal const val MAX_RUN_IMPORT_BYTES = 32 * 1024 * 1024 // 32 MB

/** FIT cap. [DurableCore.parseFitFile] returns one JSON object per FIT record, up to ~6× the
 *  file's bytes, and [fitSegments] holds that as a JSON tree, so the 32 MB read cap
 *  would admit ~190 M chars. 8 MB still fits a day-long 1 Hz recording (86 400
 *  records under 90 bytes each). */
internal const val MAX_FIT_IMPORT_BYTES = 8 * 1024 * 1024 // 8 MB

/** How much of a file the XML sniff decodes to find its root element. */
private const val SNIFF_BYTES = 4 * 1024

/** Above this many skipped files the result names none inline; "Details" lists them. */
private const val INLINE_SKIPPED_MAX = 2

/**
 * One file → the runs it holds (one per GPX `<trk>` or TCX `<Activity>`, else one).
 * Sniffs the CONTENT, not the extension/MIME (providers lie): the FIT header magic
 * first (binary, refused over [MAX_FIT_IMPORT_BYTES]), then the XML root within the
 * first [SNIFF_BYTES] to tell GPX from TCX, so a wrong-type file is refused without
 * decoding all of it. XML that breaks off or is malformed past the root (a cut-off
 * download) is [ImportFailure.Damaged], and so is a document that declares a DTD
 * ([declaresDtd]).
 */
internal fun parseRunFileSegments(bytes: ByteArray, parseFit: (ByteArray) -> String?): List<ImportedRun> =
    parseRunFileSegments(bytes, parseFit, ::importXmlReader)

/** [parseRunFileSegments] with the XML reader [xml] opens (the host tests compare the
 *  platform's with [XmlUtilImportReader]). */
internal fun parseRunFileSegments(
    bytes: ByteArray,
    parseFit: (ByteArray) -> String?,
    xml: (String) -> ImportXmlReader,
): List<ImportedRun> {
    if (isFitFile(bytes)) {
        if (bytes.size > MAX_FIT_IMPORT_BYTES) throw ImportException(ImportFailure.TooLarge)
        return listOf(ImportedRun(fitSegments(parseFit(bytes))))
    }
    // XML 1.0 §4.3.3: a UTF-16 entity starts with its byte order mark; anything
    // else is read as UTF-8 (the BOM, if any, skipped: the pull-parser throws
    // "content not allowed in prolog" on it).
    val (charset, bom) = when {
        bytes.startsWith(0xFE, 0xFF) -> ImportCharset.Utf16Be to 2
        bytes.startsWith(0xFF, 0xFE) -> ImportCharset.Utf16Le to 2
        bytes.startsWith(0xEF, 0xBB, 0xBF) -> ImportCharset.Utf8 to 3
        else -> ImportCharset.Utf8 to 0
    }
    val head = decodeImportText(bytes, bom, minOf(bytes.size - bom, SNIFF_BYTES), charset)
    val tcx = head.contains("<TrainingCenterDatabase")
    if (!tcx && !head.contains("<gpx")) throw ImportException(ImportFailure.WrongType)
    val text = decodeImportText(bytes, bom, bytes.size - bom, charset)
    if (declaresDtd(text)) throw ImportException(ImportFailure.Damaged)
    return try {
        if (tcx) parseTcxActivities(text, xml) else parseGpxRuns(text, xml)
    } catch (e: ImportXmlException) {
        throw ImportException(ImportFailure.Damaged, cause = e)
    }
}

/** The text encodings an import file is read in. */
internal enum class ImportCharset { Utf8, Utf16Be, Utf16Le }

/** [length] bytes of [bytes] from [offset] as [charset] text, malformed input
 *  replaced by U+FFFD. Android: `String(bytes, offset, length, charset)`; iOS:
 *  Kotlin's UTF-8 decoder and [decodeUtf16]. */
internal expect fun decodeImportText(bytes: ByteArray, offset: Int, length: Int, charset: ImportCharset): String

/**
 * UTF-16 ([bigEndian] or little-endian) as the JDK's decoder reads it with
 * replacement: a byte order mark stays a character, an unpaired surrogate (a high one
 * swallows the unit after it) and trailing bytes too short for a character each read
 * as one U+FFFD.
 */
internal fun decodeUtf16(bytes: ByteArray, offset: Int, length: Int, bigEndian: Boolean): String {
    val end = offset + length
    val out = StringBuilder(length / 2)
    fun unit(at: Int): Char {
        val a = bytes[at].toInt() and 0xFF
        val b = bytes[at + 1].toInt() and 0xFF
        return (if (bigEndian) (a shl 8) or b else (b shl 8) or a).toChar()
    }
    var i = offset
    while (end - i >= 2) {
        val c = unit(i)
        when {
            c.isLowSurrogate() -> {
                out.append('\uFFFD')
                i += 2
            }
            c.isHighSurrogate() -> {
                if (end - i < 4) break
                val low = unit(i + 2)
                if (low.isLowSurrogate()) out.append(c).append(low) else out.append('\uFFFD')
                i += 4
            }
            else -> {
                out.append(c)
                i += 2
            }
        }
    }
    if (i < end) out.append('\uFFFD')
    return out.toString()
}

private fun ByteArray.startsWith(vararg prefix: Int): Boolean =
    size >= prefix.size && prefix.indices.all { this[it] == prefix[it].toByte() }

/**
 * Whether [text]'s prolog (what precedes the root element: the XML declaration,
 * comments, processing instructions) holds a `<!DOCTYPE`. No GPX or TCX exporter
 * writes one, and the platform pull parser expands a DTD's internal entities
 * without any limit: ten nested entities in a 700-byte file make 10^10
 * characters (billion laughs, an OutOfMemoryError). So such a file is refused
 * before it reaches the parser (OWASP: disable DTDs entirely); external entities
 * go with it. A `<!DOCTYPE` anywhere else is no declaration (XML 1.0 §2.8 allows
 * it only in the prolog), and the parser refuses it as malformed.
 */
internal fun declaresDtd(text: String): Boolean {
    var i = 0
    while (i < text.length) {
        i = when {
            text[i].isWhitespace() -> i + 1
            text.startsWith("<?", i) -> text.indexOf("?>", i + 2).takeIf { it >= 0 }?.plus(2) ?: return false
            text.startsWith("<!--", i) -> text.indexOf("-->", i + 4).takeIf { it >= 0 }?.plus(3) ?: return false
            else -> return text.startsWith("<!DOCTYPE", i)
        }
    }
    return false
}

/** One picked/shared file, already read and parsed: its display [name] and the
 *  runs it holds, or why it couldn't be read. */
internal data class ParsedRunFile(val name: String, val runs: Result<List<ImportedRun>>)

/** One file that produced no run, and why ([detail] as in [ImportException]). */
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
 *  - a run logged at the same second as an already-logged run ([existingObservedAt])
 *    or as an earlier run in this batch IS that run (entry ids differ per import, so
 *    the core can't pair them): it is skipped and counted, never double-counted into
 *    weekly volume or the spike baseline. A run's own GPX export names its logged-at
 *    stamp ([ImportedRun.loggedAtSec]), so it matches even when its track ends
 *    earlier (a standing-still finish dropped by QC, a trimmed end).
 */
internal fun planRunImport(
    files: List<ParsedRunFile>,
    existingObservedAt: Set<Long>,
    build: (ImportedRun) -> Event.LogRunTrack,
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
        val activities = file.runs.getOrThrow().filter { it.segments.isNotEmpty() }
        if (activities.isEmpty()) {
            failures += RunImportFailure(file.name, ImportFailure.NoTrack)
            continue
        }
        var firstFailure: Throwable? = null
        val built = activities.mapNotNull { run ->
            runCatching { build(run) }.onFailure { if (firstFailure == null) firstFailure = it }.getOrNull()
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
    if (e is ImportException) {
        RunImportFailure(fileName, e.reason, e.detail)
    } else {
        RunImportFailure(fileName, ImportFailure.Unknown)
    }

/** The plain-language reason for one skipped file ("no GPS points", "a cycling
 *  activity, not a run"). */
internal fun importFailureReason(res: PlatformResources, failure: RunImportFailure): String = when (failure.reason) {
    ImportFailure.ReadFailed -> res.getString(Res.string.import_reason_read_failed)
    ImportFailure.TooLarge -> res.getString(Res.string.import_reason_too_large)
    ImportFailure.WrongType -> res.getString(Res.string.import_reason_wrong_type)
    ImportFailure.NoTrack -> res.getString(Res.string.import_reason_no_track)
    ImportFailure.TooShort -> res.getString(
        Res.string.import_reason_too_short,
        shortRunThresholdLabel(resolveDistanceUnit(AppSettings.distanceUnitOverride.value)),
    )
    ImportFailure.NoTimestamps -> res.getString(Res.string.import_reason_no_timestamps)
    ImportFailure.Damaged -> res.getString(Res.string.import_reason_damaged)
    ImportFailure.NotARun -> failure.detail
        ?.let { res.getString(Res.string.import_reason_not_a_run, it) }
        ?: res.getString(Res.string.import_reason_not_a_run_unknown)
    ImportFailure.ImplausiblePace -> res.getString(Res.string.import_reason_implausible_pace)
    ImportFailure.Rejected -> res.getString(Res.string.import_reason_rejected)
    ImportFailure.Unknown -> res.getString(Res.string.import_reason_unknown)
}

/** "bike.fit (a cycling activity, not a run)": one skipped file, named, with why. */
internal fun importFailureLine(res: PlatformResources, failure: RunImportFailure): String =
    res.getString(Res.string.import_skip_item, failure.fileName, importFailureReason(res, failure))

/** A run's local date for the import result ("Jun 12, 2017"), in the app's language. */
internal fun importDateLabel(epochSec: Long, locale: PlatformLocale = appLocale(), zone: TimeZone = TimeZone.currentSystemDefault()): String =
    localizedDayFormatter("yMMMd", locale).format(Instant.fromEpochSeconds(epochSec).toLocalDateTime(zone))

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
    res: PlatformResources,
    imported: Int,
    newestImportedAt: Long?,
    duplicates: Int,
    failures: List<RunImportFailure>,
): RunImportSummary {
    if (imported == 0 && failures.isEmpty() && duplicates > 0) {
        return RunImportSummary(res.getString(Res.string.main_import_duplicate), emptyList())
    }
    val parts = ArrayList<String>(2)
    when {
        imported == 1 && newestImportedAt != null && newestImportedAt > 0L ->
            parts += res.getString(Res.string.import_result_one, importDateLabel(newestImportedAt))
        imported >= 1 -> parts += res.getQuantityString(Res.plurals.import_result_many, imported, imported)
        else -> parts += res.getString(Res.string.import_result_none)
    }
    if (duplicates > 0) {
        parts += res.getQuantityString(Res.plurals.main_import_summary_duplicates, duplicates, duplicates)
    }
    val head = parts.joinToString(" · ")
    if (failures.isEmpty()) return RunImportSummary(head, emptyList())
    val lines = failures.map { importFailureLine(res, it) }
    return if (failures.size <= INLINE_SKIPPED_MAX) {
        RunImportSummary(res.getString(Res.string.import_result_with_skipped, head, lines.joinToString(", ")), emptyList())
    } else {
        val skipped = res.getQuantityString(Res.plurals.import_result_skipped_count, failures.size, failures.size)
        RunImportSummary(res.getString(Res.string.import_result_with_skipped_count, head, skipped), lines)
    }
}

/** One picked, shared or opened file of an import batch, read when the batch gets
 *  to it (both calls run on the IO dispatcher). */
internal interface RunImportFile {
    /** The file's name as the user sees it. */
    fun displayName(): String

    /** The file's bytes: [ImportFailure.TooLarge] above [MAX_RUN_IMPORT_BYTES] (checked
     *  before the bytes reach the parsers), [ImportFailure.ReadFailed] when unreadable. */
    fun readBytes(): ByteArray
}

/**
 * Run import from files: the Log sheet's multi-file picker, the History Runs import
 * button and the system "Open with" / "Share to" entry points share this one
 * pipeline. Shell-side parse (content sniff → GPX/TCX/FIT), then the SAME
 * `LogRunTrack` event a live-tracked save uses, so distance/pace/splits/zone all
 * derive in-core.
 *
 * Imports every file in [files] as one batch and posts the result through
 * [UiMessages] in [resources] (read once the batch is done). Process-scoped
 * (outlives the sheet / screen that started it). Each file is read AND parsed one at
 * a time, while a picker's or sharer's read grant is certainly live, so only one
 * file's bytes are ever held; the batch then waits for boot hydration (a Share-to can
 * cold-start the app) so duplicate detection and the measured-HRmax lookup see the
 * real history, and is sent as ONE exclusive turn of the serial event consumer. The
 * result counts what the core ACTUALLY accepted (post-state membership), not what was
 * enqueued.
 */
internal fun launchRunImport(files: List<RunImportFile>, resources: () -> PlatformResources) {
    if (files.isEmpty()) return
    AppRuntime.scope.launch {
        val parsed = withContext(Dispatchers.IO) {
            files.map { file ->
                ParsedRunFile(
                    name = file.displayName(),
                    runs = runCatching { parseRunFileSegments(file.readBytes(), AppRuntime.engine::parseFitFile) }
                        .onFailure { e ->
                            // A non-import exception is a shell bug or an IO error, not a
                            // bad file; leave a diagnostic trail while the batch carries on.
                            if (e !is ImportException) PlatformLog.w("lollop", "run import file failed", e)
                        },
                )
            }
        }
        val hydrated = EventDispatcher.model.filterNotNull().first()
        val hrMax = hydrated.profile?.measured_hr_max
        val plan = withContext(Dispatchers.Default) {
            planRunImport(
                files = parsed,
                existingObservedAt = hydrated.runs.mapTo(HashSet()) { it.observed_at },
                build = { run -> importedRunEvent(run, hrMax) },
            )
        }
        var appliedRuns: List<PlannedRun> = emptyList()
        if (plan.runs.isNotEmpty()) {
            val vm = runCatching {
                EventDispatcher.exclusive {
                    withContext(CoreDispatcher) { AppRuntime.engine.sendAll(plan.runs.map { it.event }).view }
                }
            }.onFailure {
                if (it is CancellationException) throw it
                PlatformLog.w("lollop", "run import batch not applied", it)
            }.getOrNull()
            val logged = vm?.runs?.mapTo(HashSet()) { it.observed_at }.orEmpty()
            appliedRuns = plan.runs.filter { it.event.observedAt in logged }
        }
        // A run the core refused is a failed import, reported by its file's name.
        val rejected = plan.runs.filter { it !in appliedRuns }
            .map { RunImportFailure(it.fileName, ImportFailure.Rejected) }
        val newest = appliedRuns.maxByOrNull { it.event.observedAt }?.event
        val skipped = plan.failures + rejected
        val res = resources()
        val summary = runImportSummary(
            res, appliedRuns.size, newest?.observedAt, plan.duplicates, skipped,
        )
        val long = skipped.isNotEmpty()
        UiMessages.post(
            when {
                summary.details.isNotEmpty() -> UiMessage(
                    summary.text, res.getString(Res.string.main_details), details = summary.details,
                    detailsTitle = res.getQuantityString(
                        Res.plurals.import_details_title, skipped.size, skipped.size,
                    ),
                    long = long,
                )
                newest != null -> UiMessage(summary.text, res.getString(Res.string.import_view), long = long) {
                    HistoryFocus.request(newest.entryId, newest.observedAt)
                }
                else -> UiMessage(summary.text, long = long)
            },
        )
    }
}
