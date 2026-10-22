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
import androidx.compose.foundation.layout.requiredSizeIn
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.foundation.layout.wrapContentSize
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Icon
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
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import java.util.Locale

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
        stringResource(R.string.main_hide_details)
    } else {
        stringResource(R.string.main_hide_details_subject, subject)
    }
    val detailsDesc = if (subject.isNullOrBlank()) {
        stringResource(R.string.main_details)
    } else {
        stringResource(R.string.main_details_subject, subject)
    }
    // ≥48dp touch target (accessibility) around the visual disc: an outer box
    // carries the clickable.
    Box(
        modifier = Modifier
            .size(ActionMinHeight)
            .clip(RoundedCornerShape(100))
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

/** Visual size of the "?" disc (it grows into a pill at a large font scale). */
private val DisclosureDiscSize = 24.dp

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
    val shape = RoundedCornerShape(100)
    Box(
        modifier = Modifier
            .sizeIn(minWidth = DisclosureDiscSize, minHeight = DisclosureDiscSize)
            .clip(shape)
            .then(if (filled) Modifier.background(tint) else Modifier.border(1.dp, tint, shape)),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            "?",
            color = if (filled) onTint else tint,
            style = Type.Chip.copy(fontWeight = FontWeight.Bold),
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
                .clip(RoundedCornerShape(100))
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
    grade: String,
    citation: String,
    confidence: Float,
    safetyCritical: Boolean,
    contested: Boolean,
    section: String? = null,
    confidenceInWhy: Boolean = true,
    container: Color = BgElevated,
    border: Color? = null,
    status: StatusTone? = null,
    statusOverline: String? = null,
    extraDetail: String? = null,
    why: WhyView? = null,
    // A jargon term the card names without spelling it out (e.g. "hrmax"): its
    // glossary link joins the term links inside the "?" panel, so the card face
    // never carries a second "?" next to the disclosure.
    glossaryKey: String? = null,
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
    // Styled face text (e.g. a bold exercise line over a regular prescription
    // line). Renders in place of [summary] on the face; [summary] stays the
    // disclosure-state key and the glossary-scan source.
    summaryRich: AnnotatedString? = null,
    // Card-owned detail rendered INSIDE the "?" panel, after the evidence lines
    // (e.g. a hold card's follow-up evidence), so the card keeps ONE "?".
    panelExtra: (@Composable () -> Unit)? = null,
    // Force the regular prose weight whatever the summary's length: a family of
    // sibling cards (Today's coaching cards) then wears one style.
    proseSummary: Boolean = false,
) {
    val statusColors = LocalStatusColors.current
    // Every "?" names what it explains ("Details: <subject>"): the caller's
    // subject, else the card's overline, else the summary's first clause.
    val disclosureSubject = subject ?: section ?: statusOverline ?: summary.substringBefore(". ").take(DisclosureSubjectChars)
    // Keyed on the card's identity (summary+citation), not composition position:
    // in an unkeyed/reordered list a positional saveable can re-attach an open
    // "?" panel to a DIFFERENT card after reorder. Keying
    // also resets the panel when a slot's content changes, correct, since the
    // disclosure belongs to the claim, not the slot.
    var expanded by rememberSaveable(summary, citation) { mutableStateOf(false) }
    // Jargon in the summary (RIR, RPE, e1RM, …) plus the caller's [glossaryKey]
    // get glossary links inside the "?" panel, so the face stays clean (ONE "?")
    // but every term has a path to its meaning.
    val panelGlossaryKeys = remember(summary, glossaryKey) {
        (listOfNotNull(glossaryKey) + glossaryKeysIn(summary)).distinct()
    }
    // Bold is a summary weight: a long coaching paragraph (or a sectioned
    // note under its own overline) reads as prose, so it drops to regular
    // weight and stops outweighing the card titles around it. A safety
    // card keeps bold (maximal emphasis), and a rich summary carries its
    // own spans over the bold base.
    val summaryStyle = if (
        summaryRich == null && !safetyCritical &&
        (proseSummary || section != null || summary.length > ProseSummaryChars)
    ) {
        Type.Body
    } else {
        Type.Body.copy(fontWeight = FontWeight.Bold)
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
                grade = grade,
                citation = citation,
                confidence = confidence,
                contested = contested,
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
            SummaryText(summary, summaryRich, summaryStyle, Modifier.fillMaxWidth())
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
                    SummaryText(summary, summaryRich, summaryStyle, Modifier.weight(1f))
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
                        if (safetyCritical) Chip(stringResource(R.string.main_chip_safety), statusColors.danger)
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
                    SummaryText(summary, summaryRich, summaryStyle, Modifier.fillMaxWidth())
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
                    stringResource(R.string.main_action_subject, actionLabel, it)
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

/** Summary length (chars) past which a face summary reads as prose: regular weight. */
private const val ProseSummaryChars = 90

/** Longest summary clause used as a "?" button's spoken subject. */
private const val DisclosureSubjectChars = 60

/** Height of the chip/section header row on a two-row EvidenceCard face. */
private val CompactHeaderHeight = 28.dp

/** A [DisclosureButton]'s 48dp target is wider than its disc: a trailing "?"
 *  shifts by this so the disc (not the target) ends at the content edge. */
internal val DisclosureEdgeInset = (ActionMinHeight - DisclosureDiscSize) / 2

/** The card's face summary: [rich] when the caller styled it, else plain [summary]. */
@Composable
private fun SummaryText(summary: String, rich: AnnotatedString?, style: TextStyle, modifier: Modifier) {
    if (rich != null) {
        Text(rich, modifier = modifier, color = OnBgBody, style = style)
    } else {
        Text(summary, modifier = modifier, color = OnBgBody, style = style)
    }
}

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
    grade: String,
    citation: String,
    confidence: Float,
    contested: Boolean,
    showConfidence: Boolean,
    extraDetail: String?,
    glossaryKeys: List<String> = emptyList(),
    confidenceHigh: Float? = null,
) {
    val hasWhy = why != null &&
        (why.basis.isNotBlank() || why.grade_note.isNotBlank() || why.improves.isNotBlank())
    Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
        // Grade badge (it opens the grading legend) + the CONTESTED marker.
        // SAFETY stays on the card face (HARD RULE), never here.
        Row(
            horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            GradeChip(grade)
            // Warn as TEXT: the per-theme WarnText token (the fixed warn fill fails
            // AA as text on most grounds).
            if (contested) OutlinedChip(stringResource(R.string.main_chip_contested), WarnText)
        }
        if (hasWhy) {
            // 1. What this is based on.
            if (why.basis.isNotBlank()) {
                WhyLine(stringResource(R.string.main_why_based_on), why.basis)
            }
            // 2. Why THIS grade (+ confidence figure when the card shows one).
            val gradeLine = why.grade_note.ifBlank {
                gradeLabel(grade)?.let { stringResource(R.string.main_evidence_grade, it) } ?: ""
            }
            if (gradeLine.isNotBlank()) {
                val low = Math.round(confidence * 100)
                val high = confidenceHigh?.let { Math.round(it * 100) } ?: low
                val gradeLineFull = when {
                    !showConfidence -> gradeLine
                    high != low -> stringResource(R.string.main_grade_line_confidence_range, gradeLine, low, high)
                    else -> stringResource(R.string.main_grade_line_confidence, gradeLine, low)
                }
                WhyLine(stringResource(R.string.main_why_this_grade), gradeLineFull)
            }
            // 3. What would improve it, the engagement loop. Skipped when the
            //    core reports nothing would ("-").
            val improves = why.improves.trim()
            if (improves.isNotBlank() && improves != "-") {
                WhyLine(stringResource(R.string.main_why_to_improve), improves)
            }
        } else {
            // No why block: the one-line grade restatement. Hidden for an
            // unmapped grade rather than leaking a raw Debug string.
            gradeLabel(grade)?.let { label ->
                Text(
                    if (showConfidence) {
                        stringResource(R.string.main_evidence_confidence, label, Math.round(confidence * 100))
                    } else {
                        stringResource(R.string.main_evidence_label, label)
                    },
                    color = OnBgMuted,
                    style = Type.Caption.merge(TabularFigures),
                )
            }
        }
        Text(citationLabel(citation), color = OnBgFaint, style = Type.Caption)
        // Internal KB claim ids ("(AUTOREG-RIR-001)") are engine bookkeeping, never
        // user copy: keep only the human clause of a core-composed detail line.
        extraDetail?.let(::stripClaimIds)?.takeIf { it.isNotBlank() }?.let {
            Text(it, color = OnBgMuted, style = Type.Caption)
        }
        // The core composes a contested grade note whenever it has one (it names
        // the open question); only a card with no grade note needs the generic
        // explainer.
        if (contested && why?.grade_note.isNullOrBlank()) {
            Text(
                stringResource(R.string.main_contested_explainer),
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
internal fun GradeChip(grade: String) {
    // Unmapped/future grades have no human label; hide the whole badge rather
    // than leak a raw Debug string.
    val label = gradeChipLabel(grade) ?: return
    val status = LocalStatusColors.current
    val openLegend = LocalEvidenceLegend.current
    val legendDesc = stringResource(R.string.main_evidence_legend_title)
    Box(contentAlignment = Alignment.Center) {
        Row(
            modifier = Modifier
                .clip(RoundedCornerShape(Space.Md.dp - Space.Xs.dp))
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
            .border(1.dp, border, RoundedCornerShape(Space.Md.dp - Space.Xs.dp))
            .padding(horizontal = Space.Md.dp, vertical = Space.Sm.dp - 1.dp),
    )
}

/**
 * Static "How evidence grading works" reference sheet. Explains the five
 * grades and the SAFETY / CONTESTED markers. No coaching logic, no wire data.
 */
@Composable
internal fun EvidenceLegendSheet(definitions: List<GradeDefView> = emptyList()) {
    val status = LocalStatusColors.current
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .verticalScroll(rememberScrollState())
            // Edge-to-edge sheet: the last row must clear the system nav bar.
            .navigationBarsPadding()
            .padding(horizontal = Space.ReadingSheet.dp)
            .padding(bottom = Space.Lg.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        Text(stringResource(R.string.main_evidence_legend_title), color = OnBgBody, style = Type.Title)
        Text(
            stringResource(R.string.main_evidence_legend_body),
            color = OnBgMuted,
            style = Type.Body,
        )
        // Definitions come from the core so the legend and the cards agree by
        // construction; shell copy covers a view that carries none.
        if (definitions.isNotEmpty()) {
            definitions.forEach { d ->
                val label = d.label.ifBlank { gradeLabel(d.grade) }
                if (label != null) {
                    LegendRow(
                        label,
                        status.gradeColor(d.grade),
                        d.definition + stringResource(R.string.main_confidence_paren, Math.round(d.confidence * 100)),
                    )
                }
            }
        } else {
            LegendRow(stringResource(R.string.main_grade_strong), status.gradeColor("Strong"), stringResource(R.string.main_grade_strong_def))
            LegendRow(stringResource(R.string.main_grade_moderate), status.gradeColor("Moderate"), stringResource(R.string.main_grade_moderate_def))
            LegendRow(stringResource(R.string.main_grade_weak), status.gradeColor("Weak"), stringResource(R.string.main_grade_weak_def))
            LegendRow(stringResource(R.string.main_grade_expert), status.gradeColor("ExpertOpinion"), stringResource(R.string.main_grade_expert_def))
            LegendRow(stringResource(R.string.main_grade_myth), status.gradeColor("MarketingMyth"), stringResource(R.string.main_grade_myth_def))
        }
        HorizontalDivider(color = Hairline)
        // The two markers share the grade rows' chip column so every body line
        // starts at the same edge.
        LegendRow(stringResource(R.string.main_chip_safety), status.danger, stringResource(R.string.main_legend_safety))
        LegendRow(stringResource(R.string.main_chip_contested), status.warn, stringResource(R.string.main_legend_contested))
    }
}

// ── Glossary ─────────────────────────────────────────────────────────────────
// One app-wide bottom sheet defining the app's jargon, reachable from tappable
// term chips wherever those terms appear. These are UI copy, plain-language
// definitions of terminology, NOT knowledge-base training claims, so they live
// as a static shell resource (HARD RULE 1 concerns training claims; a
// definition of "RPE" asserts none).

/** One glossary entry: a stable key, the string resources for its display
 *  term and plain definition (resolved at the @Composable call sites), and the
 *  pattern that spots the term in card copy (null = too generic a word to
 *  auto-detect; reachable only from an explicit [GlossaryInfo]). */
private data class GlossaryEntry(val key: String, val termRes: Int, val defRes: Int, val pattern: Regex?)

private val GLOSSARY: List<GlossaryEntry> = listOf(
    GlossaryEntry("e1rm", R.string.main_glossary_e1rm_term, R.string.main_glossary_e1rm_def, Regex("""\be1RM\b""", RegexOption.IGNORE_CASE)),
    GlossaryEntry("1rm", R.string.main_glossary_1rm_term, R.string.main_glossary_1rm_def, Regex("""(?<![A-Za-z0-9])1RM\b""")),
    GlossaryEntry("rpe", R.string.main_glossary_rpe_term, R.string.main_glossary_rpe_def, Regex("""\bRPE\b""")),
    GlossaryEntry("rir", R.string.main_glossary_rir_term, R.string.main_glossary_rir_def, Regex("""\bRIR\b""")),
    GlossaryEntry("zscore", R.string.main_glossary_zscore_term, R.string.main_glossary_zscore_def, Regex("""\bz-scores?\b""", RegexOption.IGNORE_CASE)),
    GlossaryEntry("vdot", R.string.main_glossary_vdot_term, R.string.main_glossary_vdot_def, Regex("""\bVDOT\b""")),
    GlossaryEntry("decoupling", R.string.main_glossary_decoupling_term, R.string.main_glossary_decoupling_def, Regex("""\bdecoupling\b""", RegexOption.IGNORE_CASE)),
    GlossaryEntry("vi", R.string.main_glossary_vi_term, R.string.main_glossary_vi_def, Regex("""\bvariability index\b""", RegexOption.IGNORE_CASE)),
    GlossaryEntry("tonnage", R.string.main_glossary_tonnage_term, R.string.main_glossary_tonnage_def, Regex("""\btonnage\b""", RegexOption.IGNORE_CASE)),
    GlossaryEntry("spike", R.string.main_glossary_spike_term, R.string.main_glossary_spike_def, null),
    GlossaryEntry("hrmax", R.string.main_glossary_hrmax_term, R.string.main_glossary_hrmax_def, Regex("""HRmax""")),
    GlossaryEntry("deload", R.string.main_glossary_deload_term, R.string.main_glossary_deload_def, Regex("""\bdeload""", RegexOption.IGNORE_CASE)),
    GlossaryEntry("mesocycle", R.string.main_glossary_mesocycle_term, R.string.main_glossary_mesocycle_def, Regex("""\bmesocycles?\b""", RegexOption.IGNORE_CASE)),
    GlossaryEntry("ctl", R.string.main_glossary_ctl_term, R.string.main_glossary_ctl_def, Regex("""\bCTL\b""")),
    GlossaryEntry("trimp", R.string.main_glossary_trimp_term, R.string.main_glossary_trimp_def, Regex("""\bTRIMP\b""")),
    GlossaryEntry("hrzones", R.string.main_glossary_hrzones_term, R.string.main_glossary_hrzones_def, Regex("""\bHR zones?\b""")),
    GlossaryEntry("volume", R.string.main_glossary_volume_term, R.string.main_glossary_volume_def, null),
    GlossaryEntry("maf", R.string.main_glossary_maf_term, R.string.main_glossary_maf_def, Regex("""\bMAF\b""")),
    GlossaryEntry("pap", R.string.main_glossary_pap_term, R.string.main_glossary_pap_def, Regex("""\bPAPE?\b""")),
    GlossaryEntry("reds", R.string.main_glossary_reds_term, R.string.main_glossary_reds_def, Regex("""\bRED-S\b""")),
)

/**
 * Glossary keys for the jargon terms that appear in [text], in glossary order,
 * each at most once. Drives the term links inside a card's "?" panel so a bare
 * "RIR 3" / "85% e1RM" on the face always has a path to its meaning.
 */
internal fun glossaryKeysIn(text: String): List<String> =
    GLOSSARY.filter { e -> e.pattern?.containsMatchIn(text) == true }.map { it.key }

// A parenthesised internal KB claim id, e.g. " (AUTOREG-RIR-001)" or
// " (SAFE-PAIN-001)", or the "(-)" placeholder for a missing id.
private val CLAIM_ID_PAREN = Regex("""\s*\((?:-|[A-Z][A-Z0-9]*(?:-[A-Z0-9]+)+)\)""")

/**
 * [text] with every parenthesised internal claim id removed. Claim ids are
 * engine bookkeeping, never user copy: "load −10%: readiness (AUTOREG-RIR-001)"
 * renders as "load −10%: readiness". Plain parentheticals ("(RPE 8)") survive.
 */
internal fun stripClaimIds(text: String): String = text.replace(CLAIM_ID_PAREN, "").trim()

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
    val explainDesc = stringResource(R.string.main_explain, term)
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
private fun GlossaryTermLinks(keys: List<String>) {
    val open = LocalGlossary.current
    Column(verticalArrangement = Arrangement.spacedBy(1.dp)) {
        Text(stringResource(R.string.main_why_terms), color = Accent, style = Type.Chip)
        // Offset so the first term's label (not its touch padding) lines up with
        // the panel's text edge.
        FlowRow(Modifier.offset(x = -TextActionInset)) {
            keys.forEach { key ->
                val entry = GLOSSARY.firstOrNull { it.key == key } ?: return@forEach
                val term = stringResource(entry.termRes)
                TextAction(
                    term,
                    onClick = { open(key) },
                    description = stringResource(R.string.main_explain, term),
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
 * The glossary bottom sheet (ui-sweep §12): the title and a search well pinned
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
        Text(stringResource(R.string.main_glossary_title), color = OnBgBody, style = Type.Title)
        GlossarySearchWell(query) { query = it }
        if (shown.isEmpty()) {
            Text(
                stringResource(R.string.main_glossary_no_match, needle),
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
        Text(row.term, color = OnBgBody, style = Type.Body.copy(fontWeight = FontWeight.Bold))
        Text(row.definition, color = OnBgMuted, style = Type.Body)
    }
}

/** The glossary's search well (the ui-sweep §8 well: `bgSunken`, a 1dp
 *  [ChoiceCellBorder], Accent while typing) with a leading search glyph and a
 *  faint "Search terms" placeholder. The keyboard's Search key folds it. */
@Composable
private fun GlossarySearchWell(value: String, onChange: (String) -> Unit) {
    val focusManager = LocalFocusManager.current
    var focused by remember { mutableStateOf(false) }
    val shape = RoundedCornerShape(Space.Card.dp)
    val hint = stringResource(R.string.main_glossary_search_hint)
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = ActionMinHeight)
            .clip(shape)
            .background(BgSunken)
            .border(if (focused) 1.5.dp else 1.dp, if (focused) Accent else ChoiceCellBorder, shape)
            .padding(horizontal = Space.Card.dp, vertical = Space.Md.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        Icon(
            painterResource(R.drawable.ic_ui_search),
            contentDescription = null,
            tint = OnBgFaint,
            modifier = Modifier.size(18.dp),
        )
        BasicTextField(
            value = value,
            onValueChange = onChange,
            singleLine = true,
            textStyle = Type.Body.copy(color = OnBgBody),
            cursorBrush = SolidColor(Accent),
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
            keyboardActions = KeyboardActions(onSearch = { focusManager.clearFocus() }),
            modifier = Modifier
                .weight(1f)
                .testTag("glossary-search")
                .semantics { contentDescription = hint }
                .onFocusChanged { focused = it.isFocused },
            decorationBox = { inner ->
                Box {
                    if (value.isEmpty()) Text(hint, color = OnBgFaint, style = Type.Body)
                    inner()
                }
            },
        )
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
            Chip(label.uppercase(Locale.US), chipColor)
        }
        Text(body, color = OnBgMuted, style = Type.Caption, modifier = Modifier.weight(1f))
    }
}

/** Grade-badge text: the human label, uppercased to match the other chips
 *  (SAFETY / CONTESTED / SPIKE). Display-only. */
@Composable
internal fun gradeChipLabel(grade: String): String? = gradeLabel(grade)?.uppercase(Locale.US)

/**
 * Human label for an evidence grade's wire name, or null for an unmapped grade
 * (callers hide the chip/row rather than leak a raw Debug string). Display-only -
 * badges, sorting, and colors still key off the raw wire value.
 */
@Composable
internal fun gradeLabel(grade: String): String? = gradeLabelRes(grade)?.let { stringResource(it) }

/** String resource of [gradeLabel]: the same labels the legend rows use. */
private fun gradeLabelRes(grade: String): Int? = when (grade) {
    "Strong" -> R.string.main_grade_strong
    "Moderate" -> R.string.main_grade_moderate
    "Weak" -> R.string.main_grade_weak
    "ExpertOpinion" -> R.string.main_grade_expert
    "MarketingMyth" -> R.string.main_grade_myth
    else -> null
}

/**
 * Human-readable citation line. Verbatim-render contract (design/user-decisions
 * "Evidence display"): the shell renders the CORE's citation string exactly as
 * emitted; it never paraphrases, renumbers, or collapses a distinction the core
 * drew. The registry emits real published references (evidence.rs
 * `primary_citations`); a blank citation is the only thing we touch, rendering
 * it as a dash.
 */
internal fun citationLabel(citation: String): String =
    citation.ifBlank { "-" }

