package app.milestone

import app.milestone.widget.WidgetSurfaces
import kotlin.concurrent.Volatile
import kotlin.properties.ReadOnlyProperty
import kotlin.reflect.KProperty
import kotlinx.atomicfu.locks.SynchronizedObject
import kotlinx.atomicfu.locks.synchronized
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/** The app's clocks: wall time for every "now" the app stamps or shows, monotonic
 *  time (never jumps, keeps counting in deep sleep) for durations. */
internal interface WallClock {
    fun nowMillis(): Long

    fun nowSec(): Long = nowMillis() / 1000L

    fun monotonicMillis(): Long
}

/** The device's clocks. */
internal object DeviceClock : WallClock {
    override fun nowMillis(): Long = wallClockMillis()

    override fun monotonicMillis(): Long = deviceMonotonicMillis()
}

/**
 * The shell's clock ([AppRuntime.clock]): every "now" the app stamps or shows
 * (today's epoch-day, new-entry timestamps, relative dates, rest readouts) reads
 * it. Sensor time (GPS fixes) and entry-id uniqueness read the device clock.
 */
internal val AppClock: WallClock get() = AppRuntime.clock

/** A piece of the app that runs while its process does, started once by [AppRuntime.start]. */
internal fun interface AppFeature {
    fun start(ctx: PlatformContext)

    /** The wall clock was set or the time zone changed ([zoneChanged]): the local
     *  day may differ ([AppRuntime.onTimeChanged]). */
    fun onTimeChanged(ctx: PlatformContext, zoneChanged: Boolean) {}
}

/** The features only this platform has, in start order (common ones: [AppRuntime]). */
internal expect fun platformFeatures(): List<AppFeature>

/**
 * A value that lives as long as the app's process: created on first read, gone
 * with the process. Every singleton's process state is one of these, so
 * [AppRuntime.resetForTest] starts all of them over at once.
 */
internal class ProcessLocal<T>(internal val create: () -> T) : ReadOnlyProperty<Any?, T> {
    override fun getValue(thisRef: Any?, property: KProperty<*>): T = AppRuntime.local(this)
}

/**
 * The app's process: its coroutine scope, core engine, clock, boot ledger and the
 * process state of every feature ([ProcessLocal]). [start] runs once per process
 * on each platform (Android: every component the system may start the process
 * for; iOS: the app's launch), loading the settings and starting each feature.
 */
internal object AppRuntime : BootLedger {
    private class Process(engine: DurableCore?, val sink: EventSink?, val clock: WallClock) : SynchronizedObject() {
        // Work that must outlive a sheet or screen (imports, export, the data
        // restore) and every feature's collectors. Made on first use: a plain-JVM
        // test reads the clock with no main dispatcher.
        val scope: Lazy<CoroutineScope> = lazy { CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate) }
        val engine: Lazy<DurableCore> = lazy { engine ?: platformCoreEngine() }
        val locals = HashMap<ProcessLocal<*>, Any?>()
        var started = false
        var bootHandled = false
        @Volatile var lastToday: Pair<Long, Long>? = null
        @Volatile var debuggable = false
        val generation = MutableStateFlow(0)
    }

    @Volatile private var process = Process(null, null, DeviceClock)

    val scope: CoroutineScope get() = process.scope.value

    /** The core engine: Android's JNI [Core], iOS's engine over the C ABI. Its
     *  first touch belongs on [CoreDispatcher] (Android loads the library then). */
    val engine: DurableCore get() = process.engine.value

    /** Where the event queue sends: [engine] on its thread. */
    internal val sink: EventSink get() = process.sink ?: EngineSink

    val clock: WallClock get() = process.clock

    /** A debuggable build: the engine logs error envelopes in full. */
    val debuggable: Boolean get() = process.debuggable

    /** Common features, in start order; [platformFeatures] follow. */
    private val commonFeatures: List<AppFeature> = listOf(ActiveRunStore, SessionReminder, WidgetSurfaces)

    private val features: List<AppFeature> get() = commonFeatures + platformFeatures()

    /** Start the process once: settings (migrated, then loaded), then every
     *  feature. Later calls do nothing. */
    fun start(ctx: PlatformContext) {
        val p = process
        synchronized(p) {
            if (p.started) return
            p.started = true
        }
        p.debuggable = ctx.isDebuggable()
        AppSettings.migrate(ctx)
        AppSettings.load(ctx)
        features.forEach { it.start(ctx) }
    }

    /** The platform reported a clock or time zone change ([zoneChanged]): every
     *  feature reacts once, in start order. */
    fun onTimeChanged(ctx: PlatformContext, zoneChanged: Boolean) {
        start(ctx)
        features.forEach { it.onTimeChanged(ctx, zoneChanged) }
    }

    internal fun <T> local(key: ProcessLocal<T>): T {
        val p = process
        return synchronized(p) {
            @Suppress("UNCHECKED_CAST")
            if (key in p.locals) return p.locals[key] as T
            key.create().also { p.locals[key] = it }
        }
    }

    // ── Boot ledger ─────────────────────────────────────────────────────────

    override var bootHandled: Boolean
        get() = process.bootHandled
        set(value) {
            process.bootHandled = value
        }

    override var lastToday: Pair<Long, Long>?
        get() = process.lastToday
        set(value) {
            process.lastToday = value
        }

    override val published: ViewModel? get() = EventDispatcher.model.value

    override fun report(r: SendResult) = EventDispatcher.report(r)

    /** Bumped by [requestReboot]; the session's boot effect runs once per value. */
    val bootGeneration: StateFlow<Int> get() = process.generation.asStateFlow()

    /**
     * Boot again in this process, as a fresh process would, after the event log was
     * replaced (a data import): the next boot turn probes the core, empties it
     * ([DurableCore.resetForReboot]), replays the new log and makes the
     * once-per-process decisions again, all in one exclusive turn, so no queued
     * event lands between the reset and the replay. Main thread only.
     */
    fun requestReboot() {
        val p = process
        p.bootHandled = false
        p.lastToday = null
        p.generation.value++
    }

    /**
     * A fresh process for the next test, as Robolectric runs every test in one JVM:
     * the scope cancelled (every feature's collectors), every [ProcessLocal] gone
     * (closed first when it is [AutoCloseable]: the event queue), the boot ledger
     * cleared, nothing started. An engine the process used starts over
     * ([DurableCore.forgetProcess]); one it never touched is left alone (a plain-JVM
     * test loads the native library only when it asks the core). A test may run the process on its
     * own [engine], event [sink] or [clock].
     */
    fun resetForTest(engine: DurableCore? = null, sink: EventSink? = null, clock: WallClock = DeviceClock) {
        val old = process
        if (old.scope.isInitialized()) old.scope.value.cancel()
        synchronized(old) { old.locals.values.toList() }.forEach { (it as? AutoCloseable)?.close() }
        if (old.engine.isInitialized()) old.engine.value.forgetProcess()
        process = Process(engine, sink, clock)
    }
}
