package app.milestone

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.wrapContentWidth
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.BasicAlertDialog
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDefaults
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.DatePickerState
import androidx.compose.material3.DisplayMode
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.TimePicker
import androidx.compose.material3.rememberTimePickerState
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.DialogProperties
import app.milestone.resources.Res
import app.milestone.resources.log_ok
import app.milestone.resources.main_cancel
import app.milestone.resources.main_clear
import app.milestone.resources.main_training_age_row
import org.jetbrains.compose.resources.stringResource

/** The elevated content card (hairline outline, no shadow); with [onClick] the
 *  whole card is one tappable surface, a button to TalkBack. [contentPadding] and
 *  [spacing] default to the card recipe; edge-to-edge row lists ([ActionRows], a
 *  hairline grid) pass zero and draw their own row padding. [selected] (a list
 *  card whose entry is open in the detail pane beside the list) draws a
 *  [SelectedBarWidth] `Accent` bar along the start edge and is spoken as
 *  selected; the other cards carry no selection state, so TalkBack does not
 *  read "not selected" on each. The card's ground and text stay as they are, so
 *  every tone keeps its contrast. */
@Composable
internal fun PlainCard(
    onClick: (() -> Unit)? = null,
    contentPadding: PaddingValues = PaddingValues(Space.Card.dp),
    spacing: Dp = Space.Md.dp,
    selected: Boolean = false,
    content: @Composable () -> Unit,
) {
    val border = BorderStroke(1.dp, OnBgBody.copy(alpha = HairlineAlpha.Card))
    val shape = RoundedCornerShape(Space.Card.dp)
    val colors = CardDefaults.cardColors(containerColor = BgElevated)
    val elevation = CardDefaults.cardElevation(defaultElevation = 0.dp)
    val body: @Composable () -> Unit = {
        Box(Modifier.fillMaxWidth()) {
            Column(
                Modifier.padding(contentPadding),
                verticalArrangement = Arrangement.spacedBy(spacing),
            ) { content() }
            // The card's surface clips the bar to its rounded start corners.
            if (selected) {
                Box(
                    Modifier
                        .matchParentSize()
                        .wrapContentWidth(Alignment.Start)
                        .width(SelectedBarWidth)
                        .background(Accent),
                )
            }
        }
    }
    // Clickable variant uses the Material 3 `Card(onClick=…)` overload so the tap ripple
    // is clipped by the card's own rounded surface at any height (no soft-edge
    // leak on tall/expanded cards); the static variant is a plain Card.
    if (onClick != null) {
        Card(
            onClick = onClick,
            colors = colors,
            elevation = elevation,
            shape = shape,
            border = border,
            // Material's clickable Card sets no role.
            modifier = Modifier.fillMaxWidth().semantics {
                role = Role.Button
                if (selected) this.selected = true
            },
        ) { body() }
    } else {
        Card(
            colors = colors,
            elevation = elevation,
            shape = shape,
            border = border,
            modifier = Modifier.fillMaxWidth(),
        ) { body() }
    }
}

/** A small status chip: white [Type.Chip] text on the [bg] fill (SAFETY, SPIKE, PAUSED…). */
@Composable
internal fun Chip(text: String, bg: Color) {
    Text(
        text,
        color = OnStatusFill,
        style = Type.Chip,
        modifier = Modifier
            .background(bg, SegmentShape)
            .chipPadding(),
    )
}

internal fun Modifier.chipPadding(): Modifier = padding(horizontal = Space.Md.dp, vertical = Space.Sm.dp - 1.dp)

/**
 * One compact Reference row (02-coach §6): the claim title + its citation
 * line. Deliberately NOT an EvidenceCard: the full graded card is already
 * rendered wherever the claim was recommended; Reference only lists sources.
 */
@Composable
internal fun ReferenceRow(row: GuidanceView) {
    Column(verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
        Text(
            guidanceTitle(row),
            color = OnBgBody,
            style = Type.BodyBold,
        )
        Text(citationLabel(row.citation), color = OnBgFaint, style = Type.Caption)
    }
}

/** A guidance row's title: the training-age row in the copy deck's grammar
 *  ("Training age · Intermediate"), every other row its summary. */
@Composable
internal fun guidanceTitle(row: GuidanceView): String =
    if (row.training_age.isNotEmpty()) stringResource(Res.string.main_training_age_row, row.training_age) else row.summary

/** The app's [AlertDialog]: the card shape, and a [traversalGroup] so an RTL
 *  locale hears the buttons start to end. Every dialog window goes through it,
 *  [AppDatePickerDialog] or [AppTimePickerDialog]. */
@Composable
internal fun AppDialog(
    onDismiss: () -> Unit,
    confirmButton: @Composable () -> Unit,
    dismissButton: (@Composable () -> Unit)? = null,
    title: (@Composable () -> Unit)? = null,
    text: (@Composable () -> Unit)? = null,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        confirmButton = confirmButton,
        modifier = Modifier.traversalGroup(),
        dismissButton = dismissButton,
        title = title,
        text = text,
        shape = RoundedCornerShape(Space.Card.dp),
    )
}

/** A [DatePicker] on [state] in an [AppDialog]-styled window; a short window (a
 *  phone in landscape, [WindowLayout.shortHeight]) gets [ShortDatePickerDialog]. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun AppDatePickerDialog(
    state: DatePickerState,
    onDismiss: () -> Unit,
    onConfirm: () -> Unit,
    confirmEnabled: Boolean = true,
) {
    val confirm = @Composable {
        TextButton(onClick = onConfirm, enabled = confirmEnabled) { Text(stringResource(Res.string.log_ok)) }
    }
    val dismiss = @Composable { TextButton(onClick = onDismiss) { Text(stringResource(Res.string.main_cancel)) } }
    if (LocalWindowLayout.current.shortHeight) {
        ShortDatePickerDialog(state, onDismiss, confirm, dismiss)
        return
    }
    DatePickerDialog(
        onDismissRequest = onDismiss,
        confirmButton = confirm,
        modifier = Modifier.traversalGroup(),
        dismissButton = dismiss,
        shape = RoundedCornerShape(Space.Card.dp),
    ) {
        DatePicker(state)
    }
}

/**
 * [AppDatePickerDialog] on a short window. The month grid is taller than the
 * window and ran under Cancel/OK, so the picker is the typed date field alone:
 * no calendar toggle, no title and no headline (the field shows the date). Its
 * keyboard leaves ~130dp of a landscape phone: the dialog window resizes for it
 * (the platform panned it, OK under the keyboard) and Cancel/OK stand beside the
 * field, as a landscape editor's controls do, so the height is the field's:
 * label, entry box and error line. Under the buttons and the headline ~70dp were
 * left: the error line, which says why OK is off, sat under the window (e37
 * emulator). The field still scrolls at a large font.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ShortDatePickerDialog(
    state: DatePickerState,
    onDismiss: () -> Unit,
    confirm: @Composable () -> Unit,
    dismiss: @Composable () -> Unit,
) {
    // Before the picker reads it, so a short window never draws the grid (also
    // after a rotation with the dialog open).
    if (state.displayMode != DisplayMode.Input) state.displayMode = DisplayMode.Input
    BasicAlertDialog(
        onDismissRequest = onDismiss,
        modifier = Modifier.traversalGroup(),
        properties = DialogProperties(usePlatformDefaultWidth = false),
    ) {
        DialogResizesForKeyboard()
        Surface(
            shape = RoundedCornerShape(Space.Card.dp),
            color = DatePickerDefaults.colors().containerColor,
            tonalElevation = DatePickerDefaults.TonalElevation,
        ) {
            Row {
                Column(Modifier.width(DateFieldDialogWidth).verticalScroll(rememberScrollState()).testTag("date-picker-scroll")) {
                    DatePicker(state, title = null, headline = null, showModeToggle = false)
                }
                Column(
                    Modifier.align(Alignment.Bottom).padding(end = Space.Md.dp, bottom = Space.Md.dp),
                    horizontalAlignment = Alignment.End,
                ) {
                    dismiss()
                    confirm()
                }
            }
        }
    }
}

/** Width of the typed date picker in a short window's date dialog, Cancel/OK
 *  beside it: Material's date picker dialog width. */
private val DateFieldDialogWidth = 360.dp

/** A [TimePicker] titled [title] in an [AppDialog]: OK closes through
 *  [onDismiss] and sends the picked hour and minute to [onConfirm]. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun AppTimePickerDialog(
    title: String,
    initialHour: Int,
    initialMinute: Int,
    is24Hour: Boolean,
    onDismiss: () -> Unit,
    onConfirm: (hour: Int, minute: Int) -> Unit,
) {
    val state = rememberTimePickerState(initialHour = initialHour, initialMinute = initialMinute, is24Hour = is24Hour)
    AppDialog(
        onDismiss = onDismiss,
        confirmButton = {
            TextButton(onClick = {
                onDismiss()
                onConfirm(state.hour, state.minute)
            }) { Text(stringResource(Res.string.log_ok)) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(Res.string.main_cancel)) } },
        title = { Text(title) },
        text = { TimePicker(state) },
    )
}

/** A confirmation [AppDialog] (chrome §6): [title], an optional [body], a
 *  dismiss button ([dismissLabel]) and one confirm button in [confirmColor]
 *  (`DangerText` when destructive, else the button's own colour), disabled while
 *  not [confirmEnabled]. Each button runs exactly its own callback. */
@Composable
internal fun ConfirmDialog(
    title: String,
    body: String?,
    confirmLabel: String,
    onDismiss: () -> Unit,
    onConfirm: () -> Unit,
    confirmColor: Color = Color.Unspecified,
    confirmEnabled: Boolean = true,
    dismissLabel: String = stringResource(Res.string.main_cancel),
) {
    AppDialog(
        onDismiss = onDismiss,
        confirmButton = {
            TextButton(onClick = onConfirm, enabled = confirmEnabled) { Text(confirmLabel, color = confirmColor) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(dismissLabel) } },
        title = { Text(title) },
        text = body?.let { { Text(it) } },
    )
}

/** The irreversible-clear confirmation (remove a plan or report, a hold
 *  resolution): a [ConfirmDialog] whose confirm closes it, then clears.
 *  [confirmColor] is `DangerText` unless the confirm is not destructive. */
@Composable
internal fun ClearConfirmDialog(
    title: String,
    message: String,
    confirmLabel: String = stringResource(Res.string.main_clear),
    confirmColor: Color = DangerText,
    onDismiss: () -> Unit,
    onClear: () -> Unit,
) {
    ConfirmDialog(
        title = title,
        body = message,
        confirmLabel = confirmLabel,
        onDismiss = onDismiss,
        onConfirm = {
            onDismiss()
            onClear()
        },
        confirmColor = confirmColor,
    )
}

/** One ≥48dp key/value row (set detail facts, calculator results): the label
 *  muted at the start, the value at the end, Bold when [emphasis]. Each takes its
 *  own share of the line, so a large font scale wraps them in place rather than
 *  overlapping. One semantics node, read label then value. */
@Composable
internal fun KeyValueRow(label: String, value: String, emphasis: Boolean = false) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = ActionMinHeight)
            .padding(vertical = Space.Sm.dp)
            .semantics(mergeDescendants = true) {},
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        Text(label, color = OnBgMuted, style = Type.Body, modifier = Modifier.weight(1f))
        Text(
            value,
            color = OnBgBody,
            style = (if (emphasis) Type.BodyBold else Type.Body).merge(TabularFigures),
            textAlign = TextAlign.End,
            modifier = Modifier.weight(1f),
        )
    }
}

/** The core's citation verbatim; "-" when blank. */
internal fun citationLabel(citation: String): String =
    citation.ifBlank { "-" }
