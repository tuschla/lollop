package app.milestone.qa

import android.content.Context
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.text.TextLayoutResult
import androidx.test.core.app.ApplicationProvider
import app.milestone.AppSettings
import app.milestone.AppTheme
import app.milestone.ConcurrentGoal
import app.milestone.Core
import app.milestone.DarkMode
import app.milestone.DistanceUnitOverride
import app.milestone.Event
import app.milestone.EventDispatcher
import app.milestone.GoalDistance
import app.milestone.HealthScreen
import app.milestone.LiftGoal
import app.milestone.ProgressionCadence
import app.milestone.R
import app.milestone.ViewModel
import org.junit.After
import org.junit.Before
import org.junit.Rule
import org.robolectric.RuntimeEnvironment
import java.util.Locale

/**
 * The app over the REAL core with screen probes shared by [RandomUiWalkTest] and
 * [LayoutRegressionTest]: settling the UI onto the core's model, cut-off and
 * overlapping text, the hold banner's chrome overlaps, and screen dumps. Each test
 * restores the qualifiers, the font scale and the app settings it changed.
 */
abstract class UiProbeTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    protected val ctx: Context get() = ApplicationProvider.getApplicationContext()
    protected lateinit var originalQualifiers: String

    @Before
    fun setUpProbe() {
        Locale.setDefault(Locale.US)
        originalQualifiers = RuntimeEnvironment.getQualifiers()
        QaHarness.reset(ctx)
        resetSettings()
    }

    @After
    fun tearDownProbe() {
        QaHarness.teardown()
        RuntimeEnvironment.setQualifiers(originalQualifiers)
        RuntimeEnvironment.setFontScale(1f)
        resetSettings()
    }

    protected fun resetSettings() {
        AppSettings.theme.value = AppTheme.Paper
        AppSettings.dynamicAccent.value = true
        AppSettings.darkMode.value = DarkMode.System
        AppSettings.distanceUnitOverride.value = DistanceUnitOverride.System
        AppSettings.showRouteMaps.value = true
        clearAppliedCueLoads()
    }

    /** A cue load applied in a loop is process-scoped (keyed by day and exercise;
     *  QaHarness.reset does not clear it): it must not prefill the next walk's or
     *  another test's loop, and a process death drops it. */
    protected fun clearAppliedCueLoads() {
        (app.milestone.AppliedCueLoads.javaClass.getDeclaredField("byItem").apply { isAccessible = true }
            .get(app.milestone.AppliedCueLoads) as MutableMap<*, *>).clear()
    }

    /** Fail with [msg] and what is on screen. */
    protected open fun fail(msg: String): Nothing =
        throw AssertionError("$msg\nscreen:\n" + runCatching { screenDump() }.getOrElse { "<unavailable: $it>" })

    // ── Seeds ───────────────────────────────────────────────────────────────

    protected val morning get() = app.milestone.todayEpochDay() * 86_400L + 8 * 3_600L

    protected fun profile(
        weeklySets: Int = 12,
        runDays: Int = 0,
        runKm: Double = 0.0,
        goal: GoalDistance = GoalDistance.General,
        concurrent: ConcurrentGoal = ConcurrentGoal.Strength,
        age: Double = 30.0,
        lb: Boolean = false,
        health: HealthScreen = HealthScreen(),
    ): Event = Event.SetProfile(
        progressionCadence = ProgressionCadence.WeekToWeek,
        liftGoal = LiftGoal.MaxStrength,
        goalDistance = goal,
        concurrentGoal = concurrent,
        weeklySets = weeklySets,
        runningDaysPerWeek = runDays,
        runningKmPerWeek = runKm,
        advanced = false,
        bodyweightKg = 78.0,
        ageYears = age,
        weightLb = lb,
        health = health,
    )

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
        Event.LogRun(5.0 + i, 28.0 + 5 * i, 0.0, at, at * 1000)
    }

    // ── Settling ────────────────────────────────────────────────────────────

    /** Idle the UI, then wait until the published model agrees with the core (a
     *  tap's event reaches the core on its JNI thread and the view comes back on a
     *  later main-looper turn). Diverging for 15 s is a failure. */
    protected fun settle() {
        val t0 = System.currentTimeMillis()
        while (true) {
            idle()
            val core = Core.currentView()
            val ui = EventDispatcher.model.value
            if (ui != null && entrySignature(ui) == entrySignature(core)) return
            if (ui == null && System.currentTimeMillis() - t0 > 30_000) fail("stuck splash: no model published after 30 s")
            if (ui != null && System.currentTimeMillis() - t0 > 15_000) {
                fail("UI model diverged from the core:\n ui   ${entrySignature(ui)}\n core ${entrySignature(core)}")
            }
            Thread.sleep(20)
        }
    }

    /** One idle pass of [settle]. */
    protected open fun idle() {
        rule.waitForIdle()
        roots()
    }

    protected fun entrySignature(m: ViewModel) =
        "lifts=${m.lifts.size} runs=${m.runs.size} activities=${m.activities.size} blocked=${m.train_blocked} hold=${m.hold_reason} " +
            "liftIds=${m.lifts.map { it.entry_id }.hashCode()} runIds=${m.runs.map { it.entry_id }.hashCode()}"

    /** Wait until the core holds [n] logged sets (an [EventDispatcher] send reaches
     *  it on its consumer), then [settle] the UI onto that view. */
    protected fun awaitCoreLifts(n: Int) {
        val t0 = System.currentTimeMillis()
        while (Core.currentView().lifts.size != n) {
            if (System.currentTimeMillis() - t0 > 10_000) fail("the core holds ${Core.currentView().lifts.size} sets, expected $n")
            rule.waitForIdle()
            Thread.sleep(20)
        }
        settle()
    }

    // ── Screen ──────────────────────────────────────────────────────────────

    protected fun roots(): List<SemanticsNode> = rule.onAllNodes(isRoot()).fetchSemanticsNodes()

    protected fun flatten(n: SemanticsNode): List<SemanticsNode> = listOf(n) + n.children.flatMap { flatten(it) }

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

    /** Visible text the layout cut off (ellipsis, lines past maxLines, an unwrapped
     *  line wider than its box) or that overlaps another visible text in the same
     *  window by more than 2dp each way (same rules as LongHistoryUiTest). A text
     *  containing one of [typed] (what a user typed) is not judged for cut-off. */
    protected fun textProblems(typed: Collection<String> = emptyList()): List<String> {
        val out = ArrayList<String>()
        val d = rule.density.density
        val slack = 2f * d
        // Floating layers ride over the scrolling list by design: the "Log" button
        // (the list's end clearance is RedesignFlowsTest's gate) and a snackbar
        // (its host is the node with a dismiss action), so text under them is not
        // an overlap.
        val floating = roots().firstOrNull()?.let { r ->
            flatten(r).filter { n ->
                (
                    n.config.getOrNull(SemanticsActions.OnClick) != null &&
                        n.config.getOrNull(SemanticsProperties.ContentDescription).orEmpty().any { it == str(R.string.main_log) }
                    ) || n.config.getOrNull(SemanticsActions.Dismiss) != null
            }.map { it.boundsInRoot }
        }.orEmpty()
        rule.onAllNodes(isRoot(), useUnmergedTree = true).fetchSemanticsNodes().forEachIndexed { ri, root ->
            val boxes = ArrayList<Pair<String, Rect>>()
            fun visit(n: SemanticsNode) {
                // A composed-but-unplaced group (SetEditor's shownOnly) draws nothing.
                if (!n.layoutInfo.isPlaced) return
                val b = n.boundsInRoot
                if (b.width > 0f && b.height > 0f) {
                    n.config.getOrNull(SemanticsActions.GetTextLayoutResult)?.action?.let { get ->
                        val layouts = ArrayList<TextLayoutResult>()
                        if (get(layouts)) {
                            layouts.filter { it.cutsOffText }
                                .filterNot { l -> typed.any { it.isNotBlank() && l.layoutInput.text.text.contains(it.take(20)) } }
                                .forEach { l ->
                                    val why = when {
                                        l.didOverflowHeight -> "taller than its ${(l.size.height / d).toInt()}dp box"
                                        (0 until l.lineCount).any { l.isLineEllipsized(it) } -> "ellipsized at ${l.lineCount} line(s)"
                                        else -> "one line wider than its ${(l.size.width / d).toInt()}dp box"
                                    }
                                    out += "cut off: \"${l.layoutInput.text.text}\" ($why, font ${l.layoutInput.style.fontSize}, node at ${(b.left / d).toInt()},${(b.top / d).toInt()}dp)"
                                }
                        }
                    }
                    n.config.getOrNull(SemanticsProperties.Text)?.let { t -> boxes += t.joinToString(" ") { it.text } to b }
                }
                n.children.forEach(::visit)
            }
            visit(root)
            for (i in boxes.indices) for (j in i + 1 until boxes.size) {
                val a = boxes[i].second
                val b = boxes[j].second
                val w = minOf(a.right, b.right) - maxOf(a.left, b.left)
                val hgt = minOf(a.bottom, b.bottom) - maxOf(a.top, b.top)
                val under = ri == 0 && floating.any { f -> a.overlaps(f) || b.overlaps(f) }
                // The M3 date picker keeps its year grid composed behind the day grid.
                val yearGrid = listOf(boxes[i].first, boxes[j].first).any { it.startsWith("Navigate to year") }
                if (w > slack && hgt > slack && !under && !yearGrid) {
                    fun Rect.dp() = "(${(left / d).toInt()},${(top / d).toInt()})-(${(right / d).toInt()},${(bottom / d).toInt()})dp"
                    out += "overlap: \"${boxes[i].first}\" ${a.dp()} × \"${boxes[j].first}\" ${b.dp()}"
                }
            }
        }
        return out.distinct()
    }

    /** The activity window's chrome that could be laid over the banner: the
     *  bottom nav, the side rail and the Log button, where placed and visible. */
    private fun chromeNodes(): List<SemanticsNode> = allUnmerged(roots().first()).filter { n ->
        n.layoutInfo.isPlaced && n.isVisible() && n.config.getOrNull(SemanticsProperties.TestTag) in setOf("bottom-nav", "nav-rail", "log-fab")
    }

    /** Every intersection of the safety banner (its card, and each of its nodes'
     *  bounds and touch bounds) with [chromeNodes], beyond half a dp. */
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

    private val TextLayoutResult.cutsOffText: Boolean
        get() = didOverflowHeight ||
            (0 until lineCount).any { isLineEllipsized(it) } ||
            (!layoutInput.softWrap && multiParagraph.intrinsics.maxIntrinsicWidth > size.width + 0.5f)
}
