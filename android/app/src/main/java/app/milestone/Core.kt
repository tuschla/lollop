package app.milestone

import android.content.Context
import java.io.File
import java.util.concurrent.Executors
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.serialization.Serializable
import kotlinx.serialization.SerializationException
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
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

/**
 * JNI bridge to the Rust/crux core (`libshared.so`).
 *
 * The core speaks JSON over the FFI: an [Event] is serialised to the exact
 * serde wire form (externally-tagged enums), pushed via [update]; the resulting
 * [ViewModel] is read back via [view]. All coaching logic, evidence grades,
 * safety tiers, contested markers lives in Rust and is rendered verbatim.
 */
object Core {
    init {
        System.loadLibrary("shared")
    }

    private external fun update(event: ByteArray): ByteArray?
    private external fun view(): ByteArray?

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

    /** Build the GPX 1.1 document for ONE logged run ON DEMAND. The list
     *  payload no longer ships a per-run GPX string (it was a ~0.5 MB blob
     *  re-serialised over JNI + re-parsed on every event); the shell calls this
     *  only for the one run it inspects, matched by the same (entry_id,
     *  observed_at) identity as delete/amend. Returns "" for a hand-entered run,
     *  an unknown id, or a track with no exportable route; null only if the JNI
     *  string couldn't be allocated (the caller treats null like ""). Reads the
     *  registry the last [view] populated, so call it after the run list rendered.
     *  Heavy (builds + returns the whole document): call OFF the main thread. */
    external fun exportGpx(entryId: Long, observedAt: Long): String?

    // Append-only, newline-delimited event log. The crux core holds all state in
    // memory only, so we persist the event *stream* (not the derived model) and
    // rebuild by replaying it. Each line is one event's exact serde wire JSON.
    private var log: File? = null

    // The native core holds all state in memory for the whole process lifetime,
    // so replaying the log more than once (e.g. on Activity recreation across a
    // config change) would double every logged lift/run. Guard so replay happens
    // exactly once per process.
    private var restored = false

    /**
     * Point the core at [ctx]'s event log and replay it to rebuild state. Safe to
     * call on every Activity creation: replay runs only on the first call per
     * process. Returns true only on a genuinely FRESH INSTALL: the log file
     * never existed, so the caller may seed a default profile + onboarding. A
     * log that exists but compacts to zero surviving lines (a returning user who
     * cleared everything) returns false: replaying "nothing left" is their real
     * state and must not be re-seeded over. Read+compact live in [EventLog.load]
     * (pure Kotlin, unit tested); this only replays the survivors.
     *
     * Never throws for a bad log: an unreadable log is logged and leaves the core
     * empty, a line that resets the core is skipped, and either is reported as
     * [SendResult.stale] on every later send.
     */
    @Synchronized
    fun restore(ctx: Context): Boolean {
        val file = File(ctx.filesDir, "event-log.ndjson")
        log = file
        // Already replayed this process (Activity recreation): the only way this
        // is still a fresh install is if nothing was ever persisted.
        if (restored) return !file.exists()
        // Marked BEFORE replaying: the native core keeps whatever a partial replay
        // applied, so a second attempt in this process would double those events.
        restored = true
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

    // The most recent SetToday applied in-memory. SetToday is non-persisted (the
    // shell's clock, re-sent every foreground), so it is NOT in the replayed log; a
    // panic_reset that rebuilds the core from the log alone would lose today's date.
    // Cache it so [replayLogAfterReset] can re-establish it immediately.
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

    /** Apply one event: dispatch it to the core, persist it to the log (only if
     *  the core accepted it AND the event is [Event.persist]), and log a rejection.
     *  Does NOT read the view back — the caller does that once, so a multi-event
     *  batch decodes the view a single time.
     *
     *  Only ever called under the [Core] monitor (via [send]/[sendAll]), which it
     *  shares with [restore] and [currentView]. Every caller routes through
     *  [CoreJniDispatcher] via `EventDispatcher`; the monitor guards
     *  `restore`/`currentView` re-entrancy. Without the lock two concurrent calls
     *  could interleave their JNI `update` → `appendText` steps and persist the
     *  event log in a different order than the in-memory core applied them, or race the
     *  [lastGoodView] cache. The monitor makes each apply → append (and the trailing
     *  read-back) atomic, so the guarantee holds: **the log is written in exactly the
     *  order the core accepted the events, so a later replay reconstructs the
     *  identical model.** */
    private fun apply(event: Event): Applied {
        val wire = event.toJson().toString()
        val response = update(wire.toByteArray(Charsets.UTF_8))
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
        // A non-persisted event (SetToday, the shell's clock) is applied in-memory
        // but never written: it is re-sent every launch.
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
        val outcome = replayLog(lines, MAX_REPLAY_RESETS) { update(it.toByteArray(Charsets.UTF_8)) }
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
        lastSetToday?.let { update(it.toJson().toString().toByteArray(Charsets.UTF_8)) }
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
    private fun isAccepted(response: ByteArray?): Boolean {
        val first = response?.firstOrNull { !it.toInt().toChar().isWhitespace() } ?: return false
        return first == '['.code.toByte()
    }

    /** Current view model without dispatching an event. Shares the [Core] monitor
     *  with [send]/[restore] so a read-back never observes a half-applied event or
     *  races the [lastGoodView] read/write. Every path that cannot produce a fresh
     *  view returns the last good one (never a blank that would wipe a live
     *  DO-NOT-TRAIN hold) and marks the result stale. */
    @Synchronized
    fun currentView(): ViewModel {
        // A null return is the JNI byte-array alloc failure (ffi byte_array_or_null
        // exhausted its retries).
        val bytes = view() ?: run {
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
            val detail = if (BuildConfig.DEBUG) ": $text" else ""
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
internal fun isPanicResetResponse(response: ByteArray?): Boolean {
    val first = response?.firstOrNull { !it.toInt().toChar().isWhitespace() } ?: return false
    return first == '{'.code.toByte() && isCorePanicReset(String(response, Charsets.UTF_8))
}

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

// ── View model (decode) ──────────────────────────────────────────────────────
// Field names + shapes mirror shared/src/app/mod.rs::ViewModel exactly.

@Serializable
data class ViewModel(
    val safety_tier: String? = null,
    val train_blocked: Boolean = false,
    val adjustments: List<AdjustmentView> = emptyList(),
    val review_adjustments: List<AdjustmentView> = emptyList(),
    // A persisted session review exists (app/mod.rs ViewModel.review_present): the
    // shell shows a "Clear review" affordance.
    val review_present: Boolean = false,
    // A weekly check-in is in force (ViewModel.weekly_checkin_present): the shell
    // offers "Clear weekly check-in" (Event.ClearWeeklyCheckin).
    val weekly_checkin_present: Boolean = false,
    // An illness report is active, or a high resting-HR morning asks for the
    // illness check: the morning check-in asks "Feeling ill?" first.
    val illness_check_prompt: Boolean = false,
    // Every calisthenics ladder with its rungs (empty for a Weights profile): the
    // Log chooser lists them grouped by movement.
    val calisthenics_ladders: List<CalisLadderView> = emptyList(),
    val input_count: Int = 0,
    val lifts: List<LiftResultView> = emptyList(),
    val runs: List<RunResultView> = emptyList(),
    val guidance: List<GuidanceView> = emptyList(),
    val feedback: FeedbackView? = null,
    val reference: List<GuidanceView> = emptyList(),
    val profile: ProfileView? = null,
    val race_prediction: RacePredictionView? = null,
    val hypertrophy_plan: List<GuidanceView> = emptyList(),
    val protein_targets: List<GuidanceView> = emptyList(),
    val hr_zones: List<GuidanceView> = emptyList(),
    // CTL/ATL/TSB training-load bookkeeping over the logged run history
    // (app/mod.rs ViewModel.training_load). Null until a dated, HR-carrying run
    // exists. Bookkeeping only — TSB is NOT a validated performance predictor.
    val training_load: TrainingLoadView? = null,
    // Session-RPE load (minutes × whole-session 0–10 rating; LOAD-SRPE-001), the
    // separate sensor-free weekly series for rated runs (app ViewModel.srpe_load).
    // Null until a run is rated: the shell then renders nothing.
    val srpe_load: SrpeLoadView? = null,
    // Running progress: 16 Monday-start weeks of volume, longest run and
    // best efforts (app ViewModel.run_progress). Null until a dated run with a
    // measurable distance exists.
    val run_progress: RunProgressView? = null,
    // Weekly running-volume system rows for the most recent logged week:
    // week-over-week increase, two-week ramp flag, Daniels volume caps,
    // easy-share floor, quality-plan check, hybrid combined-load guard (app/mod.rs
    // ViewModel.weekly_report). Empty until a dated run exists.
    val weekly_report: List<GuidanceView> = emptyList(),
    // Lift-session audit rows for the most recent dated lifting day: Prilepin
    // volume check per exercise + the depth-jump readiness gate when bodyweight
    // and a squat e1RM are known (app/mod.rs ViewModel.lift_audit). Empty otherwise.
    val lift_audit: List<GuidanceView> = emptyList(),
    // Raw calculator inputs echoed back so forms rehydrate after a cold start
    // (see the matching structs above). race_prediction echoes its own inline.
    val hr_zone_input: HrZoneInputView? = null,
    val protein_input: ProteinInputView? = null,
    val hypertrophy_input: HypertrophyInputView? = null,
    // Weekly direct-set counts of the hypertrophy plan's ramp row (app/mod.rs
    // ViewModel.hypertrophy_ramp); empty with no plan, a hold or an unknown muscle.
    val hypertrophy_ramp: List<Int> = emptyList(),
    // feedback-027/028/029 longitudinal trend message, when the review carries a
    // trend direction (app/mod.rs ViewModel.trend). Rendered under the lift review.
    val trend: AdjustmentView? = null,
    // feedback-040 provisional framing: present while fewer than ~14 distinct
    // logged days exist — recommendations are population defaults still
    // converging on the user (app/mod.rs ViewModel.provisional). A safety-relevant
    // honesty disclosure: rendered next to the readiness verdict, not buried.
    val provisional: AdjustmentView? = null,
    // Which autoregulation signal source is active given the submitted readiness
    // data (autoreg-047/048 graceful fallback: HRV rolling, subjective +
    // performance, or performance-only-hold; app/mod.rs ViewModel.autoreg_source).
    // Null until any readiness input exists. Honesty disclosure — rendered next
    // to the readiness verdict.
    val autoreg_source: AdjustmentView? = null,
    // KB-honest per-signal readiness summary (app/mod.rs readiness_summary): the
    // latest state of each observed signal judged by the core's own KB
    // thresholds. Deliberately NO composite 0–100 score exists on the wire.
    val readiness_summary: List<ReadinessSignalView> = emptyList(),
    // Core-owned "today's call": safety hold > adjustment > feedback >
    // all-clear.
    val today_headline: TodayHeadlineView? = null,
    // Static signal→group metadata ("metric" | "red_flag") driving the
    // readiness picker's red-flag fence from the core.
    val signal_groups: List<SignalGroupView> = emptyList(),
    // The most recent morning check-in echoed for rehydration +
    // a "checked in today" cue. Null until a check-in exists.
    val checkin_today: CheckinEchoView? = null,
    // Honest "collecting your baseline" rows for each check-in channel that has
    // data but not yet enough history to emit a z-score/delta. Empty otherwise.
    val baseline_status: List<BaselineStatusView> = emptyList(),
    // Evidence-grade legend exported from core data (File 09) so
    // the "How evidence grading works" sheet renders KB definitions, not
    // hardcoded shell copy.
    val grade_definitions: List<GradeDefView> = emptyList(),
    // Coach-as-planner. The concrete next session (the Coach hero),
    // the current week strip, and the program summary. Null/empty until the user
    // accepts a plan.
    val next_session: SessionPlanView? = null,
    val week_plan: List<SessionPlanView> = emptyList(),
    val program: ProgramSummaryView? = null,
    // Structured HRmax figure (bpm + measured/estimate + Tanaka
    // split) for the last HR-zone query, so the shell reads it instead of
    // regex-scraping the hr_zones rows. Null until a query is made / when the age
    // is out of range.
    val hr_max: HrMaxView? = null,
    // Structured protein g/day figures paralleling protein_targets,
    // so the shell reads them instead of regex-scraping those rows. Empty until a
    // protein query is made.
    val protein_figures: List<ProteinFigureView> = emptyList(),
    // Structured Daniels %HRmax bands of the last HR-zone query, one per band
    // row of hr_zones (app/mod.rs ViewModel.hr_zone_bands), so the zone table
    // reads numbers, never the prose. Empty with no query / a refused age.
    val hr_zone_bands: List<HrZoneBandView> = emptyList(),
    // Structured figures of the hypertrophy plan (app/mod.rs
    // ViewModel.hypertrophy_figures). Null whenever the planner shows no ramp.
    val hypertrophy_figures: HypertrophyFiguresView? = null,
    // Echo of the core's `model.today_epoch_day` (app/mod.rs ViewModel.today_epoch_day):
    // the day SetToday last set. The shell reads this for every plan gate (hero
    // actionability, week-strip today ring, "Today's plan" tile, planPrefill,
    // past-day stamps) instead of the wall clock, so a session that crosses
    // midnight while foregrounded keeps rolling over in lockstep with the core.
    // Null before the first SetToday → the shell falls back to todayEpochDay().
    val today_epoch_day: Long? = null,
    // Today's completed plan session, one line per item (app/mod.rs
    // ViewModel.session_done): the Today "session done" card. Null until today's
    // planned session is complete. The coaching takeaway rides in `feedback`.
    val session_done: SessionDoneView? = null,
    // One row per active pain / illness hold: the banner's "I'm pain-free now" /
    // "I've recovered" action (Event.ResolveHold). Empty when no hold.
    val hold_resolutions: List<HoldResolutionView> = emptyList(),
    // Planned-vs-done for the active plan: this week + the last 8 plan weeks +
    // neutral notes. No streaks, no loss framing. Null without a plan.
    val consistency: ConsistencyView? = null,
    // Why training is held (C2, view_types.rs HoldReason, snake_case): parq |
    // youth | pregnancy | reds | injury_rehab | symptomatic |
    // acute_illness_profile | illness | pain | medical_signal. Set whenever
    // `train_blocked`; the most specific reason wins. Drives the banner's
    // reason line and its one resolve action.
    val hold_reason: String? = null,
    // A morning check-in is dated today (C6). False → the readiness pills come
    // from an earlier check-in and Today invites today's.
    val checked_in_today: Boolean = false,
    // Why no plan can be built right now (view_types.rs PlanWithheld, snake_case):
    // health_hold | nothing_to_plan; null when GeneratePlan can build one. While
    // set, Today offers no "Plan my training" and says why instead.
    val plan_withheld: String? = null,
    // Every logged other-activity row (view_types.rs ViewModel.activities), same
    // order as `lifts`/`runs`.
    val activities: List<ActivityResultView> = emptyList(),
    // The WHO 5–17 weekly activity card (youth-pa-001): set only for an
    // under-18 profile once SetToday has run. Information and tracking only.
    val youth_activity: YouthActivityView? = null,
)

/**
 * The structured HRmax figure the shell used to regex-scrape out of
 * the hr_zones summary rows (incl. the "208 − 0.7 × age" Tanaka split). Field
 * names mirror the Rust HrMaxView (app/mod.rs) serde names exactly. `bpm` is
 * core-rounded.
 * `tanaka_intercept`/`tanaka_slope` are 208/0.7 for the age estimate, 0 when
 * `measured` (a logged maximum, which needs no age).
 */
@Serializable
data class HrMaxView(
    val bpm: Double = 0.0,
    val measured: Boolean = false,
    val age_years: Double = 0.0,
    val tanaka_intercept: Double = 0.0,
    val tanaka_slope: Double = 0.0,
)

/**
 * One structured protein g/day target paralleling a protein_targets
 * row, so the shell reads it instead of regex-scraping the prose. Field names
 * mirror the Rust ProteinFigureView (app/mod.rs) serde names exactly. `kind` is
 * "masters" or "deficit"; g/day figures are
 * core-rounded and 0 when `refused` (a RED-S deficit refusal carries no number).
 * `low/high_g_per_kg` is the graded g/kg band the grams come from; `row` indexes
 * this figure's evidence row in protein_targets.
 */
@Serializable
data class ProteinFigureView(
    val kind: String = "",
    val low_g_per_day: Double = 0.0,
    val high_g_per_day: Double = 0.0,
    val refused: Boolean = false,
    val low_g_per_kg: Double = 0.0,
    val high_g_per_kg: Double = 0.0,
    val row: Int = 0,
)

/**
 * One Daniels training band of the HR-zone table (app/mod.rs HrZoneBandView).
 * `kind` is the band's wire name ("Easy", "Marathon", "Threshold", "Interval",
 * "Repetition"), the key its swatch colour is chosen by.
 * `pct_*` is %HRmax (whole numbers), `bpm_*` the core-rounded absolute range;
 * a `pace_governed` band is run by pace and carries no bpm target. The
 * `karvonen_*` heart-rate-reserve range is 0 unless the resting HR selected it.
 * `row` indexes this band's evidence row in `hr_zones`.
 */
@Serializable
data class HrZoneBandView(
    val kind: String = "",
    val name: String = "",
    val pct_low: Double = 0.0,
    val pct_high: Double = 0.0,
    val bpm_low: Double = 0.0,
    val bpm_high: Double = 0.0,
    val pace_governed: Boolean = false,
    val karvonen_low_bpm: Double = 0.0,
    val karvonen_high_bpm: Double = 0.0,
    val row: Int = 0,
)

/**
 * The hypertrophy plan's numbers (app/mod.rs HypertrophyFiguresView): weekly-set
 * landmarks, the reps-left schedule, peak frequency, rest windows (seconds) and
 * the block shape. `face_rows` indexes the `hypertrophy_plan` rows these figures
 * stand for; every other row is a note.
 */
@Serializable
data class HypertrophyFiguresView(
    val min_sets: Int = 0,
    val productive_low: Int = 0,
    val productive_high: Int = 0,
    val max_sets: Int = 0,
    val rir_by_week: List<Int> = emptyList(),
    val freq_low: Int = 0,
    val freq_high: Int = 0,
    val per_session_low: Int = 0,
    val per_session_high: Int = 0,
    val rest_heavy_low_s: Int = 0,
    val rest_heavy_high_s: Int = 0,
    val rest_isolation_low_s: Int = 0,
    val rest_isolation_high_s: Int = 0,
    val build_weeks_low: Int = 0,
    val build_weeks_high: Int = 0,
    val deload_weeks: Int = 0,
    val face_rows: List<Int> = emptyList(),
)

/**
 * The three-part "why?" disclosure carried by every action-bearing card
 * (app/mod.rs WhyView): basis (what it's based on) →
 * grade_note (why this grade) → improves (what data would sharpen it). An empty
 * block makes the card fall back to the plain evidence restatement.
 */
@Serializable
data class WhyView(
    val basis: String = "",
    val grade_note: String = "",
    val improves: String = "",
)

/** One prescribed exercise, core-flattened (app/mod.rs PrescriptionView): the
 *  concrete do-X contract. `load_kg` is set only when anchored to the
 *  user's logged e1RM; otherwise `intensity_label` carries the RIR/pace target
 *  (no invented load, HARD RULE 1). Carries the full evidence block + why?. */
@Serializable
data class PrescriptionView(
    val summary: String = "",
    val exercise: String = "",
    val sets: Int = 0,
    val reps_low: Int = 0,
    val reps_high: Int = 0,
    val load_kg: Double? = null,
    val intensity_label: String = "",
    val rest_sec: Int = 0,
    // True for a timed-hold calisthenics rung: rep_volume carries "20-45 s hold"
    // and reps_low/high stay 0 (app/mod.rs PrescriptionView.timed_hold). Default false.
    val timed_hold: Boolean = false,
    // Numeric prefill targets mirroring app/mod.rs PrescriptionView: target_rpe maps a
    // prescribed RIR/RPE for the log editor, hold_low/high_sec carry a timed band,
    // added_kg is the calisthenics added load (Some(0.0) = bodyweight).
    val target_rpe: Double? = null,
    val hold_low_sec: Int = 0,
    val hold_high_sec: Int = 0,
    val added_kg: Double? = null,
    val rep_volume: String = "",
    val anchored_on: String = "",
    // `anchored_on` is a last-time line ("Last: 60 kg × 8"): the face shows it
    // as what was lifted last time (view_types.rs PrescriptionView.anchored_on_last).
    val anchored_on_last: Boolean = false,
    val adjusted_note: String = "",
    // The post-illness return check standing in for the whole session
    // (view_types.rs PrescriptionView.return_check): its dose and stop list are
    // the advice, shown once on this item.
    val return_check: Boolean = false,
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
    val why: WhyView = WhyView(),
    // True when this session's day already has the prescribed work logged
    // (app/mod.rs PrescriptionView.done): the week strip marks the item complete and
    // omits its past-day Log action.
    val done: Boolean = false,
    // Per-set completion wire (app/mod.rs PrescriptionView).
    // `sets_done`: qualifying same-movement sets attributed to this item's
    // session day (0 for runs). `logged_sets`: every same-movement set that day,
    // chronological, each flagged `qualifies` (warm-ups / below-target stay
    // listed with qualifies=false). `ended_early`: the user ended this exercise
    // via SetExerciseEnded. `run_logged_km`/`run_logged_min`: for a run item,
    // totals attributed to the session day (0 otherwise).
    val sets_done: Int = 0,
    val logged_sets: List<ItemSetView> = emptyList(),
    val ended_early: Boolean = false,
    val run_logged_km: Double = 0.0,
    val run_logged_min: Double = 0.0,
    // The newest set of this movement from a PRIOR session day (same shape as
    // logged_sets): prefills the weight when load_kg is null and reads "Last time".
    val last_set: ItemSetView? = null,
    // Starting weight (kg) for an effort-prescribed item with no load_kg.
    val suggested_load_kg: Double? = null,
    // Plates per side of a standard bar, in the display unit, largest first.
    val plates_per_side: List<Double> = emptyList(),
    // Short execution cue ("move fast"); already part of `summary`.
    val effort_cue: String = "",
    // Calisthenics-only data (ladder, rung, easier/harder, load_capable,
    // ballistic); null for lifts and runs.
    val calis: CalisItemView? = null,
    // Run items only (view_types.rs PrescriptionView.run_target / run_structure):
    // numeric pace / heart-rate / effort targets and the step structure the GPS
    // tracker walks (warm-up → reps + recovery → cool-down → strides, or run/walk
    // bouts). Both null for lifts/calisthenics; run_structure is also null for a
    // plain continuous run.
    val run_target: RunTargetView? = null,
    val run_structure: RunStructureView? = null,
    // Item completion (C4, view_types.rs ItemStatus, snake_case): open |
    // in_progress | done | done_lighter | skipped | skipped_pain.
    val status: String = "open",
    // Why a safety cap shaped this item, plain language with distances in km like
    // `summary` (view_types.rs PrescriptionView.safety_reason, e.g. "Capped at
    // 12.5 km: at most 10% over your longest recent run"); empty otherwise.
    val safety_reason: String = "",
    // `safety_reason` with its distance in miles; empty when it states none.
    val safety_reason_mi: String = "",
    // Face parts of `summary` (view_types.rs PrescriptionView): the bold name line
    // and the prescription line under it for a lift / calisthenics item (both empty
    // for runs and the return check), the compact row's one line, and a run's
    // volume head before its target ("6 × 800 m", distances in km).
    val face_name: String = "",
    val face_detail: String = "",
    val face_lead: String = "",
    val run_head: String = "",
    // The run head's km as shown when the head is a plain distance ("8 km" → 8.0).
    val run_head_km: Double? = null,
)

/** Numeric execution targets for one run prescription (view_types.rs
 *  RunTargetView). Pace in seconds per km (0 = none), heart rate in bpm (0 =
 *  none), CR-10 effort band (0/0 = none); the labels are display-ready. */
@Serializable
data class RunTargetView(
    val band: String = "",
    val pace_fast_sec_per_km: Int = 0,
    val pace_slow_sec_per_km: Int = 0,
    val pace_label_km: String = "",
    val pace_label_mi: String = "",
    val hr_low_bpm: Int = 0,
    val hr_high_bpm: Int = 0,
    val hr_label: String = "",
    val rpe_low: Int = 0,
    val rpe_high: Int = 0,
    val effort_cue: String = "",
    // `effort_cue` without its leading "effort" word ("3–4 of 10").
    val effort_detail: String = "",
    val vdot: Double = 0.0,
    val hr_max_bpm: Double = 0.0,
    val hr_max_measured: Boolean = false,
    val hr_rest_bpm: Double = 0.0,
    val citation: String = "",
    val grade: String = "",
    val confidence: Float = 0f,
)

/** Numeric structure of a prescribed run (view_types.rs RunStructureView); every
 *  field 0/empty when it does not apply. Drives the tracker's step machine. */
@Serializable
data class RunStructureView(
    val reps: Int = 0,
    val rep_m: Int = 0,
    val rep_sec: Int = 0,
    val recovery_sec: Int = 0,
    val recovery_m: Int = 0,
    val recovery_equal_to_rep: Boolean = false,
    val warmup_min: Int = 0,
    val cooldown_min: Int = 0,
    val strides_reps: Int = 0,
    val strides_sec: Int = 0,
    val strides_recovery_sec: Int = 0,
    val run_walk: List<RunWalkStepView> = emptyList(),
)

/** One run/walk bout of a beginner session: run [run_sec], then walk [walk_sec]. */
@Serializable
data class RunWalkStepView(
    val run_sec: Int = 0,
    val walk_sec: Int = 0,
)

/** One logged set attributed to a prescription item's session day
 *  (app/mod.rs ItemSetView). `summary` is display-ready and unit-aware (the " @ RPE x"
 *  segment is already omitted when rpe == 0). `qualifies` is false for a warm-up
 *  or below-target set, which the shell still lists but marks. */
@Serializable
data class ItemSetView(
    val entry_id: Long = 0,
    val observed_at: Long = 0,
    val weight_kg: Double = 0.0,
    val reps: Int = 0,
    val rpe: Double = 0.0,
    val hold_sec: Int? = null,
    val qualifies: Boolean = false,
    // A counted set below the full prescribed load ("lighter than planned").
    val lighter: Boolean = false,
    // A counted set short of the prescribed reps / hold time ("missed reps").
    val short: Boolean = false,
    val summary: String = "",
)

/** One rung of a calisthenics ladder (view_types.rs CalisRungView). */
@Serializable
data class CalisRungView(
    val rung: Int = 0,
    val name: String = "",
    val target: String = "",
    val available: Boolean = false,
    // A timed-hold rung (its `target` is a hold band).
    val timed_hold: Boolean = false,
)

/** Calisthenics-only data on a plan item (view_types.rs CalisItemView).
 *  `load_capable`: show the added-load field even when added_kg is 0.
 *  `ballistic`: max-intent power item, no RPE hint. */
@Serializable
data class CalisItemView(
    val ladder: CalisLadder = CalisLadder.HorizontalPush,
    val rung: Int = 0,
    val rung_count: Int = 0,
    val easier: CalisRungView? = null,
    val harder: CalisRungView? = null,
    val load_capable: Boolean = false,
    val test_hold_sec: Int = 0,
    val ballistic: Boolean = false,
    val needs_effort_rating: Boolean = false,
)

/** One calisthenics ladder for pickers and the Log chooser (view_types.rs
 *  CalisLadderView). `pattern` is the movement name, e.g. "Vertical pull". */
@Serializable
data class CalisLadderView(
    val ladder: CalisLadder = CalisLadder.HorizontalPush,
    val pattern: String = "",
    val skill: Boolean = false,
    val power: Boolean = false,
    val current_rung: Int? = null,
    val rungs: List<CalisRungView> = emptyList(),
    // The rung the athlete placed themselves on, before the equipment check
    // (current_rung is the rung the kit allows; null when none fits).
    val placed_rung: Int? = null,
    // Plain-language note when the kit limits this ladder ("Full pull-up needs a
    // pull-up bar or rings…"); empty otherwise.
    val equipment_note: String = "",
)

/** One planned day in the week (app/mod.rs SessionPlanView). Rendered
 *  strictly downstream of the safety gates: a hold sets status "blocked" and
 *  empties items. status ∈ next|planned|done|done_lighter|missed|partial|
 *  adjusted|blocked|rest|before_plan ("partial": a past day with some work
 *  logged, under the done-lighter share; neither done nor missed). */
@Serializable
data class SessionPlanView(
    val epoch_day: Long = 0,
    val title: String = "",
    val session_type: String = "",
    val status: String = "",
    val items: List<PrescriptionView> = emptyList(),
    val adjustment: AdjustmentView? = null,
    // Days this session can be moved to (Event.MoveSession{from = epoch_day}).
    // Empty for rest/done days.
    val move_targets: List<Long> = emptyList(),
    // The day the athlete moved this session from, when moved.
    val moved_from_epoch_day: Long? = null,
    // Why a requested move was refused, plain language.
    val move_note: String? = null,
    // The day whose logged same-discipline session credited this planned one
    // (C4, a swap within the week): the strip reads "Done Tue" on this day.
    val credited_from_epoch_day: Long? = null,
    // Why this session cannot move (C5, plain language) when move_targets is empty.
    val move_blocked_reason: String? = null,
)

/** The active program summary card (app/mod.rs ProgramSummaryView). */
@Serializable
data class ProgramSummaryView(
    val name: String = "",
    val goal: String = "",
    val phase: String = "",
    val week: Int = 0,
    val weeks_total: Int = 0,
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
    val why: WhyView = WhyView(),
    // Hypertrophy goal only: weekly working sets per muscle vs the target, plus
    // one plain note about them. Empty otherwise.
    val muscle_volume: List<MuscleVolumeView> = emptyList(),
    val muscle_volume_note: String = "",
)

/** One muscle's weekly volume on the program card (app/mod.rs MuscleVolumeView).
 *  `target_sets` 0 = no direct target; `label` is display-ready. */
@Serializable
data class MuscleVolumeView(
    val muscle: String = "",
    val target_sets: Int = 0,
    val delivered_sets: Float = 0f,
    val label: String = "",
)

/** Today's completed plan session (app/mod.rs SessionDoneView). Descriptive
 *  only (a summary of logs), so it carries no evidence tag. */
@Serializable
data class SessionDoneView(
    val epoch_day: Long = 0,
    val title: String = "",
    val items: List<SessionDoneItemView> = emptyList(),
)

/** One exercise (or "Run") of a completed session: a display-ready line, its
 *  item status (the hero's verdict: done | done_short | done_lighter | skipped |
 *  skipped_pain | stopped_pain) with `met` == (status == "done"), and for a run
 *  the logged and planned distance (km) / time (min) to render in the user's
 *  unit (`summary` is in km, a fallback only). */
@Serializable
data class SessionDoneItemView(
    val exercise: String = "",
    val summary: String = "",
    val met: Boolean = false,
    val status: String = "",
    val run_km: Double? = null,
    val run_min: Double? = null,
    val run_planned_km: Double? = null,
    val run_planned_min: Double? = null,
)

/** The resolve action for one active pain / illness hold (app/mod.rs
 *  HoldResolutionView). `signal` is the ResolveHold wire name. */
@Serializable
data class HoldResolutionView(
    val signal: String = "",
    val paused_text: String = "",
    val action_label: String = "",
    val confirm_text: String = "",
    // Confirming also switches off the profile's "I'm ill right now" answer
    // (a SetProfile effective today alongside the ResolveHold).
    val ends_profile_illness: Boolean = false,
)

/** Planned vs done for the active plan (app/mod.rs ConsistencyView). */
@Serializable
data class ConsistencyView(
    val this_week_done: Int = 0,
    val this_week_planned: Int = 0,
    // The last (up to) 8 finished plan weeks, oldest first.
    val weeks: List<ConsistencyWeekView> = emptyList(),
    val share_pct: Int? = null,
    val repeating_week: Boolean = false,
    // The done/planned summary first, then any missed / repeated-week / re-entry note.
    val notes: List<GuidanceView> = emptyList(),
)

@Serializable
data class ConsistencyWeekView(
    val week_monday_epoch_day: Long = 0,
    val done: Int = 0,
    val planned: Int = 0,
)

/** One row of the evidence-grade legend, core-provided (app/mod.rs GradeDefView). */
@Serializable
data class GradeDefView(
    val grade: String = "",
    val label: String = "",
    val definition: String = "",
    val confidence: Float = 0f,
)

/** The most recent morning check-in echoed by the core (app/mod.rs CheckinEchoView),
 *  so the shell can rehydrate the sheet and show today's check-in is recorded. */
@Serializable
data class CheckinEchoView(
    val observed_at: Long = 0,
    val sleep_quality: Int? = null,
    val soreness: Int? = null,
    val mood: Int? = null,
    val fatigue: Int? = null,
    val stress: Int? = null,
    val resting_hr_bpm: Double? = null,
    val hrv_rmssd_ms: Double? = null,
)

/** One check-in channel still collecting its baseline (app/mod.rs BaselineStatusView):
 *  an honest progress row shown IN PLACE of a derived signal until enough history
 *  exists, never a fabricated number (HARD RULE 1). Carries no evidence tag. */
@Serializable
data class BaselineStatusView(
    val signal: String = "",
    val label: String = "",
    val have: Int = 0,
    val need: Int = 0,
    // "Collecting" (still gathering history) | "SparseWeek" (this week under-sampled
    // vs the 4/week cadence). Old core omits it → decodes to "Collecting".
    val kind: String = "Collecting",
)

/** One readiness signal's latest state (app/mod.rs ReadinessSignalView). The
 *  evidence fields cite the rule whose threshold judged `state`; grade is
 *  empty for plain factual rows ("recorded"/"clear") that judge nothing. */
@Serializable
data class ReadinessSignalView(
    val signal: String = "",
    val group: String = "", // "metric" | "red_flag"
    val value: Double = 0.0,
    val streak: Int = 0,
    val state: String = "",
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
    // Human sub-line for the banner ("right knee, sharp, 6/10"), serde-default
    // empty. Populated by the core for a characterized Pain report; when empty the
    // shell falls back to its own echo.
    val detail: String = "",
    // The value comes from today's check-in (C6). Defaults true so a core that
    // omits the field never makes a reading look stale.
    val from_today: Boolean = true,
    // The reading is recency-expired (app ReadinessSignalView.stale): `state` is the
    // core's "out of date" text, so the shell adds no "earlier" qualifier.
    val stale: Boolean = false,
)

/** The core's single highest-priority call for today (app/mod.rs TodayHeadlineView).
 *  `kind`: "safety_hold" | "adjustment" | "feedback" | "prescription" | "all_clear". The
 *  all-clear default carries an empty evidence tag (it asserts no claim). */
@Serializable
data class TodayHeadlineView(
    val kind: String = "",
    val summary: String = "",
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
    val why: WhyView = WhyView(),
)

/** Signal→group row for the static readiness-picker fence metadata. */
@Serializable
data class SignalGroupView(
    val signal: String = "",
    val group: String = "",
)

/** Goal-race finish prediction (app/mod.rs RacePredictionView). Daniels+Riegel. */
@Serializable
data class RacePredictionView(
    val goal_label: String = "",
    val predicted: String = "",
    val agreed: Boolean = false,
    val low_sec: Double = 0.0,
    val high_sec: Double = 0.0,
    val summary: String = "",
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
    // Raw inputs echoed by the core so the predictor form rehydrates after a
    // log replay / cold start instead of resetting to hardcoded defaults.
    val recent_distance_m: Double = 0.0,
    val recent_time_sec: Double = 0.0,
    val goal_distance_m: Double = 0.0,
    val weekly_km: Double = 0.0,
    val weeks_since_race: Int? = null,
    // Core-emitted evidence-graded caveats (staleness re-test / marathon
    // under-mileage optimism, app/mod.rs RacePredictionView.notes), rendered as
    // GuidanceView cards under the prediction.
    val notes: List<GuidanceView> = emptyList(),
)

/** Raw HR-zone query echoed back for form rehydration (app/mod.rs HrZoneInputView). */
@Serializable
data class HrZoneInputView(
    val age_years: Double = 0.0,
    val resting_hr_bpm: Double? = null,
    val weeks_since_recalc: Int? = null,
    val weeks_since_pace_test: Int? = null,
)

/** Raw protein query echoed back for form rehydration (app/mod.rs ProteinInputView). */
@Serializable
data class ProteinInputView(
    val bodyweight_kg: Double = 0.0,
    val masters: Boolean = false,
    val deficit: Boolean = false,
)

/** Raw hypertrophy-plan query echoed back for rehydration (app/mod.rs HypertrophyInputView). */
@Serializable
data class HypertrophyInputView(
    val muscle: String = "",
    val weeks: Int = 4,
    val not_growing: Boolean = false,
    val recovering_easily: Boolean = false,
)

/** Profile echoed by the core (app/mod.rs ViewModel.profile) for editor hydration. */
@Serializable
data class ProfileView(
    val progression_cadence: String = "",
    val lift_goal: String = "",
    val goal_distance: String = "",
    val concurrent_goal: String = "",
    val weekly_sets: Int = 0,
    val running_days_per_week: Int = 0,
    val running_km_per_week: Double = 0.0,
    // False while the weekly distance was never given (Profile shows "Not set");
    // the core serde-defaults it true for older logs.
    val running_km_per_week_set: Boolean = true,
    val advanced: Boolean = false,
    // Consolidated person data. Entered once on the profile; the
    // Coach protein/HR-zone calculators prefill from these. All optional so a
    // profile persisted before these fields existed decodes with them absent
    // (null).
    val female: Boolean? = null,
    val bodyweight_kg: Double? = null,
    val age_years: Double? = null,
    val resting_hr_bpm: Double? = null,
    val measured_hr_max: Double? = null,
    // Display weights in pounds instead of kg. DISPLAY-ONLY: every numeric field
    // (bodyweight_kg, and the load/e1RM figures on other views) stays kg; the
    // core already renders its verbatim weight STRINGS in the chosen unit. Wire
    // default false → a pre-unit-flag profile decodes metric. Mirrors
    // app::Profile::weight_lb.
    val weight_lb: Boolean = false,
    // Stage-0 onboarding health screen (schema.rs HealthScreen, File 08
    // onboard-050). Echoed so the Profile health rows rehydrate after a cold
    // start.
    val health: HealthScreen = HealthScreen(),
    // Resistance-training modality (app::Profile::resistance_modality):
    // "Weights" (default) or "Calisthenics". Echoed so the editor rehydrates the
    // choice.
    val resistance_modality: String = "Weights",
    // Advanced training-context fields (app::Profile). Echoed so the editor
    // rehydrates them after a cold start; absent → unset.
    // `environment` is the bare Rust variant name ("Heat"/"Altitude"/"Cold"/
    // "Neutral"); the editor maps it back through Environment.valueOf.
    val high_load_block: Boolean = false,
    val environment: String? = null,
    val env_temp_c: Double? = null,
    val env_altitude_m: Double? = null,
    val weeks_off: Double? = null,
    // Coaching inputs (app::Profile; absent → unset). Mirrors the SetProfile wire
    // names exactly.
    val lift_days_per_week: Int? = null,
    val training_weekdays: Int? = null,
    val goal_race_epoch_day: Long? = null,
    val recent_race: RecentRace? = null,
    val stated_longest_recent_km: Double? = null,
    val layoff_returned_epoch_day: Long? = null,
    val meet_epoch_day: Long? = null,
    val calis_equipment: CalisEquipment? = null,
    val specialization_muscle: String? = null,
    val runner_strength: Boolean = false,
    // Birthday, local epoch day (app::Profile::birth_epoch_day). When set, the
    // core derives age_years from it each day; absent → a legacy stated age only.
    val birth_epoch_day: Long? = null,
)

/** A recent race or time trial the athlete reports (app::RecentRace): distance
 *  in km, finish time in whole seconds, optional local epoch day it was run. */
@Serializable
data class RecentRace(
    val distance_km: Double = 0.0,
    val time_sec: Int = 0,
    val epoch_day: Long? = null,
) : java.io.Serializable

/** Calisthenics equipment on hand (app::CalisEquipment). The core reads an
 *  absent profile field as "everything available"; an answered set is sent whole. */
@Serializable
data class CalisEquipment(
    val pull_up_bar: Boolean = false,
    val dip_bars: Boolean = false,
    val rings: Boolean = false,
) : java.io.Serializable

/** The seven PAR-Q+ general health questions (onboard-051), in form order. Each
 *  "Yes" maps onto one core gate flag: Q2/Q3 are symptoms (onboard-052, not
 *  clearable), Q6 is a bone/joint problem (injury/rehab), the rest are
 *  doctor-follow-up items (parq_positive). [bit] is the item's bit in
 *  [HealthScreen.parq_yes]. */
enum class ParqItem(val bit: Int) {
    HeartOrBloodPressure(0),
    ChestPain(1),
    DizzyOrFainting(2),
    OtherCondition(3),
    Medication(4),
    BoneOrJoint(5),
    SupervisedOnly(6),
    ;

    val symptom: Boolean get() = this == ChestPain || this == DizzyOrFainting
    val injury: Boolean get() = this == BoneOrJoint
}

/** The clearable doctor-follow-up questions (Q1/Q4/Q5/Q7) as a [HealthScreen.parq_yes]
 *  mask; mirrors schema.rs `PARQ_FOLLOWUP_BITS`. */
private val PARQ_FOLLOW_UP_BITS: Int = ParqItem.entries
    .filter { !it.symptom && !it.injury }
    .fold(0) { acc, it -> acc or (1 shl it.bit) }

/**
 * Stage-0 onboarding health screen (schema.rs HealthScreen, File 08 onboard-050).
 * Every flag defaults false → a profile with no gates matches the
 * pre-health-screen wire (the core serde-defaults the whole object). `youth` is
 * derived core-side from `age_years` (no shell-invented age cutoff, HARD RULE 1);
 * the shell only collects the answerable screens (the seven PAR-Q+ questions,
 * clearance, current illness, pregnancy + warning sign, RED-S). Kotlin-
 * `Serializable` too so it can ride in the `ProfileDraft` rememberSaveable bundle.
 *
 * The PAR-Q+ answers live in [parq_yes] (bit per [ParqItem]); the gate flags are
 * derived from it by [withParqAnswer] so they can never disagree: Q1/Q4/Q5/Q7 →
 * `parq_positive`, Q2/Q3 → `symptomatic`, Q6 → `injury_or_rehab`.
 */
@Serializable
data class HealthScreen(
    val youth: Boolean = false,
    val parq_positive: Boolean = false,
    val medically_cleared: Boolean = false,
    val pregnant: Boolean = false,
    val pregnancy_warning_sign: Boolean = false,
    val injury_or_rehab: Boolean = false,
    val reds_signal: Boolean = false,
    val symptomatic: Boolean = false,
    val acute_illness: Boolean = false,
    val parq_yes: Int = 0,
    /** Follow-up answers the doctor's clearance covers (schema.rs
     *  `cleared_parq_mask`). A follow-up Yes outside it gates again core-side. */
    val cleared_parq_mask: Int = 0,
) : java.io.Serializable {
    /** Any SHELL-answerable flag raised. `youth` is deliberately EXCLUDED: it is
     *  core-derived from age, never asserted by the shell (a re-emitted SetProfile
     *  that carried a derived youth=true would pin the pediatric gate even after
     *  the age is raised). The gate-relevant `parq_positive` counts even when
     *  cleared, so a cleared PARQ still persists across a re-emitted SetProfile. */
    fun anyRaised(): Boolean =
        parq_positive || medically_cleared || pregnant ||
            pregnancy_warning_sign || injury_or_rehab || reds_signal ||
            symptomatic || acute_illness || parq_yes != 0

    /** A "Doctor-flagged" answer from the old single toggle, before the seven
     *  questions existed: raised, but no per-question answer recorded. */
    val legacyParq: Boolean get() = parq_yes == 0 && parq_positive

    /** This PAR-Q+ question answered Yes. Q6 also reads the injury flag, which
     *  predates the questions and means the same thing. */
    fun parqYes(item: ParqItem): Boolean =
        parq_yes and (1 shl item.bit) != 0 || (item.injury && injury_or_rehab)

    /** How many of the seven questions are answered Yes. */
    val parqYesCount: Int get() = ParqItem.entries.count { parqYes(it) }

    /** Answer one PAR-Q+ question and re-derive the gate flags from all seven.
     *  Once any per-question answer exists the answers are authoritative; a
     *  legacy raised toggle is kept only while none is. A clearance survives
     *  only while every follow-up Yes was already covered by it: a NEW follow-up
     *  Yes drops `medically_cleared` (safety-044); symptoms are never clearable
     *  (onboard-052). */
    fun withParqAnswer(item: ParqItem, yes: Boolean): HealthScreen {
        val bit = 1 shl item.bit
        // Fold a legacy injury flag into its question first, so answering another
        // question can't silently clear it.
        val base = if (injury_or_rehab) parq_yes or (1 shl ParqItem.BoneOrJoint.bit) else parq_yes
        val mask = if (yes) base or bit else base and bit.inv()
        fun has(i: ParqItem) = mask and (1 shl i.bit) != 0
        val followUpBits = PARQ_FOLLOW_UP_BITS
        val oldFollowUps = parq_yes and followUpBits
        val newFollowUps = mask and followUpBits
        val followUp = newFollowUps != 0 || (mask == 0 && legacyParq)
        // What the existing clearance covers: the recorded mask, or (a clearance
        // saved before the mask existed / a legacy toggle) the answers at the time.
        val covered = when {
            !medically_cleared -> 0
            cleared_parq_mask != 0 -> cleared_parq_mask
            else -> oldFollowUps
        }
        val stillCleared = medically_cleared && followUp &&
            (newFollowUps and covered.inv()) == 0 &&
            (newFollowUps != 0 || covered == 0)
        return copy(
            parq_yes = mask,
            parq_positive = followUp,
            medically_cleared = stillCleared,
            cleared_parq_mask = if (stillCleared) newFollowUps else 0,
            symptomatic = ParqItem.entries.any { it.symptom && has(it) },
            injury_or_rehab = has(ParqItem.BoneOrJoint),
        )
    }

    /** Set the "Cleared by a doctor" answer. The clearance covers exactly the
     *  follow-up answers given now, so a later new Yes is not covered. */
    fun withClearance(cleared: Boolean): HealthScreen =
        copy(
            medically_cleared = cleared,
            cleared_parq_mask = if (cleared) parq_yes and PARQ_FOLLOW_UP_BITS else 0,
        )
}

@Serializable
data class AdjustmentView(
    val summary: String = "",
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
    val why: WhyView = WhyView(),
    // An item-scoped in-session cue: the plan item's exact exercise name (null on
    // a session-wide row), the plate-grid load (kg) it suggests for the remaining
    // sets when it changes load, and whether it suggests ending the exercise.
    val exercise: String? = null,
    val suggested_load_kg: Double? = null,
    val suggests_end_exercise: Boolean = false,
)

@Serializable
data class LiftResultView(
    val exercise: String = "",
    val weight_kg: Double = 0.0,
    val reps: Int = 0,
    val rpe: Double = 0.0,
    val e1rm_kg: Double = 0.0,
    val pct_1rm: Double = 0.0,
    val rir: Double = 0.0,
    // Whether an RPE was actually recorded for this set (app/mod.rs LiftResultView
    // .rpe_recorded; s.rpe > 0). When false the shell omits the "· RIR x" caption
    // rather than implying a measured effort.
    val rpe_recorded: Boolean = false,
    // Cross-checked e1RM range across Epley/Brzycki/Lombardi (strength-005/006,
    // app/mod.rs LiftResultView.cross_check). Null when the estimate is unreliable
    // per strength-006 (0 reps, >10 reps, or an isolation lift): the shell then
    // suggests a 3–6 rep test set instead of showing a false range.
    val cross_check: E1rmRangeView? = null,
    // e1RM change vs the previous set of the same exercise, kg (core-computed,
    // app/mod.rs LiftResultView.e1rm_delta_kg). Null for an exercise's first set.
    // Factual measurement: the shell must NOT phrase it as improving/declining
    // (that judgment belongs to the core's trend feedback, FB-TREND-001).
    val e1rm_delta_kg: Double? = null,
    // "up" / "down" / "flat" direction of the delta; null iff the delta is null.
    val e1rm_direction: String? = null,
    val summary: String = "",
    // When the set was logged, unix seconds; 0 when undated. The shell dates the
    // history card from this (app/mod.rs LiftResultView.observed_at).
    val observed_at: Long = 0,
    // Stable per-entry id the shell sends back in AmendSet/
    // DeleteEntry to edit or delete THIS set. 0 for a legacy row written before
    // entry ids existed: the shell then targets it by observed_at.
    val entry_id: Long = 0,
    // Timed-hold seconds for a static/hold set; null for a reps×load set
    // (app/mod.rs LiftResultView.hold_sec). When set, `summary` reads "<name> - <s> s hold".
    val hold_sec: Int? = null,
    // True when this set's e1RM beats every PRIOR e1RM of the same exercise
    // (app LiftResultView.e1rm_pr): the history card's "New best" chip.
    val e1rm_pr: Boolean = false,
    // Load × reps of this set in kg, 0 for a timed hold (app LiftResultView.tonnage_kg):
    // the one tonnage definition History sums per session and per week.
    val tonnage_kg: Double = 0.0,
)

/**
 * The strength-006 multi-formula e1RM cross-check, flattened for shells
 * (app/mod.rs E1rmRangeView). `low_kg`/`high_kg` span the Epley/Brzycki/Lombardi
 * estimates (0.1-rounded); `formulas` is how many were cross-checked. Carries
 * the full evidence tag. Null on LiftResultView when the estimate is unreliable.
 */
@Serializable
data class E1rmRangeView(
    val low_kg: Double = 0.0,
    val high_kg: Double = 0.0,
    val formulas: Int = 0,
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
)

/**
 * Core-owned pacing verdict for a run's measured split (app/mod.rs SplitVerdictView,
 * feedback-016/017). Carries the chip label, coaching copy, and the full evidence
 * tag, so the ~3% fade threshold never leaks into shell code.
 */
@Serializable
data class SplitVerdictView(
    val verdict: String = "", // "fade" | "even" | "negative"
    val label: String = "",
    val message: String = "",
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
)

// Interval-vs-steady verdict from the GPS track's variability index
// (RUN-INTERVAL-VI-001). Lets the shell show why two runs of the same average
// pace rate differently. Null for a hand-entered run or a track too short.
@Serializable
data class IntervalVerdictView(
    val kind: String = "", // "interval" | "steady"
    val label: String = "",
    val message: String = "",
    val variability_index: Double = 0.0,
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
)

/**
 * User-declared run-intent label. Mirrors app/mod.rs `schema::WorkoutType`.
 * USER DATA, like an exercise name: carries NO evidence and drives NO coaching
 * output (storage + display only). Entry NAMES are the exact serde wire strings
 * (`WorkoutType.name` ⇒ "Steady"/"Interval"/…); [label] is the display text.
 */
enum class WorkoutType(val label: String) {
    Steady("Steady"),
    Interval("Interval"),
    Tempo("Tempo"),
    LongRun("Long run"),
    Recovery("Recovery"),
    Easy("Easy"),
    ;

    companion object {
        /** Decode-safe: an unknown wire string (future variant) ⇒ null, never a crash. */
        fun fromWire(s: String?): WorkoutType? =
            s?.let { w -> entries.firstOrNull { it.name == w } }
    }
}

/**
 * Kind of a logged other activity. Mirrors the core's `ActivityKind`; entry
 * NAMES are the exact serde wire strings. USER DATA: display only.
 */
@Serializable
enum class ActivityKind { Sport, Cycling, Swimming, Walking, Dance, Strength, Other }

/** One logged other activity (view_types.rs ActivityResultView). */
@Serializable
data class ActivityResultView(
    val entry_id: Long = 0,
    val observed_at: Long = 0,
    val kind: ActivityKind = ActivityKind.Other,
    val minutes: Double = 0.0,
    val vigorous: Boolean = false,
    val strengthening: Boolean = false,
)

/**
 * The under-18 weekly activity card (view_types.rs YouthActivityView): this
 * local Mon–Sun week's moderate-to-vigorous minutes and vigorous /
 * strengthening days against the WHO 5–17 targets (youth-pa-001). The targets
 * ride the wire with the evidence tag, never shell literals.
 */
@Serializable
data class YouthActivityView(
    val week_monday_epoch_day: Long = 0,
    val minutes: Int = 0,
    val run_minutes: Int = 0,
    val lift_minutes: Int = 0,
    val activity_minutes: Int = 0,
    val daily_target_minutes: Int = 0,
    val weekly_target_minutes: Int = 0,
    val vigorous_days: Int = 0,
    val strengthening_days: Int = 0,
    val target_days: Int = 0,
    val week_entries: List<ActivityResultView> = emptyList(),
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
    val why: WhyView = WhyView(),
)

@Serializable
data class RunResultView(
    val zone: String = "",
    val pace: String = "",
    val distance_km: Double = 0.0,
    // Raw duration + HR echoed for the manual-run edit prefill.
    val duration_min: Double = 0.0,
    val hr_pct_max: Double = 0.0,
    // The average heart rate the athlete entered, bpm; null when none was (a
    // tracked run's HR is %HRmax only). Echoed so Undo re-logs the bpm itself.
    val avg_hr_bpm: Double? = null,
    // Ready-to-show heart-rate line from the core: "148 bpm · Z2" when a bpm
    // was entered, "HR 80% · Z2" without one, "" with no heart rate. A hard-
    // rated run never carries a contradicting Z1 label.
    val hr_label: String = "",
    val spike_flag: Boolean = false,
    val spike_note: String = "",
    val split_pct: Double? = null,
    // Pacing verdict + chip data for split_pct; null exactly when split_pct is
    // null (hand-entered run or a track too short/degenerate to split).
    val split: SplitVerdictView? = null,
    // Interval-vs-steady verdict from the track's variability index; null for a
    // hand-entered run or a track too short to derive it.
    val interval: IntervalVerdictView? = null,
    // User-declared run-intent label echoed for history display. Held as
    // the raw wire string (decode-safe: an unknown future variant simply decodes
    // as a string and is ignored by WorkoutType.fromWire). null = untagged.
    val workout_type: String? = null,
    val summary: String = "",
    val citation: String = "",
    // Whether this run has an exportable GPS route (>=2 QC-passing fixes). The
    // full GPX document is no longer shipped per-run in the list payload; the shell
    // reads this cheap flag to decide whether to offer the map / share affordances
    // and fetches the document on demand via [Core.exportGpx]. False for a
    // hand-entered run.
    val has_gpx: Boolean = false,
    val observed_at: Long = 0,
    // Stable per-entry id; see LiftResultView.entry_id. 0 = legacy.
    val entry_id: Long = 0,
    // Per-km / per-mi splits derived from the GPS track. Both
    // lists carry the SAME cumulative distance_km at each split end; the shell
    // picks km vs mi by the user's distance-unit override. Empty for a
    // hand-entered run (no track) → no split section rendered.
    val splits_km: List<RunSplitView> = emptyList(),
    val splits_mi: List<RunSplitView> = emptyList(),
    // Structured spike-baseline provenance the shell used to scrape
    // from spike_note (contains("no prior run")). true when a prior 30-day
    // baseline distance exists to gauge this run against; false for a first run
    // with no baseline. Only meaningful when spike_flag.
    val spike_has_baseline: Boolean = false,
    // Whole-session effort, CR-10 (0–10), as rated by the athlete; null =
    // unrated (the detail sheet's Effort field rates it via AmendRunMeta).
    val session_rpe: Int? = null,
    // Session-RPE load = moving minutes × session_rpe (arbitrary units); null
    // when unrated or unmeasurable. Never added to the heart-rate load.
    val srpe_load_au: Double? = null,
    // Best-effort records this run holds, e.g. ["Fastest 5K"].
    val best_effort_labels: List<String> = emptyList(),
)

// One km-or-mi split of a GPS run. `pace` is pre-formatted "m:ss" (render
// verbatim, the core already applied the unit); `distance_km` is the CUMULATIVE
// km at the split's end (identical value in splits_km and splits_mi); `partial`
// marks the trailing sub-unit remainder.
@Serializable
data class RunSplitView(
    val index: Int = 0,
    val pace: String = "",
    // Numeric per-unit pace in whole seconds — the same figure `pace` is
    // formatted from. Drives splits bar-chart bar lengths without re-parsing the
    // "m:ss" string.
    val seconds_per_unit: Int = 0,
    val distance_km: Double = 0.0,
    val partial: Boolean = false,
)

@Serializable
data class GuidanceView(
    val section: String = "",
    val summary: String = "",
    // The summary restated in miles for a row that states a distance; empty on
    // every other row (view_types.rs GuidanceView.summary_mi).
    val summary_mi: String = "",
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
    val why: WhyView = WhyView(),
    // On the profile's training-age row, the training age ("Intermediate"); empty
    // on every other row (view_types.rs GuidanceView.training_age).
    val training_age: String = "",
)

/**
 * CTL/ATL/TSB fitness–fatigue bookkeeping over the logged run history
 * (app/mod.rs TrainingLoadView, File 07 impulse-response; LOAD-PMC-001). Daily load
 * is a Lucia 3-zone TRIMP from each dated HR-carrying run; `method` states what
 * was counted. Bookkeeping only — TSB is NOT a validated performance predictor
 * (the claim statement carries that caveat).
 */
@Serializable
data class TrainingLoadView(
    val ctl: Double = 0.0,
    val atl: Double = 0.0,
    val tsb: Double = 0.0,
    val days: Int = 0,
    val sessions_counted: Int = 0,
    val sessions_skipped: Int = 0,
    val method: String = "",
    val summary: String = "",
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
)

/**
 * Session-RPE weekly load (app SrpeLoadView, LOAD-SRPE-001): this week vs last
 * in arbitrary units, rated/unrated run counts, a plain method line and summary.
 * Its own series, never mixed into [TrainingLoadView].
 */
@Serializable
data class SrpeLoadView(
    val week_au: Double = 0.0,
    val prev_week_au: Double = 0.0,
    val rated_runs: Int = 0,
    val unrated_runs: Int = 0,
    val method: String = "",
    val summary: String = "",
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
)

/**
 * Running progress over time (app RunProgressView): the last 16
 * Monday-start local weeks oldest first (empty weeks as zeros), the profile's
 * planned weekly km (0 = none), the longest run and the best efforts. Factual
 * aggregates; every distance is canonical km, the shell converts.
 */
@Serializable
data class RunProgressView(
    val weekly: List<RunWeekView> = emptyList(),
    val planned_weekly_km: Double = 0.0,
    val longest_run_km: Double = 0.0,
    val longest_run_observed_at: Long = 0,
    val best_efforts: List<BestEffortView> = emptyList(),
    val citation: String = "",
    val grade: String = "",
    val confidence: Float = 0f,
)

/** One Monday-start local week of running (app RunWeekView). */
@Serializable
data class RunWeekView(
    val week_start_epoch_day: Long = 0,
    val km: Double = 0.0,
    val longest_km: Double = 0.0,
    val runs: Int = 0,
    val moving_min: Double = 0.0,
    val srpe_load_au: Double = 0.0,
    val rated_runs: Int = 0,
)

/**
 * The fastest known effort over one standard distance (app BestEffortView):
 * 1000/5000/10000/21097 m. `pace_label` is per km only, so the shell derives the
 * displayed pace from [time_sec] and [distance_m] in the user's unit.
 * `from_split` = measured from a stretch of a longer GPS run.
 */
@Serializable
data class BestEffortView(
    val distance_m: Int = 0,
    val label: String = "",
    val time_sec: Int = 0,
    val time_label: String = "",
    val pace_label: String = "",
    val entry_id: Long = 0,
    val observed_at: Long = 0,
    val from_split: Boolean = false,
)

@Serializable
data class FeedbackView(
    val category: String = "",
    val category_label: String = "",
    val message: String = "",
    // `message` with its distances in miles; empty when it states none.
    val message_mi: String = "",
    val suppresses_praise: Boolean = false,
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
    val why: WhyView = WhyView(),
)

// ── Events (encode) ──────────────────────────────────────────────────────────
// Hand-built JSON to match serde's externally-tagged enum representation:
//   unit variant      -> "VariantName"
//   struct variant    -> {"VariantName": { ...fields }}
//   newtype variant   -> {"VariantName": <inner value>}

// Names must match the Rust `ReadinessSignal` variants exactly: `signal.name`
// is the wire value serde reads. The trailing medical-referral signals
// (Illness/RedS/CardiacRedFlag/BoneStress) sit at the top of the safety ladder;
// omitting them here makes that tier unreachable from the shell.
enum class ReadinessSignal {
    Rpe, EstimatedOneRm, BarVelocity, VelocityLoss, WellnessZ,
    HrvLnRmssd, HrvCv, AerobicDecoupling, RestingHr, Pain,
    // Soreness sits between Pain and Illness in the core's ladder (schema.rs);
    // it's a metric (1–7 Hooper), the autoreg-030 second-clause downgrade signal.
    Soreness,
    Illness, RedS, CardiacRedFlag, BoneStress,
}

enum class ProgressionCadence { EverySession, WeekToWeek, MonthToMonth }
enum class LiftGoal { MaxStrength, Power, Hypertrophy }
enum class GoalDistance { General, C25k, FiveK, TenK, HalfMarathon, Marathon }
enum class ConcurrentGoal { Strength, Power, Hypertrophy, EndurancePriority }
enum class ResistanceModality { Weights, Calisthenics }

// Calisthenics progression ladders (mirrors calisthenics::LadderId). Names cross
// the JSON FFI verbatim as the Rust variant names.
enum class CalisLadder {
    HorizontalPush, VerticalPull, Squat, Dip, VerticalPush, HorizontalPull, Hinge, Core,
    Handstand, FrontLever, Planche, BackLever, MuscleUp, Jump, PlyoPush,
}

// Training environment (mirrors individualization::Environment). Names must
// match the Rust variants exactly: they cross the JSON FFI as the bare variant
// name (the Option<Environment> profile field), arming the File-08 environment
// modifiers (indiv-025 / safety-024) and the running-041 pace-correction notes.
enum class Environment { Heat, Altitude, Cold, Neutral }

// Pain characterization for a Pain readiness report (schema.rs PainDetail,
// File-08 Table 4.1). Names must match the Rust `PainKind`/`PainTrend` variants
// exactly: they are the wire values serde reads.
enum class PainKind { SharpJoint, TendonLoadRelated, Doms, Other }
enum class PainTrend { Stable, Rising, WorseningAfter24h }

/**
 * A characterized pain report (schema.rs PainDetail). Attached to a
 * [Event.SubmitReadiness] whose signal is Pain so the core's graded pain gate
 * (autoreg pain_gate, File-08 Table 4.1) is reachable, SharpJoint→hard stop,
 * TendonLoadRelated→graded by severity/trend, Doms→continue. Without it every
 * pain report is the conservative bare-report hard stop.
 *
 * `location` is display-only context for the banner sub-line ("Left knee"), no
 * core rule branches on it. Every field is serde-defaulted on the Rust side so
 * old logs (no pain object) still replay.
 */
data class PainDetail(
    val kind: PainKind,
    // 0–10 numeric rating scale; null = not rated (omitted on the wire, the core
    // treats an unrated tendon report conservatively).
    val severity: Int? = null,
    val trend: PainTrend,
    val persists: Boolean = false,
    val location: String? = null,
)

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

    /** Whether this event is written to the durable log when the core accepts it.
     *  Everything is persisted except the shell's clock ([SetToday]), which is
     *  re-sent every launch and kept as a single last-write-wins line — writing it
     *  on every foreground would only grow the log with a line compaction drops. */
    val persist: Boolean get() = true

    data class SubmitReadiness(
        val signal: ReadinessSignal,
        val value: Double,
        val observedAt: Long,
        // Consecutive days/sessions the signal's condition has held (ReadinessInput
        // .streak). 0 = not tracked. The multi-day autoreg rules (e1RM ≥2 sessions,
        // RHR ≥2 days, SubjectiveMultiDay tier) are dead until the shell sends this.
        val streak: Int = 0,
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
                put("streak", streak)
                if (pain != null) put("pain", buildJsonObject {
                    put("kind", pain.kind.name)
                    pain.severity?.let { put("severity", it) }
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
        // False = the athlete never gave a weekly distance (C9): the core forces
        // runningKmPerWeek to 0.0 and plans from logged runs or its KB floors. A
        // real "I don't run yet" answer is 0.0 with this true. Emitted only when
        // false so every answered profile keeps the legacy wire.
        val runningKmPerWeekSet: Boolean = true,
        // Consolidated person data: sent on EVERY SetProfile so
        // the last-write-wins profile keeps them (an omitted field defaults to
        // None on the core, so the draft must always re-emit them). Unanswered
        // person data is omitted from the wire (serde default), keeping the line
        // identical to the base eight-field form when nothing is set.
        val female: Boolean? = null,
        val bodyweightKg: Double? = null,
        val ageYears: Double? = null,
        val restingHrBpm: Double? = null,
        val measuredHrMax: Double? = null,
        // Display weights in pounds instead of kg (mirrors app::Profile::weight_lb).
        // Display-only; the core converts its verbatim weight strings. Emitted only
        // when true so an all-metric profile keeps the same wire form.
        val weightLb: Boolean = false,
        // Stage-0 onboarding health screen (schema.rs HealthScreen). Sent so the
        // pediatric/PAR-Q+/pregnancy/injury/RED-S deferral gates are reachable.
        // Emitted only when a flag is raised, so an all-clear profile keeps
        // the same pre-health wire; the core serde-defaults the rest.
        val health: HealthScreen = HealthScreen(),
        // Resistance-training modality (mirrors app::Profile::resistance_modality).
        // Emitted only when Calisthenics so a weights profile keeps the
        // same pre-calisthenics wire (core serde-defaults Weights).
        val resistanceModality: ResistanceModality = ResistanceModality.Weights,
        // Advanced training-context fields (mirror app::Profile). All
        // #[serde(default)] on the core, so each is emitted only when set and an
        // unset advanced profile keeps the same legacy wire. Carried on
        // every SetProfile (last-write-wins) so a later training-field edit can't
        // drop them.
        //   high_load_block  — arms autoreg-029's parasympathetic-saturation guard
        //   environment       — File-08 environment modifiers (heat/altitude/cold)
        //   env_temp_c/_m     — running-041 pace-correction triggers
        //   weeks_off         — REENTRY-001 ramp + hypertrophy-045 reduced MEV note
        val highLoadBlock: Boolean = false,
        val environment: Environment? = null,
        val envTempC: Double? = null,
        val envAltitudeM: Double? = null,
        val weeksOff: Double? = null,
        // Coaching inputs (mirror app::Profile; all serde-default on the core, so
        // each is emitted only when answered and an unanswered profile keeps the
        // legacy wire):
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
        // Profile-history effective day (mirrors app::Profile
        // effective_from_epoch_day). The core keys its profile history by this: a
        // SetProfile with eff E supersedes every entry with eff >= E, and a past
        // day is prescribed from the profile effective THAT day, so an edit no
        // longer re-prescribes settled days. EVERY production construction site
        // passes the core's today (model.today_epoch_day ?: todayEpochDay());
        // null replaces the whole history (a legacy undated edit). Emitted as
        // `effective_from_epoch_day` only when non-null so an all-metric legacy
        // wire is unchanged.
        val effectiveFromEpochDay: Long? = null,
        // Birthday, local epoch day (mirrors app::Profile::birth_epoch_day). The
        // core derives age_years and the under-18 gate from it and its today;
        // emitted only when set, so a legacy age-only profile keeps its wire.
        val birthEpochDay: Long? = null,
    ) : Event {
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
                // Only emit person data that is set: an all-absent profile stays
                // identical to the base eight-field wire (test-pinned), and the
                // core's serde defaults fill None for anything omitted.
                if (female != null) put("female", female)
                if (bodyweightKg != null) put("bodyweight_kg", bodyweightKg)
                if (ageYears != null) put("age_years", ageYears)
                if (restingHrBpm != null) put("resting_hr_bpm", restingHrBpm)
                if (measuredHrMax != null) put("measured_hr_max", measuredHrMax)
                // Emit only when set to lb so an all-metric profile stays
                // identical to the pre-unit-flag wire (core serde-defaults false).
                if (weightLb) put("weight_lb", true)
                if (resistanceModality != ResistanceModality.Weights) {
                    put("resistance_modality", resistanceModality.name)
                }
                // Advanced training-context fields: emit only when set so an unset
                // advanced profile stays the same as the legacy wire (the
                // core serde-defaults each to None/false). `environment` rides as
                // the bare variant name the Rust enum deserializes.
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
                // Health screen: emit only when a gate flag is raised so the
                // all-clear line stays identical to the pre-health-screen form.
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
                holdSec?.let { put("hold_sec", it) }
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
        val longestRecentKm: Double,
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
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("LogRun", buildJsonObject {
                put("distance_km", distanceKm)
                put("duration_min", durationMin)
                put("hr_pct_max", hrPctMax)
                put("longest_recent_km", longestRecentKm)
                put("observed_at", observedAt)
                put("entry_id", entryId)
                workoutType?.let { put("workout_type", it.name) }
                sessionRpe?.let { put("session_rpe", it) }
                avgHrBpm?.let { put("avg_hr_bpm", it) }
            })
        }
    }

    data class LogRunTrack(
        val points: List<GpsPoint>,
        val hrPctMax: Double,
        val longestRecentKm: Double,
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
                put("longest_recent_km", longestRecentKm)
                put("observed_at", observedAt)
                put("entry_id", entryId)
                workoutType?.let { put("workout_type", it.name) }
                if (segmentStarts.isNotEmpty()) {
                    putJsonArray("segment_starts") { segmentStarts.forEach { add(it) } }
                }
                sessionRpe?.let { put("session_rpe", it) }
                avgHrBpm?.let { put("avg_hr_bpm", it) }
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
                holdSec?.let { put("hold_sec", it) }
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
        val longestRecentKm: Double = 0.0,
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
                put("distance_km", distanceKm)
                put("duration_min", durationMin)
                put("hr_pct_max", hrPctMax)
                put("longest_recent_km", longestRecentKm)
                put("observed_at", observedAt)
                put("observed_at_fallback", observedAtFallback)
                workoutType?.let { put("workout_type", it.name) }
                sessionRpe?.let { put("session_rpe", it) }
                avgHrBpm?.let { put("avg_hr_bpm", it) }
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

    /** "Changed device? Relearn your HRV normal": HRV readings before [observedAt]
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
