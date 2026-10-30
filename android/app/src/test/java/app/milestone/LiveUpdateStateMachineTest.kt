package app.milestone

import android.Manifest
import android.app.AlarmManager
import android.app.Application
import android.app.Notification
import android.app.NotificationManager
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Looper
import android.os.SystemClock
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import androidx.test.core.app.ApplicationProvider
import java.time.Duration
import kotlin.math.abs
import kotlin.random.Random
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.android.controller.ServiceController
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowAlarmManager
import org.robolectric.shadows.ShadowSystemClock

/**
 * Seeded walks over the rest countdown and the run notification as Android 16
 * Live Updates (steps in `restWalk`/`runWalk`); after every step the posted
 * notifications must match the walk's model. `LIVE_SM_SEEDS` / `LIVE_SM_STEPS` /
 * `LIVE_SM_SEED` lengthen or pin a run; a failure prints its replay line.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class LiveUpdateStateMachineTest {
    private val app: Application = ApplicationProvider.getApplicationContext()
    private val nm get() = app.getSystemService(NotificationManager::class.java)
    private val am get() = app.getSystemService(AlarmManager::class.java)

    @Before
    fun setUp() {
        shadowOf(app).grantPermissions(
            Manifest.permission.POST_NOTIFICATIONS,
            Manifest.permission.ACCESS_FINE_LOCATION,
            Manifest.permission.ACCESS_COARSE_LOCATION,
        )
        clean()
    }

    @After
    fun tearDown() = clean()

    private fun clean() {
        RestAlarm.cancel(app)
        RestAlarm.resetProcessState()
        RunSession.reset()
        AppClock.offsetMillis = 0L
        AppSettings.restAlert.value = true
        ShadowAlarmManager.setCanScheduleExactAlarms(true)
        nm.cancelAll()
    }

    private fun walk(name: String, seed: Long, steps: Int, body: (Random, (String) -> Unit) -> Unit) =
        seededWalk("$name seed=$seed", "LIVE_SM_SEED=$seed LIVE_SM_SEEDS=1 LIVE_SM_STEPS=$steps") { note -> body(Random(seed), note) }

    private fun canNotify() =
        ContextCompat.checkSelfPermission(app, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED

    // ── Rest ─────────────────────────────────────────────────────────────────

    private data class Rest(val start: Long, val end: Long, val exercise: String)

    @Test
    fun restWalksNeverShowAStaleCountdown() {
        val seeds = System.getenv("LIVE_SM_SEEDS")?.toIntOrNull() ?: 30
        val steps = System.getenv("LIVE_SM_STEPS")?.toIntOrNull() ?: 80
        for (i in 0 until seeds) {
            clean()
            walk("rest", SEED_BASE + i, steps) { rng, note -> restWalk(rng, steps, note) }
        }
        // Not vacuous: the walks reached every notification state.
        assertTrue("countdowns checked: $countdownsSeen", countdownsSeen > seeds)
        assertTrue("rest-done alerts checked: $donesSeen", donesSeen > seeds / 2)
    }

    private var countdownsSeen = 0
    private var donesSeen = 0
    private var runNotifsSeen = 0
    private var pausedSeen = 0
    private var noGpsSeen = 0
    private var movingSeen = 0

    private fun restWalk(rng: Random, steps: Int, note: (String) -> Unit) {
        val exercises = listOf("Back squat", "Bench press", "Pull-up")
        // The model: the rest whose alarm is pending, the one that last ended, the
        // loop's readout on screen (this process), and what this process knows.
        var current: Rest? = null
        var ended: Rest? = null
        var inApp = false
        var ownsCurrent = false
        var dismissed = false
        var countdown: Notification? = null
        var countdownAt = 0L
        var done: Notification? = null
        var doneAt = 0L
        val appNow = { AppClock.nowMillis() }

        fun observe() {
            val c = shadowOf(nm).getNotification(RestAlarm.COUNTDOWN_NOTIF_ID)
            if (c != null && c !== countdown) {
                assertFalse("a countdown re-posted after the user swiped it away", dismissed && ownsCurrent)
                assertTrue("a countdown posted without the notification permission", canNotify())
                countdownAt = appNow()
                // At post time the chip counts down to the rest end.
                val r = checkNotNull(current) { "a countdown with no rest running" }
                val chipLeft = c.`when` - System.currentTimeMillis()
                assertTrue("chip ${chipLeft} ms left, rest ${r.end - appNow()} ms", abs(chipLeft - (r.end - appNow())) < 2_000)
            }
            countdown = c
            val d = shadowOf(nm).getNotification(RestAlarm.DONE_NOTIF_ID)
            if (d != null && d !== done) doneAt = appNow()
            done = d
        }

        /** The system takes down a notification whose timeout ran out. */
        fun expire() {
            countdown?.let { if (appNow() >= countdownAt + it.timeoutAfter) nm.cancel(RestAlarm.COUNTDOWN_NOTIF_ID) }
            done?.let { if (it.timeoutAfter > 0 && appNow() >= doneAt + it.timeoutAfter) nm.cancel(RestAlarm.DONE_NOTIF_ID) }
            observe()
        }

        fun restAlarms() = shadowOf(am).scheduledAlarms.filter { a ->
            a.operation?.let { shadowOf(it).savedIntent.action } == RestAlarm.ACTION_REST_END
        }

        fun deliverDue(force: Boolean) {
            val a = restAlarms().singleOrNull() ?: return
            if (a.triggerAtTime > SystemClock.elapsedRealtime()) return
            // An exact alarm comes on time; an inexact one maybe later.
            if (!force && a.windowLengthMs != ShadowAlarmManager.WINDOW_EXACT && rng.nextBoolean()) return
            val intent = Intent(shadowOf(a.operation).savedIntent)
            am.cancel(a.operation!!)
            val start = intent.getLongExtra(RestAlarm.EXTRA_REST_START, -1L)
            RestEndReceiver().onReceive(app, intent)
            if (current?.start == start) {
                ended = current
                current = null
                ownsCurrent = false
            }
            observe()
        }

        fun advance(ms: Long) {
            AppClock.offsetMillis += ms
            ShadowSystemClock.advanceBy(Duration.ofMillis(ms))
            expire()
            deliverDue(force = false)
        }

        fun check(op: String) {
            observe()
            val c = countdown
            val d = done
            assertTrue("countdown and rest-done alert both posted", c == null || d == null)
            if (c != null) {
                assertPromotableLiveUpdate(c)
                countdownsSeen++
                val r = current ?: throw AssertionError("a countdown with no rest running (ended=$ended)")
                assertTrue("countdown past the rest end", appNow() < r.end)
                assertFalse("countdown while the loop shows the rest", inApp)
                assertEquals(app.getString(R.string.log_rest_notif_title, r.exercise), c.extras.getCharSequence(Notification.EXTRA_TITLE).toString())
                assertEquals("the countdown's dismissal names its rest", r.start, shadowOf(c.deleteIntent).savedIntent.getLongExtra(RestAlarm.EXTRA_REST_START, -1))
                assertTrue(c.extras.getBoolean(Notification.EXTRA_CHRONOMETER_COUNT_DOWN))
                assertEquals(OpenDest.LogSet, openDestOf(shadowOf(c.contentIntent).savedIntent))
                assertTrue(nm.getNotificationChannel(c.channelId).importance > NotificationManager.IMPORTANCE_MIN)
            }
            if (d != null) {
                donesSeen++
                assertNull("rest-done alert while a rest runs", current)
                assertNotNull("rest-done alert with no ended rest", ended)
                assertFalse("rest-done alert while the loop shows the rest", inApp)
                assertFalse("the rest-done alert is not promoted", NotificationCompat.isRequestPromotedOngoing(d))
                assertEquals(0, d.flags and Notification.FLAG_ONGOING_EVENT)
                assertEquals(OpenDest.LogSet, openDestOf(shadowOf(d.contentIntent).savedIntent))
            }
            // Exactly the running rest holds the rest-end alarm.
            val alarms = restAlarms()
            if (current != null) {
                assertEquals("rest-end alarms", 1, alarms.size)
                assertEquals(current!!.start, shadowOf(alarms.single().operation).savedIntent.getLongExtra(RestAlarm.EXTRA_REST_START, -1))
            } else {
                assertTrue("a rest-end alarm with no rest running", alarms.isEmpty())
            }
            // Liveness: this process's running rest shows its countdown off-loop.
            if (current != null && ownsCurrent && !inApp && !dismissed && canNotify() && appNow() < current!!.end && op.startsWith("schedule")) {
                assertNotNull("no countdown for the rest just started", c)
            }
        }

        AppClock.offsetMillis = rng.nextLong(-DAY, DAY)
        check("start")
        for (i in 0 until steps) {
            val w = rng.nextInt(100)
            val op = when {
                w < 22 -> {
                    val sec = listOf(-5, 0, 15, 45, 90, 120, 180, 300)[rng.nextInt(8)]
                    val ex = exercises[rng.nextInt(exercises.size)]
                    RestAlarm.schedule(app, sec, ex)
                    if (sec > 0) {
                        val r = checkNotNull(RestAlarm.running.value)
                        current = Rest(r.startMillis, r.endAtMillis, ex)
                        ended = null
                        ownsCurrent = true
                        dismissed = false
                    }
                    // A set with no rest leaves the running rest alone.
                    if (sec > 0) "schedule $sec s $ex" else "no rest ($sec s)"
                }
                w < 36 -> { inApp = !inApp; RestAlarm.setInAppVisible(app, inApp); "in-app visible=$inApp" }
                w < 42 -> {
                    val next = if (rng.nextBoolean()) RestAlarm.NextSet(exercises[rng.nextInt(3)], "Set ${rng.nextInt(1, 5)} of 4") else null
                    RestAlarm.setNextSet(app, next)
                    "next set $next"
                }
                w < 66 -> { val ms = rng.nextLong(1_000, 200_000); advance(ms); "advance ${ms / 1000} s" }
                w < 70 -> { deliverDue(force = true); "late alarm" }
                w < 76 -> {
                    val c = countdown
                    if (c == null) {
                        "swipe (none)"
                    } else {
                        nm.cancel(RestAlarm.COUNTDOWN_NOTIF_ID)
                        RestDismissReceiver().onReceive(app, Intent(shadowOf(c.deleteIntent).savedIntent))
                        if (ownsCurrent) dismissed = true
                        // Half the time the loop then opens and closes again, the path that would re-post it.
                        if (rng.nextBoolean()) {
                            RestAlarm.setInAppVisible(app, true)
                            RestAlarm.setInAppVisible(app, false)
                            "swipe countdown, loop opened and closed"
                        } else {
                            "swipe countdown"
                        }
                    }
                }
                w < 80 -> {
                    // The rest-done alert's tap opens the set editor (auto-cancel); the loop shows the rest.
                    if (done != null) nm.cancel(RestAlarm.DONE_NOTIF_ID)
                    inApp = true
                    RestAlarm.setInAppVisible(app, true)
                    "open set editor"
                }
                w < 86 -> { RestAlarm.cancel(app); current = null; ended = null; ownsCurrent = false; "undo / finish (cancel)" }
                w < 91 -> {
                    RestAlarm.resetProcessState()
                    inApp = false
                    ownsCurrent = false
                    "process death"
                }
                w < 94 -> {
                    val granted = rng.nextBoolean()
                    if (granted) shadowOf(app).grantPermissions(Manifest.permission.POST_NOTIFICATIONS) else shadowOf(app).denyPermissions(Manifest.permission.POST_NOTIFICATIONS)
                    "notifications allowed=$granted"
                }
                w < 97 -> {
                    val exact = rng.nextBoolean()
                    ShadowAlarmManager.setCanScheduleExactAlarms(exact)
                    RestAlarm.recheckExactAlarms(app)
                    "exact alarms=$exact"
                }
                else -> { val on = rng.nextBoolean(); AppSettings.restAlert.value = on; "rest alert=$on" }
            }
            note("#$i $op")
            check(op)
        }
        shadowOf(app).grantPermissions(Manifest.permission.POST_NOTIFICATIONS)
    }

    // ── Run ──────────────────────────────────────────────────────────────────

    @Test
    fun runWalksKeepTheLiveUpdateInStepWithTheService() {
        val seeds = System.getenv("LIVE_SM_SEEDS")?.toIntOrNull() ?: 12
        val steps = System.getenv("LIVE_SM_STEPS")?.toIntOrNull() ?: 80
        for (i in 0 until seeds) {
            clean()
            walk("run", SEED_BASE + i, steps) { rng, note -> runWalk(rng, steps, note) }
        }
        assertTrue("run notifications checked: $runNotifsSeen", runNotifsSeen > seeds * 5)
        assertTrue("paused run notifications checked: $pausedSeen", pausedSeen > seeds)
        assertTrue("'No GPS' chips checked: $noGpsSeen", noGpsSeen > 0)
        assertTrue("moving-time chips past 0:00 checked: $movingSeen", movingSeen > seeds / 2)
    }

    private fun serviceIntent(action: String) = Intent(app, RunTrackingService::class.java).setAction("app.milestone.action.$action")

    private fun runWalk(rng: Random, steps: Int, note: (String) -> Unit) {
        var svc: ServiceController<RunTrackingService>? = null
        var startId = 0
        // The model: the service's state as the tracker and the notification drive it.
        var running = false
        var recording = false
        var armed = false
        var paused = false
        var dismissed = false
        // When the run last became live (recording, not paused), for the loss watchdog.
        var wasLive = false
        var liveSince = 0L

        fun notif(): Notification? = shadowOf(nm).getNotification(RunTrackingService.NOTIF_ID)
        fun idle() = shadowOf(Looper.getMainLooper()).idle()

        fun send(action: String): ServiceController<RunTrackingService> {
            val c = svc ?: Robolectric.buildService(RunTrackingService::class.java, serviceIntent(action)).create().also { svc = it }
            c.withIntent(serviceIntent(action)).startCommand(0, ++startId)
            idle()
            return c
        }

        /** The service stopped itself (or never started): the system destroys it. */
        fun destroyIfStopped() {
            val c = svc ?: return
            if (shadowOf(c.get()).isStoppedBySelf) {
                c.destroy()
                svc = null
                idle()
            }
        }

        fun stopModel() {
            running = false
            recording = false
            armed = false
            dismissed = false
        }

        fun check(op: String) {
            idle()
            val n = notif()
            assertEquals("recording flag", recording, RunSession.tracking.value)
            assertEquals("paused flag", paused, RunSession.paused.value)
            val live = recording && !paused
            if (live && !wasLive) liveSince = SystemClock.elapsedRealtime()
            wasLive = live
            if (!running) {
                assertNull("a run notification with no service behind it", n)
                assertFalse(RunSession.armed.value)
                assertFalse(RunSession.locating.value)
                return
            }
            if (dismissed) {
                assertNull("re-posted after the user dismissed it", n)
                return
            }
            assertNotNull("the running service shows no notification", n)
            n!!
            assertPromotableLiveUpdate(n)
            runNotifsSeen++
            if (recording && paused) pausedSeen++
            assertEquals("title", app.getString(trackingNotifTitleRes(recording, armed, paused)), n.extras.getCharSequence(Notification.EXTRA_TITLE).toString())
            val chip = n.shortCriticalText?.toString()
            val sinceFix = SystemClock.elapsedRealtime() - RunSession.lastFixElapsedMs.value
            val lastFix = RunSession.lastFixElapsedMs.value
            if (live) {
                if (chip == app.getString(R.string.live_chip_no_gps)) {
                    noGpsSeen++
                    assertTrue("'No GPS' ${sinceFix} ms after a fix", lastFix > 0 && sinceFix > SIGNAL_LOSS_MS)
                    assertEquals(app.getString(R.string.track_notif_signal_lost), n.extras.getCharSequence(Notification.EXTRA_TEXT).toString())
                } else {
                    assertEquals("chip is the moving time now", formatElapsed(RunSession.elapsedSec.value), chip)
                    if (RunSession.elapsedSec.value > 0) movingSeen++
                    // The watchdog reports a loss within its check slack once a live run
                    // has had no fix for the loss window.
                    val quiet = SystemClock.elapsedRealtime() - maxOf(lastFix, liveSince)
                    assertFalse("no 'No GPS' after $quiet ms without a fix", op.startsWith("no fix") && lastFix > 0 && quiet > SIGNAL_LOSS_MS + 1_000)
                }
            } else {
                assertEquals("chip", trackingChipRes(recording, armed, paused, signalLost = false)?.let(app::getString), chip)
            }
            val labels = n.actions.orEmpty().map { it.title.toString() }
            val expected = listOfNotNull(
                if (recording) app.getString(if (paused) R.string.track_action_resume else R.string.track_action_pause) else null,
                app.getString(R.string.track_action_stop),
            )
            assertEquals("actions", expected, labels)
            assertEquals(OpenDest.Track, openDestOf(shadowOf(n.contentIntent).savedIntent))
            assertTrue("channel above MIN", nm.getNotificationChannel(n.channelId).importance > NotificationManager.IMPORTANCE_MIN)
        }

        var fixes = 0
        check("start")
        for (i in 0 until steps) {
            val w = rng.nextInt(125)
            val op = when {
                // The tracker opens: a locate preview, only with nothing else going on.
                w < 12 -> if (!RunSession.tracking.value && !RunSession.armed.value && !RunSession.locating.value) {
                    send("LOCATE"); running = true; recording = false; armed = false; dismissed = false; "locate"
                } else {
                    "locate (guarded)"
                }
                w < 26 -> if (!RunSession.tracking.value) {
                    send("START"); running = true; recording = true; armed = false; dismissed = false; "start"
                } else {
                    "start (guarded)"
                }
                w < 34 -> if (!RunSession.tracking.value) {
                    send("ARM"); running = true; recording = false; armed = true; dismissed = false; "arm"
                } else {
                    "arm (guarded)"
                }
                w < 40 -> if (armed) { send("START_NOW"); recording = true; armed = false; "start now" } else "start now (not armed)"
                w < 50 -> {
                    val pause = rng.nextBoolean()
                    send(if (pause) "PAUSE" else "RESUME")
                    destroyIfStopped()
                    // Only a recording run pauses or resumes.
                    if (recording) paused = pause
                    "notification ${if (pause) "pause" else "resume"}"
                }
                w < 60 -> if (recording) { paused = !paused; RunSession.setPaused(paused); "in-app pause=$paused" } else "in-app pause (not recording)"
                w < 70 -> {
                    val n = notif()
                    if (n == null) {
                        "dismiss (none)"
                    } else {
                        nm.cancel(RunTrackingService.NOTIF_ID)
                        val c = svc
                        if (c != null) {
                            c.withIntent(Intent(shadowOf(n.deleteIntent).savedIntent)).startCommand(0, ++startId)
                            idle()
                        }
                        dismissed = true
                        "dismiss"
                    }
                }
                w < 80 -> { send("STOP"); destroyIfStopped(); stopModel(); "stop" }
                w < 88 -> {
                    // Finish → Save (or Discard): the tracker stops the service, then clears the session.
                    send("STOP"); destroyIfStopped(); RunSession.reset(); stopModel(); paused = false; "save"
                }
                w < 94 -> { svc?.destroy(); svc = null; idle(); stopModel(); "system stops the service" }
                w < 100 -> { svc?.destroy(); svc = null; RunSession.reset(); idle(); stopModel(); paused = false; "process death" }
                w < 118 -> {
                    // A GPS fix a few seconds and ~10 m on.
                    val dt = rng.nextLong(1_000, 5_000)
                    shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMillis(dt))
                    fixes++
                    val loc = android.location.Location(android.location.LocationManager.GPS_PROVIDER).apply {
                        latitude = 52.5 + fixes * 0.0001
                        longitude = 13.4
                        accuracy = 5f
                        time = System.currentTimeMillis()
                        elapsedRealtimeNanos = SystemClock.elapsedRealtimeNanos()
                    }
                    shadowOf(app.getSystemService(android.location.LocationManager::class.java)).simulateLocation(loc)
                    idle()
                    // An armed run begins recording on a fix that clears its gates.
                    if (armed && RunSession.tracking.value) {
                        recording = true
                        armed = false
                    }
                    "fix +${dt} ms"
                }
                else -> {
                    val dt = rng.nextLong(5_000, 40_000)
                    shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMillis(dt))
                    "no fix for ${dt / 1000} s"
                }
            }
            note("#$i $op")
            check(op)
        }
        svc?.destroy()
    }

    companion object {
        private const val DAY = 24 * 60 * 60_000L
        private val SEED_BASE = System.getenv("LIVE_SM_SEED")?.toLongOrNull() ?: 20_261_030L
    }
}
