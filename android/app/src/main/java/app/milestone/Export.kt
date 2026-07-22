package app.milestone

import android.content.Context
import android.content.Intent
import androidx.core.content.FileProvider
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
 * local timestamp (`run-2026-07-14-143205.gpx`), not an opaque epoch. Seconds are
 * included so two runs exported within the same minute get distinct files rather
 * than the second overwriting the first before the chooser reads it.
 */
fun shareGpx(ctx: Context, gpx: String) {
    val stamp = LocalDateTime.now().format(DateTimeFormatter.ofPattern("yyyy-MM-dd-HHmmss"))
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

    val uri = FileProvider.getUriForFile(ctx, "${ctx.packageName}.fileprovider", file)
    val send = Intent(Intent.ACTION_SEND).apply {
        type = "application/gpx+xml"
        putExtra(Intent.EXTRA_STREAM, uri)
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    }
    ctx.startActivity(Intent.createChooser(send, "Export run").apply {
        addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    })
}
