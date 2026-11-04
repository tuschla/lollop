package app.milestone

import android.Manifest
import android.app.AlarmManager
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.SharedPreferences
import android.os.Bundle
import android.os.Build
import android.text.format.DateFormat
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.Composable
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.edit
import app.milestone.widget.WidgetDayKind
import app.milestone.widget.WidgetMidnight
import app.milestone.widget.WidgetSnapshot
import app.milestone.widget.WidgetSnapshotStore
import java.time.Instant
import java.time.LocalTime
import java.time.ZoneId
import java.time.ZonedDateTime
import kotlin.math.abs

/**
 * The daily session reminder (Profile › During training, off by default). At the
 * chosen local time an inexact, idle-allowed alarm ([SessionReminderReceiver]) sets
 * the next day's alarm and posts at most one notification per local day when
 * today has a planned session that is still open ([reminderOf]): its title, one
 * line of what is planned, a tap that opens Today and one action that starts it.
 * It reads only the widget snapshot ([WidgetSnapshotStore]), never the core. A
 * posted reminder follows later snapshots and goes once its session is done, its
 * run records, its action is tapped or its local day has passed. The alarm is set
 * again after it fires, after boot or an app update, on a time zone or clock
 * change, when the app starts and when the setting changes.
 */
internal actual object SessionReminder {
    internal const val CHANNEL = "session_reminder"
    internal const val NOTIF_ID = 4301
    private const val ALARM_REQUEST = 4302
    internal const val ACTION_FIRE = "app.milestone.action.SESSION_REMINDER"

    /** 07:30, the time a first switch-on uses. */
    actual const val DEFAULT_MINUTE_OF_DAY = 7 * 60 + 30

    /** The oldest snapshot whose days ahead still stand in for today. */
    actual const val AHEAD_MAX_DAYS = 7L

    private const val PREFS = "session_reminder"
    private const val KEY_ENABLED = "enabled"
    private const val KEY_MINUTE = "minute_of_day"
    private const val KEY_REMINDED_DAYS = "reminded_epoch_days"
    private const val ACTION_REQUEST = 4303

    /** How far from the latest post a reminded day is kept. A time zone change moves
     *  the local date by two days at most; a clock set back further reminds again. */
    private const val REMINDED_DAYS_KEPT = 7L

    // The shown reminder's local day and kind, read back from the posted notification.
    private const val EXTRA_DAY = "app.milestone.reminder.epoch_day"
    private const val EXTRA_KIND = "app.milestone.reminder.kind"

    /** On the action's open intent: the tap takes the reminder down ([onOpenIntent]). */
    private const val EXTRA_FROM_REMINDER = "app.milestone.reminder.action"

    actual fun setting(ctx: Context): SessionReminderSetting {
        val p = prefs(ctx)
        return SessionReminderSetting(
            enabled = p.getBoolean(KEY_ENABLED, false),
            minuteOfDay = p.getInt(KEY_MINUTE, DEFAULT_MINUTE_OF_DAY).coerceIn(0, MINUTES_PER_DAY - 1),
        )
    }

    /** The local days a reminder was posted for, near the day of the last post. */
    private fun remindedDays(app: Context): List<Long> =
        prefs(app).getString(KEY_REMINDED_DAYS, null).orEmpty().split(',').mapNotNull { it.toLongOrNull() }

    /** Switch the reminder on or off; off also takes down a posted reminder. */
    actual fun setEnabled(ctx: Context, enabled: Boolean) {
        val app = ctx.applicationContext
        prefs(app).edit { putBoolean(KEY_ENABLED, enabled) }
        if (!enabled) cancelNotification(app)
        reschedule(app)
    }

    /** The reminder's local time of day, in minutes after midnight. */
    actual fun setMinuteOfDay(ctx: Context, minuteOfDay: Int) {
        require(minuteOfDay in 0 until MINUTES_PER_DAY) { "minute of day $minuteOfDay" }
        val app = ctx.applicationContext
        prefs(app).edit { putInt(KEY_MINUTE, minuteOfDay) }
        reschedule(app)
    }

    /** Set the next reminder alarm when the reminder is on, else remove it.
     *  Replaces any earlier one (same PendingIntent). */
    fun reschedule(ctx: Context) {
        val app = ctx.applicationContext
        val am = app.getSystemService(AlarmManager::class.java) ?: return
        val s = setting(app)
        if (!s.enabled) {
            am.cancel(alarmIntent(app))
            return
        }
        // Inexact: a reminder minutes late is fine, and it needs no exact-alarm grant.
        val next = nextFireMillis(AppClock.nowMillis(), s.minuteOfDay, ZoneId.systemDefault())
        am.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, next, alarmIntent(app))
    }

    /** The first local [minuteOfDay] strictly after [nowMillis] in [zone]. A time
     *  that a daylight-saving jump skips moves forward by the gap. */
    fun nextFireMillis(nowMillis: Long, minuteOfDay: Int, zone: ZoneId): Long {
        val now = Instant.ofEpochMilli(nowMillis).atZone(zone)
        val time = LocalTime.of(minuteOfDay / 60, minuteOfDay % 60)
        var at = ZonedDateTime.of(now.toLocalDate(), time, zone)
        if (!at.isAfter(now)) at = ZonedDateTime.of(now.toLocalDate().plusDays(1), time, zone)
        return at.toInstant().toEpochMilli()
    }

    private fun alarmIntent(app: Context): PendingIntent = PendingIntent.getBroadcast(
        app,
        ALARM_REQUEST,
        Intent(app, SessionReminderReceiver::class.java).setAction(ACTION_FIRE),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

    /** The alarm went off: set tomorrow's, then remind about today. */
    internal fun onAlarm(ctx: Context) {
        val app = ctx.applicationContext
        reschedule(app)
        post(app)
    }

    /**
     * Post today's reminder when the reminder is on, notifications are allowed,
     * no reminder was posted for today yet and the stored snapshot shows an open
     * session today ([reminderOf]). True when one was posted.
     */
    internal fun post(ctx: Context, recording: Boolean = RunSession.tracking.value): Boolean {
        val today = todayEpochDay()
        val zone = ZoneId.systemDefault().id
        val app = ctx.applicationContext
        if (!setting(app).enabled || !canNotify(app)) return false
        val days = remindedDays(app)
        if (today in days) return false
        val reminder = reminderOf(WidgetSnapshotStore.read(app), today, zone) ?: return false
        // A run already recording is today's run under way.
        if (recording && reminder.kind == WidgetDayKind.Run) return false
        notify(app, reminder, today, silent = false)
        val kept = (days + today).filter { abs(it - today) <= REMINDED_DAYS_KEPT }
        prefs(app).edit { putString(KEY_REMINDED_DAYS, kept.joinToString(",")) }
        return true
    }

    /**
     * A fresh snapshot was stored. A shown reminder for another local day goes.
     * A shown reminder for today goes once a snapshot for today has the session
     * done, today no longer a session day, or a run day while a run records;
     * otherwise it is redrawn quietly (a changed session).
     */
    internal fun onSnapshot(ctx: Context, snapshot: WidgetSnapshot) {
        val today = todayEpochDay()
        val recording = RunSession.tracking.value
        val app = ctx.applicationContext
        val nm = nm(app) ?: return
        val day = shownDay(nm) ?: return
        if (day != today) {
            nm.cancel(NOTIF_ID)
            return
        }
        if (snapshot.stale || snapshot.epochDay != today) return
        val reminder = reminderOf(snapshot, today, ZoneId.systemDefault().id)
        if (reminder == null || !canNotify(app) || (recording && reminder.kind == WidgetDayKind.Run)) {
            nm.cancel(NOTIF_ID)
        } else {
            notify(app, reminder, today, silent = true)
        }
    }

    /**
     * The local day may have changed (the midnight tick, a time zone or clock
     * change): a shown reminder posted for a day other than [today] goes. Its
     * timeout alone runs to the midnight of the zone and clock it was posted in.
     */
    internal fun onDayChanged(ctx: Context, today: Long = todayEpochDay()) {
        val nm = nm(ctx) ?: return
        if ((shownDay(nm) ?: return) != today) nm.cancel(NOTIF_ID)
    }

    /** A shown run reminder goes once a run records ([RecordingWatch]): its "Start
     *  run" is done. */
    internal fun onRecording(ctx: Context) {
        val nm = nm(ctx) ?: return
        if (shown(nm)?.extras?.getString(EXTRA_KIND) == WidgetDayKind.Run.name) nm.cancel(NOTIF_ID)
    }

    /** MainActivity got [intent]: the reminder's action was tapped, so the reminder goes
     *  (a tap on an action does not auto-cancel the notification). */
    internal fun onOpenIntent(ctx: Context, intent: Intent?) {
        if (intent?.getBooleanExtra(EXTRA_FROM_REMINDER, false) == true) cancelNotification(ctx)
    }

    private fun nm(ctx: Context): NotificationManager? =
        ctx.applicationContext.getSystemService(NotificationManager::class.java)

    private fun shown(nm: NotificationManager): Notification? =
        nm.activeNotifications.firstOrNull { it.id == NOTIF_ID }?.notification

    /** The local day the shown reminder was posted for; null when none is shown. */
    private fun shownDay(nm: NotificationManager): Long? = shown(nm)?.extras?.getLong(EXTRA_DAY, Long.MIN_VALUE)

    private fun cancelNotification(ctx: Context) {
        nm(ctx)?.cancel(NOTIF_ID)
    }

    /** [reminder] for local day [day]. */
    private fun notify(app: Context, reminder: Reminder, day: Long, silent: Boolean) {
        ensureChannel(app)
        val res = app.latinDigitResources()
        val start = NotificationCompat.Action.Builder(
            if (reminder.kind == WidgetDayKind.Run) R.drawable.ic_content_track_run else R.drawable.ic_content_set_dumbbell,
            reminderActionLabel(res, reminder),
            actionPendingIntent(app, reminder.dest),
        )
            // From the lock screen the session opens after unlock.
            .setAuthenticationRequired(true)
            .build()
        val line = reminderLine(res, reminder)
        val now = AppClock.nowMillis()
        val b = NotificationCompat.Builder(app, CHANNEL)
            .setSmallIcon(R.drawable.ic_stat_mark)
            .setColor(notificationAccent(app))
            .setContentTitle(reminder.title.ifBlank { res.getString(R.string.reminder_title_fallback) })
            .setCategory(NotificationCompat.CATEGORY_REMINDER)
            .setPriority(NotificationCompat.PRIORITY_DEFAULT)
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .setContentIntent(openPendingIntent(app, OpenDest.Today))
            .addAction(start)
            .setAutoCancel(true)
            .setOnlyAlertOnce(true)
            .setSilent(silent)
            .addExtras(Bundle().apply { putLong(EXTRA_DAY, day); putString(EXTRA_KIND, reminder.kind.name) })
            // Today's reminder never stands into tomorrow.
            .setTimeoutAfter((WidgetMidnight.nextMidnightMillis(now) - now).coerceAtLeast(1L))
        if (line != null) b.setContentText(line)
        nm(app)?.notify(NOTIF_ID, b.build())
    }

    /** The action's open intent to [dest], marked so that MainActivity takes the
     *  reminder down ([onOpenIntent]). Its own request code: PendingIntents match
     *  without extras, so the shared [openPendingIntent] would lose the mark. */
    private fun actionPendingIntent(app: Context, dest: OpenDest): PendingIntent = PendingIntent.getActivity(
        app,
        ACTION_REQUEST,
        openIntent(app, dest).putExtra(EXTRA_FROM_REMINDER, true),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

    /** POST_NOTIFICATIONS granted (API 33+) and the app's notifications not blocked. */
    actual fun canNotify(ctx: Context): Boolean =
        canPostNotifications(ctx) && NotificationManagerCompat.from(ctx).areNotificationsEnabled()

    actual fun checkCanNotify(ctx: Context, onResult: (Boolean) -> Unit) = onResult(canNotify(ctx))

    private fun ensureChannel(app: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val nm = nm(app) ?: return
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL, app.getString(R.string.reminder_channel), NotificationManager.IMPORTANCE_DEFAULT)
                .apply { description = app.getString(R.string.reminder_channel_description) },
        )
    }

    private fun prefs(ctx: Context): SharedPreferences =
        ctx.applicationContext.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
}

private const val MINUTES_PER_DAY = 24 * 60

/**
 * The reminder alarm ([SessionReminder.ACTION_FIRE]) and the system events after
 * which the alarm must be set again: boot, an app update (alarms do not survive
 * either), a time zone change (07:30 is another instant) and a clock change.
 * Each of these may also have changed the local day, which takes down a reminder
 * posted for another day ([SessionReminder.onDayChanged]).
 */
class SessionReminderReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        // A process started for the alarm has not read the theme the reminder's colour follows.
        AppSettings.load(context)
        when (intent.action) {
            SessionReminder.ACTION_FIRE -> SessionReminder.onAlarm(context)
            Intent.ACTION_BOOT_COMPLETED,
            Intent.ACTION_MY_PACKAGE_REPLACED,
            Intent.ACTION_TIMEZONE_CHANGED,
            Intent.ACTION_TIME_CHANGED,
            -> {
                SessionReminder.reschedule(context)
                SessionReminder.onDayChanged(context)
            }
        }
    }
}

/** Android 13+: the POST_NOTIFICATIONS request, launched only while it is not
 *  granted. */
@Composable
internal actual fun rememberNotificationPermissionRequest(ctx: Context, onResult: () -> Unit): () -> Unit {
    val permission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { onResult() }
    return {
        if (!canPostNotifications(ctx)) permission.launch(Manifest.permission.POST_NOTIFICATIONS)
    }
}

/** The app's notification settings; its details page where a build lacks them. */
internal actual fun openNotificationSettings(ctx: Context) = ctx.openFirst(
    appNotificationSettingsIntent(ctx).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
    appDetailsIntent(ctx).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
)

internal actual fun uses24HourClock(ctx: Context): Boolean = DateFormat.is24HourFormat(ctx)
