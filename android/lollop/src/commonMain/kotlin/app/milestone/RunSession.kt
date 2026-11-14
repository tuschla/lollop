package app.milestone

import kotlin.concurrent.Volatile
import kotlinx.atomicfu.locks.SynchronizedObject
import kotlinx.atomicfu.locks.synchronized
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow

/**
 * In-memory holder the platform's recorder ([RunRecorder]: Android's foreground
 * RunTrackingService, iOS's CoreLocation tracker) writes and the UI observes.
 * Living here rather than inside the Composable is what lets a run keep recording
 * while the screen is off or the app is backgrounded: the recorder owns the
 * location stream, the screen only renders this state.
 *
 * Every flow is read-only; state changes only through the commands below. The
 * recording-engine state (tracking, locating, armed, the preview fix, a failed
 * stream) is [RunRecorder]'s alone; the tracker screen drives the run's plan, its
 * save claim, pause and teardown. The state lives as long as the process
 * ([ProcessLocal]).
 */
object RunSession {
    private val session by ProcessLocal { Session() }

    /** The captured track, republished per accepted fix as an O(1) immutable view
     *  over the append-only backing. */
    val points: StateFlow<List<GpsPoint>> get() = session.points

    /** Whether [points] is non-empty. Flips only on empty↔non-empty, so the screen
     *  observes it without recomposing per fix. */
    val hasPoints: StateFlow<Boolean> get() = session.hasPoints
    val tracking: StateFlow<Boolean> get() = session.tracking

    // Locate-only preview: the service is acquiring GPS but NOT
    // recording yet. `locating` is true from opening the tracker until Stop/Back;
    // `lastFix` is the latest accepted fix (recording or not); it drives the
    // "GPS lock · ±N m" readout, the self-location dot before Start, and enables
    // the Start button once a good fix lands.
    val locating: StateFlow<Boolean> get() = session.locating
    val lastFix: StateFlow<GpsPoint?> get() = session.lastFix

    // Armed start: Start is tappable before any lock, and recording begins
    // AUTOMATICALLY at the first qualifying fix. [RunRecorder] sets this on the tap
    // ([RunRecorder.armRequested]) and on entering ARM mode, and clears it the instant
    // recording begins (or on teardown). A process singleton so the transition
    // survives rotation/backgrounding. [restore] forces it false, a recovered run is
    // already recording, never armed.
    val armed: StateFlow<Boolean> get() = session.armed

    /** A run is under way: recording, or armed to start at the first good fix. */
    val runActive: Boolean get() = session.tracking.value || session.armed.value

    // The location stream could not be started or died on start (no GPS provider on
    // the device, the fused client's async failure, a permission race). Set by the
    // recorder's failure unwind; the screen stops re-launching the locate preview while
    // it is set (otherwise each failure would immediately restart the service in a
    // tight start/stop loop) and clears it on ON_RESUME, Try again or Start.
    val locationFailed: StateFlow<Boolean> get() = session.locationFailed

    // A crash-recovered run the user chose to Resume while precise location was not
    // granted (revoked in Settings, or an "Only this time" grant that lapsed with the
    // process). Its fixes are restored but nothing records: the service cannot start
    // a location FGS without the grant. The tracker asks for it and, the moment it is
    // granted, re-issues the recording start ([RunTrackingService.start]); until then
    // the run is a capture the runner can still save or discard. Cleared by [reset]
    // and by the tracker once it restarts recording.
    val resumeAwaitingLocation: StateFlow<Boolean> get() = session.resumeAwaitingLocation

    // Monotonic (elapsedRealtime) stamp of the last ACCEPTED fix, set by the recorder on
    // every plausible fix (preview/armed fixes never reach [add], so it cannot live
    // there). Drives [isFreshLock]'s freshness gate and the signal-loss pill: a lock is
    // "fresh" only for [LOCK_FRESH_MS] after its fix, so a single stale fix cannot
    // latch Start/GPS-lock forever. 0 = no fix.
    val lastFixElapsedMs: StateFlow<Long> get() = session.lastFixElapsedMs

    // Session token, bumped by [reset]. The recorder captures it when a stream starts
    // and stands down once it differs: Cancel/Back/discard runs `stop()` (async
    // ACTION_STOP) then `reset()`, and a fix already queued on the main looper must
    // not record into the cleared session.
    val generation: Long get() = session.generation

    // Paused: the service keeps running (the run stays live/reentrant) but
    // incoming fixes are dropped, so the paused span records no route. The core's
    // of-record duration/pace/splits are MOVING time (legs below the auto-pause
    // floor are excluded), so a stop trims both the path and the pace clock, a
    // paused run is not penalised as if it were slow.
    val paused: StateFlow<Boolean> get() = session.paused

    // A run save is in flight; gates Save/Discard/Stop. Process-scoped so a
    // rotation can neither resurrect it (double-save) nor orphan it true (stuck
    // buttons after a failed save). Claimed by [claimSaving]; released by [reset]
    // (inside the save critical section, after the persist landed) and by the
    // save-failure path ([releaseSaving]).
    val saving: StateFlow<Boolean> get() = session.saving

    // Running path length (km); equals a full segment-aware haversine over `points`.
    val distanceKm: StateFlow<Double> get() = session.distanceKm

    // Live elapsed MOVING time (seconds): the accumulated duration of MOVING legs
    // only, pause-bridge legs AND sub-[CoreConstants.MIN_MOVING_SPEED_MPS] standing-still legs are
    // excluded, exactly as the core's of-record `moving_duration_min` excludes them,
    // so this live figure matches the saved run's moving-time duration/pace instead of
    // freezing at a pause then JUMPING the pause span back in on resume. Driven by
    // each fix's own MONOTONIC per-leg dt (elapsedRealtimeNanos), NOT wall-clock
    // `observedAt`, so a clock change / NTP correction can never make it jump or go
    // negative. The OF-RECORD duration is still core-derived from observedAt on save.
    val elapsedSec: StateFlow<Long> get() = session.elapsedSec

    /** Per-N-minute pace slices. [setPaceBucketMinutes] rebuilds them from the stored
     *  track for a new slice size, as [restore] does for a recovered run. */
    val paceBucketsFlow: StateFlow<List<PaceBucket>> get() = session.paceBucketsFlow

    /** The CURRENT pace (canonical min/km) over the trailing [RollingPace] window
     *  (≥[ROLLING_PACE_MIN_SPAN_MS] and ≥[ROLLING_PACE_MIN_METERS] of recent legs,
     *  capped at [ROLLING_PACE_MAX_SPAN_MS]), so a surge, hill or rep shows up within
     *  one window instead of being averaged away over the whole run.
     *  Non-finite = not enough recent ground covered (standing still, a fresh
     *  segment) → rendered [EmptyValuePlaceholder]. Standing-still legs keep their time
     *  in the window, so stopping drives it to the placeholder rather than freezing
     *  the last running pace. */
    val currentPaceMinPerKm: StateFlow<Double> get() = session.currentPaceMinPerKm

    /** Today's planned run item the tracker follows ([followPlan]); null for a free
     *  run. Its [PrescriptionView.run_target] drives the on-screen targets and its
     *  [PrescriptionView.run_structure] the step guide. Cleared by [reset]. */
    val plannedRun: StateFlow<PrescriptionView?> get() = session.plannedRun

    /** The live step guide (null when the run has no structured steps). Driven on the
     *  main thread (the recorder's tick + fix callbacks, the screen's Skip step). */
    internal val stepProgress: StateFlow<StepProgress?> get() = session.stepProgress

    // [RunRecorder] is the only caller of these recorder commands in the app.

    /** The location stream is up: locating, and [tracking] iff it records. */
    internal fun streamStarted(recording: Boolean) {
        val s = session
        s.locating.value = true
        s.tracking.value = recording
    }

    /** An armed stream began recording. */
    internal fun recordingStarted() {
        session.tracking.value = true
    }

    internal fun setArmed(armed: Boolean) {
        session.armed.value = armed
    }

    /** The latest accepted fix (recording, armed or locate-only) at its own
     *  monotonic stamp [elapsedMs]. */
    internal fun previewFix(fix: GpsPoint, elapsedMs: Long) {
        val s = session
        s.lastFix.value = fix
        s.lastFixElapsedMs.value = elapsedMs
    }

    internal fun markLocationFailed() {
        session.locationFailed.value = true
    }

    /** Reset only the engine-state flags ([tracking], [locating], [armed]) to stopped,
     *  keeping the captured track and the save/pause bookkeeping (unlike [reset]), so
     *  no stop leaves "Finding GPS…" latched with no engine behind it. */
    fun markStopped() {
        val s = session
        s.tracking.value = false
        s.locating.value = false
        s.armed.value = false
    }

    /** The runner tapped Start with "Follow today's plan" on ([item]) or off (null). */
    fun followPlan(item: PrescriptionView?) {
        session.plannedRun.value = item
    }

    /** Back from settings, Try again or Start: a fresh locate attempt may run. */
    fun clearLocationFailed() {
        session.locationFailed.value = false
    }

    /** A recovered run is resuming without a precise-location grant: it shows as a
     *  stopped capture until the grant lets recording restart. */
    fun awaitLocationForResume() {
        val s = session
        s.tracking.value = false
        s.resumeAwaitingLocation.value = true
    }

    /** Recording restarted for a recovered run. */
    fun clearResumeAwaitingLocation() {
        session.resumeAwaitingLocation.value = false
    }

    /** Take the save claim ([claimSaving]); false when another save holds it. */
    fun claimSaving(ownsSaving: Boolean = false): Boolean = app.milestone.claimSaving(session.saving, ownsSaving)

    /** A save failed or was abandoned before it persisted: Save/Discard work again. */
    fun releaseSaving() {
        session.saving.value = false
    }

    /** Pause or resume recording; the screen and the recorder both go through here.
     *  Pausing marks the segment boundary at once, so the first fix after Resume
     *  opens a new segment even when no fix was accepted while paused. */
    fun setPaused(p: Boolean) = session.setPaused(p)

    /** Start the step guide for [plannedRun] at the moment recording begins. A no-op
     *  without a structured plan or when a guide is already running. */
    fun startSteps(nowMs: Long) = session.startSteps(nowMs)

    /** Advance the guide to [nowMs]; true when the step changed (the recorder cues). */
    fun tickSteps(nowMs: Long): Boolean = session.tickSteps(nowMs)

    fun skipStep(nowMs: Long) = session.skipStep(nowMs)

    /** Append one fix. [fixElapsedRealtimeNanos] is the fix's own monotonic timestamp
     *  (Location.getElapsedRealtimeNanos), the clock behind moving time and each leg's
     *  dt, immune to wall-clock steps. Returns true iff the fix was appended, so the
     *  crash sidecar mirrors only accepted fixes. */
    fun add(p: GpsPoint, fixElapsedRealtimeNanos: Long): Boolean = session.add(p, fixElapsedRealtimeNanos)

    /** Set the live pace-bucket size (minutes). A no-op when unchanged; otherwise
     *  rebuilds the incremental accumulator from the backing store so the exposed
     *  [paceBucketsFlow] matches the new size. Called from the live sheet whenever
     *  the user's `paceBucketMinutes` preference changes. */
    fun setPaceBucketMinutes(minutes: Int) = session.setPaceBucketMinutes(minutes)

    /** Whether [p], just accepted by [add], opened a new recording segment (a
     *  Pause/Resume or a long fix gap), so the crash sidecar can record the
     *  boundary ([ActiveRunStore.append]). False when [p] is not the newest fix. */
    fun isSegmentStart(p: GpsPoint): Boolean = session.isSegmentStart(p)

    /** [restore] a run read back from [ActiveRunStore], breaking it at the segment
     *  boundaries the live run recorded. */
    fun restore(recovered: RecoveredTrack) = restore(recovered, recovered.segmentStarts)

    /**
     * Repopulate an interrupted run recovered from [ActiveRunStore] and put the
     * session back into recording state. Segments break at [recordedStarts] (the
     * live run's own boundaries, Pause/Resume included) and at any fix-time gap
     * over [CoreConstants.MAX_FIX_GAP_SEC]; distance and moving time are the same segment-aware
     * sums used live (a single O(n) pass, not per-fix), so a recovered run never
     * shows more than it had before the crash.
     */
    fun restore(recovered: List<GpsPoint>, recordedStarts: Collection<Int> = emptyList()): Unit =
        session.restore(recovered, recordedStarts)

    /**
     * The track the save sends the core: the true coordinates, thinned to ~[maxPoints]
     * fixes by the core ([thinTrack]) so one long run can't bloat the append-only log
     * (the endpoints and every segment start are kept). [decimatedSegmentStarts] with
     * the same cap gives the matching boundaries. Below the cap the track is whole.
     */
    fun decimatedTrackForCore(maxPoints: Int = CoreConstants.TRACK_DECIMATION_CAP): List<GpsPoint> =
        session.decimated(maxPoints).first

    /** The segment starts remapped to positions in [decimatedTrackForCore] (same
     *  [maxPoints]); sent as `LogRunTrack.segment_starts`. Empty for an unpaused run. */
    fun decimatedSegmentStarts(maxPoints: Int = CoreConstants.TRACK_DECIMATION_CAP): List<Int> =
        session.decimated(maxPoints).second

    /**
     * The `LogRunTrack` that saves this run, built under ONE lock so its points and
     * boundaries agree. No HR sensor yet, so the shell sends no HR figure.
     * Dated at its last GPS fix, not "now": a crash-recovery save can run hours after
     * the run, and History and the load windows must land on when it happened (like
     * the GPX import). Falls back to now only for an empty track. The Finish review
     * previews this same event's figures ([DurableCore.savedFigures]).
     */
    fun trackEventForCore(entryId: Long, workoutType: WorkoutType?, sessionRpe: Int?): Event.LogRunTrack =
        session.trackEventForCore(entryId, workoutType, sessionRpe)

    /** End the session: the track, every flag and the guide cleared, the
     *  [generation] advanced. */
    fun reset() = session.reset()

    /** Live counters that disagree with the track (a live leg the core's QC drops):
     *  the Finish review must show the core's figures instead. Tests only. */
    internal fun overrideLiveFiguresForTest(distanceKm: Double, elapsedSec: Long) {
        val s = session
        s.distanceKm.value = distanceKm
        s.elapsedSec.value = elapsedSec
    }

    private class Session : SynchronizedObject() {
        // Append-only: slots below pointCount never change, and reset/restore/grow
        // allocate a fresh array, so a published [TrackSnapshot] stays immutable
        // without a copy. Single writer (the GPS callback, under the session lock);
        // main-thread readers touch only the published [TrackSnapshot], never these fields.
        private var pointArr: Array<GpsPoint?> = arrayOfNulls(INITIAL_TRACK_CAP)
        private var pointCount = 0

        val points = MutableStateFlow<List<GpsPoint>>(TrackSnapshot(pointArr, 0))
        val hasPoints = MutableStateFlow(false)
        val tracking = MutableStateFlow(false)
        val locating = MutableStateFlow(false)
        val lastFix = MutableStateFlow<GpsPoint?>(null)
        val armed = MutableStateFlow(false)
        val locationFailed = MutableStateFlow(false)
        val resumeAwaitingLocation = MutableStateFlow(false)
        val lastFixElapsedMs = MutableStateFlow(0L)

        // @Volatile for the recorder's main-thread read; written only under reset()'s lock.
        @Volatile
        var generation: Long = 0L
            private set

        val paused = MutableStateFlow(false)
        val saving = MutableStateFlow(false)
        val distanceKm = MutableStateFlow(0.0)
        val elapsedSec = MutableStateFlow(0L)

        // Monotonic bookkeeping (ms, elapsedRealtime timebase). [movingMs] accumulates the
        // per-leg dt of MOVING legs only (the live moving-time clock behind [elapsedSec]);
        // [lastElapsedMs] is the most recent fix's monotonic stamp, the base for each
        // per-leg dt (segment-gap + speed jitter/stop-floor checks). Both monotonic, so
        // neither depends on wall-clock timestamps that an NTP step could move.
        private var movingMs: Long = 0L
        private var lastElapsedMs: Long = 0L

        val paceBucketsFlow = MutableStateFlow<List<PaceBucket>>(emptyList())
        private var bucketMin = DEFAULT_PACE_BUCKET_MINUTES
        private val completedBuckets = ArrayList<PaceBucket>()
        private var bucketCurSec = 0.0
        private var bucketCurKm = 0.0

        val currentPaceMinPerKm = MutableStateFlow(Double.NaN)
        private val rollingPace = RollingPace()

        val plannedRun = MutableStateFlow<PrescriptionView?>(null)

        // [reset] clears the guide from the save's IO thread after the service
        // stopped, hence @Volatile on [stepMachine].
        val stepProgress = MutableStateFlow<StepProgress?>(null)
        @Volatile private var stepMachine: RunStepMachine? = null

        fun startSteps(nowMs: Long) {
            if (stepMachine != null) return
            val steps = plannedRun.value?.run_structure?.let { buildRunSteps(it) }.orEmpty()
            if (steps.isEmpty()) return
            val m = RunStepMachine(steps, nowMs, distanceKm.value * 1000.0)
            stepMachine = m
            stepProgress.value = m.progress()
        }

        fun tickSteps(nowMs: Long): Boolean {
            val m = stepMachine ?: return false
            val changed = m.advance(nowMs, distanceKm.value * 1000.0, paused.value)
            stepProgress.value = m.progress()
            return changed
        }

        fun skipStep(nowMs: Long) {
            val m = stepMachine ?: return
            m.skip(nowMs, distanceKm.value * 1000.0, paused.value)
            stepProgress.value = m.progress()
        }

        // Indices into `points` that begin a new recording segment: the first fix after
        // a pause or a long fix gap. The leg into such a fix is a pause bridge (the
        // runner may have relocated), so it counts neither live nor in the saved run.
        // Index 0 has no preceding leg and is never stored. Accessed under the lock.
        private val segmentStarts = HashSet<Int>()

        // Set when the run pauses: the next accepted fix opens a new segment, so its
        // bridge leg back to the last pre-pause fix is dropped.
        @Volatile private var pendingBreak = false

        fun setPaused(p: Boolean) {
            if (p) pendingBreak = true
            paused.value = p
        }

        /** [dtMs] is monotonic; [add] has already dropped a fix with dtMs < 0. */
        private fun isBoundary(prev: GpsPoint?, dtMs: Long): Boolean {
            if (prev == null) return false
            if (pendingBreak) return true
            return dtMs > CoreConstants.MAX_FIX_GAP_SEC * 1000L
        }

        fun add(p: GpsPoint, fixElapsedRealtimeNanos: Long): Boolean = synchronized(this) {
            val last = if (pointCount > 0) pointArr[pointCount - 1] else null
            val fixMs = fixElapsedRealtimeNanos / 1_000_000
            val dtMs = fixMs - lastElapsedMs
            // A fix older than the previous one (dt < 0) is dropped rather than appended
            // out of order, which would split a leg or drag the line backwards. The first
            // fix is exempt: its dt is the boot offset (lastElapsedMs starts at 0).
            if (last != null && dtMs < 0L) return false
            if (isBoundary(last, dtMs)) {
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

        /** A fresh list each call is the publication barrier that hands the GPS-thread
         *  accumulator safely to the main-thread observer. */
        private fun emitPaceBuckets() {
            val out = ArrayList<PaceBucket>(completedBuckets.size + 1)
            out.addAll(completedBuckets)
            if (bucketCurSec > 0.0) {
                val minutes = bucketCurSec / 60.0
                out.add(PaceBucket(paceMinPerKm = minutes / bucketCurKm, minutes = minutes, complete = false))
            }
            paceBucketsFlow.value = out
        }

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

        fun setPaceBucketMinutes(minutes: Int): Unit = synchronized(this) {
            val n = minutes.coerceAtLeast(1)
            if (n == bucketMin) return
            bucketMin = n
            rebuildPaceBuckets()
        }

        fun isSegmentStart(p: GpsPoint): Boolean = synchronized(this) {
            pointCount > 0 && pointArr[pointCount - 1] == p && segmentStarts.contains(pointCount - 1)
        }

        fun restore(recovered: List<GpsPoint>, recordedStarts: Collection<Int>): Unit = synchronized(this) {
            segmentStarts.clear()
            // The splice from the last recovered fix to the first live fix is a segment
            // boundary: the app may have been dead for hours while the runner moved, yet
            // the monotonic gap [add] measures is only seconds, so [isBoundary]'s gap rule
            // would not fire.
            pendingBreak = true
            pointArr = arrayOfNulls(maxOf(INITIAL_TRACK_CAP, recovered.size))
            pointCount = 0
            for (pt in recovered) pointArr[pointCount++] = pt
            val recorded = recordedStarts as? Set<Int> ?: recordedStarts.toHashSet()
            var dist = 0.0
            var movingSec = 0.0
            for (i in 1 until recovered.size) {
                val dtSec = (recovered[i].observedAt - recovered[i - 1].observedAt).toDouble()
                if (i in recorded || dtSec > CoreConstants.MAX_FIX_GAP_SEC) {
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
            lastElapsedMs = AppClock.monotonicMillis()
            elapsedSec.value = (movingMs / 1000L).coerceAtLeast(0L)
            rollingPace.clear()
            currentPaceMinPerKm.value = Double.NaN
            paused.value = false
            tracking.value = true
            // A recovered run is already recording, never armed: force the arm flag off so
            // a crash while merely armed can't leave the resumed screen stuck armed.
            armed.value = false
            lastFixElapsedMs.value = 0L
            rebuildPaceBuckets()
        }

        fun decimated(maxPoints: Int): Pair<List<GpsPoint>, List<Int>> = synchronized(this) {
            thinTrack(points.value, maxPoints, segmentStarts)
        }

        fun trackEventForCore(entryId: Long, workoutType: WorkoutType?, sessionRpe: Int?): Event.LogRunTrack = synchronized(this) {
            val (track, starts) = decimated(CoreConstants.TRACK_DECIMATION_CAP)
            val observedAt = points.value.lastOrNull()?.observedAt ?: AppClock.nowSec()
            Event.LogRunTrack(
                points = track,
                hrPctMax = 0.0,
                observedAt = observedAt,
                entryId = entryId,
                workoutType = workoutType,
                segmentStarts = starts,
                sessionRpe = sessionRpe,
                utcOffsetSec = utcOffsetSec(observedAt),
            )
        }

        fun reset(): Unit = synchronized(this) {
            // Bump the session token FIRST so a location result already queued on the main
            // looper (from the service that is about to receive ACTION_STOP) observes a
            // newer generation and refuses to record into this now-cleared session, instead
            // of resurrecting a ghost run + sidecar after the clear/reset below.
            generation++
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
            // [bucketMin] stays: the user's chosen slice size persists across runs.
            completedBuckets.clear()
            bucketCurSec = 0.0
            bucketCurKm = 0.0
            paceBucketsFlow.value = emptyList()
            rollingPace.clear()
            currentPaceMinPerKm.value = Double.NaN
            plannedRun.value = null
            stepMachine = null
            stepProgress.value = null
        }
    }

    /**
     * Immutable O(1) view over the append-only backing prefix `[0, len)`. Slots in
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
 * Trailing-window current pace over recent legs. The window is the shortest suffix spanning
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
     *  holds too little time or ground for a pace. */
    fun paceMinPerKm(): Double =
        if (spanMs < ROLLING_PACE_MIN_SHOW_SPAN_MS || meters < ROLLING_PACE_MIN_SHOW_METERS) Double.NaN
        else (spanMs / 60_000.0) / (meters / 1000.0)
}

// Single-sourced here so the foreground service (live map + distance), the UI lock
// pill, and [isFreshLock] can never drift apart. A live fix counts only within the
// core's accuracy bound ([CoreConstants.MAX_GPS_ACCURACY_M]), so live distance and
// pace cannot exceed the saved run's; the precise-start gate
// ([GPS_PRECISE_ACCURACY_M]) is stricter still.

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
 *  it, and below [CoreConstants.MAX_FIX_GAP_SEC] (the segment-break gap) so the runner is
 *  warned BEFORE the track actually breaks a segment. */
internal const val SIGNAL_LOSS_MS = 15_000L

/** True once a RECORDING run has gone longer than [SIGNAL_LOSS_MS] without an
 *  accepted fix. Shared by the in-app pill and the service's screen-off watchdog so
 *  the two can never disagree. [lastFixElapsedMs] == 0 means no fix yet → false
 *  (nothing has been lost). Both stamps are monotonic elapsedRealtime. */
internal fun signalLossDue(lastFixElapsedMs: Long, nowElapsedMs: Long): Boolean =
    lastFixElapsedMs > 0L && nowElapsedMs - lastFixElapsedMs > SIGNAL_LOSS_MS

/** Consecutive MOVING legs the "Start on move" gate needs before it fires: enough to
 *  tell a real run-up (at a ~1 Hz cadence) from a single jitter step without a long
 *  stall at the line. */
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
 * [CoreConstants.MAX_GPS_ACCURACY_M] and fixed within [LOCK_FRESH_MS]. Accuracy alone would let one
 * good fix latch "locked" forever. [fixElapsedMs] == 0 means no fix yet → false; a
 * null accuracy → false.
 */
internal fun isFreshLock(accuracyM: Double?, fixElapsedMs: Long, nowElapsedMs: Long): Boolean {
    if (accuracyM == null || fixElapsedMs == 0L) return false
    val ageMs = nowElapsedMs - fixElapsedMs
    return accuracyM in 0.0..CoreConstants.MAX_GPS_ACCURACY_M && ageMs in 0L..LOCK_FRESH_MS
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
        if (requirePrecise && point.accuracyM > GPS_PRECISE_ACCURACY_M) {
            if (requireMoving) window.clear()
            return null
        }
        if (!requireMoving) {
            return listOf(point to fixElapsedNanos)
        }
        window.add(point to fixElapsedNanos)
        val cutoffNanos = fixElapsedNanos - ARM_MOVING_WINDOW_MS * 1_000_000L
        while (window.size > 1 && window.first().second < cutoffNanos) {
            window.removeAt(0)
        }
        if (window.size <= MOVING_ARM_MIN_LEGS) return null
        val anchor = window.first()
        val dtSec = (fixElapsedNanos - anchor.second) / 1_000_000_000.0
        if (dtSec <= 0.0) return null
        val netMeters = segmentKm(anchor.first, point) * 1000.0
        val avgSpeedMps = netMeters / dtSec
        return if (netMeters >= MOVING_ARM_MIN_NET_M &&
            avgSpeedMps >= CoreConstants.MIN_MOVING_SPEED_MPS &&
            avgSpeedMps <= CoreConstants.MAX_PLAUSIBLE_SPEED_MPS
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
 * sheet text below it.
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

/**
 * [pts] thinned to ~[maxPoints] fixes by the core's save rule (`ThinTrack` op): a
 * uniform stride plus the last fix and every segment start in [starts], with the
 * starts remapped to positions in the thinned track. Shared by the save
 * ([RunSession.decimatedTrackForCore]) and crash recovery
 * ([ActiveRunStore.thinForRecovery]); [CoreConstants.TRACK_DECIMATION_CAP] is the cap.
 * A core that can't answer thins nothing: every fix, the starts as recorded.
 */
internal fun thinTrack(pts: List<GpsPoint>, maxPoints: Int, starts: Collection<Int>): Pair<List<GpsPoint>, List<Int>> {
    val thin = AppCore.engine.thinTrack(TrackThinning(pts.size, maxPoints, starts.toList()))
        ?: return pts to starts.sorted()
    val kept = thin.kept
    return (if (kept.size == pts.size) pts else kept.map(pts::get)) to thin.segment_starts
}

/** Initial capacity of the append-only GPS backing array: ~4 min at 1 Hz. */
private const val INITIAL_TRACK_CAP = 256
