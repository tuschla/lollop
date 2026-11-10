package app.milestone

import android.app.AlarmManager
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.SystemClock
import android.provider.Settings
import androidx.annotation.RequiresApi
import androidx.core.app.NotificationCompat

/**
 * The rest timer on Android. Off the loop's screen a silent ongoing countdown (a
 * promoted Live Update on Android 16+) shows the rest, and at zero an alerting
 * "rest done" notification replaces it. The rest end is an [AlarmManager]
 * broadcast to [RestEndReceiver] with the rest saved in prefs, so the alert posts
 * even from a fresh process. The alarm is exact while the user allows exact alarms
 * (always below Android 12), else inexact: the system may deliver it late.
 */
internal actual object RestAlarmPlatform {
    internal const val CHANNEL_COUNTDOWN = "rest_live"

    /** The countdown's channel before it moved to [ensureLiveChannel] (IMPORTANCE_LOW). */
    private const val RETIRED_CHANNEL_COUNTDOWN = "rest_countdown"
    private const val CHANNEL_DONE = "rest_done"
    // The rest-end alarm's identity: its request code, [ACTION_REST_END] and
    // [RestEndReceiver]. A new rest replaces the alarm, cancel finds it.
    internal const val ACTION_REST_END = "app.milestone.action.REST_END"
    // The rest-done alert clears itself if it is never looked at.
    private const val DONE_TIMEOUT_MS = 10 * 60_000L
    internal const val EXTRA_REST_START = "app.milestone.extra.REST_START"

    // The running rest, saved for a rest-end alarm delivered to a fresh process.
    private const val PREFS = "rest_alarm"
    private const val KEY_START = "start_millis"
    private const val KEY_EXERCISE = "exercise"
    // The next set's exercise the loop reported; "" = it reported none (after the
    // session's last set). Absent until the loop reports.
    private const val KEY_NEXT_EXERCISE = "next_exercise"
    // The exact-alarm notice was dismissed or its action used: not shown again.
    private const val KEY_EXACT_PROMPT_DONE = "exact_alarm_prompt_done"

    actual val countdown = RestCountdown.OffLoop

    actual val exactAlarms: ExactAlarms? = ExactAlarmGrant

    actual fun arm(ctx: PlatformContext, run: RestRun, delayMs: Long): Boolean {
        val app = ctx.applicationContext
        prefs(app).edit {
            putLong(KEY_START, run.startMillis)
            putString(KEY_EXERCISE, run.exercise)
        }
        val am = app.getSystemService(AlarmManager::class.java) ?: return false
        val at = SystemClock.elapsedRealtime() + delayMs
        val op = PendingIntent.getBroadcast(
            app, AndroidIds.REST_END_ALARM_REQUEST,
            restEndIntent(app).putExtra(EXTRA_REST_START, run.startMillis),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val exact = ExactAlarmGrant.allowed(app)
        if (exact) {
            am.setExactAndAllowWhileIdle(AlarmManager.ELAPSED_REALTIME_WAKEUP, at, op)
        } else {
            am.setAndAllowWhileIdle(AlarmManager.ELAPSED_REALTIME_WAKEUP, at, op)
        }
        return !exact
    }

    actual fun renameDone(ctx: PlatformContext, run: RestRun, exercise: String) {
        prefs(ctx.applicationContext).edit { putString(KEY_NEXT_EXERCISE, exercise) }
    }

    actual fun disarm(ctx: PlatformContext) {
        val app = ctx.applicationContext
        PendingIntent.getBroadcast(
            app, AndroidIds.REST_END_ALARM_REQUEST, restEndIntent(app),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_NO_CREATE,
        )?.let { app.getSystemService(AlarmManager::class.java)?.cancel(it) }
        clearSavedRest(app)
        withdrawDone(app)
    }

    actual fun withdrawDone(ctx: PlatformContext) {
        ctx.getSystemService(NotificationManager::class.java)?.cancel(AndroidIds.REST_DONE_NOTIFICATION)
    }

    // The countdown has its own id, apart from the rest-done alert's: its
    // timeout (the rest end) can fire just after the alert is posted, and the
    // system then cancels whatever holds the countdown's id.
    actual fun hideCountdown(ctx: PlatformContext) {
        ctx.getSystemService(NotificationManager::class.java)?.cancel(AndroidIds.REST_COUNTDOWN_NOTIFICATION)
    }

    actual fun showCountdown(ctx: PlatformContext, run: RestRun, next: RestNextSet?) {
        val app = ctx.applicationContext
        val leftMs = run.endAtMillis - AppClock.nowMillis()
        if (leftMs <= 0 || !canPostNotifications(app)) return
        ensureChannels(app)
        // Every argument is a name or a detail the set loop already formatted, so the
        // app's own resources do (no digits are formatted here).
        val res = app.resources
        val text = countdownText(res, run.exercise, next)
        val b = NotificationCompat.Builder(app, CHANNEL_COUNTDOWN)
            .setSmallIcon(R.drawable.ic_stat_mark)
            .setColor(notificationAccent(app))
            .setCategory(NotificationCompat.CATEGORY_WORKOUT)
            .setContentTitle(res.getString(R.string.log_rest_notif_title, run.exercise))
            // A live countdown chronometer to the rest end; a promoted Live Update
            // shows it in the status-bar chip. System UI draws it in the system's
            // digits (the app cannot pick them) and counts against the device's
            // wall clock, so its end is the time left on the device clock.
            .setUsesChronometer(true)
            .setChronometerCountDown(true)
            .setWhen(DeviceClock.nowMillis() + leftMs)
            .setShowWhen(true)
            .setOngoing(true)
            .setRequestPromotedOngoing(true)
            .setOnlyAlertOnce(true)
            .setSilent(true)
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .setContentIntent(openPendingIntent(app, OpenDest.LogSet))
            .setDeleteIntent(dismissIntent(app, run.startMillis))
            .setTimeoutAfter(leftMs)
        if (text != null) {
            b.setContentText(text).setStyle(NotificationCompat.BigTextStyle().bigText(text))
        }
        app.getSystemService(NotificationManager::class.java)?.notify(AndroidIds.REST_COUNTDOWN_NOTIFICATION, b.build())
    }

    /** The rest-end alarm of the rest started at [startMillis] fired, in this
     *  process or in one started for it, which has only the saved rest. An alarm
     *  of a cancelled or replaced rest does nothing. */
    internal fun onRestEnd(ctx: Context, startMillis: Long) {
        val app = ctx.applicationContext
        val saved = prefs(app)
        if (KEY_START !in saved || saved.getLong(KEY_START, 0L) != startMillis) return
        // Not reported yet: the rested exercise. Reported as none: no "next set" line.
        val next = saved.getString(KEY_NEXT_EXERCISE, null) ?: saved.getString(KEY_EXERCISE, null).orEmpty()
        clearSavedRest(app)
        RestAlarm.ended(app)
        if (RestAlarm.inAppVisible || !canPostNotifications(app)) {
            withdrawDone(app)
            return
        }
        val nm = app.getSystemService(NotificationManager::class.java) ?: return
        ensureChannels(app)
        val b = NotificationCompat.Builder(app, CHANNEL_DONE)
            .setSmallIcon(R.drawable.ic_stat_mark)
            .setColor(notificationAccent(app))
            .setContentTitle(app.getString(R.string.log_rest_notif_done_title))
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setCategory(NotificationCompat.CATEGORY_REMINDER)
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .setContentIntent(openPendingIntent(app, OpenDest.LogSet))
            .setAutoCancel(true)
            .setTimeoutAfter(DONE_TIMEOUT_MS)
            // Profile "Rest timer alert" off: still say the rest is done, but
            // without the channel's sound or vibration.
            .setSilent(!AppSettings.restAlert.value)
        if (next.isNotEmpty()) b.setContentText(app.getString(R.string.log_rest_notif_done_body, next))
        nm.notify(AndroidIds.REST_DONE_NOTIFICATION, b.build())
    }

    private fun restEndIntent(app: Context): Intent =
        Intent(app, RestEndReceiver::class.java).setAction(ACTION_REST_END)

    private fun dismissIntent(app: Context, startMillis: Long): PendingIntent =
        PendingIntent.getBroadcast(
            app, AndroidIds.REST_DISMISS_REQUEST,
            Intent(app, RestDismissReceiver::class.java).putExtra(EXTRA_REST_START, startMillis),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )

    private fun prefs(app: Context): KeyValueStore = app.keyValueStore(PREFS)

    private fun clearSavedRest(app: Context) {
        prefs(app).edit {
            remove(KEY_START)
            remove(KEY_EXERCISE)
            remove(KEY_NEXT_EXERCISE)
        }
    }

    // The countdown's channel shows on the lock screen without a sound
    // ([ensureLiveChannel]); the rest-done alert's is the one that alerts.
    private fun ensureChannels(app: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val nm = app.getSystemService(NotificationManager::class.java) ?: return
        ensureLiveChannel(app, CHANNEL_COUNTDOWN, app.getString(R.string.log_rest_channel_countdown), RETIRED_CHANNEL_COUNTDOWN)
        nm.createNotificationChannel(
            NotificationChannel(
                CHANNEL_DONE,
                app.getString(R.string.log_rest_channel_done),
                NotificationManager.IMPORTANCE_HIGH,
            ).apply { enableVibration(true) },
        )
    }

    /** Always allowed below Android 12, then the user's `SCHEDULE_EXACT_ALARM` grant. */
    private object ExactAlarmGrant : ExactAlarms {
        override fun allowed(ctx: PlatformContext): Boolean =
            Build.VERSION.SDK_INT < Build.VERSION_CODES.S ||
                ctx.getSystemService(AlarmManager::class.java)?.canScheduleExactAlarms() == true

        override fun promptDone(ctx: PlatformContext): Boolean =
            prefs(ctx.applicationContext).getBoolean(KEY_EXACT_PROMPT_DONE, false)

        override fun setPromptDone(ctx: PlatformContext) {
            prefs(ctx.applicationContext).edit { putBoolean(KEY_EXACT_PROMPT_DONE, true) }
        }

        override fun openSettings(ctx: PlatformContext) {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) ctx.openFirst(exactAlarmSettingsIntent(ctx), appDetailsIntent(ctx))
        }
    }
}

/** The rest countdown was swiped away: [RestAlarm] does not post it again for
 *  that rest (the rest-done alert still comes). */
class RestDismissReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        AppRuntime.start(context)
        RestAlarm.onCountdownDismissed(intent.getLongExtra(RestAlarmPlatform.EXTRA_REST_START, -1L))
    }
}

/** The rest-end alarm: the rest-done alert replaces the countdown, also in a
 *  process started only for this broadcast. */
class RestEndReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != RestAlarmPlatform.ACTION_REST_END) return
        AppRuntime.start(context)
        RestAlarmPlatform.onRestEnd(context, intent.getLongExtra(RestAlarmPlatform.EXTRA_REST_START, -1L))
    }
}

/** The system screen where the user allows exact alarms for this app (Android 12+). */
@RequiresApi(Build.VERSION_CODES.S)
internal fun exactAlarmSettingsIntent(ctx: Context): Intent =
    Intent(Settings.ACTION_REQUEST_SCHEDULE_EXACT_ALARM, Uri.fromParts("package", ctx.packageName, null))
