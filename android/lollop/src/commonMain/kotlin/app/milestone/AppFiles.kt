package app.milestone

/** A file in the app's private files (the event log, the run sidecar, their
 *  temporaries): `java.io.File` on Android, a POSIX path on iOS. */
expect class AppPath

/**
 * The app's file operations ([EventLog], [DurableCore], [ActiveRunStore]). Android
 * runs java.io; iOS the POSIX equivalents. The line
 * logic over them ([forEachLine], [endsWithNewline], [dropTornTail]) is shared,
 * through [RawFile]. Lines are UTF-8. Failures throw, except where a Boolean
 * reports them the way `java.io.File` does.
 */
internal expect object AppFiles {
    /** [file]'s path with [suffix] appended (`event-log.ndjson.tmp`). */
    fun sibling(file: AppPath, suffix: String): AppPath

    fun exists(file: AppPath): Boolean

    /** Delete [file]; false when it wasn't there or couldn't be deleted. */
    fun delete(file: AppPath): Boolean

    /** Rename [from] to [to], replacing [to] (atomic within one file system);
     *  false when it failed. */
    fun rename(from: AppPath, to: AppPath): Boolean

    /** Copy [from] to [to], which must not exist yet. No fsync. */
    fun copy(from: AppPath, to: AppPath)

    /** [file] opened for reading; throws when it is missing. */
    fun openRead(file: AppPath): RawFile

    /** [file] opened for reading and truncating, created empty when missing. */
    fun openTruncatable(file: AppPath): RawFile

    /** Append [text] to [file], creating it when missing. No fsync: a kill
     *  mid-append leaves a torn tail that [EventLog.load] heals. */
    fun append(file: AppPath, text: String)

    /** Replace [file]'s content with [text], creating it when missing. No fsync. */
    fun writeText(file: AppPath, text: String)

    /** Write [lines] to [file], one per line, replacing its content, and fsync
     *  before returning: a rename over the log can reach the disk before the
     *  data, and a power loss then truncates the whole log. Streamed line by
     *  line so no joined copy of a multi-year log doubles the heap. */
    fun writeLinesSynced(file: AppPath, lines: List<String>)
}

/** One open file under the shared line logic: the raw reads and the truncate
 *  ([AppFiles.openRead], [AppFiles.openTruncatable]). Closing a truncatable file
 *  reports a failed write. */
internal interface RawFile : AutoCloseable {
    fun length(): Long

    /** Read up to [count] bytes from the current position into [buf]'s start;
     *  0 at the end of the file. */
    fun read(buf: ByteArray, count: Int): Int

    /** Read exactly [count] bytes at [at] into [buf]'s start; a short file fails. */
    fun readFully(at: Long, buf: ByteArray, count: Int)

    fun truncate(len: Long)
}

private const val LF = '\n'.code.toByte()
private const val CR = '\r'.code.toByte()
private const val LINE_CHUNK = 64 * 1024
private const val TAIL_CHUNK = 4096

/** Call [action] with each line of [file], streamed. `\n`, `\r\n` and `\r`
 *  end a line; the terminators are not part of it, and a final line
 *  without one is still a line. Malformed UTF-8 reads as U+FFFD. */
internal fun AppFiles.forEachLine(file: AppPath, action: (String) -> Unit) {
    openRead(file).use { f ->
        val chunk = ByteArray(LINE_CHUNK)
        var line = ByteArray(256)
        var len = 0
        // A '\r' ended the last line: a '\n' right after it belongs to it.
        var afterCr = false
        while (true) {
            val n = f.read(chunk, LINE_CHUNK)
            if (n == 0) break
            for (i in 0 until n) {
                val b = chunk[i]
                if (afterCr && b == LF) {
                    afterCr = false
                } else if (b == LF || b == CR) {
                    action(line.decodeToString(0, len))
                    len = 0
                    afterCr = b == CR
                } else {
                    afterCr = false
                    if (len == line.size) line = line.copyOf(line.size * 2)
                    line[len++] = b
                }
            }
        }
        if (len > 0) action(line.decodeToString(0, len))
    }
}

/** Whether [file]'s last byte is `\n`. An empty file counts as terminated
 *  (nothing to glue onto). */
internal fun AppFiles.endsWithNewline(file: AppPath): Boolean = openRead(file).use { f ->
    val size = f.length()
    if (size == 0L) {
        true
    } else {
        val last = ByteArray(1)
        f.readFully(size - 1, last, 1)
        last[0] == LF
    }
}

/** Cut a torn tail off [file]: every byte after its last `\n` (the whole file
 *  when it has none), creating it empty when missing. A file that ends with
 *  `\n` is left alone after a one-byte read. */
internal fun AppFiles.dropTornTail(file: AppPath) {
    openTruncatable(file).use { f ->
        val len = f.length()
        if (len == 0L) return
        val chunk = ByteArray(TAIL_CHUNK)
        f.readFully(len - 1, chunk, 1)
        if (chunk[0] == LF) return
        var end = len
        while (end > 0) {
            val start = maxOf(0L, end - chunk.size)
            val n = (end - start).toInt()
            f.readFully(start, chunk, n)
            for (i in n - 1 downTo 0) {
                if (chunk[i] == LF) {
                    f.truncate(start + i + 1)
                    return
                }
            }
            end = start
        }
        f.truncate(0)
    }
}

/** This install's event log: [AppFileNames.EVENT_LOG] in the app's private files
 *  (Android `filesDir`, iOS Application Support). */
internal expect fun PlatformContext.eventLogPath(): AppPath

/** The file [name] in the app's private files (Android `filesDir`, iOS
 *  Application Support). */
internal expect fun PlatformContext.appFile(name: String): AppPath

/** Whether the app is a debuggable build: error envelopes are then logged in full. */
internal expect fun PlatformContext.isDebuggable(): Boolean

/** Whether this install has an event log yet, i.e. it is not a fresh install. */
internal fun PlatformContext.hasEventLog(): Boolean = AppFiles.exists(eventLogPath())
