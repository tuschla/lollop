package app.milestone

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.unit.dp
import java.time.ZoneId
import java.util.Locale

/** The tap scales, built once: 0–30 % in 0.5 steps (drift, second-half
 *  slowdown), 0–100 % in 5-point steps (time above the easy band) and 0–7
 *  sessions a week; the bar-speed drop a velocity device can plausibly report. */
private val HalfPercentScale = (0..60).map { it * 0.5 }
private val FivePercentScale = (0..100 step 5).toList()
private val WeekSessionCounts = (0..7).toList()
private val VelocityDropRange = 0.0..0.30

/** Where the decoupling-drift scale opens once "drift recorded" is switched on. */
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
    // Mirror unsaved-edits up to a host so its swipe-down dismissal can guard it.
    onDirtyChange: (Boolean) -> Unit = {},
    // Whether the run questions apply: a lift-only day never asks them.
    showRun: Boolean = reviewAsksRun(EventDispatcher.model.collectAsState().value),
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
    var observedAt by rememberSaveable { mutableStateOf(AppClock.nowSec()) }
    var dirty by rememberSaveable { mutableStateOf(false) }
    ReportDirty(dirty, onDirtyChange)

    EditorScaffold(
        title = stringResource(R.string.log_review_title),
        dirty = dirty,
        // Nothing answered → nothing to save (no fabricated "fine" review).
        saveEnabled = dirty,
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
                stringResource(R.string.log_review_intro),
                color = OnBgMuted,
                style = Type.Caption,
            )
            SwitchRow(stringResource(R.string.log_switch_off_day), badDay) { badDay = it; dirty = true }
        }
        if (showRun) FormCard {
            Text(stringResource(R.string.log_review_run_header), color = OnBgBody, style = Type.Body)
            SwitchRow(stringResource(R.string.log_switch_decoupling_recorded), hasDecoupling) { hasDecoupling = it; dirty = true }
            if (hasDecoupling) {
                // Tap-scale over 0–30 % in 0.5 steps, not a −/+ stepper.
                FieldLabel(
                    stringResource(R.string.log_field_decoupling_drift),
                    stringResource(R.string.log_value_pct, String.format(Locale.US, "%.1f", driftPct)),
                )
                ScrollableScaleRow(
                    HalfPercentScale,
                    driftPct,
                    { String.format(Locale.US, "%.1f", it) },
                    label = stringResource(R.string.log_field_decoupling_drift),
                ) { driftPct = it; dirty = true }
                // The core only judges drift on a cool, steady, easy effort; leave
                // this on for a genuine easy run.
                SwitchRow(stringResource(R.string.log_switch_cool_steady), coolSteady) { coolSteady = it; dirty = true }
            }
            SwitchRow(stringResource(R.string.log_switch_easy_intensity), hasEasyIntensity) { hasEasyIntensity = it; dirty = true }
            if (hasEasyIntensity) {
                // Tap-scale over 0–100 % in 5-point steps, not a −/+ stepper.
                FieldLabel(stringResource(R.string.log_field_time_above_z2), stringResource(R.string.log_value_pct, "$easyPctAboveZ1"))
                ScrollableScaleRow(FivePercentScale, easyPctAboveZ1, { "$it" }, label = stringResource(R.string.log_field_time_above_z2)) {
                    easyPctAboveZ1 = it
                    dirty = true
                }
            }
            SwitchRow(stringResource(R.string.log_switch_positive_split), hasPositiveSplit) { hasPositiveSplit = it; dirty = true }
            if (hasPositiveSplit) {
                // Second half slower than the first, as a percent; the core flags
                // pacing strictly beyond +3 %. Tap-scale over 0–30 % in 0.5 steps.
                FieldLabel(
                    stringResource(R.string.log_field_second_half_slowdown),
                    stringResource(R.string.log_value_pct, String.format(Locale.US, "%.1f", positiveSplitPct)),
                )
                ScrollableScaleRow(
                    HalfPercentScale,
                    positiveSplitPct,
                    { String.format(Locale.US, "%.1f", it) },
                    label = stringResource(R.string.log_field_second_half_slowdown),
                ) { positiveSplitPct = it; dirty = true }
            }
        }
        ObservedAtRow(observedAt, withTime = false) { observedAt = it; dirty = true }
    }
}

/** Whether today's session review asks the run details: today's plan has a run,
 *  or a run is logged today. Without a model yet it asks them (nothing to rule
 *  them out). Pure. */
internal fun reviewAsksRun(model: ViewModel?, zone: ZoneId = ZoneId.systemDefault()): Boolean {
    if (model == null) return true
    val today = model.today()
    val plannedRun = model.week_plan.firstOrNull { it.epoch_day == today }?.let { s ->
        sessionDiscipline(s.session_type) == "Run" ||
            s.items.any { it.exercise.isEmpty() && !isReturnCheckItem(it, s.session_type) }
    } == true
    return plannedRun || model.runs.any { localEpochDay(it.observed_at, zone) == today }
}

/** "How long?" answers for the overtraining signs, in weeks: < 1, 1–2, > 2. The
 *  core acts only on 2+ signs held for a week or more (feedback-036). */
private val OvertrainingWeeks = listOf(0.5, 1.5, 3.0)

/** The explicit "Nothing to report this week" check-in: every count stated as
 *  0, no sign ticked, every clinical screen answered no. Only ever sent by that
 *  button, never by Save on an untouched form. Pure. */
internal fun weeklyAllClear(observedAt: Long): Event.SubmitReview = Event.SubmitReview(
    bonePainRedFlag = false,
    compulsiveFlag = false,
    redsSignalReported = false,
    overtrainingSignalCount = 0,
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
    // Mirror unsaved-edits up to a host so its swipe-down dismissal can guard it.
    onDirtyChange: (Boolean) -> Unit = {},
    onClear: () -> Unit = {},
    onSubmit: (Event.SubmitWeeklyCheckin) -> Unit,
) {
    var worsePerformance by rememberSaveable { mutableStateOf(false) }
    var lowMood by rememberSaveable { mutableStateOf(false) }
    var higherRestingHr by rememberSaveable { mutableStateOf(false) }
    var worseSleep by rememberSaveable { mutableStateOf(false) }
    var signWeeks by rememberSaveable { mutableStateOf<Double?>(null) }
    // Every answer starts unanswered: the screens select neither No nor Yes and
    // the counts no number, so a disabled Save is honest about why. A quiet week
    // is sent explicitly with "Nothing to report this week".
    var reds by rememberSaveable { mutableStateOf<Boolean?>(null) }
    var bonePain by rememberSaveable { mutableStateOf<Boolean?>(null) }
    var compulsive by rememberSaveable { mutableStateOf<Boolean?>(null) }
    var failedKeySessions by rememberSaveable { mutableStateOf<Int?>(null) }
    var rpeLoadGapSessions by rememberSaveable { mutableStateOf<Int?>(null) }
    // Bar-speed drop, typed on the keypad (m/s); empty = not measured, nothing sent.
    var velocityText by rememberSaveable { mutableStateOf("") }
    var velocityFresh by rememberSaveable { mutableStateOf(true) }
    var velocityKeypad by rememberSaveable { mutableStateOf(false) }
    val velocityParsed = parseKeypadNumber(velocityText)
    val velocityValid = velocityText.isEmpty() || (velocityParsed != null && velocityParsed in VelocityDropRange)
    var moreDetails by rememberSaveable { mutableStateOf(false) }
    var observedAt by rememberSaveable { mutableStateOf(AppClock.nowSec()) }
    var dirty by rememberSaveable { mutableStateOf(false) }
    var confirmClear by remember { mutableStateOf(false) }
    // In-flight latch for the all-clear, like EditorScaffold's Save: the sheet
    // takes a moment to hide, and a second tap must not send a second check-in.
    var allClearSent by remember { mutableStateOf(false) }
    ReportDirty(dirty, onDirtyChange)
    val signs = OvertrainingSigns(
        worsePerformance = worsePerformance,
        lowMood = lowMood,
        higherRestingHr = higherRestingHr,
        worseSleep = worseSleep,
    )
    val savedMessage = stringResource(R.string.log_weekly_saved)
    fun submit(review: Event.SubmitReview) {
        onSubmit(Event.SubmitWeeklyCheckin(review))
        UiMessages.post(UiMessage(savedMessage))
    }

    EditorScaffold(
        title = stringResource(R.string.log_weekly_title),
        dirty = dirty,
        // An untouched form would submit a fabricated all-clear week over the
        // stored check-in: nothing to save until the user answers something.
        saveEnabled = dirty && velocityValid,
        onClose = onClose,
        onSave = {
            submit(
                Event.SubmitReview(
                    // An unanswered screen reports nothing (the wire's "no").
                    bonePainRedFlag = bonePain == true,
                    compulsiveFlag = compulsive == true,
                    redsSignalReported = reds == true,
                    // The core counts the ticked signs; the count rides along
                    // for an older reader.
                    overtrainingSignalCount = listOf(worsePerformance, lowMood, higherRestingHr, worseSleep).count { it },
                    overtrainingSigns = signs,
                    overtrainingSignalWeeks = if (signs.any) signWeeks else null,
                    // Whole-week deload triggers (autoreg-023/026/036); an
                    // unanswered count is omitted, never sent as 0.
                    failedKeySessions = failedKeySessions,
                    rpeLoadGapSessions = rpeLoadGapSessions,
                    weeklyVelocityDropMs = if (velocityText.isEmpty()) null else velocityParsed,
                    observedAt = observedAt,
                ),
            )
        },
        bottomExtra = {
            // The explicit all-clear, offered while nothing is answered: the user
            // states the quiet week instead of Save inventing one.
            if (!dirty) {
                TextAction(
                    stringResource(R.string.log_weekly_nothing),
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
            // Both counts start unanswered (nothing selected, nothing sent).
            val sessionsShown = WeekSessionCounts.associateWith { pluralStringResource(R.plurals.rating_sessions, it, it) }
            RatingScale(
                label = stringResource(R.string.log_weekly_failed_sessions),
                values = WeekSessionCounts,
                selected = failedKeySessions,
                onSelect = { failedKeySessions = it; dirty = true },
                startCaption = sessionsShown.getValue(WeekSessionCounts.first()),
                endCaption = sessionsShown.getValue(WeekSessionCounts.last()),
                selectedText = { sessionsShown.getValue(it) },
                clearable = false,
                testTag = "weekly-failed",
                // A full Space.Lg step before the next question, as in the morning check-in.
                modifier = Modifier.padding(bottom = (Space.Lg - Space.Md).dp),
            )
            // rpe_load_gap_sessions: the target effort was reached at a load ≥7% below
            // plan (a fatigue sign), said in plain words.
            RatingScale(
                label = stringResource(R.string.log_weekly_rpe_gap),
                values = WeekSessionCounts,
                selected = rpeLoadGapSessions,
                onSelect = { rpeLoadGapSessions = it; dirty = true },
                startCaption = sessionsShown.getValue(WeekSessionCounts.first()),
                endCaption = sessionsShown.getValue(WeekSessionCounts.last()),
                selectedText = { sessionsShown.getValue(it) },
                clearable = false,
                testTag = "weekly-rpe-gap",
            )
            // Bar-speed drop is lab data (a velocity device): folded behind "More
            // details" and sent only once typed, never a fabricated 0.00.
            ExpandRow(stringResource(R.string.log_weekly_more_details), moreDetails) { moreDetails = !moreDetails }
            if (moreDetails) {
                KeypadValueField(
                    stringResource(R.string.log_weekly_velocity_drop),
                    stringResource(R.string.log_unit_m_per_s),
                    velocityText,
                    active = velocityKeypad,
                    invalid = !velocityValid,
                    min = VelocityDropRange.start,
                    max = VelocityDropRange.endInclusive,
                    format = "%.2f",
                    adjustments = listOf(-0.01, 0.01),
                    onActivate = { velocityFresh = true; velocityKeypad = true },
                    onText = { velocityText = it; velocityFresh = false; dirty = true },
                )
                if (velocityKeypad) {
                    NumericKeypad(
                        onKey = { key ->
                            velocityText = editNumericBuffer(velocityText, key, velocityFresh)
                            velocityFresh = false
                            dirty = true
                        },
                        onBackspace = {
                            velocityText = velocityText.dropLast(1)
                            velocityFresh = false
                            dirty = true
                        },
                        onDone = { velocityKeypad = false },
                    )
                }
            }
        }

        // The four listed signs, then how long, instead of a bare count.
        FormCard {
            FieldLabel(stringResource(R.string.log_weekly_signs_q))
            Text(stringResource(R.string.log_weekly_signs_ctx), color = OnBgMuted, style = Type.Caption)
            SwitchRow(stringResource(R.string.log_weekly_sign_performance), worsePerformance) { worsePerformance = it; dirty = true }
            SwitchRow(stringResource(R.string.log_weekly_sign_mood), lowMood) { lowMood = it; dirty = true }
            SwitchRow(stringResource(R.string.log_weekly_sign_rhr), higherRestingHr) { higherRestingHr = it; dirty = true }
            SwitchRow(stringResource(R.string.log_weekly_sign_sleep), worseSleep) { worseSleep = it; dirty = true }
            if (signs.any) {
                val weekLabels = listOf(
                    stringResource(R.string.log_weekly_signs_lt1),
                    stringResource(R.string.log_weekly_signs_1to2),
                    stringResource(R.string.log_weekly_signs_gt2),
                )
                FieldLabel(stringResource(R.string.log_weekly_signs_how_long))
                OptionList(OvertrainingWeeks, signWeeks ?: -1.0, { weekLabels[OvertrainingWeeks.indexOf(it)] }) {
                    signWeeks = it
                    dirty = true
                }
            }
        }

        // Clinical screens, humanely framed: medical-referral triggers get
        // context, not a bare switch.
        SectionOverline(stringResource(R.string.log_weekly_clinical_header))
        FormCard {
            ClinicalQuestion(
                question = stringResource(R.string.log_clinical_compulsive_q),
                context = stringResource(R.string.log_clinical_compulsive_ctx),
                checked = compulsive,
            ) { compulsive = it; dirty = true }
            ClinicalQuestion(
                question = stringResource(R.string.log_clinical_bone_q),
                context = stringResource(R.string.log_clinical_bone_ctx),
                checked = bonePain,
            ) { bonePain = it; dirty = true }
            // AUTOREG-10 / REDS-SCREEN-001: the low-energy screen in the wording
            // that fits the athlete.
            ClinicalQuestion(
                question = stringResource(
                    when (female) {
                        true -> R.string.log_clinical_reds_q_female
                        false -> R.string.log_clinical_reds_q_male
                        null -> R.string.log_clinical_reds_q_any
                    },
                ),
                context = stringResource(R.string.log_clinical_reds_ctx),
                checked = reds,
            ) { reds = it; dirty = true }
        }
        ObservedAtRow(observedAt, withTime = false) { observedAt = it; dirty = true }
        if (checkinPresent) {
            TextAction(
                stringResource(R.string.log_weekly_clear),
                { confirmClear = true },
                modifier = Modifier.offset(x = -TextActionInset),
                danger = true,
            )
        }
    }
    if (confirmClear) {
        AlertDialog(
            onDismissRequest = { confirmClear = false },
            shape = RoundedCornerShape(Space.Card.dp),
            title = { Text(stringResource(R.string.log_weekly_clear_title)) },
            text = { Text(stringResource(R.string.log_weekly_clear_body)) },
            confirmButton = {
                TextButton(onClick = {
                    confirmClear = false
                    onClear()
                }) { Text(stringResource(R.string.log_weekly_clear_confirm)) }
            },
            dismissButton = {
                TextButton(onClick = { confirmClear = false }) { Text(stringResource(R.string.main_cancel)) }
            },
        )
    }
}

/** The answers to a clinical screen in reading order, plus the not-yet-answered
 *  state, which is never one of the offered segments so neither is selected. */
private enum class YesNo { No, Yes, Unanswered }

/**
 * A humanely-framed clinical screen: the question as the primary line, a
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
        val no = stringResource(R.string.log_no)
        val yes = stringResource(R.string.log_yes)
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
