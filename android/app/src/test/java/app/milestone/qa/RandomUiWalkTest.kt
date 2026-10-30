package app.milestone.qa

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.click
import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performTextReplacement
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeDown
import androidx.compose.ui.test.swipeLeft
import androidx.compose.ui.test.swipeRight
import androidx.compose.ui.test.swipeUp
import androidx.lifecycle.Lifecycle
import androidx.test.espresso.Espresso
import app.milestone.AppSettings
import app.milestone.AppTheme
import app.milestone.Core
import app.milestone.DarkMode
import app.milestone.DistanceUnitOverride
import app.milestone.EventDispatcher
import app.milestone.HistoryFocus
import app.milestone.R
import app.milestone.UiMessages
import app.milestone.ViewModel
import app.milestone.groupLiftSessions
import app.milestone.showLifting
import app.milestone.showRunning
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import java.time.ZoneId

/**
 * Seeded, model-based random UI exploration ("monkey with an oracle") over the
 * REAL core: from each of several starting states it picks a random enabled
 * clickable / long-clickable / text-input / scrollable node in the TOP window
 * (weighted), or a whole-app gesture (Back, swipe a sheet down, recreate, rotate,
 * font scale, theme, dark mode, process death), and after every step asserts:
 *
 *  - no crash (an exception thrown from app code), no stuck splash (a model is
 *    published), no empty screen;
 *  - [UxInvariants] (labels, 48dp targets, clipped actions, forbidden text) and no
 *    cut-off / overlapping text;
 *  - at most one bottom sheet and at most one dialog open at once;
 *  - Back always makes progress (the screen changes or the app leaves);
 *  - the UI's model equals the core's ([Core.currentView]) once settled; History
 *    renders one row per core entry; Today never shows a session while the core
 *    holds training; process death + relaunch replays the same entries.
 *
 * The bugs the walk found are pinned in [LayoutRegressionTest]; the walk
 * tolerates none of them, so it guards them too.
 *
 * Replay: every failure prints the start state, seed, step, the action trace and
 * the environment that replays it.
 */
@RunWith(RobolectricTestRunner::class)
class RandomUiWalkTest : UiProbeTest() {

    @Test fun freshInstall() = walk(Start.FreshInstall)
    @Test fun runner() = walk(Start.Runner)
    @Test fun lifter() = walk(Start.Lifter)
    @Test fun hybrid() = walk(Start.Hybrid)
    @Test fun calisthenics() = walk(Start.Calisthenics)
    @Test fun under18() = walk(Start.Under18)
    @Test fun painHold() = walk(Start.PainHold)
    @Test fun illness() = walk(Start.Illness)
    @Test fun pregnancy() = walk(Start.Pregnancy)
    @Test fun longHistory() = walk(Start.LongHistory)
    @Test fun milesAndPounds() = walk(Start.MilesLb)

    // ── The walk ────────────────────────────────────────────────────────────

    private lateinit var start: Start
    private var seed = 0L
    private var step = 0
    private val trace = ArrayList<String>()
    private val typed = HashSet<String>()
    private var fontScale = 1f
    private var landscape = false
    private val screensSeen = HashSet<String>()

    private fun walk(s: Start) {
        val only = System.getenv("UIWALK_STATE")
        assumeTrue("UIWALK_STATE=$only selects another walk", only.isNullOrBlank() || only.equals(s.name, ignoreCase = true))
        val steps = System.getenv("UIWALK_STEPS")?.toIntOrNull() ?: DEFAULT_STEPS
        val fixed = System.getenv("UIWALK_SEED")?.toLongOrNull()
        // Long mode: UIWALK_SEEDS=n walks n consecutive seeds from each start.
        val count = if (fixed != null) 1 else System.getenv("UIWALK_SEEDS")?.toIntOrNull() ?: 1
        // UIWALK_FIRST=k starts the long mode at the k-th seed (fresh seeds per run).
        val first = System.getenv("UIWALK_FIRST")?.toIntOrNull() ?: 0
        // UIWALK_KEEPGOING=1 reports every failing seed instead of stopping at the first.
        val keepGoing = System.getenv("UIWALK_KEEPGOING") == "1"
        val failures = ArrayList<String>()
        for (k in 0 until count) {
            if (k > 0) freshProcess()
            try {
                walkOnce(s, fixed ?: (BASE_SEED + s.ordinal * 7_919L + (first + k) * 104_729L), steps)
            } catch (e: AssertionError) {
                // A framework assertion (not [fail]) carries no seed: give it the replay line.
                val err = if (e is ProbeFailure) e else walkError("${e.javaClass.simpleName}: ${e.message}").apply { initCause(e) }
                if (!keepGoing) throw err
                println("UIWALK-FAIL " + err.message)
                failures += err.message.orEmpty().lineSequence().first()
            }
        }
        if (failures.isNotEmpty()) throw AssertionError("${failures.size} of $count walks failed:\n" + failures.joinToString("\n"))
    }

    private fun walkOnce(s: Start, walkSeed: Long, steps: Int) {
        start = s
        seed = walkSeed
        step = 0
        trace.clear()
        screensSeen.clear()
        typed.clear()
        fontScale = 1f
        landscape = false
        val rng = java.util.Random(seed)

        if (s == Start.MilesLb) AppSettings.setDistanceUnitOverride(ctx, DistanceUnitOverride.Mi)
        seedEvents(s)?.let { QaHarness.seedLog(ctx, it) }
        trace += "boot $s"
        guard("boot") {
            if (s == Start.FreshInstall) rule.launchAppExpectingSetup() else rule.launchApp()
        }
        settle()
        checkAll(afterBack = null)

        for (i in 1..steps) {
            step = i
            val before = BackBaseline(fingerprint(), popupOpen(), textFieldFocused())
            screensSeen += before.fingerprint
            val wasBack = actOnce(rng)
            settle()
            checkAll(afterBack = if (wasBack) before else null)
            if (DEBUG) println("UIWALK step $i\n${screenDump()}")
        }
        step = steps + 1
        endOfWalkChecks()
    }

    /** One random action; returns true when it was a Back press. */
    private fun actOnce(rng: java.util.Random): Boolean {
        val roots = roots()
        // A popup (the drag handle's tooltip) does not block the window under it.
        val top = (listOf(roots.first()) + roots.drop(1).filterNot { it.isPopup() }).last()
        val overlay = roots.size > 1
        val cands = ArrayList<Pair<Int, () -> Unit>>()
        val descs = ArrayList<String>()
        fun add(w: Int, d: String, a: () -> Unit) {
            cands += w to a
            descs += d
        }

        for (n in flatten(top)) {
            if (!n.isVisible() || n.config.getOrNull(SemanticsProperties.Disabled) != null) continue
            val label = labelOf(n).ifBlank {
                val b = n.boundsInRoot
                "<${n.config.getOrNull(SemanticsProperties.Role) ?: "node"} #${n.id} @${b.left.toInt()},${b.top.toInt()} ${b.width.toInt()}x${b.height.toInt()}px>"
            }
            val c = n.config
            if (c.getOrNull(SemanticsActions.OnClick) != null) {
                val isTab = !overlay && c.getOrNull(SemanticsProperties.Role) == Role.Tab &&
                    label in setOf(str(R.string.main_dest_today), str(R.string.main_dest_history), str(R.string.main_dest_profile))
                add(if (isTab) 30 else 10, "tap \"$label\"") { node(n).performTouchInput { click() } }
                add(1, "long-press \"$label\"") { node(n).performTouchInput { longClick() } }
            } else if (c.getOrNull(SemanticsActions.OnLongClick) != null) {
                add(3, "long-press \"$label\"") { node(n).performTouchInput { longClick() } }
            }
            if (c.getOrNull(SemanticsActions.SetText) != null) {
                val text = TEXTS[rng.nextInt(TEXTS.size)]
                add(12, "type ${quote(text)} into \"$label\"") {
                    if (text.length >= 3) typed += text.trim()
                    node(n).performTextReplacement(text)
                }
            }
            if (c.getOrNull(SemanticsActions.ScrollBy) != null) {
                val dir = rng.nextInt(4)
                add(8, "scroll ${listOf("down", "up", "far down", "fling up")[dir]} \"$label\"") {
                    val h = n.boundsInRoot.height
                    when (dir) {
                        0 -> node(n).performSemanticsAction(SemanticsActions.ScrollBy) { it(0f, h * 0.7f) }
                        1 -> node(n).performSemanticsAction(SemanticsActions.ScrollBy) { it(0f, -h * 0.7f) }
                        2 -> node(n).performTouchInput { swipeUp() }
                        else -> node(n).performTouchInput { swipeDown() }
                    }
                }
            }
        }
        // A finger anywhere on the top window (custom pointer-input widgets such as
        // the rating track have no click action of their own).
        val tb = top.boundsInRoot
        val px = tb.left + rng.nextFloat() * tb.width
        val py = tb.top + rng.nextFloat() * tb.height
        add(4, "tap at (${px.toInt()},${py.toInt()})px") { node(top).performTouchInput { click(Offset(px - tb.left, py - tb.top)) } }
        add(1, "swipe sideways") { node(top).performTouchInput { if (rng.nextBoolean()) swipeLeft() else swipeRight() } }
        if (overlay) add(6, "swipe sheet down") { node(top).performTouchInput { swipeDown() } }
        add(8, "BACK") { Espresso.pressBackUnconditionally() }
        add(2, "recreate activity") { recreate() }
        add(2, "rotate") {
            landscape = !landscape
            RuntimeEnvironment.setQualifiers(if (landscape) "w891dp-h411dp-land-xxhdpi" else "w411dp-h891dp-port-xxhdpi")
            recreate()
        }
        val scale = FONT_SCALES[rng.nextInt(FONT_SCALES.size)]
        add(2, "font scale $scale") {
            fontScale = scale
            RuntimeEnvironment.setFontScale(scale)
            recreate()
        }
        val theme = AppTheme.entries[rng.nextInt(AppTheme.entries.size)]
        add(1, "theme $theme") { AppSettings.setTheme(ctx, theme) }
        val dark = DarkMode.entries[rng.nextInt(DarkMode.entries.size)]
        add(1, "dark mode $dark") { AppSettings.setDarkMode(ctx, dark) }
        add(1, "process death + relaunch") { processDeath() }

        val total = cands.sumOf { it.first }
        var pick = rng.nextInt(total)
        var idx = 0
        while (pick >= cands[idx].first) {
            pick -= cands[idx].first
            idx++
        }
        val desc = descs[idx]
        trace += "#$step $desc"
        guard(desc) { cands[idx].second() }
        // Leaving the app is Back's progress; the reopened app is checked as a new screen.
        return desc == "BACK" && !relaunchIfLeft().also { if (it) trace += "   (app left; relaunched)" }
    }

    override fun onSkipped(what: String, e: Throwable) {
        trace += "   (skipped: ${e.javaClass.simpleName}: ${e.message?.lineSequence()?.firstOrNull()?.take(160)})"
    }

    // ── Lifecycle actions ───────────────────────────────────────────────────

    private fun recreate() {
        checkNotNull(lastScenario).recreate()
        rule.waitUntil(15_000) { roots().isNotEmpty() }
    }

    /** Kill the process (every singleton back to a fresh process, the durable log
     *  and prefs kept), relaunch, and require the same entries back. */
    private fun processDeath() {
        val beforeModel = EventDispatcher.model.value
        QaHarness.teardown()
        QaHarness.resetCore()
        EventDispatcher.resetForTest()
        app.milestone.RunSession.reset()
        UiMessages.clear()
        HistoryFocus.consume()
        clearAppliedCueLoads()
        AppSettings.load(ctx)
        lastScenario = launchMain()
        rule.waitUntil(30_000) { roots().isNotEmpty() && EventDispatcher.model.value != null }
        settle()
        val after = EventDispatcher.model.value
        if (beforeModel != null && after != null) {
            val b = entrySignature(beforeModel)
            val a = entrySignature(after)
            if (a != b) fail("process death changed the entries:\n before $b\n after  $a")
        }
    }

    // ── Settling ────────────────────────────────────────────────────────────

    /** Idle with framework hiccups skipped and an app crash failing the walk. */
    override fun idle() {
        guard("idle") { rule.waitForIdle() }
        guard("idle") { roots() }
    }

    // ── Invariants ──────────────────────────────────────────────────────────

    /** What Back's progress is judged against: the screen before it, plus what
     *  a Back may legitimately spend itself on without a visible change. */
    private class BackBaseline(val fingerprint: String, val popup: Boolean, val textFocused: Boolean)

    private fun checkAll(afterBack: BackBaseline?) {
        val roots = roots()
        if (roots.isEmpty()) fail("no window on screen")
        if (EventDispatcher.model.value == null) fail("stuck splash: no model published")

        // Non-empty screen: something labelled is visible.
        val labelled = roots.flatMap { flatten(it) }.count { it.isVisible() && labelOf(it).isNotBlank() }
        if (labelled == 0) fail("empty screen: no visible labelled node")

        // One sheet / one dialog at most. A popup (a tooltip, a dropdown) is
        // neither, nor is a date/time picker over the form whose date row opened
        // it, nor the "Discard this entry?" confirm over the editor it guards (the
        // loop's in-place Edit set is a Dialog; Back on it asks before dropping).
        // Two sheets are designed stacks when the top one explains the sheet below:
        // the run rating over its run's detail (RedesignFlowsTest: Back returns to
        // the detail), the glossary and the evidence-grade legend over the sheet
        // whose "?" opened them (RedesignFlowsTest: Back closes the glossary and
        // leaves the calculator open).
        val overlays = roots.drop(1)
        val sheetRoots = overlays.filter { it.isSheet() }
        val dialogs = overlays.count { !it.isSheet() && it.isDialog() && !it.isPicker() && !it.isDiscardConfirm() }
        val stackTitles = setOf(str(R.string.run_rate_title), str(R.string.main_glossary_title), str(R.string.main_evidence_legend_title))
        val designedStack = sheetRoots.size == 2 && flatten(sheetRoots.last()).any { labelOf(it) in stackTitles }
        if ((sheetRoots.size > 1 && !designedStack) || dialogs > 1) {
            fail("${sheetRoots.size} bottom sheets and $dialogs dialogs open at once:\n${screenDump()}")
        }

        // UX invariants. The touch-target rule is judged on fully visible actions
        // only ([smallTargets]): UxInvariants reads the CLIPPED touch bounds, so a
        // 40dp chip with its 48dp touch expansion half scrolled under a sheet's
        // edge reads as small. Two cards sharing one plan-item tag (the hero item
        // and the same item in the expanded week-strip day) are not one card.
        val sharedCards = allUnmergedRoots().flatMap { flatten(it) }
            .mapNotNull { it.config.getOrNull(SemanticsProperties.TestTag) }
            .filter { it.startsWith("plan-item:") }
            .groupingBy { it }.eachCount().filterValues { it > 1 }.keys
        val ux = UxInvariants.check(rule, rule.density)
            .filter { it.kind in UX_KINDS }
            .filterNot { v -> typed.any { t -> t.isNotBlank() && t.take(20) in v.detail } }
            .filterNot { v -> v.kind == "duplicate-action-in-card" && sharedCards.any { "in card \"$it\"" in v.detail } }
            .map { it.toString() } + smallTargets().map { "[small-touch-target] $it" }
        if (ux.isNotEmpty()) fail("UX invariants:\n" + ux.take(12).joinToString("\n"))
        val text = textProblems(typed)
        if (text.isNotEmpty()) fail("text cut off / overlapping (${text.size}):\n" + text.take(12).joinToString("\n"))
        // The pinned hold banner is never overlapped (INVARIANT 3): not by the
        // bottom nav, not by the Log button, text or not.
        val overBanner = chromeOverBanner()
        if (overBanner.isNotEmpty()) fail("the hold banner is overlapped:\n" + overBanner.joinToString("\n"))

        // Back makes progress. Not judged when Back went to a focused text field
        // (on a device it hides the keyboard; Robolectric shows none) or to a popup
        // such as the sheet drag handle's tooltip.
        if (afterBack != null && !afterBack.popup && !afterBack.textFocused &&
            fingerprint() == afterBack.fingerprint && lastScenario?.state != Lifecycle.State.DESTROYED
        ) {
            fail("Back did nothing: the screen is unchanged:\n${screenDump()}")
        }

        val m = EventDispatcher.model.value!!
        val onMain = roots.size == 1 && selectedTab() != null

        // Today never shows a session while the core holds training: no plan item
        // outside the week strip, and inside it only a day the core still plans
        // (a pain hold blanks the next session; other days keep their items, and an
        // illness blanks every upcoming day - plan_week.rs).
        if (m.train_blocked && selectedTab() == str(R.string.main_dest_today)) {
            val held = m.next_session?.epoch_day
            val stillPlanned = m.week_plan.filter { it.epoch_day != held && it.epoch_day != m.today_epoch_day }
                .flatMap { d -> d.items.map { it.exercise } }
            val bad = ArrayList<String>()
            fun visit(n: SemanticsNode, inWeek: Boolean) {
                if (!n.layoutInfo.isPlaced) return
                val tag = n.config.getOrNull(SemanticsProperties.TestTag)
                val week = inWeek || tag == "this-week"
                if (tag != null && tag.startsWith("plan-item:") && n.isVisible()) {
                    val ex = tag.removePrefix("plan-item:")
                    val ok = week && stillPlanned.isNotEmpty() && (ex in stillPlanned || ex == "run" || ex == "check")
                    if (!ok) bad += tag + if (week) " (week strip)" else ""
                }
                n.children.forEach { visit(it, week) }
            }
            allUnmerged(roots.first()).firstOrNull()?.let { visit(it, false) }
            if (bad.isNotEmpty()) fail("Today shows plan items $bad while training is held (${m.hold_reason})")
        }

        // History renders one row per core entry.
        if (onMain && selectedTab() == str(R.string.main_dest_history)) checkHistoryRows(m)
    }

    /** The History list's item count equals what the core's entries need: the fixed
     *  rows above the stream, then one card per lift SESSION (local day) or run, a
     *  month header per month, or the empty state. */
    private fun checkHistoryRows(m: ViewModel) {
        val count = historyItemCount() ?: return
        val zone = ZoneId.systemDefault()
        val lifts = groupLiftSessions(m.lifts, zone)
        val hasAny = m.lifts.isNotEmpty() || m.runs.isNotEmpty()
        val showSegment = (m.showLifting() || m.lifts.isNotEmpty()) && (m.showRunning() || m.runs.isNotEmpty())
        val records = m.run_progress?.let { it.longest_run_km > 0.0 || it.best_efforts.isNotEmpty() } == true
        fun months(stamps: List<Long>): Int {
            var prev: String? = null
            var n = 0
            stamps.forEach { s ->
                val mo = if (s <= 0L) "" else java.time.Instant.ofEpochSecond(s).atZone(zone).let { "${it.year}-${it.monthValue}" }
                if (mo.isNotBlank() && mo != prev) n++
                prev = mo
            }
            return n
        }
        fun expected(runsFilter: Boolean): Int {
            var n = (if (hasAny) 2 else 0) + (if (showSegment) 1 else 0)
            if (runsFilter) {
                if (m.runs.isNotEmpty()) n++
                if (records) n++
                n += if (m.runs.isEmpty()) 1 else m.runs.size + months(m.runs.asReversed().map { it.observed_at })
            } else {
                n += if (lifts.isEmpty()) 1 else lifts.size + months(lifts.map { it.firstObservedAt })
            }
            return n
        }
        val ok = listOf(expected(false), expected(true))
        if (count !in ok) {
            fail("History lists $count rows for ${m.lifts.size} sets on ${lifts.size} days and ${m.runs.size} runs (expected one of $ok)")
        }
    }

    /** The History LazyColumn's item count, read from its scroll-to-index bound;
     *  null when there is no list, or when it moved or left in the frame the
     *  lookup waited for (`node` matches id AND bounds): the next step counts. */
    private fun historyItemCount(): Int? {
        val lists = flatten(roots().first()).filter { it.config.getOrNull(SemanticsActions.ScrollToIndex) != null && it.isVisible() }
        val list = lists.maxByOrNull { it.boundsInRoot.height } ?: return null
        val target = node(list)
        return try {
            target.performSemanticsAction(SemanticsActions.ScrollToIndex) { it(Int.MAX_VALUE) }
            null
        } catch (e: IllegalArgumentException) {
            Regex("""out of bounds \[0, (\d+)\)""").find(e.message.orEmpty())?.groupValues?.get(1)?.toInt()
        } catch (e: AssertionError) {
            if (runCatching { target.fetchSemanticsNode() }.isSuccess) throw e
            null
        }
    }

    private fun endOfWalkChecks() {
        trace += "end: process death + relaunch"
        guard("end-of-walk process death") { processDeath() }
        settle()
        checkAll(afterBack = null)
    }

    // ── Screen helpers ──────────────────────────────────────────────────────

    private fun SemanticsNode.isSheet() =
        flatten(this).any { n -> n.config.getOrNull(SemanticsProperties.ContentDescription).orEmpty().any { it == SHEET_SCRIM } }

    private fun SemanticsNode.isDialog() = flatten(this).any { n -> n.config.getOrNull(SemanticsProperties.IsDialog) != null }

    /** An overlay window (never the activity's) that is neither a sheet nor a
     *  dialog: a tooltip, a menu. */
    private fun SemanticsNode.isPopup() = !isSheet() && !isDialog()

    /** The editors' "Discard this entry?" confirm. */
    private fun SemanticsNode.isDiscardConfirm(): Boolean = flatten(this).any { labelOf(it) == str(R.string.log_discard_title) }

    /** The M3 date picker, or the editor's "Logged at" time picker dialog. */
    private fun SemanticsNode.isPicker(): Boolean {
        val labels = flatten(this).map { labelOf(it) }.toSet()
        return "Select date" in labels ||
            (str(R.string.log_logged_at) in labels && str(R.string.log_ok) in labels && flatten(this).none { it.config.getOrNull(SemanticsProperties.TestTag) == "editor-primary" })
    }

    private fun popupOpen(): Boolean = roots().drop(1).any { it.isPopup() }

    /** A text field holds focus in the top window (Back would hide its keyboard). */
    private fun textFieldFocused(): Boolean = roots().lastOrNull()?.let { top ->
        flatten(top).any { it.config.getOrNull(SemanticsActions.SetText) != null && it.config.getOrNull(SemanticsProperties.Focused) == true }
    } == true

    private fun quote(s: String) = "\"" + (if (s.length > 24) s.take(24) + "…(${s.length})" else s).replace("\n", "\\n") + "\""

    override fun fail(msg: String): Nothing = throw walkError(msg)

    private fun walkError(msg: String): ProbeFailure {
        val replay = "UIWALK_STATE=${start.name} UIWALK_SEED=$seed UIWALK_STEPS=$step"
        return ProbeFailure(
            "UIWALK [$start] seed=$seed step=$step font=$fontScale land=$landscape: $msg\n" +
                "replay: $replay ./gradlew :app:testDebugUnitTest --tests 'app.milestone.qa.RandomUiWalkTest'\n" +
                "trace:\n" + trace.takeLast(200).joinToString("\n") + "\n" +
                "screen:\n" + runCatching { screenDump() }.getOrElse { "<unavailable: $it>" },
        )
    }

    companion object {
        /** Steps per walk by default (`UIWALK_STEPS` overrides). */
        private val DEFAULT_STEPS = 150
        private const val BASE_SEED = 0x5EED_0A1CL

        /** `UIWALK_DEBUG=1` prints every screen after its step (for replays). */
        private val DEBUG = System.getenv("UIWALK_DEBUG") == "1"

        private const val SHEET_SCRIM = "Close sheet"

        /** [UxInvariants] kinds gated here; the touch target is [smallTargets]'. */
        private val UX_KINDS = UxInvariants.KINDS - "small-touch-target"

        private val FONT_SCALES = listOf(1f, 1f, 1.3f, 2f)

        /** Plausible and hostile text-field input: empty, blank, numbers with
         *  commas / dots / signs, huge, unicode, emoji, RTL, control characters. */
        private val TEXTS = listOf(
            "", " ", "0", "5", "12,5", "1,234.5", "1.234,5", "-3", "99999999999", "1e9", "NaN", "0.0001",
            "Back Squat", "squat", "Bench", "knee", "Front Lever", "Müller-Übung", "深蹲", "قرفصاء", "💪🏽🔥",
            "a".repeat(400), "Line\nbreak", "\t", "' OR 1=1 --", "<b>bold</b>", "%s %d", "\u200B",
        )
    }
}
