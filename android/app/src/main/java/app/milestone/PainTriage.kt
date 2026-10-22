package app.milestone

import androidx.annotation.StringRes
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp

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
 * Pain triage sheet: a ~10-second characterization that opens
 * BEFORE any hold is set, so an accidental tap can't freeze the app. It captures
 * exactly what the File-08 pain gate needs: body area, character (→ [PainKind]),
 * severity 0–10, and whether it's worsening (→ [PainTrend]); then "Report pain"
 * sends a [PainDetail] via [Event.SubmitReadiness] and the core decides hold vs
 * modify-and-monitor. Nothing is pre-answered: character starts at "Not sure"
 * (the conservative stop) and severity unrated (sent as absent), so one quick
 * tap never records a pain the user didn't describe. Under a pinning host
 * ([LocalEditorPinnedActions]) the form scrolls and the commit stays pinned
 * above the navigation bar.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun PainTriageSheet(
    onClose: () -> Unit = {},
    // Report unsaved-triage state so a host sheet's swipe-down can guard it.
    onDirtyChange: (Boolean) -> Unit = {},
    onSubmit: (Event.SubmitReadiness) -> Unit,
) {
    val status = LocalStatusColors.current
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
    // Any filled-in triage marks the sheet dirty: the host's swipe-down and the
    // sheet's own X then guard it like every other editor. The "Report pain"
    // submit stays always enabled regardless (safety escape hatch).
    val dirty = location != null ||
        character != PainCharacter.NotSure ||
        severity != null ||
        rising ||
        worseningAfter24h ||
        observedAt != initialObservedAt
    ReportDirty(dirty, onDirtyChange)
    var confirmDiscard by remember { mutableStateOf(false) }
    // In-flight latch, same as EditorScaffold's Save pill: onSubmit is
    // `{ ev -> onEvent(ev); onDismiss() }` and the sheet takes ~300 ms to hide, so
    // a second tap in that window fires a second SubmitReadiness and double-reports
    // the pain. This button isn't an EditorScaffold pill (it's the danger commit),
    // so it needs its own latch. Plain `remember` boolean, it resets when the sheet
    // is disposed and re-mounted, and we DON'T disable the button (HR3: the pain
    // escape hatch must never look blocked), we just swallow re-taps below.
    var submitted by remember { mutableStateOf(false) }
    val characterLabels = PainCharacter.entries.associateWith { stringResource(it.labelRes) }
    val severityText = PainSeverityValues.associateWith { stringResource(R.string.log_pain_severity_value, it) }
    val severitySpoken = PainSeverityValues.associateWith { stringResource(R.string.rating_pain_spoken, it) }

    val form: @Composable () -> Unit = {
        Text(
            // Honest for BOTH core outcomes: a red-flag report becomes a hold, a
            // tolerable one a modify-and-monitor adjustment. The core decides the
            // tier from what's reported, so the copy must not assert a hold up-front.
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
            SwitchRow(stringResource(R.string.log_pain_worse), rising) { rising = it }
            SwitchRow(stringResource(R.string.pain_trend_worsening), worseningAfter24h) { worseningAfter24h = it }
            ObservedAtRow(observedAt, withTime = true) { observedAt = it }
        }
    }

    Column(
        // The danger commit is the sheet's last row: keep it above the system
        // navigation bar.
        modifier = Modifier.fillMaxWidth().navigationBarsPadding(),
        verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        // Header: close (back to the chooser) + centered title. No Save: the
        // commit is the danger "Report pain" button below, deliberately distinct.
        EditorTitleRow(stringResource(R.string.log_report_pain)) {
            if (dirty) confirmDiscard = true else onClose()
        }
        if (LocalEditorPinnedActions.current) {
            // The host leaves scrolling to the editor: the form scrolls between
            // the header and the always-visible commit.
            Box(modifier = Modifier.weight(1f, fill = false).fillMaxWidth()) {
                Column(
                    modifier = Modifier.fillMaxWidth().verticalScroll(rememberScrollState()),
                    verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
                ) { form() }
            }
        } else {
            form()
        }
        // Danger commit. Always enabled: the bare/uncertain report is the safety
        // escape hatch and must never be blocked (HR3).
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = ActionMinHeight)
                .clip(RoundedCornerShape(Space.Card.dp))
                .background(status.danger)
                .clickable(role = Role.Button) {
                    // Swallow re-taps: the first commit is the only commit, so a
                    // double-tap in the sheet's dismiss window can't send two reports.
                    if (!submitted) {
                        submitted = true
                        onSubmit(
                            Event.SubmitReadiness(
                                signal = ReadinessSignal.Pain,
                                value = 1.0,
                                observedAt = observedAt,
                                streak = 0,
                                pain = PainDetail(
                                    kind = character.kind,
                                    // Unrated stays absent on the wire; the core
                                    // treats an unrated tendon report conservatively.
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
                    }
                }
                .padding(Space.Card.dp),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                // Neutral action, not an asserted outcome: the core decides hold
                // vs modify-and-monitor from the report, so the button never
                // promises a hold on a tolerable pain.
                stringResource(R.string.log_report_pain),
                color = Color.White,
                style = Type.Body.copy(fontWeight = FontWeight.ExtraBold),
            )
        }
    }
    if (confirmDiscard) {
        DiscardEntryDialog(onKeepEditing = { confirmDiscard = false }) {
            confirmDiscard = false
            onClose()
        }
    }
}
