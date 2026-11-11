package app.milestone

import app.milestone.core.CoreBridge
import kotlin.concurrent.Volatile
import kotlinx.atomicfu.locks.SynchronizedObject
import kotlinx.atomicfu.locks.synchronized
import kotlinx.serialization.KSerializer
import kotlinx.serialization.SerializationException
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.builtins.serializer

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
 * Android's engine is [Core] (over JNI), iOS's one over the C ABI, tests' over
 * scripted bridges. State is guarded by this object's lock (the JVM monitor on
 * Android).
 */
open class DurableCore(private val bridge: CoreBridge) : SynchronizedObject(), CoreEngine, LogCompaction {
    /** Why the platform's core library failed to load, or null. Only Android can
     *  fail here (`System.loadLibrary`); the iOS core is linked into the app. */
    internal open val loadError: Throwable? get() = null

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
        val failure: Throwable? = loadError ?: try {
            val text = bridge.view()?.decodeToString()
            when {
                text == null -> IllegalStateException("view() answered null")
                isCoreErrorEnvelope(text) -> IllegalStateException("view() answered an error envelope")
                else -> try {
                    coreJson.decodeFromString(ViewModel.serializer(), text)
                    null
                } catch (e: SerializationException) {
                    e
                }
            }
        } catch (e: Throwable) {
            if (!e.isLinkageError()) throw e
            e
        }
        if (failure != null) {
            PlatformLog.e("Core", "native core unavailable at boot", failure)
            unavailable = true
        }
        failure == null
    }

    /** Latch [unavailable] after boot: the core read a view but refused the boot
     *  clock event every working core accepts. */
    fun markUnavailable() = synchronized(this) {
        PlatformLog.e("Core", "native core refused the boot clock event, treating it as unavailable")
        unavailable = true
    }

    /** The distance and moving time [track] will be SAVED with, from the core's own
     *  QC and derivation (the same as the saved run's card), so the Finish review
     *  shows what History will list. Null when the core can't answer. O(fixes):
     *  fine on the main thread for a capped (decimated) track. */
    fun savedFigures(track: Event.LogRunTrack): TrackFigures? {
        val json = query { bridge.trackFigures(track.toJson().toString().encodeToByteArray()) } ?: return null
        return try {
            coreJson.decodeFromString(TrackFigures.serializer(), json)
        } catch (e: SerializationException) {
            PlatformLog.w("Core", "track figures unreadable", e)
            null
        }
    }

    /** The core's import verdict on a run file's [bytes] (`ParseRunFile`: FIT, GPX or
     *  TCX): the runs to log, or why the file holds none; null when the core can't
     *  answer. Reads no core state, so it runs on the calling thread. */
    fun parseRunFile(bytes: ByteArray): ImportVerdict? =
        decodeQuery("run file import", ImportVerdict.serializer()) { bridge.parseRunFile(bytes) }

    /** The core's match key of each of [names], in order (`names::exercise_key`:
     *  two spellings of one exercise share a key); null when the core can't answer.
     *  Reads no core state, so it runs on the calling thread. */
    fun exerciseKeys(names: List<String>): List<String>? =
        decodeQuery("exercise keys", ListSerializer(String.serializer())) { bridge.exerciseKeys(names.toTypedArray()) }

    /** The core's warm-up ramp for [query] (`strength::warm_up_ramp`, STR-WARMUP-001);
     *  null when the core can't answer. Reads no core state, so it runs on the
     *  calling thread. */
    fun warmUpRamp(query: WarmUpQuery): List<WarmUpStep>? =
        decodeQuery("warm-up ramp", ListSerializer(WarmUpStep.serializer())) {
            bridge.warmUpRamp(coreJson.encodeToString(WarmUpQuery.serializer(), query).encodeToByteArray())
        }

    /** The core's reading of a profile still being edited (`DraftFacts` op): the
     *  age, the youth and health gates it would raise and the PAR-Q+ answers after
     *  [ProfileDraft.answer]; null when the core can't answer. Reads no core state,
     *  so it runs on the calling thread. */
    fun draftFacts(draft: ProfileDraft): DraftFacts? =
        decodeQuery("draft facts", DraftFacts.serializer()) {
            bridge.draftFacts(coreJson.encodeToString(ProfileDraft.serializer(), draft).encodeToByteArray())
        }

    /** The core's thinning of a recorded track (`ThinTrack` op, `gps::thin_track`):
     *  the fixes a save or a crash recovery keeps and the segment starts among them;
     *  null when the core can't answer. Reads no core state, so it runs on the
     *  calling thread. */
    fun thinTrack(track: TrackThinning): ThinnedTrack? =
        decodeQuery("track thinning", ThinnedTrack.serializer()) {
            bridge.thinTrack(coreJson.encodeToString(TrackThinning.serializer(), track).encodeToByteArray())
        }

    /** An imported run's `hr_pct_max` (`ImportedHrPct` op,
     *  `import::imported_hr_pct_max`); null when the core can't answer. Reads no
     *  core state, so it runs on the calling thread. */
    fun importedHrPct(hr: ImportedHr): Double? =
        decodeQuery("imported heart rate", Double.serializer()) {
            bridge.importedHrPct(coreJson.encodeToString(ImportedHr.serializer(), hr).encodeToByteArray())
        }

    private inline fun <T> decodeQuery(what: String, serializer: KSerializer<T>, call: () -> ByteArray?): T? {
        val json = query(call) ?: return null
        if (isCoreErrorEnvelope(json)) {
            PlatformLog.w("Core", "$what failed: $json")
            return null
        }
        return try {
            coreJson.decodeFromString(serializer, json)
        } catch (e: SerializationException) {
            PlatformLog.w("Core", "$what unreadable", e)
            null
        }
    }

    /** The GPX 1.1 document of ONE logged run, by the same (entry_id, observed_at)
     *  identity as delete/amend: "" for a hand-entered run, an unknown id, or a core
     *  that can't answer. Heavy, so off the main thread. */
    fun runGpx(entryId: Long, observedAt: Long): String {
        val run = coreJson.encodeToString(RunRef.serializer(), RunRef(entryId, observedAt))
        val gpx = query { bridge.exportGpx(run.encodeToByteArray()) } ?: return ""
        if (isCoreErrorEnvelope(gpx)) {
            PlatformLog.w("Core", "GPX export failed: $gpx")
            return ""
        }
        return gpx
    }

    /** The route fixes of ONE logged run (the fixes its GPX export writes), one list
     *  per recording segment, by the same (entry_id, observed_at) identity as
     *  delete/amend: empty for a hand-entered run, an unknown id, or a core that
     *  can't answer. Off the main thread. */
    fun runTrack(entryId: Long, observedAt: Long): List<List<GpsPoint>> {
        val run = coreJson.encodeToString(RunRef.serializer(), RunRef(entryId, observedAt))
        return decodeQuery("run track", RunTrack.serializer()) { bridge.runTrack(run.encodeToByteArray()) }
            ?.segments.orEmpty()
    }

    /** The core's compaction verdict on the event-log [lines] (log.rs), or null when
     *  the core can't answer: the caller then keeps every line. */
    override fun compaction(lines: List<String>): Compaction? {
        val json = query { bridge.compactLog(lines.toTypedArray()) } ?: return null
        if (isCoreErrorEnvelope(json)) {
            PlatformLog.w("Core", "compaction failed, keeping every line: $json")
            return null
        }
        return try {
            coreJson.decodeFromString(Compaction.serializer(), json)
        } catch (e: SerializationException) {
            PlatformLog.w("Core", "compaction verdict unreadable, keeping every line", e)
            null
        }
    }

    /** A core call outside the event path, as text: null when the library didn't
     *  load, the core is [unavailable], the answer couldn't be allocated, or the
     *  call hit a missing native symbol. */
    private inline fun query(call: () -> ByteArray?): String? {
        if (loadError != null || unavailable) return null
        return try {
            call()?.decodeToString()
        } catch (e: Throwable) {
            if (!e.isLinkageError()) throw e
            null
        }
    }

    // Append-only, newline-delimited event log. The crux core holds all state in
    // memory only, so we persist the event *stream* (not the derived model) and
    // rebuild by replaying it. Each line is one event's exact serde wire JSON.
    private var log: AppPath? = null

    // The native core holds all state in memory for the whole process lifetime,
    // so replaying the log more than once (e.g. on Activity recreation across a
    // config change) would double every logged lift/run.
    private var restored = false

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
        if (unavailable) return false
        restoreFrom(ctx.eventLogPath())
    }

    /** [restore] from the event log at [file]. */
    fun restoreFrom(file: AppPath): Boolean = synchronized(this) {
        if (unavailable) return false
        // Already replayed this process (Activity recreation): the only way this
        // is still a fresh install is if nothing was ever persisted. [log] stays as
        // it is, so a recreation can't re-attach a log a data import detached.
        if (restored) return !AppFiles.exists(file)
        // Marked BEFORE replaying: the native core keeps whatever a partial replay
        // applied, so a second attempt in this process would double those events.
        restored = true
        log = file
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
        // A failed torn-tail repair (often disk full, the same cause as the tear)
        // leaves the durable tail newline-less: the next append must lead with one.
        appendTornGuard = try {
            AppFiles.exists(file) && !AppFiles.endsWithNewline(file)
        } catch (e: Exception) {
            PlatformLog.w("Core", "event log tail unreadable, the next append leads with a newline", e)
            true
        }
        if (replay(loaded.lines)) logDegraded = true
        loaded.freshInstall
    }

    /** Stop persisting: later accepted events stay in memory only. The data import
     *  calls this inside an exclusive turn before replacing the log file, so nothing
     *  can append to the imported log before the process restarts. Returns the
     *  detached file for [reattachLog]. */
    fun detachLog(): AppPath? = synchronized(this) { log.also { log = null } }

    /** Undo [detachLog] after a failed swap: the original log is still in place. */
    fun reattachLog(file: AppPath?) = synchronized(this) {
        log = file
    }

    /**
     * Put this engine back where a new process starts it: the native core emptied
     * ([CoreBridge.reset]) and the replayed log forgotten, so the next [restore]
     * replays the event log file afresh. A reboot into an imported log
     * ([AppRuntime.requestReboot]) calls this before its restore.
     */
    fun resetForReboot() = synchronized(this) { startOver() }

    /** Start over as a new process starts the engine ([AppRuntime.resetForTest]):
     *  [resetForReboot], and the [unavailable] latch forgotten too. */
    internal fun forgetProcess() = synchronized(this) {
        startOver()
        unavailable = false
    }

    private fun startOver() {
        if (loadError == null) bridge.reset()
        forgetLog()
    }

    private fun forgetLog() {
        restored = false
        log = null
        lastGoodView = null
        appendTornGuard = false
        lastSetToday = null
        viewStale = false
        logDegraded = false
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

    private enum class Applied { Rejected, Saved, NotPersisted }

    /** Apply one event: dispatch it to the core, persist it when the core accepted it
     *  and it is [Event.persist], log a rejection; no view read (the caller reads once
     *  per batch). Called only under this object's lock, so the log order equals the
     *  apply order and a replay rebuilds the same model. */
    private fun apply(event: Event): Applied {
        if (unavailable) return Applied.Rejected
        val wire = event.toJson().toString()
        val response = bridge.update(wire.encodeToByteArray())
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
        // A serde-rejected event answers no bytes, a core panic an {"error":...}
        // object, and a null is an answer that couldn't be allocated.
        // None is persisted: the line would just be re-dropped on every replay.
        if (!isAccepted(response)) {
            PlatformLog.e("Core", "event rejected: ${event::class.simpleName}")
            return Applied.Rejected
        }
        if (event is Event.SetToday) lastSetToday = event
        val file = log
        if (!event.persist || file == null) return Applied.Saved
        return try {
            val prefix = if (appendTornGuard) "\n" else ""
            AppFiles.append(file, prefix + wire + "\n")
            appendTornGuard = false
            Applied.Saved
        } catch (e: Exception) {
            // Disk full / IO error: the in-memory core already applied the event, so
            // the UI shows it, but it is gone after the next restart; the caller
            // tells the user.
            appendTornGuard = true
            PlatformLog.w("Core", "failed to persist event to log", e)
            Applied.NotPersisted
        }
    }

    /** Replay [lines] into the native core with [replayLog]'s panic-reset recovery.
     *  Returns true when the core ended up missing any line. Raw [CoreBridge.update],
     *  never [apply], so a reset during replay cannot recurse into [replayLogAfterReset]. */
    private fun replay(lines: List<String>): Boolean {
        val outcome = replayLog(lines, MAX_REPLAY_RESETS) { bridge.update(it.encodeToByteArray()) }
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
        lastSetToday?.let { bridge.update(it.toJson().toString().encodeToByteArray()) }
    }

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
     *  object (leading `{`); a null (an unallocatable answer) is likewise not accepted. */
    private fun isAccepted(response: ByteArray?): Boolean = response.firstNonBlank() == '['.code.toByte()

    /** Current view model without dispatching an event. Shares this object's lock
     *  with [send]/[restore] so a read-back never observes a half-applied event or
     *  races the [lastGoodView] read/write. Every path that cannot produce a fresh
     *  view returns the last good one (never a blank that would wipe a live
     *  DO-NOT-TRAIN hold) and marks the result stale. */
    fun currentView(): ViewModel = synchronized(this) {
        val bytes = bridge.view() ?: run {
            PlatformLog.e("Core", "core view() answered null (allocation failure), keeping last good view")
            return staleView()
        }
        // A view panic leaves the core intact (it doesn't poison the model), so it
        // arrives as the plain error envelope handled below.
        val text = bytes.decodeToString()
        // The FFI panic firewall returns {"error":{...}} on a core panic. With
        // ignoreUnknownKeys that would decode into an all-default ViewModel -
        // train_blocked=false, safety_tier=null, silently blanking safety state.
        if (isCoreErrorEnvelope(text)) {
            val detail = if (AppProcess.debuggable) ": $text" else ""
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
