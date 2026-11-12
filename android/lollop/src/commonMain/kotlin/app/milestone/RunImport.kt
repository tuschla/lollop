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

/** Above this many skipped files the result names none inline; "Details" lists them. */
private const val INLINE_SKIPPED_MAX = 2

/** Why one imported file produced no run: the core's [ImportRefusal]s and the shell's
 *  own read and save failures. The shell maps each to plain-language copy beside the
 *  file's name. */
internal enum class ImportFailure {
    ReadFailed, TooLarge, WrongType, NoTrack, TooShort, NoTimestamps, Damaged, NotARun, ImplausiblePace, Rejected, Unknown,
}

/** An import that produced no event, with its [reason]. [detail] carries the one
 *  variable part a reason needs (the activity's sport for [ImportFailure.NotARun]). */
internal class ImportException(val reason: ImportFailure, val detail: String? = null, cause: Throwable? = null) :
    Exception(detail?.let { "$reason: $it" } ?: reason.name, cause)

private fun ImportRefusal.failure(): ImportFailure = when (this) {
    ImportRefusal.TooLarge -> ImportFailure.TooLarge
    ImportRefusal.WrongType -> ImportFailure.WrongType
    ImportRefusal.NoTrack -> ImportFailure.NoTrack
    ImportRefusal.TooShort -> ImportFailure.TooShort
    ImportRefusal.NoTimestamps -> ImportFailure.NoTimestamps
    ImportRefusal.Damaged -> ImportFailure.Damaged
    ImportRefusal.NotARun -> ImportFailure.NotARun
    ImportRefusal.ImplausiblePace -> ImportFailure.ImplausiblePace
}

/**
 * One file → the runs it holds, by the core's import ([DurableCore.parseRunFile]
 * through [parse]): the format sniffed from the content (FIT, GPX or TCX), every run
 * gated and decimated there. A file that holds none throws its [ImportException]
 * (the sport a [ImportFailure.NotARun] file names as its detail); a core that can't
 * answer reads as a damaged file.
 */
internal fun parseRunFile(bytes: ByteArray, parse: (ByteArray) -> ImportVerdict?): List<ImportedRun> {
    val verdict = parse(bytes) ?: throw ImportException(ImportFailure.Damaged)
    verdict.refusal?.let { throw ImportException(it.failure(), verdict.sport?.takeIf { s -> s.isNotBlank() }) }
    return verdict.runs.ifEmpty { throw ImportException(ImportFailure.NoTrack) }
}

/**
 * The [Event.LogRunTrack] for an imported [run]: the core's gated, decimated fixes,
 * sent as a live-tracked save sends its own, so distance, pace, splits and zone all
 * derive in-core, stamped when the run happened. The file's average heart rate goes
 * as bpm with `hr_pct_max` 0.0, as the run editor sends a bpm entry: the core
 * converts it against the profile in force on the run's date. Every run gets its
 * own entry id, so the runs of one multi-activity file, built in a tight loop, never
 * share one.
 */
internal fun importedRunEvent(run: ImportedRun): Event.LogRunTrack = Event.LogRunTrack(
    points = run.points,
    hrPctMax = 0.0,
    entryId = EntryIds.next(),
    observedAt = run.observed_at,
    workoutType = null,
    segmentStarts = run.segment_starts,
    avgHrBpm = run.avg_hr_bpm,
)

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
 * Turn a batch of parsed files into the runs to import. [build] is injected
 * (normally [importedRunEvent]) so the batch rules are JVM-testable:
 *  - each file stands alone, so one unreadable / wrong-type / route-only / cycling /
 *    implausibly fast file is reported by name and never blocks the others;
 *  - a run logged at the same second as an already-logged run ([existingObservedAt])
 *    or as an earlier run in this batch IS that run (entry ids differ per import, so
 *    the core can't pair them): it is skipped and counted, never double-counted into
 *    weekly volume or the spike baseline. A run's own GPX export names its logged-at
 *    stamp, so it matches even when its track ends earlier (a standing-still finish
 *    dropped by QC, a trimmed end).
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
        val fileRuns = file.runs.getOrElse { e ->
            failures += if (e is ImportException) {
                RunImportFailure(file.name, e.reason, e.detail)
            } else {
                RunImportFailure(file.name, ImportFailure.Unknown)
            }
            continue
        }
        for (run in fileRuns) {
            val ev = build(run)
            if (seen.add(ev.observedAt)) runs += PlannedRun(file.name, ev) else duplicates++
        }
    }
    return RunImportPlan(runs, duplicates, failures)
}

/** The import size cap in MiB, as the too-large reason states it. */
internal fun importCapMbLabel(): String = trimDecimal(CoreConstants.MAX_RUN_IMPORT_BYTES / (1024.0 * 1024.0))

/** The plain-language reason for one skipped file ("no GPS points", "a cycling
 *  activity, not a run"). */
internal fun importFailureReason(res: PlatformResources, failure: RunImportFailure): String = when (failure.reason) {
    ImportFailure.ReadFailed -> res.getString(Res.string.import_reason_read_failed)
    ImportFailure.TooLarge -> res.getString(Res.string.import_reason_too_large, importCapMbLabel())
    ImportFailure.WrongType -> res.getString(Res.string.import_reason_wrong_type)
    ImportFailure.NoTrack -> res.getString(Res.string.import_reason_no_track)
    ImportFailure.TooShort -> res.getString(
        Res.string.import_reason_too_short,
        shortRunThresholdLabel(EventDispatcher.model.value.distanceUnit()),
        shortRunMinutesLabel(),
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
    fun displayName(): String

    /** The file's bytes: [ImportFailure.TooLarge] above [CoreConstants.MAX_RUN_IMPORT_BYTES]
     *  (checked before the bytes are held), [ImportFailure.ReadFailed] when unreadable. */
    fun readBytes(): ByteArray
}

/**
 * Run import from files: the Log sheet's multi-file picker, the History Runs import
 * button and the system "Open with" / "Share to" entry points share this one
 * pipeline. The core reads each file ([parseRunFile]: content sniff → GPX/TCX/FIT,
 * the run gates), then the shell sends the SAME `LogRunTrack` event a live-tracked
 * save uses, so distance/pace/splits/zone all derive in-core.
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
    AppProcess.scope.launch {
        val parsed = withContext(Dispatchers.IO) {
            files.map { file ->
                ParsedRunFile(
                    name = file.displayName(),
                    runs = runCatching { parseRunFile(file.readBytes(), AppCore.engine::parseRunFile) }
                        .onFailure { e ->
                            // A non-import exception is a shell bug or an IO error, not a
                            // bad file; leave a diagnostic trail while the batch carries on.
                            if (e !is ImportException) PlatformLog.w("lollop", "run import file failed", e)
                        },
                )
            }
        }
        val hydrated = EventDispatcher.model.filterNotNull().first()
        val plan = withContext(Dispatchers.Default) {
            planRunImport(
                files = parsed,
                existingObservedAt = hydrated.runs.mapTo(HashSet()) { it.observed_at },
                build = ::importedRunEvent,
            )
        }
        var appliedRuns: List<PlannedRun> = emptyList()
        if (plan.runs.isNotEmpty()) {
            val vm = runCatching {
                EventDispatcher.exclusive {
                    withContext(CoreDispatcher) { AppCore.engine.sendAll(plan.runs.map { it.event }).view }
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
