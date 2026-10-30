package app.milestone

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/** The elevated content card (hairline outline, no shadow); with [onClick] the
 *  whole card is one tappable surface, a button to TalkBack. [contentPadding] and
 *  [spacing] default to the card recipe; edge-to-edge row lists ([ActionRows], a
 *  hairline grid) pass zero and draw their own row padding. */
@Composable
internal fun PlainCard(
    onClick: (() -> Unit)? = null,
    contentPadding: PaddingValues = PaddingValues(Space.Card.dp),
    spacing: Dp = Space.Md.dp,
    content: @Composable () -> Unit,
) {
    val border = BorderStroke(1.dp, OnBgBody.copy(alpha = HairlineAlpha.Card))
    val shape = RoundedCornerShape(Space.Card.dp)
    val colors = CardDefaults.cardColors(containerColor = BgElevated)
    val elevation = CardDefaults.cardElevation(defaultElevation = 0.dp)
    val body: @Composable () -> Unit = {
        Column(
            Modifier.padding(contentPadding),
            verticalArrangement = Arrangement.spacedBy(spacing),
        ) { content() }
    }
    // Clickable variant uses the Material 3 `Card(onClick=…)` overload so the tap ripple
    // is clipped by the card's own rounded surface at any height (no soft-edge
    // leak on tall/expanded cards); the static variant is a plain Card.
    if (onClick != null) {
        Card(
            onClick = onClick,
            colors = colors,
            elevation = elevation,
            shape = shape,
            border = border,
            // Material's clickable Card sets no role.
            modifier = Modifier.fillMaxWidth().semantics { role = Role.Button },
        ) { body() }
    } else {
        Card(
            colors = colors,
            elevation = elevation,
            shape = shape,
            border = border,
            modifier = Modifier.fillMaxWidth(),
        ) { body() }
    }
}

/** A small status chip: white [Type.Chip] text on the [bg] fill (SAFETY, SPIKE, ON HOLD…). */
@Composable
internal fun Chip(text: String, bg: Color) {
    Text(
        text,
        color = Color.White,
        style = Type.Chip,
        modifier = Modifier
            .background(bg, SegmentShape)
            .chipPadding(),
    )
}

/** Inner padding of a [Chip] or an outlined chip. */
internal fun Modifier.chipPadding(): Modifier = padding(horizontal = Space.Md.dp, vertical = Space.Sm.dp - 1.dp)

/**
 * One compact Reference row (02-coach §6): the claim title + its citation
 * line. Deliberately NOT an EvidenceCard, the full graded card already
 * rendered wherever the claim was recommended; Reference only lists sources.
 */
@Composable
internal fun ReferenceRow(row: GuidanceView) {
    Column(verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
        Text(
            guidanceTitle(row),
            color = OnBgBody,
            style = Type.BodyBold,
        )
        Text(citationLabel(row.citation), color = OnBgFaint, style = Type.Caption)
    }
}

/** A guidance row's title: the training-age row in the copy deck's grammar
 *  ("Training age · Intermediate"), every other row as [guidanceSummary]. */
@Composable
internal fun guidanceTitle(row: GuidanceView): String =
    if (row.training_age.isNotEmpty()) stringResource(R.string.main_training_age_row, row.training_age) else guidanceSummary(row)

/** A guidance row's summary in the user's distance unit: the core's miles twin
 *  (`summary_mi`) for a miles user when the row states a distance, else `summary`. */
@Composable
internal fun guidanceSummary(row: GuidanceView): String =
    if (rememberDistanceUnit() == DistanceUnit.Mi) row.summary_mi.ifBlank { row.summary } else row.summary

/** A confirmation [AlertDialog] (chrome §6): [title], an optional [body], a
 *  dismiss button ([dismissLabel]) and one confirm button in [confirmColor]
 *  (`DangerText` when destructive, else the button's own colour), disabled while
 *  not [confirmEnabled]. Each button runs exactly its own callback. */
@Composable
internal fun ConfirmDialog(
    title: String,
    body: String?,
    confirmLabel: String,
    onDismiss: () -> Unit,
    onConfirm: () -> Unit,
    confirmColor: Color = Color.Unspecified,
    confirmEnabled: Boolean = true,
    dismissLabel: String = stringResource(R.string.main_cancel),
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        modifier = Modifier.windowTraversalGroup(),
        shape = RoundedCornerShape(Space.Card.dp),
        title = { Text(title) },
        text = body?.let { { Text(it) } },
        confirmButton = {
            TextButton(onClick = onConfirm, enabled = confirmEnabled) { Text(confirmLabel, color = confirmColor) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(dismissLabel) } },
    )
}

/** The irreversible-clear confirmation (remove a plan or report, a hold
 *  resolution): a [ConfirmDialog] whose confirm closes it, then clears.
 *  [confirmColor] is `DangerText` unless the confirm is not destructive. */
@Composable
internal fun ClearConfirmDialog(
    visible: Boolean,
    title: String,
    message: String,
    confirmLabel: String = stringResource(R.string.main_clear),
    confirmColor: Color = DangerText,
    onDismiss: () -> Unit,
    onClear: () -> Unit,
) {
    if (visible) {
        ConfirmDialog(
            title = title,
            body = message,
            confirmLabel = confirmLabel,
            onDismiss = onDismiss,
            onConfirm = {
                onDismiss()
                onClear()
            },
            confirmColor = confirmColor,
        )
    }
}

/** One ≥48dp key/value row (set detail facts, calculator results): the label
 *  muted at the start, the value at the end, Bold when [emphasis]. Each takes its
 *  own share of the line, so a large font scale wraps them in place rather than
 *  overlapping. One semantics node, read label then value. */
@Composable
internal fun KeyValueRow(label: String, value: String, emphasis: Boolean = false) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = ActionMinHeight)
            .padding(vertical = Space.Sm.dp)
            .semantics(mergeDescendants = true) {},
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        Text(label, color = OnBgMuted, style = Type.Body, modifier = Modifier.weight(1f))
        Text(
            value,
            color = OnBgBody,
            style = (if (emphasis) Type.BodyBold else Type.Body).merge(TabularFigures),
            textAlign = TextAlign.End,
            modifier = Modifier.weight(1f),
        )
    }
}
