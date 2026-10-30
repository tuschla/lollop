package app.milestone

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.relocation.BringIntoViewRequester
import androidx.compose.foundation.relocation.bringIntoViewRequester
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.withFrameNanos
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
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.util.Locale

/** Corner shape of a raised keypad key (and the Done key under the grid). */
private val KeypadKeyShape = RoundedCornerShape(10.dp)

/** Longest buffer [editNumericBuffer] accepts. */
private const val NumericBufferMaxLength = 6

/** The smallest size (sp, the [Type.Title] size) a field's value figure steps
 *  down to before it wraps, and the step. */
private const val ValueFigureFloorSp = 20
private const val ValueFigureStepSp = 2

/** The value figure's style in [maxWidthPx]: [base] ([Type.Display]) while the
 *  figure fits one line, else stepped down to [ValueFigureFloorSp]; the flag is
 *  false when even the floor does not fit (the figure then wraps). Pure but for
 *  the measurer. */
private fun fittedValueStyle(
    measurer: androidx.compose.ui.text.TextMeasurer,
    text: String,
    base: TextStyle,
    maxWidthPx: Int,
): Pair<TextStyle, Boolean> {
    fun fits(s: TextStyle) = measurer.measure(text, s, maxLines = 1, softWrap = false).size.width <= maxWidthPx
    var sp = base.fontSize.value.toInt()
    var style = base
    while (!fits(style)) {
        if (sp <= ValueFigureFloorSp) return style to false
        sp = maxOf(ValueFigureFloorSp, sp - ValueFigureStepSp)
        style = base.copy(fontSize = sp.sp)
    }
    return style to true
}

/**
 * Big numeric value driven by the in-sheet [NumericKeypad] instead of the IME.
 * [text] is both the display and the submit source; the caller flags an
 * unparseable or out-of-range buffer as [invalid] (danger border + range hint)
 * and blocks its submit. [active] outlines the field the shared keypad edits;
 * tapping the field calls [onActivate] so a multi-field form can retarget it.
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
                // TalkBack hears the field name with its value ("Age, 35 years"), not
                // the bare number, and an empty field is not announced as "0".
                .keypadWell(invalid, active, if (text.isEmpty()) label else "$label, $text $unit", onActivate),
            horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            // The whole value always shows: Display while it fits one line, stepped
            // down to the Title size in a narrow well at a large font (a 6-digit
            // "127.52" in the Edit set dialog at 200 %), then wrapped.
            BoxWithConstraints(Modifier.weight(1f)) {
                val measurer = rememberTextMeasurer()
                // An empty buffer shows the "no value" dash, never a "0" that reads
                // as an entry (and submits nothing, so it must not show a number).
                // Light weight: a bold display dash reads as a minus sign.
                val shown = text.ifEmpty { EmptyValuePlaceholder }
                val base = Type.Display.merge(TabularFigures)
                    .let { if (text.isEmpty()) it.copy(fontWeight = FontWeight.Light) else it }
                val (style, oneLine) = remember(shown, base, constraints.maxWidth, measurer) {
                    fittedValueStyle(measurer, shown, base, constraints.maxWidth)
                }
                Text(
                    shown,
                    color = if (text.isEmpty()) OnBgFaint else OnBgBody,
                    style = style,
                    maxLines = if (oneLine) 1 else Int.MAX_VALUE,
                    softWrap = !oneLine,
                )
            }
            // Steps only from a real displayed value: an empty (or unparseable)
            // buffer disables the keys rather than inventing a value from the range floor.
            val base = parseKeypadNumber(text)
            adjustments.forEach { adj ->
                // Spoken as the action with its unit ("Add 2.5 kg"), not a bare "+2.5".
                val adjDesc = stringResource(
                    if (adj < 0) R.string.ui_cd_adjust_sub else R.string.ui_cd_adjust_add,
                    fmtLosslessPrefill(Math.abs(adj)),
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
            color = if (invalid) DangerText else hintColor,
            style = Type.Caption.merge(TabularFigures),
            minLines = 1,
            modifier = Modifier.padding(top = Space.Xs.dp),
        )
    }
}

/** The keypad field's recessed well: a constant hairline, so an unfocused field
 *  still reads as an editable box on any ground; [active] or [invalid] strengthen
 *  the outline. A tap calls [onActivate]; TalkBack hears [description]. */
@Composable
internal fun Modifier.keypadWell(invalid: Boolean, active: Boolean, description: String, onActivate: () -> Unit): Modifier {
    val status = LocalStatusColors.current
    val shape = RoundedCornerShape(Space.Card.dp)
    return clip(shape)
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
        .semantics { contentDescription = description }
        .padding(Space.Card.dp)
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
    Column(
        modifier = Modifier
            .testTag("numeric-keypad")
            .fillMaxWidth()
            .clip(RoundedCornerShape(Space.Card.dp))
            // The palette's recessed ground, so it follows the resolved theme.
            .background(BgSunken)
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
                modifier = Modifier.fillMaxWidth().keypadKey(onDone),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    stringResource(R.string.log_keypad_done),
                    color = Accent,
                    style = Type.BodyBold,
                )
            }
        }
    }
}

/** [NumericKeypad] editing the buffer [text]: each key or backspace reports the
 *  new buffer to [onBuffer]; [fresh] makes the first key replace the old value. */
@Composable
fun NumericKeypad(
    text: String,
    fresh: Boolean,
    onBuffer: (String) -> Unit,
    allowDecimal: Boolean = true,
    onDone: (() -> Unit)? = null,
) = NumericKeypad(
    onKey = { onBuffer(editNumericBuffer(text, it, fresh)) },
    onBackspace = { onBuffer(text.dropLast(1)) },
    allowDecimal = allowDecimal,
    onDone = onDone,
)

/** Shows [content] (a keypad) while [open] and scrolls it into view when it
 *  opens, then [alsoReveal] (the field it types into), so a short window never
 *  leaves the keypad below the fold. */
@Composable
fun KeypadPanel(
    open: Boolean,
    alsoReveal: BringIntoViewRequester? = null,
    content: @Composable () -> Unit,
) {
    if (!open) return
    val requester = remember { BringIntoViewRequester() }
    LaunchedEffect(Unit) {
        withFrameNanos { }
        requester.bringIntoView()
        alsoReveal?.bringIntoView()
    }
    Box(Modifier.bringIntoViewRequester(requester)) { content() }
}

/** One raised [NumericKeypad] key: a digit, the decimal point or backspace. */
@Composable
private fun KeypadKey(key: Char, onKey: (Char) -> Unit, onBackspace: () -> Unit, modifier: Modifier) {
    Box(
        modifier = modifier.keypadKey { if (key == '⌫') onBackspace() else onKey(key) },
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

/** A raised keypad key's chrome: ≥48dp tall, rounded, on the elevated ground
 *  (a theme colour, so it reads the composition). */
@Composable
private fun Modifier.keypadKey(onClick: () -> Unit): Modifier = this
    .heightIn(min = ActionMinHeight)
    .clip(KeypadKeyShape)
    .background(BgElevated)
    .clickable(role = Role.Button, onClick = onClick)
    .padding(vertical = Space.Md.dp)

/**
 * Apply one keypad key to a numeric text buffer. `replaceAll` implements
 * calculator-style entry: the first key after (re)activating a field starts a
 * fresh number. A second decimal point is ignored, a leading '.' becomes "0.",
 * a digit replaces a lone "0", and the length is capped at
 * [NumericBufferMaxLength]. Parsing and validation stay with the caller.
 */
fun editNumericBuffer(text: String, key: Char, replaceAll: Boolean): String {
    val base = if (replaceAll) "" else text
    if (base.length >= NumericBufferMaxLength) return base
    if (key == '.') {
        if (base.contains('.')) return base
        return if (base.isEmpty()) "0." else "$base."
    }
    if (base == "0") return "$key"
    return base + key
}

/** `+2.5` / `−5` / `+0.01` label for a relative-adjust chip, at the step's own
 *  precision ([trimDecimal] would print a 0.01 m/s step as "0"). */
private fun adjLabel(a: Double): String = (if (a < 0) "−" else "+") + fmtLosslessPrefill(Math.abs(a))
