package app.milestone

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.wrapContentHeight
import androidx.compose.foundation.layout.wrapContentWidth
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import java.time.DayOfWeek
import java.time.LocalDate
import java.time.format.DateTimeFormatter
import java.time.format.TextStyle
import java.util.Locale

/**
 * A minimal line chart over an ordered numeric series, no axes, no labels. Used
 * to show the shape of a factual logged sequence (e.g. e1RM over sessions); it
 * plots the values as given and computes nothing about them. Renders nothing for
 * a series shorter than two points.
 */
@Composable
fun Sparkline(
    values: List<Float>,
    color: Color,
    modifier: Modifier = Modifier,
    endDot: Boolean = false,
    description: String? = null,
) {
    if (values.size < 2) return
    // TalkBack can't see a Canvas, so summarize the plotted series: point count and
    // the first→last direction. Callers with domain context (units, what the series
    // is) can pass a richer [description]; otherwise this factual auto-summary stands.
    val first = values.first()
    val last = values.last()
    val trendRes = when {
        last > first -> R.plurals.ui_sparkline_cd_up
        last < first -> R.plurals.ui_sparkline_cd_down
        else -> R.plurals.ui_sparkline_cd_flat
    }
    val desc = description ?: pluralStringResource(
        trendRes, values.size, values.size, trimDecimal(first.toDouble()), trimDecimal(last.toDouble()),
    )
    Canvas(modifier.semantics { contentDescription = desc }) {
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

/** One local day's logged activity on the heatmap: whether any set was lifted that
 *  day, and how many runs were logged. */
internal data class DayActivity(val lifted: Boolean, val runs: Int) {
    /** Sessions that day: a lifting day is one, each run is one. */
    val sessions: Int get() = (if (lifted) 1 else 0) + runs
}

/** A locale-ordered date formatter for [skeleton] ("MMMd" → "Sep 21" / "21. Sep."). */
internal fun localizedDayFormatter(skeleton: String, locale: Locale = Locale.getDefault()): DateTimeFormatter =
    DateTimeFormatter.ofPattern(android.text.format.DateFormat.getBestDateTimePattern(locale, skeleton), locale)

/** [localizedDayFormatter] for a local-day index (days since the epoch). */
internal fun DateTimeFormatter.formatEpochDay(epochDay: Long): String = LocalDate.ofEpochDay(epochDay).format(this)

/**
 * GitHub-style activity heatmap: one cell per day over the last [weeks] weeks,
 * columns = weeks, rows = day-of-week (Monday top, matching the plan's weeks). A
 * cell is tinted by DISCIPLINE ([days], keyed by local-day index): the accent for a
 * run day, the muted ink for a lifting day, split half and half on a day with both
 * (diagonally under an ink accent such as Paper's, where the two greys sit close).
 * One session a day is the norm, so the tint answers "what did I do", not "how
 * much". Purely a factual layout of when sessions were logged, no coaching, no
 * streak-nudging; empty days read as the faint track, future cells in the current
 * week are blank. Day indices are local-day numbers (days since the Unix epoch in
 * the device timezone) so the caller keeps timezone handling out of the core. A
 * logged day is tappable when [onDayClick] is given.
 */
@Composable
internal fun ContributionHeatmap(
    days: Map<Long, DayActivity>,
    todayLocalDay: Long,
    weeks: Int = 16,
    modifier: Modifier = Modifier,
    onDayClick: ((Long) -> Unit)? = null,
) {
    val palette = LocalPalette.current
    val track = palette.onBgBody.copy(alpha = HairlineAlpha.Card)
    val runTint = palette.accent
    val liftTint = palette.onBgMuted
    val diagonalBoth = runTint.isAchromatic()
    // Epoch day 0 (1970-01-01) is a Thursday; shift so Monday = 0.
    fun dow(day: Long): Int = Math.floorMod(day + 3, 7L).toInt()
    val thisWeekStart = todayLocalDay - dow(todayLocalDay)
    val firstWeekStart = thisWeekStart - (weeks - 1) * 7L

    // TalkBack summary of the window (active-day count + busiest week); each logged
    // cell also speaks its own date and activity.
    var activeDays = 0
    var busiestWeek = 0
    for (col in 0 until weeks) {
        var weekSessions = 0
        for (row in 0..6) {
            val day = firstWeekStart + col * 7L + row
            if (day > todayLocalDay) continue
            val c = days[day]?.sessions ?: 0
            if (c > 0) activeDays++
            weekSessions += c
        }
        if (weekSessions > busiestWeek) busiestWeek = weekSessions
    }
    val heatmapDesc = stringResource(
        R.string.history_heatmap_desc,
        weeks,
        pluralStringResource(R.plurals.history_active_days, activeDays, activeDays),
        pluralStringResource(R.plurals.history_sessions, busiestWeek, busiestWeek),
    )

    // Axis labels: localized narrow weekday initials down the left gutter (Mon top),
    // month ticks along the bottom at each column where the month first changes.
    val locale = Locale.getDefault()
    val weekdayInitials = remember(locale) { (0..6).map { DayOfWeek.of(it + 1).getDisplayName(TextStyle.NARROW, locale) } }
    val monthFmt = remember(locale) { localizedDayFormatter("MMM", locale) }
    val cellDateFmt = remember(locale) { localizedDayFormatter("EEEEMMMMd", locale) }
    // First column of each distinct month → its label (blank elsewhere).
    val rawTicks = (0 until weeks).map { col ->
        val colMonth = monthFmt.formatEpochDay(firstWeekStart + col * 7L)
        val prevMonth = if (col == 0) "" else monthFmt.formatEpochDay(firstWeekStart + (col - 1) * 7L)
        if (colMonth != prevMonth) colMonth else ""
    }
    val liftLabel = stringResource(R.string.history_heatmap_lift)
    val openLabel = stringResource(R.string.history_heatmap_open_day)
    val res = androidx.compose.ui.platform.LocalContext.current.resources

    val gap = Space.Xs.dp
    val density = LocalDensity.current
    // Weekday gutter as wide as its widest labelled initial at the current font
    // scale, so a large font never clips "M"/"W" against the first column.
    val measurer = rememberTextMeasurer()
    val gutterPx = LabelledWeekdayRows.maxOf { measurer.measure(weekdayInitials[it], style = Type.Chip).size.width }
    val gutter = with(density) { gutterPx.toDp() }
    BoxWithConstraints(modifier = modifier.fillMaxWidth()) {
        // Square cells sized from the width left after the gutter + inter-column
        // gaps, so the grid always spans the full content width. Whole pixels, so
        // per-column rounding never pushes the last week past the edge.
        val cell = with(density) {
            ((constraints.maxWidth - gutterPx - gap.roundToPx() * weeks) / weeks).coerceAtLeast(1).toDp()
        }
        // A leading partial month one column before the next month's tick ("Jul" over
        // the last days of July) can't keep a word space before it at a large font: it
        // gives way.
        val monthTicks = with(density) {
            spacedMonthTicks(
                rawTicks,
                rawTicks.map { if (it.isEmpty()) 0 else measurer.measure(it, style = Type.Chip).size.width },
                pitchPx = cell.roundToPx() + gap.roundToPx(),
                minGapPx = Space.Md.dp.roundToPx(),
            )
        }
        Column(verticalArrangement = Arrangement.spacedBy(gap)) {
            Row(
                horizontalArrangement = Arrangement.spacedBy(gap),
                modifier = Modifier.semantics { contentDescription = heatmapDesc },
            ) {
                // Weekday gutter (decorative: each cell names its own day).
                Column(
                    modifier = Modifier.width(gutter).clearAndSetSemantics { },
                    verticalArrangement = Arrangement.spacedBy(gap),
                ) {
                    for (row in 0..6) {
                        Box(Modifier.height(cell), contentAlignment = Alignment.CenterEnd) {
                            // Label alternate rows (Mon/Wed/Fri) to avoid crowding.
                            if (row in LabelledWeekdayRows) {
                                Text(
                                    weekdayInitials[row],
                                    color = OnBgFaint,
                                    style = Type.Chip,
                                    maxLines = 1,
                                    softWrap = false,
                                    // A tall line at a large font scale spills into the
                                    // unlabelled neighbour rows rather than clipping.
                                    modifier = Modifier.wrapContentHeight(unbounded = true),
                                )
                            }
                        }
                    }
                }
                // One tap surface over the whole grid, split into exactly one zone per
                // day (the cell plus half the gap on each side), so a tap opens the
                // day under the finger and never a neighbour; a 48dp target per ~20dp
                // cell could only overlap its neighbours. The cells carry no click
                // semantics of their own: each logged cell speaks its day and offers
                // "Open this day" as an accessibility action.
                val pitchPx = with(density) { cell.roundToPx() + gap.roundToPx() }.toFloat()
                val halfGapPx = with(density) { gap.roundToPx() } / 2f
                Row(
                    horizontalArrangement = Arrangement.spacedBy(gap),
                    modifier = if (onDayClick != null) {
                        Modifier.pointerInput(onDayClick, days, firstWeekStart, todayLocalDay, pitchPx) {
                            detectTapGestures { at ->
                                val col = ((at.x + halfGapPx) / pitchPx).toInt().coerceIn(0, weeks - 1)
                                val row = ((at.y + halfGapPx) / pitchPx).toInt().coerceIn(0, 6)
                                val day = firstWeekStart + col * 7L + row
                                if (day <= todayLocalDay && (days[day]?.sessions ?: 0) > 0) onDayClick(day)
                            }
                        }
                    } else {
                        Modifier
                    },
                ) {
                    for (col in 0 until weeks) {
                        Column(verticalArrangement = Arrangement.spacedBy(gap)) {
                            for (row in 0..6) {
                                val day = firstWeekStart + col * 7L + row
                                val cellMod = Modifier
                                    .size(cell)
                                    .clip(RoundedCornerShape(2.dp))
                                val activity = days[day]?.takeIf { it.sessions > 0 }
                                if (day > todayLocalDay) {
                                    Box(cellMod)
                                } else if (activity == null) {
                                    Box(cellMod.background(track))
                                } else {
                                    val what = listOfNotNull(
                                        liftLabel.takeIf { activity.lifted },
                                        activity.runs.takeIf { it > 0 }
                                            ?.let { res.getQuantityString(R.plurals.history_heatmap_runs, it, it) },
                                    ).joinToString(", ")
                                    val cellDesc = stringResource(R.string.history_heatmap_cell, cellDateFmt.formatEpochDay(day), what)
                                    Box(
                                        cellMod
                                            .drawBehind { drawDayTint(activity, liftTint, runTint, diagonalBoth) }
                                            .semantics {
                                                contentDescription = cellDesc
                                                if (onDayClick != null) {
                                                    customActions = listOf(
                                                        CustomAccessibilityAction(openLabel) { onDayClick(day); true },
                                                    )
                                                }
                                            },
                                    )
                                }
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
                            // Overflows into the following (label-free) columns
                            // rather than wrapping inside one narrow cell.
                            Text(
                                monthTicks[col],
                                color = OnBgFaint,
                                style = Type.Chip,
                                maxLines = 1,
                                softWrap = false,
                                modifier = Modifier.wrapContentWidth(Alignment.Start, unbounded = true),
                            )
                        }
                    }
                }
            }
            // Tint legend: lift, run, both. Decorative for TalkBack (cells speak
            // their own activity).
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
                modifier = Modifier
                    .padding(start = gutter + gap, top = Space.Xs.dp)
                    .clearAndSetSemantics { },
            ) {
                HeatmapLegendItem(DayActivity(lifted = true, runs = 0), liftTint, runTint, diagonalBoth, liftLabel)
                HeatmapLegendItem(DayActivity(lifted = false, runs = 1), liftTint, runTint, diagonalBoth, stringResource(R.string.history_heatmap_run))
                HeatmapLegendItem(DayActivity(lifted = true, runs = 1), liftTint, runTint, diagonalBoth, stringResource(R.string.history_heatmap_both))
            }
        }
    }
}

/** Month tick labels with every collision resolved: a label (width [widthsPx]) that
 *  would run into the next kept label, [minGapPx] apart at a column [pitchPx], is
 *  dropped, so the later label (a month's first full week) wins over a leading partial
 *  month. Blank entries are columns without a tick. Pure. */
internal fun spacedMonthTicks(ticks: List<String>, widthsPx: List<Int>, pitchPx: Int, minGapPx: Int): List<String> {
    val out = ticks.toMutableList()
    var nextCol = -1
    for (col in ticks.indices.reversed()) {
        if (ticks[col].isEmpty()) continue
        if (nextCol != -1 && widthsPx[col] + minGapPx > (nextCol - col) * pitchPx) {
            out[col] = ""
        } else {
            nextCol = col
        }
    }
    return out
}

@Composable
private fun HeatmapLegendItem(sample: DayActivity, liftTint: Color, runTint: Color, diagonal: Boolean, label: String) {
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
        Box(
            Modifier
                .size(12.dp)
                .clip(RoundedCornerShape(2.dp))
                .drawBehind { drawDayTint(sample, liftTint, runTint, diagonal) },
        )
        Text(label, color = OnBgFaint, style = Type.Chip)
    }
}

/** Whether a colour carries no hue to tell it from the grey lift ink (Paper's ink
 *  accent): its sRGB channels sit within a hair of each other. */
internal fun Color.isAchromatic(): Boolean =
    maxOf(red, green, blue) - minOf(red, green, blue) < ACHROMATIC_SPREAD

/** Largest channel spread (0..1) that still reads as a neutral grey. */
private const val ACHROMATIC_SPREAD = 0.04f

/** A heatmap cell's discipline tint: the run accent, the lift ink, or half of each:
 *  side by side, or split on the diagonal ([diagonal]: lift upper-left, run
 *  lower-right) so two close greys still read as two tones. */
private fun androidx.compose.ui.graphics.drawscope.DrawScope.drawDayTint(
    a: DayActivity,
    liftTint: Color,
    runTint: Color,
    diagonal: Boolean,
) {
    when {
        a.lifted && a.runs > 0 && diagonal -> {
            drawRect(runTint)
            drawPath(
                androidx.compose.ui.graphics.Path().apply {
                    moveTo(0f, 0f)
                    lineTo(size.width, 0f)
                    lineTo(0f, size.height)
                    close()
                },
                liftTint,
            )
        }
        a.lifted && a.runs > 0 -> {
            drawRect(liftTint, size = Size(size.width / 2f, size.height))
            drawRect(runTint, topLeft = Offset(size.width / 2f, 0f), size = Size(size.width / 2f, size.height))
        }
        a.lifted -> drawRect(liftTint)
        else -> drawRect(runTint)
    }
}

/** Heatmap rows that carry a weekday initial (Mon / Wed / Fri, Monday-top). */
private val LabelledWeekdayRows = listOf(0, 2, 4)

/**
 * Garmin-style per-day running-distance bar chart: one accent bar per local day
 * over the last [days] days, height ∝ that day's summed distance ([kmByDay], km,
 * keyed by local-day index), every figure shown in the user's distance [unit]
 * (km or mi). A purely factual layout of logged distance, no coaching,
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
    unit: DistanceUnit,
    days: Int = 14,
    modifier: Modifier = Modifier,
    chartHeight: Dp = 96.dp,
) {
    val firstDay = todayLocalDay - (days - 1)
    val vals = (0 until days).map { metersToDisplay((kmByDay[firstDay + it] ?: 0.0) * 1000.0, unit) }
    val maxKm = vals.maxOrNull()?.takeIf { it > 0.0 } ?: 0.0
    val totalKm = vals.sum()
    // Nice round y-max ceiling so the axis label reads cleanly (e.g. 8/10/15).
    val yMax = niceAxisCeiling(maxKm)

    val accent = Accent
    val baseline = OnBgBody.copy(alpha = HairlineAlpha.Card)
    val todayTick = Accent.copy(alpha = CurrentPeriodTickAlpha)
    val gridline = HairlineStrong

    // Device-locale date label in the locale's own field order (prose for the
    // reader, not a serialized value).
    val dayFmt = remember { localizedDayFormatter("MMMd") }
    fun dayLabel(day: Long): String = dayFmt.formatEpochDay(day)

    // TalkBack summary of the bar canvas (invisible to a screen reader otherwise):
    // window length, total distance, and the single longest day.
    val longestIdx = vals.indices.maxByOrNull { vals[it] } ?: 0
    val daysText = pluralStringResource(R.plurals.history_days, days, days)
    val barsDesc = if (totalKm > 0.0) {
        stringResource(
            R.string.history_bars_desc,
            daysText, fmtDistance(totalKm), unit.distanceLabel,
            fmtDistance(vals[longestIdx]), dayLabel(firstDay + longestIdx),
        )
    } else {
        stringResource(R.string.history_bars_desc_empty, daysText)
    }

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
                if (yMax > 0.0) "${fmtDistance(yMax)} ${unit.distanceLabel}" else EmptyValuePlaceholder,
                color = OnBgFaint,
                style = Type.Caption.merge(TabularFigures),
            )
            Text(
                stringResource(R.string.history_bars_total, fmtDistance(totalKm), unit.distanceLabel),
                color = OnBgFaint,
                style = Type.Caption.merge(TabularFigures),
            )
        }
        Canvas(
            modifier = Modifier
                .fillMaxWidth()
                .height(chartHeight)
                .semantics { contentDescription = barsDesc },
        ) {
            val n = days
            val slot = size.width / n
            val barW = slot * 0.58f
            val corner = CornerRadius(stroke(2f), stroke(2f))
            // Reserve a hair at the bottom for the baseline + today underline.
            val underline = stroke(2.5f)
            val usableH = (size.height - underline - stroke(2f)).coerceAtLeast(1f)
            // Scale ceiling: a faint hairline at y-max anchors the axis label above.
            if (yMax > 0.0) drawAxisCeiling(gridline)
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
            Text(stringResource(R.string.main_today_axis), color = OnBgFaint, style = Type.Caption)
        }
    }
}

/** Trim a whole-number `.0` off a distance figure; one decimal otherwise. */
private fun fmtDistance(value: Double): String = trimDecimal(value)

/** Opacity of the accent underline under today's / this week's bar. */
private const val CurrentPeriodTickAlpha = 0.55f

/** A round y-axis ceiling at or above [peak] (e.g. 8 / 10 / 15), 0 for none, so a
 *  chart's axis label reads cleanly. */
private fun niceAxisCeiling(peak: Double): Double = when {
    peak <= 0.0 -> 0.0
    peak <= 5.0 -> kotlin.math.ceil(peak)
    peak <= 20.0 -> kotlin.math.ceil(peak / 2.0) * 2.0
    else -> kotlin.math.ceil(peak / 5.0) * 5.0
}

/** The y-max gridline of a bar chart: a 1dp dashed hairline across the top of the
 *  plot (where a full-scale bar ends), so the y-max label reads as the scale
 *  ceiling, not a value. Drawn before the bars so they sit over it. */
private fun androidx.compose.ui.graphics.drawscope.DrawScope.drawAxisCeiling(color: Color) {
    val y = stroke(0.5f)
    drawLine(
        color = color,
        start = Offset(0f, y),
        end = Offset(size.width, y),
        strokeWidth = stroke(1f),
        pathEffect = androidx.compose.ui.graphics.PathEffect.dashPathEffect(floatArrayOf(stroke(2f), stroke(3f))),
    )
}

/**
 * Weekly running distance: one accent bar per Monday-start local week of
 * the core's series ([weeks], oldest left, this week right), every figure in the
 * user's distance [unit]. When the profile plans a weekly distance ([plannedKm],
 * canonical km, 0 = none) a dashed reference line marks it, starting at the week
 * the plan began ([planStartEpochDay]; null = the whole window). Tapping a bar
 * reads out its week and distance ("Week of Sep 21 · 16 mi"); tapping it again
 * clears it. Factual self-monitoring only; empty weeks read as a faint baseline
 * stub, this week carries the accent underline. One TalkBack summary (total,
 * busiest week, this week, plan) stands in for the canvas.
 */
@Composable
fun WeeklyDistanceChart(
    weeks: List<RunWeekView>,
    plannedKm: Double,
    unit: DistanceUnit,
    modifier: Modifier = Modifier,
    chartHeight: Dp = 112.dp,
    planStartEpochDay: Long? = null,
) {
    if (weeks.isEmpty()) return
    val vals = weeks.map { metersToDisplay(it.km * 1000.0, unit) }
    val n = weeks.size
    // First bar the plan line covers: the week holding the plan's first day.
    val planFrom = if (planStartEpochDay == null) {
        0
    } else {
        weeks.indexOfFirst { it.week_start_epoch_day + 6 >= planStartEpochDay }.let { if (it < 0) n else it }
    }
    val planned = if (plannedKm > 0.0 && planFrom < n) metersToDisplay(plannedKm * 1000.0, unit) else 0.0
    val yMax = niceAxisCeiling(maxOf(vals.max(), planned))
    val total = vals.sum()
    var selected by remember(weeks) { mutableStateOf<Int?>(null) }

    val accent = Accent
    val accentDimmed = Accent.copy(alpha = 0.45f)
    val baseline = OnBgBody.copy(alpha = HairlineAlpha.Card)
    val thisWeekTick = Accent.copy(alpha = CurrentPeriodTickAlpha)
    val planColor = OnBgMuted
    val gridline = HairlineStrong

    // Device-locale week label from the Monday's epoch day, in the locale's own field
    // order (prose, not a wire value).
    val weekFmt = remember { localizedDayFormatter("MMMd") }
    fun weekLabel(day: Long): String = weekFmt.formatEpochDay(day)
    val busiest = vals.indices.maxByOrNull { vals[it] } ?: 0
    val planDesc = if (planned > 0.0) {
        stringResource(R.string.history_weekly_desc_plan, fmtDistance(planned), unit.distanceLabel)
    } else {
        ""
    }
    val chartDesc = if (total > 0.0) {
        stringResource(
            R.string.history_weekly_desc,
            n, fmtDistance(total), unit.distanceLabel,
            fmtDistance(vals[busiest]), weekLabel(weeks[busiest].week_start_epoch_day),
            fmtDistance(vals.last()),
        ) + planDesc
    } else {
        stringResource(R.string.history_weekly_desc_empty, n) + planDesc
    }

    Column(
        modifier = modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
    ) {
        // Header: y-max on the left, the plan legend (dashed swatch) on the right.
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
        ) {
            Text(
                if (yMax > 0.0) "${fmtDistance(yMax)} ${unit.distanceLabel}" else EmptyValuePlaceholder,
                color = OnBgFaint,
                style = Type.Caption.merge(TabularFigures),
                modifier = Modifier.weight(1f),
            )
            if (planned > 0.0) {
                Canvas(Modifier.width(16.dp).height(8.dp)) {
                    drawLine(
                        color = planColor,
                        start = Offset(0f, size.height / 2f),
                        end = Offset(size.width, size.height / 2f),
                        strokeWidth = stroke(1.5f),
                        pathEffect = androidx.compose.ui.graphics.PathEffect.dashPathEffect(
                            floatArrayOf(stroke(3f), stroke(2f)),
                        ),
                    )
                }
                Text(
                    stringResource(R.string.history_weekly_plan_legend, fmtDistance(planned), unit.distanceLabel),
                    color = OnBgFaint,
                    style = Type.Caption.merge(TabularFigures),
                )
            }
        }
        Canvas(
            modifier = Modifier
                .fillMaxWidth()
                .height(chartHeight)
                .semantics { contentDescription = chartDesc }
                .pointerInput(n) {
                    detectTapGestures { tap ->
                        val i = (tap.x / (size.width.toFloat() / n)).toInt().coerceIn(0, n - 1)
                        selected = if (selected == i) null else i
                    }
                },
        ) {
            val slot = size.width / n
            val barW = slot * 0.62f
            val corner = CornerRadius(stroke(2f), stroke(2f))
            val underline = stroke(2.5f)
            val usableH = (size.height - underline - stroke(2f)).coerceAtLeast(1f)
            val picked = selected
            // Scale ceiling: a faint hairline at y-max anchors the axis label above.
            if (yMax > 0.0) drawAxisCeiling(gridline)
            for (i in 0 until n) {
                val v = vals[i]
                val left = slot * i + (slot - barW) / 2f
                val barTop = if (yMax > 0.0) {
                    usableH - (v / yMax).toFloat().coerceIn(0f, 1f) * usableH
                } else {
                    usableH
                }
                if (v > 0.0) {
                    drawRoundRect(
                        color = if (picked == null || picked == i) accent else accentDimmed,
                        topLeft = Offset(left, barTop),
                        size = Size(barW, usableH - barTop),
                        cornerRadius = corner,
                    )
                } else {
                    drawRoundRect(
                        color = baseline,
                        topLeft = Offset(left, usableH - stroke(2f)),
                        size = Size(barW, stroke(2f)),
                        cornerRadius = corner,
                    )
                }
                if (i == n - 1) {
                    drawRoundRect(
                        color = thisWeekTick,
                        topLeft = Offset(left, usableH + stroke(1.5f)),
                        size = Size(barW, underline),
                        cornerRadius = corner,
                    )
                }
            }
            // The planned weekly distance as a dashed reference line across the bars of
            // the weeks the plan existed.
            if (planned > 0.0 && yMax > 0.0) {
                val y = usableH - (planned / yMax).toFloat().coerceIn(0f, 1f) * usableH
                drawLine(
                    color = planColor,
                    start = Offset(slot * planFrom, y),
                    end = Offset(size.width, y),
                    strokeWidth = stroke(1.5f),
                    pathEffect = androidx.compose.ui.graphics.PathEffect.dashPathEffect(
                        floatArrayOf(stroke(5f), stroke(4f)),
                    ),
                )
            }
        }
        // Axis ticks: the first week's Monday on the left, this week on the right.
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Text(weekLabel(weeks.first().week_start_epoch_day), color = OnBgFaint, style = Type.Caption.merge(TabularFigures))
            Text(stringResource(R.string.history_weekly_this_week), color = OnBgFaint, style = Type.Caption)
        }
        selected?.takeIf { it < n }?.let { i ->
            Text(
                stringResource(
                    R.string.history_weekly_bar_value,
                    weekLabel(weeks[i].week_start_epoch_day), fmtDistance(vals[i]), unit.distanceLabel,
                ),
                color = OnBgBody,
                style = Type.Caption.merge(TabularFigures),
            )
        }
    }
}
