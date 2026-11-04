package app.milestone

import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

internal actual class SerialWorker actual constructor() {
    private val io = Executors.newSingleThreadExecutor()

    actual fun post(task: () -> Unit) = io.execute { task() }

    actual fun runAndWait(task: () -> Unit) {
        io.submit(Runnable { task() }).get()
    }

    actual fun drain(timeoutMs: Long) {
        io.submit(Runnable { }).get(timeoutMs, TimeUnit.MILLISECONDS)
    }
}
