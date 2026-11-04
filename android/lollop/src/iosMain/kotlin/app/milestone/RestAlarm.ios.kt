package app.milestone

import app.milestone.resources.Res
import app.milestone.resources.log_rest_notif_done_body
import app.milestone.resources.log_rest_notif_done_title
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import platform.UserNotifications.UNMutableNotificationContent
import platform.UserNotifications.UNNotificationRequest
import platform.UserNotifications.UNNotificationSound
import platform.UserNotifications.UNTimeIntervalNotificationTrigger

/**
 * The rest timer on iOS. The running rest is the in-app countdown's, as on
 * Android ([AppClock] end time, kept after the end for the overtime readout until
 * cancelled or replaced). At the rest end a "rest done" notification alerts
 * ([REST_DONE_ID], a time-interval trigger, so it comes with the app suspended):
 * the same title and "next set" line as Android's, with the default sound while
 * the Profile "Rest timer alert" is on, silent otherwise. A new rest replaces it,
 * cancel removes it. While the loop's readout is on screen the in-app cue covers
 * the rest end: the delegate ([startIosReminders]) does not show it then.
 * iOS has no ongoing countdown notification (that would be a Live Activity) and
 * no exact-alarm permission: the notice never shows.
 */
internal actual object RestAlarm {
    /** The rest-done notification's identifier: one at a time, replaced by a new rest. */
    internal const val REST_DONE_ID = "rest_done"

    private val _running = MutableStateFlow<RestRun?>(null)
    actual val running: StateFlow<RestRun?> = _running.asStateFlow()

    actual val exactAlarmPrompt: StateFlow<Boolean> = MutableStateFlow(false).asStateFlow()

    /** The loop's rest readout is on a resumed screen. */
    internal var inAppVisible = false
        private set

    // The exercise the rest-done line names: null until the loop reports its next
    // set (then the rested exercise), "" when it reported none.
    private var nextExercise: String? = null

    // Asked once per process; the system prompts only while undetermined.
    private var authorizationAsked = false

    actual fun schedule(ctx: PlatformContext, restSec: Int, exerciseName: String) {
        if (restSec <= 0) return
        cancel(ctx)
        val now = AppClock.nowMillis()
        _running.value = RestRun(now, now + restSec * 1000L, exerciseName)
        if (!authorizationAsked) {
            authorizationAsked = true
            requestNotificationAuthorization()
        }
        postDone()
    }

    actual fun cancel(ctx: PlatformContext) {
        nextExercise = null
        _running.value = null
        notificationCenter.removePendingNotificationRequestsWithIdentifiers(listOf(REST_DONE_ID))
        notificationCenter.removeDeliveredNotificationsWithIdentifiers(listOf(REST_DONE_ID))
    }

    actual fun setNextSet(ctx: PlatformContext, next: RestNextSet?) {
        val exercise = next?.exercise.orEmpty()
        if (_running.value == null || exercise == nextExercise) return
        nextExercise = exercise
        // The pending alert is replaced with the new line; once the rest is over
        // the alert (if any) already came.
        postDone()
    }

    actual fun setInAppVisible(ctx: PlatformContext, visible: Boolean) {
        inAppVisible = visible
        if (visible) notificationCenter.removeDeliveredNotificationsWithIdentifiers(listOf(REST_DONE_ID))
    }

    /** No exact-alarm permission on iOS: nothing to recheck. */
    actual fun recheckExactAlarms(ctx: PlatformContext) = Unit

    /** The notice never shows on iOS ([exactAlarmPrompt]). */
    actual fun dismissExactAlarmPrompt(ctx: PlatformContext) = Unit

    /** No such system screen on iOS; the notice that links it never shows. */
    actual fun openExactAlarmSettings(ctx: PlatformContext) = Unit

    /** (Re)schedule the running rest's rest-done alert at its end. */
    private fun postDone() {
        val run = _running.value ?: return
        val leftMs = run.endAtMillis - AppClock.nowMillis()
        if (leftMs <= 0) return
        val res = systemResources()
        val next = nextExercise ?: run.exercise
        val content = UNMutableNotificationContent().apply {
            setTitle(res.getString(Res.string.log_rest_notif_done_title))
            if (next.isNotEmpty()) setBody(res.getString(Res.string.log_rest_notif_done_body, next))
            if (AppSettings.restAlert.value) setSound(UNNotificationSound.defaultSound())
            setUserInfo(mapOf<Any?, Any?>(NOTIF_KEY_DEST to OpenDest.LogSet.wire))
        }
        val trigger = UNTimeIntervalNotificationTrigger.triggerWithTimeInterval(leftMs / 1000.0, repeats = false)
        notificationCenter.addNotificationRequest(
            UNNotificationRequest.requestWithIdentifier(REST_DONE_ID, content, trigger),
            withCompletionHandler = null,
        )
    }
}
