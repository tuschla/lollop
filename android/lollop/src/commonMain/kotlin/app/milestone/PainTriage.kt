package app.milestone

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import app.milestone.resources.Res
import app.milestone.resources.log_optional
import app.milestone.resources.log_pain_area_ankle
import app.milestone.resources.log_pain_area_elbow_wrist
import app.milestone.resources.log_pain_area_foot
import app.milestone.resources.log_pain_area_hip
import app.milestone.resources.log_pain_area_left_knee
import app.milestone.resources.log_pain_area_lower_back
import app.milestone.resources.log_pain_area_neck
import app.milestone.resources.log_pain_area_other
import app.milestone.resources.log_pain_area_right_knee
import app.milestone.resources.log_pain_area_shoulder
import app.milestone.resources.log_pain_char_doms
import app.milestone.resources.log_pain_char_not_sure
import app.milestone.resources.log_pain_char_sharp
import app.milestone.resources.log_pain_char_tendon
import app.milestone.resources.log_pain_feel
import app.milestone.resources.log_pain_intro
import app.milestone.resources.log_pain_severity
import app.milestone.resources.log_pain_severity_value
import app.milestone.resources.log_pain_where
import app.milestone.resources.log_pain_worse
import app.milestone.resources.log_report_pain
import app.milestone.resources.pain_trend_worsening
import app.milestone.resources.rating_pain_end
import app.milestone.resources.rating_pain_spoken
import app.milestone.resources.rating_pain_start
import org.jetbrains.compose.resources.StringResource
import org.jetbrains.compose.resources.stringResource

/** Pain character → core [PainKind] (schema.rs, File-08 Table 4.1). The labels
 *  are human; the mapping decides the gate: SharpJoint hard-stops, tendon pain
 *  is graded by severity/trend, DOMS continues, "Not sure" is the conservative
 *  bare-report stop. */
private enum class PainCharacter(val labelRes: StringResource, val kind: PainKind) {
    Sharp(Res.string.log_pain_char_sharp, PainKind.SharpJoint),
    Tendon(Res.string.log_pain_char_tendon, PainKind.TendonLoadRelated),
    Doms(Res.string.log_pain_char_doms, PainKind.Doms),
    NotSure(Res.string.log_pain_char_not_sure, PainKind.Other),
}

/** The chip text of a preset body area ([PainArea] is the core's: its wire
 *  name is the location the report carries). */
private val PainArea.labelRes: StringResource
    get() = when (this) {
        PainArea.LeftKnee -> Res.string.log_pain_area_left_knee
        PainArea.RightKnee -> Res.string.log_pain_area_right_knee
        PainArea.Ankle -> Res.string.log_pain_area_ankle
        PainArea.Hip -> Res.string.log_pain_area_hip
        PainArea.LowerBack -> Res.string.log_pain_area_lower_back
        PainArea.Shoulder -> Res.string.log_pain_area_shoulder
        PainArea.ElbowWrist -> Res.string.log_pain_area_elbow_wrist
        PainArea.Neck -> Res.string.log_pain_area_neck
        PainArea.Foot -> Res.string.log_pain_area_foot
        PainArea.Other -> Res.string.log_pain_area_other
    }

/** The chip label of the area the active pain report names (`active_pain.area`);
 *  null when it names none. */
@Composable
internal fun painAreaLabel(area: PainArea?): String? = area?.let { stringResource(it.labelRes) }

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
    val characterLabels = PainCharacter.entries.associateWith { stringResource(it.labelRes) }
    val severityText = PainSeverityValues.associateWith { stringResource(Res.string.log_pain_severity_value, it) }
    val severitySpoken = PainSeverityValues.associateWith { stringResource(Res.string.rating_pain_spoken, it) }

    EditorScaffold(
        title = stringResource(Res.string.log_report_pain),
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
        actionLabel = stringResource(Res.string.log_report_pain),
        danger = true,
    ) {
        Text(
            // Honest for BOTH core outcomes: a red-flag report becomes a hold, a
            // tolerable one a modify-and-monitor adjustment.
            stringResource(Res.string.log_pain_intro),
            color = OnBgMuted,
            style = Type.Caption,
        )
        FormCard {
            FieldLabel(
                stringResource(Res.string.log_pain_where),
                location?.let { stringResource(it.labelRes) } ?: stringResource(Res.string.log_optional),
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
                stringResource(Res.string.log_pain_feel),
                PainCharacter.entries.toList(),
                character,
                display = { characterLabels.getValue(it) },
            ) { character = it }
            // Unrated until picked; tapping the picked number again clears it.
            RatingScale(
                label = stringResource(Res.string.log_pain_severity),
                values = PainSeverityValues,
                selected = severity,
                onSelect = { severity = it },
                startCaption = stringResource(Res.string.rating_pain_start),
                endCaption = stringResource(Res.string.rating_pain_end),
                selectedText = { severityText.getValue(it) },
                spokenText = { severitySpoken.getValue(it) },
                testTag = "pain-severity",
                hint = stringResource(Res.string.log_optional),
            )
            LabeledSwitchRow(stringResource(Res.string.log_pain_worse), rising) { rising = it }
            LabeledSwitchRow(stringResource(Res.string.pain_trend_worsening), worseningAfter24h) { worseningAfter24h = it }
            ObservedAtRow(observedAt, withTime = true) { observedAt = it }
        }
    }
}
