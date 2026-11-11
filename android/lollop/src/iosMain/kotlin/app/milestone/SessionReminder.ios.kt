package app.milestone

import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import app.milestone.resources.Res
import app.milestone.resources.reminder_title_fallback
import app.milestone.widget.WidgetDayKind
import app.milestone.widget.WidgetItemKind
import app.milestone.widget.widgetInputs
import app.milestone.widget.widgetSnapshotOf
import kotlin.coroutines.resume
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.conflate
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.datetime.LocalDate
import kotlinx.datetime.TimeZone
import kotlinx.datetime.number
import platform.Foundation.NSDateComponents
import platform.Foundation.NSURL
import platform.UIKit.UIApplication
import platform.UIKit.UIApplicationOpenNotificationSettingsURLString
import platform.UserNotifications.UNCalendarNotificationTrigger
import platform.UserNotifications.UNMutableNotificationContent
import platform.UserNotifications.UNNotification
import platform.UserNotifications.UNNotificationRequest
import platform.UserNotifications.UNNotificationSound

/**
 * The session reminder on iOS ([SessionReminder]). iOS runs no app code when a
 * notification fires, so the reminders are scheduled ahead: one dated notification
 * per day of the next [DAYS_SCHEDULED] with an open session at the chosen time
 * ([SessionReminder.due] over the model's snapshot, [widgetSnapshotOf], as
 * Android's reminder reads the widget snapshot), with Android's title, line and
 * action. They are set again on every change of the snapshot inputs
 * ([widgetInputs]: the published model, the distance unit, a run starting or
 * stopping, a time change) and of the setting; none while it is off. At most one
 * reminder per local day: once today's has fired a later time today does not
 * remind again. A shown reminder goes once its day has passed or, today, once
 * there is nothing left to remind about (the session done, a run recording).
 */
internal object IosReminder : SessionReminderPort {
    /** The days ahead, today included, that get a reminder scheduled. */
    private const val DAYS_SCHEDULED = 7

    // Today's scheduled reminder in [SessionReminder.store]: its local day and fire
    // time (epoch millis). Once that time has passed, today has been reminded.
    private const val KEY_SCHEDULED_DAY = "scheduled_epoch_day"
    private const val KEY_SCHEDULED_AT = "scheduled_at_millis"

    /** A reminder's identifier: this prefix and its local epoch day. */
    private const val ID_PREFIX = "session_reminder_"

    private class State {
        val lock = Mutex()

        // Authorization as last asked ([checkCanNotify]).
        var allowed = false
    }

    private val state by ProcessLocal { State() }

    /** Install the notification delegate and the reminder's actions, then keep
     *  the reminders in step with the snapshot inputs. */
    override fun start(ctx: PlatformContext) {
        installNotificationDelegate()
        checkCanNotify(ctx) {}
        // Off the main thread: the text is read from the resources, blocking.
        AppProcess.scope.launch(Dispatchers.Default) {
            widgetInputs()
                .distinctUntilChanged { a, b -> a.model == b.model && a.unit == b.unit && a.recording == b.recording && a.ticks == b.ticks }
                .conflate()
                .collect { sync(ctx) }
        }
    }

    override fun reschedule(ctx: PlatformContext) {
        AppProcess.scope.launch(Dispatchers.Default) { sync(ctx) }
    }

    // The rescheduling sync takes down what was delivered once the setting is off.
    override fun takeDown(ctx: PlatformContext) {}

    // The snapshot inputs tick on every time change, which sets the reminders again.
    override fun onTimeChanged(ctx: PlatformContext) {}

    override fun canNotify(ctx: PlatformContext): Boolean = state.allowed

    override fun checkCanNotify(ctx: PlatformContext, onResult: (Boolean) -> Unit) =
        checkNotificationAuthorization { ok ->
            state.allowed = ok
            onResult(ok)
        }

    private suspend fun sync(ctx: PlatformContext) = state.lock.withLock {
        val s = SessionReminder.setting(ctx)
        val model = EventDispatcher.model.value
        // Before the first model there is nothing to tell; what is scheduled stands.
        if (s.enabled && model == null) return@withLock
        val recording = RunSession.tracking.value
        val zone = TimeZone.currentSystemDefault()
        val now = AppClock.nowMillis()
        val today = localEpochDayAtMillis(now, zone)
        val prefs = SessionReminder.store(ctx)
        val remindedToday = prefs.getLong(KEY_SCHEDULED_DAY, Long.MIN_VALUE) == today &&
            prefs.getLong(KEY_SCHEDULED_AT, Long.MAX_VALUE) <= now
        val snapshot = model?.let { widgetSnapshotOf(it, it.distanceUnit(), zone.id) }
        fun open(day: Long): Reminder? = SessionReminder.due(snapshot, day, today, zone.id, recording)
        val res = PlatformContext.INSTANCE.appResources()
        val wanted = mutableMapOf<String, UNNotificationRequest>()
        var todayAt: Long? = null
        if (s.enabled) {
            for (day in today until today + DAYS_SCHEDULED) {
                if (day == today && remindedToday) continue
                val at = SessionReminder.fireMillis(day, s.minuteOfDay, zone)
                if (at <= now) continue
                val reminder = open(day) ?: continue
                if (day == today) todayAt = at
                wanted[idOf(day)] = request(res, reminder, day, s.minuteOfDay)
            }
        }
        if (!remindedToday) {
            val at = todayAt
            prefs.edit {
                if (at != null) {
                    putLong(KEY_SCHEDULED_DAY, today)
                    putLong(KEY_SCHEDULED_AT, at)
                } else {
                    remove(KEY_SCHEDULED_DAY)
                    remove(KEY_SCHEDULED_AT)
                }
            }
        }
        val center = notificationCenter
        val pending = suspendCancellableCoroutine { cont ->
            center.getPendingNotificationRequestsWithCompletionHandler { list ->
                cont.resume(list.orEmpty().filterIsInstance<UNNotificationRequest>().map { it.identifier })
            }
        }
        center.removePendingNotificationRequestsWithIdentifiers(pending.filter { it.startsWith(ID_PREFIX) && it !in wanted })
        // The same identifier replaces a pending one.
        wanted.values.forEach { center.addNotificationRequest(it, withCompletionHandler = null) }
        val delivered = suspendCancellableCoroutine { cont ->
            center.getDeliveredNotificationsWithCompletionHandler { list ->
                cont.resume(list.orEmpty().filterIsInstance<UNNotification>().map { it.request.identifier })
            }
        }
        val keepToday = s.enabled && open(today) != null
        center.removeDeliveredNotificationsWithIdentifiers(
            delivered.filter { it.startsWith(ID_PREFIX) && (it != idOf(today) || !keepToday) },
        )
    }

    private fun idOf(day: Long) = "$ID_PREFIX$day"

    /** [reminder] for local epoch [day] at [minuteOfDay]: a tap opens Today, its
     *  action starts the session ([Reminder.dest]). */
    private fun request(res: PlatformResources, reminder: Reminder, day: Long, minuteOfDay: Int): UNNotificationRequest {
        val content = UNMutableNotificationContent().apply {
            setTitle(reminder.title.ifBlank { res.getString(Res.string.reminder_title_fallback) })
            reminderLine(res, reminder)?.let { setBody(it) }
            setSound(UNNotificationSound.defaultSound())
            setCategoryIdentifier(
                when {
                    reminder.kind == WidgetDayKind.Run -> CATEGORY_REMINDER_RUN
                    reminder.items.any { it.kind == WidgetItemKind.Lift && it.done } -> CATEGORY_REMINDER_NEXT_SET
                    else -> CATEGORY_REMINDER_FIRST_SET
                },
            )
            setUserInfo(mapOf<Any?, Any?>(NOTIF_KEY_DEST to OpenDest.Today.wire, NOTIF_KEY_ACTION_DEST to reminder.dest.wire))
        }
        val date = LocalDate.fromEpochDays(day.toInt())
        val components = NSDateComponents().apply {
            year = date.year.toLong()
            month = date.month.number.toLong()
            this.day = date.day.toLong()
            hour = (minuteOfDay / 60).toLong()
            minute = (minuteOfDay % 60).toLong()
        }
        val trigger = UNCalendarNotificationTrigger.triggerWithDateMatchingComponents(components, repeats = false)
        return UNNotificationRequest.requestWithIdentifier(idOf(day), content, trigger)
    }
}

internal actual val sessionReminderPort: SessionReminderPort get() = IosReminder

/** iOS: the alert and sound authorization (the system asks only once); the
 *  reminders are set again once answered. */
@Composable
internal actual fun rememberNotificationPermissionRequest(ctx: PlatformContext, onResult: () -> Unit): () -> Unit {
    val latest by rememberUpdatedState(onResult)
    return remember(ctx) {
        {
            requestNotificationAuthorization {
                SessionReminder.reschedule(ctx)
                latest()
            }
        }
    }
}

/** The app's notification settings in the Settings app. */
internal actual fun openNotificationSettings(ctx: PlatformContext) {
    val url = NSURL.URLWithString(UIApplicationOpenNotificationSettingsURLString) ?: return openAppSettings(ctx)
    UIApplication.sharedApplication.openURL(url, options = emptyMap<Any?, Any?>(), completionHandler = null)
}
