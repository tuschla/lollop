package app.milestone

import android.os.Build
import androidx.compose.foundation.ScrollState
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.windowInsetsBottomHeight
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.NavigationRail
import androidx.compose.material3.NavigationRailItem
import androidx.compose.material3.NavigationRailItemDefaults
import androidx.compose.material3.SheetState
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.SideEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.isTraversalGroup
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.DialogWindowProvider

// Shared shell chrome (design/screens/00-chrome.md): brand top bar, three-slot
// bottom nav with pill selection and the run-in-progress chip. Pure
// presentation, every value here is a token or an exact dp from the chrome spec.

/** The three destinations, in nav order (chrome §1; Today owns both the day's
 *  call and the plan). */
enum class Dest(val labelRes: Int, val iconSelected: Int, val iconUnselected: Int) {
    Today(R.string.main_dest_today, R.drawable.ic_nav_today_selected, R.drawable.ic_nav_today_unselected),
    History(R.string.main_dest_history, R.drawable.ic_nav_history_selected, R.drawable.ic_nav_history_unselected),
    Profile(R.string.main_dest_profile, R.drawable.ic_nav_profile_selected, R.drawable.ic_nav_profile_unselected),
}

/** Bar height of [BrandTopBar] (chrome §2). */
private val BrandTopBarHeight = 44.dp

/** Size of the route mark in the brand lockup (chrome §2). */
private val BrandMarkSize = 20.dp

/** Opacity of the system navigation band under [MilestoneNavBar], on a dark and
 *  a light ground: a shade off the tab bar so the system buttons never read as
 *  a fourth app tab. */
private const val NavInsetBandAlphaDark = 0.06f
private const val NavInsetBandAlphaLight = 0.04f

/**
 * Top app bar for the destinations (chrome §2): the brand lockup, route
 * mark + `app_name` ("lollop") wordmark, both `Accent`, IS the title; no secondary
 * title string and no actions.
 */
@Composable
fun BrandTopBar() {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(BgTop)
            .height(BrandTopBarHeight)
            .padding(start = Space.Screen.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        Icon(
            painterResource(R.drawable.ic_brand_mark),
            contentDescription = null,
            tint = Accent,
            modifier = Modifier.size(BrandMarkSize),
        )
        Text(
            stringResource(R.string.app_name),
            color = Accent,
            style = Type.Title.copy(fontWeight = FontWeight.ExtraBold),
            modifier = Modifier.weight(1f),
        )
    }
}

/**
 * Bottom navigation bar (chrome §1): three equal slots, icon pill above label.
 * Selected slot = [AccentTint] pill (5×18dp padding, pill radius) + `Accent`
 * icon/label, label Bold; unselected = no pill, `OnBgFaint`, Regular. Only the
 * Today icon fills when selected (the asset pair encodes that).
 */
@Composable
fun MilestoneNavBar(selected: Dest, onSelect: (Dest) -> Unit) {
    // The palette MilestoneTheme actually resolved, not the OS setting, so the
    // divider stays right when the DarkMode override forces the other mode.
    val dark = LocalPalette.current.isDark
    Column(Modifier.fillMaxWidth().background(BgTop).testTag("bottom-nav")) {
        HorizontalDivider(
            thickness = 1.dp,
            color = OnBgBody.copy(alpha = if (dark) HairlineAlpha.Row else HairlineAlpha.Control),
        )
        Row(
            modifier = Modifier
                .fillMaxWidth()
                // An extra Sm under the labels keeps them visibly clear of the
                // system navigation bar below.
                .padding(top = Space.Md.dp, bottom = Space.Md.dp + Space.Sm.dp)
                .padding(horizontal = 10.dp),
        ) {
            Dest.entries.forEach { dest ->
                val active = dest == selected
                Column(
                    modifier = Modifier
                        .weight(1f)
                        .heightIn(min = ActionMinHeight)
                        .clip(RoundedCornerShape(Space.Card.dp))
                        // Tab role + the visible label naming the item; the icon's
                        // contentDescription is null so TalkBack announces the tab once.
                        .selectable(selected = active, role = Role.Tab, onClick = { onSelect(dest) }),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(5.dp),
                ) {
                    Box(
                        modifier = Modifier
                            .clip(PillShape)
                            .background(if (active) AccentTint else Color.Transparent)
                            .padding(horizontal = 18.dp, vertical = 5.dp),
                    ) {
                        Icon(
                            painterResource(if (active) dest.iconSelected else dest.iconUnselected),
                            contentDescription = null,
                            tint = if (active) Accent else OnBgFaint,
                            modifier = Modifier.size(24.dp),
                        )
                    }
                    Text(
                        stringResource(dest.labelRes),
                        color = if (active) Accent else OnBgFaint,
                        style = if (active) Type.Chip else Type.ChipRegular,
                    )
                }
            }
        }
        // The system navigation bar's own band: the real inset height (small
        // gesture pill or tall 3-button bar).
        Box(
            Modifier
                .fillMaxWidth()
                .windowInsetsBottomHeight(WindowInsets.navigationBars)
                .background(OnBgBody.copy(alpha = if (dark) NavInsetBandAlphaDark else NavInsetBandAlphaLight)),
        )
    }
}

/**
 * Side navigation rail: [MilestoneNavBar]'s three destinations (same icons,
 * labels and colours) down the start edge, with Log ([onLog], Today only, as the
 * FAB is) at its top. Used instead of the bottom bar and the Log FAB when they
 * would be laid over the pinned safety banner (a landscape phone under a hold):
 * beside the banner, never over it.
 */
@Composable
fun MilestoneNavRail(selected: Dest, onSelect: (Dest) -> Unit, onLog: (() -> Unit)?) {
    val logLabel = stringResource(R.string.main_log)
    NavigationRail(
        modifier = Modifier.testTag("nav-rail"),
        containerColor = BgTop,
        header = onLog?.let { log ->
            {
                FloatingActionButton(
                    onClick = log,
                    modifier = Modifier.testTag("log-fab").semantics { contentDescription = logLabel },
                    containerColor = Accent,
                    contentColor = OnAccent,
                    shape = RoundedCornerShape(Space.Card.dp),
                ) {
                    Icon(painterResource(R.drawable.ic_action_log_plus), contentDescription = null, modifier = Modifier.size(20.dp))
                }
            }
        },
    ) {
        Dest.entries.forEach { dest ->
            val active = dest == selected
            NavigationRailItem(
                selected = active,
                onClick = { onSelect(dest) },
                icon = {
                    Icon(
                        painterResource(if (active) dest.iconSelected else dest.iconUnselected),
                        contentDescription = null,
                        modifier = Modifier.size(24.dp),
                    )
                },
                label = {
                    Text(
                        stringResource(dest.labelRes),
                        style = if (active) Type.Chip else Type.ChipRegular,
                    )
                },
                colors = NavigationRailItemDefaults.colors(
                    selectedIconColor = Accent,
                    selectedTextColor = Accent,
                    indicatorColor = AccentTint,
                    unselectedIconColor = OnBgFaint,
                    unselectedTextColor = OnBgFaint,
                ),
            )
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
            .clickable(role = Role.Button) { onClick() }
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
            stringResource(R.string.main_run_in_progress),
            color = OnBgBody,
            style = Type.BodyBold,
            modifier = Modifier.weight(1f),
        )
        // Drawn like a TextAction (Bold Accent + chevron); the whole chip is the target.
        Text(stringResource(R.string.main_return), color = Accent, style = Type.BodyBold)
        RowChevron(tint = Accent)
    }
}

/**
 * The sheet runs in its own dialog window, which on gesture-less (3-button)
 * navigation gets the system's contrast scrim, a near-white band under a light
 * palette. Turning it off lets the sheet's own surface (already drawn under the
 * nav inset) be the band, the same theme-derived ground the activity window gets
 * in MainActivity. API 35+ ignores the nav bar colour (edge-to-edge), so only the
 * scrim switch applies there.
 */
@Composable
private fun SheetNavBarScrim() {
    val window = (LocalView.current.parent as? DialogWindowProvider)?.window ?: return
    SideEffect {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.VANILLA_ICE_CREAM) {
            @Suppress("DEPRECATION")
            window.navigationBarColor = android.graphics.Color.TRANSPARENT
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) window.isNavigationBarContrastEnforced = false
    }
}

/**
 * The app's bottom sheet: fully expanded by default, so Back and a swipe close it
 * in one step (a half-height detent made Back collapse it first), on the sheet
 * ground [BgSunken] unless [containerColor] says otherwise, with
 * [SheetNavBarScrim] applied.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun AppSheet(
    onDismiss: () -> Unit,
    sheetState: SheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
    containerColor: Color = BgSunken,
    sheetGesturesEnabled: Boolean = true,
    content: @Composable ColumnScope.() -> Unit,
) {
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        modifier = Modifier.traversalGroup(),
        sheetState = sheetState,
        sheetGesturesEnabled = sheetGesturesEnabled,
        containerColor = containerColor,
    ) {
        SheetNavBarScrim()
        content()
    }
}

/**
 * Marks a node as a traversal group: TalkBack reads its children together, sorted
 * by the group's own layout direction. Every window's top container (the
 * activity's content, a sheet, a dialog) needs it: a Compose window's root node is
 * always left to right (`AndroidComposeView` never sets its direction), so without
 * it an RTL locale hears a row outside a lazy list (the bottom nav, a dialog's
 * buttons) end to start.
 */
internal fun Modifier.traversalGroup(): Modifier = semantics { isTraversalGroup = true }

/** A sheet's scrolling body: full width, scrolled by [scroll], clear of the
 *  navigation bar, with the sheet's [sheetPadding] at [gutter]. */
internal fun Modifier.sheetBody(scroll: ScrollState, gutter: Dp = Space.Sheet.dp): Modifier =
    fillMaxWidth().verticalScroll(scroll).navigationBarsPadding().sheetPadding(gutter)

/** A sheet body's side ([gutter]) and bottom padding. */
internal fun Modifier.sheetPadding(gutter: Dp = Space.Sheet.dp): Modifier = padding(horizontal = gutter).padding(bottom = Space.Lg.dp)
