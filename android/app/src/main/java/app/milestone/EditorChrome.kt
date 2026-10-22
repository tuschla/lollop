package app.milestone

import android.text.format.DateFormat
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.wrapContentHeight
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.SelectableDates
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
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import java.text.SimpleDateFormat
import java.util.Calendar
import java.util.Date
import java.util.Locale
import java.util.TimeZone

/** True under a host that pins the editors' primary action to a bottom bar and
 *  lets the editor scroll its own body (the Log sheet). A host that scrolls the
 *  whole editor itself (the History edit sheet) leaves it false: nesting the
 *  editor's scroll inside a scrolling host would measure it at infinite height. */
internal val LocalEditorPinnedActions = staticCompositionLocalOf { false }

/** Minimum gap between two committed loop saves. A chalky or gloved double tap
 *  lands well inside it; a real next set (after its rest) never does. */
internal const val LoopRepeatGuardMs = 800L

/** Whether a loop Save tapped at [nowMillis] is a new set rather than a repeat
 *  tap of the save committed at [lastCommitMillis]. A clock that stepped back
 *  never blocks saving. Pure. */
internal fun loopSaveAccepted(lastCommitMillis: Long?, nowMillis: Long): Boolean {
    if (lastCommitMillis == null) return true
    val gap = nowMillis - lastCommitMillis
    return gap < 0 || gap >= LoopRepeatGuardMs
}

/**
 * Editor shell shared by the log editors. Under a pinning host (the Log sheet,
 * see [LocalEditorPinnedActions]) it is a close · title row, the form scrolling
 * in between, and a full-width primary action pinned to the bottom in thumb
 * reach, labelled with what it does ("Log set 2 of 4"); [bottomExtra] (the loop's
 * undo row) sits right above it. Under a host that scrolls the editor itself the
 * same full-width action ends the form, so Save never moves to the header. A
 * dirty editor's close asks "Discard this entry?" first (05-log §3).
 */
@Composable
fun EditorScaffold(
    title: String,
    dirty: Boolean,
    saveEnabled: Boolean,
    onClose: () -> Unit,
    onSave: () -> Unit,
    // Gym-floor loop (NEW set): the sheet stays open after Save so the next set is
    // one tap away. A repeat tap within [LoopRepeatGuardMs] of a commit is
    // swallowed (a double tap must log ONE set) and each commit ticks the haptic so
    // the lifter feels it registered. The closing path (EDIT / dismiss) latches
    // after its single commit instead.
    keepOpenAfterSave: Boolean = false,
    // What the primary action does; "Save" when null.
    actionLabel: String? = null,
    bottomExtra: @Composable ColumnScope.() -> Unit = {},
    content: @Composable ColumnScope.() -> Unit,
) {
    var confirmDiscard by rememberSaveable { mutableStateOf(false) }
    // In-flight latch for the closing path: the host's Save callback is
    // `{ ev -> onEvent(ev); onDismiss() }`, and the sheet takes ~300 ms to hide. A
    // second tap in that window would log a second fresh entry_id (a plain
    // `remember`, so it resets when a new editor is mounted).
    var submitted by remember { mutableStateOf(false) }
    // Clock time of the last committed loop save, for the double-tap guard.
    var lastCommitAt by remember { mutableStateOf<Long?>(null) }
    val haptic = LocalHapticFeedback.current
    val label = actionLabel ?: stringResource(R.string.main_save)
    val enabled = saveEnabled && !submitted
    val commit: () -> Unit = {
        if (keepOpenAfterSave) {
            val now = AppClock.nowMillis()
            if (saveEnabled && loopSaveAccepted(lastCommitAt, now)) {
                lastCommitAt = now
                haptic.performHapticFeedback(HapticFeedbackType.LongPress)
                onSave()
            }
        } else if (!submitted) {
            submitted = true
            onSave()
        }
    }
    val requestClose: () -> Unit = { if (dirty) confirmDiscard = true else onClose() }
    // System Back is the close button: inside the host sheet or dialog, so the
    // discard confirm opens OVER the editor and "Keep editing" really keeps it.
    // A host that scrolls the editor itself keeps its own Back while clean.
    BackHandler(enabled = LocalEditorPinnedActions.current || dirty) { requestClose() }
    if (LocalEditorPinnedActions.current) {
        Column(modifier = Modifier.fillMaxWidth()) {
            EditorTitleRow(title, requestClose)
            val scroll = rememberScrollState()
            Box(modifier = Modifier.weight(1f, fill = false).fillMaxWidth()) {
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .verticalScroll(scroll)
                        // The end of the form clears the pinned action by a full
                        // step, so the last field never sits flush against it.
                        .padding(top = Space.Sm.dp, bottom = Space.Lg.dp),
                    verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
                ) { content() }
                // While more form lies below, the content fades into the sheet
                // ground above the pinned action instead of being cut hard.
                if (scroll.canScrollForward) {
                    Box(
                        modifier = Modifier
                            .align(Alignment.BottomCenter)
                            .fillMaxWidth()
                            .height(Space.Lg.dp)
                            .background(Brush.verticalGradient(listOf(Color.Transparent, BgSunken))),
                    )
                }
            }
            // A full step between the pinned extras (the effort chips) and the
            // primary, so the chips never sit flush on Save.
            Column(
                modifier = Modifier.fillMaxWidth().padding(top = Space.Md.dp),
                verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
            ) {
                bottomExtra()
                // The pinned primary action, tagged `editor-primary`.
                PrimaryButton(
                    label,
                    commit,
                    Modifier.fillMaxWidth().testTag("editor-primary"),
                    enabled = enabled,
                    minHeight = LargeControlHeight,
                )
            }
        }
    } else {
        Column(
            modifier = Modifier.fillMaxWidth(),
            verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
        ) {
            EditorTitleRow(title, requestClose)
            content()
            bottomExtra()
            // The same full-width primary at the end of the form as the pinned
            // editors, so Save sits in one place in every editor.
            PrimaryButton(
                label,
                commit,
                Modifier.fillMaxWidth().testTag("editor-primary"),
                enabled = enabled,
                minHeight = LargeControlHeight,
            )
        }
    }
    if (confirmDiscard) {
        DiscardEntryDialog(onKeepEditing = { confirmDiscard = false }) {
            confirmDiscard = false
            onClose()
        }
    }
}

/** "Discard this entry?" over a dirty editor: Discard (danger text) drops the
 *  entry via [onDiscard]; Keep editing, Back or an outside tap ([onKeepEditing])
 *  return to it. */
@Composable
internal fun DiscardEntryDialog(onKeepEditing: () -> Unit, onDiscard: () -> Unit) {
    AlertDialog(
        onDismissRequest = onKeepEditing,
        shape = RoundedCornerShape(Space.Card.dp),
        title = { Text(stringResource(R.string.log_discard_title)) },
        confirmButton = {
            TextButton(onClick = onDiscard) { Text(stringResource(R.string.log_discard_confirm), color = DangerText) }
        },
        dismissButton = {
            TextButton(onClick = onKeepEditing) { Text(stringResource(R.string.log_keep_editing)) }
        },
    )
}

/** Mirrors an editor's unsaved-edits flag up to its host, so the host sheet's
 *  swipe-down dismissal can guard it. */
@Composable
internal fun ReportDirty(dirty: Boolean, onDirtyChange: (Boolean) -> Unit) {
    LaunchedEffect(dirty) { onDirtyChange(dirty) }
}

/** A keypad buffer as a number; null while it is empty or incomplete. The
 *  keypad types '.', a ',' decimal (a locale-formatted prefill) parses too. */
internal fun parseKeypadNumber(text: String): Double? = text.replace(',', '.').toDoubleOrNull()

/** An editor's top row: `ui-close` (a 48dp target) and the centered title. The
 *  primary action lives at the bottom, so the right side only balances the
 *  close box. */
@Composable
internal fun EditorTitleRow(title: String, onClose: () -> Unit) {
    Row(
        modifier = Modifier.fillMaxWidth().heightIn(min = ActionMinHeight),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            modifier = Modifier
                .size(ActionMinHeight)
                .clip(RoundedCornerShape(Space.Md.dp))
                .clickable(role = Role.Button, onClick = onClose),
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                painterResource(R.drawable.ic_ui_close),
                contentDescription = stringResource(R.string.main_close),
                tint = OnBgBody,
                modifier = Modifier.size(24.dp),
            )
        }
        Text(
            title,
            color = OnBgBody,
            style = Type.Title,
            modifier = Modifier.weight(1f),
            textAlign = TextAlign.Center,
        )
        Spacer(Modifier.size(ActionMinHeight))
    }
}

/** Unix second of the local midnight that starts the day containing [epochSec]. */
internal fun localDayStartSec(epochSec: Long, zone: java.time.ZoneId = java.time.ZoneId.systemDefault()): Long =
    java.time.Instant.ofEpochSecond(epochSec).atZone(zone).toLocalDate().atStartOfDay(zone).toEpochSecond()

/**
 * Lossless prefill for the weight/distance keypad buffers: the exact stored
 * double without trailing zeros (100.0 → "100", 42.195 → "42.195", 12.50 →
 * "12.5"), never a fixed-decimal rounding, so an untouched Save re-encodes the
 * stored value. BigDecimal.valueOf uses the canonical `Double.toString`, so no
 * binary-fraction noise leaks in; toPlainString avoids scientific notation the
 * keypad can't parse.
 */
internal fun fmtLosslessPrefill(v: Double): String =
    java.math.BigDecimal.valueOf(v).stripTrailingZeros().toPlainString()

/** Local noon (device zone) of [epochDay], the seed stamp for logging a PAST
 *  prescribed session from the week strip: noon avoids DST/midnight edge
 *  cases and lands the entry squarely on that calendar day. */
internal fun planDayObservedAt(epochDay: Long): Long =
    java.time.LocalDate.ofEpochDay(epochDay)
        .atTime(12, 0)
        .atZone(java.time.ZoneId.systemDefault())
        .toEpochSecond()

/**
 * Backdating control for every log editor: a compact chip stating when the
 * entry is logged ("Today · 14:32" / "Jul 15 · 09:10"), defaulting to now.
 * Tapping opens a Material 3 date picker (future dates unselectable); [withTime]
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
    // Device 12/24-hour preference (needs a Context) drives both the chip's rendered
    // time and the Material 3 time picker below, so the input and the label always agree.
    val use24h = DateFormat.is24HourFormat(LocalContext.current)
    var showDate by rememberSaveable { mutableStateOf(false) }
    var showTime by rememberSaveable { mutableStateOf(false) }
    var pendingDayUtcMillis by rememberSaveable { mutableStateOf<Long?>(null) }
    // Set when the last pick landed in the future and was clamped back to now, so
    // the row can say so instead of silently moving the stamp.
    var clamped by rememberSaveable { mutableStateOf(false) }
    // Clamp a picked instant to now and record whether it had to, then emit it.
    fun commit(dayMillis: Long, hour: Int, minute: Int) {
        val raw = localInstantSeconds(dayMillis, hour, minute)
        val now = AppClock.nowSec()
        clamped = raw > now
        onChange(minOf(raw, now))
    }

    Column(verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(stringResource(R.string.log_logged), color = OnBgBody, style = Type.Body)
            val todayLabel = stringResource(R.string.log_today)
            val dayStart = localDayStartSec(AppClock.nowSec())
            val shown = remember(epochSec, withTime, use24h, todayLabel, dayStart) {
                formatObservedAt(epochSec, withTime, use24h, todayLabel)
            }
            val changeLabel = stringResource(R.string.log_logged_change_cd)
            Text(
                shown,
                color = if (enabled) Accent else OnBgFaint,
                style = Type.Body.merge(TabularFigures),
                modifier = Modifier
                    .heightIn(min = ActionMinHeight)
                    .clip(RoundedCornerShape(Space.Md.dp))
                    .background(BgTop)
                    .then(
                        if (enabled) {
                            Modifier.clickable(role = Role.Button, onClickLabel = changeLabel) { showDate = true }
                        } else {
                            Modifier
                        },
                    )
                    .wrapContentHeight(Alignment.CenterVertically)
                    .padding(horizontal = Space.Card.dp, vertical = Space.Md.dp),
            )
        }
        if (!enabled) {
            Text(
                stringResource(R.string.log_backdate_locked),
                color = OnBgFaint,
                style = Type.Caption,
            )
        }
        // Always-composed single-line slot: empty until a future pick is clamped,
        // so surfacing the clamp never re-measures the sheet mid-edit.
        Text(
            if (clamped) stringResource(R.string.log_future_clamped) else "",
            color = OnBgMuted,
            style = Type.Caption,
            maxLines = 1,
        )
    }

    if (showDate) {
        val state = rememberDatePickerState(
            // Material 3 DatePicker reads the initial millis as a UTC-midnight day, but
            // epochSec is a local instant: near midnight in a UTC+ zone the raw
            // value lands on the previous UTC day and highlights the wrong cell.
            // Shift by the local offset so the highlighted day matches local time.
            initialSelectedDateMillis = (epochSec * 1000L).let {
                it + TimeZone.getDefault().getOffset(it)
            },
            selectableDates = object : SelectableDates {
                // Future dates blocked: DatePicker days are UTC-midnight stamps of
                // the picked calendar date, so the last selectable one is today's
                // local date at UTC midnight.
                override fun isSelectableDate(utcTimeMillis: Long): Boolean =
                    utcTimeMillis <= latestSelectableDayUtcMillis(AppClock.nowMillis())

                override fun isSelectableYear(year: Int): Boolean =
                    year <= Calendar.getInstance().apply { timeInMillis = AppClock.nowMillis() }.get(Calendar.YEAR)
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
                            commit(sel, cur.get(Calendar.HOUR_OF_DAY), cur.get(Calendar.MINUTE))
                        }
                    }
                }) { Text(stringResource(R.string.log_ok)) }
            },
            dismissButton = {
                TextButton(onClick = { showDate = false }) { Text(stringResource(R.string.main_cancel)) }
            },
        ) { DatePicker(state) }
    }

    if (showTime) {
        val cur = Calendar.getInstance().apply { timeInMillis = epochSec * 1000L }
        val tState = rememberTimePickerState(
            initialHour = cur.get(Calendar.HOUR_OF_DAY),
            initialMinute = cur.get(Calendar.MINUTE),
            is24Hour = use24h,
        )
        AlertDialog(
            onDismissRequest = { showTime = false },
            title = { Text(stringResource(R.string.log_logged_at)) },
            text = { TimePicker(tState) },
            confirmButton = {
                TextButton(onClick = {
                    showTime = false
                    pendingDayUtcMillis?.let { day ->
                        commit(day, tState.hour, tState.minute)
                    }
                }) { Text(stringResource(R.string.log_ok)) }
            },
            dismissButton = {
                TextButton(onClick = { showTime = false }) { Text(stringResource(R.string.main_cancel)) }
            },
        )
    }
}

/**
 * Combine a DatePicker day (UTC-midnight millis: its Y/M/D are the *picked*
 * calendar date) with a local wall-clock time into local unix seconds. UNCLAMPED:
 * the caller ([ObservedAtRow.commit]) clamps to now and surfaces when it had to,
 * so a future stamp can never be logged silently.
 */
private fun localInstantSeconds(utcDayMillis: Long, hour: Int, minute: Int): Long {
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
    return local.timeInMillis / 1000L
}

/** The DatePicker stamp (UTC midnight) of today's LOCAL calendar date at
 *  [nowMillis]: the latest day a log may be dated. Pure for a given [zone]. */
internal fun latestSelectableDayUtcMillis(nowMillis: Long, zone: java.time.ZoneId = java.time.ZoneId.systemDefault()): Long =
    java.time.Instant.ofEpochMilli(nowMillis).atZone(zone).toLocalDate()
        .atStartOfDay(java.time.ZoneOffset.UTC).toInstant().toEpochMilli()

/** "Today · 14:32" / "Jul 15 · 09:10" / date-only variants for the chip. The date
 *  renders in the device locale ("15 Jul", "7月15日", …) and the time honours the
 *  device 12/24-hour setting ([use24h], resolved from the Context by the caller).
 *  Pure formatting only: the wire's `observed_at` epoch is untouched. */
private fun formatObservedAt(epochSec: Long, withTime: Boolean, use24h: Boolean, todayLabel: String): String {
    val now = Calendar.getInstance().apply { timeInMillis = AppClock.nowMillis() }
    val then = Calendar.getInstance().apply { timeInMillis = epochSec * 1000L }
    val sameDay = now.get(Calendar.YEAR) == then.get(Calendar.YEAR) &&
        now.get(Calendar.DAY_OF_YEAR) == then.get(Calendar.DAY_OF_YEAR)
    val locale = Locale.getDefault()
    val day = if (sameDay) todayLabel else SimpleDateFormat("MMM d", locale).format(Date(epochSec * 1000L))
    return if (withTime) {
        val timePattern = if (use24h) "HH:mm" else "h:mm a"
        "$day · ${SimpleDateFormat(timePattern, locale).format(Date(epochSec * 1000L))}"
    } else {
        day
    }
}

/** An editor toggle row: [LabeledSwitchRow] (a `Type.Body` label over an optional
 *  `Type.Caption` [hint]) with the change handler as the trailing lambda. */
@Composable
internal fun SwitchRow(label: String, checked: Boolean, hint: String? = null, onChange: (Boolean) -> Unit) =
    LabeledSwitchRow(label = label, checked = checked, onCheckedChange = onChange, subtitle = hint)

/** A quiet disclosure row ("More", "More details"): an Accent label with the
 *  shared row chevron, turned down while [expanded]. A whole-row ≥48dp target,
 *  announced as expanded / collapsed. The caller renders the folded content. */
@Composable
internal fun ExpandRow(label: String, expanded: Boolean, onToggle: () -> Unit) {
    val state = stringResource(if (expanded) R.string.ui_cd_expanded else R.string.ui_cd_collapsed)
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = ActionMinHeight)
            .clip(RoundedCornerShape(Space.Md.dp))
            .clickable(role = Role.Button, onClick = onToggle)
            .semantics { stateDescription = state },
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
    ) {
        Text(
            label,
            color = Accent,
            style = Type.Body.copy(fontWeight = FontWeight.Bold),
            modifier = Modifier.weight(1f),
        )
        RowChevron(expanded)
    }
}

/** The editors' flat field card: the elevated ground, fields a step apart. */
@Composable
internal fun FormCard(content: @Composable () -> Unit) {
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
