@file:OptIn(ExperimentalForeignApi::class)

package app.milestone.core

import app.milestone.core.cinterop.LollopBytes
import app.milestone.core.cinterop.LollopSlice
import app.milestone.core.cinterop.lollop_bytes_free
import app.milestone.core.cinterop.lollop_call
import app.milestone.core.cinterop.lollop_call_lines
import kotlinx.cinterop.CPointer
import kotlinx.cinterop.CValue
import kotlinx.cinterop.ExperimentalForeignApi
import kotlinx.cinterop.UByteVar
import kotlinx.cinterop.addressOf
import kotlinx.cinterop.allocArray
import kotlinx.cinterop.allocArrayOf
import kotlinx.cinterop.convert
import kotlinx.cinterop.get
import kotlinx.cinterop.memScoped
import kotlinx.cinterop.readBytes
import kotlinx.cinterop.reinterpret
import kotlinx.cinterop.usePinned
import kotlinx.cinterop.useContents

/** [CoreBridge] over the C entry points (shared/include/lollop.h). Every returned
 *  buffer is copied into Kotlin memory and freed at once. */
object NativeCore : CoreCalls() {
    override fun call(op: Int, input: ByteArray): ByteArray =
        withBytes(input) { data, len -> lollop_call(op.convert(), data, len) }.take()

    override fun callLines(op: Int, lines: Array<String>): ByteArray = memScoped {
        val slices = allocArray<LollopSlice>(lines.size)
        lines.forEachIndexed { i, line ->
            val bytes = line.encodeToByteArray()
            slices[i].data = if (bytes.isEmpty()) null else allocArrayOf(bytes).reinterpret()
            slices[i].len = bytes.size.convert()
        }
        lollop_call_lines(op.convert(), slices, lines.size.convert()).take()
    }
}

/** Call [f] with a pointer to [bytes] (null for an empty array), pinned for the call. */
internal inline fun <R> withBytes(bytes: ByteArray, f: (CPointer<UByteVar>?, ULong) -> R): R =
    if (bytes.isEmpty()) {
        f(null, 0u)
    } else {
        bytes.usePinned { f(it.addressOf(0).reinterpret(), bytes.size.convert()) }
    }

/** Copy a core-owned buffer into a [ByteArray] and free it. */
fun CValue<LollopBytes>.take(): ByteArray {
    val out = useContents { data?.readBytes(len.convert()) ?: ByteArray(0) }
    lollop_bytes_free(this)
    return out
}
