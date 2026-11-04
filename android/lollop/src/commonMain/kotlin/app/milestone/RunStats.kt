package app.milestone

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.text.BasicText
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import app.milestone.resources.Res
import app.milestone.resources.run_stat_avg_pace
import app.milestone.resources.run_stat_cd
import app.milestone.resources.run_stat_distance
import app.milestone.resources.run_stat_time
import org.jetbrains.compose.resources.stringResource

// A run's headline stats (distance, time, pace): the run detail sheet and the
// full-screen route map show them.

/** One headline stat column: overline [label], tabular [value], its [unit] (may be
 *  blank) and the merged [spoken] form ("Distance, 3.18 miles"). */
internal data class RunStat(val label: String, val value: String, val unit: String, val spoken: String)

/** Distance, moving time and average pace of [run] in [unit], each only when measured. */
@Composable
internal fun runStats(run: RunResultView, unit: DistanceUnit): List<RunStat> {
    val stats = ArrayList<RunStat>(3)
    val res = currentResources()
    if (run.distance_km > 0.0) {
        val dist = distanceFigure(run.distance_km, unit)
        val label = stringResource(Res.string.run_stat_distance)
        val spoken = spokenDistance(res, dist, unit)
        stats += RunStat(label, dist, unit.distanceLabel, stringResource(Res.string.run_stat_cd, label, spoken))
    }
    if (run.duration_min > 0.0) {
        val time = formatElapsed((run.duration_min * 60.0).roundOrZero())
        val label = stringResource(Res.string.run_stat_time)
        stats += RunStat(label, time, "", stringResource(Res.string.run_stat_cd, label, time))
    }
    if (run.distance_km > 0.0 && run.duration_min > 0.0) {
        val paceMinPerKm = run.duration_min / run.distance_km
        val pace = formatPaceMinutes(paceInUnit(paceMinPerKm, unit))
        val label = stringResource(Res.string.run_stat_avg_pace)
        val spoken = spokenPace(res, paceMinPerKm, unit)
        stats += RunStat(label, pace, unit.paceSuffix, stringResource(Res.string.run_stat_cd, label, spoken))
    }
    return stats
}

/** Smallest a headline stat value auto-sizes to before it would wrap (spec §2.2). */
private val StatValueMinSize = 24.sp

/** Step a headline stat value shrinks by while auto-sizing toward [StatValueMinSize]. */
private const val StatValueStepSp = 1f

/**
 * The 3-up headline stats (spec §2.2, the tracker's stat typography a step
 * smaller): equal columns of a [TileOverline] label over a `Type.Display` value
 * (auto-sized down to 24sp before it would wrap) with its unit in `Type.Caption` on
 * the value's baseline. [compact] (the full-screen map's stat sheet) sets the
 * values in `Type.Title` and reads the whole row once. When a value at its
 * smallest size (or a label word) can't fit its column (a large font on a narrow
 * phone), the stats wrap: each takes the full width, one under the other, so a
 * number is never cut and a label never splits. The size is picked here, by
 * measuring, so the value is laid out at the size it shows.
 */
@Composable
internal fun RunStatColumns(stats: List<RunStat>, compact: Boolean) {
    val valueStyle = (if (compact) Type.Title.merge(TabularFigures) else Type.Display).copy(color = OnBgBody)
    val measurer = rememberTextMeasurer()
    BoxWithConstraints(
        Modifier
            .fillMaxWidth()
            .then(if (compact) Modifier.semantics(mergeDescendants = true) {} else Modifier),
    ) {
        val density = LocalDensity.current
        val gapPx = with(density) { Space.Md.dp.roundToPx() }
        val unitGapPx = with(density) { Space.Xs.dp.roundToPx() }
        val width = constraints.maxWidth
        val n = stats.size.coerceAtLeast(1)
        val columnPx = ((width - gapPx * (n - 1)) / n).coerceAtLeast(0)
        fun lineWidth(text: String, style: TextStyle) =
            measurer.measure(text, style, maxLines = 1, softWrap = false).size.width
        // The largest value size (Display stepped down to the floor; compact: Title
        // only) that fits [roomPx] beside its unit, or null when none does.
        fun fittingStyle(stat: RunStat, roomPx: Int): TextStyle? {
            val unitPx = if (stat.unit.isEmpty()) 0 else unitGapPx + lineWidth(stat.unit, Type.Caption)
            val sizes = if (compact) {
                sequenceOf(valueStyle.fontSize.value)
            } else {
                generateSequence(Type.Display.fontSize.value) { it - StatValueStepSp }.takeWhile { it >= StatValueMinSize.value }
            }
            return sizes.map { valueStyle.copy(fontSize = it.sp) }.firstOrNull { lineWidth(stat.value, it) + unitPx <= roomPx }
        }
        val labelsFit = stats.all { s -> s.label.uppercase().split(' ').all { lineWidth(it, Type.Overline) <= columnPx } }
        val columnStyles = if (labelsFit) stats.map { fittingStyle(it, columnPx) } else emptyList()
        if (labelsFit && columnStyles.all { it != null }) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                stats.forEachIndexed { i, stat -> RunStatColumn(stat, columnStyles[i]!!, Modifier.weight(1f)) }
            }
        } else {
            Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                stats.forEach { stat ->
                    val floor = if (compact) valueStyle else valueStyle.copy(fontSize = StatValueMinSize)
                    RunStatColumn(stat, fittingStyle(stat, width) ?: floor, Modifier.fillMaxWidth())
                }
            }
        }
    }
}

/** One headline stat: its overline label over the value at [valueStyle] and its unit. */
@Composable
private fun RunStatColumn(stat: RunStat, valueStyle: TextStyle, modifier: Modifier) {
    Column(
        modifier.clearAndSetSemantics { contentDescription = stat.spoken },
        verticalArrangement = Arrangement.spacedBy(Space.Xs.dp),
    ) {
        TileOverline(stat.label)
        Row {
            BasicText(
                stat.value,
                style = valueStyle,
                modifier = Modifier.alignByBaseline().weight(1f, fill = false),
            )
            if (stat.unit.isNotEmpty()) {
                Spacer(Modifier.width(Space.Xs.dp))
                Text(stat.unit, color = OnBgMuted, style = Type.Caption, maxLines = 1, modifier = Modifier.alignByBaseline())
            }
        }
    }
}
