package app.milestone

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.requiredHeight
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.foundation.layout.wrapContentSize
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import app.milestone.resources.Res
import app.milestone.resources.main_action_subject
import app.milestone.resources.main_chip_contested
import app.milestone.resources.main_chip_safety
import app.milestone.resources.main_confidence_paren
import app.milestone.resources.main_contested_explainer
import app.milestone.resources.main_details
import app.milestone.resources.main_details_subject
import app.milestone.resources.main_evidence_confidence
import app.milestone.resources.main_evidence_grade
import app.milestone.resources.main_evidence_label
import app.milestone.resources.main_evidence_legend_body
import app.milestone.resources.main_evidence_legend_title
import app.milestone.resources.main_explain
import app.milestone.resources.main_glossary_1rm_def
import app.milestone.resources.main_glossary_1rm_term
import app.milestone.resources.main_glossary_ctl_def
import app.milestone.resources.main_glossary_ctl_term
import app.milestone.resources.main_glossary_decoupling_def
import app.milestone.resources.main_glossary_decoupling_term
import app.milestone.resources.main_glossary_deload_def
import app.milestone.resources.main_glossary_deload_term
import app.milestone.resources.main_glossary_e1rm_def
import app.milestone.resources.main_glossary_e1rm_term
import app.milestone.resources.main_glossary_hrmax_def
import app.milestone.resources.main_glossary_hrmax_term
import app.milestone.resources.main_glossary_hrzones_def
import app.milestone.resources.main_glossary_hrzones_term
import app.milestone.resources.main_glossary_maf_def
import app.milestone.resources.main_glossary_maf_term
import app.milestone.resources.main_glossary_mesocycle_def
import app.milestone.resources.main_glossary_mesocycle_term
import app.milestone.resources.main_glossary_no_match
import app.milestone.resources.main_glossary_pap_def
import app.milestone.resources.main_glossary_pap_term
import app.milestone.resources.main_glossary_reds_def
import app.milestone.resources.main_glossary_reds_term
import app.milestone.resources.main_glossary_rir_def
import app.milestone.resources.main_glossary_rir_term
import app.milestone.resources.main_glossary_rpe_def
import app.milestone.resources.main_glossary_rpe_term
import app.milestone.resources.main_glossary_search_hint
import app.milestone.resources.main_glossary_spike_def
import app.milestone.resources.main_glossary_spike_term
import app.milestone.resources.main_glossary_title
import app.milestone.resources.main_glossary_tonnage_def
import app.milestone.resources.main_glossary_tonnage_term
import app.milestone.resources.main_glossary_trimp_def
import app.milestone.resources.main_glossary_trimp_term
import app.milestone.resources.main_glossary_vdot_def
import app.milestone.resources.main_glossary_vdot_term
import app.milestone.resources.main_glossary_vi_def
import app.milestone.resources.main_glossary_vi_term
import app.milestone.resources.main_glossary_volume_def
import app.milestone.resources.main_glossary_volume_term
import app.milestone.resources.main_glossary_zscore_def
import app.milestone.resources.main_glossary_zscore_term
import app.milestone.resources.main_grade_expert
import app.milestone.resources.main_grade_line_confidence
import app.milestone.resources.main_grade_line_confidence_range
import app.milestone.resources.main_grade_moderate
import app.milestone.resources.main_grade_myth
import app.milestone.resources.main_grade_strong
import app.milestone.resources.main_grade_weak
import app.milestone.resources.main_hide_details
import app.milestone.resources.main_hide_details_subject
import app.milestone.resources.main_legend_contested
import app.milestone.resources.main_legend_safety
import app.milestone.resources.main_why_based_on
import app.milestone.resources.main_why_terms
import app.milestone.resources.main_why_this_grade
import app.milestone.resources.main_why_to_improve
import org.jetbrains.compose.resources.StringResource
import org.jetbrains.compose.resources.stringResource

/** The single unified disclosure affordance: a small circular "?" that toggles a
 *  card's evidence panel (grade badge, confidence, citation, why) on every card
 *  that has one (EvidenceCard, RunCard, prescription/adjustment/calculator/
 *  reference cards), so the collapsed face never leads with evidence chrome; only
 *  the summary and the SAFETY chip sit on the face (SAFETY stays, HARD RULE).
 *  Filled when open. The "?" is the app's ONE "explain this" glyph: the glossary
 *  affordance ([GlossaryInfo]) and the grading-legend link ([GradeChip]) draw the
 *  same [DisclosureDisc], so there is one symbol at one size everywhere.
 *  `tint`/`onTint` recolor the SAME affordance for non-default surfaces (the
 *  danger SafetyBanner uses white-on-danger: mid-tone Accent fails on red);
 *  shape/behaviour never vary per call site. `subject` names what the details
 *  are about ("Details: Back Squat") so repeated "?" buttons stay
 *  distinguishable to a screen reader. */
@Composable
internal fun DisclosureButton(
    expanded: Boolean,
    tint: Color = Accent,
    onTint: Color = OnAccent,
    subject: String? = null,
    onClick: () -> Unit,
) {
    // Resolved outside the (non-composable) semantics lambda below.
    val hideDetailsDesc = if (subject.isNullOrBlank()) {
        stringResource(Res.string.main_hide_details)
    } else {
        stringResource(Res.string.main_hide_details_subject, subject)
    }
    val detailsDesc = if (subject.isNullOrBlank()) {
        stringResource(Res.string.main_details)
    } else {
        stringResource(Res.string.main_details_subject, subject)
    }
    // ≥48dp touch target (accessibility) around the visual disc: an outer box
    // carries the clickable.
    Box(
        modifier = Modifier
            .size(ActionMinHeight)
            .clip(PillShape)
            .clickable(role = Role.Button) { onClick() }
            // Name the affordance for screen readers (the bare "?" glyph is
            // meaningless spoken); state-aware so the toggle reads its effect.
            .semantics {
                contentDescription = if (expanded) hideDetailsDesc else detailsDesc
            },
        contentAlignment = Alignment.Center,
    ) {
        DisclosureDisc(filled = expanded, tint = tint, onTint = onTint)
    }
}

/**
 * The drawing of the app's one "explain this" glyph: a [DisclosureDiscSize] disc
 * holding a Bold `Type.Chip` "?" in `tint` inside a 1dp `tint` ring on the host
 * ground, or `onTint` on a solid `tint` fill when [filled] (an open panel). The
 * glyph sits on the bare ground, never on an accent wash: Accent text on its own
 * tint dips under AA (Rose-light, Meadow-light), Accent on every ground clears
 * it (ThemeContrastTest). Purely visual; the caller owns the ≥48dp touch target
 * and the spoken label. The disc is a minimum size, so the glyph grows it into a
 * pill at a large font scale instead of clipping.
 */
@Composable
private fun DisclosureDisc(filled: Boolean = false, tint: Color = Accent, onTint: Color = OnAccent) {
    Box(
        modifier = Modifier
            .sizeIn(minWidth = DisclosureDiscSize, minHeight = DisclosureDiscSize)
            .clip(PillShape)
            .then(if (filled) Modifier.background(tint) else Modifier.border(1.dp, tint, PillShape)),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            "?",
            color = if (filled) onTint else tint,
            style = Type.Chip,
        )
    }
}

/**
 * A compact "?" link next to an inline value or label: the [DisclosureDisc] at its
 * natural size (so a dense row keeps its height), with an overlaid ≥48dp square
 * (requiredSize + matchParentSize, so it OVERFLOWS rather than grows the row)
 * carrying the real tap target and the spoken [description].
 */
@Composable
private fun InlineDisclosureLink(description: String, onClick: () -> Unit) {
    Box(contentAlignment = Alignment.Center) {
        DisclosureDisc()
        Box(
            Modifier
                .matchParentSize()
                .requiredSize(ActionMinHeight)
                .clip(PillShape)
                .clickable(role = Role.Button, onClick = onClick)
                .semantics { contentDescription = description },
        )
    }
}

/**
 * EvidenceCard (02-coach §EvidenceCard, INVARIANTS 2 & 5): the collapsed FACE
 * carries ONLY the summary, the SAFETY chip (safety must stay visible) and the
 * single unified "?" disclosure. The grade badge, CONTESTED marker, confidence,
 * citation and grade note ALL live behind the "?", so no card leads with
 * evidence chrome; every recommendation still carries its grade + confidence +
 * citation.
 *
 * [confidenceInWhy] gates the confidence figure inside the "?" panel. A safety
 * hold passes false: safety is a rule, not a probability.
 * `container`/`border` let a caller tint the card. [status] draws the card as a
 * [StatusCard] of that tone (Today's adjustment, hold and coach-note cards):
 * ring + mark tile + [statusOverline] header, SAFETY chip and "?" on that row,
 * and the actions in a [StatusCardFooter] ([actionLabel] as its button,
 * [secondaryActionLabel] as its text action). `extraDetail` is an optional line
 * rendered inside the "?" panel (the HRmax card's Tanaka formula).
 */
@Composable
internal fun EvidenceCard(
    summary: String,
    evidence: EvidenceTagView,
    section: String? = null,
    confidenceInWhy: Boolean = true,
    container: Color = BgElevated,
    border: Color? = null,
    status: StatusTone? = null,
    statusOverline: String? = null,
    extraDetail: String? = null,
    why: WhyView? = null,
    // Optional action pill at the tail of the content column. Kept OFF the card
    // body (the whole card is deliberately non-clickable, see below): the ripple
    // stays bounded to the small pill, matching the DisclosureButton precedent.
    actionLabel: String? = null,
    onAction: (() -> Unit)? = null,
    // A quieter second verb (a [status] card's footer text action, e.g. "Remove report").
    secondaryActionLabel: String? = null,
    onSecondaryAction: (() -> Unit)? = null,
    // What the card is about (an exercise name): appended to the "?" and action
    // descriptions so identical controls on sibling cards stay distinguishable.
    subject: String? = null,
    // The one "do this next" action is a filled Accent pill; any other action
    // on the screen is a tonal pill (same size and tap target, quieter fill).
    actionPrimary: Boolean = true,
    // Card-owned detail rendered INSIDE the card, between the summary/panel and
    // the action pill (per-set slots, progress), so it reads as part of this card.
    footer: (@Composable () -> Unit)? = null,
    // Face detail directly under the summary (load, plates, effort, pace), ABOVE
    // the "?" panel: the do-this numbers stay on the face next to the summary.
    subline: (@Composable () -> Unit)? = null,
    // Card-owned detail rendered INSIDE the "?" panel, after the evidence lines
    // (e.g. a hold card's follow-up evidence), so the card keeps ONE "?".
    panelExtra: (@Composable () -> Unit)? = null,
    // The summary reads as prose: the row's `prose` (a long paragraph), or a
    // family of sibling cards (Today's coaching cards) that wears one style.
    proseSummary: Boolean = false,
    // The glossary terms the core marked in [summary] (the row's `glossary`).
    glossary: List<GlossarySpan> = emptyList(),
    // What the card is about in the core's words (the row's `subject`), when the
    // caller passes no [subject], [section] or [statusOverline].
    spokenSubject: String? = null,
) {
    val statusColors = LocalStatusColors.current
    // Every "?" names what it explains ("Details: <subject>"): the caller's
    // subject, else the card's overline, else the core's subject of the summary.
    val disclosureSubject = subject ?: section ?: statusOverline ?: spokenSubject
    // Keyed on the card's identity (summary+citation), not composition position:
    // in an unkeyed/reordered list a positional saveable can re-attach an open
    // "?" panel to a DIFFERENT card after reorder. Keying
    // also resets the panel when a slot's content changes, correct, since the
    // disclosure belongs to the claim, not the slot.
    var expanded by rememberSaveable(summary, evidence.citation) { mutableStateOf(false) }
    // Jargon in the summary (RIR, RPE, e1RM, …) gets glossary links inside the
    // "?" panel, so the face stays clean (ONE "?") but every term has a path to
    // its meaning.
    val panelGlossaryKeys = remember(glossary) { glossary.terms() }
    // Bold is a summary weight: a coaching paragraph (prose, or a sectioned note
    // under its own overline) drops to regular weight and stops outweighing the
    // card titles around it. A safety card keeps bold (maximal emphasis).
    val safetyCritical = evidence.safety_critical
    val summaryStyle = if (!safetyCritical && (proseSummary || section != null)) {
        Type.Body
    } else {
        Type.BodyBold
    }
    val disclosure: @Composable () -> Unit = {
        DisclosureButton(expanded, subject = disclosureSubject) { expanded = !expanded }
    }
    // Everything under the face: the do-this numbers, the "?" panel, the card's
    // own footer detail.
    val details: @Composable () -> Unit = {
        subline?.invoke()
        if (expanded) {
            WhyDetail(
                why = why,
                evidence = evidence,
                // Confidence appears inside the "?" for every card except a
                // safety hold (safety is a rule, not a probability).
                showConfidence = confidenceInWhy,
                extraDetail = extraDetail,
                glossaryKeys = panelGlossaryKeys,
            )
            panelExtra?.invoke()
        }
        footer?.invoke()
    }
    if (status != null) {
        StatusCard(
            tone = status,
            overline = statusOverline ?: section.orEmpty(),
            safety = safetyCritical,
            trailing = disclosure,
            footer = if (actionLabel != null || secondaryActionLabel != null) {
                {
                    StatusCardFooter(
                        primaryLabel = actionLabel?.takeIf { onAction != null },
                        onPrimary = { onAction?.invoke() },
                        secondaryLabel = secondaryActionLabel?.takeIf { onSecondaryAction != null },
                        onSecondary = { onSecondaryAction?.invoke() },
                    )
                }
            } else {
                null
            },
        ) {
            Text(summary, modifier = Modifier.fillMaxWidth(), color = OnBgBody, style = summaryStyle)
            details()
        }
        return
    }
    // A safety-critical card carries the danger border unless the caller styled it
    // (01-today §C.2): an explicit `border` wins.
    val effectiveBorder = border ?: if (safetyCritical) statusColors.danger.copy(alpha = 0.35f) else null
    // The card itself is NOT clickable: the single "?" DisclosureButton toggles
    // expansion. A whole-card ripple on a tall (expanded) card is a circular,
    // partial highlight that can never fill the rounded rectangle and reads as a
    // soft-edged shadow; the only ripple is the small, self-contained "?".
    Card(
        colors = CardDefaults.cardColors(containerColor = container),
        elevation = CardDefaults.cardElevation(defaultElevation = 0.dp),
        shape = RoundedCornerShape(Space.Card.dp),
        modifier = Modifier
            .fillMaxWidth()
            .then(
                if (effectiveBorder != null) {
                    Modifier.border(1.dp, effectiveBorder, RoundedCornerShape(Space.Card.dp))
                } else {
                    Modifier
                },
            ),
    ) {
        Column(
            Modifier.padding(Space.Card.dp),
            verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
        ) {
            // A bare face (no SAFETY chip, no section overline) would collapse the
            // header to a lone right-aligned "?" floating above the summary, which
            // reads as an empty gap (the hero's nested prescription cards). Fold
            // the summary and "?" into ONE row instead. The chip/section variant
            // keeps the two-row face.
            if (!safetyCritical && section == null) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(summary, modifier = Modifier.weight(1f), color = OnBgBody, style = summaryStyle)
                    Box(Modifier.offset(x = DisclosureEdgeInset)) { disclosure() }
                }
            } else {
                // Header + summary sit Space.Sm apart: the header row is only as
                // tall as its chip/overline, and the "?" keeps its full 48dp
                // target by overflowing that compact row instead of padding the
                // header with dead air.
                Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
                    Row(
                        modifier = Modifier.fillMaxWidth().heightIn(min = CompactHeaderHeight),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp + Space.Xs.dp),
                    ) {
                        // Only the SAFETY chip stays on the collapsed face: safety
                        // visibility is a HARD RULE, not a declutter tradeoff. The
                        // CONTESTED marker (an evidence-quality tag, not a safety
                        // signal), grade badge, confidence, citation and grade note
                        // live behind the "?".
                        if (safetyCritical) Chip(stringResource(Res.string.main_chip_safety), statusColors.danger)
                        // The card's category label uses the one overline grammar.
                        section?.let { TileOverline(it, color = Accent) }
                        Spacer(Modifier.weight(1f))
                        // The 48dp target overflows this compact box unbounded and stays
                        // centred on it, so the disc's own inset in the BOX (not
                        // [DisclosureEdgeInset], which is measured in the 48dp target)
                        // is what lands the disc on the content edge.
                        Box(
                            Modifier.size(CompactHeaderHeight).offset(x = (CompactHeaderHeight - DisclosureDiscSize) / 2).wrapContentSize(unbounded = true),
                            contentAlignment = Alignment.Center,
                        ) {
                            disclosure()
                        }
                    }
                    Text(summary, modifier = Modifier.fillMaxWidth(), color = OnBgBody, style = summaryStyle)
                }
            }
            details()
            if (actionLabel != null && onAction != null) {
                // Full-width action (Log / Track) from the one button system: the
                // "do this next" is a PrimaryButton, any other is a SecondaryButton
                // (same 48dp size so out-of-order logging stays one tap, quieter
                // fill). Still bounded to its own ripple: the card body stays
                // non-clickable (see the rationale above).
                val actionDesc = subject?.takeIf { it.isNotBlank() }?.let {
                    stringResource(Res.string.main_action_subject, actionLabel, it)
                }
                if (actionPrimary) {
                    PrimaryButton(
                        actionLabel,
                        onAction,
                        Modifier.fillMaxWidth().testTag("plan-action-filled"),
                        description = actionDesc,
                    )
                } else {
                    SecondaryButton(
                        actionLabel,
                        onAction,
                        Modifier.fillMaxWidth().testTag("plan-action-tonal"),
                        description = actionDesc,
                    )
                }
            }
        }
    }
}

/** Height of the chip/section header row on a two-row EvidenceCard face. */
private val CompactHeaderHeight = 28.dp

/**
 * The three-part "why?" disclosure body. Renders the core-provided [WhyView] as:
 * basis (what it's based on) → why THIS grade → what data would improve it, then
 * the citation. A card with no why block falls back to a one-line grade (+
 * confidence) restatement so the panel is never empty. The confidence figure
 * only appears when [showConfidence] (never on a safety hold, safety is a rule).
 * A block that speaks for several items passes their highest confidence as
 * [confidenceHigh] (and the lowest as [confidence]): the figure is then stated
 * once, as a range when they differ.
 */
@Composable
internal fun WhyDetail(
    why: WhyView?,
    evidence: EvidenceTagView,
    showConfidence: Boolean,
    extraDetail: String? = null,
    glossaryKeys: List<GlossaryTerm> = emptyList(),
    confidenceHigh: Float? = null,
) {
    val hasWhy = why != null &&
        (why.basis.isNotEmpty() || why.grade_note.isNotBlank() || why.improves.isNotBlank())
    Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
        // Grade badge (it opens the grading legend) + the CONTESTED marker.
        // SAFETY stays on the card face (HARD RULE), never here.
        Row(
            horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            GradeChip(evidence.grade)
            // Warn as TEXT: the per-theme WarnText token (the fixed warn fill fails
            // AA as text on most grounds).
            if (evidence.contested) OutlinedChip(stringResource(Res.string.main_chip_contested), WarnText)
        }
        if (hasWhy) {
            // 1. What this is based on.
            if (why.basis.isNotEmpty()) {
                WhyLine(stringResource(Res.string.main_why_based_on), why.basis.joinToString(" "))
            }
            // 2. Why THIS grade (+ confidence figure when the card shows one).
            val gradeLine = why.grade_note
            if (gradeLine.isNotBlank()) {
                val low = (evidence.confidence * 100).roundOrZero()
                val high = confidenceHigh?.let { (it * 100).roundOrZero() } ?: low
                val gradeLineFull = when {
                    !showConfidence -> gradeLine
                    high != low -> stringResource(Res.string.main_grade_line_confidence_range, gradeLine, low, high)
                    else -> stringResource(Res.string.main_grade_line_confidence, gradeLine, low)
                }
                WhyLine(stringResource(Res.string.main_why_this_grade), gradeLineFull)
            }
            // 3. What would improve it, the engagement loop. Skipped when the
            //    core reports nothing would ("-").
            val improves = why.improves.trim()
            if (improves.isNotBlank() && improves != "-") {
                WhyLine(stringResource(Res.string.main_why_to_improve), improves)
            }
        } else {
            // No why block: the one-line grade restatement. Hidden for an
            // unmapped grade rather than leaking a raw Debug string.
            gradeLabel(evidence.grade)?.let { label ->
                Text(
                    if (showConfidence) {
                        stringResource(Res.string.main_evidence_confidence, label, (evidence.confidence * 100).roundOrZero())
                    } else {
                        stringResource(Res.string.main_evidence_label, label)
                    },
                    color = OnBgMuted,
                    style = Type.Caption.merge(TabularFigures),
                )
            }
        }
        Text(citationLabel(evidence.citation), color = OnBgFaint, style = Type.Caption)
        extraDetail?.takeIf { it.isNotBlank() }?.let {
            Text(it, color = OnBgMuted, style = Type.Caption)
        }
        // The core composes a contested grade note whenever it has one (it names
        // the open question); only a card with no grade note needs the generic
        // explainer.
        if (evidence.contested && why?.grade_note.isNullOrBlank()) {
            Text(
                stringResource(Res.string.main_contested_explainer),
                color = OnBgMuted,
                style = Type.Caption,
            )
        }
        if (glossaryKeys.isNotEmpty()) GlossaryTermLinks(glossaryKeys)
    }
}

/** One labelled line of the why? disclosure: a small accent label + body copy. */
@Composable
private fun WhyLine(label: String, body: String) {
    Column(verticalArrangement = Arrangement.spacedBy(1.dp)) {
        Text(label, color = Accent, style = Type.Chip)
        Text(body, color = OnBgMuted, style = Type.Caption)
    }
}

/** The evidence-grade legend, opened by tapping any grade badge. */
val LocalEvidenceLegend = staticCompositionLocalOf<() -> Unit> { {} }

/**
 * Grade badge that opens the "How evidence grading works" legend when tapped. The
 * badge IS the link (a trailing chevron inside it says "opens something"), so the
 * "?" panel never nests a second "?" bubble. The badge color/label still key off
 * the raw wire grade. Compact visual; the overlay carries the ≥48dp target and the
 * spoken label ("Moderate. How evidence grading works").
 */
@Composable
internal fun GradeChip(grade: EvidenceGrade?) {
    // A row that asserts no claim has no grade, and no badge.
    val label = gradeLabel(grade)?.uppercase() ?: return
    val status = LocalStatusColors.current
    val openLegend = LocalEvidenceLegend.current
    val legendDesc = stringResource(Res.string.main_evidence_legend_title)
    Box(contentAlignment = Alignment.Center) {
        Row(
            modifier = Modifier
                .clip(SegmentShape)
                .background(status.gradeColor(grade))
                .padding(start = Space.Md.dp, end = Space.Sm.dp, top = Space.Sm.dp - 1.dp, bottom = Space.Sm.dp - 1.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Space.Xs.dp),
        ) {
            Text(label, color = Color.White, style = Type.Chip)
            RowChevron(tint = Color.White)
        }
        Box(
            Modifier
                .matchParentSize()
                .requiredHeight(ActionMinHeight)
                .clip(RoundedCornerShape(Space.Md.dp))
                .clickable(role = Role.Button, onClick = openLegend)
                .semantics { contentDescription = legendDesc },
        )
    }
}

/** Outlined variant of [Chip]: same footprint but a border and transparent
 *  fill, for the CONTESTED evidence-quality marker. An outline reads as
 *  "flagged, not asserted", so it never competes visually with a filled SAFETY
 *  chip; the filled grade badge keeps its solid style. */
@Composable
private fun OutlinedChip(text: String, border: Color) {
    Text(
        text,
        color = border,
        style = Type.Chip,
        modifier = Modifier
            .border(1.dp, border, SegmentShape)
            .chipPadding(),
    )
}

/** The grade badges, CONTESTED marker and citations of the findings one "?" speaks
 *  for, marked the same way as [WhyDetail]. */
@Composable
@OptIn(ExperimentalLayoutApi::class)
internal fun ClaimMarkers(grades: List<EvidenceGrade>, contested: Boolean, citations: List<String>) {
    if (grades.isNotEmpty() || contested) {
        FlowRow(horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
            grades.forEach { GradeChip(it) }
            if (contested) OutlinedChip(stringResource(Res.string.main_chip_contested), WarnText)
        }
    }
    citations.forEach { Text(citationLabel(it), color = OnBgFaint, style = Type.Caption) }
}

/**
 * Static "How evidence grading works" reference sheet: the core's five grade
 * definitions ([definitions]), then the SAFETY / CONTESTED markers. No coaching
 * logic.
 */
@Composable
internal fun EvidenceLegendSheet(definitions: List<GradeDefView>) {
    val status = LocalStatusColors.current
    Column(
        modifier = Modifier.sheetBody(rememberScrollState(), Space.ReadingSheet.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        SheetTitle(stringResource(Res.string.main_evidence_legend_title))
        Text(
            stringResource(Res.string.main_evidence_legend_body),
            color = OnBgMuted,
            style = Type.Body,
        )
        definitions.forEach { d ->
            val label = gradeLabel(d.grade)
            if (label != null) {
                LegendRow(
                    label,
                    status.gradeColor(d.grade),
                    d.definition + stringResource(Res.string.main_confidence_paren, (d.confidence * 100).roundOrZero()),
                )
            }
        }
        HorizontalDivider(color = Hairline)
        // The two markers share the grade rows' chip column so every body line
        // starts at the same edge.
        LegendRow(stringResource(Res.string.main_chip_safety), status.danger, stringResource(Res.string.main_legend_safety))
        LegendRow(stringResource(Res.string.main_chip_contested), status.warn, stringResource(Res.string.main_legend_contested))
    }
}

// ── Glossary ─────────────────────────────────────────────────────────────────
// One app-wide bottom sheet defining the app's jargon, reachable from tappable
// term chips wherever those terms appear. These are UI copy, plain-language
// definitions of terminology, NOT knowledge-base training claims, so they live
// as a static shell resource (HARD RULE 1 concerns training claims; a
// definition of "RPE" asserts none).

/** One glossary entry: the core's term (its wire name is the key the sheet
 *  opens at) and the string resources for its display term and plain
 *  definition (resolved at the @Composable call sites). */
private data class GlossaryEntry(val term: GlossaryTerm, val termRes: StringResource, val defRes: StringResource) {
    val key: String get() = term.wire
}

private val GLOSSARY: List<GlossaryEntry> = listOf(
    GlossaryEntry(GlossaryTerm.E1rm, Res.string.main_glossary_e1rm_term, Res.string.main_glossary_e1rm_def),
    GlossaryEntry(GlossaryTerm.OneRm, Res.string.main_glossary_1rm_term, Res.string.main_glossary_1rm_def),
    GlossaryEntry(GlossaryTerm.Rpe, Res.string.main_glossary_rpe_term, Res.string.main_glossary_rpe_def),
    GlossaryEntry(GlossaryTerm.Rir, Res.string.main_glossary_rir_term, Res.string.main_glossary_rir_def),
    GlossaryEntry(GlossaryTerm.ZScore, Res.string.main_glossary_zscore_term, Res.string.main_glossary_zscore_def),
    GlossaryEntry(GlossaryTerm.Vdot, Res.string.main_glossary_vdot_term, Res.string.main_glossary_vdot_def),
    GlossaryEntry(GlossaryTerm.Decoupling, Res.string.main_glossary_decoupling_term, Res.string.main_glossary_decoupling_def),
    GlossaryEntry(GlossaryTerm.VariabilityIndex, Res.string.main_glossary_vi_term, Res.string.main_glossary_vi_def),
    GlossaryEntry(GlossaryTerm.Tonnage, Res.string.main_glossary_tonnage_term, Res.string.main_glossary_tonnage_def),
    GlossaryEntry(GlossaryTerm.Spike, Res.string.main_glossary_spike_term, Res.string.main_glossary_spike_def),
    GlossaryEntry(GlossaryTerm.HrMax, Res.string.main_glossary_hrmax_term, Res.string.main_glossary_hrmax_def),
    GlossaryEntry(GlossaryTerm.Deload, Res.string.main_glossary_deload_term, Res.string.main_glossary_deload_def),
    GlossaryEntry(GlossaryTerm.Mesocycle, Res.string.main_glossary_mesocycle_term, Res.string.main_glossary_mesocycle_def),
    GlossaryEntry(GlossaryTerm.Ctl, Res.string.main_glossary_ctl_term, Res.string.main_glossary_ctl_def),
    GlossaryEntry(GlossaryTerm.Trimp, Res.string.main_glossary_trimp_term, Res.string.main_glossary_trimp_def),
    GlossaryEntry(GlossaryTerm.HrZones, Res.string.main_glossary_hrzones_term, Res.string.main_glossary_hrzones_def),
    GlossaryEntry(GlossaryTerm.Volume, Res.string.main_glossary_volume_term, Res.string.main_glossary_volume_def),
    GlossaryEntry(GlossaryTerm.Maf, Res.string.main_glossary_maf_term, Res.string.main_glossary_maf_def),
    GlossaryEntry(GlossaryTerm.Pap, Res.string.main_glossary_pap_term, Res.string.main_glossary_pap_def),
    GlossaryEntry(GlossaryTerm.RedS, Res.string.main_glossary_reds_term, Res.string.main_glossary_reds_def),
)

/** The terms the core marked in a text, each once, in glossary order: the term
 *  links inside a card's "?" panel, so a bare "RIR 3" / "85% e1RM" on the face
 *  always has a path to its meaning. */
internal fun List<GlossarySpan>.terms(): List<GlossaryTerm> = map { it.term }.distinct().sortedBy { it.ordinal }

/** Opens the glossary sheet at a given term key. Provided by the app scaffold. */
val LocalGlossary = staticCompositionLocalOf<(String) -> Unit> { {} }

/**
 * The glossary affordance: the app's one "?" glyph, compact, for placing
 * next to an already-labelled value (the e1RM line, the tonnage tile) without
 * repeating the term. Opens the glossary at `key`; spoken as "Explain <term>".
 * One per term per card or screen: a term that repeats down a list carries it on
 * its first occurrence only, and inside an [EvidenceCard] the term moves into the
 * "?" panel instead (one "?" per card).
 */
@Composable
internal fun GlossaryInfo(key: String) {
    val open = LocalGlossary.current
    val term = GLOSSARY.firstOrNull { it.key == key }?.let { stringResource(it.termRes) } ?: key
    val explainDesc = stringResource(Res.string.main_explain, term)
    // A built-in gap so the disc never touches the value it follows.
    Box(Modifier.padding(start = Space.Xs.dp)) { InlineDisclosureLink(explainDesc) { open(key) } }
}

/**
 * The "Terms" row inside a "?" panel: one [TextAction] per jargon term the card
 * uses, each opening the glossary at that term (≥48dp targets; spoken "Explain
 * RIR"). Text actions, not outlined pills: one action style app-wide.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun GlossaryTermLinks(terms: List<GlossaryTerm>) {
    val open = LocalGlossary.current
    Column(verticalArrangement = Arrangement.spacedBy(1.dp)) {
        Text(stringResource(Res.string.main_why_terms), color = Accent, style = Type.Chip)
        // Offset so the first term's label (not its touch padding) lines up with
        // the panel's text edge.
        FlowRow(Modifier.offset(x = -TextActionInset)) {
            terms.forEach { t ->
                val entry = GLOSSARY.firstOrNull { it.term == t } ?: return@forEach
                val term = stringResource(entry.termRes)
                TextAction(
                    term,
                    onClick = { open(entry.key) },
                    description = stringResource(Res.string.main_explain, term),
                    chevron = true,
                )
            }
        }
    }
}

/** One resolved glossary row: its key, display term and definition. */
private data class GlossaryRow(val key: String, val term: String, val definition: String)

/** Section letter of a term: its first letter, uppercased; "#" for a term that
 *  opens on a digit or symbol ("1RM / %1RM"). */
private fun glossaryLetter(term: String): String =
    term.firstOrNull()?.takeIf { it.isLetter() }?.uppercaseChar()?.toString() ?: "#"

/**
 * The glossary bottom sheet: the title and a search well pinned
 * above an alphabetical list, one [SectionOverline] per first letter, each term in
 * Bold over its definition. Typing filters on the term and the definition; a query
 * that matches nothing says so. Opens scrolled to `initialKey`, which keeps a ring.
 */
@Composable
internal fun GlossarySheet(initialKey: String) {
    var query by rememberSaveable { mutableStateOf("") }
    val rows = GLOSSARY.map { GlossaryRow(it.key, stringResource(it.termRes), stringResource(it.defRes)) }
        .sortedWith(compareBy(String.CASE_INSENSITIVE_ORDER) { it.term.trimStart('%') })
    val needle = query.trim()
    val shown = if (needle.isEmpty()) {
        rows
    } else {
        rows.filter { it.term.contains(needle, ignoreCase = true) || it.definition.contains(needle, ignoreCase = true) }
    }
    val groups = shown.groupBy { glossaryLetter(it.term) }
    val listState = rememberLazyListState()
    LaunchedEffect(initialKey) {
        // Flattened position of the term: every group before it adds its header.
        var index = 0
        for ((_, entries) in groups) {
            val at = entries.indexOfFirst { it.key == initialKey }
            if (at >= 0) {
                listState.scrollToItem(index + 1 + at)
                break
            }
            index += 1 + entries.size
        }
    }
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .navigationBarsPadding()
            .padding(horizontal = Space.ReadingSheet.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        SheetTitle(stringResource(Res.string.main_glossary_title))
        SearchWell(
            query,
            { query = it },
            hint = stringResource(Res.string.main_glossary_search_hint),
            testTag = "glossary-search",
            imeAction = ImeAction.Search,
            verticalPadding = Space.Md.dp,
        )
        if (shown.isEmpty()) {
            Text(
                stringResource(Res.string.main_glossary_no_match, needle),
                color = OnBgMuted,
                style = Type.Body,
                modifier = Modifier.padding(vertical = Space.Md.dp),
            )
        }
        LazyColumn(
            state = listState,
            modifier = Modifier.fillMaxWidth().testTag("glossary-list"),
            contentPadding = PaddingValues(bottom = Space.Lg.dp),
            verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
        ) {
            groups.forEach { (letter, entries) ->
                item(key = "letter-$letter") {
                    Box(Modifier.padding(top = Space.Md.dp)) { SectionOverline(letter) }
                }
                items(entries, key = { it.key }) { e -> GlossaryTerm(e, highlighted = e.key == initialKey) }
            }
        }
    }
}

/** One glossary term: the term in Bold over its plain definition. The term the
 *  sheet was opened for keeps a quiet accent ring (no fill). */
@Composable
private fun GlossaryTerm(row: GlossaryRow, highlighted: Boolean) {
    val shape = RoundedCornerShape(Space.Card.dp)
    Column(
        verticalArrangement = Arrangement.spacedBy(Space.Xs.dp),
        modifier = Modifier
            .fillMaxWidth()
            .then(if (highlighted) Modifier.border(1.dp, Accent.copy(alpha = 0.5f), shape) else Modifier)
            .padding(Space.Md.dp)
            .semantics(mergeDescendants = true) {},
    ) {
        Text(row.term, color = OnBgBody, style = Type.BodyBold)
        Text(row.definition, color = OnBgMuted, style = Type.Body)
    }
}

/** One legend row: the chip in a fixed-width column, then its plain definition. */
@Composable
private fun LegendRow(label: String, chipColor: Color, body: String) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        Box(modifier = Modifier.width(96.dp)) {
            Chip(label.uppercase(), chipColor)
        }
        Text(body, color = OnBgMuted, style = Type.Caption, modifier = Modifier.weight(1f))
    }
}

/** Human label for an evidence grade, or null for none (callers hide the
 *  chip/row). */
@Composable
internal fun gradeLabel(grade: EvidenceGrade?): String? = gradeLabelRes(grade)?.let { stringResource(it) }

/** String resource of [gradeLabel]: the same labels the legend rows use. */
private fun gradeLabelRes(grade: EvidenceGrade?): StringResource? = when (grade) {
    EvidenceGrade.Strong -> Res.string.main_grade_strong
    EvidenceGrade.Moderate -> Res.string.main_grade_moderate
    EvidenceGrade.Weak -> Res.string.main_grade_weak
    EvidenceGrade.ExpertOpinion -> Res.string.main_grade_expert
    EvidenceGrade.MarketingMyth -> Res.string.main_grade_myth
    null -> null
}

/** Grade badge color from the theme-fixed status palette. See design spec §2. */
fun StatusColors.gradeColor(grade: EvidenceGrade?): Color = when (grade) {
    EvidenceGrade.Strong -> evidenceStrong
    EvidenceGrade.Moderate -> evidenceModerate
    EvidenceGrade.Weak -> evidenceWeak
    EvidenceGrade.ExpertOpinion -> evidenceExpert
    EvidenceGrade.MarketingMyth, null -> evidenceUnknown
}

