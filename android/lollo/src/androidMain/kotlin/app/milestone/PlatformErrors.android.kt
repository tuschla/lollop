package app.milestone

internal actual fun Throwable.isLinkageError(): Boolean = this is LinkageError

internal actual fun Throwable.isFatalToVm(): Boolean =
    this is VirtualMachineError && this !is OutOfMemoryError && this !is StackOverflowError
