package app.milestone

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.graphics.painter.Painter
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.TimeZone

/** Tone for text/icons sitting on the [Accent] fill (owner ruling + spec
 *  "Accent fill + BgTop text"): the active theme's `bgTop`, the dark
 *  #141210 on the dark palettes (white fails AA on the mid-tone accents,
 *  3.1:1 on Beton's #E0733A vs 5.9:1 for this) and warm paper on the light
 *  palettes, whose accents are darkened for contrast on paper. */
internal val OnAccent: Color @Composable get() = LocalPalette.current.bgTop

/**
 * Full-width list of exclusive options: one comfortable (≥48dp) whole-row tap
 * target per option, the selected row tinted + check-marked so the current state
 * is unmissable. Replaces the old anchored dropdown-menu pickers, which were
 * tiny, easy to fumble, and clipped long option lists.
 *
 * [display] only affects the on-screen text; callers that serialize the choice
 * still key off the value itself (e.g. an enum's `.name`), so a friendly label
 * here never touches the wire contract.
 *
 * [divideBefore] draws a divider above the first option for which it returns
 * true, so a caller can visually fence a subgroup, e.g. the readiness picker
 * separates the medical red-flag signals from the routine metrics, since
 * mis-tapping a red flag has outsized (safety) consequences.
 */
@Composable
fun <T> OptionList(
    options: List<T>,
    current: T,
    display: (T) -> String,
    divideBefore: (T) -> Boolean = { false },
    onSelect: (T) -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(Space.Md.dp))
            .background(BgTop),
    ) {
        options.forEach { value ->
            if (divideBefore(value)) HorizontalDivider(color = OnBgFaint.copy(alpha = 0.3f))
            val selected = value == current
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = 48.dp)
                    .background(if (selected) Accent.copy(alpha = 0.14f) else Color.Transparent)
                    .clickable { onSelect(value) }
                    .padding(horizontal = Space.Card.dp, vertical = Space.Md.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    display(value),
                    color = if (selected) Accent else OnBgBody,
                    style = Type.Body,
                    modifier = Modifier.weight(1f),
                )
                if (selected) {
                    Icon(
                        painterResource(R.drawable.ic_ui_check),
                        contentDescription = "Selected",
                        tint = Accent,
                        modifier = Modifier.size(18.dp),
                    )
                }
            }
        }
    }
}

/**
 * Label + enum picker as a tap-row that expands into a full-width [OptionList].
 * The header row (label left, current value + chevron right) is a ≥48dp
 * whole-row tap target; opening it reveals every option at once with the same
 * generous targets, and picking one collapses the list. Same wire contract as
 * ever, only the value's `.name` is serialized by callers.
 */
@Composable
fun <T : Enum<T>> EnumRow(
    label: String,
    values: List<T>,
    current: T,
    display: (T) -> String = { it.name },
    divideBefore: (T) -> Boolean = { false },
    onSelect: (T) -> Unit,
) {
    var expanded by rememberSaveable { mutableStateOf(false) }
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = 48.dp)
                .clip(RoundedCornerShape(Space.Md.dp))
                .clickable { expanded = !expanded },
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(label, color = OnBgBody, style = Type.Body)
            Row(
                horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(display(current), color = Accent, style = Type.Body)
                RowChevron(expanded)
            }
        }
        if (expanded) {
            OptionList(values, current, display, divideBefore) {
                onSelect(it)
                expanded = false
            }
        }
    }
}

/**
 * Label + segmented buttons for a short exclusive enum (design/usability-ia-spec §2:
 * 2–5 options). Every option is visible at once and one tap selects any of them -
 * the "recognition over recall" win a dropdown can't give for a handful of choices,
 * with the current selection always obvious. Use [EnumRow] for longer sets (6+) or
 * where a subgroup divider is needed (e.g. the readiness red-flag fence).
 *
 * [display] only affects the on-screen text; the wire contract still keys off the
 * variant (`.name`), so a friendly label never touches serialization.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun <T : Enum<T>> SegmentedEnumRow(
    label: String,
    values: List<T>,
    current: T,
    display: (T) -> String = { it.name },
    onSelect: (T) -> Unit,
) {
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
    ) {
        Text(label, color = OnBgBody, style = Type.Body)
        SingleChoiceSegmentedButtonRow(modifier = Modifier.fillMaxWidth()) {
            // Clay accent for the selected segment, not Material3's default
            // secondaryContainer lavender (owner rule: short enums = segmented
            // buttons, selected = Accent fill + BgTop text). Matches TwoSegmentRow.
            val segColors = SegmentedButtonDefaults.colors(
                activeContainerColor = Accent,
                activeContentColor = OnAccent,
                activeBorderColor = Accent,
                inactiveContainerColor = BgTop,
                inactiveContentColor = OnBgMuted,
                inactiveBorderColor = OnBgFaint.copy(alpha = 0.3f),
            )
            values.forEachIndexed { index, value ->
                SegmentedButton(
                    selected = value == current,
                    onClick = { onSelect(value) },
                    colors = segColors,
                    shape = SegmentedButtonDefaults.itemShape(index = index, count = values.size),
                ) {
                    Text(display(value), style = Type.Chip, maxLines = 1)
                }
            }
        }
    }
}

/** Label + −/value/+ stepper over integers, clamped to [min, max]. */
@Composable
fun IntStepperRow(
    label: String,
    value: Int,
    min: Int,
    max: Int,
    step: Int,
    onChange: (Int) -> Unit,
) {
    StepperShell(
        label = label,
        text = "$value",
        canDecrement = value > min,
        canIncrement = value < max,
        onDecrement = { onChange((value - step).coerceAtLeast(min)) },
        onIncrement = { onChange((value + step).coerceAtMost(max)) },
    )
}

/**
 * Label + −/value/+ stepper over doubles, clamped to [min, max]. Used where the
 * step is fractional (e.g. 2.5 kg plate jumps, 0.5-RPE granularity) so an Int
 * stepper can't express it.
 */
@Composable
fun DoubleStepperRow(
    label: String,
    value: Double,
    min: Double,
    max: Double,
    step: Double,
    format: String = "%.1f",
    onChange: (Double) -> Unit,
) {
    StepperShell(
        label = label,
        // Force a '.' decimal so the stepper matches the core's '.'-formatted
        // values on the same screen (the device locale could otherwise render ',').
        text = String.format(Locale.US, format, value),
        canDecrement = value > min,
        canIncrement = value < max,
        onDecrement = { onChange(snapToStep(value - step, min, step).coerceAtLeast(min)) },
        onIncrement = { onChange(snapToStep(value + step, min, step).coerceAtMost(max)) },
    )
}

/**
 * Snap a double back onto the `min + n·step` grid. Repeated ±step on a
 * fractional step (e.g. the 0.01 m/s velocity stepper) accumulates binary
 * floating-point drift, so a value the user reads as "0.06" can actually hold
 * 0.06000000000000001, enough to trip a core threshold defined as `> 0.06`.
 * Rounding to the nearest grid point after each step keeps the stored value
 * exactly what the display shows, so it means the same thing to the core.
 */
private fun snapToStep(value: Double, min: Double, step: Double): Double =
    min + Math.round((value - min) / step) * step

@Composable
private fun StepperShell(
    label: String,
    text: String,
    canDecrement: Boolean,
    canIncrement: Boolean,
    onDecrement: () -> Unit,
    onIncrement: () -> Unit,
) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, color = OnBgBody, style = Type.Body)
        Row(
            horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            OutlinedButton(onClick = onDecrement, enabled = canDecrement) { Text("−") }
            Text(
                text,
                color = OnBgBody,
                style = Type.Body.merge(TabularFigures),
                textAlign = TextAlign.Center,
                modifier = Modifier.width(48.dp),
            )
            OutlinedButton(onClick = onIncrement, enabled = canIncrement) { Text("+") }
        }
    }
}

// --- Redesign entry primitives (design import: "no dropdowns / no −+ steppers") ---
// Recognition over recall for the logging forms: every choice is visible and one
// tap picks it, replacing the −/+ steppers (which hid neighbours and needed N taps).
// These carry no coaching logic: they only gather the raw inputs the core derives
// e1RM / RIR / zone from.

/** Small uppercase field caption with an optional right-aligned live value hint
 *  (e.g. `REPS … 5`, `RPE … 8.0 · RIR 2`). */
@Composable
fun FieldLabel(label: String, trailing: String? = null) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.Bottom,
    ) {
        Text(label.uppercase(Locale.US), color = OnBgMuted, style = Type.Chip.copy(letterSpacing = 1.2.sp))
        if (trailing != null) Text(trailing, color = OnBgFaint, style = Type.Caption.merge(TabularFigures))
    }
}

/**
 * Width-filling row of discrete choices, every option visible, one tap to pick,
 * the selection filled with the accent. For small fixed sets (RPE, HR%) where the
 * whole scale fits the screen. Use [ScrollableScaleRow] when the set is longer.
 */
@Composable
fun <T> ChoiceScaleRow(
    options: List<T>,
    current: T,
    render: (T) -> String,
    onSelect: (T) -> Unit,
) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
    ) {
        options.forEach { opt ->
            val selected = opt == current
            Box(
                modifier = Modifier
                    .weight(1f)
                    .heightIn(min = 48.dp)
                    .clip(RoundedCornerShape(6.dp))
                    .background(if (selected) Accent else BgElevated)
                    .clickable { onSelect(opt) }
                    .padding(vertical = Space.Md.dp),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    render(opt),
                    color = if (selected) OnAccent else OnBgMuted,
                    style = Type.Body.merge(TabularFigures),
                    maxLines = 1,
                )
            }
        }
    }
}

/**
 * Horizontally-scrollable scale of discrete choices for a longer bounded set
 * (e.g. reps 1–20): every value is reachable by scroll + one tap, no stepper
 * cycling, the current value filled with the accent.
 */
@Composable
fun <T> ScrollableScaleRow(
    options: List<T>,
    current: T,
    render: (T) -> String,
    onSelect: (T) -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .horizontalScroll(rememberScrollState()),
        horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
    ) {
        options.forEach { opt ->
            val selected = opt == current
            Box(
                modifier = Modifier
                    .heightIn(min = 48.dp)
                    .clip(RoundedCornerShape(6.dp))
                    .background(if (selected) Accent else BgElevated)
                    .clickable { onSelect(opt) }
                    .widthIn(min = 48.dp)
                    .padding(vertical = Space.Md.dp, horizontal = Space.Md.dp),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    render(opt),
                    color = if (selected) OnAccent else OnBgMuted,
                    style = Type.Body.merge(TabularFigures),
                    maxLines = 1,
                )
            }
        }
    }
}

/**
 * Row of quick-fill preset chips (e.g. common exercises). The chip matching the
 * current free-text value is highlighted; tapping one fills that value. Purely a
 * shortcut over the editable field beside it, any value is still typable.
 */
@Composable
fun PresetChipsRow(options: List<String>, current: String, onPick: (String) -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .horizontalScroll(rememberScrollState()),
        horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
    ) {
        options.forEach { opt ->
            val selected = opt.equals(current.trim(), ignoreCase = true)
            Text(
                opt,
                color = if (selected) OnAccent else OnBgMuted,
                style = Type.Caption,
                modifier = Modifier
                    .clip(RoundedCornerShape(100))
                    .background(if (selected) Accent else BgElevated)
                    .clickable { onPick(opt) }
                    .padding(horizontal = Space.Card.dp, vertical = Space.Md.dp),
            )
        }
    }
}

/**
 * Big numeric value driven by the in-sheet [NumericKeypad] instead of the IME
 * (design import: keypad log editors). The text buffer IS the display AND the
 * submit source, the caller parses exactly what is shown, so the
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
) {
    val status = LocalStatusColors.current
    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
        FieldLabel(label, unit)
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(Space.Card.dp))
                .background(BgElevated)
                .then(
                    when {
                        invalid -> Modifier.border(1.dp, status.danger, RoundedCornerShape(Space.Card.dp))
                        active -> Modifier.border(1.dp, Accent.copy(alpha = 0.6f), RoundedCornerShape(Space.Card.dp))
                        else -> Modifier
                    },
                )
                .clickable { onActivate() }
                .padding(Space.Card.dp),
            horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text.ifEmpty { "0" },
                color = if (text.isEmpty()) OnBgFaint else OnBgBody,
                style = Type.Display.merge(TabularFigures),
                maxLines = 1,
                modifier = Modifier.weight(1f),
            )
            adjustments.forEach { adj ->
                Text(
                    adjLabel(adj),
                    color = OnBgMuted,
                    style = Type.Caption.merge(TabularFigures),
                    modifier = Modifier
                        .clip(RoundedCornerShape(6.dp))
                        .background(BgTop)
                        .clickable {
                            // Adjust from what is DISPLAYED (or the range floor when
                            // the buffer is unparseable), clamped and re-formatted -
                            // the display stays the single source of truth.
                            val base = text.replace(',', '.').toDoubleOrNull() ?: min
                            onActivate()
                            onText(
                                String.format(
                                    Locale.US,
                                    format,
                                    (base + adj).coerceIn(min, max),
                                ),
                            )
                        }
                        .padding(horizontal = Space.Md.dp, vertical = Space.Md.dp),
                )
            }
        }
        if (invalid) {
            Text(
                "Enter ${String.format(Locale.US, format, min)}–${String.format(Locale.US, format, max)} $unit",
                color = status.danger,
                style = Type.Caption.merge(TabularFigures),
            )
        }
    }
}

/**
 * In-form 3×4 numeric keypad (1–9, decimal point, 0, backspace) for the log
 * editors, big fixed targets that never scroll away under an IME. Emits raw
 * keys; the caller owns the buffer (and its validation), so the keypad itself
 * carries no numeric logic at all.
 */
@Composable
fun NumericKeypad(onKey: (Char) -> Unit, onBackspace: () -> Unit) {
    // Keypad ground (spec 05-log §2): a hair below BgElevated in the dark
    // themes; the plain screen ground on paper. C6: keyed off the RESOLVED palette
    // (a forced-Light-on-dark-OS sheet must not get the dark ground), not the OS.
    val ground = if (LocalPalette.current.bgTop.luminance() < 0.5f) Color(0xFF1A1815) else BgTop
    Column(
        modifier = Modifier
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
                    Box(
                        modifier = Modifier
                            .weight(1f)
                            .heightIn(min = 48.dp)
                            .clip(RoundedCornerShape(10.dp))
                            .background(BgElevated)
                            .clickable { if (key == '⌫') onBackspace() else onKey(key) }
                            .padding(vertical = Space.Md.dp),
                        contentAlignment = Alignment.Center,
                    ) {
                        if (key == '⌫') {
                            Icon(
                                painterResource(R.drawable.ic_ui_backspace),
                                contentDescription = "Backspace",
                                tint = OnBgMuted,
                                modifier = Modifier.size(22.dp),
                            )
                        } else {
                            Text(
                                "$key",
                                color = if (key == '.') OnBgMuted else OnBgBody,
                                style = Type.Title.copy(fontSize = 22.sp).merge(TabularFigures),
                            )
                        }
                    }
                }
            }
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
    if (base.length >= 6) return base
    return base + key
}

/** `+2.5` / `−5` label for a relative-adjust chip; trims a whole-number `.0`. */
private fun adjLabel(a: Double): String {
    val mag = Math.abs(a)
    val s = if (mag % 1.0 == 0.0) "${mag.toInt()}" else String.format(Locale.US, "%.1f", mag)
    return (if (a < 0) "−" else "+") + s
}

// --- Redesign display primitives (Today trend, History summary, Profile swatches) ---

/**
 * A minimal line chart over an ordered numeric series, no axes, no labels. Used
 * to show the shape of a factual logged sequence (e.g. e1RM over sessions); it
 * plots the values as given and computes nothing about them. Renders nothing for
 * a series shorter than two points.
 */
@Composable
fun Sparkline(values: List<Float>, color: Color, modifier: Modifier = Modifier, endDot: Boolean = false) {
    if (values.size < 2) return
    Canvas(modifier) {
        val minV = values.min()
        val maxV = values.max()
        val range = (maxV - minV).takeIf { it > 0f } ?: 1f
        // End-point dot (spec 01-today §3: r3.2) needs its own inset so it isn't
        // clipped at the right/top edge.
        val dotR = if (endDot) stroke(3.2f) else 0f
        val stepX = (size.width - dotR) / (values.size - 1)
        // Leave a hair of vertical inset so the stroke's round cap isn't clipped at
        // the exact top/bottom of the canvas on a new min/max point.
        val inset = maxOf(stroke(2.4f) / 2f, dotR)
        val usableH = (size.height - inset * 2f).coerceAtLeast(1f)
        fun y(v: Float) = inset + (usableH - (v - minV) / range * usableH)
        var prev = Offset(0f, y(values[0]))
        for (i in 1 until values.size) {
            val cur = Offset(i * stepX, y(values[i]))
            drawLine(color, prev, cur, strokeWidth = stroke(2.4f), cap = StrokeCap.Round)
            prev = cur
        }
        if (endDot) drawCircle(color, radius = dotR, center = prev)
    }
}

private fun androidx.compose.ui.graphics.drawscope.DrawScope.stroke(dp: Float): Float = dp * density

/** Fixed tile height shared by every small tile in the app (Coach tool tiles,
 *  History stat tiles, Today quick tiles) so mixed tile rows read as one grid. */
val TileHeight = 84.dp

/**
 * One compact stat cell for a horizontal summary strip (History week header),
 * using the app-wide tile anatomy: overline label top-left, big tabular value
 * (with optional small unit) bottom-left, fixed [TileHeight]. A [RowScope]
 * extension so callers place several across a row with `weight`.
 */
@Composable
fun RowScope.StatTile(value: String, unit: String?, label: String, glossaryKey: String? = null) {
    // History week-strip anatomy (spec 03-history §1): value ABOVE (19sp
    // ExtraBold, tabular) with the unit inline muted, label below-left.
    Column(
        modifier = Modifier
            .weight(1f)
            .clip(RoundedCornerShape(12.dp))
            .background(BgElevated)
            .padding(horizontal = Space.Card.dp, vertical = Space.Md.dp + Space.Xs.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Xs.dp),
    ) {
        Row(verticalAlignment = Alignment.Bottom) {
            Text(
                value,
                color = OnBgBody,
                style = Type.Title.copy(fontSize = 19.sp, fontWeight = FontWeight.ExtraBold)
                    .merge(TabularFigures),
                maxLines = 1,
            )
            if (unit != null) {
                Text(" $unit", color = OnBgFaint, style = Type.Caption.merge(TabularFigures))
            }
        }
        // Label, with an optional glossary affordance (m2) for jargon tiles.
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
            Text(label, color = OnBgFaint, style = Type.Caption)
            glossaryKey?.let { GlossaryInfo(it) }
        }
    }
}

/**
 * GitHub-style activity heatmap: one cell per day over the last [weeks] weeks,
 * columns = weeks, rows = day-of-week (Sunday top). Cell shade ramps with that
 * day's session count ([countsByDay], keyed by local-day index). Purely a factual
 * layout of when sessions were logged, no coaching, no streak-nudging; empty and
 * rest days simply read as the faint track. Future cells in the current week are
 * left blank. Day indices are local-day numbers (days since the Unix epoch in the
 * device timezone) so the caller keeps timezone handling out of the deterministic
 * core. Uses the decorative [Accent] ramp, never a semantic status color.
 */
@Composable
fun ContributionHeatmap(
    countsByDay: Map<Long, Int>,
    todayLocalDay: Long,
    weeks: Int = 16,
    modifier: Modifier = Modifier,
) {
    val accent = Accent
    // 0-count track per spec 03-history §3: `OnBgBody @6%`, ramping to full Accent.
    val track = OnBgBody.copy(alpha = 0.06f)
    fun cellColor(count: Int): Color = when {
        count <= 0 -> track
        count == 1 -> accent.copy(alpha = 0.40f)
        count == 2 -> accent.copy(alpha = 0.62f)
        count == 3 -> accent.copy(alpha = 0.82f)
        else -> accent
    }
    // Epoch day 0 (1970-01-01) is a Thursday; shift so Sunday = 0.
    fun dow(day: Long): Int = (((day % 7) + 4) % 7).toInt()
    val lastSunday = todayLocalDay - dow(todayLocalDay)
    val firstSunday = lastSunday - (weeks - 1) * 7L

    // m5: axis labels. Weekday initials down the left gutter (Sun top), month
    // ticks along the bottom at each column where the month first changes.
    val weekdayInitials = listOf("S", "M", "T", "W", "T", "F", "S")
    val monthFmt = SimpleDateFormat("MMM", Locale.US).apply {
        timeZone = TimeZone.getTimeZone("UTC")
    }
    fun monthOf(day: Long): String = monthFmt.format(Date(day * 86_400_000L))
    // First column of each distinct month → its label (blank elsewhere).
    val monthTicks = (0 until weeks).map { col ->
        val colMonth = monthOf(firstSunday + col * 7L)
        val prevMonth = if (col == 0) "" else monthOf(firstSunday + (col - 1) * 7L)
        if (colMonth != prevMonth) colMonth else ""
    }

    val gutter = 14.dp
    val gap = Space.Xs.dp
    BoxWithConstraints(modifier = modifier.fillMaxWidth()) {
        // Square cells sized to fill the width after the gutter + inter-column gaps.
        val cell = (maxWidth - gutter - gap * weeks) / weeks
        Column(verticalArrangement = Arrangement.spacedBy(gap)) {
            Row(horizontalArrangement = Arrangement.spacedBy(gap)) {
                // Weekday gutter.
                Column(
                    modifier = Modifier.width(gutter),
                    verticalArrangement = Arrangement.spacedBy(gap),
                ) {
                    for (row in 0..6) {
                        Box(Modifier.height(cell), contentAlignment = Alignment.CenterEnd) {
                            // Label alternate rows (Mon/Wed/Fri) to avoid crowding.
                            if (row % 2 == 1) {
                                Text(weekdayInitials[row], color = OnBgFaint, style = Type.Chip)
                            }
                        }
                    }
                }
                for (col in 0 until weeks) {
                    Column(verticalArrangement = Arrangement.spacedBy(gap)) {
                        for (row in 0..6) {
                            val day = firstSunday + col * 7L + row
                            val cellMod = Modifier
                                .size(cell)
                                .clip(RoundedCornerShape(2.dp))
                            if (day > todayLocalDay) {
                                Box(cellMod)
                            } else {
                                Box(cellMod.background(cellColor(countsByDay[day] ?: 0)))
                            }
                        }
                    }
                }
            }
            // Month ticks aligned under their column.
            Row(horizontalArrangement = Arrangement.spacedBy(gap)) {
                Box(Modifier.width(gutter))
                for (col in 0 until weeks) {
                    Box(Modifier.width(cell)) {
                        if (monthTicks[col].isNotEmpty()) {
                            Text(monthTicks[col], color = OnBgFaint, style = Type.Chip)
                        }
                    }
                }
            }
        }
    }
}

/**
 * Garmin-style per-day running-distance bar chart: one accent bar per local day
 * over the last [days] days, height ∝ that day's summed km ([kmByDay], keyed by
 * local-day index). A purely factual layout of logged distance, no coaching,
 * no goals, no target lines; empty days read as the faint baseline. Day indices
 * are local-day numbers (days since the Unix epoch in the device timezone), so
 * the caller keeps timezone bucketing out of the deterministic core. Uses the
 * decorative [Accent] ramp, never a semantic status color. The y-max label and
 * a couple of date ticks orient the eye; today's bar carries an accent underline.
 */
@Composable
fun RunDistanceBars(
    kmByDay: Map<Long, Double>,
    todayLocalDay: Long,
    days: Int = 14,
    modifier: Modifier = Modifier,
) {
    val firstDay = todayLocalDay - (days - 1)
    val vals = (0 until days).map { kmByDay[firstDay + it] ?: 0.0 }
    val maxKm = vals.maxOrNull()?.takeIf { it > 0.0 } ?: 0.0
    val totalKm = vals.sum()
    // Nice round y-max ceiling so the axis label reads cleanly (e.g. 8/10/15 km).
    val yMax = when {
        maxKm <= 0.0 -> 0.0
        maxKm <= 5.0 -> kotlin.math.ceil(maxKm)
        maxKm <= 20.0 -> kotlin.math.ceil(maxKm / 2.0) * 2.0
        else -> kotlin.math.ceil(maxKm / 5.0) * 5.0
    }

    val accent = Accent
    val baseline = OnBgBody.copy(alpha = 0.06f)
    val todayTick = Accent.copy(alpha = 0.55f)

    fun dayLabel(day: Long): String =
        SimpleDateFormat("MMM d", Locale.US).apply {
            timeZone = TimeZone.getTimeZone("UTC")
        }.format(Date(day * 86_400_000L))

    Column(
        modifier = modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
    ) {
        // Header: y-max on the left, factual window total on the right.
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Text(
                if (yMax > 0.0) "${fmtKm(yMax)} km" else "-",
                color = OnBgFaint,
                style = Type.Caption.merge(TabularFigures),
            )
            Text(
                "${fmtKm(totalKm)} km · $days days",
                color = OnBgFaint,
                style = Type.Caption.merge(TabularFigures),
            )
        }
        Canvas(
            modifier = Modifier
                .fillMaxWidth()
                .height(96.dp),
        ) {
            val n = days
            val slot = size.width / n
            val barW = slot * 0.58f
            val corner = CornerRadius(stroke(2f), stroke(2f))
            // Reserve a hair at the bottom for the baseline + today underline.
            val underline = stroke(2.5f)
            val usableH = (size.height - underline - stroke(2f)).coerceAtLeast(1f)
            for (i in 0 until n) {
                val km = vals[i]
                val cx = slot * i + slot / 2f
                val left = cx - barW / 2f
                val barTop = if (yMax > 0.0) {
                    (usableH - (km / yMax).toFloat().coerceIn(0f, 1f) * usableH)
                } else {
                    usableH
                }
                val isToday = (firstDay + i) == todayLocalDay
                if (km > 0.0) {
                    drawRoundRect(
                        color = accent,
                        topLeft = Offset(left, barTop),
                        size = Size(barW, usableH - barTop),
                        cornerRadius = corner,
                    )
                } else {
                    // Empty day: a faint baseline stub so the day still reads.
                    drawRoundRect(
                        color = baseline,
                        topLeft = Offset(left, usableH - stroke(2f)),
                        size = Size(barW, stroke(2f)),
                        cornerRadius = corner,
                    )
                }
                if (isToday) {
                    drawRoundRect(
                        color = todayTick,
                        topLeft = Offset(left, usableH + stroke(1.5f)),
                        size = Size(barW, underline),
                        cornerRadius = corner,
                    )
                }
            }
        }
        // A couple of date ticks: window start, and today on the right.
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Text(dayLabel(firstDay), color = OnBgFaint, style = Type.Caption.merge(TabularFigures))
            Text("Today", color = OnBgFaint, style = Type.Caption)
        }
    }
}

/** Trim a whole-number `.0` off a km figure; one decimal otherwise. */
private fun fmtKm(km: Double): String =
    if (km % 1.0 == 0.0) "${km.toInt()}" else String.format(Locale.US, "%.1f", km)

/** Row-disclosure chevron (`ui-chevron-right`, 16dp, `OnBgFaint`); rotates 90°
 *  when the row is expanded. One shared affordance for every tappable row. */
@Composable
fun RowChevron(expanded: Boolean = false) {
    Icon(
        painterResource(R.drawable.ic_ui_chevron_right),
        contentDescription = null,
        tint = OnBgFaint,
        modifier = Modifier
            .size(16.dp)
            .rotate(if (expanded) 90f else 0f),
    )
}

/** The ONE tile/card overline (chrome §3): 11sp Bold UPPERCASE, `OnBgFaint`,
 *  top-left. Every data tile and card label goes through this. */
@Composable
fun TileOverline(text: String, color: Color = OnBgFaint) {
    Text(text.uppercase(Locale.US), color = color, style = Type.Overline)
}

/** Section overline with an optional trailing accent action ("History →"). */
@Composable
fun SectionOverline(text: String, trailing: String? = null, onTrailing: (() -> Unit)? = null) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(top = Space.Sm.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(text.uppercase(Locale.US), color = Accent, style = Type.Section)
        if (trailing != null) {
            Text(
                trailing,
                color = Accent,
                style = Type.Body,
                modifier = Modifier
                    .clip(RoundedCornerShape(Space.Md.dp))
                    .clickable(enabled = onTrailing != null) { onTrailing?.invoke() }
                    .padding(horizontal = Space.Sm.dp),
            )
        }
    }
}

/** A rounded icon ground (36/38/44dp) holding a tinted 24-grid symbol, the
 *  leading tile on chooser rows, activity rows and history cards. */
@Composable
fun IconTile(painter: Painter, tint: Color, ground: Color, size: Dp = 36.dp, iconSize: Dp = 20.dp) {
    Box(
        modifier = Modifier
            .size(size)
            .clip(RoundedCornerShape(10.dp))
            .background(ground),
        contentAlignment = Alignment.Center,
    ) {
        Icon(painter, contentDescription = null, tint = tint, modifier = Modifier.size(iconSize))
    }
}

/**
 * Two-option segmented control (spec 03-history §2, INVARIANT 4): exactly two
 * segments, selected = `Accent` fill + dark [OnAccent] text (owner AA ruling on
 * accent fills), unselected = `OnBgMuted` on `BgElevated`. No third option, no
 * counts in the labels.
 */
@Composable
fun TwoSegmentRow(left: String, right: String, selectedIndex: Int, onSelect: (Int) -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(Space.Md.dp))
            .background(BgElevated)
            .padding(Space.Xs.dp),
        horizontalArrangement = Arrangement.spacedBy(Space.Xs.dp),
    ) {
        listOf(left, right).forEachIndexed { i, label ->
            val selected = i == selectedIndex
            Box(
                modifier = Modifier
                    .weight(1f)
                    .heightIn(min = 40.dp)
                    .clip(RoundedCornerShape(Space.Md.dp - Space.Xs.dp))
                    .background(if (selected) Accent else Color.Transparent)
                    .clickable { onSelect(i) },
                contentAlignment = Alignment.Center,
            ) {
                Text(label, color = if (selected) OnAccent else OnBgMuted, style = Type.Chip)
            }
        }
    }
}

/** Small date badge for history cards: `OnBgFaint` on `BgTop`, chip radius. */
@Composable
fun DateBadge(text: String) {
    if (text.isEmpty()) return
    Text(
        text,
        color = OnBgFaint,
        style = Type.Chip,
        modifier = Modifier
            .clip(RoundedCornerShape(6.dp))
            .background(BgTop)
            .padding(horizontal = Space.Md.dp, vertical = Space.Sm.dp),
    )
}

/**
 * A theme choice as a swatch card: the theme's own accent + ground squares over
 * its name, outlined in the accent when selected. Reads the theme's [AppTheme.dark]
 * palette directly so each card previews itself regardless of the active theme.
 * A [RowScope] extension so the three sit across one row with `weight`.
 */
@Composable
fun RowScope.ThemeSwatchCard(theme: AppTheme, selected: Boolean, onClick: () -> Unit) {
    val pal = theme.dark
    Column(
        modifier = Modifier
            .weight(1f)
            .clip(RoundedCornerShape(Space.Card.dp))
            .background(BgElevated)
            .then(
                if (selected) Modifier.border(1.5.dp, Accent, RoundedCornerShape(Space.Card.dp)) else Modifier,
            )
            .clickable { onClick() }
            .padding(vertical = Space.Card.dp, horizontal = Space.Md.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
            Box(Modifier.size(16.dp).clip(RoundedCornerShape(5.dp)).background(pal.accent))
            Box(
                Modifier
                    .size(16.dp)
                    .clip(RoundedCornerShape(5.dp))
                    .background(pal.bgTop)
                    .border(1.dp, OnBgFaint.copy(alpha = 0.4f), RoundedCornerShape(5.dp)),
            )
        }
        Text(
            theme.label,
            color = if (selected) Accent else OnBgMuted,
            style = Type.Chip,
            maxLines = 1,
        )
    }
}
