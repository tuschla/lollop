package app.milestone

/** Whether [this] is the JVM's `LinkageError` (a native library or symbol that
 *  never loaded). Kotlin/Native links the core statically, so never on iOS. */
internal expect fun Throwable.isLinkageError(): Boolean

/** A throwable after which the runtime itself can't be trusted to go on, so it
 *  must not be reported as a failed import. Android: a VirtualMachineError other
 *  than an OutOfMemoryError (one allocation the heap couldn't serve) or a
 *  StackOverflowError, whose throwing frames unwind and free what they held, so
 *  the user keeps their old log. Kotlin/Native has no such error (it aborts). */
internal expect fun Throwable.isFatalToVm(): Boolean
