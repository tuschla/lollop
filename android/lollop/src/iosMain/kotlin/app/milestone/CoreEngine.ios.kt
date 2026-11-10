package app.milestone

import kotlin.concurrent.Volatile

@Volatile private var iosEngine: DurableCore? = null

/**
 * The iOS app's one start, before its UI: the core engine over [bridge] (the C ABI,
 * `android/core-bridge`'s `NativeCore`), then [AppRuntime.start] (the settings,
 * the notifications and reminders, the surfaces' sync).
 */
fun startIosApp(bridge: CoreBridge) {
    iosEngine = BridgeCore(bridge)
    AppRuntime.start(PlatformContext.INSTANCE)
}

internal actual fun platformCoreEngine(): DurableCore? = iosEngine

internal actual fun Throwable.isLinkageError(): Boolean = false
