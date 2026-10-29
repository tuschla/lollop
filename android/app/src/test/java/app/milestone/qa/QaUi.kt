package app.milestone.qa

import android.content.Context
import android.content.Intent
import android.view.View
import android.view.ViewGroup
import android.view.WindowManager
import androidx.annotation.PluralsRes
import androidx.annotation.StringRes
import androidx.compose.ui.test.ComposeTimeoutException
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.hasScrollToNodeAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.ComposeTestRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performScrollToNode
import androidx.test.core.app.ActivityScenario
import androidx.test.core.app.ApplicationProvider
import app.milestone.AppSettings
import app.milestone.DarkMode
import app.milestone.EventDispatcher
import app.milestone.MainActivity
import app.milestone.R
import app.milestone.ViewModel
import com.github.takahirom.roborazzi.captureScreenRoboImage
import java.util.Timer
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.concurrent.schedule

/** Shared UI drivers for the offline scenario suite. */

/** The app's own copy for [id], so an assertion follows the string resource
 *  rather than pinning its English wording. Without [args] the resource is
 *  read verbatim (no format pass, so a literal `%` stays as is). */
fun str(@StringRes id: Int, vararg args: Any): String {
    val ctx = ApplicationProvider.getApplicationContext<Context>()
    return if (args.isEmpty()) ctx.getString(id) else ctx.getString(id, *args)
}

/** The leading text of [id] up to its first placeholder past [args], with [args]
 *  filled in: a stable substring when a later argument varies, e.g.
 *  `strPrefix(R.string.plan_log_set_n_of_n, 1)` is "Log set 1 of" for any N. */
fun strPrefix(@StringRes id: Int, vararg args: Any): String {
    val template = str(id)
    val next = template.indexOf("%${args.size + 1}\$")
    val head = if (next < 0) template else template.substring(0, next)
    return head.format(*args).trimEnd()
}

/** [str] for a plurals resource. */
fun plural(@PluralsRes id: Int, count: Int, vararg args: Any): String =
    ApplicationProvider.getApplicationContext<Context>().resources.getQuantityString(id, count, *args)


// The most recently launched Activity, closed by [QaHarness.teardown] so boot
// effects + the process-scoped event consumer from a prior test don't accumulate
// and starve the single-threaded dispatcher.
internal var lastScenario: ActivityScenario<MainActivity>? = null

/** How long a launch may hold the splash before the watchdog gives up on boot:
 *  far past the long-history boot budget (45 s), so only a stuck boot reaches it. */
private const val SPLASH_WATCHDOG_MS = 120_000L

/**
 * Run [launch] (an Activity start, which idles the main looper until no frame is
 * pending) under a watchdog. The splash cancels every frame while boot has
 * published no model, so a boot that never publishes would keep that idle
 * spinning forever; after [SPLASH_WATCHDOG_MS] the watchdog stands an empty model
 * in to release the splash, and the launch fails with the reason instead.
 */
internal fun <T> launchWithSplashWatchdog(launch: () -> T): T {
    val stalled = AtomicBoolean(false)
    val watchdog = Timer("splash-watchdog", true)
    watchdog.schedule(SPLASH_WATCHDOG_MS) {
        if (EventDispatcher.model.compareAndSet(null, ViewModel())) stalled.set(true)
    }
    val launched = try {
        launch()
    } finally {
        watchdog.cancel()
    }
    if (stalled.get()) {
        throw AssertionError(
            "boot published no model within ${SPLASH_WATCHDOG_MS / 1000} s, so the splash held every " +
                "frame and the launch could not go idle (is the EventDispatcher consumer stuck?)",
        )
    }
    return launched
}

/** Start [MainActivity] (with [intent], e.g. an open intent) under [launchWithSplashWatchdog]. */
internal fun launchMain(intent: Intent? = null): ActivityScenario<MainActivity> = launchWithSplashWatchdog {
    if (intent == null) ActivityScenario.launch(MainActivity::class.java) else ActivityScenario.launch(intent)
}

/** Launch the real Activity and wait until boot hydration finished: the restored
 *  (and, for a set-up profile, freshly planned) model is published and the main
 *  scaffold replaced the loading spinner. The semantics fetch comes FIRST: under
 *  Robolectric it is what idles the main looper, so the boot coroutine (which
 *  resumes on Main) can only progress while the condition keeps fetching. */
fun ComposeTestRule.launchApp() {
    lastScenario = launchMain()
    waitUntil(30_000) {
        onAllNodesWithText(str(R.string.main_dest_history)).fetchSemanticsNodes().isNotEmpty() &&
            EventDispatcher.bootHandled
    }
    waitForIdle()
}

/** Launch the real Activity on a FRESH install and wait for the guided-setup
 *  screen (which replaces the scaffold) to appear. */
fun ComposeTestRule.launchAppExpectingSetup() {
    lastScenario = launchMain()
    waitUntil(30_000) {
        onAllNodesWithText(str(R.string.setup_step_train_q)).fetchSemanticsNodes().isNotEmpty()
    }
}

/**
 * Assert that exactly [count] nodes match [matcher] once the UI settles. A user
 * action reaches the core on its own JNI thread and the new model arrives on a
 * later main-looper turn, which Compose's idling cannot see; so wait (polling
 * the semantics tree, which also idles the looper) for the expected count, then
 * assert — on timeout the assertion reports the real count.
 */
fun ComposeTestRule.assertCountEventually(matcher: SemanticsMatcher, count: Int, timeoutMs: Long = 10_000) {
    try {
        waitUntil(timeoutMs) { onAllNodes(matcher).fetchSemanticsNodes().size == count }
    } catch (_: ComposeTimeoutException) {
        // Fall through: the assertion below names the matcher and actual count.
    }
    onAllNodes(matcher).assertCountEquals(count)
}

/** [assertCountEventually] for an exact visible text. */
fun ComposeTestRule.assertTextCountEventually(text: String, count: Int) =
    assertCountEventually(hasText(text), count)

/** Switch the bottom-nav destination by its visible label. */
fun ComposeTestRule.selectTab(label: String) {
    onNodeWithText(label).performClick()
    waitForIdle()
}

/** On Profile: scroll to the folded "Your plan answers" tile and unfold it, so
 *  the training-profile rows (Plan … Training context) are composed. */
fun ComposeTestRule.openPlanAnswers() {
    val tile = hasText(str(R.string.main_plan_answers_title)) and androidx.compose.ui.test.hasClickAction()
    onAllNodes(hasScrollToNodeAction())[0].performScrollToNode(tile)
    onNode(tile).performClick()
    waitForIdle()
}

/** Type [figures] ("60", "137.5") into the set editor's weight field on its
 *  in-form keypad (the ± chips only step from a value already there), then fold
 *  the keypad with its Done key so the effort row is back. */
fun ComposeTestRule.typeWeight(figures: String) {
    onAllNodes(
        androidx.compose.ui.test.hasClickAction() and
            androidx.compose.ui.test.hasContentDescription(str(R.string.log_field_weight), substring = true),
    )[0].performScrollTo().performClick()
    figures.forEach { c ->
        val key = if (c == '.') {
            androidx.compose.ui.test.hasContentDescription(str(R.string.ui_cd_decimal_point))
        } else {
            hasText("$c")
        }
        onNode(
            androidx.compose.ui.test.hasClickAction() and
                (key or androidx.compose.ui.test.hasAnyDescendant(key)) and
                androidx.compose.ui.test.hasAnyAncestor(androidx.compose.ui.test.hasTestTag("numeric-keypad")),
        ).performScrollTo().performClick()
    }
    onNode(
        androidx.compose.ui.test.hasClickAction() and hasText(str(R.string.log_keypad_done)) and
            androidx.compose.ui.test.hasAnyAncestor(androidx.compose.ui.test.hasTestTag("numeric-keypad")),
    ).performScrollTo().performClick()
    waitForIdle()
}

/** Open the set loop for [exercise] from Today: the current item's filled pill or
 *  another open item's compact row (both speak "Log set 1 of N, <exercise>"). */
fun ComposeTestRule.openLoopFor(exercise: String) {
    onNode(
        androidx.compose.ui.test.hasClickAction() and
            androidx.compose.ui.test.hasContentDescription(strPrefix(R.string.plan_log_set_n_of_n, 1), substring = true) and
            androidx.compose.ui.test.hasAnyAncestor(androidx.compose.ui.test.hasTestTag("plan-item:$exercise")),
    ).performScrollTo().performClick()
    waitUntil(15_000) {
        onAllNodes(hasText(strPrefix(R.string.log_set_header_n_of_n, 1), substring = true)).fetchSemanticsNodes().isNotEmpty()
    }
}

/** The set loop's "Set k of N" header. */
fun setHeader(k: Int, n: Int) = str(R.string.log_set_header_n_of_n, k, n)

/** Select a History sub-filter ("Lifts" / "Runs"). */
fun ComposeTestRule.selectHistoryFilter(label: String) {
    onNodeWithText(label).performClick()
    waitForIdle()
}

/** Pick [value] on the [app.milestone.RatingScale] tagged [tag] (`effort-scale`,
 *  `pain-severity`, `checkin-sleep` …). The track is ONE adjustable node, so the
 *  segment "$tag-$value" lives in the unmerged tree; clicking it taps the track
 *  over that segment, as a finger would. */
fun ComposeTestRule.pickRating(tag: String, value: Int) {
    val segment = "$tag-$value"
    val inScroll = onAllNodes(
        androidx.compose.ui.test.hasTestTag(segment) and
            androidx.compose.ui.test.hasAnyAncestor(androidx.compose.ui.test.hasScrollAction()),
        useUnmergedTree = true,
    ).fetchSemanticsNodes().isNotEmpty()
    val node = onNodeWithTag(segment, useUnmergedTree = true)
    if (inScroll) node.performScrollTo()
    node.performClick()
    waitForIdle()
}

/**
 * Scroll the (Lazy) list on the current screen until a row containing [text] is
 * composed, so an assertion on an initially-below-the-fold history entry is
 * reliable.
 */
fun ComposeTestRule.scrollToText(text: String) {
    onNode(hasScrollToNodeAction()).performScrollToNode(hasText(text, substring = true))
    waitForIdle()
}

/** A capture name's light/dark suffix. */
val DarkMode.suffix: String get() = if (this == DarkMode.Dark) "dark" else "light"

/** Force light or dark for the next launch. */
fun useDarkMode(mode: DarkMode) = AppSettings.setDarkMode(ApplicationProvider.getApplicationContext(), mode)

/** Capture the screen to the golden `src/test/screenshots/[name].png`. */
@OptIn(com.github.takahirom.roborazzi.ExperimentalRoborazziApi::class)
fun ComposeTestRule.shot(name: String) =
    captureWithoutRipples { withWindowsInAddOrder { captureScreenRoboImage("src/test/screenshots/$name.png") } }

/**
 * Run [capture] with every Compose ripple host hidden, so no tap ripple lands in a
 * screenshot.
 *
 * Compose draws its Android ripple as a platform RippleDrawable in a RippleHostView.
 * Under Robolectric that view is only ever drawn inside a capture (the hardware
 * PixelCopy), so a tap's pending ripple starts at the first capture after the tap
 * and then runs as RenderThread animators on the real wall clock: the next capture
 * showed it at whatever stage real elapsed time had reached, which neither Compose's
 * test clock nor the paused main looper controls. Every capture follows completed
 * taps, where the settled screen shows no ripple; hiding the hosts captures exactly
 * that and never starts the animation.
 */
private fun ComposeTestRule.captureWithoutRipples(capture: () -> Unit) {
    waitForIdle()
    val hosts = windowViews().flatMap { it.descendants() }
        .filter { rippleHostClass.isInstance(it) && it.visibility == View.VISIBLE }
    hosts.forEach { it.visibility = View.INVISIBLE }
    try {
        capture()
    } finally {
        hosts.forEach { it.visibility = View.VISIBLE }
    }
}

private val rippleHostClass by lazy { Class.forName("androidx.compose.material.ripple.RippleHostView") }

/**
 * Roborazzi composites the window roots sorted by `LayoutParams.type`, and
 * Espresso lists same-type roots newest-first, so a dialog opened over a sheet
 * (both TYPE_APPLICATION dialogs) is painted UNDER the older sheet and never
 * shows in the capture. For the capture only, rank every window by the order it
 * was added (= its on-device z-order), then restore the real types.
 */
@Suppress("UNCHECKED_CAST")
private fun <T> withWindowsInAddOrder(block: () -> T): T {
    val globalClass = Class.forName("android.view.WindowManagerGlobal")
    val global = globalClass.getMethod("getInstance").invoke(null)
    val field = globalClass.getDeclaredField("mParams").apply { isAccessible = true }
    val params = (field.get(global) as List<WindowManager.LayoutParams>).toList()
    val saved = params.map { it.type }
    params.forEachIndexed { i, p -> p.type = WindowManager.LayoutParams.FIRST_APPLICATION_WINDOW + i }
    try {
        return block()
    } finally {
        params.forEachIndexed { i, p -> p.type = saved[i] }
    }
}

@Suppress("UNCHECKED_CAST")
private fun windowViews(): List<View> {
    val globalClass = Class.forName("android.view.WindowManagerGlobal")
    val global = globalClass.getMethod("getInstance").invoke(null)
    return (globalClass.getMethod("getWindowViews").invoke(global) as List<View>).toList()
}

private fun View.descendants(): Sequence<View> = sequence {
    yield(this@descendants)
    if (this@descendants is ViewGroup) {
        for (i in 0 until childCount) yieldAll(getChildAt(i).descendants())
    }
}
