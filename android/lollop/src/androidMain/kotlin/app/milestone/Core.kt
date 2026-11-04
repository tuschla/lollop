package app.milestone

import android.content.Context
import java.io.File
import java.util.concurrent.Executors
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.serialization.SerializationException
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObjectBuilder
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.add
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray

/** Dispatcher for every view()-producing JNI call ([Core.send] / [Core.sendAll] /
 *  [Core.restore] / [Core.currentView]). SINGLE thread on purpose, not
 *  [kotlinx.coroutines.Dispatchers.IO]: the core's derived-view memos
 *  (RUN_VIEW_CACHE, QC_TRACK_CACHE, the list caches) are thread_local in Rust, so
 *  every IO-pool thread hop lands on cold caches and redoes the full O(history)
 *  rebuild. Top-level rather than a [Core] member: callers evaluate it on the main
 *  thread, and touching [Core] runs its initializer (`System.loadLibrary`), which
 *  must happen on this thread, not the UI thread at cold start. */
internal val CoreJniDispatcher = Executors.newSingleThreadExecutor { r ->
    Thread(r, "core-jni")
}.asCoroutineDispatcher()

/** The production ViewModel decoder (decode only). Top-level so the JVM wire tests
 *  decode with exactly this configuration without loading the native library. */
internal val coreJson = Json { ignoreUnknownKeys = true }

/** How many log lines a replay may skip for resetting the core before it gives up
 *  and leaves the core with what replayed. */
private const val MAX_REPLAY_RESETS = 16

/** A stand-in for the native core's two round trips ([Core.faultForTest]): the raw
 *  `update` response bytes for one event's wire JSON, and the raw `view` bytes. */
internal interface CoreFault {
    fun update(event: ByteArray): ByteArray?
    fun view(): ByteArray?
}

/**
 * JNI bridge to the Rust/crux core (`libshared.so`).
 *
 * The core speaks JSON over the FFI: an [Event] is serialised to the exact
 * serde wire form (externally-tagged enums), pushed via [update]; the resulting
 * [ViewModel] is read back via [view]. All coaching logic, evidence grades,
 * safety tiers, contested markers lives in Rust and is rendered verbatim.
 */
object Core {
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

    /** TEST-ONLY fault injection: when set, every core round trip ([update]/[view])
     *  goes here instead of the JNI library, so a JVM test can boot the app on a core
     *  that answers null (an emulated arm64 library under translation, a JNI
     *  allocation failure) or throws [UnsatisfiedLinkError] (a library that never
     *  loaded). Never set on device. */
    @Volatile internal var faultForTest: CoreFault? = null

    private fun jniUpdate(event: ByteArray): ByteArray? = faultForTest.let { if (it != null) it.update(event) else update(event) }

    private fun jniView(): ByteArray? = faultForTest.let { if (it != null) it.view() else view() }

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
            android.util.Log.e("Core", "native core unavailable at boot", loadError)
            unavailable = true
        }
        return ok
    }

    /** Latch [unavailable] after boot: the core read a view but refused the boot
     *  clock event every working core accepts. */
    @Synchronized
    fun markUnavailable() {
        android.util.Log.e("Core", "native core refused the boot clock event, treating it as unavailable")
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
        val json = trackFigures(track.toJson().toString().toByteArray(Charsets.UTF_8)) ?: return null
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
            compactLog(lines.toTypedArray())
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
            android.util.Log.e("Core", "event log unreadable, starting without it", e)
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
            android.util.Log.e("Core", "event reset the core: ${event::class.simpleName}")
            replayLogAfterReset()
            return Applied.Rejected
        }
        // A serde-rejected event returns empty bytes (ffi out.clear()); a core panic
        // returns an {"error":...} object; a null is a JNI byte-array alloc failure.
        // None is persisted: the line would just be re-dropped on every replay.
        if (!isAccepted(response)) {
            android.util.Log.e("Core", "event rejected: ${event::class.simpleName}")
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
            android.util.Log.w("Core", "failed to persist event to log", e)
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
        android.util.Log.e("Core", "replay skipped ${outcome.skipped.size} line(s) that reset the core ($which)$gaveUp")
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
            android.util.Log.e("Core", "event log unreadable during reset recovery", e)
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
    fun send(event: Event): SendResult {
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
    fun sendAll(events: List<Event>): SendResult {
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
            android.util.Log.e("Core", "core view() returned null (JNI alloc failure), keeping last good view")
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
            android.util.Log.e("Core", "core returned error envelope, keeping last good view$detail")
            return staleView()
        }
        val vm = try {
            coreJson.decodeFromString(ViewModel.serializer(), text)
        } catch (e: SerializationException) {
            android.util.Log.e("Core", "core view failed to decode, keeping last good view", e)
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

/** Whether [text] is the FFI panic-firewall error envelope (`{"error":…}`). A cheap
 *  prefix check avoids tree-parsing a possibly large view blob on every read.
 *  Top-level (not a [Core] member) so it is JVM-unit-testable without loading the
 *  native library. */
internal fun isCoreErrorEnvelope(text: String): Boolean =
    text.trimStart().startsWith("{\"error\"")

/** Whether [text] is specifically a `panic_reset` envelope: the FFI caught a panic
 *  AND swapped in a fresh (empty) core bridge, so the shell must rebuild the
 *  core from the persisted log. The plain `panic` kind (bridge NOT reset) is a
 *  generic error envelope handled by keeping the last good view. */
internal fun isCorePanicReset(text: String): Boolean =
    isCoreErrorEnvelope(text) && text.contains("\"kind\":\"panic_reset\"")

/** Whether a raw JNI `update` response is a `panic_reset` envelope. An accepted
 *  event's response is an effect array (`[`), so only a response opening with `{`
 *  is decoded to text. */
internal fun isPanicResetResponse(response: ByteArray?): Boolean =
    response != null && response.firstNonBlank() == '{'.code.toByte() && isCorePanicReset(String(response, Charsets.UTF_8))

/** The first non-whitespace byte of a JNI response; null for a blank or missing one. */
private fun ByteArray?.firstNonBlank(): Byte? = this?.firstOrNull { !it.toInt().toChar().isWhitespace() }

/** Outcome of [replayLog]: the indices of lines skipped because applying them reset
 *  the core, and whether the final pass replayed every other line ([complete] is
 *  false only when the reset budget ran out, leaving the core with what replayed). */
internal data class ReplayOutcome(val skipped: Set<Int>, val complete: Boolean)

/**
 * Replay [lines] in order through [apply] (one raw JNI `update` each). A line whose
 * response is a `panic_reset` envelope has swapped in an EMPTY core, discarding
 * everything replayed before it (a DO-NOT-TRAIN hold included), so that line is
 * skipped and the replay restarts from the first line, at most [maxResets] times.
 * Top-level so the recovery is JVM-testable without the native library.
 */
internal fun replayLog(lines: List<String>, maxResets: Int, apply: (String) -> ByteArray?): ReplayOutcome {
    val skipped = LinkedHashSet<Int>()
    while (true) {
        val reset = lines.indices.firstOrNull { i -> i !in skipped && isPanicResetResponse(apply(lines[i])) }
            ?: return ReplayOutcome(skipped, complete = true)
        skipped += reset
        if (skipped.size > maxResets) return ReplayOutcome(skipped, complete = false)
    }
}

/**
 * The outcome of [Core.send]: the fresh [view]; whether the core [accepted] the
 * event (false for a serde-rejected event or a panic-firewall envelope); whether
 * an accepted event reached the durable log ([persisted] false: applied in memory
 * but the append failed, so it is gone after the next restart); and whether [view]
 * is [stale] (a stand-in for a view that couldn't be read, or a model missing part
 * of the saved log). The dispatcher surfaces each so a lost entry can't look saved.
 */
data class SendResult(
    val view: ViewModel,
    val accepted: Boolean,
    val persisted: Boolean = true,
    val stale: Boolean = false,
)

// ── Events (encode) ──────────────────────────────────────────────────────────
// Hand-built JSON to match serde's externally-tagged enum representation:
//   unit variant      -> "VariantName"
//   struct variant    -> {"VariantName": { ...fields }}
//   newtype variant   -> {"VariantName": <inner value>}

/** Monotonic per-entry id source. `System.currentTimeMillis()` alone collides
 *  when two entries are created within the same millisecond (a multi-activity
 *  TCX/GPX import builds several runs in a tight loop), which would make a later
 *  DeleteEntry/AmendSet target the wrong row. Threading each id through this
 *  counter guarantees strict uniqueness while staying epoch-millis-shaped. */
object EntryIds {
    private var last = 0L
    @Synchronized
    fun next(): Long {
        last = maxOf(System.currentTimeMillis(), last + 1)
        return last
    }

    /** Never hand out an id at or below [floor]: boot raises it to the largest
     *  restored entry id, so a wall clock moved backwards can't reuse one. */
    @Synchronized
    fun raiseTo(floor: Long) {
        last = maxOf(last, floor)
    }
}

sealed interface Event {
    fun toJson(): JsonElement

    /** Whether this event is written to the durable log when the core accepts it
     *  (all but [SetToday]). */
    val persist: Boolean get() = true

    data class SubmitReadiness(
        val signal: ReadinessSignal,
        val value: Double,
        val observedAt: Long,
        // Pain characterization for a Pain report (ReadinessInput.pain). null keeps
        // the conservative bare-report hard stop; a full detail reaches the graded
        // File-08 pain gate.
        val pain: PainDetail? = null,
        // Effort duration (minutes) for a duration-gated signal (AerobicDecoupling
        // is valid only for efforts >20 min, File 06). null → the core cannot
        // validate it and discards it (ReadinessInput.effort_min serde-defaults None).
        val effortMin: Double? = null,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("SubmitReadiness", buildJsonObject {
                put("signal", signal.name)
                put("value", value)
                put("observed_at", observedAt)
                if (effortMin != null) put("effort_min", effortMin)
                // Always 0: the core derives streaks from check-ins.
                put("streak", 0)
                if (pain != null) put("pain", buildJsonObject {
                    put("kind", pain.kind.name)
                    if (pain.severity != null) put("severity", pain.severity)
                    put("trend", pain.trend.name)
                    put("persists", pain.persists)
                    // Optional free-text/body-part location; omitted when absent
                    // (serde default None on the Rust side).
                    if (pain.location != null) put("location", pain.location)
                })
            })
        }
    }

    data object ClearReadiness : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearReadiness")
    }

    /**
     * Undo one report: the core removes its most recent input carrying [signal]
     * (for Pain, the worst active report). Dated with the removal time, so the
     * input stays in force for the past days it shaped and is gone from today on.
     */
    data class RemoveReadiness(
        val signal: ReadinessSignal,
        val observedAt: Long = AppClock.nowSec(),
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("RemoveReadiness", buildJsonObject {
                put("signal", signal.name)
                put("observed_at", observedAt)
            })
        }
    }

    /**
     * One morning check-in: raw HUMAN observations only. The core
     * normalizes the retained history into the z-scores/deltas the autoreg rules
     * consume; the user never enters a z-score. Every scored field is optional;
     * only the answered items ride the wire (serde default None on the Rust side),
     * so a check-in written by any app version replays. Mirrors schema.rs::CheckinInput.
     */
    data class SubmitCheckin(
        val observedAt: Long,
        val sleepQuality: Int? = null,  // 1–5 (5 = slept great)
        val soreness: Int? = null,      // 1–5 (5 = very sore)
        val mood: Int? = null,          // 1–5 (5 = great mood/motivation); higher = better
        val fatigue: Int? = null,       // 1–5 (5 = utterly wiped); higher = worse
        val stress: Int? = null,        // 1–5 (5 = very stressed); higher = worse
        val restingHrBpm: Double? = null,
        val hrvRmssdMs: Double? = null,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("SubmitCheckin", buildJsonObject {
                put("observed_at", observedAt)
                if (sleepQuality != null) put("sleep_quality", sleepQuality)
                if (soreness != null) put("soreness", soreness)
                if (mood != null) put("mood", mood)
                if (fatigue != null) put("fatigue", fatigue)
                if (stress != null) put("stress", stress)
                if (restingHrBpm != null) put("resting_hr_bpm", restingHrBpm)
                if (hrvRmssdMs != null) put("hrv_rmssd_ms", hrvRmssdMs)
            })
        }
    }

    /** Drop the whole check-in history (part of "Clear all data"). */
    data object ClearCheckins : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearCheckins")
    }

    data class SetProfile(
        val progressionCadence: ProgressionCadence,
        val liftGoal: LiftGoal,
        val goalDistance: GoalDistance,
        val concurrentGoal: ConcurrentGoal,
        val weeklySets: Int,
        val runningDaysPerWeek: Int,
        val runningKmPerWeek: Double,
        val advanced: Boolean,
        // False = the athlete never gave a weekly distance: the core forces
        // runningKmPerWeek to 0.0 and plans from logged runs or its KB floors. A
        // real "I don't run yet" answer is 0.0 with this true.
        val runningKmPerWeekSet: Boolean = true,
        // Every field below mirrors app::Profile and is sent on EVERY SetProfile
        // (last-write-wins, so an omitted field would reset to the core default).
        // Optional fields are emitted only when set (serde defaults on the core),
        // so an unanswered profile keeps the base eight-field wire.
        val female: Boolean? = null,
        val bodyweightKg: Double? = null,
        val ageYears: Double? = null,
        val restingHrBpm: Double? = null,
        val measuredHrMax: Double? = null,
        // Display weights in pounds; display-only.
        val weightLb: Boolean = false,
        // Stage-0 health screen (schema.rs HealthScreen): the pediatric/PAR-Q+/
        // pregnancy/injury/RED-S deferral gates.
        val health: HealthScreen = HealthScreen(),
        val resistanceModality: ResistanceModality = ResistanceModality.Weights,
        //   high_load_block  — arms autoreg-029's parasympathetic-saturation guard
        //   environment       — File-08 environment modifiers (heat/altitude/cold)
        //   env_temp_c/_m     — running-041 pace-correction triggers
        //   weeks_off         — REENTRY-001 ramp + hypertrophy-045 reduced MEV note
        val highLoadBlock: Boolean = false,
        val environment: Environment? = null,
        val envTempC: Double? = null,
        val envAltitudeM: Double? = null,
        val weeksOff: Double? = null,
        //   lift_days_per_week        — 2..=5 lifting sessions the athlete can do
        //   training_weekdays         — bitmask bit0=Mon..bit6=Sun; null = any day
        //   goal_race_epoch_day       — race date (taper/periodisation target)
        //   recent_race               — a recent race/time trial (pace anchor)
        //   stated_longest_recent_km  — longest run in the last 30 days (spike cap)
        //   layoff_returned_epoch_day — the day training resumed after weeks_off
        //   meet_epoch_day            — strength test / meet date (taper/peak)
        //   calis_equipment           — bar / dip bars / rings on hand
        //   specialization_muscle     — hypertrophy priority muscle key
        //   runner_strength           — a pure runner opted into 2 strength sessions
        val liftDaysPerWeek: Int? = null,
        val trainingWeekdays: Int? = null,
        val goalRaceEpochDay: Long? = null,
        val recentRace: RecentRace? = null,
        val statedLongestRecentKm: Double? = null,
        val layoffReturnedEpochDay: Long? = null,
        val meetEpochDay: Long? = null,
        val calisEquipment: CalisEquipment? = null,
        val specializationMuscle: String? = null,
        val runnerStrength: Boolean = false,
        // The day this profile takes effect (app::Profile effective_from_epoch_day):
        // it supersedes every history entry from that day on, and a past day is
        // prescribed from the profile in force then. Production sites pass the
        // core's today; null replaces the whole history.
        val effectiveFromEpochDay: Long? = null,
        // Birthday, local epoch day: the core derives the age and the under-18 gate.
        val birthEpochDay: Long? = null,
        // When the edit was made (unix seconds): on its effective day the edit is in
        // force only from this instant, so an evening edit never re-converts a run
        // logged that morning. Stamped by [effectiveFrom] for a dated edit.
        val editedAtSec: Long? = null,
        // java.io.Serializable: the Profile editor and guided setup keep the event
        // itself as their rememberSaveable draft.
    ) : Event, java.io.Serializable {
        override fun toJson() = buildJsonObject {
            put("SetProfile", buildJsonObject {
                put("progression_cadence", progressionCadence.name)
                put("lift_goal", liftGoal.name)
                put("goal_distance", goalDistance.name)
                put("concurrent_goal", concurrentGoal.name)
                put("weekly_sets", weeklySets)
                put("running_days_per_week", runningDaysPerWeek)
                put("running_km_per_week", runningKmPerWeek)
                put("advanced", advanced)
                if (!runningKmPerWeekSet) put("running_km_per_week_set", false)
                if (female != null) put("female", female)
                if (bodyweightKg != null) put("bodyweight_kg", bodyweightKg)
                if (ageYears != null) put("age_years", ageYears)
                if (restingHrBpm != null) put("resting_hr_bpm", restingHrBpm)
                if (measuredHrMax != null) put("measured_hr_max", measuredHrMax)
                if (weightLb) put("weight_lb", true)
                if (resistanceModality != ResistanceModality.Weights) {
                    put("resistance_modality", resistanceModality.name)
                }
                // `environment` is the bare Rust variant name.
                if (highLoadBlock) put("high_load_block", true)
                if (environment != null) put("environment", environment.name)
                if (envTempC != null) put("env_temp_c", envTempC)
                if (envAltitudeM != null) put("env_altitude_m", envAltitudeM)
                if (weeksOff != null) put("weeks_off", weeksOff)
                if (liftDaysPerWeek != null) put("lift_days_per_week", liftDaysPerWeek)
                if (trainingWeekdays != null) put("training_weekdays", trainingWeekdays)
                if (goalRaceEpochDay != null) put("goal_race_epoch_day", goalRaceEpochDay)
                if (recentRace != null) put("recent_race", buildJsonObject {
                    put("distance_km", recentRace.distance_km)
                    put("time_sec", recentRace.time_sec)
                    if (recentRace.epoch_day != null) put("epoch_day", recentRace.epoch_day)
                })
                if (statedLongestRecentKm != null) put("stated_longest_recent_km", statedLongestRecentKm)
                if (layoffReturnedEpochDay != null) put("layoff_returned_epoch_day", layoffReturnedEpochDay)
                if (meetEpochDay != null) put("meet_epoch_day", meetEpochDay)
                if (calisEquipment != null) put("calis_equipment", buildJsonObject {
                    put("pull_up_bar", calisEquipment.pull_up_bar)
                    put("dip_bars", calisEquipment.dip_bars)
                    put("rings", calisEquipment.rings)
                })
                if (specializationMuscle != null) put("specialization_muscle", specializationMuscle)
                if (runnerStrength) put("runner_strength", true)
                if (effectiveFromEpochDay != null) {
                    put("effective_from_epoch_day", effectiveFromEpochDay)
                }
                if (birthEpochDay != null) put("birth_epoch_day", birthEpochDay)
                if (editedAtSec != null) put("edited_at", editedAtSec)
                // Health screen: only when a gate flag is raised.
                if (health.anyRaised()) put("health", buildJsonObject {
                    // youth is core-derived from age; the shell never asserts it,
                    // so re-emitting a profile can never pin the pediatric gate.
                    put("youth", false)
                    put("parq_positive", health.parq_positive)
                    put("medically_cleared", health.medically_cleared)
                    put("pregnant", health.pregnant)
                    put("pregnancy_warning_sign", health.pregnancy_warning_sign)
                    put("injury_or_rehab", health.injury_or_rehab)
                    put("reds_signal", health.reds_signal)
                    put("symptomatic", health.symptomatic)
                    put("acute_illness", health.acute_illness)
                    put("parq_yes", health.parq_yes)
                    put("cleared_parq_mask", health.cleared_parq_mask)
                })
            })
        }

        /** Holds [from] and [SEED] (ProfileEditor.kt). */
        companion object
    }

    data object ClearProfile : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearProfile")
    }

    /**
     * Place a calisthenics ladder at a rung (the setup "Where are you now?"
     * screen). [rung] is the 0-based index on that ladder; an out-of-range index
     * is a core no-op. Only answered ladders are sent: an unanswered one starts at
     * its easiest rung.
     */
    data class SetCalisthenicsRung(
        val ladder: CalisLadder,
        val rung: Int,
        val observedAt: Long,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("SetCalisthenicsRung", buildJsonObject {
                put("ladder", ladder.name)
                put("rung", rung)
                put("observed_at", observedAt)
            })
        }
    }

    data class LogSet(
        val exercise: String,
        val weightKg: Double,
        val reps: Int,
        val rpe: Double,
        // Log time, unix seconds. Defaulted to now at construction so the history
        // card can be dated; baked into the persisted line, so replay keeps the
        // original stamp (the core never re-stamps on replay).
        val observedAt: Long = AppClock.nowSec(),
        // Stable per-entry id: epoch-millis at log, so it survives
        // edits and never collides with a backdated observed_at. Sent back in
        // AmendSet/DeleteEntry to target this exact set.
        val entryId: Long = EntryIds.next(),
        // Timed-hold seconds for a static/hold set (calisthenics lever/plank/
        // dead-hang rung); null for a reps×load set. Omitted from the wire when
        // null so a normal set matches serde's `#[serde(default)]` shape.
        val holdSec: Int? = null,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("LogSet", buildJsonObject {
                put("exercise", exercise)
                put("weight_kg", weightKg)
                put("reps", reps)
                put("rpe", rpe)
                put("observed_at", observedAt)
                put("entry_id", entryId)
                if (holdSec != null) put("hold_sec", holdSec)
            })
        }
    }

    data object ClearSets : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearSets")
    }

    /**
     * Mark (or reopen) an exercise as ended for a session day. The
     * user tapped "End exercise" on a plan item to finish it before every
     * prescribed set is logged (an RPE-stop, a dropped last set); `ended=false`
     * (the "Reopen" action) undoes it. Last-write-wins per
     * (same_movement-normalized exercise, epoch_day); a later ClearSets drops
     * every marker. Persisted so the day stays settled across relaunches.
     */
    data class SetExerciseEnded(
        val exercise: String,
        val epochDay: Long,
        val ended: Boolean,
        val observedAt: Long = AppClock.nowSec(),
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("SetExerciseEnded", buildJsonObject {
                put("exercise", exercise)
                put("epoch_day", epochDay)
                put("ended", ended)
                put("observed_at", observedAt)
            })
        }
    }

    data class LogRun(
        val distanceKm: Double,
        val durationMin: Double,
        val hrPctMax: Double,
        val observedAt: Long = AppClock.nowSec(),
        val entryId: Long = EntryIds.next(),
        // User-declared run-intent label; null = untagged. Omitted from the
        // wire when null so the event matches serde's `#[serde(default)]` shape.
        val workoutType: WorkoutType? = null,
        // Whole-session effort, CR-10 (0–10); null = unrated, omitted from wire.
        val sessionRpe: Int? = null,
        // Average heart rate in bpm; the core converts it to %HRmax from the
        // profile's measured HRmax (else the age estimate). Null = omitted.
        val avgHrBpm: Double? = null,
        // The device's UTC offset (seconds east) when the run was logged: the
        // core reads the run's local day with it, so a later time-zone change
        // never re-converts its %HRmax. Production senders pass utcOffsetSec();
        // Undo re-sends the row's echoed one. Null (legacy) = omitted.
        val utcOffsetSec: Int? = null,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("LogRun", buildJsonObject {
                putManualRun(distanceKm, durationMin, hrPctMax, observedAt, workoutType, sessionRpe, avgHrBpm)
                put("entry_id", entryId)
                if (utcOffsetSec != null) put("utc_offset_sec", utcOffsetSec)
            })
        }
    }

    data class LogRunTrack(
        val points: List<GpsPoint>,
        val hrPctMax: Double,
        // The session's logged-at stamp for history display; distinct from each
        // GPS fix's own per-point `observed_at`.
        val observedAt: Long = AppClock.nowSec(),
        val entryId: Long = EntryIds.next(),
        // User-declared run-intent label; null = untagged, omitted from wire.
        val workoutType: WorkoutType? = null,
        // Indices into [points] that begin a new recording segment: the pause +
        // relocation boundaries. The core skips each pause-bridge leg
        // (no distance, no time) and breaks the GPX <trkseg> there, keeping the
        // TRUE coordinates. Empty (the common, un-paused run) is OMITTED from the
        // wire so it matches serde's `#[serde(default)]` shape; old-shape parity.
        val segmentStarts: List<Int> = emptyList(),
        // Whole-session effort, CR-10 (0–10); null = unrated, omitted from wire.
        val sessionRpe: Int? = null,
        // Average heart rate in bpm (see [LogRun.avgHrBpm]); null = omitted.
        val avgHrBpm: Double? = null,
        // The device's UTC offset when the run was logged (see [LogRun.utcOffsetSec]).
        val utcOffsetSec: Int? = null,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("LogRunTrack", buildJsonObject {
                putJsonArray("points") {
                    points.forEach { p ->
                        add(buildJsonObject {
                            put("lat", p.lat)
                            put("lon", p.lon)
                            put("observed_at", p.observedAt)
                            put("accuracy_m", p.accuracyM)
                        })
                    }
                }
                put("hr_pct_max", hrPctMax)
                put("longest_recent_km", 0.0)
                put("observed_at", observedAt)
                put("entry_id", entryId)
                if (workoutType != null) put("workout_type", workoutType.name)
                if (segmentStarts.isNotEmpty()) {
                    putJsonArray("segment_starts") { segmentStarts.forEach { add(it) } }
                }
                if (sessionRpe != null) put("session_rpe", sessionRpe)
                if (avgHrBpm != null) put("avg_hr_bpm", avgHrBpm)
                if (utcOffsetSec != null) put("utc_offset_sec", utcOffsetSec)
            })
        }
    }

    data object ClearRuns : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearRuns")
    }

    /**
     * Other moderate-or-harder activity minutes (sport, cycling, PE, …) for the
     * under-18 weekly activity card (youth-pa-001). The core ignores a row whose
     * minutes are not finite or outside (0, 600].
     */
    data class LogActivity(
        val kind: ActivityKind,
        val minutes: Double,
        val vigorous: Boolean = false,
        val strengthening: Boolean = false,
        val observedAt: Long = AppClock.nowSec(),
        val entryId: Long = EntryIds.next(),
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("LogActivity", buildJsonObject {
                put("kind", kind.name)
                put("minutes", minutes)
                put("vigorous", vigorous)
                put("strengthening", strengthening)
                put("observed_at", observedAt)
                put("entry_id", entryId)
            })
        }
    }

    /** Drop every logged activity (part of "Clear all data"). */
    data object ClearActivities : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearActivities")
    }

    /** Which logged family a [DeleteEntry] targets (mirrors app/mod.rs EntryKind). */
    enum class EntryKind { Set, Run, Activity }

    /**
     * Delete one logged set, run or activity. The core removes the newest
     * entry whose `entry_id` matches, or, for a legacy row with no id, whose
     * `observed_at` matches [observedAtFallback]. A no-op when nothing matches.
     */
    data class DeleteEntry(
        val kind: EntryKind,
        val entryId: Long,
        val observedAtFallback: Long = 0,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("DeleteEntry", buildJsonObject {
                put("kind", kind.name)
                put("entry_id", entryId)
                put("observed_at_fallback", observedAtFallback)
            })
        }
    }

    /** Edit one logged set's fields in place. The core deletes the
     *  matched set and re-pushes it carrying the SAME [entryId]. */
    data class AmendSet(
        val entryId: Long,
        val exercise: String,
        val weightKg: Double,
        val reps: Int,
        val rpe: Double,
        val observedAt: Long,
        // The row's ORIGINAL observed_at (before any date change), so a legacy
        // (entry_id == 0) row is matched and REPLACED rather than duplicated.
        val observedAtFallback: Long = 0,
        // Timed-hold seconds (see LogSet.holdSec); null for a reps×load set.
        val holdSec: Int? = null,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("AmendSet", buildJsonObject {
                put("entry_id", entryId)
                put("exercise", exercise)
                put("weight_kg", weightKg)
                put("reps", reps)
                put("rpe", rpe)
                put("observed_at", observedAt)
                put("observed_at_fallback", observedAtFallback)
                if (holdSec != null) put("hold_sec", holdSec)
            })
        }
    }

    /** Edit one hand-entered run's fields. GPS-tracked runs are delete-only for
     *  field edits, so amending one replaces it with a manual run; the sole
     *  exception is [TrimRunTrack], which cuts a GPS run's head/tail in place. */
    data class AmendRun(
        val entryId: Long,
        val distanceKm: Double,
        val durationMin: Double,
        val hrPctMax: Double,
        val observedAt: Long,
        // The row's ORIGINAL observed_at (before any date change), so a legacy
        // (entry_id == 0) row is matched and REPLACED rather than duplicated.
        val observedAtFallback: Long = 0,
        // User-declared run-intent label; null = untagged, omitted from wire.
        val workoutType: WorkoutType? = null,
        // Whole-session effort, CR-10 (0–10); null = unrated, omitted from wire.
        val sessionRpe: Int? = null,
        // Average heart rate in bpm (see [LogRun.avgHrBpm]); null = omitted.
        val avgHrBpm: Double? = null,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("AmendRun", buildJsonObject {
                put("entry_id", entryId)
                putManualRun(distanceKm, durationMin, hrPctMax, observedAt, workoutType, sessionRpe, avgHrBpm)
                put("observed_at_fallback", observedAtFallback)
            })
        }
    }

    /** Trim a saved GPS run's recorded head/tail: keep only the fixes whose
     *  per-point observed_at lies in [keepFromSec, keepUntilSec] (inclusive,
     *  absolute unix seconds). The run's identity (entry_id, observed_at) is
     *  UNCHANGED and its track is edited in place; every derived figure
     *  re-derives in the core. [observedAtFallback] is the run's observed_at, so
     *  a legacy (entry_id == 0) run is located by timestamp. Matches like
     *  DeleteEntry/AmendRun. The one field-edit allowed on a GPS run. */
    data class TrimRunTrack(
        val entryId: Long,
        val observedAtFallback: Long,
        val keepFromSec: Long,
        val keepUntilSec: Long,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("TrimRunTrack", buildJsonObject {
                put("entry_id", entryId)
                put("observed_at_fallback", observedAtFallback)
                put("keep_from_sec", keepFromSec)
                put("keep_until_sec", keepUntilSec)
            })
        }
    }

    /** Rate or tag one logged run IN PLACE (GPS runs included): its whole-session
     *  CR-10 effort and its workout tag. BOTH fields are replaced on every send,
     *  so a null clears that field: callers pass the current value of the one they
     *  aren't changing. [observedAtFallback] locates a legacy (entry_id 0) run. */
    data class AmendRunMeta(
        val entryId: Long,
        val observedAtFallback: Long,
        val sessionRpe: Int?,
        val workoutType: WorkoutType?,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("AmendRunMeta", buildJsonObject {
                put("entry_id", entryId)
                put("observed_at_fallback", observedAtFallback)
                put("session_rpe", sessionRpe)
                put("workout_type", workoutType?.name)
            })
        }
    }

    data class SubmitReview(
        val bonePainRedFlag: Boolean = false,
        val compulsiveFlag: Boolean = false,
        val overtrainingSignalCount: Int = 0,
        val decoupling: Decouple? = null,
        val easyFracTimeAboveVt1: Double? = null,
        val positiveSplitPct: Double? = null,
        val rpeLoadGapSessions: Int? = null,
        val weeklyVelocityDropMs: Double? = null,
        val failedKeySessions: Int? = null,
        val badDay: Boolean = false,
        // How long the overtraining signs have co-occurred, in weeks (<1 / 1–2 / >2
        // answered as 0.5 / 1.5 / 3.0). Null = not asked.
        val overtrainingSignalWeeks: Double? = null,
        // Which of the four listed overtraining signs were ticked; when present
        // the core counts the ticked signs instead of [overtrainingSignalCount].
        val overtrainingSigns: OvertrainingSigns? = null,
        // Weekly RED-S screen answered yes (REDS-SCREEN-001 referral).
        val redsSignalReported: Boolean = false,
        // When the review was submitted, unix seconds (backdatable). Baked into
        // the persisted line so replay keeps the original stamp.
        val observedAt: Long = AppClock.nowSec(),
    ) : Event {
        /** The SessionReview payload shared by SubmitReview and SubmitWeeklyCheckin. */
        fun payload() = buildJsonObject {
            put("bone_pain_red_flag", bonePainRedFlag)
            put("compulsive_flag", compulsiveFlag)
            put("overtraining_signal_count", overtrainingSignalCount)
            if (overtrainingSignalWeeks != null)
                put("overtraining_signal_weeks", overtrainingSignalWeeks)
            if (overtrainingSigns != null) put("overtraining_signs", buildJsonObject {
                put("worse_performance", overtrainingSigns.worsePerformance)
                put("low_mood", overtrainingSigns.lowMood)
                put("higher_resting_hr", overtrainingSigns.higherRestingHr)
                put("worse_sleep", overtrainingSigns.worseSleep)
            })
            if (redsSignalReported) put("reds_signal_reported", true)
            if (decoupling != null) put("decoupling", buildJsonObject {
                put("drift_pct", decoupling.driftPct)
                put("cool_steady_context", decoupling.coolSteadyContext)
            })
            if (easyFracTimeAboveVt1 != null)
                put("easy_frac_time_above_vt1", easyFracTimeAboveVt1)
            if (positiveSplitPct != null)
                put("positive_split_pct", positiveSplitPct)
            if (rpeLoadGapSessions != null)
                put("rpe_load_gap_sessions", rpeLoadGapSessions)
            if (weeklyVelocityDropMs != null)
                put("weekly_velocity_drop_m_s", weeklyVelocityDropMs)
            if (failedKeySessions != null)
                put("failed_key_sessions", failedKeySessions)
            put("bad_day", badDay)
            put("observed_at", observedAt)
        }

        override fun toJson() = buildJsonObject { put("SubmitReview", payload()) }
    }

    /** The weekly check-in (week-level counts + clinical screens): the core reads
     *  only the weekly part of [review] and keeps every one as history. */
    data class SubmitWeeklyCheckin(val review: SubmitReview) : Event {
        override fun toJson() = buildJsonObject { put("SubmitWeeklyCheckin", review.payload()) }
    }

    data object ClearWeeklyCheckin : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearWeeklyCheckin")
    }

    data object ClearReview : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearReview")
    }

    data class PredictRace(
        val recentDistanceM: Double,
        val recentTimeSec: Double,
        val goalDistanceM: Double,
        val weeklyKm: Double,
        // Weeks since the input race (running-041 freshness). null keeps the wire
        // pre-freshness-identical; a value lets the core flag a stale result.
        val weeksSinceRace: Int? = null,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("PredictRace", buildJsonObject {
                put("recent_distance_m", recentDistanceM)
                put("recent_time_sec", recentTimeSec)
                put("goal_distance_m", goalDistanceM)
                put("weekly_km", weeklyKm)
                if (weeksSinceRace != null) put("weeks_since_race", weeksSinceRace)
            })
        }
    }

    data object ClearRacePrediction : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearRacePrediction")
    }

    data class PlanHypertrophyMeso(
        val muscle: String,
        val weeks: Int,
        // hypertrophy-008 next-meso decision inputs (mirror the core's
        // PlanHypertrophyMeso event): the muscle is not growing on the current
        // volume, and recovery from it is easy. Both #[serde(default)] on the
        // core, so each is emitted only when true and a plain plan keeps the
        // same two-field wire.
        val notGrowing: Boolean = false,
        val recoveringEasily: Boolean = false,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("PlanHypertrophyMeso", buildJsonObject {
                put("muscle", muscle)
                put("weeks", weeks)
                if (notGrowing) put("not_growing", true)
                if (recoveringEasily) put("recovering_easily", true)
            })
        }
    }

    data object ClearHypertrophyPlan : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearHypertrophyPlan")
    }

    data class ComputeProtein(
        val bodyweightKg: Double,
        val masters: Boolean,
        val deficit: Boolean,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("ComputeProtein", buildJsonObject {
                put("bodyweight_kg", bodyweightKg)
                put("masters", masters)
                put("deficit", deficit)
            })
        }
    }

    data object ClearProtein : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearProtein")
    }

    data class ComputeHrZones(
        val ageYears: Double,
        // Resting HR (bpm) so the core can run Karvonen heart-rate-reserve zones
        // (running-005) instead of plain %HRmax. null keeps the age-only wire.
        val restingHrBpm: Double? = null,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("ComputeHrZones", buildJsonObject {
                put("age_years", ageYears)
                if (restingHrBpm != null) put("resting_hr_bpm", restingHrBpm)
            })
        }
    }

    data object ClearHrZones : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearHrZones")
    }

    /** Accept a synthesized plan: "Plan my training". */
    data class GeneratePlan(val startEpochDay: Long) : Event {
        override fun toJson(): JsonElement = buildJsonObject {
            put("GeneratePlan", buildJsonObject { put("start_epoch_day", startEpochDay) })
        }
    }

    /** Drop the plan (Coach returns to no-plan state). */
    data object ClearPlan : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearPlan")
    }

    /** Move a planned session to another day of its week (one of its
     *  `move_targets`). Kept with the plan request; ClearPlan sweeps it. */
    data class MoveSession(val fromEpochDay: Long, val toEpochDay: Long) : Event {
        override fun toJson(): JsonElement = buildJsonObject {
            put("MoveSession", buildJsonObject {
                put("from_epoch_day", fromEpochDay)
                put("to_epoch_day", toEpochDay)
            })
        }
    }

    /** "I'm pain-free now" / "I've recovered": retracts every active report of
     *  [signal] (history kept) and the coach restarts with an easier session. */
    data class ResolveHold(
        val signal: ReadinessSignal,
        val observedAt: Long = AppClock.nowSec(),
    ) : Event {
        override fun toJson(): JsonElement = buildJsonObject {
            put("ResolveHold", buildJsonObject {
                put("signal", signal.name)
                put("observed_at", observedAt)
            })
        }
    }

    /** "Relearn your HRV normal" (e.g. after a device change): HRV readings before [observedAt]
     *  stop counting toward the baseline, which starts collecting again. */
    data class RestartHrvBaseline(val observedAt: Long = AppClock.nowSec()) : Event {
        override fun toJson(): JsonElement = buildJsonObject {
            put("RestartHrvBaseline", buildJsonObject { put("observed_at", observedAt) })
        }
    }

    /** The shell's clock as event data: today's epoch-day, sent on
     *  foreground so the core dates the week + picks the next session. Also carries
     *  the device's current UTC offset in seconds EAST of UTC so the core can
     *  match a logged session's UTC `observed_at` to the correct LOCAL day. */
    data class SetToday(val epochDay: Long, val utcOffsetSec: Int = 0) : Event {
        // The shell's clock, re-sent every foreground; kept as one last-write-wins
        // line, so it is applied in-memory but never appended to the durable log.
        override val persist: Boolean get() = false
        override fun toJson(): JsonElement = buildJsonObject {
            put("SetToday", buildJsonObject {
                put("epoch_day", epochDay)
                put("utc_offset_sec", utcOffsetSec)
            })
        }
    }
}

/** The fields a hand-entered run's [Event.LogRun] and [Event.AmendRun] share. */
private fun JsonObjectBuilder.putManualRun(
    distanceKm: Double,
    durationMin: Double,
    hrPctMax: Double,
    observedAt: Long,
    workoutType: WorkoutType?,
    sessionRpe: Int?,
    avgHrBpm: Double?,
) {
    put("distance_km", distanceKm)
    put("duration_min", durationMin)
    put("hr_pct_max", hrPctMax)
    // Always 0: the core derives the baseline from logged runs.
    put("longest_recent_km", 0.0)
    put("observed_at", observedAt)
    if (workoutType != null) put("workout_type", workoutType.name)
    if (sessionRpe != null) put("session_rpe", sessionRpe)
    if (avgHrBpm != null) put("avg_hr_bpm", avgHrBpm)
}

/** The four listed overtraining signs on the weekly check-in (model.rs OvertrainingSigns). */
data class OvertrainingSigns(
    val worsePerformance: Boolean = false,
    val lowMood: Boolean = false,
    val higherRestingHr: Boolean = false,
    val worseSleep: Boolean = false,
) {
    val any: Boolean get() = worsePerformance || lowMood || higherRestingHr || worseSleep
}

/** Aerobic-decoupling context for a run review. Mirrors shared/src/app/mod.rs::Decouple. */
data class Decouple(val driftPct: Double, val coolSteadyContext: Boolean)

/** One GPS fix. Mirrors shared/src/running.rs::GpsPoint. */
data class GpsPoint(
    val lat: Double,
    val lon: Double,
    val observedAt: Long,
    val accuracyM: Double,
)
