package app.milestone

import kotlin.concurrent.Volatile

// The iOS app installs its engine (the event log + replay over the C ABI's
// CoreBridge) before it starts the UI; nothing sends an event before that.
@Volatile private var installedEngine: CoreEngine? = null

/** Install the core engine the event queue sends to. Call once, at startup. */
fun installCoreEngine(engine: CoreEngine) {
    installedEngine = engine
}

internal actual fun platformCoreEngine(): CoreEngine =
    checkNotNull(installedEngine) { "installCoreEngine() must run before the first event" }
