package app.milestone

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

    private fun p(ctx: PlatformContext) = ctx.keyValueStore(PREFS)

    /** Keep [draft]'s running answers as running is switched off. */
    fun stashRun(ctx: PlatformContext, draft: Event.SetProfile) {
        // A side already at 0 has nothing worth keeping: writing it would
        // clobber the last real numbers.
        if (draft.runningDaysPerWeek <= 0) return
        val km = draft.runningKmPerWeek.takeIf { draft.runningKmPerWeekSet && it > 0.0 }
        p(ctx).edit {
            putInt(K_DAYS, draft.runningDaysPerWeek)
            if (km != null) putLong(K_KM_BITS, km.toRawBits()) else remove(K_KM_BITS)
            putString(K_RUN_GOAL, draft.goalDistance.name)
            draft.goalRaceEpochDay?.let { putLong(K_RACE_DAY, it) } ?: remove(K_RACE_DAY)
            putInt(K_RUN_WEEKDAYS, draft.trainingWeekdays ?: ANY_WEEKDAYS)
        }
    }

    /** Keep [draft]'s lifting answers as lifting is switched off. */
    fun stashLift(ctx: PlatformContext, draft: Event.SetProfile) {
        if (draft.weeklySets <= 0) return
        p(ctx).edit {
            putInt(K_LIFT_SETS, draft.weeklySets)
            putString(K_LIFT_GOAL, draft.liftGoal.name)
            draft.liftDaysPerWeek?.let { putInt(K_LIFT_DAYS, it) } ?: remove(K_LIFT_DAYS)
            putInt(K_LIFT_WEEKDAYS, draft.trainingWeekdays ?: ANY_WEEKDAYS)
        }
    }

    /** The stashed running answers, or null when running was never switched off. */
    fun run(ctx: PlatformContext): StashedRun? = p(ctx).takeIf { it.contains(K_DAYS) }?.let { s ->
        StashedRun(
            days = s.getInt(K_DAYS, 0),
            km = if (s.contains(K_KM_BITS)) Double.fromBits(s.getLong(K_KM_BITS, 0L)) else null,
            goal = s.getString(K_RUN_GOAL, null)?.let { g -> GoalDistance.entries.firstOrNull { it.name == g } },
            raceEpochDay = if (s.contains(K_RACE_DAY)) s.getLong(K_RACE_DAY, 0L) else null,
            weekdays = weekdaysOf(s.getInt(K_RUN_WEEKDAYS, ANY_WEEKDAYS)),
        )
    }

    /** The stashed lifting answers, or null when lifting was never switched off. */
    fun lift(ctx: PlatformContext): StashedLift? = p(ctx).let { s ->
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

/**
 * The profile switched back to a single side the user already set up, when the
 * stash holds every answer that side's setup would ask (its goal and days, with the
 * stashed weekdays still covering them); the other side is dropped as
 * [withoutSide] does. Null = ask in the guided setup. Both always asks (which side
 * leads is never stashed).
 */
internal fun Event.SetProfile.resumedSide(target: Modality, lift: StashedLift?, run: StashedRun?, today: Long): Event.SetProfile? = when (target) {
    Modality.Lift -> lift?.takeIf { s ->
        weeklySets == 0 && s.weeklySets > 0 && s.days != null && s.days in LIFT_DAYS && weekdaysCover(s.weekdays, s.days)
    }?.let { s ->
        copy(weeklySets = s.weeklySets, liftDaysPerWeek = s.days, liftGoal = s.goal, trainingWeekdays = s.weekdays)
            .withoutSide(Modality.Lift)
    }
    Modality.Run -> run?.takeIf { s ->
        runningDaysPerWeek == 0 && s.days in dayOptions(Modality.Run) && weekdaysCover(s.weekdays, s.days)
    }?.let { s ->
        val goal = s.goal ?: return@let null
        copy(
            runningDaysPerWeek = s.days,
            runningKmPerWeek = s.km ?: 0.0,
            runningKmPerWeekSet = s.km != null,
            goalDistance = goal,
            goalRaceEpochDay = s.raceEpochDay?.takeIf { it > today && goal.isRace },
            liftDaysPerWeek = null,
            trainingWeekdays = s.weekdays,
        ).withoutSide(Modality.Run)
    }
    Modality.Both -> null
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

    private fun p(ctx: PlatformContext) = ctx.keyValueStore(PREFS)

    fun save(ctx: PlatformContext, level: SetupLevel) {
        p(ctx).edit { putString(K_LEVEL, level.name) }
    }

    /** The recorded level, or null when none is recorded or it contradicts [advanced]. */
    fun read(ctx: PlatformContext, advanced: Boolean): SetupLevel? =
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
internal fun parqRevealItem(id: ProfileRowId?, health: HealthScreen): ParqQuestion? = when (id) {
    ProfileRowId.ParqSymptom -> draftFacts(ProfileDraft(health = health)).first_symptom
    ProfileRowId.ParqInjury ->
        ParqQuestion.BoneOrJoint.takeIf { it in draftFacts(ProfileDraft(health = health)).parq_yes }
    else -> null
}

/** The profile edit dated for dispatch: in force from [today] (the core's today,
 *  so settled past days keep the profile they had), stamped with the instant it
 *  is made (a run logged earlier that day stays on the profile it was logged
 *  under). An undated SetProfile replaces the whole profile history (tests only). */
internal fun Event.SetProfile.effectiveFrom(today: Long): Event.SetProfile =
    copy(effectiveFromEpochDay = today, editedAt = AppClock.nowSec())

/** Rebuild a draft from the core-echoed profile (post log-replay hydration). */
internal fun Event.SetProfile.Companion.from(p: Profile) = Event.SetProfile(
    progressionCadence = p.progression_cadence,
    liftGoal = p.lift_goal,
    goalDistance = p.goal_distance,
    concurrentGoal = p.concurrent_goal,
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
    distanceUnit = p.distance_unit,
    weightLb = p.weight_lb,
    health = p.health,
    resistanceModality = p.resistance_modality,
    highLoadBlock = p.high_load_block,
    environment = p.environment,
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

/**
 * Weekly running-volume SCALE in the user's distance unit (ProfileEditor
 * "Running km/wk" / "Running mi/wk"). The core stores canonical km; the scale's
 * options, highlight and row value are whole display units, and a pick converts
 * back to km for the wire (km exact, mi via [miToKmWire]). Range 1–150 km, i.e.
 * 1–93 mi. Pure.
 */
const val WEEKLY_VOLUME_MAX_KM: Double = 150.0

fun weeklyVolumeScaleOptions(unit: DistanceUnit): List<Int> =
    if (unit == DistanceUnit.Mi) {
        (1..kmToMi(WEEKLY_VOLUME_MAX_KM).roundOrZero().toInt()).toList()
    } else {
        (1..WEEKLY_VOLUME_MAX_KM.toInt()).toList()
    }

fun weeklyVolumeScaleValue(km: Double, unit: DistanceUnit): Int =
    (if (unit == DistanceUnit.Mi) kmToMi(km) else km).roundOrZero().toInt()

fun weeklyVolumeScaleToKm(value: Int, unit: DistanceUnit): Double =
    if (unit == DistanceUnit.Mi) miToKmWire(value.toDouble()) else value.toDouble()

/**
 * Fallback starting point for the full editor only (rehydration when no
 * profile is echoed). Never seeded on a fresh install: first run goes
 * through the guided setup, which writes user-asserted values rather than
 * these opinionated defaults.
 */
internal val Event.SetProfile.Companion.SEED: Event.SetProfile get() = ProfileSeed

private val ProfileSeed = Event.SetProfile(
    progressionCadence = ProgressionCadence.WeekToWeek,
    liftGoal = LiftGoal.MaxStrength,
    goalDistance = GoalDistance.TenK,
    concurrentGoal = ConcurrentGoal.Strength,
    weeklySets = 14,
    runningDaysPerWeek = 4,
    runningKmPerWeek = 45.0,
    advanced = false,
)

/**
 * Change the lift goal and keep the hybrid "which leads" answer coherent: while
 * lifting leads (and always for a lift-only profile, where the row is hidden) the
 * concurrent goal follows the new lift goal; a run-led hybrid stays run-led.
 */
internal fun Event.SetProfile.withLiftGoal(goal: LiftGoal): Event.SetProfile {
    val runLeads = weeklySets > 0 && runningDaysPerWeek > 0 &&
        concurrentGoal == ConcurrentGoal.EndurancePriority
    return copy(
        liftGoal = goal,
        concurrentGoal = if (runLeads) ConcurrentGoal.EndurancePriority else goal.toConcurrent(),
    )
}

/** The two-way "Which leads?" answer mapped onto the concurrent goal (same mapping
 *  the guided setup uses), with the day counts fitted to that lead's caps. */
internal fun Event.SetProfile.withLead(lead: BothPriority): Event.SetProfile {
    val liftDays = liftDaysPerWeek?.coerceIn(hybridLiftDayRange(lead))
    val runMax = hybridRunDayOptions(liftDays ?: CoreConstants.HYBRID_DEFAULT_LIFT_DAYS, lead).last
    return copy(
        concurrentGoal = leadingConcurrentGoal(lead, liftGoal),
        liftDaysPerWeek = liftDays,
        runningDaysPerWeek = runningDaysPerWeek.coerceAtMost(runMax),
    )
}

/** A Both profile with one side dropped ([keep] = the side that stays). The
 *  concurrent goal follows the guided setup's mapping: lift-only = the lift
 *  goal, run-only = endurance priority. */
internal fun Event.SetProfile.withoutSide(keep: Modality): Event.SetProfile = when (keep) {
    Modality.Lift -> copy(
        runningDaysPerWeek = 0,
        runningKmPerWeek = 0.0,
        runningKmPerWeekSet = true,
        concurrentGoal = liftGoal.toConcurrent(),
    )
    Modality.Run -> copy(weeklySets = 0, concurrentGoal = ConcurrentGoal.EndurancePriority)
    Modality.Both -> this
}

/** Lift days as the Training days row shows them: unanswered = what the engine
 *  schedules (the core's default for a hybrid or a lifting-only week). */
internal fun Event.SetProfile.shownLiftDays(): Int =
    liftDaysPerWeek ?: if (runningDaysPerWeek > 0) CoreConstants.HYBRID_DEFAULT_LIFT_DAYS else CoreConstants.LIFT_ONLY_DEFAULT_LIFT_DAYS

/** The week's session days: lift days, run days, or both added up. */
internal fun Event.SetProfile.trainingDayTotal(): Int {
    val lift = weeklySets > 0
    val run = runningDaysPerWeek > 0
    return when {
        lift && run -> shownLiftDays() + runningDaysPerWeek
        run -> runningDaysPerWeek
        else -> shownLiftDays()
    }
}
