package app.milestone

import kotlin.concurrent.Volatile

/** The app clock the host tests run on ([AppRuntime.resetForTest]): the device
 *  clock shifted by [offsetMillis], which a test moves to change "now". */
internal object TestClock : WallClock {
    @Volatile var offsetMillis: Long = 0L

    /** Non-null: [nowMillis] stands still at this instant ([offsetMillis] ignored), so a
     *  ticking readout reads the same value every frame (screenshot captures). */
    @Volatile var frozenAtMillis: Long? = null

    override fun nowMillis(): Long = frozenAtMillis ?: (System.currentTimeMillis() + offsetMillis)

    override fun monotonicMillis(): Long = deviceMonotonicMillis()
}

/** A plain-JVM test's clock (no Robolectric, so no `SystemClock`): the wall clock,
 *  and a monotonic clock that stays at 0. */
internal object PlainJvmClock : WallClock {
    override fun nowMillis(): Long = System.currentTimeMillis()

    override fun monotonicMillis(): Long = 0L
}

/** Restores the JVM default Locale and TimeZone that a test (or [app.milestone.qa.QaHarness.reset]) changed. */
class JvmDefaultsRule : org.junit.rules.ExternalResource() {
    private lateinit var locale: java.util.Locale
    private lateinit var zone: java.util.TimeZone

    override fun before() {
        locale = java.util.Locale.getDefault()
        zone = java.util.TimeZone.getDefault()
    }

    override fun after() {
        java.util.Locale.setDefault(locale)
        java.util.TimeZone.setDefault(zone)
    }
}

/** The event queue's send path in the host tests ([AppRuntime.resetForTest]): the
 *  real engine until a test swaps [sender] / [batchSender] for a fake or a wrapper. */
internal object TestSink : EventSink {
    @Volatile var sender: suspend (Event) -> SendResult = EngineSink::send
    @Volatile var batchSender: suspend (List<Event>) -> SendResult = EngineSink::sendAll

    override suspend fun send(e: Event): SendResult = sender(e)

    override suspend fun sendAll(events: List<Event>): SendResult = batchSender(events)

    fun reset() {
        sender = EngineSink::send
        batchSender = EngineSink::sendAll
    }
}
