package app.milestone

import kotlinx.cinterop.ExperimentalForeignApi
import kotlinx.cinterop.useContents
import platform.CoreGraphics.CGRectMake
import platform.UIKit.UIActivityViewController
import platform.UIKit.UIApplication
import platform.UIKit.UIViewController
import platform.UIKit.popoverPresentationController

/** UIKit's share sheet over the topmost screen; on iPad it opens as a popover
 *  from the middle of that screen (UIKit requires an anchor there). */
@OptIn(ExperimentalForeignApi::class)
internal actual fun shareCoachSummary(ctx: PlatformContext, model: ViewModel) {
    val text = coachSummaryText(systemResources(), model)
    val host = topViewController() ?: return
    val sheet = UIActivityViewController(activityItems = listOf(text), applicationActivities = null)
    sheet.popoverPresentationController?.let { popover ->
        popover.sourceView = host.view
        host.view.bounds.useContents { popover.sourceRect = CGRectMake(size.width / 2, size.height / 2, 0.0, 0.0) }
    }
    host.presentViewController(sheet, animated = true, completion = null)
}

/** The view controller on top: the key window's root, then whatever it presents. */
private fun topViewController(): UIViewController? {
    var top = UIApplication.sharedApplication.keyWindow?.rootViewController ?: return null
    while (true) top = top.presentedViewController ?: return top
}
