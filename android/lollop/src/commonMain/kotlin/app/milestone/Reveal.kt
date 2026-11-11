package app.milestone

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.relocation.BringIntoViewRequester
import androidx.compose.foundation.relocation.bringIntoViewRequester
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.layout.LayoutCoordinates
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInWindow
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.isActive

/** [content] (one row inside a Profile card) scrolled into view once the layout
 *  settles while [request] holds, then [onRevealed]: a banner's deep link landing
 *  on the exact answer behind a hold, high in the list ([LocalRevealViewport]). */
@Composable
internal fun RevealTarget(request: Boolean, onRevealed: () -> Unit, content: @Composable () -> Unit) {
    val requester = remember { BringIntoViewRequester() }
    val top = remember { WindowTop() }
    val landIn = LocalRevealViewport.current
    LaunchedEffect(request) {
        if (request) {
            revealWhenSettled(top, requester, landIn = landIn)
            onRevealed()
        }
    }
    Box(
        Modifier
            .bringIntoViewRequester(requester)
            .onGloballyPositioned { top.place(it) },
    ) { content() }
}

/** Last laid-out window y and width of a reveal target; written only from layout
 *  callbacks, so a plain holder rather than snapshot state. */
internal class WindowTop {
    var y = Float.NaN
    var width = 0f

    fun place(coords: LayoutCoordinates) {
        y = coords.positionInWindow().y
        width = coords.size.width.toFloat()
    }
}

/** Height in px of the list a deep link reveals its row in (0 = unknown), written
 *  by that list's layout callback; a plain holder like [WindowTop]. */
internal class RevealViewport {
    var height = 0
}

/** The list a deep-link reveal lands in. Without one (0 height) the reveal only
 *  brings the row on screen. */
internal val LocalRevealViewport = staticCompositionLocalOf { RevealViewport() }

/** Where a deep link lands its row in a [RevealViewport]: a row below scrolls up
 *  until its top sits a third of the way down, a row above down to a quarter, so
 *  the answers around it stay in view instead of the row hugging an edge. */
private const val LAND_FROM_BELOW = 1f / 3
private const val LAND_FROM_ABOVE = 1f / 4

/** Frames a reveal waits at most for the layout to stop moving. */
private const val REVEAL_MAX_FRAMES = 12

/**
 * Brings [requesters] into view (in order) once the surrounding layout settles. A
 * tab switch, or a hold banner appearing above the list, keeps moving the target
 * for a few frames, so a single frame's wait can scroll to a stale position: this
 * re-requests until [top] holds still across a frame. A competing scroll (the
 * list keeping a toggled row under the finger) cancels one request, not the reveal.
 * With [landIn] (a deep link) each request asks for the band around the row that
 * lands it high in the list ([LAND_FROM_BELOW]); a row already there stays put.
 */
internal suspend fun revealWhenSettled(
    top: WindowTop,
    vararg requesters: BringIntoViewRequester,
    landIn: RevealViewport? = null,
) {
    var last = Float.NaN
    repeat(REVEAL_MAX_FRAMES) {
        withFrameNanos { }
        val viewport = landIn?.height ?: 0
        val band = if (viewport > 0) {
            Rect(0f, -viewport * LAND_FROM_ABOVE, top.width, viewport * (1f - LAND_FROM_BELOW))
        } else {
            null
        }
        for (r in requesters) r.bringIntoViewUnlessPreempted(band)
        withFrameNanos { }
        if (top.y == last) return
        last = top.y
    }
}

/**
 * [BringIntoViewRequester.bringIntoView] that a competing scroll ends without ending
 * the caller: Compose cancels an in-flight request with a [CancellationException]
 * when another scroll takes over the container (the list keeping a toggled row under
 * the finger). The caller's own cancellation is rethrown.
 */
private suspend fun BringIntoViewRequester.bringIntoViewUnlessPreempted(band: Rect?) {
    try {
        bringIntoView(band)
    } catch (e: CancellationException) {
        if (!currentCoroutineContext().isActive) throw e
    }
}
