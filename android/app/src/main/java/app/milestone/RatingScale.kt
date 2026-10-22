package app.milestone

import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.hideFromAccessibility
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.setProgress
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt

/** Least room a digit needs on each side inside its segment. */
private val RatingLabelInset = 2.dp

/**
 * The ONE control for a 0–N rating (VDS §6, UI sweep §0.4): a single segmented
 * track with one contiguous segment per value, 40dp visual inside a 48dp touch row.
 * Above it, [label] at the start and the picked value's word ([selectedText], e.g.
 * "5 · Hard") at the end; the word slot is always composed, so picking never moves
 * the layout. Below it, the endpoint words [startCaption] / [endCaption].
 *
 * [values] are in display order, left to right; a reversed core scale (soreness
 * 5→1) is passed reversed, never re-sorted. Unrated ([selected] null): no segment
 * is filled and the word slot is empty. Tapping the picked segment again clears it
 * when [clearable].
 *
 * The track is ONE adjustable accessibility node ("Effort, 5 of 10, Hard"; swipe up
 * or down to change), with [spokenText] as its state. [hint] is a quiet note in the
 * word slot while nothing is picked ("optional", "Last time: 4"), never a selection.
 * Each segment keeps the test tag "$testTag-<value>" in the unmerged tree, so a
 * test click on it picks it.
 * When a value's digits do not fit their segment at the current font scale, the
 * track falls back to [ChoiceScaleRow]'s balanced rows.
 */
@Composable
fun RatingScale(
    label: String,
    values: List<Int>,
    selected: Int?,
    onSelect: (Int?) -> Unit,
    startCaption: String,
    endCaption: String,
    selectedText: (Int) -> String,
    modifier: Modifier = Modifier,
    clearable: Boolean = true,
    testTag: String = "rating-scale",
    spokenText: (Int) -> String = selectedText,
    hint: String? = null,
) {
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(Modifier.weight(1f)) { FieldLabel(label) }
            if (selected != null || hint == null) {
                Text(
                    selected?.let(selectedText).orEmpty(),
                    color = OnBgBody,
                    style = Type.Body.copy(fontWeight = FontWeight.Bold).merge(TabularFigures),
                    textAlign = TextAlign.End,
                    // The track speaks the value; the word stays in the tree for tests.
                    modifier = Modifier.semantics { hideFromAccessibility() },
                )
            } else {
                Text(hint, color = OnBgFaint, style = Type.Caption.merge(TabularFigures), textAlign = TextAlign.End)
            }
        }
        BoxWithConstraints(Modifier.fillMaxWidth()) {
            val measurer = rememberTextMeasurer()
            val density = LocalDensity.current
            val n = values.size.coerceAtLeast(1)
            val widestPx = remember(values, measurer) {
                values.maxOfOrNull { measurer.measure("$it", Type.Chip.merge(TabularFigures), maxLines = 1, softWrap = false).size.width } ?: 0
            }
            val roomPx = with(density) {
                ((maxWidth - SegmentTrackPadding * 2 - SegmentTrackPadding * (n - 1)) / n - RatingLabelInset * 2).toPx()
            }
            if (widestPx <= roomPx) {
                RatingTrack(label, values, selected, onSelect, clearable, testTag, spokenText)
            } else {
                Box(Modifier.testTag(testTag)) {
                    ChoiceScaleRow(values, selected ?: Int.MIN_VALUE, { "$it" }, label = label) { v ->
                        onSelect(if (clearable && v == selected) null else v)
                    }
                }
            }
        }
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
        ) {
            Text(startCaption, color = OnBgMuted, style = Type.Caption.merge(TabularFigures), modifier = Modifier.weight(1f))
            Text(endCaption, color = OnBgMuted, style = Type.Caption.merge(TabularFigures), textAlign = TextAlign.End)
        }
    }
}

@Composable
private fun RatingTrack(
    label: String,
    values: List<Int>,
    selected: Int?,
    onSelect: (Int?) -> Unit,
    clearable: Boolean,
    testTag: String,
    spokenText: (Int) -> String,
) {
    val trackFill = ChoiceCellFill
    val last = values.lastIndex
    val index = values.indexOf(selected)
    val state = selected?.let(spokenText) ?: stringResource(R.string.rating_unrated)
    val currentSelected by rememberUpdatedState(selected)
    val currentOnSelect by rememberUpdatedState(onSelect)
    val currentClearable by rememberUpdatedState(clearable)
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .height(ActionMinHeight)
            .testTag(testTag)
            .segmentTrackGround(trackFill, SegmentTouchInset)
            .pointerInput(values) {
                detectTapGestures { pos ->
                    val pad = SegmentTrackPadding.toPx()
                    val span = (size.width - pad * 2f).coerceAtLeast(1f)
                    val i = ((pos.x - pad) / span * values.size).toInt().coerceIn(0, last)
                    val v = values[i]
                    currentOnSelect(if (currentClearable && v == currentSelected) null else v)
                }
            }
            .semantics(mergeDescendants = true) {
                contentDescription = label
                stateDescription = state
                progressBarRangeInfo = ProgressBarRangeInfo(
                    current = index.coerceAtLeast(0).toFloat(),
                    range = 0f..last.coerceAtLeast(1).toFloat(),
                    steps = (last - 1).coerceAtLeast(0),
                )
                setProgress { target ->
                    val i = target.roundToInt().coerceIn(0, last)
                    if (values[i] != selected) onSelect(values[i])
                    true
                }
            }
            .padding(horizontal = SegmentTrackPadding, vertical = SegmentTouchInset + SegmentTrackPadding),
        horizontalArrangement = Arrangement.spacedBy(SegmentTrackPadding),
    ) {
        values.forEach { v ->
            val on = v == selected
            Box(
                modifier = Modifier
                    .weight(1f)
                    .fillMaxHeight()
                    .testTag("$testTag-$v")
                    .clip(SegmentShape)
                    .then(if (on) Modifier.background(Accent) else Modifier),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    "$v",
                    color = if (on) OnAccent else OnBgBody,
                    style = (if (on) Type.Chip else Type.Chip.copy(fontWeight = FontWeight.Medium)).merge(TabularFigures),
                    maxLines = 1,
                    softWrap = false,
                    modifier = Modifier.clearAndSetSemantics {},
                )
            }
        }
    }
}

/** The CR-10 effort values, left to right. */
private val EffortValues = (0..10).toList()

/** The CR-10 word for an effort value (0 Rest … 10 Maximal). */
@Composable
internal fun effortWord(n: Int): String = stringResource(
    when (n) {
        0 -> R.string.history_effort_anchor_0
        1 -> R.string.history_effort_anchor_1
        2 -> R.string.history_effort_anchor_2
        3 -> R.string.history_effort_anchor_3
        4 -> R.string.history_effort_anchor_4
        5 -> R.string.history_effort_anchor_5
        6 -> R.string.history_effort_anchor_6
        7 -> R.string.history_effort_anchor_7
        8 -> R.string.history_effort_anchor_8
        9 -> R.string.history_effort_anchor_9
        else -> R.string.history_effort_anchor_10
    },
)

/**
 * The whole-session effort (CR-10, 0–10) as a [RatingScale]: identical on Log run,
 * the finish review and the Rate run sheet. Skippable; tapping the picked value
 * clears it. Test tags: "effort-scale", segments "effort-scale-<n>".
 */
@Composable
fun EffortScale(selected: Int?, onSelect: (Int?) -> Unit, modifier: Modifier = Modifier) {
    val shown = EffortValues.map { stringResource(R.string.history_effort_selected, it, effortWord(it)) }
    val spoken = EffortValues.map { stringResource(R.string.rating_effort_spoken, it, effortWord(it)) }
    RatingScale(
        label = stringResource(R.string.history_effort_overline),
        values = EffortValues,
        selected = selected,
        onSelect = onSelect,
        startCaption = stringResource(R.string.rating_effort_start),
        endCaption = stringResource(R.string.rating_effort_end),
        selectedText = { shown[it] },
        modifier = modifier,
        testTag = "effort-scale",
        spokenText = { spoken[it] },
    )
}
