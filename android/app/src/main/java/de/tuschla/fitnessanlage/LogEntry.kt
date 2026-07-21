package de.tuschla.fitnessanlage

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import java.util.Locale

/**
 * Manual lift-set entry. Emits a real [Event.LogSet] with the user's own
 * exercise / weight / reps / RPE, the e1RM + RIR derivation lives in the Rust
 * core, so this form carries no coaching logic.
 */
@Composable
fun LogSetEditor(onLog: (Event.LogSet) -> Unit) {
    var exercise by rememberSaveable { mutableStateOf("Back Squat") }
    var weightKg by rememberSaveable { mutableStateOf(100.0) }
    var weightValid by rememberSaveable { mutableStateOf(true) }
    var reps by rememberSaveable { mutableStateOf(5) }
    var rpe by rememberSaveable { mutableStateOf(8.0) }

    FormCard {
        FieldLabel("Exercise")
        OutlinedTextField(
            value = exercise,
            onValueChange = { exercise = it },
            singleLine = true,
            placeholder = { Text("Search exercise") },
            modifier = Modifier.fillMaxWidth(),
        )
        PresetChipsRow(listOf("Back Squat", "Bench", "Deadlift", "OHP"), exercise) { exercise = it }
        // Weight: big editable value + plate-jump quick-adjust (−2.5 / +2.5 / +5).
        // The validity flag gates the submit button so what is logged is always
        // exactly what the field shows (never a stale committed value behind an
        // out-of-range or cleared display).
        BigValueField(
            "Weight", "kg", weightKg, "%.1f", 0.0, 400.0, listOf(-2.5, 2.5, 5.0),
            onValidChange = { weightValid = it },
        ) {
            weightKg = it
        }
        // Reps: a tap-scale over 1–20 rather than a stepper.
        FieldLabel("Reps", "$reps")
        ScrollableScaleRow((1..20).toList(), reps, { "$it" }) { reps = it }
        // RPE: fixed half-point scale; RIR is RPE's definition (10 − RPE), shown as
        // a hint: the authoritative RIR is still derived in the core on log.
        FieldLabel("RPE", "${fmtRpe(rpe)} · RIR ${(10.0 - rpe).toInt()}")
        ChoiceScaleRow(listOf(6.0, 7.0, 7.5, 8.0, 8.5, 9.0, 10.0), rpe, { fmtRpe(it) }) { rpe = it }
        Button(
            onClick = { onLog(Event.LogSet(exercise.trim(), weightKg, reps, rpe)) },
            enabled = exercise.isNotBlank() && weightValid,
            modifier = Modifier.fillMaxWidth(),
        ) { Text("Log set") }
    }
}

/** `8` for a whole RPE, `7.5` otherwise, display only. */
private fun fmtRpe(v: Double): String =
    if (v % 1.0 == 0.0) "${v.toInt()}" else String.format(Locale.US, "%.1f", v)

/**
 * Manual run entry for runs recorded without GPS tracking. `longestRecentKm` is
 * left at 0, the core derives the spike baseline from prior logged runs.
 */
@Composable
fun LogRunEditor(onLog: (Event.LogRun) -> Unit) {
    var distanceKm by rememberSaveable { mutableStateOf(10.0) }
    var distanceValid by rememberSaveable { mutableStateOf(true) }
    var durationMin by rememberSaveable { mutableStateOf(50) }
    var durationValid by rememberSaveable { mutableStateOf(true) }
    // The core treats hr_pct_max == 0 as "no HR sample" and reports zone "-"
    // rather than fabricating one. Gate HR behind a toggle so a run logged without
    // a monitor sends 0 instead of a made-up %, keeping the zone honest.
    var hasHr by rememberSaveable { mutableStateOf(false) }
    var hrPctMax by rememberSaveable { mutableStateOf(78) }

    FormCard {
        // Both fields gate the submit on their validity flag so the logged run is
        // always exactly the displayed numbers (see LogSetEditor's weight note).
        BigValueField(
            "Distance", "km", distanceKm, "%.2f", 0.0, 100.0, listOf(-0.5, 0.5, 1.0),
            onValidChange = { distanceValid = it },
        ) {
            distanceKm = it
        }
        BigValueField(
            "Duration", "min", durationMin.toDouble(), "%.0f", 0.0, 600.0, listOf(-1.0, 1.0, 5.0),
            onValidChange = { durationValid = it },
        ) {
            durationMin = it.toInt()
        }
        SwitchRow("Recorded HR", hasHr) { hasHr = it }
        if (hasHr) {
            FieldLabel("HR", "% max")
            ChoiceScaleRow(listOf(60, 65, 70, 75, 80, 85, 90, 95), hrPctMax, { "$it" }) { hrPctMax = it }
        }
        Button(
            onClick = {
                onLog(
                    Event.LogRun(
                        distanceKm = distanceKm,
                        durationMin = durationMin.toDouble(),
                        hrPctMax = if (hasHr) hrPctMax.toDouble() else 0.0,
                        longestRecentKm = 0.0,
                    )
                )
            },
            enabled = distanceKm > 0.0 && durationMin > 0 && distanceValid && durationValid,
            modifier = Modifier.fillMaxWidth(),
        ) { Text("Log run") }
    }
}

/**
 * Manual readiness entry. Surfaces every [ReadinessSignal], including the
 * medical-referral flags (Illness/RedS/CardiacRedFlag/BoneStress) that gate the
 * top of the safety ladder, so the core's safety layer is reachable from the
 * device. Signal semantics differ (z-scores, bpm deltas, %, 0/1 flags), so the
 * value stepper spans a deliberately wide range; the core interprets it.
 */
@Composable
fun ReadinessEditor(onSubmit: (Event.SubmitReadiness) -> Unit) {
    var signal by rememberSaveable { mutableStateOf(ReadinessSignal.WellnessZ) }
    var value by rememberSaveable { mutableStateOf(defaultReadinessValue(ReadinessSignal.WellnessZ)) }

    FormCard {
        // Reset the value to the newly-selected signal's sensible default: the core
        // reads a flag signal as "present" only when value > 0 (Pain/RED-S/cardiac/
        // bone-stress) and an Illness by severity band, so carrying over a 0.0 or a
        // z-score would silently submit a *non-triggering* red flag, the opposite
        // of what a user picking "Pain" intends.
        EnumRow(
            "Signal",
            ReadinessSignal.entries,
            signal,
            display = { it.label },
            // Fence the medical red-flag block (Pain onward) off from the routine
            // metrics above it; Pain is the first such signal in enum order.
            divideBefore = { it == ReadinessSignal.Pain },
        ) {
            signal = it
            value = defaultReadinessValue(it)
        }
        when {
            signal.isBinaryFlag ->
                SwitchRow("Present", value > 0.0) { value = if (it) 1.0 else 0.0 }
            signal == ReadinessSignal.Illness -> {
                val level = IllnessLevel.entries.lastOrNull { value >= it.value } ?: IllnessLevel.None
                // Dropdown, not segmented: the triage labels ("Above-neck (no
                // fever)", "Below-neck / fever") are too long to segment, and must
                // not be abbreviated: the distinction is safety-relevant.
                EnumRow("Severity", IllnessLevel.entries.toList(), level, display = { it.label }) {
                    value = it.value
                }
            }
            else -> {
                val spec = continuousSpec(signal)
                DoubleStepperRow("Value", value, spec.min, spec.max, spec.step, spec.format) {
                    value = it
                }
                // The bare "Value" stepper gives no clue what unit a z-score / bpm
                // delta / ratio wants; the core interprets each signal differently,
                // so spell out the unit and the point where it starts to matter.
                Text(spec.hint, color = OnBgMuted, style = Type.Caption)
            }
        }
        Button(
            onClick = {
                onSubmit(
                    Event.SubmitReadiness(
                        signal = signal,
                        value = value,
                        observedAt = System.currentTimeMillis() / 1000,
                    )
                )
            },
            modifier = Modifier.fillMaxWidth(),
        ) { Text("Submit readiness") }
    }
}

/**
 * Signals the core treats as a present/absent red flag (`value > 0` fires). For
 * these a numeric stepper is meaningless, and worse, its 0.0 default silently
 * records the *absence* of the flag, so the editor shows a plain on/off switch.
 */
private val ReadinessSignal.isBinaryFlag: Boolean
    get() = this == ReadinessSignal.Pain ||
        this == ReadinessSignal.RedS ||
        this == ReadinessSignal.CardiacRedFlag ||
        this == ReadinessSignal.BoneStress

/**
 * The value a freshly-selected signal should start at. Flags default to 1.0
 * ("present") so a user who picks a red flag and taps Submit actually triggers
 * the safety tier; Illness defaults to the milder above-neck band; continuous
 * signals start at their spec default (neutral for deltas/z-scores, 1.0 for the
 * e1RM ratio, a value the core reads as "no change vs baseline").
 */
private fun defaultReadinessValue(signal: ReadinessSignal): Double = when {
    signal.isBinaryFlag -> 1.0
    signal == ReadinessSignal.Illness -> IllnessLevel.AboveNeck.value
    else -> continuousSpec(signal).default
}

/**
 * Stepper bounds/step/default for one continuous readiness signal. A single wide
 * range fits none of them: the core reads [ReadinessSignal.EstimatedOneRm] as a
 * ~0.9–1.05 ratio, [ReadinessSignal.Rpe] as a signed delta, RestingHr as a bpm
 * delta, others as z-scores or percents. Each maps to the thresholds in
 * autoreg.rs, so the stepper lands near where the signal actually triggers.
 *
 * On the threshold numbers in the hints (task-14 dedup review): the core now
 * owns the run-split verdict and the e1RM history delta on the ViewModel, and
 * the shell logic duplicating those was deleted (MainActivity). These hints are
 * different, they cite autoreg.rs *input* thresholds that the core does not
 * export on the bridge, shown BEFORE submission as a stepper affordance so the
 * user knows where a value becomes meaningful. Nothing keys off them; the
 * core's evidence-cited adjustment after submit stays authoritative. They stay
 * until the core exports its readiness thresholds, at which point they should
 * be rendered from the wire instead.
 */
private data class ValueSpec(
    val min: Double,
    val max: Double,
    val step: Double,
    val default: Double,
    val hint: String,
    val format: String = "%.1f",
)

private fun continuousSpec(signal: ReadinessSignal): ValueSpec = when (signal) {
    // Signed RPE delta (actual − target); ±1/±2 gate load adjustments.
    ReadinessSignal.Rpe ->
        ValueSpec(-4.0, 4.0, 0.5, 0.0, "RPE minus target · ±1 shifts load, ±2 more")
    // e1RM today ÷ baseline; <0.90/<0.95/>1.05 gate deload/cap/add-load.
    ReadinessSignal.EstimatedOneRm ->
        ValueSpec(0.70, 1.15, 0.01, 1.0, "e1RM ÷ baseline · <0.95 caps load, >1.05 adds", "%.2f")
    // Mean concentric velocity, m/s (dormant, no autoreg gate yet).
    ReadinessSignal.BarVelocity ->
        ValueSpec(0.0, 2.0, 0.05, 0.5, "Mean concentric velocity (m/s) · recorded, no auto-adjustment yet", "%.2f")
    // Within-set velocity-loss %; ≥20 terminates the set.
    ReadinessSignal.VelocityLoss ->
        ValueSpec(0.0, 50.0, 5.0, 0.0, "Within-set velocity drop (%) · ≥20 ends set", "%.0f")
    // Composite wellness z-score; ≤ −1.5 downgrades.
    ReadinessSignal.WellnessZ ->
        ValueSpec(-3.0, 3.0, 0.5, 0.0, "Wellness z-score · ≤ −1.5 downgrades")
    // lnRMSSD z vs rolling baseline; < −0.5 downgrades.
    ReadinessSignal.HrvLnRmssd ->
        ValueSpec(-3.0, 3.0, 0.5, 0.0, "lnRMSSD z vs baseline · < −0.5 downgrades")
    // HRV coefficient of variation % (dormant, no autoreg gate yet).
    ReadinessSignal.HrvCv ->
        ValueSpec(0.0, 15.0, 0.5, 0.0, "HRV coefficient of variation (%) · recorded, no auto-adjustment yet")
    // Aerobic-decoupling drift %; >10 keeps the run easy.
    ReadinessSignal.AerobicDecoupling ->
        ValueSpec(0.0, 30.0, 1.0, 0.0, "Aerobic decoupling (%) · >10 keeps run easy", "%.0f")
    // RHR delta vs baseline, bpm; 5–9 downgrades, ≥10 stops.
    ReadinessSignal.RestingHr ->
        ValueSpec(-5.0, 20.0, 1.0, 0.0, "RHR minus baseline (bpm) · +5 downgrades, +10 stops", "%.0f")
    // Flags/Illness never reach continuousSpec; keep the branch total with a
    // neutral fallback rather than crashing if a new signal is added upstream.
    else -> ValueSpec(-5.0, 40.0, 0.5, 0.0, "")
}

/**
 * Shell-side severity picker for the Illness signal. The wire value is still the
 * numeric `value` the core decodes (schema.rs `IllnessSeverity::from_value`:
 * ≥2 below-neck/fever, ≥1 above-neck, else none), this enum only drives the UI.
 */
private enum class IllnessLevel(val value: Double, val label: String) {
    None(0.0, "None"),
    AboveNeck(1.0, "Above-neck (no fever)"),
    BelowNeckOrFever(2.0, "Below-neck / fever"),
}

/**
 * End-of-session review. Emits [Event.SubmitReview], which drives the coaching
 * feedback layer and the disordered-eating / overtraining safety checks. Lift
 * execution is optional (a run-only day carries no set), so it's gated behind a
 * toggle and only attached when the user reports a completed lift.
 */
@Composable
fun ReviewEditor(onSubmit: (Event.SubmitReview) -> Unit) {
    var badDay by rememberSaveable { mutableStateOf(false) }
    var bonePain by rememberSaveable { mutableStateOf(false) }
    var compulsive by rememberSaveable { mutableStateOf(false) }
    var overtraining by rememberSaveable { mutableStateOf(0) }
    var hasLift by rememberSaveable { mutableStateOf(true) }
    var repsMet by rememberSaveable { mutableStateOf(true) }
    var rirActual by rememberSaveable { mutableStateOf(2) }
    var rirTarget by rememberSaveable { mutableStateOf(2) }
    // Run-review context. The core resolves feedback in priority order lift >
    // decoupling > easy-run intensity, so these two only produce a verdict when
    // no lift is under review, hence they live behind the same !hasLift gate.
    var hasDecoupling by rememberSaveable { mutableStateOf(false) }
    var driftPct by rememberSaveable { mutableStateOf(4.0) }
    var coolSteady by rememberSaveable { mutableStateOf(true) }
    var hasEasyIntensity by rememberSaveable { mutableStateOf(false) }
    var easyPctAboveZ2 by rememberSaveable { mutableStateOf(0) }
    // Manual positive-split entry for runs logged without a GPS track. A tracked
    // run derives this automatically in the core, so this only fills the gap for
    // hand-logged runs and, like the other run context, sits behind the !hasLift gate.
    var hasPositiveSplit by rememberSaveable { mutableStateOf(false) }
    var positiveSplitPct by rememberSaveable { mutableStateOf(0.0) }
    // Week-level fatigue counts, deload triggers that accrue across the week
    // rather than from this one session, so they sit outside the lift/run gate.
    var hasWeekFatigue by rememberSaveable { mutableStateOf(false) }
    var failedKeySessions by rememberSaveable { mutableStateOf(0) }
    var rpeLoadGapSessions by rememberSaveable { mutableStateOf(0) }
    var velocityDropMs by rememberSaveable { mutableStateOf(0.0) }

    FormCard {
        SwitchRow("Bad day", badDay) { badDay = it }
        SwitchRow("Bone pain (red flag)", bonePain) { bonePain = it }
        SwitchRow("Compulsive exercise", compulsive) { compulsive = it }
        IntStepperRow("Overtraining signals", overtraining, 0, 10, 1) { overtraining = it }
        SwitchRow("Completed a lift", hasLift) { hasLift = it }
        if (hasLift) {
            SwitchRow("Reps met", repsMet) { repsMet = it }
            IntStepperRow("RIR actual", rirActual, 0, 10, 1) { rirActual = it }
            IntStepperRow("RIR target", rirTarget, 0, 10, 1) { rirTarget = it }
        } else {
            SwitchRow("Aerobic decoupling recorded", hasDecoupling) { hasDecoupling = it }
            if (hasDecoupling) {
                DoubleStepperRow("Decoupling drift (%)", driftPct, 0.0, 30.0, 0.5) { driftPct = it }
                // The core only issues a decoupling verdict on cool, steady,
                // sub-threshold efforts; leave this on for a genuine easy run.
                SwitchRow("Cool steady sub-threshold run", coolSteady) { coolSteady = it }
            }
            SwitchRow("Easy-run intensity recorded", hasEasyIntensity) { hasEasyIntensity = it }
            if (hasEasyIntensity) {
                IntStepperRow("Time above Zone 2 (%)", easyPctAboveZ2, 0, 100, 5) {
                    easyPctAboveZ2 = it
                }
            }
            SwitchRow("Positive split recorded", hasPositiveSplit) { hasPositiveSplit = it }
            if (hasPositiveSplit) {
                // Second half slower than the first, as a percent; the core flags
                // intensity-discipline coaching strictly beyond +3 %.
                DoubleStepperRow(
                    "Second-half slowdown (%)",
                    positiveSplitPct, 0.0, 30.0, 0.5,
                ) { positiveSplitPct = it }
            }
        }
        SwitchRow("Week-level fatigue review", hasWeekFatigue) { hasWeekFatigue = it }
        if (hasWeekFatigue) {
            // ≥2 of any one trigger prompts a deload (autoreg-023/026/036).
            IntStepperRow("Failed key sessions", failedKeySessions, 0, 7, 1) {
                failedKeySessions = it
            }
            // rpe_load_gap_sessions is an RPE-based metric (target RPE reached at
            // a load ≥7% below plan): the label must say RPE, not RIR.
            IntStepperRow("Sessions RPE met ≥7% below plan", rpeLoadGapSessions, 0, 7, 1) {
                rpeLoadGapSessions = it
            }
            DoubleStepperRow(
                "Weekly bar-velocity drop (m/s)",
                velocityDropMs, 0.0, 0.3, 0.01,
                // Hundredths step needs a hundredths format, else 0.01 increments
                // round to a stuck "0.0" and the >0.06 deload threshold is unreachable.
                format = "%.2f",
            ) { velocityDropMs = it }
        }
        Button(
            onClick = {
                onSubmit(
                    Event.SubmitReview(
                        bonePainRedFlag = bonePain,
                        compulsiveFlag = compulsive,
                        overtrainingSignalCount = overtraining,
                        lift = if (hasLift) {
                            LiftExec(repsMet = repsMet, rirActual = rirActual, rirTarget = rirTarget)
                        } else {
                            null
                        },
                        decoupling = if (!hasLift && hasDecoupling) {
                            Decouple(driftPct = driftPct, coolSteadyContext = coolSteady)
                        } else {
                            null
                        },
                        // The core reads this as a fraction of the run's duration;
                        // the UI collects a friendlier whole percent.
                        easyFracTimeAboveVt1 = if (!hasLift && hasEasyIntensity) {
                            easyPctAboveZ2 / 100.0
                        } else {
                            null
                        },
                        positiveSplitPct = if (!hasLift && hasPositiveSplit) {
                            positiveSplitPct
                        } else {
                            null
                        },
                        failedKeySessions = if (hasWeekFatigue) failedKeySessions else null,
                        rpeLoadGapSessions = if (hasWeekFatigue) rpeLoadGapSessions else null,
                        weeklyVelocityDropMs = if (hasWeekFatigue) velocityDropMs else null,
                        badDay = badDay,
                    )
                )
            },
            modifier = Modifier.fillMaxWidth(),
        ) { Text("Submit review") }
    }
}

/**
 * Standard road-race distances the predictor works over, with their exact metre
 * length. Display-only labels; the wire value the core reads is [meters], fed to
 * [Event.PredictRace], the core runs the Riegel + Daniels equivalency on the raw
 * distances, so these presets never encode coaching logic.
 */
private enum class RacePreset(val meters: Double, val label: String) {
    FiveK(5000.0, "5K"),
    TenK(10000.0, "10K"),
    Half(21097.5, "Half"),
    Full(42195.0, "Marathon"),
}

/**
 * Goal-race finish predictor. Takes a recent race result (distance + time) and a
 * target distance, and emits [Event.PredictRace]; the core answers with a
 * two-method (Riegel + Daniels) evidence-graded estimate rendered from
 * `model.race_prediction`. Weekly volume tunes the Riegel fatigue exponent, so a
 * high-mileage runner's marathon extrapolation fades less than a novice's.
 */
@Composable
fun RacePredictorForm(onPredict: (Event.PredictRace) -> Unit) {
    var recent by rememberSaveable { mutableStateOf(RacePreset.FiveK) }
    var recentMin by rememberSaveable { mutableStateOf(22) }
    var recentSec by rememberSaveable { mutableStateOf(0) }
    var goal by rememberSaveable { mutableStateOf(RacePreset.TenK) }
    var weeklyKm by rememberSaveable { mutableStateOf(40.0) }

    FormCard {
        SegmentedEnumRow("Recent race", RacePreset.entries.toList(), recent, display = { it.label }) {
            recent = it
        }
        IntStepperRow("Recent time - min", recentMin, 1, 359, 1) { recentMin = it }
        IntStepperRow("Recent time - sec", recentSec, 0, 55, 5) { recentSec = it }
        SegmentedEnumRow("Goal race", RacePreset.entries.toList(), goal, display = { it.label }) {
            goal = it
        }
        DoubleStepperRow("Weekly volume (km)", weeklyKm, 0.0, 250.0, 5.0, "%.0f") { weeklyKm = it }
        Button(
            onClick = {
                onPredict(
                    Event.PredictRace(
                        recentDistanceM = recent.meters,
                        recentTimeSec = (recentMin * 60 + recentSec).toDouble(),
                        goalDistanceM = goal.meters,
                        weeklyKm = weeklyKm,
                    )
                )
            },
            modifier = Modifier.fillMaxWidth(),
        ) { Text("Predict finish time") }
    }
}

/**
 * Muscle picker for the hypertrophy volume planner. [wire] is the exact string the
 * core's `hypertrophy::landmarks_for` looks up (case-insensitive, but sent verbatim);
 * [label] is display-only. The set MUST match the LANDMARKS table in `hypertrophy.rs`
 * - an unknown muscle yields a single explanatory row instead of a plan.
 */
private enum class HypertrophyMuscle(val wire: String, val label: String) {
    Chest("chest", "Chest"),
    Back("back", "Back"),
    Quads("quads", "Quads"),
    Hamstrings("hamstrings", "Hamstrings"),
    Glutes("glutes", "Glutes"),
    SideDelts("side delts", "Side delts"),
    RearDelts("rear delts", "Rear delts"),
    Biceps("biceps", "Biceps"),
    Triceps("triceps", "Triceps"),
    Calves("calves", "Calves"),
    Abs("abs", "Abs"),
}

/**
 * Hypertrophy accumulation-block volume planner. Picks a target muscle and a number
 * of accumulation weeks, and emits [Event.PlanHypertrophyMeso]; the core answers with
 * an evidence-graded per-week plan (volume landmarks, MEV→MRV set ramp, RIR schedule,
 * peak frequency) rendered from `model.hypertrophy_plan`. All numbers come from the
 * knowledge-base hypertrophy tables, this form only picks the inputs.
 */
@Composable
fun HypertrophyPlannerForm(onPlan: (Event.PlanHypertrophyMeso) -> Unit) {
    var muscle by rememberSaveable { mutableStateOf(HypertrophyMuscle.Chest) }
    var weeks by rememberSaveable { mutableStateOf(4) }

    FormCard {
        EnumRow(
            "Muscle",
            HypertrophyMuscle.entries.toList(),
            muscle,
            display = { it.label },
        ) { muscle = it }
        IntStepperRow("Accumulation weeks", weeks, 2, 8, 1) { weeks = it }
        Button(
            onClick = {
                onPlan(
                    Event.PlanHypertrophyMeso(
                        muscle = muscle.wire,
                        weeks = weeks,
                    )
                )
            },
            modifier = Modifier.fillMaxWidth(),
        ) { Text("Plan volume") }
    }
}

/**
 * Absolute daily protein target tool. Picks a bodyweight and one or both graded
 * goal contexts (masters 65+, caloric deficit) and emits [Event.ComputeProtein];
 * the core answers with an evidence-graded g/day range (bodyweight × each graded
 * g/kg bound) rendered from `model.protein_targets`. No general/default number
 * exists, with neither toggle set the core returns nothing, so this form only
 * picks the inputs.
 */
@Composable
fun ProteinForm(onCompute: (Double, Boolean, Boolean) -> Unit) {
    var bodyweightKg by rememberSaveable { mutableStateOf(75.0) }
    var masters by rememberSaveable { mutableStateOf(false) }
    var deficit by rememberSaveable { mutableStateOf(false) }

    FormCard {
        DoubleStepperRow("Bodyweight (kg)", bodyweightKg, 30.0, 250.0, 0.5, "%.1f") {
            bodyweightKg = it
        }
        SwitchRow("Masters (65+)", masters) { masters = it }
        SwitchRow("Caloric deficit", deficit) { deficit = it }
        if (!masters && !deficit) {
            // No general/default protein figure is evidence-graded, so the core
            // returns nothing until a graded context is chosen. Without this the
            // button would look actionable yet render no result on tap: say why
            // and gate the button, matching the other forms' validity idiom.
            Text(
                "Pick a context above - no general target is evidence-backed.",
                color = OnBgMuted,
                style = Type.Caption,
            )
        }
        Button(
            onClick = { onCompute(bodyweightKg, masters, deficit) },
            enabled = masters || deficit,
            modifier = Modifier.fillMaxWidth(),
        ) { Text("Compute protein target") }
    }
}

/**
 * On-demand heart-rate-zone calculator. The user enters an age and emits
 * [Event.ComputeHrZones]; the core answers with an evidence-graded HRmax estimate
 * (Tanaka) plus the five Daniels %HRmax training bands as absolute bpm ranges,
 * rendered from `model.hr_zones`. All numbers come from the core (load.rs /
 * running.rs, RUN-VDOT-001), this form only picks the age.
 */
@Composable
fun HrZonesForm(onCompute: (Double) -> Unit) {
    var ageYears by rememberSaveable { mutableStateOf(30.0) }

    FormCard {
        DoubleStepperRow("Age (years)", ageYears, 5.0, 100.0, 1.0, "%.0f") {
            ageYears = it
        }
        Button(
            onClick = { onCompute(ageYears) },
            modifier = Modifier.fillMaxWidth(),
        ) { Text("Compute HR zones") }
    }
}

/**
 * Human-readable label for the readiness signal picker. Display-only, the wire
 * value is still `signal.name` (see [Event.SubmitReadiness]), so these strings can
 * change freely. Exhaustive `when` (no `else`) so a new [ReadinessSignal] variant
 * fails to compile until it is given a label rather than silently showing a
 * cryptic raw name.
 */
private val ReadinessSignal.label: String
    get() = when (this) {
        ReadinessSignal.Rpe -> "RPE"
        ReadinessSignal.EstimatedOneRm -> "Estimated 1RM"
        ReadinessSignal.BarVelocity -> "Bar velocity"
        ReadinessSignal.VelocityLoss -> "Velocity loss"
        ReadinessSignal.WellnessZ -> "Wellness (z-score)"
        ReadinessSignal.HrvLnRmssd -> "HRV (ln rMSSD)"
        ReadinessSignal.HrvCv -> "HRV (CV)"
        ReadinessSignal.AerobicDecoupling -> "Aerobic decoupling"
        ReadinessSignal.RestingHr -> "Resting HR"
        ReadinessSignal.Pain -> "Pain"
        ReadinessSignal.Illness -> "Illness"
        ReadinessSignal.RedS -> "RED-S (energy deficiency)"
        ReadinessSignal.CardiacRedFlag -> "Cardiac red flag"
        ReadinessSignal.BoneStress -> "Bone stress"
    }

@Composable
private fun SwitchRow(label: String, checked: Boolean, onChange: (Boolean) -> Unit) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, color = OnBgBody, style = Type.Body)
        Switch(checked = checked, onCheckedChange = onChange)
    }
}

@Composable
private fun FormCard(content: @Composable () -> Unit) {
    Card(
        colors = CardDefaults.cardColors(containerColor = BgElevated),
        elevation = CardDefaults.cardElevation(defaultElevation = 0.dp),
        shape = RoundedCornerShape(Space.Card.dp),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(
            Modifier.padding(Space.Card.dp),
            verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
        ) { content() }
    }
}
