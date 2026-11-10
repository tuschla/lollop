package app.milestone

import android.Manifest
import android.app.Application
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.ui.graphics.toArgb
import androidx.test.core.app.ApplicationProvider
import app.milestone.widget.WidgetDayKind
import app.milestone.widget.WidgetItem
import app.milestone.widget.WidgetItemKind
import app.milestone.widget.WidgetSnapshot
import app.milestone.widget.WidgetSnapshotStore
import java.io.File
import java.time.ZoneId
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf

/**
 * Every lollop notification looks like one family: the route-mark small icon, the
 * app theme's accent as its colour (the wallpaper accent only where the app uses
 * it; never colorized), shown in full on the lock screen, in the category of what
 * it is: the run and the rest countdown a workout, the rest-done alert and the
 * session reminder a reminder.
 */
@RunWith(RobolectricTestRunner::class)
class NotificationStyleTest {
    private val app: Application = ApplicationProvider.getApplicationContext()
    private val nm get() = app.getSystemService(NotificationManager::class.java)

    @Before
    fun setUp() {
        shadowOf(app).grantPermissions(Manifest.permission.POST_NOTIFICATIONS)
        resetRest(app)
        File(app.filesDir, AppFileNames.WIDGET_SNAPSHOT).delete()
        SessionReminder.setEnabled(app, false)
    }

    @After
    fun tearDown() {
        resetRest(app)
        SessionReminder.setEnabled(app, false)
        AppSettings.setTheme(app, AppTheme.Paper)
        RuntimeEnvironment.setQualifiers("+notnight")
    }

    private fun pending(code: Int): PendingIntent =
        PendingIntent.getActivity(app, code, Intent(app, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE)

    private fun run(): Notification = buildRunNotification(
        runNotificationBuilder(app, RunTrackingService.CHANNEL, "Tracking run", pending(0), pending(1)),
        RunNotifFrame("Moving 23:41 · 4.12 km · 5:45 /km now", "23:41", null),
    )

    private fun restCountdown(): Notification {
        RestAlarm.schedule(app, 90, "Back squat")
        return postedRest(app)!!
    }

    private fun restDone(): Notification {
        RestAlarm.schedule(app, 90, "Back squat")
        fireRestEnd(app)
        return postedRest(app)!!
    }

    private fun reminder(): Notification {
        SessionReminder.setEnabled(app, true)
        WidgetSnapshotStore.write(
            app,
            WidgetSnapshot(
                epochDay = todayEpochDay(), kind = WidgetDayKind.Lift, title = "Upper body",
                items = listOf(WidgetItem(WidgetItemKind.Lift, "Back squat", "3 × 5 @ 100 kg")),
                zone = ZoneId.systemDefault().id,
            ),
        )
        SessionReminderReceiver().onReceive(app, Intent(AndroidReminder.ACTION_FIRE))
        return shadowOf(nm).getNotification(AndroidIds.REMINDER_NOTIFICATION)!!
    }

    private fun all() = listOf(
        "run" to run(),
        "rest countdown" to restCountdown(),
        "rest done" to restDone(),
        "reminder" to reminder(),
    )

    @Test
    fun everyNotificationSharesIconAccentAndLockScreenVisibility() {
        AppSettings.setTheme(app, AppTheme.Rose)
        for ((what, n) in all()) {
            assertEquals("$what icon", R.drawable.ic_stat_mark, n.smallIcon.resId)
            assertEquals("$what colour: Rose's light accent", AppTheme.Rose.light.accent.toArgb(), n.color)
            assertFalse("$what not colorized", n.extras.getBoolean(Notification.EXTRA_COLORIZED))
            assertEquals("$what on the lock screen", Notification.VISIBILITY_PUBLIC, n.visibility)
        }
    }

    @Test
    fun liveUpdatesShowOnTheLockScreenWithoutASound() {
        // An install from before: both Live Updates on IMPORTANCE_LOW channels, which
        // System UI's default "hide silent notifications" kept off the lock screen.
        for (old in listOf("run_tracking", "rest_countdown")) {
            nm.createNotificationChannel(NotificationChannel(old, old, NotificationManager.IMPORTANCE_LOW))
        }
        val service = Robolectric.buildService(RunTrackingService::class.java).create()
        try {
            assertEquals(RestAlarmPlatform.CHANNEL_COUNTDOWN, restCountdown().channelId)
            for (id in listOf(RunTrackingService.CHANNEL, RestAlarmPlatform.CHANNEL_COUNTDOWN)) {
                val ch = nm.getNotificationChannel(id)
                assertEquals("$id: not a silent channel (shown on the lock screen)", NotificationManager.IMPORTANCE_DEFAULT, ch.importance)
                assertNull("$id: no sound", ch.sound)
                assertFalse("$id: no vibration", ch.shouldVibrate())
            }
            assertNull("the low run channel is gone", nm.getNotificationChannel("run_tracking"))
            assertNull("the low rest channel is gone", nm.getNotificationChannel("rest_countdown"))
        } finally {
            service.destroy()
        }
    }

    @Test
    fun categoriesSayWhatEachIs() {
        val byWhat = all().toMap()
        assertEquals(Notification.CATEGORY_WORKOUT, byWhat.getValue("run").category)
        assertEquals(Notification.CATEGORY_WORKOUT, byWhat.getValue("rest countdown").category)
        assertEquals(Notification.CATEGORY_REMINDER, byWhat.getValue("rest done").category)
        assertEquals(Notification.CATEGORY_REMINDER, byWhat.getValue("reminder").category)
    }

    @Test
    fun accentFollowsTheShadeModeAndTheSystemAccentSetting() {
        // A dark shade gets the dark palette's accent.
        AppSettings.setTheme(app, AppTheme.Meadow)
        RuntimeEnvironment.setQualifiers("+night")
        assertEquals(AppTheme.Meadow.dark.accent.toArgb(), notificationAccent(app))
        RuntimeEnvironment.setQualifiers("+notnight")
        assertEquals(AppTheme.Meadow.light.accent.toArgb(), notificationAccent(app))
        // Paper with the system accent (the default): the wallpaper accent through the
        // app's contrast guard, like the app and the widget.
        AppSettings.setTheme(app, AppTheme.Paper)
        assertEquals(
            AppTheme.Paper.light.withDynamicAccent(dynamicLightColorScheme(app).primary).accent.toArgb(),
            notificationAccent(app),
        )
        AppSettings.setDynamicAccent(app, false)
        assertEquals(AppTheme.Paper.light.accent.toArgb(), notificationAccent(app))
    }
}
