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
import android.os.Build
import android.os.Bundle
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.Composable
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import app.milestone.widget.WidgetDayKind
import app.milestone.widget.WidgetSnapshot
import app.milestone.widget.WidgetSnapshotStore
import app.milestone.widget.drawable
import kotlin.math.abs
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.flow.filter
import kotlinx.coroutines.launch
import kotlinx.datetime.TimeZone

/**
 * The session reminder on Android ([SessionReminder]). At the chosen local time an
 * inexact, idle-allowed alarm ([SessionReminderReceiver]) sets the next day's alarm
 * and posts at most one notification per local day when today has a planned
 * session that is still open ([SessionReminder.due]): its title, one line of what
 * is planned, a tap that opens Today and one action that starts it. It reads only
 * the widget snapshot ([WidgetSnapshotStore]), never the core. A posted reminder
 * follows later snapshots and goes once its session is done, its run records, its
 * action is tapped or its local day has passed. The alarm is set again after it
 * fires, after boot or an app update, on a time zone or clock change, when the app
 * starts and when the setting changes.
 */
internal object AndroidReminder : SessionReminderPort {
    internal const val CHANNEL = "session_reminder"
    internal const val ACTION_FIRE = "app.milestone.action.SESSION_REMINDER"

    /** The local days a reminder was posted for, in [SessionReminder.store]. */
    internal const val KEY_REMINDED_DAYS = "reminded_epoch_days"

    /** How far from the latest post a reminded day is kept. A time zone change moves
     *  the local date by two days at most; a clock set back further reminds again. */
    private const val REMINDED_DAYS_KEPT = 7L

    // The shown reminder's local day and kind, read back from the posted notification.
    private const val EXTRA_DAY = "app.milestone.reminder.epoch_day"
    private const val EXTRA_KIND = "app.milestone.reminder.kind"

    /** Set the alarm (a force stop clears it; the next process sets it again) and
     *  follow every stored snapshot ([onSnapshot]) and a run starting to record
     *  ([onRecording]). */
    override fun start(ctx: Context) {
        val app = ctx.applicationContext
        reschedule(app)
        AppProcess.scope.launch {
            // The revision ticks on every write and stale mark; the one stored now
            // was already followed when it was written.
            WidgetSnapshotStore.revision.drop(1).collect { WidgetSnapshotStore.read(app)?.let { onSnapshot(app, it) } }
        }
        AppProcess.scope.launch {
            RunSession.tracking.filter { it }.collect { onRecording(app) }
        }
    }

    /** Set the next alarm when the reminder is on, else remove it. Replaces any
     *  earlier one (same PendingIntent). */
    override fun reschedule(ctx: Context) {
        val app = ctx.applicationContext
        val am = app.getSystemService(AlarmManager::class.java) ?: return
        val s = SessionReminder.setting(app)
        if (!s.enabled) {
            am.cancel(alarmIntent(app))
            return
        }
        // Inexact: a reminder minutes late is fine, and it needs no exact-alarm grant.
        val next = SessionReminder.nextFireMillis(AppClock.nowMillis(), s.minuteOfDay, TimeZone.currentSystemDefault())
        am.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, next, alarmIntent(app))
    }

    override fun takeDown(ctx: Context) {
        nm(ctx)?.cancel(AndroidIds.REMINDER_NOTIFICATION)
    }

    /** 07:30 is another instant now, and the local day may have changed. */
    override fun onTimeChanged(ctx: Context) {
        reschedule(ctx)
        onDayChanged(ctx)
    }

    /** POST_NOTIFICATIONS granted (API 33+) and the app's notifications not blocked. */
    override fun canNotify(ctx: Context): Boolean =
        canPostNotifications(ctx) && NotificationManagerCompat.from(ctx).areNotificationsEnabled()

    override fun checkCanNotify(ctx: Context, onResult: (Boolean) -> Unit) = onResult(canNotify(ctx))

    private fun alarmIntent(app: Context): PendingIntent = PendingIntent.getBroadcast(
        app,
        AndroidIds.REMINDER_ALARM_REQUEST,
        Intent(app, SessionReminderReceiver::class.java).setAction(ACTION_FIRE),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

    internal fun onAlarm(ctx: Context) {
        val app = ctx.applicationContext
        reschedule(app)
        post(app)
    }

    private fun remindedDays(app: Context): List<Long> =
        SessionReminder.store(app).getString(KEY_REMINDED_DAYS, null).orEmpty().split(',').mapNotNull { it.toLongOrNull() }

    /**
     * Post today's reminder when the reminder is on, notifications are allowed,
     * no reminder was posted for today yet and the stored snapshot shows an open
     * session today ([SessionReminder.due]). True when one was posted.
     */
    internal fun post(ctx: Context, recording: Boolean = RunSession.tracking.value): Boolean {
        val today = todayEpochDay()
        val zone = TimeZone.currentSystemDefault().id
        val app = ctx.applicationContext
        if (!SessionReminder.setting(app).enabled || !canNotify(app)) return false
        val days = remindedDays(app)
        if (today in days) return false
        val reminder = SessionReminder.due(WidgetSnapshotStore.read(app), today, today, zone, recording) ?: return false
        notify(app, reminder, today, silent = false)
        val kept = (days + today).filter { abs(it - today) <= REMINDED_DAYS_KEPT }
        SessionReminder.store(app).edit { putString(KEY_REMINDED_DAYS, kept.joinToString(",")) }
        return true
    }

    /**
     * A snapshot was stored. A shown reminder for another local day goes.
     * A shown reminder for today goes once a snapshot for today has nothing left
     * to remind about ([SessionReminder.due]); otherwise it is redrawn quietly (a
     * changed session).
     */
    private fun onSnapshot(ctx: Context, snapshot: WidgetSnapshot) {
        val today = todayEpochDay()
        val app = ctx.applicationContext
        val nm = nm(app) ?: return
        val day = shownDay(nm) ?: return
        if (day != today) {
            nm.cancel(AndroidIds.REMINDER_NOTIFICATION)
            return
        }
        if (snapshot.stale || snapshot.epochDay != today) return
        val reminder = SessionReminder.due(snapshot, today, today, TimeZone.currentSystemDefault().id, RunSession.tracking.value)
        if (reminder == null || !canNotify(app)) {
            nm.cancel(AndroidIds.REMINDER_NOTIFICATION)
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
        if ((shownDay(nm) ?: return) != today) nm.cancel(AndroidIds.REMINDER_NOTIFICATION)
    }

    /** A shown run reminder goes once a run records: its "Start run" is done. */
    private fun onRecording(ctx: Context) {
        val nm = nm(ctx) ?: return
        if (shown(nm)?.extras?.getString(EXTRA_KIND) == WidgetDayKind.Run.name) nm.cancel(AndroidIds.REMINDER_NOTIFICATION)
    }

    private fun nm(ctx: Context): NotificationManager? =
        ctx.applicationContext.getSystemService(NotificationManager::class.java)

    private fun shown(nm: NotificationManager): Notification? =
        nm.activeNotifications.firstOrNull { it.id == AndroidIds.REMINDER_NOTIFICATION }?.notification

    /** The local day the shown reminder was posted for; null when none is shown. */
    private fun shownDay(nm: NotificationManager): Long? = shown(nm)?.extras?.getLong(EXTRA_DAY, Long.MIN_VALUE)

    private fun notify(app: Context, reminder: Reminder, day: Long, silent: Boolean) {
        ensureChannel(app)
        val res = app.appResources()
        val action = reminder.action
        val start = NotificationCompat.Action.Builder(
            action.icon.drawable,
            res.getString(action.labelRes),
            actionPendingIntent(app, action.dest),
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
            .setTimeoutAfter((nextLocalMidnightMillis(now) - now).coerceAtLeast(1L))
        if (line != null) b.setContentText(line)
        nm(app)?.notify(AndroidIds.REMINDER_NOTIFICATION, b.build())
    }

    /** The action's open intent to [dest], which takes the reminder down when MainActivity
     *  opens. Its own request code: PendingIntents match without extras, so the shared
     *  [openPendingIntent] would lose the mark. */
    private fun actionPendingIntent(app: Context, dest: OpenDest): PendingIntent = PendingIntent.getActivity(
        app,
        AndroidIds.REMINDER_ACTION_REQUEST,
        openIntent(app, dest, cancels = AndroidIds.REMINDER_NOTIFICATION),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

    private fun ensureChannel(app: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val nm = nm(app) ?: return
        val res = app.appResources()
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL, res.getString(R.string.reminder_channel), NotificationManager.IMPORTANCE_DEFAULT)
                .apply { description = res.getString(R.string.reminder_channel_description) },
        )
    }
}

internal actual val sessionReminderPort: SessionReminderPort get() = AndroidReminder

/**
 * The reminder alarm ([AndroidReminder.ACTION_FIRE]) and the system events after
 * which the alarm must be set again: boot and an app update (alarms do not survive
 * either), which may also have changed the local day, taking down a reminder
 * posted for another day ([AndroidReminder.onDayChanged]). Time zone and clock
 * changes and local midnight reach the reminder through [AppRuntime.onTimeChanged].
 */
class SessionReminderReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        AppRuntime.start(context)
        when (intent.action) {
            AndroidReminder.ACTION_FIRE -> AndroidReminder.onAlarm(context)
            Intent.ACTION_BOOT_COMPLETED,
            Intent.ACTION_MY_PACKAGE_REPLACED,
            -> {
                AndroidReminder.reschedule(context)
                AndroidReminder.onDayChanged(context)
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
