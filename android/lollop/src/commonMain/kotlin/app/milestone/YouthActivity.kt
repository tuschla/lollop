package app.milestone

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.dp
import app.milestone.resources.Res
import app.milestone.resources.activity_daily_hint
import app.milestone.resources.activity_editor_title
import app.milestone.resources.activity_effort_moderate
import app.milestone.resources.activity_effort_moderate_short
import app.milestone.resources.activity_effort_moderate_sub
import app.milestone.resources.activity_effort_vigorous
import app.milestone.resources.activity_effort_vigorous_short
import app.milestone.resources.activity_effort_vigorous_sub
import app.milestone.resources.activity_entry_line
import app.milestone.resources.activity_entry_line_strengthening
import app.milestone.resources.activity_field_effort
import app.milestone.resources.activity_field_kind
import app.milestone.resources.activity_field_minutes
import app.milestone.resources.activity_from
import app.milestone.resources.activity_from_lifting
import app.milestone.resources.activity_from_other
import app.milestone.resources.activity_from_runs
import app.milestone.resources.activity_line_reached
import app.milestone.resources.activity_log_action
import app.milestone.resources.activity_logged_this_week
import app.milestone.resources.activity_minutes_hint
import app.milestone.resources.activity_minutes_invalid
import app.milestone.resources.activity_remove_cd
import app.milestone.resources.activity_removed
import app.milestone.resources.activity_saved
import app.milestone.resources.activity_strength_days
import app.milestone.resources.activity_strengthening
import app.milestone.resources.activity_strengthening_hint
import app.milestone.resources.activity_subject
import app.milestone.resources.activity_vigorous_days
import app.milestone.resources.activity_week_minutes
import app.milestone.resources.log_undo
import app.milestone.resources.log_unit_min
import app.milestone.resources.main_delete
import kotlinx.datetime.TimeZone
import org.jetbrains.compose.resources.stringResource

/** Height of the weekly-minutes bar on the activity card. */
private val ActivityBarHeight = 6.dp

/** Opacity of the bar's track behind the accent fill (OnBgFaint). */
private const val ActivityBarTrackAlpha = 0.4f

/** The Log activity editor's minutes range: the core keeps 0 < minutes ≤ 600. */
private const val ActivityMinutesMin = 1.0
private const val ActivityMinutesMax = 600.0

/**
 * The under-18 weekly activity card (WHO 5–17, youth-pa-001): this week's
 * moderate-to-vigorous minutes against the weekly target with a bar, the
 * vigorous and muscle-and-bone strengthening days against theirs, where the
 * minutes came from, this week's logged activities (each removable, with Undo)
 * and "Log activity". Every target comes from the core's view; a met target
 * reads "· Reached", an unmet one is just a count (no streaks, no red).
 * The evidence sits behind the card's one "?".
 */
@Composable
internal fun YouthActivityCard(view: YouthActivityView, onLog: () -> Unit, onEvent: (Event) -> Unit) {
    val res = currentResources()
    fun reached(line: String, met: Boolean) = if (met) res.getString(Res.string.activity_line_reached, line) else line
    val target = view.weekly_target_minutes
    val summary = reached(
        res.getString(Res.string.activity_week_minutes, view.minutes, target),
        target > 0 && view.minutes >= target,
    )
    val fromParts = buildList {
        if (view.run_minutes > 0) add(res.getString(Res.string.activity_from_runs, view.run_minutes))
        if (view.lift_minutes > 0) add(res.getString(Res.string.activity_from_lifting, view.lift_minutes))
        if (view.activity_minutes > 0) add(res.getString(Res.string.activity_from_other, view.activity_minutes))
    }
    EvidenceCard(
        summary = summary,
        grade = view.grade,
        citation = view.citation,
        confidence = view.confidence,
        safetyCritical = view.safety_critical,
        contested = view.contested,
        why = view.why,
        subject = stringResource(Res.string.activity_subject),
        actionLabel = stringResource(Res.string.activity_log_action),
        onAction = onLog,
        subline = {
            Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
                // The summary line above states the figure; the bar adds nothing to hear.
                ProgressPill(
                    if (target > 0) view.minutes.toFloat() / target else 0f,
                    ActivityBarHeight,
                    OnBgFaint.copy(alpha = ActivityBarTrackAlpha),
                    Modifier.fillMaxWidth().clearAndSetSemantics {},
                )
                if (view.daily_target_minutes > 0) {
                    Text(stringResource(Res.string.activity_daily_hint, view.daily_target_minutes), color = OnBgMuted, style = Type.Caption)
                }
                val days = view.target_days
                Text(
                    reached(res.getString(Res.string.activity_vigorous_days, view.vigorous_days, days), days > 0 && view.vigorous_days >= days),
                    color = OnBgBody,
                    style = Type.Body,
                )
                Text(
                    reached(res.getString(Res.string.activity_strength_days, view.strengthening_days, days), days > 0 && view.strengthening_days >= days),
                    color = OnBgBody,
                    style = Type.Body,
                )
                if (fromParts.isNotEmpty()) {
                    Text(stringResource(Res.string.activity_from, fromParts.joinToString(" · ")), color = OnBgMuted, style = Type.Caption)
                }
            }
        },
        footer = if (view.week_entries.isEmpty()) null else {
            { WeekActivityList(view.week_entries, onEvent) }
        },
    )
}

/** This week's logged activities, newest last, each with Delete (Undo re-logs it). */
@Composable
private fun WeekActivityList(entries: List<ActivityResultView>, onEvent: (Event) -> Unit) {
    val res = currentResources()
    val zone = TimeZone.currentSystemDefault()
    Column(verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
        TileOverline(stringResource(Res.string.activity_logged_this_week))
        entries.forEach { a ->
            val line = activityEntryLine(res, a, zone)
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(line, color = OnBgBody, style = Type.Caption, modifier = Modifier.weight(1f))
                TextAction(
                    stringResource(Res.string.main_delete),
                    onClick = {
                        onEvent(Event.DeleteEntry(EntryKind.Activity, a.entry_id, observedAtFallback = a.observed_at))
                        postUndoable(res.getString(Res.string.activity_removed), res.getString(Res.string.log_undo), listOf(restoreActivityEvent(a)))
                    },
                    description = stringResource(Res.string.activity_remove_cd, line),
                )
            }
        }
    }
}

/** "Mon · Sport or PE · 45 min · vigorous · strengthening" for one logged
 *  activity; the last part only when it counted as muscle- or bone-strengthening. */
internal fun activityEntryLine(res: PlatformResources, a: ActivityResultView, zone: TimeZone): String {
    val day = localizedDayFormatter("EEE").formatEpochDay(localEpochDay(a.observed_at, zone))
    val effort = res.getString(if (a.vigorous) Res.string.activity_effort_vigorous_short else Res.string.activity_effort_moderate_short)
    val line = res.getString(Res.string.activity_entry_line, day, res.getString(a.kind.labelRes), a.minutes.roundOrZero().toInt(), effort)
    return if (a.strengthening) res.getString(Res.string.activity_entry_line_strengthening, line) else line
}

/** The event that logs a removed activity again, for "Undo": the same entry_id,
 *  observed_at and fields the view shows. */
internal fun restoreActivityEvent(a: ActivityResultView): Event.LogActivity =
    Event.LogActivity(
        kind = a.kind,
        minutes = a.minutes,
        vigorous = a.vigorous,
        strengthening = a.strengthening,
        observedAt = a.observed_at,
        entryId = a.entry_id,
    )

/**
 * Log activity (other moderate-or-harder active time for the weekly card): what
 * it was, the active minutes on the shared keypad, the effort on the WHO 0–10
 * relative scale (moderate 5–6, vigorous 7–8), whether it strengthened muscles or
 * bones (on by default for a Strength activity until the user sets it), and the
 * backdating chip. Sends [Event.LogActivity].
 */
@Composable
fun LogActivityEditor(
    onClose: () -> Unit,
    onDirtyChange: (Boolean) -> Unit,
    onSave: (Event) -> Unit,
) {
    var kind by rememberSaveable { mutableStateOf<ActivityKind?>(null) }
    var minutesText by rememberSaveable { mutableStateOf("") }
    var fresh by rememberSaveable { mutableStateOf(true) }
    var keypadOpen by rememberSaveable { mutableStateOf(false) }
    var vigorous by rememberSaveable { mutableStateOf(false) }
    var strengthening by rememberSaveable { mutableStateOf(false) }
    var strengtheningSet by rememberSaveable { mutableStateOf(false) }
    var observedAt by rememberSaveable { mutableStateOf(AppClock.nowSec()) }
    var dirty by rememberSaveable { mutableStateOf(false) }
    ReportDirty(dirty, onDirtyChange)

    val minutes = parseKeypadNumber(minutesText)
    val minutesValid = minutes != null && minutes in ActivityMinutesMin..ActivityMinutesMax
    val savedText = stringResource(Res.string.activity_saved)

    EditorScaffold(
        title = stringResource(Res.string.activity_editor_title),
        dirty = dirty,
        saveEnabled = kind != null && minutesValid,
        onClose = onClose,
        onSave = {
            val k = kind
            if (k != null && minutes != null) {
                onSave(Event.LogActivity(k, minutes, vigorous = vigorous, strengthening = strengthening, observedAt = observedAt))
                UiMessages.post(UiMessage(savedText))
            }
        },
    ) {
        BackHandler(enabled = keypadOpen) { keypadOpen = false }
        FormCard {
            ActivityKindField(kind) { picked ->
                kind = picked
                if (!strengtheningSet) strengthening = picked == ActivityKind.Strength
                dirty = true
            }
            KeypadValueField(
                stringResource(Res.string.activity_field_minutes), stringResource(Res.string.log_unit_min), minutesText,
                active = keypadOpen,
                invalid = minutesText.isNotEmpty() && !minutesValid,
                min = ActivityMinutesMin, max = ActivityMinutesMax, decimals = 0,
                adjustments = listOf(-5.0, 5.0, 15.0),
                onActivate = { fresh = true; keypadOpen = true },
                onText = { minutesText = it; fresh = false; dirty = true },
                hint = if (minutesText.isNotEmpty() && !minutesValid) {
                    stringResource(Res.string.activity_minutes_invalid)
                } else {
                    stringResource(Res.string.activity_minutes_hint)
                },
                hintColor = if (minutesText.isNotEmpty() && !minutesValid) DangerText else OnBgMuted,
            )
            // Opens below the minutes field, often past the sheet's fold.
            KeypadPanel(keypadOpen) {
                NumericKeypad(minutesText, fresh, { minutesText = it; fresh = false; dirty = true }, allowDecimal = false)
            }
            ActivityEffortField(vigorous) { vigorous = it; dirty = true }
            Column(verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
                LabeledSwitchRow(stringResource(Res.string.activity_strengthening), strengthening) {
                    strengthening = it
                    strengtheningSet = true
                    dirty = true
                }
                Text(stringResource(Res.string.activity_strengthening_hint), color = OnBgMuted, style = Type.Caption)
            }
            ObservedAtRow(observedAt, withTime = false) { observedAt = it; dirty = true }
        }
    }
}

/** What the activity was: every [ActivityKind] as one pick-one chip, laid out in
 *  balanced rows of equal-width cells. */
@Composable
@OptIn(ExperimentalLayoutApi::class)
private fun ActivityKindField(selected: ActivityKind?, onSelect: (ActivityKind) -> Unit) {
    val kinds = ActivityKind.entries
    val labels = kinds.map { stringResource(it.labelRes) }
    val measurer = rememberTextMeasurer()
    val chipStyle = Type.Chip
    val widestPx = remember(measurer, chipStyle, labels) {
        labels.maxOf { measurer.measure(it, chipStyle, maxLines = 1, softWrap = false).size.width }
    }
    val density = LocalDensity.current
    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
        FieldLabel(stringResource(Res.string.activity_field_kind))
        BoxWithConstraints(Modifier.fillMaxWidth().selectableGroup()) {
            val need = with(density) { widestPx.toDp() } + Space.Card.dp * 2
            val fit = ((maxWidth + ChoiceChipGap) / (need + ChoiceChipGap)).toInt().coerceIn(1, kinds.size)
            val rows = (kinds.size + fit - 1) / fit
            val perRow = (kinds.size + rows - 1) / rows
            val cell = (maxWidth - ChoiceChipGap * (perRow - 1)) / perRow - 1.dp
            FlowRow(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(ChoiceChipGap),
                verticalArrangement = Arrangement.spacedBy(ChoiceChipGap),
                maxItemsInEachRow = perRow,
            ) {
                kinds.forEachIndexed { i, k ->
                    ChoiceChip(
                        label = labels[i],
                        selected = k == selected,
                        onClick = { onSelect(k) },
                        modifier = Modifier.width(cell),
                    )
                }
            }
        }
    }
}

/** Effort on the WHO relative 0–10 scale: two whole-row options, each with its
 *  plain description. */
@Composable
private fun ActivityEffortField(vigorous: Boolean, onSelect: (Boolean) -> Unit) {
    val options = listOf(
        false to (stringResource(Res.string.activity_effort_moderate) to stringResource(Res.string.activity_effort_moderate_sub)),
        true to (stringResource(Res.string.activity_effort_vigorous) to stringResource(Res.string.activity_effort_vigorous_sub)),
    )
    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
        FieldLabel(stringResource(Res.string.activity_field_effort))
        Column(
            modifier = Modifier.fillMaxWidth().selectableGroup(),
            verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
        ) {
            options.forEach { (value, text) ->
                ChoiceRow(text.first, text.second, selected = vigorous == value) { onSelect(value) }
            }
        }
    }
}
