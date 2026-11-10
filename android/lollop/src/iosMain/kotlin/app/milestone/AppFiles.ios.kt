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

actual class AppPath(val path: String) {
    override fun toString(): String = path
}

/** A failed file call: the operation, the path and the `errno` text. */
internal class LogFileException(message: String) : Exception(message)

private const val CHUNK = 64 * 1024

/** rw-r--r--, what java.io creates files with under Android's umask. */
private const val MODE = 0x1A4

private const val LF = '\n'.code.toByte()

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

/** A POSIX fd under the shared line logic; closing a [written] one reports a
 *  failed write ([closeWritten]). */
private class PosixRawFile(private val fd: Int, private val path: String, private val written: Boolean) : RawFile {
    override fun length(): Long {
        val size = lseek(fd, 0, SEEK_END)
        if (size < 0) fail("seek", path)
        return size
    }

    override fun read(buf: ByteArray, count: Int): Int = readSome(fd, buf, count, path)

    override fun readFully(at: Long, buf: ByteArray, count: Int) {
        if (lseek(fd, at, SEEK_SET) < 0) fail("seek", path)
        var off = 0
        while (off < count) {
            val n = buf.usePinned { platform.posix.read(fd, it.addressOf(off), (count - off).convert()) }
            if (n > 0) {
                off += n.toInt()
            } else if (n == 0L || errno != EINTR) {
                fail("read", path)
            }
        }
    }

    override fun truncate(len: Long) {
        if (ftruncate(fd, len) != 0) fail("truncate", path)
    }

    override fun close() {
        if (written) closeWritten(fd, path) else platform.posix.close(fd)
    }
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

internal actual object AppFiles {
    actual fun sibling(file: AppPath, suffix: String): AppPath = AppPath(file.path + suffix)

    actual fun exists(file: AppPath): Boolean = access(file.path, F_OK) == 0

    actual fun delete(file: AppPath): Boolean = unlink(file.path) == 0

    actual fun rename(from: AppPath, to: AppPath): Boolean = platform.posix.rename(from.path, to.path) == 0

    actual fun copy(from: AppPath, to: AppPath) {
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

    actual fun openRead(file: AppPath): RawFile = PosixRawFile(openFd(file.path, O_RDONLY), file.path, written = false)

    actual fun openTruncatable(file: AppPath): RawFile =
        PosixRawFile(openFd(file.path, O_RDWR or O_CREAT), file.path, written = true)

    actual fun append(file: AppPath, text: String) {
        val bytes = text.encodeToByteArray()
        val fd = openFd(file.path, O_WRONLY or O_APPEND or O_CREAT)
        try {
            writeFully(fd, bytes, bytes.size, file.path)
        } finally {
            closeWritten(fd, file.path)
        }
    }

    actual fun writeText(file: AppPath, text: String) {
        val bytes = text.encodeToByteArray()
        val fd = openFd(file.path, O_WRONLY or O_CREAT or O_TRUNC)
        try {
            writeFully(fd, bytes, bytes.size, file.path)
        } finally {
            closeWritten(fd, file.path)
        }
    }

    actual fun writeLinesSynced(file: AppPath, lines: List<String>) {
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

internal actual fun PlatformContext.eventLogPath(): AppPath = appFile(AppFileNames.EVENT_LOG)

internal actual fun PlatformContext.appFile(name: String): AppPath {
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
    return AppPath("$dir/$name")
}

@OptIn(ExperimentalNativeApi::class)
internal actual fun PlatformContext.isDebuggable(): Boolean = Platform.isDebugBinary
