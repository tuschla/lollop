package app.milestone

import android.Manifest
import android.app.Application
import android.app.Notification
import android.app.NotificationManager
import android.content.Intent
import androidx.core.app.NotificationCompat
import androidx.test.core.app.ApplicationProvider
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.Shadows.shadowOf

/**
 * The set loop's rest countdown as a Live Update: an ongoing notification that
 * requests promotion, counts down to the rest end in a chronometer (the status-bar
 * chip on Android 16+), names the next set and opens the set editor; at the rest
 * end the non-promoted rest-done alert replaces it. Runs on the shared Robolectric
 * SDK 34 config; the countdown test pins SDK 36 to check the built notification
 * against the documented Live Update rules ([assertPromotableLiveUpdate]).
 */
@RunWith(RobolectricTestRunner::class)
class LiveRestNotificationTest {
    private val app: Application = ApplicationProvider.getApplicationContext()
    private val nm get() = app.getSystemService(NotificationManager::class.java)
    private val res get() = app.resources

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
        AppSettings.restAlert.value = true
    }

    private fun posted() = postedRest(app)

    private fun squatNext() = RestAlarm.NextSet("Back squat", "Set 3 of 4 · 100\u00A0kg × 5")

    @Test
    @Config(sdk = [36])
    fun countdownIsAPromotableChronometerToTheRestEnd() {
        val before = System.currentTimeMillis()
        // The loop's order: the rest starts while the loop shows it, the loop
        // reports its next set, then the loop closes.
        RestAlarm.setInAppVisible(app, true)
        RestAlarm.schedule(app, 90, "Back squat")
        RestAlarm.setNextSet(app, squatNext())
        RestAlarm.setInAppVisible(app, false)
        val after = System.currentTimeMillis()
        val n = posted()!!

        assertPromotableLiveUpdate(n)
        assertTrue("channel above MIN", nm.getNotificationChannel(n.channelId).importance > NotificationManager.IMPORTANCE_MIN)
        assertEquals(Notification.BigTextStyle::class.java.name, n.extras.getString(Notification.EXTRA_TEMPLATE))
        assertTrue("chronometer", n.extras.getBoolean(Notification.EXTRA_SHOW_CHRONOMETER))
        assertTrue("counts down", n.extras.getBoolean(Notification.EXTRA_CHRONOMETER_COUNT_DOWN))
        assertTrue("when ${n.`when`} is the rest end", n.`when` in (before + 89_000L)..(after + 90_000L))
        assertEquals(res.getString(R.string.log_rest_notif_title, "Back squat"), n.extras.getCharSequence(Notification.EXTRA_TITLE).toString())
        assertEquals(res.getString(R.string.log_rest_notif_next, squatNext().detail), n.extras.getCharSequence(Notification.EXTRA_TEXT).toString())
    }

    @Test
    fun tapOpensTheSetEditorDeepLink() {
        RestAlarm.schedule(app, 90, "Back squat")
        val tap = shadowOf(posted()!!.contentIntent)
        assertTrue(tap.isActivityIntent)
        assertEquals(ACTION_OPEN, tap.savedIntent.action)
        assertEquals(OpenDest.LogSet, openDestOf(tap.savedIntent))
    }

    @Test
    fun restEndReplacesTheChipWithTheNonPromotedDoneAlert() {
        RestAlarm.schedule(app, 90, "Back squat")
        fireRestEnd(app)
        val n = posted()!!
        assertEquals(res.getString(R.string.log_rest_notif_done_title), n.extras.getCharSequence(Notification.EXTRA_TITLE).toString())
        assertFalse("not promoted", NotificationCompat.isRequestPromotedOngoing(n))
        assertEquals("not ongoing", 0, n.flags and Notification.FLAG_ONGOING_EVENT)
        assertFalse("no chronometer", n.extras.getBoolean(Notification.EXTRA_SHOW_CHRONOMETER))
        assertEquals(OpenDest.LogSet, openDestOf(shadowOf(n.contentIntent).savedIntent))
    }

    @Test
    fun nothingIsPostedWhileTheLoopShowsTheRest() {
        RestAlarm.setInAppVisible(app, true)
        RestAlarm.schedule(app, 90, "Back squat")
        assertNull(posted())
        // Closing the loop moves the countdown into the notification, and back.
        RestAlarm.setInAppVisible(app, false)
        assertTrue(NotificationCompat.isRequestPromotedOngoing(posted()!!))
        RestAlarm.setInAppVisible(app, true)
        assertNull(posted())
    }

    @Test
    fun aSwipedAwayCountdownIsNotPostedAgainForThatRest() {
        RestAlarm.schedule(app, 90, "Back squat")
        val delete = shadowOf(posted()!!.deleteIntent)
        assertTrue(delete.isBroadcastIntent)
        assertEquals(RestDismissReceiver::class.java.name, delete.savedIntent.component?.className)
        // The user swipes it away: the system cancels it and sends the delete intent.
        nm.cancel(RestAlarm.COUNTDOWN_NOTIF_ID)
        RestDismissReceiver().onReceive(app, delete.savedIntent)

        RestAlarm.setInAppVisible(app, true)
        RestAlarm.setInAppVisible(app, false)
        assertNull("not reposted", posted())
        // The rest-done alert still comes.
        fireRestEnd(app)
        assertEquals(res.getString(R.string.log_rest_notif_done_title), posted()!!.extras.getCharSequence(Notification.EXTRA_TITLE).toString())

        // The next rest posts its countdown again.
        RestAlarm.schedule(app, 60, "Back squat")
        assertTrue(NotificationCompat.isRequestPromotedOngoing(posted()!!))
    }

    @Test
    fun aDismissalOfAnOlderRestLeavesTheRunningOneAlone() {
        RestAlarm.schedule(app, 90, "Back squat")
        // A delete broadcast of the previous rest arriving after the next one started.
        val stale = Intent(app, RestDismissReceiver::class.java)
            .putExtra(RestAlarm.EXTRA_REST_START, RestAlarm.running.value!!.startMillis)
        AppClock.offsetMillis += 5_000L
        try {
            RestAlarm.schedule(app, 90, "Back squat")
            RestDismissReceiver().onReceive(app, stale)
            RestAlarm.setInAppVisible(app, true)
            RestAlarm.setInAppVisible(app, false)
            assertNotNull(posted())
        } finally {
            AppClock.offsetMillis -= 5_000L
        }
    }

    @Test
    fun withoutNotificationPermissionNothingIsPosted() {
        shadowOf(app).denyPermissions(Manifest.permission.POST_NOTIFICATIONS)
        RestAlarm.schedule(app, 90, "Back squat")
        assertNull(posted())
        fireRestEnd(app)
        assertNull(posted())
    }

    // ── The next-set line, from the set loop's own data ──

    @Test
    fun nextSetNamesTheLoopsSetNumberAndFields() {
        val squat = PrescriptionView(exercise = "Back squat", sets = 4, sets_done = 2, rest_sec = 120)
        val load = restNextLoad(res, null, 100.0, 5, bodyweight = false, weightUnit = WeightUnit.Kg)
        assertEquals("100\u00A0kg × 5", load)
        val next = restNextSet(res, LoopPrimary.LogPlannedSet, "Back squat", squat, null, 3, load)
        val detail = res.getString(R.string.log_rest_notif_join, res.getString(R.string.log_set_header_n_of_n, 3, 4), load)
        assertEquals(RestAlarm.NextSet("Back squat", detail), next)
        assertEquals(res.getString(R.string.log_rest_notif_next, detail), countdownText(res, "Back squat", next))
    }

    @Test
    fun aCompleteItemPointsAtTheNextItemsFirstSet() {
        val squat = PrescriptionView(exercise = "Back squat", sets = 3, sets_done = 3, done = true)
        val bench = PrescriptionView(exercise = "Bench press", sets = 3)
        val next = restNextSet(res, LoopPrimary.Next, "Back squat", squat, bench, 4, "100\u00A0kg × 5")
        val firstSet = res.getString(R.string.log_set_header_n_of_n, 1, 3)
        assertEquals(RestAlarm.NextSet("Bench press", firstSet), next)
        assertEquals(
            res.getString(R.string.log_rest_notif_next, res.getString(R.string.log_rest_notif_join, "Bench press", firstSet)),
            countdownText(res, "Back squat", next),
        )
    }

    @Test
    fun offPlanExtraAndFinishLines() {
        assertEquals(
            RestAlarm.NextSet("Curl", res.getString(R.string.log_rest_notif_join, res.getString(R.string.log_set_header_n, 2), "12\u00A0kg × 10")),
            restNextSet(res, LoopPrimary.LogOffPlanSet, "Curl", null, null, 2, "12\u00A0kg × 10"),
        )
        assertEquals(
            RestAlarm.NextSet("Curl", res.getString(R.string.log_rest_notif_extra_set)),
            restNextSet(res, LoopPrimary.LogExtraSet, "Curl", null, null, 2, null),
        )
        assertNull(restNextSet(res, LoopPrimary.FinishSession, "Curl", null, null, 2, null))
        assertNull(countdownText(res, "Curl", null))
        // A bodyweight rung names its reps, a hold its seconds; nothing while unentered.
        assertEquals("12 reps", restNextLoad(res, null, 0.0, 12, bodyweight = true, weightUnit = WeightUnit.Kg))
        assertEquals("30\u00A0s hold", restNextLoad(res, 30, 0.0, null, bodyweight = true, weightUnit = WeightUnit.Kg))
        assertNull(restNextLoad(res, null, null, 5, bodyweight = false, weightUnit = WeightUnit.Kg))
    }
}
