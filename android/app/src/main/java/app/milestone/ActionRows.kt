package app.milestone

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.painter.Painter
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp

/** One row of an [ActionRows] block; it opens something, so it wears a chevron. */
internal data class ActionRowItem(
    val label: String,
    val icon: Painter,
    val onClick: () -> Unit,
    val testTag: String? = null,
)

/** The destructive last row of an [ActionRows] block: it always confirms first
 *  ([ClearConfirmDialog]). */
internal data class DestructiveAction(
    val label: String,
    val icon: Painter,
    val confirmTitle: String,
    val confirmMessage: String,
    val confirmLabel: String,
    val onConfirm: () -> Unit,
    val testTag: String? = null,
)

/**
 * The footer of every detail sheet (ui-sweep §0.5): a [PlainCard] of full-width
 * ≥56dp rows, each a 24dp `OnBgMuted` icon, a `Type.Body` label and a chevron when
 * it opens something, separated by `OnBgBody @8%` hairlines. The [destructive]
 * row comes last in `DangerText`, without a chevron, and confirms first.
 */
@Composable
internal fun ActionRows(
    rows: List<ActionRowItem>,
    destructive: DestructiveAction? = null,
) {
    var confirming by rememberSaveable { mutableStateOf(false) }
    PlainCard(contentPadding = PaddingValues(0.dp), spacing = 0.dp) {
        rows.forEachIndexed { i, row ->
            if (i > 0) HorizontalDivider(color = Hairline)
            ActionRowLine(row.label, row.icon, destructive = false, row.testTag, row.onClick)
        }
        destructive?.let { d ->
            if (rows.isNotEmpty()) HorizontalDivider(color = Hairline)
            ActionRowLine(d.label, d.icon, destructive = true, d.testTag) { confirming = true }
        }
    }
    destructive?.let { d ->
        ClearConfirmDialog(
            visible = confirming,
            title = d.confirmTitle,
            message = d.confirmMessage,
            confirmLabel = d.confirmLabel,
            onDismiss = { confirming = false },
            onClear = d.onConfirm,
        )
    }
}

@Composable
private fun ActionRowLine(
    label: String,
    icon: Painter,
    // DangerText icon and label, no chevron (it confirms first).
    destructive: Boolean,
    tag: String?,
    onClick: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = LargeControlHeight)
            .clickable(role = Role.Button, onClick = onClick)
            .then(if (tag != null) Modifier.testTag(tag) else Modifier)
            .padding(horizontal = Space.Card.dp, vertical = Space.Md.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.Card.dp),
    ) {
        Icon(icon, contentDescription = null, tint = if (destructive) DangerText else OnBgMuted, modifier = Modifier.size(24.dp))
        Text(label, color = if (destructive) DangerText else OnBgBody, style = Type.Body, modifier = Modifier.weight(1f))
        if (!destructive) RowChevron()
    }
}
