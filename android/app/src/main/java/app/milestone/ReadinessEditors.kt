package app.milestone

import androidx.annotation.ArrayRes
import androidx.annotation.StringRes
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringArrayResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import java.util.Locale

/** The run length the decoupling reading opens on: past the core's 20-minute
 *  validity floor, so an untouched reading counts. */
private const val DecouplingDefaultMin = 30.0

/** The decoupling run-length scale: 0–180 min in 5-minute steps. */
private const val DecouplingRunMaxMin = 180
private const val DecouplingRunStepMin = 5

/** The morning check-in's HRV rMSSD (ms) a user can type; resting HR takes
 *  [RESTING_HR_RANGE] like the profile. */
private val HrvMsRange = 1.0..300.0

/**
 * Manual readiness entry. Surfaces every [ReadinessSignal], including the
 * medical-referral flags (Illness/RedS/CardiacRedFlag/BoneStress) that gate the
 * top of the safety ladder, so the core's safety layer is reachable from the
 * device. Signal semantics differ (z-scores, bpm deltas, %, 0/1 flags); the core
 * interprets the value.
 */
@Composable
fun ReadinessEditor(
    // Core-exported signal→group map (ViewModel.signal_groups): picker order and
    // the red-flag fence come from it, not from a shell-side predicate.
    signalGroups: Map<String, String>,
    // Deep-link entry point (Today "+ Add" chips): the signal the editor opens
    // pre-selected. Defaults to WellnessZ (the manual/advanced default).
    initialSignal: ReadinessSignal = ReadinessSignal.WellnessZ,
    onClose: () -> Unit = {},
    // Mirror unsaved-edits up to a host so its swipe-down dismissal can guard it.
    onDirtyChange: (Boolean) -> Unit = {},
    onSubmit: (Event.SubmitReadiness) -> Unit,
) {
    var signal by rememberSaveable { mutableStateOf(initialSignal) }
    var value by rememberSaveable { mutableStateOf(defaultReadinessValue(initialSignal)) }
    var observedAt by rememberSaveable { mutableStateOf(AppClock.nowSec()) }
    // Effort minutes for the duration-gated AerobicDecoupling signal (valid only
    // >20 min, File 06). Defaults to a validating 30 min; sent only for that
    // signal so the core can validate the reading instead of silently discarding it.
    var effortMin by rememberSaveable { mutableStateOf(DecouplingDefaultMin) }
    var dirty by rememberSaveable { mutableStateOf(false) }
    ReportDirty(dirty, onDirtyChange)
    // First red-flag signal in wire order, so the fence lands before the first
    // medical flag as displayed.
    val firstRedFlag = remember(signalGroups) {
        signalGroups.entries.firstOrNull { it.value == "red_flag" }
            ?.let { e -> ReadinessSignal.entries.firstOrNull { it.name == e.key } }
    }

    EditorScaffold(
        title = stringResource(R.string.log_readiness_title),
        dirty = dirty,
        saveEnabled = dirty,
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
        FormCard {
            // What a readiness signal is and what submitting one does; the
            // coaching interpretation stays in the core.
            Text(
                stringResource(R.string.log_readiness_intro),
                color = OnBgMuted,
                style = Type.Caption,
            )
            // Picker order comes from the core's signal_groups metadata (ALL_SIGNALS
            // order on the wire), so routine metrics, Soreness included, render
            // together above the medical red-flag fence.
            val ordered = remember(signalGroups) {
                signalGroups.keys.mapNotNull { name -> ReadinessSignal.entries.firstOrNull { it.name == name } }
            }
            val signalLabels = ordered.associateWith { stringResource(it.labelRes) }
            EnumRow(
                stringResource(R.string.log_field_signal),
                ordered,
                signal,
                display = { signalLabels.getValue(it) },
                // Fence the medical red-flag block off from the routine metrics
                // above it, where the core's signal_groups says it starts.
                divideBefore = { it == firstRedFlag },
            ) {
                signal = it
                value = defaultReadinessValue(it)
                dirty = true
            }
            // One-line neutral explainer for the selected signal: what it is, not
            // what number triggers what (the value hints below cite thresholds,
            // see [ValueSpec]).
            Text(stringResource(signal.ui.explainer), color = OnBgMuted, style = Type.Caption)
            val spec = signal.ui.spec
            when {
                spec != null -> {
                    // The whole value grid is visible and one tap picks any point,
                    // negatives (z-scores, deltas) included, which the keypad can't type.
                    FieldLabel(stringResource(R.string.log_field_value), String.format(Locale.US, spec.format, value))
                    ScrollableScaleRow(
                        spec.gridOptions(),
                        value,
                        { String.format(Locale.US, spec.format, it) },
                        label = stringResource(signal.labelRes),
                    ) {
                        value = it
                        dirty = true
                    }
                    // The unit and the point where the value starts to matter.
                    Text(stringResource(spec.hint), color = OnBgMuted, style = Type.Caption)
                }
                signal == ReadinessSignal.Illness -> {
                    val level = IllnessLevel.entries.lastOrNull { value >= it.value } ?: IllnessLevel.None
                    val levelLabels = IllnessLevel.entries.associateWith { stringResource(it.labelRes) }
                    // Whole-row option list, not segmented: the triage labels are too
                    // long to segment and must not be abbreviated (safety-relevant).
                    EnumRow(stringResource(R.string.log_field_severity), IllnessLevel.entries.toList(), level, display = { levelLabels.getValue(it) }) {
                        value = it.value
                        dirty = true
                    }
                }
                else ->
                    // A yes/no answer to a red-flag symptom question (the signal and
                    // its explainer sit right above), so the switch is the affirmative.
                    LabeledSwitchRow(stringResource(R.string.log_switch_yes_have), value > 0.0) {
                        value = if (it) 1.0 else 0.0
                        dirty = true
                    }
            }
            // AerobicDecoupling is duration-gated (valid only >20 min, File 06): the
            // core discards a reading with no duration, so the run length is sent too.
            if (signal == ReadinessSignal.AerobicDecoupling) {
                FieldLabel(stringResource(R.string.log_field_run_duration), stringResource(R.string.log_value_min, effortMin.toInt()))
                ScrollableScaleRow(
                    (0..DecouplingRunMaxMin step DecouplingRunStepMin).map { it.toDouble() },
                    effortMin,
                    { "${it.toInt()}" },
                    label = stringResource(R.string.log_field_run_duration),
                ) {
                    effortMin = it
                    dirty = true
                }
                Text(
                    stringResource(R.string.log_decoupling_hint),
                    color = OnBgMuted,
                    style = Type.Caption,
                )
            }
            ObservedAtRow(observedAt, withTime = false) { observedAt = it; dirty = true }
        }
    }
}

/** Which optional watch number the shared keypad currently edits. */
private enum class CheckinField { RestingHr, Hrv }

/** The 1–5 cells of a check-in scale where 5 is the good end (sleep, mood). */
private val ScaleHighGood = (1..5).toList()

/** The cells of a scale where 5 is the bad end (soreness, fatigue, stress),
 *  reversed so the good end sits on the right like every other row. */
private val ScaleHighBad = (5 downTo 1).toList()

/** Whether a check-in echo stamped [echoObservedAt] is today's own answer
 *  (rehydrated for editing) rather than an earlier day's (shown only as a
 *  "Last time" hint), in the device's local calendar. Pure. */
internal fun checkinEchoIsToday(echoObservedAt: Long, nowSec: Long, zone: java.time.ZoneId): Boolean =
    localEpochDay(echoObservedAt, zone) == localEpochDay(nowSec, zone)

/**
 * Morning check-in, the primary readiness entry: the five 1–5 wellness items, two
 * optional watch numbers and the Illness item (first when [illnessPrompt]), all
 * optional. The core turns the history into z-scores; the user never enters one.
 */
@Composable
fun MorningCheckinSheet(
    // The core's most recent check-in echo, if any.
    echo: CheckinEchoView? = null,
    onClose: () -> Unit = {},
    // Mirror unsaved-edits up to a host so its swipe-down dismissal can guard it.
    onDirtyChange: (Boolean) -> Unit = {},
    // The core asks for the illness check (an active illness report, or a high
    // resting-HR morning): the Illness item moves to the top.
    illnessPrompt: Boolean = false,
    // The check-in (null when only the illness question was answered), and the
    // illness answer when one was given.
    onSubmit: (Event.SubmitCheckin?, Event.SubmitReadiness?) -> Unit,
) {
    // Today's own check-in is today's answer, so a reopen rehydrates it for
    // editing. An earlier day's answers are only a hint beside each item ("Last
    // time: 4"): saved as today's they would skew the baseline the coach compares
    // against, so every item needs a fresh tap. Every item is optional (the core
    // averages only what was answered), so Save opens with the first answer.
    val todayEcho = echo?.takeIf { checkinEchoIsToday(it.observed_at, AppClock.nowSec(), java.time.ZoneId.systemDefault()) }
    val pastEcho = if (todayEcho == null) echo else null
    var sleep by rememberSaveable { mutableStateOf(todayEcho?.sleep_quality) }
    var soreness by rememberSaveable { mutableStateOf(todayEcho?.soreness) }
    var mood by rememberSaveable { mutableStateOf(todayEcho?.mood) }
    // The rest of the KB 5-item short-form. Every item MAY stay unset: the core
    // averages only the answered items and the encode omits an unanswered key
    // from the wire.
    var fatigue by rememberSaveable { mutableStateOf(todayEcho?.fatigue) }
    var stress by rememberSaveable { mutableStateOf(todayEcho?.stress) }
    var observedAt by rememberSaveable { mutableStateOf(AppClock.nowSec()) }
    // Optional illness answer (AUTOREG-4): null = not answered, nothing sent.
    var illness by rememberSaveable { mutableStateOf<IllnessLevel?>(null) }
    var dirty by rememberSaveable { mutableStateOf(false) }
    ReportDirty(dirty, onDirtyChange)

    // Optional watch numbers behind toggles: a check-in without a watch sends
    // neither, and the core simply won't derive that channel, honest, never
    // fabricated. A toggle starts on when the last check-in carried that number
    // (the user has the watch), but only today's own reading fills the field;
    // an empty field sends nothing.
    var hasRhr by rememberSaveable { mutableStateOf(echo?.resting_hr_bpm != null) }
    var hasHrv by rememberSaveable { mutableStateOf(echo?.hrv_rmssd_ms != null) }
    var rhrText by rememberSaveable {
        mutableStateOf(todayEcho?.resting_hr_bpm?.let { String.format(Locale.US, "%.0f", it) } ?: "")
    }
    var hrvText by rememberSaveable {
        mutableStateOf(todayEcho?.hrv_rmssd_ms?.let { String.format(Locale.US, "%.0f", it) } ?: "")
    }
    var active by rememberSaveable { mutableStateOf(CheckinField.RestingHr) }
    var fresh by rememberSaveable { mutableStateOf(true) }
    // The keypad opens right under the field it types into, on a tap of that
    // field (or switching its toggle on), and folds when another item is answered.
    var keypadOpen by rememberSaveable { mutableStateOf(false) }

    val rhrParsed = parseKeypadNumber(rhrText)
    val hrvParsed = parseKeypadNumber(hrvText)
    val rhrValid = !hasRhr || rhrText.isEmpty() || (rhrParsed != null && rhrParsed in RESTING_HR_RANGE)
    val hrvValid = !hasHrv || hrvText.isEmpty() || (hrvParsed != null && hrvParsed in HrvMsRange)
    val savedMessage = stringResource(R.string.log_checkin_saved)

    fun activate(field: CheckinField) {
        active = field
        fresh = true
        keypadOpen = true
    }
    // A watch number's toggle. Switching the keypad's field off re-routes the
    // keypad to the other field (if shown), so it never types into a hidden one.
    fun toggle(field: CheckinField, on: Boolean) {
        if (field == CheckinField.RestingHr) hasRhr = on else hasHrv = on
        dirty = true
        val other = if (field == CheckinField.RestingHr) CheckinField.Hrv else CheckinField.RestingHr
        val otherShown = if (field == CheckinField.RestingHr) hasHrv else hasRhr
        if (on) {
            activate(field)
        } else if (active == field) {
            keypadOpen = false
            if (otherShown) {
                active = other
                fresh = true
            }
        }
    }
    fun setText(field: CheckinField, text: String) {
        if (field == CheckinField.RestingHr) rhrText = text else hrvText = text
        fresh = false
        dirty = true
    }
    // A scale or illness answer hands focus away from the watch numbers.
    fun answer(pick: () -> Unit) {
        pick()
        keypadOpen = false
        dirty = true
    }

    EditorScaffold(
        title = stringResource(R.string.log_checkin_title),
        dirty = dirty,
        // Any one answer is a check-in; nothing is ever filled in for the user.
        saveEnabled = rhrValid && hrvValid && (
            sleep != null || soreness != null || mood != null || fatigue != null || stress != null ||
                (hasRhr && rhrParsed != null) || (hasHrv && hrvParsed != null) || illness != null
            ),
        onClose = onClose,
        onSave = {
            val rhr = if (hasRhr) rhrParsed else null
            val hrv = if (hasHrv) hrvParsed else null
            val anyCheckin = sleep != null || soreness != null || mood != null || fatigue != null ||
                stress != null || rhr != null || hrv != null
            val checkin = if (anyCheckin) {
                Event.SubmitCheckin(
                    observedAt = observedAt,
                    sleepQuality = sleep,
                    soreness = soreness,
                    mood = mood,
                    fatigue = fatigue,
                    stress = stress,
                    restingHrBpm = rhr,
                    hrvRmssdMs = hrv,
                )
            } else {
                null
            }
            val ill = illness?.let { Event.SubmitReadiness(ReadinessSignal.Illness, it.value, observedAt) }
            onSubmit(checkin, ill)
            UiMessages.post(UiMessage(savedMessage))
        },
    ) {
        FormCard {
            if (illnessPrompt) {
                IllnessQuestion(illness, prompted = true) { v -> answer { illness = v } }
            }
            // The core's order is kept (soreness, fatigue and stress run 5→1), so
            // one note says which way every row reads.
            Text(stringResource(R.string.log_checkin_scale_note), color = OnBgMuted, style = Type.Caption)
            CheckinItem.entries.forEach { item ->
                val (value, past) = when (item) {
                    CheckinItem.Sleep -> sleep to pastEcho?.sleep_quality
                    CheckinItem.Soreness -> soreness to pastEcho?.soreness
                    CheckinItem.Mood -> mood to pastEcho?.mood
                    CheckinItem.Fatigue -> fatigue to pastEcho?.fatigue
                    CheckinItem.Stress -> stress to pastEcho?.stress
                }
                CheckinScaleRow(item, value, lastTimeHint(past)) { v ->
                    answer {
                        when (item) {
                            CheckinItem.Sleep -> sleep = v
                            CheckinItem.Soreness -> soreness = v
                            CheckinItem.Mood -> mood = v
                            CheckinItem.Fatigue -> fatigue = v
                            CheckinItem.Stress -> stress = v
                        }
                    }
                }
            }

            for (field in CheckinField.entries) {
                val rhr = field == CheckinField.RestingHr
                WatchNumberRow(
                    switchLabel = stringResource(if (rhr) R.string.log_switch_add_rhr else R.string.log_switch_add_hrv),
                    switchHint = stringResource(if (rhr) R.string.log_switch_rhr_hint else R.string.log_switch_hrv_hint),
                    shown = if (rhr) hasRhr else hasHrv,
                    onToggle = { toggle(field, it) },
                    label = stringResource(if (rhr) R.string.log_field_resting_hr else R.string.log_field_hrv),
                    unit = stringResource(if (rhr) R.string.log_unit_bpm else R.string.log_unit_ms),
                    text = if (rhr) rhrText else hrvText,
                    // Red only once something out-of-range is typed: an empty
                    // field isn't an error, it sends nothing.
                    invalid = if (rhr) !rhrValid else !hrvValid,
                    range = if (rhr) RESTING_HR_RANGE else HrvMsRange,
                    adjustments = if (rhr) listOf(-1.0, 1.0) else listOf(-1.0, 1.0, 5.0),
                    lastValue = if (rhr) pastEcho?.resting_hr_bpm else pastEcho?.hrv_rmssd_ms,
                    keypadOpen = keypadOpen && active == field,
                    fresh = fresh,
                    onActivate = { activate(field) },
                    // A ± chip sets a whole value; it needs no keypad, so it folds.
                    onChip = { setText(field, it); keypadOpen = false },
                    onBuffer = { setText(field, it) },
                    onDone = { keypadOpen = false },
                )
                // AUTOREG-6: the HRV baseline only means something from one source.
                if (!rhr && hasHrv) Text(stringResource(R.string.log_hrv_help), color = OnBgMuted, style = Type.Caption)
            }
            if (!illnessPrompt) {
                IllnessQuestion(illness, prompted = false) { v -> answer { illness = v } }
            }
            ObservedAtRow(observedAt, withTime = false) { observedAt = it; dirty = true }
        }
    }
}

/** "Last time: 4" beside an item whose previous answer is from an earlier day;
 *  null (no hint) when there is none. */
@Composable
private fun lastTimeHint(value: Int?): String? =
    value?.let { stringResource(R.string.log_checkin_last, "$it") }

/** The five 1–5 wellness items of the morning check-in, in the core's order:
 *  [options] is the drawn order (soreness, fatigue and stress run 5→1, worse end
 *  first), [left]/[right] name the two ends and [words] holds the value words
 *  (indexed value − 1). */
private enum class CheckinItem(
    @StringRes val label: Int,
    val options: List<Int>,
    @StringRes val left: Int,
    @StringRes val right: Int,
    @ArrayRes val words: Int,
) {
    Sleep(R.string.log_checkin_sleep, ScaleHighGood, R.string.log_anchor_terrible, R.string.log_anchor_great, R.array.rating_sleep_words),
    Soreness(R.string.log_checkin_soreness, ScaleHighBad, R.string.log_anchor_very_sore, R.string.log_anchor_none, R.array.rating_soreness_words),
    Mood(R.string.log_checkin_mood, ScaleHighGood, R.string.log_anchor_awful, R.string.log_anchor_great, R.array.rating_mood_words),
    Fatigue(R.string.log_checkin_fatigue, ScaleHighBad, R.string.log_anchor_exhausted, R.string.log_anchor_fresh, R.array.rating_fatigue_words),
    Stress(R.string.log_checkin_stress, ScaleHighBad, R.string.log_anchor_stressed, R.string.log_anchor_calm, R.array.rating_stress_words),
}

/**
 * One check-in [item] as a [RatingScale] with the picked value's word at the
 * label's end. [hint] is an earlier answer shown while unanswered, never a
 * selection. The stored value is the number the core reads
 * (schema.rs::CheckinInput), whatever side it is drawn on. Test tag
 * "checkin-<item, lowercase>".
 */
@Composable
private fun CheckinScaleRow(item: CheckinItem, value: Int?, hint: String?, onValue: (Int) -> Unit) {
    val words = stringArrayResource(item.words)
    val shown = item.options.associateWith { stringResource(R.string.rating_value_word, it, words.getOrElse(it - 1) { "" }) }
    RatingScale(
        label = stringResource(item.label),
        values = item.options,
        selected = value,
        onSelect = { picked -> picked?.let(onValue) },
        startCaption = stringResource(item.left),
        endCaption = stringResource(item.right),
        selectedText = { shown.getValue(it) },
        // FormCard spaces fields by Space.Md; the extra bottom padding makes the
        // gap to the next field Space.Lg.
        modifier = Modifier.padding(bottom = (Space.Lg - Space.Md).dp),
        clearable = false,
        testTag = "checkin-${item.name.lowercase()}",
        hint = hint,
    )
}

/** An optional watch number behind its toggle: the switch, and while [shown] a
 *  whole-number keypad field with the earlier reading ([lastValue]) as its hint
 *  and, while [keypadOpen], the keypad under it. */
@Composable
private fun WatchNumberRow(
    switchLabel: String,
    switchHint: String,
    shown: Boolean,
    onToggle: (Boolean) -> Unit,
    label: String,
    unit: String,
    text: String,
    invalid: Boolean,
    range: ClosedFloatingPointRange<Double>,
    adjustments: List<Double>,
    lastValue: Double?,
    keypadOpen: Boolean,
    fresh: Boolean,
    onActivate: () -> Unit,
    onChip: (String) -> Unit,
    onBuffer: (String) -> Unit,
    onDone: () -> Unit,
) {
    LabeledSwitchRow(switchLabel, shown, switchHint, onCheckedChange = onToggle)
    if (!shown) return
    KeypadValueField(
        label, unit, text,
        active = keypadOpen,
        invalid = text.isNotEmpty() && invalid,
        min = range.start, max = range.endInclusive, format = "%.0f",
        adjustments = adjustments,
        onActivate = onActivate,
        onText = onChip,
        hint = lastValue?.let { stringResource(R.string.log_checkin_last_unit, String.format(Locale.US, "%.0f", it), unit) },
    )
    // Resting HR and rMSSD are whole numbers.
    KeypadPanel(keypadOpen) { NumericKeypad(text, fresh, onBuffer, allowDecimal = false, onDone = onDone) }
}

/**
 * The check-in's Illness item (AUTOREG-4, ILLNESS-RTT-001): none / head
 * cold only / flu, fever or body aches, the three levels the core's illness
 * gate reads (value 0 / 1 above the neck / 2 below the neck or fever). The
 * question is a [FieldLabel] like every other question (a TalkBack heading); a
 * helper line says what each answer means for training; each answer is its own
 * full-width choice cell (the scale cells' look), so the three read as choices,
 * not a paragraph. Optional unless the core asks first ([prompted]).
 */
@Composable
private fun IllnessQuestion(value: IllnessLevel?, prompted: Boolean, onValue: (IllnessLevel) -> Unit) {
    FieldLabel(
        stringResource(R.string.log_illness_q),
        trailing = if (prompted) null else stringResource(R.string.log_optional),
        heading = true,
    )
    Text(stringResource(R.string.log_illness_help), color = OnBgMuted, style = Type.Caption)
    Column(
        modifier = Modifier.fillMaxWidth().selectableGroup(),
        verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
    ) {
        IllnessLevel.entries.forEach { level ->
            ChoiceRow(stringResource(level.answerRes), null, selected = value == level) { onValue(level) }
        }
    }
}

/**
 * The value a freshly-selected signal should start at. Flags default to 1.0
 * ("present") so a user who picks a red flag and taps Submit actually triggers
 * the safety tier; Illness defaults to the milder above-neck band; continuous
 * signals start at their spec default (neutral for deltas/z-scores, 1.0 for the
 * e1RM ratio, a value the core reads as "no change vs baseline").
 */
private fun defaultReadinessValue(signal: ReadinessSignal): Double =
    signal.ui.spec?.default ?: if (signal == ReadinessSignal.Illness) IllnessLevel.AboveNeck.value else 1.0

/**
 * Scale bounds/step/default for one continuous readiness signal, mapped to the
 * autoreg.rs input thresholds so the scale lands near where the signal triggers:
 * the core reads [ReadinessSignal.EstimatedOneRm] as a ~0.9–1.05 ratio,
 * [ReadinessSignal.Rpe] as a signed delta, RestingHr as a bpm delta, others as
 * z-scores or percents. Hint thresholds cite autoreg.rs inputs the bridge does
 * not export.
 */
private data class ValueSpec(
    val min: Double,
    val max: Double,
    val step: Double,
    val default: Double,
    // Unit + threshold caption.
    @StringRes val hint: Int,
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

/**
 * The picker's view of one signal: its label, a one-line explainer (what the
 * signal IS and, for the medical flags, the core-documented consequence; no
 * shell-side thresholds), and the value scale of a continuous signal. [spec] is
 * null for Illness (a severity picker) and for the flags the core reads as
 * present at `value > 0` (Pain, RedS, CardiacRedFlag, BoneStress): those get an
 * on/off switch, since a scale's 0.0 default would record the flag's absence.
 */
private class SignalUi(@StringRes val label: Int, @StringRes val explainer: Int, val spec: ValueSpec? = null)

/** One exhaustive `when`, so a new [ReadinessSignal] fails to compile until it
 *  has a label, an explainer and a scale decision. Display-only: the wire value
 *  is still `signal.name` (see [Event.SubmitReadiness]). */
private val SignalUis: Map<ReadinessSignal, SignalUi> = ReadinessSignal.entries.associateWith { signal ->
    when (signal) {
        // Signed RPE delta (actual − target); ±1/±2 gate load adjustments.
        ReadinessSignal.Rpe -> SignalUi(
            R.string.log_signal_rpe, R.string.log_signal_rpe_explainer,
            ValueSpec(-4.0, 4.0, 0.5, 0.0, R.string.log_spec_rpe),
        )
        // e1RM today ÷ baseline; <0.90/<0.95/>1.05 gate deload/cap/add-load.
        ReadinessSignal.EstimatedOneRm -> SignalUi(
            R.string.log_signal_e1rm, R.string.log_signal_e1rm_explainer,
            ValueSpec(0.70, 1.15, 0.01, 1.0, R.string.log_spec_e1rm, "%.2f"),
        )
        // Mean concentric velocity, m/s (dormant, no autoreg gate yet).
        ReadinessSignal.BarVelocity -> SignalUi(
            R.string.log_signal_bar_velocity, R.string.log_signal_bar_velocity_explainer,
            ValueSpec(0.0, 2.0, 0.05, 0.5, R.string.log_spec_bar_velocity, "%.2f"),
        )
        // Within-set velocity-loss %; ≥20 terminates the set.
        ReadinessSignal.VelocityLoss -> SignalUi(
            R.string.log_signal_velocity_loss, R.string.log_signal_velocity_loss_explainer,
            ValueSpec(0.0, 50.0, 5.0, 0.0, R.string.log_spec_velocity_loss, "%.0f"),
        )
        // Composite wellness z-score; ≤ −1.5 downgrades.
        ReadinessSignal.WellnessZ -> SignalUi(
            R.string.log_signal_wellness, R.string.log_signal_wellness_explainer,
            ValueSpec(-3.0, 3.0, 0.5, 0.0, R.string.log_spec_wellness),
        )
        // lnRMSSD z vs rolling baseline; < −0.5 downgrades.
        ReadinessSignal.HrvLnRmssd -> SignalUi(
            R.string.log_signal_hrv, R.string.log_signal_hrv_explainer,
            ValueSpec(-3.0, 3.0, 0.5, 0.0, R.string.log_spec_hrv),
        )
        // HRV coefficient of variation % (dormant, no autoreg gate yet).
        ReadinessSignal.HrvCv -> SignalUi(
            R.string.log_signal_hrv_cv, R.string.log_signal_hrv_cv_explainer,
            ValueSpec(0.0, 15.0, 0.5, 0.0, R.string.log_spec_hrv_cv),
        )
        // Aerobic-decoupling drift %; >10 keeps the run easy.
        ReadinessSignal.AerobicDecoupling -> SignalUi(
            R.string.log_signal_decoupling, R.string.log_signal_decoupling_explainer,
            ValueSpec(0.0, 30.0, 1.0, 0.0, R.string.log_spec_decoupling, "%.0f"),
        )
        // RHR delta vs baseline, bpm; 5–9 downgrades, ≥10 stops. The label says
        // "vs normal": this field submits a delta, not the absolute bpm the
        // morning check-in takes.
        ReadinessSignal.RestingHr -> SignalUi(
            R.string.log_signal_rhr, R.string.log_signal_rhr_explainer,
            ValueSpec(-5.0, 20.0, 1.0, 0.0, R.string.log_spec_rhr, "%.0f"),
        )
        // Soreness on the 1–7 Hooper scale; ≥6 downgrades intensity one level.
        ReadinessSignal.Soreness -> SignalUi(
            R.string.log_signal_soreness, R.string.log_signal_soreness_explainer,
            ValueSpec(1.0, 7.0, 1.0, 3.0, R.string.log_spec_soreness, "%.0f"),
        )
        ReadinessSignal.Pain -> SignalUi(R.string.log_signal_pain, R.string.log_signal_pain_explainer)
        ReadinessSignal.Illness -> SignalUi(R.string.log_signal_illness, R.string.log_signal_illness_explainer)
        ReadinessSignal.RedS -> SignalUi(R.string.log_signal_reds, R.string.log_signal_reds_explainer)
        ReadinessSignal.CardiacRedFlag -> SignalUi(R.string.log_signal_cardiac, R.string.log_signal_cardiac_explainer)
        ReadinessSignal.BoneStress -> SignalUi(R.string.log_signal_bone_stress, R.string.log_signal_bone_stress_explainer)
    }
}

private val ReadinessSignal.ui: SignalUi get() = SignalUis.getValue(this)

/** The signal's picker label (the readiness editor and Today's chips). */
@get:StringRes
internal val ReadinessSignal.labelRes: Int get() = ui.label

/**
 * The Illness answer (AUTOREG-4, ILLNESS-RTT-001). [value] is the wire value the
 * core decodes (schema.rs `IllnessSeverity::from_value`: ≥2 below-neck/fever, ≥1
 * above-neck, else none); [labelRes] is the raw-signal editor's short label,
 * [answerRes] the check-in's longer answer.
 */
private enum class IllnessLevel(val value: Double, @StringRes val labelRes: Int, @StringRes val answerRes: Int) {
    None(0.0, R.string.log_illness_level_none, R.string.log_illness_level_none),
    AboveNeck(1.0, R.string.log_illness_level_above, R.string.log_illness_above_neck),
    BelowNeckOrFever(2.0, R.string.log_illness_level_below, R.string.log_illness_below_neck),
}
