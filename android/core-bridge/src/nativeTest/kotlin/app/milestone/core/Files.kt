@file:OptIn(ExperimentalForeignApi::class)

package app.milestone.core

import kotlinx.cinterop.ExperimentalForeignApi
import kotlinx.cinterop.addressOf
import kotlinx.cinterop.convert
import kotlinx.cinterop.pointed
import kotlinx.cinterop.toKString
import kotlinx.cinterop.usePinned
import platform.posix.SEEK_END
import platform.posix.SEEK_SET
import platform.posix.closedir
import platform.posix.fclose
import platform.posix.fopen
import platform.posix.fread
import platform.posix.fseek
import platform.posix.ftell
import platform.posix.opendir
import platform.posix.readdir

internal fun readRepoFile(path: String): ByteArray {
    val full = "$REPO_ROOT/$path"
    val file = fopen(full, "rb") ?: error("cannot open $full")
    try {
        fseek(file, 0, SEEK_END)
        val size = ftell(file).toInt()
        fseek(file, 0, SEEK_SET)
        val bytes = ByteArray(size)
        if (size > 0) {
            val read = bytes.usePinned { fread(it.addressOf(0), 1u, size.convert(), file) }
            check(read.toInt() == size) { "short read of $full" }
        }
        return bytes
    } finally {
        fclose(file)
    }
}

internal fun readLogLines(path: String): List<String> =
    readRepoFile(path).decodeToString().lines().filter { it.isNotBlank() }

internal fun listRepoDir(path: String): List<String> {
    val full = "$REPO_ROOT/$path"
    val dir = opendir(full) ?: error("cannot open $full")
    val names = mutableListOf<String>()
    try {
        while (true) {
            val entry = readdir(dir) ?: break
            val name = entry.pointed.d_name.toKString()
            if (name != "." && name != "..") names += name
        }
    } finally {
        closedir(dir)
    }
    return names.sorted()
}
