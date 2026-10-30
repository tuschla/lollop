package app.milestone.qa

import android.content.Context
import android.os.Looper
import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasScrollToNodeAction
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeUp
import androidx.compose.ui.text.Paragraph
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.toSize
import androidx.test.core.app.ApplicationProvider
import app.milestone.Event
import app.milestone.EventDispatcher
import app.milestone.FieldLabel
import app.milestone.MilestoneTheme
import app.milestone.R
import app.milestone.ReadinessSignal
import app.milestone.SendResult
import app.milestone.ViewModel
import java.util.Locale
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.launch
import org.junit.After
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

/** Text-layout helpers shared by the review proofs (same rules as SmallScreenLargeFontTest). */
private fun SemanticsNode.textLayout(): TextLayoutResult? {
    val get = config.getOrNull(SemanticsActions.GetTextLayoutResult)?.action ?: return null
    val out = mutableListOf<TextLayoutResult>()
    return if (get(out)) out.firstOrNull() else null
}

/** The text re-laid out at the width the node really has. */
private fun TextLayoutResult.paragraphAt(widthPx: Int) = Paragraph(
    text = layoutInput.text.text,
    style = layoutInput.style,
    constraints = Constraints(maxWidth = widthPx.coerceAtLeast(1)),
    density = layoutInput.density,
    fontFamilyResolver = layoutInput.fontFamilyResolver,
)

/** A soft line break INSIDE a word at the node's real width ("Maratho/n"). */
private fun SemanticsNode.splitsAWord(): Boolean {
    val l = textLayout() ?: return false
    if (size.width <= 0) return false
    val p = l.paragraphAt(size.width)
    val text = l.layoutInput.text.text
    return (0 until p.lineCount - 1).any { i ->
        val end = p.getLineEnd(i)
        end in 1 until text.length && text[end - 1].isLetterOrDigit() && text[end].isLetterOrDigit()
    }
}

/** Lines the node's text needs at its real width. */
private fun SemanticsNode.linesNeeded(): Int {
    val l = textLayout() ?: return 0
    return l.paragraphAt(size.width).lineCount
}

private fun SemanticsNode.text(): String =
    config.getOrNull(SemanticsProperties.Text)?.joinToString { it.text }.orEmpty()

private fun SemanticsNode.allDescendants(): List<SemanticsNode> = children.flatMap { listOf(it) + it.allDescendants() }

/**
 * Shell layout guards over the REAL core: RTL placement of the hero and undo
 * actions, the pain-hold banner shown whole at small and landscape windows and
 * large fonts, and words kept whole at in-between font scales.
 */
@RunWith(RobolectricTestRunner::class)
@Config(qualifiers = "w320dp-h640dp-xhdpi")
class ReviewShellTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @Before
    fun setUp() {
        QaHarness.reset(ctx)
    }

    @After
    fun tearDown() {
        RuntimeEnvironment.setFontScale(1f)
        QaHarness.teardown()
    }

    private fun dp(px: Float) = with(rule.density) { px.toDp().value }

    // ═══════════ RTL: the restacked Layouts mirror ═══════════════════════════

    /** In RTL the hero's title sits at the start (right) and its trailing "Move" /
     *  "?" at the end (left): the hero's Layout places by layout direction. */
    @Test
    @Config(qualifiers = "ar-rEG-ldrtl-w360dp-h640dp-xhdpi")
    fun heroTitle_rtl_actionsSitAtTheEnd() {
        QaHarness.seedStrengthHistory(ctx)
        rule.launchApp()
        // Control: the app really is RTL: the configuration says so and the Scaffold's
        // end-positioned "+ Log" FAB sits on the left half.
        var dir = -1
        lastScenario!!.onActivity { dir = it.resources.configuration.layoutDirection }
        val fab = rule.onNode(hasClickAction() and hasContentDescription(str(R.string.main_log))).fetchSemanticsNode().boundsInRoot
        val window = rule.onAllNodes(isRoot()).fetchSemanticsNodes()[0].boundsInRoot
        assertTrue(
            "RTL is active (activity dir $dir, app dir ${ctx.resources.configuration.layoutDirection}, qualifiers ${RuntimeEnvironment.getQualifiers()}, FAB $fab in $window)",
            dir == android.view.View.LAYOUT_DIRECTION_RTL && fab.center.x < window.center.x,
        )
        val title = EventDispatcher.model.value!!.next_session!!.title
        val titleNode = rule.onAllNodes(hasText(title), useUnmergedTree = true).fetchSemanticsNodes()
            .filter { it.boundsInRoot.height > 0f }.minBy { it.boundsInRoot.top }
        val actions = rule.onAllNodes(hasClickAction() and hasContentDescription(title, substring = true)).fetchSemanticsNodes()
            .filter { kotlin.math.abs(it.boundsInRoot.center.y - titleNode.boundsInRoot.center.y) < with(rule.density) { 40.dp.toPx() } }
        assertTrue("the hero has a Move or ? action on the title line", actions.isNotEmpty())
        for (a in actions) {
            assertTrue(
                "RTL: hero action at ${dp(a.boundsInRoot.left)}–${dp(a.boundsInRoot.right)}dp must sit at the END (left of) " +
                    "the title at ${dp(titleNode.boundsInRoot.left)}–${dp(titleNode.boundsInRoot.right)}dp",
                a.boundsInRoot.right <= titleNode.boundsInRoot.left + 1f,
            )
        }
    }

    /** The set loop's "Logged … · Undo" row (SetEditor.kt UndoRow, same restack
     *  pattern): in RTL the line belongs at the start (right), Undo at the end (left). */
    @Test
    @Config(qualifiers = "ar-rEG-ldrtl-w360dp-h640dp-xhdpi")
    fun undoRow_rtl_undoSitsAtTheEnd() {
        QaHarness.seedStrengthHistory(ctx)
        rule.launchApp()
        val first = EventDispatcher.model.value!!.next_session!!.items.first { it.exercise.isNotEmpty() }
        // Locale-agnostic: the ar config formats the set numbers in Arabic-Indic digits.
        val logSet = androidx.compose.ui.test.SemanticsMatcher("log set action") { n ->
            n.config.getOrNull(SemanticsProperties.ContentDescription).orEmpty().any { it.startsWith("Log set ") }
        }
        rule.onAllNodes(
            hasClickAction() and logSet and androidx.compose.ui.test.hasAnyAncestor(hasTestTag("plan-item:${first.exercise}")),
        )[0].performScrollTo().performClick()
        rule.waitUntil(10_000) { rule.onAllNodes(hasTestTag("editor-primary")).fetchSemanticsNodes().isNotEmpty() }
        rule.onNode(hasTestTag("editor-primary")).performClick()
        val prefix = strPrefix(R.string.log_undo_logged) + " "
        val logged = androidx.compose.ui.test.SemanticsMatcher("undo row text") { n ->
            n.config.getOrNull(SemanticsProperties.Text).orEmpty().any { it.text.startsWith(prefix) }
        }
        rule.waitUntil(10_000) { rule.onAllNodes(logged, useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty() }
        rule.waitForIdle()
        val line = rule.onAllNodes(logged, useUnmergedTree = true).fetchSemanticsNodes().first().boundsInRoot
        val undo = rule.onAllNodes(hasText(str(R.string.log_undo)), useUnmergedTree = true).fetchSemanticsNodes()
            .map { it.boundsInRoot }.minBy { kotlin.math.abs(it.center.y - line.center.y) }
        assertTrue(
            "RTL: Undo at ${dp(undo.left)}–${dp(undo.right)}dp must sit at the END (left of) the line at ${dp(line.left)}–${dp(line.right)}dp",
            undo.right <= line.left + 1f,
        )
    }

    // ═══════════ The pinned DO-NOT-TRAIN banner under its half-screen cap ═════

    private fun painHoldBannerWhole(scale: Float) {
        RuntimeEnvironment.setFontScale(scale)
        val morning = app.milestone.todayEpochDay() * 86_400L + 8 * 3_600L
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), Event.SubmitReadiness(ReadinessSignal.Pain, 1.0, morning)))
        rule.launchApp()
        rule.waitUntil(10_000) { rule.onAllNodes(hasText(str(R.string.main_do_not_train)), useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty() }
        rule.waitForIdle()
        val problems = mutableListOf<String>()
        // The banner states the hold (INVARIANT 3): its headline and the reason
        // under it show whole, at the height their text needs (not squeezed, not
        // scrolled away, not clipped).
        for (text in listOf(str(R.string.main_do_not_train), str(R.string.main_hold_reason_pain))) {
            val n = rule.onAllNodes(hasText(text), useUnmergedTree = true).fetchSemanticsNodes().firstOrNull()
            if (n == null) {
                problems += "\"$text\" missing"
                continue
            }
            val needs = n.textLayout()?.multiParagraph?.height ?: n.size.height.toFloat()
            if (n.boundsInRoot.height < needs - 1f) {
                problems += "\"$text\" shows ${dp(n.boundsInRoot.height)}dp of its ${dp(needs)}dp"
            }
        }
        val labels = EventDispatcher.model.value!!.hold_resolutions.map { it.action_label }.filter { it.isNotBlank() } +
            listOf(str(R.string.main_remove_report), str(R.string.main_add_details))
        for (label in labels) {
            val s = rule.onAllNodes(hasText(label) and hasClickAction()).fetchSemanticsNodes().firstOrNull()
            if (s == null) {
                problems += "action \"$label\" missing"
                continue
            }
            val unclipped = Rect(s.positionInRoot, s.size.toSize())
            if (s.boundsInRoot.height < unclipped.height - 1f || s.boundsInRoot.height <= 0f) {
                problems += "action \"$label\" shows ${dp(s.boundsInRoot.height)}dp of ${dp(unclipped.height)}dp"
            }
        }
        assertTrue("banner @ x$scale:\n" + problems.joinToString("\n"), problems.isEmpty())
    }

    /** A phone in landscape (360dp tall → 180dp share): the pain hold's actions
     *  took the whole cap, so the headline block (DO NOT TRAIN, the reason) got
     *  0dp and the banner showed only buttons. */
    @Test
    @Config(qualifiers = "w640dp-h360dp-xhdpi")
    fun painBanner_landscape_font100_headlineAndActionsWhole() = painHoldBannerWhole(1.0f)

    @Test
    @Config(qualifiers = "w640dp-h360dp-xhdpi")
    fun painBanner_landscape_font150_headlineAndActionsWhole() = painHoldBannerWhole(1.5f)

    /** A 411dp-wide phone in landscape (411dp tall → 205dp cap). */
    @Test
    @Config(qualifiers = "w731dp-h411dp-xhdpi")
    fun painBanner_landscape411_font100_headlineAndActionsWhole() = painHoldBannerWhole(1.0f)

    @Test
    @Config(qualifiers = "w731dp-h411dp-xhdpi")
    fun painBanner_landscape411_font130_headlineAndActionsWhole() = painHoldBannerWhole(1.3f)

    /** Split-screen / a short window: 320dp wide, 480dp tall (240dp cap) at 130 %. */
    @Test
    @Config(qualifiers = "w320dp-h480dp-xhdpi")
    fun painBanner_320x480_font130_headlineAndActionsWhole() = painHoldBannerWhole(1.3f)

    /** Portrait on the 320dp phone at 175 %: the claim "Its actions always show whole". */
    @Test
    fun painBanner_320x640_font175_headlineAndActionsWhole() = painHoldBannerWhole(1.75f)

    // ═══════════ In-between font scales: words kept whole ═══════════════════

    /** Unmerged text nodes under [root] tagged by [where] that break inside a word. */
    private fun wordSplits(nodes: List<SemanticsNode>): List<String> =
        nodes.filter { it.boundsInRoot.width > 0f && it.splitsAWord() }.map { "\"${it.text()}\" @ ${dp(it.size.width.toFloat())}dp" }

    private fun hrZones(scale: Float) {
        RuntimeEnvironment.setFontScale(scale)
        QaHarness.seedLog(ctx, listOf(QaHarness.hybridFiveKProfile(), Event.ComputeHrZones(30.0, null)))
        rule.launchApp()
        val calcRow = hasText(str(R.string.main_tool_hr_title)) and hasClickAction()
        rule.onNode(hasText(str(R.string.main_dest_profile)) and hasClickAction()).performClick()
        rule.waitForIdle()
        rule.onAllNodes(hasScrollToNodeAction())[0].performScrollToNode(calcRow)
        rule.onAllNodes(calcRow)[0].performClick()
        rule.waitUntil(10_000) { rule.onAllNodes(hasTestTag("calc-result")).fetchSemanticsNodes().isNotEmpty() }
        rule.waitForIdle()
        val bands = EventDispatcher.model.value!!.hr_zone_bands.map { it.name }.toSet()
        // The zone rows: every unmerged text node in the result card.
        val card = rule.onNode(hasTestTag("calc-result"), useUnmergedTree = true).fetchSemanticsNode()
        val texts = card.allDescendants().filter { it.text().isNotEmpty() }
        assertTrue("found the zone names in the card", texts.any { it.text() in bands })
        val split = wordSplits(texts)
        assertTrue("HR zone rows @ x$scale split words:\n" + split.joinToString("\n"), split.isEmpty())
    }

    /** 115 % is a stock Android font step; a fixed "stack from 130 %" split
     *  "122–14/8" here. The rows stack when a word would not fit its column. */
    @Test
    fun hrZones_font115_320dp_noWordSplit() = hrZones(1.15f)

    @Test
    fun hrZones_font100_320dp_noWordSplit() = hrZones(1.0f)

    @Test
    @Config(qualifiers = "w360dp-h640dp-xhdpi")
    fun hrZones_font115_360dp_noWordSplit() = hrZones(1.15f)

    /** Today at an in-between scale: the week strip's day labels whole (one line
     *  each, no cut) and the hero title never split inside a word. */
    private fun todayWhole(scale: Float) {
        RuntimeEnvironment.setFontScale(scale)
        QaHarness.seedStrengthHistory(ctx)
        rule.launchApp()
        rule.waitForIdle()
        val problems = mutableListOf<String>()
        fun all() = rule.onAllNodes(isRoot(), useUnmergedTree = true).fetchSemanticsNodes().flatMap { listOf(it) + it.allDescendants() }
        val title = EventDispatcher.model.value!!.next_session!!.title
        problems += wordSplits(all().filter { it.text() == title })
        val isDay = androidx.compose.ui.test.SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Tab)
        runCatching { rule.onAllNodes(androidx.compose.ui.test.hasScrollToIndexAction())[0].performScrollToNode(isDay) }
        rule.waitForIdle()
        val days = all().filter { it.config.getOrNull(SemanticsProperties.Role) == Role.Tab && it.boundsInRoot.height > 0f }
        assertTrue("found the 7 week-strip days (${days.size})", days.size >= 7)
        for (d in days) for (t in d.allDescendants().filter { it.text().isNotEmpty() }) {
            if (t.linesNeeded() > 1) problems += "day label \"${t.text()}\" needs ${t.linesNeeded()} lines in ${dp(t.size.width.toFloat())}dp"
        }
        assertTrue("Today @ x$scale:\n" + problems.joinToString("\n"), problems.isEmpty())
    }

    @Test fun today_320dp_font115() = todayWhole(1.15f)

    @Test fun today_320dp_font150() = todayWhole(1.5f)

    @Test fun today_320dp_font175() = todayWhole(1.75f)

    @Test
    @Config(qualifiers = "w360dp-h640dp-xhdpi")
    fun today_360dp_font150() = todayWhole(1.5f)

    /** Every visible unmerged text node on screen that breaks inside a word. */
    private fun screenWordSplits(): List<String> = wordSplits(
        rule.onAllNodes(isRoot(), useUnmergedTree = true).fetchSemanticsNodes()
            .flatMap { listOf(it) + it.allDescendants() }
            .filter { it.text().isNotEmpty() && it.boundsInRoot.height > 0f },
    )

    /** The weekly check-in (FieldLabels over its questions, the bar-speed field)
     *  at an in-between scale: no word broken anywhere on the sheet, scrolled through. */
    private fun weeklyWhole(scale: Float) {
        RuntimeEnvironment.setFontScale(scale)
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        rule.onNode(hasClickAction() and hasContentDescription(str(R.string.main_log))).performClick()
        val title = str(R.string.main_weekly_checkin_title)
        rule.waitUntil(10_000) { rule.onAllNodes(hasText(title) and hasClickAction()).fetchSemanticsNodes().isNotEmpty() }
        rule.onNode(hasText(title) and hasClickAction()).performScrollTo().performClick()
        rule.waitUntil(10_000) { rule.onAllNodes(hasTestTag("editor-primary")).fetchSemanticsNodes().isNotEmpty() }
        rule.onNode(hasText(str(R.string.log_weekly_more_details)) and hasClickAction()).performScrollTo().performClick()
        rule.waitForIdle()
        val problems = mutableListOf<String>()
        problems += screenWordSplits()
        repeat(6) {
            runCatching { rule.onAllNodes(hasScrollToNodeAction()).onLast().performTouchInput { swipeUp() } }
            rule.waitForIdle()
            problems += screenWordSplits()
        }
        assertTrue("weekly check-in @ x$scale:\n" + problems.distinct().joinToString("\n"), problems.isEmpty())
    }

    @Test fun weekly_320dp_font115() = weeklyWhole(1.15f)

    @Test fun weekly_320dp_font150() = weeklyWhole(1.5f)

    @Test
    @Config(qualifiers = "w411dp-h731dp-xhdpi")
    fun today_411dp_font175() = todayWhole(1.75f)
}

/** Component-level proofs (no core): FieldLabel in RTL, the dispatcher reset. */
@RunWith(RobolectricTestRunner::class)
class ReviewShellComponentTest {

    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    /** In RTL a FieldLabel's label belongs at the start (right edge) and its hint
     *  at the end (left edge), as the Row it replaced did. */
    @Test
    fun fieldLabel_rtl_labelAtStart_hintAtEnd() {
        rule.setContent {
            MilestoneTheme {
                CompositionLocalProvider(LocalLayoutDirection provides LayoutDirection.Rtl) {
                    Box(Modifier.width(300.dp)) { FieldLabel("Weight", "kg") }
                }
            }
        }
        val label = rule.onNodeWithText("Weight").fetchSemanticsNode().boundsInRoot
        val hint = rule.onNodeWithText("kg").fetchSemanticsNode().boundsInRoot
        val right = with(rule.density) { 300.dp.toPx() }
        assertTrue("RTL: label $label must end at the start edge (${right}px)", label.right >= right - 1f)
        assertTrue("RTL: hint $hint must start at the end edge (0px)", hint.left <= 1f)
    }

    /** Control: the same FieldLabel in LTR (label left, hint right). */
    @Test
    fun fieldLabel_ltr_labelAtStart_hintAtEnd() {
        rule.setContent {
            MilestoneTheme {
                Box(Modifier.width(300.dp)) { FieldLabel("Weight", "kg") }
            }
        }
        val label = rule.onNodeWithText("Weight").fetchSemanticsNode().boundsInRoot
        val hint = rule.onNodeWithText("kg").fetchSemanticsNode().boundsInRoot
        val right = with(rule.density) { 300.dp.toPx() }
        assertTrue("LTR label $label at 0", label.left <= 1f)
        assertTrue("LTR hint $hint at the right edge", hint.right >= right - 1f)
    }

    /**
     * Claim check (passes = VERIFIED): SmallScreenLargeFontTest's text walker CAN
     * see the run-stat value and label nodes in the unmerged tree despite the
     * column's clearAndSetSemantics, so `runDetail_font200`
     * is a real gate for cut numbers.
     */
    @Test
    fun runStatColumns_valueAndLabelVisibleToTheTextCutWalker() {
        val stats = listOf(
            app.milestone.RunStat("Distance", "3.72", "mi", "Distance 3.72 miles"),
            app.milestone.RunStat("Moving time", "29:57", "", "Moving time 29:57"),
            app.milestone.RunStat("Avg pace", "8:03", "/mi", "Average pace 8:03 per mile"),
        )
        rule.setContent {
            MilestoneTheme {
                Box(Modifier.width(288.dp)) { app.milestone.RunStatColumns(stats, compact = false) }
            }
        }
        // Control: the stat is on screen (merged: its spoken description).
        assertTrue(rule.onAllNodes(hasContentDescription("Moving time 29:57")).fetchSemanticsNodes().isNotEmpty())
        val withLayout = rule.onAllNodes(isRoot(), useUnmergedTree = true).fetchSemanticsNodes()
            .flatMap { listOf(it) + it.allDescendants() }
            .filter { it.textLayout() != null }
            .map { it.text() }
        for (s in stats) {
            assertTrue("value \"${s.value}\" has a text-layout node the walker can check (have: $withLayout)", s.value in withLayout)
            assertTrue("label \"${s.label.uppercase(Locale.US)}\" has a text-layout node (have: $withLayout)", s.label.uppercase(Locale.US) in withLayout)
        }
    }

    /** RunStatColumns at [scale] in [widthDp]: every value on one line, no word split. */
    private fun runStatsWhole(scale: Float, widthDp: Int) {
        val stats = listOf(
            app.milestone.RunStat("Distance", "13.72", "km", "Distance 13.72 kilometres"),
            app.milestone.RunStat("Moving time", "1:29:57", "", "Moving time 1:29:57"),
            app.milestone.RunStat("Avg pace", "6:33", "/km", "Average pace 6:33 per kilometre"),
        )
        rule.setContent {
            MilestoneTheme {
                val d = androidx.compose.ui.platform.LocalDensity.current
                CompositionLocalProvider(androidx.compose.ui.platform.LocalDensity provides androidx.compose.ui.unit.Density(d.density, scale)) {
                    Box(Modifier.width(widthDp.dp)) { app.milestone.RunStatColumns(stats, compact = false) }
                }
            }
        }
        val nodes = rule.onAllNodes(isRoot(), useUnmergedTree = true).fetchSemanticsNodes()
            .flatMap { listOf(it) + it.allDescendants() }.filter { it.textLayout() != null }
        val problems = mutableListOf<String>()
        for (s in stats) {
            nodes.filter { it.text() == s.value }.forEach { if (it.linesNeeded() > 1) problems += "value \"${s.value}\" needs ${it.linesNeeded()} lines" }
        }
        problems += nodes.filter { it.splitsAWord() }.map { "split \"${it.text()}\"" }
        assertTrue("run stats @ x$scale in ${widthDp}dp: $problems", problems.isEmpty())
    }

    @Test fun runStats_288dp_font115() = runStatsWhole(1.15f, 288)

    @Test fun runStats_288dp_font150() = runStatsWhole(1.5f, 288)

    @Test fun runStats_288dp_font175() = runStatsWhole(1.75f, 288)

    @Test fun runStats_328dp_font130() = runStatsWhole(1.3f, 328)

    /**
     * [EventDispatcher.resetForTest] must settle the [EventDispatcher.exclusive]
     * blocks still waiting in the old queue. When the in-flight unit ignores
     * cancellation (as Export.kt's `withContext(CoreJniDispatcher + NonCancellable)`
     * restore body does), the cancelled consumer can't drain them, and a dropped
     * block's awaiter would hang forever. The in-flight send that finishes after the
     * reset publishes nothing into the fresh process.
     */
    @Test
    fun resetForTest_completesOrFailsQueuedExclusiveAwaiters() {
        val real = EventDispatcher.sender
        val gate = CompletableDeferred<Unit>()
        val late = ViewModel(input_count = 777)
        try {
            EventDispatcher.resetForTest()
            // Hold the consumer on one non-cancellable in-flight send so the next unit waits in the queue.
            EventDispatcher.sender = { _ ->
                kotlinx.coroutines.withContext(kotlinx.coroutines.NonCancellable) { gate.await() }
                SendResult(late, accepted = true, persisted = true, stale = false)
            }
            EventDispatcher.send(Event.SetToday(1L, 0))
            var settled = false
            val awaiter = EventDispatcher.scope.launch {
                runCatching { EventDispatcher.exclusive { null } }
                settled = true
            }
            shadowOf(Looper.getMainLooper()).idle()
            EventDispatcher.resetForTest()
            gate.complete(Unit) // the in-flight unit finishes after the reset
            shadowOf(Looper.getMainLooper()).idleFor(5, TimeUnit.SECONDS)
            assertTrue("the queued exclusive() awaiter settled after resetForTest (still active: ${awaiter.isActive})", settled)
            assertTrue("the send that finished after the reset published nothing", EventDispatcher.model.value !== late)
            awaiter.cancel()
        } finally {
            gate.complete(Unit)
            EventDispatcher.sender = real
            EventDispatcher.resetForTest()
        }
    }

    /** An exclusive body in flight across the reset (non-cancellable, like the
     *  restore) still hands its awaiter its result, but publishes nothing; the block
     *  queued behind it is settled. */
    @Test
    fun resetForTest_exclusiveInFlightAnswersItsAwaiterButPublishesNothing() {
        val gate = CompletableDeferred<Unit>()
        val late = ViewModel(input_count = 778)
        try {
            EventDispatcher.resetForTest()
            var got: ViewModel? = null
            val first = EventDispatcher.scope.launch {
                got = EventDispatcher.exclusive {
                    kotlinx.coroutines.withContext(kotlinx.coroutines.NonCancellable) { gate.await() }
                    late
                }
            }
            var queuedSettled = false
            val queued = EventDispatcher.scope.launch {
                runCatching { EventDispatcher.exclusive { null } }
                queuedSettled = true
            }
            shadowOf(Looper.getMainLooper()).idle()
            EventDispatcher.resetForTest()
            gate.complete(Unit)
            shadowOf(Looper.getMainLooper()).idleFor(5, TimeUnit.SECONDS)
            assertTrue("the in-flight body's awaiter got its result (got $got)", got === late)
            assertTrue("the block queued behind it settled", queuedSettled)
            assertTrue("a body that finished after the reset published nothing", EventDispatcher.model.value !== late)
            first.cancel()
            queued.cancel()
        } finally {
            gate.complete(Unit)
            EventDispatcher.resetForTest()
        }
    }

    /**
     * A cancelled consumer that resumes while its queue still holds units runs and
     * publishes nothing more (the loop's `runCatching` once swallowed the
     * CancellationException and `for (m in queue)` took the next buffered unit).
     */
    @Test
    fun resetForTest_cancelledConsumerRunsNothingMore() {
        val real = EventDispatcher.sender
        val gate = CompletableDeferred<Unit>()
        val ranAfterCancel = mutableListOf<Event>()
        var published: ViewModel? = null
        val second = ViewModel(input_count = 4242)
        try {
            EventDispatcher.resetForTest()
            var calls = 0
            EventDispatcher.sender = { e ->
                if (calls++ == 0) {
                    gate.await()
                    SendResult(ViewModel(), accepted = true)
                } else {
                    ranAfterCancel += e
                    SendResult(second, accepted = true)
                }
            }
            val watcher = EventDispatcher.scope.launch { EventDispatcher.model.collect { if (it === second) published = it } }
            EventDispatcher.send(Event.SetToday(1L, 0))
            EventDispatcher.send(Event.SetToday(2L, 0))
            shadowOf(Looper.getMainLooper()).idle()
            EventDispatcher.resetForTest()
            shadowOf(Looper.getMainLooper()).idleFor(5, TimeUnit.SECONDS)
            watcher.cancel()
            assertTrue(
                "after resetForTest the cancelled consumer ran $ranAfterCancel and published ${published?.input_count}",
                ranAfterCancel.isEmpty() && published == null,
            )
        } finally {
            gate.complete(Unit)
            EventDispatcher.sender = real
            EventDispatcher.resetForTest()
        }
    }

    /** resetForTest gives "the process-scoped state of a freshly started process":
     *  a notice a previous test left uncollected must not survive into the next,
     *  nor a fake send path it installed. */
    @Test
    fun resetForTest_dropsBufferedNoticesAndFakeSendPaths() {
        EventDispatcher.resetForTest()
        // A previous test's rejected entry, with no Activity collecting.
        EventDispatcher.report(SendResult(ViewModel(), accepted = false))
        val fake: suspend (Event) -> SendResult = { _ -> SendResult(ViewModel(), accepted = true) }
        val fakeBatch: suspend (List<Event>) -> SendResult = { _ -> SendResult(ViewModel(), accepted = true) }
        EventDispatcher.sender = fake
        EventDispatcher.batchSender = fakeBatch
        EventDispatcher.resetForTest()
        var leaked: app.milestone.CoreNotice? = null
        val job = EventDispatcher.scope.launch { EventDispatcher.notices.collect { leaked = it } }
        shadowOf(Looper.getMainLooper()).idle()
        job.cancel()
        assertTrue("a fresh process has no pending notice, got $leaked", leaked == null)
        assertTrue("a fresh process sends to the core", EventDispatcher.sender !== fake && EventDispatcher.batchSender !== fakeBatch)
    }

    /** Open the sheet's date picker, pick the day whose label contains [day] (or
     *  leave the selection as is), and confirm. */
    private fun confirmDate(day: String? = null) {
        rule.onNode(hasText(str(R.string.log_today), substring = true) and hasClickAction()).performScrollTo().performClick()
        rule.waitForIdle()
        if (day != null) rule.onNode(hasText(day, substring = true) and hasClickAction()).performClick()
        rule.onNode(hasText(str(R.string.log_ok)) and hasClickAction()).performClick()
        rule.waitForIdle()
    }

    /** With the clock and zone pinned (Monday 2026-09-21 10:00:37, UTC), so the day
     *  before is in the picker's month. The seconds past the minute are what a real
     *  clock has and a date-only pick drops, so "same day" can't pass as "same instant". */
    private fun pinnedClock(body: () -> Unit) {
        java.util.TimeZone.setDefault(java.util.TimeZone.getTimeZone("UTC"))
        app.milestone.AppClock.offsetMillis = QaHarness.PINNED_NOW_MILLIS + 37_000L - System.currentTimeMillis()
        try {
            body()
        } finally {
            app.milestone.AppClock.offsetMillis = 0L
        }
    }

    /**
     * Session review: confirming the date picker on the SAME day is not an edit
     * (it set the discard guard while Save stayed off: a change that didn't exist
     * and couldn't be saved); picking another day is one.
     */
    @Test
    fun sessionReview_sameDateConfirmed_isNotAnEdit() = pinnedClock {
        var dirty = false
        rule.setContent {
            MilestoneTheme {
                CompositionLocalProvider(app.milestone.LocalEditorPinnedActions provides true) {
                    app.milestone.SessionReviewSheet(onDirtyChange = { dirty = it }, showRun = false, onSubmit = {})
                }
            }
        }
        confirmDate()
        val saveEnabled = rule.onNode(hasTestTag("editor-primary")).fetchSemanticsNode().config
            .getOrNull(SemanticsProperties.Disabled) == null
        assertTrue("same day confirmed: dirty=$dirty while Save enabled=$saveEnabled", !dirty)
        confirmDate("September 20, 2026")
        assertTrue("another day picked is an edit", dirty)
    }

    /** The weekly check-in: the same day confirmed is no edit, another day is. */
    @Test
    fun weeklyCheckin_sameDateConfirmed_isNotAnEdit() = pinnedClock {
        var dirty = false
        rule.setContent {
            MilestoneTheme {
                CompositionLocalProvider(app.milestone.LocalEditorPinnedActions provides true) {
                    app.milestone.WeeklyCheckinSheet(onDirtyChange = { dirty = it }, onSubmit = {})
                }
            }
        }
        confirmDate()
        assertTrue("same day confirmed: dirty=$dirty", !dirty)
        confirmDate("September 20, 2026")
        assertTrue("another day picked is an edit", dirty)
    }

    /** Claim check (VERIFIED if green): after resetForTest a fresh send is consumed
     *  and published by the new consumer. */
    @Test
    fun resetForTest_newConsumerPublishes() {
        val real = EventDispatcher.sender
        try {
            EventDispatcher.resetForTest()
            val vm = ViewModel()
            EventDispatcher.sender = { _ -> SendResult(vm, accepted = true, persisted = true, stale = false) }
            EventDispatcher.send(Event.SetToday(1L, 0))
            shadowOf(Looper.getMainLooper()).idle()
            assertTrue("published", EventDispatcher.model.value === vm)
        } finally {
            EventDispatcher.sender = real
            EventDispatcher.resetForTest()
        }
    }
}
