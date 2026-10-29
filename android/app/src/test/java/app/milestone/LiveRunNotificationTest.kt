package app.milestone

import android.Manifest
import android.app.Application
import android.app.Notification
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Looper
import android.provider.Settings
import androidx.core.app.NotificationCompat
import androidx.test.core.app.ApplicationProvider
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

/** The run-tracking notification as an Android 16 Live Update: the documented
 *  promotion criteria, the status-bar chip text, the guided-run step bar, the same
 *  notification on older Android, and no re-post after the user dismissed it. */
@RunWith(RobolectricTestRunner::class)
class LiveRunNotificationTest {

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @After
    fun tearDown() {
        RunSession.paused.value = false
        RunSession.reset()
    }

    private fun pending(code: Int): PendingIntent =
        PendingIntent.getActivity(ctx, code, Intent(ctx, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE)

    private fun build(frame: RunNotifFrame): Notification = buildRunNotification(
        runNotificationBuilder(ctx, "run_tracking", "Tracking run", pending(0), pending(1)),
        frame,
    )

    private val recordingFrame = RunNotifFrame("Moving 23:41 · 4.12 km · 5:45 /km now", "23:41", null)

    /** The criteria developer.android.com lists for a Live Update, checked on the built
     *  notification. (`Notification.hasPromotableCharacteristics()` is not used: the
     *  Android 16 release build of it still required a colorized notification; the
     *  QPR2 rewrite matches the documented list asserted here.) */
    private fun assertPromotable(n: Notification) {
        assertTrue("requests promotion", n.extras.getBoolean(Notification.EXTRA_REQUEST_PROMOTED_ONGOING))
        assertTrue("ongoing", n.flags and Notification.FLAG_ONGOING_EVENT != 0)
        assertFalse("title", n.extras.getCharSequence(Notification.EXTRA_TITLE).isNullOrEmpty())
        assertNull("no custom content view", n.contentView)
        assertNull("no custom big view", n.bigContentView)
        assertTrue("not a group summary", n.flags and Notification.FLAG_GROUP_SUMMARY == 0)
        assertFalse("not colorized", n.extras.getBoolean(Notification.EXTRA_COLORIZED))
        assertNotNull("dismissal is reported", n.deleteIntent)
    }

    @Test
    @Config(sdk = [36])
    fun recordingNotificationIsAPromotableLiveUpdateWithTheMovingTimeChip() {
        val n = build(recordingFrame)
        assertPromotable(n)
        assertEquals("23:41", n.shortCriticalText)
        // BigTextStyle (a Live Update style) so the always-expanded card wraps the line.
        assertEquals(Notification.BigTextStyle::class.java.name, n.extras.getString(Notification.EXTRA_TEMPLATE))
        assertEquals(recordingFrame.content, n.extras.getCharSequence(Notification.EXTRA_BIG_TEXT).toString())
        assertEquals(recordingFrame.content, n.extras.getCharSequence(Notification.EXTRA_TEXT).toString())
    }

    @Test
    @Config(sdk = [36])
    fun guidedRunShowsAProgressStyleBarOfItsSteps() {
        val bar = StepBar(List(5) { STEP_BAR_UNITS }, 2 * STEP_BAR_UNITS + 50)
        val n = build(recordingFrame.copy(content = "Rep 2 of 6 · 2:00 left · Moving 23:41", steps = bar))
        assertPromotable(n)
        assertEquals(Notification.ProgressStyle::class.java.name, n.extras.getString(Notification.EXTRA_TEMPLATE))
        val style = Notification.Builder.recoverBuilder(ctx, n).style as Notification.ProgressStyle
        assertEquals(5, style.progressSegments.size)
        assertEquals(5 * STEP_BAR_UNITS, style.progressMax)
        assertEquals(250, style.progress)
        // The current step name leads the content line.
        assertTrue(n.extras.getCharSequence(Notification.EXTRA_TEXT).toString().startsWith("Rep 2 of 6"))
    }

    @Test
    @Config(sdk = [34])
    fun belowAndroid16TheSameNotificationPostsAsAnOrdinaryOngoingOne() {
        val n = build(recordingFrame)
        assertTrue(n.flags and Notification.FLAG_ONGOING_EVENT != 0)
        assertEquals("Tracking run", n.extras.getCharSequence(Notification.EXTRA_TITLE).toString())
        assertEquals(recordingFrame.content, n.extras.getCharSequence(Notification.EXTRA_TEXT).toString())
        assertNotNull(n.contentIntent)
        // A guided run falls back to a plain progress bar.
        val guided = build(recordingFrame.copy(steps = StepBar(List(4) { STEP_BAR_UNITS }, 150)))
        assertEquals(4 * STEP_BAR_UNITS, guided.extras.getInt(Notification.EXTRA_PROGRESS_MAX))
        assertEquals(150, guided.extras.getInt(Notification.EXTRA_PROGRESS))
    }

    @Test
    fun chipTextFollowsTheStateAndFitsTheChip() {
        assertEquals(R.string.live_chip_paused, trackingChipRes(recording = true, armed = false, paused = true, signalLost = true))
        assertEquals(R.string.live_chip_no_gps, trackingChipRes(recording = true, armed = false, paused = false, signalLost = true))
        assertNull("recording shows the moving time", trackingChipRes(recording = true, armed = false, paused = false, signalLost = false))
        assertEquals(R.string.live_chip_waiting, trackingChipRes(recording = false, armed = true, paused = false, signalLost = false))
        assertEquals(R.string.live_chip_locating, trackingChipRes(recording = false, armed = false, paused = false, signalLost = false))
        val chips = listOf(R.string.live_chip_paused, R.string.live_chip_no_gps, R.string.live_chip_waiting, R.string.live_chip_locating)
            .map { ctx.getString(it) } + formatElapsed(9 * 3600L + 59 * 60 + 59)
        for (c in chips) {
            assertTrue("chip '$c' is not empty", c.isNotBlank())
            assertTrue("chip '$c' fits $LIVE_CHIP_MAX_CHARS characters", c.length <= LIVE_CHIP_MAX_CHARS)
        }
    }

    @Test
    fun stepBarCountsStepsAndTheDoneShareOfTheCurrentOne() {
        val rep = RunStep(RunStepKind.Rep, number = 2, count = 6, durationSec = 120)
        // Step 3 of 5, a 2:00 rep with 0:30 left: 2 full steps + 75 % of the third.
        assertEquals(
            StepBar(List(5) { STEP_BAR_UNITS }, 275),
            stepBar(StepProgress(rep, index = 2, total = 5, remainingSec = 30, remainingM = null, next = null)),
        )
        // A distance step, 320 m left of 400 m.
        val dist = RunStep(RunStepKind.Rep, distanceM = 400)
        assertEquals(20, stepBar(StepProgress(dist, index = 0, total = 3, remainingSec = null, remainingM = 320, next = null))!!.progress)
        // An open step has no known length: it adds nothing until it ends.
        val open = RunStep(RunStepKind.Main)
        assertEquals(100, stepBar(StepProgress(open, index = 1, total = 3, remainingSec = null, remainingM = null, next = null))!!.progress)
        // A step at its very end stays short of the next boundary.
        assertEquals(99, stepBar(StepProgress(rep, index = 0, total = 3, remainingSec = 0, remainingM = null, next = null))!!.progress)
        // Finished: full bar.
        assertEquals(300, stepBar(StepProgress(null, index = 3, total = 3, remainingSec = null, remainingM = null, next = null))!!.progress)
        // Past the platform's 15-segment limit the bar is one segment.
        assertEquals(listOf(20 * STEP_BAR_UNITS), stepBar(StepProgress(rep, index = 0, total = 20, remainingSec = 120, remainingM = null, next = null))!!.segments)
        assertNull(stepBar(StepProgress(null, index = 0, total = 0, remainingSec = null, remainingM = null, next = null)))
    }

    @Test
    @Config(sdk = [36])
    fun promotionSettingsLinkOpensTheLiveUpdatesSwitchForThisApp() {
        val i = promotedNotificationSettingsIntent(ctx)
        assertEquals(Settings.ACTION_APP_NOTIFICATION_PROMOTION_SETTINGS, i.action)
        assertEquals(ctx.packageName, i.getStringExtra(Settings.EXTRA_APP_PACKAGE))
    }

    @Test
    @Config(sdk = [34])
    fun belowAndroid16NoLiveUpdatesNoticeIsShown() {
        assertFalse(liveUpdatesTurnedOff(ctx))
        assertEquals(Settings.ACTION_APP_NOTIFICATION_SETTINGS, promotedNotificationSettingsIntent(ctx).action)
    }

    @Test
    @Config(sdk = [36])
    fun aDismissedNotificationIsNotRepostedUntilTheNextStart() {
        shadowOf(ctx as Application).grantPermissions(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION)
        val start = Intent(ctx, RunTrackingService::class.java).setAction("app.milestone.action.START")
        val controller = Robolectric.buildService(RunTrackingService::class.java, start).create().startCommand(0, 1)
        val nm = shadowOf(ctx.getSystemService(NotificationManager::class.java))
        fun posted(): Notification = nm.getNotification(1)

        val first = shadowOf(controller.get()).lastForegroundNotification
        assertPromotable(first)
        assertEquals("recording starts at zero moving time", "0:00", first.shortCriticalText)

        // An in-app pause re-posts with the paused chip (the throttled path still works).
        RunSession.paused.value = true
        shadowOf(Looper.getMainLooper()).idle()
        assertEquals(ctx.getString(R.string.live_chip_paused), posted().shortCriticalText)

        // The user swipes it away: the system fires its delete intent at the service.
        val dismissed = shadowOf(posted().deleteIntent).savedIntent
        controller.withIntent(dismissed).startCommand(0, 2)
        RunSession.paused.value = false
        shadowOf(Looper.getMainLooper()).idle()
        assertEquals("no re-post after dismissal", ctx.getString(R.string.live_chip_paused), posted().shortCriticalText)

        // The next explicit start shows it again.
        controller.withIntent(start).startCommand(0, 3)
        assertEquals("the start re-posts it", "0:00", posted().shortCriticalText)
        controller.destroy()
    }
}
