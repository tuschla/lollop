package app.milestone

import kotlinx.cinterop.BetaInteropApi
import kotlinx.cinterop.ExperimentalForeignApi
import kotlinx.cinterop.addressOf
import kotlinx.cinterop.usePinned
import kotlinx.cinterop.useContents
import platform.CoreGraphics.CGRectMake
import platform.Foundation.NSData
import platform.Foundation.NSFileManager
import platform.Foundation.NSFileSize
import platform.Foundation.NSNumber
import platform.Foundation.NSTemporaryDirectory
import platform.Foundation.NSURL
import platform.Foundation.create
import platform.Foundation.dataWithContentsOfURL
import platform.Foundation.writeToURL
import platform.UIKit.UIActivityViewController
import platform.UIKit.UIApplication
import platform.UIKit.UIDocumentPickerDelegateProtocol
import platform.UIKit.UIDocumentPickerViewController
import platform.UIKit.UIViewController
import platform.UIKit.UIWindow
import platform.UIKit.UIWindowScene
import platform.UIKit.popoverPresentationController
import platform.UniformTypeIdentifiers.UTTypeItem
import platform.darwin.NSObject
import platform.posix.memcpy

// UIKit presentation and file plumbing the iOS actuals share: the topmost screen,
// the share sheet, the document picker, and NSData <-> ByteArray.

/** The view controller on top: the key window's root, then whatever it presents. */
internal fun topViewController(): UIViewController? {
    val window = UIApplication.sharedApplication.connectedScenes
        .filterIsInstance<UIWindowScene>()
        .flatMap { scene -> scene.windows.filterIsInstance<UIWindow>() }
        .let { windows -> windows.firstOrNull { it.isKeyWindow() } ?: windows.firstOrNull() }
    var top = window?.rootViewController ?: return null
    while (true) top = top.presentedViewController ?: return top
}

/** UIKit's share sheet for [items] (strings, file URLs) over the topmost screen; on
 *  iPad it opens as a popover from the middle of that screen (UIKit requires an
 *  anchor there). [onDone] runs once the sheet is gone: `completed` is true when the
 *  user saved or sent the items, false on cancel (or when nothing could present it). */
@OptIn(ExperimentalForeignApi::class)
internal fun presentShareSheet(items: List<Any>, onDone: (completed: Boolean) -> Unit = {}) {
    val host = topViewController() ?: return onDone(false)
    val sheet = UIActivityViewController(activityItems = items, applicationActivities = null)
    sheet.completionWithItemsHandler = { _, completed, _, _ -> onDone(completed) }
    sheet.popoverPresentationController?.let { popover ->
        popover.sourceView = host.view
        host.view.bounds.useContents { popover.sourceRect = CGRectMake(size.width / 2, size.height / 2, 0.0, 0.0) }
    }
    host.presentViewController(sheet, animated = true, completion = null)
}

// UIKit holds a picker's delegate weakly: the one open picker's delegate lives here.
private var openPicker: PickerDelegate? = null

private class PickerDelegate(private val onPicked: (List<NSURL>) -> Unit) : NSObject(), UIDocumentPickerDelegateProtocol {
    override fun documentPicker(controller: UIDocumentPickerViewController, didPickDocumentsAtURLs: List<*>) {
        openPicker = null
        onPicked(didPickDocumentsAtURLs.filterIsInstance<NSURL>())
    }

    override fun documentPickerWasCancelled(controller: UIDocumentPickerViewController) {
        openPicker = null
        onPicked(emptyList())
    }
}

/**
 * The Files picker over the topmost screen, for any file type (as Android's run
 * import picker). The picked files arrive as copies in the app's temporary inbox
 * ([onPicked] gets their URLs; none on cancel), so reading them needs no
 * security-scoped access.
 */
internal fun presentDocumentPicker(multiple: Boolean, onPicked: (List<NSURL>) -> Unit) {
    val host = topViewController() ?: return onPicked(emptyList())
    val picker = UIDocumentPickerViewController(forOpeningContentTypes = listOf(UTTypeItem), asCopy = true)
    picker.allowsMultipleSelection = multiple
    val delegate = PickerDelegate(onPicked)
    openPicker = delegate
    picker.delegate = delegate
    host.presentViewController(picker, animated = true, completion = null)
}

/** The file's size in bytes, or -1 when it can't be read. */
@OptIn(ExperimentalForeignApi::class)
internal fun NSURL.fileSize(): Long {
    val path = path ?: return -1L
    val attrs = NSFileManager.defaultManager.attributesOfItemAtPath(path, error = null) ?: return -1L
    return (attrs[NSFileSize] as? NSNumber)?.longLongValue ?: -1L
}

/** The file's bytes, or null when it can't be read. */
internal fun NSURL.readBytes(): ByteArray? = NSData.dataWithContentsOfURL(this)?.toByteArray()

@OptIn(ExperimentalForeignApi::class)
internal fun NSData.toByteArray(): ByteArray {
    val size = length.toInt()
    val out = ByteArray(size)
    if (size > 0) out.usePinned { memcpy(it.addressOf(0), bytes, length) }
    return out
}

@OptIn(ExperimentalForeignApi::class, BetaInteropApi::class)
internal fun ByteArray.toNSData(): NSData =
    if (isEmpty()) NSData() else usePinned { NSData.create(bytes = it.addressOf(0), length = size.toULong()) }

/** Write [bytes] to [name] in the app's temporary directory (replacing a file of
 *  that name) and return its URL, or null when the write failed. */
internal fun writeTempFile(name: String, bytes: ByteArray): NSURL? {
    val url = NSURL.fileURLWithPath(NSTemporaryDirectory() + name)
    return url.takeIf { bytes.toNSData().writeToURL(it, atomically = true) }
}
