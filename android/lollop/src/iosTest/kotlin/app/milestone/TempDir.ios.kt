package app.milestone

import kotlinx.cinterop.ExperimentalForeignApi
import platform.Foundation.NSFileManager
import platform.Foundation.NSTemporaryDirectory
import platform.Foundation.NSUUID

@OptIn(ExperimentalForeignApi::class)
actual fun newTempDir(): AppPath {
    val path = NSTemporaryDirectory() + "lollop-log-" + NSUUID().UUIDString
    check(NSFileManager.defaultManager.createDirectoryAtPath(path, withIntermediateDirectories = true, attributes = null, error = null)) {
        "couldn't create $path"
    }
    return AppPath(path)
}
