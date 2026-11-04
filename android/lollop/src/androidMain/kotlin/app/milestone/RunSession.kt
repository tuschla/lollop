package app.milestone

import android.os.SystemClock
import java.util.Locale
import java.util.TreeSet
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
     *  plain `List<GpsPoint>` so consumers (map draw, save, recovery, tests) need no
     *  knowledge of the snapshot type. */
    val points = MutableStateFlow<List<GpsPoint>>(TrackSnapshot(pointArr, 0))

    /** Whether [points] is non-empty. Flips only on empty↔non-empty, so the screen
     *  observes it without recomposing per fix. */
    val hasPoints = MutableStateFlow(false)
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

    // A crash-recovered run the user chose to Resume while precise location was not
    // granted (revoked in Settings, or an "Only this time" grant that lapsed with the
    // process). Its fixes are restored but nothing records: the service cannot start
    // a location FGS without the grant. The tracker asks for it and, the moment it is
    // granted, re-issues the recording start ([RunTrackingService.start]); until then
    // the run is a capture the runner can still save or discard. Cleared by [reset]
    // and by the tracker once it restarts recording.
    val resumeAwaitingLocation = MutableStateFlow(false)

    // Monotonic (elapsedRealtime) stamp of the last ACCEPTED fix, set by the service on
    // every plausible fix (preview/armed fixes never reach [add], so it cannot live
    // there). Drives [isFreshLock]'s freshness gate and the signal-loss pill: a lock is
    // "fresh" only for [LOCK_FRESH_MS] after its fix, so a single stale fix cannot
    // latch Start/GPS-lock forever. 0 = no fix.
    val lastFixElapsedMs = MutableStateFlow(0L)

    // Session token, bumped by [reset]. The service captures it when a stream starts
    // and stands down once it differs: Cancel/Back/discard runs `stop()` (async
    // ACTION_STOP) then `reset()`, and a fix already queued on the main looper must
    // not record into the cleared session. @Volatile for the service's main-thread
    // read; written only under reset()'s lock.
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
    private var bucketMin = DEFAULT_PACE_BUCKET_MINUTES
    private val completedBuckets = ArrayList<PaceBucket>()
    private var bucketCurSec = 0.0
    private var bucketCurKm = 0.0

    /** The CURRENT pace (canonical min/km) over the trailing [RollingPace] window
     *  (≥[ROLLING_PACE_MIN_SPAN_MS] and ≥[ROLLING_PACE_MIN_METERS] of recent legs,
     *  capped at [ROLLING_PACE_MAX_SPAN_MS]), so a surge, hill or rep shows up within
     *  one window instead of being averaged away over the whole run.
     *  Non-finite = not enough recent ground covered (standing still, a fresh
     *  segment) → rendered [EmptyValuePlaceholder]. Standing-still legs keep their time
     *  in the window, so stopping drives it to the placeholder rather than freezing
     *  the last running pace. */
    val currentPaceMinPerKm = MutableStateFlow(Double.NaN)
    private val rollingPace = RollingPace()

    /** Today's planned run item the tracker follows, set by the screen when
     *  the runner taps Start with "Follow today's plan" on; null for a free run. Its
     *  [PrescriptionView.run_target] drives the on-screen targets and its
     *  [PrescriptionView.run_structure] the step guide. Cleared by [reset]. */
    val plannedRun = MutableStateFlow<PrescriptionView?>(null)

    /** The live step guide (null when the run has no structured steps). Driven on the
     *  main thread (the service's tick + fix callbacks, the screen's Skip step);
     *  [reset] clears it from the save's IO thread after the service stopped, hence
     *  @Volatile on [stepMachine]. */
    internal val stepProgress = MutableStateFlow<StepProgress?>(null)
    @Volatile private var stepMachine: RunStepMachine? = null

    /** Start the step guide for [plannedRun] at the moment recording begins. A no-op
     *  without a structured plan or when a guide is already running. */
    fun startSteps(nowMs: Long) {
        if (stepMachine != null) return
        val steps = plannedRun.value?.run_structure?.let { buildRunSteps(it) }.orEmpty()
        if (steps.isEmpty()) return
        val m = RunStepMachine(steps, nowMs, distanceKm.value * 1000.0)
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

    // Indices into `points` that begin a new recording segment: the first fix after
    // a pause or a long fix gap. The leg into such a fix is a pause bridge (the
    // runner may have relocated), so it counts neither live nor in the saved run.
    // Index 0 has no preceding leg and is never stored. Accessed under the lock.
    private val segmentStarts = TreeSet<Int>()

    // Set when the run pauses: the next accepted fix opens a new segment, so its
    // bridge leg back to the last pre-pause fix is dropped. Cleared by [add].
    @Volatile private var pendingBreak = false

    /** Pause or resume recording; the screen and the service both go through here.
     *  Pausing marks the segment boundary at once, so the first fix after Resume
     *  opens a new segment even when no fix was accepted while paused. */
    fun setPaused(p: Boolean) {
        if (p) pendingBreak = true
        paused.value = p
    }

    /** True when a fix arriving [dtMs] (monotonic) after [prev] opens a new segment:
     *  a pause boundary or a fix gap longer than [MAX_FIX_GAP_SEC]. [add] drops a
     *  fix with dtMs < 0 before this check. */
    private fun isBoundary(prev: GpsPoint?, dtMs: Long): Boolean {
        if (prev == null) return false
        if (pendingBreak) return true
        return dtMs > MAX_FIX_GAP_SEC * 1000L
    }

    // Synchronized so the read-modify-write of points + distance is atomic.
    // [fixElapsedRealtimeNanos] is the fix's own monotonic timestamp
    // (Location.getElapsedRealtimeNanos), the clock behind moving time and each
    // leg's dt, immune to wall-clock steps. Returns true iff the fix was appended,
    // so the crash sidecar mirrors only accepted fixes.
    @Synchronized
    fun add(p: GpsPoint, fixElapsedRealtimeNanos: Long): Boolean {
        val last = if (pointCount > 0) pointArr[pointCount - 1] else null
        val fixMs = fixElapsedRealtimeNanos / 1_000_000
        val dtMs = fixMs - lastElapsedMs
        // A fix older than the previous one (dt < 0) is dropped rather than appended
        // out of order, which would split a leg or drag the line backwards. The first
        // fix is exempt: its dt is the boot offset (lastElapsedMs starts at 0).
        if (last != null && dtMs < 0L) return false
        if (isBoundary(last, dtMs)) {
            // New segment: the bridge leg adds no distance, moving time or pace
            // slice, and the current-pace window restarts.
            segmentStarts.add(pointCount)
            rollingPace.clear()
        } else if (last != null && dtMs > 0L) {
            // The live figures apply the saved run's moving-leg rule; the core also
            // drops fixes under 2.5 m apart (`qc_track`), so the saved distance can
            // differ slightly. The Finish review shows the core's own figures.
            val legKm = movingLegKm(segmentKm(last, p), dtMs / 1000.0)
            if (legKm != null) {
                movingMs += dtMs
                distanceKm.value += legKm
                rollingPace.add(dtMs, legKm * 1000.0)
                feedPaceLeg(legKm, dtMs / 1000.0)
            } else {
                // Standing still: the time stays in the current-pace window with no
                // distance, so "Pace now" decays to [EmptyValuePlaceholder].
                rollingPace.add(dtMs, 0.0)
            }
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
        hasPoints.value = true
    }

    /** Fold one moving leg ([legKm], already capped by [movingLegKm], over [legSec])
     *  into the pace-slice accumulator, splitting it by time across a slice edge.
     *  Standing-still and pause-bridge legs never reach here. */
    private fun feedPaceLeg(legKm: Double, legSec: Double) {
        val n = bucketMin.coerceAtLeast(1)
        val bucketSec = n * 60.0
        var restSec = legSec
        var restKm = legKm
        while (bucketCurSec + restSec >= bucketSec) {
            val take = bucketSec - bucketCurSec
            val frac = if (restSec > 0.0) take / restSec else 0.0
            val kmChunk = restKm * frac
            bucketCurKm += kmChunk
            completedBuckets.add(PaceBucket(paceMinPerKm = n / bucketCurKm, minutes = n.toDouble(), complete = true))
            restSec -= take
            restKm -= kmChunk
            bucketCurSec = 0.0
            bucketCurKm = 0.0
        }
        bucketCurSec += restSec
        bucketCurKm += restKm
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
            // The stored fixes carry only wall-clock observedAt, so the leg dt comes
            // from that rather than the monotonic clock the live path uses.
            val dtSec = (b.observedAt - a.observedAt).toDouble()
            movingLegKm(segmentKm(a, b), dtSec)?.let { feedPaceLeg(it, dtSec) }
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

    /** Whether [p], just accepted by [add], opened a new recording segment (a
     *  Pause/Resume or a long fix gap), so the crash sidecar can record the
     *  boundary ([ActiveRunStore.append]). False when [p] is not the newest fix. */
    @Synchronized
    fun isSegmentStart(p: GpsPoint): Boolean =
        pointCount > 0 && pointArr[pointCount - 1] == p && segmentStarts.contains(pointCount - 1)

    /** [restore] a run read back from [ActiveRunStore], breaking it at the segment
     *  boundaries the live run recorded. */
    fun restore(recovered: RecoveredTrack) = restore(recovered, recovered.segmentStarts)

    /**
     * Repopulate an interrupted run recovered from [ActiveRunStore] and put the
     * session back into recording state. Segments break at [recordedStarts] (the
     * live run's own boundaries, Pause/Resume included) and at any fix-time gap
     * over [MAX_FIX_GAP_SEC]; distance and moving time are the same segment-aware
     * sums used live (a single O(n) pass, not per-fix), so a recovered run never
     * shows more than it had before the crash.
     */
    @Synchronized
    fun restore(recovered: List<GpsPoint>, recordedStarts: Collection<Int> = emptyList()) {
        segmentStarts.clear()
        // The splice from the last recovered fix to the first live fix is a segment
        // boundary: the app may have been dead for hours while the runner moved, yet
        // the monotonic gap [add] measures is only seconds, so [isBoundary]'s gap rule
        // would not fire.
        pendingBreak = true
        // Refill the append-only backing store from the recovered fixes (a FRESH
        // array so any snapshot published before recovery keeps its own backing).
        pointArr = arrayOfNulls(maxOf(INITIAL_TRACK_CAP, recovered.size))
        pointCount = 0
        for (pt in recovered) pointArr[pointCount++] = pt
        val recorded = recordedStarts as? Set<Int> ?: recordedStarts.toHashSet()
        var dist = 0.0
        var movingSec = 0.0
        for (i in 1 until recovered.size) {
            val dtSec = (recovered[i].observedAt - recovered[i - 1].observedAt).toDouble()
            if (i in recorded || dtSec > MAX_FIX_GAP_SEC) {
                segmentStarts.add(i)
            } else {
                val legKm = movingLegKm(segmentKm(recovered[i - 1], recovered[i]), dtSec)
                if (legKm != null) {
                    movingSec += dtSec
                    dist += legKm
                }
            }
        }
        points.value = TrackSnapshot(pointArr, pointCount)
        hasPoints.value = pointCount > 0
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
     * The track the save sends the core: the true coordinates, thinned to ~[maxPoints]
     * fixes by [decimationKeep] so one long run can't bloat the append-only log (the
     * endpoints and every segment start are kept). [decimatedSegmentStarts] with the
     * same cap gives the matching boundaries. Below the cap the track is whole.
     */
    @Synchronized
    fun decimatedTrackForCore(maxPoints: Int = TRACK_DECIMATION_CAP): List<GpsPoint> {
        val full = points.value
        return decimationKeep(full.size, maxPoints, segmentStarts).map { full[it] }
    }

    /** [segmentStarts] remapped to positions in [decimatedTrackForCore] (same
     *  [maxPoints]); sent as `LogRunTrack.segment_starts`. Empty for an unpaused run. */
    @Synchronized
    fun decimatedSegmentStarts(maxPoints: Int = TRACK_DECIMATION_CAP): List<Int> =
        remapStarts(decimationKeep(points.value.size, maxPoints, segmentStarts), segmentStarts)

    /**
     * The `LogRunTrack` that saves this run, built under ONE lock so its points and
     * boundaries agree. No HR sensor yet, so the shell sends no HR figure.
     * Dated at its last GPS fix, not "now": a crash-recovery save can run hours after
     * the run, and History and the load windows must land on when it happened (like
     * the GPX import). Falls back to now only for an empty track. The Finish review
     * previews this same event's figures ([Core.savedFigures]).
     */
    @Synchronized
    fun trackEventForCore(entryId: Long, workoutType: WorkoutType?, sessionRpe: Int?): Event.LogRunTrack =
        Event.LogRunTrack(
            points = decimatedTrackForCore(),
            hrPctMax = 0.0,
            observedAt = points.value.lastOrNull()?.observedAt ?: AppClock.nowSec(),
            entryId = entryId,
            workoutType = workoutType,
            segmentStarts = decimatedSegmentStarts(),
            sessionRpe = sessionRpe,
        )

    /** Reset only the engine-state flags ([tracking], [locating], [armed]) to stopped,
     *  keeping the captured track and the save/pause bookkeeping (unlike [reset]).
     *  The service's teardown and [RunTrackingService.stop]'s dead-service catch call
     *  this, so no stop leaves "Finding GPS…" latched with no engine behind it. */
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
        hasPoints.value = false
        tracking.value = false
        locating.value = false
        lastFix.value = null
        armed.value = false
        resumeAwaitingLocation.value = false
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
     * Immutable O(1) view over the append-only [pointArr] prefix `[0, len)`. Slots in
     * that prefix never change once published (reset/restore swap in a fresh array),
     * so the view needs no copy or lock. [equals] short-cuts on the same backing and
     * length, keeping StateFlow's per-fix dedup O(1); otherwise it compares
     * element-wise like any List.
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
            // Shortcut for the definitely-equal case only: a different backing falls
            // through to element-wise equality, so equals stays symmetric with plain
            // Lists (and consistent with AbstractList's hashCode).
            if (other is TrackSnapshot && backing === other.backing && len == other.len) return true
            return super.equals(other)
        }
    }
}

/**
 * One per-N-minute pace slice of [RunSession.paceBucketsFlow]. [paceMinPerKm] is
 * canonical minutes-per-km (convert to the display unit with `paceInUnit`; a
 * non-finite value means "no distance covered" → render [EmptyValuePlaceholder]).
 * [minutes] is the moving-time span of the slice, always the bucket size for a
 * [complete] slice, less for the trailing in-progress one.
 */
data class PaceBucket(
    val paceMinPerKm: Double,
    val minutes: Double,
    val complete: Boolean,
)

private const val ROLLING_PACE_MIN_SPAN_MS = 30_000L
private const val ROLLING_PACE_MIN_METERS = 200.0
private const val ROLLING_PACE_MAX_SPAN_MS = 60_000L
private const val ROLLING_PACE_MIN_SHOW_SPAN_MS = 10_000L
private const val ROLLING_PACE_MIN_SHOW_METERS = 20.0

/**
 * Trailing-window current pace over recent legs, O(1) amortised per leg (a deque of
 * (duration, distance) with running sums). The window is the shortest suffix spanning
 * at least [ROLLING_PACE_MIN_SPAN_MS] and [ROLLING_PACE_MIN_METERS] (so one noisy 1 s
 * leg can't swing it), never more than [ROLLING_PACE_MAX_SPAN_MS] (so standing still
 * empties it of distance and the pace reads [EmptyValuePlaceholder]). Below
 * [ROLLING_PACE_MIN_SHOW_SPAN_MS] / [ROLLING_PACE_MIN_SHOW_METERS] it reports NaN.
 */
internal class RollingPace {
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
            if (spanMs > ROLLING_PACE_MAX_SPAN_MS || (restSpan >= ROLLING_PACE_MIN_SPAN_MS && restM >= ROLLING_PACE_MIN_METERS)) {
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
        if (spanMs < ROLLING_PACE_MIN_SHOW_SPAN_MS || meters < ROLLING_PACE_MIN_SHOW_METERS) Double.NaN
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

/** The distance (km) a leg of [km] over [dtSec] adds to a run, or null when it is not
 *  a moving leg (no elapsed time, or slower than [MIN_MOVING_SPEED_MPS]). A moving
 *  leg keeps all its time; its distance is capped at [MAX_PLAUSIBLE_SPEED_MPS]·dt
 *  (GPS jitter). Shared by the live figures, the pace slices, crash recovery, the
 *  import gates and the trim preview, which test it against null for the moving rule. */
internal fun movingLegKm(km: Double, dtSec: Double): Double? {
    if (dtSec <= 0.0 || km * 1000.0 / dtSec < MIN_MOVING_SPEED_MPS) return null
    return minOf(km, MAX_PLAUSIBLE_SPEED_MPS * dtSec / 1000.0)
}

/** A gap longer than this (seconds) between consecutive accepted fixes opens a
 *  new recording segment: the straight-line bridge across it (a pause+relocation,
 *  a tunnel, a signal dropout) is not counted as run distance. Conservative, a
 *  normal ~1–2 s fix cadence never trips it, so only genuine pauses/losses break. */
internal const val MAX_FIX_GAP_SEC = 30L

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

/** Max age (ms, monotonic) of an armed or preview fix; see [armedFixPassesAgeGate]. */
internal const val ARMED_FIX_MAX_AGE_MS = 10_000L

/** The armed/preview staleness gate (never consulted while recording, which keeps every
 *  batched fix). A fix older than [ARMED_FIX_MAX_AGE_MS] is rejected only when it
 *  arrives alone: the fused provider's first callback is often a cached fix minutes
 *  old. A fix in a multi-fix batch is screen-off delivery and passes even when old,
 *  or the [ArmTrigger] window could never fill and 'Start on move' would never fire. */
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
 * True when [accuracyM] is a lock the tracker can start from: within
 * [MAX_GPS_ACCURACY_M] and fixed within [LOCK_FRESH_MS]. Accuracy alone would let one
 * good fix latch "locked" forever. [fixElapsedMs] == 0 means no fix yet → false; a
 * null accuracy → false.
 */
internal fun isFreshLock(accuracyM: Double?, fixElapsedMs: Long, nowElapsedMs: Long): Boolean {
    if (accuracyM == null || fixElapsedMs == 0L) return false
    val ageMs = nowElapsedMs - fixElapsedMs
    return accuracyM in 0.0..MAX_GPS_ACCURACY_M && ageMs in 0L..LOCK_FRESH_MS
}

/**
 * Decides when an armed run begins recording. [offer] takes each accepted preview fix
 * with its monotonic stamp and returns the seed fixes to open the run with (keeping a
 * moving start's run-up), or null to keep waiting. No gate: the first fix seeds.
 * Precise: the first fix within [GPS_PRECISE_ACCURACY_M]. Moving: the NET displacement
 * across the [ARM_MOVING_WINDOW_MS] window covers [MOVING_ARM_MIN_NET_M] at a plausible
 * average speed over [MOVING_ARM_MIN_LEGS] legs; using the endpoints lets a jitter or
 * duplicate fix mid-window age out instead of stalling the start. Under both gates a
 * coarse fix also clears the window, so the seed is wholly precise.
 */
internal class ArmTrigger(
    private val requirePrecise: Boolean,
    private val requireMoving: Boolean,
) {
    // The rolling run-up window (fix + its monotonic nanos), oldest first. Only the
    // moving gate uses it; empty otherwise.
    private val window = ArrayList<Pair<GpsPoint, Long>>()

    fun offer(point: GpsPoint, fixElapsedNanos: Long): List<Pair<GpsPoint, Long>>? {
        // Precise gate: a fix worse than the threshold can't anchor a precise start.
        // Under the moving gate it also BREAKS the run-up window (the seed must be
        // wholly precise); otherwise it is simply ignored until a precise fix lands.
        if (requirePrecise && point.accuracyM > GPS_PRECISE_ACCURACY_M) {
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
        val cutoffNanos = fixElapsedNanos - ARM_MOVING_WINDOW_MS * 1_000_000L
        while (window.size > 1 && window.first().second < cutoffNanos) {
            window.removeAt(0)
        }
        // Need at least MOVING_ARM_MIN_LEGS legs (one more fix) of window context.
        if (window.size <= MOVING_ARM_MIN_LEGS) return null
        val anchor = window.first()
        val dtSec = (fixElapsedNanos - anchor.second) / 1_000_000_000.0
        if (dtSec <= 0.0) return null
        // NET displacement from the window anchor: real ground (so wander around a
        // point can't qualify) at a plausible average speed (the floor rejects
        // standing wander, the ceiling a teleport).
        val netMeters = segmentKm(anchor.first, point) * 1000.0
        val avgSpeedMps = netMeters / dtSec
        return if (netMeters >= MOVING_ARM_MIN_NET_M &&
            avgSpeedMps >= MIN_MOVING_SPEED_MPS &&
            avgSpeedMps <= MAX_PLAUSIBLE_SPEED_MPS
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
 * "Finding GPS…" while it physically cannot get a fix, which would contradict the
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

/** Cap on the number of GPS fixes a saved run keeps in one `LogRunTrack` line (the
 *  heaviest thing the append-only log stores). 5400 ≈ 90 min at 1 Hz, so ordinary
 *  runs keep full detail and only very long runs are thinned ([decimationKeep]). */
const val TRACK_DECIMATION_CAP = 5400

/**
 * The indices (ascending) kept when thinning an [n]-fix track to ~[maxPoints]: a
 * uniform stride (ceil division, so the strided count stays within the cap) plus the
 * last fix and every segment start in [starts]. At or below the cap, every index.
 * Shared by the save ([RunSession.decimatedTrackForCore]), crash recovery
 * ([ActiveRunStore.thinForRecovery]) and the import ([importedRunEvent]).
 */
internal fun decimationKeep(n: Int, maxPoints: Int, starts: Collection<Int>): List<Int> {
    if (n <= maxPoints || maxPoints < 2) return (0 until n).toList()
    val keep = TreeSet<Int>()
    val stride = (n + maxPoints - 1) / maxPoints
    for (i in 0 until n step stride) keep.add(i)
    keep.add(n - 1)
    for (s in starts) if (s in 1 until n) keep.add(s)
    return keep.toList()
}

/** The positions in [kept] (original indices, ascending) of the segment [starts]. */
internal fun remapStarts(kept: List<Int>, starts: Set<Int>): List<Int> =
    kept.indices.filter { kept[it] in starts }

/** Initial capacity of the append-only GPS backing array ([RunSession]'s growable
 *  store). ~4 min at 1 Hz; it doubles on overflow (amortised O(1) append), so the
 *  starting size only trades a little idle memory against the first few reallocs. */
private const val INITIAL_TRACK_CAP = 256

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
