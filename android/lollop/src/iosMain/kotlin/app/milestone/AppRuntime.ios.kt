package app.milestone

import platform.Foundation.NSNotificationCenter
import platform.Foundation.NSOperationQueue
import platform.Foundation.NSSystemTimeZoneDidChangeNotification
import platform.UIKit.UIApplicationSignificantTimeChangeNotification

internal actual fun platformFeatures(): List<AppFeature> = listOf(
    IosTimeChanges,
)

/** The system's time zone and significant time changes (a clock set, local
 *  midnight, a daylight-saving jump), handed to every feature ([AppRuntime.onTimeChanged]). */
private object IosTimeChanges : AppFeature {
    override fun start(ctx: PlatformContext) {
        val center = NSNotificationCenter.defaultCenter
        center.addObserverForName(NSSystemTimeZoneDidChangeNotification, `object` = null, queue = NSOperationQueue.mainQueue) { _ ->
            AppRuntime.onTimeChanged(ctx, zoneChanged = true)
        }
        center.addObserverForName(UIApplicationSignificantTimeChangeNotification, `object` = null, queue = NSOperationQueue.mainQueue) { _ ->
            AppRuntime.onTimeChanged(ctx, zoneChanged = false)
        }
    }
}
