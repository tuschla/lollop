package app.milestone

import androidx.compose.foundation.ScrollState
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.windowInsetsBottomHeight
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.BottomSheetDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.NavigationRail
import androidx.compose.material3.NavigationRailDefaults
import androidx.compose.material3.NavigationRailItem
import androidx.compose.material3.NavigationRailItemDefaults
import androidx.compose.material3.SheetState
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.milestone.resources.Res
import app.milestone.resources.app_name
import app.milestone.resources.ic_action_log_plus
import app.milestone.resources.ic_brand_mark
import app.milestone.resources.ic_content_run
import app.milestone.resources.ic_ui_close
import app.milestone.resources.main_close
import app.milestone.resources.main_log
import app.milestone.resources.main_return
import app.milestone.resources.main_run_in_progress
import org.jetbrains.compose.resources.painterResource
import org.jetbrains.compose.resources.stringResource

// Shared shell chrome, specified in planning/design/screens/00-chrome.md.

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
 * mark + `app_name` ("lollo") wordmark, both `Accent`, IS the title; no secondary
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
            painterResource(Res.drawable.ic_brand_mark),
            contentDescription = null,
            tint = Accent,
            modifier = Modifier.size(BrandMarkSize),
        )
        Text(
            stringResource(Res.string.app_name),
            color = Accent,
            style = Type.Title.copy(fontWeight = FontWeight.ExtraBold),
            modifier = Modifier.weight(1f),
        )
    }
}

/**
 * Bottom navigation bar (chrome §1). Only the Today icon fills when selected
 * (the asset pair encodes that).
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
 * FAB is) at its top. Used instead of the bottom bar and the Log FAB on a wide or
 * a short window, and whenever they would be laid over the pinned safety banner
 * (a landscape phone under a hold): beside the banner, never over it. With
 * [showBrand] (a short window, where the wordmark bar is left out) the route mark
 * tops the rail.
 */
@Composable
fun MilestoneNavRail(selected: Dest, onSelect: (Dest) -> Unit, onLog: (() -> Unit)?, showBrand: Boolean = false) {
    val logLabel = stringResource(Res.string.main_log)
    // Material's NavigationRail puts its header inside the destinations' pick-one
    // group, so TalkBack read Log as a fourth tab; Log and the mark sit above the
    // group instead.
    Column(
        modifier = Modifier
            .testTag("nav-rail")
            .fillMaxHeight()
            .background(BgTop)
            .windowInsetsPadding(NavigationRailDefaults.windowInsets)
            .widthIn(min = RailWidth)
            .padding(vertical = Space.Sm.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
    ) {
        if (showBrand) {
            Icon(
                painterResource(Res.drawable.ic_brand_mark),
                contentDescription = null,
                tint = Accent,
                modifier = Modifier.padding(vertical = Space.Md.dp).size(BrandMarkSize).testTag("rail-brand"),
            )
        }
        onLog?.let { log ->
            FloatingActionButton(
                onClick = log,
                modifier = Modifier.padding(bottom = Space.Md.dp).testTag("log-fab").semantics { contentDescription = logLabel },
                containerColor = Accent,
                contentColor = OnAccent,
                shape = RoundedCornerShape(Space.Card.dp),
            ) {
                Icon(painterResource(Res.drawable.ic_action_log_plus), contentDescription = null, modifier = Modifier.size(20.dp))
            }
        }
        Column(
            modifier = Modifier.selectableGroup(),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
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
}

/** Width of [MilestoneNavRail] (Material's rail). */
private val RailWidth = 80.dp

/**
 * Run-in-progress chip (chrome §4): pinned just above the nav bar on every
 * destination while a run records.
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
            painterResource(Res.drawable.ic_content_run),
            contentDescription = null,
            tint = Accent,
            modifier = Modifier.size(18.dp),
        )
        Text(
            stringResource(Res.string.main_run_in_progress),
            color = OnBgBody,
            style = Type.BodyBold,
            modifier = Modifier.weight(1f),
        )
        // Drawn like a TextAction (Bold Accent + chevron); the whole chip is the target.
        Text(stringResource(Res.string.main_return), color = Accent, style = Type.BodyBold)
        RowChevron(tint = Accent)
    }
}

/**
 * The app's bottom sheet: fully expanded by default, so Back and a swipe close it
 * in one step (a half-height detent made Back collapse it first), on the sheet
 * ground [BgSunken] unless [containerColor] says otherwise, with
 * [SheetNavBarScrim] applied. A short window (a phone in landscape) leaves the
 * drag handle out and gives its 48dp to the content: the set logger's keypad
 * column, Done and Save need it there. The sheet still drags by its body, and
 * the scrim and Back close it.
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
    val shortHeight = LocalWindowLayout.current.shortHeight
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        modifier = Modifier.traversalGroup(),
        sheetState = sheetState,
        sheetGesturesEnabled = sheetGesturesEnabled,
        containerColor = containerColor,
        dragHandle = if (shortHeight) null else ({ BottomSheetDefaults.DragHandle() }),
    ) {
        SheetNavBarScrim()
        if (shortHeight) Spacer(Modifier.height(Space.Md.dp))
        content()
    }
}

/** The pane a two-pane screen (History) offers its detail; null under one column. */
internal val LocalDetailDock = staticCompositionLocalOf<SlotDock?> { null }

/**
 * An entry's detail: an [AppSheet] on one column; on a two-pane screen
 * ([LocalDetailDock]) the same body stands in the detail pane beside the list,
 * on the sheet ground [BgSunken] (every entry kind alike) with a close row on
 * top. Back and the close button both call [onDismiss], as the sheet's swipe and
 * scrim do.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun DetailSheet(
    onDismiss: () -> Unit,
    content: @Composable ColumnScope.() -> Unit,
) {
    val dock = LocalDetailDock.current
    if (dock == null) {
        AppSheet(onDismiss = onDismiss, content = content)
        return
    }
    Docked(dock) {
        BackHandler(onBack = onDismiss)
        Column(
            Modifier
                .fillMaxSize()
                .clip(RoundedCornerShape(topStart = Space.Card.dp, topEnd = Space.Card.dp))
                .background(BgSunken)
                .testTag("detail-pane")
                .traversalGroup(),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth().padding(horizontal = Space.Sm.dp, vertical = Space.Sm.dp),
                horizontalArrangement = Arrangement.End,
            ) {
                IconButton(onClick = onDismiss, modifier = Modifier.testTag("detail-pane-close")) {
                    Icon(
                        painterResource(Res.drawable.ic_ui_close),
                        contentDescription = stringResource(Res.string.main_close),
                        tint = OnBgMuted,
                    )
                }
            }
            content()
        }
    }
}

internal fun Modifier.sheetBody(scroll: ScrollState, gutter: Dp = Space.Sheet.dp): Modifier =
    fillMaxWidth().verticalScroll(scroll).navigationBarsPadding().sheetPadding(gutter)

internal fun Modifier.sheetPadding(gutter: Dp = Space.Sheet.dp): Modifier = padding(horizontal = gutter).padding(bottom = Space.Lg.dp)
