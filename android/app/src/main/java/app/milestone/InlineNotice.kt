package app.milestone

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.dp

/**
 * A small in-form caution (ui-sweep §0.6): a 20dp warning glyph in
 * [StatusColors.warnMark] and a `Type.Caption` `OnBgBody` line on the bare
 * ground, with a trailing [TextAction] when there is an action. No card, no fill,
 * no stripe.
 */
@Composable
internal fun InlineNotice(
    text: String,
    modifier: Modifier = Modifier,
    actionLabel: String? = null,
    onAction: (() -> Unit)? = null,
) {
    Row(
        modifier = modifier.fillMaxWidth(),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        Icon(
            painterResource(R.drawable.ic_safety_warning_triangle),
            contentDescription = null,
            tint = LocalStatusColors.current.warnMark,
            modifier = Modifier.size(20.dp),
        )
        Text(text, color = OnBgBody, style = Type.Caption, modifier = Modifier.weight(1f))
        if (actionLabel != null && onAction != null) TextAction(actionLabel, onAction)
    }
}
