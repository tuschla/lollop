package app.milestone

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.RangeSlider
import androidx.compose.material3.RangeSliderState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.saveable.listSaver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** Saves a [RangeSlider] window as its two ends. */
private val FloatRangeSaver = listSaver<ClosedFloatingPointRange<Float>, Float>(
    save = { listOf(it.start, it.endInclusive) },
    restore = { it[0]..it[1] },
)

/** Thickness of the trim slider's track (spec §4.3). */
private val TrimTrackHeight = 6.dp

/** Visual size of a trim thumb, inside its 48dp target. */
private val TrimThumbSize = 24.dp

/** Opacity of the trim slider's inactive `OnBgBody` track. */
private const val TrimInactiveAlpha = 0.12f

/**
 * Trim sheet for a saved GPS run's head/tail (spec §4): the "Trim run" title, the
 * route card with the kept part in the progression ramp and the trimmed ends dashed
 * (markers at the new ends, moving live), a token-coloured [RangeSlider] over the
 * track's [firstFixSec, lastFixSec] whose thumbs mirror the start ring and finish
 * disc, a "Keeps <dist> · <moving time>" summary with what each end removes, then Cancel
 * (text) and Trim. Trim stays disabled until a handle has moved and confirms first;
 * on confirm [onConfirm] gets the absolute unix-second bounds. Distance and time
 * here are PREVIEW figures; the core re-derives the of-record ones from the trim.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun TrimRunSheetContent(
    run: RunResultView,
    onCancel: () -> Unit,
    onConfirm: (keepFromSec: Long, keepUntilSec: Long) -> Unit,
) {
    val unit = rememberDistanceUnit()
    // Same on-demand, off-main-thread GPX fetch as the detail sheet, keyed on the
    // run's stable identity so it loads once per opened run; null while loading.
    val segsAndPoints: Pair<List<List<GpxFix>>, List<TimedTrackPoint>>? by produceState<Pair<List<List<GpxFix>>, List<TimedTrackPoint>>?>(
        initialValue = null,
        run.entry_id,
        run.observed_at,
    ) {
        value = withContext(Dispatchers.IO) {
            // Fetch AND parse the GPX ONCE off the main thread: the parsed segments feed
            // the trim map, and the timed track (for the slider + readout) is derived
            // from the SAME parse, so nothing re-parses on the main thread.
            val g = Core.exportGpx(run.entry_id, run.observed_at)
            val segs = if (g.isNullOrBlank()) emptyList() else parseGpx(g)
            segs to timedTrackFromSegments(segs)
        }
    }
    val segments = segsAndPoints?.first.orEmpty()
    val points = segsAndPoints?.second.orEmpty()
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .verticalScroll(rememberScrollState())
            .navigationBarsPadding()
            .padding(horizontal = Space.Sheet.dp)
            .padding(bottom = Space.Lg.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Lg.dp),
    ) {
        Text(stringResource(R.string.main_trim_run), color = OnBgBody, style = Type.Title)

        val trimmable = points.size >= 2 && points.last().timeSec > points.first().timeSec
        if (trimmable) {
            val firstFixSec = points.first().timeSec
            val lastFixSec = points.last().timeSec
            // The slider works in RELATIVE seconds [0, span]: a Float can't represent
            // an absolute unix second (~1.7e9) exactly, but a run's span (at most a
            // few hours) sits well inside Float's exact-integer range.
            val spanF = (lastFixSec - firstFixSec).toFloat()
            // Saveable (keyed on the track) so a rotation keeps the chosen window under
            // a still-open confirm, which would otherwise trim the whole track.
            var range by rememberSaveable(firstFixSec, lastFixSec, stateSaver = FloatRangeSaver) {
                mutableStateOf(0f..spanF)
            }
            val keepFromSec = (firstFixSec + Math.round(range.start).toLong())
                .coerceIn(firstFixSec, lastFixSec)
            val keepUntilSec = (firstFixSec + Math.round(range.endInclusive).toLong())
                .coerceIn(firstFixSec, lastFixSec)

            // Fixes inside the window are contiguous (points are time-ordered), so a
            // single walk finds the first/last kept index with no per-frame alloc.
            var firstKept = -1
            var lastKept = -1
            for (i in points.indices) {
                val t = points[i].timeSec
                if (t in keepFromSec..keepUntilSec) {
                    if (firstKept == -1) firstKept = i
                    lastKept = i
                }
            }
            val keptCount = if (firstKept == -1) 0 else lastKept - firstKept + 1
            val trackKm = points.last().cumKm
            val keptKm = if (keptCount >= 2) {
                trimPreviewShare(points[lastKept].cumKm - points[firstKept].cumKm, trackKm, run.distance_km)
            } else {
                0.0
            }
            val keptTime = fmtRunTime(trimKeptMovingSec(points, firstKept, lastKept, run.duration_min * 60.0))
            val headKm = trimPreviewShare(if (firstKept >= 0) points[firstKept].cumKm else trackKm, trackKm, run.distance_km)
            val tailKm = trimPreviewShare(if (lastKept >= 0) trackKm - points[lastKept].cumKm else 0.0, trackKm, run.distance_km)
            val headLabel = runDistLabel(headKm, unit)
            val tailLabel = runDistLabel(tailKm, unit)

            if (segments.isNotEmpty()) TrimRouteMap(segments, run.entry_id, keepFromSec, keepUntilSec)

            val startDesc = stringResource(R.string.trim_start_thumb)
            val endDesc = stringResource(R.string.trim_end_thumb)
            val startState = stringResource(R.string.trim_removed_state, headLabel)
            val endState = stringResource(R.string.trim_removed_state, tailLabel)
            RangeSlider(
                value = range,
                onValueChange = { range = it },
                valueRange = 0f..spanF,
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = ActionMinHeight),
                startThumb = { TrimThumb(start = true, description = startDesc, state = startState) },
                endThumb = { TrimThumb(start = false, description = endDesc, state = endState) },
                track = { TrimTrack(it) },
            )

            // No-op guard: keeping <2 fixes, or the whole track (both bounds at the
            // extremes), can't produce a meaningful trim.
            val wholeTrack = keepFromSec <= firstFixSec && keepUntilSec >= lastFixSec
            val canApply = keptCount >= 2 && !wholeTrack
            Column(verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
                Text(
                    stringResource(R.string.main_trim_keeps, runDistLabel(keptKm, unit), keptTime),
                    color = OnBgBody,
                    style = Type.Title.merge(TabularFigures),
                )
                Text(
                    if (wholeTrack) stringResource(R.string.trim_hint) else stringResource(R.string.trim_removes, headLabel, tailLabel),
                    color = OnBgMuted,
                    style = Type.Caption.merge(TabularFigures),
                )
            }

            var confirm by rememberSaveable { mutableStateOf(false) }
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
            ) {
                TextAction(stringResource(R.string.main_cancel), onClick = onCancel, modifier = Modifier.offset(x = -TextActionInset))
                PrimaryButton(
                    stringResource(R.string.main_trim_confirm),
                    onClick = { confirm = true },
                    enabled = canApply,
                    modifier = Modifier.weight(1f),
                    minHeight = LargeControlHeight,
                )
            }

            if (confirm) {
                val removedStartMin = Math.round((keepFromSec - firstFixSec) / 60.0).toInt()
                val removedEndMin = Math.round((lastFixSec - keepUntilSec) / 60.0).toInt()
                AlertDialog(
                    onDismissRequest = { confirm = false },
                    shape = RoundedCornerShape(Space.Card.dp),
                    title = {
                        Text(stringResource(R.string.main_trim_confirm_title, runDistLabel(keptKm, unit), keptTime))
                    },
                    text = {
                        Text(stringResource(R.string.main_trim_confirm_message, removedStartMin, removedEndMin))
                    },
                    confirmButton = {
                        TextButton(onClick = {
                            confirm = false
                            onConfirm(keepFromSec, keepUntilSec)
                        }) { Text(stringResource(R.string.main_trim_confirm)) }
                    },
                    dismissButton = {
                        TextButton(onClick = { confirm = false }) { Text(stringResource(R.string.main_cancel)) }
                    },
                )
            }
        } else {
            if (segsAndPoints == null) {
                // Track still loading: the card at full height, Cancel reachable.
                RouteMapPlaceholder()
                Text(stringResource(R.string.history_trim_loading), color = OnBgMuted, style = Type.Body)
            } else {
                // Loaded but not trimmable: the route for context and an honest note.
                RouteMapCard(segments, run.entry_id, distanceMarkers = false, recordKm = run.distance_km)
                Text(stringResource(R.string.main_trim_unavailable), color = OnBgMuted, style = Type.Body)
            }
            TextAction(stringResource(R.string.main_cancel), onClick = onCancel, modifier = Modifier.offset(x = -TextActionInset))
        }
    }
}

/**
 * A trim thumb that mirrors its map marker: the start a [TrimThumbSize] `BgElevated`
 * disc with the start marker's `Accent` ring, the end a solid `Accent` disc, each in
 * a 48dp target and spoken "Trim start, 0.42 mi removed".
 */
@Composable
private fun TrimThumb(start: Boolean, description: String, state: String) {
    Box(
        Modifier
            .size(ActionMinHeight)
            .semantics {
                contentDescription = description
                stateDescription = state
            },
        contentAlignment = Alignment.Center,
    ) {
        Box(
            Modifier
                .size(TrimThumbSize)
                .then(
                    if (start) {
                        Modifier.background(BgElevated, CircleShape).border(RouteMarker.StartRing, Accent, CircleShape)
                    } else {
                        Modifier.background(Accent, CircleShape)
                    },
                ),
        )
    }
}

/** The trim slider's track: `OnBgBody` @ 12% end to end, `Accent` over the kept window; no ticks. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun TrimTrack(state: RangeSliderState) {
    val inactive = OnBgBody.copy(alpha = TrimInactiveAlpha)
    val active = Accent
    Canvas(Modifier.fillMaxWidth().height(TrimTrackHeight)) {
        val span = (state.valueRange.endInclusive - state.valueRange.start).takeIf { it > 0f } ?: 1f
        val from = (state.activeRangeStart - state.valueRange.start) / span
        val to = (state.activeRangeEnd - state.valueRange.start) / span
        val r = CornerRadius(size.height / 2, size.height / 2)
        drawRoundRect(inactive, cornerRadius = r)
        drawRoundRect(
            active,
            topLeft = Offset(size.width * from, 0f),
            size = Size(size.width * (to - from), size.height),
            cornerRadius = r,
        )
    }
}

/** A trim readout figure: the raw window amount [window] as a share of the whole
 *  parsed track's ([track]), applied to the run's of-record figure ([record], the
 *  core's QC'd moving distance or moving time). The untouched window therefore
 *  reads exactly what History and the run detail show, and a trimmed one moves in
 *  proportion. Falls back to the raw figure when either total is unknown. Pure. */
internal fun trimPreviewShare(window: Double, track: Double, record: Double): Double =
    if (track > 0.0 && record > 0.0) record * (window / track) else window

/** The trim readout's kept time, whole seconds: the MOVING time between the kept
 *  fixes [firstKept]..[lastKept] (pause gaps and standing still excluded, as in
 *  [timedTrackFromSegments]) scaled by [trimPreviewShare] onto the run's of-record
 *  moving time [recordSec], so it reads like the run detail's Time, never the
 *  wall-clock span. 0 when fewer than two fixes are kept. Pure. */
internal fun trimKeptMovingSec(points: List<TimedTrackPoint>, firstKept: Int, lastKept: Int, recordSec: Double): Int {
    if (firstKept < 0 || lastKept - firstKept < 1) return 0
    val window = points[lastKept].movingSec - points[firstKept].movingSec
    return Math.round(trimPreviewShare(window, points.last().movingSec, recordSec)).toInt()
}
