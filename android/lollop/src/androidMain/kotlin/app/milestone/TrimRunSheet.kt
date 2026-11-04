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
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.RangeSlider
import androidx.compose.material3.RangeSliderState
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.listSaver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp

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
 *
 * Takes a loaded, [RunRoute.trimmable] [route] (see [rememberRunRoute] for why the
 * sheet never loads its own).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun TrimRunSheetContent(
    run: RunResultView,
    route: RunRoute,
    onCancel: () -> Unit,
    onConfirm: (keepFromSec: Long, keepUntilSec: Long) -> Unit,
) {
    val unit = rememberDistanceUnit()
    val points = route.track
    Column(
        modifier = Modifier.sheetBody(rememberScrollState()),
        verticalArrangement = Arrangement.spacedBy(Space.Lg.dp),
    ) {
        SheetTitle(stringResource(R.string.main_trim_run))

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
        val keptTime = formatElapsed(trimKeptMovingSec(points, firstKept, lastKept, run.duration_min * 60.0).toLong())
        val headKm = trimPreviewShare(if (firstKept >= 0) points[firstKept].cumKm else trackKm, trackKm, run.distance_km)
        val tailKm = trimPreviewShare(if (lastKept >= 0) trackKm - points[lastKept].cumKm else 0.0, trackKm, run.distance_km)
        val headLabel = runDistLabel(headKm, unit)
        val tailLabel = runDistLabel(tailKm, unit)

        TrimRouteMap(route.segments, keepFromSec, keepUntilSec, run.distance_km)

        val startDesc = stringResource(R.string.trim_start_thumb)
        val endDesc = stringResource(R.string.trim_end_thumb)
        val res = LocalResources.current
        val startState = stringResource(R.string.trim_removed_state, spokenRunDistance(res, headKm, unit))
        val endState = stringResource(R.string.trim_removed_state, spokenRunDistance(res, tailKm, unit))
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
            ConfirmDialog(
                title = stringResource(R.string.main_trim_confirm_title, runDistLabel(keptKm, unit), keptTime),
                body = stringResource(R.string.main_trim_confirm_message, headLabel, tailLabel),
                confirmLabel = stringResource(R.string.main_trim_confirm),
                onDismiss = { confirm = false },
                onConfirm = {
                    confirm = false
                    onConfirm(keepFromSec, keepUntilSec)
                },
            )
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

/** The trim slider's track: `OnBgBody` @ 12% end to end, `Accent` over the kept
 *  window (mirrored under RTL, as the slider's thumbs are); no ticks. */
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
        // A DrawScope is not mirrored under RTL, while the RangeSlider's thumbs are.
        val left = if (layoutDirection == LayoutDirection.Rtl) 1f - to else from
        drawRoundRect(
            active,
            topLeft = Offset(size.width * left, 0f),
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

/**
 * One parsed GPX fix reduced to what the trim preview needs: its absolute
 * epoch-second timestamp, the cumulative haversine distance (km) and the cumulative
 * MOVING seconds from the track start up to this point. PREVIEW figures ONLY — the
 * of-record distance/duration are always re-derived in the core after a trim event.
 */
data class TimedTrackPoint(val timeSec: Long, val cumKm: Double, val movingSec: Double)

/**
 * Parsed route [segments] reshaped for the trim preview: every fix flattened into one
 * time-ordered list carrying its epoch-second time, the cumulative distance in km
 * (the same running haversine sum [RouteTrack] colours by) and the cumulative moving
 * time. A segment gap (a GPS pause between `<trkseg>`s) adds neither; within a
 * segment a leg adds its seconds only when it clears [MIN_MOVING_SPEED_MPS], the
 * core's moving-leg rule (`running::leg`), so standing still counts no time either.
 */
fun timedTrackFromSegments(segments: List<List<GpxFix>>): List<TimedTrackPoint> {
    val out = ArrayList<TimedTrackPoint>()
    var runningM = 0.0
    var movingSec = 0.0
    for (seg in segments) {
        var prev: GpxFix? = null
        for (fix in seg) {
            if (prev != null) {
                val legM = haversineM(prev.lat, prev.lon, fix.lat, fix.lon)
                runningM += legM
                val dt = (fix.timeSec - prev.timeSec).toDouble()
                if (movingLegKm(legM / 1000.0, dt) != null) movingSec += dt
            }
            out += TimedTrackPoint(fix.timeSec, runningM / 1000.0, movingSec)
            prev = fix
        }
    }
    return out
}
