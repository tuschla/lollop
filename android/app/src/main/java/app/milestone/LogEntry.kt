package app.milestone

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.ui.graphics.Color
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.SelectableDates
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TimePicker
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.material3.rememberTimePickerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import java.text.SimpleDateFormat
import java.util.Calendar
import java.util.Date
import java.util.Locale
import java.util.TimeZone

/**
 * Manual lift-set entry (keypad rework, design import). Emits a real
 * [Event.LogSet] with the user's own exercise / weight / reps / RPE, the
 * e1RM + RIR derivation lives in the Rust core, so this form carries no
 * coaching logic. Weight is typed on the in-form [NumericKeypad]; the buffer
 * text is what gets parsed at submit, so the display-committed invariant holds
 * (an invalid buffer blocks the button). [recentExercises] are the user's own
 * most recent lifts (newest first), padded with common defaults, quick-pick
 * chips over the free-text field.
 *
 * NOTE (shell-thinness): the mockup's live "→ e1RM …" preview is deliberately
 * NOT rendered, e1RM is derived only in the core, after submit. Rendering it
 * live would require the core to expose a preview query; noted as future core
 * work, never computed in Kotlin.
 */
/**
 * Editor shell shared by the four log editors (chrome/04-profile editor
 * pattern): the `ui-close` · centered title · Save-pill header on top of the
 * form. A dirty editor's close asks "Discard this entry?" first (05-log §3).
 */
@Composable
fun EditorScaffold(
    title: String,
    dirty: Boolean,
    saveEnabled: Boolean,
    onClose: () -> Unit,
    onSave: () -> Unit,
    content: @Composable ColumnScope.() -> Unit,
) {
    var confirmDiscard by remember { mutableStateOf(false) }
    // In-flight latch: the host's Save callback is `{ ev -> onEvent(ev); onDismiss() }`,
    // and the sheet takes ~300 ms to hide. A second tap in that window fires onSave
    // again and double-logs a fresh entry_id (a plain `remember` boolean, so it
    // resets when the sheet is disposed and a new editor is mounted). Gate the pill
    // on it AND swallow re-taps in onAction so the first commit is the only commit.
    var submitted by remember { mutableStateOf(false) }
    val status = LocalStatusColors.current
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        EditorHeader(
            title = title,
            onClose = { if (dirty) confirmDiscard = true else onClose() },
            actionEnabled = saveEnabled && !submitted,
            onAction = {
                if (!submitted) {
                    submitted = true
                    onSave()
                }
            },
        )
        content()
    }
    if (confirmDiscard) {
        AlertDialog(
            onDismissRequest = { confirmDiscard = false },
            shape = RoundedCornerShape(Space.Card.dp),
            title = { Text("Discard this entry?") },
            confirmButton = {
                TextButton(onClick = {
                    confirmDiscard = false
                    onClose()
                }) { Text("Discard", color = status.danger) }
            },
            dismissButton = {
                TextButton(onClick = { confirmDiscard = false }) { Text("Keep editing") }
            },
        )
    }
}

/** Exercise search field (05-log §2.1): `ui-search` 18dp + Bold value on
 *  `BgElevated`, `1dp Accent @50%` border while active. No dropdown. */
@Composable
private fun ExerciseSearchField(value: String, onChange: (String) -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(Space.Card.dp))
            .background(BgElevated)
            .border(1.dp, Accent.copy(alpha = 0.5f), RoundedCornerShape(Space.Card.dp))
            .padding(Space.Card.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        Icon(
            painterResource(R.drawable.ic_ui_search),
            contentDescription = null,
            tint = OnBgFaint,
            modifier = Modifier.size(18.dp),
        )
        BasicTextField(
            value = value,
            onValueChange = onChange,
            singleLine = true,
            textStyle = Type.Body.copy(fontWeight = FontWeight.Bold, color = OnBgBody),
            cursorBrush = SolidColor(Accent),
            modifier = Modifier.weight(1f),
            decorationBox = { inner ->
                if (value.isEmpty()) {
                    Text("Search exercise", color = OnBgFaint, style = Type.Body)
                }
                inner()
            },
        )
    }
}

@Composable
fun LogSetEditor(
    recentExercises: List<String> = emptyList(),
    // Non-null → EDIT mode: prefill from this logged set and emit
    // an AmendSet targeting its entry_id instead of a fresh LogSet.
    initial: LiftResultView? = null,
    onClose: () -> Unit = {},
    // Report unsaved-edits state so a host sheet's swipe-down can guard it.
    onDirtyChange: (Boolean) -> Unit = {},
    onSave: (Event) -> Unit,
) {
    val quickPicks = remember(recentExercises) {
        (recentExercises + listOf("Back Squat", "Bench", "Deadlift", "OHP"))
            .map { it.trim() }
            .filter { it.isNotEmpty() }
            .distinct()
            .take(6)
    }
    var exercise by rememberSaveable { mutableStateOf(initial?.exercise ?: quickPicks.first()) }
    var weightText by rememberSaveable {
        // Lossless prefill (was `%.1f`, which rounded an untouched Save's re-encode).
        mutableStateOf(initial?.let { fmtLosslessPrefill(it.weight_kg) } ?: "100.0")
    }
    // Calculator-style entry: the first digit after opening replaces the
    // prefill instead of appending to it.
    var weightFresh by rememberSaveable { mutableStateOf(true) }
    // Don't coerce a logged >20-rep set down to 20 on prefill; an untouched
    // Save would silently rewrite the count. Keep the original; the scale row just
    // won't highlight an out-of-range value (still editable within 1–20 on tap).
    var reps by rememberSaveable { mutableStateOf((initial?.reps ?: 5).coerceAtLeast(1)) }
    var rpe by rememberSaveable { mutableStateOf(initial?.rpe ?: 8.0) }
    var observedAt by rememberSaveable {
        mutableStateOf(
            initial?.observed_at?.takeIf { it > 0 } ?: System.currentTimeMillis() / 1000
        )
    }
    var dirty by rememberSaveable { mutableStateOf(false) }
    // Mirror dirty up to a host so its swipe-down dismissal can guard it.
    LaunchedEffect(dirty) { onDirtyChange(dirty) }
    // A legacy row (entry_id == 0) is amended by matching its original stamp,
    // so its date can't be changed without orphaning it; lock the date chip.
    val dateEditable = initial == null || initial.entry_id != 0L

    val weightParsed = weightText.replace(',', '.').toDoubleOrNull()
    val weightValid = weightParsed != null && weightParsed in 0.0..400.0

    EditorScaffold(
        title = if (initial == null) "Log set" else "Edit set",
        dirty = dirty,
        saveEnabled = exercise.isNotBlank() && weightValid,
        onClose = onClose,
        onSave = {
            weightParsed?.let { w ->
                onSave(setEditEvent(initial, exercise.trim(), w, reps, rpe, observedAt))
            }
        },
    ) {
        FormCard {
            FieldLabel("Exercise")
            ExerciseSearchField(exercise) { exercise = it; dirty = true }
            PresetChipsRow(quickPicks, exercise) { exercise = it; dirty = true }
            KeypadValueField(
                "Weight", "kg", weightText,
                active = true,
                invalid = !weightValid,
                min = 0.0, max = 400.0, format = "%.1f",
                adjustments = listOf(-2.5, 2.5, 5.0),
                onActivate = {},
                onText = { weightText = it; weightFresh = false; dirty = true },
            )
            // Reps: a tap-scale over 1–20 rather than a stepper.
            FieldLabel("Reps", "$reps")
            ScrollableScaleRow((1..20).toList(), reps, { "$it" }) { reps = it; dirty = true }
            // RPE: fixed half-point scale; RIR is RPE's definition (10 − RPE), shown as
            // a hint: the authoritative RIR is still derived in the core on log.
            FieldLabel("RPE", "${fmtRpe(rpe)} · RIR ${(10.0 - rpe).toInt()}")
            ChoiceScaleRow(listOf(6.0, 7.0, 7.5, 8.0, 8.5, 9.0, 10.0), rpe, { fmtRpe(it) }) { rpe = it; dirty = true }
            ObservedAtRow(observedAt, withTime = true, enabled = dateEditable) { observedAt = it; dirty = true }
            NumericKeypad(
                onKey = { key ->
                    weightText = editNumericBuffer(weightText, key, weightFresh)
                    weightFresh = false
                    dirty = true
                },
                onBackspace = {
                    weightText = weightText.dropLast(1)
                    weightFresh = false
                    dirty = true
                },
            )
        }
    }
}

/** `8` for a whole RPE, `7.5` otherwise, display only. */
private fun fmtRpe(v: Double): String = trimDecimal(v)

/** Lossless minutes prefill for the run editor. A whole value shows as "50";
 *  a fractional one keeps a decimal ("50.5") so an untouched Save doesn't round a
 *  logged 50.5-min run up to 51 (the old `%.0f` prefill did). */
private fun fmtDurationPrefill(min: Double): String = trimDecimal(min)

/**
 * Lossless prefill for the weight/distance keypad buffers. The old fixed-decimal
 * prefills (`%.1f` weight, `%.2f` distance) ROUNDED the stored value into the
 * buffer, so an untouched Save re-encoded the rounded number (e.g. a logged
 * 100.25 kg → "100.3", 42.195 km → "42.20"). Render the exact stored double and
 * trim trailing zeros instead: 100.0 → "100", 42.195 → "42.195", 12.50 → "12.5".
 * BigDecimal.valueOf uses the canonical `Double.toString`, so no binary-fraction
 * noise leaks in; toPlainString avoids scientific notation the keypad can't parse.
 */
internal fun fmtLosslessPrefill(v: Double): String =
    java.math.BigDecimal.valueOf(v).stripTrailingZeros().toPlainString()

/** Build the set event for the editor: a fresh [Event.LogSet] when adding, or an
 *  [Event.AmendSet] targeting the logged set when editing. A legacy row (no
 *  entry_id) is matched on its ORIGINAL observed_at, so its stamp is preserved. */
private fun setEditEvent(
    initial: LiftResultView?,
    exercise: String,
    weightKg: Double,
    reps: Int,
    rpe: Double,
    observedAt: Long,
): Event = when {
    initial == null -> Event.LogSet(exercise, weightKg, reps, rpe, observedAt)
    // observedAtFallback is the row's ORIGINAL stamp, so the core replaces the
    // matched row instead of duplicating it; especially a legacy row (id 0).
    initial.entry_id != 0L ->
        Event.AmendSet(initial.entry_id, exercise, weightKg, reps, rpe, observedAt, observedAtFallback = initial.observed_at)
    else -> Event.AmendSet(0L, exercise, weightKg, reps, rpe, initial.observed_at, observedAtFallback = initial.observed_at)
}

/** Run analog of [setEditEvent]: fresh [Event.LogRun] or an [Event.AmendRun]. */
private fun runEditEvent(
    initial: RunResultView?,
    distanceKm: Double,
    durationMin: Double,
    hrPctMax: Double,
    observedAt: Long,
    // User-declared run-intent label; null = untagged (never fabricated).
    workoutType: WorkoutType?,
): Event = when {
    initial == null -> Event.LogRun(distanceKm, durationMin, hrPctMax, 0.0, observedAt, workoutType = workoutType)
    // observedAtFallback = the row's ORIGINAL stamp → replace, don't duplicate.
    initial.entry_id != 0L ->
        Event.AmendRun(initial.entry_id, distanceKm, durationMin, hrPctMax, 0.0, observedAt, observedAtFallback = initial.observed_at, workoutType = workoutType)
    else -> Event.AmendRun(0L, distanceKm, durationMin, hrPctMax, 0.0, initial.observed_at, observedAtFallback = initial.observed_at, workoutType = workoutType)
}

/** Which numeric field the run editor's shared keypad currently edits. */
private enum class RunField { Distance, Duration }

/**
 * Manual run entry for runs recorded without GPS tracking (keypad rework).
 * `longestRecentKm` is left at 0, the core derives the spike baseline from
 * prior logged runs. Distance and duration share one in-form [NumericKeypad];
 * tapping a field switches the keypad's target. Both buffers are parsed at
 * submit exactly as displayed (display-committed invariant); an invalid buffer
 * blocks the button.
 */
@Composable
fun LogRunEditor(
    // Non-null → EDIT mode for a MANUAL run: prefill and emit an
    // AmendRun. GPS-tracked runs are delete-only (their track isn't editable).
    initial: RunResultView? = null,
    onClose: () -> Unit = {},
    // Report unsaved-edits state so a host sheet's swipe-down can guard it.
    onDirtyChange: (Boolean) -> Unit = {},
    onSave: (Event) -> Unit,
) {
    var distText by rememberSaveable {
        // Lossless prefill (was `%.2f`, which rounded an untouched Save's re-encode).
        mutableStateOf(initial?.let { fmtLosslessPrefill(it.distance_km) } ?: "10.00")
    }
    var durText by rememberSaveable {
        // Lossless prefill: `%.0f` used to round a logged 50.5 → 51 on an
        // untouched Save.
        mutableStateOf(initial?.let { fmtDurationPrefill(it.duration_min) } ?: "50")
    }
    var active by rememberSaveable { mutableStateOf(RunField.Distance) }
    // First key after (re)targeting a field starts a fresh number.
    var fresh by rememberSaveable { mutableStateOf(true) }
    // The core treats hr_pct_max == 0 as "no HR sample" and reports zone "-"
    // rather than fabricating one. Gate HR behind a toggle so a run logged without
    // a monitor sends 0 instead of a made-up %, keeping the zone honest.
    var hasHr by rememberSaveable { mutableStateOf((initial?.hr_pct_max ?: 0.0) > 0.0) }
    // Default must be a selectable chip value or no chip highlights while 78 was
    // silently submitted, 75 is in the ChoiceScaleRow list below.
    var hrPctMax by rememberSaveable {
        mutableStateOf(initial?.hr_pct_max?.takeIf { it > 0.0 }?.toInt() ?: 75)
    }
    var observedAt by rememberSaveable {
        mutableStateOf(
            initial?.observed_at?.takeIf { it > 0 } ?: System.currentTimeMillis() / 1000
        )
    }
    var dirty by rememberSaveable { mutableStateOf(false) }
    // Mirror dirty up to a host so its swipe-down dismissal can guard it.
    LaunchedEffect(dirty) { onDirtyChange(dirty) }
    // A legacy row (entry_id == 0) is amended by matching its original stamp,
    // so its date can't be changed without orphaning it; lock the date chip.
    val dateEditable = initial == null || initial.entry_id != 0L
    // User-declared run-intent label. Optional; null = untagged, which the
    // editor never fabricates. Prefilled from the row when editing (decode-safe:
    // an unknown wire string maps to null via WorkoutType.fromWire).
    var workoutType by rememberSaveable {
        mutableStateOf(WorkoutType.fromWire(initial?.workout_type))
    }

    val distParsed = distText.replace(',', '.').toDoubleOrNull()
    val durParsed = durText.replace(',', '.').toDoubleOrNull()
    val distValid = distParsed != null && distParsed in 0.0..100.0
    val durValid = durParsed != null && durParsed in 0.0..600.0
    // A 0-km/0-min run is a legitimate in-progress buffer (not "invalid", unlike a
    // 0-kg set, which logs fine), so it doesn't mark the fields red. But the core
    // won't take a zero-distance/duration run, so Save is gated on >0. When BOTH
    // parse as valid numbers yet one is still zero, the gate silently kills Save;
    // surface a run-specific caption so the dead button has a reason.
    val positive = (distParsed ?: 0.0) > 0.0 && (durParsed ?: 0.0) > 0.0
    val zeroBlocksSave = distValid && durValid && !positive

    fun buffer() = if (active == RunField.Distance) distText else durText
    fun setBuffer(v: String) = if (active == RunField.Distance) distText = v else durText = v

    EditorScaffold(
        title = if (initial == null) "Log run" else "Edit run",
        dirty = dirty,
        saveEnabled = distValid && durValid && positive,
        onClose = onClose,
        onSave = {
            if (distParsed != null && durParsed != null) {
                onSave(
                    runEditEvent(
                        initial,
                        distanceKm = distParsed,
                        durationMin = durParsed,
                        hrPctMax = if (hasHr) hrPctMax.toDouble() else 0.0,
                        observedAt = observedAt,
                        workoutType = workoutType,
                    )
                )
            }
        },
    ) {
        FormCard {
            KeypadValueField(
                "Distance", "km", distText,
                active = active == RunField.Distance,
                invalid = !distValid,
                min = 0.0, max = 100.0, format = "%.2f",
                adjustments = listOf(-0.5, 0.5, 1.0),
                onActivate = { active = RunField.Distance; fresh = true },
                onText = { distText = it; fresh = false; dirty = true },
            )
            KeypadValueField(
                "Duration", "min", durText,
                active = active == RunField.Duration,
                invalid = !durValid,
                min = 0.0, max = 600.0, format = "%.0f",
                adjustments = listOf(-1.0, 1.0, 5.0),
                onActivate = { active = RunField.Duration; fresh = true },
                onText = { durText = it; fresh = false; dirty = true },
            )
            // Explain the >0 Save gate when the numbers are valid but still zero, so
            // the disabled Save pill isn't a mystery. Muted caption, matching the
            // other editors' hint styling; the fields stay un-reddened.
            if (zeroBlocksSave) {
                Text(
                    "Enter a distance and duration above zero to save.",
                    color = OnBgMuted,
                    style = Type.Caption,
                )
            }
            SwitchRow("Recorded HR", hasHr) { hasHr = it; dirty = true }
            if (hasHr) {
                FieldLabel("HR", "% max")
                ChoiceScaleRow(listOf(60, 65, 70, 75, 80, 85, 90, 95), hrPctMax, { "$it" }) { hrPctMax = it; dirty = true }
            }
            // Optional user-declared run type. USER DATA only; no coaching
            // reads it. Tapping the active chip clears it back to untagged; the
            // editor never fabricates a label the user didn't pick.
            WorkoutTypeSelector(workoutType) { picked ->
                workoutType = if (picked == workoutType) null else picked
                dirty = true
            }
            ObservedAtRow(observedAt, withTime = true, enabled = dateEditable) { observedAt = it; dirty = true }
            NumericKeypad(
                onKey = { key ->
                    setBuffer(editNumericBuffer(buffer(), key, fresh))
                    fresh = false
                    dirty = true
                },
                onBackspace = {
                    setBuffer(buffer().dropLast(1))
                    fresh = false
                    dirty = true
                },
            )
        }
    }
}

/**
 * Optional run-type picker. A labeled row of toggle chips over the short
 * [WorkoutType] enum: every option visible, "recognition over recall" (owner
 * Controls rule for short enums). This is USER DATA only: nothing coaching-side
 * reads the result. [current] `null` = untagged; the caller toggles the label
 * back off when the active chip is tapped again, so a run is never forced to
 * carry a fabricated type.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun WorkoutTypeSelector(current: WorkoutType?, onSelect: (WorkoutType) -> Unit) {
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
    ) {
        FieldLabel("Type", "optional")
        FlowRow(horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
            WorkoutType.entries.forEach { wt ->
                SelectChip(wt.label, selected = wt == current) { onSelect(wt) }
            }
        }
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
fun ReadinessEditor(
    // Core-exported signal→group map (ViewModel.signal_groups): the red-flag
    // fence in the picker is drawn where the core says the block starts, not
    // from a shell-side predicate. Empty map (old core) falls back to Pain.
    signalGroups: Map<String, String> = emptyMap(),
    // Deep-link entry point (Today "+ Add" chips): the signal the editor opens
    // pre-selected. Defaults to WellnessZ (the manual/advanced default).
    initialSignal: ReadinessSignal = ReadinessSignal.WellnessZ,
    onClose: () -> Unit = {},
    onSubmit: (Event.SubmitReadiness) -> Unit,
) {
    var signal by rememberSaveable { mutableStateOf(initialSignal) }
    var value by rememberSaveable { mutableStateOf(defaultReadinessValue(initialSignal)) }
    var observedAt by rememberSaveable { mutableStateOf(System.currentTimeMillis() / 1000) }
    // Effort minutes for the duration-gated AerobicDecoupling signal (valid only
    // >20 min, File 06). Defaults to a validating 30 min; sent only for that
    // signal so the core can validate the reading instead of silently discarding it.
    var effortMin by rememberSaveable { mutableStateOf(30.0) }
    var dirty by rememberSaveable { mutableStateOf(false) }
    val firstRedFlag = remember(signalGroups) {
        ReadinessSignal.entries.firstOrNull { signalGroups[it.name] == "red_flag" }
            ?: ReadinessSignal.Pain
    }

    EditorScaffold(
        title = "Log readiness",
        dirty = dirty,
        saveEnabled = true,
        onClose = onClose,
        onSave = {
            onSubmit(
                Event.SubmitReadiness(
                    signal = signal,
                    value = value,
                    observedAt = observedAt,
                    effortMin = if (signal == ReadinessSignal.AerobicDecoupling) effortMin else null,
                )
            )
        },
    ) {
        ReadinessEditorBody(
            signal = signal,
            value = value,
            observedAt = observedAt,
            effortMin = effortMin,
            firstRedFlag = firstRedFlag,
            onSignal = { signal = it; value = defaultReadinessValue(it); dirty = true },
            onValue = { value = it; dirty = true },
            onEffortMin = { effortMin = it; dirty = true },
            onObservedAt = { observedAt = it; dirty = true },
        )
    }
}

/** Which optional watch number the shared keypad currently edits. */
private enum class CheckinField { RestingHr, Hrv }

/**
 * Morning check-in: the PRIMARY readiness entry point. The user
 * answers three human questions (sleep / soreness / mood-stress, 1–5 with word
 * anchors) plus two optional watch numbers (resting HR, HRV rMSSD). The CORE
 * normalizes the retained history into z-scores/deltas/streaks; the user never
 * enters a z-score. One Submit → one [Event.SubmitCheckin]. The old raw-signal
 * editor survives behind "Advanced / lab data".
 */
@Composable
fun MorningCheckinSheet(
    // Rehydrate from the core's most recent check-in echo, if any.
    echo: CheckinEchoView? = null,
    onClose: () -> Unit = {},
    onSubmit: (Event.SubmitCheckin) -> Unit,
) {
    // No-fabrication: the three wellness scales start UNSET (null) when there is no
    // echo to rehydrate, the old `?: 3` default meant an untouched Save silently
    // submitted a middle 3/3/3 the user never answered. Submit is blocked below until
    // all three are actually picked. (An echo is the user's own prior answer, so
    // rehydrating it is a legitimate pre-fill, not a fabricated default.)
    var sleep by rememberSaveable { mutableStateOf(echo?.sleep_quality) }
    var soreness by rememberSaveable { mutableStateOf(echo?.soreness) }
    var mood by rememberSaveable { mutableStateOf(echo?.mood) }
    var observedAt by rememberSaveable { mutableStateOf(System.currentTimeMillis() / 1000) }
    var dirty by rememberSaveable { mutableStateOf(false) }

    // Optional watch numbers behind toggles (like "Recorded HR" in the run
    // editor): a check-in without a watch sends neither, and the core simply
    // won't derive that channel, honest, never fabricated.
    var hasRhr by rememberSaveable { mutableStateOf(echo?.resting_hr_bpm != null) }
    var hasHrv by rememberSaveable { mutableStateOf(echo?.hrv_rmssd_ms != null) }
    // Empty until the user types (or an echo exists), a "55"/"45" placeholder
    // was submittable untouched as a real watch reading (BUGS.md residual,
    // fixed 2026-08-03). Save stays gated on a parsed in-range value.
    var rhrText by rememberSaveable {
        mutableStateOf(echo?.resting_hr_bpm?.let { String.format(Locale.US, "%.0f", it) } ?: "")
    }
    var hrvText by rememberSaveable {
        mutableStateOf(echo?.hrv_rmssd_ms?.let { String.format(Locale.US, "%.0f", it) } ?: "")
    }
    var active by rememberSaveable { mutableStateOf(CheckinField.RestingHr) }
    var fresh by rememberSaveable { mutableStateOf(true) }

    val rhrParsed = rhrText.replace(',', '.').toDoubleOrNull()
    val hrvParsed = hrvText.replace(',', '.').toDoubleOrNull()
    val rhrValid = !hasRhr || (rhrParsed != null && rhrParsed in 25.0..120.0)
    val hrvValid = !hasHrv || (hrvParsed != null && hrvParsed in 1.0..300.0)

    fun buffer() = if (active == CheckinField.RestingHr) rhrText else hrvText
    fun setBuffer(v: String) = if (active == CheckinField.RestingHr) rhrText = v else hrvText = v

    EditorScaffold(
        title = "Morning check-in",
        dirty = dirty,
        // Every wellness scale must be answered (no fabricated 3/3/3) before Save.
        saveEnabled = sleep != null && soreness != null && mood != null && rhrValid && hrvValid,
        onClose = onClose,
        onSave = onSave@{
            val s = sleep ?: return@onSave
            val so = soreness ?: return@onSave
            val m = mood ?: return@onSave
            onSubmit(
                Event.SubmitCheckin(
                    observedAt = observedAt,
                    sleepQuality = s,
                    soreness = so,
                    mood = m,
                    restingHrBpm = if (hasRhr) rhrParsed else null,
                    hrvRmssdMs = if (hasHrv) hrvParsed else null,
                )
            )
        },
    ) {
        FormCard {
            Text(
                "How are you this morning? Answer what you know. The app learns your normal and does the maths.",
                color = OnBgMuted,
                style = Type.Caption,
            )
            CheckinScaleRow("Sleep", "terrible", "great", sleep) { sleep = it; dirty = true }
            CheckinScaleRow("Soreness", "none", "very sore", soreness) { soreness = it; dirty = true }
            CheckinScaleRow("Mood / stress", "awful", "great", mood) { mood = it; dirty = true }

            SwitchRow("Add resting HR (from your watch)", hasRhr) {
                hasRhr = it; dirty = true
                // Toggle-off must also re-route the shared keypad: leaving
                // `active` on the now-hidden field would silently type into it
                // while the visible one never updates.
                if (it) {
                    active = CheckinField.RestingHr; fresh = true
                } else if (active == CheckinField.RestingHr && hasHrv) {
                    active = CheckinField.Hrv; fresh = true
                }
            }
            if (hasRhr) {
                KeypadValueField(
                    "Resting HR", "bpm", rhrText,
                    active = active == CheckinField.RestingHr,
                    // Red only once something out-of-range is typed: an empty
                    // just-toggled field isn't an error, it's an invitation.
                    invalid = rhrText.isNotEmpty() && !rhrValid,
                    min = 25.0, max = 120.0, format = "%.0f",
                    adjustments = listOf(-1.0, 1.0),
                    onActivate = { active = CheckinField.RestingHr; fresh = true },
                    onText = { rhrText = it; fresh = false; dirty = true },
                )
            }
            SwitchRow("Add HRV: rMSSD ms (from your watch/app)", hasHrv) {
                hasHrv = it; dirty = true
                if (it) {
                    active = CheckinField.Hrv; fresh = true
                } else if (active == CheckinField.Hrv && hasRhr) {
                    active = CheckinField.RestingHr; fresh = true
                }
            }
            if (hasHrv) {
                KeypadValueField(
                    "HRV (rMSSD)", "ms", hrvText,
                    active = active == CheckinField.Hrv,
                    invalid = hrvText.isNotEmpty() && !hrvValid,
                    min = 1.0, max = 300.0, format = "%.0f",
                    adjustments = listOf(-1.0, 1.0, 5.0),
                    onActivate = { active = CheckinField.Hrv; fresh = true },
                    onText = { hrvText = it; fresh = false; dirty = true },
                )
            }
            ObservedAtRow(observedAt, withTime = false) { observedAt = it; dirty = true }
            if (hasRhr || hasHrv) {
                NumericKeypad(
                    onKey = { key ->
                        setBuffer(editNumericBuffer(buffer(), key, fresh))
                        fresh = false
                        dirty = true
                    },
                    onBackspace = {
                        setBuffer(buffer().dropLast(1))
                        fresh = false
                        dirty = true
                    },
                )
            }
        }
    }
}

/**
 * One friendly 1–5 wellness row for the morning check-in: a numbered
 * [ChoiceScaleRow] with word anchors under the ends, so the scale reads in plain
 * language. The stored value is the number the core reads (schema.rs::CheckinInput).
 */
@Composable
private fun CheckinScaleRow(
    label: String,
    lowWord: String,
    highWord: String,
    value: Int?,
    onValue: (Int) -> Unit,
) {
    // `value ?: 0` is outside the 1..5 grid, so an unset scale highlights nothing -
    // the user must actually tap a number before it counts as answered.
    FieldLabel(label, value?.let { "$it / 5" } ?: "- / 5")
    ChoiceScaleRow((1..5).toList(), value ?: 0, { "$it" }) { onValue(it) }
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Text(lowWord, color = OnBgFaint, style = Type.Caption)
        Text(highWord, color = OnBgFaint, style = Type.Caption)
    }
}

/** Pain character → core [PainKind] (schema.rs, File-08 Table 4.1). The labels
 *  are human; the mapping decides the gate: SharpJoint hard-stops, tendon pain
 *  is graded by severity/trend, DOMS continues, "Not sure" is the conservative
 *  bare-report stop. */
private enum class PainCharacter(val label: String, val kind: PainKind) {
    Sharp("Sharp or joint pain", PainKind.SharpJoint),
    Tendon("Dull, tendon-like (load-related)", PainKind.TendonLoadRelated),
    Doms("Muscle soreness (DOMS)", PainKind.Doms),
    NotSure("Not sure", PainKind.Other),
}

/** Preset body areas for the pain sub-line. Display-only context, no core rule
 *  branches on location (HR1); it only humanizes the banner + history. */
private val painBodyAreas = listOf(
    "Left knee", "Right knee", "Ankle / Achilles", "Hip",
    "Lower back", "Shoulder", "Elbow / wrist", "Neck", "Foot", "Other",
)

/**
 * Pain triage sheet: a ~10-second characterization that opens
 * BEFORE any hold is set, so an accidental tap can't freeze the app. It captures
 * exactly what the File-08 pain gate needs: body area (display-only), character
 * (→ [PainKind]), severity 0–10, and whether it's worsening (→ [PainTrend]);
 * then "Report pain" sends a full [PainDetail] via [Event.SubmitReadiness] and
 * the core decides hold vs modify-and-monitor. The escape hatch is never weaker
 * than before: "Not sure" (→ Other) and an unspecified area still hard-stop,
 * they just take one deliberate tap.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun PainTriageSheet(
    onClose: () -> Unit = {},
    onSubmit: (Event.SubmitReadiness) -> Unit,
) {
    val status = LocalStatusColors.current
    var location by rememberSaveable { mutableStateOf<String?>(null) }
    var character by rememberSaveable { mutableStateOf(PainCharacter.Sharp) }
    var severity by rememberSaveable { mutableStateOf(5) }
    var rising by rememberSaveable { mutableStateOf(false) }
    val observedAt = remember { System.currentTimeMillis() / 1000 }
    // In-flight latch, same as EditorScaffold's Save pill: onSubmit is
    // `{ ev -> onEvent(ev); onDismiss() }` and the sheet takes ~300 ms to hide, so
    // a second tap in that window fires a second SubmitReadiness and double-reports
    // the pain. This button isn't an EditorScaffold pill (it's the danger commit),
    // so it needs its own latch. Plain `remember` boolean, it resets when the sheet
    // is disposed and re-mounted, and we DON'T disable the button (HR3: the pain
    // escape hatch must never look blocked), we just swallow re-taps below.
    var submitted by remember { mutableStateOf(false) }

    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        // Header: close (back to chooser) + centered title. No Save pill: the
        // commit is the danger "Report pain" button below, deliberately distinct.
        Row(
            modifier = Modifier.fillMaxWidth().heightIn(min = 44.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(
                painterResource(R.drawable.ic_ui_close),
                contentDescription = "Close",
                tint = OnBgBody,
                modifier = Modifier
                    .clip(RoundedCornerShape(Space.Md.dp))
                    .clickable { onClose() }
                    .padding(Space.Md.dp)
                    .size(24.dp),
            )
            Text(
                "Report pain",
                color = OnBgBody,
                style = Type.Title,
                modifier = Modifier.weight(1f),
                textAlign = androidx.compose.ui.text.style.TextAlign.Center,
            )
            // Balance the close icon so the title stays centered.
            Spacer(Modifier.size(48.dp))
        }
        Text(
            // Honest for BOTH core outcomes: a red-flag report becomes a hold, a
            // tolerable one a modify-and-monitor adjustment. The core decides the
            // tier from what's reported, so the copy must not assert a hold up-front.
            "About 10 seconds. The coach pauses or adjusts training based on what you report. Answer what you can.",
            color = OnBgMuted,
            style = Type.Caption,
        )
        FormCard {
            FieldLabel("Where does it hurt?", location ?: "optional")
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
                verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
            ) {
                painBodyAreas.forEach { area ->
                    SelectChip(area, selected = location == area) {
                        location = if (location == area) null else area
                    }
                }
            }
            EnumRow(
                "What does it feel like?",
                PainCharacter.entries.toList(),
                character,
                display = { it.label },
            ) { character = it }
            FieldLabel("How bad is it?", "$severity / 10")
            ScrollableScaleRow((0..10).toList(), severity, { "$it" }) { severity = it }
            SwitchRow("Getting worse after sessions?", rising) { rising = it }
        }
        // Danger commit. Always enabled: the bare/uncertain report is the safety
        // escape hatch and must never be blocked (HR3).
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = 48.dp)
                .clip(RoundedCornerShape(Space.Card.dp))
                .background(status.danger)
                .clickable {
                    // Swallow re-taps: the first commit is the only commit, so a
                    // double-tap in the sheet's dismiss window can't send two reports.
                    if (!submitted) {
                        submitted = true
                        onSubmit(
                            Event.SubmitReadiness(
                                signal = ReadinessSignal.Pain,
                                value = 1.0,
                                observedAt = observedAt,
                                streak = 0,
                                pain = PainDetail(
                                    kind = character.kind,
                                    severity = severity,
                                    trend = if (rising) PainTrend.Rising else PainTrend.Stable,
                                    persists = false,
                                    location = location,
                                ),
                            )
                        )
                    }
                }
                .padding(Space.Card.dp),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                // Neutral action, not an asserted outcome: the core decides hold
                // vs modify-and-monitor from the report, so the button no longer
                // over-promises "Set hold" on a tolerable pain.
                "Report pain",
                color = Color.White,
                style = Type.Body.copy(fontWeight = FontWeight.ExtraBold),
            )
        }
    }
}

/** A selectable pill for the pain-area grid: Accent fill + dark text when picked
 *  (owner AA ruling on accent fills), faint outline otherwise. */
@Composable
private fun SelectChip(label: String, selected: Boolean, onClick: () -> Unit) {
    Text(
        label,
        color = if (selected) OnAccent else OnBgMuted,
        style = Type.Caption,
        modifier = Modifier
            .clip(RoundedCornerShape(100))
            .background(if (selected) Accent else BgElevated)
            .border(
                1.dp,
                if (selected) Accent else OnBgFaint.copy(alpha = 0.3f),
                RoundedCornerShape(100),
            )
            .clickable { onClick() }
            .padding(horizontal = Space.Md.dp, vertical = Space.Sm.dp),
    )
}

@Composable
private fun ReadinessEditorBody(
    signal: ReadinessSignal,
    value: Double,
    observedAt: Long,
    effortMin: Double,
    firstRedFlag: ReadinessSignal,
    onSignal: (ReadinessSignal) -> Unit,
    onValue: (Double) -> Unit,
    onEffortMin: (Double) -> Unit,
    onObservedAt: (Long) -> Unit,
) {
    FormCard {
        // Plain-language framing so the picker isn't a wall of jargon: what a
        // readiness signal IS and what submitting one does. Generic UI copy -
        // the coaching interpretation stays entirely in the core.
        Text(
            "Readiness signals feed today's coaching call. Report only what you actually measured or felt. One signal at a time.",
            color = OnBgMuted,
            style = Type.Caption,
        )
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
            // Fence the medical red-flag block off from the routine metrics
            // above it: where the block starts comes from the core's
            // signal_groups metadata (fallback: Pain, the first red flag).
            divideBefore = { it == firstRedFlag },
        ) {
            onSignal(it)
        }
        // One-line neutral explainer for the selected signal: what it is, not
        // what number triggers what (thresholds live in the core; the stepper
        // hints below cite them separately, see the ValueSpec note).
        Text(signal.explainer, color = OnBgMuted, style = Type.Caption)
        when {
            signal.isBinaryFlag ->
                // A bare "Present" reads as a neutral status; this is a yes/no answer to
                // a red-flag symptom question (the signal + its explainer sit right
                // above), so label the switch as the affirmative it is.
                SwitchRow("Yes, I have this", value > 0.0) { onValue(if (it) 1.0 else 0.0) }
            signal == ReadinessSignal.Illness -> {
                val level = IllnessLevel.entries.lastOrNull { value >= it.value } ?: IllnessLevel.None
                // Whole-row option list, not segmented: the triage labels ("Above-neck
                // (no fever)", "Below-neck / fever") are too long to segment, and must
                // not be abbreviated: the distinction is safety-relevant.
                EnumRow("Severity", IllnessLevel.entries.toList(), level, display = { it.label }) {
                    onValue(it.value)
                }
            }
            else -> {
                // Log-set primitive, not a −/+ stepper (owner ban, 05-log §2):
                // the whole value scale is visible and one tap picks any point
                // on the signal's own grid, negatives (z-scores, deltas)
                // included, which the numeric keypad can't express.
                val spec = continuousSpec(signal)
                FieldLabel("Value", String.format(Locale.US, spec.format, value))
                ScrollableScaleRow(
                    spec.gridOptions(),
                    value,
                    { String.format(Locale.US, spec.format, it) },
                ) { onValue(it) }
                // The bare "Value" scale gives no clue what unit a z-score / bpm
                // delta / ratio wants; the core interprets each signal differently,
                // so spell out the unit and the point where it starts to matter.
                Text(spec.hint, color = OnBgMuted, style = Type.Caption)
            }
        }
        // AerobicDecoupling is duration-gated (valid only >20 min, File 06): the
        // core discards a reading with no duration, so capture the run length here
        // and send it, rather than submitting a reading the engine silently drops.
        if (signal == ReadinessSignal.AerobicDecoupling) {
            DoubleStepperRow("Run duration (min)", effortMin, 0.0, 180.0, 5.0) { onEffortMin(it) }
            Text(
                "Decoupling only counts for a steady effort over 20 minutes.",
                color = OnBgMuted,
                style = Type.Caption,
            )
        }
        ObservedAtRow(observedAt, withTime = false) { onObservedAt(it) }
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

/**
 * The spec's `min + n·step` grid as a picklist for [ScrollableScaleRow]. Each
 * point is round-tripped through the spec's own display format so the stored
 * double is EXACTLY what the row shows (no binary-step drift) and the current
 * value, always produced by this same grid or the spec default, matches an
 * option by equality.
 */
private fun ValueSpec.gridOptions(): List<Double> {
    val n = Math.round((max - min) / step).toInt()
    return (0..n).map { String.format(Locale.US, format, min + it * step).toDouble() }
}

private fun continuousSpec(signal: ReadinessSignal): ValueSpec = when (signal) {
    // Signed RPE delta (actual − target); ±1/±2 gate load adjustments.
    ReadinessSignal.Rpe ->
        ValueSpec(-4.0, 4.0, 0.5, 0.0, "RPE minus target · ±1 shifts load, ±2 more")
    // e1RM today ÷ baseline; <0.90/<0.95/>1.05 gate deload/cap/add-load.
    ReadinessSignal.EstimatedOneRm ->
        ValueSpec(0.70, 1.15, 0.01, 1.0, "e1RM ÷ baseline · <0.95 caps load, >1.05 adds", "%.2f")
    // Mean concentric velocity, m/s (dormant, no autoreg gate yet).
    ReadinessSignal.BarVelocity ->
        ValueSpec(0.0, 2.0, 0.05, 0.5, "Mean concentric velocity (m/s)", "%.2f")
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
        ValueSpec(0.0, 15.0, 0.5, 0.0, "HRV coefficient of variation (%)")
    // Aerobic-decoupling drift %; >10 keeps the run easy.
    ReadinessSignal.AerobicDecoupling ->
        ValueSpec(0.0, 30.0, 1.0, 0.0, "Aerobic decoupling (%) · >10 keeps run easy", "%.0f")
    // RHR delta vs baseline, bpm; 5–9 downgrades, ≥10 stops.
    ReadinessSignal.RestingHr ->
        ValueSpec(-5.0, 20.0, 1.0, 0.0, "RHR minus baseline (bpm) · +5 downgrades, +10 stops", "%.0f")
    // Soreness on the 1–7 Hooper scale; ≥6 downgrades intensity one level.
    ReadinessSignal.Soreness ->
        ValueSpec(1.0, 7.0, 1.0, 3.0, "Soreness 1–7 · ≥6 eases intensity", "%.0f")
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
 * Post-session review: the per-session feedback items ONLY; did the lift go
 * to plan (reps / RIR), or how the run felt (decoupling / easy-run intensity /
 * split). Contextual: offered after logging a session and from the Log menu. Emits
 * [Event.SubmitReview] with the whole-week and clinical fields left at their
 * defaults; the weekly qualitative + medical screens live in the separate
 * [WeeklyCheckinSheet]. (Same singleton wire; this is a UI decomposition, not a
 * new event.)
 */
@Composable
fun SessionReviewSheet(onClose: () -> Unit = {}, onSubmit: (Event.SubmitReview) -> Unit) {
    var badDay by rememberSaveable { mutableStateOf(false) }
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
    var observedAt by rememberSaveable { mutableStateOf(System.currentTimeMillis() / 1000) }
    var dirty by rememberSaveable { mutableStateOf(false) }

    EditorScaffold(
        title = "Session review",
        dirty = dirty,
        saveEnabled = true,
        onClose = onClose,
        onSave = {
            onSubmit(
                Event.SubmitReview(
                    // Clinical + weekly fields stay at defaults here: they belong
                    // to the Weekly check-in. This review only carries per-session
                    // execution context.
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
                    badDay = badDay,
                    observedAt = observedAt,
                )
            )
        },
    ) {
        FormCard {
            Text(
                "How did this session go? Just this one workout.",
                color = OnBgMuted,
                style = Type.Caption,
            )
            SwitchRow("Off day (felt worse than usual)", badDay) { badDay = it; dirty = true }
            SwitchRow("Completed a lift", hasLift) { hasLift = it; dirty = true }
            if (hasLift) {
                SwitchRow("Reps met", repsMet) { repsMet = it; dirty = true }
                IntStepperRow("RIR actual", rirActual, 0, 10, 1) { rirActual = it; dirty = true }
                IntStepperRow("RIR target", rirTarget, 0, 10, 1) { rirTarget = it; dirty = true }
            } else {
                SwitchRow("Aerobic decoupling recorded", hasDecoupling) { hasDecoupling = it; dirty = true }
                if (hasDecoupling) {
                    DoubleStepperRow("Decoupling drift (%)", driftPct, 0.0, 30.0, 0.5) { driftPct = it; dirty = true }
                    // The core only issues a decoupling verdict on cool, steady,
                    // sub-threshold efforts; leave this on for a genuine easy run.
                    SwitchRow("Cool steady sub-threshold run", coolSteady) { coolSteady = it; dirty = true }
                }
                SwitchRow("Easy-run intensity recorded", hasEasyIntensity) { hasEasyIntensity = it; dirty = true }
                if (hasEasyIntensity) {
                    IntStepperRow("Time above Zone 2 (%)", easyPctAboveZ2, 0, 100, 5) {
                        easyPctAboveZ2 = it
                        dirty = true
                    }
                }
                SwitchRow("Positive split recorded", hasPositiveSplit) { hasPositiveSplit = it; dirty = true }
                if (hasPositiveSplit) {
                    // Second half slower than the first, as a percent; the core flags
                    // intensity-discipline coaching strictly beyond +3 %.
                    DoubleStepperRow(
                        "Second-half slowdown (%)",
                        positiveSplitPct, 0.0, 30.0, 0.5,
                    ) { positiveSplitPct = it; dirty = true }
                }
            }
            ObservedAtRow(observedAt, withTime = false) { observedAt = it; dirty = true }
        }
    }
}

/**
 * Weekly check-in: the whole-week qualitative counts plus the two medical
 * screens; now with HUMANE framing, not bare toggles. Each clinical item
 * (disordered-exercise, bone pain) is a plain question with a sentence of context,
 * because both are File-08 medical-deferral triggers. They still flow into the same
 * [Event.SubmitReview] and reach the core's safety gates unchanged (HARD RULE 3);
 * the framing is UI-only.
 */
@Composable
fun WeeklyCheckinSheet(onClose: () -> Unit = {}, onSubmit: (Event.SubmitReview) -> Unit) {
    var overtraining by rememberSaveable { mutableStateOf(0) }
    var bonePain by rememberSaveable { mutableStateOf(false) }
    var compulsive by rememberSaveable { mutableStateOf(false) }
    var failedKeySessions by rememberSaveable { mutableStateOf(0) }
    var rpeLoadGapSessions by rememberSaveable { mutableStateOf(0) }
    var velocityDropMs by rememberSaveable { mutableStateOf(0.0) }
    var observedAt by rememberSaveable { mutableStateOf(System.currentTimeMillis() / 1000) }
    var dirty by rememberSaveable { mutableStateOf(false) }

    EditorScaffold(
        title = "Weekly check-in",
        dirty = dirty,
        saveEnabled = true,
        onClose = onClose,
        onSave = {
            onSubmit(
                Event.SubmitReview(
                    bonePainRedFlag = bonePain,
                    compulsiveFlag = compulsive,
                    overtrainingSignalCount = overtraining,
                    // Whole-week deload triggers (autoreg-023/026/036). Always sent
                    // from the weekly check-in: the counts are a week-level rollup.
                    failedKeySessions = failedKeySessions,
                    rpeLoadGapSessions = rpeLoadGapSessions,
                    weeklyVelocityDropMs = velocityDropMs,
                    observedAt = observedAt,
                )
            )
        },
    ) {
        FormCard {
            Text(
                "A quick look back at the whole week, not one session.",
                color = OnBgMuted,
                style = Type.Caption,
            )
            // Week-level fatigue rollup: ≥2 of any one trigger prompts an easier week.
            // Plain-language labels (owner jargon ruling): the wire fields are unchanged.
            IntStepperRow("Hard sessions you couldn't finish this week", failedKeySessions, 0, 7, 1) {
                failedKeySessions = it
                dirty = true
            }
            // rpe_load_gap_sessions: the target effort was reached at a load ≥7% below
            // plan (a fatigue sign). Say it in plain words, not "RPE met ≥7% below plan".
            IntStepperRow("Sessions that felt as hard as usual but at a lighter weight", rpeLoadGapSessions, 0, 7, 1) {
                rpeLoadGapSessions = it
                dirty = true
            }
            DoubleStepperRow(
                "Weekly bar-speed drop (m/s)",
                velocityDropMs, 0.0, 0.3, 0.01,
                // Hundredths step needs a hundredths format, else 0.01 increments
                // round to a stuck "0.0" and the >0.06 easier-week threshold is unreachable.
                format = "%.2f",
            ) { velocityDropMs = it; dirty = true }
            IntStepperRow("Overtraining signs you noticed", overtraining, 0, 10, 1) { overtraining = it; dirty = true }
        }

        // Clinical screens, humanely framed. These are medical-deferral triggers,
        // so they get context, not a bare switch, and route into the core's File-08
        // safety gates unchanged.
        Text(
            "Checking in on you",
            color = OnBgFaint,
            style = Type.Overline,
        )
        FormCard {
            ClinicalQuestion(
                question = "Over the last few weeks, have you felt you must exercise even when ill or injured, or anxious when you can't?",
                context = "If yes, we'll gently suggest talking it over with a professional. There's no penalty. This just helps keep training healthy.",
                checked = compulsive,
            ) { compulsive = it; dirty = true }
            ClinicalQuestion(
                question = "Any deep, localised, worsening bone pain: pain in one spot that hurts on impact or at rest?",
                context = "This can be an early stress-injury sign. If yes, we'll pause loading and point you to a clinician. Ordinary muscle soreness doesn't count here.",
                checked = bonePain,
            ) { bonePain = it; dirty = true }
        }
        ObservedAtRow(observedAt, withTime = false) { observedAt = it; dirty = true }
    }
}

/**
 * A humanely-framed clinical screen: the question as the primary line, a
 * sentence explaining what a "yes" means (so a medical-deferral trigger never reads
 * as a cryptic toggle), and a Yes/No switch. The wire value is the same boolean the
 * core's File-08 gates consume.
 */
@Composable
private fun ClinicalQuestion(
    question: String,
    context: String,
    checked: Boolean,
    onCheck: (Boolean) -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
        Text(question, color = OnBgBody, style = Type.Body)
        Text(context, color = OnBgMuted, style = Type.Caption)
        SwitchRow(if (checked) "Yes" else "No, all good", checked) { onCheck(it) }
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
/** Which numeric field the race form's shared keypad currently edits. */
private enum class RaceField { Minutes, Seconds, Volume }

@Composable
fun RacePredictorForm(initial: RacePredictionView? = null, onPredict: (Event.PredictRace) -> Unit) {
    // Seed from the core's echoed query (form rehydration after a cold start /
    // log replay) when present; otherwise the hardcoded starter defaults. The
    // rememberSaveable initial lambda runs once: a warm reopen restores the
    // user's own last edits, a cold start falls back here to the replayed query.
    fun presetFor(meters: Double, fallback: RacePreset) =
        RacePreset.entries.firstOrNull { kotlin.math.abs(it.meters - meters) < 1.0 } ?: fallback
    var recent by rememberSaveable {
        mutableStateOf(initial?.let { presetFor(it.recent_distance_m, RacePreset.FiveK) } ?: RacePreset.FiveK)
    }
    var goal by rememberSaveable {
        mutableStateOf(initial?.let { presetFor(it.goal_distance_m, RacePreset.TenK) } ?: RacePreset.TenK)
    }
    // Keypad-driven numeric entry, same primitives as Log set (02-coach §1:
    // "same primitives as Log set"; owner ban on −/+ steppers as primary).
    val seedMin = initial?.let { (it.recent_time_sec / 60.0).toInt().toString() } ?: "22"
    val seedSec = initial?.let { Math.round(it.recent_time_sec % 60.0).toString() } ?: "0"
    val seedKm = initial?.let { String.format(Locale.US, "%.0f", it.weekly_km) } ?: "40"
    var minText by rememberSaveable { mutableStateOf(seedMin) }
    var secText by rememberSaveable { mutableStateOf(seedSec) }
    var kmText by rememberSaveable { mutableStateOf(seedKm) }
    var active by rememberSaveable { mutableStateOf(RaceField.Minutes) }
    var fresh by rememberSaveable { mutableStateOf(true) }
    // D1: weeks since that race, the core flags a stale result (running-041). Off
    // by default (a fresh result); a toggle reveals the scale, seeded from the echo.
    var raceStale by rememberSaveable { mutableStateOf((initial?.weeks_since_race ?: 0) > 0) }
    var weeksSinceRace by rememberSaveable { mutableStateOf(initial?.weeks_since_race ?: 4) }

    val minParsed = minText.replace(',', '.').toDoubleOrNull()
    val secParsed = secText.replace(',', '.').toDoubleOrNull()
    val kmParsed = kmText.replace(',', '.').toDoubleOrNull()
    val minValid = minParsed != null && minParsed in 1.0..359.0
    val secValid = secParsed != null && secParsed in 0.0..59.0
    val kmValid = kmParsed != null && kmParsed in 0.0..250.0

    fun buffer() = when (active) {
        RaceField.Minutes -> minText
        RaceField.Seconds -> secText
        RaceField.Volume -> kmText
    }
    fun setBuffer(v: String) = when (active) {
        RaceField.Minutes -> minText = v
        RaceField.Seconds -> secText = v
        RaceField.Volume -> kmText = v
    }

    FormCard {
        SegmentedEnumRow("Recent race", RacePreset.entries.toList(), recent, display = { it.label }) {
            recent = it
        }
        KeypadValueField(
            "Recent time: minutes", "min", minText,
            active = active == RaceField.Minutes,
            invalid = !minValid,
            min = 1.0, max = 359.0, format = "%.0f",
            adjustments = listOf(-1.0, 1.0, 5.0),
            onActivate = { active = RaceField.Minutes; fresh = true },
            onText = { minText = it; fresh = false },
        )
        KeypadValueField(
            "Recent time: seconds", "sec", secText,
            active = active == RaceField.Seconds,
            invalid = !secValid,
            min = 0.0, max = 59.0, format = "%.0f",
            adjustments = listOf(-5.0, 5.0),
            onActivate = { active = RaceField.Seconds; fresh = true },
            onText = { secText = it; fresh = false },
        )
        SegmentedEnumRow("Goal race", RacePreset.entries.toList(), goal, display = { it.label }) {
            goal = it
        }
        KeypadValueField(
            "Weekly volume", "km", kmText,
            active = active == RaceField.Volume,
            invalid = !kmValid,
            min = 0.0, max = 250.0, format = "%.0f",
            adjustments = listOf(-5.0, 5.0),
            onActivate = { active = RaceField.Volume; fresh = true },
            onText = { kmText = it; fresh = false },
        )
        // D1: freshness of the input race (running-041). Off → sent as null (a
        // recent result); on reveals a weeks scale so the core can flag staleness.
        SwitchRow("That race wasn't recent", raceStale) { raceStale = it }
        if (raceStale) {
            FieldLabel("Weeks since that race", "$weeksSinceRace")
            ScrollableScaleRow((1..52).toList(), weeksSinceRace, { "$it" }) { weeksSinceRace = it }
        }
        NumericKeypad(
            onKey = { key ->
                setBuffer(editNumericBuffer(buffer(), key, fresh))
                fresh = false
            },
            onBackspace = {
                setBuffer(buffer().dropLast(1))
                fresh = false
            },
        )
        Button(
            onClick = {
                if (minParsed != null && secParsed != null && kmParsed != null) {
                    onPredict(
                        Event.PredictRace(
                            recentDistanceM = recent.meters,
                            recentTimeSec = minParsed * 60.0 + secParsed,
                            goalDistanceM = goal.meters,
                            weeklyKm = kmParsed,
                            weeksSinceRace = if (raceStale) weeksSinceRace else null,
                        )
                    )
                }
            },
            enabled = minValid && secValid && kmValid,
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
fun HypertrophyPlannerForm(initial: HypertrophyInputView? = null, onPlan: (Event.PlanHypertrophyMeso) -> Unit) {
    // Rehydrate from the core's echoed query on a cold start; else defaults.
    var muscle by rememberSaveable {
        mutableStateOf(
            initial?.let { i -> HypertrophyMuscle.entries.firstOrNull { it.wire == i.muscle } }
                ?: HypertrophyMuscle.Chest,
        )
    }
    var weeks by rememberSaveable { mutableStateOf(initial?.weeks?.coerceIn(2, 8) ?: 4) }

    FormCard {
        EnumRow(
            "Muscle",
            HypertrophyMuscle.entries.toList(),
            muscle,
            display = { it.label },
        ) { muscle = it }
        // Whole scale visible, one tap picks (owner stepper ban; 05-log §2
        // primitives): the 2–8 week range fits a single ChoiceScaleRow.
        FieldLabel("Accumulation weeks", "$weeks")
        ChoiceScaleRow((2..8).toList(), weeks, { "$it" }) { weeks = it }
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
fun ProteinForm(
    initial: ProteinInputView? = null,
    profileBodyweightKg: Double? = null,
    onCompute: (Double, Boolean, Boolean) -> Unit,
) {
    // Keypad-driven bodyweight (Log-set primitives, owner stepper ban). The
    // buffer text is exactly what is parsed at submit (display-committed).
    // Seed order: the core's echoed query (last calc) first, then the
    // profile's consolidated person bodyweight, then the plain default; so a
    // user who set bodyweight in Profile never re-types it here (override still
    // allowed). Absent both, the neutral 75 kg starter.
    var bwText by rememberSaveable {
        val seed = initial?.bodyweight_kg ?: profileBodyweightKg
        mutableStateOf(seed?.let { String.format(Locale.US, "%.1f", it) } ?: "75.0")
    }
    var fresh by rememberSaveable { mutableStateOf(true) }
    var masters by rememberSaveable { mutableStateOf(initial?.masters ?: false) }
    var deficit by rememberSaveable { mutableStateOf(initial?.deficit ?: false) }

    val bwParsed = bwText.replace(',', '.').toDoubleOrNull()
    val bwValid = bwParsed != null && bwParsed in 30.0..250.0

    FormCard {
        KeypadValueField(
            "Bodyweight", "kg", bwText,
            active = true,
            invalid = !bwValid,
            min = 30.0, max = 250.0, format = "%.1f",
            adjustments = listOf(-2.5, 2.5, 5.0),
            onActivate = { fresh = true },
            onText = { bwText = it; fresh = false },
        )
        if (initial == null && profileBodyweightKg != null) {
            Text(
                "Prefilled from your profile. Edit to override.",
                color = OnBgMuted,
                style = Type.Caption,
            )
        }
        SwitchRow("Masters (65+)", masters) { masters = it }
        SwitchRow("Caloric deficit", deficit) { deficit = it }
        if (!masters && !deficit) {
            // No general/default protein figure is evidence-graded, so the core
            // returns nothing until a graded context is chosen. Without this the
            // button would look actionable yet render no result on tap: say why
            // and gate the button, matching the other forms' validity idiom.
            Text(
                "Pick a context above. No general target is evidence-backed.",
                color = OnBgMuted,
                style = Type.Caption,
            )
        }
        NumericKeypad(
            onKey = { key ->
                bwText = editNumericBuffer(bwText, key, fresh)
                fresh = false
            },
            onBackspace = {
                bwText = bwText.dropLast(1)
                fresh = false
            },
        )
        Button(
            onClick = { bwParsed?.let { onCompute(it, masters, deficit) } },
            enabled = bwValid && (masters || deficit),
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
fun HrZonesForm(
    initial: HrZoneInputView? = null,
    profileAgeYears: Double? = null,
    profileRestingHrBpm: Double? = null,
    // D1: (age, restingHrBpm?), a non-null RHR lets the core run Karvonen
    // heart-rate-reserve zones (running-005) instead of plain %HRmax.
    onCompute: (Double, Double?) -> Unit,
) {
    // Keypad-driven age entry (Log-set primitives, owner stepper ban).
    // Seed order: the echoed query first, then the profile's consolidated
    // age, then the plain default; enter your age once on Profile, prefilled here.
    var ageText by rememberSaveable {
        val seed = initial?.age_years ?: profileAgeYears
        mutableStateOf(seed?.let { String.format(Locale.US, "%.0f", it) } ?: "30")
    }
    var fresh by rememberSaveable { mutableStateOf(true) }
    // D1: optional resting HR. Seeded from the echoed query, then the profile.
    // Karvonen zones are more personal than age-only %HRmax, so default it ON
    // whenever a resting HR is known (from the echo or the profile).
    val seedRhr = initial?.resting_hr_bpm ?: profileRestingHrBpm
    var useRhr by rememberSaveable { mutableStateOf(seedRhr != null) }
    var rhr by rememberSaveable { mutableStateOf(seedRhr?.toInt() ?: 60) }

    val ageParsed = ageText.replace(',', '.').toDoubleOrNull()
    val ageValid = ageParsed != null && ageParsed in 5.0..100.0

    FormCard {
        KeypadValueField(
            "Age", "years", ageText,
            active = true,
            invalid = !ageValid,
            min = 5.0, max = 100.0, format = "%.0f",
            adjustments = listOf(-1.0, 1.0),
            onActivate = { fresh = true },
            onText = { ageText = it; fresh = false },
        )
        if (initial == null && (profileAgeYears != null || profileRestingHrBpm != null)) {
            Text(
                "Prefilled from your profile. Edit to override.",
                color = OnBgMuted,
                style = Type.Caption,
            )
        }
        // D1: resting HR unlocks Karvonen (heart-rate-reserve) zones. Off → the
        // core falls back to age-only %HRmax, so this stays optional.
        SwitchRow("Use resting HR (Karvonen zones)", useRhr) { useRhr = it }
        if (useRhr) {
            FieldLabel("Resting HR", "$rhr bpm")
            ScrollableScaleRow((35..90).toList(), rhr, { "$it" }) { rhr = it }
        }
        NumericKeypad(
            onKey = { key ->
                ageText = editNumericBuffer(ageText, key, fresh)
                fresh = false
            },
            onBackspace = {
                ageText = ageText.dropLast(1)
                fresh = false
            },
        )
        Button(
            onClick = { ageParsed?.let { onCompute(it, if (useRhr) rhr.toDouble() else null) } },
            enabled = ageValid,
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
/**
 * One-line neutral explainer for the readiness picker: says what the signal IS
 * (and, for the medical flags, the core-documented consequence of reporting
 * it). Deliberately no shell-side thresholds, the numbers that trigger
 * adjustments live only in the core (see the ValueSpec note above).
 */
private val ReadinessSignal.explainer: String
    get() = when (this) {
        ReadinessSignal.Rpe -> "How hard training felt compared to the plan."
        ReadinessSignal.EstimatedOneRm -> "Today's estimated single-rep max relative to your baseline."
        ReadinessSignal.BarVelocity -> "Measured bar speed on a reference lift."
        ReadinessSignal.VelocityLoss -> "How much bar speed dropped within a set."
        ReadinessSignal.WellnessZ -> "Sleep, soreness, stress and mood combined, versus your normal."
        ReadinessSignal.HrvLnRmssd -> "Heart-rate variability compared to your rolling baseline."
        ReadinessSignal.HrvCv -> "How much your HRV readings vary day to day."
        ReadinessSignal.AerobicDecoupling -> "Heart-rate drift versus pace during a steady run."
        ReadinessSignal.RestingHr -> "This morning's resting heart rate versus your baseline."
        ReadinessSignal.Soreness -> "Muscle soreness today on a 1–7 scale: 6+ eases intensity."
        ReadinessSignal.Pain -> "Sharp or joint pain: a red flag that pauses training."
        ReadinessSignal.Illness -> "Feeling sick: the severity decides the training call."
        ReadinessSignal.RedS -> "Signs of under-fueling (RED-S): routes to a medical referral."
        ReadinessSignal.CardiacRedFlag -> "Chest pain, fainting or palpitations: routes to a professional."
        ReadinessSignal.BoneStress -> "Focal bone pain: possible bone-stress injury, routes to a professional."
    }

internal val ReadinessSignal.label: String
    get() = when (this) {
        ReadinessSignal.Rpe -> "RPE"
        ReadinessSignal.EstimatedOneRm -> "Estimated 1RM"
        ReadinessSignal.BarVelocity -> "Bar velocity"
        ReadinessSignal.VelocityLoss -> "Velocity loss"
        ReadinessSignal.WellnessZ -> "Wellness (z-score)"
        ReadinessSignal.HrvLnRmssd -> "HRV (ln rMSSD)"
        ReadinessSignal.HrvCv -> "HRV (CV)"
        ReadinessSignal.AerobicDecoupling -> "Aerobic decoupling"
        // This raw-signal field submits a DELTA versus baseline (grid −5..+20; the core
        // reads it as a bpm delta, +5 downgrades / +10 stops), NOT an absolute bpm, so
        // the pill has to say "vs baseline" to match the helper below it and the
        // explainer. (Absolute resting HR is entered in the morning check-in instead.)
        ReadinessSignal.RestingHr -> "Resting HR vs baseline"
        ReadinessSignal.Soreness -> "Soreness"
        ReadinessSignal.Pain -> "Pain"
        ReadinessSignal.Illness -> "Illness"
        ReadinessSignal.RedS -> "RED-S (energy deficiency)"
        ReadinessSignal.CardiacRedFlag -> "Cardiac red flag"
        ReadinessSignal.BoneStress -> "Bone stress"
    }

/**
 * Backdating control for every log editor: a compact chip stating when the
 * entry is logged ("Today · 14:32" / "Jul 15 · 09:10"), defaulting to now.
 * Tapping opens an M3 date picker (future dates unselectable); [withTime]
 * (sets/runs) chains a time picker after the date. The result is clamped to
 * now so a future stamp can never be submitted. Date-only editors
 * (readiness/review) keep the current time-of-day on the chosen date, so
 * same-day ordering vs other inputs stays sensible. The value rides the
 * wire's existing `observed_at` unix seconds, the core holds no clock.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ObservedAtRow(
    epochSec: Long,
    withTime: Boolean,
    // Legacy entries (entry_id == 0) are amended by matching their ORIGINAL
    // observed_at, so a changed date can't be honored without orphaning the row;
    // rather than silently discard the edit, the caller disables the chip.
    enabled: Boolean = true,
    onChange: (Long) -> Unit,
) {
    var showDate by remember { mutableStateOf(false) }
    var showTime by remember { mutableStateOf(false) }
    var pendingDayUtcMillis by remember { mutableStateOf<Long?>(null) }

    Column(verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("Logged", color = OnBgBody, style = Type.Body)
            Text(
                formatObservedAt(epochSec, withTime),
                color = if (enabled) Accent else OnBgFaint,
                style = Type.Body.merge(TabularFigures),
                modifier = Modifier
                    .clip(RoundedCornerShape(Space.Md.dp))
                    .background(BgTop)
                    .then(if (enabled) Modifier.clickable { showDate = true } else Modifier)
                    .padding(horizontal = Space.Card.dp, vertical = Space.Md.dp),
            )
        }
        if (!enabled) {
            Text(
                "This older entry's date can't be changed. Delete and re-log it to backdate.",
                color = OnBgFaint,
                style = Type.Caption,
            )
        }
    }

    if (showDate) {
        val state = rememberDatePickerState(
            // M3 DatePicker reads the initial millis as a UTC-midnight day, but
            // epochSec is a local instant: near midnight in a UTC+ zone the raw
            // value lands on the previous UTC day and highlights the wrong cell.
            // Shift by the local offset so the highlighted day matches local time.
            initialSelectedDateMillis = (epochSec * 1000L).let {
                it + TimeZone.getDefault().getOffset(it)
            },
            selectableDates = object : SelectableDates {
                // Future dates blocked. DatePicker days are UTC-midnight stamps;
                // allow through "today" in the most permissive zone (UTC+14) and
                // let the final clamp-to-now catch the rest.
                override fun isSelectableDate(utcTimeMillis: Long): Boolean =
                    utcTimeMillis <= System.currentTimeMillis() + 14 * 3_600_000L

                override fun isSelectableYear(year: Int): Boolean =
                    year <= Calendar.getInstance().get(Calendar.YEAR)
            },
        )
        DatePickerDialog(
            onDismissRequest = { showDate = false },
            confirmButton = {
                TextButton(onClick = {
                    val sel = state.selectedDateMillis
                    showDate = false
                    if (sel != null) {
                        if (withTime) {
                            pendingDayUtcMillis = sel
                            showTime = true
                        } else {
                            // Keep the entry's current local time-of-day.
                            val cur = Calendar.getInstance().apply { timeInMillis = epochSec * 1000L }
                            onChange(
                                combineDayAndTime(
                                    sel,
                                    cur.get(Calendar.HOUR_OF_DAY),
                                    cur.get(Calendar.MINUTE),
                                ),
                            )
                        }
                    }
                }) { Text("OK") }
            },
            dismissButton = {
                TextButton(onClick = { showDate = false }) { Text("Cancel") }
            },
        ) { DatePicker(state) }
    }

    if (showTime) {
        val cur = Calendar.getInstance().apply { timeInMillis = epochSec * 1000L }
        val tState = rememberTimePickerState(
            initialHour = cur.get(Calendar.HOUR_OF_DAY),
            initialMinute = cur.get(Calendar.MINUTE),
            is24Hour = true,
        )
        AlertDialog(
            onDismissRequest = { showTime = false },
            title = { Text("Logged at") },
            text = { TimePicker(tState) },
            confirmButton = {
                TextButton(onClick = {
                    showTime = false
                    pendingDayUtcMillis?.let { day ->
                        onChange(combineDayAndTime(day, tState.hour, tState.minute))
                    }
                }) { Text("OK") }
            },
            dismissButton = {
                TextButton(onClick = { showTime = false }) { Text("Cancel") }
            },
        )
    }
}

/**
 * Combine a DatePicker day (UTC-midnight millis: its Y/M/D are the *picked*
 * calendar date) with a local wall-clock time into local unix seconds, clamped
 * to now so a future stamp can never be logged.
 */
private fun combineDayAndTime(utcDayMillis: Long, hour: Int, minute: Int): Long {
    val utc = Calendar.getInstance(TimeZone.getTimeZone("UTC")).apply { timeInMillis = utcDayMillis }
    val local = Calendar.getInstance().apply {
        clear()
        set(
            utc.get(Calendar.YEAR),
            utc.get(Calendar.MONTH),
            utc.get(Calendar.DAY_OF_MONTH),
            hour,
            minute,
            0,
        )
    }
    return minOf(local.timeInMillis / 1000L, System.currentTimeMillis() / 1000L)
}

/** "Today · 14:32" / "Jul 15 · 09:10" / date-only variants for the chip. */
private fun formatObservedAt(epochSec: Long, withTime: Boolean): String {
    val now = Calendar.getInstance()
    val then = Calendar.getInstance().apply { timeInMillis = epochSec * 1000L }
    val sameDay = now.get(Calendar.YEAR) == then.get(Calendar.YEAR) &&
        now.get(Calendar.DAY_OF_YEAR) == then.get(Calendar.DAY_OF_YEAR)
    val day = if (sameDay) "Today" else SimpleDateFormat("MMM d", Locale.US).format(Date(epochSec * 1000L))
    return if (withTime) {
        "$day · ${SimpleDateFormat("HH:mm", Locale.US).format(Date(epochSec * 1000L))}"
    } else {
        day
    }
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
