package app.milestone

import app.milestone.resources.Res
import app.milestone.resources.reminder_action_log_first_set
import app.milestone.resources.reminder_action_log_next_set
import app.milestone.resources.reminder_action_start_run
import kotlinx.cinterop.ExperimentalForeignApi
import kotlinx.cinterop.ObjCSignatureOverride
import platform.UserNotifications.UNAuthorizationOptionAlert
import platform.UserNotifications.UNAuthorizationOptionSound
import platform.UserNotifications.UNAuthorizationStatusAuthorized
import platform.UserNotifications.UNAuthorizationStatusProvisional
import platform.UserNotifications.UNNotification
import platform.UserNotifications.UNNotificationAction
import platform.UserNotifications.UNNotificationActionOptionAuthenticationRequired
import platform.UserNotifications.UNNotificationActionOptionForeground
import platform.UserNotifications.UNNotificationCategory
import platform.UserNotifications.UNNotificationDefaultActionIdentifier
import platform.UserNotifications.UNNotificationPresentationOptionBanner
import platform.UserNotifications.UNNotificationPresentationOptionList
import platform.UserNotifications.UNNotificationPresentationOptionSound
import platform.UserNotifications.UNNotificationPresentationOptions
import platform.UserNotifications.UNNotificationResponse
import platform.UserNotifications.UNUserNotificationCenter
import platform.UserNotifications.UNUserNotificationCenterDelegateProtocol
import platform.darwin.NSObject
import platform.darwin.dispatch_async
import platform.darwin.dispatch_get_main_queue

// The app's local notifications on iOS: the rest-done alert ([RestAlarm]) and the
// session reminders ([SessionReminder]). One delegate decides how they show while
// the app is in front and where a tap leads.

/** The notification's open destination ([OpenDest.wire]) for a tap on its body. */
internal const val NOTIF_KEY_DEST = "dest"

/** The destination of the notification's one action ([NOTIF_ACTION_START]). */
internal const val NOTIF_KEY_ACTION_DEST = "action_dest"

/** The session reminder's one action: start the session. */
internal const val NOTIF_ACTION_START = "start"

/** The reminder's categories, one per action label ([reminderActionLabel]). */
internal const val CATEGORY_REMINDER_RUN = "session_reminder_run"
internal const val CATEGORY_REMINDER_FIRST_SET = "session_reminder_first_set"
internal const val CATEGORY_REMINDER_NEXT_SET = "session_reminder_next_set"

internal val notificationCenter: UNUserNotificationCenter
    get() = UNUserNotificationCenter.currentNotificationCenter()

/** Run [block] on the main thread; UserNotifications calls back on its own queue. */
internal fun onMain(block: () -> Unit) = dispatch_async(dispatch_get_main_queue(), block)

/** Ask for alert and sound authorization (the system asks the user only once);
 *  [onDone] runs on the main thread with the answer. */
internal fun requestNotificationAuthorization(onDone: (Boolean) -> Unit = {}) {
    notificationCenter.requestAuthorizationWithOptions(UNAuthorizationOptionAlert or UNAuthorizationOptionSound) { granted, _ ->
        onMain { onDone(granted) }
    }
}

/** Whether the app may alert (authorized or provisional); [onResult] runs on the
 *  main thread. */
internal fun checkNotificationAuthorization(onResult: (Boolean) -> Unit) {
    notificationCenter.getNotificationSettingsWithCompletionHandler { settings ->
        val status = settings?.authorizationStatus
        val allowed = status == UNAuthorizationStatusAuthorized || status == UNAuthorizationStatusProvisional
        onMain { onResult(allowed) }
    }
}

/**
 * The delegate: in front, the rest-done alert shows only while the set loop's
 * readout is not on screen (as Android's rest-done notification), a session
 * reminder always does. A tap on a notification or its action opens its
 * destination. Both run on the main thread.
 */
private class NotificationDelegate : NSObject(), UNUserNotificationCenterDelegateProtocol {
    @OptIn(ExperimentalForeignApi::class)
    @ObjCSignatureOverride
    override fun userNotificationCenter(
        center: UNUserNotificationCenter,
        willPresentNotification: UNNotification,
        withCompletionHandler: (UNNotificationPresentationOptions) -> Unit,
    ) {
        val restDone = willPresentNotification.request.identifier == RestAlarm.REST_DONE_ID
        if (restDone && RestAlarm.inAppVisible) {
            withCompletionHandler(0uL)
        } else {
            withCompletionHandler(
                UNNotificationPresentationOptionBanner or UNNotificationPresentationOptionList or UNNotificationPresentationOptionSound,
            )
        }
    }

    @OptIn(ExperimentalForeignApi::class)
    @ObjCSignatureOverride
    override fun userNotificationCenter(
        center: UNUserNotificationCenter,
        didReceiveNotificationResponse: UNNotificationResponse,
        withCompletionHandler: () -> Unit,
    ) {
        val info = didReceiveNotificationResponse.notification.request.content.userInfo
        val key = when (didReceiveNotificationResponse.actionIdentifier) {
            UNNotificationDefaultActionIdentifier -> NOTIF_KEY_DEST
            NOTIF_ACTION_START -> NOTIF_KEY_ACTION_DEST
            else -> null
        }
        if (key != null) requestOpen(OpenDest.fromWire(info[key] as? String))
        withCompletionHandler()
    }
}

// UNUserNotificationCenter holds its delegate weakly.
private var delegate: NotificationDelegate? = null

/**
 * Start the iOS notifications once at boot: install the delegate (a tap on a
 * notification opens its destination, [requestOpen]), register the reminder's
 * actions and keep the session reminders scheduled from every published model
 * ([SessionReminder.start]). A second call does nothing.
 */
internal fun startIosReminders() {
    if (delegate != null) return
    val d = NotificationDelegate()
    delegate = d
    val center = notificationCenter
    center.delegate = d
    val res = systemResources()
    fun category(id: String, label: String) = UNNotificationCategory.categoryWithIdentifier(
        id,
        actions = listOf(
            // From the lock screen the session opens after unlock (Android:
            // setAuthenticationRequired).
            UNNotificationAction.actionWithIdentifier(
                NOTIF_ACTION_START,
                label,
                UNNotificationActionOptionForeground or UNNotificationActionOptionAuthenticationRequired,
            ),
        ),
        intentIdentifiers = emptyList<Any>(),
        options = 0uL,
    )
    center.setNotificationCategories(
        setOf(
            category(CATEGORY_REMINDER_RUN, res.getString(Res.string.reminder_action_start_run)),
            category(CATEGORY_REMINDER_FIRST_SET, res.getString(Res.string.reminder_action_log_first_set)),
            category(CATEGORY_REMINDER_NEXT_SET, res.getString(Res.string.reminder_action_log_next_set)),
        ),
    )
    SessionReminder.start(PlatformContext.INSTANCE)
}
