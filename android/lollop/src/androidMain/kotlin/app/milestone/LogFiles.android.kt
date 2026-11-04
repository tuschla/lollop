package app.milestone

import android.content.Context
import android.content.pm.ApplicationInfo
import java.io.File
import java.io.FileOutputStream
import java.io.RandomAccessFile

actual typealias LogPath = File

/** The app's event log in [dir] (the app's `filesDir`). */
internal fun eventLogFile(dir: File) = File(dir, EVENT_LOG_NAME)

internal actual fun Context.eventLogPath(): LogPath = eventLogFile(filesDir)

internal actual fun Context.appFile(name: String): LogPath = File(applicationContext.filesDir, name)

internal actual fun Context.isDebuggable(): Boolean = (applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE) != 0

internal actual object LogFiles {
    actual fun sibling(file: File, suffix: String): File = File(file.path + suffix)

    actual fun exists(file: File): Boolean = file.exists()

    actual fun delete(file: File): Boolean = file.delete()

    actual fun rename(from: File, to: File): Boolean = from.renameTo(to)

    actual fun copy(from: File, to: File) {
        from.copyTo(to)
    }

    actual fun forEachLine(file: File, action: (String) -> Unit) {
        file.bufferedReader().useLines { it.forEach(action) }
    }

    actual fun endsWithNewline(file: File): Boolean =
        RandomAccessFile(file, "r").use {
            if (it.length() == 0L) true else { it.seek(it.length() - 1); it.read() == '\n'.code }
        }

    actual fun append(file: File, text: String) = file.appendText(text)

    actual fun writeText(file: File, text: String) = file.writeText(text)

    actual fun dropTornTail(file: File) {
        RandomAccessFile(file, "rw").use { raf ->
            val len = raf.length()
            if (len == 0L) return
            raf.seek(len - 1)
            if (raf.read() == '\n'.code) return
            val chunk = ByteArray(4096)
            var end = len
            while (end > 0) {
                val start = maxOf(0L, end - chunk.size)
                val n = (end - start).toInt()
                raf.seek(start)
                raf.readFully(chunk, 0, n)
                for (i in n - 1 downTo 0) {
                    if (chunk[i] == '\n'.code.toByte()) {
                        raf.setLength(start + i + 1)
                        return
                    }
                }
                end = start
            }
            raf.setLength(0)
        }
    }

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
