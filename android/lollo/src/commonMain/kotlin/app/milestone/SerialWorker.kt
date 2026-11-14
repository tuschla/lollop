package app.milestone

/** One background thread running tasks in the order they were posted (Android: a
 *  single-thread executor; iOS: a serial dispatch queue). */
internal expect class SerialWorker() {
    /** Queue [task] behind every earlier one. */
    fun post(task: () -> Unit)

    /** Queue [task] and block until it (and so every earlier task) has run; its
     *  exception is rethrown. Never call it from a task. */
    fun runAndWait(task: () -> Unit)

    /** Block until every task queued so far has run; throws when that takes longer
     *  than [timeoutMs]. */
    fun drain(timeoutMs: Long)

    /** Take no more tasks; the queued ones still run, then the thread ends. */
    fun close()
}
