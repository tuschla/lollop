package app.milestone

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.relocation.BringIntoViewRequester
import androidx.compose.foundation.relocation.bringIntoViewRequester
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.RoundedCornerShape
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.dp
import java.time.LocalDate
import java.time.ZoneId

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
 * reads "Reached this week", an unmet one is just a count (no streaks, no red).
 * The evidence sits behind the card's one "?".
 */
@Composable
internal fun YouthActivityCard(view: YouthActivityView, onLog: () -> Unit, onEvent: (Event) -> Unit) {
    val res = LocalContext.current.resources
    fun reached(line: String, met: Boolean) = if (met) res.getString(R.string.activity_line_reached, line) else line
    val target = view.weekly_target_minutes
    val summary = reached(
        res.getString(R.string.activity_week_minutes, view.minutes, target),
        target > 0 && view.minutes >= target,
    )
    val fromParts = buildList {
        if (view.run_minutes > 0) add(res.getString(R.string.activity_from_runs, view.run_minutes))
        if (view.lift_minutes > 0) add(res.getString(R.string.activity_from_lifting, view.lift_minutes))
        if (view.activity_minutes > 0) add(res.getString(R.string.activity_from_other, view.activity_minutes))
    }
    EvidenceCard(
        summary = summary,
        grade = view.grade,
        citation = view.citation,
        confidence = view.confidence,
        safetyCritical = view.safety_critical,
        contested = view.contested,
        why = view.why,
        subject = stringResource(R.string.activity_subject),
        actionLabel = stringResource(R.string.activity_log_action),
        onAction = onLog,
        subline = {
            Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
                ActivityBar(if (target > 0) view.minutes.toFloat() / target else 0f)
                if (view.daily_target_minutes > 0) {
                    Text(stringResource(R.string.activity_daily_hint, view.daily_target_minutes), color = OnBgMuted, style = Type.Caption)
                }
                val days = view.target_days
                Text(
                    reached(res.getString(R.string.activity_vigorous_days, view.vigorous_days, days), days > 0 && view.vigorous_days >= days),
                    color = OnBgBody,
                    style = Type.Body,
                )
                Text(
                    reached(res.getString(R.string.activity_strength_days, view.strengthening_days, days), days > 0 && view.strengthening_days >= days),
                    color = OnBgBody,
                    style = Type.Body,
                )
                if (fromParts.isNotEmpty()) {
                    Text(stringResource(R.string.activity_from, fromParts.joinToString(" · ")), color = OnBgMuted, style = Type.Caption)
                }
            }
        },
        footer = if (view.week_entries.isEmpty()) null else {
            { WeekActivityList(view.week_entries, onEvent) }
        },
    )
}

/** The card's weekly-minutes bar: an accent fill over a faint track, capped full. */
@Composable
private fun ActivityBar(fraction: Float) {
    val shape = RoundedCornerShape(50)
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .height(ActivityBarHeight)
            .clip(shape)
            .background(OnBgFaint.copy(alpha = ActivityBarTrackAlpha))
            // The summary line above states the figure; the bar adds nothing to hear.
            .clearAndSetSemantics {},
    ) {
        Box(
            Modifier
                .fillMaxWidth(fraction.coerceIn(0f, 1f))
                .fillMaxHeight()
                .clip(shape)
                .background(Accent),
        )
    }
}

/** This week's logged activities, newest last, each with Delete (Undo re-logs it). */
@Composable
private fun WeekActivityList(entries: List<ActivityResultView>, onEvent: (Event) -> Unit) {
    val res = LocalContext.current.resources
    val zone = ZoneId.systemDefault()
    Column(verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
        TileOverline(stringResource(R.string.activity_logged_this_week))
        entries.forEach { a ->
            val line = activityEntryLine(res, a, zone)
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(line, color = OnBgBody, style = Type.Caption, modifier = Modifier.weight(1f))
                TextAction(
                    stringResource(R.string.main_delete),
                    onClick = {
                        onEvent(Event.DeleteEntry(Event.EntryKind.Activity, a.entry_id, observedAtFallback = a.observed_at))
                        UiMessages.post(
                            UiMessage(
                                res.getString(R.string.activity_removed),
                                actionLabel = res.getString(R.string.log_undo),
                                onAction = { EventDispatcher.send(restoreActivityEvent(a)) },
                            ),
                        )
                    },
                    description = stringResource(R.string.activity_remove_cd, line),
                )
            }
        }
    }
}

/** "Mon · Sport or PE · 45 min · vigorous · strengthening" for one logged
 *  activity; the last part only when it counted as muscle- or bone-strengthening. */
internal fun activityEntryLine(res: android.content.res.Resources, a: ActivityResultView, zone: ZoneId): String {
    val day = LocalDate.ofEpochDay(localEpochDay(a.observed_at, zone)).format(localizedDayFormatter("EEE"))
    val effort = res.getString(if (a.vigorous) R.string.activity_effort_vigorous_short else R.string.activity_effort_moderate_short)
    val line = res.getString(R.string.activity_entry_line, day, res.getString(a.kind.labelRes), Math.round(a.minutes).toInt(), effort)
    return if (a.strengthening) res.getString(R.string.activity_entry_line_strengthening, line) else line
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
    onClose: () -> Unit = {},
    onDirtyChange: (Boolean) -> Unit = {},
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
    val savedText = stringResource(R.string.activity_saved)
    // The keypad opens below the minutes field, often past the sheet's fold:
    // scroll it whole into view so every key (and "0") is reachable at once.
    val keypadRequester = remember { BringIntoViewRequester() }
    LaunchedEffect(keypadOpen) {
        if (keypadOpen) {
            withFrameNanos { }
            keypadRequester.bringIntoView()
        }
    }

    EditorScaffold(
        title = stringResource(R.string.activity_editor_title),
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
                stringResource(R.string.activity_field_minutes), stringResource(R.string.log_unit_min), minutesText,
                active = keypadOpen,
                invalid = minutesText.isNotEmpty() && !minutesValid,
                min = ActivityMinutesMin, max = ActivityMinutesMax, format = "%.0f",
                adjustments = listOf(-5.0, 5.0, 15.0),
                onActivate = { fresh = true; keypadOpen = true },
                onText = { minutesText = it; fresh = false; dirty = true },
                hint = if (minutesText.isNotEmpty() && !minutesValid) {
                    stringResource(R.string.activity_minutes_invalid)
                } else {
                    stringResource(R.string.activity_minutes_hint)
                },
                hintColor = if (minutesText.isNotEmpty() && !minutesValid) DangerText else OnBgMuted,
            )
            if (keypadOpen) {
                Box(Modifier.bringIntoViewRequester(keypadRequester)) {
                    NumericKeypad(
                        onKey = { key ->
                            minutesText = editNumericBuffer(minutesText, key, fresh)
                            fresh = false
                            dirty = true
                        },
                        onBackspace = {
                            minutesText = minutesText.dropLast(1)
                            fresh = false
                            dirty = true
                        },
                        allowDecimal = false,
                    )
                }
            }
            ActivityEffortField(vigorous) { vigorous = it; dirty = true }
            Column(verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
                SwitchRow(stringResource(R.string.activity_strengthening), strengthening) {
                    strengthening = it
                    strengtheningSet = true
                    dirty = true
                }
                Text(stringResource(R.string.activity_strengthening_hint), color = OnBgMuted, style = Type.Caption)
            }
            ObservedAtRow(observedAt, withTime = false) { observedAt = it; dirty = true }
        }
    }
}

/** What the activity was: every [ActivityKind] as one pick-one chip, laid out in
 *  balanced rows of equal-width cells (as the run type field). */
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
        FieldLabel(stringResource(R.string.activity_field_kind))
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
        false to (stringResource(R.string.activity_effort_moderate) to stringResource(R.string.activity_effort_moderate_sub)),
        true to (stringResource(R.string.activity_effort_vigorous) to stringResource(R.string.activity_effort_vigorous_sub)),
    )
    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
        FieldLabel(stringResource(R.string.activity_field_effort))
        Column(
            modifier = Modifier.fillMaxWidth().selectableGroup(),
            verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
        ) {
            options.forEach { (value, text) ->
                val selected = vigorous == value
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = ActionMinHeight)
                        .choiceCell(selected)
                        .selectable(selected = selected, role = Role.RadioButton) { onSelect(value) }
                        .padding(horizontal = Space.Card.dp, vertical = Space.Md.dp),
                    verticalArrangement = Arrangement.spacedBy(Space.Xs.dp),
                ) {
                    Text(
                        text.first,
                        color = choiceCellContent(selected),
                        style = if (selected) Type.Body.copy(fontWeight = FontWeight.Bold) else Type.Body,
                    )
                    Text(text.second, color = choiceCellContent(selected), style = Type.Caption)
                }
            }
        }
    }
}
