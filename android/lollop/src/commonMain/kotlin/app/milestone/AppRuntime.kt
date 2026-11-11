package app.milestone

import app.milestone.widget.WidgetSurfaces
import kotlin.concurrent.Volatile
import kotlinx.atomicfu.locks.SynchronizedObject
import kotlinx.atomicfu.locks.synchronized
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/** The features only this platform has, in start order (common ones: [AppRuntime]). */
internal expect fun platformFeatures(): List<AppFeature>

/**
 * The app on its [AppProcess] and [AppCore]: the boot ledger and the features.
 * [start] runs once per process
 * on each platform (Android: every component the system may start the process
 * for; iOS: the app's launch), loading the settings and starting each feature.
 */
internal object AppRuntime : BootLedger {
    private class Process : SynchronizedObject() {
        var started = false
        var bootHandled = false
        @Volatile var lastToday: Pair<Long, Long>? = null
        val generation = MutableStateFlow(0)
    }

    @Volatile private var process = Process()

    /** Common features, in start order. */
    private val commonFeatures: List<AppFeature> = listOf(ActiveRunStore, SessionReminder, WidgetSurfaces)

    private val features: List<AppFeature> get() = commonFeatures + platformFeatures()

    /** Start the process once; later calls do nothing. */
    fun start(ctx: PlatformContext) {
        val p = process
        synchronized(p) {
            if (p.started) return
            p.started = true
        }
        AppProcess.debuggable = ctx.isDebuggable()
        AppSettings.migrate(ctx)
        AppSettings.load(ctx)
        features.forEach { it.start(ctx) }
    }

    /** The platform reported a clock or time zone change ([zoneChanged]) or local
     *  midnight: every feature reacts once, in start order. */
    fun onTimeChanged(ctx: PlatformContext, zoneChanged: Boolean) {
        start(ctx)
        features.forEach { it.onTimeChanged(ctx, zoneChanged) }
    }

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
     * A fresh process for the next test, as Robolectric runs every test in one JVM.
     * A test may run the process on its own [engine], event [sink] or [clock].
     */
    fun resetForTest(engine: DurableCore? = null, sink: EventSink? = null, clock: WallClock = DeviceClock) {
        AppProcess.reset(clock)
        AppCore.reset(engine, sink)
        process = Process()
    }
}
