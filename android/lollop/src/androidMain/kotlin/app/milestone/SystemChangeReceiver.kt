package app.milestone

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import app.milestone.widget.AndroidWidgets
import kotlin.coroutines.cancellation.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

/**
 * The system's clock, time zone and language changes. Clock and zone changes go
 * to every feature of the app ([AppRuntime.onTimeChanged]): the local day and the
 * instants of local times may differ after either. The language is shown by the
 * widget and the Quick Settings tile ([AndroidWidgets.onLocaleChanged]).
 */
class SystemChangeReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        when (intent.action) {
            Intent.ACTION_TIMEZONE_CHANGED -> AppRuntime.onTimeChanged(context, zoneChanged = true)
            Intent.ACTION_TIME_CHANGED -> AppRuntime.onTimeChanged(context, zoneChanged = false)
            Intent.ACTION_LOCALE_CHANGED -> {
                val pending = goAsync()
                CoroutineScope(Dispatchers.IO).launch {
                    try {
                        AndroidWidgets.onLocaleChanged(context)
                    } catch (e: CancellationException) {
                        throw e
                    } catch (e: Exception) {
                        PlatformLog.w("SystemChange", "language change not applied", e)
                    } finally {
                        pending.finish()
                    }
                }
            }
        }
    }
}
