package app.milestone

import kotlin.concurrent.Volatile

// The iOS app installs its engine (the event log + replay over the C ABI's
// CoreBridge) before it starts the UI; nothing sends an event before that.
@Volatile private var installedEngine: DurableCore? = null

/**
 * Install the core engine over [bridge] (the C ABI, `kmp/core-bridge`'s
 * `NativeCore`) for the event queue and the event log. Call once at startup,
 * before the first event; a later call keeps the engine already installed, which
 * holds the process's replayed state.
 */
fun installCoreEngine(bridge: CoreBridge) {
    if (installedEngine == null) installedEngine = BridgeCore(bridge)
}

internal actual fun platformCoreEngine(): DurableCore =
    checkNotNull(installedEngine) { "installCoreEngine() must run before the first event" }

internal actual fun Throwable.isLinkageError(): Boolean = false
