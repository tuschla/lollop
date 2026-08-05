package app.milestone

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.ui.graphics.luminance
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.border
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp

// Shared shell chrome (design/screens/00-chrome.md): brand top bar, three-slot
// bottom nav with pill selection, Log FAB content, run-in-progress chip, and
// the editor top-bar pattern. Pure presentation, every value here is a token
// or an exact dp from the chrome spec.

/** The three destinations, in nav order (chrome §1; owner ruling 2026-08-03:
 *  Coach merged into Today, one screen owns the day's call AND the plan). */
enum class Dest(val label: String, val iconSelected: Int, val iconUnselected: Int) {
    Today("Today", R.drawable.ic_nav_today_selected, R.drawable.ic_nav_today_unselected),
    History("History", R.drawable.ic_nav_history_selected, R.drawable.ic_nav_history_unselected),
    Profile("Profile", R.drawable.ic_nav_profile_selected, R.drawable.ic_nav_profile_unselected),
}

/**
 * Top app bar for the destinations (chrome §2): the brand lockup, route
 * mark 20dp + "milestone" wordmark, both `Accent` IS the title; no secondary
 * title string. A right overflow (⋮) holds only Clear all data (via [actions]).
 */
@Composable
fun BrandTopBar(actions: @Composable RowScope.() -> Unit = {}) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(BgTop)
            .height(44.dp)
            .padding(start = Space.Screen.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        Icon(
            painterResource(R.drawable.ic_brand_mark),
            contentDescription = null,
            tint = Accent,
            modifier = Modifier.size(20.dp),
        )
        Text(
            "milestone",
            color = Accent,
            style = Type.Title.copy(fontWeight = FontWeight.ExtraBold),
            modifier = Modifier.weight(1f),
        )
        actions()
    }
}

/** Overflow (⋮) button for the top bar. */
@Composable
fun OverflowButton(onClick: () -> Unit, content: @Composable () -> Unit) {
    Box {
        IconButton(onClick = onClick) {
            Icon(Icons.Filled.MoreVert, contentDescription = "More options", tint = OnBgMuted)
        }
        content()
    }
}

/**
 * Bottom navigation bar (chrome §1): three equal slots, icon pill above label.
 * Selected slot = `Accent @16%` pill (5×18dp padding, pill radius) + `Accent`
 * icon/label, label Bold; unselected = no pill, `OnBgFaint`, Regular. Only the
 * Today icon fills when selected (the asset pair encodes that).
 */
@Composable
fun MilestoneNavBar(selected: Dest, onSelect: (Dest) -> Unit) {
    // Derive "is dark" from the palette MilestoneTheme actually resolved, not
    // the OS setting, so the divider stays right when the DarkMode override
    // forces Dark on a light OS (or vice-versa). A dark ground has low luminance.
    val dark = LocalPalette.current.bgTop.luminance() < 0.5f
    Column(Modifier.fillMaxWidth().background(BgTop)) {
        HorizontalDivider(
            thickness = 1.dp,
            color = OnBgBody.copy(alpha = if (dark) 0.05f else 0.07f),
        )
        Row(
            modifier = Modifier
                .fillMaxWidth()
                // Real system navigation-bar inset, not a fixed 16dp. On 3-button
                // nav (e.g. Samsung) the bar is ~48dp tall; a hardcoded 16dp let
                // the buttons overflow into the nav row. navigationBarsPadding
                // pushes the row above whatever the device's nav bar actually is
                // (small gesture pill or tall button bar), edge-to-edge safe.
                .navigationBarsPadding()
                .padding(top = Space.Md.dp, bottom = Space.Md.dp)
                .padding(horizontal = 10.dp),
        ) {
            Dest.entries.forEach { dest ->
                val active = dest == selected
                Column(
                    modifier = Modifier
                        .weight(1f)
                        .heightIn(min = 48.dp)
                        .clip(RoundedCornerShape(Space.Card.dp))
                        .clickable { onSelect(dest) },
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(5.dp),
                ) {
                    Box(
                        modifier = Modifier
                            .clip(RoundedCornerShape(100))
                            .background(if (active) Accent.copy(alpha = 0.16f) else androidx.compose.ui.graphics.Color.Transparent)
                            .padding(horizontal = 18.dp, vertical = 5.dp),
                    ) {
                        Icon(
                            painterResource(if (active) dest.iconSelected else dest.iconUnselected),
                            contentDescription = dest.label,
                            tint = if (active) Accent else OnBgFaint,
                            modifier = Modifier.size(24.dp),
                        )
                    }
                    Text(
                        dest.label,
                        color = if (active) Accent else OnBgFaint,
                        style = if (active) Type.Chip else Type.Chip.copy(fontWeight = FontWeight.Normal),
                    )
                }
            }
        }
    }
}

/**
 * Run-in-progress chip (chrome §4): pinned just above the nav bar on every
 * destination while a run records. `BgElevated` fill, `1dp Accent @40%`
 * border, Radius Card; `content-run` 18dp + label + "Return ›" in `Accent`.
 */
@Composable
fun RunInProgressChip(onClick: () -> Unit) {
    Row(
        modifier = Modifier
            .padding(horizontal = Space.Screen.dp)
            .padding(bottom = Space.Md.dp)
            .fillMaxWidth()
            .clip(RoundedCornerShape(Space.Card.dp))
            .background(BgElevated)
            .border(1.dp, Accent.copy(alpha = 0.4f), RoundedCornerShape(Space.Card.dp))
            .clickable { onClick() }
            .padding(horizontal = Space.Card.dp, vertical = Space.Md.dp + Space.Xs.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        Icon(
            painterResource(R.drawable.ic_content_run),
            contentDescription = null,
            tint = Accent,
            modifier = Modifier.size(18.dp),
        )
        Text(
            "Run in progress",
            color = OnBgBody,
            style = Type.Body.copy(fontWeight = FontWeight.Bold),
            modifier = Modifier.weight(1f),
        )
        Text("Return", color = Accent, style = Type.Body)
        Icon(
            painterResource(R.drawable.ic_ui_chevron_right),
            contentDescription = null,
            tint = Accent,
            modifier = Modifier.size(16.dp),
        )
    }
}

/**
 * Editor top-bar pattern (chrome §2 / 04-profile): left `ui-close` 24dp
 * dismiss, centered screen title, right primary-action pill (`Accent` fill,
 * dark [OnAccent] text per the owner's AA ruling on accent fills).
 */
@Composable
fun EditorHeader(
    title: String,
    onClose: () -> Unit,
    actionLabel: String = "Save",
    actionEnabled: Boolean = true,
    onAction: () -> Unit,
) {
    Row(
        modifier = Modifier.fillMaxWidth().heightIn(min = 44.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(
            painterResource(R.drawable.ic_ui_close),
            contentDescription = "Close",
            tint = OnBgBody,
            modifier = Modifier
                .clip(RoundedCornerShape(Space.Md.dp))
                .clickable { onClose() }
                .padding(Space.Md.dp)
                .size(24.dp),
        )
        Text(
            title,
            color = OnBgBody,
            style = Type.Title,
            modifier = Modifier.weight(1f),
            textAlign = androidx.compose.ui.text.style.TextAlign.Center,
        )
        Text(
            actionLabel,
            color = if (actionEnabled) OnAccent else OnBgFaint,
            style = Type.Body.copy(fontWeight = FontWeight.Bold),
            modifier = Modifier
                .clip(RoundedCornerShape(100))
                .background(if (actionEnabled) Accent else BgTop)
                .clickable(enabled = actionEnabled) { onAction() }
                .padding(horizontal = Space.Card.dp, vertical = Space.Md.dp),
        )
    }
}
