package app.milestone

/** A path the event log, the active-run file and their temporaries live at:
 *  `java.io.File` on Android, a POSIX path on iOS. */
expect class LogPath

/**
 * The file operations of the event log ([EventLog], [DurableCore]). Android runs
 * the java.io calls the log has always used; iOS the POSIX equivalents. Lines are
 * UTF-8. Failures throw, except where a Boolean reports them the way
 * `java.io.File` does.
 */
internal expect object LogFiles {
    /** [file]'s path with [suffix] appended (`event-log.ndjson.tmp`). */
    fun sibling(file: LogPath, suffix: String): LogPath

    fun exists(file: LogPath): Boolean

    /** Delete [file]; false when it wasn't there or couldn't be deleted. */
    fun delete(file: LogPath): Boolean

    /** Rename [from] to [to], replacing [to] (atomic within one file system);
     *  false when it failed. */
    fun rename(from: LogPath, to: LogPath): Boolean

    /** Copy [from] to [to], which must not exist yet. No fsync. */
    fun copy(from: LogPath, to: LogPath)

    /** Call [action] with each line of [file], streamed. `\n`, `\r\n` and `\r`
     *  end a line; the terminators are not part of it, and a final line
     *  without one is still a line. Malformed UTF-8 reads as U+FFFD. */
    fun forEachLine(file: LogPath, action: (String) -> Unit)

    /** Whether [file]'s last byte is `\n`. An empty file counts as terminated
     *  (nothing to glue onto). */
    fun endsWithNewline(file: LogPath): Boolean

    /** Append [text] to [file], creating it when missing. No fsync: a kill
     *  mid-append leaves a torn tail that [EventLog.load] heals. */
    fun append(file: LogPath, text: String)

    /** Replace [file]'s content with [text], creating it when missing. No fsync. */
    fun writeText(file: LogPath, text: String)

    /** Cut a torn tail off [file]: every byte after its last `\n` (the whole file
     *  when it has none), creating it empty when missing. A file that ends with
     *  `\n` is left alone after a one-byte read. */
    fun dropTornTail(file: LogPath)

    /** Write [lines] to [file], one per line, replacing its content, and fsync
     *  before returning: a rename over the log can reach the disk before the
     *  data, and a power loss then truncates the whole log. Streamed line by
     *  line so no joined copy of a multi-year log doubles the heap. */
    fun writeLinesSynced(file: LogPath, lines: List<String>)
}

/** This install's event log: [EVENT_LOG_NAME] in the app's private files
 *  (Android `filesDir`, iOS Application Support). */
internal expect fun PlatformContext.eventLogPath(): LogPath

/** The file [name] in the app's private files (Android `filesDir`, iOS
 *  Application Support). */
internal expect fun PlatformContext.appFile(name: String): LogPath

/** Whether the app is a debuggable build: error envelopes are then logged in full. */
internal expect fun PlatformContext.isDebuggable(): Boolean

/** Whether this install has an event log yet, i.e. it is not a fresh install. */
internal fun PlatformContext.hasEventLog(): Boolean = LogFiles.exists(eventLogPath())
