@file:OptIn(ExperimentalForeignApi::class)

package app.milestone

import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import app.milestone.resources.Res
import app.milestone.resources.main_export_failed
import app.milestone.resources.main_export_gpx_failed
import app.milestone.resources.main_export_ok
import kotlinx.cinterop.ExperimentalForeignApi
import kotlinx.cinterop.addressOf
import kotlinx.cinterop.convert
import kotlinx.cinterop.usePinned
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.IO
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import platform.Foundation.NSFileManager
import platform.Foundation.NSTemporaryDirectory
import platform.Foundation.NSURL
import platform.posix.EINTR
import platform.posix.O_CLOEXEC
import platform.posix.O_RDONLY
import platform.posix.close
import platform.posix.errno
import platform.posix.open
import platform.posix.read

/** The GPX as a temporary `run-….gpx` file ([gpxFileName]) in the share sheet. */
internal actual suspend fun shareGpx(ctx: PlatformContext, gpx: String, runStartEpochSec: Long) {
    val file = withContext(Dispatchers.IO) { writeTempFile(gpxFileName(runStartEpochSec), gpx.encodeToByteArray()) }
    if (file == null) {
        UiMessages.post(UiMessage(ctx.appResources().getString(Res.string.main_export_gpx_failed), long = true))
        return
    }
    presentShareSheet(listOf(file))
}

/** The backup as a temporary `lollo-….ndjson` file ([dataExportFilename]) in the
 *  share sheet; it counts as exported (notice, [onExported]) once the sheet saved or
 *  sent it, and the temporary goes when the sheet does. */
@Composable
internal actual fun rememberDataExport(onExported: () -> Unit): () -> Unit {
    val exported by rememberUpdatedState(onExported)
    return remember {
        {
            AppProcess.scope.launch {
                val ctx = PlatformContext.INSTANCE
                val res = ctx.appResources()
                val file = readDataExport(ctx)?.let { lines -> withContext(Dispatchers.IO) { writeBackupFile(lines) } }
                if (file == null) {
                    UiMessages.post(UiMessage(res.getString(Res.string.main_export_failed), long = true))
                    return@launch
                }
                presentShareSheet(listOf(file)) { completed ->
                    NSFileManager.defaultManager.removeItemAtURL(file, error = null)
                    if (completed) {
                        exported()
                        UiMessages.post(UiMessage(res.getString(Res.string.main_export_ok), long = true))
                    }
                }
            }
        }
    }
}

/** Write [lines] to the backup's temporary file: its URL, or null when the write failed. */
private fun writeBackupFile(lines: List<String>): NSURL? = runCatching {
    val path = NSTemporaryDirectory() + dataExportFilename()
    AppFiles.writeLinesSynced(AppPath(path), lines)
    NSURL.fileURLWithPath(path)
}.onFailure { PlatformLog.e("Export", "couldn't write the backup file", it) }.getOrNull()

/** The Files picker ([presentDocumentPicker]) for one file; the reference is the
 *  file URL of the copy it hands over. */
@Composable
internal actual fun rememberBackupPicker(onPicked: (String) -> Unit): () -> Unit {
    val picked by rememberUpdatedState(onPicked)
    return remember {
        {
            presentDocumentPicker(multiple = false) { urls ->
                urls.firstOrNull()?.absoluteString?.let { picked(it) }
            }
        }
    }
}

internal actual suspend fun readPickedBackup(ctx: PlatformContext, file: String): ImportCheck = withContext(Dispatchers.IO) {
    runCatching {
        val url = NSURL.URLWithString(file) ?: return@runCatching ImportCheck.Invalid
        if (url.fileSize() > MAX_IMPORT_BYTES) return@runCatching ImportCheck.Invalid
        val path = url.path ?: return@runCatching ImportCheck.Invalid
        val fd = open(path, O_RDONLY or O_CLOEXEC)
        if (fd < 0) return@runCatching ImportCheck.Invalid
        try {
            streamValidateImport({ buf -> readChunk(fd, buf) }, MAX_IMPORT_BYTES)
        } finally {
            close(fd)
        }
    }.onFailure { PlatformLog.w("Export", "picked backup unreadable", it) }.getOrDefault(ImportCheck.Invalid)
}

/** Fill [buf]'s start from [fd]: the count read, or -1 at the end of the file. */
private fun readChunk(fd: Int, buf: ByteArray): Int {
    while (true) {
        val n = buf.usePinned { read(fd, it.addressOf(0), buf.size.convert()) }
        if (n > 0) return n.toInt()
        if (n == 0L) return -1
        if (errno != EINTR) error("reading the backup failed (errno $errno)")
    }
}
