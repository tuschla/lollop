package app.milestone

import android.content.Context
import java.io.File
import java.io.RandomAccessFile
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import kotlinx.serialization.json.Json
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
 * Writes run on a single background thread (FIFO, so fix order is preserved and
 * no file I/O touches the main / location-callback thread). A kill can lose at
 * most the last queued line: recovery tolerates a torn/short final line.
 *
 * A clean Stop & save (or an abandon) deletes the sidecar so no stale run is
 * ever offered for recovery.
 */
object ActiveRunStore {
    private const val FILE = "active-run.ndjson"
    // First line marks an active run even before the first fix arrives. Presence
    // of the file is the real "run in progress" signal; this makes it explicit.
    private const val HEADER = "{\"run\":\"active\"}"

    private val io = Executors.newSingleThreadExecutor()
    private val json = Json { ignoreUnknownKeys = true }

    @Volatile private var file: File? = null

    fun init(ctx: Context) {
        if (file == null) file = File(ctx.applicationContext.filesDir, FILE)
    }

    /** Start a fresh run's sidecar: truncate and write the active marker. */
    fun begin(ctx: Context) {
        init(ctx)
        val f = file ?: return
        io.execute { runCatching { f.writeText(HEADER + "\n") } }
    }

    /** Append one accepted fix, called right after [RunSession.add] accepted [p].
     *  A fix that opened a new recording segment there (the first fix after a
     *  Pause/Resume, or after a long fix gap) is marked `"seg":true`, so a
     *  recovered run breaks at exactly the live run's boundaries: a short pause's
     *  walk is then neither distance nor moving time after a crash either. Off the
     *  main thread; failures are swallowed so a disk hiccup never crashes the
     *  location callback. */
    fun append(p: GpsPoint) {
        val f = file ?: return
        val opensSegment = RunSession.isSegmentStart(p)
        io.execute {
            runCatching {
                val line = buildJsonObject {
                    put("lat", p.lat)
                    put("lon", p.lon)
                    put("observed_at", p.observedAt)
                    put("accuracy_m", p.accuracyM)
                    if (opensSegment) put("seg", true)
                }.toString() + "\n"
                RandomAccessFile(f, "rw").use { raf ->
                    dropTornTail(raf)
                    raf.seek(raf.length())
                    raf.write(line.toByteArray())
                }
            }
        }
    }

    /**
     * Cut a torn final line (a kill mid-append left bytes after the last '\n') off
     * the sidecar, so the next fix starts its own line instead of gluing onto the
     * partial one. A resumed run keeps appending to the same file (Resume restarts
     * the service without [begin]), so without this the first post-resume fix, and
     * its `"seg":true` splice mark, would be unparseable on the next recovery. The
     * torn bytes were never a recoverable fix ([recover] skips them), so nothing is
     * lost. One one-byte read per append when the tail is whole.
     */
    private fun dropTornTail(raf: RandomAccessFile) {
        val len = raf.length()
        if (len == 0L) return
        raf.seek(len - 1)
        if (raf.read() == '\n'.code) return
        val chunk = ByteArray(4096)
        var end = len
        while (end > 0) {
            val start = maxOf(0L, end - chunk.size)
            val n = (end - start).toInt()
            raf.seek(start)
            raf.readFully(chunk, 0, n)
            for (i in n - 1 downTo 0) {
                if (chunk[i] == '\n'.code.toByte()) {
                    raf.setLength(start + i + 1)
                    return
                }
            }
            end = start
        }
        raf.setLength(0)
    }

    /** True if an interrupted run's sidecar is on disk (never cleanly stopped). */
    fun hasActiveRun(ctx: Context): Boolean {
        init(ctx)
        return file?.exists() == true
    }

    /**
     * Read every recorded fix back, tolerating a torn/partial last line (a line
     * half-written when the process died just fails to parse and is skipped),
     * with the indices of the fixes that opened a segment. Blocking file read:
     * call off the main thread.
     */
    fun recover(ctx: Context): RecoveredTrack {
        init(ctx)
        val f = file ?: return RecoveredTrack(emptyList(), emptyList())
        if (!f.exists()) return RecoveredTrack(emptyList(), emptyList())
        val out = ArrayList<GpsPoint>()
        val starts = ArrayList<Int>()
        runCatching {
            f.forEachLine { line ->
                val t = line.trim()
                if (t.isEmpty() || t.startsWith("{\"run\"")) return@forEachLine
                runCatching {
                    val o = json.parseToJsonElement(t).jsonObject
                    val p = GpsPoint(
                        lat = o["lat"]!!.jsonPrimitive.double,
                        lon = o["lon"]!!.jsonPrimitive.double,
                        observedAt = o["observed_at"]!!.jsonPrimitive.long,
                        accuracyM = o["accuracy_m"]!!.jsonPrimitive.double,
                    )
                    if (o["seg"]?.jsonPrimitive?.booleanOrNull == true) starts.add(out.size)
                    out.add(p)
                }
            }
        }
        // Thin an unbounded-length recovery back to the save-time cap so a very long
        // interrupted run doesn't reload tens of thousands of fixes into memory (and
        // the subsequent save would decimate to this same cap anyway).
        return thinForRecovery(out, segmentStarts = starts)
    }

    /** Clean stop / abandon: delete the sidecar so no recovery is offered. Queued
     *  fire-and-forget on the io thread, fine for the DISCARD/abandon paths, where
     *  nothing was persisted and ordering vs another event doesn't matter. */
    fun clear() {
        val f = file ?: return
        io.execute { runCatching { f.delete() } }
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
        val f = file ?: return
        runCatching { io.submit { runCatching { f.delete() } }.get() }
    }

    /** Best-effort: block until queued writes have flushed (onTaskRemoved), so a
     *  subsequent OS kill loses nothing already accepted. BOUNDED wait, onTaskRemoved
     *  runs on the main thread, so an unbounded get() risks an ANR if a disk hiccup
     *  stalls the io thread. 2 s is ample for the small queued appends; on timeout we
     *  give up gracefully rather than block the main thread (at most the last few
     *  fixes are unflushed, and the sidecar's already-written lines still allow
     *  recovery). */
    fun flush() {
        runCatching { io.submit { }.get(2, TimeUnit.SECONDS) }
    }

    /**
     * Thin a recovered track to at most ~[maxPoints] fixes. The sidecar appends
     * EVERY raw fix for the whole run, the one place per-run growth is unbounded (a
     * multi-hour run is tens of thousands of lines), so a naive recovery would load
     * that entire list back into memory and re-send it. Mirrors the save-time
     * decimation ([RunSession.decimatedTrackForCore], same [TRACK_DECIMATION_CAP]): a
     * uniform stride that always keeps the two endpoints and every fix that opened a
     * segment ([segmentStarts], remapped into the result). At this cap the kept cadence
     * stays far below [MAX_FIX_GAP_SEC] for any realistic run, so dropping
     * intermediate fixes can only WIDEN an existing gap and never fabricates a new
     * segment boundary. No-op below the cap. Pure, so it is unit-tested directly.
     */
    internal fun thinForRecovery(
        pts: List<GpsPoint>,
        maxPoints: Int = TRACK_DECIMATION_CAP,
        segmentStarts: Collection<Int> = emptyList(),
    ): RecoveredTrack {
        val n = pts.size
        val starts = segmentStarts.filter { it in 1 until n }.toSortedSet()
        if (n <= maxPoints || maxPoints < 2) return RecoveredTrack(pts, starts.toList())
        // Ceil division so the strided count never exceeds the cap.
        val stride = ((n + maxPoints - 1) / maxPoints).coerceAtLeast(1)
        val out = ArrayList<GpsPoint>()
        val outStarts = ArrayList<Int>()
        for (i in 0 until n) {
            val opens = i in starts
            // Always keep the true final fix (the end of the run) even if the stride
            // skips past it, and every segment's first fix so the boundary survives.
            if (i % stride != 0 && i != n - 1 && !opens) continue
            if (opens) outStarts.add(out.size)
            out.add(pts[i])
        }
        return RecoveredTrack(out, outStarts)
    }
}

/**
 * A run read back from [ActiveRunStore]: its fixes, as a plain list, plus the
 * indices (ascending, ≥ 1) of the fixes that opened a new recording segment live
 * (a Pause/Resume or a long fix gap). [RunSession.restore] breaks the recovered
 * track at exactly these, so it counts no leg the live run did not.
 */
class RecoveredTrack(
    private val points: List<GpsPoint>,
    val segmentStarts: List<Int>,
) : AbstractList<GpsPoint>() {
    override val size: Int get() = points.size
    override fun get(index: Int): GpsPoint = points[index]
}
