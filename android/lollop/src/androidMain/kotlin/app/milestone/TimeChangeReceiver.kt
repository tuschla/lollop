package app.milestone

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/** The system's clock and time zone changes, handed to every feature of the app
 *  ([AppRuntime.onTimeChanged]): the local day and the instants of local times
 *  may differ after either. */
class TimeChangeReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val zoneChanged = when (intent.action) {
            Intent.ACTION_TIMEZONE_CHANGED -> true
            Intent.ACTION_TIME_CHANGED -> false
            else -> return
        }
        AppRuntime.onTimeChanged(context, zoneChanged)
    }
}
