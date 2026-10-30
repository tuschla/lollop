package app.milestone

import android.Manifest
import android.app.AlarmManager
import android.app.Application
import android.app.Notification
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.os.SystemClock
import androidx.test.core.app.ApplicationProvider
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowAlarmManager

/**
 * The rest-end alarm is a system alarm with a broadcast PendingIntent to
 * [RestEndReceiver], so the rest-done alert comes after the app is frozen or its
 * process is gone: exact while exact alarms are allowed (always below Android 12),
 * inexact otherwise; one alarm per rest, replaced by the next rest and removed
 * with the rest. Without exact alarms the set loop's notice asks once. Runs on
 * the shared SDK 34 config (exact alarms need the user's grant there) unless a
 * test pins another SDK.
 */
@RunWith(RobolectricTestRunner::class)
class RestAlarmDeliveryTest {
    private val app: Application = ApplicationProvider.getApplicationContext()
    private val nm get() = app.getSystemService(NotificationManager::class.java)
    private val am get() = app.getSystemService(AlarmManager::class.java)

    @Before
    fun setUp() {
        shadowOf(app).grantPermissions(Manifest.permission.POST_NOTIFICATIONS)
        RestAlarm.cancel(app)
        RestAlarm.resetProcessState()
        AppSettings.restAlert.value = true
    }

    @After
    fun tearDown() {
        RestAlarm.cancel(app)
        RestAlarm.resetProcessState()
    }

    /** What the rest shows: the rest-done alert, else the countdown; never both. */
    private fun posted(): Notification? {
        val done = shadowOf(nm).getNotification(RestAlarm.DONE_NOTIF_ID)
        val countdown = shadowOf(nm).getNotification(RestAlarm.COUNTDOWN_NOTIF_ID)
        assertTrue("countdown and rest-done alert both posted", done == null || countdown == null)
        return done ?: countdown
    }
    private fun title(n: Notification?) = n?.extras?.getCharSequence(Notification.EXTRA_TITLE)?.toString()
    private val resting get() = app.getString(R.string.log_rest_notif_title, "Back squat")
    private val restDone get() = app.getString(R.string.log_rest_notif_done_title)
    private fun alarm() = shadowOf(am).scheduledAlarms.single()
    /** The broadcast the system sends when [alarm] goes off, as it was set (a
     *  copy: a later rest updates the PendingIntent's extras in place). */
    private fun broadcastOf(a: ShadowAlarmManager.ScheduledAlarm): Intent = Intent(shadowOf(a.operation).savedIntent)
    private fun deliver(intent: Intent) = RestEndReceiver().onReceive(app, intent)

    /** One rest-end alarm, [restSec] from [before] (elapsed realtime), waking the
     *  device and allowed while idle, sent to [RestEndReceiver] as an immutable
     *  broadcast. */
    private fun assertRestEndAlarm(a: ShadowAlarmManager.ScheduledAlarm, before: Long, after: Long, restSec: Int) {
        assertEquals(AlarmManager.ELAPSED_REALTIME_WAKEUP, a.type)
        assertTrue("trigger ${a.triggerAtTime} is the rest end", a.triggerAtTime in (before + restSec * 1000L)..(after + restSec * 1000L))
        assertTrue("allowed while idle", a.isAllowWhileIdle)
        val op = shadowOf(a.operation)
        assertTrue(op.isBroadcastIntent)
        assertTrue("immutable", op.flags and PendingIntent.FLAG_IMMUTABLE != 0)
        assertEquals(RestEndReceiver::class.java.name, op.savedIntent.component?.className)
        assertEquals(RestAlarm.ACTION_REST_END, op.savedIntent.action)
    }

    @Test
    fun withExactAlarmsAllowedTheAlarmIsExact() {
        ShadowAlarmManager.setCanScheduleExactAlarms(true)
        val before = SystemClock.elapsedRealtime()
        RestAlarm.schedule(app, 90, "Back squat")
        val a = alarm()
        assertRestEndAlarm(a, before, SystemClock.elapsedRealtime(), 90)
        assertEquals("exact", ShadowAlarmManager.WINDOW_EXACT, a.windowLengthMs)
        assertFalse("no notice", RestAlarm.exactAlarmPrompt.value)
    }

    @Test
    fun withoutExactAlarmsTheAlarmIsInexactButStillSet() {
        ShadowAlarmManager.setCanScheduleExactAlarms(false)
        val before = SystemClock.elapsedRealtime()
        RestAlarm.schedule(app, 90, "Back squat")
        val a = alarm()
        assertRestEndAlarm(a, before, SystemClock.elapsedRealtime(), 90)
        assertEquals("inexact", ShadowAlarmManager.WINDOW_HEURISTIC, a.windowLengthMs)
        assertTrue("the loop's notice asks", RestAlarm.exactAlarmPrompt.value)
    }

    /** Below Android 12 exact alarms need no grant. */
    @Test
    @Config(sdk = [30])
    fun belowAndroid12TheAlarmIsExactWithoutAGrant() {
        ShadowAlarmManager.setCanScheduleExactAlarms(false)
        val before = SystemClock.elapsedRealtime()
        RestAlarm.schedule(app, 90, "Back squat")
        val a = alarm()
        assertRestEndAlarm(a, before, SystemClock.elapsedRealtime(), 90)
        assertEquals("exact", ShadowAlarmManager.WINDOW_EXACT, a.windowLengthMs)
        assertFalse("no notice", RestAlarm.exactAlarmPrompt.value)
    }

    /** The process that scheduled the rest is gone when the alarm fires: the
     *  receiver, in a fresh process with only the saved rest, replaces the
     *  countdown with the rest-done alert naming the next set's exercise. */
    @Test
    fun aFreshProcessPostsTheDoneAlertInPlaceOfTheCountdown() {
        ShadowAlarmManager.setCanScheduleExactAlarms(true)
        AppSettings.setRestAlert(app, false)
        RestAlarm.schedule(app, 90, "Back squat")
        RestAlarm.setNextSet(app, RestAlarm.NextSet("Bench press", "Set 1 of 3"))
        assertEquals(resting, title(posted()))
        val fired = broadcastOf(alarm())

        RestAlarm.resetProcessState()
        AppSettings.restAlert.value = true
        deliver(fired)

        val n = posted()!!
        assertEquals(restDone, title(n))
        assertEquals(app.getString(R.string.log_rest_notif_done_body, "Bench press"), n.extras.getCharSequence(Notification.EXTRA_TEXT).toString())
        assertEquals("not ongoing", 0, n.flags and Notification.FLAG_ONGOING_EVENT)
        assertFalse("no chronometer", n.extras.getBoolean(Notification.EXTRA_SHOW_CHRONOMETER))
        assertEquals("the countdown is gone", 1, shadowOf(nm).allNotifications.size)
        assertEquals("rest_done", n.channelId)
        // The saved "Rest timer alert" off is read in the fresh process.
        assertFalse(AppSettings.restAlert.value)

        // The saved rest is used up: a repeated broadcast posts nothing.
        nm.cancelAll()
        deliver(fired)
        assertNull(posted())
    }

    /** The countdown's timeout is the rest end, as is the exact alarm, so the
     *  system can remove the countdown's id just after the alert is posted: the
     *  alert has its own id and stays. */
    @Test
    fun theCountdownsTimeoutAtTheRestEndLeavesTheDoneAlert() {
        ShadowAlarmManager.setCanScheduleExactAlarms(true)
        RestAlarm.schedule(app, 90, "Back squat")
        assertEquals(resting, title(shadowOf(nm).getNotification(RestAlarm.COUNTDOWN_NOTIF_ID)))
        deliver(broadcastOf(alarm()))
        assertNull("the countdown is taken down explicitly", shadowOf(nm).getNotification(RestAlarm.COUNTDOWN_NOTIF_ID))

        // The countdown's setTimeoutAfter fires: the system cancels its id.
        nm.cancel(RestAlarm.COUNTDOWN_NOTIF_ID)

        assertEquals(restDone, title(shadowOf(nm).getNotification(RestAlarm.DONE_NOTIF_ID)))
        assertEquals(1, shadowOf(nm).allNotifications.size)
        // A new rest or cancel still clears the alert.
        RestAlarm.cancel(app)
        assertEquals(0, shadowOf(nm).allNotifications.size)
    }

    @Test
    fun cancelRemovesTheAlarmAndItsBroadcastDoesNothing() {
        ShadowAlarmManager.setCanScheduleExactAlarms(true)
        RestAlarm.schedule(app, 90, "Back squat")
        val fired = broadcastOf(alarm())
        RestAlarm.cancel(app)
        assertTrue(shadowOf(am).scheduledAlarms.isEmpty())
        deliver(fired)
        assertNull(posted())
    }

    /** A new rest replaces the alarm (same PendingIntent); the old rest's
     *  broadcast, if it was already on its way, does nothing. */
    @Test
    fun aNewRestReplacesTheAlarm() {
        ShadowAlarmManager.setCanScheduleExactAlarms(true)
        RestAlarm.schedule(app, 90, "Back squat")
        val first = broadcastOf(alarm())
        AppClock.offsetMillis += 5_000L
        try {
            val before = SystemClock.elapsedRealtime()
            RestAlarm.schedule(app, 60, "Back squat")
            val a = alarm()
            assertRestEndAlarm(a, before, SystemClock.elapsedRealtime(), 60)
            deliver(first)
            assertEquals("still counting down", resting, title(posted()))
            deliver(broadcastOf(a))
            assertEquals(restDone, title(posted()))
        } finally {
            AppClock.offsetMillis -= 5_000L
        }
    }

    @Test
    fun anotherBroadcastIsIgnored() {
        RestAlarm.schedule(app, 90, "Back squat")
        deliver(Intent(app, RestEndReceiver::class.java).putExtra(RestAlarm.EXTRA_REST_START, RestAlarm.running.value!!.startMillis))
        assertEquals(resting, title(posted()))
    }

    // ── The set loop's one-time notice ──

    @Test
    fun theNoticeIsShownOnceAndStaysDismissed() {
        ShadowAlarmManager.setCanScheduleExactAlarms(false)
        RestAlarm.schedule(app, 90, "Back squat")
        assertTrue(RestAlarm.exactAlarmPrompt.value)
        RestAlarm.dismissExactAlarmPrompt(app)
        assertFalse(RestAlarm.exactAlarmPrompt.value)
        RestAlarm.schedule(app, 90, "Back squat")
        assertFalse("not again", RestAlarm.exactAlarmPrompt.value)
        // Nor in a later process.
        RestAlarm.resetProcessState()
        RestAlarm.schedule(app, 90, "Back squat")
        assertFalse("not after a restart", RestAlarm.exactAlarmPrompt.value)
        assertEquals("the alarm is still set, inexact", ShadowAlarmManager.WINDOW_HEURISTIC, alarm().windowLengthMs)
    }

    /** Back from the system screen with exact alarms allowed: the notice goes and
     *  the running rest's alarm becomes exact, still at the rest end. */
    @Test
    fun aGrantSeenOnResumeMakesTheRunningRestExact() {
        ShadowAlarmManager.setCanScheduleExactAlarms(false)
        val before = SystemClock.elapsedRealtime()
        RestAlarm.schedule(app, 90, "Back squat")
        val after = SystemClock.elapsedRealtime()
        RestAlarm.recheckExactAlarms(app)
        assertTrue("still not allowed: the notice stays", RestAlarm.exactAlarmPrompt.value)
        assertEquals(ShadowAlarmManager.WINDOW_HEURISTIC, alarm().windowLengthMs)

        ShadowAlarmManager.setCanScheduleExactAlarms(true)
        RestAlarm.recheckExactAlarms(app)
        assertFalse(RestAlarm.exactAlarmPrompt.value)
        val a = alarm()
        assertEquals("exact", ShadowAlarmManager.WINDOW_EXACT, a.windowLengthMs)
        assertTrue("same rest end", a.triggerAtTime in (before + 89_000L)..(after + 90_000L))
    }
}
