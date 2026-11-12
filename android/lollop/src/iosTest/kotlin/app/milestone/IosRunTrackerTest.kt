@file:OptIn(ExperimentalForeignApi::class)

package app.milestone

import app.milestone.widget.IosRunLive
import app.milestone.widget.iosSurfaceJson
import kotlinx.cinterop.ExperimentalForeignApi
import platform.CoreLocation.CLLocation
import platform.CoreLocation.CLLocationCoordinate2DMake
import platform.CoreLocation.CLLocationManager
import platform.CoreLocation.CLLocationManagerDelegateProtocol
import platform.CoreLocation.kCLErrorDenied
import platform.CoreLocation.kCLErrorDomain
import platform.CoreLocation.kCLErrorLocationUnknown
import platform.Foundation.NSDate
import platform.Foundation.NSError
import platform.Foundation.NSRunLoop
import platform.Foundation.dateByAddingTimeInterval
import platform.Foundation.dateWithTimeIntervalSinceNow
import platform.Foundation.runUntilDate
import platform.Foundation.timeIntervalSince1970
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * The iOS tracker driven the way CoreLocation drives it: simulated [CLLocation]
 * sequences into the stream delegate ([IosRunRecording.delegate]), through the
 * shared [RunRecorder] into [RunSession] and the real active-run file. The stream
 * itself is a fake (the simulator test has no location grant).
 */
class IosRunTrackerTest {
    private val dir = newTempDir()
    private val sidecar = fileIn(dir, AppFileNames.ACTIVE_RUN)
    private val stream = FakeStream()
    private val clock = ShiftedClock()
    private lateinit var tracker: IosRunRecording
    private val manager = CLLocationManager()
    private val base = NSDate()

    @BeforeTest
    fun setUp() {
        AppRuntime.resetForTest(clock = clock)
        RunSession.reset()
        ActiveRunStore.open(sidecar)
        tracker = IosRunRecording(stream)
    }

    @AfterTest
    fun tearDown() {
        tracker.recorder.stop()
        ActiveRunStore.flush()
        AppFiles.delete(sidecar)
        AppFiles.delete(dir)
        RunSession.reset()
        AppRuntime.resetForTest()
    }

    /** Fix [i] of a straight run east along the equator (~3.3 m apart), stamped
     *  [ageSec] before the test's start. */
    private fun location(i: Int, ageSec: Double, accuracyM: Double = 5.0) =
        locationAt(i, base.dateByAddingTimeInterval(-ageSec), accuracyM)

    private fun locationAt(i: Int, at: NSDate, accuracyM: Double = 5.0) = CLLocation(
        coordinate = CLLocationCoordinate2DMake(0.0, i * 3e-5),
        altitude = 0.0,
        horizontalAccuracy = accuracyM,
        verticalAccuracy = -1.0,
        timestamp = at,
    )

    /** Run the main run loop (the surface's flow collectors) until [done]. */
    private fun runLoopUntil(what: String, done: () -> Boolean) {
        val deadline = NSDate().timeIntervalSince1970 + 10.0
        while (!done()) {
            assertTrue(NSDate().timeIntervalSince1970 < deadline, "no $what within 10 s")
            NSRunLoop.mainRunLoop.runUntilDate(NSDate.dateWithTimeIntervalSinceNow(0.01))
        }
    }

    private fun deliver(vararg fixes: CLLocation) =
        tracker.delegate.locationManager(manager, didUpdateLocations = fixes.toList())

    private fun error(code: Long) = NSError(domain = kCLErrorDomain, code = code, userInfo = null)

    @Test
    fun anArmedRunRecordsADeferredBatchInTimeOrder() {
        tracker.recorder.start(record = false, arming = true)
        assertTrue(stream.running)
        val fixes = (0..10).map { location(it, ageSec = 10.0 - it) }
        deliver(*fixes.shuffled().toTypedArray())
        assertTrue(RunSession.tracking.value, "the first fix of a batch passes the armed age gate")
        val points = RunSession.points.value
        assertEquals((0..10).map { it * 3e-5 }, points.map { it.lon }, "oldest first")
        assertTrue(RunSession.distanceKm.value in 0.030..0.040, "${RunSession.distanceKm.value}")
        assertTrue(RunSession.elapsedSec.value in 9L..10L, "moving time from the fix stamps: ${RunSession.elapsedSec.value}")
        ActiveRunStore.flush()
        assertEquals(points.toList(), ActiveRunStore.recover().toList(), "the sidecar holds the run")
    }

    @Test
    fun thePreviewSkipsInvalidWideAndCachedFixes() {
        tracker.recorder.start(record = false)
        deliver(location(0, ageSec = 0.0, accuracyM = -1.0))
        deliver(location(1, ageSec = 0.0, accuracyM = 65.0))
        deliver(location(2, ageSec = 60.0))
        assertNull(RunSession.lastFix.value, "invalid, too wide, a cached old fix")
        deliver(location(3, ageSec = 0.0))
        assertEquals(3 * 3e-5, assertNotNull(RunSession.lastFix.value).lon)
        assertTrue(RunSession.points.value.isEmpty(), "a preview records nothing")
    }

    @Test
    fun aPauseIsASegmentInTheSidecar() {
        tracker.recorder.start(record = false, arming = true)
        deliver(*(0..4).map { location(it, ageSec = 20.0 - it) }.toTypedArray())
        RunSession.setPaused(true)
        deliver(location(5, ageSec = 15.0))
        RunSession.setPaused(false)
        deliver(*(6..7).map { location(it, ageSec = 20.0 - it) }.toTypedArray())
        assertEquals(7, RunSession.points.value.size)
        ActiveRunStore.flush()
        assertEquals(listOf(5), ActiveRunStore.recover().segmentStarts)
    }

    @Test
    fun aDeniedGrantEndsTheStreamAndAnUnknownLocationDoesNot() {
        tracker.recorder.start(record = false, arming = true)
        deliver(location(0, ageSec = 0.0))
        tracker.delegate.locationManager(manager, didFailWithError = error(kCLErrorLocationUnknown.toLong()))
        assertTrue(RunSession.tracking.value)
        tracker.delegate.locationManager(manager, didFailWithError = error(kCLErrorDenied.toLong()))
        assertFalse(RunSession.tracking.value)
        assertTrue(RunSession.locationFailed.value)
        assertFalse(stream.running)
        assertEquals(1, RunSession.points.value.size, "the fixes stay for Save")
    }

    @Test
    fun losingPreciseLocationMidRunEndsTheStream() {
        tracker.recorder.start(record = false, arming = true)
        deliver(location(0, ageSec = 0.0))
        tracker.delegate.locationManagerDidChangeAuthorization(manager)
        assertTrue(RunSession.tracking.value, "still precise: nothing changes")
        stream.precise = false
        tracker.delegate.locationManagerDidChangeAuthorization(manager)
        assertFalse(RunSession.tracking.value)
        assertTrue(RunSession.locationFailed.value)
    }

    @Test
    fun noGrantNoStream() {
        stream.precise = false
        tracker.recorder.start(record = false)
        assertFalse(stream.running)
        assertTrue(RunSession.locationFailed.value)
    }

    @Test
    fun aRunResumedAfterARelaunchAppendsBehindASplice() {
        // The run before the app was killed: five fixes in the sidecar.
        ActiveRunStore.begin()
        val before = (0..4).map { GpsPoint(0.0, it * 3e-5, 1_790_000_000L + it, 5.0) }
        before.forEach { ActiveRunStore.append(it, segmentStart = false) }
        ActiveRunStore.flush()
        // The relaunch: recover, restore, start recording again (CoachSession's Resume).
        val recovered = ActiveRunStore.recover()
        RunSession.restore(recovered)
        tracker.recorder.start(record = true)
        assertTrue(RunSession.tracking.value)
        // The first live fixes: stamped after the restore, as a fresh fix is. One
        // stamped before it would be dropped as out of order (Android alike).
        clock.shiftMs += 1200L
        val now = NSDate()
        deliver(locationAt(100, now.dateByAddingTimeInterval(-1.0)), locationAt(101, now))
        assertEquals(7, RunSession.points.value.size)
        ActiveRunStore.flush()
        val back = ActiveRunStore.recover()
        assertEquals(RunSession.points.value.toList(), back.toList())
        assertEquals(listOf(5), back.segmentStarts, "the gap while the app was dead is no run leg")
    }

    @Test
    fun theSurfaceGetsAFrameForEachPhase() {
        // Each frame as the activity shows it: (recording, paused).
        val frames = mutableListOf<Pair<Boolean, Boolean>>()
        var ended = 0
        IosSurfaces.install(object : IosSurfaceHost {
            override fun publishWidget(json: String) = Unit

            override fun updateRunActivity(json: String) {
                val live = iosSurfaceJson.decodeFromString(IosRunLive.serializer(), json)
                frames += live.recording to live.paused
            }

            override fun endRunActivity() {
                ended++
            }

            override fun updateRestActivity(json: String) = Unit

            override fun endRestActivity() = Unit
        })
        val armed = false to false
        val recording = true to false
        val paused = false to true
        tracker.recorder.start(record = false, arming = true)
        runLoopUntil("armed frame") { armed in frames }
        deliver(location(0, ageSec = 2.0), location(1, ageSec = 1.0))
        RunSession.setPaused(true)
        // The pause reaches the surface through the main-thread flow collector.
        runLoopUntil("paused frame") { frames.lastOrNull() == paused }
        deliver(location(2, ageSec = 0.0))
        tracker.recorder.stop()
        assertEquals(listOf(armed, recording, paused), frames.distinct())
        assertEquals(1, ended)
    }

    @Test
    fun aFixMovesOntoTheMonotonicClockByItsAge() {
        val nowWallMs = (base.timeIntervalSince1970 * 1000.0).toLong()
        val fix = liveFixOf(location(7, ageSec = 3.0, accuracyM = 8.0), nowWallMs, nowMonoNanos = 50_000_000_000L)
        assertEquals(7 * 3e-5, fix.lon)
        assertEquals(8.0, fix.accuracyM)
        assertEquals(nowWallMs - 3000L, fix.wallMs, absoluteTolerance = 1L)
        assertEquals(47_000_000_000L, fix.monoNanos, absoluteTolerance = 1_000_000L)
        assertNull(liveFixOf(location(0, ageSec = 0.0, accuracyM = -1.0), nowWallMs, 0L).accuracyM)
        val future = liveFixOf(location(0, ageSec = -5.0), nowWallMs, nowMonoNanos = 9L)
        assertEquals(9L, future.monoNanos, "a fix stamped ahead of the clock is now, not later")
    }
}

private fun assertEquals(expected: Long, actual: Long, absoluteTolerance: Long) =
    assertTrue(kotlin.math.abs(expected - actual) <= absoluteTolerance, "expected $expected ± $absoluteTolerance, was $actual")

/** The device's clocks, the monotonic one moved ahead by [shiftMs]: time passing
 *  without the test waiting for it. */
private class ShiftedClock : WallClock {
    var shiftMs = 0L

    override fun nowMillis(): Long = wallClockMillis()

    override fun monotonicMillis(): Long = deviceMonotonicMillis() + shiftMs
}

/** A [LocationStream] with a scripted grant; the test feeds the delegate itself. */
private class FakeStream : LocationStream {
    var precise = true
    var running = false

    override fun preciseGranted(): Boolean = precise

    override fun start(delegate: CLLocationManagerDelegateProtocol) {
        running = true
    }

    override fun stop() {
        running = false
    }
}
