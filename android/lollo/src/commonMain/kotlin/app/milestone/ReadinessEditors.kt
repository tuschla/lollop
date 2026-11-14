package app.milestone

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
import androidx.compose.ui.unit.dp
import app.milestone.resources.Res
import app.milestone.resources.log_anchor_awful
import app.milestone.resources.log_anchor_calm
import app.milestone.resources.log_anchor_exhausted
import app.milestone.resources.log_anchor_fresh
import app.milestone.resources.log_anchor_great
import app.milestone.resources.log_anchor_none
import app.milestone.resources.log_anchor_stressed
import app.milestone.resources.log_anchor_terrible
import app.milestone.resources.log_anchor_very_sore
import app.milestone.resources.log_checkin_fatigue
import app.milestone.resources.log_checkin_last
import app.milestone.resources.log_checkin_last_unit
import app.milestone.resources.log_checkin_mood
import app.milestone.resources.log_checkin_saved
import app.milestone.resources.log_checkin_scale_note
import app.milestone.resources.log_checkin_sleep
import app.milestone.resources.log_checkin_soreness
import app.milestone.resources.log_checkin_stress
import app.milestone.resources.log_checkin_title
import app.milestone.resources.log_decoupling_hint
import app.milestone.resources.log_field_hrv
import app.milestone.resources.log_field_resting_hr
import app.milestone.resources.log_field_run_duration
import app.milestone.resources.log_field_severity
import app.milestone.resources.log_field_signal
import app.milestone.resources.log_field_value
import app.milestone.resources.log_hrv_help
import app.milestone.resources.log_illness_above_neck
import app.milestone.resources.log_illness_below_neck
import app.milestone.resources.log_illness_help
import app.milestone.resources.log_illness_level_above
import app.milestone.resources.log_illness_level_below
import app.milestone.resources.log_illness_level_none
import app.milestone.resources.log_illness_q
import app.milestone.resources.log_optional
import app.milestone.resources.log_readiness_intro
import app.milestone.resources.log_readiness_title
import app.milestone.resources.log_signal_bar_velocity_explainer
import app.milestone.resources.log_signal_bone_stress_explainer
import app.milestone.resources.log_signal_cardiac_explainer
import app.milestone.resources.log_signal_decoupling_explainer
import app.milestone.resources.log_signal_e1rm_explainer
import app.milestone.resources.log_signal_hrv_cv_explainer
import app.milestone.resources.log_signal_hrv_explainer
import app.milestone.resources.log_signal_illness_explainer
import app.milestone.resources.log_signal_pain_explainer
import app.milestone.resources.log_signal_reds_explainer
import app.milestone.resources.log_signal_rhr_explainer
import app.milestone.resources.log_signal_rpe_explainer
import app.milestone.resources.log_signal_soreness_explainer
import app.milestone.resources.log_signal_velocity_loss_explainer
import app.milestone.resources.log_signal_wellness_explainer
import app.milestone.resources.log_spec_bar_velocity
import app.milestone.resources.log_spec_decoupling
import app.milestone.resources.log_spec_e1rm
import app.milestone.resources.log_spec_hrv
import app.milestone.resources.log_spec_hrv_cv
import app.milestone.resources.log_spec_rhr
import app.milestone.resources.log_spec_rpe
import app.milestone.resources.log_spec_soreness
import app.milestone.resources.log_spec_velocity_loss
import app.milestone.resources.log_spec_wellness
import app.milestone.resources.log_switch_add_hrv
import app.milestone.resources.log_switch_add_rhr
import app.milestone.resources.log_switch_hrv_hint
import app.milestone.resources.log_switch_rhr_hint
import app.milestone.resources.log_switch_yes_have
import app.milestone.resources.log_unit_bpm
import app.milestone.resources.log_unit_ms
import app.milestone.resources.log_value_min
import app.milestone.resources.rating_fatigue_words
import app.milestone.resources.rating_mood_words
import app.milestone.resources.rating_sleep_words
import app.milestone.resources.rating_soreness_words
import app.milestone.resources.rating_stress_words
import app.milestone.resources.rating_value_word
import kotlinx.datetime.TimeZone
import org.jetbrains.compose.resources.StringArrayResource
import org.jetbrains.compose.resources.StringResource
import org.jetbrains.compose.resources.stringArrayResource
import org.jetbrains.compose.resources.stringResource

/** The run length the decoupling reading opens on: past the core's 20-minute
 *  validity floor, so an untouched reading counts. */
private const val DecouplingDefaultMin = 30.0

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
    signalGroups: Map<ReadinessSignal, SignalGroup>,
    onClose: () -> Unit,
    onSubmit: (Event.SubmitReadiness) -> Unit,
) {
    var signal by rememberSaveable { mutableStateOf(ReadinessSignal.WellnessZ) }
    var value by rememberSaveable { mutableStateOf(defaultReadinessValue(ReadinessSignal.WellnessZ)) }
    var observedAt by rememberSaveable { mutableStateOf(AppClock.nowSec()) }
    // Effort minutes for the duration-gated AerobicDecoupling signal (valid only
    // >20 min, File 06), sent only for that signal so the core can validate the
    // reading instead of silently discarding it.
    var effortMin by rememberSaveable { mutableStateOf(DecouplingDefaultMin) }
    var dirty by rememberSaveable { mutableStateOf(false) }
    // First red-flag signal in wire order, so the fence lands before the first
    // medical flag as displayed.
    val firstRedFlag = remember(signalGroups) {
        signalGroups.entries.firstOrNull { it.value == SignalGroup.RedFlag }?.key
    }

    EditorScaffold(
        title = stringResource(Res.string.log_readiness_title),
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
                stringResource(Res.string.log_readiness_intro),
                color = OnBgMuted,
                style = Type.Caption,
            )
            // Picker order comes from the core's signal_groups metadata (its
            // summary order on the wire), so routine metrics, Soreness included,
            // render together above the medical red-flag fence.
            val ordered = remember(signalGroups) { signalGroups.keys.toList() }
            val signalLabels = ordered.associateWith { stringResource(it.labelRes) }
            EnumRow(
                stringResource(Res.string.log_field_signal),
                ordered,
                signal,
                display = { signalLabels.getValue(it) },
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
                    FieldLabel(stringResource(Res.string.log_field_value), formatFixed(value, spec.decimals))
                    ScrollableScaleRow(
                        spec.gridOptions(),
                        value,
                        { formatFixed(it, spec.decimals) },
                        label = stringResource(signal.labelRes),
                    ) {
                        value = it
                        dirty = true
                    }
                    // The unit and the point where the value starts to matter.
                    Text(stringResource(spec.hint, *spec.hintArgs.toTypedArray()), color = OnBgMuted, style = Type.Caption)
                }
                signal == ReadinessSignal.Illness -> {
                    val level = IllnessLevel.entries.lastOrNull { value >= it.value } ?: IllnessLevel.None
                    val levelLabels = IllnessLevel.entries.associateWith { stringResource(it.labelRes) }
                    // Whole-row option list, not segmented: the triage labels are too
                    // long to segment and must not be abbreviated (safety-relevant).
                    EnumRow(stringResource(Res.string.log_field_severity), IllnessLevel.entries.toList(), level, display = { levelLabels.getValue(it) }) {
                        value = it.value
                        dirty = true
                    }
                }
                else ->
                    // A yes/no answer to a red-flag symptom question (the signal and
                    // its explainer sit right above), so the switch is the affirmative.
                    LabeledSwitchRow(stringResource(Res.string.log_switch_yes_have), value > 0.0) {
                        value = if (it) 1.0 else 0.0
                        dirty = true
                    }
            }
            // AerobicDecoupling is duration-gated (valid only >20 min, File 06): the
            // core discards a reading with no duration, so the run length is sent too.
            if (signal == ReadinessSignal.AerobicDecoupling) {
                FieldLabel(stringResource(Res.string.log_field_run_duration), stringResource(Res.string.log_value_min, effortMin.toInt()))
                ScrollableScaleRow(
                    (0..DecouplingRunMaxMin step DecouplingRunStepMin).map { it.toDouble() },
                    effortMin,
                    { "${it.toInt()}" },
                    label = stringResource(Res.string.log_field_run_duration),
                ) {
                    effortMin = it
                    dirty = true
                }
                Text(
                    stringResource(Res.string.log_decoupling_hint, decouplingMinEffortLabel()),
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
 *  "Last time" hint), in the device's local calendar. */
internal fun checkinEchoIsToday(echoObservedAt: Long, nowSec: Long, zone: TimeZone): Boolean =
    localEpochDay(echoObservedAt, zone) == localEpochDay(nowSec, zone)

/**
 * Morning check-in, the primary readiness entry: the five 1–5 wellness items, two
 * optional watch numbers and the Illness item (first when [illnessPrompt]), all
 * optional. The core turns the history into z-scores; the user never enters one.
 */
@Composable
fun MorningCheckinSheet(
    // The core's most recent check-in echo, if any.
    echo: CheckinInput? = null,
    onClose: () -> Unit = {},
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
    val todayEcho = echo?.takeIf { checkinEchoIsToday(it.observed_at, AppClock.nowSec(), TimeZone.currentSystemDefault()) }
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

    // Optional watch numbers behind toggles: a check-in without a watch sends
    // neither, and the core derives nothing for that channel (no value is
    // invented). A toggle starts on when the last check-in carried that number
    // (the user has the watch), but only today's own reading fills the field;
    // an empty field sends nothing.
    var hasRhr by rememberSaveable { mutableStateOf(echo?.resting_hr_bpm != null) }
    var hasHrv by rememberSaveable { mutableStateOf(echo?.hrv_rmssd_ms != null) }
    var rhrText by rememberSaveable {
        mutableStateOf(todayEcho?.resting_hr_bpm?.let { formatFixed(it, 0) } ?: "")
    }
    var hrvText by rememberSaveable {
        mutableStateOf(todayEcho?.hrv_rmssd_ms?.let { formatFixed(it, 0) } ?: "")
    }
    var active by rememberSaveable { mutableStateOf(CheckinField.RestingHr) }
    var fresh by rememberSaveable { mutableStateOf(true) }
    // The keypad opens right under the field it types into, on a tap of that
    // field (or switching its toggle on), and folds when another item is answered.
    var keypadOpen by rememberSaveable { mutableStateOf(false) }

    val rhrParsed = rhrText.toDoubleOrNull()
    val hrvParsed = hrvText.toDoubleOrNull()
    val rhrValid = !hasRhr || rhrText.isEmpty() || (rhrParsed != null && rhrParsed in RESTING_HR_RANGE)
    val hrvValid = !hasHrv || hrvText.isEmpty() || (hrvParsed != null && hrvParsed in HrvMsRange)
    val savedMessage = stringResource(Res.string.log_checkin_saved)

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
        title = stringResource(Res.string.log_checkin_title),
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
            Text(stringResource(Res.string.log_checkin_scale_note), color = OnBgMuted, style = Type.Caption)
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
                    switchLabel = stringResource(if (rhr) Res.string.log_switch_add_rhr else Res.string.log_switch_add_hrv),
                    switchHint = stringResource(if (rhr) Res.string.log_switch_rhr_hint else Res.string.log_switch_hrv_hint),
                    shown = if (rhr) hasRhr else hasHrv,
                    onToggle = { toggle(field, it) },
                    label = stringResource(if (rhr) Res.string.log_field_resting_hr else Res.string.log_field_hrv),
                    unit = stringResource(if (rhr) Res.string.log_unit_bpm else Res.string.log_unit_ms),
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
                if (!rhr && hasHrv) Text(stringResource(Res.string.log_hrv_help), color = OnBgMuted, style = Type.Caption)
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
    value?.let { stringResource(Res.string.log_checkin_last, "$it") }

/** The five 1–5 wellness items of the morning check-in, in the core's order:
 *  [options] is the drawn order (soreness, fatigue and stress run 5→1, worse end
 *  first), [left]/[right] name the two ends and [words] holds the value words
 *  (indexed value − 1). */
private enum class CheckinItem(
    val label: StringResource,
    val options: List<Int>,
    val left: StringResource,
    val right: StringResource,
    val words: StringArrayResource,
) {
    Sleep(Res.string.log_checkin_sleep, ScaleHighGood, Res.string.log_anchor_terrible, Res.string.log_anchor_great, Res.array.rating_sleep_words),
    Soreness(Res.string.log_checkin_soreness, ScaleHighBad, Res.string.log_anchor_very_sore, Res.string.log_anchor_none, Res.array.rating_soreness_words),
    Mood(Res.string.log_checkin_mood, ScaleHighGood, Res.string.log_anchor_awful, Res.string.log_anchor_great, Res.array.rating_mood_words),
    Fatigue(Res.string.log_checkin_fatigue, ScaleHighBad, Res.string.log_anchor_exhausted, Res.string.log_anchor_fresh, Res.array.rating_fatigue_words),
    Stress(Res.string.log_checkin_stress, ScaleHighBad, Res.string.log_anchor_stressed, Res.string.log_anchor_calm, Res.array.rating_stress_words),
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
    val shown = item.options.associateWith { stringResource(Res.string.rating_value_word, it, words.getOrElse(it - 1) { "" }) }
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
        min = range.start, max = range.endInclusive, decimals = 0,
        adjustments = adjustments,
        onActivate = onActivate,
        onText = onChip,
        hint = lastValue?.let { stringResource(Res.string.log_checkin_last_unit, formatFixed(it, 0), unit) },
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
        stringResource(Res.string.log_illness_q),
        trailing = if (prompted) null else stringResource(Res.string.log_optional),
        heading = true,
    )
    Text(stringResource(Res.string.log_illness_help), color = OnBgMuted, style = Type.Caption)
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
 * z-scores or percents. [hintArgs] fill the hint's figures from the core's
 * thresholds ([CoreConstants]), so the caption never restates a rule's number.
 */
private data class ValueSpec(
    val min: Double,
    val max: Double,
    val step: Double,
    val default: Double,
    // Unit + threshold caption.
    val hint: StringResource,
    val decimals: Int = 1,
    val hintArgs: List<String> = emptyList(),
)

/** A signed threshold figure, [trimDecimal]-trimmed: "+5", "−1.5" (a real minus sign). */
private fun signedFigure(v: Double): String = when {
    v > 0 -> "+" + trimDecimal(v)
    v < 0 -> "−" + trimDecimal(-v)
    else -> trimDecimal(v)
}

/**
 * The spec's `min + n·step` grid as a picklist for [ScrollableScaleRow]. Each
 * point is round-tripped through the spec's own display format so the stored
 * double is EXACTLY what the row shows (no binary-step drift) and the current
 * value, always produced by this same grid or the spec default, matches an
 * option by equality.
 */
private fun ValueSpec.gridOptions(): List<Double> {
    val n = ((max - min) / step).roundOrZero().toInt()
    return (0..n).map { formatFixed(min + it * step, decimals).toDouble() }
}

/**
 * The picker's view of one signal ([labelRes] is its label): a one-line explainer (what the
 * signal IS and, for the medical flags, the core-documented consequence; no
 * shell-side thresholds), and the value scale of a continuous signal. [spec] is
 * null for Illness (a severity picker) and for the flags the core reads as
 * present at `value > 0` (Pain, RedS, CardiacRedFlag, BoneStress): those get an
 * on/off switch, since a scale's 0.0 default would record the flag's absence.
 */
private class SignalUi(val explainer: StringResource, val spec: ValueSpec? = null)

/** One exhaustive `when`, so a new [ReadinessSignal] fails to compile until it
 *  has an explainer and a scale decision. Display-only: the wire value
 *  is still `signal.name` (see [Event.SubmitReadiness]). */
private val SignalUis: Map<ReadinessSignal, SignalUi> = ReadinessSignal.entries.associateWith { signal ->
    when (signal) {
        // Signed RPE delta (actual − target); RPE_DELTA_ABOVE / _WELL_ABOVE gate load adjustments.
        ReadinessSignal.Rpe -> SignalUi(
            Res.string.log_signal_rpe_explainer,
            ValueSpec(
                -4.0, 4.0, 0.5, 0.0, Res.string.log_spec_rpe,
                hintArgs = listOf(trimDecimal(CoreConstants.RPE_DELTA_ABOVE), trimDecimal(CoreConstants.RPE_DELTA_WELL_ABOVE)),
            ),
        )
        // e1RM today ÷ baseline; E1RM_REDUCE_RATIO / E1RM_INCREASE_RATIO gate cap/add-load.
        ReadinessSignal.EstimatedOneRm -> SignalUi(
            Res.string.log_signal_e1rm_explainer,
            ValueSpec(
                0.70, 1.15, 0.01, 1.0, Res.string.log_spec_e1rm, 2,
                hintArgs = listOf(formatFixed(CoreConstants.E1RM_REDUCE_RATIO, 2), formatFixed(CoreConstants.E1RM_INCREASE_RATIO, 2)),
            ),
        )
        // Mean concentric velocity, m/s (dormant, no autoreg gate yet).
        ReadinessSignal.BarVelocity -> SignalUi(
            Res.string.log_signal_bar_velocity_explainer,
            ValueSpec(0.0, 2.0, 0.05, 0.5, Res.string.log_spec_bar_velocity, 2),
        )
        // Within-set velocity-loss %; the goal's VL_STOP_*_FRAC terminates the set.
        ReadinessSignal.VelocityLoss -> SignalUi(
            Res.string.log_signal_velocity_loss_explainer,
            ValueSpec(
                0.0, 50.0, 5.0, 0.0, Res.string.log_spec_velocity_loss, 0,
                hintArgs = listOf(
                    trimDecimal(CoreConstants.VL_STOP_POWER_FRAC * 100.0),
                    trimDecimal(CoreConstants.VL_STOP_HYPERTROPHY_FRAC * 100.0),
                ),
            ),
        )
        // Composite wellness z-score; ≤ WELLNESS_SINGLE_DAY_Z downgrades.
        ReadinessSignal.WellnessZ -> SignalUi(
            Res.string.log_signal_wellness_explainer,
            ValueSpec(
                -3.0, 3.0, 0.5, 0.0, Res.string.log_spec_wellness,
                hintArgs = listOf(signedFigure(CoreConstants.WELLNESS_SINGLE_DAY_Z)),
            ),
        )
        // lnRMSSD z vs rolling baseline; < HRV_SWC_LOWER_Z downgrades.
        ReadinessSignal.HrvLnRmssd -> SignalUi(
            Res.string.log_signal_hrv_explainer,
            ValueSpec(
                -3.0, 3.0, 0.5, 0.0, Res.string.log_spec_hrv,
                hintArgs = listOf(signedFigure(CoreConstants.HRV_SWC_LOWER_Z)),
            ),
        )
        // HRV coefficient of variation % (dormant, no autoreg gate yet).
        ReadinessSignal.HrvCv -> SignalUi(
            Res.string.log_signal_hrv_cv_explainer,
            ValueSpec(0.0, 15.0, 0.5, 0.0, Res.string.log_spec_hrv_cv),
        )
        // Aerobic-decoupling drift %; DECOUPLING_HIGH_PCT keeps the run easy.
        ReadinessSignal.AerobicDecoupling -> SignalUi(
            Res.string.log_signal_decoupling_explainer,
            ValueSpec(
                0.0, 30.0, 1.0, 0.0, Res.string.log_spec_decoupling, 0,
                hintArgs = listOf(trimDecimal(CoreConstants.DECOUPLING_HIGH_PCT)),
            ),
        )
        // RHR delta vs baseline, bpm; RHR_DOWNGRADE_FLOOR_BPM for RHR_DOWNGRADE_STREAK
        // days downgrades, RHR_STOP_BPM stops. The label says "vs normal": this field
        // submits a delta, not the absolute bpm the morning check-in takes.
        ReadinessSignal.RestingHr -> SignalUi(
            Res.string.log_signal_rhr_explainer,
            ValueSpec(
                -5.0, 20.0, 1.0, 0.0, Res.string.log_spec_rhr, 0,
                hintArgs = listOf(
                    signedFigure(CoreConstants.RHR_DOWNGRADE_FLOOR_BPM),
                    CoreConstants.RHR_DOWNGRADE_STREAK.toString(),
                    signedFigure(CoreConstants.RHR_STOP_BPM),
                ),
            ),
        )
        // Soreness on the 1–7 Hooper scale; ≥ SORENESS_HIGH downgrades intensity one level.
        ReadinessSignal.Soreness -> SignalUi(
            Res.string.log_signal_soreness_explainer,
            ValueSpec(
                1.0, 7.0, 1.0, 3.0, Res.string.log_spec_soreness, 0,
                hintArgs = listOf(trimDecimal(CoreConstants.SORENESS_HIGH)),
            ),
        )
        ReadinessSignal.Pain -> SignalUi(Res.string.log_signal_pain_explainer)
        ReadinessSignal.Illness -> SignalUi(Res.string.log_signal_illness_explainer)
        ReadinessSignal.RedS -> SignalUi(Res.string.log_signal_reds_explainer)
        ReadinessSignal.CardiacRedFlag -> SignalUi(Res.string.log_signal_cardiac_explainer)
        ReadinessSignal.BoneStress -> SignalUi(Res.string.log_signal_bone_stress_explainer)
    }
}

private val ReadinessSignal.ui: SignalUi get() = SignalUis.getValue(this)

/** The signal's value-scale hint and the core thresholds that fill it (null
 *  for a signal without a scale). */
internal val ReadinessSignal.valueHint: Pair<StringResource, List<String>>?
    get() = ui.spec?.let { it.hint to it.hintArgs }

/** The shortest steady effort whose decoupling counts, in minutes. */
internal fun decouplingMinEffortLabel(): String = trimDecimal(CoreConstants.DECOUPLING_MIN_EFFORT_MIN)

/**
 * The Illness answer (AUTOREG-4, ILLNESS-RTT-001). [value] is the wire value the
 * core decodes (schema.rs `IllnessSeverity::from_value`: ≥2 below-neck/fever, ≥1
 * above-neck, else none); [labelRes] is the raw-signal editor's short label,
 * [answerRes] the check-in's longer answer.
 */
private enum class IllnessLevel(val value: Double, val labelRes: StringResource, val answerRes: StringResource) {
    None(0.0, Res.string.log_illness_level_none, Res.string.log_illness_level_none),
    AboveNeck(1.0, Res.string.log_illness_level_above, Res.string.log_illness_above_neck),
    BelowNeckOrFever(2.0, Res.string.log_illness_level_below, Res.string.log_illness_below_neck),
}
