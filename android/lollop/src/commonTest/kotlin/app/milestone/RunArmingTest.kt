package app.milestone

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class RunArmingTest {

    // ~1 m of longitude at the equator, for building legs of a known ground speed.
    private val degPerM = 1.0 / 111_320.0
    private fun sec(t: Long) = t * 1_000_000_000L // monotonic nanos for a whole second

    @Test
    fun aLockIsAnAccurateRecentFix() {
        // Within the core's CoreConstants.MAX_GPS_ACCURACY_M and LOCK_FRESH_MS old at
        // most, so a Start never latches onto a coarse or stale cached fix.
        val now = 100_000L
        val rows = listOf(
            // accuracy m, fix age ms, lock
            Triple(10.0, 4_000L, true),
            Triple(CoreConstants.MAX_GPS_ACCURACY_M, 0L, true), // just landed: the pill resamples the clock per fix
            Triple(10.0, LOCK_FRESH_MS, true),
            Triple(10.0, LOCK_FRESH_MS + 1, false),
            Triple(10.0, -1_000L, false), // stamped after now
            Triple(CoreConstants.MAX_GPS_ACCURACY_M + 0.1, 1_000L, false),
            Triple(60.0, 1_000L, false),
            Triple(null, 1_000L, false),
        )
        for ((accuracy, age, lock) in rows) {
            assertEquals(lock, isFreshLock(accuracy, now - age, now), "accuracy $accuracy m, age $age ms")
        }
        // fixElapsedMs == 0: no fix yet, whatever the accuracy.
        assertFalse(isFreshLock(accuracyM = 10.0, fixElapsedMs = 0L, nowElapsedMs = 5_000L))
    }

    @Test
    fun noGatesSeedsOnTheFirstFix() {
        val t = ArmTrigger(requirePrecise = false, requireMoving = false)
        val p = GpsPoint(0.0, 0.0, 0, 30.0)
        assertEquals(listOf(p), t.offer(p, sec(0))?.map { it.first })
    }

    @Test
    fun preciseGateWaitsForAPreciseFix() {
        val t = ArmTrigger(requirePrecise = true, requireMoving = false)
        // 40 m accuracy > GPS_PRECISE_ACCURACY_M: keep waiting.
        assertNull(t.offer(GpsPoint(0.0, 0.0, 0, 40.0), sec(0)))
        val precise = GpsPoint(0.0, 0.0, 1, 15.0)
        assertEquals(listOf(precise), t.offer(precise, sec(1))?.map { it.first })
    }

    @Test
    fun movingGateFiresAtTwoLegsAndSeedsTheWholeWindowInOrder() {
        val t = ArmTrigger(requirePrecise = false, requireMoving = true)
        val p0 = GpsPoint(0.0, 0.0, 0, 5.0)
        val p1 = GpsPoint(0.0, 3.0 * degPerM, 1, 5.0) // 3 m/s
        val p2 = GpsPoint(0.0, 6.0 * degPerM, 2, 5.0) // 3 m/s
        assertNull(t.offer(p0, sec(0)), "0 legs → wait")
        assertNull(t.offer(p1, sec(1)), "1 leg → wait")
        val seed = t.offer(p2, sec(2)) // 2 legs → fire
        assertEquals(listOf(p0, p1, p2), seed?.map { it.first }, "seed is the whole run-up window, oldest first")
    }

    @Test
    fun movingGateToleratesAnIsolatedStationaryFix() {
        // A single duplicate / near-stationary fix does not reset the run-up: the
        // gate measures net ground over the window, so it still fires while moving.
        val t = ArmTrigger(requirePrecise = false, requireMoving = true)
        val p0 = GpsPoint(0.0, 0.0, 0, 5.0)
        val m1 = GpsPoint(0.0, 3.0 * degPerM, 1, 5.0) // clean 3 m/s leg
        val dupe = GpsPoint(0.0, 3.0 * degPerM, 2, 5.0) // a duplicate / stationary fix
        val m2 = GpsPoint(0.0, 6.0 * degPerM, 3, 5.0) // running continues
        assertNull(t.offer(p0, sec(0)))
        assertNull(t.offer(m1, sec(1)))
        // The blip does NOT reset the run-up; net so far (3 m) is just under the floor.
        assertNull(t.offer(dupe, sec(2)))
        // Net over the window now covers real ground (6 m, 2 m/s) → fire, seeding the
        // whole window INCLUDING the blip (proof it was not reset/re-anchored).
        val seed = t.offer(m2, sec(3))
        assertEquals(listOf(p0, m1, dupe, m2), seed?.map { it.first })
    }

    @Test
    fun movingGateToleratesAnIsolatedJitterSpike() {
        // A one-off GPS teleport in the MIDDLE of the run-up must not stall the start:
        // net uses the window endpoints, so a mid-window spike is ignored once the
        // next real fix returns to the true track.
        val t = ArmTrigger(requirePrecise = false, requireMoving = true)
        val p0 = GpsPoint(0.0, 0.0, 0, 5.0)
        val m1 = GpsPoint(0.0, 3.0 * degPerM, 1, 5.0) // clean 3 m/s
        val spike = GpsPoint(0.0, 100.0 * degPerM, 2, 5.0) // ~97 m teleport (jitter)
        val m2 = GpsPoint(0.0, 6.0 * degPerM, 3, 5.0) // back on the true track
        assertNull(t.offer(p0, sec(0)))
        assertNull(t.offer(m1, sec(1)))
        // The spike as the NEWEST fix reads implausibly fast (50 m/s) → don't fire ON
        // it, but it does not reset the run-up either.
        assertNull(t.offer(spike, sec(2)))
        // Next real fix: net(p0→m2) = 6 m / 3 s = 2 m/s ignores the mid-window spike.
        val seed = t.offer(m2, sec(3))
        assertEquals(listOf(p0, m1, spike, m2), seed?.map { it.first })
    }

    @Test
    fun movingGateDoesNotFireOnStationaryWander() {
        // Standing at the line with GPS wander (±2 m) must never auto-start: net
        // displacement stays under the floor, so no fix in the window fires.
        val t = ArmTrigger(requirePrecise = false, requireMoving = true)
        listOf(0.0, 2.0, -1.0, 1.0, -2.0, 1.0).forEachIndexed { i, m ->
            val p = GpsPoint(0.0, m * degPerM, i.toLong(), 5.0)
            assertNull(t.offer(p, sec(i.toLong())), "wander fix $i must not auto-start")
        }
    }

    @Test
    fun bothGatesRequirePreciseAndSustainedMovement() {
        val t = ArmTrigger(requirePrecise = true, requireMoving = true)
        // A coarse fix neither seeds nor enters the moving window.
        assertNull(t.offer(GpsPoint(0.0, 0.0, 0, 40.0), sec(0)))
        // Precise + moving legs then fire, seeding the whole precise run-up.
        val a = GpsPoint(0.0, 0.0, 1, 10.0)
        val b = GpsPoint(0.0, 3.0 * degPerM, 2, 10.0)
        val c = GpsPoint(0.0, 6.0 * degPerM, 3, 10.0)
        assertNull(t.offer(a, sec(1)))
        assertNull(t.offer(b, sec(2)))
        val seed = t.offer(c, sec(3))
        assertEquals(listOf(a, b, c), seed?.map { it.first })
    }

    @Test
    fun alreadyMovingAtTapFiresAfterTwoLegs() {
        // A runner already moving when they arm: the first three consecutive moving
        // fixes (two legs) fire, so recording begins after ~2–3 s rather than stalling.
        val t = ArmTrigger(requirePrecise = false, requireMoving = true)
        var lon = 0.0
        val pts = (0..2).map {
            val p = GpsPoint(0.0, lon, it.toLong(), 5.0)
            lon += 3.0 * degPerM
            p
        }
        assertNull(t.offer(pts[0], sec(0)))
        assertNull(t.offer(pts[1], sec(1)))
        assertTrue(t.offer(pts[2], sec(2)) != null, "fires on the third moving fix")
    }

    @Test
    fun pillPriorityLadder() {
        // A missing precise-location grant outranks EVERYTHING, even a recording state,
        // so the pill can never read "Acquiring GPS…" while it physically can't get a
        // fix (which would contradict the sheet below).
        assertEquals(
            TrackerPill.PermissionNeeded,
            pillState(hasPermission = false, locationOn = true, tracking = true, paused = false, armed = false, signalLost = true, freshLock = true, requirePrecise = false, requireMoving = false),
        )
        // With the grant but system location off, "Location is off" outranks the rest.
        assertEquals(
            TrackerPill.LocationOff,
            pillState(hasPermission = true, locationOn = false, tracking = true, paused = false, armed = false, signalLost = true, freshLock = true, requirePrecise = false, requireMoving = false),
        )
        // signalLost outranks everything else while recording.
        assertEquals(
            TrackerPill.SignalLost,
            pillState(hasPermission = true, locationOn = true, tracking = true, paused = true, armed = false, signalLost = true, freshLock = true, requirePrecise = false, requireMoving = false),
        )
        assertEquals(
            TrackerPill.Paused,
            pillState(hasPermission = true, locationOn = true, tracking = true, paused = true, armed = false, signalLost = false, freshLock = false, requirePrecise = false, requireMoving = false),
        )
        assertEquals(
            TrackerPill.Recording,
            pillState(hasPermission = true, locationOn = true, tracking = true, paused = false, armed = false, signalLost = false, freshLock = false, requirePrecise = false, requireMoving = false),
        )
        // Armed text is gate-aware, and moving outranks precise when both are set.
        assertEquals(
            TrackerPill.ArmedMoving,
            pillState(hasPermission = true, locationOn = true, tracking = false, paused = false, armed = true, signalLost = false, freshLock = true, requirePrecise = true, requireMoving = true),
        )
        assertEquals(
            TrackerPill.ArmedPrecise,
            pillState(hasPermission = true, locationOn = true, tracking = false, paused = false, armed = true, signalLost = false, freshLock = true, requirePrecise = true, requireMoving = false),
        )
        assertEquals(
            TrackerPill.ArmedWaiting,
            pillState(hasPermission = true, locationOn = true, tracking = false, paused = false, armed = true, signalLost = false, freshLock = true, requirePrecise = false, requireMoving = false),
        )
        assertEquals(
            TrackerPill.FreshLock,
            pillState(hasPermission = true, locationOn = true, tracking = false, paused = false, armed = false, signalLost = false, freshLock = true, requirePrecise = false, requireMoving = false),
        )
        assertEquals(
            TrackerPill.Acquiring,
            pillState(hasPermission = true, locationOn = true, tracking = false, paused = false, armed = false, signalLost = false, freshLock = false, requirePrecise = false, requireMoving = false),
        )
    }

    @Test
    fun signalLostIsSuppressedWhenNotRecording() {
        // Not tracking → the signalLost flag can't surface (it is only meaningful for
        // an in-progress recording); a fresh lock shows instead.
        assertEquals(
            TrackerPill.FreshLock,
            pillState(hasPermission = true, locationOn = true, tracking = false, paused = false, armed = false, signalLost = true, freshLock = true, requirePrecise = false, requireMoving = false),
        )
    }

    // A stop that can't reach the service ([RunRecorder.stopUndelivered]) and every
    // recorder teardown call RunSession.markStopped(): clearing only `tracking` would
    // leave `locating` / `armed` latched and the tracker stuck on "Acquiring GPS" with
    // no engine.
    @Test
    fun markStoppedClearsLocatingAndArmedNotJustTracking() {
        RunSession.streamStarted(recording = true)
        RunRecorder.armRequested()
        RunSession.markStopped()
        assertFalse(RunSession.tracking.value)
        assertFalse(RunSession.locating.value, "locating cleared: no stuck 'Acquiring GPS'")
        assertFalse(RunSession.armed.value, "armed cleared: no stuck armed engine")
        RunSession.reset()
    }

    @Test
    fun markStoppedKeepsTheCapturedTrackUnlikeReset() {
        RunSession.reset()
        RunSession.add(GpsPoint(0.0, 0.0000, 0, 5.0), 0L)
        RunSession.add(GpsPoint(0.0, 0.0003, 10, 5.0), 10_000_000_000L)
        val captured = RunSession.points.value
        RunSession.markStopped()
        assertEquals(captured, RunSession.points.value, "markStopped preserves the captured track (unlike reset)")
        RunSession.reset()
    }

    @Test
    fun ageGateExemptsMultiFixBatchesButRejectsLoneStaleFixes() {
        assertTrue(armedFixPassesAgeGate(ageMs = 3_000, batchSize = 1))
        // A lone stale fix (the fused provider's cached first callback) is rejected -
        // the hazard this gate exists for.
        assertFalse(armedFixPassesAgeGate(ageMs = 60_000, batchSize = 1))
        // The SAME stale age inside a multi-fix batch is exempt: a batch is genuine
        // ongoing screen-off delivery, not the lone cached fix.
        assertTrue(armedFixPassesAgeGate(ageMs = 60_000, batchSize = 2))
        assertTrue(armedFixPassesAgeGate(ageMs = 60_000, batchSize = 3))
    }

    @Test
    fun batchedStaleDeliveryArmsButALoneStaleCachedFixDoesNot() {
        // Screen off: the fused provider flushes 3 fixes at once after a >10 s Doze
        // gap. Every fix is older than ARMED_FIX_MAX_AGE_MS; a per-fix age gate would
        // admit only the newest, the ArmTrigger moving window (needs 3 fixes) could
        // never fill, and 'Start on move' would silently never fire.
        val nowMs = 20_000L
        // Three fixes 4 s apart (span within the moving window), 3 m/s legs; every
        // age is past the staleness gate.
        data class Fix(val p: GpsPoint, val nanos: Long, val ageMs: Long)
        val batch = listOf(
            Fix(GpsPoint(0.0, 0.0, 0, 5.0), sec(0), nowMs - 0L),
            Fix(GpsPoint(0.0, 12.0 * degPerM, 4, 5.0), sec(4), nowMs - 4_000L),
            Fix(GpsPoint(0.0, 24.0 * degPerM, 8, 5.0), sec(8), nowMs - 8_000L),
        )
        assertTrue(batch.all { it.ageMs > ARMED_FIX_MAX_AGE_MS }, "every fix is stale")

        // Delivered as ONE batch (batchSize = 3): all three clear the exempted gate and
        // reach the trigger, whose window fills and fires, seeding the whole run-up.
        val trigger = ArmTrigger(requirePrecise = false, requireMoving = true)
        var seed: List<GpsPoint>? = null
        for (f in batch) {
            if (armedFixPassesAgeGate(f.ageMs, batchSize = batch.size)) {
                seed = trigger.offer(f.p, f.nanos)?.map { it.first }
            }
        }
        assertEquals(batch.map { it.p }, seed, "batched stale delivery arms and seeds the run-up")

        // Counterfactual: the SAME fixes arriving one-per-callback (batchSize = 1) are
        // each rejected by the staleness gate, so the trigger never fills → never fires.
        val lone = ArmTrigger(requirePrecise = false, requireMoving = true)
        var loneFired = false
        for (f in batch) {
            if (armedFixPassesAgeGate(f.ageMs, batchSize = 1)) {
                if (lone.offer(f.p, f.nanos) != null) loneFired = true
            }
        }
        assertFalse(loneFired, "a lone stale cached fix stays rejected")
    }

    @Test
    fun locationPanelSaysWhatTheUserActuallyAllowed() {
        // Before the system prompt: the rationale, never a "denied" message.
        assertEquals(LocationPrompt.Rationale, locationPrompt(asked = false, permaDenied = false, coarseGranted = false))
        // Refused outright: denied copy, not the approximate-only explanation.
        assertEquals(LocationPrompt.Denied, locationPrompt(asked = true, permaDenied = false, coarseGranted = false))
        // Approximate only: say so, whether or not this screen asked.
        assertEquals(LocationPrompt.CoarseOnly, locationPrompt(asked = true, permaDenied = false, coarseGranted = true))
        assertEquals(LocationPrompt.CoarseOnly, locationPrompt(asked = false, permaDenied = false, coarseGranted = true))
        // "Don't ask again" wins: only app settings can restore it.
        assertEquals(LocationPrompt.Blocked, locationPrompt(asked = true, permaDenied = true, coarseGranted = false))
        assertEquals(LocationPrompt.Blocked, locationPrompt(asked = true, permaDenied = true, coarseGranted = true))
    }
}
