package app.milestone

import app.milestone.resources.Res
import app.milestone.resources.log_rest_notif_done_body
import app.milestone.resources.log_rest_notif_done_title
import platform.UserNotifications.UNMutableNotificationContent
import platform.UserNotifications.UNNotificationRequest
import platform.UserNotifications.UNNotificationSound
import platform.UserNotifications.UNTimeIntervalNotificationTrigger

/**
 * The rest timer on iOS. At the rest end a "rest done" notification alerts
 * ([REST_DONE_ID], a time-interval trigger, so it comes with the app suspended):
 * the same title and "next set" line as Android's, with the default sound while
 * the Profile "Rest timer alert" is on, silent otherwise. While the loop's readout
 * is on screen the in-app cue covers the rest end: the delegate
 * ([installNotificationDelegate]) does not show it then. The countdown is the
 * rest's Live Activity ([IosSurfaces.showRest]: lock screen and Dynamic Island,
 * counting down to the end; the system hides it while the app is in front). iOS
 * has no exact-alarm permission: the notice never shows.
 */
internal actual object RestAlarmPlatform {
    /** The rest-done notification's identifier: one at a time, replaced by a new rest. */
    internal const val REST_DONE_ID = "rest_done"

    actual val countdown = RestCountdown.WholeRest

    actual val exactAlarms: ExactAlarms? = null

    actual fun arm(ctx: PlatformContext, run: RestRun, delayMs: Long): Boolean {
        // The system prompts only while the answer is undetermined.
        requestNotificationAuthorization()
        postDone(run.exercise, delayMs)
        return false
    }

    actual fun renameDone(ctx: PlatformContext, run: RestRun, exercise: String) {
        // Once the rest is over the alert (if any) already came.
        val leftMs = run.endAtMillis - AppClock.nowMillis()
        if (leftMs > 0) postDone(exercise, leftMs)
    }

    actual fun disarm(ctx: PlatformContext) {
        notificationCenter.removePendingNotificationRequestsWithIdentifiers(listOf(REST_DONE_ID))
        withdrawDone(ctx)
    }

    actual fun withdrawDone(ctx: PlatformContext) {
        notificationCenter.removeDeliveredNotificationsWithIdentifiers(listOf(REST_DONE_ID))
    }

    actual fun showCountdown(ctx: PlatformContext, run: RestRun, next: RestNextSet?) = IosSurfaces.showRest(run, next)

    actual fun hideCountdown(ctx: PlatformContext) = IosSurfaces.endRest()

    /** (Re)schedule the rest-done alert [leftMs] from now, naming [exercise] ("" none). */
    private fun postDone(exercise: String, leftMs: Long) {
        val res = systemResources()
        val content = UNMutableNotificationContent().apply {
            setTitle(res.getString(Res.string.log_rest_notif_done_title))
            if (exercise.isNotEmpty()) setBody(res.getString(Res.string.log_rest_notif_done_body, exercise))
            if (AppSettings.restAlert.value) setSound(UNNotificationSound.defaultSound())
            setUserInfo(mapOf<Any?, Any?>(NOTIF_KEY_DEST to OpenDest.LogSet.wire))
        }
        val trigger = UNTimeIntervalNotificationTrigger.triggerWithTimeInterval(leftMs / 1000.0, repeats = false)
        notificationCenter.addNotificationRequest(
            UNNotificationRequest.requestWithIdentifier(REST_DONE_ID, content, trigger),
        ) { error ->
            if (error != null) PlatformLog.w("RestAlarm", "rest-done alert not scheduled: ${error.localizedDescription}")
        }
    }
}

/** Show the running rest's Live Activity again ([IosSurfaces] after its host attached). */
internal fun RestAlarm.resendActivity() = showCountdownAgain(PlatformContext.INSTANCE)
