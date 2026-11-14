package app.milestone

import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.platform.LocalContext
import androidx.core.content.FileProvider
import app.milestone.resources.Res
import app.milestone.resources.main_export_failed
import app.milestone.resources.main_export_ok
import java.io.File
import java.io.FileNotFoundException
import java.io.InputStream
import java.io.OutputStream
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

private const val MAX_KEPT_EXPORTS = 20

/** This app's FileProvider (its GPX shares): the manifest declares it as
 *  `${applicationId}.fileprovider`. */
internal fun Context.fileProviderAuthority(): String = "$packageName.fileprovider"

internal actual suspend fun shareGpx(ctx: Context, gpx: String, runStartEpochSec: Long) {
    val uri = withContext(Dispatchers.IO) { writeGpxToCache(ctx, gpx, runStartEpochSec) }
    val send = Intent(Intent.ACTION_SEND).apply {
        type = RunFileType.Gpx.mimeTypes.first()
        putExtra(Intent.EXTRA_STREAM, uri)
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    }
    ctx.startActivity(Intent.createChooser(send, ctx.appResources().getString(R.string.ui_export_chooser_title)).apply {
        addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    })
}

private fun writeGpxToCache(ctx: Context, gpx: String, runStartEpochSec: Long): Uri {
    val dir = File(ctx.cacheDir, "exports").apply { mkdirs() }
    // These are share-temp files the OS only evicts under cache pressure, so a
    // heavy exporter would otherwise let them pile up indefinitely. Keep the most
    // recently written handful and drop the rest before writing the new one, so a
    // just-shared file is never the victim.
    val name = gpxFileName(runStartEpochSec)
    dir.listFiles { f -> f.name.endsWith(".gpx") && f.name != name }
        ?.sortedByDescending { it.lastModified() }
        ?.drop(MAX_KEPT_EXPORTS - 1)
        ?.forEach { it.delete() }
    val file = File(dir, name)
    file.writeText(gpx)
    return FileProvider.getUriForFile(ctx, ctx.fileProviderAuthority(), file)
}

@Composable
internal actual fun rememberDataExport(onExported: () -> Unit): () -> Unit {
    val app = LocalContext.current.applicationContext
    val exported by rememberUpdatedState(onExported)
    val launcher = rememberLauncherForActivityResult(
        ActivityResultContracts.CreateDocument("application/x-ndjson"),
    ) { uri ->
        if (uri != null) AppProcess.scope.launch {
            val ok = exportDataTo(app, uri)
            if (ok) exported()
            UiMessages.post(UiMessage(app.appResources().getString(if (ok) Res.string.main_export_ok else Res.string.main_export_failed), long = true))
        }
    }
    return { launcher.launch(dataExportFilename()) }
}

/** Write the data export ([readDataExport]) to the user-chosen document [uri]; true
 *  only when every line was written. */
suspend fun exportDataTo(ctx: Context, uri: Uri): Boolean {
    val lines = readDataExport(ctx) ?: return false
    return withContext(Dispatchers.IO) {
        runCatching {
            val out = openForOverwrite(ctx, uri) ?: return@runCatching false
            out.bufferedWriter().use { w ->
                lines.forEach { w.append(it); w.append('\n') }
            }
            true
        }.onFailure { PlatformLog.e("Export", "couldn't write the data export", it) }.getOrDefault(false)
    }
}

/** Open the document [uri] to be written from its start. Some document providers
 *  reject the "wt" (write + truncate) mode, throwing IllegalArgumentException or
 *  FileNotFoundException, so those get plain "w". */
private fun openForOverwrite(ctx: Context, uri: Uri): OutputStream? = try {
    ctx.contentResolver.openOutputStream(uri, "wt")
} catch (e: IllegalArgumentException) {
    ctx.contentResolver.openOutputStream(uri, "w")
} catch (e: FileNotFoundException) {
    ctx.contentResolver.openOutputStream(uri, "w")
}

@Composable
internal actual fun rememberBackupPicker(onPicked: (String) -> Unit): () -> Unit {
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.GetContent()) { uri ->
        if (uri != null) onPicked(uri.toString())
    }
    return { launcher.launch("*/*") }
}

internal actual suspend fun readPickedBackup(ctx: Context, file: String): ImportCheck =
    readImportCandidate(ctx, Uri.parse(file))

/** [readPickedBackup] for a content [uri]: a declared size past [MAX_IMPORT_BYTES]
 *  is refused unread; the stream is still bounded, as a provider may not know it. */
suspend fun readImportCandidate(ctx: Context, uri: Uri): ImportCheck = withContext(Dispatchers.IO) {
    runCatching {
        val declared = ctx.contentResolver.openFileDescriptor(uri, "r")?.use { it.statSize } ?: -1L
        if (declared > MAX_IMPORT_BYTES) return@runCatching ImportCheck.Invalid
        ctx.contentResolver.openInputStream(uri)?.use { streamValidateImport(it, MAX_IMPORT_BYTES) }
            ?: ImportCheck.Invalid
    }.onFailure { PlatformLog.w("Export", "picked backup unreadable", it) }.getOrDefault(ImportCheck.Invalid)
}

internal fun streamValidateImport(input: InputStream, max: Long, maxLine: Long = MAX_IMPORT_LINE_BYTES): ImportCheck =
    streamValidateImport({ input.read(it) }, max, maxLine)
