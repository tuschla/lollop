package app.milestone

import app.milestone.resources.Res
import app.milestone.resources.reminder_action_log_first_set
import app.milestone.resources.reminder_action_log_next_set
import app.milestone.resources.reminder_action_start_run
import app.milestone.resources.reminder_line_exercises
import app.milestone.resources.reminder_line_progress
import app.milestone.resources.reminder_line_single
import app.milestone.widget.WidgetDayKind
import app.milestone.widget.WidgetItem
import app.milestone.widget.WidgetItemKind
import app.milestone.widget.WidgetSnapshot
import kotlinx.datetime.LocalDate
import kotlinx.datetime.LocalDateTime
import kotlinx.datetime.LocalTime
import kotlinx.datetime.TimeZone
import kotlinx.datetime.toInstant

/**
 * The daily session reminder (Profile › During training, off by default): at the
 * chosen local time, a notification about the day's planned session while it is
 * still open ([due]). The setting and the rules are here; the platform delivers
 * it ([SessionReminderPort]): Android, an alarm that posts it from the widget
 * snapshot; iOS, dated notifications scheduled ahead from the published model.
 */
internal object SessionReminder : AppFeature {
    /** 07:30, the time a first switch-on uses. */
    const val DEFAULT_MINUTE_OF_DAY = 7 * 60 + 30

    /** The oldest snapshot whose days ahead still stand in for today. */
    const val AHEAD_MAX_DAYS = 7L

    const val MINUTES_PER_DAY = 24 * 60

    private const val KEY_ENABLED = "enabled"
    private const val KEY_MINUTE = "minute_of_day"

    override fun start(ctx: PlatformContext) = sessionReminderPort.start(ctx)

    override fun onTimeChanged(ctx: PlatformContext, zoneChanged: Boolean) = sessionReminderPort.onTimeChanged(ctx)

    /** The reminder's store; the platform keeps its delivery records there too. */
    fun store(ctx: PlatformContext): KeyValueStore = ctx.keyValueStore(AppFileNames.REMINDER_STORE)

    fun setting(ctx: PlatformContext): SessionReminderSetting {
        val p = store(ctx)
        return SessionReminderSetting(
            enabled = p.getBoolean(KEY_ENABLED, false),
            minuteOfDay = p.getInt(KEY_MINUTE, DEFAULT_MINUTE_OF_DAY).coerceIn(0, MINUTES_PER_DAY - 1),
        )
    }

    /** Switch the reminder on or off; off also takes down what it posted. */
    fun setEnabled(ctx: PlatformContext, enabled: Boolean) {
        store(ctx).edit { putBoolean(KEY_ENABLED, enabled) }
        if (!enabled) sessionReminderPort.takeDown(ctx)
        reschedule(ctx)
    }

    /** The reminder's local time of day, in minutes after midnight. */
    fun setMinuteOfDay(ctx: PlatformContext, minuteOfDay: Int) {
        require(minuteOfDay in 0 until MINUTES_PER_DAY) { "minute of day $minuteOfDay" }
        store(ctx).edit { putInt(KEY_MINUTE, minuteOfDay) }
        reschedule(ctx)
    }

    /** Schedule the reminder again from the current setting; none while it is off. */
    fun reschedule(ctx: PlatformContext) = sessionReminderPort.reschedule(ctx)

    /** The app may show notifications, as last known. */
    fun canNotify(ctx: PlatformContext): Boolean = sessionReminderPort.canNotify(ctx)

    /** Ask the system whether the app may show notifications; [onResult] runs on
     *  the main thread (at once on Android). */
    fun checkCanNotify(ctx: PlatformContext, onResult: (Boolean) -> Unit) = sessionReminderPort.checkCanNotify(ctx, onResult)

    /** The reminder for local [day] from [stored] ([reminderOf]); none for
     *  [today]'s run while a run records ([recording]): it is under way. */
    fun due(stored: WidgetSnapshot?, day: Long, today: Long, zone: String, recording: Boolean): Reminder? =
        reminderOf(stored, day, zone)?.takeUnless { day == today && recording && it.kind == WidgetDayKind.Run }

    /** Local [minuteOfDay] of epoch day [day] in [zone]. A time that a
     *  daylight-saving jump skips moves forward by the gap. */
    fun fireMillis(day: Long, minuteOfDay: Int, zone: TimeZone): Long =
        LocalDateTime(LocalDate.fromEpochDays(day.toInt()), LocalTime(minuteOfDay / 60, minuteOfDay % 60))
            .toInstant(zone).toEpochMilliseconds()

    /** The first local [minuteOfDay] strictly after [nowMillis] in [zone]. */
    fun nextFireMillis(nowMillis: Long, minuteOfDay: Int, zone: TimeZone): Long {
        val today = localEpochDayAtMillis(nowMillis, zone)
        val at = fireMillis(today, minuteOfDay, zone)
        return if (at > nowMillis) at else fireMillis(today + 1, minuteOfDay, zone)
    }
}

/** How a platform delivers the [SessionReminder]. */
internal interface SessionReminderPort {
    /** The process started ([AppRuntime.start]). */
    fun start(ctx: PlatformContext)

    /** Schedule the reminder again from [SessionReminder.setting]. */
    fun reschedule(ctx: PlatformContext)

    /** The reminder was switched off: take down what it posted. */
    fun takeDown(ctx: PlatformContext)

    /** The wall clock was set, the time zone changed or local midnight passed: the
     *  local day and the reminder's instant may differ. */
    fun onTimeChanged(ctx: PlatformContext)

    fun canNotify(ctx: PlatformContext): Boolean

    fun checkCanNotify(ctx: PlatformContext, onResult: (Boolean) -> Unit)
}

internal expect val sessionReminderPort: SessionReminderPort

/** The reminder's switch and its local time of day (minutes after midnight). */
internal data class SessionReminderSetting(val enabled: Boolean, val minuteOfDay: Int)

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
