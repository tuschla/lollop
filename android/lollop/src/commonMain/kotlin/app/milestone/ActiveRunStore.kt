package app.milestone

import kotlin.concurrent.Volatile
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.double
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import kotlinx.serialization.json.put

/**
 * Crash-durable sidecar for the IN-PROGRESS run. [RunSession] lives only in
 * memory, so a process/service kill or a Recents-swipe mid-run would otherwise
 * lose the whole track, a multi-hour run gone silently. Each accepted GPS fix
 * is appended here as one NDJSON line the instant it arrives; the file's mere
 * existence marks "a run is active and was never cleanly stopped". On launch the
 * shell reads it back and resumes recording ([RunSession.restore]).
 *
 * Writes run on a single background thread ([SerialWorker]: FIFO, so fix order
 * is preserved and no file I/O touches the main / location-callback thread). A
 * kill can lose at most the last queued line: recovery tolerates a torn/short
 * final line. File access goes through the [AppFiles] seam (java.io / POSIX).
 *
 * A clean Stop & save (or an abandon) deletes the sidecar so no stale run is
 * ever offered for recovery.
 *
 * Started by [AppRuntime.start] (the file in the app's files); every other call
 * needs that start, or [open] with a path.
 */
internal object ActiveRunStore : AppFeature {
    private const val TAG = "ActiveRunStore"

    // First line marks an active run even before the first fix arrives. Presence
    // of the file is the real "run in progress" signal; this makes it explicit.
    private const val HEADER = "{\"run\":\"active\"}"

    // [flush] gives up after this: it runs on the main thread (onTaskRemoved), where
    // a disk hiccup stalling the io thread must not become an ANR.
    private const val FLUSH_TIMEOUT_MS = 2_000L

    private class Store : AutoCloseable {
        val io = SerialWorker()

        @Volatile var file: AppPath? = null

        override fun close() = io.close()
    }

    private val store by ProcessLocal { Store() }

    private val Store.path: AppPath
        get() = checkNotNull(file) { "ActiveRunStore used before AppRuntime.start" }

    override fun start(ctx: PlatformContext) = open(ctx.appFile(AppFileNames.ACTIVE_RUN))

    /** Keep the sidecar at [path] (the app's files: [start]; the common tests: their own). */
    internal fun open(path: AppPath) {
        store.file = path
    }

    /** Start a fresh run's sidecar: truncate and write the active marker. */
    fun begin() {
        val s = store
        val f = s.path
        s.io.post {
            runCatching { AppFiles.writeText(f, HEADER + "\n") }
                .onFailure { PlatformLog.w(TAG, "could not start the run's sidecar", it) }
        }
    }

    /** Append one fix [RunSession.add] accepted. [segmentStart]: it opened a new
     *  recording segment there (the first fix after a Pause/Resume, or after a long
     *  fix gap; [RunSession.isSegmentStart]), marked `"seg":true` so a recovered run
     *  breaks at exactly the live run's boundaries: a short pause's walk is then
     *  neither distance nor moving time after a crash either. Off the main thread; a
     *  failure is logged, never thrown, so a disk hiccup never crashes the location
     *  callback. */
    fun append(p: GpsPoint, segmentStart: Boolean) {
        val s = store
        val f = s.path
        s.io.post {
            runCatching {
                val line = buildJsonObject {
                    put("lat", p.lat)
                    put("lon", p.lon)
                    put("observed_at", p.observedAt)
                    put("accuracy_m", p.accuracyM)
                    if (segmentStart) put("seg", true)
                }.toString() + "\n"
                // Cut a torn final line (a kill mid-append left bytes after the last
                // '\n') first, so this fix starts its own line instead of gluing onto
                // the partial one. A resumed run keeps appending to the same file
                // (Resume restarts recording without [begin]), so without this the
                // first post-resume fix, and its `"seg":true` splice mark, would be
                // unparseable on the next recovery. The torn bytes were never a
                // recoverable fix ([recover] skips them), so nothing is lost. One
                // one-byte read per append when the tail is whole.
                AppFiles.dropTornTail(f)
                AppFiles.append(f, line)
            }.onFailure { PlatformLog.w(TAG, "could not append a fix to the run's sidecar", it) }
        }
    }

    /** True if an interrupted run's sidecar is on disk (never cleanly stopped). */
    fun hasActiveRun(): Boolean = AppFiles.exists(store.path)

    /**
     * Read every recorded fix back, tolerating a torn/partial last line (a line
     * half-written when the process died just fails to parse and is skipped),
     * with the indices of the fixes that opened a segment. Blocking file read:
     * call off the main thread.
     */
    fun recover(): RecoveredTrack {
        val f = store.path
        if (!AppFiles.exists(f)) return RecoveredTrack(emptyList(), emptyList())
        val out = ArrayList<GpsPoint>()
        val starts = ArrayList<Int>()
        var unreadable = 0
        runCatching {
            AppFiles.forEachLine(f) { line ->
                val t = line.trim()
                if (t.isEmpty() || t.startsWith("{\"run\"")) return@forEachLine
                runCatching {
                    val o = coreJson.parseToJsonElement(t).jsonObject
                    val p = GpsPoint(
                        lat = o["lat"]!!.jsonPrimitive.double,
                        lon = o["lon"]!!.jsonPrimitive.double,
                        observedAt = o["observed_at"]!!.jsonPrimitive.long,
                        accuracyM = o["accuracy_m"]!!.jsonPrimitive.double,
                    )
                    if (o["seg"]?.jsonPrimitive?.booleanOrNull == true) starts.add(out.size)
                    out.add(p)
                }.onFailure { unreadable++ }
            }
        }.onFailure { PlatformLog.w(TAG, "could not read the run's sidecar; recovering ${out.size} fixes", it) }
        if (unreadable > 0) PlatformLog.w(TAG, "skipped $unreadable unreadable sidecar lines")
        // Thin to the save cap so RunSession's restored track holds what a save would
        // send (this list still peaks at every recorded fix while reading).
        return thinForRecovery(out, segmentStarts = starts)
    }

    /** Clean stop / abandon: delete the sidecar so no recovery is offered. Queued
     *  fire-and-forget on the io thread, fine for the DISCARD/abandon paths, where
     *  nothing was persisted and ordering vs another event doesn't matter. */
    fun clear() {
        val s = store
        val f = s.path
        s.io.post { delete(f) }
    }

    /**
     * Ordered clear for the POST-SAVE path: submit the delete onto the same
     * single-thread FIFO io executor as the fix appends and BLOCK until it (and
     * every append queued before it) has run. Call OFF the main thread (it runs
     * inside logCaptured's IO context) right after the save's event-log append
     * returns, so process death in the gap between "run is in the append-only log"
     * and "sidecar deleted" can't leave the just-saved run's sidecar on disk to be
     * resurrected by the recovery prompt (a duplicate saved run). Routing the
     * delete through the io thread, rather than deleting on the caller, also
     * orders it AFTER any still-queued fix appends, so a late append can't recreate
     * the file after a direct delete.
     */
    fun clearSync() {
        val s = store
        val f = s.path
        runCatching { s.io.runAndWait { delete(f) } }
            .onFailure { PlatformLog.w(TAG, "the run's sidecar delete did not complete", it) }
    }

    private fun delete(f: AppPath) {
        runCatching { AppFiles.delete(f) }.onFailure { PlatformLog.w(TAG, "could not delete the run's sidecar", it) }
    }

    /** Best-effort: block until queued writes have flushed (onTaskRemoved), so a
     *  subsequent OS kill loses nothing already accepted. Bounded by
     *  [FLUSH_TIMEOUT_MS]; on timeout it gives up rather than block the main thread
     *  (at most the last few fixes are unflushed, and the sidecar's already-written
     *  lines still allow recovery). */
    fun flush() {
        runCatching { store.io.drain(timeoutMs = FLUSH_TIMEOUT_MS) }
            .onFailure { PlatformLog.w(TAG, "the run's sidecar writes did not flush", it) }
    }

    /**
     * Thin a recovered track to ~[maxPoints] fixes with the save's rule ([thinTrack],
     * the core's): a uniform stride plus the last fix and every fix that opened a
     * segment ([segmentStarts], remapped into the result). Dropping fixes only widens
     * gaps; a widened gap opens a new segment only once stride × fix cadence exceeds
     * [CoreConstants.MAX_FIX_GAP_SEC].
     * No-op at or below the cap.
     */
    internal fun thinForRecovery(
        pts: List<GpsPoint>,
        maxPoints: Int = CoreConstants.TRACK_DECIMATION_CAP,
        segmentStarts: Collection<Int> = emptyList(),
    ): RecoveredTrack {
        val (thinned, starts) = thinTrack(pts, maxPoints, segmentStarts)
        return RecoveredTrack(thinned, starts)
    }
}
