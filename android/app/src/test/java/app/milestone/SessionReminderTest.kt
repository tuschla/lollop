package app.milestone

import android.Manifest
import android.app.AlarmManager
import android.app.Application
import android.app.Notification
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.os.Looper
import androidx.test.core.app.ApplicationProvider
import app.milestone.qa.QaHarness
import app.milestone.widget.WidgetAheadDay
import app.milestone.widget.WidgetDayKind
import app.milestone.widget.WidgetItem
import app.milestone.widget.WidgetItemKind
import app.milestone.widget.WidgetSnapshot
import app.milestone.widget.WidgetSnapshotStore
import app.milestone.widget.widgetSnapshotOf
import java.io.File
import java.time.LocalDateTime
import java.time.ZoneId
import java.util.TimeZone
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.shadows.ShadowAlarmManager

/**
 * The daily session reminder: the alarm follows the setting, the time and the time
 * zone and comes back after boot; the receiver posts for an open lift or run day
 * (from today's snapshot or an earlier snapshot's days ahead) with the right
 * action, and nothing on a rest day, during a hold, without a plan, for a stale
 * or missing day, once the session is done or without the notification
 * permission; a newer snapshot takes a posted reminder down.
 */
@RunWith(RobolectricTestRunner::class)
class SessionReminderTest {
    private val app: Application = ApplicationProvider.getApplicationContext()
    private val res get() = app.resources
    private val nm get() = app.getSystemService(NotificationManager::class.java)
    private val am get() = app.getSystemService(AlarmManager::class.java)
    private val savedZone: TimeZone = TimeZone.getDefault()
    private val today get() = todayEpochDay()
    private val zone get() = ZoneId.systemDefault().id

    @Before
    fun setUp() {
        TimeZone.setDefault(TimeZone.getTimeZone("Europe/Berlin"))
        shadowOf(app).grantPermissions(Manifest.permission.POST_NOTIFICATIONS)
        File(app.filesDir, WidgetSnapshotStore.FILE_NAME).delete()
        SessionReminder.setEnabled(app, false)
        SessionReminder.setMinuteOfDay(app, SessionReminder.DEFAULT_MINUTE_OF_DAY)
        RunSession.tracking.value = false
    }

    @After
    fun tearDown() {
        SessionReminder.setEnabled(app, false)
        TimeZone.setDefault(savedZone)
        RunSession.tracking.value = false
    }

    // ── Fixtures ─────────────────────────────────────────────────────────────

    private val liftItems = listOf(
        WidgetItem(WidgetItemKind.Lift, "Back squat", "3 × 5 @ 100 kg"),
        WidgetItem(WidgetItemKind.Lift, "Bench press", "3 × 8 @ 60 kg"),
        WidgetItem(WidgetItemKind.Lift, "Row", "3 × 10 @ 50 kg"),
    )
    private val runItems = listOf(WidgetItem(WidgetItemKind.Run, detail = "8 km", pace = "5:40–6:00 /km"))

    private fun liftDay(day: Long = today, items: List<WidgetItem> = liftItems, done: Boolean = false) =
        WidgetSnapshot(epochDay = day, kind = WidgetDayKind.Lift, title = "Upper body", items = items, sessionDone = done, zone = zone)

    private fun runDay(day: Long = today) =
        WidgetSnapshot(epochDay = day, kind = WidgetDayKind.Run, title = "Easy run 8 km", items = runItems, trackable = true, zone = zone)

    private fun store(s: WidgetSnapshot) {
        WidgetSnapshotStore.write(app, s)
    }

    private fun posted(): Notification? = shadowOf(nm).getNotification(SessionReminder.NOTIF_ID)
    private fun title(n: Notification?) = n?.extras?.getCharSequence(Notification.EXTRA_TITLE)?.toString()
    private fun text(n: Notification?) = n?.extras?.getCharSequence(Notification.EXTRA_TEXT)?.toString()
    private fun destOf(pi: PendingIntent): String? = shadowOf(pi).savedIntent.getStringExtra(EXTRA_DEST)
    private fun alarms(): List<ShadowAlarmManager.ScheduledAlarm> = shadowOf(am).scheduledAlarms
    private fun fire() = SessionReminderReceiver().onReceive(app, Intent(SessionReminder.ACTION_FIRE))
    private fun millis(at: String, z: String) = LocalDateTime.parse(at).atZone(ZoneId.of(z)).toInstant().toEpochMilli()

    /** Forget the days already reminded (one reminder per local day), so a test
     *  can fire again on the same date. */
    private fun forgetRemindedDays() {
        app.getSharedPreferences("session_reminder", 0).edit().remove("reminded_epoch_days").commit()
    }

    // ── Next time ────────────────────────────────────────────────────────────

    @Test
    fun nextFireIsTheNextLocalTimeInTheZone() {
        val berlin = ZoneId.of("Europe/Berlin")
        val m = 7 * 60 + 30
        assertEquals(millis("2026-10-29T07:30", "Europe/Berlin"), SessionReminder.nextFireMillis(millis("2026-10-29T06:00", "Europe/Berlin"), m, berlin))
        // At or after the time: tomorrow.
        assertEquals(millis("2026-10-30T07:30", "Europe/Berlin"), SessionReminder.nextFireMillis(millis("2026-10-29T07:30", "Europe/Berlin"), m, berlin))
        assertEquals(millis("2026-10-30T07:30", "Europe/Berlin"), SessionReminder.nextFireMillis(millis("2026-10-29T21:00", "Europe/Berlin"), m, berlin))
        // The same instant in another zone: that zone's 07:30.
        val ny = ZoneId.of("America/New_York")
        assertEquals(millis("2026-10-29T07:30", "America/New_York"), SessionReminder.nextFireMillis(millis("2026-10-29T06:00", "Europe/Berlin"), m, ny))
        // A time the spring-forward jump skips moves forward by the gap.
        assertEquals(millis("2026-03-29T03:30", "Europe/Berlin"), SessionReminder.nextFireMillis(millis("2026-03-29T01:00", "Europe/Berlin"), 2 * 60 + 30, berlin))
    }

    // ── Scheduling ───────────────────────────────────────────────────────────

    @Test
    fun offByDefaultWithNoAlarm() {
        app.getSharedPreferences("session_reminder", 0).edit().clear().commit()
        assertEquals(SessionReminder.Setting(false, 7 * 60 + 30), SessionReminder.setting(app))
        SessionReminder.reschedule(app)
        assertTrue(alarms().isEmpty())
    }

    @Test
    fun switchingOnSetsOneInexactAlarmAtTheTimeAndOffRemovesIt() {
        val before = AppClock.nowMillis()
        SessionReminder.setEnabled(app, true)
        val a = alarms().single()
        assertEquals(AlarmManager.RTC_WAKEUP, a.type)
        assertTrue("allowed while idle", a.isAllowWhileIdle)
        val expected = setOf(
            SessionReminder.nextFireMillis(before, 7 * 60 + 30, ZoneId.systemDefault()),
            SessionReminder.nextFireMillis(AppClock.nowMillis(), 7 * 60 + 30, ZoneId.systemDefault()),
        )
        assertTrue("trigger ${a.triggerAtTime} is the next 07:30", a.triggerAtTime in expected)
        val op = shadowOf(a.operation)
        assertTrue(op.isBroadcastIntent)
        assertTrue("immutable", op.flags and PendingIntent.FLAG_IMMUTABLE != 0)
        assertEquals(SessionReminderReceiver::class.java.name, op.savedIntent.component?.className)
        assertEquals(SessionReminder.ACTION_FIRE, op.savedIntent.action)

        // A new time replaces the alarm.
        SessionReminder.setMinuteOfDay(app, 18 * 60 + 15)
        val b = alarms().single()
        val now = AppClock.nowMillis()
        assertEquals(SessionReminder.nextFireMillis(now, 18 * 60 + 15, ZoneId.systemDefault()), b.triggerAtTime)
        assertEquals(18 * 60 + 15, SessionReminder.setting(app).minuteOfDay)

        SessionReminder.setEnabled(app, false)
        assertTrue(alarms().isEmpty())
        // Back on: the chosen time stays.
        SessionReminder.setEnabled(app, true)
        assertEquals(18 * 60 + 15, SessionReminder.setting(app).minuteOfDay)
    }

    @Test
    fun timeZoneChangeMovesTheAlarmToTheNewZonesTime() {
        SessionReminder.setEnabled(app, true)
        val berlin = alarms().single().triggerAtTime
        TimeZone.setDefault(TimeZone.getTimeZone("America/New_York"))
        SessionReminderReceiver().onReceive(app, Intent(Intent.ACTION_TIMEZONE_CHANGED))
        val ny = alarms().single().triggerAtTime
        assertNotEquals(berlin, ny)
        val at = java.time.Instant.ofEpochMilli(ny).atZone(ZoneId.of("America/New_York"))
        assertEquals(7, at.hour)
        assertEquals(30, at.minute)
    }

    @Test
    fun bootAndUpdateSetTheAlarmAgainOnlyWhenOn() {
        SessionReminder.setEnabled(app, true)
        // A reboot drops the app's alarms.
        am.cancel(alarms().single().operation!!)
        assertTrue(alarms().isEmpty())
        SessionReminderReceiver().onReceive(app, Intent(Intent.ACTION_BOOT_COMPLETED))
        assertEquals(1, alarms().size)
        am.cancel(alarms().single().operation!!)
        SessionReminderReceiver().onReceive(app, Intent(Intent.ACTION_MY_PACKAGE_REPLACED))
        assertEquals(1, alarms().size)

        SessionReminder.setEnabled(app, false)
        SessionReminderReceiver().onReceive(app, Intent(Intent.ACTION_BOOT_COMPLETED))
        assertTrue(alarms().isEmpty())
    }

    @Test
    fun firingPostsAndSetsTomorrowsAlarm() {
        SessionReminder.setEnabled(app, true)
        store(liftDay())
        am.cancel(alarms().single().operation!!)
        fire()
        assertEquals("Upper body", title(posted()))
        val next = alarms().single().triggerAtTime
        assertTrue(next > AppClock.nowMillis())
    }

    // ── What it posts ────────────────────────────────────────────────────────

    @Test
    fun liftDayPostsTitleLineAndLogFirstSet() {
        SessionReminder.setEnabled(app, true)
        store(liftDay())
        fire()
        val n = posted()!!
        assertEquals("Upper body", title(n))
        assertEquals("3 exercises · Back squat first", text(n))
        assertEquals(SessionReminder.CHANNEL, n.channelId)
        assertTrue("auto-cancel", n.flags and Notification.FLAG_AUTO_CANCEL != 0)
        assertEquals(Notification.CATEGORY_REMINDER, n.category)
        assertEquals(1, n.actions.size)
        assertEquals("Log first set", n.actions[0].title.toString())
        assertTrue(shadowOf(n.actions[0].actionIntent).isActivityIntent)
        assertEquals(OpenDest.LogSet.wire, destOf(n.actions[0].actionIntent))
        assertEquals(OpenDest.Today.wire, destOf(n.contentIntent))
        val channel = nm.getNotificationChannel(SessionReminder.CHANNEL)
        assertEquals(NotificationManager.IMPORTANCE_DEFAULT, channel.importance)
        assertEquals("Session reminders", channel.name.toString())
    }

    @Test
    fun runDayPostsDistancePaceAndStartRunToTheTracker() {
        SessionReminder.setEnabled(app, true)
        store(runDay())
        fire()
        val n = posted()!!
        assertEquals("Easy run 8 km", title(n))
        assertEquals("8 km · 5:40–6:00 /km", text(n))
        assertEquals("Start run", n.actions.single().title.toString())
        assertEquals(OpenDest.Track.wire, destOf(n.actions.single().actionIntent))
        assertEquals(OpenDest.Today.wire, destOf(n.contentIntent))
    }

    @Test
    fun startedLiftDayNamesWhatIsLeft() {
        val started = liftItems.mapIndexed { i, it -> if (i == 0) it.copy(done = true) else it }
        val r = reminderOf(liftDay(items = started), today, zone)!!
        assertEquals("1 of 3 done · Bench press next", reminderLine(res, r))
        assertEquals("Log next set", reminderActionLabel(res, r))
        val single = reminderOf(liftDay(items = liftItems.take(1)), today, zone)!!
        assertEquals("Back squat · 3 × 5 @ 100 kg", reminderLine(res, single))
    }

    @Test
    fun nothingWithoutAnOpenSessionToday() {
        SessionReminder.setEnabled(app, true)
        val cases = listOf(
            "rest day" to WidgetSnapshot(epochDay = today, kind = WidgetDayKind.Rest, restDay = true, zone = zone),
            "hold" to WidgetSnapshot(epochDay = today, kind = WidgetDayKind.Paused, holdReason = "pain", zone = zone),
            "under 18" to WidgetSnapshot(epochDay = today, kind = WidgetDayKind.Paused, holdReason = "youth", zone = zone),
            "no plan" to WidgetSnapshot(epochDay = today, kind = WidgetDayKind.NoPlan, zone = zone),
            "no profile" to WidgetSnapshot(epochDay = today, kind = WidgetDayKind.Setup, zone = zone),
            "done" to liftDay(done = true),
            "run done" to runDay().copy(sessionDone = true, trackable = false),
            "stale today" to liftDay().copy(stale = true),
            "yesterday, nothing ahead" to liftDay(day = today - 1),
            "tomorrow" to liftDay(day = today + 1),
        )
        for ((name, s) in cases) {
            store(s)
            fire()
            assertNull(name, posted())
        }
        File(app.filesDir, WidgetSnapshotStore.FILE_NAME).delete()
        fire()
        assertNull("no snapshot", posted())
    }

    @Test
    fun nothingWhileOffOrWithoutPermission() {
        store(liftDay())
        fire()
        assertNull("setting off", posted())
        SessionReminder.setEnabled(app, true)
        shadowOf(app).denyPermissions(Manifest.permission.POST_NOTIFICATIONS)
        assertFalse(SessionReminder.canNotify(app))
        fire()
        assertNull("permission off", posted())
        // The alarm still runs, so granting it later is enough.
        assertEquals(1, alarms().size)
        shadowOf(app).grantPermissions(Manifest.permission.POST_NOTIFICATIONS)
        fire()
        assertEquals("Upper body", title(posted()))
    }

    @Test
    fun noRunReminderWhileARunRecords() {
        SessionReminder.setEnabled(app, true)
        store(runDay())
        assertFalse(SessionReminder.post(app, recording = true))
        assertNull(posted())
    }

    // ── An earlier snapshot's days ahead ─────────────────────────────────────

    private fun yesterday(
        kind: WidgetDayKind = WidgetDayKind.Rest,
        age: Long = 1,
        z: String = zone,
        ahead: List<WidgetAheadDay> = listOf(WidgetAheadDay(today, WidgetDayKind.Lift, "Lower body", liftItems)),
        stale: Boolean = false,
    ) = WidgetSnapshot(epochDay = today - age, kind = kind, stale = stale, ahead = ahead, zone = z)

    @Test
    fun yesterdaysSnapshotRemindsFromItsDayAhead() {
        SessionReminder.setEnabled(app, true)
        // Marked stale at midnight: still the plan's word for today.
        store(yesterday(stale = true))
        fire()
        assertEquals("Lower body", title(posted()))
        assertEquals("Log first set", posted()!!.actions.single().title.toString())
        nm.cancelAll()
        forgetRemindedDays()
        // A run day ahead, from a lift day's snapshot a week old.
        store(yesterday(kind = WidgetDayKind.Lift, age = 7, ahead = listOf(WidgetAheadDay(today, WidgetDayKind.Run, "Easy run 8 km", runItems))))
        fire()
        assertEquals(OpenDest.Track.wire, destOf(posted()!!.actions.single().actionIntent))
    }

    @Test
    fun earlierSnapshotThatIsHeldOldOrFromAnotherZoneIsSilent() {
        SessionReminder.setEnabled(app, true)
        val cases = listOf(
            "held" to yesterday(kind = WidgetDayKind.Paused),
            "no plan" to yesterday(kind = WidgetDayKind.NoPlan),
            "eight days old" to yesterday(age = 8),
            "other zone" to yesterday(z = "America/New_York"),
            "zone unknown" to yesterday(z = ""),
            "today not ahead" to yesterday(ahead = listOf(WidgetAheadDay(today + 1, WidgetDayKind.Lift, "Lower body", liftItems))),
            "empty ahead" to yesterday(ahead = emptyList()),
        )
        for ((name, s) in cases) {
            store(s)
            fire()
            assertNull(name, posted())
        }
    }

    @Test
    fun snapshotCarriesPlannedDaysAheadAndItsZone() {
        val day = today
        val lift = PrescriptionView(exercise = "Back squat", face_lead = "3 × 5 @ 100 kg")
        val run = PrescriptionView(run_head = "8 km", run_head_km = 8.0, run_target = RunTargetView(pace_label_km = "5:40–6:00 /km"))
        val week = listOf(
            SessionPlanView(day, "Upper body", "Lift(Strength)", "next", listOf(lift)),
            SessionPlanView(day + 1, "", "Rest", "rest"),
            SessionPlanView(day + 2, "Easy run 8 km", "Run(Easy)", "planned", listOf(run)),
            SessionPlanView(day + 3, "Lower body", "Lift(Strength)", "planned", listOf(lift)),
        )
        val model = ViewModel(today_epoch_day = day, profile = ProfileView(), next_session = week[0], week_plan = week)
        val s = widgetSnapshotOf(model, DistanceUnit.Km, zone = "Europe/Berlin")
        assertEquals("Europe/Berlin", s.zone)
        assertEquals(listOf(day + 2, day + 3), s.ahead.map { it.epochDay })
        assertEquals(WidgetDayKind.Run, s.ahead[0].kind)
        assertEquals(WidgetItem(WidgetItemKind.Run, detail = "8 km", pace = "5:40–6:00 /km"), s.ahead[0].items.single())
        assertEquals(WidgetDayKind.Lift, s.ahead[1].kind)
        // Paused: nothing ahead.
        val held = widgetSnapshotOf(model.copy(train_blocked = true, hold_reason = "pain"), DistanceUnit.Km, zone = "Europe/Berlin")
        assertTrue(held.ahead.isEmpty())
    }

    // ── A newer snapshot ─────────────────────────────────────────────────────

    @Test
    fun doneSessionOrAPlanChangeTakesThePostedReminderDown() {
        SessionReminder.setEnabled(app, true)
        store(liftDay())
        fire()
        assertTrue(posted() != null)
        store(liftDay(done = true))
        assertNull("session done", posted())

        store(runDay())
        forgetRemindedDays()
        fire()
        assertTrue(posted() != null)
        store(WidgetSnapshot(epochDay = today, kind = WidgetDayKind.Rest, restDay = true, zone = zone))
        assertNull("today is a rest day now", posted())

        store(runDay())
        forgetRemindedDays()
        fire()
        store(WidgetSnapshot(epochDay = today, kind = WidgetDayKind.Paused, holdReason = "pain", zone = zone))
        assertNull("training paused", posted())
    }

    @Test
    fun aChangedSessionRedrawsAShownReminderAndNoneIsPostedUnasked() {
        SessionReminder.setEnabled(app, true)
        // A snapshot write alone never posts.
        store(liftDay())
        assertNull(posted())
        fire()
        val started = liftItems.mapIndexed { i, it -> if (i == 0) it.copy(done = true) else it }
        store(liftDay(items = started))
        assertEquals("1 of 3 done · Bench press next", text(posted()))
        assertEquals("Log next set", posted()!!.actions.single().title.toString())
    }

    @Test
    fun switchingOffTakesTheReminderDown() {
        SessionReminder.setEnabled(app, true)
        store(liftDay())
        fire()
        assertTrue(posted() != null)
        SessionReminder.setEnabled(app, false)
        assertNull(posted())
        assertTrue(alarms().isEmpty())
    }

    // ── Stale reminders ──────────────────────────────────────────────────────

    /** "Start run" opens the tracker; an action tap does not auto-cancel the
     *  notification, so the reminder must go once the run records (the reminder
     *  posts nothing "while a run is recording"). */
    @Test
    fun aShownRunReminderGoesOnceTheRunRecords() {
        SessionReminder.setEnabled(app, true)
        store(runDay())
        fire()
        assertEquals(OpenDest.Track.wire, destOf(posted()!!.actions.single().actionIntent))
        shadowOf(app).grantPermissions(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION)
        val start = Intent(app, RunTrackingService::class.java).setAction("app.milestone.action.START")
        val controller = Robolectric.buildService(RunTrackingService::class.java, start).create().startCommand(0, 1)
        try {
            shadowOf(Looper.getMainLooper()).idle()
            assertTrue("the run records", RunSession.tracking.value)
            assertNull("the 'Start run' reminder stays up beside the recording run", posted())
        } finally {
            controller.destroy()
            RunSession.reset()
        }
    }

    /** A tap on the action opens the app with a mark that takes the reminder
     *  down; the widget's and tile's open intents carry no mark. */
    @Test
    fun aTapOnTheActionTakesTheReminderDown() {
        SessionReminder.setEnabled(app, true)
        store(liftDay())
        fire()
        val n = posted()!!
        SessionReminder.onOpenIntent(app, shadowOf(n.contentIntent).savedIntent)
        SessionReminder.onOpenIntent(app, openIntent(app, OpenDest.LogSet))
        assertTrue("the content intent and other surfaces leave it up", posted() != null)
        val action = shadowOf(n.actions.single().actionIntent).savedIntent
        assertEquals(OpenDest.LogSet, openDestOf(action))
        SessionReminder.onOpenIntent(app, action)
        assertNull("the action tap takes it down", posted())
    }

    /** The reminder fired at 07:30 in Berlin and was opened; a move to London
     *  (06:31 there, the same date) sets the alarm for 07:30 London, which must
     *  not remind about the same day a second time. */
    @Test
    fun atMostOneReminderPerLocalDayAcrossATimeZoneChange() {
        AppClock.offsetMillis = millis("2026-10-29T07:30:30", "Europe/Berlin") - System.currentTimeMillis()
        try {
            val day = today
            SessionReminder.setEnabled(app, true)
            store(liftDay())
            fire()
            assertEquals("Upper body", title(posted()))
            // Opened from the shade: auto-cancel takes it down.
            nm.cancel(SessionReminder.NOTIF_ID)

            TimeZone.setDefault(TimeZone.getTimeZone("Europe/London"))
            SessionReminderReceiver().onReceive(app, Intent(Intent.ACTION_TIMEZONE_CHANGED))
            // The app is open: the snapshot is rewritten in the new zone, same day.
            store(liftDay())
            assertEquals(day, today)
            val next = alarms().single().triggerAtTime
            AppClock.offsetMillis = next + 30_000 - System.currentTimeMillis()
            assertEquals("07:30 London is still ${java.time.LocalDate.ofEpochDay(day)}", day, today)
            fire()
            assertNull("a second reminder for the same day", posted())
        } finally {
            AppClock.offsetMillis = 0L
        }
    }

    /** Reminded on Thursday and on Friday; the clock is then set back to Thursday
     *  morning, so Thursday's 07:30 comes again: no second reminder for Thursday. */
    @Test
    fun aClockSetBackADayDoesNotRemindThatDayAgain() {
        try {
            SessionReminder.setEnabled(app, true)
            for (at in listOf("2026-10-29T07:30:30", "2026-10-30T07:30:30")) {
                AppClock.offsetMillis = millis(at, "Europe/Berlin") - System.currentTimeMillis()
                store(liftDay())
                fire()
                assertEquals(at, "Upper body", title(posted()))
                nm.cancel(SessionReminder.NOTIF_ID)
            }
            AppClock.offsetMillis = millis("2026-10-29T06:00:00", "Europe/Berlin") - System.currentTimeMillis()
            SessionReminderReceiver().onReceive(app, Intent(Intent.ACTION_TIME_CHANGED))
            store(liftDay())
            val next = alarms().single().triggerAtTime
            assertEquals(millis("2026-10-29T07:30", "Europe/Berlin"), next)
            AppClock.offsetMillis = next + 30_000 - System.currentTimeMillis()
            fire()
            assertNull("a second reminder for Thursday", posted())
        } finally {
            AppClock.offsetMillis = 0L
        }
    }

    /** An evening reminder in Chicago; at 23:30 the phone crosses into Eastern
     *  time, where it is already 00:30 the next day. The reminder's timeout runs
     *  to Chicago midnight (01:00 there), so without a check on the day change
     *  Thursday's session shows as today's reminder on Friday. */
    @Test
    fun aTimeZoneChangePastMidnightTakesYesterdaysReminderDown() {
        TimeZone.setDefault(TimeZone.getTimeZone("America/Chicago"))
        val postedAt = millis("2026-10-29T20:00:30", "America/Chicago")
        AppClock.offsetMillis = postedAt - System.currentTimeMillis()
        try {
            SessionReminder.setMinuteOfDay(app, 20 * 60)
            SessionReminder.setEnabled(app, true)
            store(liftDay())
            fire()
            val n = posted()!!
            assertEquals("Upper body", title(n))

            val crossing = millis("2026-10-29T23:30:00", "America/Chicago")
            AppClock.offsetMillis = crossing - System.currentTimeMillis()
            TimeZone.setDefault(TimeZone.getTimeZone("America/New_York"))
            // Both receivers get the broadcast.
            SessionReminderReceiver().onReceive(app, Intent(Intent.ACTION_TIMEZONE_CHANGED))
            WidgetSnapshotStore.markStale(app, force = true, today = todayEpochDay())
            assertEquals("it is Friday in New York", java.time.LocalDate.parse("2026-10-30").toEpochDay(), today)
            assertTrue("the system timeout has not run out yet", n.timeoutAfter > crossing - postedAt)
            assertNull("yesterday's reminder still shows", posted())
        } finally {
            AppClock.offsetMillis = 0L
        }
    }

    /** The midnight tick (with the widget or tile placed) takes a reminder for the
     *  day that ended down, so it does not rest on the timeout alone (a clock set
     *  forward after the post moves the timeout past midnight). */
    @Test
    fun theMidnightTickTakesYesterdaysReminderDown() {
        val postedAt = millis("2026-10-29T07:30:30", "Europe/Berlin")
        AppClock.offsetMillis = postedAt - System.currentTimeMillis()
        try {
            SessionReminder.setEnabled(app, true)
            store(liftDay())
            fire()
            assertTrue(posted() != null)
            AppClock.offsetMillis = millis("2026-10-29T23:59:00", "Europe/Berlin") - System.currentTimeMillis()
            WidgetSnapshotStore.markStale(app, force = false, today = todayEpochDay())
            assertTrue("still the same day", posted() != null)
            AppClock.offsetMillis = millis("2026-10-30T00:00:05", "Europe/Berlin") - System.currentTimeMillis()
            WidgetSnapshotStore.markStale(app, force = false, today = todayEpochDay())
            assertNull("Thursday's reminder on Friday", posted())
        } finally {
            AppClock.offsetMillis = 0L
        }
    }

    // ── Days ahead from the real core ────────────────────────────────────────

    /** Log every item of [s] (dated [day]) to the real core; the model after it. */
    private fun trainAll(day: Long, s: SessionPlanView): ViewModel {
        var v: ViewModel? = null
        var id = day * 1_000
        val at = day * 86_400L + 43_200L
        for (item in s.items) {
            if (isRunItem(item, s.session_type)) {
                v = Core.send(Event.LogRun(distanceKm = item.run_head_km ?: 6.0, durationMin = 40.0, hrPctMax = 0.7, observedAt = at, entryId = ++id)).view
            } else if (!isReturnCheckItem(item, s.session_type)) {
                repeat(item.sets.coerceAtLeast(1)) {
                    v = Core.send(
                        Event.LogSet(
                            exercise = item.exercise, weightKg = item.load_kg ?: 0.0, reps = item.reps_high.coerceAtLeast(1), rpe = 7.0,
                            observedAt = at, entryId = ++id, holdSec = item.hold_high_sec.takeIf { item.timed_hold && it > 0 },
                        ),
                    ).view
                }
            }
        }
        return checkNotNull(v)
    }

    /** Fire the reminder at 07:30:30 UTC on [day] from [snapshot], the one the app last wrote. */
    private fun fireOn(day: Long, snapshot: WidgetSnapshot) {
        TimeZone.setDefault(TimeZone.getTimeZone("UTC"))
        AppClock.offsetMillis = day * 86_400_000L + (7 * 60 + 30) * 60_000L + 30_000L - System.currentTimeMillis()
        assertEquals(day, today)
        SessionReminder.setEnabled(app, true)
        store(snapshot)
        fire()
    }

    /** Set up on Sunday 2025-11-09 (calisthenics) and train that day's session; the
     *  app is not opened on Monday. In its own week Tuesday is a rest day, so the
     *  reminder must not post on Tuesday; Sunday's model used to name Tuesday as
     *  the next session (the start week's placement a week later). */
    @Test
    fun aSundaySnapshotNeverRemindsOnTheNextWeeksRestDay() {
        val sun = java.time.LocalDate.parse("2025-11-09").toEpochDay()
        Core.resetForTest()
        try {
            Core.send(Event.SetToday(sun))
            Core.send(QaHarness.calisthenicsProfile())
            val first = checkNotNull(Core.send(Event.GeneratePlan(sun)).view.next_session)
            assertEquals("a session on the setup Sunday", sun, first.epoch_day)
            val sunday = trainAll(sun, first)
            val next = checkNotNull(sunday.next_session) { "Sunday's model names a next session" }
            val snapshot = widgetSnapshotOf(sunday, DistanceUnit.Km, "UTC")

            val tue = sun + 2
            val tuesday = Core.send(Event.SetToday(tue)).view
            assertTrue(
                "Sunday's next session is a session day in its own week",
                tuesday.week_plan.first { it.epoch_day == next.epoch_day }.items.isNotEmpty(),
            )
            assertEquals("in its own week Tuesday is a rest day", "rest", tuesday.week_plan.first { it.epoch_day == tue }.status)
            assertNull(reminderOf(widgetSnapshotOf(tuesday, DistanceUnit.Km, "UTC"), tue, "UTC"))
            fireOn(tue, snapshot)
            assertNull("a reminder on a rest day: '${title(posted())}'", posted())
        } finally {
            AppClock.offsetMillis = 0L
            Core.resetForTest()
        }
    }

    /** A hybrid 10K plan set up on Friday 2025-11-14; the app is last opened on
     *  Sunday, whose long run is still open, and not on Monday. Monday has an easy
     *  run: Sunday's snapshot carries it from the core's next week, so Monday's
     *  reminder posts. */
    @Test
    fun mondaysSessionIsRemindedWhenTheAppWasLastOpenedOnSunday() {
        val fri = java.time.LocalDate.parse("2025-11-14").toEpochDay()
        Core.resetForTest()
        try {
            Core.send(Event.SetToday(fri))
            val base = QaHarness.strengthProfile() as Event.SetProfile
            Core.send(base.copy(goalDistance = GoalDistance.TenK, runningDaysPerWeek = 3, runningKmPerWeek = 25.0))
            Core.send(Event.GeneratePlan(fri))
            val sun = fri + 2
            val sunday = Core.send(Event.SetToday(sun)).view
            assertEquals("Sunday's session is open", sun, sunday.next_session?.epoch_day)
            val snapshot = widgetSnapshotOf(sunday, DistanceUnit.Km, "UTC")

            val mon = sun + 1
            val monday = checkNotNull(Core.send(Event.SetToday(mon)).view.todaysPlan()) { "Monday has a session" }
            fireOn(mon, snapshot)
            assertEquals("Monday's reminder", monday.title, title(posted()))
        } finally {
            AppClock.offsetMillis = 0L
            Core.resetForTest()
        }
    }
}
