package app.milestone

import android.annotation.SuppressLint
import android.Manifest
import android.app.AlarmManager
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * Rest-end alert for the gym-floor set loop. After a logged set with a
 * prescribed rest, an alarm fires when the rest is over. While the loop's rest
 * readout is on a resumed screen the in-app cue (haptic + spoken announcement)
 * covers it and no notification is posted. Once the sheet is closed, the app is
 * backgrounded or the screen is off, a silent countdown notification shows the
 * rest ticking down, and at zero an alerting notification (channel sound +
 * vibration, both user-adjustable in the system channel settings, and silenced
 * entirely by the Profile "Rest timer alert" toggle) says the rest is done.
 * Nothing is posted without the notification permission; the in-app
 * readout still works.
 *
 * The alarm is an in-process [AlarmManager.OnAlarmListener] set with
 * `setExact`, which needs no exact-alarm permission (only PendingIntent alarms
 * do), so the cue is exact on every API level. It lives as long as the process;
 * if the process dies the countdown notification simply times out.
 */
internal object RestAlarm {
    private const val CHANNEL_COUNTDOWN = "rest_countdown"
    private const val CHANNEL_DONE = "rest_done"
    private const val NOTIF_ID = 4207
    private const val ALARM_TAG = "lollop:rest-end"
    // The rest-done alert clears itself if it is never looked at.
    private const val DONE_TIMEOUT_MS = 10 * 60_000L

    /** A rest the loop scheduled ([AppClock] millis). Kept after the end (the
     *  overtime readout) until cancelled or replaced. */
    data class RestRun(val startMillis: Long, val endAtMillis: Long, val exercise: String)

    private val _running = MutableStateFlow<RestRun?>(null)
    val running: StateFlow<RestRun?> = _running.asStateFlow()

    private var listener: AlarmManager.OnAlarmListener? = null
    // Wall-clock end of the running rest ([AppClock] millis) and its exercise;
    // null when no rest is running.
    private var endAtMillis: Long? = null
    private var exercise: String = ""
    private var inAppVisible = false

    /** Start (or restart) the rest that follows a set of [exerciseName]. A set with
     *  no prescribed rest leaves a running rest alone. */
    // Lint can't tell the OnAlarmListener `setExact` overload (exempt from
    // SCHEDULE_EXACT_ALARM) from the PendingIntent one.
    @SuppressLint("ScheduleExactAlarm")
    fun schedule(ctx: Context, restSec: Int, exerciseName: String) {
        if (restSec <= 0) return
        val app = ctx.applicationContext
        cancel(app)
        val now = AppClock.nowMillis()
        endAtMillis = now + restSec * 1000L
        exercise = exerciseName
        _running.value = RestRun(now, now + restSec * 1000L, exerciseName)
        if (!inAppVisible) postCountdown(app)
        val l = AlarmManager.OnAlarmListener {
            listener = null
            onRestEnd(app)
        }
        listener = l
        val am = app.getSystemService(AlarmManager::class.java) ?: return
        val trigger = SystemClock.elapsedRealtime() + restSec * 1000L
        am.setExact(AlarmManager.ELAPSED_REALTIME_WAKEUP, trigger, ALARM_TAG, l, Handler(Looper.getMainLooper()))
    }

    /** Drop the running rest (an undone set) and anything it posted. */
    fun cancel(ctx: Context) {
        val app = ctx.applicationContext
        listener?.let { app.getSystemService(AlarmManager::class.java)?.cancel(it) }
        listener = null
        endAtMillis = null
        _running.value = null
        app.getSystemService(NotificationManager::class.java)?.cancel(NOTIF_ID)
    }

    /** The loop's rest readout entered/left a resumed screen. Leaving it while a
     *  rest is still running moves the countdown into a notification. */
    fun setInAppVisible(ctx: Context, visible: Boolean) {
        inAppVisible = visible
        val app = ctx.applicationContext
        if (visible) {
            app.getSystemService(NotificationManager::class.java)?.cancel(NOTIF_ID)
        } else if (listener != null) {
            postCountdown(app)
        }
    }

    private fun onRestEnd(app: Context) {
        endAtMillis = null
        val nm = app.getSystemService(NotificationManager::class.java) ?: return
        if (inAppVisible || !canNotify(app)) {
            nm.cancel(NOTIF_ID)
            return
        }
        ensureChannels(app)
        val n = NotificationCompat.Builder(app, CHANNEL_DONE)
            .setSmallIcon(R.drawable.ic_stat_mark)
            .setContentTitle(app.getString(R.string.log_rest_notif_done_title))
            .setContentText(app.getString(R.string.log_rest_notif_done_body, exercise))
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setCategory(NotificationCompat.CATEGORY_REMINDER)
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .setContentIntent(openAppIntent(app))
            .setAutoCancel(true)
            .setTimeoutAfter(DONE_TIMEOUT_MS)
            // Profile "Rest timer alert" off: still say the rest is done, but
            // without the channel's sound or vibration.
            .setSilent(!AppSettings.restAlert.value)
            .build()
        nm.notify(NOTIF_ID, n)
    }

    private fun postCountdown(app: Context) {
        val end = endAtMillis ?: return
        val leftMs = end - AppClock.nowMillis()
        if (leftMs <= 0 || !canNotify(app)) return
        ensureChannels(app)
        val n: Notification = NotificationCompat.Builder(app, CHANNEL_COUNTDOWN)
            .setSmallIcon(R.drawable.ic_stat_mark)
            .setContentTitle(app.getString(R.string.log_rest_notif_title, exercise))
            // A live countdown chronometer to the rest end (system wall time).
            .setUsesChronometer(true)
            .setChronometerCountDown(true)
            .setWhen(System.currentTimeMillis() + leftMs)
            .setShowWhen(true)
            .setOnlyAlertOnce(true)
            .setSilent(true)
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .setContentIntent(openAppIntent(app))
            .setTimeoutAfter(leftMs)
            .build()
        app.getSystemService(NotificationManager::class.java)?.notify(NOTIF_ID, n)
    }

    private fun canNotify(app: Context): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
            ContextCompat.checkSelfPermission(app, Manifest.permission.POST_NOTIFICATIONS) ==
            PackageManager.PERMISSION_GRANTED

    private fun ensureChannels(app: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val nm = app.getSystemService(NotificationManager::class.java) ?: return
        nm.createNotificationChannel(
            NotificationChannel(
                CHANNEL_COUNTDOWN,
                app.getString(R.string.log_rest_channel_countdown),
                NotificationManager.IMPORTANCE_LOW,
            ),
        )
        nm.createNotificationChannel(
            NotificationChannel(
                CHANNEL_DONE,
                app.getString(R.string.log_rest_channel_done),
                NotificationManager.IMPORTANCE_HIGH,
            ).apply { enableVibration(true) },
        )
    }

    private fun openAppIntent(app: Context): PendingIntent {
        val launch = app.packageManager.getLaunchIntentForPackage(app.packageName)
            ?: Intent(app, MainActivity::class.java)
        return PendingIntent.getActivity(
            app, NOTIF_ID, launch,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
    }
}
