@file:OptIn(ExperimentalForeignApi::class)

package app.milestone

import kotlin.experimental.ExperimentalNativeApi
import kotlinx.cinterop.ExperimentalForeignApi
import kotlinx.cinterop.addressOf
import kotlinx.cinterop.convert
import kotlinx.cinterop.toKString
import kotlinx.cinterop.usePinned
import platform.Foundation.NSApplicationSupportDirectory
import platform.Foundation.NSFileManager
import platform.Foundation.NSHomeDirectory
import platform.Foundation.NSUserDomainMask
import platform.posix.EINTR
import platform.posix.F_FULLFSYNC
import platform.posix.F_OK
import platform.posix.O_APPEND
import platform.posix.O_CLOEXEC
import platform.posix.O_CREAT
import platform.posix.O_EXCL
import platform.posix.O_RDONLY
import platform.posix.O_RDWR
import platform.posix.O_TRUNC
import platform.posix.O_WRONLY
import platform.posix.SEEK_END
import platform.posix.SEEK_SET
import platform.posix.access
import platform.posix.close
import platform.posix.errno
import platform.posix.fcntl
import platform.posix.fsync
import platform.posix.ftruncate
import platform.posix.lseek
import platform.posix.open
import platform.posix.read
import platform.posix.strerror
import platform.posix.unlink
import platform.posix.write

actual class LogPath(val path: String) {
    override fun toString(): String = path
}

/** A failed file call: the operation, the path and the `errno` text. */
internal class LogFileException(message: String) : Exception(message)

private const val CHUNK = 64 * 1024

/** rw-r--r--, what java.io creates files with under Android's umask. */
private const val MODE = 0x1A4

private const val LF = '\n'.code.toByte()
private const val CR = '\r'.code.toByte()

private fun fail(op: String, path: String): Nothing =
    throw LogFileException("$op $path: ${strerror(errno)?.toKString()}")

private fun openFd(path: String, flags: Int): Int {
    while (true) {
        val fd = open(path, flags or O_CLOEXEC, MODE)
        if (fd >= 0) return fd
        if (errno != EINTR) fail("open", path)
    }
}

/** Close a file that was written: a failing close can be the first report of a
 *  failed write. */
private fun closeWritten(fd: Int, path: String) {
    if (close(fd) != 0 && errno != EINTR) fail("close", path)
}

/** Read up to [buf].size bytes; 0 at the end of the file. */
private fun readSome(fd: Int, buf: ByteArray, count: Int, path: String): Int {
    while (true) {
        val n = buf.usePinned { read(fd, it.addressOf(0), count.convert()) }
        if (n >= 0) return n.toInt()
        if (errno != EINTR) fail("read", path)
    }
}

/** Read exactly [count] bytes into [buf]; a short file fails. */
private fun readFully(fd: Int, buf: ByteArray, count: Int, path: String) {
    var off = 0
    while (off < count) {
        val n = buf.usePinned { read(fd, it.addressOf(off), (count - off).convert()) }
        if (n > 0) {
            off += n.toInt()
        } else if (n == 0L || errno != EINTR) {
            fail("read", path)
        }
    }
}

private fun seek(fd: Int, at: Long, path: String) {
    if (lseek(fd, at, SEEK_SET) < 0) fail("seek", path)
}

private fun truncate(fd: Int, len: Long, path: String) {
    if (ftruncate(fd, len) != 0) fail("truncate", path)
}

private fun writeFully(fd: Int, bytes: ByteArray, len: Int, path: String) {
    var off = 0
    while (off < len) {
        val n = bytes.usePinned { write(fd, it.addressOf(off), (len - off).convert()) }
        if (n >= 0) {
            off += n.toInt()
        } else if (errno != EINTR) {
            fail("write", path)
        }
    }
}

internal actual object LogFiles {
    actual fun sibling(file: LogPath, suffix: String): LogPath = LogPath(file.path + suffix)

    actual fun exists(file: LogPath): Boolean = access(file.path, F_OK) == 0

    actual fun delete(file: LogPath): Boolean = unlink(file.path) == 0

    actual fun rename(from: LogPath, to: LogPath): Boolean = platform.posix.rename(from.path, to.path) == 0

    actual fun copy(from: LogPath, to: LogPath) {
        val src = openFd(from.path, O_RDONLY)
        try {
            val dst = openFd(to.path, O_WRONLY or O_CREAT or O_EXCL)
            try {
                val buf = ByteArray(CHUNK)
                while (true) {
                    val n = readSome(src, buf, CHUNK, from.path)
                    if (n == 0) break
                    writeFully(dst, buf, n, to.path)
                }
            } finally {
                closeWritten(dst, to.path)
            }
        } finally {
            close(src)
        }
    }

    actual fun forEachLine(file: LogPath, action: (String) -> Unit) {
        val fd = openFd(file.path, O_RDONLY)
        try {
            val chunk = ByteArray(CHUNK)
            var line = ByteArray(256)
            var len = 0
            // A '\r' ended the last line: a '\n' right after it belongs to it.
            var afterCr = false
            while (true) {
                val n = readSome(fd, chunk, CHUNK, file.path)
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
        } finally {
            close(fd)
        }
    }

    actual fun endsWithNewline(file: LogPath): Boolean {
        val fd = openFd(file.path, O_RDONLY)
        try {
            val size = lseek(fd, 0, SEEK_END)
            if (size < 0) fail("seek", file.path)
            if (size == 0L) return true
            if (lseek(fd, size - 1, SEEK_SET) < 0) fail("seek", file.path)
            val last = ByteArray(1)
            if (readSome(fd, last, 1, file.path) != 1) fail("read", file.path)
            return last[0] == LF
        } finally {
            close(fd)
        }
    }

    actual fun append(file: LogPath, text: String) {
        val bytes = text.encodeToByteArray()
        val fd = openFd(file.path, O_WRONLY or O_APPEND or O_CREAT)
        try {
            writeFully(fd, bytes, bytes.size, file.path)
        } finally {
            closeWritten(fd, file.path)
        }
    }

    actual fun writeText(file: LogPath, text: String) {
        val bytes = text.encodeToByteArray()
        val fd = openFd(file.path, O_WRONLY or O_CREAT or O_TRUNC)
        try {
            writeFully(fd, bytes, bytes.size, file.path)
        } finally {
            closeWritten(fd, file.path)
        }
    }

    actual fun dropTornTail(file: LogPath) {
        val fd = openFd(file.path, O_RDWR or O_CREAT)
        try {
            val len = lseek(fd, 0, SEEK_END)
            if (len < 0) fail("seek", file.path)
            if (len == 0L) return
            val chunk = ByteArray(4096)
            seek(fd, len - 1, file.path)
            readFully(fd, chunk, 1, file.path)
            if (chunk[0] == LF) return
            var end = len
            while (end > 0) {
                val start = maxOf(0L, end - chunk.size)
                val n = (end - start).toInt()
                seek(fd, start, file.path)
                readFully(fd, chunk, n, file.path)
                for (i in n - 1 downTo 0) {
                    if (chunk[i] == LF) {
                        truncate(fd, start + i + 1, file.path)
                        return
                    }
                }
                end = start
            }
            truncate(fd, 0, file.path)
        } finally {
            closeWritten(fd, file.path)
        }
    }

    actual fun writeLinesSynced(file: LogPath, lines: List<String>) {
        val fd = openFd(file.path, O_WRONLY or O_CREAT or O_TRUNC)
        try {
            val buf = ByteArray(CHUNK)
            var len = 0
            fun put(bytes: ByteArray) {
                if (len + bytes.size > buf.size) {
                    writeFully(fd, buf, len, file.path)
                    len = 0
                }
                if (bytes.size > buf.size) {
                    writeFully(fd, bytes, bytes.size, file.path)
                } else {
                    bytes.copyInto(buf, len)
                    len += bytes.size
                }
            }
            val newline = byteArrayOf(LF)
            for (line in lines) {
                put(line.encodeToByteArray())
                put(newline)
            }
            writeFully(fd, buf, len, file.path)
            // fsync hands the data to the drive, whose cache can still lose it on
            // power loss; F_FULLFSYNC flushes that cache too. fsync where the file
            // system doesn't support it.
            if (fcntl(fd, F_FULLFSYNC) == -1 && fsync(fd) != 0) fail("fsync", file.path)
        } finally {
            closeWritten(fd, file.path)
        }
    }
}

internal actual fun PlatformContext.eventLogPath(): LogPath = appFile(EVENT_LOG_NAME)

internal actual fun PlatformContext.appFile(name: String): LogPath {
    val files = NSFileManager.defaultManager
    // Application Support isn't created for a new app; create = true makes it.
    val dir = files.URLForDirectory(
        NSApplicationSupportDirectory,
        NSUserDomainMask,
        appropriateForURL = null,
        create = true,
        error = null,
    )?.path ?: (NSHomeDirectory() + "/Library/Application Support").also {
        files.createDirectoryAtPath(it, withIntermediateDirectories = true, attributes = null, error = null)
    }
    return LogPath("$dir/$name")
}

@OptIn(ExperimentalNativeApi::class)
internal actual fun PlatformContext.isDebuggable(): Boolean = Platform.isDebugBinary
