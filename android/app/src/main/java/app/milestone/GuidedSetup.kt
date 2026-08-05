package app.milestone

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp

/** The three training modalities the guided setup maps onto the profile's volume
 *  fields. Mirrors the [Focus] row in the full editor: Run-only zeroes lifting,
 *  Lift-only zeroes running, Both splits the week. */
private enum class Modality(val title: String, val consequence: String) {
    Lift("Lifting / strength", "Barbell & machine work. No running is scheduled unless you add it later."),
    Run("Running / endurance", "Endurance-focused plan. Lifting isn't scheduled unless you add it later."),
    Both("Both - hybrid", "Runs and lifts are spaced across your week so one doesn't blunt the other."),
}

/** The lifting intent → the profile's lift goal + (for a single-modality profile)
 *  its concurrent goal. */
private enum class LiftFocus(
    val title: String,
    val consequence: String,
    val liftGoal: LiftGoal,
    val concurrent: ConcurrentGoal,
) {
    Strength("Getting stronger", "Heavy, lower-rep strength work.", LiftGoal.MaxStrength, ConcurrentGoal.Strength),
    Muscle("Building muscle", "Higher volume per muscle for growth.", LiftGoal.Hypertrophy, ConcurrentGoal.Hypertrophy),
    Power("Power / explosiveness", "Fast, forceful lifting.", LiftGoal.Power, ConcurrentGoal.Power),
}

/** For a Both profile: which side leads → the concurrent goal (lift-lean keeps the
 *  lift goal leading, run-lean flips it to EndurancePriority). */
private enum class BothPriority(val title: String, val consequence: String) {
    LiftLean("Lifting matters more", "Strength leads; running stays supportive."),
    RunLean("Running matters more", "Endurance leads; lifting stays supportive."),
}

/** Rough training experience → progression cadence + starting volumes. */
private enum class SetupLevel(val title: String, val consequence: String) {
    Novice("New to this", "Progresses every session; starts at conservative volumes so you build a base without digging a hole."),
    Intermediate("A year or two in", "Week-to-week progression; moderate starting volumes."),
    Advanced("Experienced", "Month-to-month progression; higher starting volumes and an extended running base."),
}

/** A one-line, plain-language blurb for each race-distance goal (display only). */
private val GoalDistance.blurb: String
    get() = when (this) {
        GoalDistance.General -> "Overall endurance, no race on the calendar."
        GoalDistance.C25k -> "Couch-to-5K - build up to running 5K."
        GoalDistance.FiveK -> "Sharpen speed for a fast 5K."
        GoalDistance.TenK -> "Balance speed and endurance for 10K."
        GoalDistance.HalfMarathon -> "Half-marathon endurance base."
        GoalDistance.Marathon -> "Full marathon - the highest weekly mileage."
    }

/** For a Both plan, how the total training days split (lift-leaning). Kept in one
 *  place so the days-step consequence line and [deriveDraft] agree. */
private fun liftDaysFor(totalDays: Int): Int = (totalDays + 1) / 2

/**
 * Turn the guided answers into ONE [ProfileDraft]. Plain shell mapping (not a KB
 * training claim, it only picks which registry-backed profile values the user
 * asserted). Modality drives the volumes: Run-only zeroes weekly sets, Lift-only
 * zeroes running, Both splits the chosen days. Running volume derives from the run
 * days × an experience-scaled per-run distance (a novice running three days starts
 * near 15 km/wk, not 45).
 */
private fun deriveDraft(
    modality: Modality,
    liftFocus: LiftFocus?,
    goalDistance: GoalDistance?,
    bothPriority: BothPriority?,
    daysPerWeek: Int,
    level: SetupLevel,
    ageYears: Double?,
    bodyweightKg: Double?,
    female: Boolean,
    restingHrBpm: Double?,
    health: HealthScreen,
): ProfileDraft {
    val cadence = when (level) {
        SetupLevel.Novice -> ProgressionCadence.EverySession
        SetupLevel.Intermediate -> ProgressionCadence.WeekToWeek
        SetupLevel.Advanced -> ProgressionCadence.MonthToMonth
    }
    val setsPerMuscle = when (level) {
        SetupLevel.Novice -> 8
        SetupLevel.Intermediate -> 12
        SetupLevel.Advanced -> 16
    }
    val kmPerRunDay = when (level) {
        SetupLevel.Novice -> 5.0
        SetupLevel.Intermediate -> 8.0
        SetupLevel.Advanced -> 11.0
    }
    val runningDays = when (modality) {
        Modality.Lift -> 0
        Modality.Run -> daysPerWeek
        Modality.Both -> daysPerWeek - liftDaysFor(daysPerWeek)
    }
    // Run-only carries NO lifting scaffold (weekly_sets = 0) so the modality gate
    // reads it as a pure runner; Lift/Both use the level-scaled per-muscle volume.
    val weeklySets = when (modality) {
        Modality.Run -> 0
        else -> setsPerMuscle
    }
    val liftGoal = liftFocus?.liftGoal ?: LiftGoal.MaxStrength
    val concurrentGoal = when (modality) {
        Modality.Lift -> liftFocus?.concurrent ?: ConcurrentGoal.Strength
        Modality.Run -> ConcurrentGoal.EndurancePriority
        Modality.Both -> when (bothPriority) {
            BothPriority.RunLean -> ConcurrentGoal.EndurancePriority
            else -> liftFocus?.concurrent ?: ConcurrentGoal.Strength
        }
    }
    val goalDistanceVal = when (modality) {
        Modality.Lift -> GoalDistance.General
        else -> goalDistance ?: GoalDistance.TenK
    }

    return ProfileDraft(
        progressionCadence = cadence,
        liftGoal = liftGoal,
        goalDistance = goalDistanceVal,
        concurrentGoal = concurrentGoal,
        weeklySets = weeklySets,
        runningDaysPerWeek = runningDays,
        runningKmPerWeek = runningDays * kmPerRunDay,
        advanced = level == SetupLevel.Advanced,
        enduranceIntensityPctVo2max = 75.0,
        female = female,
        bodyweightKg = bodyweightKg,
        ageYears = ageYears,
        restingHrBpm = restingHrBpm,
        measuredHrMax = null,
        health = health,
    )
}

/**
 * Guided setup (M5, redesigned 2026-08-04: modality-first, with a health screen).
 * Six plain-language steps, what you train, your goal, days/week, level, about you,
 * and a health screen, each with a one-line consequence, ending in a single
 * [Event.SetProfile] (the same wire the full editor uses). Seedable: when [initial]
 * is non-null (the Profile "Re-run guided setup" row) every answer is pre-filled
 * from the current profile so the user can review and adjust.
 */
@Composable
fun GuidedSetup(
    initial: ProfileDraft? = null,
    onComplete: (ProfileDraft) -> Unit,
    onSkip: () -> Unit,
) {
    // Reverse-map an existing profile into the wizard's answers (best-effort, the
    // profile stores per-muscle sets + run days, not a total-days count, so `days`
    // is approximated from the run side and the user confirms it).
    val seedModality = initial?.let {
        when {
            it.weeklySets > 0 && it.runningDaysPerWeek > 0 -> Modality.Both
            it.weeklySets > 0 -> Modality.Lift
            it.runningDaysPerWeek > 0 -> Modality.Run
            else -> null
        }
    }
    val seedLiftFocus = initial?.let { d -> LiftFocus.entries.firstOrNull { it.liftGoal == d.liftGoal } }
    val seedLevel = initial?.let {
        when (it.progressionCadence) {
            ProgressionCadence.EverySession -> SetupLevel.Novice
            ProgressionCadence.WeekToWeek -> SetupLevel.Intermediate
            ProgressionCadence.MonthToMonth -> SetupLevel.Advanced
        }
    }
    val seedDays = when (seedModality) {
        Modality.Run -> initial?.runningDaysPerWeek
        Modality.Both -> initial?.runningDaysPerWeek?.let { it * 2 }
        Modality.Lift -> 3
        null -> null
    }?.coerceIn(2, 7)

    var step by rememberSaveable { mutableIntStateOf(0) }
    // No-fabrication: every non-seeded answer starts UNSET (null) and the submit is
    // blocked until the user picks it. When [initial] seeds a value the step opens
    // pre-answered (a re-run), which is the intended review flow.
    var modality by rememberSaveable { mutableStateOf(seedModality) }
    var liftFocus by rememberSaveable { mutableStateOf(seedLiftFocus) }
    var goalDistance by rememberSaveable { mutableStateOf(initial?.goalDistance) }
    var bothPriority by rememberSaveable {
        mutableStateOf(
            if (seedModality == Modality.Both) {
                if (initial?.concurrentGoal == ConcurrentGoal.EndurancePriority) BothPriority.RunLean else BothPriority.LiftLean
            } else {
                null
            },
        )
    }
    var days by rememberSaveable { mutableStateOf(seedDays) }
    var level by rememberSaveable { mutableStateOf(seedLevel) }
    var age by rememberSaveable { mutableStateOf(initial?.ageYears?.toInt()) }
    var bodyweight by rememberSaveable { mutableStateOf(initial?.bodyweightKg?.toInt()) }
    var female by rememberSaveable { mutableStateOf(initial?.female) }
    var restingHr by rememberSaveable { mutableStateOf(initial?.restingHrBpm?.toInt()) }
    // Health starts from the seed (or all-false); no default-on answers.
    var health by rememberSaveable { mutableStateOf(initial?.health ?: HealthScreen()) }

    val lastStep = 5
    val canAdvance = when (step) {
        0 -> modality != null
        1 -> when (modality) {
            Modality.Lift -> liftFocus != null
            Modality.Run -> goalDistance != null
            Modality.Both -> liftFocus != null && goalDistance != null && bothPriority != null
            null -> false
        }
        2 -> days != null
        3 -> level != null
        4 -> age != null && bodyweight != null && female != null
        5 -> true // health: no raised gate is a valid answer
        else -> false
    }

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(BgTop),
    ) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(horizontal = Space.Screen.dp)
                .padding(top = Space.Lg.dp, bottom = Space.Lg.dp),
        ) {
            // Header: skip + step progress.
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Set up your profile", color = OnBgBody, style = Type.Title)
                TextButton(onClick = onSkip) {
                    Text("Skip", color = OnBgMuted, style = Type.Body)
                }
            }
            Spacer(Modifier.size(Space.Md.dp))
            StepDots(current = step, total = lastStep + 1)
            Spacer(Modifier.size(Space.Lg.dp))

            Column(
                modifier = Modifier
                    .weight(1f)
                    .verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
            ) {
                when (step) {
                    0 -> {
                        StepHeader("What do you train?", "This sets your plan's shape - the details are all editable later.")
                        Modality.entries.forEach { m ->
                            SetupChoiceCard(m.title, m.consequence, selected = modality == m) { modality = m }
                        }
                    }
                    1 -> when (modality) {
                        Modality.Lift -> {
                            StepHeader("What's your lifting goal?", "Sets your rep ranges and how load progresses.")
                            LiftFocus.entries.forEach { g ->
                                SetupChoiceCard(g.title, g.consequence, selected = liftFocus == g) { liftFocus = g }
                            }
                        }
                        Modality.Run -> {
                            StepHeader("What are you training for?", "Sets the distance your running plan builds toward.")
                            GoalDistance.entries.forEach { d ->
                                SetupChoiceCard(d.label, d.blurb, selected = goalDistance == d) { goalDistance = d }
                            }
                        }
                        Modality.Both -> {
                            StepHeader("Set your lift and run goals", "Both sides get scheduled - then tell us which one leads.")
                            Text("Lifting", color = OnBgMuted, style = Type.Body.copy(fontWeight = FontWeight.Bold))
                            LiftFocus.entries.forEach { g ->
                                SetupChoiceCard(g.title, g.consequence, selected = liftFocus == g) { liftFocus = g }
                            }
                            Spacer(Modifier.size(Space.Sm.dp))
                            Text("Running", color = OnBgMuted, style = Type.Body.copy(fontWeight = FontWeight.Bold))
                            GoalDistance.entries.forEach { d ->
                                SetupChoiceCard(d.label, d.blurb, selected = goalDistance == d) { goalDistance = d }
                            }
                            Spacer(Modifier.size(Space.Sm.dp))
                            Text("Which matters more?", color = OnBgMuted, style = Type.Body.copy(fontWeight = FontWeight.Bold))
                            BothPriority.entries.forEach { p ->
                                SetupChoiceCard(p.title, p.consequence, selected = bothPriority == p) { bothPriority = p }
                            }
                        }
                        null -> {}
                    }
                    2 -> {
                        StepHeader("How many days a week?", "Realistic days you can train. We scale your volume to fit - no all-or-nothing week.")
                        Spacer(Modifier.size(Space.Sm.dp))
                        // `days ?: 0` is out of the 2..7 grid, so nothing is highlighted
                        // until the user actually taps a day: no pre-filled default.
                        LabeledScale("Days per week", (2..7).toList(), days ?: 0, { "$it" }) { days = it }
                        val d = days
                        if (d == null) {
                            Text("Pick how many days to continue.", color = OnBgMuted, style = Type.Caption)
                        } else if (modality == Modality.Both) {
                            // Honest split so the user sees the consequence of the total.
                            val lift = liftDaysFor(d)
                            Text(
                                "That's $lift lift ${if (lift == 1) "day" else "days"} and ${d - lift} run ${if (d - lift == 1) "day" else "days"} a week.",
                                color = OnBgMuted,
                                style = Type.Caption,
                            )
                        }
                    }
                    3 -> {
                        StepHeader("Roughly your current level?", "Sets how fast you add load and where your starting volumes sit.")
                        SetupLevel.entries.forEach { l ->
                            SetupChoiceCard(l.title, l.consequence, selected = level == l) { level = l }
                        }
                    }
                    4 -> {
                        StepHeader("A little about you", "Entered once here - the Coach protein and HR-zone tools prefill from it instead of asking again.")
                        // `?: 0` sits outside each grid, so nothing is highlighted until
                        // the user picks it; no age/bodyweight/HR is defaulted and then
                        // silently submitted as if answered.
                        LabeledScale("Age (years)", (14..90).toList(), age ?: 0, { "$it" }) { age = it }
                        LabeledScale("Bodyweight (kg)", (40..150).toList(), bodyweight ?: 0, { "$it" }) { bodyweight = it }
                        Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
                            Text("Sex", color = OnBgMuted, style = Type.Body)
                            // Nullable options list so `null` (unset) highlights neither
                            // Male nor Female.
                            ChoiceScaleRow(
                                listOf<Boolean?>(false, true),
                                female,
                                { if (it == true) "Female" else "Male" },
                            ) { female = it }
                        }
                        LabeledScale("Resting HR - optional", (35..90).toList(), restingHr ?: 0, { "$it bpm" }) { restingHr = it }
                        if (age == null || bodyweight == null || female == null) {
                            Text("Pick your age, bodyweight and sex to finish.", color = OnBgMuted, style = Type.Caption)
                        }
                    }
                    5 -> {
                        StepHeader(
                            "Anything we should know?",
                            "These keep you safe. When one applies, milestone won't generate a plan and defers to a professional instead - you can still log and track.",
                        )
                        HealthSwitchRow(
                            "Positive health screen (PAR-Q+)",
                            "Known heart, metabolic or kidney condition, uncontrolled blood pressure, recent surgery, or a doctor told you to check before vigorous exercise.",
                            health.parq_positive,
                        ) {
                            // Clear the dependent child when the parent turns OFF (else a
                            // stale `medically_cleared` would defeat the referral gate).
                            health = health.copy(
                                parq_positive = it,
                                medically_cleared = if (it) health.medically_cleared else false,
                            )
                        }
                        if (health.parq_positive) {
                            HealthSwitchRow(
                                "Cleared by a doctor",
                                "A clinician has cleared you to train since that positive screen.",
                                health.medically_cleared,
                            ) { health = health.copy(medically_cleared = it) }
                        }
                        HealthSwitchRow(
                            "Currently pregnant",
                            "The engine defers autonomous prescription during pregnancy and individualises with your provider.",
                            health.pregnant,
                        ) {
                            health = health.copy(
                                pregnant = it,
                                pregnancy_warning_sign = if (it) health.pregnancy_warning_sign else false,
                            )
                        }
                        if (health.pregnant) {
                            HealthSwitchRow(
                                "Pregnancy warning sign present",
                                "Bleeding, breathlessness before exertion, chest pain, or reduced fetal movement - stop and seek care.",
                                health.pregnancy_warning_sign,
                            ) { health = health.copy(pregnancy_warning_sign = it) }
                        }
                        HealthSwitchRow(
                            "Injury, recent surgery, or in rehab",
                            "The engine never prescribes rehabilitation - resume general programming only once cleared.",
                            health.injury_or_rehab,
                        ) { health = health.copy(injury_or_rehab = it) }
                        HealthSwitchRow(
                            "Under-fuelling / disordered-eating signal",
                            "Missed periods, rapid weight loss, compulsive exercise, or persistent unexplained fatigue - routes to a professional (RED-S).",
                            health.reds_signal,
                        ) { health = health.copy(reds_signal = it) }

                        val defers = (health.parq_positive && !health.medically_cleared) ||
                            health.pregnant || health.pregnancy_warning_sign ||
                            health.injury_or_rehab || health.reds_signal
                        if (defers) {
                            Text(
                                "Based on your answers, milestone will hold off on generating a plan and point you to a professional first. Your safety comes before any training goal.",
                                color = OnBgMuted,
                                style = Type.Caption,
                            )
                        }
                    }
                }
            }

            Spacer(Modifier.size(Space.Md.dp))
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
            ) {
                if (step > 0) {
                    OutlinedButton(
                        onClick = { step-- },
                        modifier = Modifier.weight(1f),
                    ) { Text("Back") }
                }
                Button(
                    onClick = {
                        if (step < lastStep) {
                            step++
                        } else {
                            // Every required answer is guaranteed set here by canAdvance;
                            // the elvis returns keep the mapping total without defaulting.
                            val m = modality ?: return@Button
                            val l = level ?: return@Button
                            val d = days ?: return@Button
                            val f = female ?: return@Button
                            onComplete(
                                deriveDraft(
                                    modality = m,
                                    liftFocus = liftFocus,
                                    goalDistance = goalDistance,
                                    bothPriority = bothPriority,
                                    daysPerWeek = d,
                                    level = l,
                                    ageYears = age?.toDouble(),
                                    bodyweightKg = bodyweight?.toDouble(),
                                    female = f,
                                    restingHrBpm = restingHr?.toDouble(),
                                    health = health,
                                ),
                            )
                        }
                    },
                    enabled = canAdvance,
                    modifier = Modifier.weight(1f),
                ) { Text(if (step < lastStep) "Next" else "Finish") }
            }
        }
    }
}

/** The step's question (bold) + a plain-language consequence line under it. */
@Composable
private fun StepHeader(question: String, subtitle: String) {
    Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
        Text(question, color = OnBgBody, style = Type.Title)
        Text(subtitle, color = OnBgMuted, style = Type.Body)
    }
}

/** A selectable option: title + one-line consequence, accent border when chosen. */
@Composable
private fun SetupChoiceCard(title: String, subtitle: String, selected: Boolean, onClick: () -> Unit) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(Space.Card.dp))
            .background(if (selected) Accent.copy(alpha = 0.14f) else BgElevated)
            .border(
                width = if (selected) 2.dp else 1.dp,
                color = if (selected) Accent else OnBgBody.copy(alpha = 0.06f),
                shape = RoundedCornerShape(Space.Card.dp),
            )
            .clickable { onClick() }
            .padding(Space.Card.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Xs.dp),
    ) {
        Text(title, color = OnBgBody, style = Type.Body.copy(fontWeight = FontWeight.Bold))
        Text(subtitle, color = OnBgMuted, style = Type.Caption)
    }
}

/** A labelled horizontally-scrollable numeric scale (reuses [ScrollableScaleRow]). */
@Composable
private fun LabeledScale(
    label: String,
    options: List<Int>,
    current: Int,
    render: (Int) -> String,
    onSelect: (Int) -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
        Text(label, color = OnBgMuted, style = Type.Body)
        ScrollableScaleRow(options, current, render, onSelect)
    }
}

/** A row of progress dots; the current step's dot is accent-filled and wider. */
@Composable
private fun StepDots(current: Int, total: Int) {
    Row(horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
        repeat(total) { i ->
            Box(
                modifier = Modifier
                    .heightIn(min = 6.dp)
                    .size(width = if (i == current) 22.dp else 6.dp, height = 6.dp)
                    .clip(CircleShape)
                    .background(if (i <= current) Accent else OnBgFaint.copy(alpha = 0.4f)),
            )
        }
    }
}
