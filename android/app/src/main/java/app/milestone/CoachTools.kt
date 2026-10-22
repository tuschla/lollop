package app.milestone

import androidx.activity.compose.BackHandler
import androidx.annotation.StringRes
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
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
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
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInParent
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import java.util.Locale

/** The four on-demand Coach calculators, launched from the Profile calculator rows. */
internal enum class CoachTool { Race, Volume, Protein, HrZones }

/**
 * The calculators as one card of modality-gated rows (Profile → CALCULATORS).
 * Race predictor + HR zones are running tools; Volume planner is a lifting tool;
 * Protein shows for every adult (a caloric deficit can apply at any adult age), but
 * under 65 its row says it only computes in a deficit ([proteinDeficitOnly]).
 * Under 18 neither Volume nor Protein shows: no autonomous load prescription
 * for youth (safety-011), and the KB's protein targets are adult ones ([underEighteen]).
 * A not-yet-set-up user (profile == null) sees them all. Each row names its
 * tool, then the computed value once there is one, else what a tap does;
 * tapping a row hands its [CoachTool] to [onOpen], whose caller hosts the
 * [CoachToolSheet].
 */
@Composable
internal fun CalculatorsCard(model: ViewModel, onOpen: (CoachTool) -> Unit) {
    val calcAll = model.profile == null
    val minor = model.underEighteen()
    val showRace = calcAll || model.showRunning()
    val showVolume = calcAll || (model.showLifting() && !minor)
    CoachToolList {
        if (showRace) {
            CoachToolRow(
                stringResource(R.string.main_tool_race_title),
                cta = stringResource(R.string.main_tool_race_cta),
                value = model.race_prediction?.let { listOf(it.predicted, it.goal_label).filter(String::isNotBlank).joinToString(" · ") },
            ) { onOpen(CoachTool.Race) }
        }
        if (showVolume) {
            CoachToolRow(
                stringResource(R.string.main_tool_volume_title),
                cta = stringResource(R.string.main_tool_volume_cta),
                // The last plan's weekly sets ("12→20 sets"), from the core's
                // structured ramp; the block length alone when it shows no ramp.
                value = model.hypertrophy_input?.takeIf { model.hypertrophy_plan.isNotEmpty() }?.let { input ->
                    val ramp = model.hypertrophy_ramp
                    when {
                        ramp.isEmpty() -> stringResource(R.string.main_calc_value_weeks, input.weeks)
                        ramp.first() == ramp.last() -> stringResource(R.string.main_calc_value_sets, ramp.first())
                        else -> stringResource(R.string.main_calc_value_sets_ramp, ramp.first(), ramp.last())
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
                stringResource(R.string.main_tool_protein_title),
                cta = when {
                    model.proteinDeficitOnly() -> stringResource(R.string.main_tool_protein_cta_deficit)
                    model.profile?.bodyweight_kg != null -> stringResource(R.string.main_tool_protein_cta_compute)
                    else -> stringResource(R.string.main_tool_protein_cta_add_bw)
                },
                value = when {
                    proteinGrams != null -> stringResource(R.string.log_value_with_unit, proteinGrams, stringResource(R.string.main_unit_g_per_day))
                    model.protein_targets.isNotEmpty() -> stringResource(R.string.main_value_set)
                    else -> null
                },
            ) { onOpen(CoachTool.Protein) }
        }
        if (showRace) {
            // After compute, the actual HRmax ("187 HRmax bpm"), read from
            // the core's structured hr_max (never recomputed).
            val hrMax = model.hr_max?.bpm?.toInt()
            CoachToolRow(
                stringResource(R.string.main_tool_hr_title),
                cta = if (model.profile?.age_years != null) stringResource(R.string.main_tool_hr_cta_compute) else stringResource(R.string.main_tool_hr_cta_add_age),
                value = when {
                    hrMax != null -> stringResource(R.string.main_calc_value_hrmax, hrMax)
                    model.hr_zones.isNotEmpty() -> stringResource(R.string.main_value_set)
                    else -> null
                },
            ) { onOpen(CoachTool.HrZones) }
        }
    }
}

/** The age the masters protein target is graded for; mirrors the core's
 *  `plan::MASTERS_AGE_YEARS` (MASTERS-001, File 08 indiv-013 "masters (65+)"). */
private const val MastersAgeYears = 65.0

/** True when the profile's age is known and under 65: the KB backs a protein
 *  target only for masters (65+, indiv-013) or in a caloric deficit
 *  (indiv-020), so only the deficit context can give this person a number. An
 *  unknown age leaves both contexts open. */
internal fun ViewModel.proteinDeficitOnly(): Boolean =
    profile?.age_years?.let { it < MastersAgeYears } ?: false

/** True for an under-18 profile (the core's age gate, `health.youth`): Profile
 *  offers no Volume planner or Protein calculator (safety-011: no autonomous load
 *  prescription for youth; the protein targets are graded for adults). */
internal fun ViewModel.underEighteen(): Boolean = profile?.health?.youth == true

/** Whether [CalculatorsCard] has any row: every adult gets Protein, an
 *  under-18 profile only the running tools. */
internal fun ViewModel.hasCalculators(): Boolean = profile == null || !underEighteen() || showRunning()

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
            .border(1.dp, OnBgBody.copy(alpha = 0.06f), shape)
            .padding(vertical = Space.Xs.dp),
        content = rows,
    )
}

/**
 * One calculator row: the tool name, then its computed value once there is one
 * ("45:37 · 10K", "12→20 sets", "147–220 g/day", "HRmax 184": single-line
 * `Type.Body` `OnBgMuted` tnum, end-aligned), else what a tap does ("Set a recent
 * race"), and an accent chevron: the whole ≥48dp row opens the tool's sheet. At
 * a font scale of 1.3 or more the value drops below the name, start-aligned,
 * rather than wrapping in a narrow column.
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

/** Font scale from which a calculator row's value moves under its name. */
private const val StackedValueFontScale = 1.3f

/** How much of a result card (overline + headline + caption) must show when a
 *  calculator sheet opens on a saved result. */
private val ResultAnswerRoom = 168.dp

/**
 * Body of the Coach calculator [ModalBottomSheet]: a title, the tool's FORM, then
 * its one [CalculatorResultCard] (ui-sweep §10). The forms' keypad shows only
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
    val title = when (tool) {
        CoachTool.Race -> stringResource(R.string.main_tool_race_title)
        CoachTool.Volume -> stringResource(R.string.main_tool_volume_title)
        CoachTool.Protein -> stringResource(R.string.main_tool_protein_title)
        CoachTool.HrZones -> stringResource(R.string.main_tool_hr_title)
    }
    // Weight-unit choice for the bodyweight keypad form below.
    val weightUnit = if (model.profile?.weight_lb == true) WeightUnit.Lb else WeightUnit.Kg
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
            .fillMaxWidth()
            // A tap on the bare sheet (not on a control) folds the keypad.
            .pointerInput(keypadOpen) {
                if (keypadOpen) detectTapGestures { keypadOpen = false }
            }
            // The form + result can exceed the sheet's max height.
            .verticalScroll(scroll)
            // The last action clears the system navigation bar.
            .navigationBarsPadding()
            .padding(horizontal = Space.ReadingSheet.dp)
            .padding(bottom = Space.Lg.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Card.dp),
    ) {
        Text(title, color = OnBgBody, style = Type.Title)
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
                    mastersAvailable = !model.proteinDeficitOnly(),
                ) { bodyweight, masters, deficit ->
                    compute(Event.ComputeProtein(bodyweight, masters, deficit))
                }
                // Stable-slot divergence hint: always composed (so the sheet height
                // never jumps), non-empty only when the last-computed bodyweight
                // differs from the profile's, naming the profile value it differs from.
                val profileBw = model.profile?.bodyweight_kg
                val formBw = model.protein_input?.bodyweight_kg
                val divergence = if (profileBw != null && formBw != null &&
                    Math.abs(formBw - profileBw) >= 0.05
                ) {
                    stringResource(R.string.main_differs_from_profile, weightFigure(profileBw, weightUnit), weightSuffix(weightUnit))
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
        // The tool's RESULT: one card for the answer and its evidence.
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

/** The guidance rows that belong in the Reference library (everything the core
 *  emits under `guidance` that is NOT the one Profile-context block). Shared by
 *  the Profile "Evidence & references" screen. */
private fun referenceGuidance(model: ViewModel): List<GuidanceView> =
    model.guidance.filterNot { it.section == "Profile" }

/** How many cards the Reference library holds (guidance groups + reference defaults). */
internal fun referenceLibraryCount(model: ViewModel): Int =
    referenceGuidance(model).size + model.reference.size + model.weekly_report.size +
        (if (model.training_load != null) 1 else 0) + (if (model.srpe_load != null) 1 else 0)

/**
 * The Reference library, a Profile-opened sheet (ui-sweep §12): every
 * evidence-graded programming-guidance group (Strength/Hypertrophy/Running/…),
 * the Training load group (the core-owned running-volume rows + the CTL/ATL/TSB
 * card) and the Reference source list, as one carded list of 56dp group rows
 * (name, count pill, turning chevron). An opened group's cards render below its
 * row inside the same card, between hairlines, each with its own "?" (grade chips
 * stay behind it). Every summary here is regular weight except a SAFETY card. No
 * coaching logic; it only lays out core-emitted cards.
 */
@Composable
internal fun ReferenceLibrarySheet(model: ViewModel) {
    val otherGuidance = referenceGuidance(model)
    // The session-effort (sRPE) weekly load sits beside the heart-rate load as
    // its own card: a separate series, never summed into it.
    val loadCount = model.weekly_report.size + (if (model.training_load != null) 1 else 0) +
        (if (model.srpe_load != null) 1 else 0)
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .verticalScroll(rememberScrollState())
            .navigationBarsPadding()
            .padding(horizontal = Space.ReadingSheet.dp)
            .padding(bottom = Space.Lg.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Card.dp),
    ) {
        Text(stringResource(R.string.main_evidence_references_title), color = OnBgBody, style = Type.Title)
        Text(
            stringResource(R.string.main_evidence_references_body),
            color = OnBgMuted,
            style = Type.Body,
        )
        if (referenceLibraryCount(model) == 0) {
            Text(
                stringResource(R.string.main_reference_empty),
                color = OnBgFaint,
                style = Type.Caption,
            )
            return@Column
        }
        PlainCard(contentPadding = PaddingValues(0.dp), spacing = 0.dp) {
            var first = true
            @Composable
            fun group(title: String, count: Int, cards: @Composable () -> Unit) {
                if (!first) HorizontalDivider(thickness = 1.dp, color = Hairline)
                first = false
                ReferenceGroup(title, count, cards)
            }
            if (loadCount > 0) {
                group(stringResource(R.string.main_overline_training_load), loadCount) {
                    model.weekly_report.byGuidancePriority().forEach { g ->
                        ReferenceItem {
                            ReferenceEvidenceCard(guidanceSummary(g), g.grade, g.citation, g.confidence, g.safety_critical, g.contested, why = g.why)
                        }
                    }
                    model.training_load?.let { tl ->
                        ReferenceItem {
                            ReferenceEvidenceCard(
                                tl.summary, tl.grade, tl.citation, tl.confidence, tl.safety_critical, tl.contested,
                                extraDetail = stringResource(R.string.main_training_load_detail, tl.method, tl.sessions_counted, tl.sessions_skipped),
                            )
                        }
                    }
                    model.srpe_load?.let { sl ->
                        ReferenceItem {
                            ReferenceEvidenceCard(sl.summary, sl.grade, sl.citation, sl.confidence, false, false, extraDetail = sl.method.ifBlank { null })
                        }
                    }
                }
            }
            otherGuidance.groupBy { it.section }.forEach { (section, rows) ->
                group(section, rows.size) {
                    rows.byGuidancePriority().forEach {
                        ReferenceItem {
                            ReferenceEvidenceCard(guidanceSummary(it), it.grade, it.citation, it.confidence, it.safety_critical, it.contested, why = it.why)
                        }
                    }
                }
            }
            if (model.reference.isNotEmpty()) {
                group(stringResource(R.string.main_section_reference), model.reference.size) {
                    model.reference.groupBy { it.section }.values.forEach { rows ->
                        rows.byGuidancePriority().forEach {
                            ReferenceItem { Box(Modifier.padding(Space.Card.dp)) { ReferenceRow(it) } }
                        }
                    }
                }
            }
        }
    }
}

/**
 * One group row of the Reference library (default collapsed): the group name in
 * Bold sentence case, its count in a pill, and a chevron that turns when open.
 * The whole ≥56dp row toggles it; TalkBack hears the state. The group's cards
 * mount below it only while open.
 */
@Composable
private fun ReferenceGroup(title: String, count: Int, cards: @Composable () -> Unit) {
    var open by rememberSaveable(title) { mutableStateOf(false) }
    val state = stringResource(if (open) R.string.ui_cd_expanded else R.string.ui_cd_collapsed)
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
                style = Type.Body.copy(fontWeight = FontWeight.Bold),
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

/** One item of an open group: a hairline, then the item itself. */
@Composable
private fun ReferenceItem(content: @Composable () -> Unit) {
    HorizontalDivider(thickness = 1.dp, color = OnBgBody.copy(alpha = HairlineAlpha.Row))
    content()
}

/** An [EvidenceCard] as a row of the Reference library's card: regular summary
 *  weight whatever its length (a SAFETY card stays bold), and no inner danger
 *  outline (the SAFETY chip carries it), so the list reads as one card. */
@Composable
private fun ReferenceEvidenceCard(
    summary: String,
    grade: String,
    citation: String,
    confidence: Float,
    safetyCritical: Boolean,
    contested: Boolean,
    extraDetail: String? = null,
    why: WhyView? = null,
) = EvidenceCard(
    summary, grade, citation, confidence, safetyCritical, contested,
    border = Color.Transparent,
    extraDetail = extraDetail,
    why = why,
    proseSummary = true,
)

/**
 * Standard road-race distances the predictor works over, with their exact metre
 * length. Display-only labels; the wire value the core reads is [meters], fed to
 * [Event.PredictRace], the core runs the Riegel + Daniels equivalency on the raw
 * distances, so these presets never encode coaching logic.
 */
private enum class RacePreset(val meters: Double, @StringRes val labelRes: Int) {
    FiveK(5000.0, R.string.log_race_5k),
    TenK(10000.0, R.string.log_race_10k),
    Half(21097.5, R.string.log_race_half),
    Full(42195.0, R.string.log_race_marathon),
}

/** Which numeric field the race form's shared keypad currently edits. */
private enum class RaceField { Minutes, Seconds, Volume }

/** The race form's starter values while no query is echoed: a 22:00 recent
 *  race, a weekly volume in the display unit, and the weeks-since-race scale's
 *  opening pick. */
private const val RaceMinutesSeed = "22"
private const val WeeklyVolumeSeedKm = "40"
private const val WeeklyVolumeSeedMi = "25"
private const val WeeksSinceRaceSeed = 4

/** Race-form bounds: the recent race's whole minutes, the weekly volume (km;
 *  mi converts it exactly) and the weeks-since-race scale. */
private val RaceMinutesRange = 1.0..359.0
private const val WeeklyVolumeMaxKm = 250.0
private val WeeksSinceRaceScale = (1..52).toList()

/** The HR-zone form's starter age, and its resting-HR scale (bpm) with the
 *  value it opens on when no resting HR is known. */
private const val HrZonesAgeSeed = "30"
private val HrZonesRhrScale = (35..90).toList()
private const val HrZonesRhrSeedBpm = 60

/**
 * Split a race time (in seconds) into "MM"/"SS" seed buffers for the form.
 * Rounds to whole seconds FIRST, then carries into the minute, so a fractional
 * echo like 22:59.7 rehydrates as 23:00 instead of an invalid "60"-second buffer.
 */
internal fun raceTimeSeed(recentTimeSec: Double): Pair<String, String> {
    val total = Math.round(recentTimeSec)
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
    fun presetFor(meters: Double, fallback: RacePreset) =
        RacePreset.entries.firstOrNull { kotlin.math.abs(it.meters - meters) < 1.0 } ?: fallback
    var recent by rememberSaveable {
        mutableStateOf(initial?.let { presetFor(it.recent_distance_m, RacePreset.FiveK) } ?: RacePreset.FiveK)
    }
    var goal by rememberSaveable {
        mutableStateOf(initial?.let { presetFor(it.goal_distance_m, RacePreset.TenK) } ?: RacePreset.TenK)
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
        String.format(Locale.US, "%.0f", if (unit == DistanceUnit.Mi) kmToMi(it.weekly_km) else it.weekly_km)
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

    val minParsed = parseKeypadNumber(minText)
    val secParsed = parseKeypadNumber(secText)
    val kmParsed = parseKeypadNumber(kmText)
    val minValid = minParsed != null && minParsed in RaceMinutesRange
    val secValid = secParsed != null && secParsed in 0.0..59.0
    // Bound in the displayed unit: the mi ceiling is the EXACT converted km limit
    // (155.34 mi) so a wire-valid high-mileage volume stays editable in mi.
    val kmMax = if (unit == DistanceUnit.Mi) kmToMi(WeeklyVolumeMaxKm) else WeeklyVolumeMaxKm
    val presetLabels = RacePreset.entries.associateWith { stringResource(it.labelRes) }
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
        RacePresetField(stringResource(R.string.log_field_recent_race), recent, presetLabels) {
            recent = it
            onKeypad(false)
        }
        KeypadValueField(
            stringResource(R.string.log_field_recent_min), stringResource(R.string.log_unit_min), minText,
            active = keypadOpen && active == RaceField.Minutes,
            invalid = !minValid,
            min = RaceMinutesRange.start, max = RaceMinutesRange.endInclusive, format = "%.0f",
            adjustments = listOf(-1.0, 1.0, 5.0),
            onActivate = { active = RaceField.Minutes; fresh = true; onKeypad(true) },
            onText = { minText = it; fresh = false; onKeypad(false) },
        )
        KeypadValueField(
            stringResource(R.string.log_field_recent_sec), stringResource(R.string.log_unit_sec), secText,
            active = keypadOpen && active == RaceField.Seconds,
            invalid = !secValid,
            min = 0.0, max = 59.0, format = "%.0f",
            adjustments = listOf(-5.0, 5.0),
            onActivate = { active = RaceField.Seconds; fresh = true; onKeypad(true) },
            onText = { secText = it; fresh = false; onKeypad(false) },
        )
        RacePresetField(stringResource(R.string.log_field_goal_race), goal, presetLabels) {
            goal = it
            onKeypad(false)
        }
        KeypadValueField(
            stringResource(R.string.log_field_weekly_volume), unit.distanceLabel, kmText,
            active = keypadOpen && active == RaceField.Volume,
            invalid = !kmValid,
            min = 0.0, max = kmMax, format = "%.0f",
            adjustments = listOf(-5.0, 5.0),
            onActivate = { active = RaceField.Volume; fresh = true; onKeypad(true) },
            onText = { kmText = it; fresh = false; onKeypad(false) },
        )
        // Freshness of the input race (running-041). Off → sent as null (a
        // recent result); on reveals a weeks scale so the core can flag staleness.
        SwitchRow(stringResource(R.string.log_switch_race_stale), raceStale) { raceStale = it; onKeypad(false) }
        if (raceStale) {
            FieldLabel(stringResource(R.string.log_field_weeks_since_race), "$weeksSinceRace")
            ScrollableScaleRow(WeeksSinceRaceScale, weeksSinceRace, { "$it" }, label = stringResource(R.string.log_field_weeks_since_race)) {
                weeksSinceRace = it
                onKeypad(false)
            }
        }
        if (keypadOpen) {
            NumericKeypad(
                onKey = { key ->
                    setBuffer(editNumericBuffer(buffer(), key, fresh))
                    fresh = false
                },
                onBackspace = {
                    setBuffer(buffer().dropLast(1))
                    fresh = false
                },
                allowDecimal = active == RaceField.Volume,
                onDone = { onKeypad(false) },
            )
        }
        PrimaryButton(
            stringResource(R.string.log_btn_predict),
            {
                if (minParsed != null && secParsed != null && kmParsed != null) {
                    onPredict(
                        Event.PredictRace(
                            recentDistanceM = recent.meters,
                            recentTimeSec = minParsed * 60.0 + secParsed,
                            goalDistanceM = goal.meters,
                            // Canonical km on the wire: a mi entry converts at save.
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
private fun RacePresetField(label: String, current: RacePreset, labels: Map<RacePreset, String>, onSelect: (RacePreset) -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
        FieldLabel(label)
        SegmentedEnumRow(null, RacePreset.entries.toList(), current, display = { labels.getValue(it) }, onSelect = onSelect)
    }
}

/**
 * Muscle picker for the hypertrophy volume planner. [wire] is the exact string the
 * core's `hypertrophy::landmarks_for` looks up (case-insensitive, but sent verbatim);
 * [labelRes] is display-only. The set MUST match the LANDMARKS table in
 * `hypertrophy.rs`: an unknown muscle yields a single explanatory row instead of a plan.
 */
internal enum class HypertrophyMuscle(val wire: String, @StringRes val labelRes: Int) {
    Chest("chest", R.string.log_muscle_chest),
    Back("back", R.string.log_muscle_back),
    Quads("quads", R.string.log_muscle_quads),
    Hamstrings("hamstrings", R.string.log_muscle_hamstrings),
    Glutes("glutes", R.string.log_muscle_glutes),
    SideDelts("side delts", R.string.log_muscle_side_delts),
    RearDelts("rear delts", R.string.log_muscle_rear_delts),
    Biceps("biceps", R.string.log_muscle_biceps),
    Triceps("triceps", R.string.log_muscle_triceps),
    Calves("calves", R.string.log_muscle_calves),
    Abs("abs", R.string.log_muscle_abs),
}

/** Display label for a planner wire muscle ("side delts" → "Side delts"); an
 *  unknown name (the core then explains it isn't one) shows as sent. */
@Composable
internal fun hypertrophyMuscleLabel(wire: String): String =
    HypertrophyMuscle.entries.firstOrNull { it.wire.equals(wire, ignoreCase = true) }
        ?.let { stringResource(it.labelRes) } ?: wire

/** The accumulation-block lengths the planner offers, weeks, and its opening pick. */
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
fun HypertrophyPlannerForm(initial: HypertrophyInputView? = null, onPlan: (Event.PlanHypertrophyMeso) -> Unit) {
    // Rehydrate from the core's echoed query on a cold start; else defaults.
    var muscle by rememberSaveable {
        mutableStateOf(
            initial?.let { i -> HypertrophyMuscle.entries.firstOrNull { it.wire == i.muscle } }
                ?: HypertrophyMuscle.Chest,
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
        // Every muscle visible, one tap picks it.
        Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
            FieldLabel(stringResource(R.string.log_field_muscle))
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
        // Whole scale visible, one tap picks, never a −/+ stepper: the 2–8
        // week range fits a single ChoiceScaleRow.
        FieldLabel(stringResource(R.string.log_field_accumulation_weeks), "$weeks")
        ChoiceScaleRow(AccumulationWeeks.toList(), weeks, { "$it" }, label = stringResource(R.string.log_field_accumulation_weeks)) { weeks = it }
        SwitchRow(stringResource(R.string.log_switch_not_growing), notGrowing) { notGrowing = it }
        SwitchRow(stringResource(R.string.log_switch_recovering_easily), recoveringEasily) { recoveringEasily = it }
        PrimaryButton(
            stringResource(R.string.log_btn_plan_volume),
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
 * goal contexts (masters 65+, caloric deficit) and emits [Event.ComputeProtein];
 * the core answers with an evidence-graded g/day range (bodyweight × each graded
 * g/kg bound) rendered from `model.protein_targets`. No general/default number
 * exists, with neither toggle set the core returns nothing, so this form only
 * picks the inputs. With [mastersAvailable] false (a known age under 65) the
 * masters toggle is gone and never sent, leaving the deficit context alone. The
 * keypad shows only while [keypadOpen] (the well was tapped).
 */
@Composable
fun ProteinForm(
    initial: ProteinInputView?,
    keypadOpen: Boolean,
    onKeypad: (Boolean) -> Unit,
    profileBodyweightKg: Double? = null,
    // Display unit for the bodyweight field. The core computes from kg; lb is a
    // display edge (prefill kg→lb, Compute lb→kg).
    weightUnit: WeightUnit = WeightUnit.Kg,
    mastersAvailable: Boolean = true,
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

    // Bounds/chips in the displayed unit; ±2.5/5 kg plate jumps → natural ±5/10 lb
    // steps. The bounds are the app-wide bodyweight keypad range (Profile and setup
    // use the same), so a weight accepted there is accepted here.
    // An untouched prefill is wire-valid by construction (`bwComputeKg` emits `seedKg`
    // verbatim), so display rounding must not red it out — gate validity on that path.
    val bwBounds = bodyweightBounds(weightUnit)
    val bwParsed = parseKeypadNumber(bwText)
    val bwValid = (untouchedSinceOpen && seedKg != null) ||
        (bwParsed != null && bwParsed in bwBounds)
    // The kg fed to the core: an untouched prefill emits the original kg verbatim
    // (no kg→lb→kg drift); a touched buffer submits exactly what is displayed,
    // converting lb→kg at the same save boundary as the Profile bodyweight field.
    val bwComputeKg: Double? = if (untouchedSinceOpen && seedKg != null) seedKg
        else bwParsed?.let { bodyweightDisplayToKg(it, weightUnit) }

    FormCard {
        KeypadValueField(
            stringResource(R.string.log_field_bodyweight), weightSuffix(weightUnit), bwText,
            active = keypadOpen,
            // An empty field is an invitation, not an error: the hint asks for
            // the weight and Compute stays off until one is typed.
            invalid = bwText.isNotBlank() && !bwValid,
            min = bwBounds.start, max = bwBounds.endInclusive, format = "%.1f",
            adjustments = plateSteps(weightUnit),
            onActivate = { fresh = true; onKeypad(true) },
            onText = { bwText = it; fresh = false; untouchedSinceOpen = false; onKeypad(false) },
            hint = if (bwText.isBlank()) stringResource(R.string.log_protein_weight_hint) else null,
        )
        if (initial == null && profileBodyweightKg != null) {
            Text(
                stringResource(R.string.log_prefill_profile),
                color = OnBgMuted,
                style = Type.Caption,
            )
        }
        if (mastersAvailable) {
            SwitchRow(stringResource(R.string.log_switch_masters), masters) { masters = it; onKeypad(false) }
        }
        SwitchRow(stringResource(R.string.log_switch_deficit), deficit) { deficit = it; onKeypad(false) }
        if (!mastersOn && !deficit) {
            // No general/default protein figure is evidence-graded, so the core
            // returns nothing until a graded context is chosen. Without this the
            // button would look actionable yet render no result on tap: say why
            // and gate the button, matching the other forms' validity idiom.
            Text(
                stringResource(if (mastersAvailable) R.string.log_protein_no_context else R.string.log_protein_deficit_only),
                color = OnBgMuted,
                style = Type.Caption,
            )
        }
        if (keypadOpen) {
            NumericKeypad(
                onKey = { key ->
                    bwText = editNumericBuffer(bwText, key, fresh)
                    fresh = false
                    untouchedSinceOpen = false
                },
                onBackspace = {
                    bwText = bwText.dropLast(1)
                    fresh = false
                    untouchedSinceOpen = false
                },
                onDone = { onKeypad(false) },
            )
        }
        PrimaryButton(
            stringResource(R.string.log_btn_compute_protein),
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
 * running.rs, RUN-VDOT-001), this form only picks the age. The keypad shows only
 * while [keypadOpen] (the age well was tapped).
 */
@Composable
fun HrZonesForm(
    initial: HrZoneInputView?,
    keypadOpen: Boolean,
    onKeypad: (Boolean) -> Unit,
    profileAgeYears: Double? = null,
    profileRestingHrBpm: Double? = null,
    // (age, restingHrBpm?), a non-null RHR lets the core run Karvonen
    // heart-rate-reserve zones (running-005) instead of plain %HRmax.
    onCompute: (Double, Double?) -> Unit,
) {
    // Keypad-driven age entry (Log-set primitives, never a −/+ stepper).
    // Seed order: the echoed query first, then the profile's consolidated
    // age, then the plain default; enter your age once on Profile, prefilled here.
    var ageText by rememberSaveable {
        val seed = initial?.age_years ?: profileAgeYears
        mutableStateOf(seed?.let { String.format(Locale.US, "%.0f", it) } ?: HrZonesAgeSeed)
    }
    var fresh by rememberSaveable { mutableStateOf(true) }
    // Optional resting HR. Seeded from the echoed query, then the profile.
    // Karvonen zones are more personal than age-only %HRmax, so default it ON
    // whenever a resting HR is known (from the echo or the profile).
    val seedRhr = initial?.resting_hr_bpm ?: profileRestingHrBpm
    var useRhr by rememberSaveable { mutableStateOf(seedRhr != null) }
    var rhr by rememberSaveable { mutableStateOf(seedRhr?.toInt() ?: HrZonesRhrSeedBpm) }

    val ageParsed = parseKeypadNumber(ageText)
    val ageValid = ageParsed != null && ageParsed in TanakaAgeRange

    FormCard {
        KeypadValueField(
            stringResource(R.string.log_field_age), stringResource(R.string.log_unit_years), ageText,
            active = keypadOpen,
            invalid = !ageValid,
            min = TanakaAgeRange.start, max = TanakaAgeRange.endInclusive, format = "%.0f",
            adjustments = listOf(-1.0, 1.0),
            onActivate = { fresh = true; onKeypad(true) },
            onText = { ageText = it; fresh = false; onKeypad(false) },
        )
        if (initial == null && (profileAgeYears != null || profileRestingHrBpm != null)) {
            Text(
                stringResource(R.string.log_prefill_profile),
                color = OnBgMuted,
                style = Type.Caption,
            )
        }
        // Resting HR unlocks Karvonen (heart-rate-reserve) zones. Off → the
        // core falls back to age-only %HRmax, so this stays optional.
        SwitchRow(stringResource(R.string.log_switch_use_rhr), useRhr) { useRhr = it; onKeypad(false) }
        if (useRhr) {
            FieldLabel(stringResource(R.string.log_field_resting_hr), stringResource(R.string.log_value_bpm, rhr))
            ScrollableScaleRow(HrZonesRhrScale, rhr, { "$it" }, label = stringResource(R.string.log_field_resting_hr)) {
                rhr = it
                onKeypad(false)
            }
        }
        if (keypadOpen) {
            NumericKeypad(
                onKey = { key ->
                    ageText = editNumericBuffer(ageText, key, fresh)
                    fresh = false
                },
                onBackspace = {
                    ageText = ageText.dropLast(1)
                    fresh = false
                },
                allowDecimal = false,
                onDone = { onKeypad(false) },
            )
        }
        PrimaryButton(
            stringResource(R.string.log_btn_compute_hr),
            { ageParsed?.let { onCompute(it, if (useRhr) rhr.toDouble() else null) } },
            Modifier.fillMaxWidth(),
            enabled = ageValid,
        )
    }
}
