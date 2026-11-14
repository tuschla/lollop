package app.milestone

import app.milestone.core.NativeCore

private val iosEngine: DurableCore by lazy { DurableCore(NativeCore) }

internal actual fun platformCoreEngine(): DurableCore = iosEngine
