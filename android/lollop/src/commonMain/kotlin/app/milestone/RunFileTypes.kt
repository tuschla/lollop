package app.milestone

/**
 * A run file format other apps hand the app ("Open with" / "Share to" on Android,
 * "Open in" on iOS): its uniform type identifier, file extension and MIME types.
 * Android's manifest intent filters and iosApp/iosApp/Info.plist's document types
 * can't reference Kotlin; AppIdentifiersTest checks both against this list.
 */
internal enum class RunFileType(val uti: String, val extension: String, val mimeTypes: List<String>) {
    Gpx("com.topografix.gpx", "gpx", listOf("application/gpx+xml", "application/gpx")),
    Tcx("com.garmin.tcx", "tcx", listOf("application/vnd.garmin.tcx+xml", "application/tcx+xml")),
    Fit("com.garmin.fit", "fit", listOf("application/vnd.ant.fit", "application/fit")),
    ;

    companion object {
        /** Every MIME type Android's import filters accept: the formats' own, and the
         *  generic ones providers often label run files with (RunImport sniffs the content). */
        val IMPORT_MIME_TYPES: List<String> =
            entries.flatMap { it.mimeTypes } + listOf("application/xml", "text/xml", "application/octet-stream")
    }
}
