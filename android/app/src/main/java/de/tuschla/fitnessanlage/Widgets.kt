package de.tuschla.fitnessanlage

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.util.Locale

/** Dark ground tone used for text/icons sitting on the [Accent] fill, so a
 *  selected chip reads clearly in every theme (accents are all mid-tone). */
private val OnAccent = Color(0xFF141210)

/**
 * Label + dropdown enum picker. Tapping the anchor opens a menu showing every
 * option at once, so the choice is a direct selection rather than a blind
 * tap-to-cycle (which hid all but the current value and forced N-1 taps to reach
 * a distant option).
 *
 * [display] only affects the on-screen text; callers that serialize the choice
 * still key off the variant itself (`.name`), so a friendly label here never
 * touches the wire contract.
 *
 * [divideBefore] draws a divider above the first option for which it returns true,
 * so a caller can visually separate a subgroup, e.g. the readiness picker fences
 * off the medical red-flag signals from the routine metrics, since mis-tapping a
 * red flag has outsized (safety) consequences.
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
    var expanded by remember { mutableStateOf(false) }
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, color = OnBgBody, style = Type.Body)
        // Box anchors the DropdownMenu to the button; the menu lists every option
        // so a distant choice is one tap, not N-1 cycles through the others.
        Box {
            OutlinedButton(onClick = { expanded = true }) {
                Text(display(current), style = Type.Body)
            }
            DropdownMenu(expanded = expanded, onDismissRequest = { expanded = false }) {
                values.forEach { value ->
                    if (divideBefore(value)) HorizontalDivider()
                    DropdownMenuItem(
                        text = { Text(display(value), style = Type.Body) },
                        onClick = {
                            onSelect(value)
                            expanded = false
                        },
                    )
                }
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
            values.forEachIndexed { index, value ->
                SegmentedButton(
                    selected = value == current,
                    onClick = { onSelect(value) },
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
 * A big, directly-editable numeric value (device decimal keypad) plus a few
 * one-tap relative-adjust chips, the redesign's replacement for the weight/
 * distance/duration steppers. The typed text is the display source; every parse
 * that lands in `[min, max]` is pushed up via [onChange], and an adjust chip
 * commits `current ± delta` clamped. Locale-forced `.` decimal so the value means
 * the same to the '.'-formatted core on the same screen.
 *
 * Display and committed state must never diverge at submit time: an out-of-range
 * or unparseable (including cleared) entry flags the field invalid, danger
 * border + range hint here, and [onValidChange] tells the caller so it can block
 * its submit button. Leaving the field reconciles: a parseable entry clamps and
 * commits; garbage/empty snaps the display back to the committed value.
 */
@Composable
fun BigValueField(
    label: String,
    unit: String,
    value: Double,
    format: String,
    min: Double,
    max: Double,
    adjustments: List<Double>,
    onValidChange: (Boolean) -> Unit = {},
    onChange: (Double) -> Unit,
) {
    var text by rememberSaveable { mutableStateOf(String.format(Locale.US, format, value)) }
    var valid by rememberSaveable { mutableStateOf(true) }
    val status = LocalStatusColors.current
    fun setValid(v: Boolean) {
        if (valid != v) {
            valid = v
            onValidChange(v)
        }
    }
    fun commit(v: Double) {
        val c = v.coerceIn(min, max)
        text = String.format(Locale.US, format, c)
        setValid(true)
        onChange(c)
    }
    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
        FieldLabel(label, unit)
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(Space.Card.dp))
                .background(BgElevated)
                .then(
                    if (!valid) {
                        Modifier.border(1.dp, status.danger, RoundedCornerShape(Space.Card.dp))
                    } else {
                        Modifier
                    },
                )
                .padding(Space.Card.dp),
            horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            BasicTextField(
                value = text,
                onValueChange = { raw ->
                    text = raw
                    val parsed = raw.replace(',', '.').toDoubleOrNull()
                    if (parsed != null && parsed in min..max) {
                        setValid(true)
                        onChange(parsed)
                    } else {
                        // The display now shows something the committed state does
                        // not hold: flag it so the caller's submit stays blocked
                        // until the field is corrected or loses focus (which
                        // reconciles below).
                        setValid(false)
                    }
                },
                textStyle = Type.Display.copy(color = OnBgBody),
                singleLine = true,
                cursorBrush = SolidColor(Accent),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                modifier = Modifier
                    .weight(1f)
                    .onFocusChanged { st ->
                        if (!st.isFocused && !valid) {
                            val parsed = text.replace(',', '.').toDoubleOrNull()
                            if (parsed != null) {
                                // Parseable but out of range: clamp on commit.
                                commit(parsed)
                            } else {
                                // Cleared/garbage: snap back to the committed value.
                                text = String.format(Locale.US, format, value)
                                setValid(true)
                            }
                        }
                    },
            )
            adjustments.forEach { adj ->
                Text(
                    adjLabel(adj),
                    color = OnBgMuted,
                    style = Type.Caption.merge(TabularFigures),
                    modifier = Modifier
                        .clip(RoundedCornerShape(6.dp))
                        .background(BgTop)
                        .clickable { commit((text.replace(',', '.').toDoubleOrNull() ?: value) + adj) }
                        .padding(horizontal = Space.Md.dp, vertical = Space.Md.dp),
                )
            }
        }
        if (!valid) {
            Text(
                "Enter ${String.format(Locale.US, format, min)}–${String.format(Locale.US, format, max)} $unit",
                color = status.danger,
                style = Type.Caption.merge(TabularFigures),
            )
        }
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
                    .clip(RoundedCornerShape(6.dp))
                    .background(if (selected) Accent else BgElevated)
                    .clickable { onSelect(opt) }
                    .widthIn(min = 42.dp)
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
fun Sparkline(values: List<Float>, color: Color, modifier: Modifier = Modifier) {
    if (values.size < 2) return
    Canvas(modifier) {
        val minV = values.min()
        val maxV = values.max()
        val range = (maxV - minV).takeIf { it > 0f } ?: 1f
        val stepX = size.width / (values.size - 1)
        // Leave a hair of vertical inset so the stroke's round cap isn't clipped at
        // the exact top/bottom of the canvas on a new min/max point.
        val inset = stroke(2.4f) / 2f
        val usableH = (size.height - inset * 2f).coerceAtLeast(1f)
        fun y(v: Float) = inset + (usableH - (v - minV) / range * usableH)
        var prev = Offset(0f, y(values[0]))
        for (i in 1 until values.size) {
            val cur = Offset(i * stepX, y(values[i]))
            drawLine(color, prev, cur, strokeWidth = stroke(2.4f), cap = StrokeCap.Round)
            prev = cur
        }
    }
}

private fun androidx.compose.ui.graphics.drawscope.DrawScope.stroke(dp: Float): Float = dp * density

/**
 * One compact stat cell for a horizontal summary strip (History week header):
 * a big tabular value with an optional small unit, over a muted caption. A
 * [RowScope] extension so callers place several across a row with `weight`.
 */
@Composable
fun RowScope.StatTile(value: String, unit: String?, label: String) {
    Column(
        modifier = Modifier
            .weight(1f)
            .clip(RoundedCornerShape(Space.Card.dp))
            .background(BgElevated)
            .padding(horizontal = Space.Card.dp, vertical = Space.Md.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Xs.dp),
    ) {
        Row(verticalAlignment = Alignment.Bottom) {
            Text(value, color = OnBgBody, style = Type.Title.merge(TabularFigures))
            if (unit != null) {
                Text(" $unit", color = OnBgFaint, style = Type.Caption.merge(TabularFigures))
            }
        }
        Text(label, color = OnBgMuted, style = Type.Caption)
    }
}

/**
 * A wrapping row of exclusive choice chips over an enum, every option visible,
 * the current one accent-filled, one tap selects. The tap-row picker uses this to
 * replace a dropdown/segmented control with recognition-first chips.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun <T> EnumChips(options: List<T>, current: T, label: (T) -> String, onSelect: (T) -> Unit) {
    FlowRow(
        horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
    ) {
        options.forEach { opt ->
            val selected = opt == current
            Text(
                label(opt),
                color = if (selected) OnAccent else OnBgMuted,
                style = Type.Caption,
                modifier = Modifier
                    .clip(RoundedCornerShape(100))
                    .background(if (selected) Accent else BgTop)
                    .clickable { onSelect(opt) }
                    .padding(horizontal = Space.Card.dp, vertical = Space.Md.dp),
            )
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
    val track = OnBgFaint.copy(alpha = 0.15f)
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
    Row(
        modifier = modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(Space.Xs.dp),
    ) {
        for (col in 0 until weeks) {
            Column(
                modifier = Modifier.weight(1f),
                verticalArrangement = Arrangement.spacedBy(Space.Xs.dp),
            ) {
                for (row in 0..6) {
                    val day = firstSunday + col * 7L + row
                    val cell = Modifier
                        .fillMaxWidth()
                        .aspectRatio(1f)
                        .clip(RoundedCornerShape(2.dp))
                    if (day > todayLocalDay) {
                        // Future day in the current week: leave the slot empty.
                        Box(cell)
                    } else {
                        Box(cell.background(cellColor(countsByDay[day] ?: 0)))
                    }
                }
            }
        }
    }
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
