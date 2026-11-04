package app.milestone

import app.milestone.core.rustCore

/** The iOS app's entry point. The Swift app calls `LollopApp.shared.start()`
 *  once at launch, before any UI touches the core. */
object LollopApp {
    /** Install the core engine over the Rust core's C ABI. A second call keeps
     *  the engine already installed. */
    fun start() = installCoreEngine(RustCoreBridge)
}

/** [CoreBridge] over `kmp/core-bridge`'s C ABI binding ([rustCore], `NativeCore`):
 *  the same calls with the same bytes. :lollop declares its own copy of the
 *  interface so its iOS code compiles without the cinterop module. */
internal object RustCoreBridge : CoreBridge {
    override fun update(event: ByteArray): ByteArray? = rustCore.update(event)

    override fun view(): ByteArray? = rustCore.view()

    override fun parseFit(bytes: ByteArray): String? = rustCore.parseFit(bytes)

    override fun exportGpx(entryId: Long, observedAt: Long): String? = rustCore.exportGpx(entryId, observedAt)

    override fun trackFigures(event: ByteArray): String? = rustCore.trackFigures(event)

    override fun compactLog(lines: Array<String>): String? = rustCore.compactLog(lines)
}
