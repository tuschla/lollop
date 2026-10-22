package app.milestone

import android.content.res.Resources
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle

/** Opacity of the live figures while paused. */
private const val PAUSED_FIGURE_ALPHA = 0.6f

/** The no-data-yet figure: an em dash in the faint tone, never a bare hyphen. */
internal const val NO_VALUE = "—"

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
 * The live stat readouts: ONE primary row of three equal figures (MOVING · DISTANCE ·
 * PACE NOW, each at the same display size so the eye finds them at arm's length),
 * with the whole-run average under PACE NOW, then one muted caption with the last
 * completed N-minute slice. A leaf, so the per-fix [RunSession.elapsedSec]/
 * [RunSession.distanceKm]/[RunSession.currentPaceMinPerKm]/[RunSession.paceBucketsFlow]
 * updates recompose ONLY these figures, not the whole tracking screen. The primary pace
 * is the CURRENT (rolling-window) pace, so a rep, hill or surge reads within ~30 s.
 * Following a plan, a status line under PACE NOW says what to do about it against
 * [paceTarget] ([PaceStatusLine]; null = nothing to judge) while the figure itself
 * stays in the body tone, and DISTANCE shows what is left of [planKm] (a single-step
 * plan's distance). Paused, the figures dim and MOVING says so. A value with no data
 * yet shows a faint "—" (pace) or "0.00" (distance), never a bare hyphen. Live
 * factual progress from the raw fixes only; the of-record figures are the core's on
 * save. For screen readers the whole block is ONE node with units spelled out (no
 * "5:12 · /km" fragments).
 */
@Composable
internal fun TrackerStats(
    unit: DistanceUnit,
    paceBucketN: Int,
    paused: Boolean,
    large: Boolean,
    paceTarget: RunTargetView?,
    planKm: Double?,
    modifier: Modifier = Modifier,
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
    val nowText = if (nowPace.isFinite()) formatPaceMinutes(paceInUnit(nowPace, unit)) else NO_VALUE
    val avgText = if (avgPaceMinPerKm.isFinite()) formatPaceMinutes(paceInUnit(avgPaceMinPerKm, unit)) else NO_VALUE
    val distanceText = distanceFigure(liveKm, unit)
    val cue = paceTarget?.let { paceCue(nowPace, it.pace_fast_sec_per_km, it.pace_slow_sec_per_km) }
    val cueSpoken = when (cue) {
        PaceCue.SpeedUp -> stringResource(R.string.track_cd_pace_status_speed_up)
        PaceCue.EaseOff -> stringResource(R.string.track_cd_pace_status_ease_off)
        PaceCue.OnTarget -> stringResource(R.string.track_cd_pace_status_on_target)
        null -> null
    }
    val toGoText = planKm?.let { planned ->
        val left = planned - liveKm
        if (left > 0.0) {
            stringResource(R.string.track_stat_to_go, runDistLabel(left, unit))
        } else {
            stringResource(R.string.track_stat_plan_done)
        }
    }
    // The newest COMPLETED N-minute slice; the in-progress tail is not a finished split.
    val lastSlice = paceBuckets.lastOrNull { it.complete }
    val res = LocalContext.current.resources
    val spokenFigures = stringResource(
        R.string.track_cd_stats,
        spokenDuration(res, elapsedSec),
        if (liveKm > 0.0) {
            stringResource(
                if (unit == DistanceUnit.Mi) R.string.track_cd_distance_mi else R.string.track_cd_distance_km,
                distanceText,
            )
        } else {
            stringResource(R.string.track_cd_not_available)
        },
        spokenPace(res, nowPace, unit),
        spokenPace(res, avgPaceMinPerKm, unit),
    )
    val spoken = listOfNotNull(spokenFigures, cueSpoken, toGoText).joinToString(" ")
    val avgLine = stringResource(R.string.track_stat_avg_pace, avgText)
    val sliceLine = lastSlice?.let {
        stringResource(R.string.track_stat_last_slice, paceBucketN, formatPaceMinutes(paceInUnit(it.paceMinPerKm, unit)), unit.paceSuffix)
    }
    Column(
        modifier = modifier
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
                label = stringResource(if (paused) R.string.track_stat_elapsed_paused else R.string.track_stat_elapsed),
                value = formatElapsed(elapsedSec),
                suffix = null,
                known = true,
                maxSp = maxSp,
                modifier = Modifier.weight(1f),
            )
            LiveFigure(
                label = stringResource(R.string.track_stat_distance),
                value = distanceText,
                suffix = unit.distanceLabel,
                known = liveKm > 0.0,
                maxSp = maxSp,
                note = toGoText,
                modifier = Modifier.weight(1f),
            )
            // The whole-run average sits with the pace it qualifies, under PACE NOW.
            LiveFigure(
                label = stringResource(R.string.track_stat_pace_now),
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
            val text = remember(value, suffix, faint) {
                buildAnnotatedString {
                    append(value)
                    if (suffix != null) {
                        withStyle(SpanStyle(fontSize = LIVE_FIGURE_SUFFIX_SP.sp, fontWeight = FontWeight.Normal, color = faint)) {
                            append(" $suffix")
                        }
                    }
                }
            }
            val maxPx = constraints.maxWidth
            val style = remember(text, maxPx, measurer, maxSp) {
                val base = Type.Display.copy(fontWeight = FontWeight.ExtraBold).merge(TabularFigures)
                var sp = maxSp
                var s = base.copy(fontSize = sp.sp, lineHeight = (sp + LIVE_FIGURE_LEADING_SP).sp)
                while (sp > LIVE_FIGURE_MIN_SP &&
                    measurer.measure(text, s, maxLines = 1, softWrap = false).size.width > maxPx
                ) {
                    sp -= LIVE_FIGURE_STEP_SP
                    s = base.copy(fontSize = sp.sp, lineHeight = (sp + LIVE_FIGURE_LEADING_SP).sp)
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

/**
 * What to do about the live pace against the followed plan's range: "Speed up" /
 * "Ease off" in the warn text tone behind a warning glyph, "On target" in the body
 * tone behind a check. It names an action, never the current pace, so it can't read
 * as a description that contradicts the figure above it.
 */
@Composable
private fun PaceStatusLine(cue: PaceCue) {
    val (textRes, iconRes, tone) = when (cue) {
        PaceCue.SpeedUp -> Triple(R.string.track_pace_status_speed_up, R.drawable.ic_safety_warning_triangle, WarnText)
        PaceCue.EaseOff -> Triple(R.string.track_pace_status_ease_off, R.drawable.ic_safety_warning_triangle, WarnText)
        PaceCue.OnTarget -> Triple(R.string.track_pace_status_on_target, R.drawable.ic_ui_check, OnBgBody)
    }
    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.Xs.dp),
    ) {
        Icon(painterResource(iconRes), contentDescription = null, tint = tone, modifier = Modifier.size(16.dp))
        Text(
            stringResource(textRes),
            color = tone,
            style = Type.Body.copy(fontWeight = FontWeight.Bold),
            modifier = Modifier.weight(1f, fill = false),
        )
    }
}

/** A duration read aloud with words, e.g. "1 hour 2 minutes 3 seconds"; zero
 *  leading parts are dropped ("12 minutes 3 seconds"). */
private fun spokenDuration(res: Resources, totalSec: Long): String {
    val s = totalSec.coerceAtLeast(0L)
    val h = (s / 3600).toInt()
    val m = ((s % 3600) / 60).toInt()
    val sec = (s % 60).toInt()
    val parts = ArrayList<String>(3)
    if (h > 0) parts += res.getQuantityString(R.plurals.track_cd_hours, h, h)
    if (h > 0 || m > 0) parts += res.getQuantityString(R.plurals.track_cd_minutes, m, m)
    parts += res.getQuantityString(R.plurals.track_cd_seconds, sec, sec)
    return parts.joinToString(" ")
}

/** A canonical pace (min/km) read aloud in the user's unit, e.g. "5 minutes 15
 *  seconds per kilometre", or "not available yet" when unknown. */
private fun spokenPace(res: Resources, minPerKm: Double, unit: DistanceUnit): String {
    if (!minPerKm.isFinite() || minPerKm <= 0.0) return res.getString(R.string.track_cd_not_available)
    val secPerUnit = Math.round(paceInUnit(minPerKm, unit) * 60.0)
    return res.getString(
        if (unit == DistanceUnit.Mi) R.string.track_cd_pace_mi else R.string.track_cd_pace_km,
        spokenDuration(res, secPerUnit),
    )
}
