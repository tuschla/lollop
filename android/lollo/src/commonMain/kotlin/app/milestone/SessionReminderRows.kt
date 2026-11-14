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
import app.milestone.resources.reminder_caption
import app.milestone.resources.reminder_label
import app.milestone.resources.reminder_notifications_off
import app.milestone.resources.reminder_open_settings
import app.milestone.resources.reminder_time_label
import app.milestone.resources.reminder_time_title
import kotlinx.datetime.LocalDateTime
import org.jetbrains.compose.resources.stringResource

/** A request for the notification permission, asked when the reminder is switched
 *  on and not yet granted; [onResult] runs once the user answered. Android 13+:
 *  POST_NOTIFICATIONS; iOS: the alert and sound authorization. */
@Composable
internal expect fun rememberNotificationPermissionRequest(ctx: PlatformContext, onResult: () -> Unit): () -> Unit

internal expect fun openNotificationSettings(ctx: PlatformContext)

/**
 * The "Session reminder" switch and, while it is on, its time row and, when
 * notifications are not allowed, a notice that opens the app's notification
 * settings. Switching on asks for the notification permission when it is not granted.
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
    val use24h = is24HourClock(ctx)
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
    localizedDayFormatter(if (is24HourClock(ctx)) "HHmm" else "hmm")
        .format(LocalDateTime(1970, 1, 1, minuteOfDay / 60, minuteOfDay % 60))
