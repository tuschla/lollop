package app.milestone

import android.content.Context
import android.content.pm.ApplicationInfo
import java.io.File
import java.io.FileOutputStream
import java.io.RandomAccessFile

actual typealias AppPath = File

/** The app's event log in [dir] (the app's `filesDir`). */
internal fun eventLogFile(dir: File) = File(dir, AppFileNames.EVENT_LOG)

internal actual fun Context.eventLogPath(): AppPath = eventLogFile(filesDir)

internal actual fun Context.appFile(name: String): AppPath = File(applicationContext.filesDir, name)

internal actual fun Context.isDebuggable(): Boolean = (applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE) != 0

internal actual object AppFiles {
    actual fun sibling(file: File, suffix: String): File = File(file.path + suffix)

    actual fun exists(file: File): Boolean = file.exists()

    actual fun delete(file: File): Boolean = file.delete()

    actual fun rename(from: File, to: File): Boolean = from.renameTo(to)

    actual fun copy(from: File, to: File) {
        from.copyTo(to)
    }

    actual fun openRead(file: File): RawFile = RandomAccessRawFile(RandomAccessFile(file, "r"))

    actual fun openTruncatable(file: File): RawFile = RandomAccessRawFile(RandomAccessFile(file, "rw"))

    actual fun append(file: File, text: String) = file.appendText(text)

    actual fun writeText(file: File, text: String) = file.writeText(text)

    actual fun writeLinesSynced(file: File, lines: List<String>) {
        FileOutputStream(file).use { out ->
            val w = out.bufferedWriter(Charsets.UTF_8)
            for (line in lines) {
                w.write(line)
                w.write("\n")
            }
            w.flush()
            out.fd.sync()
        }
    }
}

private class RandomAccessRawFile(private val raf: RandomAccessFile) : RawFile {
    override fun length(): Long = raf.length()

    override fun read(buf: ByteArray, count: Int): Int = raf.read(buf, 0, count).coerceAtLeast(0)

    override fun readFully(at: Long, buf: ByteArray, count: Int) {
        raf.seek(at)
        raf.readFully(buf, 0, count)
    }

    override fun truncate(len: Long) = raf.setLength(len)

    override fun close() = raf.close()
}
