package app.milestone

import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * The recording state machine both platforms run ([RunRecorder]): which fixes reach
 * the preview and the route, the armed start, pause, the sidecar it writes, the
 * failure and stale-session unwinds and the signal-loss watchdog. A scripted host
 * stands in for the location stream, the surface and the timers; [RunSession] and
 * [ActiveRunStore] are the real ones (the sidecar in a temporary directory). The
 * engine is a stand-in ([FakeCore]), so a save or recovery keeps every fix (no core
 * thinning).
 */
class RunRecorderTest {
    private val dir = newTempDir()
    private val sidecar = fileIn(dir, AppFileNames.ACTIVE_RUN)
    private val host = ScriptedHost()
    private val recorder = RunRecorder(host)

    @BeforeTest
    fun setUp() {
        AppRuntime.resetForTest(engine = DurableCore(FakeCore()))
        RunSession.reset()
        ActiveRunStore.open(sidecar)
    }

    @AfterTest
    fun tearDown() {
        ActiveRunStore.flush()
        AppFiles.delete(sidecar)
        AppFiles.delete(dir)
        RunSession.reset()
    }

    /** Fix [i] of a straight run east along the equator, ~3.3 m apart, one second
     *  apart on both clocks; [ageMs] old when delivered. */
    private fun fix(i: Int, accuracyM: Double? = 5.0, ageMs: Long = 0L) = LiveFix(
        lat = 0.0,
        lon = i * 3e-5,
        wallMs = 1_790_000_000_000L + i * 1000L,
        monoNanos = (host.now - ageMs) * 1_000_000L,
        accuracyM = accuracyM,
    )

    private fun deliver(vararg indices: Int) {
        for (i in indices) {
            host.now += 1000L
            recorder.onFix(fix(i), batchSize = 1)
        }
    }

    private fun recovered(): RecoveredTrack {
        ActiveRunStore.flush()
        return ActiveRunStore.recover()
    }

    @Test
    fun aLocatePreviewShowsTheFixButRecordsNothing() {
        recorder.start(record = false)
        assertTrue(host.streaming)
        assertTrue(RunSession.locating.value)
        assertFalse(RunSession.tracking.value)
        deliver(0)
        assertEquals(0.0, assertNotNull(RunSession.lastFix.value).lon)
        assertEquals(host.now, RunSession.lastFixElapsedMs.value, "the fix's own monotonic stamp")
        assertTrue(RunSession.points.value.isEmpty())
        ActiveRunStore.flush()
        assertFalse(ActiveRunStore.hasActiveRun(), "no sidecar before a run records")
    }

    @Test
    fun thePreviewDropsAStaleLoneFixAndInaccurateOnes() {
        recorder.start(record = false)
        recorder.onFix(fix(0, ageMs = ARMED_FIX_MAX_AGE_MS + 1), batchSize = 1)
        assertNull(RunSession.lastFix.value, "a cached first fix")
        recorder.onFix(fix(0, ageMs = ARMED_FIX_MAX_AGE_MS + 1), batchSize = 2)
        assertNotNull(RunSession.lastFix.value, "an old fix in a batch is screen-off delivery")
        recorder.onFix(fix(1, accuracyM = CoreConstants.MAX_GPS_ACCURACY_M + 1), batchSize = 1)
        recorder.onFix(fix(2, accuracyM = null), batchSize = 1)
        assertEquals(0.0, RunSession.lastFix.value?.lon, "too wide or no accuracy: not shown")
    }

    @Test
    fun anArmedStartRecordsFromItsFirstFixIntoTheSidecar() {
        recorder.start(record = false, arming = true)
        assertTrue(RunSession.armed.value)
        deliver(0)
        assertFalse(RunSession.armed.value)
        assertTrue(RunSession.tracking.value)
        assertEquals(listOf(RecorderCue.RecordingStarted), host.cues)
        deliver(1, 2, 3, 4, 5, 6, 7, 8, 9, 10)
        val points = RunSession.points.value
        assertEquals(11, points.size)
        assertTrue(RunSession.distanceKm.value in 0.030..0.040, "10 legs of ~3.3 m: ${RunSession.distanceKm.value}")
        assertEquals(10L, RunSession.elapsedSec.value)
        val back = recovered()
        assertEquals(points.toList(), back.toList(), "every accepted fix is in the sidecar")
        assertEquals(emptyList<Int>(), back.segmentStarts)
    }

    @Test
    fun aRecordingKeepsAnOldFix() {
        recorder.start(record = true)
        assertTrue(RunSession.tracking.value)
        recorder.onFix(fix(0, ageMs = 60_000L), batchSize = 1)
        assertEquals(1, RunSession.points.value.size, "no age gate once recording")
    }

    @Test
    fun thePreciseGateWaitsForAPreciseFix() {
        recorder.start(record = false, arming = true, requirePrecise = true)
        host.now += 1000L
        recorder.onFix(fix(0, accuracyM = GPS_PRECISE_ACCURACY_M + 5), batchSize = 1)
        assertTrue(RunSession.armed.value)
        assertTrue(RunSession.points.value.isEmpty())
        assertNotNull(RunSession.lastFix.value, "the preview still shows it")
        deliver(1)
        assertTrue(RunSession.tracking.value)
        assertEquals(1, RunSession.points.value.size)
    }

    @Test
    fun aPauseDropsFixesAndTheResumeOpensASegmentTheSidecarKeeps() {
        recorder.start(record = false, arming = true)
        recorder.setPaused(true)
        assertFalse(RunSession.paused.value, "only a recording run pauses")
        deliver(0, 1, 2, 3, 4)
        recorder.setPaused(true)
        deliver(5, 6, 7)
        assertEquals(5, RunSession.points.value.size, "paused: no route")
        recorder.setPaused(false)
        deliver(8, 9)
        assertEquals(7, RunSession.points.value.size)
        assertEquals(listOf(5), RunSession.decimatedSegmentStarts())
        assertEquals(listOf(5), recovered().segmentStarts)
    }

    @Test
    fun startNowRecordsAnArmedRunBeforeAnyFix() {
        recorder.start(record = false, arming = true)
        recorder.startNow()
        assertTrue(RunSession.tracking.value)
        assertFalse(RunSession.armed.value)
        ActiveRunStore.flush()
        assertTrue(ActiveRunStore.hasActiveRun(), "the sidecar marks the run")
        assertEquals(0, recovered().size)
        recorder.startNow()
        assertEquals(1, host.cues.size, "a second Start now is a no-op")
    }

    @Test
    fun anArmOverALiveRecordingKeepsIt() {
        recorder.start(record = false, arming = true)
        deliver(0, 1)
        recorder.start(record = false, arming = true)
        assertTrue(recorder.recording)
        assertFalse(RunSession.armed.value)
        assertEquals(1, host.reasserts)
        deliver(2)
        assertEquals(3, RunSession.points.value.size)
    }

    @Test
    fun aFailedStartUnwinds() {
        host.grant = false
        RunRecorder.armRequested() // the screen's tap
        recorder.start(record = false, arming = true)
        assertTrue(RunSession.locationFailed.value)
        assertFalse(RunSession.armed.value)
        assertFalse(RunSession.locating.value)
        assertEquals(listOf(false), host.finishes)
    }

    @Test
    fun aStreamFailureAfterTheStartUnwindsOnce() {
        recorder.start(record = false, arming = true)
        deliver(0)
        recorder.onStreamFailed()
        recorder.onStreamFailed()
        assertTrue(RunSession.locationFailed.value)
        assertFalse(RunSession.tracking.value)
        assertFalse(host.streaming)
        assertEquals(listOf(false), host.finishes)
        assertEquals(1, RunSession.points.value.size, "the captured fixes stay for Save")
    }

    @Test
    fun aResetSessionStandsTheStreamDown() {
        recorder.start(record = false, arming = true)
        RunSession.reset()
        deliver(0)
        assertNull(RunSession.lastFix.value)
        assertFalse(host.streaming)
        assertFalse(RunSession.locationFailed.value, "not a failure")
        assertEquals(listOf(false), host.finishes)
        ActiveRunStore.flush()
        assertFalse(ActiveRunStore.hasActiveRun())
    }

    @Test
    fun aStopTearsDownAndALateFixIsIgnored() {
        recorder.start(record = false, arming = true)
        deliver(0, 1, 2)
        recorder.stop()
        assertFalse(RunSession.tracking.value)
        assertFalse(RunSession.locating.value)
        assertEquals(listOf(true), host.finishes)
        val refreshes = host.refreshes
        deliver(3)
        assertEquals(3, RunSession.points.value.size)
        assertEquals(refreshes, host.refreshes, "no surface after the stop")
    }

    @Test
    fun theWatchdogCuesOnceWhenTheSignalGoesAndClearsOnTheNextFix() {
        recorder.start(record = false, arming = true)
        deliver(0)
        val watchdog = host.timer("lollo:signal-watchdog")
        host.now += SIGNAL_LOSS_MS / 2
        watchdog.fire()
        assertFalse(recorder.signalLost, "not due yet: re-armed for the rest")
        host.now += SIGNAL_LOSS_MS
        watchdog.fire()
        assertTrue(recorder.signalLost)
        assertEquals(listOf(RecorderCue.RecordingStarted, RecorderCue.SignalLost), host.cues)
        watchdog.fire()
        assertEquals(2, host.cues.size, "one cue per episode")
        val invalidations = host.invalidations
        deliver(1)
        assertFalse(recorder.signalLost)
        assertEquals(invalidations + 1, host.invalidations, "the normal line comes back")
    }

    @Test
    fun aPausedRunGetsNoSignalCue() {
        recorder.start(record = false, arming = true)
        deliver(0)
        RunSession.setPaused(true)
        host.now += SIGNAL_LOSS_MS * 2
        host.timer("lollo:signal-watchdog").fire()
        assertFalse(recorder.signalLost)
        assertEquals(listOf(RecorderCue.RecordingStarted), host.cues)
    }
}

/** A [RecorderHost] with a hand-moved clock, a scripted grant and timers the test fires. */
private class ScriptedHost : RecorderHost {
    var now = 5_000_000L
    var grant = true
    var streaming = false
    var reasserts = 0
    var refreshes = 0
    var invalidations = 0
    val cues = mutableListOf<RecorderCue>()
    val finishes = mutableListOf<Boolean>()
    private val timers = mutableMapOf<String, ScriptedTimer>()

    inner class ScriptedTimer(private val earliestWins: Boolean, private val action: () -> Unit) : RecorderTimer {
        private var dueAt: Long? = null

        override fun schedule(delayMs: Long) {
            val at = now + delayMs
            val pending = dueAt
            if (earliestWins && pending != null && pending <= at) return
            dueAt = at
        }

        override fun cancel() {
            dueAt = null
        }

        fun fire() {
            val at = dueAt ?: return
            if (now < at) return
            dueAt = null
            action()
        }
    }

    fun timer(tag: String): ScriptedTimer = timers.getValue(tag)

    override fun nowNanos(): Long = now * 1_000_000L

    override fun nowMs(): Long = now

    override fun startStream(): Boolean {
        streaming = grant
        return grant
    }

    override fun stopStream() {
        streaming = false
    }

    override fun openSidecar() = ActiveRunStore.begin()

    override fun reassertSurface() {
        reasserts++
    }

    override fun refresh() {
        refreshes++
    }

    override fun invalidateSurface() {
        invalidations++
    }

    override fun cue(cue: RecorderCue) {
        cues += cue
    }

    override fun releaseCues() = Unit

    override fun finished(userStop: Boolean) {
        finishes += userStop
    }

    override fun wakeTimer(tag: String, action: () -> Unit): RecorderTimer =
        ScriptedTimer(earliestWins = true, action).also { timers[tag] = it }

    override fun ticker(action: () -> Unit): RecorderTimer = ScriptedTimer(earliestWins = false, action)
}
