package app.milestone

internal actual fun platformCoreEngine(): DurableCore? = Core

internal actual fun Throwable.isLinkageError(): Boolean = this is LinkageError
