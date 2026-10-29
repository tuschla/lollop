package app.milestone.qa

import android.content.Context
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasScrollToIndexAction
import androidx.compose.ui.test.hasScrollToNodeAction
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performScrollToIndex
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.isSelected
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeUp
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.toSize
import androidx.test.core.app.ApplicationProvider
import app.milestone.ConcurrentGoal
import app.milestone.Event
import app.milestone.EventDispatcher
import app.milestone.GoalDistance
import app.milestone.LiftGoal
import app.milestone.ProgressionCadence
import app.milestone.R
import app.milestone.ReadinessSignal
import java.util.Locale
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

/**
 * The owner journeys on the smallest common phone width (320dp) at the largest
 * system font scale (200 %) and the common enlarged one (130 %), over the REAL
 * core. On every screen of every journey: no action spills off either edge, no
 * text is cut off (ellipsis, height clip, an unwrapped line wider than its box),
 * no two actions overlap, every action keeps a 48dp target, every action of the
 * screen can be brought into view (and is not left under the floating Log
 * button), and the three bottom tabs stay whole and switch destinations. The
 * Today-only "+ Log" FAB is extended at the top and icon-only once the list
 * scrolls, and Today's last row always scrolls clear of it.
 */
@RunWith(RobolectricTestRunner::class)
@Config(qualifiers = "w320dp-h640dp-xhdpi")
class SmallScreenLargeFontTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @Before
    fun setUp() {
        Locale.setDefault(Locale.US)
        QaHarness.reset(ctx)
    }

    @After
    fun tearDown() = QaHarness.teardown()

    // ── Layout checks ───────────────────────────────────────────────────────

    private val logFab get() = hasClickAction() and hasContentDescription(str(R.string.main_log))

    private fun dp(px: Float) = with(rule.density) { px.toDp().value }

    /** Every layout problem on the current screen, each naming its node. */
    private fun layoutProblems(): List<String> {
        rule.waitForIdle()
        val out = mutableListOf<String>()
        UxInvariants.check(rule, rule.density)
            .filter { it.kind in UxInvariants.KINDS }
            .forEach { out += it.toString() }
        // Text cut-offs live on the unmerged text nodes.
        fun walkText(n: SemanticsNode) {
            val b = n.boundsInRoot
            n.config.getOrNull(SemanticsActions.GetTextLayoutResult)?.action?.let { get ->
                val layouts = mutableListOf<TextLayoutResult>()
                if (b.width > 0f && b.height > 0f && get(layouts)) {
                    layouts.filter { it.cutsOffText || it.needsMoreLinesAt(n.size.width) }
                        .forEach { out += "[text-cut] \"${it.layoutInput.text.text}\"" }
                    layouts.filter { it.splitsAWord }.forEach { out += "[word-split] \"${it.layoutInput.text.text}\"" }
                }
            }
            n.children.forEach(::walkText)
        }
        rule.onAllNodes(isRoot(), useUnmergedTree = true).fetchSemanticsNodes().forEach(::walkText)
        // Actions are what the user taps: the merged tree.
        val fabIds = rule.onAllNodes(logFab).fetchSemanticsNodes().map { it.id }.toSet()
        for (root in rule.onAllNodes(isRoot()).fetchSemanticsNodes()) {
            val rootBounds = root.boundsInRoot
            val actions = mutableListOf<SemanticsNode>()
            fun walk(n: SemanticsNode) {
                val b = n.boundsInRoot
                val visible = b.width > 0f && b.height > 0f
                // A sheet's scrim ("Close sheet") spans its whole window by design.
                val scrim = b.width * b.height >= rootBounds.width * rootBounds.height * 0.9f
                if (visible && SemanticsActions.OnClick in n.config && n.id !in fabIds && !scrim) actions += n
                // Spills off the LEFT edge (the invariant checks the right one).
                if (visible && SemanticsActions.OnClick in n.config && b.left < rootBounds.left - 1f) {
                    out += "[clipped-left] ${label(n)} starts at ${dp(b.left)}dp"
                }
                n.children.forEach(::walk)
            }
            walk(root)
            for (i in actions.indices) for (j in i + 1 until actions.size) {
                val a = actions[i]
                val b = actions[j]
                if (a.isAncestorOf(b) || b.isAncestorOf(a)) continue
                // A small control laid wholly over a larger tappable surface (the "?" on
                // a tappable row, the map card's expand button) is a deliberate overlay.
                if (a.boundsInRoot.encloses(b.boundsInRoot) || b.boundsInRoot.encloses(a.boundsInRoot)) continue
                val inter = a.boundsInRoot.intersect(b.boundsInRoot)
                if (inter.width > 2f && inter.height > 2f) {
                    out += "[overlap] ${label(a)} ∩ ${label(b)} (${dp(inter.width)}x${dp(inter.height)}dp)"
                }
            }
        }
        return out.distinct()
    }

    private fun SemanticsNode.isAncestorOf(other: SemanticsNode): Boolean {
        var p = other.parent
        while (p != null) {
            if (p.id == id) return true
            p = p.parent
        }
        return false
    }

    private fun label(n: SemanticsNode): String =
        (n.config.getOrNull(SemanticsProperties.ContentDescription)?.firstOrNull()
            ?: n.config.getOrNull(SemanticsProperties.Text)?.firstOrNull()?.text
            ?: n.config.getOrNull(SemanticsProperties.TestTag)
            ?: "node#${n.id}").let { "\"${it.take(40)}\"" }

    /** The layout dropped text: lines past maxLines or below a height clip, an
     *  ellipsis, or an unwrapped line wider than its box. */
    private val TextLayoutResult.cutsOffText: Boolean
        get() = didOverflowHeight ||
            (0 until lineCount).any { isLineEllipsized(it) } ||
            (!layoutInput.softWrap && multiParagraph.intrinsics.maxIntrinsicWidth > size.width + 1f)

    /** Lines this text needs at [widthPx] (the width its node really has: the
     *  semantics layout is re-run at the parent's max width, so it can't show a
     *  line cap that only bites at the real width). */
    private fun TextLayoutResult.linesAt(widthPx: Int): Int = androidx.compose.ui.text.Paragraph(
        text = layoutInput.text.text,
        style = layoutInput.style,
        constraints = androidx.compose.ui.unit.Constraints(maxWidth = widthPx.coerceAtLeast(1)),
        density = layoutInput.density,
        fontFamilyResolver = layoutInput.fontFamilyResolver,
    ).lineCount

    /** A line-capped (maxLines) wrapping text needs more lines than it may show. */
    private fun TextLayoutResult.needsMoreLinesAt(widthPx: Int): Boolean =
        layoutInput.softWrap && layoutInput.maxLines != Int.MAX_VALUE && linesAt(widthPx) > layoutInput.maxLines

    /** A soft line break fell INSIDE a word ("DISTA / NCE"): the box is
     *  narrower than one of the text's words. */
    private val TextLayoutResult.splitsAWord: Boolean
        get() {
            val text = layoutInput.text.text
            return (0 until lineCount - 1).any { i ->
                val end = getLineEnd(i)
                end in 1 until text.length && text[end - 1].isLetterOrDigit() && text[end].isLetterOrDigit()
            }
        }

    private fun Rect.encloses(o: Rect) =
        o.left >= left - 1f && o.top >= top - 1f && o.right <= right + 1f && o.bottom <= bottom + 1f

    /** Layout problems seen along the journey, "[where] problem". */
    private val problems = mutableListOf<String>()

    private fun record(where: String, found: List<String>) {
        problems += found.map { "[$where @ x$fontScale] $it" }
    }

    /** Record the current screen's layout problems under [where]. */
    private fun assertClean(where: String) = record(where, layoutProblems())

    /** Fail with every layout problem seen along the journey. */
    private fun assertJourneyClean() {
        val left = problems.distinct()
        assertTrue("layout problems at 320dp:\n" + left.joinToString("\n"), left.isEmpty())
    }

    private var fontScale = 1f

    private fun useFont(scale: Float) {
        fontScale = scale
        RuntimeEnvironment.setFontScale(scale)
    }

    /** Bring every action matching [actions] into view, one by one: each fully
     *  on screen (nothing cut by an edge) and, as a finger would scroll a little
     *  further, out from under the floating Log button. */
    private fun assertEveryActionReachable(where: String, actions: SemanticsMatcher) {
        val n = rule.onAllNodes(actions).fetchSemanticsNodes().size
        assertTrue("[$where] has actions", n > 0)
        for (i in 0 until n) {
            val node = rule.onAllNodes(actions)[i]
            runCatching { node.performScrollTo() }
            rule.waitForIdle()
            if (underFab(node.fetchSemanticsNode())) {
                // performScrollTo stops at the edge the FAB floats over: scroll on by the FAB.
                val fab = rule.onNode(logFab).fetchSemanticsNode().boundsInRoot
                rule.onAllNodes(hasScrollToIndexAction())[0]
                    .performSemanticsAction(SemanticsActions.ScrollBy) { it(0f, fab.height + with(rule.density) { 16.dp.toPx() }) }
                rule.waitForIdle()
            }
            val s = node.fetchSemanticsNode()
            val unclipped = Rect(s.positionInRoot, s.size.toSize())
            assertTrue(
                "[$where] ${label(s)} can't be brought fully into view: shown ${s.boundsInRoot} of $unclipped",
                s.boundsInRoot.height >= unclipped.height - 1f && s.boundsInRoot.width >= unclipped.width - 1f,
            )
            assertTrue("[$where] ${label(s)} stays under the Log button however Today scrolls", !underFab(s))
        }
    }

    /** [s] is (nearly) covered by the Log FAB of its OWN window (a sheet's
     *  rows sit in a window above the FAB, which can't cover them). */
    private fun underFab(s: SemanticsNode): Boolean = rule.onAllNodes(logFab).fetchSemanticsNodes().any { fab ->
        val cover = s.boundsInRoot.intersect(fab.boundsInRoot)
        s.rootId() == fab.rootId() &&
            cover.width > 2f && cover.height > 2f && s.boundsInRoot.height - cover.height < with(rule.density) { 24.dp.toPx() }
    }

    private fun SemanticsNode.rootId(): Int {
        var r = this
        while (r.parent != null) r = r.parent!!
        return r.id
    }

    /** The three tabs: whole, ≥48dp, inside the window, and each one switches. */
    private fun assertTabsWholeAndWorking(where: String) {
        val tabs = listOf(R.string.main_dest_today, R.string.main_dest_history, R.string.main_dest_profile)
        val window = rule.onAllNodes(isRoot()).fetchSemanticsNodes()[0].boundsInRoot
        val minPx = with(rule.density) { 48.dp.toPx() } - 0.5f
        for (t in tabs) {
            val node = rule.onNode(hasText(str(t)) and hasClickAction()).fetchSemanticsNode()
            val b = node.boundsInRoot
            assertTrue("[$where] tab ${str(t)} inside the window: $b of $window", b.left >= window.left - 1f && b.right <= window.right + 1f && b.bottom <= window.bottom + 1f)
            assertTrue("[$where] tab ${str(t)} ≥48dp: ${dp(b.width)}x${dp(b.height)}", b.height >= minPx && b.width >= minPx)
        }
        val navCut = layoutProblems().filter { p -> tabs.any { p.contains("\"${str(it)}\"") } }
        assertTrue("[$where] tab labels: $navCut", navCut.isEmpty())
    }

    /** Switch to [tab] by tapping it; the tap lands (the tab turns selected). */
    private fun goTo(tab: Int) {
        rule.onNode(hasText(str(tab)) and hasClickAction()).performClick()
        rule.waitForIdle()
        rule.waitUntil(10_000) { rule.onAllNodes(hasText(str(tab)) and isSelected()).fetchSemanticsNodes().isNotEmpty() }
    }

    private fun scrollTodayToEnd() {
        val list = rule.onAllNodes(hasScrollToIndexAction())[0]
        val info = list.fetchSemanticsNode().config.getOrNull(SemanticsProperties.CollectionInfo)
        if (info != null && info.rowCount > 0) list.performScrollToIndex(info.rowCount - 1)
        repeat(3) { list.performTouchInput { swipeUp() } }
        rule.waitForIdle()
    }

    private fun fabWidthDp() = dp(rule.onNode(logFab).fetchSemanticsNode().boundsInRoot.width)

    private fun strengthWithHistory() = QaHarness.seedLog(
        ctx,
        listOf(
            QaHarness.strengthProfile(),
            QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(6)),
            QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(4)),
            QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(2)),
        ),
    )

    private fun hybridProfile(): Event = Event.SetProfile(
        progressionCadence = ProgressionCadence.WeekToWeek,
        liftGoal = LiftGoal.MaxStrength,
        goalDistance = GoalDistance.FiveK,
        concurrentGoal = ConcurrentGoal.Strength,
        weeklySets = 12,
        runningDaysPerWeek = 3,
        runningKmPerWeek = 20.0,
        advanced = false,
        bodyweightKg = 80.0,
        ageYears = 30.0,
    )

    // ═══════════════ Today + the Log FAB + the chooser ═════════════════════

    private fun todayAndChooser(scale: Float) {
        useFont(scale)
        strengthWithHistory()
        rule.launchApp()
        assertClean("today top")
        assertTabsWholeAndWorking("today")
        // Extended "+ Log" at the top, inside the screen.
        val window = rule.onAllNodes(isRoot()).fetchSemanticsNodes()[0].boundsInRoot
        val fabTop = rule.onNode(logFab).fetchSemanticsNode().boundsInRoot
        assertTrue("the Log FAB fits the 320dp screen: $fabTop", fabTop.left >= window.left && fabTop.right <= window.right + 1f)
        val extended = fabWidthDp()
        assertTrue("extended FAB at the top (${extended}dp)", extended > 60f)
        // Every action on Today can be reached and is not left under the FAB.
        assertEveryActionReachable(
            "today",
            hasClickAction() and hasAnyAncestor(hasScrollToIndexAction()),
        )
        scrollTodayToEnd()
        assertClean("today end")
        val iconOnly = fabWidthDp()
        assertEquals("icon-only once scrolled (${iconOnly}dp)", 56f, iconOnly, 1.5f)
        val listNode = rule.onAllNodes(hasScrollToIndexAction())[0].fetchSemanticsNode()
        val lastBottom = listNode.children.map { it.boundsInRoot }.filter { it.height > 0f }.maxOf { it.bottom }
        val fab = rule.onNode(logFab).fetchSemanticsNode().boundsInRoot
        assertTrue("the last row (${dp(lastBottom)}dp) clears the Log FAB (${dp(fab.top)}dp)", lastBottom <= fab.top + 1f)
        // The chooser: every row reachable, nothing cut or overlapping.
        rule.onNode(logFab).performClick()
        rule.waitUntil(10_000) { rule.onAllNodes(hasText(str(R.string.main_report_pain)) and hasClickAction()).fetchSemanticsNodes().isNotEmpty() }
        assertClean("chooser")
        assertEveryActionReachable("chooser", hasClickAction() and hasAnyAncestor(SemanticsMatcher.keyIsDefined(SemanticsActions.ScrollBy)) and !hasAnyAncestor(hasScrollToIndexAction()))
        assertClean("chooser scrolled")
        assertJourneyClean()
    }

    /** Also the hero session title split mid-word ("Heav/y day") beside Move. */
    @Test fun todayAndChooser_font200() = todayAndChooser(2.0f)

    /** Also the week strip's day-type label "Rest" cut to "Res". */
    @Test fun todayAndChooser_font130() = todayAndChooser(1.3f)

    // ═══════════════ The set loop ═════════════════════════════════════════

    private fun setLoop(scale: Float) {
        useFont(scale)
        strengthWithHistory()
        rule.launchApp()
        val first = EventDispatcher.model.value!!.next_session!!.items.first { it.exercise.isNotEmpty() }
        rule.onNode(
            hasClickAction() and hasContentDescription(strPrefix(R.string.plan_log_set_n_of_n, 1), substring = true) and
                hasAnyAncestor(hasTestTag("plan-item:${first.exercise}")),
        ).performScrollTo().performClick()
        rule.waitUntil(10_000) { rule.onAllNodes(hasTestTag("editor-primary")).fetchSemanticsNodes().isNotEmpty() }
        assertClean("set loop")
        // The pinned primary is whole and on screen without a scroll.
        val primary = rule.onNode(hasTestTag("editor-primary")).fetchSemanticsNode()
        assertEquals("the loop's primary is whole", primary.size.height.toFloat(), primary.boundsInRoot.height, 1f)
        assertEveryActionReachable("set loop", hasClickAction() and hasAnyAncestor(hasTestTag("effort-scale")))
        // Log the set: the next header shows and the screen stays clean, also once
        // the rest countdown has joined the header's first row (its ticker shows
        // it within a second, so a check right after the tap would miss it).
        rule.onNode(hasTestTag("editor-primary")).performClick()
        rule.waitUntil(10_000) {
            rule.onAllNodes(hasText(str(R.string.log_set_header_n_of_n, 2, first.sets))).fetchSemanticsNodes().isNotEmpty() &&
                rule.onAllNodes(hasTestTag("rest-readout")).fetchSemanticsNodes().isNotEmpty()
        }
        assertClean("set loop after a set")
        assertJourneyClean()
    }

    @Test fun setLoop_font200() = setLoop(2.0f)

    /** Also field labels wrapped letter by letter beside their hint ("W/ar/m/-u/p"). */
    @Test fun setLoop_font130() = setLoop(1.3f)

    /** The "Logged … · Undo" row keeps the whole set (its reps), at most 2 lines. */
    @Test
    fun setLoopUndoRow_font200_showsTheWholeSet() {
        useFont(2.0f)
        strengthWithHistory()
        rule.launchApp()
        val first = EventDispatcher.model.value!!.next_session!!.items.first { it.exercise.isNotEmpty() }
        rule.onNode(
            hasClickAction() and hasContentDescription(strPrefix(R.string.plan_log_set_n_of_n, 1), substring = true) and
                hasAnyAncestor(hasTestTag("plan-item:${first.exercise}")),
        ).performScrollTo().performClick()
        rule.waitUntil(10_000) { rule.onAllNodes(hasTestTag("editor-primary")).fetchSemanticsNodes().isNotEmpty() }
        rule.onNode(hasTestTag("editor-primary")).performClick()
        // "Logged <set>" (not the bare "Logged" date chip label).
        val prefix = strPrefix(R.string.log_undo_logged) + " "
        val logged = SemanticsMatcher("undo row text") { n ->
            n.config.getOrNull(SemanticsProperties.Text).orEmpty().any { it.text.startsWith(prefix) }
        }
        rule.waitUntil(10_000) { rule.onAllNodes(logged, useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty() }
        rule.waitForIdle()
        val node = rule.onAllNodes(logged, useUnmergedTree = true).fetchSemanticsNodes().first()
        val layouts = mutableListOf<TextLayoutResult>()
        node.config[SemanticsActions.GetTextLayoutResult].action!!.invoke(layouts)
        val l = layouts.single()
        // The semantics layout is re-run at the parent's max width, so measure the
        // text again at the width the row actually gives it.
        val needed = l.linesAt(node.size.width)
        assertTrue("the undo row shows \"${l.layoutInput.text.text}\" whole: needs $needed lines, shows at most 2", needed <= 2)
    }

    // ═══════════════ History › run detail › rate sheet ════════════════════

    private fun runDetail(scale: Float) {
        useFont(scale)
        val end = QaHarness.daysAgoAtMidday(1)
        val points = (0 until 600).map { i ->
            app.milestone.GpsPoint(lat = 47.0 + i * 10.0 / 111_195.0, lon = 8.0, observedAt = end - (599 - i) * 3L, accuracyM = 5.0)
        }
        QaHarness.seedLog(
            ctx,
            listOf(
                hybridProfile(),
                Event.LogRunTrack(points = points, hrPctMax = 0.0, observedAt = end, entryId = end * 1000),
            ),
        )
        rule.launchApp()
        val title = str(R.string.history_run_weekday, "Sunday")
        goTo(R.string.main_dest_history)
        rule.scrollToText(title)
        assertClean("history")
        assertTabsWholeAndWorking("history")
        rule.onAllNodes(hasText(title))[0].performClick()
        rule.waitUntil(10_000) { rule.onAllNodes(hasTestTag("run-trim")).fetchSemanticsNodes().isNotEmpty() }
        assertClean("run detail top")
        for (tag in listOf("run-log-effort", "run-log-type", "run-export", "run-trim", "run-delete")) {
            assertEveryActionReachable("run detail $tag", hasTestTag(tag))
        }
        assertClean("run detail bottom")
        rule.onNode(hasTestTag("run-log-effort")).performScrollTo().performClick()
        rule.waitUntil(10_000) { rule.onAllNodes(hasTestTag("run-rate-save")).fetchSemanticsNodes().isNotEmpty() }
        assertClean("rate sheet")
        assertEveryActionReachable("rate sheet", hasTestTag("run-rate-save"))
        assertEveryActionReachable("rate sheet types", SemanticsMatcher("run type chip") {
            it.config.getOrNull(SemanticsProperties.TestTag)?.startsWith("run-type-") == true
        })
        assertJourneyClean()
    }

    /** Also the 3-up stats cutting their numbers ("3.7", "29:5", "8:"). */
    @Test fun runDetail_font200() = runDetail(2.0f)

    @Test fun runDetail_font130() = runDetail(1.3f)

    // ═══════════════ A pinned DO-NOT-TRAIN banner ═════════════════════════

    /** The banner is pinned (never scrolls); on a 320dp screen at a large font
     *  it must still show every one of its actions whole, and leave Today and
     *  the tabs usable under it. */
    private fun painHold(scale: Float) {
        useFont(scale)
        val morning = app.milestone.todayEpochDay() * 86_400L + 8 * 3_600L
        QaHarness.seedLog(
            ctx,
            listOf(QaHarness.strengthProfile(), Event.SubmitReadiness(ReadinessSignal.Pain, 1.0, morning)),
        )
        rule.launchApp()
        rule.waitUntil(10_000) { rule.onAllNodes(hasText(str(R.string.main_do_not_train))).fetchSemanticsNodes().isNotEmpty() }
        assertClean("pain hold")
        val bannerActions = resolveLabels().map { hasText(it) and hasClickAction() } +
            listOf(hasText(str(R.string.main_remove_report)) and hasClickAction(), hasText(str(R.string.main_add_details)) and hasClickAction())
        for (a in bannerActions) {
            val s = rule.onNode(a).fetchSemanticsNode()
            val unclipped = Rect(s.positionInRoot, s.size.toSize())
            assertTrue("banner action ${label(s)} is whole on screen: ${s.boundsInRoot} of $unclipped", s.boundsInRoot.height >= unclipped.height - 1f && s.boundsInRoot.height > 0f)
        }
        // The banner never scrolls: nothing may float over any of it.
        val banner = rule.onNode(hasText(str(R.string.main_do_not_train))).fetchSemanticsNode()
        var card = banner
        while (card.parent != null && card.parent!!.boundsInRoot.width < banner.rootBounds().width - 1f) card = card.parent!!
        rule.onAllNodes(logFab).fetchSemanticsNodes().forEach { fab ->
            val cover = card.boundsInRoot.intersect(fab.boundsInRoot)
            if (cover.width > 2f && cover.height > 2f) record("pain hold", listOf("[fab-over-banner] the Log button covers ${dp(cover.width)}x${dp(cover.height)}dp of the banner"))
        }
        assertTabsWholeAndWorking("pain hold")
        // Today under the banner keeps at least one whole touch row, so it stays
        // reachable. The banner's hold statement (headline, reason, actions) always
        // shows whole first (HARD RULE 3, INVARIANT 3), so this is all Today is owed.
        val list = rule.onAllNodes(hasScrollToIndexAction())[0].fetchSemanticsNode().boundsInRoot
        if (dp(list.height) < app.milestone.ActionMinHeight.value) record("pain hold", listOf("[no-room] Today gets ${dp(list.height)}dp under the pinned banner"))
        goTo(R.string.main_dest_profile)
        assertEquals("the banner stays pinned on Profile", 1, rule.onAllNodes(hasText(str(R.string.main_do_not_train))).fetchSemanticsNodes().size)
        assertJourneyClean()
    }

    private fun SemanticsNode.rootBounds(): Rect {
        var r = this
        while (r.parent != null) r = r.parent!!
        return r.boundsInRoot
    }

    /** Also the pinned banner taking the whole screen with the Log button over it. */
    @Test fun painHold_font200() = painHold(2.0f)

    @Test fun painHold_font130() = painHold(1.3f)

    // ═══════════════ Check-ins (RatingScale rows + pinned Save) ════════════

    private fun checkins(scale: Float) {
        useFont(scale)
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        for (title in listOf(R.string.main_morning_checkin_title, R.string.main_weekly_checkin_title)) {
            rule.onNode(logFab).performClick()
            rule.waitUntil(10_000) { rule.onAllNodes(hasText(str(title)) and hasClickAction()).fetchSemanticsNodes().isNotEmpty() }
            rule.onNode(hasText(str(title)) and hasClickAction()).performScrollTo().performClick()
            rule.waitUntil(10_000) { rule.onAllNodes(hasTestTag("editor-primary")).fetchSemanticsNodes().isNotEmpty() }
            assertClean(str(title))
            // Every rating (track or its large-font fallback) is reachable, whole.
            val tags = if (title == R.string.main_morning_checkin_title) {
                listOf("checkin-sleep", "checkin-soreness", "checkin-mood")
            } else {
                listOf("weekly-failed", "weekly-rpe-gap")
            }
            tags.forEach { assertEveryActionReachable("${str(title)} $it", hasTestTag(it) or (hasClickAction() and hasAnyAncestor(hasTestTag(it)))) }
            assertClean("${str(title)} scrolled")
            val primary = rule.onNode(hasTestTag("editor-primary")).fetchSemanticsNode()
            assertEquals("${str(title)}: Save is whole", primary.size.height.toFloat(), primary.boundsInRoot.height, 1f)
            rule.onNode(hasContentDescription(str(R.string.main_close)) and hasClickAction()).performClick()
            rule.waitUntil(10_000) { rule.onAllNodes(hasTestTag("editor-primary")).fetchSemanticsNodes().isEmpty() }
        }
        assertJourneyClean()
    }

    @Test fun checkins_font200() = checkins(2.0f)

    @Test fun checkins_font130() = checkins(1.3f)

    // ═══════════════ Profile › HR zones calculator ════════════════════════

    private fun profileCalculator(scale: Float) {
        useFont(scale)
        QaHarness.seedLog(ctx, listOf(hybridProfile(), Event.ComputeHrZones(30.0, null)))
        rule.launchApp()
        val calcRow = hasText(str(R.string.main_tool_hr_title)) and hasClickAction()
        goTo(R.string.main_dest_profile)
        assertClean("profile")
        assertTabsWholeAndWorking("profile")
        rule.onAllNodes(hasScrollToNodeAction())[0].performScrollToNode(calcRow)
        rule.onAllNodes(calcRow)[0].performClick()
        rule.waitUntil(10_000) { rule.onAllNodes(hasTestTag("calc-result")).fetchSemanticsNodes().isNotEmpty() }
        assertClean("hr zones")
        assertEveryActionReachable("hr zones", hasText(str(R.string.log_btn_compute_hr)) and hasClickAction())
        assertEveryActionReachable("hr zones clear", hasText(str(R.string.main_calc_clear)) and hasClickAction())
        assertClean("hr zones scrolled")
        assertJourneyClean()
    }

    @Test fun profileCalculator_font200() = profileCalculator(2.0f)

    /** Also the HR zones table splitting names and ranges mid-word ("Maratho/n", "122–14/8"). */
    @Test fun profileCalculator_font130() = profileCalculator(1.3f)
}
