package app.milestone

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material3.adaptive.currentWindowAdaptiveInfoV2
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.window.core.layout.WindowSizeClass

// Adaptive layout (visual-design-system §5 "Adaptive layout"): what a wide or a
// short window changes. Decided once from the activity window's size class and
// handed down through [LocalWindowLayout], so a sheet or a dialog (each its own
// window) lays out for the app window it sits over, not for itself.

/**
 * How the app window is laid out.
 *
 * [navRail]: the destinations sit in a side rail instead of the bottom bar (a
 * window at least Medium wide, or a short one, where the bar would take a sixth
 * of the height). [shortHeight]: a Compact-height window (a phone in landscape):
 * the wordmark bar gives its height back and the route mark tops the rail.
 * [twoPane]: a screen with two groups of content (Today's session and its week,
 * History's list and the open entry) shows them side by side (an Expanded-wide
 * window: a landscape phone, a tablet in landscape). [sideControls]: a task
 * screen (a log editor, a setup question, the run tracker) puts its controls
 * (keypad, options, stats) beside its content instead of under it, so nothing
 * scrolls out of reach (a landscape window at least Medium wide).
 */
@Immutable
internal data class WindowLayout(
    val navRail: Boolean,
    val shortHeight: Boolean,
    val twoPane: Boolean,
    val sideControls: Boolean,
) {
    companion object {
        /** A portrait phone: the bottom bar, one column, controls under the content. */
        val Compact = WindowLayout(navRail = false, shortHeight = false, twoPane = false, sideControls = false)
    }
}

/** The layout for a window of [sizeClass] (Material window size classes) that
 *  is wider than tall when [landscape]. */
internal fun windowLayoutFor(sizeClass: WindowSizeClass, landscape: Boolean): WindowLayout {
    val medium = sizeClass.isWidthAtLeastBreakpoint(WindowSizeClass.WIDTH_DP_MEDIUM_LOWER_BOUND)
    val expanded = sizeClass.isWidthAtLeastBreakpoint(WindowSizeClass.WIDTH_DP_EXPANDED_LOWER_BOUND)
    val shortHeight = !sizeClass.isHeightAtLeastBreakpoint(WindowSizeClass.HEIGHT_DP_MEDIUM_LOWER_BOUND)
    return WindowLayout(
        navRail = medium || shortHeight,
        shortHeight = shortHeight,
        twoPane = expanded,
        sideControls = medium && landscape,
    )
}

/** The current app window's layout; read only at the activity root. */
@Composable
internal fun rememberWindowLayout(): WindowLayout {
    val sizeClass = currentWindowAdaptiveInfoV2().windowSizeClass
    val size = LocalWindowInfo.current.containerSize
    val landscape = size.width > size.height
    return remember(sizeClass, landscape) { windowLayoutFor(sizeClass, landscape) }
}

/** The app window's layout, provided at the activity root ([CoachScaffold]). */
internal val LocalWindowLayout = staticCompositionLocalOf { WindowLayout.Compact }

/** The side gutter that centres a column of at most [max] in [width], never
 *  under [min]: the plain screen gutter on a phone, wider on a wide window. */
internal fun centringGutter(width: Dp, min: Dp = Space.Screen.dp, max: Dp = ContentMaxWidth): Dp =
    maxOf(min, (width - max) / 2)

/**
 * A full-size box that hands [content] the side gutter centring a column of at
 * most [ContentMaxWidth] (see [centringGutter]). A list keeps the whole width as
 * its scroll area and takes the gutter as its content padding, so it scrolls
 * from anywhere on a tablet while its cards stay readable.
 */
@Composable
internal fun ReadableWidth(modifier: Modifier = Modifier, content: @Composable (gutter: Dp) -> Unit) {
    BoxWithConstraints(modifier.fillMaxSize()) { content(centringGutter(maxWidth)) }
}

/**
 * Two panes side by side, each at most [PaneMaxWidth], centred together. [first]
 * is read first (TalkBack traverses start pane, then end pane).
 */
@Composable
internal fun TwoPaneRow(
    modifier: Modifier = Modifier,
    first: @Composable RowScope.() -> Unit,
    second: @Composable RowScope.() -> Unit,
) {
    Box(modifier.fillMaxSize(), contentAlignment = Alignment.TopCenter) {
        Row(
            modifier = Modifier.widthIn(max = PaneMaxWidth * 2 + Space.Screen.dp).fillMaxSize().traversalGroup(),
            horizontalArrangement = Arrangement.spacedBy(0.dp),
        ) {
            first()
            second()
        }
    }
}

/**
 * A place a composable can be shown somewhere else than where it is declared:
 * the side pane of a two-pane editor shows the keypad its form declares
 * ([KeypadPanel]), so the form keeps the keypad's state and logic while the
 * keypad sits beside it. The host draws [content] where it wants it.
 */
@Stable
internal class SlotDock {
    var content by mutableStateOf<(@Composable () -> Unit)?>(null)
        private set

    /** Show [slot] in the dock while the caller is composed (see [Docked]). */
    internal fun show(slot: @Composable () -> Unit) {
        content = slot
    }

    internal fun clear(slot: @Composable () -> Unit) {
        if (content === slot) content = null
    }
}

/** Hand [content] to [dock] for as long as this call is composed; it always draws
 *  the latest [content] (state read inside it stays live). */
@Composable
internal fun Docked(dock: SlotDock, content: @Composable () -> Unit) {
    val latest by rememberUpdatedState(content)
    val slot: @Composable () -> Unit = remember { { latest() } }
    DisposableEffect(dock, slot) {
        dock.show(slot)
        onDispose { dock.clear(slot) }
    }
}

/** The dock a two-pane editor offers its keypad; null when the editor is one column. */
internal val LocalKeypadDock = staticCompositionLocalOf<SlotDock?> { null }
