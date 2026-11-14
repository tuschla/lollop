package app.milestone

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.wrapContentHeight
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.SheetValue
import androidx.compose.material3.Icon
import androidx.compose.material3.SelectableDates
import androidx.compose.material3.Text
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.Stable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import app.milestone.resources.Res
import app.milestone.resources.ic_ui_close
import app.milestone.resources.log_discard_confirm
import app.milestone.resources.log_discard_title
import app.milestone.resources.log_future_clamped
import app.milestone.resources.log_keep_editing
import app.milestone.resources.log_logged
import app.milestone.resources.log_logged_at
import app.milestone.resources.log_logged_change_cd
import app.milestone.resources.log_today
import app.milestone.resources.main_close
import app.milestone.resources.main_save
import kotlin.time.Instant
import kotlinx.coroutines.launch
import kotlinx.datetime.TimeZone
import kotlinx.datetime.atStartOfDayIn
import kotlinx.datetime.offsetAt
import kotlinx.datetime.toLocalDateTime
import org.jetbrains.compose.resources.painterResource
import org.jetbrains.compose.resources.stringResource

/** True under a host that pins the editors' primary action to a bottom bar and
 *  lets the editor scroll its own body (the Log and History edit sheets). A host
 *  that scrolls the whole editor itself leaves it false: nesting the editor's
 *  scroll inside a scrolling host would measure it at infinite height. */
internal val LocalEditorPinnedActions = staticCompositionLocalOf { false }

/** Minimum gap between two committed loop saves. A chalky or gloved double tap
 *  lands well inside it; a real next set (after its rest) never does. */
internal const val LoopRepeatGuardMs = 800L

/** Whether a loop Save tapped at [nowMillis] is a new set rather than a repeat
 *  tap of the save committed at [lastCommitMillis]. A clock that stepped back
 *  never blocks saving. */
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
    // A safety commit ("Report pain"): danger fill, and the button never looks
    // blocked (HARD RULE 3); the latch still swallows re-taps.
    danger: Boolean = false,
    bottomExtra: @Composable ColumnScope.() -> Unit = {},
    // An editor with an in-form keypad (set, run): on a landscape window
    // ([WindowLayout.sideControls]) its keypad, [bottomExtra] and the primary
    // action stand in a column beside the form instead of under it, so a short
    // window never scrolls the keypad or Save away.
    controlsBeside: Boolean = false,
    // The host sheet must refuse a swipe or scrim tap (a set loop in progress).
    locksSheet: Boolean = false,
    content: @Composable ColumnScope.() -> Unit,
) {
    val guard = LocalEditorGuard.current
    DisposableEffect(guard, dirty, locksSheet) {
        guard?.report(dirty, locksSheet)
        onDispose { guard?.release() }
    }
    var confirmDiscard by rememberSaveable { mutableStateOf(false) }
    // In-flight latch for the closing path: the host's Save callback is
    // `{ ev -> onEvent(ev); onDismiss() }`, and the sheet takes ~300 ms to hide. A
    // second tap in that window would log a second fresh entry_id (a plain
    // `remember`, so it resets when a new editor is mounted).
    var submitted by remember { mutableStateOf(false) }
    var lastCommitAt by remember { mutableStateOf<Long?>(null) }
    val haptic = LocalHapticFeedback.current
    val label = actionLabel ?: stringResource(Res.string.main_save)
    val enabled = saveEnabled && (danger || !submitted)
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
    val primary: @Composable () -> Unit = {
        PrimaryButton(
            label,
            commit,
            Modifier.fillMaxWidth().testTag("editor-primary"),
            enabled = enabled,
            minHeight = LargeControlHeight,
            danger = danger,
        )
    }
    // System Back is the close button: inside the host sheet or dialog, so the
    // discard confirm opens OVER the editor and "Keep editing" really keeps it.
    // A host that scrolls the editor itself keeps its own Back while clean.
    BackHandler(enabled = LocalEditorPinnedActions.current || dirty) { requestClose() }
    if (LocalEditorPinnedActions.current && controlsBeside && LocalWindowLayout.current.sideControls) {
        SideControlsEditor(title, requestClose, bottomExtra, primary, content)
    } else if (LocalEditorPinnedActions.current) {
        Column(modifier = Modifier.fillMaxWidth()) {
            EditorTitleRow(title, requestClose)
            val scroll = rememberScrollState()
            Box(modifier = Modifier.weight(1f, fill = false).fillMaxWidth()) {
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .verticalScroll(scroll)
                        .padding(top = Space.Sm.dp, bottom = Space.Lg.dp),
                    verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
                ) { content() }
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
            Column(
                modifier = Modifier.fillMaxWidth().padding(top = Space.Md.dp),
                verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
            ) {
                bottomExtra()
                primary()
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
            primary()
        }
    }
    if (confirmDiscard) {
        DiscardEntryDialog(onKeepEditing = { confirmDiscard = false }) {
            confirmDiscard = false
            onClose()
        }
    }
}

/**
 * [EditorScaffold] on a landscape window: the close · title row and the form
 * scroll in the start column; the end column holds the keypad the form opens
 * ([KeypadPanel] docks it there through [LocalKeypadDock]), [bottomExtra] and
 * the primary action, bottom-aligned in thumb reach. A short window (a phone in
 * landscape) has the height for the keypad and Save or for the extras and Save,
 * not all three: there the extras give way while a keypad is open, and the
 * keypad's Done brings them back. TalkBack reads the form first, then the
 * controls.
 */
@Composable
private fun SideControlsEditor(
    title: String,
    requestClose: () -> Unit,
    bottomExtra: @Composable ColumnScope.() -> Unit,
    primary: @Composable () -> Unit,
    content: @Composable ColumnScope.() -> Unit,
) {
    val dock = remember { SlotDock() }
    Row(
        modifier = Modifier.fillMaxWidth().fillMaxHeight().testTag("editor-side-by-side"),
        horizontalArrangement = Arrangement.spacedBy(Space.Lg.dp),
    ) {
        Column(Modifier.weight(1f).fillMaxHeight().traversalGroup()) {
            EditorTitleRow(title, requestClose)
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .weight(1f)
                    .verticalScroll(rememberScrollState())
                    .padding(top = Space.Sm.dp, bottom = Space.Lg.dp),
                verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
            ) {
                CompositionLocalProvider(LocalKeypadDock provides dock) { content() }
            }
        }
        Column(
            modifier = Modifier.weight(1f).fillMaxHeight().padding(top = Space.Md.dp).traversalGroup(),
            verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
        ) {
            Box(Modifier.fillMaxWidth().weight(1f), contentAlignment = Alignment.BottomCenter) {
                Column(
                    modifier = Modifier.fillMaxWidth().verticalScroll(rememberScrollState()),
                    verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
                ) {
                    val keypad = dock.content
                    keypad?.invoke()
                    if (keypad == null || !LocalWindowLayout.current.shortHeight) bottomExtra()
                }
            }
            primary()
        }
    }
}

/** "Discard this entry?" over a dirty editor: Discard (danger text) drops the
 *  entry via [onDiscard]; Keep editing, Back or an outside tap ([onKeepEditing])
 *  return to it. */
@Composable
internal fun DiscardEntryDialog(onKeepEditing: () -> Unit, onDiscard: () -> Unit) =
    ConfirmDialog(
        title = stringResource(Res.string.log_discard_title),
        body = null,
        confirmLabel = stringResource(Res.string.log_discard_confirm),
        onDismiss = onKeepEditing,
        onConfirm = onDiscard,
        confirmColor = DangerText,
        dismissLabel = stringResource(Res.string.log_keep_editing),
    )

/**
 * What an editor host guards, as its [EditorScaffold] reports it: unsaved edits
 * ([dirty]) and a set loop the host must not let go of ([locked]). Both clear
 * when the editor leaves.
 */
@Stable
internal class EditorGuard {
    var dirty by mutableStateOf(false)
        private set
    var locked by mutableStateOf(false)
        private set

    /** What a sheet close or an open intent must not drop. */
    val busy: Boolean get() = dirty || locked

    fun report(dirty: Boolean, locked: Boolean) {
        this.dirty = dirty
        this.locked = locked
    }

    fun release() = report(dirty = false, locked = false)
}

/** The [EditorGuard] of the sheet or dialog an editor sits in; null outside one. */
internal val LocalEditorGuard = staticCompositionLocalOf<EditorGuard?> { null }

/**
 * The bottom sheet that hosts editors (the Log sheet, History's edit sheet):
 * fully expanded, growing with the keyboard (the exercise search is the one
 * IME-summoning field), and guarding its editor through [guard]. A locked set
 * loop refuses to be swiped or scrim-tapped away; a dirty editor blocks the
 * swipe and raises "Discard this entry?" instead, the sheet staying visible
 * under the dialog so "Keep editing" leaves it in place. [content] gets the
 * animated close for after a save; [onClosed] runs once the sheet is gone.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun EditorSheet(
    guard: EditorGuard,
    onClosed: () -> Unit,
    content: @Composable ColumnScope.(close: () -> Unit) -> Unit,
) {
    var confirmDiscard by rememberSaveable { mutableStateOf(false) }
    val sheetState = rememberModalBottomSheetState(
        skipPartiallyExpanded = true,
        confirmValueChange = { target ->
            when {
                target != SheetValue.Hidden -> true
                guard.locked -> false
                guard.dirty -> {
                    confirmDiscard = true
                    false
                }
                else -> true
            }
        },
    )
    val scope = rememberCoroutineScope()
    val close: () -> Unit = {
        guard.release()
        scope.launch { sheetState.hide() }.invokeOnCompletion {
            if (!sheetState.isVisible) onClosed()
        }
    }
    // The guards above already kept a dirty or locked sheet up; by the time this
    // fires the hide is committed, so it only finishes the close.
    AppSheet(onDismiss = { guard.release(); onClosed() }, sheetState = sheetState, sheetGesturesEnabled = !guard.locked) {
        Column(Modifier.fillMaxWidth().imePadding()) {
            CompositionLocalProvider(LocalEditorGuard provides guard) { content(close) }
        }
    }
    if (confirmDiscard) {
        DiscardEntryDialog(onKeepEditing = { confirmDiscard = false }) {
            confirmDiscard = false
            guard.release()
            onClosed()
        }
    }
}

/** An editor's top row: `ui-close` and the centered title. The
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
                painterResource(Res.drawable.ic_ui_close),
                contentDescription = stringResource(Res.string.main_close),
                tint = OnBgBody,
                modifier = Modifier.size(24.dp),
            )
        }
        SheetTitle(title, Modifier.weight(1f), textAlign = TextAlign.Center)
        Spacer(Modifier.size(ActionMinHeight))
    }
}

/**
 * Backdating chip for every log editor ("Today · 14:32"), defaulting to now: a
 * date picker (no future dates), then a time picker with [withTime]; the result
 * is clamped to now. A date-only editor keeps the current time of day.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ObservedAtRow(
    epochSec: Long,
    withTime: Boolean,
    onChange: (Long) -> Unit,
) {
    // Device 12/24-hour preference (needs a Context) drives both the chip's rendered
    // time and the Material 3 time picker below, so the input and the label always agree.
    val use24h = is24HourClock(currentPlatformContext())
    var showDate by rememberSaveable { mutableStateOf(false) }
    var showTime by rememberSaveable { mutableStateOf(false) }
    var pendingDayUtcMillis by rememberSaveable { mutableStateOf<Long?>(null) }
    // The stamp a future pick was clamped to, so the row can say so instead of
    // silently moving it; a stamp set from outside (the next set) hides the note.
    var clampedTo by rememberSaveable { mutableStateOf<Long?>(null) }
    fun commit(dayMillis: Long, hour: Int, minute: Int) {
        val raw = localInstantSeconds(dayMillis, hour, minute)
        val now = AppClock.nowSec()
        clampedTo = if (raw > now) now else null
        onChange(minOf(raw, now))
    }

    Column(verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(stringResource(Res.string.log_logged), color = OnBgBody, style = Type.Body)
            val todayLabel = stringResource(Res.string.log_today)
            val dayStart = localDayStartSec(AppClock.nowSec())
            val shown = remember(epochSec, withTime, use24h, todayLabel, dayStart) {
                formatObservedAt(epochSec, withTime, use24h, todayLabel)
            }
            val changeLabel = stringResource(Res.string.log_logged_change_cd)
            Text(
                shown,
                color = Accent,
                style = Type.Body.merge(TabularFigures),
                modifier = Modifier
                    .heightIn(min = ActionMinHeight)
                    .clip(RoundedCornerShape(Space.Md.dp))
                    .background(BgTop)
                    .clickable(role = Role.Button, onClickLabel = changeLabel) { showDate = true }
                    .wrapContentHeight(Alignment.CenterVertically)
                    .padding(horizontal = Space.Card.dp, vertical = Space.Md.dp),
            )
        }
        // Always-composed single-line slot: empty until a future pick is clamped,
        // so surfacing the clamp never re-measures the sheet mid-edit.
        Text(
            if (clampedTo == epochSec) stringResource(Res.string.log_future_clamped) else "",
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
                it + TimeZone.currentSystemDefault().offsetAt(Instant.fromEpochMilliseconds(it)).totalSeconds * 1000L
            },
            selectableDates = object : SelectableDates {
                // Future dates blocked: DatePicker days are UTC-midnight stamps of
                // the picked calendar date, so the last selectable one is today's
                // local date at UTC midnight.
                override fun isSelectableDate(utcTimeMillis: Long): Boolean =
                    utcTimeMillis <= latestSelectableDayUtcMillis(AppClock.nowMillis())

                override fun isSelectableYear(year: Int): Boolean =
                    year <= Instant.fromEpochMilliseconds(AppClock.nowMillis()).toLocalDateTime(TimeZone.currentSystemDefault()).year
            },
        )
        AppDatePickerDialog(state, onDismiss = { showDate = false }, onConfirm = {
            val sel = state.selectedDateMillis
            showDate = false
            if (sel != null) {
                if (withTime) {
                    pendingDayUtcMillis = sel
                    showTime = true
                } else {
                    val cur = Instant.fromEpochSeconds(epochSec).toLocalDateTime(TimeZone.currentSystemDefault())
                    commit(sel, cur.hour, cur.minute)
                }
            }
        })
    }

    if (showTime) {
        val cur = Instant.fromEpochSeconds(epochSec).toLocalDateTime(TimeZone.currentSystemDefault())
        AppTimePickerDialog(
            title = stringResource(Res.string.log_logged_at),
            initialHour = cur.hour,
            initialMinute = cur.minute,
            is24Hour = use24h,
            onDismiss = { showTime = false },
        ) { hour, minute -> pendingDayUtcMillis?.let { day -> commit(day, hour, minute) } }
    }
}

/** The DatePicker stamp (UTC midnight) of today's LOCAL calendar date at
 *  [nowMillis]: the latest day a log may be dated. */
internal fun latestSelectableDayUtcMillis(nowMillis: Long, zone: TimeZone = TimeZone.currentSystemDefault()): Long =
    Instant.fromEpochMilliseconds(nowMillis).toLocalDateTime(zone).date
        .atStartOfDayIn(TimeZone.UTC).toEpochMilliseconds()

/** "Today · 14:32" / "15 Jul · 09:10" / date-only variants for the chip. The date
 *  uses the locale's day/month order ([localizedDayFormatter]) and the time the
 *  device 12/24-hour setting ([use24h], resolved from the Context by the caller). */
private fun formatObservedAt(epochSec: Long, withTime: Boolean, use24h: Boolean, todayLabel: String): String {
    val zone = TimeZone.currentSystemDefault()
    val then = Instant.fromEpochSeconds(epochSec).toLocalDateTime(zone)
    val today = Instant.fromEpochMilliseconds(AppClock.nowMillis()).toLocalDateTime(zone).date
    val day = if (then.date == today) todayLabel else localizedDayFormatter("MMMd").format(then)
    if (!withTime) return day
    return "$day · ${localizedDayFormatter(if (use24h) "Hmm" else "hmm").format(then)}"
}

/** A quiet disclosure row ("More", "More details"): an Accent label with the
 *  shared row chevron, turned down while [expanded]. A whole-row target,
 *  announced as expanded / collapsed. The caller renders the folded content. */
@Composable
internal fun ExpandRow(label: String, expanded: Boolean, onToggle: () -> Unit) =
    ChevronRow(onToggle, Modifier.clip(RoundedCornerShape(Space.Md.dp)), expanded, spacing = Space.Sm.dp) {
        Text(label, color = Accent, style = Type.BodyBold)
    }

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
