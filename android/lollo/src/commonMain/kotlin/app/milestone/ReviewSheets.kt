package app.milestone

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
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
import app.milestone.resources.log_clinical_bone_ctx
import app.milestone.resources.log_clinical_bone_q
import app.milestone.resources.log_clinical_compulsive_ctx
import app.milestone.resources.log_clinical_compulsive_q
import app.milestone.resources.log_clinical_reds_ctx
import app.milestone.resources.log_clinical_reds_q_any
import app.milestone.resources.log_clinical_reds_q_female
import app.milestone.resources.log_clinical_reds_q_male
import app.milestone.resources.log_field_decoupling_drift
import app.milestone.resources.log_field_second_half_slowdown
import app.milestone.resources.log_field_time_above_z2
import app.milestone.resources.log_no
import app.milestone.resources.log_review_intro
import app.milestone.resources.log_review_run_header
import app.milestone.resources.log_review_title
import app.milestone.resources.log_switch_cool_steady
import app.milestone.resources.log_switch_decoupling_recorded
import app.milestone.resources.log_switch_easy_intensity
import app.milestone.resources.log_switch_off_day
import app.milestone.resources.log_switch_positive_split
import app.milestone.resources.log_unit_m_per_s
import app.milestone.resources.log_value_pct
import app.milestone.resources.log_weekly_clear
import app.milestone.resources.log_weekly_clear_body
import app.milestone.resources.log_weekly_clear_confirm
import app.milestone.resources.log_weekly_clear_title
import app.milestone.resources.log_weekly_clinical_header
import app.milestone.resources.log_weekly_failed_sessions
import app.milestone.resources.log_weekly_more_details
import app.milestone.resources.log_weekly_nothing
import app.milestone.resources.log_weekly_rpe_gap
import app.milestone.resources.log_weekly_saved
import app.milestone.resources.log_weekly_sign_mood
import app.milestone.resources.log_weekly_sign_performance
import app.milestone.resources.log_weekly_sign_rhr
import app.milestone.resources.log_weekly_sign_sleep
import app.milestone.resources.log_weekly_signs_1to2
import app.milestone.resources.log_weekly_signs_ctx
import app.milestone.resources.log_weekly_signs_gt2
import app.milestone.resources.log_weekly_signs_how_long
import app.milestone.resources.log_weekly_signs_lt1
import app.milestone.resources.log_weekly_signs_q
import app.milestone.resources.log_weekly_title
import app.milestone.resources.log_weekly_velocity_drop
import app.milestone.resources.log_yes
import app.milestone.resources.rating_sessions
import kotlinx.datetime.TimeZone
import org.jetbrains.compose.resources.StringResource
import org.jetbrains.compose.resources.pluralStringResource
import org.jetbrains.compose.resources.stringResource

private val HalfPercentScale = (0..60).map { it * 0.5 }
private val FivePercentScale = (0..100 step 5).toList()
private val WeekSessionCounts = (0..7).toList()
/** The bar-speed drop (m/s) a velocity device can plausibly report. */
private val VelocityDropRange = 0.0..0.30

private const val DriftSeedPct = 4.0

/**
 * Post-session review, kept small: the coach reads how each lift went from the
 * logged sets themselves, so this only asks "Off day" plus, when today has a run
 * ([showRun]), the run details a hand-logged run (no GPS track) can't carry:
 * heart-rate drift, time above the easy band, and a second-half slowdown. Emits
 * [Event.SubmitReview] with only the session fields set; the weekly and clinical
 * questions live in [WeeklyCheckinSheet].
 */
@Composable
fun SessionReviewSheet(
    onClose: () -> Unit = {},
    showRun: Boolean,
    onSubmit: (Event.SubmitReview) -> Unit,
) {
    var badDay by rememberSaveable { mutableStateOf(false) }
    var hasDecoupling by rememberSaveable { mutableStateOf(false) }
    var driftPct by rememberSaveable { mutableStateOf(DriftSeedPct) }
    var coolSteady by rememberSaveable { mutableStateOf(true) }
    var hasEasyIntensity by rememberSaveable { mutableStateOf(false) }
    var easyPctAboveZ1 by rememberSaveable { mutableStateOf(0) }
    // A tracked run derives its split in the core; this fills the gap for a
    // hand-logged one.
    var hasPositiveSplit by rememberSaveable { mutableStateOf(false) }
    var positiveSplitPct by rememberSaveable { mutableStateOf(0.0) }
    val openedAt = rememberSaveable { AppClock.nowSec() }
    var observedAt by rememberSaveable { mutableStateOf(openedAt) }
    val dateEdited = localDayStartSec(observedAt) != localDayStartSec(openedAt)
    val answered = badDay || (showRun && (hasDecoupling || hasEasyIntensity || hasPositiveSplit))
    val dirty = answered || dateEdited

    EditorScaffold(
        title = stringResource(Res.string.log_review_title),
        dirty = dirty,
        // Nothing answered → nothing to save (no fabricated "fine" review).
        saveEnabled = answered,
        onClose = onClose,
        onSave = {
            onSubmit(
                Event.SubmitReview(
                    decoupling = if (showRun && hasDecoupling) Decouple(driftPct = driftPct, coolSteadyContext = coolSteady) else null,
                    // The core reads this as a fraction of the run's duration;
                    // the UI collects a friendlier whole percent.
                    easyFracTimeAboveVt1 = if (showRun && hasEasyIntensity) easyPctAboveZ1 / 100.0 else null,
                    positiveSplitPct = if (showRun && hasPositiveSplit) positiveSplitPct else null,
                    badDay = badDay,
                    observedAt = observedAt,
                )
            )
        },
    ) {
        FormCard {
            Text(
                stringResource(Res.string.log_review_intro),
                color = OnBgMuted,
                style = Type.Caption,
            )
            LabeledSwitchRow(stringResource(Res.string.log_switch_off_day), badDay) { badDay = it }
        }
        if (showRun) FormCard {
            Text(stringResource(Res.string.log_review_run_header), color = OnBgBody, style = Type.Body)
            LabeledSwitchRow(stringResource(Res.string.log_switch_decoupling_recorded), hasDecoupling) { hasDecoupling = it }
            if (hasDecoupling) {
                HalfPercentField(Res.string.log_field_decoupling_drift, driftPct) { driftPct = it }
                // The core only judges drift on a cool, steady, easy effort; this
                // switch records whether the run was one.
                LabeledSwitchRow(stringResource(Res.string.log_switch_cool_steady), coolSteady) { coolSteady = it }
            }
            LabeledSwitchRow(stringResource(Res.string.log_switch_easy_intensity), hasEasyIntensity) { hasEasyIntensity = it }
            if (hasEasyIntensity) {
                FieldLabel(stringResource(Res.string.log_field_time_above_z2), stringResource(Res.string.log_value_pct, "$easyPctAboveZ1"))
                ScrollableScaleRow(FivePercentScale, easyPctAboveZ1, { "$it" }, label = stringResource(Res.string.log_field_time_above_z2)) {
                    easyPctAboveZ1 = it
                }
            }
            LabeledSwitchRow(stringResource(Res.string.log_switch_positive_split), hasPositiveSplit) { hasPositiveSplit = it }
            if (hasPositiveSplit) {
                // Second half slower than the first, as a percent; the core flags
                // pacing strictly beyond +3 %.
                HalfPercentField(Res.string.log_field_second_half_slowdown, positiveSplitPct) { positiveSplitPct = it }
            }
        }
        ObservedAtRow(observedAt, withTime = false) { observedAt = it }
    }
}

@Composable
private fun HalfPercentField(label: StringResource, value: Double, onPick: (Double) -> Unit) {
    val text = stringResource(label)
    FieldLabel(text, stringResource(Res.string.log_value_pct, formatFixed(value, 1)))
    ScrollableScaleRow(HalfPercentScale, value, { formatFixed(it, 1) }, label = text, onSelect = onPick)
}

/** Whether today's session review asks the run details: today's plan has a run,
 *  or a run started today ([runLocalDay]). */
internal fun reviewAsksRun(model: ViewModel, zone: TimeZone = TimeZone.currentSystemDefault()): Boolean {
    val today = model.today()
    val plannedRun = model.week_plan.firstOrNull { it.epoch_day == today }?.let { s ->
        s.session_type.discipline == SessionDiscipline.Run || s.items.any { it.kind == ItemKind.Run }
    } == true
    return plannedRun || model.runs.any { runLocalDay(it, zone) == today }
}

/** The "How long?" band edges for the overtraining signs, in weeks: the
 *  duration the core's recovery concern needs ([CoreConstants.OVERTRAINING_MIN_WEEKS],
 *  feedback-036) and the decline the overtraining referrals need
 *  ([CoreConstants.NFOR_DECLINE_WEEKS], safety-042 / autoreg-042). */
private val OvertrainingWeeksMin = CoreConstants.OVERTRAINING_MIN_WEEKS
private val OvertrainingWeeksNfor = CoreConstants.NFOR_DECLINE_WEEKS.toDouble()

/** "How long?" answers, one value inside each band: below the minimum, between
 *  the two edges, past the NFOR edge. */
internal val OvertrainingWeeks = listOf(
    OvertrainingWeeksMin / 2,
    (OvertrainingWeeksMin + OvertrainingWeeksNfor) / 2,
    2 * OvertrainingWeeksNfor - OvertrainingWeeksMin,
)

internal fun overtrainingWeekEdgeLabels(): Pair<String, String> =
    trimDecimal(OvertrainingWeeksMin) to trimDecimal(OvertrainingWeeksNfor)

/** The plural quantity of the copy stating [OvertrainingWeeksMin]: the whole
 *  weeks, or 2 for a fraction of a week, which English reads plural ("1.5 weeks"). */
internal val OvertrainingWeeksMinQuantity: Int =
    OvertrainingWeeksMin.toInt().takeIf { it.toDouble() == OvertrainingWeeksMin } ?: 2

/** The explicit "Nothing to report this week" check-in: every count stated as
 *  0, no sign ticked, every clinical screen answered no. Only ever sent by that
 *  button, never by Save on an untouched form. */
internal fun weeklyAllClear(observedAt: Long): Event.SubmitWeeklyCheckin = Event.SubmitWeeklyCheckin(
    bonePainRedFlag = false,
    compulsiveFlag = false,
    redsSignalReported = false,
    overtrainingSigns = OvertrainingSigns(),
    failedKeySessions = 0,
    rpeLoadGapSessions = 0,
    observedAt = observedAt,
)

/**
 * Weekly check-in: the whole-week counts, the four listed overtraining signs with
 * how long they've lasted, and the clinical screens (disordered exercise, bone
 * pain, the RED-S low-energy question). Each clinical item is a plain question
 * with a sentence on why it's asked, because all three are medical-referral
 * triggers; the answers reach the core's safety gates unchanged (HARD RULE 3).
 * Emits [Event.SubmitWeeklyCheckin]; while one is in force ([checkinPresent]) a
 * "Clear weekly check-in" action sends [Event.ClearWeeklyCheckin] after a confirm.
 */
@Composable
fun WeeklyCheckinSheet(
    // The profile's sex answer picks the RED-S wording: true asks about periods,
    // false about sex drive / morning erections, null (not said) asks both.
    female: Boolean? = null,
    checkinPresent: Boolean = false,
    onClose: () -> Unit = {},
    onClear: () -> Unit = {},
    onSubmit: (Event.SubmitWeeklyCheckin) -> Unit,
) {
    var worsePerformance by rememberSaveable { mutableStateOf(false) }
    var lowMood by rememberSaveable { mutableStateOf(false) }
    var higherRestingHr by rememberSaveable { mutableStateOf(false) }
    var worseSleep by rememberSaveable { mutableStateOf(false) }
    var signWeeks by rememberSaveable { mutableStateOf<Double?>(null) }
    // Every answer starts unanswered: the screens select neither No nor Yes and
    // the counts no number, so Save stays disabled until the user answers. A quiet
    // week is sent explicitly with "Nothing to report this week".
    var reds by rememberSaveable { mutableStateOf<Boolean?>(null) }
    var bonePain by rememberSaveable { mutableStateOf<Boolean?>(null) }
    var compulsive by rememberSaveable { mutableStateOf<Boolean?>(null) }
    var failedKeySessions by rememberSaveable { mutableStateOf<Int?>(null) }
    var rpeLoadGapSessions by rememberSaveable { mutableStateOf<Int?>(null) }
    // Bar-speed drop, typed on the keypad (m/s); empty = not measured, nothing sent.
    var velocityText by rememberSaveable { mutableStateOf("") }
    var velocityFresh by rememberSaveable { mutableStateOf(true) }
    var velocityKeypad by rememberSaveable { mutableStateOf(false) }
    val velocityParsed = velocityText.toDoubleOrNull()
    val velocityValid = velocityText.isEmpty() || (velocityParsed != null && velocityParsed in VelocityDropRange)
    var moreDetails by rememberSaveable { mutableStateOf(false) }
    val openedAt = rememberSaveable { AppClock.nowSec() }
    var observedAt by rememberSaveable { mutableStateOf(openedAt) }
    val dateEdited = localDayStartSec(observedAt) != localDayStartSec(openedAt)
    var confirmClear by remember { mutableStateOf(false) }
    // In-flight latch for the all-clear, like EditorScaffold's Save: the sheet
    // takes a moment to hide, and a second tap must not send a second check-in.
    var allClearSent by remember { mutableStateOf(false) }
    val signs = OvertrainingSigns(
        worsePerformance = worsePerformance,
        lowMood = lowMood,
        higherRestingHr = higherRestingHr,
        worseSleep = worseSleep,
    )
    // Anything the user has said this week. A typed-then-emptied bar speed or an
    // unticked sign says nothing, and nothing is what an all-"no" Save would invent.
    val answered = signs.anyTicked || failedKeySessions != null || rpeLoadGapSessions != null ||
        velocityText.isNotEmpty() || compulsive != null || bonePain != null || reds != null
    val dirty = answered || dateEdited
    val savedMessage = stringResource(Res.string.log_weekly_saved)
    fun submit(checkin: Event.SubmitWeeklyCheckin) {
        onSubmit(checkin)
        UiMessages.post(UiMessage(savedMessage))
    }

    EditorScaffold(
        title = stringResource(Res.string.log_weekly_title),
        dirty = dirty,
        // An untouched form would submit a fabricated all-clear week over the
        // stored check-in: nothing to save until the user answers something.
        saveEnabled = answered && velocityValid,
        onClose = onClose,
        onSave = {
            submit(
                Event.SubmitWeeklyCheckin(
                    // An unanswered screen reports nothing (the wire's "no").
                    bonePainRedFlag = bonePain == true,
                    compulsiveFlag = compulsive == true,
                    redsSignalReported = reds == true,
                    overtrainingSigns = signs,
                    overtrainingSignalWeeks = if (signs.anyTicked) signWeeks else null,
                    // Whole-week deload triggers (autoreg-023/026/036); an
                    // unanswered count is omitted, never sent as 0.
                    failedKeySessions = failedKeySessions,
                    rpeLoadGapSessions = rpeLoadGapSessions,
                    weeklyVelocityDropMS = if (velocityText.isEmpty()) null else velocityParsed,
                    observedAt = observedAt,
                ),
            )
        },
        bottomExtra = {
            // The explicit all-clear, offered while nothing is answered: the user
            // states the quiet week instead of Save inventing one.
            if (!answered) {
                TextAction(
                    stringResource(Res.string.log_weekly_nothing),
                    onClick = {
                        if (!allClearSent) {
                            allClearSent = true
                            submit(weeklyAllClear(observedAt))
                        }
                    },
                    modifier = Modifier.offset(x = -TextActionInset),
                )
            }
        },
    ) {
        FormCard {
            // Week-level fatigue rollup: ≥2 of any one trigger prompts an easier week.
            WeekSessionCountScale(
                label = stringResource(Res.string.log_weekly_failed_sessions),
                selected = failedKeySessions,
                onSelect = { failedKeySessions = it },
                testTag = "weekly-failed",
                // A full Space.Lg step before the next question, as in the morning check-in.
                modifier = Modifier.padding(bottom = (Space.Lg - Space.Md).dp),
            )
            // rpe_load_gap_sessions: the target effort was reached at a load ≥7% below
            // plan (a fatigue sign), said in plain words.
            WeekSessionCountScale(
                label = stringResource(Res.string.log_weekly_rpe_gap),
                selected = rpeLoadGapSessions,
                onSelect = { rpeLoadGapSessions = it },
                testTag = "weekly-rpe-gap",
            )
            // Bar-speed drop is lab data (a velocity device): folded behind "More
            // details" and sent only once typed, never a fabricated 0.00.
            ExpandRow(stringResource(Res.string.log_weekly_more_details), moreDetails) { moreDetails = !moreDetails }
            if (moreDetails) {
                KeypadValueField(
                    stringResource(Res.string.log_weekly_velocity_drop),
                    stringResource(Res.string.log_unit_m_per_s),
                    velocityText,
                    active = velocityKeypad,
                    invalid = !velocityValid,
                    min = VelocityDropRange.start,
                    max = VelocityDropRange.endInclusive,
                    decimals = 2,
                    adjustments = listOf(-0.01, 0.01),
                    onActivate = { velocityFresh = true; velocityKeypad = true },
                    onText = { velocityText = it; velocityFresh = false },
                )
                KeypadPanel(velocityKeypad) {
                    NumericKeypad(
                        velocityText,
                        velocityFresh,
                        { velocityText = it; velocityFresh = false },
                        onDone = { velocityKeypad = false },
                    )
                }
            }
        }

        FormCard {
            FieldLabel(stringResource(Res.string.log_weekly_signs_q))
            val (minWeeks, nforWeeks) = overtrainingWeekEdgeLabels()
            Text(
                pluralStringResource(
                    Res.plurals.log_weekly_signs_ctx, OvertrainingWeeksMinQuantity, CoreConstants.OVERTRAINING_MIN_SIGNS, minWeeks,
                ),
                color = OnBgMuted,
                style = Type.Caption,
            )
            LabeledSwitchRow(stringResource(Res.string.log_weekly_sign_performance), worsePerformance) { worsePerformance = it }
            LabeledSwitchRow(stringResource(Res.string.log_weekly_sign_mood), lowMood) { lowMood = it }
            LabeledSwitchRow(stringResource(Res.string.log_weekly_sign_rhr), higherRestingHr) { higherRestingHr = it }
            LabeledSwitchRow(stringResource(Res.string.log_weekly_sign_sleep), worseSleep) { worseSleep = it }
            if (signs.anyTicked) {
                val weekLabels = listOf(
                    pluralStringResource(Res.plurals.log_weekly_signs_lt1, OvertrainingWeeksMinQuantity, minWeeks),
                    stringResource(Res.string.log_weekly_signs_1to2, minWeeks, nforWeeks),
                    stringResource(Res.string.log_weekly_signs_gt2, nforWeeks),
                )
                FieldLabel(stringResource(Res.string.log_weekly_signs_how_long))
                OptionList(OvertrainingWeeks, signWeeks ?: -1.0, { weekLabels[OvertrainingWeeks.indexOf(it)] }) {
                    signWeeks = it
                }
            }
        }

        SectionOverline(stringResource(Res.string.log_weekly_clinical_header))
        FormCard {
            ClinicalQuestion(
                question = stringResource(Res.string.log_clinical_compulsive_q),
                context = stringResource(Res.string.log_clinical_compulsive_ctx),
                checked = compulsive,
            ) { compulsive = it }
            ClinicalQuestion(
                question = stringResource(Res.string.log_clinical_bone_q),
                context = stringResource(Res.string.log_clinical_bone_ctx),
                checked = bonePain,
            ) { bonePain = it }
            // AUTOREG-10 / REDS-SCREEN-001: the low-energy screen in the wording
            // that fits the athlete.
            ClinicalQuestion(
                question = stringResource(
                    when (female) {
                        true -> Res.string.log_clinical_reds_q_female
                        false -> Res.string.log_clinical_reds_q_male
                        null -> Res.string.log_clinical_reds_q_any
                    },
                ),
                context = stringResource(Res.string.log_clinical_reds_ctx),
                checked = reds,
            ) { reds = it }
        }
        ObservedAtRow(observedAt, withTime = false) { observedAt = it }
        if (checkinPresent) {
            TextAction(
                stringResource(Res.string.log_weekly_clear),
                { confirmClear = true },
                modifier = Modifier.offset(x = -TextActionInset),
                danger = true,
            )
        }
    }
    if (confirmClear) ClearConfirmDialog(
        title = stringResource(Res.string.log_weekly_clear_title),
        message = stringResource(Res.string.log_weekly_clear_body),
        confirmLabel = stringResource(Res.string.log_weekly_clear_confirm),
        onDismiss = { confirmClear = false },
        onClear = onClear,
    )
}

@Composable
private fun WeekSessionCountScale(
    label: String,
    selected: Int?,
    onSelect: (Int?) -> Unit,
    testTag: String,
    modifier: Modifier = Modifier,
) {
    val shown = WeekSessionCounts.associateWith { pluralStringResource(Res.plurals.rating_sessions, it, it) }
    RatingScale(
        label = label,
        values = WeekSessionCounts,
        selected = selected,
        onSelect = onSelect,
        startCaption = shown.getValue(WeekSessionCounts.first()),
        endCaption = shown.getValue(WeekSessionCounts.last()),
        selectedText = { shown.getValue(it) },
        modifier = modifier,
        clearable = false,
        testTag = testTag,
    )
}

/** The answers to a clinical screen in reading order, plus the not-yet-answered
 *  state, which is never one of the offered segments so neither is selected. */
private enum class YesNo { No, Yes, Unanswered }

/**
 * A clinical screen in plain words: the question as the primary line, a
 * sentence explaining what a "yes" means (so a medical-deferral trigger never reads
 * as a cryptic toggle), and a neutral "No | Yes" segmented answer. [checked] null
 * selects neither (unanswered; Save then reports nothing for it). The wire value
 * is the same boolean the core's File-08 gates consume.
 */
@Composable
private fun ClinicalQuestion(
    question: String,
    context: String,
    checked: Boolean?,
    onCheck: (Boolean) -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
        Text(question, color = OnBgBody, style = Type.Body)
        Text(context, color = OnBgMuted, style = Type.Caption)
        val no = stringResource(Res.string.log_no)
        val yes = stringResource(Res.string.log_yes)
        SegmentedEnumRow(
            label = null,
            values = listOf(YesNo.No, YesNo.Yes),
            current = when (checked) {
                true -> YesNo.Yes
                false -> YesNo.No
                null -> YesNo.Unanswered
            },
            display = { if (it == YesNo.Yes) yes else no },
        ) { onCheck(it == YesNo.Yes) }
    }
}
