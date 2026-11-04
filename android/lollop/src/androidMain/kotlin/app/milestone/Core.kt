package app.milestone

import android.content.Context
import java.io.File
import kotlinx.serialization.SerializationException

/**
 * JNI bridge to the Rust/crux core (`libshared.so`).
 *
 * The core speaks JSON over the FFI: an [Event] is serialised to the exact
 * serde wire form (externally-tagged enums), pushed via [update]; the resulting
 * [ViewModel] is read back via [view]. All coaching logic, evidence grades,
 * safety tiers, contested markers lives in Rust and is rendered verbatim.
 * Called on [CoreDispatcher] by the event queue ([CoreEngine]).
 */
object Core : CoreEngine {
    /** Why `libshared.so` failed to load (a missing or ABI-incompatible library),
     *  or null. Caught instead of letting the initializer throw, which would make
     *  every later touch of [Core] a NoClassDefFoundError crash: boot reads it via
     *  [probe] and shows the core-unavailable state instead. */
    internal val loadError: Throwable? = try {
        System.loadLibrary("shared")
        null
    } catch (e: UnsatisfiedLinkError) {
        e
    } catch (e: SecurityException) {
        e
    }

    private external fun update(event: ByteArray): ByteArray?
    private external fun view(): ByteArray?

    /** The JNI exports as the platform-neutral [CoreBridge] every call below goes
     *  through. The natives keep their names and signatures: `Java_app_milestone_Core_*`. */
    private val natives: CoreBridge = object : CoreBridge {
        override fun update(event: ByteArray): ByteArray? = this@Core.update(event)
        override fun view(): ByteArray? = this@Core.view()
        override fun parseFit(bytes: ByteArray): String? = this@Core.parseFit(bytes)
        override fun exportGpx(entryId: Long, observedAt: Long): String? = this@Core.exportGpx(entryId, observedAt)
        override fun trackFigures(event: ByteArray): String? = this@Core.trackFigures(event)
        override fun compactLog(lines: Array<String>): String? = this@Core.compactLog(lines)
    }

    /** TEST-ONLY fault injection: when set, every core round trip ([update]/[view])
     *  goes here instead of the JNI library, so a JVM test can boot the app on a core
     *  that answers null (an emulated arm64 library under translation, a JNI
     *  allocation failure) or throws [UnsatisfiedLinkError] (a library that never
     *  loaded). Never set on device. */
    @Volatile internal var faultForTest: CoreFault? = null

    private fun jniUpdate(event: ByteArray): ByteArray? = faultForTest.let { if (it != null) it.update(event) else natives.update(event) }

    private fun jniView(): ByteArray? = faultForTest.let { if (it != null) it.view() else natives.view() }

    /** Latched for the process once boot found the core unusable ([probe] failed, or
     *  [markUnavailable] after the boot clock event was refused). While set, [apply]
     *  refuses every event and [restore] replays nothing, so a dead core can never
     *  append to (or compete with) the user's durable log. */
    @Volatile var unavailable = false
        private set

    /**
     * Boot health check, run BEFORE [restore] touches the log: the library loaded and
     * [view] returns a ViewModel that decodes. A core that answers null (every JNI call
     * failing, as a release arm64-only APK does under x86_64 translation), an error
     * envelope or undecodable bytes, or that throws [LinkageError], is unavailable:
     * latches [unavailable] and returns false. Reads only; applies no event.
     */
    @Synchronized
    fun probe(): Boolean {
        if (unavailable) return false
        val ok = loadError == null && try {
            val bytes = jniView()
            bytes != null && String(bytes, Charsets.UTF_8).let { text ->
                !isCoreErrorEnvelope(text) &&
                    runCatching { coreJson.decodeFromString(ViewModel.serializer(), text) }.isSuccess
            }
        } catch (e: LinkageError) {
            false
        }
        if (!ok) {
            PlatformLog.e("Core", "native core unavailable at boot", loadError)
            unavailable = true
        }
        return ok
    }

    /** Latch [unavailable] after boot: the core read a view but refused the boot
     *  clock event every working core accepts. */
    @Synchronized
    fun markUnavailable() {
        PlatformLog.e("Core", "native core refused the boot clock event, treating it as unavailable")
        unavailable = true
    }

    /** TEST-ONLY: undo [unavailable] (with [faultForTest] cleared) between scenarios. */
    @Synchronized
    internal fun clearUnavailableForTest() {
        unavailable = false
    }

    /** TEST-ONLY bridge reset: drops the native core's global
     *  bridge and rebuilds it empty so a host-JVM UI/UX QA scenario starts from
     *  a clean core. Present only in the `jni-host` host build the QA gradle task
     *  links (`Java_app_milestone_Core_resetForTest`); it is never compiled into a
     *  device APK and never called on device, so the missing device symbol is
     *  inert (JNI resolves natives lazily, at first call). */
    external fun resetForTest()

    /** Pure FIT-file parse in the Rust core (fitparser), stateless, touches no
     *  model/log. Returns `{"segments":[[{lat,lon,time_sec,hr_bpm?},…],…]}` or
     *  `{"error":"…"}`; the shell converts into its LogRunTrack import path.
     *  Nullable: the hardened JNI returns a null jstring if the result string
     *  can't be allocated (double alloc-failure), the decode site treats null
     *  like the error envelope. */
    external fun parseFit(bytes: ByteArray): String?

    /** Build the GPX 1.1 document for ONE logged run, matched by the same (entry_id,
     *  observed_at) identity as delete/amend. Returns "" for a hand-entered run or
     *  an unknown id; for a GPS run QC left unmeasurable (gps_unmeasurable) it is
     *  the recorded fixes, read only by Undo (no route to map or share). null only
     *  if the JNI string couldn't be allocated (the caller treats null like ""). Reads
     *  the registry the last [view] populated, so call it after the run list rendered.
     *  Heavy (builds + returns the whole document): call OFF the main thread. */
    external fun exportGpx(entryId: Long, observedAt: Long): String?

    /** Pure core query (no model/log): `{"distance_km":…,"duration_min":…}` for a
     *  `LogRunTrack` event's wire JSON, or `{"error":…}`; null only if the JNI
     *  string couldn't be allocated. See [savedFigures]. */
    private external fun trackFigures(event: ByteArray): String?

    /** The distance and moving time [track] will be SAVED with, from the core's own
     *  QC and derivation (the same as the saved run's card), so the Finish review
     *  shows what History will list. Null when the core can't answer (an error
     *  envelope or a JNI allocation failure). O(fixes): fine on the main thread for
     *  a capped (decimated) track. */
    fun savedFigures(track: Event.LogRunTrack): TrackFigures? {
        val json = natives.trackFigures(track.toJson().toString().toByteArray(Charsets.UTF_8)) ?: return null
        return runCatching { coreJson.decodeFromString(TrackFigures.serializer(), json) }.getOrNull()
    }

    /** Pure core query (no model/log): the [LogCompaction] JSON for the event-log
     *  [lines], or null. See [compaction]. */
    private external fun compactLog(lines: Array<String>): String?

    /** The core's compaction verdict on the event-log [lines] (log.rs), or null when
     *  the core can't answer (the library didn't load, it is [unavailable] or under
     *  [faultForTest], or the call failed): the caller then keeps every line. */
    internal fun compaction(lines: List<String>): LogCompaction? {
        if (loadError != null || unavailable || faultForTest != null) return null
        val json = try {
            natives.compactLog(lines.toTypedArray())
        } catch (e: LinkageError) {
            null
        } ?: return null
        return runCatching { coreJson.decodeFromString(LogCompaction.serializer(), json) }.getOrNull()
    }

    // Append-only, newline-delimited event log. The crux core holds all state in
    // memory only, so we persist the event *stream* (not the derived model) and
    // rebuild by replaying it. Each line is one event's exact serde wire JSON.
    private var log: File? = null

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

    /** The app's debuggable flag ([restore]); stands in for BuildConfig.DEBUG,
     *  which the library module doesn't have. */
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
    @Synchronized
    fun restore(ctx: Context): Boolean {
        debuggable = (ctx.applicationInfo.flags and android.content.pm.ApplicationInfo.FLAG_DEBUGGABLE) != 0
        // An unavailable core never gets the log: nothing is replayed into it and no
        // event it might accept can be appended (the boot shows the error state).
        if (unavailable) return false
        val file = eventLogFile(ctx.filesDir)
        // Already replayed this process (Activity recreation): the only way this
        // is still a fresh install is if nothing was ever persisted. [log] stays as
        // it is, so a recreation can't re-attach a log a data import detached.
        if (restored) return !file.exists()
        // Marked BEFORE replaying: the native core keeps whatever a partial replay
        // applied, so a second attempt in this process would double those events.
        restored = true
        log = file
        restoredPlanRemoved = false
        // Compacting before replay matters here: a run's GPS track (thousands of
        // points) is the heaviest line in the log, and every `Clear*` supersedes
        // its whole family: the compacted replay is model-equivalent.
        val loaded = try {
            EventLog.load(file)
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
        appendTornGuard = runCatching { file.exists() && !EventLog.endsWithNewline(file) }.getOrDefault(true)
        if (replay(loaded.lines)) logDegraded = true
        return loaded.freshInstall
    }

    /** Stop persisting: later accepted events stay in memory only. The data import
     *  calls this inside an exclusive turn before replacing the log file, so nothing
     *  can append to the imported log before the process restarts. Returns the
     *  detached file for [reattachLog]. */
    @Synchronized
    fun detachLog(): File? = log.also { log = null }

    /** Undo [detachLog] after a failed swap: the original log is still in place. */
    @Synchronized
    fun reattachLog(file: File?) {
        log = file
    }

    // The last ViewModel that decoded cleanly. If a later core call comes back as
    // a panic-firewall error envelope, we return this instead of an all-default
    // blank one, a blank would silently wipe a live DO-NOT-TRAIN hold (HARD RULE 3).
    private var lastGoodView: ViewModel? = null

    // A prior [send] appendText that failed mid-write may have left a torn
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
     *  per batch). Called only under the [Core] monitor, so the log order equals the
     *  apply order and a replay rebuilds the same model. */
    private fun apply(event: Event): Applied {
        // A core found unusable at boot refuses everything: nothing reaches the log.
        if (unavailable) return Applied.Rejected
        val wire = event.toJson().toString()
        val response = jniUpdate(wire.toByteArray(Charsets.UTF_8))
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
        // returns an {"error":...} object; a null is a JNI byte-array alloc failure.
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
            file.appendText(prefix + wire + "\n")
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
     *  Returns true when the core ended up missing any line. Raw JNI [update], never
     *  [apply], so a reset during replay cannot recurse into [replayLogAfterReset]. */
    private fun replay(lines: List<String>): Boolean {
        val outcome = replayLog(lines, MAX_REPLAY_RESETS) { jniUpdate(it.toByteArray(Charsets.UTF_8)) }
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
            EventLog.read(file).lines
        } catch (e: Exception) {
            PlatformLog.e("Core", "event log unreadable during reset recovery", e)
            logDegraded = true
            emptyList()
        }
        if (replay(lines)) logDegraded = true
        lastSetToday?.let { jniUpdate(it.toJson().toString().toByteArray(Charsets.UTF_8)) }
    }

    /** Dispatch one event via [apply], then return the fresh view model plus how
     *  the event fared, as a [SendResult] the caller surfaces. Applies the event
     *  BEFORE reading the view so the returned model reflects it. */
    @Synchronized
    override fun send(event: Event): SendResult {
        val applied = apply(event)
        val view = currentView()
        return SendResult(
            view,
            accepted = applied != Applied.Rejected,
            persisted = applied != Applied.NotPersisted,
            stale = viewStale || logDegraded,
        )
    }

    /** Apply a batch of events under ONE monitor turn, then decode the view a
     *  SINGLE time (a multi-activity import builds several runs and would otherwise
     *  rebuild the O(history) view once per event). `accepted` is true iff every
     *  event was accepted; `persisted` iff no accepted event failed to reach the log. */
    @Synchronized
    override fun sendAll(events: List<Event>): SendResult {
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
        return SendResult(view, accepted, persisted, stale = viewStale || logDegraded)
    }

    /** True if the core ACCEPTED an event: crux emits a non-empty JSON effect
     *  array. A rejected event yields empty bytes; a panic yields an `{"error"…}`
     *  object (leading `{`); a null (JNI alloc failure) is likewise not accepted. */
    private fun isAccepted(response: ByteArray?): Boolean = response.firstNonBlank() == '['.code.toByte()

    /** Current view model without dispatching an event. Shares the [Core] monitor
     *  with [send]/[restore] so a read-back never observes a half-applied event or
     *  races the [lastGoodView] read/write. Every path that cannot produce a fresh
     *  view returns the last good one (never a blank that would wipe a live
     *  DO-NOT-TRAIN hold) and marks the result stale. */
    @Synchronized
    fun currentView(): ViewModel {
        // A null return is the JNI byte-array alloc failure (ffi byte_array_or_null
        // exhausted its retries).
        val bytes = jniView() ?: run {
            PlatformLog.e("Core", "core view() returned null (JNI alloc failure), keeping last good view")
            return staleView()
        }
        // view() never resets the bridge (ffi.rs `catch_panic("view", false, …)`: a
        // panic there does not poison the model), so a panic arrives as the plain
        // error envelope handled below.
        val text = String(bytes, Charsets.UTF_8)
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
        return vm
    }

    private fun staleView(): ViewModel {
        viewStale = true
        return lastGoodView ?: ViewModel()
    }
}
