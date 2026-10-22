package app.milestone

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.res.stringResource
import java.util.Locale

/** Run analog of [setEditEvent]: fresh [Event.LogRun] or an [Event.AmendRun]. */
private fun runEditEvent(
    initial: RunResultView?,
    distanceKm: Double,
    durationMin: Double,
    hrPctMax: Double,
    observedAt: Long,
    // User-declared run-intent label; null = untagged (never fabricated).
    workoutType: WorkoutType?,
    // Whole-session effort, CR-10 (0–10); null = unrated.
    sessionRpe: Int?,
    // Average heart rate in bpm (the core converts it to %HRmax); null when the HR
    // was given as % of max or not at all.
    avgHrBpm: Double?,
): Event = when {
    initial == null -> Event.LogRun(distanceKm, durationMin, hrPctMax, 0.0, observedAt, workoutType = workoutType, sessionRpe = sessionRpe, avgHrBpm = avgHrBpm)
    // observedAtFallback = the row's ORIGINAL stamp → replace, don't duplicate.
    initial.entry_id != 0L ->
        Event.AmendRun(initial.entry_id, distanceKm, durationMin, hrPctMax, 0.0, observedAt, observedAtFallback = initial.observed_at, workoutType = workoutType, sessionRpe = sessionRpe, avgHrBpm = avgHrBpm)
    else -> Event.AmendRun(0L, distanceKm, durationMin, hrPctMax, 0.0, initial.observed_at, observedAtFallback = initial.observed_at, workoutType = workoutType, sessionRpe = sessionRpe, avgHrBpm = avgHrBpm)
}

/** Which numeric field the run editor's shared keypad currently edits. */
private enum class RunField { Distance, Duration, HeartRate }

/** The run editor's distance ceiling, km (mi converts it exactly). */
private const val RunDistanceMaxKm = 100.0

/** The run editor's duration ceiling, minutes (10 h). */
private const val RunDurationMaxMin = 600.0

/** Average heart-rate entry bounds: a floor shared by bpm and % of max, the bpm
 *  ceiling, and 100 % of max (the core's `MAX_HR_PCT_MAX`). */
private const val RunHrMin = 40.0
private const val RunAvgHrMaxBpm = 230.0
private const val RunHrMaxPct = 100.0

/** A measured HRmax the core trusts over the Tanaka estimate
 *  (`running::plausible_measured_hr_max`). */
internal val PlausibleHrMaxBpm = 100.0..240.0

/** The ages the core's Tanaka HRmax estimate covers (`running::tanaka_age_in_range`). */
internal val TanakaAgeRange = 5.0..100.0

/**
 * Manual run entry for runs recorded without GPS tracking. The spike baseline
 * (`longestRecentKm`) is left at 0: the core derives it from prior logged runs.
 * Distance, duration and the optional average heart rate share one in-form
 * [NumericKeypad]; tapping a field switches the keypad's target. Every buffer
 * is parsed at submit exactly as displayed (display-committed invariant); an
 * invalid buffer blocks the button.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun LogRunEditor(
    // Non-null → EDIT mode for a MANUAL run: prefill and emit an
    // AmendRun. GPS-tracked runs are delete-only (their track isn't editable).
    initial: RunResultView? = null,
    // Seed the stamp to a plan day's local noon when logging a PAST prescribed run
    // (null falls back to now). The date chip stays user-editable.
    initialObservedAt: Long? = null,
    // NEW-mode coach prescription. The wire PrescriptionView carries no structured
    // distance/duration for a run, so there is nothing to prefill and the buffers
    // stay empty; the editor never fabricates a run (HARD RULE 1).
    prescription: PrescriptionView? = null,
    // NEW-mode run type of the planned run this entry logs (the Finish review's
    // rule, [workoutTypeForPlan]); preselected, still the user's to change.
    plannedType: WorkoutType? = null,
    // The athlete's profile: whether the core can turn an average bpm into %HRmax.
    profile: ProfileView? = EventDispatcher.model.collectAsState().value?.profile,
    onClose: () -> Unit = {},
    // Report unsaved-edits state so a host sheet's swipe-down can guard it.
    onDirtyChange: (Boolean) -> Unit = {},
    onSave: (Event) -> Unit,
) {
    // Distance honors the mi override symmetric with the lb weight field: entry is in
    // the display unit, canonical km on the wire. `unit` fixes the editor's unit for
    // its lifetime (read once, like the set editor's weightUnit).
    val unit = rememberDistanceUnit()
    var distText by rememberSaveable {
        // km-native prefill is lossless; mi shows the converted %.2f figure. A blank
        // buffer (a fresh entry with nothing to seed) stays empty.
        mutableStateOf(
            initial?.let {
                if (unit == DistanceUnit.Mi) String.format(Locale.US, "%.2f", kmToMi(it.distance_km))
                else fmtLosslessPrefill(it.distance_km)
            } ?: ""
        )
    }
    // Lossless untouched-prefill for distance, mirroring weightFresh: a mi entry
    // the user never edits must re-emit the stored km verbatim (km→mi→km drifts).
    // Flips false on the first distance edit.
    var distFresh by rememberSaveable { mutableStateOf(initial != null) }
    // Duration is typed like a timer: h:mm:ss filling right to left ("14530" reads
    // 1:45:30). The digit buffer is exactly what is displayed and parsed at Save.
    var durDigits by rememberSaveable {
        mutableStateOf(initial?.let { minutesToDurationDigits(it.duration_min) } ?: "")
    }
    // Lossless untouched prefill: the h:mm:ss display rounds to whole seconds, so an
    // EDIT saved without touching the duration re-emits the stored minutes.
    var durFresh by rememberSaveable { mutableStateOf(initial != null) }
    var active by rememberSaveable { mutableStateOf(RunField.Distance) }
    // First key after (re)targeting a field starts a fresh number.
    var fresh by rememberSaveable { mutableStateOf(true) }
    // The shared keypad: open on the editor's first field, folded by system Back
    // (Back closes the keypad before it asks to discard or closes the editor);
    // tapping a field opens it again.
    var keypadOpen by rememberSaveable { mutableStateOf(true) }
    // Average heart rate: ONE keypad field, empty = not recorded. The core treats
    // hr_pct_max == 0 as "no HR sample" and reports zone "-" rather than
    // fabricating one, so a run logged without a monitor sends 0. bpm, the figure
    // a watch or strap shows, is offered when the core can convert it (a
    // plausible measured HRmax or an age for the estimate); % of max is the
    // alternative unit. A fresh entry, and an edit of a row logged in bpm, opens on
    // bpm with the logged bpm; an edit of a row stored as % opens on % with the
    // stored value. Either way an untouched Save re-emits exactly what was logged
    // (the view's hr_pct_max of a bpm row is the core's derived figure, never typed).
    val hrMaxKnown = profile != null &&
        ((profile.measured_hr_max ?: 0.0) in PlausibleHrMaxBpm || (profile.age_years ?: 0.0) in TanakaAgeRange)
    val loggedBpm = initial?.avg_hr_bpm?.takeIf { it > 0.0 }
    val loggedPct = initial?.let { loggedHrPctMax(it) }?.takeIf { it > 0.0 }
    var hrInBpm by rememberSaveable { mutableStateOf(loggedPct == null) }
    // A row logged in bpm stays in bpm even if the profile no longer lets the core
    // convert it: the bpm is the user's figure, and % would misread it.
    val bpmMode = hrInBpm && (hrMaxKnown || loggedBpm != null)
    var hrText by rememberSaveable {
        mutableStateOf((loggedBpm ?: loggedPct)?.let { fmtLosslessPrefill(it) } ?: "")
    }
    val hrParsed = parseKeypadNumber(hrText)
    val hrMax = if (bpmMode) RunAvgHrMaxBpm else RunHrMaxPct
    // Blank is a legitimate "no HR"; anything typed must be in range.
    val hrValid = hrText.isBlank() || (hrParsed != null && hrParsed in RunHrMin..hrMax)
    // Whole-session effort (CR-10), optional; prefilled from the row when editing.
    var sessionRpe by rememberSaveable { mutableStateOf(initial?.session_rpe) }
    var observedAt by rememberSaveable {
        mutableStateOf(
            initial?.observed_at?.takeIf { it > 0 } ?: initialObservedAt ?: AppClock.nowSec()
        )
    }
    var dirty by rememberSaveable { mutableStateOf(false) }
    ReportDirty(dirty, onDirtyChange)
    // A legacy row (entry_id == 0) is amended by matching its original stamp,
    // so its date can't be changed without orphaning it; lock the date chip.
    val dateEditable = initial == null || initial.entry_id != 0L
    // User-declared run-intent label. Optional; null = untagged, which the
    // editor never fabricates. Prefilled from the row when editing (decode-safe:
    // an unknown wire string maps to null via WorkoutType.fromWire), and from the
    // planned run a new entry logs.
    var workoutType by rememberSaveable {
        mutableStateOf(if (initial != null) WorkoutType.fromWire(initial.workout_type) else plannedType)
    }

    val distParsed = parseKeypadNumber(distText)
    val durParsed = durationDigitsToMin(durDigits)
    // Bound in the displayed unit: the mi ceiling is the EXACT converted km limit
    // (62.14 mi) so a wire-valid near-max km run stays editable in mi mode.
    val distMax = if (unit == DistanceUnit.Mi) kmToMi(RunDistanceMaxKm) else RunDistanceMaxKm
    val distValid = distParsed != null && distParsed in 0.0..distMax
    val durValid = durParsed != null && durParsed in 0.0..RunDurationMaxMin
    // A blank buffer is "not entered yet", not an error: no red, Save stays gated.
    val distEmpty = distText.isBlank()
    val durInvalid = durDigits.isNotEmpty() && !durValid
    // A 0-km/0-min run is a legitimate in-progress buffer (not "invalid", unlike a
    // 0-kg set, which logs fine), so it doesn't mark the fields red. But the core
    // won't take a zero-distance/duration run, so Save is gated on >0. When BOTH
    // parse as valid numbers yet one is still zero, the gate silently kills Save;
    // surface a run-specific caption so the dead button has a reason.
    val positive = (distParsed ?: 0.0) > 0.0 && (durParsed ?: 0.0) > 0.0
    val zeroBlocksSave = distValid && durValid && !positive

    fun keyIn(key: Char) {
        when (active) {
            RunField.Distance -> {
                distText = editNumericBuffer(distText, key, fresh)
                distFresh = false
            }
            RunField.Duration -> {
                durDigits = editDurationDigits(durDigits, key, fresh)
                durFresh = false
            }
            RunField.HeartRate -> hrText = editNumericBuffer(hrText, key, fresh)
        }
        fresh = false
        dirty = true
    }
    fun backspace() {
        when (active) {
            RunField.Distance -> {
                distText = distText.dropLast(1)
                distFresh = false
            }
            RunField.Duration -> {
                durDigits = durDigits.dropLast(1)
                durFresh = false
            }
            RunField.HeartRate -> hrText = hrText.dropLast(1)
        }
        fresh = false
        dirty = true
    }

    val runSavedText = stringResource(R.string.log_run_saved)
    val viewLabel = stringResource(R.string.import_view)

    EditorScaffold(
        title = if (initial == null) stringResource(R.string.main_log_run_title) else stringResource(R.string.log_run_title_edit),
        dirty = dirty,
        saveEnabled = distValid && durValid && positive && hrValid,
        onClose = onClose,
        onSave = {
            if (distParsed != null && durParsed != null) {
                val ev = runEditEvent(
                    initial,
                    // Canonical km on the wire: a mi entry converts at the save
                    // boundary via miToKmWire (km entry passes through exact).
                    distanceKm = runSaveDistanceKm(initial, distFresh, unit, distParsed),
                    durationMin = if (initial != null && durFresh) initial.duration_min else durParsed,
                    hrPctMax = if (bpmMode || hrText.isBlank()) 0.0 else hrParsed ?: 0.0,
                    observedAt = observedAt,
                    workoutType = workoutType,
                    sessionRpe = sessionRpe,
                    avgHrBpm = if (bpmMode && hrText.isNotBlank()) hrParsed else null,
                )
                onSave(ev)
                // A new run (possibly backdated, so not on today's screen) confirms
                // with a way to it; an edit is confirmed by its History host.
                if (ev is Event.LogRun) {
                    UiMessages.post(
                        UiMessage(
                            text = runSavedText,
                            actionLabel = viewLabel,
                            onAction = { HistoryFocus.request(ev.entryId, ev.observedAt) },
                        ),
                    )
                }
            }
        },
    ) {
        BackHandler(enabled = keypadOpen) { keypadOpen = false }
        FormCard {
            KeypadValueField(
                stringResource(R.string.log_field_distance), unit.distanceLabel, distText,
                active = keypadOpen && active == RunField.Distance,
                invalid = !distEmpty && !distValid,
                min = 0.0, max = distMax, format = "%.2f",
                adjustments = listOf(-0.5, 0.5, 1.0),
                onActivate = { active = RunField.Distance; fresh = true; keypadOpen = true },
                onText = { distText = it; fresh = false; distFresh = false; dirty = true },
            )
            // Timer-style h:mm:ss: no ± chips (they step a plain number), the keypad
            // types the digits. Its one hint slot says why Save is dead: an
            // impossible h:mm:ss, or numbers that are still zero.
            KeypadValueField(
                stringResource(R.string.log_field_duration), stringResource(R.string.log_unit_hms),
                fmtDurationDigits(durDigits),
                active = keypadOpen && active == RunField.Duration,
                invalid = false,
                min = 0.0, max = RunDurationMaxMin, format = "%.0f",
                adjustments = emptyList(),
                onActivate = { active = RunField.Duration; fresh = true; keypadOpen = true },
                onText = {},
                hint = when {
                    durInvalid -> stringResource(R.string.log_duration_invalid)
                    zeroBlocksSave -> stringResource(R.string.log_run_zero_hint)
                    else -> null
                },
                hintColor = if (durInvalid) DangerText else OnBgMuted,
            )
            // The keypad sits right under the two fields it types into.
            // Distance takes a decimal; the h:mm:ss digits and the HR number don't.
            if (keypadOpen) {
                NumericKeypad(onKey = { keyIn(it) }, onBackspace = { backspace() }, allowDecimal = active == RunField.Distance)
            }
            // Typed on the shared keypad above, like distance and duration.
            KeypadValueField(
                stringResource(R.string.track_log_hr_avg),
                stringResource(if (bpmMode) R.string.track_log_hr_bpm else R.string.log_hr_pct_max),
                hrText,
                active = keypadOpen && active == RunField.HeartRate,
                invalid = hrText.isNotBlank() && !hrValid,
                min = RunHrMin, max = hrMax, format = "%.0f",
                adjustments = listOf(-5.0, 5.0),
                onActivate = { active = RunField.HeartRate; fresh = true; keypadOpen = true },
                onText = { hrText = it; fresh = false; dirty = true },
                // When the core can convert bpm, the unit itself is the swap ("bpm ⇄"):
                // no extra row. The typed number belongs to the old unit, so it clears.
                onUnitClick = if (hrMaxKnown) {
                    {
                        hrInBpm = !hrInBpm
                        if (hrText.isNotEmpty()) dirty = true
                        hrText = ""
                        active = RunField.HeartRate
                        fresh = true
                        keypadOpen = true
                    }
                } else {
                    null
                },
                unitClickDescription = stringResource(if (bpmMode) R.string.log_hr_use_pct else R.string.log_hr_use_bpm),
            )
            // Whole-session effort (CR-10): optional; tap the picked number again to
            // clear it. Feeds the session-effort load series.
            EffortScale(sessionRpe, { sessionRpe = it; dirty = true })
            // Optional user-declared run type. USER DATA only; no coaching
            // reads it. Tapping the active chip clears it back to untagged; the
            // editor never fabricates a label the user didn't pick.
            RunTypeChips(workoutType, { picked ->
                workoutType = if (picked == workoutType) null else picked
                dirty = true
            })
            ObservedAtRow(observedAt, withTime = true, enabled = dateEditable) { observedAt = it; dirty = true }
        }
    }
}

/** Wire km for a run Save: an untouched prefill ([distFresh]) re-emits the stored
 *  km verbatim so a mi round-trip (km→mi→km) can't drift the distance; a touched
 *  field converts the displayed value (mi→km at the wire, km passes through). */
internal fun runSaveDistanceKm(
    initial: RunResultView?,
    distFresh: Boolean,
    unit: DistanceUnit,
    distParsed: Double,
): Double = if (initial != null && distFresh) initial.distance_km
    else if (unit == DistanceUnit.Mi) miToKmWire(distParsed) else distParsed

/** Longest timer-style duration buffer: hhmmss. */
private const val DurationDigitsMax = 6

/** Timer-style duration digits → minutes. Digits fill right to left: the last two
 *  are seconds, the two before minutes, the rest hours ("14530" = 1:45:30 =
 *  105.5 min). Null when empty, when seconds reach 60, or when minutes reach 60
 *  under an hours field (with no hours, "9000" reads 90:00 = 90 min). Pure. */
internal fun durationDigitsToMin(digits: String): Double? {
    if (digits.isEmpty() || digits.length > DurationDigitsMax || !digits.all { it.isDigit() }) return null
    val p = digits.padStart(DurationDigitsMax, '0')
    val h = p.substring(0, 2).toInt()
    val m = p.substring(2, 4).toInt()
    val s = p.substring(4, 6).toInt()
    if (s >= 60 || (h > 0 && m >= 60)) return null
    return h * 60.0 + m + s / 60.0
}

/** Display form of a duration digit buffer: "1:45:30", "45:30", "0:30"; "" when empty. Pure. */
internal fun fmtDurationDigits(digits: String): String {
    if (digits.isEmpty()) return ""
    val p = digits.padStart(DurationDigitsMax, '0').takeLast(DurationDigitsMax)
    val h = p.substring(0, 2).toInt()
    val m = p.substring(2, 4)
    val s = p.substring(4, 6)
    return if (h > 0) "$h:$m:$s" else "${m.toInt()}:$s"
}

/** A stored duration (minutes) as a timer digit buffer, rounded to the second
 *  ("105.5" → "14530"); "" for zero. Pure. */
internal fun minutesToDurationDigits(min: Double): String {
    val total = Math.round(min * 60.0).coerceIn(0L, 359_999L)
    val h = total / 3600
    val m = (total % 3600) / 60
    val s = total % 60
    val raw = if (h > 0) {
        String.format(Locale.US, "%d%02d%02d", h, m, s)
    } else {
        String.format(Locale.US, "%d%02d", m, s)
    }
    return raw.trimStart('0')
}

/** Apply one keypad key to a duration digit buffer: digits append (no leading
 *  zero, at most hhmmss), the decimal point is ignored. [replaceAll] starts a
 *  fresh number, like [editNumericBuffer]. Pure. */
internal fun editDurationDigits(digits: String, key: Char, replaceAll: Boolean): String {
    if (!key.isDigit()) return digits
    val base = if (replaceAll) "" else digits
    if (base.length >= DurationDigitsMax) return base
    if (base.isEmpty() && key == '0') return base
    return base + key
}
