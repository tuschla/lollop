package app.milestone

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.painter.Painter
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.dp
import androidx.compose.ui.util.fastMap
import java.util.Locale

/** The tone of a [StatusCard] (ui-sweep §0.3): which ring, mark tile and overline colour it wears. */
enum class StatusTone { Info, Adjusted, Danger }

/** The colours a [StatusTone] wears: the [ring], the mark tile's [tileFill], the
 *  [glyph] tint inside it and the [text] (overline) colour. The one source for
 *  [StatusCard] and the Log chooser's Report pain row (Danger, ui-sweep §0.3). */
internal class StatusToneColors(val ring: Color, val tileFill: Color, val glyph: Color, val text: Color)

@Composable
internal fun StatusTone.colors(): StatusToneColors {
    val status = LocalStatusColors.current
    return when (this) {
        StatusTone.Info -> StatusToneColors(ring = Accent, tileFill = AccentTint, glyph = Accent, text = Accent)
        // The tile is the on-ground warn mark (≥3:1 on every card ground; the
        // fixed warn fill measures 2.6:1 on Meadow-dark's card). On the
        // light grounds that is the warn fill with a white glyph (4.9:1); on the
        // dark ones the lighter ochre takes the ground's own ink instead.
        StatusTone.Adjusted -> StatusToneColors(
            ring = status.warnMark,
            tileFill = status.warnMark,
            glyph = if (status.warnMark == status.warn) Color.White else BgTop,
            text = WarnText,
        )
        StatusTone.Danger -> StatusToneColors(
            ring = DangerText,
            tileFill = status.danger.copy(alpha = DangerTileAlpha),
            glyph = DangerText,
            text = DangerText,
        )
    }
}

/** Size of a [StatusCard]'s solid mark tile. */
private val StatusMarkSize = 40.dp

/** Size of the glyph inside a [StatusCard]'s mark tile. */
private val StatusGlyphSize = 22.dp

/**
 * The one card for "this is not normal" (ui-sweep §0.3): the theme's own card
 * surface (`BgElevated`, [Space.Card] radius and padding) with no fill tint and
 * no stripe. Colour lives in exactly two places, a [StatusRingWidth] ring and the
 * solid 40dp mark tile, so a small solid object carries the hue and the card can
 * never read as beige.
 *
 * Header: mark tile ([glyph], else the tone's default), the [overline] in the
 * tone's colour, the red SAFETY chip when [safety], then [trailing] (the "?"
 * disclosure) flush to the end. [content] starts [Space.Md] below at the card's
 * content start, full width. [footer] (usually [StatusCardFooter]) closes it.
 * The card is one semantics group: overline first, then the body.
 */
@Composable
internal fun StatusCard(
    tone: StatusTone,
    overline: String,
    modifier: Modifier = Modifier,
    glyph: Painter? = null,
    markDescription: String? = null,
    safety: Boolean = false,
    trailing: (@Composable () -> Unit)? = null,
    footer: (@Composable () -> Unit)? = null,
    content: @Composable ColumnScope.() -> Unit,
) {
    val status = LocalStatusColors.current
    val shape = RoundedCornerShape(Space.Card.dp)
    val colors = tone.colors()
    val icon = glyph ?: painterResource(
        when {
            safety -> R.drawable.ic_safety_shield
            tone == StatusTone.Info -> R.drawable.ic_safety_info
            else -> R.drawable.ic_safety_warning_triangle
        },
    )
    val mark = markDescription ?: stringResource(
        when {
            safety || tone == StatusTone.Danger -> R.string.main_status_mark_safety
            tone == StatusTone.Adjusted -> R.string.main_status_mark_adjusted
            else -> R.string.main_status_mark_info
        },
    )
    Column(
        modifier = modifier
            .fillMaxWidth()
            .clip(shape)
            .background(BgElevated)
            .border(StatusRingWidth, colors.ring, shape)
            .padding(Space.Card.dp)
            .semantics(mergeDescendants = true) {},
        verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
        ) {
            Box(
                Modifier
                    .size(StatusMarkSize)
                    .clip(ChoiceCellShape)
                    .background(colors.tileFill)
                    .semantics { contentDescription = mark },
                contentAlignment = Alignment.Center,
            ) {
                Icon(icon, contentDescription = null, tint = colors.glyph, modifier = Modifier.size(StatusGlyphSize))
            }
            Text(
                overline.uppercase(Locale.US),
                color = colors.text,
                style = Type.Overline,
                modifier = Modifier.weight(1f),
            )
            if (safety) Chip(stringResource(R.string.main_chip_safety), status.danger)
            trailing?.let {
                // The "?" keeps its 48dp target; its visual disc ends flush with the content edge.
                Box(Modifier.offset(x = DisclosureEdgeInset)) { it() }
            }
        }
        content()
        footer?.invoke()
    }
}

/** Fill opacity of the Danger tone's mark tile (the Report pain row's icon tile). */
private const val DangerTileAlpha = 0.12f

/**
 * A [StatusCard]'s action footer (ui-sweep §5): a hairline, then the secondary
 * verb as a [TextAction] at the start (offset by `-TextActionInset`) and the main
 * verb as a content-sized [SecondaryButton] at the end. When the two do not fit
 * on one line (font scale) they stack: the button full width first, the text
 * action below it.
 */
@Composable
internal fun StatusCardFooter(
    primaryLabel: String?,
    onPrimary: () -> Unit,
    secondaryLabel: String? = null,
    onSecondary: () -> Unit = {},
) {
    if (primaryLabel == null && secondaryLabel == null) return
    Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
        HorizontalDivider(color = Hairline)
        val secondary: @Composable () -> Unit = {
            if (secondaryLabel != null) {
                TextAction(secondaryLabel, onSecondary, Modifier.offset(x = -TextActionInset))
            }
        }
        val primary: @Composable () -> Unit = {
            if (primaryLabel != null) SecondaryButton(primaryLabel, onPrimary)
        }
        FooterLayout(secondary, primary)
    }
}

/** One row (secondary at the start, primary at the end) when both fit at their
 *  natural width, else a column: primary full width, secondary below it. */
@Composable
private fun FooterLayout(secondary: @Composable () -> Unit, primary: @Composable () -> Unit) {
    Layout(contents = listOf(secondary, primary)) { (secondaryM, primaryM), constraints ->
        val gap = Space.Md.dp.roundToPx()
        val width = constraints.maxWidth
        val loose = Constraints(maxWidth = width)
        val sWidth = secondaryM.sumOf { it.maxIntrinsicWidth(Constraints.Infinity) }
        val pWidth = primaryM.sumOf { it.maxIntrinsicWidth(Constraints.Infinity) }
        val oneRow = sWidth + pWidth + (if (sWidth > 0 && pWidth > 0) gap else 0) <= width
        if (oneRow) {
            val s = secondaryM.fastMap { it.measure(loose) }
            val p = primaryM.fastMap { it.measure(loose) }
            val h = (s + p).maxOfOrNull { it.height } ?: 0
            layout(width, h) {
                s.forEach { it.place(0, (h - it.height) / 2) }
                p.forEach { it.place(width - it.width, (h - it.height) / 2) }
            }
        } else {
            val p = primaryM.fastMap { it.measure(Constraints.fixedWidth(width)) }
            val s = secondaryM.fastMap { it.measure(loose) }
            val pH = p.sumOf { it.height }
            val sH = s.sumOf { it.height }
            val h = pH + sH + (if (pH > 0 && sH > 0) gap else 0)
            layout(width, h) {
                var y = 0
                p.forEach { it.place(0, y); y += it.height }
                if (pH > 0) y += gap
                s.forEach { it.place(0, y); y += it.height }
            }
        }
    }
}

/**
 * The hero's status chip (ui-sweep §5 dedupe rule): "Adjusted" in Bold
 * [Type.Chip] white on a [StatusColors.warn] pill, 24dp visual, [ChoiceCellShape].
 * The adjustment prose lives in the [StatusCard] above; this only points at it.
 */
@Composable
internal fun AdjustedChip(modifier: Modifier = Modifier) {
    val desc = stringResource(R.string.main_adjusted_chip_cd)
    Box(
        modifier = modifier
            .heightIn(min = AdjustedChipHeight)
            .clip(ChoiceCellShape)
            .background(LocalStatusColors.current.warn)
            .clearAndSetSemantics { contentDescription = desc }
            .padding(horizontal = Space.Md.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            stringResource(R.string.main_adjusted_chip),
            color = Color.White,
            style = Type.Chip.copy(fontWeight = FontWeight.Bold),
            maxLines = 1,
        )
    }
}

private val AdjustedChipHeight = 24.dp
