package de.tuschla.fitnessanlage

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.width
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
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import java.util.Locale

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
                color = Color.White,
                style = Type.Body.merge(TabularFigures),
                textAlign = TextAlign.Center,
                modifier = Modifier.width(48.dp),
            )
            OutlinedButton(onClick = onIncrement, enabled = canIncrement) { Text("+") }
        }
    }
}
