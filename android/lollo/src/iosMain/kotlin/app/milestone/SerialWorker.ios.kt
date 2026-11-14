package app.milestone

import platform.darwin.DISPATCH_TIME_NOW
import platform.darwin.dispatch_async
import platform.darwin.dispatch_queue_create
import platform.darwin.dispatch_semaphore_create
import platform.darwin.dispatch_semaphore_signal
import platform.darwin.dispatch_semaphore_wait
import platform.darwin.dispatch_sync
import platform.darwin.dispatch_time

internal actual class SerialWorker actual constructor() {
    // A null attribute makes a serial queue: one task at a time, in order.
    private val queue = dispatch_queue_create("app.milestone.serial-worker", null)

    actual fun post(task: () -> Unit) = dispatch_async(queue, task)

    actual fun runAndWait(task: () -> Unit) {
        var failure: Throwable? = null
        // An exception must not leave the block: GCD would end the process.
        dispatch_sync(queue) { failure = runCatching(task).exceptionOrNull() }
        failure?.let { throw it }
    }

    actual fun drain(timeoutMs: Long) {
        val done = dispatch_semaphore_create(0)
        dispatch_async(queue) { dispatch_semaphore_signal(done) }
        if (dispatch_semaphore_wait(done, dispatch_time(DISPATCH_TIME_NOW, timeoutMs * 1_000_000)) != 0L) {
            throw IllegalStateException("queued tasks still running after $timeoutMs ms")
        }
    }

    // The queue runs what it holds and is released with the worker.
    actual fun close() {}
}
