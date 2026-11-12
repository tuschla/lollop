package app.milestone

import android.content.ContentResolver
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
import androidx.core.content.IntentCompat
import java.io.ByteArrayOutputStream
import java.io.InputStream

private const val READ_BUFFER_BYTES = 64 * 1024

/**
 * Read at most [cap] bytes from [stream], throwing [ImportFailure.TooLarge] if the
 * source is larger, so the bytes are size-checked BEFORE they reach the core.
 * minSdk-24-safe (a manual bounded copy, not InputStream.readNBytes) and never
 * buffers more than the cap.
 */
internal fun readCappedBytes(stream: InputStream, cap: Int = CoreConstants.MAX_RUN_IMPORT_BYTES): ByteArray {
    val out = ByteArrayOutputStream()
    val buf = ByteArray(READ_BUFFER_BYTES)
    var total = 0L
    while (true) {
        val n = stream.read(buf)
        if (n < 0) break
        total += n
        if (total > cap) throw ImportException(ImportFailure.TooLarge)
        out.write(buf, 0, n)
    }
    return out.toByteArray()
}

/** Android's run import entry points: the picked or shared `content://` URIs, read
 *  through the ContentResolver into the common pipeline ([launchRunImport]). */
internal object RunImport {

    /** The file URIs an incoming VIEW / SEND / SEND_MULTIPLE intent carries, or empty
     *  for any other intent (launcher, notification deep link). Only other apps'
     *  `content://` URIs pass: the import reads with this app's permissions, so a
     *  `file://` path or its own FileProvider ([fileProviderAuthority]) would expose
     *  its private files. */
    fun urisFrom(ctx: Context, intent: Intent?): List<Uri> {
        if (intent == null) return emptyList()
        val direct = when (intent.action) {
            Intent.ACTION_VIEW -> listOfNotNull(intent.data)
            Intent.ACTION_SEND ->
                listOfNotNull(IntentCompat.getParcelableExtra(intent, Intent.EXTRA_STREAM, Uri::class.java))
            Intent.ACTION_SEND_MULTIPLE ->
                IntentCompat.getParcelableArrayListExtra(intent, Intent.EXTRA_STREAM, Uri::class.java)
                    ?.filterNotNull()
                    .orEmpty()
            else -> return emptyList()
        }
        val uris = direct.ifEmpty {
            // Some senders only populate ClipData (which also carries the read grant).
            val clip = intent.clipData ?: return emptyList()
            (0 until clip.itemCount).mapNotNull { clip.getItemAt(it).uri }
        }
        val own = ctx.fileProviderAuthority()
        return uris.filter { it.scheme == ContentResolver.SCHEME_CONTENT && it.authority != own }
    }

    /** Import every file in [uris] as one batch ([launchRunImport]); the result reads
     *  in the app's language with Latin digits. */
    fun importRuns(ctx: Context, uris: List<Uri>) {
        if (uris.isEmpty()) return
        val app = ctx.applicationContext
        launchRunImport(uris.map { UriRunImportFile(app, it) }) { app.appResources() }
    }

    private class UriRunImportFile(private val ctx: Context, private val uri: Uri) : RunImportFile {
        /** The file's name as the user sees it in their file picker; the URI's last
         *  path segment when the provider won't say. */
        override fun displayName(): String {
            val fromProvider = runCatching {
                ctx.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
                    if (c.moveToFirst() && !c.isNull(0)) c.getString(0) else null
                }
            }.onFailure { PlatformLog.w("lollop", "import file name not readable from its provider", it) }.getOrNull()
            return fromProvider?.takeIf { it.isNotBlank() }
                ?: uri.lastPathSegment?.substringAfterLast('/')?.takeIf { it.isNotBlank() }
                ?: ctx.appResources().getString(R.string.import_unnamed_file)
        }

        override fun readBytes(): ByteArray =
            ctx.contentResolver.openInputStream(uri)?.use { readCappedBytes(it) }
                ?: throw ImportException(ImportFailure.ReadFailed)
    }
}
