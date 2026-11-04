package app.milestone

import androidx.annotation.StringRes
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.res.stringResource

/** Pain character → core [PainKind] (schema.rs, File-08 Table 4.1). The labels
 *  are human; the mapping decides the gate: SharpJoint hard-stops, tendon pain
 *  is graded by severity/trend, DOMS continues, "Not sure" is the conservative
 *  bare-report stop. */
private enum class PainCharacter(@StringRes val labelRes: Int, val kind: PainKind) {
    Sharp(R.string.log_pain_char_sharp, PainKind.SharpJoint),
    Tendon(R.string.log_pain_char_tendon, PainKind.TendonLoadRelated),
    Doms(R.string.log_pain_char_doms, PainKind.Doms),
    NotSure(R.string.log_pain_char_not_sure, PainKind.Other),
}

/** Preset body areas for the pain report. [wire] is what the report carries and
 *  must stay a fixed English key: the core maps it to a body region to scope a
 *  sharp-pain stop (`autoreg::pain_region`) and echoes it on the banner;
 *  [labelRes] is only the chip text. */
private enum class PainArea(val wire: String, @StringRes val labelRes: Int) {
    LeftKnee("Left knee", R.string.log_pain_area_left_knee),
    RightKnee("Right knee", R.string.log_pain_area_right_knee),
    Ankle("Ankle / Achilles", R.string.log_pain_area_ankle),
    Hip("Hip", R.string.log_pain_area_hip),
    LowerBack("Lower back", R.string.log_pain_area_lower_back),
    Shoulder("Shoulder", R.string.log_pain_area_shoulder),
    ElbowWrist("Elbow / wrist", R.string.log_pain_area_elbow_wrist),
    Neck("Neck", R.string.log_pain_area_neck),
    Foot("Foot", R.string.log_pain_area_foot),
    Other("Other", R.string.log_pain_area_other),
}

/** The body area a pain report names, as its chip label: from the report's own
 *  [location] when the shell still holds it, else from the head of the core's
 *  pain detail line ("Left knee · sharp/joint · 6/10"). Null when no preset area
 *  is named. */
@Composable
internal fun painAreaLabel(location: String?, coreDetail: String?): String? {
    val wire = location ?: coreDetail?.substringBefore(" · ")?.trim()
    val area = PainArea.entries.firstOrNull { it.wire.equals(wire, ignoreCase = true) && it != PainArea.Other }
    return area?.let { stringResource(it.labelRes) }
}

/** Pain severity, 0 (none) to 10 (worst), left to right. */
private val PainSeverityValues = (0..10).toList()

/**
 * Pain triage: a ~10-second characterization BEFORE any hold is set (body area,
 * character, severity 0–10, worsening), sent as a [PainDetail]; the core decides
 * hold vs modify-and-monitor. Nothing is pre-answered (character "Not sure", the
 * conservative stop; severity unrated), and "Report pain" is always enabled.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun PainTriageSheet(
    onClose: () -> Unit,
    // Report unsaved-triage state so a host sheet's swipe-down can guard it.
    onDirtyChange: (Boolean) -> Unit,
    onSubmit: (Event.SubmitReadiness) -> Unit,
) {
    var location by rememberSaveable { mutableStateOf<PainArea?>(null) }
    var character by rememberSaveable { mutableStateOf(PainCharacter.NotSure) }
    var severity by rememberSaveable { mutableStateOf<Int?>(null) }
    var rising by rememberSaveable { mutableStateOf(false) }
    // A delayed-worsening report (tendon reactive: worse the morning AFTER load) is
    // a distinct signal from immediate rising pain; the core reads it as Rising.
    var worseningAfter24h by rememberSaveable { mutableStateOf(false) }
    // Backdatable stamp (every editor supports it): defaults to now; the picked
    // value is what the emitted report carries.
    var observedAt by rememberSaveable { mutableStateOf(AppClock.nowSec()) }
    // The now captured at open, so a real backdate is told apart from the default.
    val initialObservedAt = rememberSaveable { observedAt }
    // Any filled-in triage marks the sheet dirty, so a close or swipe-down asks first.
    val dirty = location != null ||
        character != PainCharacter.NotSure ||
        severity != null ||
        rising ||
        worseningAfter24h ||
        observedAt != initialObservedAt
    ReportDirty(dirty, onDirtyChange)
    val characterLabels = PainCharacter.entries.associateWith { stringResource(it.labelRes) }
    val severityText = PainSeverityValues.associateWith { stringResource(R.string.log_pain_severity_value, it) }
    val severitySpoken = PainSeverityValues.associateWith { stringResource(R.string.rating_pain_spoken, it) }

    EditorScaffold(
        title = stringResource(R.string.log_report_pain),
        dirty = dirty,
        saveEnabled = true,
        onClose = onClose,
        onSave = {
            onSubmit(
                Event.SubmitReadiness(
                    signal = ReadinessSignal.Pain,
                    value = 1.0,
                    observedAt = observedAt,
                    pain = PainDetail(
                        kind = character.kind,
                        // Unrated stays absent on the wire; the core treats an
                        // unrated tendon report conservatively.
                        severity = severity,
                        trend = when {
                            worseningAfter24h -> PainTrend.WorseningAfter24h
                            rising -> PainTrend.Rising
                            else -> PainTrend.Stable
                        },
                        persists = false,
                        location = location?.wire,
                    ),
                )
            )
        },
        // A neutral action, not an asserted outcome: the core decides hold vs
        // modify-and-monitor from the report.
        actionLabel = stringResource(R.string.log_report_pain),
        danger = true,
    ) {
        Text(
            // Honest for BOTH core outcomes: a red-flag report becomes a hold, a
            // tolerable one a modify-and-monitor adjustment.
            stringResource(R.string.log_pain_intro),
            color = OnBgMuted,
            style = Type.Caption,
        )
        FormCard {
            FieldLabel(
                stringResource(R.string.log_pain_where),
                location?.let { stringResource(it.labelRes) } ?: stringResource(R.string.log_optional),
            )
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(ChoiceChipGap),
                verticalArrangement = Arrangement.spacedBy(ChoiceChipGap),
            ) {
                PainArea.entries.forEach { area ->
                    ChoiceChip(stringResource(area.labelRes), selected = location == area, onClick = {
                        location = if (location == area) null else area
                    })
                }
            }
            EnumRow(
                stringResource(R.string.log_pain_feel),
                PainCharacter.entries.toList(),
                character,
                display = { characterLabels.getValue(it) },
            ) { character = it }
            // Unrated until picked; tapping the picked number again clears it.
            RatingScale(
                label = stringResource(R.string.log_pain_severity),
                values = PainSeverityValues,
                selected = severity,
                onSelect = { severity = it },
                startCaption = stringResource(R.string.rating_pain_start),
                endCaption = stringResource(R.string.rating_pain_end),
                selectedText = { severityText.getValue(it) },
                spokenText = { severitySpoken.getValue(it) },
                testTag = "pain-severity",
                hint = stringResource(R.string.log_optional),
            )
            LabeledSwitchRow(stringResource(R.string.log_pain_worse), rising) { rising = it }
            LabeledSwitchRow(stringResource(R.string.pain_trend_worsening), worseningAfter24h) { worseningAfter24h = it }
            ObservedAtRow(observedAt, withTime = true) { observedAt = it }
        }
    }
}
