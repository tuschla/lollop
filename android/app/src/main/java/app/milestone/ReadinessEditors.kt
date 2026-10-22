package app.milestone

import androidx.annotation.StringRes
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.relocation.BringIntoViewRequester
import androidx.compose.foundation.relocation.bringIntoViewRequester
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringArrayResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import java.util.Locale

/** The run length the decoupling reading opens on: past the core's 20-minute
 *  validity floor, so an untouched reading counts. */
private const val DecouplingDefaultMin = 30.0

/** The decoupling run-length scale: 0–180 min in 5-minute steps. */
private const val DecouplingRunMaxMin = 180
private const val DecouplingRunStepMin = 5

/** The morning check-in's watch numbers: a plausible resting HR (bpm) and HRV
 *  rMSSD (ms) a user can type. */
private val RestingHrBpmRange = 25.0..120.0
private val HrvMsRange = 1.0..300.0

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
    val firstRedFlag = remember(signalGroups) {
        // First red-flag signal in wire order (matches the picker's ordering),
        // so the fence lands before the first medical flag as displayed.
        signalGroups.entries.firstOrNull { it.value == "red_flag" }
            ?.let { e -> ReadinessSignal.entries.firstOrNull { it.name == e.key } }
            ?: ReadinessSignal.Pain
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
        ReadinessEditorBody(
            signal = signal,
            value = value,
            observedAt = observedAt,
            effortMin = effortMin,
            firstRedFlag = firstRedFlag,
            signalGroups = signalGroups,
            onSignal = { signal = it; value = defaultReadinessValue(it); dirty = true },
            onValue = { value = it; dirty = true },
            onEffortMin = { effortMin = it; dirty = true },
            onObservedAt = { observedAt = it; dirty = true },
        )
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
 * Morning check-in: the PRIMARY readiness entry point. The user answers any of
 * the five wellness scales (sleep / soreness / mood / fatigue / stress, the KB
 * 5-item short-form) on a 1–5 scale with word anchors, plus two optional watch
 * numbers (resting HR, HRV rMSSD); every item is optional and Save opens with the
 * first answer. The CORE normalizes the retained history into
 * z-scores/deltas/streaks; the user never enters a z-score. One Submit → one
 * [Event.SubmitCheckin] (none when only the illness item was answered), plus an Illness
 * [Event.SubmitReadiness] when the "Feeling ill?" item is answered (asked first
 * when the core raises [illnessPrompt]). The raw-signal editor stays reachable
 * from the Log chooser's "More" group.
 */
@Composable
fun MorningCheckinSheet(
    // The core's most recent check-in echo, if any.
    echo: CheckinEchoView? = null,
    onClose: () -> Unit = {},
    // Mirror unsaved-edits up to a host so its swipe-down dismissal can guard it.
    onDirtyChange: (Boolean) -> Unit = {},
    // The core asks for the illness check (an active illness report, or a high
    // resting-HR morning): the "Feeling ill?" item moves to the top.
    illnessPrompt: Boolean = false,
    // The check-in (null when only the illness question was answered), and the
    // illness answer when one was given (value 0 none, 1 above the neck, 2 below
    // the neck or fever).
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
    var illness by rememberSaveable { mutableStateOf<Int?>(null) }
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
    val rhrValid = !hasRhr || rhrText.isEmpty() || (rhrParsed != null && rhrParsed in RestingHrBpmRange)
    val hrvValid = !hasHrv || hrvText.isEmpty() || (hrvParsed != null && hrvParsed in HrvMsRange)
    val savedMessage = stringResource(R.string.log_checkin_saved)
    val bpm = stringResource(R.string.log_unit_bpm)
    val ms = stringResource(R.string.log_unit_ms)

    fun buffer() = if (active == CheckinField.RestingHr) rhrText else hrvText
    fun setBuffer(v: String) = if (active == CheckinField.RestingHr) rhrText = v else hrvText = v
    // Like the set keypad: the open keypad scrolls fully into view (its rows sat
    // under the fade and Save), and its Done key folds it away.
    val keypadRequester = remember { BringIntoViewRequester() }
    LaunchedEffect(keypadOpen, active) {
        if (keypadOpen) {
            withFrameNanos { }
            keypadRequester.bringIntoView()
        }
    }
    val keypad: @Composable () -> Unit = {
        Box(Modifier.bringIntoViewRequester(keypadRequester)) {
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
                // Resting HR and rMSSD are whole numbers.
                allowDecimal = false,
                onDone = { keypadOpen = false },
            )
        }
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
            val ill = illness?.let { Event.SubmitReadiness(ReadinessSignal.Illness, it.toDouble(), observedAt) }
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
            CheckinScaleRow(
                "sleep",
                stringResource(R.string.log_checkin_sleep),
                ScaleHighGood,
                stringResource(R.string.log_anchor_terrible),
                stringResource(R.string.log_anchor_great),
                stringArrayResource(R.array.rating_sleep_words),
                sleep,
                lastTimeHint(pastEcho?.sleep_quality),
            ) { v -> answer { sleep = v } }
            CheckinScaleRow(
                "soreness",
                stringResource(R.string.log_checkin_soreness),
                ScaleHighBad,
                stringResource(R.string.log_anchor_very_sore),
                stringResource(R.string.log_anchor_none),
                stringArrayResource(R.array.rating_soreness_words),
                soreness,
                lastTimeHint(pastEcho?.soreness),
            ) { v -> answer { soreness = v } }
            CheckinScaleRow(
                "mood",
                stringResource(R.string.log_checkin_mood),
                ScaleHighGood,
                stringResource(R.string.log_anchor_awful),
                stringResource(R.string.log_anchor_great),
                stringArrayResource(R.array.rating_mood_words),
                mood,
                lastTimeHint(pastEcho?.mood),
            ) { v -> answer { mood = v } }
            CheckinScaleRow(
                "fatigue",
                stringResource(R.string.log_checkin_fatigue),
                ScaleHighBad,
                stringResource(R.string.log_anchor_exhausted),
                stringResource(R.string.log_anchor_fresh),
                stringArrayResource(R.array.rating_fatigue_words),
                fatigue,
                lastTimeHint(pastEcho?.fatigue),
            ) { v -> answer { fatigue = v } }
            CheckinScaleRow(
                "stress",
                stringResource(R.string.log_checkin_stress),
                ScaleHighBad,
                stringResource(R.string.log_anchor_stressed),
                stringResource(R.string.log_anchor_calm),
                stringArrayResource(R.array.rating_stress_words),
                stress,
                lastTimeHint(pastEcho?.stress),
            ) { v -> answer { stress = v } }

            SwitchRow(stringResource(R.string.log_switch_add_rhr), hasRhr, stringResource(R.string.log_switch_rhr_hint)) {
                hasRhr = it; dirty = true
                // Toggle-off must also re-route the shared keypad: leaving
                // `active` on the now-hidden field would silently type into it
                // while the visible one never updates.
                if (it) {
                    active = CheckinField.RestingHr; fresh = true; keypadOpen = true
                } else if (active == CheckinField.RestingHr) {
                    keypadOpen = false
                    if (hasHrv) {
                        active = CheckinField.Hrv; fresh = true
                    }
                }
            }
            if (hasRhr) {
                KeypadValueField(
                    stringResource(R.string.log_field_resting_hr), bpm, rhrText,
                    active = keypadOpen && active == CheckinField.RestingHr,
                    // Red only once something out-of-range is typed: an empty
                    // field isn't an error, it sends nothing.
                    invalid = rhrText.isNotEmpty() && !rhrValid,
                    min = RestingHrBpmRange.start, max = RestingHrBpmRange.endInclusive, format = "%.0f",
                    adjustments = listOf(-1.0, 1.0),
                    onActivate = { active = CheckinField.RestingHr; fresh = true; keypadOpen = true },
                    // A ± chip sets a whole value; it needs no keypad, so it folds.
                    onText = { rhrText = it; fresh = false; keypadOpen = false; dirty = true },
                    hint = pastEcho?.resting_hr_bpm?.let {
                        stringResource(R.string.log_checkin_last_unit, String.format(Locale.US, "%.0f", it), bpm)
                    },
                )
                if (keypadOpen && active == CheckinField.RestingHr) keypad()
            }
            SwitchRow(stringResource(R.string.log_switch_add_hrv), hasHrv, stringResource(R.string.log_switch_hrv_hint)) {
                hasHrv = it; dirty = true
                if (it) {
                    active = CheckinField.Hrv; fresh = true; keypadOpen = true
                } else if (active == CheckinField.Hrv) {
                    keypadOpen = false
                    if (hasRhr) {
                        active = CheckinField.RestingHr; fresh = true
                    }
                }
            }
            if (hasHrv) {
                KeypadValueField(
                    stringResource(R.string.log_field_hrv), ms, hrvText,
                    active = keypadOpen && active == CheckinField.Hrv,
                    invalid = hrvText.isNotEmpty() && !hrvValid,
                    min = HrvMsRange.start, max = HrvMsRange.endInclusive, format = "%.0f",
                    adjustments = listOf(-1.0, 1.0, 5.0),
                    onActivate = { active = CheckinField.Hrv; fresh = true; keypadOpen = true },
                    onText = { hrvText = it; fresh = false; keypadOpen = false; dirty = true },
                    hint = pastEcho?.hrv_rmssd_ms?.let {
                        stringResource(R.string.log_checkin_last_unit, String.format(Locale.US, "%.0f", it), ms)
                    },
                )
                if (keypadOpen && active == CheckinField.Hrv) keypad()
                // AUTOREG-6: the HRV baseline only means something from one source.
                Text(stringResource(R.string.log_hrv_help), color = OnBgMuted, style = Type.Caption)
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

/**
 * One 1–5 wellness row for the morning check-in: a [RatingScale] in [options]
 * order (the core's own direction, so soreness/fatigue/stress run 5→1 with the
 * worse end on the left) with both ends named under it ([leftWord], [rightWord])
 * and the picked value's word from [words] (indexed value − 1) at the label's end.
 * [hint] is an earlier answer shown in the word slot while unanswered, never a
 * selection. The stored value is the number the core reads
 * (schema.rs::CheckinInput), whatever side it is drawn on. [key] is the stable,
 * untranslated item name its test tag "checkin-<key>" is built from.
 */
@Composable
private fun CheckinScaleRow(
    key: String,
    label: String,
    options: List<Int>,
    leftWord: String,
    rightWord: String,
    words: Array<String>,
    value: Int?,
    hint: String?,
    onValue: (Int) -> Unit,
) {
    val shown = options.associateWith { stringResource(R.string.rating_value_word, it, words.getOrElse(it - 1) { "" }) }
    RatingScale(
        label = label,
        values = options,
        selected = value,
        onSelect = { picked -> picked?.let(onValue) },
        startCaption = leftWord,
        endCaption = rightWord,
        selectedText = { shown.getValue(it) },
        // FormCard spaces fields by Space.Md; the extra bottom padding makes the
        // gap to the next field Space.Lg.
        modifier = Modifier.padding(bottom = (Space.Lg - Space.Md).dp),
        clearable = false,
        testTag = "checkin-$key",
        hint = hint,
    )
}

/**
 * The check-in's "Feeling ill?" item (AUTOREG-4, ILLNESS-RTT-001): fine / head
 * cold only / flu, fever or body aches, the three levels the core's illness
 * gate reads (value 0 / 1 above the neck / 2 below the neck or fever). The
 * question is a [FieldLabel] like every other question (a TalkBack heading); a
 * helper line says what each answer means for training; each answer is its own
 * full-width choice cell (the scale cells' look), so the three read as choices,
 * not a paragraph. Optional unless the core asks first ([prompted]).
 */
@Composable
private fun IllnessQuestion(value: Int?, prompted: Boolean, onValue: (Int) -> Unit) {
    val labels = listOf(
        stringResource(R.string.log_illness_none),
        stringResource(R.string.log_illness_above_neck),
        stringResource(R.string.log_illness_below_neck),
    )
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
        labels.forEachIndexed { i, label ->
            val selected = value == i
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = ActionMinHeight)
                    .choiceCell(selected)
                    .selectable(selected = selected, role = Role.RadioButton) { onValue(i) }
                    .padding(horizontal = Space.Card.dp, vertical = Space.Md.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    label,
                    color = choiceCellContent(selected),
                    style = if (selected) Type.Body.copy(fontWeight = FontWeight.Bold) else Type.Body,
                )
            }
        }
    }
}

@Composable
private fun ReadinessEditorBody(
    signal: ReadinessSignal,
    value: Double,
    observedAt: Long,
    effortMin: Double,
    firstRedFlag: ReadinessSignal,
    signalGroups: Map<String, String>,
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
            stringResource(R.string.log_readiness_intro),
            color = OnBgMuted,
            style = Type.Caption,
        )
        // Picker order comes from the core's signal_groups metadata (ALL_SIGNALS
        // order on the wire), so routine metrics — Soreness included — render
        // together above the medical red-flag fence. Empty map (old core) falls
        // back to the enum's declaration order.
        val ordered = remember(signalGroups) {
            signalGroups.keys
                .mapNotNull { name -> ReadinessSignal.entries.firstOrNull { it.name == name } }
                .ifEmpty { ReadinessSignal.entries.toList() }
        }
        // Reset the value to the newly-selected signal's sensible default: the core
        // reads a flag signal as "present" only when value > 0 (Pain/RED-S/cardiac/
        // bone-stress) and an Illness by severity band, so carrying over a 0.0 or a
        // z-score would silently submit a *non-triggering* red flag, the opposite
        // of what a user picking "Pain" intends.
        val signalLabels = ordered.associateWith { stringResource(it.labelRes) }
        EnumRow(
            stringResource(R.string.log_field_signal),
            ordered,
            signal,
            display = { signalLabels.getValue(it) },
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
        Text(stringResource(signal.explainerRes), color = OnBgMuted, style = Type.Caption)
        when {
            signal.isBinaryFlag ->
                // A bare "Present" reads as a neutral status; this is a yes/no answer to
                // a red-flag symptom question (the signal + its explainer sit right
                // above), so label the switch as the affirmative it is.
                SwitchRow(stringResource(R.string.log_switch_yes_have), value > 0.0) { onValue(if (it) 1.0 else 0.0) }
            signal == ReadinessSignal.Illness -> {
                val level = IllnessLevel.entries.lastOrNull { value >= it.value } ?: IllnessLevel.None
                val levelLabels = IllnessLevel.entries.associateWith { stringResource(it.labelRes) }
                // Whole-row option list, not segmented: the triage labels ("Above-neck
                // (no fever)", "Below-neck / fever") are too long to segment, and must
                // not be abbreviated: the distinction is safety-relevant.
                EnumRow(stringResource(R.string.log_field_severity), IllnessLevel.entries.toList(), level, display = { levelLabels.getValue(it) }) {
                    onValue(it.value)
                }
            }
            else -> {
                // Tap-scale primitive, not a −/+ stepper: one tap picks a value,
                // the whole value scale is visible and one tap picks any point
                // on the signal's own grid, negatives (z-scores, deltas)
                // included, which the numeric keypad can't express.
                val spec = continuousSpec(signal)
                FieldLabel(stringResource(R.string.log_field_value), String.format(Locale.US, spec.format, value))
                ScrollableScaleRow(
                    spec.gridOptions(),
                    value,
                    { String.format(Locale.US, spec.format, it) },
                    label = stringResource(signal.labelRes),
                ) { onValue(it) }
                // The bare "Value" scale gives no clue what unit a z-score / bpm
                // delta / ratio wants; the core interprets each signal differently,
                // so spell out the unit and the point where it starts to matter.
                spec.hint?.let { Text(stringResource(it), color = OnBgMuted, style = Type.Caption) }
            }
        }
        // AerobicDecoupling is duration-gated (valid only >20 min, File 06): the
        // core discards a reading with no duration, so capture the run length here
        // and send it, rather than submitting a reading the engine silently drops.
        if (signal == ReadinessSignal.AerobicDecoupling) {
            // Tap-scale over 0–180 min (5-min steps), not a −/+ stepper: one tap
            // picks the run length the decoupling reading needs.
            FieldLabel(stringResource(R.string.log_field_run_duration), stringResource(R.string.log_value_min, effortMin.toInt()))
            ScrollableScaleRow(
                (0..DecouplingRunMaxMin step DecouplingRunStepMin).map { it.toDouble() },
                effortMin,
                { "${it.toInt()}" },
                label = stringResource(R.string.log_field_run_duration),
            ) { onEffortMin(it) }
            Text(
                stringResource(R.string.log_decoupling_hint),
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
 * On the threshold numbers in the hints: they are not a second copy of any
 * core verdict. They cite autoreg.rs *input* thresholds that the core does not
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
    // Unit + threshold caption; null shows none.
    @StringRes val hint: Int?,
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
        ValueSpec(-4.0, 4.0, 0.5, 0.0, R.string.log_spec_rpe)
    // e1RM today ÷ baseline; <0.90/<0.95/>1.05 gate deload/cap/add-load.
    ReadinessSignal.EstimatedOneRm ->
        ValueSpec(0.70, 1.15, 0.01, 1.0, R.string.log_spec_e1rm, "%.2f")
    // Mean concentric velocity, m/s (dormant, no autoreg gate yet).
    ReadinessSignal.BarVelocity ->
        ValueSpec(0.0, 2.0, 0.05, 0.5, R.string.log_spec_bar_velocity, "%.2f")
    // Within-set velocity-loss %; ≥20 terminates the set.
    ReadinessSignal.VelocityLoss ->
        ValueSpec(0.0, 50.0, 5.0, 0.0, R.string.log_spec_velocity_loss, "%.0f")
    // Composite wellness z-score; ≤ −1.5 downgrades.
    ReadinessSignal.WellnessZ ->
        ValueSpec(-3.0, 3.0, 0.5, 0.0, R.string.log_spec_wellness)
    // lnRMSSD z vs rolling baseline; < −0.5 downgrades.
    ReadinessSignal.HrvLnRmssd ->
        ValueSpec(-3.0, 3.0, 0.5, 0.0, R.string.log_spec_hrv)
    // HRV coefficient of variation % (dormant, no autoreg gate yet).
    ReadinessSignal.HrvCv ->
        ValueSpec(0.0, 15.0, 0.5, 0.0, R.string.log_spec_hrv_cv)
    // Aerobic-decoupling drift %; >10 keeps the run easy.
    ReadinessSignal.AerobicDecoupling ->
        ValueSpec(0.0, 30.0, 1.0, 0.0, R.string.log_spec_decoupling, "%.0f")
    // RHR delta vs baseline, bpm; 5–9 downgrades, ≥10 stops.
    ReadinessSignal.RestingHr ->
        ValueSpec(-5.0, 20.0, 1.0, 0.0, R.string.log_spec_rhr, "%.0f")
    // Soreness on the 1–7 Hooper scale; ≥6 downgrades intensity one level.
    ReadinessSignal.Soreness ->
        ValueSpec(1.0, 7.0, 1.0, 3.0, R.string.log_spec_soreness, "%.0f")
    // Flags/Illness never reach continuousSpec; keep the branch total with a
    // neutral fallback rather than crashing if a new signal is added upstream.
    else -> ValueSpec(-5.0, 40.0, 0.5, 0.0, null)
}

/**
 * Shell-side severity picker for the Illness signal. The wire value is still the
 * numeric `value` the core decodes (schema.rs `IllnessSeverity::from_value`:
 * ≥2 below-neck/fever, ≥1 above-neck, else none), this enum only drives the UI.
 */
private enum class IllnessLevel(val value: Double, @StringRes val labelRes: Int) {
    None(0.0, R.string.log_illness_level_none),
    AboveNeck(1.0, R.string.log_illness_level_above),
    BelowNeckOrFever(2.0, R.string.log_illness_level_below),
}

/**
 * Label for the readiness signal picker. Display-only, the wire value is still
 * `signal.name` (see [Event.SubmitReadiness]). Exhaustive `when` (no `else`) so
 * a new [ReadinessSignal] variant fails to compile until it is given a label
 * rather than silently showing a cryptic raw name.
 */
@get:StringRes
internal val ReadinessSignal.labelRes: Int
    get() = when (this) {
        ReadinessSignal.Rpe -> R.string.log_signal_rpe
        ReadinessSignal.EstimatedOneRm -> R.string.log_signal_e1rm
        ReadinessSignal.BarVelocity -> R.string.log_signal_bar_velocity
        ReadinessSignal.VelocityLoss -> R.string.log_signal_velocity_loss
        ReadinessSignal.WellnessZ -> R.string.log_signal_wellness
        ReadinessSignal.HrvLnRmssd -> R.string.log_signal_hrv
        ReadinessSignal.HrvCv -> R.string.log_signal_hrv_cv
        ReadinessSignal.AerobicDecoupling -> R.string.log_signal_decoupling
        // This raw-signal field submits a DELTA versus baseline (the core reads it
        // as a bpm delta), NOT an absolute bpm, so the label says "vs normal" to
        // match its hint and explainer. Absolute resting HR is entered in the
        // morning check-in instead.
        ReadinessSignal.RestingHr -> R.string.log_signal_rhr
        ReadinessSignal.Soreness -> R.string.log_signal_soreness
        ReadinessSignal.Pain -> R.string.log_signal_pain
        ReadinessSignal.Illness -> R.string.log_signal_illness
        ReadinessSignal.RedS -> R.string.log_signal_reds
        ReadinessSignal.CardiacRedFlag -> R.string.log_signal_cardiac
        ReadinessSignal.BoneStress -> R.string.log_signal_bone_stress
    }

/**
 * One-line neutral explainer for the readiness picker: says what the signal IS
 * (and, for the medical flags, the core-documented consequence of reporting
 * it). Deliberately no shell-side thresholds: the numbers that trigger
 * adjustments live only in the core (see the ValueSpec note above).
 */
@get:StringRes
private val ReadinessSignal.explainerRes: Int
    get() = when (this) {
        ReadinessSignal.Rpe -> R.string.log_signal_rpe_explainer
        ReadinessSignal.EstimatedOneRm -> R.string.log_signal_e1rm_explainer
        ReadinessSignal.BarVelocity -> R.string.log_signal_bar_velocity_explainer
        ReadinessSignal.VelocityLoss -> R.string.log_signal_velocity_loss_explainer
        ReadinessSignal.WellnessZ -> R.string.log_signal_wellness_explainer
        ReadinessSignal.HrvLnRmssd -> R.string.log_signal_hrv_explainer
        ReadinessSignal.HrvCv -> R.string.log_signal_hrv_cv_explainer
        ReadinessSignal.AerobicDecoupling -> R.string.log_signal_decoupling_explainer
        ReadinessSignal.RestingHr -> R.string.log_signal_rhr_explainer
        ReadinessSignal.Soreness -> R.string.log_signal_soreness_explainer
        ReadinessSignal.Pain -> R.string.log_signal_pain_explainer
        ReadinessSignal.Illness -> R.string.log_signal_illness_explainer
        ReadinessSignal.RedS -> R.string.log_signal_reds_explainer
        ReadinessSignal.CardiacRedFlag -> R.string.log_signal_cardiac_explainer
        ReadinessSignal.BoneStress -> R.string.log_signal_bone_stress_explainer
    }
