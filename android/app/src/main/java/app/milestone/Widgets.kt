package app.milestone

import androidx.compose.foundation.ScrollState
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.requiredHeight
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.minimumInteractiveComponentSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.CompositingStrategy
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.painter.Painter
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.layout.positionInParent
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.collapse
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.expand
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import java.util.Locale

// --- Selectable-control vocabulary (one look for every "pick one" control) ---
// Scale cells and choice chips ([ChoiceChip]) share: a Space.Md
// (8dp) corner; unselected = a 4% OnBgBody tint inside a 1dp OnBgFaint @30%
// hairline with OnBgBody text, so the target has visible bounds on ANY ground
// (sheet, card, screen) in light and dark; selected = solid Accent with OnAccent
// text, never an outline ring. Scale cells are 48dp tall; text chips are 40dp
// visual with a ≥48dp touch target. Segmented controls ([SegmentedEnumRow],
// [TwoSegmentRow]) are one borderless track ([SegmentTrack]) with the same solid
// Accent selected segment.

/** Corner shape of every selectable cell / chip / segment. */
val ChoiceCellShape = RoundedCornerShape(Space.Md.dp)

/** Visual height of a text choice chip (touch target stays ≥48dp). */
val ChipVisualHeight = 40.dp

/** Opacity of the OnBgBody wash inside an unselected cell (ThemeContrastTest). */
internal const val ChoiceCellFillAlpha = 0.04f

/** Tint inside an unselected cell (and a disabled action): a 4% OnBgBody wash. */
val ChoiceCellFill: Color @Composable get() = OnBgBody.copy(alpha = ChoiceCellFillAlpha)

/** Hairline that bounds an unselected cell, an idle input field and a disabled action. */
val ChoiceCellBorder: Color @Composable get() = HairlineStrong

/** Clip + fill (+ hairline when unselected) for one selectable cell. Chain the
 *  caller's `.selectable(...)` and inner padding after it. */
@Composable
fun Modifier.choiceCell(selected: Boolean, shape: Shape = ChoiceCellShape): Modifier {
    val clipped = clip(shape)
    return if (selected) {
        clipped.background(Accent)
    } else {
        clipped.background(ChoiceCellFill).border(1.dp, ChoiceCellBorder, shape)
    }
}

/** Text/icon tone on a [choiceCell] or [choiceCard]: OnAccent on the accent fill,
 *  OnBgBody otherwise (OnBgMuted on the 4% tint dips under AA on the light grounds). */
@Composable
fun choiceCellContent(selected: Boolean): Color = if (selected) OnAccent else OnBgBody

/** Secondary line (a subtitle / consequence) on a [choiceCard]: OnAccent on the
 *  accent fill (a muted tone would fail AA there), OnBgMuted on the card ground. */
@Composable
fun choiceCellSecondaryContent(selected: Boolean): Color = if (selected) OnAccent else OnBgMuted

/** Corner shape of a selectable card ([choiceCard]). */
val ChoiceCardShape = RoundedCornerShape(Space.Card.dp)

/**
 * Clip + fill for a selectable CARD (a multi-line option such as a setup goal or a
 * theme swatch): the same selected look as every chip, cell and segment, solid
 * `Accent` with no outline ring; unselected = the `BgElevated` card ground inside a
 * 1dp [ChoiceCellBorder] hairline. Chain `.selectable(...)` and inner padding after
 * it; colour the title with [choiceCellContent] and any second line with
 * [choiceCellSecondaryContent].
 */
@Composable
fun Modifier.choiceCard(selected: Boolean): Modifier {
    val clipped = clip(ChoiceCardShape)
    return if (selected) {
        clipped.background(Accent)
    } else {
        clipped.background(BgElevated).border(1.dp, ChoiceCellBorder, ChoiceCardShape)
    }
}

/** Fill of a disabled primary action: the neutral cell tint plus
 *  [DisabledActionBorder], so the button keeps its pill on every ground (never the
 *  plain ground colour, which vanishes on a light sheet). Label: OnBgMuted. An
 *  inactive control is exempt from WCAG 1.4.3, so the label stays deliberately quiet. */
val DisabledActionFill: Color @Composable get() = ChoiceCellFill

/** Outline of a disabled primary action (see [DisabledActionFill]). */
val DisabledActionBorder: Color @Composable get() = ChoiceCellBorder

/** Shown in place of an empty numeric value: a faint dash, never a "0" that reads
 *  as an entry. */
const val EmptyValuePlaceholder = "—"

/** Width of the edge fade on a horizontally scrolling row. */
private val EdgeFadeWidth = 16.dp

/**
 * Fades the clipped edge(s) of a horizontally scrolling row to transparent, so a
 * cut-off cell reads as "more this way" rather than a cropped layout. An alpha
 * mask, not a gradient to a guessed surface colour, so it works on any ground.
 * Apply BEFORE `horizontalScroll` (it masks the viewport, not the content).
 */
fun Modifier.horizontalEdgeFade(state: ScrollState): Modifier =
    graphicsLayer { compositingStrategy = CompositingStrategy.Offscreen }
        .drawWithContent {
            drawContent()
            val fade = EdgeFadeWidth.toPx().coerceAtMost(size.width / 2f)
            if (state.value > 0) {
                drawRect(
                    brush = Brush.horizontalGradient(listOf(Color.Transparent, Color.Black), startX = 0f, endX = fade),
                    size = Size(fade, size.height),
                    blendMode = BlendMode.DstIn,
                )
            }
            if (state.value < state.maxValue) {
                drawRect(
                    brush = Brush.horizontalGradient(
                        listOf(Color.Black, Color.Transparent),
                        startX = size.width - fade,
                        endX = size.width,
                    ),
                    topLeft = Offset(size.width - fade, 0f),
                    size = Size(fade, size.height),
                    blendMode = BlendMode.DstIn,
                )
            }
        }

/**
 * Full-width list of exclusive options: one comfortable (≥48dp) whole-row tap
 * target per option, announced as one radio group. The selected row is the one
 * list-selection look ([AccentTint] fill, Bold `OnBgBody` label,
 * `Accent` check), shared with the [SegmentedEnumRow] list fallback. Used instead
 * of an anchored dropdown menu, whose rows are tiny, easy to fumble, and clip
 * long option lists.
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
            .background(BgTop)
            .selectableGroup(),
    ) {
        options.forEachIndexed { i, value ->
            // A subgroup fence (divideBefore) is the stronger OnBgFaint @30% rule;
            // every other row boundary gets a faint hairline so an unanswered list
            // reads as separate choices, not one grey paragraph.
            if (divideBefore(value)) {
                HorizontalDivider(color = HairlineStrong)
            } else if (i > 0) {
                HorizontalDivider(color = Hairline)
            }
            val selected = value == current
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = ActionMinHeight)
                    .background(if (selected) AccentTint else Color.Transparent)
                    .selectable(selected = selected, role = Role.RadioButton, onClick = { onSelect(value) })
                    .padding(horizontal = Space.Card.dp, vertical = Space.Md.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                // OnBgBody, not Accent: Accent text on its own tint dips under AA
                // on the light grounds; the check carries the hue.
                Text(
                    display(value),
                    color = OnBgBody,
                    style = if (selected) Type.Body.copy(fontWeight = FontWeight.Bold) else Type.Body,
                    modifier = Modifier.weight(1f),
                )
                if (selected) {
                    // The row's radio semantics already announce "selected".
                    Icon(
                        painterResource(R.drawable.ic_ui_check),
                        contentDescription = null,
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
 * generous targets, and picking one collapses the list. Callers serialize only
 * the value's `.name`.
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
                .heightIn(min = ActionMinHeight)
                .clip(RoundedCornerShape(Space.Md.dp))
                .clickable(role = Role.Button) { expanded = !expanded }
                .semantics {
                    if (expanded) collapse { expanded = false; true } else expand { expanded = true; true }
                },
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
 * Segments are equal-width, so at a large font scale (Android allows 200 %) a label
 * like "Hypertrophy" can outgrow its segment. Every label is measured against the
 * width a segment actually gets; if any would be cut off, the same options render
 * as a full-width radio list instead (one ≥48dp row per option), so no text is
 * ever silently clipped (WCAG 1.4.4).
 *
 * [display] only affects the on-screen text; the wire contract still keys off the
 * value itself (an enum's `.name`, a plain number), so a friendly label never
 * touches serialization. [label] may be null when the caller already renders the
 * field's label.
 */
@Composable
fun <T> SegmentedEnumRow(
    label: String?,
    values: List<T>,
    current: T,
    display: (T) -> String = { it.toString() },
    onSelect: (T) -> Unit,
) {
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
    ) {
        // Null = the caller labels the row itself (e.g. a FieldLabel above it).
        if (label != null) Text(label, color = OnBgBody, style = Type.Body)
        BoxWithConstraints(Modifier.fillMaxWidth()) {
            val measurer = rememberTextMeasurer()
            val density = LocalDensity.current
            // Measured in the exact style the segment renders (Type.Chip) against
            // the room a SegmentTrack segment leaves for its label.
            val labelRoomPx = with(density) { segmentLabelRoom(maxWidth, values.size).toPx() }
            val fits = values.all { v ->
                measurer.measure(display(v), style = Type.Chip, maxLines = 1, softWrap = false).size.width <= labelRoomPx
            }
            if (fits) {
                // The same borderless track as TwoSegmentRow: one segmented look app-wide.
                SegmentTrack(
                    labels = values.map(display),
                    selectedIndex = values.indexOf(current),
                    role = Role.RadioButton,
                    onSelect = { onSelect(values[it]) },
                )
            } else {
                // Same rows and selection look as every other option list.
                OptionList(values, current, display, onSelect = onSelect)
            }
        }
    }
}

// --- Entry primitives: no dropdowns, no −/+ steppers ---
// Recognition over recall for the logging forms: every choice is visible and one
// tap picks it. These carry no coaching logic: they only gather the raw inputs the
// core derives e1RM / RIR / zone from.

/**
 * The ONE field / sub-question label (log editors, keypad fields, setup steps,
 * check-ins): sentence case, `Type.Body` Bold, `OnBgMuted`, so a label reads as a
 * quiet heading over its control and a long question ("Hard sessions you couldn't
 * finish") reads as a sentence rather than a shouted overline. An optional
 * right-aligned live value / unit hint (e.g. `Reps … 5`, `Weight … kg`) sits on the
 * label's first baseline; a long label wraps within its own column and always
 * keeps a Space.Md gap to it. [heading] marks a sub-question as a heading for
 * TalkBack's heading navigation (setup steps); plain field labels leave it off.
 * [onTrailing] turns the trailing hint into a quiet in-place action (a unit swap
 * such as "bpm ⇄"): Bold `Accent`, spoken as [trailingDescription], with a 48dp
 * touch target that overflows the label row rather than growing it.
 *
 * Forms only: a read-only section header (a detail sheet's EFFORT / RUN TYPE /
 * SPLITS) uses [TileOverline], so one sheet never mixes the two heading styles.
 */
@Composable
fun FieldLabel(
    label: String,
    trailing: String? = null,
    heading: Boolean = false,
    onTrailing: (() -> Unit)? = null,
    trailingDescription: String? = null,
) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        Text(
            label,
            color = OnBgMuted,
            style = Type.Body.copy(fontWeight = FontWeight.Bold),
            modifier = Modifier
                .weight(1f)
                .alignByBaseline()
                .then(if (heading) Modifier.semantics { heading() } else Modifier),
        )
        if (trailing != null && onTrailing != null) {
            // Padded (and shifted back by the same amount, so the label stays flush
            // right) so the target is at least ~48dp wide; the overlay adds the
            // 48dp height without growing the label row.
            Box(
                Modifier.alignByBaseline().offset(x = Space.Sm.dp),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    trailing,
                    color = Accent,
                    style = Type.Caption.copy(fontWeight = FontWeight.Bold).merge(TabularFigures),
                    // The overlay below carries the role and the spoken label.
                    modifier = Modifier
                        .clearAndSetSemantics {}
                        .padding(horizontal = Space.Sm.dp),
                )
                Box(
                    Modifier
                        .matchParentSize()
                        .requiredHeight(ActionMinHeight)
                        .clip(RoundedCornerShape(Space.Md.dp))
                        .clickable(role = Role.Button, onClick = onTrailing)
                        .semantics { contentDescription = trailingDescription ?: trailing },
                )
            }
        } else if (trailing != null) {
            Text(
                trailing,
                color = OnBgFaint,
                style = Type.Caption.merge(TabularFigures),
                modifier = Modifier.alignByBaseline(),
            )
        }
    }
}

/**
 * A settings toggle whose WHOLE row is the switch: one ≥48dp touch target, and
 * TalkBack reads the label with the state ("Show route maps, switch, on"). The
 * inner [Switch] only draws the state, so it never surfaces as an unnamed control.
 */
@Composable
fun LabeledSwitchRow(
    label: String,
    checked: Boolean,
    onCheckedChange: (Boolean) -> Unit,
    subtitle: String? = null,
    subtitleColor: Color = OnBgMuted,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = ActionMinHeight)
            .clip(RoundedCornerShape(Space.Md.dp))
            .toggleable(value = checked, role = Role.Switch, onValueChange = onCheckedChange),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(
            // Vertical inset inside the row's clip: the last caption line never
            // sits on the clipped edge, so its descenders can't be cut.
            modifier = Modifier.weight(1f).padding(end = Space.Md.dp).padding(vertical = Space.Sm.dp),
            verticalArrangement = Arrangement.spacedBy(Space.Xs.dp),
        ) {
            Text(label, color = OnBgBody, style = Type.Body)
            if (subtitle != null) Text(subtitle, color = subtitleColor, style = Type.Caption)
        }
        Switch(checked = checked, onCheckedChange = null)
    }
}

/**
 * Width-filling grid of discrete choices, every option visible, one tap to pick,
 * the selection filled with the accent. For small fixed sets (RPE, HR%, 0–7
 * counts). Use [ScrollableScaleRow] when the set is longer. [label] names the
 * field for TalkBack: each cell reads "label, value" instead of a bare number,
 * and the row is one radio group.
 *
 * Cells share ONE column width: all on one row when each gets ≥48dp, otherwise
 * balanced rows (0–7 → 4 + 4, nine values → 5 + 4) so a wrapped cell keeps its
 * column width and left edge instead of stretching into an orphan.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun <T> ChoiceScaleRow(
    options: List<T>,
    current: T,
    render: (T) -> String,
    label: String? = null,
    onSelect: (T) -> Unit,
) {
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val density = LocalDensity.current
        val gapPx = with(density) { Space.Sm.dp.roundToPx() }
        val minCellPx = with(density) { ActionMinHeight.roundToPx() }
        val n = options.size.coerceAtLeast(1)
        val widthPx = if (constraints.hasBoundedWidth) constraints.maxWidth else minCellPx * n + gapPx * (n - 1)
        val perRow = ((widthPx + gapPx) / (minCellPx + gapPx)).coerceIn(1, n)
        val rows = (n + perRow - 1) / perRow
        val cols = (n + rows - 1) / rows
        // Floored to whole px so `cols` cells + gaps never overflow the row and
        // wrap one early.
        val cellWidth = with(density) { ((widthPx - gapPx * (cols - 1)) / cols).toDp() }
        FlowRow(
            modifier = Modifier.fillMaxWidth().selectableGroup(),
            horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
            verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
            maxItemsInEachRow = cols,
        ) {
            options.forEach { opt ->
                val selected = opt == current
                Box(
                    modifier = Modifier
                        .width(cellWidth)
                        .heightIn(min = ActionMinHeight)
                        .choiceCell(selected)
                        // Selectable (not bare clickable) so TalkBack reads the picked
                        // state + radio role instead of a stateless tap target.
                        .selectable(selected = selected, role = Role.RadioButton, onClick = { onSelect(opt) })
                        .then(scaleCellDescription(label, render(opt)))
                        .padding(vertical = Space.Md.dp),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        render(opt),
                        color = choiceCellContent(selected),
                        style = Type.Body.merge(TabularFigures),
                        maxLines = 1,
                    )
                }
            }
        }
    }
}

/**
 * Horizontally-scrollable scale of discrete choices for a longer bounded set
 * (e.g. reps): every value is reachable by scroll + one tap, no stepper
 * cycling, the current value filled with the accent. [label] names the field for
 * TalkBack, as in [ChoiceScaleRow]. [initialCenter] is the cell to open centred
 * on while nothing is picked (the common range of an unset field).
 */
@Composable
fun <T> ScrollableScaleRow(
    options: List<T>,
    current: T?,
    render: (T) -> String,
    label: String? = null,
    initialCenter: T? = null,
    onSelect: (T) -> Unit,
) {
    val scrollState = rememberScrollState()
    // Open with the current value (else [initialCenter]) in view, roughly centred,
    // instead of pinned to the left edge, so a long scale (bodyweight = 111
    // options, running km/wk = 150) doesn't hide the selection off-screen.
    // Layout-measured, not an item-width guess: the anchor cell reports its own
    // centre and the row reports its viewport, so the target is exact. Runs once
    // (guarded) so it never fights the user's manual scroll, and seeded
    // already-done when there is no anchor in the list, so the first pick never
    // scrolls the row under the finger.
    val anchor = current?.takeIf { it in options } ?: initialCenter?.takeIf { it in options }
    var didInitialScroll by rememberSaveable { mutableStateOf(anchor == null) }
    var viewportWidth by remember { mutableIntStateOf(0) }
    var anchorCenter by remember { mutableIntStateOf(-1) }
    LaunchedEffect(anchorCenter, viewportWidth, scrollState.maxValue) {
        if (!didInitialScroll && anchorCenter >= 0 && viewportWidth > 0) {
            scrollState.scrollTo((anchorCenter - viewportWidth / 2).coerceIn(0, scrollState.maxValue))
            didInitialScroll = true
        }
    }
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .onSizeChanged { viewportWidth = it.width }
            .horizontalEdgeFade(scrollState)
            .horizontalScroll(scrollState)
            // End inset inside the scroll content: the last cell clears the fade
            // and the card edge when scrolled fully right.
            .padding(end = Space.Card.dp)
            .selectableGroup(),
        horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
    ) {
        options.forEach { opt ->
            val selected = opt == current
            Box(
                modifier = Modifier
                    .heightIn(min = ActionMinHeight)
                    .choiceCell(selected)
                    .selectable(selected = selected, role = Role.RadioButton, onClick = { onSelect(opt) })
                    .then(scaleCellDescription(label, render(opt)))
                    .widthIn(min = ActionMinHeight)
                    .padding(vertical = Space.Md.dp, horizontal = Space.Md.dp)
                    .then(
                        // Record the selected cell's content-space centre. Scroll is 0
                        // on the first layout pass, so positionInParent is the intrinsic
                        // position we scroll toward.
                        if (opt == anchor) {
                            Modifier.onGloballyPositioned {
                                anchorCenter = (it.positionInParent().x + it.size.width / 2f).toInt()
                            }
                        } else {
                            Modifier
                        },
                    ),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    render(opt),
                    color = choiceCellContent(selected),
                    style = Type.Body.merge(TabularFigures),
                    maxLines = 1,
                )
            }
        }
    }
}

/** "label, value" spoken for one scale cell; no-op when the row has no label. */
private fun scaleCellDescription(label: String?, value: String): Modifier =
    if (label == null) Modifier else Modifier.semantics { contentDescription = "$label, $value" }

/** Row and column gap between [ChoiceChip]s in a wrapping (FlowRow) group. */
val ChoiceChipGap = Space.Sm.dp

/**
 * The ONE text choice chip (workout type, run type, start gates, pain areas): the
 * shared [choiceCell] look, 40dp visual inside a ≥48dp touch target, `Type.Chip`
 * label. [role] RadioButton = one option of a pick-one group (`selectable`);
 * Checkbox = an independent toggle (`toggleable`) that also shows a leading
 * check when on, so two gates side by side never read as an unpicked radio
 * pair. [description] replaces the spoken label when the visible one is terse.
 * Lay several out in a FlowRow spaced by [ChoiceChipGap].
 */
@Composable
fun ChoiceChip(
    label: String,
    selected: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    role: Role = Role.RadioButton,
    description: String? = null,
) {
    val content = choiceCellContent(selected)
    Row(
        modifier = modifier
            .minimumInteractiveComponentSize()
            .heightIn(min = ChipVisualHeight)
            .widthIn(min = ActionMinHeight)
            .choiceCell(selected)
            .then(
                if (role == Role.Checkbox) {
                    Modifier.toggleable(value = selected, role = role, onValueChange = { onClick() })
                } else {
                    Modifier.selectable(selected = selected, role = role, onClick = onClick)
                },
            )
            .then(if (description != null) Modifier.semantics { contentDescription = description } else Modifier)
            .padding(horizontal = Space.Card.dp, vertical = Space.Sm.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.Xs.dp, Alignment.CenterHorizontally),
    ) {
        if (selected && role == Role.Checkbox) {
            Icon(
                painterResource(R.drawable.ic_ui_check),
                contentDescription = null,
                tint = content,
                modifier = Modifier.size(18.dp),
            )
        }
        // Wraps rather than ellipsizing at a large font scale: a chip grows a line
        // taller instead of cutting its option text.
        Text(label, color = content, style = Type.Chip)
    }
}

/** Run types in the order a runner thinks of them, easiest first. */
private val RunTypeOrder = listOf(
    WorkoutType.Easy,
    WorkoutType.Recovery,
    WorkoutType.Steady,
    WorkoutType.LongRun,
    WorkoutType.Tempo,
    WorkoutType.Interval,
)

/**
 * The run's type as a single-select [ChoiceChip] FlowRow under its [FieldLabel]
 * (UI sweep §9): Easy, Recovery, Steady, Long run, Tempo, Interval. USER DATA only;
 * [onSelect] gets the tapped type, and the caller toggles a re-tapped type off.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun RunTypeChips(selected: WorkoutType?, onSelect: (WorkoutType) -> Unit, modifier: Modifier = Modifier) {
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
        FieldLabel(stringResource(R.string.history_workout_type))
        FlowRow(
            modifier = Modifier.fillMaxWidth().selectableGroup(),
            horizontalArrangement = Arrangement.spacedBy(ChoiceChipGap),
            verticalArrangement = Arrangement.spacedBy(ChoiceChipGap),
        ) {
            RunTypeOrder.forEach { wt ->
                ChoiceChip(
                    label = wt.label,
                    selected = wt == selected,
                    description = stringResource(R.string.history_workout_type_cd, wt.label),
                    onClick = { onSelect(wt) },
                    modifier = Modifier.testTag("run-type-${wt.name}"),
                )
            }
        }
    }
}

/**
 * One compact stat cell for a horizontal summary strip (History week header),
 * using the app-wide tile anatomy: big tabular value (with optional small unit)
 * over its label. A [RowScope] extension so callers place several across a row
 * with `weight`. A [value] of [EmptyValuePlaceholder] ("nothing logged") renders
 * as the faint dash with no unit, never a "0.0 t" that reads as a measurement.
 */
@Composable
fun RowScope.StatTile(value: String, unit: String?, label: String, glossaryKey: String? = null) {
    // History week-strip anatomy (spec 03-history §1): value ABOVE (19sp
    // ExtraBold, tabular) with the unit inline muted, label below-left.
    // fillMaxHeight: in a `height(IntrinsicSize.Min)` row every tile takes the
    // tallest tile's height, so a glossary ⓘ never leaves a ragged bottom edge.
    Column(
        modifier = Modifier
            .weight(1f)
            .fillMaxHeight()
            .clip(RoundedCornerShape(12.dp))
            .background(BgElevated)
            .padding(horizontal = Space.Card.dp, vertical = Space.Md.dp + Space.Xs.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Xs.dp),
    ) {
        val empty = value == EmptyValuePlaceholder
        Row(verticalAlignment = Alignment.Bottom) {
            Text(
                value,
                color = if (empty) OnBgFaint else OnBgBody,
                style = Type.StatValue,
                maxLines = 1,
            )
            if (unit != null && !empty) {
                Text(" $unit", color = OnBgFaint, style = Type.Caption.merge(TabularFigures))
            }
        }
        // Label, with an optional glossary affordance for jargon tiles.
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
            Text(label, color = OnBgFaint, style = Type.Caption)
            glossaryKey?.let { GlossaryInfo(it) }
        }
    }
}

/** Row-disclosure chevron (`ui-chevron-right`, 16dp, `OnBgFaint`); rotates 90°
 *  when the row is expanded. One shared affordance for every tappable row; a
 *  [TextAction] that opens something tints it with its label tone. */
@Composable
fun RowChevron(expanded: Boolean = false, tint: Color = OnBgFaint) {
    Icon(
        painterResource(R.drawable.ic_ui_chevron_right),
        contentDescription = null,
        tint = tint,
        modifier = Modifier
            .size(16.dp)
            .rotate(if (expanded) 90f else 0f),
    )
}

// --- Action buttons: ONE system, three tiers ---
// * PrimaryButton: the one "do this next" of a screen or card. Solid Accent fill,
//   ExtraBold OnAccent label.
// * SecondaryButton: any other button-weight action (Edit, Export GPX, Trim run,
//   Back, Cancel). Tonal AccentTint fill, no border, Bold OnBgBody label (Accent
//   text on the tint dips under AA on Meadow-light).
// * TextAction: a quiet in-place or navigating action (Change, Skip exercise,
//   Finish, Move session ›, Delete). Bold Accent label (DangerText when it destroys
//   something), a trailing chevron only when it opens something, no fill.
// Both buttons share [ActionShape] and a ≥48dp height; a disabled button keeps its
// pill ([DisabledActionFill] + [DisabledActionBorder]) with an OnBgMuted label.
// Every tier keeps a ≥48dp touch target. Outlined buttons are NOT part of the
// system: an outline reads as an input field or a selected chip, and a danger
// outline weighs as much as the primary beside it.

/** Corner shape of every [PrimaryButton] / [SecondaryButton]. */
val ActionShape = RoundedCornerShape(Space.Card.dp)

/**
 * The primary action: solid `Accent` fill, ExtraBold `OnAccent` label, [ActionShape],
 * ≥[minHeight] (a pinned editor action passes 56dp). Width is the caller's
 * (`Modifier.fillMaxWidth()` or a `weight` in a row). The label wraps rather than
 * clipping at a large font scale. [description] replaces the spoken label when the
 * same label repeats on one screen ("Log set 1 of 4: Back Squat").
 */
@Composable
fun PrimaryButton(
    label: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    description: String? = null,
    minHeight: Dp = ActionMinHeight,
) {
    ActionButton(label, onClick, modifier, enabled, description, minHeight, Accent, OnAccent, FontWeight.ExtraBold)
}

/** The secondary action: tonal [AccentTint] fill, no border, Bold `OnBgBody`
 *  label; otherwise identical to [PrimaryButton]. */
@Composable
fun SecondaryButton(
    label: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    description: String? = null,
    minHeight: Dp = ActionMinHeight,
) {
    ActionButton(
        label, onClick, modifier, enabled, description, minHeight,
        AccentTint, OnBgBody, FontWeight.Bold,
    )
}

@Composable
private fun ActionButton(
    label: String,
    onClick: () -> Unit,
    modifier: Modifier,
    enabled: Boolean,
    description: String?,
    minHeight: Dp,
    fill: Color,
    labelColor: Color,
    weight: FontWeight,
) {
    Box(
        modifier = modifier
            .heightIn(min = minHeight)
            .clip(ActionShape)
            .then(
                if (enabled) {
                    Modifier.background(fill)
                } else {
                    Modifier.background(DisabledActionFill).border(1.dp, DisabledActionBorder, ActionShape)
                },
            )
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick)
            .semantics { if (description != null) contentDescription = description }
            .padding(horizontal = Space.Card.dp, vertical = Space.Sm.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            label,
            color = if (enabled) labelColor else OnBgMuted,
            style = Type.Body.copy(fontWeight = weight),
            textAlign = TextAlign.Center,
        )
    }
}

/**
 * The tertiary (text) action: a Bold `Accent` label ([DangerText] when [danger])
 * with no fill, inside a ≥48dp touch target, so it reads as a control and never as
 * a second section label. [chevron] adds a trailing chevron in the label tone when
 * the action opens something (a picker, a sheet, another screen); in-place actions
 * (Edit, Finish, Undo, Delete) go without. [tint] recolours the SAME action for a
 * non-default surface only (white on the danger SafetyBanner, where Accent fails);
 * style and behaviour never vary per call site. See [TextActionInset] for start
 * alignment.
 */
@Composable
fun TextAction(
    label: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    description: String? = null,
    danger: Boolean = false,
    chevron: Boolean = false,
    enabled: Boolean = true,
    tint: Color? = null,
) {
    val tone = when {
        !enabled -> OnBgMuted
        tint != null -> tint
        danger -> DangerText
        else -> Accent
    }
    Row(
        modifier = modifier
            .heightIn(min = ActionMinHeight)
            .widthIn(min = ActionMinHeight)
            .clip(RoundedCornerShape(Space.Md.dp))
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick)
            .semantics { if (description != null) contentDescription = description }
            .padding(horizontal = TextActionInset),
        horizontalArrangement = Arrangement.spacedBy(Space.Xs.dp, Alignment.CenterHorizontally),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // Weighted (not filling): a label that fills the line wraps, and the
        // chevron keeps its room instead of being squeezed out at a large font.
        Text(label, color = tone, style = Type.Body.copy(fontWeight = FontWeight.Bold), modifier = Modifier.weight(1f, fill = false))
        if (chevron) RowChevron(tint = tone)
    }
}

/** The ONE tile/card overline (chrome §3): 12sp Bold UPPERCASE, `OnBgFaint`,
 *  top-left. Every data tile and card label goes through this. */
@Composable
fun TileOverline(text: String, color: Color = OnBgFaint) {
    Text(text.uppercase(Locale.US), color = color, style = Type.Overline)
}

/** Section overline: the top-left accent UPPERCASE label over a screen section. */
@Composable
fun SectionOverline(text: String) {
    Text(
        text.uppercase(Locale.US),
        color = Accent,
        style = Type.Section,
        modifier = Modifier.fillMaxWidth().padding(top = Space.Sm.dp),
    )
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
 * segments, drawn as the app's one [SegmentTrack]. Tab role (it switches a view).
 * No third option, no counts in the labels. [compact] is for a secondary toggle
 * that refines a section rather than switching the page (a chart's range on its
 * overline row): same solid Accent selection, but the track wraps its labels and
 * draws 32dp tall (touch targets stay ≥48dp), so it never weighs as much as a
 * full-width page filter above it.
 */
@Composable
fun TwoSegmentRow(left: String, right: String, selectedIndex: Int, compact: Boolean = false, onSelect: (Int) -> Unit) {
    SegmentTrack(listOf(left, right), selectedIndex, Role.Tab, compact, onSelect)
}

/**
 * The ONE segmented-control look, shared by [TwoSegmentRow] and [SegmentedEnumRow]:
 * a borderless track in the [ChoiceCellFill] wash (40dp visual, so it reads on any
 * ground; each segment keeps a ≥48dp touch target). Selected = a solid `Accent`
 * segment with bold `OnAccent` text and no outline ring; unselected = plain
 * `OnBgBody` text on the track (`OnBgMuted` on the wash dips under AA on the light
 * grounds). No check glyph: the fill is the selection. [compact]: the track wraps
 * its content and draws 32dp tall inside the same 48dp touch row.
 */
@Composable
private fun SegmentTrack(
    labels: List<String>,
    selectedIndex: Int,
    role: Role,
    compact: Boolean = false,
    onSelect: (Int) -> Unit,
) {
    val trackFill = ChoiceCellFill
    // The visible track is inset top and bottom inside the 48dp touch row, so the
    // control reads 40dp (compact: 32dp) tall while each segment's tap target spans
    // the full 48dp.
    val touchInset = if (compact) CompactSegmentTouchInset else SegmentTouchInset
    Row(
        modifier = Modifier
            .then(if (compact) Modifier else Modifier.fillMaxWidth())
            .selectableGroup()
            .segmentTrackGround(trackFill, touchInset)
            .padding(horizontal = SegmentTrackPadding),
        horizontalArrangement = Arrangement.spacedBy(SegmentTrackPadding),
    ) {
        labels.forEachIndexed { i, label ->
            val selected = i == selectedIndex
            Box(
                modifier = Modifier
                    .then(if (compact) Modifier.widthIn(min = ActionMinHeight) else Modifier.weight(1f))
                    .heightIn(min = ActionMinHeight)
                    // Selectable so TalkBack announces the selected state (and
                    // "1 of N") instead of a bare, stateless click target.
                    .selectable(
                        selected = selected,
                        role = role,
                        onClick = { onSelect(i) },
                    )
                    // Track inset + its 2dp inner gap.
                    .padding(vertical = touchInset + SegmentTrackPadding),
                contentAlignment = Alignment.Center,
            ) {
                Box(
                    modifier = Modifier
                        .then(if (compact) Modifier.widthIn(min = 44.dp) else Modifier.fillMaxWidth())
                        .heightIn(min = if (compact) 28.dp else 36.dp)
                        .clip(SegmentShape)
                        .then(if (selected) Modifier.background(Accent) else Modifier)
                        .padding(horizontal = if (compact) Space.Md.dp else SegmentLabelInset),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        label,
                        color = if (selected) OnAccent else OnBgBody,
                        style = if (selected) Type.Chip else Type.Chip.copy(fontWeight = FontWeight.Medium),
                        textAlign = TextAlign.Center,
                    )
                }
            }
        }
    }
}

/** Vertical gap between a 48dp touch row and a compact track's 32dp visual. */
private val CompactSegmentTouchInset = 8.dp

/** Vertical gap between a 48dp touch row and its 40dp visible track. */
internal val SegmentTouchInset = 4.dp

/** Track inner padding and the gap between segments. */
internal val SegmentTrackPadding = Space.Xs.dp

/** A selected segment's fill shape inside the track. */
internal val SegmentShape = RoundedCornerShape(Space.Md.dp - Space.Xs.dp)

/** The segmented track's ground: a [fill] round rect inset [inset] top and bottom
 *  inside the 48dp touch row, shared by [SegmentTrack] and [RatingScale]. */
internal fun Modifier.segmentTrackGround(fill: Color, inset: Dp): Modifier = drawBehind {
    val px = inset.toPx()
    drawRoundRect(
        fill,
        topLeft = Offset(0f, px),
        size = Size(size.width, size.height - px * 2f),
        cornerRadius = CornerRadius(Space.Md.dp.toPx()),
    )
}

/** Horizontal padding between a segment's edge and its label. */
private val SegmentLabelInset = Space.Sm.dp

/** Width one segment's label may take inside a [SegmentTrack] of [count] segments
 *  spanning [trackWidth]: track padding, inter-segment gaps and label insets come
 *  off, floored by 1dp so a label that "just fits" never clips. */
private fun segmentLabelRoom(trackWidth: Dp, count: Int): Dp =
    (trackWidth - SegmentTrackPadding * 2 - SegmentTrackPadding * (count - 1)) / count -
        SegmentLabelInset * 2 - 1.dp

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

/** Gap between theme swatch cards, across a row and between grid rows. */
private val ThemeSwatchGap = Space.Md.dp

/** Side padding inside a theme swatch card (also the label's measured room). */
private val ThemeSwatchInset = Space.Md.dp

/**
 * The theme picker: one [ThemeSwatchCard] per [AppTheme], in enum order. Four
 * across while every name fits on one line in its card at the current font
 * scale (measured in the exact style the card renders, like [SegmentedEnumRow]);
 * otherwise a 2×2 grid, so a name never breaks mid-word at 130 % / 200 % type.
 * A radio group to TalkBack.
 */
@Composable
fun ThemeSwatchPicker(current: AppTheme, onSelect: (AppTheme) -> Unit) {
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val measurer = rememberTextMeasurer()
        val density = LocalDensity.current
        val themes = AppTheme.entries
        val labelRoomPx = with(density) {
            ((maxWidth - ThemeSwatchGap * (themes.size - 1)) / themes.size - ThemeSwatchInset * 2).toPx()
        }
        val fits = themes.all {
            measurer.measure(it.label, style = Type.Chip, maxLines = 1, softWrap = false).size.width <= labelRoomPx
        }
        Column(
            modifier = Modifier.fillMaxWidth().selectableGroup(),
            verticalArrangement = Arrangement.spacedBy(ThemeSwatchGap),
        ) {
            themes.chunked(if (fits) themes.size else 2).forEach { row ->
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(ThemeSwatchGap),
                ) {
                    row.forEach { t -> ThemeSwatchCard(t, selected = t == current) { onSelect(t) } }
                }
            }
        }
    }
}

/**
 * A theme choice as a swatch card: a mini preview of the theme's LIGHT face (its
 * ground with an elevated-card bar and its accent dot) over its name, drawn as a
 * [choiceCard] (selected = solid Accent, no outline, like every other selection).
 * The light face is each theme's most recognisable one (rose blush, mint, butter
 * cream, white paper) and it reads the theme's own [AppTheme.light] palette, so
 * every card previews itself regardless of the active theme or dark mode. The
 * preview plate keeps a 1dp hairline so a white ground (Paper) never melts into a
 * light card, and an OnAccent hairline on the selected accent fill.
 */
@Composable
private fun RowScope.ThemeSwatchCard(theme: AppTheme, selected: Boolean, onClick: () -> Unit) {
    val pal = theme.light
    val plateShape = RoundedCornerShape(Space.Md.dp)
    val barShape = RoundedCornerShape(Space.Sm.dp)
    Column(
        modifier = Modifier
            .weight(1f)
            .choiceCard(selected)
            .selectable(selected = selected, role = Role.RadioButton, onClick = onClick)
            .padding(vertical = Space.Card.dp, horizontal = ThemeSwatchInset),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clip(plateShape)
                .background(pal.bgTop)
                .border(1.dp, if (selected) OnAccent.copy(alpha = 0.4f) else ChoiceCellBorder, plateShape)
                .padding(Space.Sm.dp),
            horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(Modifier.size(16.dp).clip(CircleShape).background(pal.accent))
            Box(
                Modifier
                    .weight(1f)
                    .height(16.dp)
                    .clip(barShape)
                    .background(pal.bgElevated)
                    .border(1.dp, pal.onBgFaint.copy(alpha = HairlineAlpha.Strong), barShape),
            )
        }
        // Wraps rather than clipping if a name still outgrows its card.
        Text(
            theme.label,
            color = choiceCellContent(selected),
            style = Type.Chip,
            textAlign = TextAlign.Center,
        )
    }
}
