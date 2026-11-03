@file:OptIn(ExperimentalForeignApi::class)

package app.milestone.core

import app.milestone.core.cinterop.LollopBytes
import app.milestone.core.cinterop.LollopSlice
import app.milestone.core.cinterop.lollop_bytes_free
import app.milestone.core.cinterop.lollop_compact_log
import app.milestone.core.cinterop.lollop_export_gpx
import app.milestone.core.cinterop.lollop_parse_fit
import app.milestone.core.cinterop.lollop_track_figures
import app.milestone.core.cinterop.lollop_update
import app.milestone.core.cinterop.lollop_view
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

actual val rustCore: CoreBridge = NativeCore

/** [CoreBridge] over the `lollop_*` C ABI (shared/include/lollop.h). Every
 *  returned buffer is copied into Kotlin memory and freed at once. */
internal object NativeCore : CoreBridge {
    override fun update(event: ByteArray): ByteArray =
        withBytes(event) { data, len -> lollop_update(data, len) }.take()

    override fun view(): ByteArray = lollop_view().take()

    override fun parseFit(bytes: ByteArray): String =
        withBytes(bytes) { data, len -> lollop_parse_fit(data, len) }.take().decodeToString()

    override fun exportGpx(entryId: Long, observedAt: Long): String =
        lollop_export_gpx(entryId, observedAt).take().decodeToString()

    override fun trackFigures(event: ByteArray): String =
        withBytes(event) { data, len -> lollop_track_figures(data, len) }.take().decodeToString()

    override fun compactLog(lines: Array<String>): String? = memScoped {
        val slices = allocArray<LollopSlice>(lines.size)
        lines.forEachIndexed { i, line ->
            val bytes = line.encodeToByteArray()
            slices[i].data = if (bytes.isEmpty()) null else allocArrayOf(bytes).reinterpret()
            slices[i].len = bytes.size.convert()
        }
        lollop_compact_log(slices, lines.size.convert()).take()
    }.takeIf { it.isNotEmpty() }?.decodeToString()
}

/** Call [f] with a pointer to [bytes] (null for an empty array), pinned for the call. */
internal inline fun <R> withBytes(bytes: ByteArray, f: (CPointer<UByteVar>?, ULong) -> R): R =
    if (bytes.isEmpty()) {
        f(null, 0u)
    } else {
        bytes.usePinned { f(it.addressOf(0).reinterpret(), bytes.size.convert()) }
    }

/** Copy a core-owned buffer into a [ByteArray] and free it. */
internal fun CValue<LollopBytes>.take(): ByteArray {
    val out = useContents { data?.readBytes(len.convert()) ?: ByteArray(0) }
    lollop_bytes_free(this)
    return out
}
