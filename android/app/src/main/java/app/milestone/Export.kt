package app.milestone

import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.core.content.FileProvider
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.File
import java.time.LocalDateTime
import java.time.format.DateTimeFormatter

/** Cap on retained GPX share-temp files in the cache; older ones are pruned. */
private const val MAX_KEPT_EXPORTS = 20

/**
 * Write a core-produced GPX document to app cache and fire a share sheet so the
 * user can hand it to Strava/Garmin/Komoot/Drive/etc. The GPX string itself is
 * built in the Rust core ([RunResultView.gpx]); the shell only persists + shares.
 *
 * The filename the user sees in the share target / saves to Drive is a readable
 * local timestamp (`run-2026-07-14-143205-812.gpx`), not an opaque epoch.
 * Milliseconds are appended so two runs exported within the same SECOND get
 * distinct files rather than the second overwriting the first before the chooser
 * reads it (a second-resolution stamp collided on rapid successive exports). The
 * zero-padded millis keep the name lexically sortable for the pruning below.
 */
suspend fun shareGpx(ctx: Context, gpx: String) {
    // The cache prune + file write are disk IO: do them off the main thread
    // (mirrors the already-IO import path); only the share-sheet launch returns
    // to the caller's (main) dispatcher.
    val uri = withContext(Dispatchers.IO) { writeGpxToCache(ctx, gpx) }
    val send = Intent(Intent.ACTION_SEND).apply {
        type = "application/gpx+xml"
        putExtra(Intent.EXTRA_STREAM, uri)
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    }
    ctx.startActivity(Intent.createChooser(send, "Export run").apply {
        addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    })
}

/** Persist the GPX to app cache and return a shareable FileProvider URI. Disk IO. */
private fun writeGpxToCache(ctx: Context, gpx: String): Uri {
    val stamp = LocalDateTime.now().format(DateTimeFormatter.ofPattern("yyyy-MM-dd-HHmmss-SSS"))
    val dir = File(ctx.cacheDir, "exports").apply { mkdirs() }
    // These are share-temp files the OS only evicts under cache pressure, so a
    // heavy exporter would otherwise let them pile up indefinitely. Keep the most
    // recent handful (by filename, which is a sortable timestamp) and drop the
    // rest before writing the new one, so a just-shared file is never the victim.
    dir.listFiles { f -> f.name.endsWith(".gpx") }
        ?.sortedByDescending { it.name }
        ?.drop(MAX_KEPT_EXPORTS - 1)
        ?.forEach { it.delete() }
    val file = File(dir, "run-$stamp.gpx")
    file.writeText(gpx)
    return FileProvider.getUriForFile(ctx, "${ctx.packageName}.fileprovider", file)
}
