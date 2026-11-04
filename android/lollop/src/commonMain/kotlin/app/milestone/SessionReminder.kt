package app.milestone

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleResumeEffect
import app.milestone.resources.Res
import app.milestone.resources.reminder_action_log_first_set
import app.milestone.resources.reminder_action_log_next_set
import app.milestone.resources.reminder_action_start_run
import app.milestone.resources.reminder_caption
import app.milestone.resources.reminder_label
import app.milestone.resources.reminder_line_exercises
import app.milestone.resources.reminder_line_progress
import app.milestone.resources.reminder_line_single
import app.milestone.resources.reminder_notifications_off
import app.milestone.resources.reminder_open_settings
import app.milestone.resources.reminder_time_label
import app.milestone.resources.reminder_time_title
import app.milestone.widget.WidgetDayKind
import app.milestone.widget.WidgetItem
import app.milestone.widget.WidgetItemKind
import app.milestone.widget.WidgetSnapshot
import kotlinx.datetime.LocalDateTime
import org.jetbrains.compose.resources.stringResource

/**
 * The daily session reminder (Profile › During training, off by default): at the
 * chosen local time, a notification about the day's planned session while it is
 * still open ([reminderOf]). Android: an alarm that posts it from the widget
 * snapshot (SessionReminder.android.kt); iOS: dated notifications scheduled ahead
 * from the published model (SessionReminder.ios.kt).
 */
internal expect object SessionReminder {
    /** 07:30, the time a first switch-on uses. */
    val DEFAULT_MINUTE_OF_DAY: Int

    /** The oldest snapshot whose days ahead still stand in for today. */
    val AHEAD_MAX_DAYS: Long

    fun setting(ctx: PlatformContext): SessionReminderSetting

    /** Switch the reminder on or off; off also takes down what it posted. */
    fun setEnabled(ctx: PlatformContext, enabled: Boolean)

    /** The reminder's local time of day, in minutes after midnight. */
    fun setMinuteOfDay(ctx: PlatformContext, minuteOfDay: Int)

    /** The app may show notifications, as last known. */
    fun canNotify(ctx: PlatformContext): Boolean

    /** Ask the system whether the app may show notifications; [onResult] runs on
     *  the main thread (at once on Android). */
    fun checkCanNotify(ctx: PlatformContext, onResult: (Boolean) -> Unit)
}

/** The reminder's switch and its local time of day (minutes after midnight). */
internal data class SessionReminderSetting(val enabled: Boolean, val minuteOfDay: Int)

/** A request for the notification permission, asked when the reminder is switched
 *  on and not yet granted; [onResult] runs once the user answered. Android 13+:
 *  POST_NOTIFICATIONS; iOS: the alert and sound authorization. */
@Composable
internal expect fun rememberNotificationPermissionRequest(ctx: PlatformContext, onResult: () -> Unit): () -> Unit

/** Open the system screen where the app's notifications are turned on. */
internal expect fun openNotificationSettings(ctx: PlatformContext)

/** The device clock shows 24-hour time (outside composition: [is24HourClock]). */
internal expect fun uses24HourClock(ctx: PlatformContext): Boolean

/** Today's session as the reminder shows it: a [WidgetDayKind.Lift] or
 *  [WidgetDayKind.Run] day, its title and items. */
internal data class Reminder(val kind: WidgetDayKind, val title: String, val items: List<WidgetItem>) {
    /** Where Start leads: the tracker on a run day, the set editor on a lift day. */
    val dest: OpenDest get() = if (kind == WidgetDayKind.Run) OpenDest.Track else OpenDest.LogSet
}

/**
 * Today's open session from [stored], or null when there is nothing to remind
 * about. A snapshot for [today] decides on its own: a lift or run day whose session
 * is not done and still has an open item of its kind (a run day must be
 * trackable). A snapshot from an earlier day (at most [SessionReminder.AHEAD_MAX_DAYS]
 * back, written in time zone [zone], not paused, no plan or setup) answers with
 * its planned day ahead for [today]. A stale snapshot for today (a time zone
 * change), one from a later day, another zone or an unknown zone: null. Pure.
 */
internal fun reminderOf(stored: WidgetSnapshot?, today: Long, zone: String): Reminder? {
    if (stored == null) return null
    if (stored.epochDay == today) {
        if (stored.stale || stored.sessionDone) return null
        return when (stored.kind) {
            WidgetDayKind.Run -> if (stored.trackable) Reminder(stored.kind, stored.title, stored.items) else null
            WidgetDayKind.Lift ->
                if (stored.items.any { it.kind == WidgetItemKind.Lift && !it.done }) Reminder(stored.kind, stored.title, stored.items) else null
            else -> null
        }
    }
    val age = today - stored.epochDay
    if (age < 1 || age > SessionReminder.AHEAD_MAX_DAYS) return null
    if (stored.zone.isEmpty() || stored.zone != zone) return null
    if (stored.kind != WidgetDayKind.Lift && stored.kind != WidgetDayKind.Run && stored.kind != WidgetDayKind.Rest) return null
    val day = stored.ahead.firstOrNull { it.epochDay == today } ?: return null
    if (day.kind != WidgetDayKind.Lift && day.kind != WidgetDayKind.Run) return null
    val openKind = if (day.kind == WidgetDayKind.Run) WidgetItemKind.Run else WidgetItemKind.Lift
    if (day.items.none { it.kind == openKind && !it.done }) return null
    return Reminder(day.kind, day.title, day.items)
}

/**
 * The reminder's one line of what is planned. A run: the open run's distance or
 * structure and its pace range ("8 km · 5:40–6:00 /km"). A lift day: the one
 * exercise and its prescription ("Back squat · 3 × 5 @ 100 kg"), else the count
 * and the first open exercise ("4 exercises · Back squat first"), or, once
 * started, what is left ("1 of 4 done · Bench press next"). Null when there is
 * nothing to say. Pure.
 */
internal fun reminderLine(res: PlatformResources, reminder: Reminder): String? {
    if (reminder.kind == WidgetDayKind.Run) {
        val run = reminder.items.firstOrNull { it.kind == WidgetItemKind.Run && !it.done } ?: return null
        return listOf(run.detail, run.pace).filter { it.isNotBlank() }.joinToString(" \u00b7 ").ifBlank { null }
    }
    val lifts = reminder.items.filter { it.kind == WidgetItemKind.Lift }
    val open = lifts.firstOrNull { !it.done } ?: return null
    val done = lifts.count { it.done }
    return when {
        lifts.size == 1 -> if (open.detail.isBlank()) open.name else res.getString(Res.string.reminder_line_single, open.name, open.detail)
        done == 0 -> res.getQuantityString(Res.plurals.reminder_line_exercises, lifts.size, lifts.size, open.name)
        else -> res.getString(Res.string.reminder_line_progress, done, lifts.size, open.name)
    }
}

/** The action's label, what one tap does: "Start run", "Log first set", or "Log
 *  next set" once a set of today's lift session is in. Pure. */
internal fun reminderActionLabel(res: PlatformResources, reminder: Reminder): String = res.getString(
    when {
        reminder.kind == WidgetDayKind.Run -> Res.string.reminder_action_start_run
        reminder.items.any { it.kind == WidgetItemKind.Lift && it.done } -> Res.string.reminder_action_log_next_set
        else -> Res.string.reminder_action_log_first_set
    },
)

/**
 * Profile › During training: the "Session reminder" switch and, while it is on,
 * its time row (the Material 3 time picker in a dialog, as the log editors use)
 * and, when notifications are not allowed, a notice that opens the app's
 * notification settings. Switching on asks for the notification permission
 * when it is not granted ([rememberNotificationPermissionRequest]).
 */
@Composable
internal fun SessionReminderRows(ctx: PlatformContext) {
    var setting by remember { mutableStateOf(SessionReminder.setting(ctx)) }
    var notificationsOn by remember { mutableStateOf(SessionReminder.canNotify(ctx)) }
    LifecycleResumeEffect(Unit) {
        SessionReminder.checkCanNotify(ctx) { notificationsOn = it }
        onPauseOrDispose {}
    }
    val requestPermission = rememberNotificationPermissionRequest(ctx) {
        SessionReminder.checkCanNotify(ctx) { notificationsOn = it }
    }
    LabeledSwitchRow(
        label = stringResource(Res.string.reminder_label),
        subtitle = stringResource(Res.string.reminder_caption),
        subtitleColor = OnBgFaint,
        checked = setting.enabled,
        onCheckedChange = { on ->
            SessionReminder.setEnabled(ctx, on)
            setting = SessionReminder.setting(ctx)
            if (on) requestPermission()
        },
    )
    if (!setting.enabled) return
    var picking by rememberSaveable { mutableStateOf(false) }
    val use24h = is24HourClock()
    val time = remember(setting.minuteOfDay, use24h) { reminderTimeText(ctx, setting.minuteOfDay) }
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = ActionMinHeight)
            .clip(RoundedCornerShape(Space.Md.dp))
            .clickable(role = Role.Button) { picking = true },
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        Text(stringResource(Res.string.reminder_time_label), color = OnBgBody, style = Type.Body, modifier = Modifier.weight(1f))
        Text(time, color = OnBgMuted, style = Type.Body.merge(TabularFigures), maxLines = 1)
        RowChevron(tint = Accent)
    }
    if (!notificationsOn) {
        InlineNotice(
            stringResource(Res.string.reminder_notifications_off),
            actionLabel = stringResource(Res.string.reminder_open_settings),
            onAction = { openNotificationSettings(ctx) },
        )
    }
    if (picking) {
        AppTimePickerDialog(
            title = stringResource(Res.string.reminder_time_title),
            initialHour = setting.minuteOfDay / 60,
            initialMinute = setting.minuteOfDay % 60,
            is24Hour = use24h,
            onDismiss = { picking = false },
        ) { hour, minute ->
            SessionReminder.setMinuteOfDay(ctx, hour * 60 + minute)
            setting = SessionReminder.setting(ctx)
        }
    }
}

/** [minuteOfDay] in the phone's 12- or 24-hour format ("07:30", "7:30 AM"), in the
 *  app's language. */
internal fun reminderTimeText(ctx: PlatformContext, minuteOfDay: Int): String =
    localizedDayFormatter(if (uses24HourClock(ctx)) "HHmm" else "hmm")
        .format(LocalDateTime(1970, 1, 1, minuteOfDay / 60, minuteOfDay % 60))
