package app.milestone

import kotlin.concurrent.Volatile
import kotlin.properties.ReadOnlyProperty
import kotlin.reflect.KProperty
import kotlinx.atomicfu.locks.SynchronizedObject
import kotlinx.atomicfu.locks.synchronized
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel

/**
 * The shell's clock ([AppProcess.clock]): every "now" the app stamps or shows
 * (today's epoch-day, new-entry timestamps, relative dates, rest readouts) reads
 * it. Sensor time (GPS fixes) and entry-id uniqueness read the device clock.
 */
internal val AppClock: WallClock get() = AppProcess.clock

/**
 * A value that lives as long as the app's process: created on first read, gone
 * with the process. Every singleton's process state is one of these, so
 * [AppProcess.reset] starts all of them over at once.
 */
internal class ProcessLocal<T>(internal val create: () -> T) : ReadOnlyProperty<Any?, T> {
    override fun getValue(thisRef: Any?, property: KProperty<*>): T = AppProcess.local(this)
}

/**
 * What every part of the app shares for the life of its process: the coroutine
 * scope, the clock and the process state of every [ProcessLocal]. [AppRuntime]
 * starts the app on it.
 */
internal object AppProcess {
    private class State(val clock: WallClock) : SynchronizedObject() {
        // Work that must outlive a sheet or screen (imports, export, the data
        // restore) and every feature's collectors. Made on first use: a plain-JVM
        // test reads the clock with no main dispatcher.
        val scope: Lazy<CoroutineScope> = lazy { CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate) }
        val locals = HashMap<ProcessLocal<*>, Any?>()
        @Volatile var debuggable = false
    }

    @Volatile private var state = State(DeviceClock)

    val scope: CoroutineScope get() = state.scope.value

    val clock: WallClock get() = state.clock

    /** A debuggable build ([AppRuntime.start]): the engine logs error envelopes in full. */
    var debuggable: Boolean
        get() = state.debuggable
        set(value) {
            state.debuggable = value
        }

    internal fun <T> local(key: ProcessLocal<T>): T {
        val s = state
        return synchronized(s) {
            @Suppress("UNCHECKED_CAST")
            if (key in s.locals) return s.locals[key] as T
            key.create().also { s.locals[key] = it }
        }
    }

    /** A fresh process on [clock] ([AppRuntime.resetForTest]): the scope cancelled
     *  (every feature's collectors) and every [ProcessLocal] gone, closed first when
     *  it is [AutoCloseable] (the event queue). */
    internal fun reset(clock: WallClock) {
        val old = state
        if (old.scope.isInitialized()) old.scope.value.cancel()
        synchronized(old) { old.locals.values.toList() }.forEach { (it as? AutoCloseable)?.close() }
        state = State(clock)
    }
}

/** A piece of the app that runs while its process does, started once by [AppRuntime.start]. */
internal fun interface AppFeature {
    fun start(ctx: PlatformContext)

    /** The wall clock was set, the time zone changed ([zoneChanged]) or local
     *  midnight passed: the local day may differ ([AppRuntime.onTimeChanged]). */
    fun onTimeChanged(ctx: PlatformContext, zoneChanged: Boolean) {}
}
