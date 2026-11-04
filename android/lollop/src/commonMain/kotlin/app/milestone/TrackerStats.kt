package app.milestone

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextMeasurer
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.milestone.resources.Res
import app.milestone.resources.ic_safety_warning_triangle
import app.milestone.resources.ic_ui_check
import app.milestone.resources.run_stat_distance
import app.milestone.resources.track_cd_distance_km
import app.milestone.resources.track_cd_distance_mi
import app.milestone.resources.track_cd_hours
import app.milestone.resources.track_cd_minutes
import app.milestone.resources.track_cd_not_available
import app.milestone.resources.track_cd_pace_km
import app.milestone.resources.track_cd_pace_mi
import app.milestone.resources.track_cd_pace_status_ease_off
import app.milestone.resources.track_cd_pace_status_on_target
import app.milestone.resources.track_cd_pace_status_speed_up
import app.milestone.resources.track_cd_seconds
import app.milestone.resources.track_cd_stats
import app.milestone.resources.track_pace_status_ease_off
import app.milestone.resources.track_pace_status_on_target
import app.milestone.resources.track_pace_status_speed_up
import app.milestone.resources.track_stat_avg_pace
import app.milestone.resources.track_stat_elapsed
import app.milestone.resources.track_stat_elapsed_paused
import app.milestone.resources.track_stat_last_slice
import app.milestone.resources.track_stat_pace_now
import app.milestone.resources.track_stat_plan_done
import app.milestone.resources.track_stat_to_go
import org.jetbrains.compose.resources.painterResource
import org.jetbrains.compose.resources.stringResource

/** Opacity of the live figures while paused. */
private const val PAUSED_FIGURE_ALPHA = 0.6f

/** Starting size (sp) of a live figure beside the map. */
internal const val LIVE_FIGURE_MAX_SP = 36

/** Starting size (sp) of the live figures when no map shares the screen. */
private const val LIVE_FIGURE_LARGE_SP = 48

/** Smallest size (sp) a live figure steps down to, and the step. */
private const val LIVE_FIGURE_MIN_SP = 18
private const val LIVE_FIGURE_STEP_SP = 2

/** Line height of a live figure over its font size (sp). */
private const val LIVE_FIGURE_LEADING_SP = 4

/** Size (sp) of a live figure's unit suffix. */
private const val LIVE_FIGURE_SUFFIX_SP = 13

/**
 * The live readouts: MOVING · DISTANCE · PACE NOW at one size, the whole-run average
 * under PACE NOW, then the last completed N-minute slice. A leaf, so per-fix updates
 * recompose only these figures. Following a plan, PACE NOW gets a cue against
 * [paceTarget] and DISTANCE shows what is left of [planKm]. Screen readers get ONE
 * node with units spelled out.
 */
@Composable
internal fun TrackerStats(
    unit: DistanceUnit,
    paceBucketN: Int,
    paused: Boolean,
    large: Boolean,
    paceTarget: RunTargetView?,
    planKm: Double?,
) {
    // Elapsed is the MONOTONIC recording span (immune to an NTP step); distance is the
    // incrementally-accumulated live haversine; average pace is over that span.
    val elapsedSec by RunSession.elapsedSec.collectAsStateWithLifecycle()
    val liveKm by RunSession.distanceKm.collectAsStateWithLifecycle()
    val paceBuckets by RunSession.paceBucketsFlow.collectAsStateWithLifecycle()
    val currentPace by RunSession.currentPaceMinPerKm.collectAsStateWithLifecycle()
    val avgPaceMinPerKm = averagePaceMinPerKm(elapsedSec, liveKm)
    // While paused nothing is moving "now": the window is stale until the next segment.
    val nowPace = if (paused) Double.NaN else currentPace
    val nowText = paceFigure(nowPace, unit)
    val avgText = paceFigure(avgPaceMinPerKm, unit)
    val distanceText = distanceFigure(liveKm, unit)
    val cue = paceTarget?.let { paceCue(nowPace, it.pace_fast_sec_per_km, it.pace_slow_sec_per_km) }
    val cueSpoken = when (cue) {
        PaceCue.SpeedUp -> stringResource(Res.string.track_cd_pace_status_speed_up)
        PaceCue.EaseOff -> stringResource(Res.string.track_cd_pace_status_ease_off)
        PaceCue.OnTarget -> stringResource(Res.string.track_cd_pace_status_on_target)
        null -> null
    }
    val toGoLeft = planKm?.let { it - liveKm }
    val toGoText = toGoLeft?.let { left ->
        if (left > 0.0) stringResource(Res.string.track_stat_to_go, runDistLabel(left, unit)) else stringResource(Res.string.track_stat_plan_done)
    }
    // The newest COMPLETED N-minute slice; the in-progress tail is not a finished split.
    val lastSlice = paceBuckets.lastOrNull { it.complete }
    val res = currentResources()
    val spokenFigures = stringResource(
        Res.string.track_cd_stats,
        spokenDuration(res, elapsedSec),
        if (liveKm > 0.0) {
            spokenDistance(res, distanceText, unit)
        } else {
            stringResource(Res.string.track_cd_not_available)
        },
        spokenPace(res, nowPace, unit),
        spokenPace(res, avgPaceMinPerKm, unit),
    )
    val toGoSpoken = toGoLeft?.let { left ->
        if (left > 0.0) stringResource(Res.string.track_stat_to_go, spokenRunDistance(res, left, unit)) else toGoText
    }
    val spoken = listOfNotNull(spokenFigures, cueSpoken, toGoSpoken).joinToString(" ")
    val avgLine = stringResource(Res.string.track_stat_avg_pace, avgText)
    val sliceLine = lastSlice?.let {
        stringResource(Res.string.track_stat_last_slice, paceBucketN, formatPaceMinutes(paceInUnit(it.paceMinPerKm, unit)), unit.paceSuffix)
    }
    Column(
        modifier = Modifier
            .clearAndSetSemantics { contentDescription = spoken }
            // Paused: the figures are frozen, so all of them step back together.
            .alpha(if (paused) PAUSED_FIGURE_ALPHA else 1f),
        verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
        ) {
            val maxSp = if (large) LIVE_FIGURE_LARGE_SP else LIVE_FIGURE_MAX_SP
            LiveFigure(
                label = stringResource(if (paused) Res.string.track_stat_elapsed_paused else Res.string.track_stat_elapsed),
                value = formatElapsed(elapsedSec),
                suffix = null,
                known = true,
                maxSp = maxSp,
                modifier = Modifier.weight(1f),
            )
            LiveFigure(
                label = stringResource(Res.string.run_stat_distance),
                value = distanceText,
                suffix = unit.distanceLabel,
                known = liveKm > 0.0,
                maxSp = maxSp,
                note = toGoText,
                modifier = Modifier.weight(1f),
            )
            // The whole-run average sits with the pace it qualifies, under PACE NOW.
            LiveFigure(
                label = stringResource(Res.string.track_stat_pace_now),
                value = nowText,
                suffix = unit.paceSuffix,
                known = nowPace.isFinite(),
                maxSp = maxSp,
                status = cue?.let { c -> @Composable { PaceStatusLine(c) } },
                detail = avgLine,
                modifier = Modifier.weight(1f),
            )
        }
        sliceLine?.let {
            Text(
                it,
                color = OnBgMuted,
                style = Type.Caption.merge(TabularFigures),
            )
        }
    }
}

/**
 * One live figure: overline, then the value (ExtraBold, tabular) with an optional
 * small faint unit, then an optional one-line [note] (to go), an optional [status]
 * line (the pace status) and a muted one-line [detail] (the whole-run average). The
 * value starts at [maxSp] and steps down only as far as needed to fit its third of
 * the row on one line (a 1:05:30 elapsed, a large font scale), so the three figures
 * never clip or wrap. [known] = false draws the value in the faint tone; a known
 * value is always the body tone.
 */
@Composable
internal fun LiveFigure(
    label: String,
    value: String,
    suffix: String?,
    known: Boolean,
    maxSp: Int,
    modifier: Modifier = Modifier,
    note: String? = null,
    status: (@Composable () -> Unit)? = null,
    detail: String? = null,
) {
    Column(modifier, verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
        TileOverline(label)
        BoxWithConstraints(Modifier.fillMaxWidth()) {
            val measurer = rememberTextMeasurer()
            val faint = OnBgFaint
            val text = remember(value, suffix, faint) { liveFigureText(value, suffix, faint) }
            val maxPx = constraints.maxWidth
            val style = remember(text, maxPx, measurer, maxSp) {
                var sp = maxSp
                var s = liveFigureStyle(sp)
                while (sp > LIVE_FIGURE_MIN_SP &&
                    measurer.measure(text, s, maxLines = 1, softWrap = false).size.width > maxPx
                ) {
                    sp -= LIVE_FIGURE_STEP_SP
                    s = liveFigureStyle(sp)
                }
                s
            }
            Text(
                text,
                color = if (known) OnBgBody else faint,
                style = style,
                maxLines = 1,
                softWrap = false,
            )
        }
        if (note != null) {
            Text(note, color = OnBgMuted, style = Type.Caption.merge(TabularFigures), maxLines = 1)
        }
        status?.invoke()
        if (detail != null) {
            Text(detail, color = OnBgMuted, style = Type.Caption.merge(TabularFigures), maxLines = 1)
        }
    }
}

/** A [LiveFigure]'s value with its small faint unit. */
private fun liveFigureText(value: String, suffix: String?, faint: Color): AnnotatedString = buildAnnotatedString {
    append(value)
    if (suffix != null) {
        withStyle(SpanStyle(fontSize = LIVE_FIGURE_SUFFIX_SP.sp, fontWeight = FontWeight.Normal, color = faint)) {
            append(" $suffix")
        }
    }
}

/** A [LiveFigure]'s value style at [sp]. */
private fun liveFigureStyle(sp: Int): TextStyle =
    Type.Display.copy(fontWeight = FontWeight.ExtraBold, fontSize = sp.sp, lineHeight = (sp + LIVE_FIGURE_LEADING_SP).sp).merge(TabularFigures)

/** Width (px, at [measurer]'s density and font scale) a [LiveFigure] needs to show
 *  [value] and [suffix] whole on one line at its smallest size. */
internal fun liveFigureMinWidthPx(measurer: TextMeasurer, value: String, suffix: String?): Int =
    measurer.measure(liveFigureText(value, suffix, Color.Unspecified), liveFigureStyle(LIVE_FIGURE_MIN_SP), maxLines = 1, softWrap = false).size.width

/**
 * What to do about the live pace against the followed plan's range: "Speed up" /
 * "Ease off" in the warn text tone behind a warning glyph, "On target" in the body
 * tone behind a check. It names an action, never the current pace, so it can't read
 * as a description that contradicts the figure above it.
 */
@Composable
private fun PaceStatusLine(cue: PaceCue) {
    val (textRes, iconRes, tone) = when (cue) {
        PaceCue.SpeedUp -> Triple(Res.string.track_pace_status_speed_up, Res.drawable.ic_safety_warning_triangle, WarnText)
        PaceCue.EaseOff -> Triple(Res.string.track_pace_status_ease_off, Res.drawable.ic_safety_warning_triangle, WarnText)
        PaceCue.OnTarget -> Triple(Res.string.track_pace_status_on_target, Res.drawable.ic_ui_check, OnBgBody)
    }
    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.Xs.dp),
    ) {
        Icon(painterResource(iconRes), contentDescription = null, tint = tone, modifier = Modifier.size(16.dp))
        Text(
            stringResource(textRes),
            color = tone,
            style = Type.BodyBold,
            modifier = Modifier.weight(1f, fill = false),
        )
    }
}

/** A duration read aloud with words, e.g. "1 hour 2 minutes 3 seconds"; zero
 *  leading parts are dropped ("12 minutes 3 seconds"). */
internal fun spokenDuration(res: PlatformResources, totalSec: Long): String {
    val s = totalSec.coerceAtLeast(0L)
    val h = (s / 3600).toInt()
    val m = ((s % 3600) / 60).toInt()
    val sec = (s % 60).toInt()
    val parts = ArrayList<String>(3)
    if (h > 0) parts += res.getQuantityString(Res.plurals.track_cd_hours, h, h)
    if (h > 0 || m > 0) parts += res.getQuantityString(Res.plurals.track_cd_minutes, m, m)
    parts += res.getQuantityString(Res.plurals.track_cd_seconds, sec, sec)
    return parts.joinToString(" ")
}

/** A distance figure read aloud with its unit, e.g. "3.18 miles". */
internal fun spokenDistance(res: PlatformResources, figure: String, unit: DistanceUnit): String =
    res.getString(if (unit == DistanceUnit.Mi) Res.string.track_cd_distance_mi else Res.string.track_cd_distance_km, figure)

/** A run distance (km) read aloud in the user's unit, e.g. "3.18 miles". */
internal fun spokenRunDistance(res: PlatformResources, km: Double, unit: DistanceUnit): String =
    spokenDistance(res, distanceFigure(km, unit), unit)

/** A canonical pace (min/km) read aloud in the user's unit, e.g. "5 minutes 15
 *  seconds per kilometer", or "not available yet" when unknown. */
internal fun spokenPace(res: PlatformResources, minPerKm: Double, unit: DistanceUnit): String {
    if (!minPerKm.isFinite() || minPerKm <= 0.0) return res.getString(Res.string.track_cd_not_available)
    val secPerUnit = (paceInUnit(minPerKm, unit) * 60.0).roundOrZero()
    return res.getString(
        if (unit == DistanceUnit.Mi) Res.string.track_cd_pace_mi else Res.string.track_cd_pace_km,
        spokenDuration(res, secPerUnit),
    )
}
