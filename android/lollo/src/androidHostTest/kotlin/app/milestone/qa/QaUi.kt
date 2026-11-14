package app.milestone.qa

import android.content.Context
import android.content.Intent
import android.view.View
import android.view.ViewGroup
import android.view.WindowManager
import androidx.annotation.PluralsRes
import androidx.annotation.StringRes
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.ComposeTimeoutException
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasScrollToIndexAction
import androidx.compose.ui.test.hasScrollToNodeAction
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.junit4.ComposeTestRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performScrollToIndex
import androidx.compose.ui.test.performScrollToNode
import androidx.test.core.app.ActivityScenario
import androidx.test.core.app.ApplicationProvider
import app.milestone.AppRuntime
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

fun plural(@PluralsRes id: Int, count: Int, vararg args: Any): String =
    ApplicationProvider.getApplicationContext<Context>().resources.getQuantityString(id, count, *args)

// The most recently launched Activity, closed by [QaHarness.teardown] so boot
// effects + the process-scoped event consumer from a prior test don't accumulate
// and starve the single-threaded dispatcher.
internal var lastScenario: ActivityScenario<MainActivity>? = null

/** How long a launch may hold the splash before the watchdog gives up on boot:
 *  far past [LongHistoryUiTest]'s boot budget, so only a stuck boot reaches it. */
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
            AppRuntime.bootHandled
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
 * assert. On timeout the assertion reports the real count.
 */
fun ComposeTestRule.assertCountEventually(matcher: SemanticsMatcher, count: Int, timeoutMs: Long = 10_000) {
    try {
        waitUntil(timeoutMs) { onAllNodes(matcher).fetchSemanticsNodes().size == count }
    } catch (_: ComposeTimeoutException) {
    }
    onAllNodes(matcher).assertCountEquals(count)
}

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

/** Open the set loop at [set] for [exercise] from Today: the current item's filled
 *  pill or another open item's compact row (both speak "Log set k of N, <exercise>"). */
fun ComposeTestRule.openLoopFor(exercise: String, set: Int = 1) {
    onNode(
        androidx.compose.ui.test.hasClickAction() and
            androidx.compose.ui.test.hasContentDescription(strPrefix(R.string.plan_log_set_n_of_n, set), substring = true) and
            androidx.compose.ui.test.hasAnyAncestor(androidx.compose.ui.test.hasTestTag("plan-item:$exercise")),
    ).performScrollTo().performClick()
    waitUntil(15_000) {
        onAllNodes(hasText(strPrefix(R.string.log_set_header_n_of_n, set), substring = true)).fetchSemanticsNodes().isNotEmpty()
    }
}

fun setHeader(k: Int, n: Int) = str(R.string.log_set_header_n_of_n, k, n)

/** The bottom bar's Log button, which opens the log chooser. */
val logFab: SemanticsMatcher get() = hasClickAction() and hasContentDescription(str(R.string.main_log))

/** Wait until a node shows [text] and the UI is idle. */
fun ComposeTestRule.waitForText(text: String, substring: Boolean = false) {
    waitForNode(hasText(text, substring = substring))
    waitForIdle()
}

/** Tap [logFab] and wait for the chooser (its morning check-in row is always offered). */
fun ComposeTestRule.openLogChooser() {
    onNode(logFab).performClick()
    waitForText(str(R.string.main_morning_checkin_title))
}

/** From Today: open the morning check-in from the log chooser and wait for its first scale. */
fun ComposeTestRule.openMorningCheckin() {
    openLogChooser()
    onNodeWithText(str(R.string.main_morning_checkin_title)).performScrollTo().performClick()
    waitForNode(hasTestTag("checkin-sleep"))
}

/** Tap the open editor's primary action (Log set, Save, Next). */
fun ComposeTestRule.tapEditorPrimary() = onNodeWithTag("editor-primary").performClick()

/** Scroll Today's list to its last row. */
fun ComposeTestRule.scrollTodayToEnd() {
    val list = onAllNodes(hasScrollToIndexAction())[0]
    val info = list.fetchSemanticsNode().config.getOrNull(SemanticsProperties.CollectionInfo)
    if (info != null && info.rowCount > 0) list.performScrollToIndex(info.rowCount - 1)
    waitForIdle()
}

/** Scroll Today's list until a node matching [matcher] is composed. */
fun ComposeTestRule.scrollTodayTo(matcher: SemanticsMatcher) {
    onNode(hasScrollToNodeAction()).performScrollToNode(matcher)
    waitForIdle()
}

/** Every text and content description on screen, across all windows. */
fun ComposeTestRule.screenTexts(): List<String> =
    onAllNodes(isRoot()).fetchSemanticsNodes().flatMap { flatten(it) }.flatMap { n ->
        n.config.getOrNull(SemanticsProperties.Text).orEmpty().map { it.text } +
            n.config.getOrNull(SemanticsProperties.ContentDescription).orEmpty()
    }.filter { it.isNotBlank() }

/** [node] and every node under it, depth first. */
fun flatten(node: SemanticsNode): List<SemanticsNode> = listOf(node) + node.children.flatMap(::flatten)

/** Pick [value] on the [app.milestone.RatingScale] tagged [tag] (`effort-scale`,
 *  `pain-severity`, `checkin-sleep` …). The track is ONE adjustable node, so the
 *  segment "$tag-$value" lives in the unmerged tree; clicking it taps the track
 *  over that segment, as a finger would. When the digits do not fit (large font
 *  in a narrow column) the scale is the [app.milestone.ChoiceScaleRow] fallback
 *  instead: no segments, one selectable cell per value, and the cell is tapped.
 *  Either is scrolled into view first when it sits in a scroll container. */
fun ComposeTestRule.pickRating(tag: String, value: Int) {
    val segment = androidx.compose.ui.test.hasTestTag("$tag-$value")
    val cell = androidx.compose.ui.test.isSelectable() and hasText("$value") and
        androidx.compose.ui.test.hasAnyAncestor(androidx.compose.ui.test.hasTestTag(tag))
    val onTrack = onAllNodes(segment, useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty()
    val target = if (onTrack) segment else cell
    val inScroll = onAllNodes(
        target and androidx.compose.ui.test.hasAnyAncestor(androidx.compose.ui.test.hasScrollAction()),
        useUnmergedTree = onTrack,
    ).fetchSemanticsNodes().isNotEmpty()
    val node = onNode(target, useUnmergedTree = onTrack)
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

val DarkMode.suffix: String get() = if (this == DarkMode.Dark) "dark" else "light"

fun useDarkMode(mode: DarkMode) = AppSettings.setDarkMode(ApplicationProvider.getApplicationContext(), mode)

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
private fun <T> withWindowsInAddOrder(block: () -> T): T {
    val params = windowParams()
    val saved = params.map { it.type }
    params.forEachIndexed { i, p -> p.type = WindowManager.LayoutParams.FIRST_APPLICATION_WINDOW + i }
    try {
        return block()
    } finally {
        params.forEachIndexed { i, p -> p.type = saved[i] }
    }
}

private fun windowManagerGlobal(): Any = Class.forName("android.view.WindowManagerGlobal").getMethod("getInstance").invoke(null)!!

/** Every window's root view, in the order the windows were added. */
@Suppress("UNCHECKED_CAST")
internal fun windowViews(): List<View> = windowManagerGlobal().let { g ->
    (g.javaClass.getMethod("getWindowViews").invoke(g) as List<View>).toList()
}

/** The layout params of [windowViews], index for index (the params themselves are live). */
@Suppress("UNCHECKED_CAST")
internal fun windowParams(): List<WindowManager.LayoutParams> = windowManagerGlobal().let { g ->
    (g.javaClass.getDeclaredField("mParams").apply { isAccessible = true }.get(g) as List<WindowManager.LayoutParams>).toList()
}

internal fun View.descendants(): Sequence<View> = sequence {
    yield(this@descendants)
    if (this@descendants is ViewGroup) {
        for (i in 0 until childCount) yieldAll(getChildAt(i).descendants())
    }
}
