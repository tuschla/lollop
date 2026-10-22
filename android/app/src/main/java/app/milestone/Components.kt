package app.milestone

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
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
import androidx.compose.material3.minimumInteractiveComponentSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import java.util.Locale

/** The elevated content card (hairline outline, no shadow); with [onClick] the
 *  whole card is one tappable surface. [contentPadding] and [spacing] default to
 *  the card recipe; edge-to-edge row lists ([ActionRows], a hairline grid) pass
 *  zero and draw their own row padding. */
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
            modifier = Modifier.fillMaxWidth(),
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
            .background(bg, RoundedCornerShape(Space.Md.dp - Space.Xs.dp))
            .padding(horizontal = Space.Md.dp, vertical = Space.Sm.dp - 1.dp),
    )
}

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
            style = Type.Body.copy(fontWeight = FontWeight.Bold),
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

/** The irreversible-clear confirmation [AlertDialog] (chrome §6), shared by every
 *  destructive confirm (remove plan or report, delete an entry, discard a run). */
@Composable
internal fun ClearConfirmDialog(
    visible: Boolean,
    title: String = stringResource(R.string.main_clear_confirm_title_default),
    message: String = stringResource(R.string.main_clear_confirm_message_default),
    confirmLabel: String = stringResource(R.string.main_clear),
    onDismiss: () -> Unit,
    onClear: () -> Unit,
) {
    if (visible) {
        AlertDialog(
            onDismissRequest = onDismiss,
            shape = RoundedCornerShape(Space.Card.dp),
            title = { Text(title) },
            text = { Text(message) },
            confirmButton = {
                TextButton(onClick = {
                    onDismiss()
                    onClear()
                }) { Text(confirmLabel, color = DangerText) }
            },
            dismissButton = {
                TextButton(onClick = onDismiss) { Text(stringResource(R.string.main_cancel)) }
            },
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
            style = (if (emphasis) Type.Body.copy(fontWeight = FontWeight.Bold) else Type.Body).merge(TabularFigures),
            textAlign = TextAlign.End,
            modifier = Modifier.weight(1f),
        )
    }
}
