package app.milestone

import android.os.SystemClock
import java.util.Locale
import java.util.TreeSet
import java.util.concurrent.ConcurrentHashMap
import kotlinx.coroutines.flow.MutableStateFlow

/**
 * In-memory holder the foreground [RunTrackingService] writes and the UI
 * observes. Living here rather than inside the Composable is what lets a run
 * keep recording while the screen is off or the app is backgrounded: the
 * service owns the location stream, the screen just renders this state.
 */
object RunSession {
    // Append-only backing store for the captured track. Fixes are only ever
    // APPENDED (never mutated or removed in place; reset/restore allocate a FRESH
    // array), so a [TrackSnapshot] published over the prefix [0, pointCount) stays
    // immutable for its whole life even as later fixes land. That lets [add]
    // publish a new `points` value in O(1) amortised (a growable array + O(1)
    // snapshot) rather than copying the whole list on every fix, which would be
    // O(n²) over a multi-hour 1 Hz run, on the main looper. Single writer (the
    // GPS callback, always under @Synchronized) + lock-free main-thread readers is
    // safe by construction: every published prefix is frozen, and each grow
    // allocates a NEW array so already-published snapshots keep their own backing.
    // Readers NEVER touch these two fields, only the published [TrackSnapshot].
    private var pointArr: Array<GpsPoint?> = arrayOfNulls(INITIAL_TRACK_CAP)
    private var pointCount = 0

    /** The captured track. Its value is a [TrackSnapshot] (an O(1) immutable view
     *  over the append-only backing), republished per accepted fix. Exposed as a
     *  plain `List<GpsPoint>` so consumers (map draw, `trackForCore`, History
     *  recovery, tests) need no knowledge of the snapshot type. */
    val points = MutableStateFlow<List<GpsPoint>>(TrackSnapshot(pointArr, 0))
    val tracking = MutableStateFlow(false)

    // Locate-only preview: the service is acquiring GPS but NOT
    // recording yet. `locating` is true from opening the tracker until Stop/Back;
    // `lastFix` is the latest accepted fix (recording or not); it drives the
    // "GPS lock · ±N m" readout, the self-location dot before Start, and enables
    // the Start button once a good fix lands.
    val locating = MutableStateFlow(false)
    val lastFix = MutableStateFlow<GpsPoint?>(null)

    // Armed start: Start is tappable before any lock, and recording begins
    // AUTOMATICALLY at the first qualifying fix. The service sets this true on entering
    // ARM mode and clears it the instant recording begins (or on teardown). A process
    // singleton so the transition survives rotation/backgrounding: the service owns it,
    // the screen only observes. [restore] forces it false, a recovered run is already
    // recording, never armed.
    val armed = MutableStateFlow(false)

    // The location stream could not be started or died on start (no GPS provider on
    // the device, the fused client's async failure, a permission race). Set by the
    // service's failure unwind; the screen stops re-launching the locate preview while
    // it is set (otherwise each failure would immediately restart the service in a
    // tight start/stop loop) and clears it on ON_RESUME, Try again or Start.
    val locationFailed = MutableStateFlow(false)

    // Monotonic (elapsedRealtime) stamp of the last ACCEPTED fix, set by the service on
    // every plausible fix (preview/armed fixes never reach [add], so it cannot live
    // there). Drives [isFreshLock]'s freshness gate and the signal-loss pill: a lock is
    // "fresh" only for [LOCK_FRESH_MS] after its fix, so a single stale fix cannot
    // latch Start/GPS-lock forever. 0 = no fix.
    val lastFixElapsedMs = MutableStateFlow(0L)

    // Monotonically increasing token identifying the CURRENT session, bumped on every
    // [reset]. The service captures it when it starts a stream
    // ([RunTrackingService.startTracking]) and refuses to record (self-stopping) once
    // it no longer matches: a Cancel/Back/discard while armed runs `stop()` (async
    // ACTION_STOP) then `reset()`, but a location result already queued on the main
    // looper can fire BEFORE ACTION_STOP is processed. That late fix would still see
    // `armed` true and run beginRecording() (opening the sidecar + adding points)
    // AFTER the reset, resurrecting a ghost session and a false crash-recovery
    // prompt; the bump makes it observe a newer generation and stand down instead.
    // @Volatile for visibility to the service's main-thread read; written only under
    // reset()'s lock.
    @Volatile
    var generation: Long = 0L
        private set

    // Paused: the service keeps running (the run stays live/reentrant) but
    // incoming fixes are dropped, so the paused span records no route. The core's
    // of-record duration/pace/splits are MOVING time (legs below the auto-pause
    // floor are excluded), so a stop trims both the path and the pace clock, a
    // paused run is not penalised as if it were slow.
    val paused = MutableStateFlow(false)

    // A run save is in flight; gates Save/Discard/Stop. Process-scoped so a
    // rotation can neither resurrect it (double-save) nor orphan it true (stuck
    // buttons after a failed save). Cleared by [reset] (inside the save critical
    // section, after the persist landed) and by the save-failure path.
    val saving = MutableStateFlow(false)

    // Running path length (km), accumulated ONE segment at a time so neither the
    // notification nor the live sheet re-haversines the whole track every fix
    // (which would be O(n²) over a multi-hour run). Correctness is identical to a
    // full segment-aware haversine over `points`, just computed incrementally.
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

    /** Per-N-minute pace slices, accumulated INCREMENTALLY as fixes arrive and
     *  published here so the live sheet observes them directly; a full rescan of the
     *  track on every fix would be O(n) per fix. The accumulator ([completedBuckets] +
     *  the open [bucketCurSec]/[bucketCurKm] tail) is fed one leg at a time in [add];
     *  [rebuildPaceBuckets] replays the stored track through the SAME [feedPaceLeg]
     *  when the bucket size changes or a run is recovered. The unit tests hold a
     *  from-scratch reference and assert this flow equals it after any sequence. */
    val paceBucketsFlow = MutableStateFlow<List<PaceBucket>>(emptyList())
    // The bucket size (minutes) the accumulator is currently built for. Changed
    // only via [setPaceBucketMinutes], which rebuilds from the backing store, so
    // the accumulator is always consistent with this size.
    private var bucketMin = DEFAULT_PACE_BUCKET_MIN
    private val completedBuckets = ArrayList<PaceBucket>()
    private var bucketCurSec = 0.0
    private var bucketCurKm = 0.0

    /** The CURRENT pace (canonical min/km) over the trailing [RollingPace] window
     *  (≥[ROLLING_PACE_MIN_SPAN_MS] and ≥[ROLLING_PACE_MIN_METERS] of recent legs,
     *  capped at [ROLLING_PACE_MAX_SPAN_MS]), so a surge, hill or rep shows up within
     *  one window instead of being averaged away over the whole run.
     *  Non-finite = not enough recent ground covered (standing still, a fresh
     *  segment) → rendered "-". Standing-still legs keep their time in the window, so
     *  stopping drives it to "-" rather than freezing the last running pace. */
    val currentPaceMinPerKm = MutableStateFlow(Double.NaN)
    private val rollingPace = RollingPace()

    /** Today's planned run item the tracker follows, set by the screen when
     *  the runner taps Start with "Follow today's plan" on; null for a free run. Its
     *  [PrescriptionView.run_target] drives the on-screen targets and its
     *  [PrescriptionView.run_structure] the step guide. Cleared by [reset]. */
    val plannedRun = MutableStateFlow<PrescriptionView?>(null)

    /** The live step guide (null when the run has no structured steps). Written only
     *  on the main thread (the service's tick + fix callbacks and the screen's Skip
     *  step), so the machine needs no lock. */
    internal val stepProgress = MutableStateFlow<StepProgress?>(null)
    private var stepMachine: RunStepMachine? = null

    /** Start the step guide for [plannedRun] at the moment recording begins. A no-op
     *  without a structured plan or when a guide is already running. */
    fun startSteps(nowMs: Long) {
        if (stepMachine != null) return
        val steps = plannedRun.value?.run_structure?.let { buildRunSteps(it) }.orEmpty()
        if (steps.isEmpty()) return
        val m = RunStepMachine(steps)
        m.start(nowMs, distanceKm.value * 1000.0)
        stepMachine = m
        stepProgress.value = m.progress()
    }

    /** Advance the guide to [nowMs]; true when the step changed (the service cues). */
    fun tickSteps(nowMs: Long): Boolean {
        val m = stepMachine ?: return false
        val changed = m.advance(nowMs, distanceKm.value * 1000.0, paused.value)
        stepProgress.value = m.progress()
        return changed
    }

    /** The runner's "Skip step". */
    fun skipStep(nowMs: Long) {
        val m = stepMachine ?: return
        m.skip(nowMs, distanceKm.value * 1000.0, paused.value)
        stepProgress.value = m.progress()
    }

    // Indices into `points` that BEGIN a new recording segment, the first fix
    // captured after a pause/resume or a long gap in fixes. The leg from
    // points[i-1] to points[i] at a boundary is a PAUSE BRIDGE: the runner may
    // have relocated (walked/drove) while paused, so that displacement must NOT
    // count as run distance (live OR of-record). Every real intra-segment leg
    // still counts. index 0 has no preceding leg so it is never stored here.
    // Registered at append time (covers both explicit pause boundaries and gap
    // boundaries) so [distanceKm] and [trackForCore] agree by construction.
    private val segmentStarts = ConcurrentHashMap.newKeySet<Int>()

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
    // Returns true iff the fix was appended; a dropped non-monotonic fix returns false so
    // the crash-recovery sidecar mirrors only accepted fixes.
    @Synchronized
    fun add(p: GpsPoint, fixElapsedRealtimeNanos: Long): Boolean {
        val last = if (pointCount > 0) pointArr[pointCount - 1] else null
        val fixMs = fixElapsedRealtimeNanos / 1_000_000
        val dtMs = fixMs - lastElapsedMs
        // Non-monotonic fix on the MONOTONIC elapsedRealtime timebase (dtMs < 0):
        // impossible once the service sorts each fused batch by elapsedRealtimeNanos
        // (globally monotonic, immune to NTP steps), but DROP it as defense in depth
        // rather than append it out of order: appending would either open a spurious
        // segment (splitting one real leg in two) or drag the polyline/live distance
        // backwards. Guarded by `last != null` so the FIRST fix, whose dtMs is the
        // large positive boot offset (lastElapsedMs starts at 0) is never dropped.
        if (last != null && dtMs < 0L) return false
        if (isBoundary(last, dtMs)) {
            // New segment: record the boundary and DROP the bridge leg, no live
            // distance AND no moving time (the runner may have relocated while
            // paused). trackForCore reads segmentStarts to exclude the same leg from
            // the of-record track sent to the core. The pace accumulator ALSO drops
            // this leg (it never reaches [feedPaceLeg]), and [rebuildPaceBuckets]
            // skips the same stored boundary index (== pointCount). The current-pace
            // window restarts too: pre-pause legs are not "now".
            segmentStarts.add(pointCount)
            rollingPace.clear()
        } else if (last != null) {
            // A leg counts as MOVING only when its implied ground speed is at or
            // above [MIN_MOVING_SPEED_MPS] (mirrors `load::is_stopped` <0.5 m/s): a
            // sub-floor leg (standing still with GPS wander) and a duplicate/stuck fix
            // (dt ≤ 0) accrue NOTHING: no live distance, no moving time. A moving leg
            // accrues its dt as moving time, and, unless it's a jitter spike above
            // [MAX_PLAUSIBLE_SPEED_MPS], its haversine as live distance. The saved
            // figure applies the SAME stop floor and cap (`running::moving_legs`
            // feeds both the distance of record and the moving pace), and the core
            // additionally drops fixes under 2.5 m apart (`qc_track`), so live and
            // saved distance agree to within that jitter gate.
            val km = segmentKm(last, p)
            val speedMps = if (dtMs > 0L) km * 1000.0 / (dtMs / 1000.0) else Double.MAX_VALUE
            if (dtMs > 0L && speedMps >= MIN_MOVING_SPEED_MPS) {
                movingMs += dtMs
                // Jitter contract (mirrors the core's `moving_legs` and
                // [feedPaceLeg]): a leg above [MAX_PLAUSIBLE_SPEED_MPS] still
                // ELAPSED, so it keeps its moving time, but its distance is CAPPED at
                // the ceiling·dt rather than dropped, so live pace matches the
                // of-record moving pace instead of reading slightly slow (a dropped-
                // distance leg with kept time = an artificially slow leg). A normal
                // leg is under the cap so `min` is a no-op.
                val maxKm = MAX_PLAUSIBLE_SPEED_MPS * (dtMs / 1000.0) / 1000.0
                val legKm = minOf(km, maxKm)
                distanceKm.value += legKm
                rollingPace.add(dtMs, legKm * 1000.0)
            } else if (dtMs > 0L) {
                // Standing still: the leg's time counts toward the current-pace window
                // with no distance, so "Pace now" decays to "-" while stopped.
                rollingPace.add(dtMs, 0.0)
            }
            // Fold this (non-boundary) leg into the pace-bucket accumulator using the
            // MONOTONIC per-leg dt (the same [dtMs] the distance/moving-time block above
            // uses); [feedPaceLeg] applies the stop floor + jitter cap, so a
            // standing-still leg can't slow the open slice.
            feedPaceLeg(last, p, dtMs)
        }
        lastElapsedMs = fixMs
        elapsedSec.value = (movingMs / 1000L).coerceAtLeast(0L)
        currentPaceMinPerKm.value = rollingPace.paceMinPerKm()
        pendingBreak = false
        appendPoint(p)
        emitPaceBuckets()
        return true
    }

    /** Append one accepted fix to the growable backing store and publish a fresh
     *  O(1) [TrackSnapshot]. Amortised O(1): the array doubles on overflow (total
     *  O(n) over the run), and a grow allocates a NEW array so any already-published
     *  snapshot keeps its own frozen backing (lock-free reader safety). */
    private fun appendPoint(p: GpsPoint) {
        if (pointCount == pointArr.size) {
            pointArr = pointArr.copyOf(pointArr.size * 2)
        }
        pointArr[pointCount] = p
        pointCount++
        points.value = TrackSnapshot(pointArr, pointCount)
    }

    /** Fold a single non-pause-bridge leg into the incremental pace accumulator (same
     *  jitter cap as live distance, slices closed with a time-proportional share of
     *  the straddling leg). [legMs] is the leg's duration: the MONOTONIC per-leg dt
     *  from [add] on the live path, or the wall-clock span on the [rebuildPaceBuckets]
     *  path (the stored fixes carry no monotonic timebase). A leg whose implied ground
     *  speed is below [MIN_MOVING_SPEED_MPS] (standing still with GPS wander at a
     *  light/café) is DROPPED whole — it accrues neither slice time nor distance — so
     *  an idle stretch can't dilute the open slice's pace, mirroring [distanceKm]'s
     *  stop floor and the core's `load::is_stopped`. */
    private fun feedPaceLeg(a: GpsPoint, b: GpsPoint, legMs: Long) {
        val n = bucketMin.coerceAtLeast(1)
        val bucketSec = n * 60.0
        if (legMs <= 0L) return // no elapsed time / non-monotonic leg
        var legSec = legMs / 1000.0
        var legKm = segmentKm(a, b)
        // Stop floor: a sub-[MIN_MOVING_SPEED_MPS] leg is standing still, not a run
        // leg, so drop it before it can add time (and thus slow the slice's pace).
        if (legKm * 1000.0 / legSec < MIN_MOVING_SPEED_MPS) return
        val maxKm = MAX_PLAUSIBLE_SPEED_MPS * legSec / 1000.0
        if (legKm > maxKm) legKm = maxKm
        while (bucketCurSec + legSec >= bucketSec) {
            val take = bucketSec - bucketCurSec
            val frac = if (legSec > 0.0) take / legSec else 0.0
            val kmChunk = legKm * frac
            bucketCurKm += kmChunk
            completedBuckets.add(PaceBucket(paceMinPerKm = n / bucketCurKm, minutes = n.toDouble(), complete = true))
            legSec -= take
            legKm -= kmChunk
            bucketCurSec = 0.0
            bucketCurKm = 0.0
        }
        bucketCurSec += legSec
        bucketCurKm += legKm
    }

    /** Publish the current accumulator: the completed slices plus, if the open slice
     *  holds any moving time, an in-progress partial tail. A fresh list each call is
     *  the publication barrier that hands the GPS-thread accumulator safely to the
     *  main-thread observer. */
    private fun emitPaceBuckets() {
        val out = ArrayList<PaceBucket>(completedBuckets.size + 1)
        out.addAll(completedBuckets)
        if (bucketCurSec > 0.0) {
            val minutes = bucketCurSec / 60.0
            out.add(PaceBucket(paceMinPerKm = minutes / bucketCurKm, minutes = minutes, complete = false))
        }
        paceBucketsFlow.value = out
    }

    /** Rebuild the pace accumulator (and republish [paceBucketsFlow]) for the
     *  current [bucketMin] from the whole backing store, one O(n) pass, used when
     *  the user changes the bucket size mid-run ([setPaceBucketMinutes]) or a run is
     *  recovered ([restore]). Pause-bridge legs are skipped exactly as [add] skips
     *  them. */
    private fun rebuildPaceBuckets() {
        completedBuckets.clear()
        bucketCurSec = 0.0
        bucketCurKm = 0.0
        for (i in 1 until pointCount) {
            if (segmentStarts.contains(i)) continue
            val a = pointArr[i - 1]!!
            val b = pointArr[i]!!
            // No monotonic timebase on the rebuild path (the stored fixes carry only
            // wall-clock observedAt), so derive the leg dt from observedAt.
            feedPaceLeg(a, b, (b.observedAt - a.observedAt) * 1000L)
        }
        emitPaceBuckets()
    }

    /** Set the live pace-bucket size (minutes). A no-op when unchanged; otherwise
     *  rebuilds the incremental accumulator from the backing store so the exposed
     *  [paceBucketsFlow] matches the new size. Called from the live sheet whenever
     *  the user's `paceBucketMinutes` preference changes. */
    @Synchronized
    fun setPaceBucketMinutes(minutes: Int) {
        val n = minutes.coerceAtLeast(1)
        if (n == bucketMin) return
        bucketMin = n
        rebuildPaceBuckets()
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
        // The SPLICE from the last recovered fix to the first LIVE fix after
        // resume is a segment boundary, not a run leg. The app may have been dead
        // for up to hours of wall time while the runner relocated, yet the first
        // post-resume fix arrives only seconds later in the MONOTONIC timebase
        // ([add] measures gaps monotonically), so [isBoundary]'s gap rule would NOT
        // fire and that displacement would enter of-record distance + moving time.
        // Arm pendingBreak so the first live fix opens a new segment; exactly what
        // [markResumeBoundary] does for the explicit pause+resume case.
        pendingBreak = true
        // Refill the append-only backing store from the recovered fixes (a FRESH
        // array so any snapshot published before recovery keeps its own backing).
        pointArr = arrayOfNulls(maxOf(INITIAL_TRACK_CAP, recovered.size))
        pointCount = 0
        for (pt in recovered) pointArr[pointCount++] = pt
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
        points.value = TrackSnapshot(pointArr, pointCount)
        distanceKm.value = dist
        // Recovered fixes carry only wall-clock stamps (the crash sidecar has no
        // monotonic timebase), so seed the monotonic MOVING clock ONCE from the sum of
        // the recovered run's accepted-leg dt (its moving-time span, matching the live
        // clock) and let subsequent live fixes advance it monotonically from here: a
        // clock step after recovery still can't move it.
        movingMs = (movingSec * 1000.0).toLong().coerceAtLeast(0L)
        lastElapsedMs = SystemClock.elapsedRealtime()
        elapsedSec.value = (movingMs / 1000L).coerceAtLeast(0L)
        // The resume splice opens a new segment, so no recovered leg is "now".
        rollingPace.clear()
        currentPaceMinPerKm.value = Double.NaN
        paused.value = false
        tracking.value = true
        // A recovered run is already recording, never armed: force the arm flag off so
        // a crash while merely armed can't leave the resumed screen stuck armed.
        armed.value = false
        lastFixElapsedMs.value = 0L
        // Rebuild the pace accumulator for the recovered track (same one O(n) pass
        // as a bucket-size change) so the live sheet's slices resume correctly.
        rebuildPaceBuckets()
    }

    /**
     * The point list to hand the core in `LogRunTrack`; the TRUE captured
     * coordinates, never shifted or re-anchored, so the exported GPX draws the real
     * route. The core is segment-aware: it takes [decimatedSegmentStarts] alongside
     * these points and excludes each pause-bridge leg itself, so the core (and the
     * GPX) break the track at the pauses.
     */
    @Synchronized
    fun trackForCore(): List<GpsPoint> = points.value

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
        val keep = TreeSet<Int>()
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

    /** Reset only the live engine-state flags — recording ([tracking]), locate-preview
     *  ([locating]) and [armed] — to the STOPPED state, WITHOUT touching the captured
     *  track or the save/pause bookkeeping (unlike [reset], which wipes the whole
     *  session). Both the service's stopTracking() and [RunTrackingService.stop]'s
     *  dead-service catch call this, so a stop can never leave `locating`/`armed`
     *  latched ("Acquiring GPS" with no engine behind it) when only `tracking` was
     *  cleared. */
    fun markStopped() {
        tracking.value = false
        locating.value = false
        armed.value = false
    }

    @Synchronized
    fun reset() {
        // Bump the session token FIRST so a location result already queued on the main
        // looper (from the service that is about to receive ACTION_STOP) observes a
        // newer generation and refuses to record into this now-cleared session, instead
        // of resurrecting a ghost run + sidecar after the clear/reset below.
        generation++
        // Fresh backing array so any snapshot still held by a departing collector
        // keeps its own frozen storage (a new run can never overwrite it).
        pointArr = arrayOfNulls(INITIAL_TRACK_CAP)
        pointCount = 0
        points.value = TrackSnapshot(pointArr, 0)
        tracking.value = false
        locating.value = false
        lastFix.value = null
        armed.value = false
        locationFailed.value = false
        lastFixElapsedMs.value = 0L
        paused.value = false
        saving.value = false
        distanceKm.value = 0.0
        elapsedSec.value = 0L
        movingMs = 0L
        lastElapsedMs = 0L
        segmentStarts.clear()
        pendingBreak = false
        // Drop the pace accumulator (keep [bucketMin], the user's chosen slice size
        // persists across runs).
        completedBuckets.clear()
        bucketCurSec = 0.0
        bucketCurKm = 0.0
        paceBucketsFlow.value = emptyList()
        rollingPace.clear()
        currentPaceMinPerKm.value = Double.NaN
        // The guide and its plan belong to the run being torn down.
        plannedRun.value = null
        stepMachine = null
        stepProgress.value = null
    }

    /**
     * Immutable O(1) view over the append-only [pointArr] prefix `[0, len)`. Because
     * the store is append-only and every reset/restore swaps in a FRESH array, the
     * slots `[0, len)` never change after a snapshot is published, so this is a
     * valid immutable `List<GpsPoint>` with no copy, and lock-free to read from any
     * thread. [equals] has an O(1) fast path (same backing array + same length) so
     * [StateFlow]'s dedup, which calls `equals` on every `value =` assignment,
     * stays O(1) per fix instead of an O(n) element-wise compare. Two snapshots of
     * one run never share a length (each is published once, on append), so
     * consecutive values are always distinct and the flow always emits. Comparison
     * against a NON-snapshot List falls back to element-wise [AbstractList]
     * semantics, the path tests' `assertEquals(expectedList, points.value)` drives
     * (expected side compares).
     */
    private class TrackSnapshot(
        private val backing: Array<GpsPoint?>,
        private val len: Int,
    ) : AbstractList<GpsPoint>() {
        override val size: Int get() = len
        override fun get(index: Int): GpsPoint {
            if (index < 0 || index >= len) {
                throw IndexOutOfBoundsException("index $index, size $len")
            }
            @Suppress("UNCHECKED_CAST")
            return backing[index] as GpsPoint
        }
        override fun equals(other: Any?): Boolean {
            if (other === this) return true
            // O(1) fast path: identical backing + length ⇒ identical elements (the
            // store is append-only and each published prefix is frozen), so the two
            // views are equal without an element walk. It is ONLY a shortcut for the
            // definitely-equal case — it must NOT short-circuit to `false` on a
            // different backing, or two equal-content snapshots on distinct arrays
            // would compare unequal while each still compares EQUAL (element-wise) to a
            // plain List of the same fixes, breaking List.equals symmetry/transitivity.
            // Fall through to [AbstractList]'s element-wise semantics otherwise (itself
            // O(1) when the sizes differ — the common StateFlow per-fix case). [hashCode]
            // stays AbstractList's element-wise hash, consistent with this equals.
            if (other is TrackSnapshot && backing === other.backing && len == other.len) return true
            return super.equals(other)
        }
    }
}

/**
 * One per-N-minute pace slice of [RunSession.paceBucketsFlow]. [paceMinPerKm] is
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

/** [RollingPace] window rule (see its KDoc): the window spans at least
 *  [ROLLING_PACE_MIN_SPAN_MS] AND [ROLLING_PACE_MIN_METERS] of recent legs (so a slow
 *  jog still averages over real ground and one noisy 1 s leg can't swing it), but
 *  never more than [ROLLING_PACE_MAX_SPAN_MS] (so standing still empties it of
 *  distance). Below [ROLLING_PACE_MIN_SHOW_SPAN_MS] / [ROLLING_PACE_MIN_SHOW_METERS]
 *  there is too little evidence to show a pace at all. */
private const val ROLLING_PACE_MIN_SPAN_MS = 30_000L
private const val ROLLING_PACE_MIN_METERS = 200.0
private const val ROLLING_PACE_MAX_SPAN_MS = 60_000L
private const val ROLLING_PACE_MIN_SHOW_SPAN_MS = 10_000L
private const val ROLLING_PACE_MIN_SHOW_METERS = 20.0

/**
 * Trailing-window CURRENT pace over recent legs, O(1) amortised per leg (a deque of
 * (duration, distance) with running sums). The window is the shortest suffix that
 * spans at least [minSpanMs] AND [minMeters] (so a slow jog still averages over
 * ~200 m and GPS noise on a single 1 s leg can't swing it), but never more than
 * [maxSpanMs] (so standing still empties it of distance and the pace reads "-"
 * instead of the last running pace). Too little evidence ([minShowSpanMs] /
 * [minShowMeters]) → NaN. Pure (no clock), so the window rule is JVM-testable.
 */
internal class RollingPace(
    private val minSpanMs: Long = ROLLING_PACE_MIN_SPAN_MS,
    private val minMeters: Double = ROLLING_PACE_MIN_METERS,
    private val maxSpanMs: Long = ROLLING_PACE_MAX_SPAN_MS,
    private val minShowSpanMs: Long = ROLLING_PACE_MIN_SHOW_SPAN_MS,
    private val minShowMeters: Double = ROLLING_PACE_MIN_SHOW_METERS,
) {
    private val legMs = ArrayDeque<Long>()
    private val legM = ArrayDeque<Double>()
    private var spanMs = 0L
    private var meters = 0.0

    fun add(durationMs: Long, distanceM: Double) {
        if (durationMs <= 0L) return
        legMs.addLast(durationMs)
        legM.addLast(distanceM)
        spanMs += durationMs
        meters += distanceM
        while (legMs.size > 1) {
            val restSpan = spanMs - legMs.first()
            val restM = meters - legM.first()
            if (spanMs > maxSpanMs || (restSpan >= minSpanMs && restM >= minMeters)) {
                legMs.removeFirst()
                legM.removeFirst()
                spanMs = restSpan
                meters = restM.coerceAtLeast(0.0)
            } else {
                break
            }
        }
    }

    fun clear() {
        legMs.clear()
        legM.clear()
        spanMs = 0L
        meters = 0.0
    }

    /** Canonical minutes per km over the current window, or NaN when the window
     *  holds too little time or ground to be meaningful. */
    fun paceMinPerKm(): Double =
        if (spanMs < minShowSpanMs || meters < minShowMeters) Double.NaN
        else (spanMs / 60_000.0) / (meters / 1000.0)
}

/** A leg implying a ground speed above this (m/s) is GPS jitter, not a
 *  footfall-to-footfall run leg: its distance is capped at this speed·dt so a
 *  wander/spike can't inflate live distance. Matches the core's File-07 QC threshold
 *  (`load::MAX_PLAUSIBLE_SPEED_MPS` = 12.0 m/s) so live and of-record agree; the
 *  core re-gates the saved run on top. */
internal const val MAX_PLAUSIBLE_SPEED_MPS = 12.0

/** A leg whose implied ground speed is BELOW this (m/s) is standing still, not a
 *  run leg: it accrues neither live distance (so GPS wander at a red light / café
 *  can't inflate `distanceKm`) nor moving time. Mirrors the core's auto-pause floor
 *  (`load::is_stopped` returns true below 0.5 m/s) so the live figures match the
 *  of-record `moving_duration_min`; same pattern as the [MAX_PLAUSIBLE_SPEED_MPS]
 *  upper-bound mirror above. */
internal const val MIN_MOVING_SPEED_MPS = 0.5

/** A gap longer than this (seconds) between consecutive accepted fixes opens a
 *  new recording segment: the straight-line bridge across it (a pause+relocation,
 *  a tunnel, a signal dropout) is not counted as run distance. Conservative, a
 *  normal ~1–2 s fix cadence never trips it, so only genuine pauses/losses break. */
private const val MAX_FIX_GAP_SEC = 30L

// ── GPS fix-quality & freshness gates ────────────────────────────────────────
// Single-sourced here so the foreground service (live map + distance), the UI lock
// pill, and [isFreshLock] can never drift apart, and so the live figures stay in
// step with the core's of-record QC.

/** Worst reported accuracy (m) a live fix may carry to reach the map/distance AND to
 *  count as a usable GPS lock (drives the "GPS lock · ±N m" pill / [isFreshLock]).
 *  Mirrors the core's of-record QC bound `running::MAX_GPS_ACCURACY_M` (= 30 m,
 *  running.rs) so a fix the saved run would drop never counts live — live distance/
 *  pace cannot exceed the of-record figures. The 20 m precise-start gate
 *  ([GPS_PRECISE_ACCURACY_M], RunTracking.kt) is stricter still. */
internal const val MAX_GPS_ACCURACY_M = 30.0

/** Max age (ms, monotonic elapsedRealtime) a fix may have while merely ARMED or
 *  previewing before the tracker rejects it as a stale cached location (the fused
 *  provider's first callback is often minutes old / far away). NOT applied while
 *  RECORDING: batched fixes (Doze wakeups, main-looper stalls) must all reach
 *  [RunSession.add] + the sidecar — the FusedEngine forwards every batched fix, and
 *  [RunSession.add]'s dt<0 drop-guard + the segment boundaries handle ordering. */
internal const val ARMED_FIX_MAX_AGE_MS = 10_000L

/** The armed/preview staleness decision, pure so the batched-delivery exemption is
 *  unit-testable without a Location/SystemClock. A fix older than
 *  [ARMED_FIX_MAX_AGE_MS] is rejected ONLY when it arrives ALONE ([batchSize] == 1):
 *  that is the lone cached first callback the fused provider hands back minutes old,
 *  the exact hazard this gate exists for. A fix delivered as part of a MULTI-fix
 *  batch ([batchSize] > 1) is genuine ongoing screen-off delivery, so it is exempt
 *  even when old — otherwise a batch flushed >~10 s late lets only its newest fix
 *  through and the [ArmTrigger] moving window can never fill, so a screen-off
 *  'Start on move' silently never fires. Never consulted while recording (batched
 *  fixes are always kept then). */
internal fun armedFixPassesAgeGate(ageMs: Long, batchSize: Int): Boolean =
    batchSize > 1 || ageMs <= ARMED_FIX_MAX_AGE_MS

/** A GPS lock counts as "fresh" only within this monotonic window (ms) of its fix,
 *  so a Start can never latch onto a frozen one-time fix and record nothing. Equal to
 *  [ARMED_FIX_MAX_AGE_MS] by design: a lock the UI trusts is exactly one the
 *  recorder would also accept while armed. */
internal const val LOCK_FRESH_MS = ARMED_FIX_MAX_AGE_MS

/** No accepted fix for this long (ms) while recording surfaces the signal-loss pill.
 *  Above the worst healthy fix cadence (~4 s screen-off) so a normal gap never trips
 *  it, and below [MAX_FIX_GAP_SEC] (30 s, the segment-break gap) so the runner is
 *  warned BEFORE the track actually breaks a segment. */
internal const val SIGNAL_LOSS_MS = 15_000L

/** True once a RECORDING run has gone longer than [SIGNAL_LOSS_MS] without an
 *  accepted fix. Shared by the in-app pill and the service's screen-off watchdog so
 *  the two can never disagree. [lastFixElapsedMs] == 0 means no fix yet → false
 *  (nothing has been lost). Both stamps are monotonic elapsedRealtime. */
internal fun signalLossDue(lastFixElapsedMs: Long, nowElapsedMs: Long): Boolean =
    lastFixElapsedMs > 0L && nowElapsedMs - lastFixElapsedMs > SIGNAL_LOSS_MS

/** Consecutive MOVING legs the "Start on move" gate needs before it fires. Two legs
 *  (~2–3 s at a ~1 Hz cadence) distinguishes real run-up from a single jitter step
 *  without a long stall at the line. */
private const val MOVING_ARM_MIN_LEGS = 2

/** The "Start on move" gate detects movement over a rolling window this long (ms):
 *  long enough to hold [MOVING_ARM_MIN_LEGS] legs at both the foreground (~2 s) and
 *  background (~4 s) fix cadence, so an isolated jitter/duplicate fix ages out of the
 *  window instead of resetting the run-up. */
private const val ARM_MOVING_WINDOW_MS = 10_000L

/** Minimum NET ground the "Start on move" gate must cover over its window before it
 *  fires (m): an absolute floor so a burst of near-simultaneous fixes or a metre of
 *  GPS wander can't satisfy the speed band on its own. */
private const val MOVING_ARM_MIN_NET_M = 5.0

/**
 * True when [accuracyM] is a lock the tracker can START from: accurate enough AND
 * recent enough. Accuracy alone is not enough: one good fix would latch "locked"
 * forever, so a Start on a stale lock would record nothing (every later fix fails
 * the recorder's own age gate). Pure (the clocks are parameters) so it is
 * JVM-testable. [fixElapsedMs] == 0 means no fix yet → false; a null accuracy
 * (unmeasurable) → false.
 */
internal fun isFreshLock(
    accuracyM: Double?,
    fixElapsedMs: Long,
    nowElapsedMs: Long,
    maxAccuracyM: Double = MAX_GPS_ACCURACY_M,
    freshMs: Long = LOCK_FRESH_MS,
): Boolean {
    if (accuracyM == null || fixElapsedMs == 0L) return false
    val ageMs = nowElapsedMs - fixElapsedMs
    return accuracyM in 0.0..maxAccuracyM && ageMs in 0L..freshMs
}

/**
 * Decides WHEN an armed run begins recording. Fed each accepted preview fix via
 * [offer], it returns the SEED fixes to open the run with (so a moving start's run-up
 * distance is kept) or null to keep waiting. Lives here, beside the speed constants it
 * reuses ([MIN_MOVING_SPEED_MPS] / [MAX_PLAUSIBLE_SPEED_MPS]), and is pure, the fix's
 * monotonic stamp is passed in, no SystemClock inside, so the gate logic is
 * JVM-testable.
 *
 * Gates stack:
 *  - none: the first fix seeds immediately.
 *  - precise: the first fix at or under [preciseAccuracyM] seeds.
 *  - moving: NET displacement over a short rolling window ([windowMs]) implies a
 *    plausible average ground speed ([minMovingSpeedMps]..[maxPlausibleSpeedMps])
 *    across at least [minLegs] legs AND covers at least [minNetMeters] of ground.
 *    Using the window ENDPOINTS (net, not per-leg) makes an isolated GPS jitter, a
 *    duplicate, or a momentary stationary fix a no-op that simply ages out rather
 *    than resetting the whole run-up, so ordinary GPS noise can't stall a moving
 *    start. Wander around a point still nets ~0 (below the floor) and a teleport
 *    still reads implausibly fast, so neither false-fires.
 *    Seeds with the whole window, oldest first.
 *  - precise AND moving: a fix worse than [preciseAccuracyM] neither seeds nor
 *    extends the window, so the seed is entirely precise sustained movement.
 */
internal class ArmTrigger(
    private val requirePrecise: Boolean,
    private val requireMoving: Boolean,
    private val preciseAccuracyM: Double = GPS_PRECISE_ACCURACY_M,
    private val minMovingSpeedMps: Double = MIN_MOVING_SPEED_MPS,
    private val maxPlausibleSpeedMps: Double = MAX_PLAUSIBLE_SPEED_MPS,
    private val minLegs: Int = MOVING_ARM_MIN_LEGS,
    private val minNetMeters: Double = MOVING_ARM_MIN_NET_M,
    private val windowMs: Long = ARM_MOVING_WINDOW_MS,
) {
    // The rolling run-up window (fix + its monotonic nanos), oldest first. Only the
    // moving gate uses it; empty otherwise.
    private val window = ArrayList<Pair<GpsPoint, Long>>()

    fun offer(point: GpsPoint, fixElapsedNanos: Long): List<Pair<GpsPoint, Long>>? {
        // Precise gate: a fix worse than the threshold can't anchor a precise start.
        // Under the moving gate it also BREAKS the run-up window (the seed must be
        // wholly precise); otherwise it is simply ignored until a precise fix lands.
        if (requirePrecise && point.accuracyM > preciseAccuracyM) {
            if (requireMoving) window.clear()
            return null
        }
        if (!requireMoving) {
            // No movement requirement: this fix already cleared the precise gate (or
            // there is none), so it seeds the run immediately.
            return listOf(point to fixElapsedNanos)
        }
        window.add(point to fixElapsedNanos)
        // Age out fixes older than the run-up window. Dropping stale fixes by TIME —
        // never resetting on a single bad leg — is what lets an isolated jitter or
        // duplicate fix pass through instead of nuking the whole run-up.
        val cutoffNanos = fixElapsedNanos - windowMs * 1_000_000L
        while (window.size > 1 && window.first().second < cutoffNanos) {
            window.removeAt(0)
        }
        // Need at least [minLegs] legs (minLegs + 1 fixes) of window context.
        if (window.size <= minLegs) return null
        val anchor = window.first()
        val dtSec = (fixElapsedNanos - anchor.second) / 1_000_000_000.0
        if (dtSec <= 0.0) return null
        // NET displacement from the window anchor: genuine movement covers real ground
        // (>= [minNetMeters], so GPS wander around a point can't qualify) at a
        // plausible average speed (>= floor rejects standing wander; <= ceiling
        // rejects a teleport). A bad fix in the MIDDLE of the window doesn't move the
        // endpoints, so it can never stall the start.
        val netMeters = segmentKm(anchor.first, point) * 1000.0
        val avgSpeedMps = netMeters / dtSec
        return if (netMeters >= minNetMeters &&
            avgSpeedMps >= minMovingSpeedMps &&
            avgSpeedMps <= maxPlausibleSpeedMps
        ) {
            window.toList()
        } else {
            null
        }
    }
}

/** The tracker map pill's state, highest-priority first (see [pillState]). */
enum class TrackerPill { PermissionNeeded, LocationOff, SignalLost, Paused, Recording, ArmedMoving, ArmedPrecise, ArmedWaiting, FreshLock, Acquiring }

/**
 * The map pill's state as a strict priority ladder. A missing precise-location grant
 * or a disabled system-location toggle outranks everything: the pill must never read
 * "Acquiring GPS…" while it physically cannot get a fix, which would contradict the
 * sheet text below it. Then, while recording, a lost signal or a pause outranks the
 * "Recording" default; when merely armed the gate-aware waiting text outranks a fresh
 * lock; and a fresh lock outranks the acquiring hint. Extracted pure so the ladder is
 * unit-testable independently of Compose.
 */
internal fun pillState(
    hasPermission: Boolean,
    locationOn: Boolean,
    tracking: Boolean,
    paused: Boolean,
    armed: Boolean,
    signalLost: Boolean,
    freshLock: Boolean,
    requirePrecise: Boolean,
    requireMoving: Boolean,
): TrackerPill = when {
    !hasPermission -> TrackerPill.PermissionNeeded
    !locationOn -> TrackerPill.LocationOff
    tracking && signalLost -> TrackerPill.SignalLost
    tracking && paused -> TrackerPill.Paused
    tracking -> TrackerPill.Recording
    armed && requireMoving -> TrackerPill.ArmedMoving
    armed && requirePrecise -> TrackerPill.ArmedPrecise
    armed -> TrackerPill.ArmedWaiting
    freshLock -> TrackerPill.FreshLock
    else -> TrackerPill.Acquiring
}

/** Cap on the number of GPS fixes a saved run keeps in one `LogRunTrack` line.
 *  A multi-hour 1 Hz run is thousands of points, the single heaviest thing the
 *  append-only event log stores; [RunSession.decimatedTrackForCore] thins beyond
 *  this to a ~2–3 s cadence, bounding log growth while keeping the endpoints +
 *  every pause boundary so the core's of-record figures barely move (see the
 *  running.rs decimation tolerance tests). 5400 ≈ 90 min at a full 1 Hz cadence,
 *  or ~4.5 h once thinned to 3 s, ordinary runs keep near-full detail; only very
 *  long runs get decimated at all. */
const val TRACK_DECIMATION_CAP = 5400

/** Initial capacity of the append-only GPS backing array ([RunSession]'s growable
 *  store). ~4 min at 1 Hz; it doubles on overflow (amortised O(1) append), so the
 *  starting size only trades a little idle memory against the first few reallocs. */
private const val INITIAL_TRACK_CAP = 256

/** Pace-bucket size (minutes) the accumulator starts with before the live sheet
 *  pushes the user's preference through [RunSession.setPaceBucketMinutes]. Equals
 *  AppSettings' default, so the default preference needs no rebuild. */
private const val DEFAULT_PACE_BUCKET_MIN = 4

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
