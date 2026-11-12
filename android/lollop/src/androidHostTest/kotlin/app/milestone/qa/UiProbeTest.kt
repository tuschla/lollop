package app.milestone.qa

import android.content.Context
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.ComposeTimeoutException
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.SemanticsNodeInteraction
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.lifecycle.Lifecycle
import androidx.test.espresso.EspressoException
import androidx.test.core.app.ApplicationProvider
import app.milestone.Core
import app.milestone.DistanceUnit
import app.milestone.Event
import app.milestone.EventDispatcher
import app.milestone.HealthScreen
import app.milestone.R
import app.milestone.ReadinessSignal
import app.milestone.ViewModel
import app.milestone.train_blocked
import org.junit.After
import org.junit.Before
import org.junit.Rule
import org.robolectric.RuntimeEnvironment

/**
 * The app over the REAL core with the screen probes its UI test classes share:
 * seeded start states, settling the UI onto the core's model, node lookup, the
 * crash guard, a relaunch after Back left the app, small touch targets, the hold
 * banner's chrome overlaps, and screen dumps. Each test restores the qualifiers
 * and the font scale it changed.
 */
abstract class UiProbeTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    protected val ctx: Context get() = ApplicationProvider.getApplicationContext()
    protected lateinit var originalQualifiers: String

    @Before
    fun setUpProbe() {
        originalQualifiers = RuntimeEnvironment.getQualifiers()
        QaHarness.reset(ctx)
    }

    @After
    fun tearDownProbe() {
        QaHarness.teardown()
        RuntimeEnvironment.setQualifiers(originalQualifiers)
        RuntimeEnvironment.setFontScale(1f)
        QaHarness.freshProcess()
    }

    protected fun freshProcess() {
        QaHarness.teardown()
        RuntimeEnvironment.setQualifiers(originalQualifiers)
        RuntimeEnvironment.setFontScale(1f)
        QaHarness.reset(ctx)
    }

    protected class ProbeFailure(msg: String) : AssertionError(msg)

    protected open fun fail(msg: String): Nothing =
        throw ProbeFailure("$msg\nscreen:\n" + runCatching { screenDump() }.getOrElse { "<unavailable: $it>" })

    /** Run a crawled or random action on a node the probe saw a moment ago; false
     *  when the test framework refused it, which only skips the action: the
     *  Compose test API's [AssertionError] (the node left, moved or is no longer
     *  unique) or an [EspressoException] (Back found no focused window). Anything
     *  else thrown is the app's and fails the test as a crash. */
    protected fun guard(what: String, block: () -> Unit): Boolean = try {
        block()
        true
    } catch (e: ProbeFailure) {
        throw e
    } catch (e: Throwable) {
        if (e !is AssertionError && e !is EspressoException) {
            fail("crash during $what: $e\n${e.stackTraceToString().lines().take(30).joinToString("\n")}")
        }
        onSkipped(what, e)
        false
    }

    protected open fun onSkipped(what: String, e: Throwable) {}

    /** Back on the root screen left the app: open it again (warm start). True when it did. */
    protected fun relaunchIfLeft(): Boolean {
        val sc = lastScenario ?: return false
        val left = sc.state == Lifecycle.State.DESTROYED || run {
            var finishing = false
            sc.onActivity { finishing = it.isFinishing }
            finishing
        }
        if (!left) return false
        sc.close()
        lastScenario = launchMain()
        return true
    }

    protected val morning get() = app.milestone.todayEpochDay() * 86_400L + 8 * 3_600L

    protected fun liftDays(vararg daysAgo: Int): List<Event> = daysAgo.flatMap { d ->
        val t = QaHarness.daysAgoAtMidday(d)
        listOf("Back Squat" to 100.0, "Bench Press" to 70.0, "Deadlift" to 120.0).flatMapIndexed { ei, (ex, kg) ->
            (0 until 3).map { s ->
                val at = t + ei * 600L + s * 150L
                Event.LogSet(ex, kg, 5, 8.0, at, at * 1000)
            }
        }
    }

    protected fun runs(vararg daysAgo: Int): List<Event> = daysAgo.mapIndexed { i, d ->
        val at = QaHarness.daysAgoAtMidday(d) + 3_600L
        Event.LogRun(5.0 + i, 28.0 + 5 * i, 0.0, observedAt = at, entryId = at * 1000)
    }

    /** A seeded start state; [legacyDistanceUnit] is the unit an upgrading install
     *  kept in its settings store (boot moves it into the profile). */
    enum class Start(val legacyDistanceUnit: DistanceUnit? = null) {
        FreshInstall, Runner, Lifter, Hybrid, Calisthenics, Under18, PainHold, Illness, Pregnancy, LongHistory, MilesLb(DistanceUnit.Mi),
    }

    private fun seedEvents(start: Start): List<Event> = when (start) {
        Start.FreshInstall -> emptyList()
        Start.Runner -> listOf(QaHarness.tenKRunnerProfile()) + runs(1, 3, 6, 9, 40)
        Start.Lifter -> listOf(QaHarness.strengthProfile()) + liftDays(1, 3, 5, 8, 35)
        Start.Hybrid -> listOf(QaHarness.hybridFiveKProfile()) + liftDays(2, 4) + runs(1, 3, 5)
        Start.Calisthenics -> listOf(QaHarness.calisthenicsProfile(), QaHarness.loggedHold(QaHarness.daysAgoAtMidday(1)), QaHarness.loggedHold(QaHarness.daysAgoAtMidday(3)))
        Start.Under18 -> listOf(QaHarness.youthProfile())
        Start.PainHold -> listOf(QaHarness.strengthProfile()) + liftDays(2) + Event.SubmitReadiness(ReadinessSignal.Pain, 1.0, morning)
        Start.Illness -> listOf(QaHarness.hybridFiveKProfile()) + runs(2) + Event.SubmitReadiness(ReadinessSignal.Illness, 2.0, morning)
        Start.Pregnancy -> listOf(QaHarness.strengthProfile().copy(ageYears = 33.0, health = HealthScreen(pregnant = true)))
        Start.LongHistory -> LongHistorySeed.build(days = 150, fixEverySec = 60L).events
        Start.MilesLb -> listOf(QaHarness.strengthProfileLb()) + liftDays(1, 4) + runs(2, 6)
    }

    /** Put [start] (plus [extra] events) on disk for the next launch. */
    protected fun seedStart(start: Start, extra: List<Event> = emptyList()) {
        start.legacyDistanceUnit?.let { QaHarness.legacyDistanceUnit(ctx, it) }
        (seedEvents(start) + extra).takeIf { it.isNotEmpty() }?.let { QaHarness.seedLog(ctx, it) }
    }

    /** Idle the UI, then wait until the published model agrees with the core (a
     *  tap's event reaches the core on its JNI thread and the view comes back on a
     *  later main-looper turn). */
    protected fun settle() {
        try {
            rule.waitUntil(30_000) {
                idle()
                EventDispatcher.model.value != null
            }
        } catch (e: ComposeTimeoutException) {
            fail("stuck splash: no model published after 30 s")
        }
        var ui = ""
        var core = ""
        try {
            rule.waitUntil(15_000) {
                idle()
                ui = EventDispatcher.model.value?.let(::entrySignature) ?: "<none>"
                core = entrySignature(Core.currentView())
                ui == core
            }
        } catch (e: ComposeTimeoutException) {
            fail("UI model diverged from the core:\n ui   $ui\n core $core")
        }
    }

    protected fun idle() {
        rule.waitForIdle()
        roots()
    }

    protected fun entrySignature(m: ViewModel) =
        "lifts=${m.lifts.size} runs=${m.runs.size} activities=${m.activities.size} blocked=${m.train_blocked} hold=${m.hold_reason} " +
            "liftIds=${m.lifts.map { it.entry_id }.hashCode()} runIds=${m.runs.map { it.entry_id }.hashCode()}"

    /** Wait until the core holds [n] logged sets (an [EventDispatcher] send reaches
     *  it on its consumer), then [settle] the UI onto that view. */
    protected fun awaitCoreLifts(n: Int) {
        try {
            rule.waitUntil(10_000) {
                rule.waitForIdle()
                Core.currentView().lifts.size == n
            }
        } catch (e: ComposeTimeoutException) {
            fail("the core holds ${Core.currentView().lifts.size} sets, expected $n")
        }
        settle()
    }

    protected fun roots(): List<SemanticsNode> = rule.onAllNodes(isRoot()).fetchSemanticsNodes()

    protected fun allUnmerged(root: SemanticsNode): List<SemanticsNode> {
        val unmerged = rule.onAllNodes(isRoot(), useUnmergedTree = true).fetchSemanticsNodes()
            .firstOrNull { it.id == root.id } ?: return emptyList()
        return flatten(unmerged)
    }

    protected fun allUnmergedRoots(): List<SemanticsNode> = rule.onAllNodes(isRoot(), useUnmergedTree = true).fetchSemanticsNodes()

    protected fun SemanticsNode.isVisible() = boundsInRoot.width > 0f && boundsInRoot.height > 0f

    protected fun labelOf(n: SemanticsNode): String =
        (n.config.getOrNull(SemanticsProperties.Text).orEmpty().map { it.text } +
            n.config.getOrNull(SemanticsProperties.ContentDescription).orEmpty() +
            listOfNotNull(n.config.getOrNull(SemanticsProperties.EditableText)?.text))
            .firstOrNull { it.isNotBlank() }?.take(60) ?: (n.config.getOrNull(SemanticsProperties.TestTag) ?: "")

    /** The selected bottom-nav destination, or null off the main scaffold. */
    protected fun selectedTab(): String? {
        val tabs = setOf(str(R.string.main_dest_today), str(R.string.main_dest_history), str(R.string.main_dest_profile))
        return flatten(roots().first()).firstOrNull { n ->
            n.config.getOrNull(SemanticsProperties.Selected) == true && labelOf(n) in tabs
        }?.let { labelOf(it) }
    }

    /** The top window holds an open numeric keypad the user cannot see. */
    protected fun offscreenKeypad(): Boolean = keypadNodes().let { k -> k.isNotEmpty() && k.none { it.isVisible() } }

    protected fun keypadComposed(): Boolean = keypadNodes().isNotEmpty()

    private fun keypadNodes(): List<SemanticsNode> = roots().lastOrNull()?.let { top ->
        allUnmerged(top).filter { it.config.getOrNull(SemanticsProperties.TestTag) == "numeric-keypad" }
    }.orEmpty()

    /** [n] itself: ids are per window, so match the id, the bounds and the window root too. */
    protected fun node(n: SemanticsNode): SemanticsNodeInteraction {
        fun SemanticsNode.top(): SemanticsNode = generateSequence(this) { it.parent }.last()
        val root = n.top()
        val all = rule.onAllNodes(
            SemanticsMatcher("node #${n.id}") {
                it.id == n.id && it.boundsInRoot == n.boundsInRoot && it.top().let { t -> t.id == root.id && t.size == root.size }
            },
        )
        return all[(all.fetchSemanticsNodes().size - 1).coerceAtLeast(0)]
    }

    /** What a user perceives: the windows, the selected tab, and the top window's texts. */
    protected fun fingerprint(): String {
        val roots = roots()
        val top = roots.lastOrNull() ?: return "<none>"
        val texts = flatten(top).filter { it.isVisible() }.map { labelOf(it) + "@" + it.boundsInRoot.top.toInt() }
        return "${roots.size}|${selectedTab()}|${texts.hashCode()}"
    }

    protected fun screenDump(): String = roots().mapIndexed { i, r ->
        "window $i: " + flatten(r).filter { it.isVisible() }.map { labelOf(it) }.filter { it.isNotBlank() }.distinct().take(40).joinToString(" | ")
    }.joinToString("\n")

    /** The activity window's chrome that could be laid over the banner: the
     *  bottom nav, the side rail and the Log button, where placed and visible. */
    private fun chromeNodes(): List<SemanticsNode> = allUnmerged(roots().first()).filter { n ->
        n.layoutInfo.isPlaced && n.isVisible() && n.config.getOrNull(SemanticsProperties.TestTag) in setOf("bottom-nav", "nav-rail", "log-fab")
    }

    /** Every intersection of the safety banner (its card, and each of its nodes'
     *  bounds and touch bounds) with [chromeNodes]. */
    protected fun chromeOverBanner(): List<String> {
        val all = allUnmerged(roots().first())
        val banner = all.firstOrNull { it.layoutInfo.isPlaced && it.config.getOrNull(SemanticsProperties.TestTag) == "safety-banner" }
            ?: return emptyList()
        val d = rule.density.density
        fun Rect.dp() = "(${(left / d).toInt()},${(top / d).toInt()})-(${(right / d).toInt()},${(bottom / d).toInt()})dp"
        val parts = flatten(banner).filter { it.layoutInfo.isPlaced && it.isVisible() }
            .flatMap { n -> listOf(n to n.boundsInRoot, n to n.touchBoundsInRoot) }
        val out = ArrayList<String>()
        for (c in chromeNodes()) {
            val cb = c.boundsInRoot
            for ((n, b) in parts) {
                val w = minOf(b.right, cb.right) - maxOf(b.left, cb.left)
                val h = minOf(b.bottom, cb.bottom) - maxOf(b.top, cb.top)
                if (w > 0.5f * d && h > 0.5f * d) {
                    val name = if (n.id == banner.id) "the banner card" else "\"${labelOf(n).ifBlank { "<node>" }}\""
                    out += "${c.config.getOrNull(SemanticsProperties.TestTag)} ${cb.dp()} × $name ${b.dp()}"
                }
            }
        }
        return out.distinct()
    }

    /** Fully visible actions whose touch target (own size or touch expansion) is
     *  under [UxInvariants.MIN_TOUCH]; a partly scrolled-out action is judged once
     *  fully in view. */
    protected fun smallTargets(): List<String> {
        val d = rule.density.density
        val min = UxInvariants.MIN_TOUCH.value
        return roots().flatMap { flatten(it) }.filter { n ->
            n.config.getOrNull(SemanticsActions.OnClick) != null && n.isVisible() &&
                n.boundsInRoot.width + 1f >= n.size.width && n.boundsInRoot.height + 1f >= n.size.height
        }.mapNotNull { n ->
            val w = maxOf(n.size.width.toFloat(), n.touchBoundsInRoot.width) / d
            val h = maxOf(n.size.height.toFloat(), n.touchBoundsInRoot.height) / d
            if (w + 0.5f < min || h + 0.5f < min) "\"${labelOf(n)}\" is ${w.toInt()}x${h.toInt()}dp (<${min.toInt()}dp)" else null
        }
    }
}
