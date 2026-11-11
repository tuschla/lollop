package app.milestone

import app.milestone.core.CoreBridge
import kotlin.concurrent.Volatile

/** The app clock the host tests run on ([AppRuntime.resetForTest]): the device
 *  clock shifted by [offsetMillis], which a test moves to change "now". */
internal object TestClock : WallClock {
    @Volatile var offsetMillis: Long = 0L

    override fun nowMillis(): Long = System.currentTimeMillis() + offsetMillis

    override fun monotonicMillis(): Long = deviceMonotonicMillis()
}

/** A plain-JVM test's clock (no Robolectric, so no `SystemClock`): the wall clock,
 *  and a monotonic clock that stays at 0. */
internal object PlainJvmClock : WallClock {
    override fun nowMillis(): Long = System.currentTimeMillis()

    override fun monotonicMillis(): Long = 0L
}

/** The event queue's send path in the host tests ([AppRuntime.resetForTest]): the
 *  real engine until a test swaps [sender] / [batchSender] for a fake or a wrapper. */
internal object TestSink : EventSink {
    @Volatile var sender: suspend (Event) -> SendResult = EngineSink::send
    @Volatile var batchSender: suspend (List<Event>) -> SendResult = EngineSink::sendAll

    override suspend fun send(e: Event): SendResult = sender(e)

    override suspend fun sendAll(events: List<Event>): SendResult = batchSender(events)

    /** Back to the real engine. */
    fun reset() {
        sender = EngineSink::send
        batchSender = EngineSink::sendAll
    }
}

/** A stand-in for the native core's two round trips: the raw `update` response
 *  bytes for one event's wire JSON, and the raw `view` bytes. */
internal interface CoreFault {
    fun update(event: ByteArray): ByteArray?
    fun view(): ByteArray?
}

/** A [CoreBridge] whose two round trips are [fault]'s (a core that answers null,
 *  an error envelope, or never loaded) and that answers nothing else. */
internal class FaultBridge(@Volatile var fault: CoreFault) : CoreBridge {
    override fun update(event: ByteArray): ByteArray? = fault.update(event)

    override fun view(): ByteArray? = fault.view()

    override fun parseFit(fit: ByteArray): ByteArray? = null

    override fun exportGpx(run: ByteArray): ByteArray? = null

    override fun trackFigures(event: ByteArray): ByteArray? = null

    override fun compactLog(lines: Array<String>): ByteArray? = null

    override fun reset(): ByteArray? = null
}
