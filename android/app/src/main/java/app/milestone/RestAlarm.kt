package app.milestone

import android.app.AlarmManager
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.SharedPreferences
import android.content.res.Resources
import android.net.Uri
import android.os.Build
import android.os.SystemClock
import android.provider.Settings
import androidx.annotation.RequiresApi
import androidx.core.app.NotificationCompat
import androidx.core.content.edit
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * Rest-end alert for the gym-floor set loop. While the loop's rest readout is on
 * a resumed screen the in-app cue covers the rest; otherwise a silent ongoing
 * countdown (a promoted Live Update on Android 16+) shows it, and at zero an
 * alerting "rest done" notification replaces it. The rest end is an [AlarmManager]
 * broadcast to [RestEndReceiver] with the rest saved in prefs, so the alert posts
 * even from a fresh process. The alarm is exact while the user allows exact alarms
 * (always below Android 12), else inexact: the system may deliver it late.
 */
internal object RestAlarm {
    private const val CHANNEL_COUNTDOWN = "rest_countdown"
    private const val CHANNEL_DONE = "rest_done"
    // The countdown and the rest-done alert have their own ids: the countdown's
    // timeout (the rest end) can fire just after the alert is posted, and the
    // system then cancels whatever holds the countdown's id.
    internal const val COUNTDOWN_NOTIF_ID = 4207
    internal const val DONE_NOTIF_ID = 4209
    // The rest-end alarm's identity: this request code, [ACTION_REST_END] and
    // [RestEndReceiver]. A new rest replaces the alarm, cancel finds it.
    private const val ALARM_REQUEST = 4208
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

    /** A rest the loop scheduled ([AppClock] millis). Kept after the end (the
     *  overtime readout) until cancelled or replaced. */
    data class RestRun(val startMillis: Long, val endAtMillis: Long, val exercise: String)

    /** The set the loop logs next, as the countdown names it: its exercise and
     *  "Set 3 of 4 · 100 kg × 5" (null when the loop shows no set line). */
    data class NextSet(val exercise: String, val detail: String?)

    private val _running = MutableStateFlow<RestRun?>(null)
    val running: StateFlow<RestRun?> = _running.asStateFlow()

    private val _exactAlarmPrompt = MutableStateFlow(false)
    /** The set loop shows its one-time notice: exact alarms are not allowed, so
     *  the rest-done alert may come late while the app is in the background. */
    val exactAlarmPrompt: StateFlow<Boolean> = _exactAlarmPrompt.asStateFlow()

    // Wall-clock end of the running rest ([AppClock] millis); null until a rest
    // starts and again once it ends or is cancelled.
    private var endAtMillis: Long? = null
    private var inAppVisible = false
    // What the loop logs next, kept current by the open loop.
    private var nextSet: NextSet? = null
    // The user swiped the running rest's countdown away.
    private var dismissed = false
    // The running rest's alarm is inexact (exact alarms were not allowed).
    private var alarmInexact = false

    /** Start (or restart) the rest that follows a set of [exerciseName]. A set with
     *  no prescribed rest leaves a running rest alone. */
    fun schedule(ctx: Context, restSec: Int, exerciseName: String) {
        if (restSec <= 0) return
        val app = ctx.applicationContext
        cancel(app)
        val now = AppClock.nowMillis()
        val restMs = restSec * 1000L
        endAtMillis = now + restMs
        _running.value = RestRun(now, now + restMs, exerciseName)
        prefs(app).edit { putLong(KEY_START, now).putString(KEY_EXERCISE, exerciseName) }
        if (!inAppVisible) postCountdown(app)
        setAlarm(app, now, restMs)
        if (alarmInexact && !prefs(app).getBoolean(KEY_EXACT_PROMPT_DONE, false)) _exactAlarmPrompt.value = true
    }

    /** Set (or replace) the alarm of the rest started at [startMillis], [delayMs]
     *  from now: exact when allowed, else inexact. */
    private fun setAlarm(app: Context, startMillis: Long, delayMs: Long) {
        val am = app.getSystemService(AlarmManager::class.java) ?: return
        val at = SystemClock.elapsedRealtime() + delayMs
        val op = PendingIntent.getBroadcast(
            app, ALARM_REQUEST,
            restEndIntent(app).putExtra(EXTRA_REST_START, startMillis),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val exact = exactAlarmsAllowed(app)
        if (exact) {
            am.setExactAndAllowWhileIdle(AlarmManager.ELAPSED_REALTIME_WAKEUP, at, op)
        } else {
            am.setAndAllowWhileIdle(AlarmManager.ELAPSED_REALTIME_WAKEUP, at, op)
        }
        alarmInexact = !exact
    }

    private fun restEndIntent(app: Context): Intent =
        Intent(app, RestEndReceiver::class.java).setAction(ACTION_REST_END)

    /** Drop the running rest (an undone set, the loop closed with "Done for now"
     *  or "Finish session"), its alarm and anything it posted. */
    fun cancel(ctx: Context) {
        val app = ctx.applicationContext
        PendingIntent.getBroadcast(
            app, ALARM_REQUEST, restEndIntent(app),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_NO_CREATE,
        )?.let { app.getSystemService(AlarmManager::class.java)?.cancel(it) }
        endAtMillis = null
        nextSet = null
        dismissed = false
        alarmInexact = false
        _running.value = null
        clearSavedRest(app)
        cancelNotifications(app)
    }

    /** The open loop's next set (null: none); the countdown posted after the loop
     *  closes names it, and the rest-done alert names its exercise. */
    fun setNextSet(ctx: Context, next: NextSet?) {
        nextSet = next
        if (endAtMillis == null) return
        prefs(ctx.applicationContext).edit { putString(KEY_NEXT_EXERCISE, next?.exercise.orEmpty()) }
    }

    /** The loop's rest readout entered/left a resumed screen. Leaving it while a
     *  rest is still running moves the countdown into a notification. */
    fun setInAppVisible(ctx: Context, visible: Boolean) {
        inAppVisible = visible
        val app = ctx.applicationContext
        if (visible) {
            cancelNotifications(app)
        } else if (endAtMillis != null) {
            postCountdown(app)
        }
    }

    /** The countdown of the rest started at [startMillis] was swiped away. */
    fun onCountdownDismissed(startMillis: Long) {
        if (_running.value?.startMillis == startMillis) dismissed = true
    }

    /** The set loop is in front again: once exact alarms are allowed (the user is
     *  back from the system screen) the notice goes, and a running rest's inexact
     *  alarm is set again as an exact one. */
    fun recheckExactAlarms(ctx: Context) {
        val app = ctx.applicationContext
        if (!exactAlarmsAllowed(app)) return
        _exactAlarmPrompt.value = false
        val start = _running.value?.startMillis ?: return
        val left = (endAtMillis ?: return) - AppClock.nowMillis()
        if (alarmInexact && left > 0) setAlarm(app, start, left)
    }

    /** The notice was dismissed or its action used: it is not shown again. */
    fun dismissExactAlarmPrompt(ctx: Context) {
        prefs(ctx.applicationContext).edit { putBoolean(KEY_EXACT_PROMPT_DONE, true) }
        _exactAlarmPrompt.value = false
    }

    /** Exact alarms are allowed: always below Android 12, then the user's
     *  `SCHEDULE_EXACT_ALARM` grant. */
    private fun exactAlarmsAllowed(ctx: Context): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.S ||
            ctx.getSystemService(AlarmManager::class.java)?.canScheduleExactAlarms() == true

    /** The rest-end alarm of the rest started at [startMillis] fired, in this
     *  process or in one started for it, which has only the saved rest. An alarm
     *  of a cancelled or replaced rest does nothing. */
    internal fun onRestEnd(ctx: Context, startMillis: Long) {
        val app = ctx.applicationContext
        val saved = prefs(app)
        if (!saved.contains(KEY_START) || saved.getLong(KEY_START, 0L) != startMillis) return
        // Not reported yet: the rested exercise. Reported as none: no "next set" line.
        val next = saved.getString(KEY_NEXT_EXERCISE, null) ?: saved.getString(KEY_EXERCISE, null).orEmpty()
        clearSavedRest(app)
        endAtMillis = null
        alarmInexact = false
        val nm = app.getSystemService(NotificationManager::class.java) ?: return
        nm.cancel(COUNTDOWN_NOTIF_ID)
        if (inAppVisible || !canPostNotifications(app)) {
            nm.cancel(DONE_NOTIF_ID)
            return
        }
        ensureChannels(app)
        val b = NotificationCompat.Builder(app, CHANNEL_DONE)
            .setSmallIcon(R.drawable.ic_stat_mark)
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
        nm.notify(DONE_NOTIF_ID, b.build())
    }

    private fun postCountdown(app: Context) {
        val end = endAtMillis ?: return
        val run = _running.value ?: return
        val leftMs = end - AppClock.nowMillis()
        if (leftMs <= 0 || dismissed || !canPostNotifications(app)) return
        ensureChannels(app)
        // Every argument is a name or a detail the set loop already formatted, so the
        // app's own resources do (no digits are formatted here).
        val res = app.resources
        val text = countdownText(res, run.exercise, nextSet)
        val b = NotificationCompat.Builder(app, CHANNEL_COUNTDOWN)
            .setSmallIcon(R.drawable.ic_stat_mark)
            .setContentTitle(res.getString(R.string.log_rest_notif_title, run.exercise))
            // A live countdown chronometer to the rest end (system wall time);
            // a promoted Live Update shows it in the status-bar chip. System UI
            // draws it in the system's digits; the app cannot pick them.
            .setUsesChronometer(true)
            .setChronometerCountDown(true)
            .setWhen(System.currentTimeMillis() + leftMs)
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
        val n: Notification = b.build()
        app.getSystemService(NotificationManager::class.java)?.notify(COUNTDOWN_NOTIF_ID, n)
    }

    private fun dismissIntent(app: Context, startMillis: Long): PendingIntent =
        PendingIntent.getBroadcast(
            app, COUNTDOWN_NOTIF_ID,
            Intent(app, RestDismissReceiver::class.java).putExtra(EXTRA_REST_START, startMillis),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )

    private fun prefs(app: Context): SharedPreferences = app.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    /** Take down the countdown and the rest-done alert. */
    private fun cancelNotifications(app: Context) {
        val nm = app.getSystemService(NotificationManager::class.java) ?: return
        nm.cancel(COUNTDOWN_NOTIF_ID)
        nm.cancel(DONE_NOTIF_ID)
    }

    private fun clearSavedRest(app: Context) {
        prefs(app).edit { remove(KEY_START).remove(KEY_EXERCISE).remove(KEY_NEXT_EXERCISE) }
    }

    /** Forget what this process holds, as a fresh process starts (tests). The
     *  saved rest, the alarm and anything posted stay. */
    internal fun resetProcessState() {
        endAtMillis = null
        inAppVisible = false
        nextSet = null
        dismissed = false
        alarmInexact = false
        _running.value = null
        _exactAlarmPrompt.value = false
    }

    // Both channels stay above IMPORTANCE_MIN, which a Live Update can't use.
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
}

/** The countdown's body line: "Next: Set 3 of 4 · 100 kg × 5" for another set of
 *  the rested [exercise], "Next: Bench press · Set 1 of 3" when the loop moves on.
 *  Null when the loop shows no next set. Pure. */
internal fun countdownText(res: Resources, exercise: String, next: RestAlarm.NextSet?): String? {
    next ?: return null
    val what = when {
        sameExercise(next.exercise, exercise) -> next.detail ?: return null
        next.detail == null -> next.exercise
        else -> res.getString(R.string.log_rest_notif_join, next.exercise, next.detail)
    }
    return res.getString(R.string.log_rest_notif_next, what)
}

/**
 * The set the loop's primary action logs next, as the set loop shows it: its
 * "Set k of N" (a plan item), "Set k" (an off-plan exercise, [offPlanSetNumber])
 * or "Extra set", then the load and reps in the fields ([load], null when not
 * entered). Once the item is complete it is the next item's first open set. Null
 * when the action logs no set (finish, a blank editor, an edit). Pure.
 */
internal fun restNextSet(
    res: Resources,
    primary: LoopPrimary,
    exercise: String,
    liveItem: PrescriptionView?,
    nextItem: PrescriptionView?,
    offPlanSetNumber: Int,
    load: String?,
): RestAlarm.NextSet? {
    fun joined(set: String) = load?.let { res.getString(R.string.log_rest_notif_join, set, it) } ?: set
    return when (primary) {
        LoopPrimary.LogPlannedSet -> liveItem?.let {
            RestAlarm.NextSet(it.exercise, joined(res.getString(R.string.log_set_header_n_of_n, nextSetNumber(it), plannedSetCount(it))))
        }
        LoopPrimary.LogOffPlanSet ->
            RestAlarm.NextSet(exercise, joined(res.getString(R.string.log_set_header_n, offPlanSetNumber)))
        LoopPrimary.LogExtraSet ->
            RestAlarm.NextSet(exercise, joined(res.getString(R.string.log_rest_notif_extra_set)))
        LoopPrimary.Next -> nextItem?.let {
            RestAlarm.NextSet(it.exercise, res.getString(R.string.log_set_header_n_of_n, nextSetNumber(it), plannedSetCount(it)))
        }
        LoopPrimary.Save, LoopPrimary.LogBlank, LoopPrimary.FinishSession -> null
    }
}

/** The set loop's fields as the countdown names the next set: "100 kg × 5",
 *  "30 s hold", "12 reps" for a bodyweight rung. Null while the load or reps are
 *  not entered. Pure. */
internal fun restNextLoad(
    res: Resources,
    holdSec: Int?,
    weightKg: Double?,
    reps: Int?,
    bodyweight: Boolean,
    weightUnit: WeightUnit,
): String? = when {
    holdSec != null -> liftSetLine(res, holdSec, weightKg ?: 0.0, 1, 0.0, false, weightUnit)
    reps == null -> null
    bodyweight -> res.getQuantityString(R.plurals.log_band_reps, reps, reps)
    weightKg == null -> null
    else -> liftSetLine(res, null, weightKg, reps, 0.0, false, weightUnit)
}

/** The rest countdown was swiped away: [RestAlarm] does not post it again for
 *  that rest (the rest-done alert still comes). */
class RestDismissReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        RestAlarm.onCountdownDismissed(intent.getLongExtra(RestAlarm.EXTRA_REST_START, -1L))
    }
}

/** The rest-end alarm: [RestAlarm] posts the rest-done alert in place of the
 *  countdown, also in a process started only for this broadcast. */
class RestEndReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != RestAlarm.ACTION_REST_END) return
        // A process started for the alarm has not read the "Rest timer alert" setting.
        AppSettings.load(context)
        RestAlarm.onRestEnd(context, intent.getLongExtra(RestAlarm.EXTRA_REST_START, -1L))
    }
}

/** The system screen where the user allows exact alarms for this app (Android 12+). */
@RequiresApi(Build.VERSION_CODES.S)
internal fun exactAlarmSettingsIntent(ctx: Context): Intent =
    Intent(Settings.ACTION_REQUEST_SCHEDULE_EXACT_ALARM, Uri.fromParts("package", ctx.packageName, null))

/** Open [exactAlarmSettingsIntent]; the app's details page where a build lacks it. */
@RequiresApi(Build.VERSION_CODES.S)
internal fun openExactAlarmSettings(ctx: Context) = ctx.openFirst(exactAlarmSettingsIntent(ctx), appDetailsIntent(ctx))
