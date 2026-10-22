package app.milestone

import android.content.Context
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.relocation.BringIntoViewRequester
import androidx.compose.foundation.relocation.bringIntoViewRequester
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.layout.LayoutCoordinates
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInWindow
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import java.io.Serializable
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive

/**
 * The three training modalities the Focus row (and the guided setup) map onto the
 * volume fields: Lift = weekly_sets>0 & no running; Run = running>0 & no lifting;
 * Both = both sides carry volume. A short enum → rendered as segmented buttons.
 */
enum class Focus { Lift, Run, Both }

internal val Focus.labelRes: Int
    get() = when (this) {
        Focus.Lift -> R.string.setup_focus_lift
        Focus.Run -> R.string.setup_focus_run
        Focus.Both -> R.string.setup_focus_both
    }

/**
 * Durable stash of one side's answers while that side is switched OFF, so a later
 * Focus switch back resumes (or seeds the guided setup with) the user's prior
 * goal, days and weekdays instead of a blank. Shell chrome (not coaching state) →
 * SharedPreferences, not the crux event log.
 */
internal object ModalityStash {
    private const val PREFS = "milestone_modality"
    private const val K_DAYS = "stash_running_days"
    // Raw Double bits: a Float round-trip would re-send 24.1401 as 24.140100479….
    private const val K_KM_BITS = "stash_running_km_bits"
    private const val K_RUN_GOAL = "stash_running_goal"
    private const val K_RACE_DAY = "stash_running_race_day"
    private const val K_RUN_WEEKDAYS = "stash_running_weekdays"
    private const val K_LIFT_DAYS = "stash_lift_days"
    private const val K_LIFT_GOAL = "stash_lift_goal"
    private const val K_LIFT_SETS = "stash_lift_sets"
    private const val K_LIFT_WEEKDAYS = "stash_lift_weekdays"
    // A stored weekday mask of "Any days" (null on the draft).
    private const val ANY_WEEKDAYS = -1

    private fun p(ctx: Context) = ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    /** Keep [draft]'s running answers as running is switched off. */
    fun stashRun(ctx: Context, draft: ProfileDraft) {
        // A side already at 0 has nothing worth keeping: writing it would
        // clobber the last real numbers.
        if (draft.runningDaysPerWeek <= 0) return
        val km = draft.runningKmPerWeek.takeIf { draft.runningKmPerWeekSet && it > 0.0 }
        p(ctx).edit().apply {
            putInt(K_DAYS, draft.runningDaysPerWeek)
            if (km != null) putLong(K_KM_BITS, km.toRawBits()) else remove(K_KM_BITS)
            putString(K_RUN_GOAL, draft.goalDistance.name)
            draft.goalRaceEpochDay?.let { putLong(K_RACE_DAY, it) } ?: remove(K_RACE_DAY)
            putInt(K_RUN_WEEKDAYS, draft.trainingWeekdays ?: ANY_WEEKDAYS)
        }.apply()
    }

    /** Keep [draft]'s lifting answers as lifting is switched off. */
    fun stashLift(ctx: Context, draft: ProfileDraft) {
        if (draft.weeklySets <= 0) return
        p(ctx).edit().apply {
            putInt(K_LIFT_SETS, draft.weeklySets)
            putString(K_LIFT_GOAL, draft.liftGoal.name)
            draft.liftDaysPerWeek?.let { putInt(K_LIFT_DAYS, it) } ?: remove(K_LIFT_DAYS)
            putInt(K_LIFT_WEEKDAYS, draft.trainingWeekdays ?: ANY_WEEKDAYS)
        }.apply()
    }

    /** The stashed running answers, or null when running was never switched off. */
    fun run(ctx: Context): StashedRun? = p(ctx).takeIf { it.contains(K_DAYS) }?.let { s ->
        StashedRun(
            days = s.getInt(K_DAYS, 0),
            km = if (s.contains(K_KM_BITS)) Double.fromBits(s.getLong(K_KM_BITS, 0L)) else null,
            goal = s.getString(K_RUN_GOAL, null)?.let { g -> GoalDistance.entries.firstOrNull { it.name == g } },
            raceEpochDay = if (s.contains(K_RACE_DAY)) s.getLong(K_RACE_DAY, 0L) else null,
            weekdays = weekdaysOf(s.getInt(K_RUN_WEEKDAYS, ANY_WEEKDAYS)),
        )
    }

    /** The stashed lifting answers, or null when lifting was never switched off. */
    fun lift(ctx: Context): StashedLift? = p(ctx).let { s ->
        val goal = s.getString(K_LIFT_GOAL, null)?.let { g -> LiftGoal.entries.firstOrNull { it.name == g } }
        if (goal == null || !s.contains(K_LIFT_SETS)) return null
        StashedLift(
            days = if (s.contains(K_LIFT_DAYS)) s.getInt(K_LIFT_DAYS, 0) else null,
            goal = goal,
            weeklySets = s.getInt(K_LIFT_SETS, 0),
            weekdays = weekdaysOf(s.getInt(K_LIFT_WEEKDAYS, ANY_WEEKDAYS)),
        )
    }

    private fun weekdaysOf(mask: Int): Int? = mask.takeIf { it != ANY_WEEKDAYS && it and ALL_WEEKDAYS != 0 }
}

/** Running answers kept while running is off ([km]/[goal]/[raceEpochDay] null =
 *  never given; [weekdays] null = "Any days"). */
internal data class StashedRun(val days: Int, val km: Double?, val goal: GoalDistance?, val raceEpochDay: Long?, val weekdays: Int?)

/** Lifting answers kept while lifting is off ([days] null = never answered). */
internal data class StashedLift(val days: Int?, val goal: LiftGoal, val weeklySets: Int, val weekdays: Int?)

/**
 * The profile switched back to a single side the user already set up, when the
 * stash holds every answer that side's setup would ask (its goal and days, with the
 * stashed weekdays still covering them); the other side is dropped as
 * [withoutSide] does. Null = ask in the guided setup. Both always asks (which side
 * leads is never stashed).
 */
internal fun ProfileDraft.resumedSide(target: Focus, lift: StashedLift?, run: StashedRun?, today: Long): ProfileDraft? = when (target) {
    Focus.Lift -> lift?.takeIf { s ->
        weeklySets == 0 && s.weeklySets > 0 && s.days != null && s.days in LIFT_DAYS && weekdaysCover(s.weekdays, s.days)
    }?.let { s ->
        copy(weeklySets = s.weeklySets, liftDaysPerWeek = s.days, liftGoal = s.goal, trainingWeekdays = s.weekdays)
            .withoutSide(Focus.Lift)
    }
    Focus.Run -> run?.takeIf { s ->
        runningDaysPerWeek == 0 && s.goal != null && s.days in dayOptions(Modality.Run) && weekdaysCover(s.weekdays, s.days)
    }?.let { s ->
        val goal = s.goal ?: GoalDistance.General
        copy(
            runningDaysPerWeek = s.days,
            runningKmPerWeek = s.km ?: 0.0,
            runningKmPerWeekSet = s.km != null,
            goalDistance = goal,
            goalRaceEpochDay = s.raceEpochDay?.takeIf { it > today && goal.isRace },
            liftDaysPerWeek = null,
            trainingWeekdays = s.weekdays,
        ).withoutSide(Focus.Run)
    }
    Focus.Both -> null
}

/**
 * A Both profile's running level as the user answered it. The core stores only
 * what that answer drives (`advanced` = Advanced), so the answer itself is kept
 * here for the setup seed and Profile › Running experience, trusted only while it
 * still agrees with that flag.
 */
internal object RunLevelAnswer {
    private const val PREFS = "milestone_run_level"
    private const val K_LEVEL = "run_level"

    private fun p(ctx: Context) = ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    fun save(ctx: Context, level: SetupLevel) {
        p(ctx).edit().putString(K_LEVEL, level.name).apply()
    }

    /** The recorded level, or null when none is recorded or it contradicts [advanced]. */
    fun read(ctx: Context, advanced: Boolean): SetupLevel? =
        p(ctx).getString(K_LEVEL, null)
            ?.let { n -> SetupLevel.entries.firstOrNull { it.name == n } }
            ?.takeIf { (it == SetupLevel.Advanced) == advanced }
}

/** The Profile rows with an inline picker; at most one is open at a time. The
 *  entries after [Break] are reveal targets only (a hold banner's deep link): a
 *  PAR-Q+ answer inside the health questions, or a health switch row. */
internal enum class ProfileRowId {
    TrainingDays, Experience, RunExperience, PriorityMuscle, Meet, Equipment, WeeklySets,
    GoalDistance, RaceDate, RecentRace, WeeklyDistance, LongestRun,
    Sex, Age, Bodyweight, RestingHr, MaxHr, HealthQuestions,
    Environment, EnvTemp, EnvAltitude, Break,
    ParqSymptom, ParqInjury, ParqClearance, Ill, Pregnant, PregnancyWarning, Reds,
    ;

    /** The row whose picker a focus on this id opens: a PAR-Q+ answer opens the
     *  health questions; a switch row has no picker. */
    val pickerRow: ProfileRowId?
        get() = when (this) {
            ParqSymptom, ParqInjury, ParqClearance -> HealthQuestions
            Ill, Pregnant, PregnancyWarning, Reds -> null
            else -> this
        }
}

/** The PAR-Q+ question a focus on [id] reveals inside the open health
 *  questions: the first symptom answered Yes, or the bone/joint answer. Null when
 *  [id] names no question or no such Yes is recorded (a legacy flag). */
internal fun parqRevealItem(id: ProfileRowId?, health: HealthScreen): ParqItem? = when (id) {
    ProfileRowId.ParqSymptom -> ParqItem.entries.firstOrNull { it.symptom && health.parqYes(it) }
    ProfileRowId.ParqInjury -> ParqItem.BoneOrJoint.takeIf { health.parqYes(it) }
    else -> null
}

/**
 * Plain data holder for the [Event.SetProfile] fields. Kept separate from the
 * event so the editor can mutate a draft locally and emit a `SetProfile` per
 * committed change (the event log compacts SetProfile last-write-wins, so
 * apply-on-change costs nothing at replay).
 */
data class ProfileDraft(
    val progressionCadence: ProgressionCadence,
    val liftGoal: LiftGoal,
    val goalDistance: GoalDistance,
    val concurrentGoal: ConcurrentGoal,
    val weeklySets: Int,
    val runningDaysPerWeek: Int,
    val runningKmPerWeek: Double,
    // False while the weekly distance was never given: runningKmPerWeek is then
    // 0.0 on the wire and the engine picks its own conservative start.
    val runningKmPerWeekSet: Boolean = true,
    val advanced: Boolean,
    // Consolidated person data. Carried on the draft so every
    // SetProfile the editor emits re-sends it; otherwise a training-field edit
    // would last-write-wins away the person data set in the guided setup.
    // Null until the user answers, so the editor's Sex row can read "Not set"
    // instead of silently defaulting to Male; an answered Male is sent as an
    // explicit false so it survives the round trip.
    val female: Boolean? = null,
    val bodyweightKg: Double? = null,
    // With a birthday, the core's derived age (echoed each day); without one, a
    // legacy stated age.
    val ageYears: Double? = null,
    val restingHrBpm: Double? = null,
    val measuredHrMax: Double? = null,
    // Display-unit preference. Carried on the draft so every SetProfile
    // re-sends it; otherwise any profile edit would last-write-wins the unit
    // back to kg.
    val weightLb: Boolean = false,
    // Stage-0 onboarding health screen. Carried on the draft so every
    // SetProfile re-sends it (else a training-field edit would last-write-wins
    // away the health gates). youth is core-derived from age; never set here.
    val health: HealthScreen = HealthScreen(),
    // Resistance-training modality: barbell/machine "Weights" (default) or
    // bodyweight "Calisthenics". Carried on the draft so every SetProfile
    // re-sends it; a Weights value emits nothing (matching the legacy wire).
    val resistanceModality: ResistanceModality = ResistanceModality.Weights,
    // Advanced training-context fields (mirror app::Profile). Carried on the
    // draft so every SetProfile re-sends them (else a training-field edit would
    // last-write-wins them away). All default to unset so a plain profile emits
    // the legacy wire.
    val highLoadBlock: Boolean = false,
    val environment: Environment? = null,
    val envTempC: Double? = null,
    val envAltitudeM: Double? = null,
    val weeksOff: Double? = null,
    // Coaching inputs (mirror app::Profile). Carried on the draft so every
    // SetProfile re-sends them; null/false = unanswered (legacy wire).
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
    // Birthday, local epoch day; the source of ageYears once set.
    val birthEpochDay: Long? = null,
) : Serializable {
    /** [effectiveFromEpochDay] is the day this profile takes effect:
     *  pass the core's today so an edit re-prescribes only from today forward and
     *  leaves settled past days alone. Null (the default) replaces the whole
     *  profile history — used by tests, never a production edit. */
    fun toEvent(effectiveFromEpochDay: Long? = null) = Event.SetProfile(
        progressionCadence = progressionCadence,
        liftGoal = liftGoal,
        goalDistance = goalDistance,
        concurrentGoal = concurrentGoal,
        weeklySets = weeklySets,
        runningDaysPerWeek = runningDaysPerWeek,
        runningKmPerWeek = runningKmPerWeek,
        runningKmPerWeekSet = runningKmPerWeekSet,
        advanced = advanced,
        female = female,
        bodyweightKg = bodyweightKg,
        ageYears = ageYears,
        restingHrBpm = restingHrBpm,
        measuredHrMax = measuredHrMax,
        weightLb = weightLb,
        health = health,
        resistanceModality = resistanceModality,
        highLoadBlock = highLoadBlock,
        environment = environment,
        envTempC = envTempC,
        envAltitudeM = envAltitudeM,
        weeksOff = weeksOff,
        liftDaysPerWeek = liftDaysPerWeek,
        trainingWeekdays = trainingWeekdays,
        goalRaceEpochDay = goalRaceEpochDay,
        recentRace = recentRace,
        statedLongestRecentKm = statedLongestRecentKm,
        layoffReturnedEpochDay = layoffReturnedEpochDay,
        meetEpochDay = meetEpochDay,
        calisEquipment = calisEquipment,
        specializationMuscle = specializationMuscle,
        runnerStrength = runnerStrength,
        effectiveFromEpochDay = effectiveFromEpochDay,
        birthEpochDay = birthEpochDay,
    )

    companion object {
        /**
         * Rebuild a draft from the core-echoed profile (post log-replay hydration).
         * Runs on the first frame, so an unknown enum name (a forward-incompatible
         * log line, or a future core/shell variant divergence) must fall back to a
         * SEED default rather than throwing `valueOf` into a launch crash: the same
         * drop-don't-crash stance the FFI layer takes on replay.
         */
        fun from(p: ProfileView) = ProfileDraft(
            progressionCadence = enumOr(p.progression_cadence, SEED.progressionCadence),
            liftGoal = enumOr(p.lift_goal, SEED.liftGoal),
            goalDistance = enumOr(p.goal_distance, SEED.goalDistance),
            concurrentGoal = enumOr(p.concurrent_goal, SEED.concurrentGoal),
            weeklySets = p.weekly_sets,
            runningDaysPerWeek = p.running_days_per_week,
            runningKmPerWeek = p.running_km_per_week,
            runningKmPerWeekSet = p.running_km_per_week_set,
            advanced = p.advanced,
            female = p.female,
            bodyweightKg = p.bodyweight_kg,
            ageYears = p.age_years,
            restingHrBpm = p.resting_hr_bpm,
            measuredHrMax = p.measured_hr_max,
            weightLb = p.weight_lb,
            // Drop the core-DERIVED youth flag from the editable draft so a later
            // edit never re-asserts it (youth is owned by the core, keyed off age).
            health = p.health.copy(youth = false),
            // Values are "Weights"|"Calisthenics", matching the enum names exactly.
            resistanceModality = enumOr(p.resistance_modality, SEED.resistanceModality),
            highLoadBlock = p.high_load_block,
            // Bare Rust variant name → enum; an unknown/absent value falls back to
            // unset (same drop-don't-crash stance as enumOr above).
            environment = p.environment?.let { runCatching { enumValueOf<Environment>(it) }.getOrNull() },
            envTempC = p.env_temp_c,
            envAltitudeM = p.env_altitude_m,
            weeksOff = p.weeks_off,
            liftDaysPerWeek = p.lift_days_per_week,
            trainingWeekdays = p.training_weekdays?.takeIf { it and ALL_WEEKDAYS != 0 },
            goalRaceEpochDay = p.goal_race_epoch_day,
            recentRace = p.recent_race,
            statedLongestRecentKm = p.stated_longest_recent_km,
            layoffReturnedEpochDay = p.layoff_returned_epoch_day,
            meetEpochDay = p.meet_epoch_day,
            calisEquipment = p.calis_equipment,
            specializationMuscle = p.specialization_muscle,
            runnerStrength = p.runner_strength,
            birthEpochDay = p.birth_epoch_day,
        )

        private inline fun <reified T : Enum<T>> enumOr(name: String, default: T): T =
            runCatching { enumValueOf<T>(name) }.getOrDefault(default)

        /**
         * Fallback starting point for the full editor only (rehydration when no
         * profile is echoed). Never seeded on a fresh install: first run goes
         * through the guided setup, which writes user-asserted values rather than
         * these opinionated defaults.
         */
        val SEED = ProfileDraft(
            progressionCadence = ProgressionCadence.WeekToWeek,
            liftGoal = LiftGoal.MaxStrength,
            goalDistance = GoalDistance.TenK,
            concurrentGoal = ConcurrentGoal.Strength,
            weeklySets = 14,
            runningDaysPerWeek = 4,
            runningKmPerWeek = 45.0,
            advanced = false,
            resistanceModality = ResistanceModality.Weights,
        )
    }
}

// Human-readable labels for the profile pickers. Display-only, the wire value is
// still each variant's `.name` (see Event.SetProfile), so these strings can change
// freely. Exhaustive `when` (no `else`) so a new variant fails to compile until it
// is given a label rather than silently showing a cryptic raw name.

/** The one lifting-goal vocabulary: the Profile Training goal segments, the
 *  wizard's lift-goal cards and the review row all read these labels. */
internal val LiftGoal.labelRes: Int
    get() = when (this) {
        LiftGoal.MaxStrength -> R.string.setup_liftgoal_strength
        LiftGoal.Power -> R.string.setup_liftgoal_power
        LiftGoal.Hypertrophy -> R.string.setup_liftgoal_muscle
    }

internal val GoalDistance.labelRes: Int
    get() = when (this) {
        GoalDistance.General -> R.string.setup_goaldist_general
        GoalDistance.C25k -> R.string.setup_goaldist_c25k
        GoalDistance.FiveK -> R.string.setup_goaldist_5k
        GoalDistance.TenK -> R.string.setup_goaldist_10k
        GoalDistance.HalfMarathon -> R.string.setup_goaldist_half
        GoalDistance.Marathon -> R.string.setup_goaldist_marathon
    }

/**
 * Change the lift goal and keep the hybrid "which leads" answer coherent: while
 * lifting leads (and always for a lift-only profile, where the row is hidden) the
 * concurrent goal follows the new lift goal; a run-led hybrid stays run-led.
 */
internal fun ProfileDraft.withLiftGoal(goal: LiftGoal): ProfileDraft {
    val runLeads = weeklySets > 0 && runningDaysPerWeek > 0 &&
        concurrentGoal == ConcurrentGoal.EndurancePriority
    return copy(
        liftGoal = goal,
        concurrentGoal = if (runLeads) ConcurrentGoal.EndurancePriority else goal.toConcurrent(),
    )
}

/** The two-way "Which leads?" answer mapped onto the concurrent goal (same mapping
 *  the guided setup uses). */
internal fun ProfileDraft.withLead(lead: BothPriority): ProfileDraft =
    copy(concurrentGoal = leadingConcurrentGoal(lead, liftGoal))

internal val Environment.labelRes: Int
    get() = when (this) {
        Environment.Heat -> R.string.setup_env_heat
        Environment.Altitude -> R.string.setup_env_altitude
        Environment.Cold -> R.string.setup_env_cold
        Environment.Neutral -> R.string.setup_env_neutral
    }

/**
 * User-editable training profile. Drives the `SetProfile` event, so all
 * evidence-cited guidance is user-driven.
 *
 * Applies on every change: each committed edit immediately calls [onApply] with
 * the new draft, so a picker tap or the Advanced switch takes effect the moment
 * it is touched, with no separate "Apply" step to forget. The event log compacts
 * `SetProfile` last-write-wins, so per-tap events never bloat the replay.
 *
 * [distanceUnit] is the resolved km|mi display unit: the running-distance rows
 * show and pick in it, while the wire stays canonical km. [today] (the core's
 * local epoch day) bounds the date rows: race/meet dates are future-only, the
 * restart date is today or earlier. [onRerunSetup] opens the guided setup seeded
 * from this profile: null = the plain "Re-run guided setup" row at the end of the
 * Plan card; a [Modality] = a Focus switch that needs the new side's answers.
 * [focusRow] (e.g. a hold banner's "Check age") expands that row and brings it
 * into view once; [onFocusHandled] tells the caller the request was taken.
 */
@Composable
internal fun ProfileEditor(
    initial: ProfileDraft,
    distanceUnit: DistanceUnit,
    today: Long,
    onRerunSetup: (Modality?) -> Unit,
    focusRow: ProfileRowId? = null,
    onFocusHandled: () -> Unit = {},
    onApply: (ProfileDraft) -> Unit,
) {
    val ctx = LocalContext.current
    // rememberSaveable, not remember: this editor lives in a LazyColumn item, so it
    // is disposed when scrolled out of view. Plain remember would discard the draft
    // on scroll-away; the saveable draft survives that disposal (and process death /
    // rotation). ProfileDraft is Serializable so the autoSaver can bundle it.
    //
    // KEYED on `initial` (SAFETY / HARD RULE 3): an unkeyed saveable outlives the
    // incoming profile, so after "Re-run guided setup", which rewrites the core
    // profile, including the health gates, the stale pre-setup draft would survive
    // and, on the next field edit (apply-on-change re-sends every field via
    // last-write-wins SetProfile), silently revert the just-set values. That could
    // re-raise a just-cleared medical deferral or drop a just-raised gate. Keying on
    // the incoming profile identity invalidates the draft the moment the core echoes
    // a different profile, so a fresh setup always wins. Normal apply-on-change edits
    // round-trip (`from(toEvent())` is stable), so the key is unchanged there and the
    // draft is NOT reset mid-edit.
    var draft by rememberSaveable(initial) { mutableStateOf(initial) }
    // Commit-and-apply: the single funnel every edit goes through.
    val commit: (ProfileDraft) -> Unit = {
        draft = it
        onApply(it)
    }

    // Which row's inline picker is open. One at a time; null = all collapsed.
    // Saveable so an open picker survives scroll-away / rotation like the draft.
    var open by rememberSaveable { mutableStateOf<ProfileRowId?>(null) }
    val toggle: (ProfileRowId) -> Unit = { id -> open = if (open == id) null else id }
    // A row asked for from outside (a hold banner): its picker (if any) expanded
    // now, the row itself scrolled to once it has laid out.
    var reveal by remember { mutableStateOf<ProfileRowId?>(null) }
    LaunchedEffect(focusRow) {
        if (focusRow != null) {
            open = focusRow.pickerRow
            reveal = focusRow
            onFocusHandled()
        }
    }

    // Modality derivation: which sides carry volume drives which rows and groups show.
    val lift = draft.weeklySets > 0
    val run = draft.runningDaysPerWeek > 0
    val both = lift && run
    val focus = when {
        both -> Focus.Both
        run -> Focus.Run
        else -> Focus.Lift
    }

    // A Focus change reprograms the week, so it is confirmed first. Turning a side
    // ON needs that side's answers: a side set up before resumes from its stash
    // when every answer is there, else the guided setup asks them, seeded. Only
    // dropping a side applies here otherwise. Nothing invented is ever written.
    var pendingFocus by rememberSaveable { mutableStateOf<Focus?>(null) }
    pendingFocus?.let { target ->
        val addsSide = (target != Focus.Run && !lift) || (target != Focus.Lift && !run)
        val resumed = remember(target, draft) {
            if (addsSide) draft.resumedSide(target, ModalityStash.lift(ctx), ModalityStash.run(ctx), today) else null
        }
        val copy = focusSwitchCopy(focus, target)
        val title = if (resumed != null) {
            stringResource(if (target == Focus.Lift) R.string.setup_focus_resume_lift_title else R.string.setup_focus_resume_run_title)
        } else {
            stringResource(copy.titleRes)
        }
        val body = if (resumed != null) {
            val days = pluralStringResource(R.plurals.setup_days_count, resumed.trainingDayTotal(), resumed.trainingDayTotal())
            if (target == Focus.Lift) {
                stringResource(R.string.setup_focus_resume_lift_body, stringResource(resumed.liftGoal.labelRes), days)
            } else {
                stringResource(R.string.setup_focus_resume_run_body, stringResource(resumed.goalDistance.labelRes), days)
            }
        } else {
            stringResource(copy.bodyRes)
        }
        AlertDialog(
            onDismissRequest = { pendingFocus = null },
            shape = RoundedCornerShape(Space.Card.dp),
            title = { Text(title) },
            text = { Text(body) },
            confirmButton = {
                TextButton(onClick = {
                    pendingFocus = null
                    open = null
                    // A side switched off: keep its answers for a later switch back.
                    if (target == Focus.Lift && run) ModalityStash.stashRun(ctx, draft)
                    if (target == Focus.Run && lift) ModalityStash.stashLift(ctx, draft)
                    when {
                        resumed != null -> commit(resumed)
                        addsSide -> onRerunSetup(target.modality)
                        else -> commit(draft.withoutSide(target))
                    }
                }) {
                    Text(stringResource(if (resumed != null) R.string.setup_focus_switch_confirm else copy.confirmRes))
                }
            },
            dismissButton = {
                TextButton(onClick = { pendingFocus = null }) { Text(stringResource(R.string.main_cancel)) }
            },
        )
    }

    // One accent-labelled card per group. The Plan rows come first for every
    // Focus, so a row never moves when the modality changes.
    Column(verticalArrangement = Arrangement.spacedBy(Space.Screen.dp)) {
        ProfileGroup(stringResource(R.string.setup_section_plan)) {
            // Focus: a short enum → segmented buttons; a change is confirmed first
            // (re-tapping the active segment does nothing).
            SegmentedEnumRow(
                label = stringResource(R.string.setup_row_focus),
                values = listOf(Focus.Lift, Focus.Run, Focus.Both),
                current = focus,
                display = { ctx.getString(it.labelRes) },
                onSelect = { if (it != focus) pendingFocus = it },
            )
            RowHairline()
            // Which leads: Both only. A two-way choice (the core only reads
            // lift-first vs run-first); lifting leading means the lift goal itself
            // leads, so it can never contradict Training goal.
            if (both) {
                SegmentedEnumRow(
                    label = stringResource(R.string.setup_row_which_leads),
                    values = BothPriority.entries.toList(),
                    current = draft.concurrentGoal.lead(),
                    display = {
                        ctx.getString(
                            when (it) {
                                BothPriority.LiftLean -> R.string.setup_lead_lifting
                                BothPriority.RunLean -> R.string.setup_lead_running
                            },
                        )
                    },
                ) { commit(draft.withLead(it)) }
                RowHairline()
            }
            // Training days: the ONE days control. The value is the week's
            // session days; the caption carries the Both split and the weekdays
            // ("2 lift · 3 run · Mon, Tue, …"), so the bold value stays one line.
            // The picker sets the count per side and the weekdays.
            val dayTotal = draft.trainingDayTotal()
            val daysCaption = listOfNotNull(
                if (both) stringResource(R.string.setup_days_split_sub, draft.shownLiftDays(), draft.runningDaysPerWeek) else null,
                weekdaysLine(draft.trainingWeekdays, dayTotal),
            ).joinToString(" · ")
            ProfileRow(
                open == ProfileRowId.TrainingDays,
                { toggle(ProfileRowId.TrainingDays) },
                stringResource(R.string.setup_row_weekdays),
                pluralStringResource(R.plurals.setup_days_count, dayTotal, dayTotal),
                subtitle = daysCaption,
            ) {
                // The setup's rule: the ticked weekdays must cover every training
                // day (or be "Any days"). Picks stage here and apply only once
                // they agree; collapsing the row drops a mismatched pick.
                var work by rememberSaveable(draft) { mutableStateOf(draft) }
                val stage: (ProfileDraft) -> Unit = { next ->
                    work = next
                    if (weekdaysCover(next.trainingWeekdays, next.trainingDayTotal())) commit(next)
                }
                val workTotal = work.trainingDayTotal()
                Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                    if (lift) {
                        // 2–5 for a lifting plan; a run-led hybrid lifts at most 3
                        // (the engine holds lifting to maintenance there).
                        val liftLabel = stringResource(if (both) R.string.setup_row_lift_days else R.string.setup_days_scale_label)
                        val liftMax = if (both && work.concurrentGoal == ConcurrentGoal.EndurancePriority) HYBRID_CAPPED_DAYS else LIFT_DAYS.last
                        FieldLabel(liftLabel)
                        ChoiceScaleRow((LIFT_DAYS.first..liftMax).toList(), work.shownLiftDays(), { "$it" }, label = liftLabel) {
                            stage(work.copy(liftDaysPerWeek = it))
                        }
                    }
                    if (run) {
                        // Starts at 1, not 0: dropping running is the Focus row's job.
                        // While lifting leads a hybrid runs at most 3 days.
                        val runLabel = stringResource(if (both) R.string.setup_row_running_days else R.string.setup_days_scale_label)
                        val runMax = if (both && work.concurrentGoal != ConcurrentGoal.EndurancePriority) HYBRID_CAPPED_DAYS else WEEK_DAYS
                        FieldLabel(runLabel)
                        ChoiceScaleRow((1..runMax).toList(), work.runningDaysPerWeek, { "$it" }, label = runLabel) {
                            stage(work.copy(runningDaysPerWeek = it))
                        }
                    }
                    FieldLabel(stringResource(R.string.setup_weekdays_q))
                    WeekdayChips(work.trainingWeekdays) { stage(work.copy(trainingWeekdays = it)) }
                    if (!weekdaysCover(work.trainingWeekdays, workTotal)) {
                        Text(
                            pluralStringResource(R.plurals.setup_days_weekdays_hint, workTotal, workTotal),
                            color = LocalStatusColors.current.danger,
                            style = Type.Caption,
                        )
                    }
                }
            }
            RowHairline()
            // Experience: the same three answers and wording as the guided setup's
            // level step (progression cadence, training age, starting volumes). A
            // Both profile answers it per side, so it shows (and edits) both.
            val level = SetupLevel.fromCadence(draft.progressionCadence)
            ProfileRow(
                open == ProfileRowId.Experience,
                { toggle(ProfileRowId.Experience) },
                stringResource(if (both) R.string.setup_row_lift_experience else R.string.setup_row_experience),
                stringResource(level.titleRes),
            ) {
                OptionList(SetupLevel.entries.toList(), level, { ctx.getString(it.titleRes) }) {
                    commit(draft.copy(progressionCadence = it.cadence))
                    open = null
                }
            }
            if (both) {
                RowHairline()
                // The running level drives only `advanced` in the core; the answer
                // itself is the shell's record, shown while it agrees with that flag.
                var runLevel by remember(draft.advanced) { mutableStateOf(RunLevelAnswer.read(ctx, draft.advanced)) }
                val shownRunLevel = runLevel ?: SetupLevel.Advanced.takeIf { draft.advanced }
                ProfileRow(
                    open == ProfileRowId.RunExperience,
                    { toggle(ProfileRowId.RunExperience) },
                    stringResource(R.string.setup_row_run_experience),
                    shownRunLevel?.let { stringResource(it.titleRes) } ?: stringResource(R.string.setup_not_set),
                ) {
                    OptionList<SetupLevel?>(SetupLevel.entries.toList(), shownRunLevel, { l -> l?.let { ctx.getString(it.titleRes) }.orEmpty() }) { l ->
                        if (l != null) {
                            RunLevelAnswer.save(ctx, l)
                            runLevel = l
                            commit(draft.copy(advanced = l == SetupLevel.Advanced))
                            open = null
                        }
                    }
                }
            }
            RowHairline()
            ActionRow(stringResource(R.string.setup_rerun_setup)) { onRerunSetup(null) }
        }

        if (lift) {
            ProfileGroup(stringResource(R.string.setup_section_lifting)) {
                // Training goal: a short enum → segmented buttons, in the wizard's
                // order and with its labels; the caption glosses the terms.
                SegmentedEnumRow(
                    label = stringResource(R.string.setup_row_training_goal),
                    values = LiftFocus.entries.map { it.liftGoal },
                    current = draft.liftGoal,
                    display = { ctx.getString(it.labelRes) },
                ) { commit(draft.withLiftGoal(it)) }
                Text(stringResource(R.string.setup_row_training_goal_sub), color = OnBgFaint, style = Type.Caption)
                RowHairline()
                SegmentedEnumRow(
                    label = stringResource(R.string.setup_modality_strength_q),
                    values = ResistanceModality.entries.toList(),
                    current = draft.resistanceModality,
                    display = {
                        ctx.getString(
                            when (it) {
                                ResistanceModality.Weights -> R.string.setup_modality_weights_short
                                ResistanceModality.Calisthenics -> R.string.setup_modality_calis_short
                            },
                        )
                    },
                ) { commit(draft.copy(resistanceModality = it)) }
                RowHairline()
                if (draft.liftGoal == LiftGoal.Hypertrophy) {
                    // Optional specialization; the core applies it only for
                    // experienced lifters (HYP-SPEC-BLOCK-001).
                    ProfileRow(
                        open == ProfileRowId.PriorityMuscle,
                        { toggle(ProfileRowId.PriorityMuscle) },
                        stringResource(R.string.setup_row_priority_muscle),
                        draft.specializationMuscle?.let { m -> PRIORITY_MUSCLES.firstOrNull { it.first == m }?.let { stringResource(it.second) } ?: m },
                        placeholder = stringResource(R.string.setup_priority_none),
                    ) {
                        OptionList(
                            listOf<String?>(null) + PRIORITY_MUSCLES.map { it.first },
                            draft.specializationMuscle,
                            { key -> ctx.getString(PRIORITY_MUSCLES.firstOrNull { it.first == key }?.second ?: R.string.setup_priority_none) },
                        ) {
                            commit(draft.copy(specializationMuscle = it))
                            open = null
                        }
                    }
                } else {
                    // A test/meet date shapes the taper and peak.
                    ProfileRow(
                        open == ProfileRowId.Meet,
                        { toggle(ProfileRowId.Meet) },
                        stringResource(R.string.setup_row_meet),
                        draft.meetEpochDay?.let { epochDayLabel(it) },
                        placeholder = stringResource(R.string.setup_date_none),
                    ) {
                        DateAnswerRow(draft.meetEpochDay?.takeIf { it > today }, today, future = true, noneLabel = stringResource(R.string.setup_date_none)) {
                            commit(draft.copy(meetEpochDay = it))
                        }
                    }
                }
                if (draft.resistanceModality == ResistanceModality.Calisthenics) {
                    RowHairline()
                    ProfileRow(
                        open == ProfileRowId.Equipment,
                        { toggle(ProfileRowId.Equipment) },
                        stringResource(R.string.setup_row_equipment),
                        draft.calisEquipment?.let { equipmentText(it) },
                    ) {
                        EquipmentChips(draft.calisEquipment) { commit(draft.copy(calisEquipment = it)) }
                    }
                }
                RowHairline()
                val setsLabel = stringResource(R.string.setup_row_weekly_sets)
                ProfileRow(open == ProfileRowId.WeeklySets, { toggle(ProfileRowId.WeeklySets) }, setsLabel, "${draft.weeklySets}") {
                    ScrollableScaleRow(WEEKLY_SETS_OPTIONS.toList(), draft.weeklySets, { "$it" }, label = setsLabel) {
                        commit(draft.copy(weeklySets = it))
                    }
                }
            }
        }

        if (run) {
            ProfileGroup(stringResource(R.string.setup_section_running)) {
                ProfileRow(
                    open == ProfileRowId.GoalDistance,
                    { toggle(ProfileRowId.GoalDistance) },
                    stringResource(R.string.setup_row_goal_distance),
                    stringResource(draft.goalDistance.labelRes),
                ) {
                    OptionList(GoalDistance.entries.toList(), draft.goalDistance, { ctx.getString(it.labelRes) }) {
                        commit(draft.copy(goalDistance = it))
                        open = null
                    }
                }
                if (draft.goalDistance.isRace) {
                    RowHairline()
                    ProfileRow(
                        open == ProfileRowId.RaceDate,
                        { toggle(ProfileRowId.RaceDate) },
                        stringResource(R.string.setup_row_race_date),
                        draft.goalRaceEpochDay?.let { epochDayLabel(it) },
                        placeholder = stringResource(R.string.setup_date_none),
                    ) {
                        DateAnswerRow(draft.goalRaceEpochDay?.takeIf { it > today }, today, future = true, noneLabel = stringResource(R.string.setup_date_none)) {
                            commit(draft.copy(goalRaceEpochDay = it))
                        }
                    }
                    RowHairline()
                    ProfileRow(
                        open == ProfileRowId.RecentRace,
                        { toggle(ProfileRowId.RecentRace) },
                        stringResource(R.string.setup_row_recent_race),
                        draft.recentRace?.let { recentRaceText(it, distanceUnit) },
                    ) {
                        RecentRaceEditor(draft.recentRace) { commit(draft.copy(recentRace = it)) }
                    }
                }
                RowHairline()
                // Weekly volume in the user's distance unit (km or mi), the unit in
                // the value: options, highlight and value are whole display units; a
                // pick converts back to canonical km for the wire (mi via
                // miToKmWire), so what is shown is what is stored. Starts at 1, not
                // 0: "I don't run yet" is a setup answer and dropping running is the
                // Focus row's job. Never given → "Not set"; the engine then picks its
                // own conservative start, and "Not sure" returns to that.
                val volumeLabel = stringResource(R.string.setup_row_running_volume)
                val volume = weeklyVolumeScaleValue(draft.runningKmPerWeek, distanceUnit).takeIf { draft.runningKmPerWeekSet }
                ProfileRow(
                    open == ProfileRowId.WeeklyDistance,
                    { toggle(ProfileRowId.WeeklyDistance) },
                    volumeLabel,
                    volume?.let { stringResource(R.string.setup_distance_value, "$it", distanceUnit.distanceLabel) },
                ) {
                    Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                        ScrollableScaleRow(
                            weeklyVolumeScaleOptions(distanceUnit),
                            volume,
                            { "$it" },
                            label = volumeLabel,
                        ) {
                            commit(draft.copy(runningKmPerWeek = weeklyVolumeScaleToKm(it, distanceUnit), runningKmPerWeekSet = true))
                        }
                        AnswerChip(stringResource(R.string.setup_not_sure), selected = !draft.runningKmPerWeekSet) {
                            commit(draft.copy(runningKmPerWeek = 0.0, runningKmPerWeekSet = false))
                        }
                    }
                }
                RowHairline()
                // Longest run in the last month (RUN-SPIKE-001 day-one anchor; logged
                // runs supersede it), in the user's distance unit.
                val longestLabel = stringResource(R.string.setup_row_longest_run)
                ProfileRow(
                    open == ProfileRowId.LongestRun,
                    { toggle(ProfileRowId.LongestRun) },
                    longestLabel,
                    draft.statedLongestRecentKm?.let { stringResource(R.string.setup_distance_value, distanceBufferText(it, distanceUnit), distanceUnit.distanceLabel) },
                    revealContent = true,
                ) {
                    Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                        key(distanceUnit) {
                            ProfileKeypadEditor(
                                longestLabel,
                                distanceUnit.distanceLabel,
                                draft.statedLongestRecentKm?.let { distanceBufferText(it, distanceUnit) },
                                distanceBounds(LONGEST_RUN_KM, distanceUnit),
                                "%.1f",
                                allowDecimal = true,
                            ) { commit(draft.copy(statedLongestRecentKm = displayToKm(it, distanceUnit))) }
                        }
                        // Clears the stated value (the plan then anchors on logged runs).
                        AnswerChip(stringResource(R.string.setup_not_sure), selected = draft.statedLongestRecentKm == null) {
                            commit(draft.copy(statedLongestRecentKm = null))
                        }
                    }
                }
                if (!lift) {
                    // A pure runner can opt into two short strength sessions.
                    RowHairline()
                    LabeledSwitchRow(
                        label = stringResource(R.string.setup_row_runner_strength),
                        subtitle = stringResource(R.string.setup_row_runner_strength_sub),
                        subtitleColor = OnBgFaint,
                        checked = draft.runnerStrength,
                        onCheckedChange = { commit(draft.copy(runnerStrength = it)) },
                    )
                }
                // Advanced: its only consumer in the core is the marathon goal-week
                // plan (coaching depth follows Experience, INDIV-TRAGE-001), so the
                // row shows for a marathon goal, or while it is on so it can be
                // turned off.
                if (draft.goalDistance == GoalDistance.Marathon || draft.advanced) {
                    RowHairline()
                    LabeledSwitchRow(
                        label = stringResource(R.string.setup_row_advanced),
                        subtitle = stringResource(R.string.setup_row_advanced_sub),
                        subtitleColor = OnBgFaint,
                        checked = draft.advanced,
                        onCheckedChange = { commit(draft.copy(advanced = it)) },
                    )
                }
            }
        }

        // You: consolidated person data, entered once here; the protein /
        // HR-zone calculators (Profile → Calculators) prefill from it. All optional: "Not set" until given.
        ProfileGroup(stringResource(R.string.setup_section_you)) {
            ProfileRow(open == ProfileRowId.Sex, { toggle(ProfileRowId.Sex) }, stringResource(R.string.setup_sex_label), sexLabel(draft.female)) {
                Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
                    // Full-width rows (not a segmented cell row) so "Prefer not to
                    // say" never clips at large font sizes; it stores no answer (null).
                    OptionList(
                        listOf<Boolean?>(false, true, null),
                        draft.female,
                        { ctx.getString(sexLabelRes(it)) },
                    ) {
                        commit(draft.copy(female = it))
                        open = null
                    }
                    Text(stringResource(R.string.setup_sex_caption), color = OnBgMuted, style = Type.Caption)
                }
            }
            // Age comes from the birthday (a date picker); the core derives it each
            // day. A legacy stated age shows with a prompt to add the birthday;
            // "Prefer not to say" clears both (re-picking a birthday undoes it).
            // Bodyweight and heart rates are typed on the in-form keypad: each
            // valid entry applies at once; an out-of-range buffer is flagged and
            // never sent.
            RowHairline()
            val birthDay = draft.birthEpochDay
            val ageGiven = birthDay != null || draft.ageYears != null
            ProfileRow(
                open == ProfileRowId.Age,
                { toggle(ProfileRowId.Age) },
                stringResource(R.string.setup_row_age),
                when {
                    birthDay != null -> birthdayLabel(birthDay, today)
                    else -> draft.ageYears?.let { stringResource(R.string.setup_age_value, trimDecimal(it)) }
                },
                subtitle = if (birthDay == null && draft.ageYears != null) stringResource(R.string.setup_birthday_add_prompt) else null,
                placeholder = stringResource(R.string.setup_age_not_given),
                revealRequest = reveal == ProfileRowId.Age,
                onRevealed = { reveal = null },
                revealContent = true,
            ) {
                // The row above already shows the birthday, so the picker button
                // offers the change instead of repeating it. Without an age the HR
                // targets fall back to effort unless a measured max HR anchors them.
                BirthdayAnswer(
                    birthDay,
                    today,
                    hint = when {
                        isMinor(draft.ageYears) -> stringResource(R.string.setup_health_defer_youth)
                        !ageGiven && draft.measuredHrMax == null -> stringResource(R.string.setup_birthday_skipped_note)
                        else -> null
                    },
                    skipped = !ageGiven,
                    pickLabel = when {
                        birthDay != null -> stringResource(R.string.setup_birthday_change)
                        !ageGiven -> stringResource(R.string.setup_birthday_add)
                        else -> null
                    },
                    onSkip = { if (ageGiven) commit(draft.copy(birthEpochDay = null, ageYears = null)) },
                ) { day -> commit(draft.copy(birthEpochDay = day, ageYears = ageOnDay(day, today).toDouble())) }
            }
            RowHairline()
            val bwUnit = if (draft.weightLb) WeightUnit.Lb else WeightUnit.Kg
            val bwLabel = stringResource(R.string.setup_row_bodyweight)
            ProfileRow(
                open == ProfileRowId.Bodyweight,
                { toggle(ProfileRowId.Bodyweight) },
                bwLabel,
                draft.bodyweightKg?.let { stringResource(R.string.setup_bodyweight_value, bodyweightBufferText(it, bwUnit), weightSuffix(bwUnit)) },
                revealContent = true,
            ) {
                // Honors the kg|lb preference like every other weight surface: the
                // buffer is in the display unit and a valid entry converts back to
                // the canonical kg wire value. Keyed on the unit so a unit switch
                // re-seeds the buffer instead of reinterpreting the typed number.
                key(bwUnit) {
                    ProfileKeypadEditor(bwLabel, weightSuffix(bwUnit), draft.bodyweightKg?.let { bodyweightBufferText(it, bwUnit) }, bodyweightBounds(bwUnit), "%.1f", allowDecimal = true) {
                        commit(draft.copy(bodyweightKg = bodyweightDisplayToKg(it, bwUnit)))
                    }
                }
            }
            RowHairline()
            val rhrLabel = stringResource(R.string.setup_row_resting_hr)
            ProfileRow(
                open == ProfileRowId.RestingHr,
                { toggle(ProfileRowId.RestingHr) },
                rhrLabel,
                draft.restingHrBpm?.let { stringResource(R.string.setup_bpm_value, trimDecimal(it)) },
                revealContent = true,
            ) {
                ProfileKeypadEditor(rhrLabel, stringResource(R.string.setup_unit_bpm), draft.restingHrBpm?.let(::trimDecimal), RESTING_HR_RANGE, "%.0f") {
                    commit(draft.copy(restingHrBpm = it))
                }
            }
            RowHairline()
            val maxHrLabel = stringResource(R.string.setup_row_max_hr)
            ProfileRow(
                open == ProfileRowId.MaxHr,
                { toggle(ProfileRowId.MaxHr) },
                maxHrLabel,
                draft.measuredHrMax?.let { stringResource(R.string.setup_bpm_value, trimDecimal(it)) },
                revealContent = true,
            ) {
                ProfileKeypadEditor(
                    maxHrLabel,
                    stringResource(R.string.setup_unit_bpm),
                    draft.measuredHrMax?.let(::trimDecimal),
                    MAX_HR_RANGE,
                    "%.0f",
                    hint = stringResource(R.string.setup_max_hr_hint),
                ) {
                    commit(draft.copy(measuredHrMax = it))
                }
            }
        }

        // Health & safety, directly under the person data so the safety gates never
        // sink below the optional context fields. These arm the core's
        // medical-deferral gates (PAR-Q+, pregnancy, injury/rehab, RED-S): a raised
        // gate makes the engine defer to a professional rather than program a plan;
        // SAFETY overrides goals (HARD RULE 3). Age drives the youth gate core-side.
        ProfileGroup(stringResource(R.string.setup_section_health)) {
            // The seven PAR-Q+ questions fold into one summary row ("None apply" /
            // "2 apply") that expands to the questions and the clearance follow-up.
            // A banner's deep link lands on the answer that holds training: a
            // question or the clearance inside the open list, else a switch row.
            val parqItem = parqRevealItem(reveal, draft.health)
            val clearanceReveal = reveal == ProfileRowId.ParqClearance && draft.health.parq_positive
            val parqInner = parqItem != null || clearanceReveal
            ProfileRow(
                open == ProfileRowId.HealthQuestions,
                { toggle(ProfileRowId.HealthQuestions) },
                stringResource(R.string.setup_row_health_questions),
                parqValue(draft.health),
                revealRequest = reveal?.pickerRow == ProfileRowId.HealthQuestions && !parqInner,
                onRevealed = { reveal = null },
                revealOnExpand = !parqInner,
            ) {
                Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
                    ParqQuestions(
                        draft.health,
                        divider = { RowHairline() },
                        revealItem = parqItem,
                        revealClearance = clearanceReveal,
                        onRevealed = { reveal = null },
                    ) { commit(draft.copy(health = it)) }
                }
            }
            RowHairline()
            CurrentHealthRows(
                draft.health,
                divider = { RowHairline() },
                reveal = reveal,
                onRevealed = { reveal = null },
            ) { commit(draft.copy(health = it)) }
        }

        // Training context: environment / re-entry / overload (indiv-025 &
        // safety-024 environment modifiers, running-041 pace correction,
        // REENTRY-001 re-entry ramp, autoreg-029 overload guard). All optional:
        // "Not set" until chosen, so a plain profile arms none of them.
        ProfileGroup(stringResource(R.string.setup_section_context)) {
            if (run) {
                // Environment only changes running guidance. Its temperature and
                // altitude follow-ups show for Heat / Altitude, or while they hold a
                // value (so a set value can always be seen and cleared).
                ProfileRow(
                    open == ProfileRowId.Environment,
                    { toggle(ProfileRowId.Environment) },
                    stringResource(R.string.setup_row_environment),
                    draft.environment?.let { ctx.getString(it.labelRes) },
                ) {
                    OptionList(
                        listOf<Environment?>(null, Environment.Heat, Environment.Altitude, Environment.Cold, Environment.Neutral),
                        draft.environment,
                        { it?.let { e -> ctx.getString(e.labelRes) } ?: ctx.getString(R.string.setup_not_set) },
                    ) {
                        commit(draft.copy(environment = it))
                        open = null
                    }
                }
                if (draft.environment == Environment.Heat || draft.envTempC != null) {
                    RowHairline()
                    val tempLabel = stringResource(R.string.setup_row_env_temp)
                    ProfileRow(open == ProfileRowId.EnvTemp, { toggle(ProfileRowId.EnvTemp) }, tempLabel, draft.envTempC?.let { stringResource(R.string.setup_temp_value, it.toInt()) }) {
                        ScrollableScaleRow(ENV_TEMP_OPTIONS_C.toList(), draft.envTempC?.toInt(), { "$it" }, label = tempLabel) {
                            commit(draft.copy(envTempC = it.toDouble()))
                        }
                    }
                }
                if (draft.environment == Environment.Altitude || draft.envAltitudeM != null) {
                    RowHairline()
                    val altLabel = stringResource(R.string.setup_row_env_altitude)
                    ProfileRow(open == ProfileRowId.EnvAltitude, { toggle(ProfileRowId.EnvAltitude) }, altLabel, draft.envAltitudeM?.let { stringResource(R.string.setup_altitude_value, it.toInt()) }) {
                        ScrollableScaleRow(ENV_ALTITUDE_OPTIONS_M.toList(), draft.envAltitudeM?.toInt(), { "$it" }, label = altLabel) {
                            commit(draft.copy(envAltitudeM = it.toDouble()))
                        }
                    }
                }
                RowHairline()
            }
            // Back from a break (REENTRY-001 / reentry-002): weeks fully off plus the
            // day training restarted, so the re-entry ramp runs from that day. The
            // value is "Since Sep 11" (the year only when it isn't this year).
            val breakValue = draft.weeksOff?.let {
                stringResource(R.string.setup_break_value, epochDayShortLabel(draft.layoffReturnedEpochDay ?: today, today))
            }
            ProfileRow(
                open == ProfileRowId.Break,
                { toggle(ProfileRowId.Break) },
                stringResource(R.string.setup_row_break),
                breakValue,
                placeholder = stringResource(R.string.setup_break_none),
            ) {
                Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                    val weeksLabel = stringResource(R.string.setup_break_weeks_label)
                    FieldLabel(weeksLabel)
                    ScrollableScaleRow(WEEKS_OFF_OPTIONS.toList(), draft.weeksOff?.toInt(), { ctx.getString(R.string.setup_weeks_value, it) }, label = weeksLabel) {
                        // A first answer dates the restart today; the date pill below moves it.
                        commit(draft.copy(weeksOff = it.toDouble(), layoffReturnedEpochDay = draft.layoffReturnedEpochDay ?: today))
                    }
                    AnswerChip(stringResource(R.string.setup_break_none), selected = draft.weeksOff == null) {
                        commit(draft.copy(weeksOff = null, layoffReturnedEpochDay = null))
                    }
                    if (draft.weeksOff != null) {
                        FieldLabel(stringResource(R.string.setup_break_restart_label))
                        DateAnswerRow(draft.layoffReturnedEpochDay ?: today, today, future = false, noneLabel = null) {
                            commit(draft.copy(layoffReturnedEpochDay = it ?: today))
                        }
                    }
                }
            }
            RowHairline()
            LabeledSwitchRow(
                label = stringResource(R.string.setup_row_high_load),
                subtitle = stringResource(R.string.setup_row_high_load_sub),
                checked = draft.highLoadBlock,
                onCheckedChange = { commit(draft.copy(highLoadBlock = it)) },
            )
        }
    }
}

/** Hypertrophy priority-muscle keys: the Table 1 landmark keys the core resolves
 *  (shared/src/hypertrophy.rs LANDMARKS), with their display labels. */
internal val PRIORITY_MUSCLES: List<Pair<String, Int>> = listOf(
    "chest" to R.string.setup_muscle_chest,
    "back" to R.string.setup_muscle_back,
    "quads" to R.string.setup_muscle_quads,
    "hamstrings" to R.string.setup_muscle_hamstrings,
    "glutes" to R.string.setup_muscle_glutes,
    "side delts" to R.string.setup_muscle_side_delts,
    "rear delts" to R.string.setup_muscle_rear_delts,
    "biceps" to R.string.setup_muscle_biceps,
    "triceps" to R.string.setup_muscle_triceps,
    "calves" to R.string.setup_muscle_calves,
    "abs" to R.string.setup_muscle_abs,
)

/** The PAR-Q+ summary row value: "None apply" / "2 apply" / "Answer again". */
@Composable
private fun parqValue(health: HealthScreen): String = when {
    health.parqYesCount > 0 -> pluralStringResource(R.plurals.setup_health_flag_count, health.parqYesCount, health.parqYesCount)
    health.legacyParq -> stringResource(R.string.setup_parq_value_legacy)
    else -> stringResource(R.string.setup_parq_value_clear)
}

/** The guided-setup modality a Focus segment stands for. */
internal val Focus.modality: Modality
    get() = when (this) {
        Focus.Lift -> Modality.Lift
        Focus.Run -> Modality.Run
        Focus.Both -> Modality.Both
    }

/** Copy for the Focus-switch confirm: title, body and confirm label. */
internal class FocusSwitchCopy(val titleRes: Int, val bodyRes: Int, val confirmRes: Int)

internal fun focusSwitchCopy(from: Focus, to: Focus): FocusSwitchCopy = when (to) {
    Focus.Lift -> if (from == Focus.Both) {
        FocusSwitchCopy(R.string.setup_focus_to_lift_only_title, R.string.setup_focus_to_lift_only_body, R.string.setup_focus_switch_confirm)
    } else {
        FocusSwitchCopy(R.string.setup_focus_to_lift_title, R.string.setup_focus_to_lift_body, R.string.setup_focus_answer_lift)
    }
    Focus.Run -> if (from == Focus.Both) {
        FocusSwitchCopy(R.string.setup_focus_to_run_only_title, R.string.setup_focus_to_run_only_body, R.string.setup_focus_switch_confirm)
    } else {
        FocusSwitchCopy(R.string.setup_focus_to_run_title, R.string.setup_focus_to_run_body, R.string.setup_focus_answer_run)
    }
    Focus.Both -> if (from == Focus.Run) {
        FocusSwitchCopy(R.string.setup_focus_add_lift_title, R.string.setup_focus_add_lift_body, R.string.setup_focus_answer_lift)
    } else {
        FocusSwitchCopy(R.string.setup_focus_add_run_title, R.string.setup_focus_add_run_body, R.string.setup_focus_answer_run)
    }
}

/** A Both profile with one side dropped ([keep] = the side that stays). The
 *  concurrent goal follows the guided setup's mapping: lift-only = the lift
 *  goal, run-only = endurance priority. */
internal fun ProfileDraft.withoutSide(keep: Focus): ProfileDraft = when (keep) {
    Focus.Lift -> copy(
        runningDaysPerWeek = 0,
        runningKmPerWeek = 0.0,
        runningKmPerWeekSet = true,
        concurrentGoal = liftGoal.toConcurrent(),
    )
    Focus.Run -> copy(weeklySets = 0, concurrentGoal = ConcurrentGoal.EndurancePriority)
    Focus.Both -> this
}

/** Lift days as the Training days row shows them: unanswered = what the engine
 *  schedules (plan::mod). */
internal fun ProfileDraft.shownLiftDays(): Int =
    liftDaysPerWeek ?: if (runningDaysPerWeek > 0) HYBRID_DEFAULT_LIFT_DAYS else LIFT_ONLY_DEFAULT_DAYS

/** The week's session days: lift days, run days, or both added up. */
internal fun ProfileDraft.trainingDayTotal(): Int {
    val lift = weeklySets > 0
    val run = runningDaysPerWeek > 0
    return when {
        lift && run -> shownLiftDays() + runningDaysPerWeek
        run -> runningDaysPerWeek
        else -> shownLiftDays()
    }
}

/**
 * Edit a recent race: distance chips ("I don't know" clears it) and a finish
 * time typed on the keypad. Only a complete, plausible time applies, so the
 * stored race is always one the field displayed.
 */
@Composable
private fun RecentRaceEditor(race: RecentRace?, onChange: (RecentRace?) -> Unit) {
    var distance by rememberSaveable { mutableStateOf(race?.let { RaceDistance.of(it.distance_km) }) }
    var digits by rememberSaveable { mutableStateOf(race?.let { raceTimeDigits(it.time_sec) } ?: "") }
    var fresh by rememberSaveable { mutableStateOf(true) }
    val sec = raceTimeSec(digits)
    val d = distance
    val invalid = d != null && digits.isNotEmpty() && (sec == null || !plausibleRaceTime(d.km, sec))
    fun edit(next: String) {
        digits = next
        fresh = false
        val s = raceTimeSec(next)
        if (d != null && s != null && plausibleRaceTime(d.km, s)) onChange(RecentRace(d.km, s))
    }
    Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
        ChipChoiceRow(listOf<RaceDistance?>(null) + RaceDistance.entries, distance, { stringResource(it?.labelRes ?: R.string.setup_dont_know) }) { pick ->
            distance = pick
            fresh = true
            if (pick == null) {
                digits = ""
                onChange(null)
            } else {
                raceTimeSec(digits)?.takeIf { plausibleRaceTime(pick.km, it) }?.let { onChange(RecentRace(pick.km, it)) }
            }
        }
        if (distance != null) {
            RaceTimeField(digits, active = true, invalid = invalid) { fresh = true }
            NumericKeypad(
                onKey = { key -> appendRaceDigit(digits, key, fresh)?.let(::edit) },
                onBackspace = { edit(digits.dropLast(1)) },
                allowDecimal = false,
            )
        }
    }
}

/** Accepted keypad ranges for the person numbers (wizard + Profile share them). */
internal val AGE_RANGE = 14.0..100.0
internal val RESTING_HR_RANGE = 30.0..120.0
internal val MAX_HR_RANGE = 120.0..230.0

/** Weekly sets per muscle the Profile scale offers. Starts at 1, not 0: a
 *  modality's on/off is owned by the Focus row, which confirms the switch first. */
private val WEEKLY_SETS_OPTIONS = 1..30

/** Outdoor temperatures offered, °C: a plausible training band around the ~15 °C
 *  where the heat pace correction arms (running-041). */
private val ENV_TEMP_OPTIONS_C = -10..45

/** Altitudes offered, m: past the ~900 m where the altitude correction arms
 *  (running-041) and the 2,500 m KB environment-modifier mark (indiv-025). */
private val ENV_ALTITUDE_OPTIONS_M = 0..4500 step 100

/** Weeks fully off the Break row offers. */
private val WEEKS_OFF_OPTIONS = 1..52

/** Sex answer → its label; null (not given) → "Prefer not to say". */
internal fun sexLabelRes(female: Boolean?): Int = when (female) {
    true -> R.string.setup_sex_female
    false -> R.string.setup_sex_male
    null -> R.string.setup_sex_not_saying
}

/** The Sex row's value. The Profile exists only after setup, where no sex (null)
 *  is the "Prefer not to say" answer (or the question left unanswered: the same
 *  "not said"), never an unset field. */
@Composable
private fun sexLabel(female: Boolean?): String = stringResource(sexLabelRes(female))

/**
 * Keypad entry for one Profile person number. The buffer starts from the stored
 * value (or empty) and is exactly what is shown; every keystroke that leaves an
 * in-range number applies it at once (Profile changes apply immediately), while an
 * out-of-range or partial buffer is flagged and never sent, so the stored value is
 * always one the field displayed.
 */
@Composable
private fun ProfileKeypadEditor(
    label: String,
    unit: String,
    seedText: String?,
    range: ClosedFloatingPointRange<Double>,
    format: String,
    // Whole-number fields (age, heart rates) get no decimal key that would do nothing.
    allowDecimal: Boolean = false,
    hint: String? = null,
    onValid: (Double) -> Unit,
) {
    var text by rememberSaveable { mutableStateOf(seedText ?: "") }
    var fresh by rememberSaveable { mutableStateOf(true) }
    var touched by rememberSaveable { mutableStateOf(false) }
    val valid = parseKeypadNumber(text)?.let { it in range } == true
    val edit: (String) -> Unit = { t ->
        text = t
        fresh = false
        touched = true
        parseKeypadNumber(t)?.takeIf { it in range }?.let(onValid)
    }
    // A keystroke can insert or drop the pinned hold banner above the list (an
    // under-18 age), which slides the field being typed under it: keep the whole
    // editor, and at least the field, on screen after each one.
    val editorRequester = remember { BringIntoViewRequester() }
    val fieldRequester = remember { BringIntoViewRequester() }
    val editorTop = remember { WindowTop() }
    LaunchedEffect(text) {
        if (touched) revealWhenSettled(editorTop, editorRequester, fieldRequester)
    }
    Column(
        modifier = Modifier
            .bringIntoViewRequester(editorRequester)
            .onGloballyPositioned { editorTop.y = it.positionInWindow().y },
        verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        Box(Modifier.bringIntoViewRequester(fieldRequester)) {
            KeypadValueField(
                label, unit, text,
                active = true,
                invalid = touched && text.isNotEmpty() && !valid,
                min = range.start, max = range.endInclusive, format = format,
                // The keypad below is the one input; no ±1 steppers beside it.
                adjustments = emptyList(),
                onActivate = { fresh = true },
                onText = edit,
                hint = hint,
            )
        }
        NumericKeypad(
            onKey = { key -> edit(editNumericBuffer(text, key, fresh)) },
            onBackspace = { edit(text.dropLast(1)) },
            allowDecimal = allowDecimal,
        )
    }
}

/** The 1dp hairline between grouped profile rows (04-profile §1). */
@Composable
private fun RowHairline() {
    HorizontalDivider(thickness = 1.dp, color = OnBgBody.copy(alpha = HairlineAlpha.Row))
}

/**
 * One Profile group: its accent section overline (a TalkBack heading) over its own
 * card of hairline-divided rows, so the Profile reads as a few short cards rather
 * than one long one.
 */
@Composable
private fun ProfileGroup(title: String, content: @Composable () -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
        Box(Modifier.semantics { heading() }) { SectionOverline(title) }
        PlainCard {
            Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) { content() }
        }
    }
}

/** A quiet whole-row action inside a Profile card (label + chevron, ≥48dp),
 *  styled like the field rows around it. */
@Composable
private fun ActionRow(label: String, onClick: () -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = ActionMinHeight)
            .clip(RoundedCornerShape(Space.Md.dp))
            .clickable(role = Role.Button) { onClick() }
            .padding(vertical = Space.Sm.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, color = OnBgBody, style = Type.Body, modifier = Modifier.weight(1f).padding(end = Space.Md.dp))
        RowChevron()
    }
}

/**
 * A profile field as a tap-row (04-profile §1): label left (the same `OnBgBody`
 * as the segmented and switch rows), value Bold + `ui-chevron-right` right;
 * tapping toggles an inline [content] picker below it (full-width OptionList /
 * scale rows / keypad, no anchored dropdowns, no steppers as primary). ≥48dp
 * whole-row tap target; edits apply immediately. TalkBack hears whether the
 * picker is expanded or collapsed.
 *
 * A null [value] is an unanswered field: [placeholder] (default "Not set") is
 * shown faint and regular so empty rows never read as data. The value sits at the
 * right while it fits on one line in ~40% of the row; a longer value (or a large
 * font scale) moves under the label, left-aligned, so neither column wraps into
 * a ragged sliver. Expanding scrolls the row with its open picker into view once
 * the layout settles; [revealRequest] (a cross-tab "Check age") does the same for
 * a row opened from elsewhere, then reports [onRevealed]. With [revealContent] (a
 * keypad picker) the whole picker wins over the row's label: when the row is
 * taller than the space left under a pinned banner, the label scrolls under it
 * and the field and every key stay above the navigation bar. [revealOnExpand]
 * false leaves the scrolling to a [RevealTarget] inside the picker.
 */
@Composable
private fun ProfileRow(
    expanded: Boolean,
    onToggle: () -> Unit,
    label: String,
    value: String?,
    // Optional data line under the label (e.g. the ticked weekdays); null keeps
    // the row single-line.
    subtitle: String? = null,
    placeholder: String? = null,
    revealRequest: Boolean = false,
    onRevealed: () -> Unit = {},
    revealContent: Boolean = false,
    revealOnExpand: Boolean = true,
    content: @Composable () -> Unit,
) {
    val state = stringResource(if (expanded) R.string.ui_cd_expanded else R.string.ui_cd_collapsed)
    val requester = remember { BringIntoViewRequester() }
    val contentRequester = remember { BringIntoViewRequester() }
    val rowTop = remember { WindowTop() }
    val landIn = LocalRevealViewport.current
    LaunchedEffect(expanded, revealRequest) {
        val onExpand = expanded && revealOnExpand
        if (onExpand && revealContent) {
            revealWhenSettled(rowTop, requester, contentRequester)
        } else if (revealRequest) {
            revealWhenSettled(rowTop, requester, landIn = landIn)
        } else if (onExpand) {
            revealWhenSettled(rowTop, requester)
        }
        if (revealRequest) onRevealed()
    }
    val shown = value ?: placeholder ?: stringResource(R.string.setup_not_set)
    val valueStyle = if (value == null) Type.Body else Type.Body.copy(fontWeight = FontWeight.Bold).merge(TabularFigures)
    val valueColor = if (value == null) OnBgFaint else OnBgBody
    val measurer = rememberTextMeasurer()
    Column(
        modifier = Modifier
            .bringIntoViewRequester(requester)
            .onGloballyPositioned { rowTop.place(it) },
        verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
    ) {
        BoxWithConstraints(Modifier.fillMaxWidth()) {
            val valueMaxPx = constraints.maxWidth * 0.4f
            val inline = measurer.measure(shown, style = valueStyle, maxLines = 1, softWrap = false).size.width <= valueMaxPx
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = ActionMinHeight)
                    // Clip before clickable so the ripple stays rounded (no square
                    // flash around the row).
                    .clip(RoundedCornerShape(Space.Md.dp))
                    .clickable { onToggle() }
                    .semantics { stateDescription = state }
                    // Inside the clip so a multi-line caption's last line never
                    // touches the clip edge.
                    .padding(vertical = Space.Sm.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(
                    modifier = Modifier.weight(1f).padding(end = Space.Md.dp),
                    verticalArrangement = Arrangement.spacedBy(Space.Xs.dp),
                ) {
                    Text(label, color = OnBgBody, style = Type.Body)
                    if (!inline) {
                        Text(shown, color = valueColor, style = valueStyle)
                    }
                    if (subtitle != null) {
                        Text(subtitle, color = OnBgFaint, style = Type.Caption)
                    }
                }
                if (inline) {
                    Text(shown, color = valueColor, style = valueStyle, textAlign = TextAlign.End, maxLines = 1)
                    Spacer(Modifier.width(Space.Md.dp))
                }
                RowChevron(expanded)
            }
        }
        if (expanded) Box(Modifier.bringIntoViewRequester(contentRequester)) { content() }
    }
}

/** [content] (one row inside a Profile card) scrolled into view once the layout
 *  settles while [request] holds, then [onRevealed]: a banner's deep link landing
 *  on the exact answer behind a hold, high in the list ([LocalRevealViewport]). */
@Composable
internal fun RevealTarget(request: Boolean, onRevealed: () -> Unit, content: @Composable () -> Unit) {
    val requester = remember { BringIntoViewRequester() }
    val top = remember { WindowTop() }
    val landIn = LocalRevealViewport.current
    LaunchedEffect(request) {
        if (request) {
            revealWhenSettled(top, requester, landIn = landIn)
            onRevealed()
        }
    }
    Box(
        Modifier
            .bringIntoViewRequester(requester)
            .onGloballyPositioned { top.place(it) },
    ) { content() }
}

/** Last laid-out window y and width of a reveal target; written only from layout
 *  callbacks, so a plain holder rather than snapshot state. */
private class WindowTop {
    var y = Float.NaN
    var width = 0f

    fun place(coords: LayoutCoordinates) {
        y = coords.positionInWindow().y
        width = coords.size.width.toFloat()
    }
}

/** Height in px of the list a deep link reveals its row in (0 = unknown), written
 *  by that list's layout callback; a plain holder like [WindowTop]. */
internal class RevealViewport {
    var height = 0
}

/** The list a deep-link reveal lands in. Without one (0 height) the reveal only
 *  brings the row on screen. */
internal val LocalRevealViewport = staticCompositionLocalOf { RevealViewport() }

/** Where a deep link lands its row in a [RevealViewport]: a row below scrolls up
 *  until its top sits a third of the way down, a row above down to a quarter, so
 *  the answers around it stay in view instead of the row hugging an edge. */
private const val LAND_FROM_BELOW = 1f / 3
private const val LAND_FROM_ABOVE = 1f / 4

/** Frames a reveal waits at most for the layout to stop moving. */
private const val REVEAL_MAX_FRAMES = 12

/**
 * Brings [requesters] into view (in order) once the surrounding layout settles. A
 * tab switch, or a hold banner appearing above the list, keeps moving the target
 * for a few frames, so a single frame's wait can scroll to a stale position: this
 * re-requests until [top] holds still across a frame. A competing scroll (the
 * list keeping a toggled row under the finger) cancels one request, not the reveal.
 * With [landIn] (a deep link) each request asks for the band around the row that
 * lands it high in the list ([LAND_FROM_BELOW]); a row already there stays put.
 */
private suspend fun revealWhenSettled(
    top: WindowTop,
    vararg requesters: BringIntoViewRequester,
    landIn: RevealViewport? = null,
) {
    var last = Float.NaN
    repeat(REVEAL_MAX_FRAMES) {
        withFrameNanos { }
        val viewport = landIn?.height ?: 0
        val band = if (viewport > 0) {
            Rect(0f, -viewport * LAND_FROM_ABOVE, top.width, viewport * (1f - LAND_FROM_BELOW))
        } else {
            null
        }
        for (r in requesters) {
            try {
                r.bringIntoView(band)
            } catch (e: CancellationException) {
                currentCoroutineContext().ensureActive()
            }
        }
        withFrameNanos { }
        if (top.y == last) return
        last = top.y
    }
}
