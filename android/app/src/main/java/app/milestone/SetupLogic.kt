package app.milestone

import java.time.DayOfWeek
import java.time.LocalDate
import java.time.Period
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.time.format.TextStyle
import java.util.Locale

// Pure guided-setup logic: the answer vocabulary, the screen path, the answers →
// profile mapping and the seed a re-run or Focus switch opens with. No Compose.

/** The three training modalities the guided setup maps onto the profile's volume
 *  fields. Mirrors the [Focus] row in the full editor: Run-only zeroes lifting,
 *  Lift-only zeroes running, Both splits the week. Title/consequence carry string
 *  resource ids, resolved at the (composable) render site. */
internal enum class Modality(val titleRes: Int, val consequenceRes: Int) {
    Lift(R.string.setup_modality_lift_title, R.string.setup_modality_lift_consequence),
    Run(R.string.setup_modality_run_title, R.string.setup_modality_run_consequence),
    Both(R.string.setup_modality_both_title, R.string.setup_modality_both_consequence),
}

/** The modality a stored profile trains: whichever sides carry volume (null when
 *  neither does). */
internal fun modalityOf(draft: ProfileDraft): Modality? = when {
    draft.weeklySets > 0 && draft.runningDaysPerWeek > 0 -> Modality.Both
    draft.weeklySets > 0 -> Modality.Lift
    draft.runningDaysPerWeek > 0 -> Modality.Run
    else -> null
}

/** The lifting intent → the profile's lift goal. */
internal enum class LiftFocus(
    val consequenceRes: Int,
    val liftGoal: LiftGoal,
) {
    Strength(R.string.setup_liftfocus_strength_consequence, LiftGoal.MaxStrength),
    Muscle(R.string.setup_liftfocus_muscle_consequence, LiftGoal.Hypertrophy),
    Power(R.string.setup_liftfocus_power_consequence, LiftGoal.Power),
    ;

    /** The card title is the shared [LiftGoal] label, so the wizard, the review
     *  and the Profile Training goal row use one vocabulary. */
    val titleRes: Int get() = liftGoal.labelRes

    companion object {
        fun of(goal: LiftGoal): LiftFocus? = entries.firstOrNull { it.liftGoal == goal }
    }
}

/** The concurrent goal a lift goal implies when lifting leads (or is all there is). */
internal fun LiftGoal.toConcurrent(): ConcurrentGoal = when (this) {
    LiftGoal.MaxStrength -> ConcurrentGoal.Strength
    LiftGoal.Power -> ConcurrentGoal.Power
    LiftGoal.Hypertrophy -> ConcurrentGoal.Hypertrophy
}

/** For a Both profile: which side leads. The core only reads the concurrent goal as
 *  lift-first vs run-first, so this two-way answer is the whole choice. */
internal enum class BothPriority(val titleRes: Int, val consequenceRes: Int) {
    LiftLean(R.string.setup_bothpriority_liftlean_title, R.string.setup_bothpriority_liftlean_consequence),
    RunLean(R.string.setup_bothpriority_runlean_title, R.string.setup_bothpriority_runlean_consequence),
}

/** Which-leads → concurrent goal, shared by the wizard and the Profile editor:
 *  lifting leading keeps the lift goal itself in front (so it can never contradict
 *  Training goal); running leading is endurance priority. */
internal fun leadingConcurrentGoal(lead: BothPriority, liftGoal: LiftGoal): ConcurrentGoal = when (lead) {
    BothPriority.LiftLean -> liftGoal.toConcurrent()
    BothPriority.RunLean -> ConcurrentGoal.EndurancePriority
}

/** The which-leads answer a stored concurrent goal represents. */
internal fun ConcurrentGoal.lead(): BothPriority =
    if (this == ConcurrentGoal.EndurancePriority) BothPriority.RunLean else BothPriority.LiftLean

/** Rough training experience → progression cadence + starting volumes. The same
 *  three answers back the Profile "Experience" row via [cadence]/[fromCadence].
 *  [setsPerMuscle] is the starting weekly sets per muscle a lifting profile at this
 *  level opens with. */
internal enum class SetupLevel(
    val titleRes: Int,
    val consequenceRes: Int,
    val runConsequenceRes: Int,
    val cadence: ProgressionCadence,
    val setsPerMuscle: Int,
) {
    Novice(
        R.string.setup_level_novice_title,
        R.string.setup_level_novice_consequence,
        R.string.setup_runlevel_novice_consequence,
        ProgressionCadence.EverySession,
        setsPerMuscle = 8,
    ),
    Intermediate(
        R.string.setup_level_intermediate_title,
        R.string.setup_level_intermediate_consequence,
        R.string.setup_runlevel_intermediate_consequence,
        ProgressionCadence.WeekToWeek,
        setsPerMuscle = 12,
    ),
    Advanced(
        R.string.setup_level_advanced_title,
        R.string.setup_level_advanced_consequence,
        R.string.setup_runlevel_advanced_consequence,
        ProgressionCadence.MonthToMonth,
        setsPerMuscle = 16,
    ),
    ;

    companion object {
        fun fromCadence(cadence: ProgressionCadence): SetupLevel = entries.first { it.cadence == cadence }
    }
}

/** A sex answer: stated, or explicitly not given ("Prefer not to say" → null). */
internal enum class SexAnswer(val female: Boolean?) {
    Male(false), Female(true), NotSaying(null);

    companion object {
        /** A finished setup's stored answer. Setup stores "Prefer not to say" and a
         *  skipped question alike as no sex (null), and both mean "not said", so a
         *  profile's null reads back as [NotSaying] (the Profile picker shows it so too). */
        fun of(female: Boolean?): SexAnswer = when (female) {
            true -> Female
            false -> Male
            null -> NotSaying
        }
    }
}

/** Setup choice-card title for the strength-training modality card.
 *  Weights = barbell/machine loading; Calisthenics = bodyweight tier progression. */
internal val ResistanceModality.setupTitleRes: Int
    get() = when (this) {
        ResistanceModality.Weights -> R.string.setup_modality_weights_title
        ResistanceModality.Calisthenics -> R.string.setup_modality_calis_title
    }

internal val ResistanceModality.setupConsequenceRes: Int
    get() = when (this) {
        ResistanceModality.Weights -> R.string.setup_modality_weights_consequence
        ResistanceModality.Calisthenics -> R.string.setup_modality_calis_consequence
    }

/** The kg|lb the bodyweight field opens in: a re-run keeps the profile's choice; a
 *  first run follows the phone's region (lb in the US/LR/MM). */
internal fun seedWeightUnit(initial: ProfileDraft?, locale: Locale = Locale.getDefault()): WeightUnit =
    initial?.let { if (it.weightLb) WeightUnit.Lb else WeightUnit.Kg } ?: localeWeightUnit(locale)

/** Age at which the core's pediatric hold (SAFE-PEDS-001) stops applying. */
private const val ADULT_AGE_YEARS = 18.0

/** Under 18: the core's pediatric hold (SAFE-PEDS-001) keys off age alone. */
internal fun isMinor(ageYears: Double?): Boolean = ageYears != null && ageYears < ADULT_AGE_YEARS

/** Completed years from [birthEpochDay] to [today] (local epoch days): the same
 *  calendar count the core derives `age_years` with (a 29 February birthday
 *  completes its year on 1 March in common years). Display only before the core
 *  echoes the profile; a future birthday reads 0. */
internal fun ageOnDay(birthEpochDay: Long, today: Long): Int =
    Period.between(LocalDate.ofEpochDay(birthEpochDay), LocalDate.ofEpochDay(today)).years.coerceAtLeast(0)

/** The birthdays whose age on [today] lies inside [AGE_RANGE] (14–100), oldest
 *  first: the picker's selectable days, so no future date can be picked. */
internal fun birthdayRange(today: Long): LongRange {
    val t = LocalDate.ofEpochDay(today)
    val oldest = t.minusYears(AGE_RANGE.endInclusive.toLong() + 1).plusDays(1)
    val youngest = t.minusYears(AGE_RANGE.start.toLong())
    return oldest.toEpochDay()..youngest.toEpochDay()
}

/** The day the birthday calendar opens on: the birthday itself, else the same
 *  day [PICKER_START_YEARS_BACK] years before [today] (a typical adult, decades
 *  nearer than the youngest selectable year), kept inside [birthdayRange]. */
internal fun birthdayPickerStart(birthDay: Long?, today: Long): Long {
    val range = birthdayRange(today)
    return (birthDay ?: LocalDate.ofEpochDay(today).minusYears(PICKER_START_YEARS_BACK).toEpochDay())
        .coerceIn(range.first, range.last)
}

/** Years back from today the birthday calendar opens on when no birthday is set. */
private const val PICKER_START_YEARS_BACK = 30L

/** Whether these answers raise a core deferral gate, so the wizard can say so
 *  before Finish. Age under 18 fires the core's pediatric Defer gate (SAFE-PEDS-001,
 *  derived from age alone in app/mod.rs); the rest mirror the health flags. A
 *  symptom (Q2/Q3) defers even with a clearance answer (onboard-052). */
internal fun healthDefers(ageYears: Double?, health: HealthScreen): Boolean =
    isMinor(ageYears) ||
        (health.parq_positive && !health.medically_cleared) ||
        health.symptomatic || health.acute_illness ||
        health.pregnant || health.pregnancy_warning_sign ||
        health.injury_or_rehab || health.reds_signal

/** A display buffer parsed as a number inside [range], else null. */
internal fun parseInRange(text: String, range: ClosedFloatingPointRange<Double>): Double? =
    parseKeypadNumber(text)?.takeIf { it in range }

// ── Screen path ─────────────────────────────────────────────────────────────

/** One wizard screen. One decision per screen: a Both profile answers its lifting
 *  goal, running goal and which-leads on three separate screens. */
internal enum class SetupStep { Train, LiftGoal, Placement, RunGoal, Race, Lead, Days, Level, RunNow, About, Health, Review }

/** The screens for a modality, in order (Lift 7, Run 8, Both 10, plus the
 *  calisthenics placement and race-details screens when those answers call for
 *  them). Before a modality is picked the single-goal shape stands in so the
 *  progress bar has a length. Optional screens are always inserted AFTER the
 *  screen whose answer adds them, so the current index never shifts under the user. */
internal fun setupSteps(modality: Modality?, calisthenics: Boolean = false, raceGoal: Boolean = false): List<SetupStep> = buildList {
    add(SetupStep.Train)
    val lifts = modality != Modality.Run
    val runs = modality == Modality.Run || modality == Modality.Both
    if (lifts) {
        add(SetupStep.LiftGoal)
        if (calisthenics) add(SetupStep.Placement)
    }
    if (runs) {
        add(SetupStep.RunGoal)
        if (raceGoal) add(SetupStep.Race)
    }
    if (modality == Modality.Both) add(SetupStep.Lead)
    add(SetupStep.Days)
    add(SetupStep.Level)
    if (runs) add(SetupStep.RunNow)
    add(SetupStep.About)
    add(SetupStep.Health)
    add(SetupStep.Review)
}

/** The longest path a modality's answers can take (every optional screen in).
 *  The progress bar is sized to it, so an answer that adds a screen never
 *  moves the bar backwards. */
internal fun longestSetupSteps(modality: Modality?): List<SetupStep> =
    setupSteps(modality, calisthenics = true, raceGoal = true)

/** How far [index] in [steps] is along the modality's longest path, in (0, 1]:
 *  step 1 of N already fills 1/N, so the bar is never an empty track. The actual
 *  steps are an ordered subset of that path, so the value only grows as the user
 *  moves forward, and the review is always full. */
internal fun setupProgress(steps: List<SetupStep>, index: Int, modality: Modality?): Float {
    val longest = longestSetupSteps(modality)
    val at = longest.indexOf(steps[index.coerceIn(0, steps.lastIndex)]).coerceAtLeast(0)
    return (at + 1).toFloat() / longest.size
}

/** The path the answers had when an edit started from the review (or when a
 *  Profile Focus switch opened the setup): Next walks only what the edit
 *  changed, then returns to the review. */
internal data class ReviewReturn(val modality: Modality?, val calisthenics: Boolean, val raceGoal: Boolean) : java.io.Serializable {
    val steps: List<SetupStep> get() = setupSteps(modality, calisthenics, raceGoal)
}

/** The next screen after [index] that an edit from the review still has to
 *  visit: one the edit added to the path, one whose answer is now missing
 *  ([ready] false), or the days and level screens when the modality changed
 *  (their question means something else then). Null = straight to the review. */
internal fun nextPendingStep(
    steps: List<SetupStep>,
    index: Int,
    base: ReviewReturn,
    modality: Modality?,
    ready: (SetupStep) -> Boolean,
): Int? {
    val baseSteps = base.steps
    val modalityChanged = modality != base.modality
    return (index + 1 until steps.size).firstOrNull { i ->
        val s = steps[i]
        s != SetupStep.Review && (
            s !in baseSteps || !ready(s) ||
                (modalityChanged && (s == SetupStep.Days || s == SetupStep.Level))
            )
    }
}

/** A race-distance goal (the ones a race date and a recent result apply to). */
internal val GoalDistance.isRace: Boolean
    get() = this == GoalDistance.FiveK || this == GoalDistance.TenK ||
        this == GoalDistance.HalfMarathon || this == GoalDistance.Marathon

// ── Training days ───────────────────────────────────────────────────────────

/** Days in a week: the most training days any plan holds. */
internal const val WEEK_DAYS = 7

/** Every weekday in the `training_weekdays` bitmask (bit0 = Monday … bit6 = Sunday). */
internal const val ALL_WEEKDAYS = 0x7F

/** Days ticked in a weekday mask; null ("Any days") counts 0. */
internal fun weekdayCount(mask: Int?): Int = Integer.bitCount((mask ?: 0) and ALL_WEEKDAYS)

/** Tick/untick one weekday (0 = Monday). Unticking the last day returns null:
 *  no days picked means "Any days". */
internal fun toggleWeekday(mask: Int?, day: Int): Int? =
    ((mask ?: 0) xor (1 shl day)).takeIf { it and ALL_WEEKDAYS != 0 }

/** The setup's days rule: ticked weekdays must cover every training day, or be
 *  "Any days" (null). Shared by the wizard's Days step and Profile › Training days. */
internal fun weekdaysCover(mask: Int?, days: Int): Boolean = mask == null || weekdayCount(mask) >= days

/** Short weekday name, Monday = 0. */
internal fun weekdayShortName(day: Int, locale: Locale = Locale.getDefault()): String =
    DayOfWeek.of(day + 1).getDisplayName(TextStyle.SHORT, locale)

/** "Sat, Sun" for a mask (Monday first). */
internal fun weekdaysLabel(mask: Int, locale: Locale = Locale.getDefault()): String =
    (0 until WEEK_DAYS).filter { mask and (1 shl it) != 0 }.joinToString(", ") { weekdayShortName(it, locale) }

/** Lifting days a pure lifting plan can hold (app::Profile lift_days_per_week). */
internal val LIFT_DAYS = 2..5

/** Training days a running plan can hold. */
private val RUN_DAYS = 2..WEEK_DAYS

/** Lift days the engine schedules for a lift-only profile that never answered
 *  (plan::mod lift_days_per_week fallback). */
internal const val LIFT_ONLY_DEFAULT_DAYS = 3

/** Lift days a Both week starts at before the user changes the split. */
internal const val HYBRID_DEFAULT_LIFT_DAYS = 2

/** The engine's hybrid caps (HYB-RUNCAP-001): while lifting leads, running is
 *  capped at 3 days; while running leads, lifting is held to 3 (maintenance). */
internal const val HYBRID_CAPPED_DAYS = 3

/** Lift-day choices for a Both week of [totalDays]: at least one run day stays,
 *  and a run-led week lifts at most [HYBRID_CAPPED_DAYS] days. */
internal fun hybridLiftDayOptions(totalDays: Int, lead: BothPriority?): IntRange {
    val cap = if (lead == BothPriority.RunLean) HYBRID_CAPPED_DAYS else LIFT_DAYS.last
    return HYBRID_DEFAULT_LIFT_DAYS..maxOf(HYBRID_DEFAULT_LIFT_DAYS, minOf(cap, totalDays - 1))
}

/** Run days the engine schedules in a Both week: the rest of the athlete's days,
 *  capped at [HYBRID_CAPPED_DAYS] while lifting leads. */
internal fun hybridRunDays(totalDays: Int, liftDays: Int, lead: BothPriority?): Int {
    val rest = (totalDays - liftDays).coerceAtLeast(1)
    return if (lead == BothPriority.RunLean) rest else minOf(rest, HYBRID_CAPPED_DAYS)
}

/** Days-step range per modality: a lifting plan holds [LIFT_DAYS] sessions, a Both
 *  week needs room for [HYBRID_DEFAULT_LIFT_DAYS] lift days plus a run. */
internal fun dayOptions(modality: Modality?): IntRange = when (modality) {
    Modality.Lift -> LIFT_DAYS
    Modality.Both -> (HYBRID_DEFAULT_LIFT_DAYS + 1)..WEEK_DAYS
    else -> RUN_DAYS
}

/** Days-step seed for a re-run: the lifting days (Lift), the run days (Run), or
 *  run + lift days (Both, which stores the two sides separately). Lift falls back
 *  to the engine's default for a profile from before the lift-days answer. */
internal fun seedTotalDays(modality: Modality?, initial: ProfileDraft?): Int? = when (modality) {
    Modality.Run -> initial?.runningDaysPerWeek
    Modality.Both -> initial?.runningDaysPerWeek?.let { it + (initial.liftDaysPerWeek ?: HYBRID_DEFAULT_LIFT_DAYS) }
    Modality.Lift -> initial?.liftDaysPerWeek ?: LIFT_ONLY_DEFAULT_DAYS
    null -> null
}?.coerceIn(dayOptions(modality))

/** Days-step seed for a Profile Focus switch to [target]: only days the user
 *  already gave for the sides [target] trains (either side's days may come from
 *  the stash kept when that side was switched off). Null asks afresh. */
internal fun switchSeedDays(target: Modality, initial: ProfileDraft, stashedRunDays: Int?, stashedLiftDays: Int? = null): Int? {
    val liftDays = initial.liftDaysPerWeek?.takeIf { initial.weeklySets > 0 } ?: stashedLiftDays?.takeIf { it > 0 }
    val runDays = initial.runningDaysPerWeek.takeIf { it > 0 } ?: stashedRunDays?.takeIf { it > 0 }
    val total = when (target) {
        Modality.Lift -> liftDays
        Modality.Run -> runDays
        Modality.Both -> if (liftDays != null && runDays != null) liftDays + runDays else null
    }
    return total?.takeIf { it in dayOptions(target) }
}

// ── Running now / race details ──────────────────────────────────────────────

/** Accepted keypad ranges, canonical km. */
internal const val WEEKLY_RUN_MAX_KM = WEEKLY_VOLUME_MAX_KM
internal val LONGEST_RUN_KM = 0.5..60.0

/** A km range shown in the user's distance unit. */
internal fun distanceBounds(km: ClosedFloatingPointRange<Double>, unit: DistanceUnit): ClosedFloatingPointRange<Double> =
    if (unit == DistanceUnit.Mi) roundTenth(kmToMi(km.start))..roundTenth(kmToMi(km.endInclusive)) else km

private fun roundTenth(v: Double): Double = Math.round(v * 10.0) / 10.0

/** A canonical km value as the keypad buffer in the display unit. */
internal fun distanceBufferText(km: Double, unit: DistanceUnit): String =
    trimDecimal(if (unit == DistanceUnit.Mi) roundTenth(kmToMi(km)) else km)

/** A typed display-unit distance back to canonical km. */
internal fun displayToKm(value: Double, unit: DistanceUnit): Double =
    if (unit == DistanceUnit.Mi) miToKmWire(value) else value

/** How close a stored race distance must be to a chip's to count as that chip. */
private const val RACE_DISTANCE_MATCH_KM = 0.01

/** Distance chips for a recent race or time trial. */
internal enum class RaceDistance(val km: Double, val labelRes: Int) {
    Mile(METERS_PER_MILE / 1000.0, R.string.setup_racedist_mile),
    FiveK(5.0, R.string.setup_goaldist_5k),
    TenK(10.0, R.string.setup_goaldist_10k),
    Half(21.0975, R.string.setup_goaldist_half),
    Marathon(42.195, R.string.setup_goaldist_marathon),
    ;

    companion object {
        fun of(km: Double): RaceDistance? = entries.firstOrNull { Math.abs(it.km - km) < RACE_DISTANCE_MATCH_KM }
    }
}

/** Finish-time keypad: digits fill from the right (h mm ss), like a microwave. */
private const val RACE_TIME_DIGITS = 6

/** The digit buffer as seconds; null when empty or minutes/seconds ≥ 60. */
internal fun raceTimeSec(digits: String): Int? {
    if (digits.isEmpty() || digits.length > RACE_TIME_DIGITS) return null
    val p = digits.padStart(RACE_TIME_DIGITS, '0')
    val h = p.substring(0, 2).toInt()
    val m = p.substring(2, 4).toInt()
    val s = p.substring(4, 6).toInt()
    if (m >= 60 || s >= 60) return null
    return (h * 3600 + m * 60 + s).takeIf { it > 0 }
}

/** One keypad key on a finish-time buffer: digits fill from the right and a
 *  [fresh] field starts over; anything else, or a full buffer, is ignored (null). */
internal fun appendRaceDigit(digits: String, key: Char, fresh: Boolean): String? {
    if (!key.isDigit()) return null
    val base = if (fresh) "" else digits
    return if (base.length < RACE_TIME_DIGITS) (base + key).trimStart('0') else null
}

/** The digit buffer as shown: "m:ss" up to four digits, "h:mm:ss" past that. */
internal fun raceTimeDisplay(digits: String): String {
    val p = digits.padStart(4, '0')
    return if (p.length <= 4) {
        "${p.substring(0, 2).toInt()}:${p.substring(2, 4)}"
    } else {
        val h = p.dropLast(4)
        "${h.toInt()}:${p.takeLast(4).substring(0, 2)}:${p.takeLast(2)}"
    }
}

/** Seconds → the digit buffer that displays them. */
internal fun raceTimeDigits(sec: Int): String {
    val h = sec / 3600
    val m = sec % 3600 / 60
    val s = sec % 60
    val raw = if (h > 0) String.format(Locale.US, "%d%02d%02d", h, m, s) else String.format(Locale.US, "%d%02d", m, s)
    return raw.trimStart('0')
}

/** Running paces (min per km) a recent finish time must imply to be believed, so
 *  a typo like "25" (25 seconds) for a 5K is caught before it anchors every pace. */
private val PLAUSIBLE_RACE_PACE_MIN_PER_KM = 2.5..20.0

/** A finish time that is a real running pace ([PLAUSIBLE_RACE_PACE_MIN_PER_KM]). */
internal fun plausibleRaceTime(distanceKm: Double, sec: Int): Boolean =
    (sec / 60.0 / distanceKm) in PLAUSIBLE_RACE_PACE_MIN_PER_KM

// ── Calisthenics placement ──────────────────────────────────────────────────

/** "Where are you now?" self-placement answers → a rung on the core's ladder
 *  (calisthenics::LadderId rung indices, 0-based). "Not sure" sends nothing, so
 *  that ladder starts at its easiest rung; 1–4 and 5+ pull-ups both start at Full
 *  pull-up and the coach skips ahead from logged reps. */
internal enum class CalisPlacement(val ladder: CalisLadder, val rung: Int, val labelRes: Int) {
    PushWall(CalisLadder.HorizontalPush, 0, R.string.setup_place_push_wall),
    PushKnee(CalisLadder.HorizontalPush, 3, R.string.setup_place_push_knee),
    PushFull(CalisLadder.HorizontalPush, 4, R.string.setup_place_push_full),
    PushHard(CalisLadder.HorizontalPush, 5, R.string.setup_place_push_hard),
    PullHang(CalisLadder.VerticalPull, 0, R.string.setup_place_pull_hang),
    PullNegative(CalisLadder.VerticalPull, 3, R.string.setup_place_pull_negative),
    PullFew(CalisLadder.VerticalPull, 4, R.string.setup_place_pull_few),
    PullMany(CalisLadder.VerticalPull, 4, R.string.setup_place_pull_many),
    SquatAssisted(CalisLadder.Squat, 0, R.string.setup_place_squat_assisted),
    SquatBodyweight(CalisLadder.Squat, 1, R.string.setup_place_squat_bodyweight),
    SquatSplit(CalisLadder.Squat, 2, R.string.setup_place_squat_split),
    SquatPistol(CalisLadder.Squat, 4, R.string.setup_place_squat_pistol),
}

/** The three placement questions, in screen order. */
internal val PLACEMENT_LADDERS = listOf(
    CalisLadder.HorizontalPush to R.string.setup_place_push_q,
    CalisLadder.VerticalPull to R.string.setup_place_pull_q,
    CalisLadder.Squat to R.string.setup_place_squat_q,
)

/** One rung event per answered placement (unanswered ladders start easy). */
internal fun placementEvents(answers: Collection<CalisPlacement?>, observedAt: Long): List<Event.SetCalisthenicsRung> =
    answers.filterNotNull().map { Event.SetCalisthenicsRung(it.ladder, it.rung, observedAt) }

/** The rung events a finished setup sends: only placements that differ from the
 *  [seed]. A re-run seeds the ladders' current rungs, and re-placing a ladder on
 *  the rung it already holds would restart its progress there. */
internal fun placementEvents(answers: SetupAnswers, seed: SetupAnswers, observedAt: Long): List<Event.SetCalisthenicsRung> =
    placementEvents(
        PLACEMENT_LADDERS.map { (ladder, _) -> answers.placement(ladder)?.takeIf { it != seed.placement(ladder) } },
        observedAt,
    )

/** The placement answer naming [rung] on [ladder] exactly, or null when no answer
 *  sits on that rung (Few and Many pull-ups share Full pull-up: the first wins). */
internal fun placementAt(ladder: CalisLadder, rung: Int?): CalisPlacement? =
    rung?.let { r -> CalisPlacement.entries.firstOrNull { it.ladder == ladder && it.rung == r } }

/** Each placement ladder's rung the athlete holds now (the placed rung, before the
 *  kit check), from the model's ladder views; empty for a Weights profile. */
internal fun currentPlacementRungs(ladders: List<CalisLadderView>): Map<CalisLadder, Int> =
    ladders.filter { v -> PLACEMENT_LADDERS.any { it.first == v.ladder } }
        .mapNotNull { v -> (v.placed_rung ?: v.current_rung)?.let { v.ladder to it } }
        .toMap()

/** The calisthenics equipment pieces, for the chip rows. */
internal enum class CalisPiece(val labelRes: Int) {
    PullUpBar(R.string.setup_equip_bar),
    DipBars(R.string.setup_equip_dips),
    Rings(R.string.setup_equip_rings),
    ;

    fun has(e: CalisEquipment?): Boolean = when (this) {
        PullUpBar -> e?.pull_up_bar == true
        DipBars -> e?.dip_bars == true
        Rings -> e?.rings == true
    }

    /** Toggle this piece; an unanswered set starts from "none". */
    fun toggle(e: CalisEquipment?): CalisEquipment {
        val base = e ?: CalisEquipment()
        return when (this) {
            PullUpBar -> base.copy(pull_up_bar = !base.pull_up_bar)
            DipBars -> base.copy(dip_bars = !base.dip_bars)
            Rings -> base.copy(rings = !base.rings)
        }
    }
}

/** Something to hang and pull from: every pull-up rung needs one (the core's
 *  calisthenics rung_available, Kit::hang). */
internal val CalisEquipment.canHang: Boolean get() = pull_up_bar || rings

// ── Dates ───────────────────────────────────────────────────────────────────

/** A local epoch day as a medium localized date ("Oct 6, 2026"). */
internal fun epochDayLabel(day: Long, locale: Locale = Locale.getDefault()): String =
    LocalDate.ofEpochDay(day).format(DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM).withLocale(locale))

/** [day] as a short localized date, dropping the year when it is [today]'s year
 *  ("Sep 11"; "Dec 30, 2025" otherwise), for values that must fit a row. */
internal fun epochDayShortLabel(day: Long, today: Long, locale: Locale = Locale.getDefault()): String {
    val date = LocalDate.ofEpochDay(day)
    if (date.year != LocalDate.ofEpochDay(today).year) return epochDayLabel(day, locale)
    val pattern = android.text.format.DateFormat.getBestDateTimePattern(locale, "MMMd")
    return date.format(DateTimeFormatter.ofPattern(pattern, locale))
}

// ── Answers → profile ───────────────────────────────────────────────────────

/**
 * Turn the guided answers into ONE [ProfileDraft]. Plain shell mapping (not a KB
 * training claim, it only picks which registry-backed profile values the user
 * asserted). Modality drives the volumes: Run-only zeroes weekly sets, Lift-only
 * zeroes running, Both splits the chosen days into the lift days the user picked
 * and the run days the engine will actually schedule (capped at 3 while lifting
 * leads, HYB-RUNCAP-001). Weekly running volume is the athlete's stated figure
 * ([weeklyRunKm], 0 = "I don't run yet"); skipped, it is sent as NOT SET and the
 * engine picks its own conservative start, never a shell-invented number.
 * Nothing about the person is required: age (from the birthday, or "Prefer not
 * to say"), bodyweight, sex and resting HR may be null (not given).
 */
internal fun deriveDraft(
    modality: Modality,
    liftFocus: LiftFocus?,
    goalDistance: GoalDistance?,
    bothPriority: BothPriority?,
    daysPerWeek: Int,
    level: SetupLevel,
    ageYears: Double?,
    bodyweightKg: Double?,
    female: Boolean?,
    restingHrBpm: Double?,
    weightLb: Boolean,
    health: HealthScreen,
    initial: ProfileDraft?,
    keepLiftVolume: Boolean,
    keepRunVolume: Boolean,
    keepRunDays: Boolean,
    keepAdvanced: Boolean,
    resistanceModality: ResistanceModality = ResistanceModality.Weights,
    runLevel: SetupLevel? = null,
    liftDays: Int? = null,
    trainingWeekdays: Int? = null,
    weeklyRunKm: Double? = null,
    longestRunKm: Double? = null,
    raceDateEpochDay: Long? = null,
    recentRace: RecentRace? = null,
    meetEpochDay: Long? = null,
    calisEquipment: CalisEquipment? = null,
    birthEpochDay: Long? = null,
): ProfileDraft {
    val lifts = modality != Modality.Run
    val runs = modality != Modality.Lift
    // A Both profile has a separate running level; a single-modality profile's
    // one level is its running level too.
    val runLvl = if (modality == Modality.Both) runLevel ?: level else level
    val lead = bothPriority ?: BothPriority.LiftLean
    val liftDaysVal: Int? = when (modality) {
        Modality.Lift -> daysPerWeek.coerceIn(LIFT_DAYS)
        Modality.Run -> null
        Modality.Both -> {
            val options = hybridLiftDayOptions(daysPerWeek, lead)
            (liftDays ?: HYBRID_DEFAULT_LIFT_DAYS).coerceIn(options.first, options.last)
        }
    }
    // Re-run preservation: when the answers that drive run days are unchanged
    // from the seed, carry the profile's exact run-days count through.
    val runningDays = if (keepRunDays && initial != null) {
        initial.runningDaysPerWeek
    } else when (modality) {
        Modality.Lift -> 0
        Modality.Run -> daysPerWeek
        Modality.Both -> hybridRunDays(daysPerWeek, liftDaysVal ?: HYBRID_DEFAULT_LIFT_DAYS, lead)
    }
    // Run-only carries NO lifting scaffold (weekly_sets = 0) so the modality gate
    // reads it as a pure runner; Lift/Both use the level-scaled per-muscle volume.
    val weeklySets = when (modality) {
        Modality.Run -> 0
        // Re-run preservation: keep the profile's existing per-muscle volume when the
        // user didn't touch the answers that drive it (modality + level).
        else -> if (keepLiftVolume) initial?.weeklySets ?: level.setsPerMuscle else level.setsPerMuscle
    }
    // Weekly distance: the stated answer; a re-run with the driving answers
    // unchanged keeps the profile's value (and whether it was ever given);
    // otherwise a running profile sends it as not set.
    val kept = initial?.takeIf { keepRunVolume }
    val runningKm = when {
        runs && weeklyRunKm != null -> weeklyRunKm
        kept != null -> kept.runningKmPerWeek
        else -> 0.0
    }
    val runningKmSet = when {
        runs && weeklyRunKm != null -> true
        kept != null -> kept.runningKmPerWeekSet
        else -> !runs
    }
    val liftGoal = liftFocus?.liftGoal ?: LiftGoal.MaxStrength
    val concurrentGoal = when (modality) {
        Modality.Lift -> liftGoal.toConcurrent()
        Modality.Run -> ConcurrentGoal.EndurancePriority
        Modality.Both -> leadingConcurrentGoal(lead, liftGoal)
    }
    val goalDistanceVal = when (modality) {
        Modality.Lift -> GoalDistance.General
        else -> goalDistance ?: GoalDistance.TenK
    }
    val race = runs && goalDistanceVal.isRace

    return ProfileDraft(
        progressionCadence = level.cadence,
        liftGoal = liftGoal,
        goalDistance = goalDistanceVal,
        concurrentGoal = concurrentGoal,
        weeklySets = weeklySets,
        runningDaysPerWeek = runningDays,
        runningKmPerWeek = runningKm,
        runningKmPerWeekSet = runningKmSet,
        // Advanced only changes the running plan, so it follows the RUNNING level.
        // Re-run preservation: keep the profile's flag while that level is unchanged.
        advanced = if (keepAdvanced && initial != null) initial.advanced else runLvl == SetupLevel.Advanced,
        female = female,
        bodyweightKg = bodyweightKg,
        ageYears = ageYears,
        birthEpochDay = birthEpochDay,
        restingHrBpm = restingHrBpm,
        // The wizard never asks for a measured HRmax, so re-running must carry the
        // profile's existing value through untouched instead of nulling it.
        measuredHrMax = initial?.measuredHrMax,
        // Display-unit preference, answered on the About-you step.
        weightLb = weightLb,
        health = health,
        // Run-only carries no lifting, so the modality choice is irrelevant: pin it
        // to Weights so a pure runner emits the legacy SetProfile wire.
        resistanceModality = if (lifts) resistanceModality else ResistanceModality.Weights,
        // Training-context fields the wizard has no step for: a re-run carries the
        // profile's values through verbatim.
        highLoadBlock = initial?.highLoadBlock ?: false,
        environment = initial?.environment,
        envTempC = initial?.envTempC,
        envAltitudeM = initial?.envAltitudeM,
        weeksOff = initial?.weeksOff,
        layoffReturnedEpochDay = initial?.layoffReturnedEpochDay,
        liftDaysPerWeek = liftDaysVal,
        trainingWeekdays = trainingWeekdays?.takeIf { it and ALL_WEEKDAYS != 0 },
        goalRaceEpochDay = raceDateEpochDay.takeIf { race },
        recentRace = recentRace.takeIf { race },
        statedLongestRecentKm = longestRunKm.takeIf { runs },
        // A test/meet date only shapes strength or power work.
        meetEpochDay = meetEpochDay.takeIf { lifts && liftGoal != LiftGoal.Hypertrophy },
        calisEquipment = calisEquipment,
        // Profile-only answers: a re-run carries them through.
        specializationMuscle = initial?.specializationMuscle,
        runnerStrength = modality == Modality.Run && initial?.runnerStrength == true,
    )
}

/** Re-run preservation: which profile values a Finish keeps verbatim because
 *  the answers that drive them are unchanged from the seed. */
internal data class KeepFlags(val liftVolume: Boolean, val runVolume: Boolean, val runDays: Boolean, val advanced: Boolean)

internal fun keepFlags(
    initial: ProfileDraft?,
    seedModality: Modality?,
    seedLevel: SetupLevel?,
    seedRunLevel: SetupLevel?,
    seedDays: Int?,
    seedLiftSplit: Int?,
    seedBothPriority: BothPriority?,
    modality: Modality,
    level: SetupLevel,
    runLevel: SetupLevel,
    days: Int,
    liftSplit: Int?,
    bothPriority: BothPriority?,
): KeepFlags {
    if (initial == null) return KeepFlags(liftVolume = false, runVolume = false, runDays = false, advanced = false)
    val sameModality = modality == seedModality
    // A single-modality seed's one level is its running level too.
    val seedRun = seedRunLevel ?: seedLevel
    return KeepFlags(
        liftVolume = sameModality && level == seedLevel,
        runVolume = sameModality && runLevel == seedRun && days == seedDays,
        // Run days follow modality, the days answer, the lifting split and which
        // side leads, so the exact seed count holds only while all of them do.
        runDays = sameModality && days == seedDays && liftSplit == seedLiftSplit && bothPriority == seedBothPriority,
        // Advanced is driven by the running level alone.
        advanced = runLevel == seedRun,
    )
}

// ── The answers as one value ────────────────────────────────────────────────

/**
 * Every guided-setup answer, held as ONE saveable value (Serializable, so the
 * rememberSaveable autoSaver bundles it). No-fabrication: an answer with no seed
 * starts unset (null) and Next stays blocked until the user picks it.
 *
 * Keypad buffers ([bwText] … [longestText]) are in the DISPLAY unit, exactly what
 * is shown and parsed. Losslessness: a seeded buffer shows the profile value at
 * display precision, and each `*Touched` flag flips true only when the user edits
 * that field, so an untouched Finish re-emits the seed double verbatim. Races and
 * meets: a null date = "No date yet"; a null [recentDistance] = "I don't know", and
 * its finish time is the digit buffer [raceDigits]. Calisthenics: null [equipment]
 * = unanswered (the core assumes everything); a null placement = "Not sure, start
 * easy". [weekdays] null = "Any days". [distanceChoice] is the distance unit picked
 * in the setup (null = the app setting), saved as the app setting only on Finish.
 */
internal data class SetupAnswers(
    val modality: Modality?,
    val liftFocus: LiftFocus?,
    val goalDistance: GoalDistance?,
    val bothPriority: BothPriority?,
    val days: Int?,
    val liftSplit: Int?,
    val weekdays: Int?,
    val level: SetupLevel?,
    val runLevel: SetupLevel?,
    val raceDate: Long?,
    val meetDate: Long?,
    val recentDistance: RaceDistance?,
    val raceDigits: String,
    val equipment: CalisEquipment?,
    val pushPlace: CalisPlacement? = null,
    val pullPlace: CalisPlacement? = null,
    val squatPlace: CalisPlacement? = null,
    val resistanceModality: ResistanceModality,
    val weightUnit: WeightUnit,
    val distanceChoice: DistanceUnit? = null,
    val sex: SexAnswer?,
    val health: HealthScreen,
    // Local epoch day of the birthday; null = not picked (a legacy profile may
    // still carry a stated age, which then stands).
    val birthDay: Long?,
    // "Prefer not to say" answered the birthday: no birthday and no age are sent
    // (a legacy stated age is dropped too). Picking a birthday clears it.
    val birthSkipped: Boolean = false,
    val bwText: String,
    val rhrText: String,
    val weeklyText: String,
    val longestText: String,
    val bodyweightTouched: Boolean = false,
    val restingHrTouched: Boolean = false,
    val weeklyTouched: Boolean = false,
    val longestTouched: Boolean = false,
) : java.io.Serializable {
    /** Calisthenics placement applies: lifting is trained, on bodyweight. */
    val calisthenics: Boolean
        get() = modality != Modality.Run && resistanceModality == ResistanceModality.Calisthenics

    /** The screen path these answers take. */
    val path: ReviewReturn get() = ReviewReturn(modality, calisthenics, goalDistance?.isRace == true)

    /** The answer to one calisthenics placement question. */
    fun placement(ladder: CalisLadder): CalisPlacement? = when (ladder) {
        CalisLadder.HorizontalPush -> pushPlace
        CalisLadder.VerticalPull -> pullPlace
        else -> squatPlace
    }

    fun withPlacement(ladder: CalisLadder, answer: CalisPlacement?): SetupAnswers = when (ladder) {
        CalisLadder.HorizontalPush -> copy(pushPlace = answer)
        CalisLadder.VerticalPull -> copy(pullPlace = answer)
        else -> copy(squatPlace = answer)
    }

    fun withBirthday(day: Long): SetupAnswers = copy(birthDay = day, birthSkipped = false)

    fun withoutBirthday(): SetupAnswers = copy(birthDay = null, birthSkipped = true)

    /** Switch the units, converting what is typed so each field keeps meaning the
     *  same amount in the new unit ([shownDistance] = the distance unit the buffers
     *  are in now). An untouched seed still submits the profile's exact canonical
     *  value. */
    fun withUnits(weight: WeightUnit, distance: DistanceUnit, shownDistance: DistanceUnit): SetupAnswers {
        var next = this
        if (weight != weightUnit) {
            val bw = parseKeypadNumber(bwText)?.let { v ->
                bodyweightBufferText(if (weightUnit == WeightUnit.Lb) lbToKg(v) else v, weight)
            }
            next = next.copy(bwText = bw ?: bwText, weightUnit = weight)
        }
        if (distance != shownDistance) {
            fun convert(text: String) = parseKeypadNumber(text)?.let { distanceBufferText(displayToKm(it, shownDistance), distance) } ?: text
            next = next.copy(weeklyText = convert(weeklyText), longestText = convert(longestText), distanceChoice = distance)
        }
        return next
    }
}

/** The weekly distance a running profile (or the stash kept while running was
 *  off) already states; null when never given. */
internal fun seedWeeklyRunKm(initial: ProfileDraft?, runStash: StashedRun?): Double? =
    initial?.takeIf { it.runningDaysPerWeek > 0 && it.runningKmPerWeekSet }?.runningKmPerWeek ?: runStash?.km

/**
 * An existing profile reverse-mapped into the wizard's answers (best-effort: the
 * profile stores per-side days, not the total the user first entered); every
 * answer unset on a first run ([initial] null). The setup opens on these (unless a
 * Focus switch adjusts them, [switchedAnswers]) and compares against them for
 * unsaved changes. [recordedRunLevel] is the shell's record of a Both profile's
 * running level; [runStash] the running answers kept while running was off (only
 * for a switch that turns running back on). Dates already passed are no longer a
 * target, so a re-run asks again. Buffers are in [distanceUnit]. A calisthenics
 * profile's placements are the answers naming its [currentRungs] (a pull-up rung
 * only while the kit has something to hang from).
 */
internal fun seedAnswers(
    initial: ProfileDraft?,
    today: Long,
    recordedRunLevel: SetupLevel?,
    distanceUnit: DistanceUnit,
    runStash: StashedRun?,
    currentRungs: Map<CalisLadder, Int> = emptyMap(),
): SetupAnswers {
    val modality = initial?.let(::modalityOf)
    val both = modality == Modality.Both
    val level = initial?.let { SetupLevel.fromCadence(it.progressionCadence) }
    val weightUnit = seedWeightUnit(initial)
    val recentDistance = initial?.recentRace?.let { RaceDistance.of(it.distance_km) }
    val calis = initial != null && modality != Modality.Run && initial.resistanceModality == ResistanceModality.Calisthenics
    fun seededPlace(ladder: CalisLadder) = if (calis) placementAt(ladder, currentRungs[ladder]) else null
    return SetupAnswers(
        modality = modality,
        // A side the profile doesn't train has no real goal answer (the stored value
        // is a placeholder), so its question opens unanswered.
        liftFocus = initial?.takeIf { it.weeklySets > 0 }?.let { LiftFocus.of(it.liftGoal) },
        goalDistance = initial?.takeIf { it.runningDaysPerWeek > 0 }?.goalDistance,
        bothPriority = if (both) initial?.concurrentGoal?.lead() else null,
        days = seedTotalDays(modality, initial),
        liftSplit = if (both) initial?.liftDaysPerWeek ?: HYBRID_DEFAULT_LIFT_DAYS else null,
        weekdays = initial?.trainingWeekdays,
        level = level,
        // Without a recorded running level a Both profile seeds it from the shared one.
        runLevel = if (both) recordedRunLevel ?: level else null,
        raceDate = (initial?.goalRaceEpochDay ?: runStash?.raceEpochDay)?.takeIf { it > today },
        meetDate = initial?.meetEpochDay?.takeIf { it > today },
        recentDistance = recentDistance,
        raceDigits = initial?.recentRace?.takeIf { recentDistance != null }?.let { raceTimeDigits(it.time_sec) } ?: "",
        equipment = initial?.calisEquipment,
        pushPlace = seededPlace(CalisLadder.HorizontalPush),
        pullPlace = seededPlace(CalisLadder.VerticalPull)?.takeIf { initial?.calisEquipment?.canHang != false },
        squatPlace = seededPlace(CalisLadder.Squat),
        // A first run defaults to Weights, which keeps the legacy wire.
        resistanceModality = initial?.resistanceModality ?: ResistanceModality.Weights,
        weightUnit = weightUnit,
        sex = initial?.let { SexAnswer.of(it.female) },
        // Health starts from the profile (or all-false); no default-on answers.
        health = initial?.health ?: HealthScreen(),
        birthDay = initial?.birthEpochDay,
        // A profile saved with no age at all answered "Prefer not to say".
        birthSkipped = initial != null && initial.birthEpochDay == null && initial.ageYears == null,
        bwText = initial?.bodyweightKg?.let { bodyweightBufferText(it, weightUnit) } ?: "",
        rhrText = initial?.restingHrBpm?.let(::trimDecimal) ?: "",
        weeklyText = seedWeeklyRunKm(initial, runStash)?.let { distanceBufferText(it, distanceUnit) } ?: "",
        longestText = initial?.statedLongestRecentKm?.let { distanceBufferText(it, distanceUnit) } ?: "",
    )
}

/**
 * The answers a Profile Focus switch to [target] opens with: [seed] with that
 * modality picked, and the side it turns back on seeded from the answers kept when
 * that side was switched off ([runStash]/[liftStash], still the user's own answers,
 * shown to confirm). A switch to Both keeps the lift days the user already gave;
 * switching back to a single side restores the weekdays it had.
 */
internal fun switchedAnswers(
    seed: SetupAnswers,
    target: Modality,
    initial: ProfileDraft,
    runStash: StashedRun?,
    liftStash: StashedLift?,
): SetupAnswers = seed.copy(
    modality = target,
    liftFocus = seed.liftFocus ?: liftStash?.let { LiftFocus.of(it.goal) },
    goalDistance = seed.goalDistance ?: runStash?.goal,
    days = switchSeedDays(target, initial, stashedRunDays = runStash?.days, stashedLiftDays = liftStash?.days),
    liftSplit = when {
        target == Modality.Both && seed.modality == Modality.Lift -> initial.liftDaysPerWeek
        target == Modality.Both && seed.modality == Modality.Run -> liftStash?.days
        else -> seed.liftSplit
    },
    weekdays = when (target) {
        Modality.Lift -> if (liftStash != null) liftStash.weekdays else initial.trainingWeekdays
        Modality.Run -> if (runStash != null) runStash.weekdays else initial.trainingWeekdays
        Modality.Both -> initial.trainingWeekdays
    },
)

/** Where a Focus switch opens: the first question [start]'s path adds to [base]
 *  (the profile's current path), or the review when it adds none. */
internal fun switchStartStep(start: SetupAnswers, base: ReviewReturn): Int {
    val path = start.path.steps
    return nextPendingStep(path, 0, base, start.modality) { true } ?: path.lastIndex
}

/**
 * What the answers mean right now: the typed numbers parsed as they would be
 * submitted, their validity, and the days split. Person values: untouched → the
 * profile's value verbatim; touched → the typed buffer when in range, else null.
 * A typed-but-invalid buffer blocks Next, so nothing is ever submitted that the
 * field doesn't show. [seedWeeklyKm] stands in for an untouched weekly distance.
 * [today] (local epoch day) turns the picked birthday into the age shown and
 * gated on before Finish.
 */
internal class SetupValues(
    private val answers: SetupAnswers,
    private val seed: SetupAnswers,
    private val initial: ProfileDraft?,
    seedWeeklyKm: Double?,
    distanceUnit: DistanceUnit,
    private val today: Long,
) {
    val weeklyBounds = distanceBounds(0.0..WEEKLY_RUN_MAX_KM, distanceUnit)
    val longestBounds = distanceBounds(LONGEST_RUN_KM, distanceUnit)
    val bodyweightRange = bodyweightBounds(answers.weightUnit)

    /** The birthday's age on [today]; without one, a legacy profile's stated age
     *  unless the user chose "Prefer not to say". */
    val ageYears: Double? = answers.birthDay?.let { ageOnDay(it, today).toDouble() }
        ?: initial?.ageYears?.takeIf { !answers.birthSkipped }
    /** The birthday question has an answer: an age, or "Prefer not to say". */
    val ageAnswered: Boolean = ageYears != null || answers.birthSkipped
    /** Never asked in the setup: a re-run carries the profile's value through. */
    val measuredHrMax: Double? = initial?.measuredHrMax
    val bodyweightKg: Double? = if (answers.bodyweightTouched) {
        parseInRange(answers.bwText, bodyweightRange)?.let { bodyweightDisplayToKg(it, answers.weightUnit) }
    } else {
        initial?.bodyweightKg
    }
    val restingHrBpm: Double? = if (answers.restingHrTouched) parseInRange(answers.rhrText, RESTING_HR_RANGE) else initial?.restingHrBpm
    val weeklyRunKm: Double? = if (answers.weeklyTouched) {
        parseInRange(answers.weeklyText, weeklyBounds)?.let { displayToKm(it, distanceUnit) }
    } else {
        seedWeeklyKm
    }
    val longestRunKm: Double? = if (answers.longestTouched) {
        parseInRange(answers.longestText, longestBounds)?.let { displayToKm(it, distanceUnit) }
    } else {
        initial?.statedLongestRecentKm
    }
    val bodyweightInvalid = answers.bodyweightTouched && answers.bwText.isNotEmpty() && bodyweightKg == null
    val restingHrInvalid = answers.restingHrTouched && answers.rhrText.isNotEmpty() && restingHrBpm == null
    val weeklyInvalid = answers.weeklyTouched && answers.weeklyText.isNotEmpty() && weeklyRunKm == null
    val longestInvalid = answers.longestTouched && answers.longestText.isNotEmpty() && longestRunKm == null

    /** The recent race, once a distance has a real finish time. */
    val recentRace: RecentRace? = answers.recentDistance?.let { d ->
        raceTimeSec(answers.raceDigits)?.takeIf { plausibleRaceTime(d.km, it) }?.let { RecentRace(d.km, it) }
    }
    val raceTimeInvalid = answers.recentDistance != null && answers.raceDigits.isNotEmpty() && recentRace == null

    val dayRange = dayOptions(answers.modality)
    val totalDays: Int? = answers.days?.takeIf { it in dayRange }
    val liftOptions: IntRange? = totalDays?.let { hybridLiftDayOptions(it, answers.bothPriority) }
    /** The lift-day split as shown: the answer (or the default) inside [liftOptions]. */
    val liftSplit: Int? = liftOptions?.let { (answers.liftSplit ?: HYBRID_DEFAULT_LIFT_DAYS).coerceIn(it.first, it.last) }
    /** Ticked weekdays that don't cover every training day ("Any days" always does). */
    val weekdaysShort = totalDays != null && !weekdaysCover(answers.weekdays, totalDays)

    /** Whether a step's answers let the user move on (Next's gate; also what an
     *  edit from the review checks to know which later screens it invalidated). */
    fun ready(step: SetupStep): Boolean = when (step) {
        SetupStep.Train -> answers.modality != null
        SetupStep.LiftGoal -> answers.liftFocus != null
        SetupStep.Placement -> true
        SetupStep.RunGoal -> answers.goalDistance != null
        SetupStep.Race -> answers.recentDistance == null || recentRace != null
        SetupStep.Lead -> answers.bothPriority != null
        SetupStep.Days -> totalDays != null && !weekdaysShort
        SetupStep.Level -> answers.level != null && (answers.modality != Modality.Both || answers.runLevel != null)
        SetupStep.RunNow -> !weeklyInvalid && !longestInvalid
        // The birthday needs an answer, a date or "Prefer not to say" (a legacy
        // stated age still counts); bodyweight and resting HR may stay empty, but
        // never submit a value marked invalid.
        SetupStep.About -> ageAnswered && !bodyweightInvalid && !restingHrInvalid
        SetupStep.Health, SetupStep.Review -> true
    }

    /** The answers mapped to the profile Finish would send; null until the required
     *  answers exist. The review renders this same draft, so what the user reviews
     *  is exactly what is saved. */
    fun draft(): ProfileDraft? {
        val m = answers.modality ?: return null
        val l = answers.level ?: return null
        val d = answers.days ?: return null
        val rl = if (m == Modality.Both) answers.runLevel ?: return null else l
        val keep = keepFlags(
            initial, seed.modality, seed.level, seed.runLevel, seed.days, seed.liftSplit, seed.bothPriority,
            m, l, rl, d, answers.liftSplit, answers.bothPriority,
        )
        return deriveDraft(
            modality = m,
            liftFocus = answers.liftFocus,
            goalDistance = answers.goalDistance,
            bothPriority = answers.bothPriority,
            daysPerWeek = d,
            level = l,
            ageYears = ageYears,
            bodyweightKg = bodyweightKg,
            female = answers.sex?.female,
            restingHrBpm = restingHrBpm,
            weightLb = answers.weightUnit == WeightUnit.Lb,
            health = answers.health,
            initial = initial,
            // "Only overwrite what the user actually changed": preserve the
            // existing values whose driving answers are untouched vs the seed.
            keepLiftVolume = keep.liftVolume,
            keepRunVolume = keep.runVolume,
            keepRunDays = keep.runDays,
            keepAdvanced = keep.advanced,
            resistanceModality = answers.resistanceModality,
            runLevel = rl,
            liftDays = answers.liftSplit,
            trainingWeekdays = answers.weekdays,
            weeklyRunKm = weeklyRunKm,
            longestRunKm = longestRunKm,
            raceDateEpochDay = answers.raceDate,
            recentRace = recentRace,
            meetEpochDay = answers.meetDate,
            calisEquipment = answers.equipment,
            birthEpochDay = answers.birthDay,
        )
    }
}
