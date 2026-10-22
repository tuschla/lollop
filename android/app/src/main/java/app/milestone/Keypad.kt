package app.milestone

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import java.util.Locale

/** Corner shape of a raised keypad key (and the Done key under the grid). */
private val KeypadKeyShape = RoundedCornerShape(10.dp)

/** Longest buffer [editNumericBuffer] accepts, so the display value never overflows. */
private const val NumericBufferMaxLength = 6

/**
 * Big numeric value driven by the in-sheet [NumericKeypad] instead of the IME.
 * The text buffer IS the display AND the submit source, the caller parses
 * exactly what is shown, so the
 * display-committed invariant holds structurally: an unparseable or
 * out-of-range buffer flags [invalid] (danger border + range hint) and the
 * caller blocks its submit. The one-tap relative-adjust chips stay.
 * [active] outlines the field the shared keypad currently edits; tapping the
 * field calls [onActivate] so a multi-field form can switch targets.
 */
@Composable
fun KeypadValueField(
    label: String,
    unit: String,
    text: String,
    active: Boolean,
    invalid: Boolean,
    min: Double,
    max: Double,
    format: String,
    adjustments: List<Double>,
    onActivate: () -> Unit,
    onText: (String) -> Unit,
    // Caller-owned note for the field's one reserved caption slot (e.g. "why
    // Save is off"); the range hint for an [invalid] buffer takes precedence.
    hint: String? = null,
    hintColor: Color = OnBgMuted,
    // A tappable unit ("bpm ⇄") that swaps how the value is entered (e.g. to % of
    // max), spoken as [unitClickDescription]; null keeps the unit plain text.
    onUnitClick: (() -> Unit)? = null,
    unitClickDescription: String? = null,
) {
    val status = LocalStatusColors.current
    val shape = RoundedCornerShape(Space.Card.dp)
    Column(Modifier.fillMaxWidth()) {
        FieldLabel(
            label,
            if (onUnitClick != null) "$unit ⇄" else unit,
            onTrailing = onUnitClick,
            trailingDescription = unitClickDescription,
        )
        Spacer(Modifier.height(Space.Md.dp))
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clip(shape)
                // A recessed well with a constant hairline, so an unfocused field
                // still reads as an editable box (not a static readout) on any
                // card ground; focus/invalid only strengthen the outline.
                .background(BgSunken)
                .border(
                    when {
                        invalid -> BorderStroke(1.5.dp, status.danger)
                        active -> BorderStroke(1.5.dp, Accent)
                        else -> BorderStroke(1.dp, ChoiceCellBorder)
                    },
                    shape,
                )
                .clickable(role = Role.Button) { onActivate() }
                // TalkBack hears the field name with its value ("Age, 35 years"), not
                // the bare number, and an empty field is not announced as "0".
                .semantics { contentDescription = if (text.isEmpty()) label else "$label, $text $unit" }
                .padding(Space.Card.dp),
            horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                // An empty buffer shows the "no value" dash, never a "0" that reads
                // as an entry (and submits nothing, so it must not show a number).
                // Light weight: a bold display dash reads as a minus sign.
                text.ifEmpty { EmptyValuePlaceholder },
                color = if (text.isEmpty()) OnBgFaint else OnBgBody,
                style = Type.Display.merge(TabularFigures)
                    .let { if (text.isEmpty()) it.copy(fontWeight = FontWeight.Light) else it },
                maxLines = 1,
                modifier = Modifier.weight(1f),
            )
            // Steps only from a real displayed value: an empty (or unparseable)
            // buffer disables the keys rather than inventing a value from the range floor.
            val base = parseKeypadNumber(text)
            adjustments.forEach { adj ->
                // Spoken as the action with its unit ("Add 2.5 kg"), not a bare "+2.5".
                val adjDesc = stringResource(
                    if (adj < 0) R.string.ui_cd_adjust_sub else R.string.ui_cd_adjust_add,
                    stepText(adj),
                    unit,
                )
                // The clickable node itself spans ≥48×48dp (not a smaller chip with
                // extended touch bounds, which the well's clip can cut at a large
                // font scale); the raised key is drawn centred inside it.
                Box(
                    modifier = Modifier
                        .sizeIn(minWidth = ActionMinHeight, minHeight = ActionMinHeight)
                        .clip(ChoiceCellShape)
                        .semantics { contentDescription = adjDesc }
                        .clickable(enabled = base != null, role = Role.Button) {
                            if (base == null) return@clickable
                            onActivate()
                            onText(
                                String.format(
                                    Locale.US,
                                    format,
                                    (base + adj).coerceIn(min, max),
                                ),
                            )
                        },
                    contentAlignment = Alignment.Center,
                ) {
                    // The one choice-cell look (4% wash + hairline, 40dp visual) in
                    // every calculator and the set editor; a disabled step (empty
                    // field) stays flat, so it can't pass for a live key.
                    Box(
                        modifier = Modifier
                            .heightIn(min = ChipVisualHeight)
                            .widthIn(min = ChipVisualHeight)
                            .then(if (base != null) Modifier.choiceCell(selected = false) else Modifier)
                            .padding(horizontal = Space.Md.dp),
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(
                            adjLabel(adj),
                            color = if (base != null) OnBgBody else OnBgFaint,
                            style = Type.Chip.merge(TabularFigures),
                        )
                    }
                }
            }
        }
        // Always composed so the field's height is validity-independent: a hint
        // appearing/disappearing must not re-measure the sheet (it re-animates
        // to the new content height, "zipping" under the user's finger). It hugs
        // the well (Space.Xs, not the label gap) so it reads as the well's own
        // footnote, and an empty slot costs one caption line, not a dead band.
        // Unbounded lines: a caller note that wraps at a large font scale is shown
        // whole, never truncated.
        Text(
            when {
                invalid -> stringResource(
                    R.string.ui_keypad_range_hint,
                    String.format(Locale.US, format, min),
                    String.format(Locale.US, format, max),
                    unit,
                )
                else -> hint.orEmpty()
            },
            color = if (invalid) status.danger else hintColor,
            style = Type.Caption.merge(TabularFigures),
            minLines = 1,
            modifier = Modifier.padding(top = Space.Xs.dp),
        )
    }
}

/**
 * In-form 3×4 numeric keypad (1–9, decimal point, 0, backspace) for the log
 * editors, big fixed targets that never scroll away under an IME. Emits raw
 * keys; the caller owns the buffer (and its validation), so the keypad itself
 * carries no numeric logic at all. [allowDecimal] = false (whole-number and
 * digit-only fields: age, heart rate, days, a race time) leaves the decimal
 * cell empty, so no key is drawn that does nothing. [onDone] adds a full-width
 * "Done" key under the grid that folds the keypad away.
 */
@Composable
fun NumericKeypad(onKey: (Char) -> Unit, onBackspace: () -> Unit, allowDecimal: Boolean = true, onDone: (() -> Unit)? = null) {
    // Keypad ground (spec 05-log §2): the palette's recessed [bgSunken] surface —
    // a hair off BgElevated in the dark themes, the plain ground on light — so it
    // follows the RESOLVED palette (a forced-Light-on-dark-OS sheet gets the light
    // ground) without an OS/luminance probe.
    val ground = BgSunken
    Column(
        modifier = Modifier
            .testTag("numeric-keypad")
            .fillMaxWidth()
            .clip(RoundedCornerShape(Space.Card.dp))
            .background(ground)
            .padding(Space.Sm.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
    ) {
        val rows = listOf("123", "456", "789", ".0⌫")
        rows.forEach { row ->
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
            ) {
                row.forEach { key ->
                    if (key == '.' && !allowDecimal) {
                        // Keeps the 3-column grid; nothing to tap or announce.
                        Spacer(Modifier.weight(1f))
                    } else {
                        KeypadKey(key, onKey, onBackspace, Modifier.weight(1f))
                    }
                }
            }
        }
        if (onDone != null) {
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = ActionMinHeight)
                    .clip(KeypadKeyShape)
                    .background(BgElevated)
                    .clickable(role = Role.Button, onClick = onDone)
                    .padding(vertical = Space.Md.dp),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    stringResource(R.string.log_keypad_done),
                    color = Accent,
                    style = Type.Body.copy(fontWeight = FontWeight.Bold),
                )
            }
        }
    }
}

/** One raised [NumericKeypad] key: a digit, the decimal point or backspace. */
@Composable
private fun KeypadKey(key: Char, onKey: (Char) -> Unit, onBackspace: () -> Unit, modifier: Modifier) {
    Box(
        modifier = modifier
            .heightIn(min = ActionMinHeight)
            .clip(KeypadKeyShape)
            .background(BgElevated)
            .clickable(role = Role.Button) { if (key == '⌫') onBackspace() else onKey(key) }
            .padding(vertical = Space.Md.dp),
        contentAlignment = Alignment.Center,
    ) {
        if (key == '⌫') {
            Icon(
                painterResource(R.drawable.ic_ui_backspace),
                contentDescription = stringResource(R.string.ui_cd_delete_digit),
                tint = OnBgMuted,
                modifier = Modifier.size(22.dp),
            )
        } else {
            val decimalDesc = stringResource(R.string.ui_cd_decimal_point)
            Text(
                "$key",
                color = if (key == '.') OnBgMuted else OnBgBody,
                style = Type.KeypadKey,
                modifier = if (key == '.') {
                    Modifier.semantics { contentDescription = decimalDesc }
                } else {
                    Modifier
                },
            )
        }
    }
}

/**
 * Apply one keypad key to a numeric text buffer. `replaceAll` implements
 * calculator-style entry: the first digit after (re)activating a field starts
 * a fresh number instead of appending to the old one. A second decimal point
 * is ignored; a leading '.' becomes "0."; length is capped so the display
 * can't overflow. Pure string editing, parsing/validation stay with the caller.
 */
fun editNumericBuffer(text: String, key: Char, replaceAll: Boolean): String {
    val base = if (replaceAll) "" else text
    if (key == '.') {
        if (base.contains('.')) return base
        return if (base.isEmpty()) "0." else "$base."
    }
    if (base.length >= NumericBufferMaxLength) return base
    return base + key
}

/** `+2.5` / `−5` / `+0.01` label for a relative-adjust chip. */
private fun adjLabel(a: Double): String = (if (a < 0) "−" else "+") + stepText(a)

/** A step's magnitude at its own precision ("0.01", "2.5", "5"): [trimDecimal]'s
 *  one-decimal rounding would print a 0.01 m/s step as "0". */
private fun stepText(a: Double): String =
    java.math.BigDecimal.valueOf(Math.abs(a)).stripTrailingZeros().toPlainString()
