package app.milestone

import app.milestone.core.rustCore

/** The iOS app's entry point: the Swift host calls [start] once at launch, before
 *  the UI and the surfaces. */
object LollopApp {
    /** Start the app over the Rust core's C ABI ([startIosApp]). */
    fun start() = startIosApp(RustCoreBridge)
}

/** [CoreBridge] over `android/core-bridge`'s C ABI binding ([rustCore], `NativeCore`):
 *  the same calls with the same bytes. :lollop declares its own copy of the
 *  interface so its iOS code compiles without the cinterop module. */
internal object RustCoreBridge : CoreBridge {
    override fun update(event: ByteArray): ByteArray? = rustCore.update(event)

    override fun view(): ByteArray? = rustCore.view()

    override fun parseFit(bytes: ByteArray): String? = rustCore.parseFit(bytes)

    override fun exportGpx(entryId: Long, observedAt: Long): String? = rustCore.exportGpx(entryId, observedAt)

    override fun trackFigures(event: ByteArray): String? = rustCore.trackFigures(event)

    override fun compactLog(lines: Array<String>): String? = rustCore.compactLog(lines)

    override fun reset(): Boolean {
        rustCore.reset()
        return true
    }
}
