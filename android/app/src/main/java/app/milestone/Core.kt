package app.milestone

import android.content.Context
import java.io.File
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.add
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray

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

    private external fun update(event: ByteArray): ByteArray
    private external fun view(): ByteArray

    /** Pure FIT-file parse in the Rust core (fitparser), stateless, touches no
     *  model/log. Returns `{"segments":[[{lat,lon,time_sec,hr_bpm?},…],…]}` or
     *  `{"error":"…"}`; the shell converts into its LogRunTrack import path.
     *  Nullable: the M5-hardened JNI returns a null jstring if the result string
     *  can't be allocated (double alloc-failure), the decode site treats null
     *  like the error envelope. */
    external fun parseFit(bytes: ByteArray): String?

    private val json = Json {
        ignoreUnknownKeys = true
        encodeDefaults = true
    }

    // Append-only, newline-delimited event log. The crux core holds all state in
    // memory only, so we persist the event *stream* (not the derived model) and
    // rebuild by replaying it, the same approach as the web shell's localStorage
    // log. Each line is one event's exact serde wire JSON.
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
     */
    @Synchronized
    fun restore(ctx: Context): Boolean {
        val file = File(ctx.filesDir, "event-log.ndjson")
        log = file
        // Already replayed this process (Activity recreation): the only way this
        // is still a fresh install is if nothing was ever persisted.
        if (restored) return !file.exists()
        restored = true
        // Compacting before replay matters here: a run's GPS track (thousands of
        // points) is the heaviest line in the log, and every `Clear*` supersedes
        // its whole family: the compacted replay is model-equivalent.
        val loaded = EventLog.load(file)
        loaded.lines.forEach { update(it.toByteArray(Charsets.UTF_8)) }
        return loaded.freshInstall
    }

    // The last ViewModel that decoded cleanly. If a later core call comes back as
    // a panic-firewall error envelope, we return this instead of an all-default
    // blank one, a blank would silently wipe a live DO-NOT-TRAIN hold (HARD RULE 3).
    private var lastGoodView: ViewModel? = null

    /** Dispatch an event, persist it to the log (only if accepted), then return
     *  the fresh view model.
     *
     *  Serialized on the [Core] monitor together with [restore] and [currentView]
     *  (they share one lock). `send` is called from BOTH the main and IO
     *  dispatchers; without the lock two concurrent calls could interleave their
     *  JNI `update` → `appendText` steps and persist the event log in a different
     *  order than the in-memory core applied them, or race the [lastGoodView]
     *  cache. The monitor makes each call's JNI-apply → append → read-back atomic,
     *  so the guarantee holds: **the log is written in exactly the order the core
     *  accepted the events, so a later replay reconstructs the identical model.** */
    @Synchronized
    fun send(event: Event): ViewModel {
        val wire = event.toJson().toString()
        val response = update(wire.toByteArray(Charsets.UTF_8))
        // Persist ONLY if the core accepted the event. A serde-rejected event
        // returns empty bytes (ffi out.clear()); a core panic returns an
        // {"error":...} object. Either way, re-writing the line would just have it
        // re-dropped on every future replay, and grows the log with dead events.
        if (isAccepted(response)) {
            try {
                log?.appendText(wire + "\n")
            } catch (e: Exception) {
                // Disk full / IO error: keep going. The in-memory core already
                // applied the event; crashing the UI send path here would be worse
                // than losing this one line's durability.
                android.util.Log.w("Core", "failed to persist event to log", e)
            }
        }
        return currentView()
    }

    /** True if the core ACCEPTED an event: crux emits a non-empty JSON effect
     *  array. A rejected event yields empty bytes; a panic yields an `{"error"…}`
     *  object (leading `{`). */
    private fun isAccepted(response: ByteArray): Boolean {
        val first = response.firstOrNull { !it.toInt().toChar().isWhitespace() } ?: return false
        return first == '['.code.toByte()
    }

    /** Current view model without dispatching an event. Shares the [Core] monitor
     *  with [send]/[restore] so a read-back never observes a half-applied event or
     *  races the [lastGoodView] read/write. */
    @Synchronized
    fun currentView(): ViewModel {
        val text = String(view(), Charsets.UTF_8)
        // The FFI panic firewall returns {"error":{...}} on a core panic. With
        // ignoreUnknownKeys that would decode into an all-default ViewModel -
        // train_blocked=false, safety_tier=null, silently blanking safety state.
        // Detect the error envelope and keep the last good view instead.
        if (isErrorEnvelope(text)) {
            android.util.Log.e("Core", "core returned error envelope, keeping last good view: $text")
            return lastGoodView ?: ViewModel()
        }
        val vm = json.decodeFromString(ViewModel.serializer(), text)
        lastGoodView = vm
        return vm
    }

    private fun isErrorEnvelope(text: String): Boolean = try {
        val el = json.parseToJsonElement(text)
        el is JsonObject && el.containsKey("error")
    } catch (_: Exception) {
        false
    }
}

// ── View model (decode) ──────────────────────────────────────────────────────
// Field names + shapes mirror shared/src/app.rs::ViewModel exactly.

@Serializable
data class ViewModel(
    val safety_tier: String? = null,
    val train_blocked: Boolean = false,
    val adjustments: List<AdjustmentView> = emptyList(),
    val review_adjustments: List<AdjustmentView> = emptyList(),
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
    // Raw calculator inputs echoed back so forms rehydrate after a cold start
    // (see the matching structs above). race_prediction echoes its own inline.
    val hr_zone_input: HrZoneInputView? = null,
    val protein_input: ProteinInputView? = null,
    val hypertrophy_input: HypertrophyInputView? = null,
    // KB-honest per-signal readiness summary (app.rs readiness_summary): the
    // latest state of each observed signal judged by the core's own KB
    // thresholds. Deliberately NO composite 0–100 score exists on the wire.
    val readiness_summary: List<ReadinessSignalView> = emptyList(),
    // Core-owned "today's call": safety hold > adjustment > feedback >
    // all-clear. Null only against a pre-headline core build.
    val today_headline: TodayHeadlineView? = null,
    // Static signal→group metadata ("metric" | "red_flag") driving the
    // readiness picker's red-flag fence from the core.
    val signal_groups: List<SignalGroupView> = emptyList(),
    // The most recent morning check-in echoed for rehydration +
    // a "checked in today" cue. Null until a check-in exists / on an old core.
    val checkin_today: CheckinEchoView? = null,
    // Honest "collecting your baseline" rows for each check-in channel that has
    // data but not yet enough history to emit a z-score/delta. Empty otherwise.
    val baseline_status: List<BaselineStatusView> = emptyList(),
    // Evidence-grade legend exported from core data (File 09) so
    // the "How evidence grading works" sheet renders KB definitions, not
    // hardcoded shell copy. Empty on an old core (falls back to shell copy).
    val grade_definitions: List<GradeDefView> = emptyList(),
    // Coach-as-planner. The concrete next session (the Coach hero),
    // the current week strip, and the program summary. Null/empty until the user
    // accepts a plan / on an old core.
    val next_session: SessionPlanView? = null,
    val week_plan: List<SessionPlanView> = emptyList(),
    val program: ProgramSummaryView? = null,
    // Structured HRmax figure (bpm + measured/estimate + Tanaka
    // split) for the last HR-zone query, so the shell reads it instead of
    // regex-scraping the hr_zones rows. Null on an old core / until a query is
    // made / when the age is out of range.
    val hr_max: HrMaxView? = null,
    // Structured protein g/day figures paralleling protein_targets,
    // so the shell reads them instead of regex-scraping those rows. Empty on an
    // old core / until a protein query is made.
    val protein_figures: List<ProteinFigureView> = emptyList(),
)

/**
 * The structured HRmax figure the shell used to regex-scrape out of
 * the hr_zones summary rows (incl. the "208 − 0.7 × age" Tanaka split). Field
 * names mirror the Rust HrMaxView (app.rs) serde names exactly; all defaulted so
 * an old wire blob (without hr_max) still decodes. `bpm` is core-rounded.
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
 * mirror the Rust ProteinFigureView (app.rs) serde names exactly; all defaulted
 * for wire back-compat. `kind` is "masters" or "deficit"; g/day figures are
 * core-rounded and 0 when `refused` (a RED-S deficit refusal carries no number).
 */
@Serializable
data class ProteinFigureView(
    val kind: String = "",
    val low_g_per_day: Double = 0.0,
    val high_g_per_day: Double = 0.0,
    val refused: Boolean = false,
)

/**
 * The three-part "why?" disclosure carried by every action-bearing card
 * (app.rs WhyView): basis (what it's based on) →
 * grade_note (why this grade) → improves (what data would sharpen it). All
 * fields serde-default empty, so an old core simply yields an empty block and
 * the card falls back to the legacy evidence restatement.
 */
@Serializable
data class WhyView(
    val basis: String = "",
    val grade_note: String = "",
    val improves: String = "",
)

/** One prescribed exercise, core-flattened (app.rs PrescriptionView): the
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
    val anchored_on: String = "",
    val adjusted_note: String = "",
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
    val why: WhyView = WhyView(),
)

/** One planned day in the week (app.rs SessionPlanView). Rendered
 *  strictly downstream of the safety gates: a hold sets status "blocked" and
 *  empties items. status ∈ next|planned|done|missed|adjusted|blocked|rest. */
@Serializable
data class SessionPlanView(
    val epoch_day: Long = 0,
    val title: String = "",
    val session_type: String = "",
    val status: String = "",
    val items: List<PrescriptionView> = emptyList(),
    val adjustment: AdjustmentView? = null,
)

/** The active program summary card (app.rs ProgramSummaryView). */
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
)

/** One row of the evidence-grade legend, core-provided (app.rs GradeDefView). */
@Serializable
data class GradeDefView(
    val grade: String = "",
    val label: String = "",
    val definition: String = "",
    val confidence: Float = 0f,
)

/** The most recent morning check-in echoed by the core (app.rs CheckinEchoView),
 *  so the shell can rehydrate the sheet and show today's check-in is recorded. */
@Serializable
data class CheckinEchoView(
    val observed_at: Long = 0,
    val sleep_quality: Int? = null,
    val soreness: Int? = null,
    val mood: Int? = null,
    val resting_hr_bpm: Double? = null,
    val hrv_rmssd_ms: Double? = null,
)

/** One check-in channel still collecting its baseline (app.rs BaselineStatusView):
 *  an honest progress row shown IN PLACE of a derived signal until enough history
 *  exists, never a fabricated number (HARD RULE 1). Carries no evidence tag. */
@Serializable
data class BaselineStatusView(
    val signal: String = "",
    val label: String = "",
    val have: Int = 0,
    val need: Int = 0,
    val note: String = "",
)

/** One readiness signal's latest state (app.rs ReadinessSignalView). The
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
    // empty. Populated by the core for a characterized Pain report; empty on an
    // older core, in which case the shell falls back to its own echo.
    val detail: String = "",
)

/** The core's single highest-priority call for today (app.rs TodayHeadlineView).
 *  `kind`: "safety_hold" | "adjustment" | "feedback" | "all_clear". The
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

/** Goal-race finish prediction (app.rs RacePredictionView). Daniels+Riegel. */
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
    // under-mileage optimism, app.rs RacePredictionView.notes). Empty on an old
    // core; rendered as GuidanceView cards under the prediction.
    val notes: List<GuidanceView> = emptyList(),
)

/** Raw HR-zone query echoed back for form rehydration (app.rs HrZoneInputView). */
@Serializable
data class HrZoneInputView(
    val age_years: Double = 0.0,
    val resting_hr_bpm: Double? = null,
    val weeks_since_recalc: Int? = null,
    val weeks_since_pace_test: Int? = null,
)

/** Raw protein query echoed back for form rehydration (app.rs ProteinInputView). */
@Serializable
data class ProteinInputView(
    val bodyweight_kg: Double = 0.0,
    val masters: Boolean = false,
    val deficit: Boolean = false,
)

/** Raw hypertrophy-plan query echoed back for rehydration (app.rs HypertrophyInputView). */
@Serializable
data class HypertrophyInputView(
    val muscle: String = "",
    val weeks: Int = 4,
    val not_growing: Boolean = false,
    val recovering_easily: Boolean = false,
)

/** Profile echoed by the core (app.rs ViewModel.profile) for editor hydration. */
@Serializable
data class ProfileView(
    val progression_cadence: String = "",
    val lift_goal: String = "",
    val goal_distance: String = "",
    val concurrent_goal: String = "",
    val weekly_sets: Int = 0,
    val running_days_per_week: Int = 0,
    val running_km_per_week: Double = 0.0,
    val advanced: Boolean = false,
    val endurance_intensity_pct_vo2max: Double = 0.0,
    // Consolidated person data. Entered once on the profile; the
    // Coach protein/HR-zone calculators prefill from these. All optional so a
    // pre-Phase-5 profile decodes with them absent (null).
    val female: Boolean = false,
    val bodyweight_kg: Double? = null,
    val age_years: Double? = null,
    val resting_hr_bpm: Double? = null,
    val measured_hr_max: Double? = null,
    // Stage-0 onboarding health screen (schema.rs HealthScreen, File 08
    // onboard-050). Echoed so the Profile health rows rehydrate after a cold
    // start; null/absent on a pre-health-screen core.
    val health: HealthScreen = HealthScreen(),
)

/**
 * Stage-0 onboarding health screen (schema.rs HealthScreen, File 08 onboard-050).
 * Every flag defaults false → a profile with no gates is byte-identical to the
 * pre-health-screen wire (the core serde-defaults the whole object). `youth` is
 * derived core-side from `age_years` (no shell-invented age cutoff, HARD RULE 1);
 * the shell only collects the answerable screens (PAR-Q+, clearance, pregnancy +
 * warning sign, injury/rehab, RED-S). Kotlin-`Serializable` too so it can ride in
 * the `ProfileDraft` rememberSaveable bundle.
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
) : java.io.Serializable {
    /** Any SHELL-answerable flag raised. `youth` is deliberately EXCLUDED: it is
     *  core-derived from age, never asserted by the shell (a re-emitted SetProfile
     *  that carried a derived youth=true would pin the pediatric gate even after
     *  the age is raised). The gate-relevant `parq_positive` counts even when
     *  cleared, so a cleared PARQ still persists across a re-emitted SetProfile. */
    fun anyRaised(): Boolean =
        parq_positive || medically_cleared || pregnant ||
            pregnancy_warning_sign || injury_or_rehab || reds_signal
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
    // e1RM change vs the previous set of the same exercise, kg (core-computed,
    // app.rs LiftResultView.e1rm_delta_kg). Null for an exercise's first set.
    // Factual measurement: the shell must NOT phrase it as improving/declining
    // (that judgment belongs to the core's trend feedback, FB-TREND-001).
    val e1rm_delta_kg: Double? = null,
    // "up" / "down" / "flat" direction of the delta; null iff the delta is null.
    val e1rm_direction: String? = null,
    val summary: String = "",
    val observed_at: Long = 0,
    // Stable per-entry id the shell sends back in AmendSet/
    // DeleteEntry to edit or delete THIS set. 0 for a legacy row (pre-Phase-4
    // log): the shell then targets it by observed_at.
    val entry_id: Long = 0,
)

/**
 * Core-owned pacing verdict for a run's measured split (app.rs SplitVerdictView,
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
 * User-declared run-intent label. Mirrors app.rs `schema::WorkoutType`.
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
    ;

    companion object {
        /** Decode-safe: an unknown wire string (future variant) ⇒ null, never a crash. */
        fun fromWire(s: String?): WorkoutType? =
            s?.let { w -> entries.firstOrNull { it.name == w } }
    }
}

@Serializable
data class RunResultView(
    val zone: String = "",
    val pace: String = "",
    val distance_km: Double = 0.0,
    // Raw duration + HR echoed for the manual-run edit prefill.
    val duration_min: Double = 0.0,
    val hr_pct_max: Double = 0.0,
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
    val gpx: String = "",
    val observed_at: Long = 0,
    // Stable per-entry id; see LiftResultView.entry_id. 0 = legacy.
    val entry_id: Long = 0,
    // Per-km / per-mi splits derived from the GPS track. Both
    // lists carry the SAME cumulative distance_km at each split end; the shell
    // picks km vs mi by the user's distance-unit override. serde-default so a
    // hand-entered run (no track) or an old core simply decodes to empty → no
    // split section rendered.
    val splits_km: List<RunSplitView> = emptyList(),
    val splits_mi: List<RunSplitView> = emptyList(),
    // Structured spike-baseline provenance the shell used to scrape
    // from spike_note (contains("no prior run")). true when a prior 30-day
    // baseline distance exists to gauge this run against; false for a first run
    // with no baseline. Only meaningful when spike_flag. Defaulted for wire
    // back-compat (old core / old logged runs decode to false).
    val spike_has_baseline: Boolean = false,
)

// One km-or-mi split of a GPS run. `pace` is pre-formatted "m:ss" (render
// verbatim, the core already applied the unit); `distance_km` is the CUMULATIVE
// km at the split's end (identical value in splits_km and splits_mi); `partial`
// marks the trailing sub-unit remainder.
@Serializable
data class RunSplitView(
    val index: Int = 0,
    val pace: String = "",
    val distance_km: Double = 0.0,
    val partial: Boolean = false,
)

@Serializable
data class GuidanceView(
    val section: String = "",
    val summary: String = "",
    val grade: String = "",
    val citation: String = "",
    val confidence: Float = 0f,
    val safety_critical: Boolean = false,
    val contested: Boolean = false,
    val why: WhyView = WhyView(),
)

@Serializable
data class FeedbackView(
    val category: String = "",
    val category_label: String = "",
    val message: String = "",
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

// Pain characterization for a Pain readiness report (schema.rs PainDetail,
// File-08 Table 4.1). Names must match the Rust `PainKind`/`PainTrend` variants
// exactly: they are the wire values serde reads.
enum class PainKind { SharpJoint, TendonLoadRelated, Doms, Other }
enum class PainTrend { Stable, Rising }

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
    val severity: Int, // 0–10 numeric rating scale
    val trend: PainTrend,
    val persists: Boolean = false,
    val location: String? = null,
)

sealed interface Event {
    fun toJson(): JsonElement

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
                    put("severity", pain.severity)
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

    /** Undo one accidental report: core drops its most recent input carrying [signal]. */
    data class RemoveReadiness(val signal: ReadinessSignal) : Event {
        override fun toJson() = buildJsonObject {
            put("RemoveReadiness", buildJsonObject { put("signal", signal.name) })
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
        val mood: Int? = null,          // 1–5 (5 = great mood, low stress)
        val restingHrBpm: Double? = null,
        val hrvRmssdMs: Double? = null,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("SubmitCheckin", buildJsonObject {
                put("observed_at", observedAt)
                if (sleepQuality != null) put("sleep_quality", sleepQuality)
                if (soreness != null) put("soreness", soreness)
                if (mood != null) put("mood", mood)
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
        val enduranceIntensityPctVo2max: Double,
        // Consolidated person data: sent on EVERY SetProfile so
        // the last-write-wins profile keeps them (an omitted field defaults to
        // None/false on the core, so the draft must always re-emit them). Absent
        // person data is omitted from the wire (serde default), keeping the line
        // identical to the pre-Phase-5 nine-field form when nothing is set.
        val female: Boolean = false,
        val bodyweightKg: Double? = null,
        val ageYears: Double? = null,
        val restingHrBpm: Double? = null,
        val measuredHrMax: Double? = null,
        // Stage-0 onboarding health screen (schema.rs HealthScreen). Sent so the
        // pediatric/PAR-Q+/pregnancy/injury/RED-S deferral gates are reachable.
        // Emitted only when a flag is raised, so an all-clear profile keeps
        // the byte-identical pre-health wire; the core serde-defaults the rest.
        val health: HealthScreen = HealthScreen(),
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
                put("endurance_intensity_pct_vo2max", enduranceIntensityPctVo2max)
                // Only emit person data that is set: an all-absent profile stays
                // byte-identical to the old nine-field wire (test-pinned), and the
                // core's serde defaults fill None/false for anything omitted.
                if (female) put("female", true)
                if (bodyweightKg != null) put("bodyweight_kg", bodyweightKg)
                if (ageYears != null) put("age_years", ageYears)
                if (restingHrBpm != null) put("resting_hr_bpm", restingHrBpm)
                if (measuredHrMax != null) put("measured_hr_max", measuredHrMax)
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
                })
            })
        }
    }

    data object ClearProfile : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearProfile")
    }

    data class LogSet(
        val exercise: String,
        val weightKg: Double,
        val reps: Int,
        val rpe: Double,
        // Log time, unix seconds. Defaulted to now at construction so the history
        // card can be dated; baked into the persisted line, so replay keeps the
        // original stamp (the core never re-stamps on replay).
        val observedAt: Long = System.currentTimeMillis() / 1000,
        // Stable per-entry id: epoch-millis at log, so it survives
        // edits and never collides with a backdated observed_at. Sent back in
        // AmendSet/DeleteEntry to target this exact set.
        val entryId: Long = System.currentTimeMillis(),
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("LogSet", buildJsonObject {
                put("exercise", exercise)
                put("weight_kg", weightKg)
                put("reps", reps)
                put("rpe", rpe)
                put("observed_at", observedAt)
                put("entry_id", entryId)
            })
        }
    }

    data object ClearSets : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearSets")
    }

    data class LogRun(
        val distanceKm: Double,
        val durationMin: Double,
        val hrPctMax: Double,
        val longestRecentKm: Double,
        val observedAt: Long = System.currentTimeMillis() / 1000,
        val entryId: Long = System.currentTimeMillis(),
        // User-declared run-intent label; null = untagged. Omitted from the
        // wire when null so the event matches serde's `#[serde(default)]` shape.
        val workoutType: WorkoutType? = null,
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
            })
        }
    }

    data class LogRunTrack(
        val points: List<GpsPoint>,
        val hrPctMax: Double,
        val longestRecentKm: Double,
        // The session's logged-at stamp for history display; distinct from each
        // GPS fix's own per-point `observed_at`.
        val observedAt: Long = System.currentTimeMillis() / 1000,
        val entryId: Long = System.currentTimeMillis(),
        // User-declared run-intent label; null = untagged, omitted from wire.
        val workoutType: WorkoutType? = null,
        // Indices into [points] that begin a new recording segment: the pause +
        // relocation boundaries. The core skips each pause-bridge leg
        // (no distance, no time) and breaks the GPX <trkseg> there, keeping the
        // TRUE coordinates. Empty (the common, un-paused run) is OMITTED from the
        // wire so it matches serde's `#[serde(default)]` shape; old-shape parity.
        val segmentStarts: List<Int> = emptyList(),
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
            })
        }
    }

    data object ClearRuns : Event {
        override fun toJson(): JsonElement = JsonPrimitive("ClearRuns")
    }

    /** Which logged family a [DeleteEntry] targets (mirrors app.rs EntryKind). */
    enum class EntryKind { Set, Run }

    /**
     * Delete one logged set or run. The core removes the newest
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
            })
        }
    }

    /** Edit one hand-entered run's fields. GPS-tracked runs are
     *  delete-only, so amending one replaces it with a manual run. */
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
            })
        }
    }

    data class SubmitReview(
        val bonePainRedFlag: Boolean = false,
        val compulsiveFlag: Boolean = false,
        val overtrainingSignalCount: Int = 0,
        val singleSessionSpikeFrac: Double? = null,
        val lift: LiftExec? = null,
        val decoupling: Decouple? = null,
        val easyFracTimeAboveVt1: Double? = null,
        val positiveSplitPct: Double? = null,
        val rpeLoadGapSessions: Int? = null,
        val weeklyVelocityDropMs: Double? = null,
        val failedKeySessions: Int? = null,
        val badDay: Boolean = false,
        // When the review was submitted, unix seconds (backdatable). Baked into
        // the persisted line so replay keeps the original stamp.
        val observedAt: Long = System.currentTimeMillis() / 1000,
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("SubmitReview", buildJsonObject {
                put("bone_pain_red_flag", bonePainRedFlag)
                put("compulsive_flag", compulsiveFlag)
                put("overtraining_signal_count", overtrainingSignalCount)
                if (singleSessionSpikeFrac != null)
                    put("single_session_spike_frac", singleSessionSpikeFrac)
                if (lift != null) put("lift", buildJsonObject {
                    put("reps_met", lift.repsMet)
                    put("rir_actual", lift.rirActual)
                    put("rir_target", lift.rirTarget)
                })
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
            })
        }
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
    ) : Event {
        override fun toJson() = buildJsonObject {
            put("PlanHypertrophyMeso", buildJsonObject {
                put("muscle", muscle)
                put("weeks", weeks)
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

    /** The shell's clock as event data: today's epoch-day, sent on
     *  foreground so the core dates the week + picks the next session. Also carries
     *  the device's current UTC offset in seconds EAST of UTC so the core can
     *  match a logged session's UTC `observed_at` to the correct LOCAL day. */
    data class SetToday(val epochDay: Long, val utcOffsetSec: Int = 0) : Event {
        override fun toJson(): JsonElement = buildJsonObject {
            put("SetToday", buildJsonObject {
                put("epoch_day", epochDay)
                put("utc_offset_sec", utcOffsetSec)
            })
        }
    }
}

data class LiftExec(val repsMet: Boolean, val rirActual: Int, val rirTarget: Int)

/** Aerobic-decoupling context for a run review. Mirrors shared/src/app.rs::Decouple. */
data class Decouple(val driftPct: Double, val coolSteadyContext: Boolean)

/** One GPS fix. Mirrors shared/src/running.rs::GpsPoint. */
data class GpsPoint(
    val lat: Double,
    val lon: Double,
    val observedAt: Long,
    val accuracyM: Double,
)
