package app.milestone

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import java.util.Locale

/** One headline answer of a [CalculatorResultCard]: the figure, what it is, and
 *  an optional basis line ("1.8–2.7 g/kg × 81.7 kg · for weight loss"). */
internal data class CalcHeadline(val value: String, val caption: String, val basis: String? = null)

/** One claim behind a [CalculatorResultCard]'s "?": its core evidence row plus an
 *  optional derivation line (the Tanaka split under the HRmax row). */
internal data class CalcClaim(val row: GuidanceView, val extraDetail: String? = null)

/**
 * The ONE calculator result card (ui-sweep §10), shared by all four tools: a
 * [TileOverline] with the card's single "?", the answer as a `Type.Display`
 * headline over its caption (and basis line), the tool's own key/value rows or
 * table, the core's remaining notes, then "Clear result". The "?" panel stacks the
 * evidence of EVERY claim the card shows (grade badge, confidence, citation, why)
 * under each claim's Bold statement; the evidence itself is unchanged, only the
 * faces merged. A safety-critical claim keeps the SAFETY chip on the face.
 */
@Composable
internal fun CalculatorResultCard(
    overline: String,
    headlines: List<CalcHeadline>,
    claims: List<CalcClaim>,
    notes: List<GuidanceView>,
    onClear: () -> Unit,
    modifier: Modifier = Modifier,
    details: (@Composable () -> Unit)? = null,
) {
    val status = LocalStatusColors.current
    var expanded by rememberSaveable(overline, headlines.firstOrNull()?.value) { mutableStateOf(false) }
    val glossaryKeys = (claims.map { it.row } + notes).flatMap { glossaryKeysIn(it.summary) }.distinct()
    Box(modifier.fillMaxWidth().testTag("calc-result")) {
        PlainCard {
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
            ) {
                Box(Modifier.weight(1f)) { TileOverline(overline) }
                // A safety note carries its own chip below; the header speaks for
                // the claims the face figures stand for.
                if (claims.any { it.row.safety_critical && it.row !in notes }) Chip(stringResource(R.string.main_chip_safety), status.danger)
                // The 48dp target overflows the compact overline row instead of padding it.
                Box(Modifier.offset(x = Space.Md.dp)) {
                    DisclosureButton(expanded, subject = overline) { expanded = !expanded }
                }
            }
            headlines.forEachIndexed { i, h ->
                if (i > 0) CalcHairline()
                Column(verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
                    Text(h.value, color = OnBgBody, style = Type.Display.merge(TabularFigures), modifier = Modifier.testTag("calc-headline"))
                    Text(h.caption, color = OnBgMuted, style = Type.Caption)
                    h.basis?.let { Text(it, color = OnBgMuted, style = Type.Caption.merge(TabularFigures)) }
                }
            }
            if (expanded) {
                CalcHairline()
                claims.forEachIndexed { i, c ->
                    if (i > 0) CalcHairline()
                    Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
                        Text(stripClaimIds(guidanceSummary(c.row)), color = OnBgBody, style = Type.Body.copy(fontWeight = FontWeight.Bold))
                        WhyDetail(
                            why = c.row.why,
                            grade = c.row.grade,
                            citation = c.row.citation,
                            confidence = c.row.confidence,
                            contested = c.row.contested,
                            // Safety is a rule, not a probability.
                            showConfidence = !c.row.safety_critical,
                            extraDetail = c.extraDetail,
                            // The card's jargon links once, under its last claim.
                            glossaryKeys = if (i == claims.lastIndex) glossaryKeys else emptyList(),
                        )
                    }
                }
            }
            if (details != null) {
                CalcHairline()
                details()
            }
            notes.forEach { n ->
                CalcHairline()
                Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
                    if (n.safety_critical) Chip(stringResource(R.string.main_chip_safety), status.danger)
                    Text(
                        stripClaimIds(guidanceSummary(n)),
                        color = OnBgBody,
                        style = if (n.safety_critical) Type.Body.copy(fontWeight = FontWeight.Bold) else Type.Body,
                    )
                }
            }
            CalcHairline()
            TextAction(
                stringResource(R.string.main_calc_clear),
                onClick = onClear,
                modifier = Modifier.offset(x = -TextActionInset),
                description = stringResource(R.string.main_calc_clear_subject, overline),
            )
        }
    }
}

/** The row hairline inside a result card. */
@Composable
private fun CalcHairline() {
    HorizontalDivider(thickness = 1.dp, color = OnBgBody.copy(alpha = HairlineAlpha.Row))
}

/** "65–79" for a range, "100" when both ends round to the same figure. */
internal fun wholeRange(low: Double, high: Double): String {
    val lo = Math.round(low)
    val hi = Math.round(high)
    return if (lo == hi) "$lo" else "$lo–$hi"
}

/** Race predictor result: the finish-time range as the headline, the goal
 *  distance as the caption; the methods and caveats sit behind the "?" and in
 *  the notes. A degenerate prediction (no usable race) shows its prose instead. */
@Composable
internal fun RaceResult(rp: RacePredictionView, onClear: () -> Unit) {
    val primary = GuidanceView(
        summary = rp.summary,
        grade = rp.grade,
        citation = rp.citation,
        confidence = rp.confidence,
        safety_critical = rp.safety_critical,
        contested = rp.contested,
    )
    val usable = rp.predicted.isNotBlank() && rp.predicted != "-"
    CalculatorResultCard(
        overline = stringResource(R.string.main_calc_race_overline),
        headlines = listOf(
            if (usable) {
                CalcHeadline(rp.predicted, stringResource(R.string.main_calc_race_caption, rp.goal_label))
            } else {
                CalcHeadline(EmptyValuePlaceholder, stripClaimIds(rp.summary))
            },
        ),
        claims = listOf(CalcClaim(primary)) + rp.notes.map { CalcClaim(it) },
        notes = rp.notes,
        onClear = onClear,
    )
}

/** Protein result: one headline per graded context ("147–220" grams per day)
 *  with its g/kg × bodyweight basis. A refused (RED-S) or hold row has no figure
 *  and renders as its own safety [EvidenceCard] instead. */
@Composable
internal fun ProteinResult(model: ViewModel, onClear: () -> Unit) {
    val rows = model.protein_targets
    val figures = model.protein_figures.filter { !it.refused && it.row in rows.indices }
    if (figures.isEmpty()) {
        rows.forEach {
            EvidenceCard(guidanceSummary(it), it.grade, it.citation, it.confidence, it.safety_critical, it.contested, it.section, why = it.why)
        }
        TextAction(stringResource(R.string.main_calc_clear), onClick = onClear, modifier = Modifier.offset(x = -TextActionInset))
        return
    }
    val bwKg = model.protein_input?.bodyweight_kg
    val faceRows = figures.map { it.row }.toSet()
    CalculatorResultCard(
        overline = stringResource(R.string.main_calc_protein_overline),
        headlines = figures.map { f ->
            val context = stringResource(if (f.kind == "masters") R.string.main_calc_protein_masters else R.string.main_calc_protein_deficit)
            CalcHeadline(
                value = wholeRange(f.low_g_per_day, f.high_g_per_day),
                caption = stringResource(R.string.main_calc_protein_caption),
                basis = bwKg?.let {
                    stringResource(
                        R.string.main_calc_protein_basis,
                        String.format(Locale.US, "%.1f", f.low_g_per_kg),
                        String.format(Locale.US, "%.1f", f.high_g_per_kg),
                        trimDecimal(Math.round(it * 10) / 10.0),
                        context,
                    )
                } ?: context,
            )
        },
        claims = rows.map { CalcClaim(it) },
        notes = rows.filterIndexed { i, _ -> i !in faceRows },
        onClear = onClear,
    )
}

/** Swatch colour of a Daniels band, keyed by its core [HrZoneBandView.kind]: the
 *  History HR bar's three zone colours (Easy; Marathon and Threshold; Interval and
 *  Repetition). */
@Composable
private fun bandSwatch(kind: String): Color {
    val status = LocalStatusColors.current
    return when (kind) {
        "Easy" -> status.hrZone1
        "Marathon", "Threshold" -> status.hrZone2
        else -> status.hrZone3
    }
}

/** HR zones result: HRmax as the headline, then one 48dp row per band (swatch,
 *  name, %HRmax, bpm or "by pace"). %VO2max and the formulas sit behind the "?";
 *  the method, MAF and re-test rows are the card's notes. Without structured
 *  bands (a refused age) the core's rows render as plain [EvidenceCard]s. */
@Composable
internal fun HrZonesResult(model: ViewModel, onClear: () -> Unit) {
    val rows = model.hr_zones
    val hm = model.hr_max
    val bands = model.hr_zone_bands.filter { it.row in rows.indices }
    if (hm == null || bands.isEmpty()) {
        rows.forEach {
            EvidenceCard(guidanceSummary(it), it.grade, it.citation, it.confidence, it.safety_critical, it.contested, it.section, why = it.why)
        }
        TextAction(stringResource(R.string.main_calc_clear), onClick = onClear, modifier = Modifier.offset(x = -TextActionInset))
        return
    }
    // The age estimate's Tanaka split rides under the HRmax claim (a measured
    // maximum has none); built from the structured hr_max fields.
    val tanaka = hm.takeIf { !it.measured && it.tanaka_intercept > 0.0 }
        ?.let { stringResource(R.string.log_tool_tanaka, it.tanaka_intercept.toInt(), trimDecimal(it.tanaka_slope), it.age_years.toInt()) }
    val faceRows = bands.map { it.row }.toSet() + 0
    CalculatorResultCard(
        overline = stringResource(R.string.main_calc_hr_overline),
        headlines = listOf(
            CalcHeadline(
                "${hm.bpm.toInt()}",
                stringResource(if (hm.measured) R.string.main_calc_hr_caption_measured else R.string.main_calc_hr_caption_estimated),
            ),
        ),
        claims = rows.mapIndexed { i, r -> CalcClaim(r, extraDetail = if (i == 0) tanaka else null) },
        notes = rows.filterIndexed { i, _ -> i !in faceRows },
        onClear = onClear,
    ) {
        Column {
            bands.forEachIndexed { i, b ->
                if (i > 0) CalcHairline()
                HrZoneRow(b, bandSwatch(b.kind))
            }
        }
    }
}

/** One zone-table row: swatch, name, %HRmax, then bpm (or "by pace") end-aligned. */
@Composable
private fun HrZoneRow(band: HrZoneBandView, swatch: Color) {
    val pct = stringResource(R.string.main_calc_percent, wholeRange(band.pct_low, band.pct_high))
    val bpm = if (band.pace_governed) stringResource(R.string.main_calc_hr_by_pace) else wholeRange(band.bpm_low, band.bpm_high)
    val reserve = if (!band.pace_governed && band.karvonen_high_bpm > 0.0) {
        stringResource(R.string.main_calc_hr_reserve, wholeRange(band.karvonen_low_bpm, band.karvonen_high_bpm))
    } else {
        null
    }
    val spoken = listOfNotNull(
        band.name,
        stringResource(R.string.main_calc_hr_pct_cd, pct),
        if (band.pace_governed) bpm else stringResource(R.string.log_value_with_unit, bpm, stringResource(R.string.log_unit_bpm)),
        reserve,
    ).joinToString(", ")
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = ActionMinHeight)
            .padding(vertical = Space.Sm.dp)
            .semantics(mergeDescendants = true) { contentDescription = spoken },
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        Box(Modifier.size(12.dp).background(swatch, RoundedCornerShape(3.dp)))
        Text(band.name, color = OnBgBody, style = Type.Body.copy(fontWeight = FontWeight.Bold), modifier = Modifier.weight(1.3f))
        Text(pct, color = OnBgMuted, style = Type.Caption.merge(TabularFigures), modifier = Modifier.weight(1f))
        Column(Modifier.weight(1.2f), horizontalAlignment = Alignment.End) {
            Text(
                bpm,
                color = if (band.pace_governed) OnBgMuted else OnBgBody,
                style = if (band.pace_governed) Type.Caption else Type.Body.copy(fontWeight = FontWeight.Bold).merge(TabularFigures),
                textAlign = TextAlign.End,
            )
            reserve?.let { Text(it, color = OnBgMuted, style = Type.Caption.merge(TabularFigures), textAlign = TextAlign.End) }
        }
    }
}

/** "2–3 min", "90 s–2 min", "90 s": a rest window from whole seconds. */
@Composable
private fun restWindow(lowS: Int, highS: Int): String {
    @Composable
    fun part(s: Int) = if (s % 60 == 0) stringResource(R.string.main_calc_minutes, s / 60) else stringResource(R.string.main_calc_seconds, s)
    return when {
        lowS == highS -> part(lowS)
        lowS % 60 == 0 && highS % 60 == 0 -> stringResource(R.string.main_calc_minutes_range, lowS / 60, highS / 60)
        else -> "${part(lowS)}–${part(highS)}"
    }
}

/** Volume planner result: the first → last weekly sets as the headline, the ramp
 *  chart, then the plan's key/value rows; the rest of the core's rows are notes.
 *  Without structured figures (a hold, an unknown muscle) the rows render as
 *  plain [EvidenceCard]s. */
@Composable
internal fun VolumeResult(model: ViewModel, onClear: () -> Unit) {
    val rows = model.hypertrophy_plan
    val fig = model.hypertrophy_figures
    val ramp = model.hypertrophy_ramp
    val input = model.hypertrophy_input
    if (fig == null || ramp.isEmpty() || input == null) {
        rows.forEach {
            EvidenceCard(guidanceSummary(it), it.grade, it.citation, it.confidence, it.safety_critical, it.contested, why = it.why)
        }
        TextAction(stringResource(R.string.main_calc_clear), onClick = onClear, modifier = Modifier.offset(x = -TextActionInset))
        return
    }
    val headline = if (ramp.first() == ramp.last()) "${ramp.first()}" else stringResource(R.string.main_calc_ramp, ramp.first(), ramp.last())
    val faceRows = fig.face_rows.toSet()
    CalculatorResultCard(
        overline = stringResource(R.string.main_calc_volume_overline),
        headlines = listOf(
            CalcHeadline(headline, stringResource(R.string.main_calc_volume_caption, hypertrophyMuscleLabel(input.muscle), input.weeks)),
        ),
        claims = rows.map { CalcClaim(it) },
        notes = rows.filterIndexed { i, _ -> i !in faceRows },
        onClear = onClear,
    ) {
        VolumeRampChart(ramp, fig)
        Column {
            val kv = listOf(
                stringResource(R.string.main_calc_kv_rir) to fig.rir_by_week.joinToString(" · "),
                stringResource(R.string.main_calc_kv_frequency) to stringResource(
                    R.string.main_calc_kv_frequency_value,
                    wholeRange(fig.freq_low.toDouble(), fig.freq_high.toDouble()),
                    wholeRange(fig.per_session_low.toDouble(), fig.per_session_high.toDouble()),
                ),
                stringResource(R.string.main_calc_kv_rest_heavy) to restWindow(fig.rest_heavy_low_s, fig.rest_heavy_high_s),
                stringResource(R.string.main_calc_kv_rest_isolation) to restWindow(fig.rest_isolation_low_s, fig.rest_isolation_high_s),
                stringResource(R.string.main_calc_kv_shape) to pluralStringResource(
                    R.plurals.main_calc_kv_shape_value,
                    fig.deload_weeks,
                    wholeRange(fig.build_weeks_low.toDouble(), fig.build_weeks_high.toDouble()),
                    fig.deload_weeks,
                ),
            )
            kv.forEachIndexed { i, (label, value) ->
                if (i > 0) CalcHairline()
                KeyValueRow(label, value, emphasis = true)
            }
        }
    }
}

/** Height of the volume ramp chart (the History weekly chart's height). */
private val RampChartHeight = 112.dp

/**
 * The block's weekly-set ramp: one Accent bar per week with its set count above
 * and "Wk n" below, over an `OnBgBody @6%` band marking the productive range
 * (labelled at its right end) and a dashed `OnBgFaint @30%` line at the max.
 * Self-explanatory, no legend; spoken as one sentence.
 */
@Composable
private fun VolumeRampChart(ramp: List<Int>, fig: HypertrophyFiguresView) {
    val measurer = rememberTextMeasurer()
    val density = LocalDensity.current
    val accent = Accent
    val band = OnBgBody.copy(alpha = HairlineAlpha.Card)
    val maxLine = OnBgFaint.copy(alpha = HairlineAlpha.Strong)
    val numeralColor = OnBgBody
    val labelColor = OnBgMuted
    val numeralStyle = Type.Chip.merge(TabularFigures).copy(color = numeralColor)
    val captionStyle = Type.Caption.copy(color = labelColor)
    val productive = wholeRange(fig.productive_low.toDouble(), fig.productive_high.toDouble())
    val bandLabel = stringResource(R.string.main_calc_productive, productive)
    val weekLabels = ramp.indices.map { stringResource(R.string.main_calc_week_short, it + 1) }
    val spoken = stringResource(
        R.string.main_calc_ramp_cd,
        ramp.mapIndexed { i, n -> "${weekLabels[i]} $n" }.joinToString(", "),
        productive,
        fig.max_sets,
    )
    val top = (maxOf(fig.max_sets, ramp.maxOrNull() ?: 0) + 1).coerceAtLeast(1)
    // Laid out once per composition: the band label (two lines) sets the end
    // gutter, the week labels add a caption row under the 112dp plot.
    val bandLayouts = bandLabel.split(' ', limit = 2).map { measurer.measure(it, captionStyle) }
    val weekLayouts = weekLabels.map { measurer.measure(it, captionStyle) }
    val weekRowPx = (weekLayouts.maxOfOrNull { it.size.height } ?: 0) + with(density) { Space.Sm.dp.roundToPx() }
    val canvasHeight = RampChartHeight + with(density) { weekRowPx.toDp() }
    Canvas(
        Modifier
            .fillMaxWidth()
            .padding(vertical = Space.Sm.dp)
            .height(canvasHeight)
            .semantics { contentDescription = spoken }
            .testTag("volume-ramp-chart"),
    ) {
        val gap = Space.Md.dp.toPx()
        val gutter = bandLayouts.maxOf { it.size.width } + gap
        val plotW = size.width - gutter
        val numeralRoom = measurer.measure("00", numeralStyle).size.height + Space.Xs.dp.toPx()
        val plotBottom = size.height - weekRowPx
        val plotH = plotBottom - numeralRoom
        fun y(v: Int) = numeralRoom + plotH * (1f - v.toFloat() / top)
        // Productive band across the whole width, its label in the end gutter.
        val bandTop = y(fig.productive_high)
        val bandBottom = y(fig.productive_low)
        drawRect(band, topLeft = Offset(0f, bandTop), size = Size(size.width, bandBottom - bandTop))
        var ly = (bandTop + bandBottom) / 2f - bandLayouts.sumOf { it.size.height } / 2f
        bandLayouts.forEach {
            drawText(it, topLeft = Offset(size.width - it.size.width, ly))
            ly += it.size.height
        }
        // Max landmark: a dashed line over the bars' width.
        val my = y(fig.max_sets)
        val dash = 4.dp.toPx()
        drawLine(
            maxLine,
            start = Offset(0f, my),
            end = Offset(plotW, my),
            strokeWidth = 1.dp.toPx(),
            pathEffect = PathEffect.dashPathEffect(floatArrayOf(dash, dash)),
        )
        val slot = plotW / ramp.size
        val barW = slot * 0.5f
        ramp.forEachIndexed { i, n ->
            val x = slot * i + (slot - barW) / 2f
            val cx = x + barW / 2f
            val by = y(n)
            drawRect(accent, topLeft = Offset(x, by), size = Size(barW, plotBottom - by))
            val num = measurer.measure("$n", numeralStyle)
            drawText(num, topLeft = Offset(cx - num.size.width / 2f, by - num.size.height - Space.Xs.dp.toPx()))
            val wk = weekLayouts[i]
            drawText(wk, topLeft = Offset(cx - wk.size.width / 2f, size.height - wk.size.height))
        }
    }
}
