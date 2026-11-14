package app.milestone

import app.milestone.resources.Res
import app.milestone.resources.import_unnamed_file
import platform.Foundation.NSURL

/**
 * Import the files at [urls] as one batch ([launchRunImport]): the document picker's
 * copies ([rememberRunImport]) and files opened into the app ("Open in lollo",
 * `LolloIosOpen`). Each is named by its URL's last path component and read whole,
 * inside its security scope when it has one (a file opened in place).
 */
internal fun importRunFiles(urls: List<NSURL>) {
    launchRunImport(urls.map(::UrlRunImportFile)) { PlatformContext.INSTANCE.appResources() }
}

private class UrlRunImportFile(private val url: NSURL) : RunImportFile {
    override fun displayName(): String =
        url.lastPathComponent?.takeIf { it.isNotBlank() }
            ?: PlatformContext.INSTANCE.appResources().getString(Res.string.import_unnamed_file)

    override fun readBytes(): ByteArray {
        val scoped = url.startAccessingSecurityScopedResource()
        try {
            // The size first, so an oversized file is refused before it is read.
            if (url.fileSize() > CoreConstants.MAX_RUN_IMPORT_BYTES) throw ImportException(ImportFailure.TooLarge)
            val bytes = url.readBytes() ?: throw ImportException(ImportFailure.ReadFailed)
            if (bytes.size > CoreConstants.MAX_RUN_IMPORT_BYTES) throw ImportException(ImportFailure.TooLarge)
            return bytes
        } finally {
            if (scoped) url.stopAccessingSecurityScopedResource()
        }
    }
}
