package app.milestone

import kotlin.concurrent.Volatile
import kotlinx.atomicfu.locks.SynchronizedObject
import kotlinx.atomicfu.locks.synchronized
import kotlinx.serialization.SerializationException

/**
 * The core engine both platforms run: the Rust/crux core behind [bridge] plus the
 * durable event log that rebuilds it.
 *
 * The core speaks JSON over the FFI: an [Event] is serialised to the exact serde
 * wire form (externally-tagged enums), pushed through [CoreBridge.update]; the
 * resulting [ViewModel] is read back through [CoreBridge.view]. All coaching
 * logic, evidence grades, safety tiers, contested markers lives in Rust and is
 * rendered verbatim. Accepted events are appended to the event log ([EventLog]),
 * which [restore] replays at boot. Called on [CoreDispatcher] by the event queue
 * ([CoreEngine]).
 *
 * Android's engine is [Core] (the JNI natives); iOS installs a [BridgeCore] over
 * the C ABI. State is guarded by this object's lock (the JVM monitor on Android).
 */
abstract class DurableCore : SynchronizedObject(), CoreEngine {
    /** The core's bridge calls: the JNI natives on Android, the C ABI on iOS. */
    protected abstract val bridge: CoreBridge

    /** Why the platform's core library failed to load, or null. Only Android can
     *  fail here (`System.loadLibrary`); the iOS core is linked into the app. */
    internal open val loadError: Throwable? get() = null

    /** TEST-ONLY fault injection: when set, every core round trip ([coreUpdate]/
     *  [coreView]) goes here instead of [bridge], so a test can boot the app on a
     *  core that answers null (an emulated arm64 library under translation, a JNI
     *  allocation failure) or throws [UnsatisfiedLinkError] (a library that never
     *  loaded). Never set in the app. */
    @Volatile internal var faultForTest: CoreFault? = null

    private fun coreUpdate(event: ByteArray): ByteArray? = faultForTest.let { if (it != null) it.update(event) else bridge.update(event) }

    private fun coreView(): ByteArray? = faultForTest.let { if (it != null) it.view() else bridge.view() }

    /** Latched for the process once boot found the core unusable ([probe] failed, or
     *  [markUnavailable] after the boot clock event was refused). While set, [apply]
     *  refuses every event and [restore] replays nothing, so a dead core can never
     *  append to (or compete with) the user's durable log. */
    @Volatile var unavailable = false
        private set

    /**
     * Boot health check, run BEFORE [restore] touches the log: the library loaded and
     * the core's view decodes as a ViewModel. A core that answers null (every JNI call
     * failing, as a release arm64-only APK does under x86_64 translation), an error
     * envelope or undecodable bytes, or that throws a linkage error, is unavailable:
     * latches [unavailable] and returns false. Reads only; applies no event.
     */
    fun probe(): Boolean = synchronized(this) {
        if (unavailable) return false
        val ok = loadError == null && try {
            val bytes = coreView()
            bytes != null && bytes.decodeToString().let { text ->
                !isCoreErrorEnvelope(text) &&
                    runCatching { coreJson.decodeFromString(ViewModel.serializer(), text) }.isSuccess
            }
        } catch (e: Throwable) {
            if (!e.isLinkageError()) throw e
            false
        }
        if (!ok) {
            PlatformLog.e("Core", "native core unavailable at boot", loadError)
            unavailable = true
        }
        ok
    }

    /** Latch [unavailable] after boot: the core read a view but refused the boot
     *  clock event every working core accepts. */
    fun markUnavailable() = synchronized(this) {
        PlatformLog.e("Core", "native core refused the boot clock event, treating it as unavailable")
        unavailable = true
    }

    /** TEST-ONLY: undo [unavailable] (with [faultForTest] cleared) between scenarios. */
    internal fun clearUnavailableForTest() = synchronized(this) {
        unavailable = false
    }

    /** The distance and moving time [track] will be SAVED with, from the core's own
     *  QC and derivation (the same as the saved run's card), so the Finish review
     *  shows what History will list. Null when the core can't answer (an error
     *  envelope or an allocation failure). O(fixes): fine on the main thread for
     *  a capped (decimated) track. */
    fun savedFigures(track: Event.LogRunTrack): TrackFigures? {
        val json = bridge.trackFigures(track.toJson().toString().encodeToByteArray()) ?: return null
        return runCatching { coreJson.decodeFromString(TrackFigures.serializer(), json) }.getOrNull()
    }

    /** The core's compaction verdict on the event-log [lines] (log.rs), or null when
     *  the core can't answer (the library didn't load, it is [unavailable] or under
     *  [faultForTest], or the call failed): the caller then keeps every line. */
    internal fun compaction(lines: List<String>): LogCompaction? {
        if (loadError != null || unavailable || faultForTest != null) return null
        val json = try {
            bridge.compactLog(lines.toTypedArray())
        } catch (e: Throwable) {
            if (!e.isLinkageError()) throw e
            null
        } ?: return null
        return runCatching { coreJson.decodeFromString(LogCompaction.serializer(), json) }.getOrNull()
    }

    // Append-only, newline-delimited event log. The crux core holds all state in
    // memory only, so we persist the event *stream* (not the derived model) and
    // rebuild by replaying it. Each line is one event's exact serde wire JSON.
    private var log: LogPath? = null

    // The native core holds all state in memory for the whole process lifetime,
    // so replaying the log more than once (e.g. on Activity recreation across a
    // config change) would double every logged lift/run. Guard so replay happens
    // exactly once per process.
    private var restored = false

    /** Whether the log this process replayed ended with the plan removed (its last
     *  plan line a kept `ClearPlan`, [RestoredLog.planRemoved]). Boot reads it so a
     *  removed plan stays removed when the setup prefs' marker was lost. */
    var restoredPlanRemoved = false
        private set

    /** The app's debuggable flag ([restore]): error envelopes are logged in full. */
    @Volatile private var debuggable = false

    /**
     * Point the core at [ctx]'s event log and replay it to rebuild state. Safe to
     * call on every Activity creation: replay runs only on the first call per
     * process. Returns true only on a genuinely FRESH INSTALL: the log file
     * never existed, so the caller may seed a default profile + onboarding. A
     * log that exists but compacts to zero surviving lines (a returning user who
     * cleared everything) returns false: replaying "nothing left" is their real
     * state and must not be re-seeded over. Read, repair and compaction live in
     * [EventLog.load]; this only replays the survivors.
     *
     * Never throws for a bad log: an unreadable log is logged and leaves the core
     * empty, a line that resets the core is skipped, and either is reported as
     * [SendResult.stale] on every later send.
     */
    fun restore(ctx: PlatformContext): Boolean = synchronized(this) {
        debuggable = ctx.isDebuggable()
        // An unavailable core never gets the log: nothing is replayed into it and no
        // event it might accept can be appended (the boot shows the error state).
        if (unavailable) return false
        restoreFrom(ctx.eventLogPath())
    }

    /** [restore] from the event log at [file]. */
    fun restoreFrom(file: LogPath): Boolean = synchronized(this) {
        if (unavailable) return false
        // Already replayed this process (Activity recreation): the only way this
        // is still a fresh install is if nothing was ever persisted. [log] stays as
        // it is, so a recreation can't re-attach a log a data import detached.
        if (restored) return !LogFiles.exists(file)
        // Marked BEFORE replaying: the native core keeps whatever a partial replay
        // applied, so a second attempt in this process would double those events.
        restored = true
        log = file
        restoredPlanRemoved = false
        // Compacting before replay matters here: a run's GPS track (thousands of
        // points) is the heaviest line in the log, and every `Clear*` supersedes
        // its whole family: the compacted replay is model-equivalent.
        val loaded = try {
            EventLog.load(file, this)
        } catch (e: Exception) {
            PlatformLog.e("Core", "event log unreadable, starting without it", e)
            logDegraded = true
            appendTornGuard = true
            return false
        }
        // Entry ids seed from the wall clock; after the clock moved back they must
        // still never reuse an id a restored entry carries.
        EntryIds.raiseTo(loaded.maxEntryId)
        restoredPlanRemoved = loaded.planRemoved
        // A failed torn-tail repair (often disk full, the same cause as the tear)
        // leaves the durable tail newline-less: the next append must lead with one.
        appendTornGuard = runCatching { LogFiles.exists(file) && !LogFiles.endsWithNewline(file) }.getOrDefault(true)
        if (replay(loaded.lines)) logDegraded = true
        loaded.freshInstall
    }

    /** Stop persisting: later accepted events stay in memory only. The data import
     *  calls this inside an exclusive turn before replacing the log file, so nothing
     *  can append to the imported log before the process restarts. Returns the
     *  detached file for [reattachLog]. */
    fun detachLog(): LogPath? = synchronized(this) { log.also { log = null } }

    /** Undo [detachLog] after a failed swap: the original log is still in place. */
    fun reattachLog(file: LogPath?) = synchronized(this) {
        log = file
    }

    // The last ViewModel that decoded cleanly. If a later core call comes back as
    // a panic-firewall error envelope, we return this instead of an all-default
    // blank one, a blank would silently wipe a live DO-NOT-TRAIN hold (HARD RULE 3).
    private var lastGoodView: ViewModel? = null

    // A prior [send] append that failed mid-write may have left a torn
    // (newline-less) partial line in the log; the next append leads with a newline
    // so two events can never glue into one unparseable line.
    private var appendTornGuard = false

    // The last SetToday applied: it is not in the log, so [replayLogAfterReset]
    // re-sends it after a panic_reset rebuild.
    private var lastSetToday: Event? = null

    // True while the last [currentView] could not produce a fresh view (null bytes,
    // an error envelope, or a decode failure) and a stand-in was returned.
    private var viewStale = false

    // True once the in-memory core is known to be missing part of the durable log
    // (an unreadable log, or replayed lines skipped because they reset the core).
    // Stays set for the process: those events are absent until the next boot.
    private var logDegraded = false

    /** How [apply] ended for one event. */
    private enum class Applied { Rejected, Saved, NotPersisted }

    /** Apply one event: dispatch it to the core, persist it when the core accepted it
     *  and it is [Event.persist], log a rejection; no view read (the caller reads once
     *  per batch). Called only under this object's lock, so the log order equals the
     *  apply order and a replay rebuilds the same model. */
    private fun apply(event: Event): Applied {
        // A core found unusable at boot refuses everything: nothing reaches the log.
        if (unavailable) return Applied.Rejected
        val wire = event.toJson().toString()
        val response = coreUpdate(wire.encodeToByteArray())
        if (isPanicResetResponse(response)) {
            // The FFI caught a panic and swapped in a FRESH (empty) core bridge.
            // The offending event was never applied or persisted; rebuild the fresh
            // core from the durable log exactly like boot restore and re-establish
            // today's date, so the next view() is a real model, not a blank one (which
            // would wipe a live DO-NOT-TRAIN hold). Reported not-accepted so the caller
            // tells the user the entry wasn't saved.
            PlatformLog.e("Core", "event reset the core: ${event::class.simpleName}")
            replayLogAfterReset()
            return Applied.Rejected
        }
        // A serde-rejected event returns empty bytes (ffi out.clear()); a core panic
        // returns an {"error":...} object; a null is a byte-array alloc failure.
        // None is persisted: the line would just be re-dropped on every replay.
        if (!isAccepted(response)) {
            PlatformLog.e("Core", "event rejected: ${event::class.simpleName}")
            return Applied.Rejected
        }
        // Cache the shell's clock so a later panic_reset can re-establish it.
        if (event is Event.SetToday) lastSetToday = event
        val file = log
        if (!event.persist || file == null) return Applied.Saved
        return try {
            // Lead with a newline when a prior append tore off mid-write, so
            // this event can never glue onto a newline-less partial line.
            val prefix = if (appendTornGuard) "\n" else ""
            LogFiles.append(file, prefix + wire + "\n")
            appendTornGuard = false
            Applied.Saved
        } catch (e: Exception) {
            // Disk full / IO error: the in-memory core already applied the event, so
            // the UI shows it, but it is gone after the next restart; the caller
            // tells the user. Arm the torn-line guard so the next append can't
            // concatenate onto this partial write.
            appendTornGuard = true
            PlatformLog.w("Core", "failed to persist event to log", e)
            Applied.NotPersisted
        }
    }

    /** Replay [lines] into the native core with [replayLog]'s panic-reset recovery.
     *  Returns true when the core ended up missing any line. Raw [CoreBridge.update],
     *  never [apply], so a reset during replay cannot recurse into [replayLogAfterReset]. */
    private fun replay(lines: List<String>): Boolean {
        val outcome = replayLog(lines, MAX_REPLAY_RESETS) { coreUpdate(it.encodeToByteArray()) }
        if (outcome.skipped.isEmpty() && outcome.complete) return false
        val which = outcome.skipped.joinToString { "#$it ${EventLog.variantOf(lines[it])}" }
        val gaveUp = if (outcome.complete) "" else "; gave up after $MAX_REPLAY_RESETS resets"
        PlatformLog.e("Core", "replay skipped ${outcome.skipped.size} line(s) that reset the core ($which)$gaveUp")
        return true
    }

    /** Rebuild a freshly-reset core from the durable log, exactly like boot
     *  [restore], then re-send the last [Event.SetToday] (non-persisted, absent from
     *  the log) so the rebuilt core keeps the clock it had before the reset. Reads
     *  the log read-only ([EventLog.read]): boot already repaired it. */
    private fun replayLogAfterReset() {
        val file = log ?: return
        val lines = try {
            EventLog.read(file, this).lines
        } catch (e: Exception) {
            PlatformLog.e("Core", "event log unreadable during reset recovery", e)
            logDegraded = true
            emptyList()
        }
        if (replay(lines)) logDegraded = true
        lastSetToday?.let { coreUpdate(it.toJson().toString().encodeToByteArray()) }
    }

    /** Dispatch one event via [apply], then return the fresh view model plus how
     *  the event fared, as a [SendResult] the caller surfaces. Applies the event
     *  BEFORE reading the view so the returned model reflects it. */
    override fun send(event: Event): SendResult = synchronized(this) {
        val applied = apply(event)
        val view = currentView()
        SendResult(
            view,
            accepted = applied != Applied.Rejected,
            persisted = applied != Applied.NotPersisted,
            stale = viewStale || logDegraded,
        )
    }

    /** Apply a batch of events under ONE lock turn, then decode the view a
     *  SINGLE time (a multi-activity import builds several runs and would otherwise
     *  rebuild the O(history) view once per event). `accepted` is true iff every
     *  event was accepted; `persisted` iff no accepted event failed to reach the log. */
    override fun sendAll(events: List<Event>): SendResult = synchronized(this) {
        var accepted = true
        var persisted = true
        for (event in events) {
            when (apply(event)) {
                Applied.Rejected -> accepted = false
                Applied.NotPersisted -> persisted = false
                Applied.Saved -> {}
            }
        }
        val view = currentView()
        SendResult(view, accepted, persisted, stale = viewStale || logDegraded)
    }

    /** True if the core ACCEPTED an event: crux emits a non-empty JSON effect
     *  array. A rejected event yields empty bytes; a panic yields an `{"error"…}`
     *  object (leading `{`); a null (alloc failure) is likewise not accepted. */
    private fun isAccepted(response: ByteArray?): Boolean = response.firstNonBlank() == '['.code.toByte()

    /** Current view model without dispatching an event. Shares this object's lock
     *  with [send]/[restore] so a read-back never observes a half-applied event or
     *  races the [lastGoodView] read/write. Every path that cannot produce a fresh
     *  view returns the last good one (never a blank that would wipe a live
     *  DO-NOT-TRAIN hold) and marks the result stale. */
    fun currentView(): ViewModel = synchronized(this) {
        // A null return is the byte-array alloc failure (ffi byte_array_or_null
        // exhausted its retries).
        val bytes = coreView() ?: run {
            PlatformLog.e("Core", "core view() returned null (alloc failure), keeping last good view")
            return staleView()
        }
        // view() never resets the bridge (ffi.rs `catch_panic("view", false, …)`: a
        // panic there does not poison the model), so a panic arrives as the plain
        // error envelope handled below.
        val text = bytes.decodeToString()
        // The FFI panic firewall returns {"error":{...}} on a core panic. With
        // ignoreUnknownKeys that would decode into an all-default ViewModel -
        // train_blocked=false, safety_tier=null, silently blanking safety state.
        if (isCoreErrorEnvelope(text)) {
            val detail = if (debuggable) ": $text" else ""
            PlatformLog.e("Core", "core returned error envelope, keeping last good view$detail")
            return staleView()
        }
        val vm = try {
            coreJson.decodeFromString(ViewModel.serializer(), text)
        } catch (e: SerializationException) {
            PlatformLog.e("Core", "core view failed to decode, keeping last good view", e)
            return staleView()
        }
        viewStale = false
        lastGoodView = vm
        vm
    }

    private fun staleView(): ViewModel {
        viewStale = true
        return lastGoodView ?: ViewModel()
    }
}

/** A [DurableCore] over any [CoreBridge]: iOS's engine over the C ABI
 *  (`kmp/core-bridge`'s `NativeCore`), and the tests' scripted cores. */
class BridgeCore(override val bridge: CoreBridge) : DurableCore()

/** Whether [this] is the JVM's `LinkageError` (a native library or symbol that
 *  never loaded). Kotlin/Native links the core statically, so never on iOS. */
internal expect fun Throwable.isLinkageError(): Boolean
