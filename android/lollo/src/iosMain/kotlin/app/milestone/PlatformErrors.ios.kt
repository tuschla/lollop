package app.milestone

internal actual fun Throwable.isLinkageError(): Boolean = false

/** Kotlin/Native has no recoverable VM errors: an out-of-memory aborts the process. */
internal actual fun Throwable.isFatalToVm(): Boolean = false
