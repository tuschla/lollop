package app.milestone

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInParent
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import app.milestone.resources.Res
import app.milestone.resources.log_btn_compute_hr
import app.milestone.resources.log_btn_compute_protein
import app.milestone.resources.log_btn_plan_volume
import app.milestone.resources.log_btn_predict
import app.milestone.resources.log_field_accumulation_weeks
import app.milestone.resources.log_field_age
import app.milestone.resources.log_field_bodyweight
import app.milestone.resources.log_field_goal_race
import app.milestone.resources.log_field_muscle
import app.milestone.resources.log_field_recent_min
import app.milestone.resources.log_field_recent_race
import app.milestone.resources.log_field_recent_sec
import app.milestone.resources.log_field_resting_hr
import app.milestone.resources.log_field_weekly_volume
import app.milestone.resources.log_field_weeks_since_race
import app.milestone.resources.log_prefill_profile
import app.milestone.resources.log_protein_deficit_only
import app.milestone.resources.log_protein_no_context
import app.milestone.resources.log_protein_weight_hint
import app.milestone.resources.log_race_half
import app.milestone.resources.log_switch_deficit
import app.milestone.resources.log_switch_masters
import app.milestone.resources.log_switch_not_growing
import app.milestone.resources.log_switch_race_stale
import app.milestone.resources.log_switch_recovering_easily
import app.milestone.resources.log_switch_use_rhr
import app.milestone.resources.log_unit_min
import app.milestone.resources.log_unit_sec
import app.milestone.resources.log_unit_years
import app.milestone.resources.log_value_bpm
import app.milestone.resources.log_value_with_unit
import app.milestone.resources.main_calc_value_hrmax
import app.milestone.resources.main_calc_value_sets
import app.milestone.resources.main_calc_value_sets_ramp
import app.milestone.resources.main_calc_value_weeks
import app.milestone.resources.main_differs_from_profile
import app.milestone.resources.main_evidence_references_body
import app.milestone.resources.main_evidence_references_title
import app.milestone.resources.main_overline_training_load
import app.milestone.resources.main_reference_empty
import app.milestone.resources.main_section_reference
import app.milestone.resources.main_tool_hr_cta_add_age
import app.milestone.resources.main_tool_hr_cta_compute
import app.milestone.resources.main_tool_hr_title
import app.milestone.resources.main_tool_protein_cta_add_bw
import app.milestone.resources.main_tool_protein_cta_compute
import app.milestone.resources.main_tool_protein_cta_deficit
import app.milestone.resources.main_tool_protein_title
import app.milestone.resources.main_tool_race_cta
import app.milestone.resources.main_tool_race_title
import app.milestone.resources.main_tool_volume_cta
import app.milestone.resources.main_tool_volume_title
import app.milestone.resources.main_training_load_detail
import app.milestone.resources.main_unit_g_per_day
import app.milestone.resources.main_value_set
import app.milestone.resources.ui_cd_collapsed
import app.milestone.resources.ui_cd_expanded
import kotlin.math.roundToInt
import kotlin.math.abs
import org.jetbrains.compose.resources.StringResource
import org.jetbrains.compose.resources.pluralStringResource
import org.jetbrains.compose.resources.stringResource

/** The on-demand Coach calculators, launched from the Profile calculator rows. */
internal enum class CoachTool(val titleRes: StringResource) {
    Race(Res.string.main_tool_race_title),
    Volume(Res.string.main_tool_volume_title),
    Protein(Res.string.main_tool_protein_title),
    HrZones(Res.string.main_tool_hr_title),
}

/**
 * The calculators as one card of modality-gated rows (Profile → CALCULATORS); no
 * Volume or Protein row under 18 (safety-011, adult protein targets). Tapping a
 * row hands its [CoachTool] to [onOpen], whose caller hosts the [CoachToolSheet].
 */
@Composable
internal fun CalculatorsCard(model: ViewModel, onOpen: (CoachTool) -> Unit) {
    val minor = model.underEighteen()
    val showRace = model.showRunning()
    val showVolume = model.showLifting() && !minor
    CoachToolList {
        if (showRace) {
            CoachToolRow(
                stringResource(CoachTool.Race.titleRes),
                cta = stringResource(Res.string.main_tool_race_cta),
                // No prediction reads as the result card's placeholder.
                value = model.race_prediction?.let { listOf(it.predicted ?: EmptyValuePlaceholder, it.goal_label).filter(String::isNotBlank).joinToString(" · ") },
            ) { onOpen(CoachTool.Race) }
        }
        if (showVolume) {
            CoachToolRow(
                stringResource(CoachTool.Volume.titleRes),
                cta = stringResource(Res.string.main_tool_volume_cta),
                // The last plan's weekly sets ("12→20 sets"), from the core's
                // structured ramp; the block length alone when it shows no ramp.
                value = model.hypertrophy_input?.takeIf { model.hypertrophy_plan.isNotEmpty() }?.let { input ->
                    val ramp = model.hypertrophy_ramp
                    when {
                        ramp.isEmpty() -> stringResource(Res.string.main_calc_value_weeks, input.weeks)
                        ramp.first() == ramp.last() -> stringResource(Res.string.main_calc_value_sets, ramp.first())
                        else -> stringResource(Res.string.main_calc_value_sets_ramp, ramp.first(), ramp.last())
                    }
                },
            ) { onOpen(CoachTool.Volume) }
        }
        if (!minor) {
            // The COMPUTED value ("120–140 g/day"), read from the core's
            // structured protein_figures (no prose scrape). A RED-S deficit
            // refusal carries no g/day figure (refused), so it falls back to
            // "Set" (a result exists to reopen). Rounded like the core rounds the
            // card's figures, so the row and the card always show the same grams.
            val proteinGrams = model.protein_figures
                .firstOrNull { !it.refused }
                ?.let { wholeRange(it.low_g_per_day, it.high_g_per_day) }
            CoachToolRow(
                stringResource(CoachTool.Protein.titleRes),
                cta = when {
                    !model.protein_masters_offered -> stringResource(Res.string.main_tool_protein_cta_deficit)
                    model.profile?.bodyweight_kg != null -> stringResource(Res.string.main_tool_protein_cta_compute)
                    else -> stringResource(Res.string.main_tool_protein_cta_add_bw)
                },
                value = when {
                    proteinGrams != null -> stringResource(Res.string.log_value_with_unit, proteinGrams, stringResource(Res.string.main_unit_g_per_day))
                    model.protein_targets.isNotEmpty() -> stringResource(Res.string.main_value_set)
                    else -> null
                },
            ) { onOpen(CoachTool.Protein) }
        }
        if (showRace) {
            // After compute, the actual HRmax ("187 HRmax bpm"), read from
            // the core's structured hr_max (never recomputed).
            val hrMax = model.hr_max?.bpm?.toInt()
            CoachToolRow(
                stringResource(CoachTool.HrZones.titleRes),
                cta = if (model.profile?.age_years != null) stringResource(Res.string.main_tool_hr_cta_compute) else stringResource(Res.string.main_tool_hr_cta_add_age),
                value = when {
                    hrMax != null -> stringResource(Res.string.main_calc_value_hrmax, hrMax)
                    model.hr_zones.isNotEmpty() -> stringResource(Res.string.main_value_set)
                    else -> null
                },
            ) { onOpen(CoachTool.HrZones) }
        }
    }
}

/** True for an under-18 profile (the core's age gate, `health.youth`): Profile
 *  offers no Volume planner or Protein calculator (safety-011: no autonomous load
 *  prescription for youth; the protein targets are graded for adults). */
internal fun ViewModel.underEighteen(): Boolean = profile?.health?.youth == true

/** Whether [CalculatorsCard] has any row: every adult gets Protein, an
 *  under-18 profile only the running tools. */
internal fun ViewModel.hasCalculators(): Boolean = !underEighteen() || showRunning()

/** The calculators as one card of rows, a compact tool list rather than a grid
 *  of half-empty tiles. */
@Composable
private fun CoachToolList(rows: @Composable ColumnScope.() -> Unit) {
    val shape = RoundedCornerShape(Space.Card.dp)
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(BgElevated)
            .border(1.dp, OnBgBody.copy(alpha = HairlineAlpha.Card), shape)
            .padding(vertical = Space.Xs.dp),
        content = rows,
    )
}

/**
 * One calculator row: the tool name, then its computed value once there is one
 * ("45:37 · 10K", "12→20 sets", "147–220 g/day", "HRmax 184"), else what a tap
 * does ("Set a recent race"), and an accent chevron: the whole row opens the
 * tool's sheet. At a large font scale the value drops below the name, rather
 * than wrapping in a narrow column.
 */
@Composable
private fun ColumnScope.CoachToolRow(
    label: String,
    cta: String,
    value: String?,
    onClick: () -> Unit,
) {
    val stacked = LocalDensity.current.fontScale >= StackedValueFontScale
    val valueStyle = if (value != null) Type.Body.merge(TabularFigures) else Type.Caption
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = ActionMinHeight)
            .clickable(role = Role.Button, onClick = onClick)
            .padding(horizontal = Space.Card.dp, vertical = Space.Md.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        if (stacked) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
                Text(label, color = OnBgBody, style = Type.Body)
                Text(value ?: cta, color = OnBgMuted, style = valueStyle)
            }
        } else {
            Text(label, color = OnBgBody, style = Type.Body, modifier = Modifier.weight(1f))
            Text(value ?: cta, color = OnBgMuted, style = valueStyle, textAlign = TextAlign.End, maxLines = 1)
        }
        RowChevron(tint = Accent)
    }
}

private const val StackedValueFontScale = 1.3f

/** How much of a result card (overline + headline + caption) must show when a
 *  calculator sheet opens on a saved result. */
private val ResultAnswerRoom = 168.dp

/**
 * Body of the Coach calculator [ModalBottomSheet]: a title, the tool's FORM, then
 * its one [CalculatorResultCard]. The forms' keypad shows only
 * while a field is active: tapping a well opens it; the primary action, Back, a
 * tap on the sheet's bare ground or any other control folds it, so a sheet opens
 * with its result card in view. Computing scrolls the result into view.
 */
@Composable
internal fun CoachToolSheet(
    tool: CoachTool,
    model: ViewModel,
    onEvent: (Event) -> Unit,
) {
    val title = stringResource(tool.titleRes)
    val weightUnit = model.weightUnit()
    val scroll = rememberScrollState()
    var keypadOpen by rememberSaveable(tool) { mutableStateOf(false) }
    BackHandler(enabled = keypadOpen) { keypadOpen = false }
    // Content-space Y of the result (0 until it first lays out); computing
    // scrolls to it.
    var resultsTop by remember { mutableStateOf(0) }
    val hasResult = when (tool) {
        CoachTool.Race -> model.race_prediction != null
        CoachTool.Volume -> model.hypertrophy_plan.isNotEmpty()
        CoachTool.Protein -> model.protein_targets.isNotEmpty()
        CoachTool.HrZones -> model.hr_zones.isNotEmpty()
    }
    // Every Compute folds the keypad and asks for the result: one frame first so
    // its position reflects the folded keypad; animateScrollTo clamps to the
    // growing maxValue.
    var scrollOnResult by remember(tool) { mutableStateOf(false) }
    LaunchedEffect(hasResult, scrollOnResult) {
        if (hasResult && scrollOnResult) {
            withFrameNanos { }
            scroll.animateScrollTo(resultsTop)
            scrollOnResult = false
        }
    }
    // A sheet that opens on a saved result shows its answer without a scroll: a
    // tall form (the race predictor) scrolls just far enough that the card's
    // overline and headline clear the bottom edge.
    val answerRoomPx = with(LocalDensity.current) { ResultAnswerRoom.roundToPx() }
    LaunchedEffect(tool) {
        if (!hasResult) return@LaunchedEffect
        withFrameNanos { }
        withFrameNanos { }
        val shortfall = resultsTop + answerRoomPx - scroll.viewportSize
        if (scroll.viewportSize > 0 && shortfall > 0) scroll.scrollTo(shortfall)
    }
    val compute: (Event) -> Unit = {
        keypadOpen = false
        scrollOnResult = true
        onEvent(it)
    }
    Column(
        modifier = Modifier
            .pointerInput(keypadOpen) {
                if (keypadOpen) detectTapGestures { keypadOpen = false }
            }
            .sheetBody(scroll, Space.ReadingSheet.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Card.dp),
    ) {
        SheetTitle(title)
        // The tool's FORM, seeded from the core's echoed query so it rehydrates
        // after a cold start. Profile owns bodyweight and age; the forms prefill
        // from it and keep an override local (never written back to the profile).
        when (tool) {
            CoachTool.Race -> RacePredictorForm(model.race_prediction, keypadOpen, { keypadOpen = it }) { compute(it) }
            CoachTool.Volume -> HypertrophyPlannerForm(model.hypertrophy_input) { compute(it) }
            CoachTool.Protein -> {
                ProteinForm(
                    model.protein_input,
                    keypadOpen = keypadOpen,
                    onKeypad = { keypadOpen = it },
                    profileBodyweightKg = model.profile?.bodyweight_kg,
                    weightUnit = weightUnit,
                    mastersAvailable = model.protein_masters_offered,
                ) { bodyweight, masters, deficit ->
                    compute(Event.ComputeProtein(bodyweight, masters, deficit))
                }
                // Stable-slot divergence hint: always composed (so the sheet height
                // never jumps), non-empty only when the last-computed bodyweight
                // differs from the profile's, naming the profile value it differs from.
                val profileBw = model.profile?.bodyweight_kg
                val formBw = model.protein_input?.bodyweight_kg
                val divergence = if (profileBw != null && formBw != null &&
                    abs(formBw - profileBw) >= 0.05
                ) {
                    stringResource(Res.string.main_differs_from_profile, weightFigure(profileBw, weightUnit), weightSuffix(weightUnit))
                } else {
                    ""
                }
                Text(divergence, color = OnBgFaint, style = Type.Caption, maxLines = 1)
            }
            CoachTool.HrZones -> HrZonesForm(
                model.hr_zone_input,
                keypadOpen = keypadOpen,
                onKeypad = { keypadOpen = it },
                profileAgeYears = model.profile?.age_years,
                profileRestingHrBpm = model.profile?.resting_hr_bpm,
            ) { age, rhr -> compute(Event.ComputeHrZones(age, rhr)) }
        }
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .onGloballyPositioned { resultsTop = it.positionInParent().y.toInt() },
            verticalArrangement = Arrangement.spacedBy(Space.Card.dp),
        ) {
            when (tool) {
                CoachTool.Race -> model.race_prediction?.let { RaceResult(it) { onEvent(Event.ClearRacePrediction) } }
                CoachTool.Volume -> if (model.hypertrophy_plan.isNotEmpty()) VolumeResult(model) { onEvent(Event.ClearHypertrophyPlan) }
                CoachTool.Protein -> if (model.protein_targets.isNotEmpty()) ProteinResult(model) { onEvent(Event.ClearProtein) }
                CoachTool.HrZones -> if (model.hr_zones.isNotEmpty()) HrZonesResult(model) { onEvent(Event.ClearHrZones) }
            }
        }
    }
}

/** Cards in the Reference library's Training load group: the weekly report rows plus
 *  the heart-rate and session-effort (sRPE) load cards, two separate series. */
private fun trainingLoadCount(model: ViewModel): Int =
    model.weekly_report.size + (if (model.training_load != null) 1 else 0) + (if (model.srpe_load != null) 1 else 0)

internal fun referenceLibraryCount(model: ViewModel): Int =
    model.guidance.size + model.reference.size + trainingLoadCount(model)

private class ReferenceGroupEntry(val title: String, val count: Int, val cards: @Composable () -> Unit)

/**
 * The Reference library, a Profile-opened sheet: every
 * evidence-graded programming-guidance group (Strength/Hypertrophy/Running/…),
 * the Training load group (the core-owned weekly report rows and the heart-rate
 * and session-effort load cards) and the Reference source list, as one carded
 * list of group rows. An opened group's cards render below its row inside the
 * same card, between hairlines, each with its own "?" (grade chips stay behind
 * it). Every summary here is regular weight except a SAFETY card. No coaching
 * logic; it only lays out core-emitted cards.
 */
@Composable
internal fun ReferenceLibrarySheet(model: ViewModel) {
    val otherGuidance = model.guidance
    val loadCount = trainingLoadCount(model)
    Column(
        modifier = Modifier.sheetBody(rememberScrollState(), Space.ReadingSheet.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Card.dp),
    ) {
        SheetTitle(stringResource(Res.string.main_evidence_references_title))
        Text(
            stringResource(Res.string.main_evidence_references_body),
            color = OnBgMuted,
            style = Type.Body,
        )
        if (referenceLibraryCount(model) == 0) {
            Text(
                stringResource(Res.string.main_reference_empty),
                color = OnBgFaint,
                style = Type.Caption,
            )
            return@Column
        }
        val groups = buildList<ReferenceGroupEntry> {
            if (loadCount > 0) {
                add(
                    ReferenceGroupEntry(stringResource(Res.string.main_overline_training_load), loadCount) {
                        model.weekly_report.byGuidancePriority().forEach { g ->
                            ReferenceItem {
                                ReferenceEvidenceCard(g.summary, g.evidence, why = g.why, glossary = g.glossary, spokenSubject = g.subject)
                            }
                        }
                        model.training_load?.let { tl ->
                            ReferenceItem {
                                ReferenceEvidenceCard(
                                    tl.summary, tl.evidence,
                                    extraDetail = pluralStringResource(Res.plurals.main_training_load_detail, tl.sessions_counted, tl.method, tl.sessions_counted, tl.sessions_skipped),
                                    glossary = tl.glossary, spokenSubject = tl.subject,
                                )
                            }
                        }
                        model.srpe_load?.let { sl ->
                            ReferenceItem {
                                ReferenceEvidenceCard(sl.summary, sl.evidence, extraDetail = sl.method.ifBlank { null }, glossary = sl.glossary, spokenSubject = sl.subject)
                            }
                        }
                    },
                )
            }
            otherGuidance.groupBy { it.section }.forEach { (section, rows) ->
                add(
                    ReferenceGroupEntry(section, rows.size) {
                        rows.byGuidancePriority().forEach {
                            ReferenceItem {
                                ReferenceEvidenceCard(it.summary, it.evidence, why = it.why, glossary = it.glossary, spokenSubject = it.subject)
                            }
                        }
                    },
                )
            }
            if (model.reference.isNotEmpty()) {
                add(
                    ReferenceGroupEntry(stringResource(Res.string.main_section_reference), model.reference.size) {
                        model.reference.groupBy { it.section }.values.forEach { rows ->
                            rows.byGuidancePriority().forEach {
                                ReferenceItem { Box(Modifier.padding(Space.Card.dp)) { ReferenceRow(it) } }
                            }
                        }
                    },
                )
            }
        }
        PlainCard(contentPadding = PaddingValues(0.dp), spacing = 0.dp) {
            groups.forEachIndexed { i, g ->
                if (i > 0) HorizontalDivider(thickness = 1.dp, color = Hairline)
                ReferenceGroup(g.title, g.count, g.cards)
            }
        }
    }
}

@Composable
private fun ReferenceGroup(title: String, count: Int, cards: @Composable () -> Unit) {
    var open by rememberSaveable(title) { mutableStateOf(false) }
    val state = stringResource(if (open) Res.string.ui_cd_expanded else Res.string.ui_cd_collapsed)
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = LargeControlHeight)
            .clickable(role = Role.Button) { open = !open }
            .semantics(mergeDescendants = true) { stateDescription = state }
            .padding(horizontal = Space.Card.dp, vertical = Space.Md.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.Card.dp),
    ) {
        Row(
            modifier = Modifier.weight(1f),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
        ) {
            Text(
                title,
                color = OnBgBody,
                style = Type.BodyBold,
                modifier = Modifier.weight(1f, fill = false),
            )
            Text(
                "$count",
                color = OnBgBody,
                style = Type.Chip.merge(TabularFigures),
                modifier = Modifier
                    .clip(ChoiceCellShape)
                    .background(ChoiceCellFill)
                    .padding(horizontal = Space.Md.dp, vertical = Space.Xs.dp),
            )
        }
        RowChevron(open)
    }
    if (open) cards()
}

@Composable
private fun ReferenceItem(content: @Composable () -> Unit) {
    RowHairline()
    content()
}

/** An [EvidenceCard] as a row of the Reference library's card: regular summary
 *  weight whatever its length (a SAFETY card stays bold), and no inner danger
 *  outline (the SAFETY chip carries it), so the list reads as one card. */
@Composable
private fun ReferenceEvidenceCard(
    summary: String,
    evidence: EvidenceTagView,
    extraDetail: String? = null,
    why: WhyView? = null,
    glossary: List<GlossarySpan> = emptyList(),
    spokenSubject: String? = null,
) = EvidenceCard(
    summary, evidence,
    border = Color.Transparent,
    extraDetail = extraDetail,
    why = why,
    proseSummary = true,
    glossary = glossary,
    spokenSubject = spokenSubject,
)

/**
 * The road-race distances the predictor offers: the setup's race chips minus the
 * mile. The core runs the Riegel + Daniels equivalency on the raw distance
 * ([RaceDistance.km] × 1000 m, sent in [Event.PredictRace]), so these encode no
 * coaching logic.
 */
private val RacePresets = RaceDistance.entries - RaceDistance.Mile

/** A preset's segment label: the half marathon reads "Half" to fit the row. */
private val RaceDistance.presetLabelRes: StringResource
    get() = if (this == RaceDistance.Half) Res.string.log_race_half else labelRes

private enum class RaceField { Minutes, Seconds, Volume }

/** The race form's starter values while no query is echoed. */
private const val RaceMinutesSeed = "22"
private const val WeeklyVolumeSeedKm = "40"
private const val WeeklyVolumeSeedMi = "25"
private const val WeeksSinceRaceSeed = 4

/** Race-form bounds: the recent race's whole minutes, the weekly volume (km;
 *  mi converts it exactly) and the weeks-since-race scale. */
private val RaceMinutesRange = 1.0..599.0
private const val WeeklyVolumeMaxKm = 250.0
private val WeeksSinceRaceScale = (1..52).toList()

/** The HR-zone form's starter age, and its resting-HR scale (bpm) with the
 *  value it opens on when no resting HR is known. */
private const val HrZonesAgeSeed = "30"
private val HrZonesRhrScale = (RESTING_HR_RANGE.start.toInt()..RESTING_HR_RANGE.endInclusive.toInt()).toList()
private const val HrZonesRhrSeedBpm = 60

/**
 * Split a race time (in seconds) into "MM"/"SS" seed buffers for the form.
 * Rounds to whole seconds FIRST, then carries into the minute, so a fractional
 * echo like 22:59.7 rehydrates as 23:00 instead of an invalid "60"-second buffer.
 */
internal fun raceTimeSeed(recentTimeSec: Double): Pair<String, String> {
    val total = recentTimeSec.roundOrZero()
    return (total / 60).toString() to (total % 60).toString()
}

/**
 * Goal-race finish predictor. Takes a recent race result (distance + time) and a
 * target distance, and emits [Event.PredictRace]; the core answers with a
 * two-method (Riegel + Daniels) evidence-graded estimate rendered from
 * `model.race_prediction`. Weekly volume tunes the Riegel fatigue exponent, so a
 * high-mileage runner's marathon extrapolation fades less than a novice's. The
 * shared keypad shows only while [keypadOpen] (a well was tapped); a step chip,
 * a segment or a switch folds it, since none of them needs it.
 */
@Composable
fun RacePredictorForm(
    initial: RacePredictionView?,
    keypadOpen: Boolean,
    onKeypad: (Boolean) -> Unit,
    onPredict: (Event.PredictRace) -> Unit,
) {
    // Seed from the core's echoed query (form rehydration after a cold start /
    // log replay) when present; otherwise the hardcoded starter defaults. The
    // rememberSaveable initial lambda runs once: a warm reopen restores the
    // user's own last edits, a cold start falls back here to the replayed query.
    fun presetFor(meters: Double, fallback: RaceDistance) =
        RaceDistance.of(meters / 1000.0)?.takeIf { it != RaceDistance.Mile } ?: fallback
    var recent by rememberSaveable {
        mutableStateOf(initial?.let { presetFor(it.recent_distance_m, RaceDistance.FiveK) } ?: RaceDistance.FiveK)
    }
    var goal by rememberSaveable {
        mutableStateOf(initial?.let { presetFor(it.goal_distance_m, RaceDistance.TenK) } ?: RaceDistance.TenK)
    }
    // Keypad-driven numeric entry, the same primitives as Log set: an in-form
    // keypad and scale rows, never a −/+ stepper as the primary control.
    val seed = initial?.let { raceTimeSeed(it.recent_time_sec) }
    val seedMin = seed?.first ?: RaceMinutesSeed
    val seedSec = seed?.second ?: "0"
    // Weekly volume honors the mi override symmetric with the lb weight field: entry
    // in the display unit, canonical km on the wire (converted at save via miToKmWire).
    val unit = rememberDistanceUnit()
    val seedKm = initial?.let {
        formatFixed(if (unit == DistanceUnit.Mi) kmToMi(it.weekly_km) else it.weekly_km, 0)
    } ?: (if (unit == DistanceUnit.Mi) WeeklyVolumeSeedMi else WeeklyVolumeSeedKm)
    var minText by rememberSaveable { mutableStateOf(seedMin) }
    var secText by rememberSaveable { mutableStateOf(seedSec) }
    var kmText by rememberSaveable { mutableStateOf(seedKm) }
    var active by rememberSaveable { mutableStateOf(RaceField.Minutes) }
    var fresh by rememberSaveable { mutableStateOf(true) }
    // Weeks since that race, the core flags a stale result (running-041). Off
    // by default (a fresh result); a toggle reveals the scale, seeded from the echo.
    var raceStale by rememberSaveable { mutableStateOf((initial?.weeks_since_race ?: 0) > 0) }
    var weeksSinceRace by rememberSaveable { mutableStateOf(initial?.weeks_since_race ?: WeeksSinceRaceSeed) }

    val minParsed = minText.toDoubleOrNull()
    val secParsed = secText.toDoubleOrNull()
    val kmParsed = kmText.toDoubleOrNull()
    val minValid = minParsed != null && minParsed in RaceMinutesRange
    val secValid = secParsed != null && secParsed in 0.0..59.0
    // Bound in the displayed unit: the mi ceiling is the EXACT converted km limit,
    // so a wire-valid high-mileage volume stays editable in mi.
    val kmMax = if (unit == DistanceUnit.Mi) kmToMi(WeeklyVolumeMaxKm) else WeeklyVolumeMaxKm
    val presetLabels = RacePresets.associateWith { stringResource(it.presetLabelRes) }
    val kmValid = kmParsed != null && kmParsed in 0.0..kmMax

    fun buffer() = when (active) {
        RaceField.Minutes -> minText
        RaceField.Seconds -> secText
        RaceField.Volume -> kmText
    }
    fun setBuffer(v: String) = when (active) {
        RaceField.Minutes -> minText = v
        RaceField.Seconds -> secText = v
        RaceField.Volume -> kmText = v
    }

    FormCard {
        RacePresetField(stringResource(Res.string.log_field_recent_race), recent, presetLabels) {
            recent = it
            onKeypad(false)
        }
        KeypadValueField(
            stringResource(Res.string.log_field_recent_min), stringResource(Res.string.log_unit_min), minText,
            active = keypadOpen && active == RaceField.Minutes,
            invalid = !minValid,
            min = RaceMinutesRange.start, max = RaceMinutesRange.endInclusive, decimals = 0,
            adjustments = listOf(-1.0, 1.0, 5.0),
            onActivate = { active = RaceField.Minutes; fresh = true; onKeypad(true) },
            onText = { minText = it; fresh = false; onKeypad(false) },
        )
        KeypadValueField(
            stringResource(Res.string.log_field_recent_sec), stringResource(Res.string.log_unit_sec), secText,
            active = keypadOpen && active == RaceField.Seconds,
            invalid = !secValid,
            min = 0.0, max = 59.0, decimals = 0,
            adjustments = listOf(-5.0, 5.0),
            onActivate = { active = RaceField.Seconds; fresh = true; onKeypad(true) },
            onText = { secText = it; fresh = false; onKeypad(false) },
        )
        RacePresetField(stringResource(Res.string.log_field_goal_race), goal, presetLabels) {
            goal = it
            onKeypad(false)
        }
        KeypadValueField(
            stringResource(Res.string.log_field_weekly_volume), unit.distanceLabel, kmText,
            active = keypadOpen && active == RaceField.Volume,
            invalid = !kmValid,
            min = 0.0, max = kmMax, decimals = 0,
            adjustments = listOf(-5.0, 5.0),
            onActivate = { active = RaceField.Volume; fresh = true; onKeypad(true) },
            onText = { kmText = it; fresh = false; onKeypad(false) },
        )
        // Freshness of the input race (running-041). Off → sent as null (a
        // recent result); on reveals a weeks scale so the core can flag staleness.
        LabeledSwitchRow(stringResource(Res.string.log_switch_race_stale), raceStale) { raceStale = it; onKeypad(false) }
        if (raceStale) {
            FieldLabel(stringResource(Res.string.log_field_weeks_since_race), "$weeksSinceRace")
            ScrollableScaleRow(WeeksSinceRaceScale, weeksSinceRace, { "$it" }, label = stringResource(Res.string.log_field_weeks_since_race)) {
                weeksSinceRace = it
                onKeypad(false)
            }
        }
        KeypadPanel(keypadOpen) {
            NumericKeypad(
                buffer(),
                fresh,
                { setBuffer(it); fresh = false },
                allowDecimal = active == RaceField.Volume,
                onDone = { onKeypad(false) },
            )
        }
        PrimaryButton(
            stringResource(Res.string.log_btn_predict),
            {
                if (minParsed != null && secParsed != null && kmParsed != null) {
                    onPredict(
                        Event.PredictRace(
                            recentDistanceM = recent.km * 1000.0,
                            recentTimeSec = minParsed * 60.0 + secParsed,
                            goalDistanceM = goal.km * 1000.0,
                            weeklyKm = if (unit == DistanceUnit.Mi) miToKmWire(kmParsed) else kmParsed,
                            weeksSinceRace = if (raceStale) weeksSinceRace else null,
                        )
                    )
                }
            },
            Modifier.fillMaxWidth(),
            enabled = minValid && secValid && kmValid,
        )
    }
}

/** A race-distance pick: its [FieldLabel] over the segmented row, the label
 *  hugging the control the way a keypad field's label hugs its well. */
@Composable
private fun RacePresetField(label: String, current: RaceDistance, labels: Map<RaceDistance, String>, onSelect: (RaceDistance) -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
        FieldLabel(label)
        SegmentedEnumRow(null, RacePresets, current, display = { labels.getValue(it) }, onSelect = onSelect)
    }
}

private val AccumulationWeeks = 2..8
private const val AccumulationWeeksSeed = 4

/**
 * Hypertrophy accumulation-block volume planner. Picks a target muscle and a number
 * of accumulation weeks, and emits [Event.PlanHypertrophyMeso]; the core answers with
 * an evidence-graded per-week plan (volume landmarks, MEV→MRV set ramp, RIR schedule,
 * peak frequency) rendered from `model.hypertrophy_plan`. All numbers come from the
 * knowledge-base hypertrophy tables, this form only picks the inputs.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun HypertrophyPlannerForm(initial: HypertrophyInputView?, onPlan: (Event.PlanHypertrophyMeso) -> Unit) {
    // Rehydrate from the core's echoed query on a cold start; else defaults.
    var muscle by rememberSaveable {
        mutableStateOf(
            initial?.let { HypertrophyMuscle.of(it.muscle) } ?: HypertrophyMuscle.Chest,
        )
    }
    var weeks by rememberSaveable {
        mutableStateOf(initial?.weeks?.coerceIn(AccumulationWeeks.first, AccumulationWeeks.last) ?: AccumulationWeeksSeed)
    }
    // hypertrophy-008 next-meso decision inputs, rehydrated from the core's echoed
    // query. When BOTH are set the core adds +2 sets to the next block's landmarks.
    var notGrowing by rememberSaveable { mutableStateOf(initial?.not_growing ?: false) }
    var recoveringEasily by rememberSaveable { mutableStateOf(initial?.recovering_easily ?: false) }
    val muscleLabels = HypertrophyMuscle.entries.associateWith { stringResource(it.labelRes) }

    FormCard {
        Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
            FieldLabel(stringResource(Res.string.log_field_muscle))
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(ChoiceChipGap),
                verticalArrangement = Arrangement.spacedBy(ChoiceChipGap),
                modifier = Modifier.selectableGroup(),
            ) {
                HypertrophyMuscle.entries.forEach { m ->
                    ChoiceChip(muscleLabels.getValue(m), selected = m == muscle, onClick = { muscle = m })
                }
            }
        }
        // Whole scale visible, one tap picks, never a −/+ stepper.
        FieldLabel(stringResource(Res.string.log_field_accumulation_weeks), "$weeks")
        ChoiceScaleRow(AccumulationWeeks.toList(), weeks, { "$it" }, label = stringResource(Res.string.log_field_accumulation_weeks)) { weeks = it }
        LabeledSwitchRow(stringResource(Res.string.log_switch_not_growing), notGrowing) { notGrowing = it }
        LabeledSwitchRow(stringResource(Res.string.log_switch_recovering_easily), recoveringEasily) { recoveringEasily = it }
        PrimaryButton(
            stringResource(Res.string.log_btn_plan_volume),
            {
                onPlan(
                    Event.PlanHypertrophyMeso(
                        muscle = muscle.wire,
                        weeks = weeks,
                        notGrowing = notGrowing,
                        recoveringEasily = recoveringEasily,
                    )
                )
            },
            Modifier.fillMaxWidth(),
        )
    }
}

/**
 * Absolute daily protein target tool. Picks a bodyweight and one or both graded
 * goal contexts (masters, caloric deficit) and emits [Event.ComputeProtein];
 * the core answers with an evidence-graded g/day range (bodyweight × each graded
 * g/kg bound) rendered from `model.protein_targets`. No general/default number
 * exists, with neither toggle set the core returns nothing, so this form only
 * picks the inputs. With [mastersAvailable] false (a known age below
 * [CoreConstants.MASTERS_AGE_YEARS]) the
 * masters toggle is gone and never sent, leaving the deficit context alone. The
 * keypad shows only while [keypadOpen] (the well was tapped).
 */
@Composable
fun ProteinForm(
    initial: ProteinInputView?,
    keypadOpen: Boolean,
    onKeypad: (Boolean) -> Unit,
    profileBodyweightKg: Double?,
    // Display unit for the bodyweight field. The core computes from kg; lb is a
    // display edge (prefill kg→lb, Compute lb→kg).
    weightUnit: WeightUnit,
    mastersAvailable: Boolean,
    onCompute: (Double, Boolean, Boolean) -> Unit,
) {
    // Keypad-driven bodyweight (Log-set primitives, never a −/+ stepper). The
    // buffer text is exactly what is parsed at submit (display-committed).
    // Seed order: the core's echoed query (last calc) first, then the
    // profile's consolidated person bodyweight; so a user who set bodyweight in
    // Profile never re-types it here (override still allowed). Absent both the
    // field starts empty: no stand-in weight is ever computed or saved.
    val seedKg = initial?.bodyweight_kg ?: profileBodyweightKg
    var bwText by rememberSaveable {
        mutableStateOf(seedKg?.let { bodyweightBufferText(it, weightUnit) } ?: "")
    }
    // `fresh` is the keypad replace-buffer flag: a first digit REPLACES the seed
    // instead of appending, and it re-arms on re-activate (tapping the field). It must
    // NOT gate which value Compute emits — re-activating after typing would otherwise
    // send the seed while the field shows the typed value. `untouchedSinceOpen` tracks
    // that separately: true only until the FIRST real edit (digit/backspace/chip), never
    // re-armed on activate, so Compute keys off it to emit the displayed value once touched.
    var fresh by rememberSaveable { mutableStateOf(true) }
    var untouchedSinceOpen by rememberSaveable { mutableStateOf(true) }
    var masters by rememberSaveable { mutableStateOf(initial?.masters ?: false) }
    var deficit by rememberSaveable { mutableStateOf(initial?.deficit ?: false) }
    val mastersOn = mastersAvailable && masters

    // Bounds/chips in the displayed unit. The bounds are the app-wide bodyweight
    // keypad range (Profile and setup use the same), so a weight accepted there is
    // accepted here.
    // An untouched prefill is wire-valid by construction (`bwComputeKg` emits `seedKg`
    // verbatim), so display rounding must not red it out — gate validity on that path.
    val bwBounds = bodyweightBounds(weightUnit)
    val bwParsed = bwText.toDoubleOrNull()
    val bwValid = (untouchedSinceOpen && seedKg != null) ||
        (bwParsed != null && bwParsed in bwBounds)
    // The kg fed to the core: an untouched prefill emits the original kg verbatim
    // (no kg→lb→kg drift); a touched buffer submits exactly what is displayed,
    // converting lb→kg at the same save boundary as the Profile bodyweight field.
    val bwComputeKg: Double? = if (untouchedSinceOpen && seedKg != null) seedKg
        else bwParsed?.let { bodyweightDisplayToKg(it, weightUnit) }

    FormCard {
        KeypadValueField(
            stringResource(Res.string.log_field_bodyweight), weightSuffix(weightUnit), bwText,
            active = keypadOpen,
            // An empty field is an invitation, not an error: the hint asks for
            // the weight and Compute stays off until one is typed.
            invalid = bwText.isNotBlank() && !bwValid,
            min = bwBounds.start, max = bwBounds.endInclusive, decimals = 1,
            adjustments = plateSteps(weightUnit),
            onActivate = { fresh = true; onKeypad(true) },
            onText = { bwText = it; fresh = false; untouchedSinceOpen = false; onKeypad(false) },
            hint = if (bwText.isBlank()) stringResource(Res.string.log_protein_weight_hint) else null,
        )
        if (initial == null && profileBodyweightKg != null) {
            Text(
                stringResource(Res.string.log_prefill_profile),
                color = OnBgMuted,
                style = Type.Caption,
            )
        }
        if (mastersAvailable) {
            LabeledSwitchRow(stringResource(Res.string.log_switch_masters, mastersAgeLabel()), masters) { masters = it; onKeypad(false) }
        }
        LabeledSwitchRow(stringResource(Res.string.log_switch_deficit), deficit) { deficit = it; onKeypad(false) }
        if (!mastersOn && !deficit) {
            // No general/default protein figure is evidence-graded, so the core
            // returns nothing until a graded context is chosen. Without this the
            // button would look actionable yet render no result on tap: say why
            // and gate the button, matching the other forms' validity idiom.
            Text(
                if (mastersAvailable) {
                    stringResource(Res.string.log_protein_no_context)
                } else {
                    stringResource(Res.string.log_protein_deficit_only, mastersAgeLabel())
                },
                color = OnBgMuted,
                style = Type.Caption,
            )
        }
        KeypadPanel(keypadOpen) {
            NumericKeypad(bwText, fresh, { bwText = it; fresh = false; untouchedSinceOpen = false }, onDone = { onKeypad(false) })
        }
        PrimaryButton(
            stringResource(Res.string.log_btn_compute_protein),
            { bwComputeKg?.let { onCompute(it, mastersOn, deficit) } },
            Modifier.fillMaxWidth(),
            enabled = bwValid && (mastersOn || deficit),
        )
    }
}

/**
 * On-demand heart-rate-zone calculator. The user enters an age and emits
 * [Event.ComputeHrZones]; the core answers with an evidence-graded HRmax estimate
 * (Tanaka) plus the five Daniels %HRmax training bands as absolute bpm ranges,
 * rendered from `model.hr_zones`. All numbers come from the core (load.rs /
 * running.rs, RUN-VDOT-001), this form only picks the inputs (age, optional
 * resting HR). The keypad shows only while [keypadOpen] (the age well was tapped).
 */
@Composable
fun HrZonesForm(
    initial: HrZoneInputView?,
    keypadOpen: Boolean,
    onKeypad: (Boolean) -> Unit,
    profileAgeYears: Double?,
    profileRestingHrBpm: Double?,
    // (age, restingHrBpm?), a non-null RHR lets the core run Karvonen
    // heart-rate-reserve zones (running-005) instead of plain %HRmax.
    onCompute: (Double, Double?) -> Unit,
) {
    // Keypad-driven age entry (Log-set primitives, never a −/+ stepper).
    // Seed order: the echoed query first, then the profile's consolidated
    // age, then the plain default; enter your age once on Profile, prefilled here.
    var ageText by rememberSaveable {
        val seed = initial?.age_years ?: profileAgeYears
        mutableStateOf(seed?.let { formatFixed(it, 0) } ?: HrZonesAgeSeed)
    }
    var fresh by rememberSaveable { mutableStateOf(true) }
    // Optional resting HR. Seeded from the echoed query, then the profile.
    // Karvonen zones are more personal than age-only %HRmax, so default it ON
    // whenever a resting HR is known (from the echo or the profile).
    val seedRhr = initial?.resting_hr_bpm ?: profileRestingHrBpm
    var useRhr by rememberSaveable { mutableStateOf(seedRhr != null) }
    var rhr by rememberSaveable { mutableStateOf(seedRhr?.roundToInt() ?: HrZonesRhrSeedBpm) }

    val ageParsed = ageText.toDoubleOrNull()
    val ageValid = ageParsed != null && ageParsed in CoreConstants.TANAKA_AGE_MIN_YEARS..CoreConstants.TANAKA_AGE_MAX_YEARS

    FormCard {
        KeypadValueField(
            stringResource(Res.string.log_field_age), stringResource(Res.string.log_unit_years), ageText,
            active = keypadOpen,
            invalid = !ageValid,
            min = CoreConstants.TANAKA_AGE_MIN_YEARS, max = CoreConstants.TANAKA_AGE_MAX_YEARS, decimals = 0,
            adjustments = listOf(-1.0, 1.0),
            onActivate = { fresh = true; onKeypad(true) },
            onText = { ageText = it; fresh = false; onKeypad(false) },
        )
        if (initial == null && (profileAgeYears != null || profileRestingHrBpm != null)) {
            Text(
                stringResource(Res.string.log_prefill_profile),
                color = OnBgMuted,
                style = Type.Caption,
            )
        }
        // Resting HR unlocks Karvonen (heart-rate-reserve) zones. Off → the
        // core falls back to age-only %HRmax, so this stays optional.
        LabeledSwitchRow(stringResource(Res.string.log_switch_use_rhr), useRhr) { useRhr = it; onKeypad(false) }
        if (useRhr) {
            FieldLabel(stringResource(Res.string.log_field_resting_hr), stringResource(Res.string.log_value_bpm, rhr))
            ScrollableScaleRow(HrZonesRhrScale, rhr, { "$it" }, label = stringResource(Res.string.log_field_resting_hr)) {
                rhr = it
                onKeypad(false)
            }
        }
        KeypadPanel(keypadOpen) {
            NumericKeypad(ageText, fresh, { ageText = it; fresh = false }, allowDecimal = false, onDone = { onKeypad(false) })
        }
        PrimaryButton(
            stringResource(Res.string.log_btn_compute_hr),
            { ageParsed?.let { onCompute(it, if (useRhr) rhr.toDouble() else null) } },
            Modifier.fillMaxWidth(),
            enabled = ageValid,
        )
    }
}
