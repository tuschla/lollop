package app.milestone

import android.content.res.Resources
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import java.util.Locale

/** A display-unit length to two decimals ("0.18"), the one figure format every
 *  run distance and split length uses, so a sheet never mixes decimal marks. */
internal fun displayFigure(displayValue: Double): String = String.format(Locale.US, "%.2f", displayValue)

/** A distance figure without its unit ("3.21"), in the user's unit. */
internal fun distanceFigure(km: Double, unit: DistanceUnit): String = displayFigure(metersToDisplay(km * 1000.0, unit))

/** Distance in the display unit, e.g. "8.05 km" / "5.00 mi". */
internal fun runDistLabel(distanceKm: Double, unit: DistanceUnit): String = "${distanceFigure(distanceKm, unit)} ${unit.distanceLabel}"

/** Pace in the display unit, e.g. "5:34 /km" / "8:57 /mi". Recomputed from the
 *  run's distance+duration (not the core's always-/km `pace` string) so every run
 *  readout honours the chosen unit. "-" for a degenerate run. */
internal fun runPaceLabel(distanceKm: Double, durationMin: Double, unit: DistanceUnit): String {
    if (distanceKm <= 0.0 || durationMin <= 0.0) return "-"
    return formatPaceMinutes(paceInUnit(durationMin / distanceKm, unit)) + " " + unit.paceSuffix
}

/** The resolved distance unit (locale + user override), collected reactively. */
@Composable
internal fun rememberDistanceUnit(): DistanceUnit =
    resolveDistanceUnit(AppSettings.distanceUnitOverride.collectAsState().value)

/**
 * Run entry card (03-history §4): title (the user's run-type tag, else the weekday)
 * + date badge; distance · pace · duration line; "Effort 7/10" once rated; the HR
 * zone row with a three-segment zone legend ONLY when HR was measured; then only the
 * findings that ask for a change or are SAFETY (with their one "?") and the run's
 * top record. No type icon: the Runs list holds nothing but runs.
 */
@Composable
internal fun RunCard(r: RunResultView, nowMillis: Long, onClick: (() -> Unit)? = null) {
    val unit = rememberDistanceUnit()
    PlainCard(onClick = onClick) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Space.Md.dp + Space.Xs.dp),
        ) {
            Text(
                runTitle(r),
                style = Type.CardTitle,
                color = OnBgBody,
                modifier = Modifier.weight(1f),
            )
            // The day once: no "2 days ago" chip beside "Saturday run".
            DateBadge(runDateChip(r, nowMillis))
        }
        // A GPS run with no usable fixes has no measurable zone/pace/distance -
        // surface the core's plain-language reason instead of an empty structure.
        if (r.distance_km <= 0.0) {
            if (r.summary.isNotBlank()) Text(r.summary, color = OnBgBody, style = Type.Body)
            if (r.citation.isNotBlank()) Text(citationLabel(r.citation), color = OnBgFaint, style = Type.Caption)
            return@PlainCard
        }
        Text(
            runFaceLine(r, unit),
            color = OnBgBody,
            style = Type.Body.merge(TabularFigures),
        )
        // The athlete's whole-session rating ("Effort 7/10"), once rated.
        RunEffortLine(r)
        // Zone row, hidden when no HR was measured. No "?" here: the card's one
        // disclosure is its findings "?" (the detail sheet explains HR zones).
        RunHrRow(r, explain = false)
        // Only the findings that ask for something (a real spike, a fading split,
        // SAFETY); neutral notes and praise wait in the detail sheet.
        RunFindings(r, face = true)
        // The run's single most significant record ("Fastest 10K").
        RunRecordChips(r, face = true)
    }
}

/**
 * The run's HR readout: the core's label ("HR 82% · Z2", never invented) and the
 * [HrZoneBar], identical on the History card and the run detail; [explain] adds the
 * HR-zones "?" (the detail sheet). Nothing when no HR was measured, never a dead
 * "HR -" row.
 */
@Composable
internal fun RunHrRow(r: RunResultView, explain: Boolean) {
    if (!r.hasHrReading()) return
    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        Text(
            r.hr_label,
            color = if (r.zone.startsWith("Z")) OnBgMuted else OnBgFaint,
            style = Type.Caption.merge(TabularFigures),
        )
        HrZoneBar(r.zone, Modifier.weight(1f))
        if (explain) GlossaryInfo("hrzones")
    }
}

/** The three HR zones as one bar, the run's [zone] at full strength (History card and run detail). */
@Composable
internal fun HrZoneBar(zone: String, modifier: Modifier = Modifier) {
    val status = LocalStatusColors.current
    Row(modifier = modifier, horizontalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
        listOf("Z1" to status.hrZone1, "Z2" to status.hrZone2, "Z3" to status.hrZone3).forEach { (z, c) ->
            Box(
                Modifier
                    .weight(1f)
                    .height(6.dp)
                    .clip(RoundedCornerShape(2.dp))
                    .background(if (z == zone) c else c.copy(alpha = 0.25f)),
            )
        }
    }
}

/**
 * The run's derived findings (core-judged): SPIKE/BASELINE, the split verdict +
 * SAFETY, INTERVAL·VI, with the core's verbatim notes, and ONE "?" holding the
 * grade badges, CONTESTED marker and citations of exactly the findings shown. On
 * the card [face] only findings that ask for a change or are safety-critical
 * render (a real distance spike, a fading split, a SAFETY split); neutral or
 * praising notes (an even split, a first-run baseline, the interval descriptor)
 * live in the detail sheet, which shows every finding, so the detail never shows
 * less than the face. SAFETY chip visibility is a hard rule on both. [showSplit]
 * false leaves the split verdict to the detail sheet's SPLITS section
 * ([RunSplitsSection]). The user's own run-type tag is the run's TITLE
 * ([runTitle]), not a chip. Renders nothing when no finding applies.
 */
@Composable
@OptIn(ExperimentalLayoutApi::class)
internal fun RunFindings(r: RunResultView, face: Boolean, showSplit: Boolean = true) {
    val status = LocalStatusColors.current
    val split = r.split
    val interval = r.interval
    // A first-ever run has no prior distance to gauge a spike against; that's
    // baseline-building, not a danger. Frame it neutrally; a REAL >10% jump keeps
    // the red SPIKE alarm. spike_has_baseline is the core's structured provenance.
    val isBaseline = r.spike_flag && !r.spike_has_baseline
    val showSpike = r.spike_flag && (r.spike_has_baseline || !face)
    val splitSafety = split?.safety_critical == true
    val shownSplit = split?.takeIf { showSplit && (!face || splitSafety || it.verdict == "fade") }
    // Only the interval-LIKE case is a finding; a steady run is the default
    // expectation, so labelling it adds noise.
    val shownInterval = interval?.takeIf { !face && it.kind == "interval" }
    if (!showSpike && shownSplit == null && shownInterval == null) return
    // The split verdict reads once: when the core sends its sentence, the sentence
    // alone carries it (no label chip echoing it above). SAFETY stays regardless.
    val splitChip = shownSplit != null && shownSplit.message.isBlank()
    val grades = listOfNotNull(
        shownSplit?.grade?.takeIf { it.isNotBlank() },
        shownInterval?.grade?.takeIf { it.isNotBlank() },
    )
    val citations = listOfNotNull(
        r.citation.takeIf { showSpike && it.isNotBlank() },
        shownSplit?.citation?.takeIf { it.isNotBlank() },
        shownInterval?.citation?.takeIf { it.isNotBlank() },
    ).distinct()
    val contested = shownSplit?.contested == true || shownInterval?.contested == true
    val hasWhy = grades.isNotEmpty() || citations.isNotEmpty() || contested
    var whyOpen by rememberSaveable(r.entry_id, r.observed_at, face) { mutableStateOf(false) }
    Column(verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
                if (showSpike || splitChip || (shownSplit != null && splitSafety) || shownInterval != null) {
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                        if (showSpike) {
                            if (isBaseline) {
                                Chip(stringResource(R.string.main_chip_baseline), status.hrZone2)
                            } else {
                                Chip(stringResource(R.string.main_chip_spike), status.danger)
                                // The card face keeps ONE "?" (the findings disclosure).
                                if (!face) GlossaryInfo("spike")
                            }
                        }
                        if (shownSplit != null) {
                            if (splitChip) Chip(shownSplit.label, splitVerdictColor(shownSplit.verdict, status))
                            if (splitSafety) Chip(stringResource(R.string.main_chip_safety), status.danger)
                        }
                        if (shownInterval != null) {
                            // hrZone2 (a non-danger zone colour, white-text-safe like the
                            // BASELINE chip), NOT the danger-red hrZone3: INTERVAL is a
                            // descriptive measurement, not a safety alarm.
                            Chip(shownInterval.label, status.hrZone2)
                            GlossaryInfo("vi")
                        }
                    }
                }
                // The core's notes, verbatim: the spike reason (including the first-run
                // "no prior run" line), the pacing sentence, the interval explanation.
                if (showSpike && r.spike_note.isNotBlank()) {
                    Text(r.spike_note, color = OnBgMuted, style = Type.Caption)
                }
                if (shownSplit != null && shownSplit.message.isNotBlank()) {
                    Text(shownSplit.message, color = OnBgMuted, style = Type.Caption)
                }
                if (shownInterval != null && shownInterval.message.isNotBlank()) {
                    Text(shownInterval.message, color = OnBgMuted, style = Type.Caption)
                }
            }
            if (hasWhy) {
                DisclosureButton(whyOpen, subject = stringResource(R.string.history_findings_subject)) { whyOpen = !whyOpen }
            }
        }
        if (hasWhy && whyOpen) {
            if (grades.isNotEmpty() || contested) {
                FlowRow(horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
                    grades.forEach { GradeChip(it) }
                    if (contested) Chip(stringResource(R.string.main_chip_contested), status.warn)
                }
            }
            citations.forEach {
                Text(citationLabel(it), color = OnBgFaint, style = Type.Caption)
            }
        }
    }
}

/** A split bar's fill: the fastest full split, another full split, the partial one. */
internal enum class SplitTone { Accent, Faint, Partial }

/** The word beside the fastest / slowest split's delta. */
internal enum class SplitTag { Fastest, Slowest }

/** One split row's derived figures: bar [fraction] of the track, [tone], [deltaSec]
 *  versus the average full split (null for the partial one) and its [tag]. */
internal data class SplitBar(val fraction: Float, val tone: SplitTone, val deltaSec: Int?, val tag: SplitTag?)

/**
 * Bars for the SPLITS rows (spec §2.5, a refinement of UD 2026-08-20): length is
 * speed normalised on `[SplitBarBaseline × slowest speed, fastest speed]`, so the
 * fastest split fills the track and one 5% slower reads about 75%. With 3 or more
 * full splits only the fastest is `Accent` (the rest `AccentFaint`), else every full
 * bar is; the partial split is its own tone. Fastest / Slowest tags only with 3+
 * full splits whose fastest and slowest paces read differently. A split without a
 * numeric pace gets an empty bar. Pure.
 */
internal fun splitBars(splits: List<RunSplitView>): List<SplitBar> {
    val timed = splits.filter { it.seconds_per_unit > 0 }
    val full = timed.filter { !it.partial }
    val hi = timed.maxOfOrNull { 1.0 / it.seconds_per_unit } ?: 0.0
    val lo = SplitBarBaseline * (timed.minOfOrNull { 1.0 / it.seconds_per_unit } ?: 0.0)
    val avg = if (full.isEmpty()) 0.0 else full.sumOf { it.seconds_per_unit }.toDouble() / full.size
    val fastest = full.minByOrNull { it.seconds_per_unit }
    val slowest = full.maxByOrNull { it.seconds_per_unit }
    val tagged = full.size >= 3 && fastest != null && slowest != null && fastest.pace != slowest.pace
    return splits.map { s ->
        val sec = s.seconds_per_unit
        val fraction = when {
            sec <= 0 -> 0f
            hi - lo <= 0.0 -> 1f
            else -> ((1.0 / sec - lo) / (hi - lo)).toFloat().coerceIn(0f, 1f)
        }
        val tone = when {
            s.partial -> SplitTone.Partial
            full.size < 3 || s === fastest -> SplitTone.Accent
            else -> SplitTone.Faint
        }
        val delta = if (s.partial || sec <= 0 || full.isEmpty()) null else Math.round(sec - avg).toInt()
        val tag = when {
            !tagged -> null
            s === fastest -> SplitTag.Fastest
            s === slowest -> SplitTag.Slowest
            else -> null
        }
        SplitBar(fraction, tone, delta, tag)
    }
}

/** "−0:04" (faster), "+0:07" (slower), "±0:00": a split's pace versus the average. */
internal fun splitDeltaLabel(deltaSec: Int): String {
    val sign = when {
        deltaSec < 0 -> "\u2212"
        deltaSec > 0 -> "+"
        else -> "±"
    }
    val a = Math.abs(deltaSec)
    return "$sign${a / 60}:${String.format(Locale.US, "%02d", a % 60)}"
}

/** The partial split's length in [unit]: its end minus the previous split's end. */
private fun partialLength(splits: List<RunSplitView>, i: Int, unit: DistanceUnit): Double {
    val prevKm = if (i > 0) splits[i - 1].distance_km else 0.0
    return metersToDisplay((splits[i].distance_km - prevKm) * 1000.0, unit)
}

/** The partial split's index: "¼", "½" or "¾" when it rounds to a quarter, else "0.18". */
internal fun partialIndexLabel(length: Double): String = when (Math.round(length * 4).toInt()) {
    1 -> "¼"
    2 -> "½"
    3 -> "¾"
    else -> displayFigure(length)
}

/** Rows shown before "Show all N splits" once a run has [SplitsFoldAt] or more. */
private const val SplitsShownFolded = 10
private const val SplitsFoldAt = 20

/** Height of a split row. */
private val SplitRowHeight = 40.dp

/** Minimum width of the split index column. */
private val SplitIndexWidth = 24.dp

/**
 * The run's SPLITS (spec §2.5): the core's pacing verdict with its "?", then one
 * row per split: index, pace, a speed bar on a full-length track ([splitBars]) and
 * the delta versus the average full split (the partial split shows its distance),
 * with Fastest / Slowest beside their deltas. 20 or more splits show the first 10
 * and "Show all N splits". Each row is one spoken group.
 */
@Composable
internal fun RunSplitsSection(run: RunResultView, splits: List<RunSplitView>, unit: DistanceUnit) {
    val res = LocalContext.current.resources
    val bars = remember(splits) { splitBars(splits) }
    var showAll by rememberSaveable(run.entry_id, run.observed_at) { mutableStateOf(false) }
    val indexLabels = splits.mapIndexed { i, s -> if (s.partial) partialIndexLabel(partialLength(splits, i, unit)) else "${s.index}" }
    val deltaLabels = splits.mapIndexed { i, s ->
        val d = bars[i].deltaSec
        when {
            s.partial -> "${displayFigure(partialLength(splits, i, unit))} ${unit.distanceLabel}"
            d != null -> splitDeltaLabel(d)
            else -> ""
        }
    }
    val fastestWord = stringResource(R.string.run_split_fastest)
    val slowestWord = stringResource(R.string.run_split_slowest)
    val indexStyle = Type.Chip.merge(TabularFigures)
    val paceStyle = Type.Body.copy(fontWeight = FontWeight.Bold).merge(TabularFigures)
    val deltaStyle = Type.Caption.merge(TabularFigures)
    val tagStyle = Type.Caption.copy(fontWeight = FontWeight.Bold)
    val measurer = rememberTextMeasurer()
    val density = LocalDensity.current
    fun widest(texts: List<String>, style: TextStyle): Dp = with(density) {
        (texts.filter { it.isNotEmpty() }.maxOfOrNull { measurer.measure(it, style, maxLines = 1, softWrap = false).size.width } ?: 0).toDp()
    }
    val indexW = maxOf(SplitIndexWidth, widest(indexLabels, indexStyle))
    val paceW = widest(listOf("88:88"), paceStyle)
    val deltaW = widest(deltaLabels, deltaStyle)
    val tags = bars.mapNotNull { it.tag }.map { if (it == SplitTag.Fastest) fastestWord else slowestWord }
    val tagW = widest(tags, tagStyle)
    val shown = if (splits.size >= SplitsFoldAt && !showAll) SplitsShownFolded else splits.size
    Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
        TileOverline(stringResource(R.string.main_splits))
        run.split?.let { SplitVerdictLine(it, run) }
        Column {
            for (i in 0 until shown) {
                val s = splits[i]
                val bar = bars[i]
                val spoken = splitSpoken(res, s, bar, deltaLabels[i], unit)
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .height(SplitRowHeight)
                        .clearAndSetSemantics { contentDescription = spoken },
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
                ) {
                    Text(indexLabels[i], color = OnBgFaint, style = indexStyle, textAlign = TextAlign.End, maxLines = 1, modifier = Modifier.width(indexW))
                    Text(s.pace, color = OnBgBody, style = paceStyle, maxLines = 1, modifier = Modifier.width(paceW))
                    Box(
                        Modifier
                            .weight(1f)
                            .height(SplitBarHeight)
                            .clip(RoundedCornerShape(50))
                            .background(OnBgBody.copy(alpha = HairlineAlpha.Card)),
                    ) {
                        if (bar.fraction > 0f) {
                            Box(
                                Modifier
                                    .fillMaxWidth(bar.fraction)
                                    .fillMaxHeight()
                                    .clip(RoundedCornerShape(50))
                                    .background(
                                        when (bar.tone) {
                                            SplitTone.Accent -> Accent
                                            SplitTone.Faint -> AccentFaint
                                            SplitTone.Partial -> OnBgFaint.copy(alpha = PartialBarAlpha)
                                        },
                                    ),
                            )
                        }
                    }
                    Text(
                        deltaLabels[i],
                        color = if ((bar.deltaSec ?: 0) < 0) OnBgBody else OnBgMuted,
                        style = deltaStyle,
                        textAlign = TextAlign.End,
                        maxLines = 1,
                        modifier = Modifier.width(deltaW),
                    )
                    if (tagW > 0.dp) {
                        Text(
                            when (bar.tag) {
                                SplitTag.Fastest -> fastestWord
                                SplitTag.Slowest -> slowestWord
                                null -> ""
                            },
                            color = if (bar.tag == SplitTag.Fastest) OnBgBody else OnBgMuted,
                            style = tagStyle,
                            maxLines = 1,
                            modifier = Modifier.width(tagW),
                        )
                    }
                }
            }
        }
        if (shown < splits.size) {
            TextAction(
                stringResource(R.string.run_splits_show_all, splits.size),
                onClick = { showAll = true },
                modifier = Modifier.offset(x = -TextActionInset),
            )
        }
    }
}

/** Height of a split bar and its track. */
private val SplitBarHeight = 10.dp

/** Opacity of the partial split's `OnBgFaint` bar. */
private const val PartialBarAlpha = 0.3f

/** "Split 1, 8 minutes 22 per mile, 1 second faster than average, fastest". */
private fun splitSpoken(res: Resources, s: RunSplitView, bar: SplitBar, distance: String, unit: DistanceUnit): String {
    val pace = if (s.seconds_per_unit > 0) {
        val m = s.seconds_per_unit / 60
        res.getString(
            R.string.run_pace_spoken,
            res.getQuantityString(R.plurals.run_minutes_spoken, m, m),
            s.seconds_per_unit % 60,
            res.getString(if (unit == DistanceUnit.Mi) R.string.run_unit_mile_word else R.string.run_unit_km_word),
        )
    } else {
        "${s.pace} ${unit.paceSuffix}"
    }
    if (s.partial) return res.getString(R.string.run_split_cd_partial, pace, distance)
    val d = bar.deltaSec
    val delta = when {
        d == null -> ""
        d == 0 -> res.getString(R.string.run_split_even)
        else -> {
            val a = Math.abs(d)
            val amount = if (a >= 60) {
                res.getQuantityString(R.plurals.run_minutes_spoken, a / 60, a / 60) +
                    (if (a % 60 > 0) " " + res.getQuantityString(R.plurals.run_seconds_spoken, a % 60, a % 60) else "")
            } else {
                res.getQuantityString(R.plurals.run_seconds_spoken, a, a)
            }
            res.getString(if (d < 0) R.string.run_split_faster else R.string.run_split_slower, amount)
        }
    }
    val row = res.getString(R.string.run_split_cd, "${s.index}", pace, delta).trimEnd(',', ' ')
    return when (bar.tag) {
        SplitTag.Fastest -> res.getString(R.string.run_split_cd_fastest, row)
        SplitTag.Slowest -> res.getString(R.string.run_split_cd_slowest, row)
        null -> row
    }
}

/**
 * The core's pacing verdict under SPLITS: its sentence in `Type.Body` (the verdict
 * label when the core sends none), a SAFETY chip when safety-critical (a hard rule),
 * and one "?" holding the grade, CONTESTED marker and citation.
 */
@Composable
@OptIn(ExperimentalLayoutApi::class)
private fun SplitVerdictLine(split: SplitVerdictView, run: RunResultView) {
    val status = LocalStatusColors.current
    val hasWhy = split.grade.isNotBlank() || split.citation.isNotBlank() || split.contested
    var open by rememberSaveable(run.entry_id, run.observed_at) { mutableStateOf(false) }
    Column(verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
                if (split.safety_critical) Chip(stringResource(R.string.main_chip_safety), status.danger)
                Text(split.message.ifBlank { split.label }, color = OnBgBody, style = Type.Body)
            }
            if (hasWhy) {
                DisclosureButton(open, subject = stringResource(R.string.main_splits)) { open = !open }
            }
        }
        if (hasWhy && open) {
            if (split.grade.isNotBlank() || split.contested) {
                FlowRow(horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
                    if (split.grade.isNotBlank()) GradeChip(split.grade)
                    if (split.contested) Chip(stringResource(R.string.main_chip_contested), status.warn)
                }
            }
            if (split.citation.isNotBlank()) Text(citationLabel(split.citation), color = OnBgFaint, style = Type.Caption)
        }
    }
}

// Neutral informational chip ground: a plain slate that carries NO semantic
// meaning. Used where a chip states a fact without valence (even/negative
// split). Deliberately not a `StatusColors` token.
private val ChipNeutral = Color(0xFF334155)

/**
 * Chip ground for the core's split verdict. Only a fade gets the semantic warn;
 * even/negative splits are neutral facts. An unknown future verdict falls back
 * to neutral rather than inventing a valence.
 */
private fun splitVerdictColor(verdict: String, status: StatusColors): Color = when (verdict) {
    "fade" -> status.warn
    else -> ChipNeutral // "even", "negative", unknown
}
