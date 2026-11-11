package app.milestone

import app.milestone.core.NativeCore

private val iosEngine: DurableCore by lazy { DurableCore(NativeCore) }

/** The iOS app's one start, before its UI: [AppRuntime.start] (the settings, the
 *  notifications and reminders, the surfaces' sync). */
fun startIosApp() = AppRuntime.start(PlatformContext.INSTANCE)

internal actual fun platformCoreEngine(): DurableCore = iosEngine

internal actual fun Throwable.isLinkageError(): Boolean = false
