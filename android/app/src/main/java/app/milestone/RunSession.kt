package app.milestone

import java.util.Locale
import kotlinx.coroutines.flow.MutableStateFlow

/**
 * In-memory holder the foreground [RunTrackingService] writes and the UI
 * observes. Living here rather than inside the Composable is what lets a run
 * keep recording while the screen is off or the app is backgrounded: the
 * service owns the location stream, the screen just renders this state.
 */
object RunSession {
    val points = MutableStateFlow<List<GpsPoint>>(emptyList())
    val tracking = MutableStateFlow(false)

    // Locate-only preview (Phase 4 / M4): the service is acquiring GPS but NOT
    // recording yet. `locating` is true from opening the tracker until Stop/Back;
    // `lastFix` is the latest accepted fix (recording or not): it drives the
    // "GPS lock · ±N m" readout, the self-location dot before Start, and enables
    // the Start button once a good fix lands.
    val locating = MutableStateFlow(false)
    val lastFix = MutableStateFlow<GpsPoint?>(null)

    // Paused: the service keeps running (the run stays live/reentrant) but
    // incoming fixes are dropped, so the paused span records no route. The core's
    // of-record duration/pace/splits are MOVING time (legs below the auto-pause
    // floor are excluded), so a stop trims both the path and the pace clock, a
    // paused run is not penalised as if it were slow.
    val paused = MutableStateFlow(false)

    // Running path length (km), accumulated ONE segment at a time so neither the
    // notification nor the live sheet re-haversines the whole track every fix
    // (that was O(n²) over a multi-hour run). Correctness is identical to a full
    // segment-aware haversine over `points`, just computed incrementally.
    val distanceKm = MutableStateFlow(0.0)

    // Live elapsed MOVING time (seconds): the accumulated duration of MOVING legs
    // only, pause-bridge legs AND sub-[MIN_MOVING_SPEED_MPS] standing-still legs are
    // excluded, exactly as the core's of-record `moving_duration_min` excludes them -
    // so this live figure matches the saved run's moving-time duration/pace instead of
    // freezing at a pause then JUMPING the pause span back in on resume. Driven by
    // each fix's own MONOTONIC per-leg dt (elapsedRealtimeNanos), NOT wall-clock
    // `observedAt`, so a clock change / NTP correction can never make it jump or go
    // negative. The OF-RECORD duration is still core-derived from observedAt on save.
    val elapsedSec = MutableStateFlow(0L)

    // Monotonic bookkeeping (ms, elapsedRealtime timebase). [movingMs] accumulates the
    // per-leg dt of MOVING legs only (the live moving-time clock behind [elapsedSec]);
    // [lastElapsedMs] is the most recent fix's monotonic stamp, the base for each
    // per-leg dt (segment-gap + speed jitter/stop-floor checks). Both monotonic, so
    // neither depends on wall-clock timestamps that an NTP step could move.
    private var movingMs: Long = 0L
    private var lastElapsedMs: Long = 0L

    // Indices into `points` that BEGIN a new recording segment, the first fix
    // captured after a pause/resume or a long gap in fixes. The leg from
    // points[i-1] to points[i] at a boundary is a PAUSE BRIDGE: the runner may
    // have relocated (walked/drove) while paused, so that displacement must NOT
    // count as run distance (live OR of-record). Every real intra-segment leg
    // still counts. index 0 has no preceding leg so it is never stored here.
    // Registered at append time (covers both explicit pause boundaries and gap
    // boundaries) so [distanceKm] and [trackForCore] agree by construction.
    private val segmentStarts = java.util.concurrent.ConcurrentHashMap.newKeySet<Int>()

    // Set by the foreground service the instant it drops a fix because the run is
    // paused. The NEXT accepted fix then opens a new segment, so its bridging leg
    // back to the last pre-pause fix is dropped. Volatile: written on the location
    // callback thread, read under `add`'s lock.
    @Volatile private var pendingBreak = false

    /** The service calls this each time it drops a paused fix: the next recorded
     *  fix must start a new segment (a pause + relocation must not bridge). */
    fun markResumeBoundary() {
        pendingBreak = true
    }

    /** True when a fix arriving [dtMs] (MONOTONIC) after [prev] opens a new segment
     *  an explicit pause boundary or a fix gap longer than [MAX_FIX_GAP_SEC]. A
     *  non-monotonic dt (dtMs < 0) never reaches here: [add] DROPS such a fix before
     *  this check (see the drop guard there), so it can neither open a spurious
     *  segment nor wander the polyline backwards. */
    private fun isBoundary(prev: GpsPoint?, dtMs: Long): Boolean {
        if (prev == null) return false
        if (pendingBreak) return true
        return dtMs > MAX_FIX_GAP_SEC * 1000L
    }

    // Synchronized so the location callback's read-modify-write of points +
    // distance is atomic: rapid fixes can't clobber each other or double-count.
    // [fixElapsedRealtimeNanos] is the fix's own monotonic timestamp
    // (Location.getElapsedRealtimeNanos), the clock that drives elapsed and the
    // per-leg dt, immune to wall-clock skew.
    @Synchronized
    fun add(p: GpsPoint, fixElapsedRealtimeNanos: Long) {
        val prev = points.value
        val last = prev.lastOrNull()
        val fixMs = fixElapsedRealtimeNanos / 1_000_000
        val dtMs = fixMs - lastElapsedMs
        // Non-monotonic fix on the MONOTONIC elapsedRealtime timebase (dtMs < 0):
        // impossible once the service sorts each fused batch by elapsedRealtimeNanos
        // (globally monotonic, immune to NTP steps), but DROP it as defense in depth
        // rather than append it out of order: appending would either open a spurious
        // segment (splitting one real leg in two) or drag the polyline/live distance
        // backwards. Guarded by `last != null` so the FIRST fix, whose dtMs is the
        // large positive boot offset (lastElapsedMs starts at 0) is never dropped.
        if (last != null && dtMs < 0L) return
        if (isBoundary(last, dtMs)) {
            // New segment: record the boundary and DROP the bridge leg, no live
            // distance AND no moving time (the runner may have relocated while
            // paused). trackForCore reads segmentStarts to exclude the same leg from
            // the of-record track sent to the core.
            segmentStarts.add(prev.size)
        } else if (last != null) {
            // A leg counts as MOVING only when its implied ground speed is at or
            // above [MIN_MOVING_SPEED_MPS] (mirrors `load::is_stopped` <0.5 m/s): a
            // sub-floor leg (standing still with GPS wander) and a duplicate/stuck fix
            // (dt ≤ 0) accrue NOTHING: no live distance, no moving time. A moving leg
            // accrues its dt as moving time, and, unless it's a jitter spike above
            // [MAX_PLAUSIBLE_SPEED_MPS], its haversine as live distance (the core's
            // own QC re-gates both figures on save).
            val km = segmentKm(last, p)
            val speedMps = if (dtMs > 0L) km * 1000.0 / (dtMs / 1000.0) else Double.MAX_VALUE
            if (dtMs > 0L && speedMps >= MIN_MOVING_SPEED_MPS) {
                movingMs += dtMs
                // Jitter contract (mirrors the core `moving_legs`, running.rs:1828-1834,
                // and [paceBuckets] below): a leg above [MAX_PLAUSIBLE_SPEED_MPS] still
                // ELAPSED, so it keeps its moving time, but its distance is CAPPED at
                // the ceiling·dt rather than dropped, so live pace matches the
                // of-record moving pace instead of reading slightly slow (a dropped-
                // distance leg with kept time = an artificially slow leg). A normal
                // leg is under the cap so `min` is a no-op. The core re-gates on save.
                val maxKm = MAX_PLAUSIBLE_SPEED_MPS * (dtMs / 1000.0) / 1000.0
                distanceKm.value += minOf(km, maxKm)
            }
        }
        lastElapsedMs = fixMs
        elapsedSec.value = (movingMs / 1000L).coerceAtLeast(0L)
        pendingBreak = false
        points.value = prev + p
    }

    /**
     * Repopulate an interrupted run recovered from [ActiveRunStore] and put the
     * session back into recording state. The sidecar carries no explicit pause
     * markers, so boundaries are re-derived from fix-time gaps; distance is the
     * same segment-aware sum used live (a single O(n) pass, not per-fix).
     */
    @Synchronized
    fun restore(recovered: List<GpsPoint>) {
        segmentStarts.clear()
        // M2: the SPLICE from the last recovered fix to the first LIVE fix after
        // resume is a segment boundary, not a run leg. The app may have been dead
        // for up to hours of wall time while the runner relocated, yet the first
        // post-resume fix arrives only seconds later in the MONOTONIC timebase
        // ([add] measures gaps monotonically), so [isBoundary]'s gap rule would NOT
        // fire and that displacement would enter of-record distance + moving time.
        // Arm pendingBreak so the first live fix opens a new segment, exactly what
        // [markResumeBoundary] does for the explicit pause+resume case.
        pendingBreak = true
        var dist = 0.0
        var movingSec = 0.0
        for (i in 1 until recovered.size) {
            val dtSec = (recovered[i].observedAt - recovered[i - 1].observedAt).toDouble()
            if (dtSec > MAX_FIX_GAP_SEC) {
                segmentStarts.add(i)
            } else {
                // Same MOVING gate as the live path: a leg below the stop floor
                // (standing-still wander) accrues no distance/time; a moving leg
                // counts time and, jitter-CAPPED at [MAX_PLAUSIBLE_SPEED_MPS]·dt,
                // mirroring the live [add] path and the core: its distance, so a
                // recovered run can't show MORE distance/moving time than it had
                // before the crash.
                val km = segmentKm(recovered[i - 1], recovered[i])
                val speedMps = if (dtSec > 0.0) km * 1000.0 / dtSec else Double.MAX_VALUE
                if (dtSec > 0.0 && speedMps >= MIN_MOVING_SPEED_MPS) {
                    movingSec += dtSec
                    val maxKm = MAX_PLAUSIBLE_SPEED_MPS * dtSec / 1000.0
                    dist += minOf(km, maxKm)
                }
            }
        }
        points.value = recovered
        distanceKm.value = dist
        // Recovered fixes carry only wall-clock stamps (the crash sidecar has no
        // monotonic timebase), so seed the monotonic MOVING clock ONCE from the sum of
        // the recovered run's accepted-leg dt (its moving-time span, matching the live
        // clock) and let subsequent live fixes advance it monotonically from here: a
        // clock step after recovery still can't move it.
        movingMs = (movingSec * 1000.0).toLong().coerceAtLeast(0L)
        lastElapsedMs = android.os.SystemClock.elapsedRealtime()
        elapsedSec.value = (movingMs / 1000L).coerceAtLeast(0L)
        paused.value = false
        tracking.value = true
    }

    /**
     * The point list to hand the core in `LogRunTrack`, the TRUE captured
     * coordinates, UN-shifted (I15/B2). Older builds re-anchored each post-pause
     * segment onto the previous one so the core's flat haversine sum dropped the
     * bridge; that kept distance correct but SHIFTED the stored geometry, so the
     * exported GPX drew the real route in the wrong place. The core is now
     * segment-aware: it takes [segmentStartIndices] alongside these points and
     * excludes each pause-bridge leg itself, so the shell keeps the real
     * coordinates and lets the core (and the GPX) break the track at the pauses.
     */
    @Synchronized
    fun trackForCore(): List<GpsPoint> = points.value

    /**
     * The indices into [trackForCore]'s output that BEGIN a new recording segment
     * (a pause + possible relocation boundary), sorted ascending. Handed to the
     * core in `LogRunTrack.segment_starts` so it skips each pause-bridge leg and
     * breaks the GPX `<trkseg>` there. Empty for an un-paused run.
     */
    @Synchronized
    fun segmentStartIndices(): List<Int> {
        val n = points.value.size
        return segmentStarts.filter { it in 0 until n }.sorted()
    }

    /**
     * The ORIGINAL-track indices kept by decimation to at most ~[maxPoints] fixes:
     * the two endpoints (total span) and every pause-bridge boundary
     * ([segmentStarts], which the core must still see to break each segment), plus
     * a uniform stride. Ascending. Below the cap every index is kept. Shared by
     * [decimatedTrackForCore] and [decimatedSegmentStarts] so the thinned points
     * and the remapped boundaries can never disagree.
     */
    private fun decimatedKeptIndices(n: Int, maxPoints: Int): List<Int> {
        if (n <= maxPoints || maxPoints < 2) return (0 until n).toList()
        val keep = java.util.TreeSet<Int>()
        keep.add(0)
        keep.add(n - 1)
        for (s in segmentStarts) if (s in 0 until n) keep.add(s)
        // Uniform stride across the whole track. Ceil division so the strided
        // count never exceeds the cap; the handful of mandatory boundaries add at
        // most a few more, so the result is ~cap (bounded by cap + boundaries + 1).
        val stride = ((n + maxPoints - 1) / maxPoints).coerceAtLeast(1)
        var i = 0
        while (i < n) {
            keep.add(i)
            i += stride
        }
        return keep.toList() // TreeSet → ascending
    }

    /**
     * The point list to hand the core in `LogRunTrack`, decimated to at most
     * ~[maxPoints] fixes so the append-only event log doesn't grow without bound
     * (a multi-hour 1 Hz run is thousands of points on ONE line, the heaviest
     * thing the log stores). Downsamples [trackForCore]'s TRUE coordinates with a
     * uniform stride while ALWAYS keeping the first and last fix and every
     * pause-bridge boundary (so the core's segment-aware figures move negligibly:
     * distance within ±1 %, the interval-vs-steady VI verdict and the positive-
     * split sign unchanged at a 2–3 s cadence, proven by the decimation tolerance
     * tests in running.rs). The paired boundary indices come from
     * [decimatedSegmentStarts] with the SAME cap. Below the cap the track is
     * returned unthinned. A deliberate SAVE-TIME loss of intermediate fixes.
     */
    @Synchronized
    fun decimatedTrackForCore(maxPoints: Int = TRACK_DECIMATION_CAP): List<GpsPoint> {
        val full = trackForCore()
        return decimatedKeptIndices(full.size, maxPoints).map { full[it] }
    }

    /**
     * [segmentStarts] REMAPPED to the decimated point list of [decimatedTrackForCore]
     * (same [maxPoints]): each kept boundary's ORIGINAL index becomes its new
     * position among the kept indices. Handed to the core as
     * `LogRunTrack.segment_starts` so the thinned track still breaks at exactly the
     * pauses. Empty for an un-paused run.
     */
    @Synchronized
    fun decimatedSegmentStarts(maxPoints: Int = TRACK_DECIMATION_CAP): List<Int> {
        val n = points.value.size
        val kept = decimatedKeptIndices(n, maxPoints)
        val starts = segmentStarts.filter { it in 0 until n }.toHashSet()
        val out = ArrayList<Int>()
        kept.forEachIndexed { newIndex, origIndex -> if (origIndex in starts) out.add(newIndex) }
        return out
    }

    /**
     * Per-N-minute pace SLICES of the run, computed off the raw fixes, a factual
     * live-progress figure only (the of-record splits are the core's on save).
     *
     * A slice is [bucketMinutes] of MOVING time: pause-bridge legs (the same
     * [segmentStarts] boundaries [distanceKm]/[trackForCore] already exclude) count
     * for neither time nor distance, so a paused stretch never dilutes a slice. Each
     * leg's distance is split proportionally by time where it straddles a slice edge
     * (constant-speed within a ~1 s leg), so every completed slice spans EXACTLY
     * [bucketMinutes] of moving time and its pace is `minutes ÷ km` (canonical
     * min/km, the unit conversion happens at the display edge via Units).
     *
     * The returned list is chronological (oldest slice first, newest last). The last
     * element may be an in-progress tail ([PaceBucket.complete] == false, `minutes` <
     * N): a run shorter than one slice therefore yields at most that single partial
     * bucket, never a fabricated full one. A standing-still or all-paused run yields
     * no time and thus no buckets. One O(n) pass over the fixes.
     */
    @Synchronized
    fun paceBuckets(bucketMinutes: Int): List<PaceBucket> {
        val pts = points.value
        val n = bucketMinutes.coerceAtLeast(1)
        val bucketSec = n * 60.0
        if (pts.size < 2) return emptyList()
        val out = ArrayList<PaceBucket>()
        var curSec = 0.0 // moving seconds accumulated in the current (open) slice
        var curKm = 0.0 // distance accumulated in the current (open) slice
        for (i in 1 until pts.size) {
            // Pause-bridge leg: neither moving time nor run distance (mirrors the
            // segment-aware distance/of-record track). Drop it whole.
            if (i in segmentStarts) continue
            var legSec = (pts[i].observedAt - pts[i - 1].observedAt).toDouble()
            if (legSec < 0.0) continue // non-monotonic wall clock on this leg - skip
            var legKm = segmentKm(pts[i - 1], pts[i])
            // Jitter guard (mirrors live distance): cap a leg's distance so its
            // implied speed can't exceed MAX_PLAUSIBLE_SPEED_MPS: a GPS teleport
            // spike must not inflate a bucket's pace. Keep its time (it elapsed).
            if (legSec > 0.0) {
                val maxKm = MAX_PLAUSIBLE_SPEED_MPS * legSec / 1000.0
                if (legKm > maxKm) legKm = maxKm
            }
            // Close as many full slices as this leg spans, taking a time-proportional
            // slice of its distance at each boundary.
            while (curSec + legSec >= bucketSec) {
                val take = bucketSec - curSec
                val frac = if (legSec > 0.0) take / legSec else 0.0
                val kmChunk = legKm * frac
                curKm += kmChunk
                // Completed slice: exactly n minutes of moving time. km == 0 (stood
                // still without pausing) → non-finite pace, rendered "-" downstream.
                out.add(PaceBucket(paceMinPerKm = n / curKm, minutes = n.toDouble(), complete = true))
                legSec -= take
                legKm -= kmChunk
                curSec = 0.0
                curKm = 0.0
            }
            curSec += legSec
            curKm += legKm
        }
        // Trailing in-progress slice (< n minutes of moving time). Marked partial so
        // the UI never presents it as a full bucket.
        if (curSec > 0.0) {
            val minutes = curSec / 60.0
            out.add(PaceBucket(paceMinPerKm = minutes / curKm, minutes = minutes, complete = false))
        }
        return out
    }

    @Synchronized
    fun reset() {
        points.value = emptyList()
        tracking.value = false
        locating.value = false
        lastFix.value = null
        paused.value = false
        distanceKm.value = 0.0
        elapsedSec.value = 0L
        movingMs = 0L
        lastElapsedMs = 0L
        segmentStarts.clear()
        pendingBreak = false
    }
}

/**
 * One per-N-minute pace slice from [RunSession.paceBuckets]. [paceMinPerKm] is
 * canonical minutes-per-km (convert to the display unit with `paceInUnit`; a
 * non-finite value means "no distance covered" → render "-"). [minutes] is the
 * moving-time span of the slice, always the bucket size for a [complete] slice,
 * less for the trailing in-progress one.
 */
data class PaceBucket(
    val paceMinPerKm: Double,
    val minutes: Double,
    val complete: Boolean,
)

/** A leg implying a ground speed above this (m/s) is GPS jitter, not a
 *  footfall-to-footfall run leg: its displacement is dropped from live distance so a
 *  wander/spike can't inflate it. Matches the core's File-07 QC threshold
 *  (`load::MAX_PLAUSIBLE_SPEED_MPS` = 12.0 m/s) so live and of-record agree; the
 *  core re-gates the saved run on top. */
private const val MAX_PLAUSIBLE_SPEED_MPS = 12.0

/** A leg whose implied ground speed is BELOW this (m/s) is standing still, not a
 *  run leg: it accrues neither live distance (so GPS wander at a red light / café
 *  can't inflate `distanceKm`) nor moving time. Mirrors the core's auto-pause floor
 *  (`load::is_stopped` returns true below 0.5 m/s) so the live figures match the
 *  of-record `moving_duration_min`; same pattern as the [MAX_PLAUSIBLE_SPEED_MPS]
 *  upper-bound mirror above. */
private const val MIN_MOVING_SPEED_MPS = 0.5

/** A gap longer than this (seconds) between consecutive accepted fixes opens a
 *  new recording segment: the straight-line bridge across it (a pause+relocation,
 *  a tunnel, a signal dropout) is not counted as run distance. Conservative, a
 *  normal ~1–2 s fix cadence never trips it, so only genuine pauses/losses break. */
private const val MAX_FIX_GAP_SEC = 30L

/** Cap on the number of GPS fixes a saved run keeps in one `LogRunTrack` line.
 *  A multi-hour 1 Hz run is thousands of points, the single heaviest thing the
 *  append-only event log stores; [RunSession.decimatedTrackForCore] thins beyond
 *  this to a ~2–3 s cadence, bounding log growth while keeping the endpoints +
 *  every pause boundary so the core's of-record figures barely move (see the
 *  running.rs decimation tolerance tests). 5400 ≈ 90 min at a full 1 Hz cadence,
 *  or ~4.5 h once thinned to 3 s, ordinary runs keep near-full detail; only very
 *  long runs get decimated at all. */
const val TRACK_DECIMATION_CAP = 5400

/**
 * Elapsed run time: `m:ss` under an hour, `h:mm:ss` at or beyond one, so a long
 * run (half/marathon, both goal distances the app supports) reads "1:12:05"
 * rather than a confusing "72:05". Negative spans clamp to zero.
 */
fun formatElapsed(seconds: Long): String {
    val s = seconds.coerceAtLeast(0L)
    return if (s >= 3600) {
        "%d:%02d:%02d".format(Locale.US, s / 3600, (s % 3600) / 60, s % 60)
    } else {
        "%d:%02d".format(Locale.US, s / 60, s % 60)
    }
}
