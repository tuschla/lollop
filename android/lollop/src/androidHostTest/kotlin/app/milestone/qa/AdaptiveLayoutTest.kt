package app.milestone.qa

import android.Manifest
import android.app.Application
import android.content.Context
import android.location.LocationManager
import android.view.WindowManager
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasScrollToNodeAction
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isDialog
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.unit.dp
import androidx.core.graphics.Insets
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.test.core.app.ApplicationProvider
import androidx.window.core.layout.WindowSizeClass
import app.milestone.AppSettings
import app.milestone.ContentMaxWidth
import app.milestone.Event
import app.milestone.EventDispatcher
import app.milestone.GpsPoint
import app.milestone.HistoryFocus
import app.milestone.OpenDest
import app.milestone.R
import app.milestone.RunSession
import app.milestone.WindowLayout
import app.milestone.openIntent
import app.milestone.windowLayoutFor
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.shadows.ShadowDialog

/**
 * The adaptive layout (visual-design-system §5 "Adaptive layout") over the REAL
 * app and core: which window gets the rail, the two panes and the controls beside
 * the content; that a landscape set logger shows its keypad and Save without a
 * scroll; that the guided setup can be finished in a short landscape window; that
 * the primary actions stay on screen at 200 % font in landscape; and that a
 * rotation between one and two panes keeps what the user had open or typed.
 */
@RunWith(RobolectricTestRunner::class)
class AdaptiveLayoutTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @Before
    fun setUp() {
        QaHarness.reset(ctx)
        shadowOf(ctx as Application).grantPermissions(
            Manifest.permission.ACCESS_FINE_LOCATION,
            Manifest.permission.ACCESS_COARSE_LOCATION,
            Manifest.permission.POST_NOTIFICATIONS,
        )
        shadowOf(ctx.getSystemService(LocationManager::class.java)).setLocationEnabled(true)
        AppSettings.setShowRouteMaps(ctx, false)
    }

    @After
    fun tearDown() {
        QaHarness.teardown()
        RunSession.reset()
        QaHarness.clearSidecar(ctx)
        QaHarness.freshProcess()
    }

    private companion object {
        const val PORTRAIT = "w411dp-h891dp-port-xxhdpi"
        const val LANDSCAPE = "w891dp-h411dp-land-xxhdpi"
        const val SMALL_LANDSCAPE = "w640dp-h360dp-land-xhdpi"
        const val TABLET_PORTRAIT = "w800dp-h1280dp-port-xhdpi"
        /** A 914×411dp phone in landscape after its status bar and gesture
         *  navigation bar (24dp each), the height a sheet's content gets. */
        const val PHONE_LANDSCAPE_AFTER_BARS = "w891dp-h363dp-land-xxhdpi"
        const val SQUAT = "Back squat"
    }

    // ── Helpers ─────────────────────────────────────────────────────────────

    private fun window(qualifiers: String, font: Float = 1f) {
        RuntimeEnvironment.setQualifiers(qualifiers)
        RuntimeEnvironment.setFontScale(font)
    }

    /** A rotation: the new window size, then the activity recreated as the OS does. */
    private fun rotate(qualifiers: String) {
        RuntimeEnvironment.setQualifiers(qualifiers)
        checkNotNull(lastScenario).recreate()
        rule.waitUntil(15_000) { rule.onAllNodes(isRoot()).fetchSemanticsNodes().isNotEmpty() }
        rule.waitForIdle()
    }

    private fun dp(px: Float) = with(rule.density) { px.toDp().value }

    private val screen: Rect
        get() = ctx.resources.displayMetrics.let { Rect(0f, 0f, it.widthPixels.toFloat(), it.heightPixels.toFloat()) }

    private fun node(matcher: SemanticsMatcher): SemanticsNode = rule.onAllNodes(matcher).onFirst().fetchSemanticsNode()

    /** [n] is drawn whole (nothing clipped by a scroller or the window) on screen. */
    private fun assertWhollyOnScreen(what: String, n: SemanticsNode) {
        val b = n.boundsInRoot
        val s = screen
        assertTrue(
            "$what is cut: shown ${dp(b.width)}x${dp(b.height)}dp of ${dp(n.size.width.toFloat())}x${dp(n.size.height.toFloat())}dp at $b",
            b.height >= n.size.height - 2f && b.width >= n.size.width - 2f,
        )
        assertTrue("$what is off screen at $b (window $s)", b.top >= s.top - 1f && b.bottom <= s.bottom + 1f && b.left >= -1f && b.right <= s.right + 1f)
    }

    private fun seedSquatYesterday() {
        val at = QaHarness.daysAgoAtMidday(1)
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), Event.LogSet(SQUAT, 120.0, 3, 8.0, at, at * 1000)))
    }

    private val weightWell get() = hasClickAction() and hasContentDescription(str(R.string.log_field_weight), substring = true)

    /** Tap the weight well (by its click action: beside the keypad pane the well can
     *  sit at the scroll edge, where a centre tap falls outside the viewport). */
    private fun openWeightKeypad() {
        rule.onAllNodes(weightWell)[0].performScrollTo().performSemanticsAction(SemanticsActions.OnClick)
        rule.waitForNode(hasTestTag("numeric-keypad"))
        rule.waitForIdle()
    }

    private fun key(k: Char) {
        val label = if (k == '.') hasContentDescription(str(R.string.ui_cd_decimal_point)) else hasText("$k")
        rule.onNode(hasClickAction() and (label or androidx.compose.ui.test.hasAnyDescendant(label)) and hasAnyAncestor(hasTestTag("numeric-keypad")))
            .performClick()
    }

    private fun violations(): List<String> {
        rule.waitForIdle()
        return UxInvariants.check(rule, rule.density).filter { it.kind in UxInvariants.KINDS }.map { it.toString() }
    }

    // ── Window size classes ─────────────────────────────────────────────────

    @Test
    fun windowLayout_followsTheSizeClassAndAspect() {
        fun of(w: Float, h: Float) = windowLayoutFor(WindowSizeClass.compute(w, h), landscape = w > h)
        // A portrait phone keeps the bottom bar, one column, controls under the content.
        assertEquals(WindowLayout.Compact, of(411f, 891f))
        assertEquals(WindowLayout.Compact, of(320f, 640f))
        // A phone in landscape: rail, no wordmark bar, two panes, controls beside.
        assertEquals(WindowLayout(navRail = true, shortHeight = true, twoPane = true, sideControls = true), of(891f, 411f))
        // A small phone in landscape: too narrow for two panes.
        assertEquals(WindowLayout(navRail = true, shortHeight = true, twoPane = false, sideControls = true), of(640f, 360f))
        // A tablet / unfolded foldable in portrait: rail and one capped column.
        assertEquals(WindowLayout(navRail = true, shortHeight = false, twoPane = false, sideControls = false), of(800f, 1280f))
        assertEquals(WindowLayout(navRail = true, shortHeight = false, twoPane = false, sideControls = false), of(673f, 841f))
        // A tablet in landscape.
        assertEquals(WindowLayout(navRail = true, shortHeight = false, twoPane = true, sideControls = true), of(1280f, 800f))
        // A portrait phone in split screen (half height): the rail gives the height back.
        assertEquals(WindowLayout(navRail = true, shortHeight = true, twoPane = false, sideControls = false), of(411f, 430f))
    }

    // ── Chrome ──────────────────────────────────────────────────────────────

    @Test
    fun portraitPhone_keepsBottomBarFabAndWordmark() {
        window(PORTRAIT)
        seedSquatYesterday()
        rule.launchApp()
        assertEquals(1, rule.countOf(hasTestTag("bottom-nav")))
        assertEquals(0, rule.countOf(hasTestTag("nav-rail")))
        assertEquals(1, rule.countOf(hasText(str(R.string.app_name))))
        assertEquals(0, rule.countOf(hasTestTag("today-week-pane")))
    }

    @Test
    fun landscapePhone_railReplacesBarAndWordmark_todaySessionBesideWeek() {
        window(LANDSCAPE)
        seedSquatYesterday()
        rule.launchApp()
        assertEquals("no bottom bar", 0, rule.countOf(hasTestTag("bottom-nav")))
        assertEquals(1, rule.countOf(hasTestTag("nav-rail")))
        assertEquals("the wordmark bar gives its height back", 0, rule.countOf(hasText(str(R.string.app_name))))
        assertEquals(1, rule.countOf(hasTestTag("rail-brand")))
        // Log is the rail's header, not a floating button over the list.
        assertEquals(1, rule.countOf(hasTestTag("log-fab") and hasAnyAncestor(hasTestTag("nav-rail"))))
        // The session at the start, the week beside it, both starting at the top.
        val session = node(hasTestTag("today-session-pane")).boundsInRoot
        val week = node(hasTestTag("today-week-pane")).boundsInRoot
        assertTrue("session pane $session before week pane $week", session.right <= week.left + 1f)
        assertEquals(1, rule.countOf(hasTestTag("plan-item:$SQUAT") and hasAnyAncestor(hasTestTag("today-session-pane"))))
        assertEquals(
            1,
            rule.countOf(hasText(str(R.string.main_overline_this_week).uppercase(java.util.Locale.US)) and hasAnyAncestor(hasTestTag("today-week-pane"))),
        )
        // The hero's primary action is on screen without a scroll.
        assertWhollyOnScreen(
            "Log set 1 of 4",
            node(hasClickAction() and hasContentDescription(strPrefix(R.string.plan_log_set_n_of_n, 1), substring = true)),
        )
        assertEquals(emptyList<String>(), violations())
    }

    @Test
    fun tabletPortrait_oneColumnCappedAndCentred() {
        window(TABLET_PORTRAIT)
        seedSquatYesterday()
        rule.launchApp()
        assertEquals(1, rule.countOf(hasTestTag("nav-rail")))
        assertEquals("a tall window keeps the wordmark", 1, rule.countOf(hasText(str(R.string.app_name))))
        assertEquals(0, rule.countOf(hasTestTag("today-week-pane")))
        val item = node(hasTestTag("plan-item:$SQUAT")).boundsInRoot
        assertTrue("item ${dp(item.width)}dp wide, over the 640dp column", dp(item.width) <= 640f)
        val rail = node(hasTestTag("nav-rail")).boundsInRoot
        val leftGap = item.left - rail.right
        val rightGap = screen.right - item.right
        assertTrue("column not centred: ${dp(leftGap)}dp | ${dp(rightGap)}dp", kotlin.math.abs(leftGap - rightGap) < with(rule.density) { 40.0f * density })
    }

    // ── Set logger ──────────────────────────────────────────────────────────

    /** Bug: in landscape the set logger showed the set header and the effort row;
     *  the weight field and its keypad sat below the sheet's fold, and an open
     *  keypad covered the whole form. Now the keypad stands beside the form, whole,
     *  with Save under it. */
    @Test
    fun landscapeSetLogger_keypadBesideTheWeight_wholeWithSave() {
        window(LANDSCAPE)
        seedSquatYesterday()
        rule.launchApp()
        rule.openLoopFor(SQUAT)
        assertEquals(1, rule.countOf(hasTestTag("editor-side-by-side")))
        openWeightKeypad()
        val keypad = node(hasTestTag("numeric-keypad"))
        val well = node(weightWell)
        assertTrue("keypad ${keypad.boundsInRoot} not beside the weight ${well.boundsInRoot}", keypad.boundsInRoot.left >= well.boundsInRoot.right)
        assertWhollyOnScreen("keypad", keypad)
        assertWhollyOnScreen("Done", node(hasClickAction() and hasText(str(R.string.log_keypad_done))))
        assertWhollyOnScreen("Log set", node(hasTestTag("editor-primary")))
        assertTrue("the weight well is off screen: ${well.boundsInRoot}", well.boundsInRoot.height > 0f)
        // Typing lands in the well beside it.
        listOf('1', '2', '5').forEach(::key)
        rule.onNode(hasContentDescription("${str(R.string.log_field_weight)}, 125 kg")).assertExists()
        assertEquals(emptyList<String>(), violations())
    }

    /** Bug (e37 emulator, 914×411dp landscape): the 411dp-tall test window above
     *  has no system bars; on a phone the status and navigation bars take ~48dp,
     *  and the keypad column (four rows, a full-width Done, the effort row) then
     *  scrolled: Done and the effort row sat under the fold, a strip of a fifth
     *  key row showing (`~/.cache/ux6/UxDevice/before/e37-land-keypad.png`). The
     *  window here is that phone after its bars; keypad, Done and Save fit whole,
     *  also at 130 % font. */
    private fun keypadDoneAndSaveFitAfterSystemBars(font: Float) {
        window(PHONE_LANDSCAPE_AFTER_BARS, font)
        seedSquatYesterday()
        rule.launchApp()
        rule.openLoopFor(SQUAT)
        assertEquals(1, rule.countOf(hasTestTag("editor-side-by-side")))
        openWeightKeypad()
        assertWhollyOnScreen("keypad", node(hasTestTag("numeric-keypad")))
        assertWhollyOnScreen("Done", node(hasClickAction() and hasText(str(R.string.log_keypad_done))))
        assertWhollyOnScreen("Log set", node(hasTestTag("editor-primary")))
        listOf('1', '2', '5').forEach(::key)
        rule.onNode(hasContentDescription("${str(R.string.log_field_weight)}, 125 kg")).assertExists()
        // Done folds the keypad; Save stays put.
        rule.onNode(hasClickAction() and hasText(str(R.string.log_keypad_done))).performClick()
        rule.waitForIdle()
        assertEquals(0, rule.countOf(hasTestTag("numeric-keypad")))
        assertWhollyOnScreen("Log set after Done", node(hasTestTag("editor-primary")))
        assertEquals(emptyList<String>(), violations())
    }

    @Test
    fun landscapePhoneAfterSystemBars_keypadDoneAndSaveFitWhole() = keypadDoneAndSaveFitAfterSystemBars(1f)

    @Test
    fun landscapePhoneAfterSystemBars_font130_keypadDoneAndSaveFitWhole() = keypadDoneAndSaveFitAfterSystemBars(1.3f)

    /** Bug (e37 emulator, 914×411dp landscape, 130 % font): beside the target hint
     *  the effort label broke as "Effort: reps / left"
     *  (`~/.cache/ux6/UxDevice3/after/e37-land-f13-afterdone.png`). A label that
     *  fits its own line keeps it; the hint goes below. */
    @Test
    fun phoneLandscapeAfterBars_font130_effortLabelOnOneLine() {
        window(PHONE_LANDSCAPE_AFTER_BARS, 1.3f)
        seedSquatYesterday()
        rule.launchApp()
        rule.openLoopFor(SQUAT)
        val label = hasText(str(R.string.log_field_rpe))
        rule.waitForNode(label)
        val hint = rule.onAllNodes(hasText(str(R.string.log_rpe_target_hint, ""), substring = true), useUnmergedTree = true).fetchSemanticsNodes()
        assertTrue("the coach's target hint is shown", hint.isNotEmpty())
        val n = rule.onNode(label, useUnmergedTree = true).fetchSemanticsNode()
        val layout = mutableListOf<TextLayoutResult>()
        n.config[SemanticsActions.GetTextLayoutResult].action?.invoke(layout)
        assertEquals("\"${str(R.string.log_field_rpe)}\" lines", 1, layout.single().lineCount)
        assertTrue("the hint ${hint[0].boundsInRoot} goes below the label ${n.boundsInRoot}", hint[0].boundsInRoot.top >= n.boundsInRoot.bottom - 1f)
    }

    /** Bug (e37 emulator, landscape, camera cutout on the left): the rail took the
     *  cutout's start inset and the content beside it padded for it again, a 49dp
     *  empty strip between the rail and Today's cards
     *  (`~/.cache/ux6/UxDevice/before/e37-land-today.png`). With a cutout the
     *  rail moves over by it and the content keeps its gap to the rail. */
    @Test
    fun landscapeCutout_contentKeepsItsGapToTheRail() {
        window(LANDSCAPE)
        seedSquatYesterday()
        rule.launchApp()
        fun rail() = node(hasTestTag("nav-rail")).boundsInRoot
        fun gap() = node(hasTestTag("today-session-pane")).boundsInRoot.left - rail().right
        val railBefore = rail().width
        val before = gap()
        val cutoutPx = with(rule.density) { 48.dp.roundToPx() }
        checkNotNull(lastScenario).onActivity { activity ->
            val insets = WindowInsetsCompat.Builder()
                .setInsets(WindowInsetsCompat.Type.displayCutout(), Insets.of(cutoutPx, 0, 0, 0))
                .build()
            ViewCompat.dispatchApplyWindowInsets(activity.window.decorView, insets)
        }
        rule.waitForIdle()
        assertEquals("the rail takes the cutout (px)", railBefore + cutoutPx, rail().width, 1f)
        assertEquals("content to rail gap (dp) with a cutout", dp(before), dp(gap()), 1f)
    }

    @Test
    fun portraitSetLogger_keypadStaysUnderTheWell() {
        window(PORTRAIT)
        seedSquatYesterday()
        rule.launchApp()
        rule.openLoopFor(SQUAT)
        assertEquals(0, rule.countOf(hasTestTag("editor-side-by-side")))
        openWeightKeypad()
        assertTrue(node(hasTestTag("numeric-keypad")).boundsInRoot.top >= node(weightWell).boundsInRoot.bottom - 1f)
    }

    // ── Setup ───────────────────────────────────────────────────────────────

    /** Bug: in a 640×360 landscape window the race step's keypad band filled the
     *  screen: the question, the time field and Back/Next were laid out off screen
     *  (Next 0×0) and the keypad's last row was cut, so the setup could not go on.
     *  Now the question stands beside the answers and the keypad. */
    @Test
    fun shortLandscape_guidedSetupCanBeFinished() {
        window(SMALL_LANDSCAPE)
        rule.launchAppExpectingSetup()
        GuidedSetupWalk.walk(rule, stopAt = "race")
        GuidedSetupWalk.bothPath.single { it.name == "race" }.answer(rule)
        rule.waitForIdle()
        assertWhollyOnScreen("Next", node(hasText(str(R.string.setup_btn_next)) and hasClickAction()))
        assertWhollyOnScreen("the question", node(hasText(str(R.string.setup_step_race_q))))
        rule.onNode(hasText(str(R.string.setup_btn_next)) and hasClickAction()).performClick()
        rule.waitForIdle()
        // On from the lead question to the review, each screen answered and Next tapped.
        GuidedSetupWalk.bothPath.dropWhile { it.name != "lead" }.takeWhile { it.name != "review" }.forEach { s ->
            s.answer(rule)
            rule.waitForIdle()
            assertWhollyOnScreen("Next on ${s.name}", node(hasText(str(R.string.setup_btn_next)) and hasClickAction()))
            rule.onNode(hasText(str(R.string.setup_btn_next)) and hasClickAction()).performClick()
            rule.waitForIdle()
        }
        rule.waitForNode(hasText(str(R.string.setup_step_review_q)))
    }

    @Test
    fun font200Landscape_setupWalkReachesTheReview() {
        window(LANDSCAPE, font = 2f)
        rule.launchAppExpectingSetup()
        GuidedSetupWalk.walk(rule, stopAt = "review")
        rule.waitForNode(hasText(str(R.string.setup_step_review_q)))
        assertWhollyOnScreen("Finish", node(hasText(str(R.string.setup_btn_finish)) and hasClickAction()))
    }

    /** UxDevice5 (e37 landscape, `~/.cache/ux6/UxDevice5/after/e37-land270-setup-review.png`):
     *  beside the question, the review's scroll-edge seam (portrait's seam over the
     *  button bar, widened by the screen margin) ran past the answers card into the
     *  gutter and on to the window edge. Beside the question no bar sits under the
     *  answers: while more lies below they fade into the ground inside their column. */
    @Test
    fun landscapeSetupReview_scrollEdgeStaysInTheAnswersColumn() {
        window(PHONE_LANDSCAPE_AFTER_BARS)
        rule.launchAppExpectingSetup()
        GuidedSetupWalk.walk(rule, stopAt = "review")
        rule.waitForNode(hasText(str(R.string.setup_step_review_q)))
        rule.waitForIdle()
        val question = node(hasTestTag("setup-question-pane")).boundsInRoot
        val answers = node(hasTestTag("setup-answers")).boundsInRoot
        val image = rule.onAllNodes(isRoot())[0].captureToImage().toPixelMap()
        fun near(a: androidx.compose.ui.graphics.Color, b: androidx.compose.ui.graphics.Color) =
            maxOf(kotlin.math.abs(a.red - b.red), kotlin.math.abs(a.green - b.green), kotlin.math.abs(a.blue - b.blue)) < 0.02f
        val ground = image[(question.right + 4f).toInt(), answers.top.toInt()]
        // The gutter between the columns and the margin after the answers stay ground
        // along the answers column and a step below it.
        val xs = ((question.right + 1f).toInt() until (answers.left - 1f).toInt()) + ((answers.right + 1f).toInt() until image.width)
        val ys = answers.top.toInt() until minOf(image.height, (answers.bottom + 24 * rule.density.density).toInt())
        val off = xs.flatMap { x -> ys.filter { y -> !near(image[x, y], ground) }.map { y -> x to y } }
        assertTrue("drawn outside the answers column at ${off.take(4)} (${off.size} px)", off.isEmpty())
        // More answers lie below: the card fades into the ground at the column's foot.
        val x = (answers.left + 4f).toInt()
        assertTrue("the card under the fade", !near(image[x, (answers.bottom - 40 * rule.density.density).toInt()], ground))
        assertTrue("the column's foot is the ground", near(image[x, answers.bottom.toInt() - 1], ground))
    }

    // ── Primary actions at 200 % font in landscape ──────────────────────────

    private fun primaryActionsOnScreenAt200(qualifiers: String) {
        window(qualifiers, font = 2f)
        seedSquatYesterday()
        rule.launchApp()
        assertWhollyOnScreen("Log", node(hasTestTag("log-fab")))
        listOf(R.string.main_dest_today, R.string.main_dest_history, R.string.main_dest_profile).forEach {
            assertWhollyOnScreen(str(it), node(hasText(str(it)) and hasAnyAncestor(hasTestTag("nav-rail"))))
        }
        rule.openLoopFor(SQUAT)
        assertWhollyOnScreen("Log set", node(hasTestTag("editor-primary")))
        assertWhollyOnScreen("Close", node(hasClickAction() and hasContentDescription(str(R.string.main_close))))
        assertEquals(emptyList<String>(), violations())
    }

    @Test
    fun font200_phoneLandscape_primaryActionsOnScreen() = primaryActionsOnScreenAt200(LANDSCAPE)

    @Test
    fun font200_smallLandscape_primaryActionsOnScreen() = primaryActionsOnScreenAt200(SMALL_LANDSCAPE)

    // ── History ─────────────────────────────────────────────────────────────

    private fun gpsTenK() = QaHarness.gpsTenK(QaHarness.daysAgoAtMidday(0) - 5 * 3600 - 1010 * 3L)

    private fun seedRuns(run: Event.LogRunTrack) {
        val older = QaHarness.daysAgoAtMidday(3)
        QaHarness.seedLog(ctx, listOf(QaHarness.fiveKRunnerProfile(), Event.LogRun(6.0, 33.0, 0.0, observedAt = older, entryId = older * 1000), run))
    }

    @Test
    fun landscapeHistory_entryOpensBesideTheList_andClosesToTheOverview() {
        window(LANDSCAPE)
        seedRuns(gpsTenK())
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_history))
        assertEquals(1, rule.countOf(hasTestTag("history-list-pane")))
        assertEquals(1, rule.countOf(hasTestTag("history-overview-pane")))
        val windows = rule.countOf(isRoot())
        // The newest run's card, in the list pane.
        rule.onAllNodes(hasAnyAncestor(hasTestTag("history-list-pane")) and hasClickAction() and hasText(str(R.string.history_run_weekday, ""), substring = true))
            .onFirst().performClick()
        rule.waitForNode(hasTestTag("run-trim"))
        assertEquals("the detail opens in the pane, not a sheet", windows, rule.countOf(isRoot()))
        assertEquals(1, rule.countOf(hasTestTag("run-trim") and hasAnyAncestor(hasTestTag("detail-pane"))))
        assertEquals(0, rule.countOf(hasTestTag("history-overview-pane")))
        val list = node(hasTestTag("history-list-pane")).boundsInRoot
        val detail = node(hasTestTag("detail-pane")).boundsInRoot
        assertTrue("list $list before detail $detail", list.right <= detail.left + 1f)
        rule.onNodeWithTag("detail-pane-close").performClick()
        rule.waitForIdle()
        assertEquals(0, rule.countOf(hasTestTag("detail-pane")))
        assertEquals(1, rule.countOf(hasTestTag("history-overview-pane")))
    }

    private val selected = SemanticsMatcher.expectValue(SemanticsProperties.Selected, true)
    /** A clickable entry card in History's list (a button; the Lifts | Runs
     *  segment above the cards is selectable too, but not a button). */
    private val inList = hasAnyAncestor(hasTestTag("history-list-pane")) and hasClickAction() and
        SemanticsMatcher.expectValue(SemanticsProperties.Role, androidx.compose.ui.semantics.Role.Button)
    private val runCard get() = inList and hasText(str(R.string.history_run_weekday, ""), substring = true)
    private val liftCard get() = inList and hasText(SQUAT, substring = true, ignoreCase = true)

    /** Two runs, a squat session and a set in landscape History. */
    private fun openLandscapeHistoryWithLiftsAndRuns() {
        window(LANDSCAPE)
        val older = QaHarness.daysAgoAtMidday(3)
        val oldest = QaHarness.daysAgoAtMidday(5)
        val at = QaHarness.daysAgoAtMidday(1)
        QaHarness.seedLog(
            ctx,
            listOf(
                QaHarness.hybridFiveKProfile(), Event.LogRun(5.0, 28.0, 0.0, observedAt = oldest, entryId = oldest * 1000),
                Event.LogRun(6.0, 33.0, 0.0, observedAt = older, entryId = older * 1000), Event.LogSet(SQUAT, 120.0, 3, 8.0, at, at * 1000),
            ),
        )
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_history))
        rule.waitForNode(hasTestTag("history-list-pane"))
    }

    private fun segment(label: Int) {
        rule.onNode(hasText(str(label)) and hasClickAction()).performClick()
        rule.waitForIdle()
    }

    /** UxDevice3 (e37 landscape): with an entry open beside the list, its card in
     *  the list did not look or read as the open one
     *  (`~/.cache/ux6/UxDevice3/after/e37-land-history-entry.png`). The open
     *  run's or session's card is selected (an accent bar, TalkBack "selected");
     *  closing the pane clears it. */
    @Test
    fun landscapeHistory_theOpenEntrysCardIsSelected() {
        openLandscapeHistoryWithLiftsAndRuns()
        segment(R.string.main_segment_runs)
        assertEquals("nothing open, nothing selected", 0, rule.countOf(inList and selected))
        rule.onAllNodes(runCard).onFirst().performClick()
        rule.waitForNode(hasTestTag("detail-pane"))
        rule.assertCountEventually(runCard and selected, 1)
        assertEquals(1, rule.countOf(inList and selected))
        // What TalkBack gets for the open card and for another one.
        val open = a11yInfo(node(runCard and selected).id)
        assertTrue("open card: checked ${open.isChecked}, state \"${open.stateDescription}\"", open.isChecked && open.stateDescription?.toString() == "Selected")
        val other = a11yInfo(rule.onAllNodes(runCard and !selected).onFirst().fetchSemanticsNode().id)
        assertTrue("another card carries no selection state: \"${other.stateDescription}\"", !other.isCheckable && other.stateDescription == null)
        rule.onNodeWithTag("detail-pane-close").performClick()
        rule.waitForIdle()
        assertEquals(0, rule.countOf(inList and selected))

        segment(R.string.main_segment_lifts)
        rule.onAllNodes(liftCard).onFirst().performClick()
        rule.waitForNode(hasTestTag("detail-pane"))
        rule.assertCountEventually(liftCard and selected, 1)
        // A set opened from the session keeps its session's card selected.
        rule.onAllNodes(hasTestTag("lift-set-row")).onFirst().performClick()
        rule.waitForNode(hasTestTag("detail-pane"))
        rule.waitForIdle()
        assertEquals(1, rule.countOf(liftCard and selected))
    }

    /** The accessibility node TalkBack reads for semantics node [id] in the app window. */
    private fun a11yInfo(id: Int): android.view.accessibility.AccessibilityNodeInfo {
        var info: android.view.accessibility.AccessibilityNodeInfo? = null
        checkNotNull(lastScenario).onActivity { a ->
            fun find(v: android.view.View): android.view.View? =
                if (v.javaClass.name.endsWith("AndroidComposeView")) v
                else (v as? android.view.ViewGroup)?.let { g -> (0 until g.childCount).firstNotNullOfOrNull { find(g.getChildAt(it)) } }
            val view = checkNotNull(find(a.window.decorView)) { "no compose view" }
            info = view.accessibilityNodeProvider?.createAccessibilityNodeInfo(id)
        }
        return checkNotNull(info) { "no accessibility node for $id" }
    }

    /** The ground of the open detail pane, sampled in its start gutter. */
    private fun paneGround(): androidx.compose.ui.graphics.Color {
        rule.waitForIdle()
        val pane = node(hasTestTag("detail-pane")).boundsInRoot
        val image = rule.onAllNodes(isRoot())[0].captureToImage().toPixelMap()
        return image[(pane.left + 2f).toInt(), (pane.top + pane.height * 0.9f).toInt()]
    }

    /** UxDevice3 (e37 landscape): a run opened beside the list stood on the sheet
     *  ground, a lift session or set on the card ground, so the pane changed colour
     *  with the entry kind. Every detail is on the sheet ground. */
    @Test
    fun landscapeHistory_runAndLiftDetailsShareTheirGround() {
        openLandscapeHistoryWithLiftsAndRuns()
        segment(R.string.main_segment_runs)
        rule.onAllNodes(runCard).onFirst().performClick()
        rule.waitForNode(hasTestTag("detail-pane"))
        val run = paneGround()
        rule.onNodeWithTag("detail-pane-close").performClick()
        rule.waitForIdle()
        segment(R.string.main_segment_lifts)
        rule.onAllNodes(liftCard).onFirst().performClick()
        rule.waitForNode(hasTestTag("detail-pane"))
        val session = paneGround()
        rule.onAllNodes(hasTestTag("lift-set-row")).onFirst().performClick()
        rule.waitForIdle()
        val set = paneGround()
        assertEquals("session pane vs run pane", run, session)
        assertEquals("set pane vs run pane", run, set)
    }

    // ── Rotation keeps what was open or typed ───────────────────────────────

    @Test
    fun rotation_halfTypedSet_keepsTheSheetTheLoopAndTheWeight() {
        window(PORTRAIT)
        seedSquatYesterday()
        rule.launchApp()
        rule.openLoopFor(SQUAT)
        rule.typeWeight("122.5")
        val typed = hasContentDescription("${str(R.string.log_field_weight)}, 122.5 kg")
        rule.onNode(typed).assertExists()

        rotate(LANDSCAPE)
        rule.assertCountEventually(hasTestTag("editor-side-by-side"), 1)
        rule.assertCountEventually(typed, 1)
        rule.assertTextCountEventually(setHeader(1, 4), 1)

        rotate(PORTRAIT)
        rule.assertCountEventually(hasTestTag("editor-side-by-side"), 0)
        rule.assertCountEventually(typed, 1)
        rule.assertTextCountEventually(setHeader(1, 4), 1)
        assertEquals("a rotation logs nothing", 0, EventDispatcher.model.value!!.lifts.count { it.observed_at > QaHarness.daysAgoAtMidday(0) - 43_200L })
    }

    @Test
    fun rotation_openRunDetail_movesBetweenSheetAndPane() {
        window(PORTRAIT)
        val run = gpsTenK()
        seedRuns(run)
        rule.launchApp()
        HistoryFocus.request(run.entryId, run.observedAt)
        rule.waitForNode(hasTestTag("run-trim"))
        assertEquals(0, rule.countOf(hasTestTag("detail-pane")))

        rotate(LANDSCAPE)
        rule.waitForNode(hasTestTag("run-trim"))
        rule.assertCountEventually(hasTestTag("run-trim") and hasAnyAncestor(hasTestTag("detail-pane")), 1)

        rotate(PORTRAIT)
        rule.waitForNode(hasTestTag("run-trim"))
        rule.assertCountEventually(hasTestTag("detail-pane"), 0)
        assertEquals("no run doubled", 2, EventDispatcher.model.value!!.runs.size)
    }

    @Test
    fun rotation_setupAnswer_keepsTheStepAndTheAnswer() {
        window(PORTRAIT)
        rule.launchAppExpectingSetup()
        GuidedSetupWalk.walk(rule, stopAt = "lift-goal")
        GuidedSetupWalk.bothPath.single { it.name == "lift-goal" }.answer(rule)
        rule.waitForIdle()
        val picked = hasText(str(R.string.setup_liftgoal_strength)) and SemanticsMatcher.expectValue(SemanticsProperties.Selected, true)
        rule.assertCountEventually(picked, 1)

        rotate(LANDSCAPE)
        rule.assertCountEventually(hasTestTag("setup-question-pane"), 1)
        rule.assertCountEventually(hasText(str(R.string.setup_step_lift_q)) and hasAnyAncestor(hasTestTag("setup-question-pane")), 1)
        rule.assertCountEventually(picked, 1)

        rotate(PORTRAIT)
        rule.assertCountEventually(hasTestTag("setup-question-pane"), 0)
        rule.assertCountEventually(picked, 1)
    }

    @Test
    fun rotation_recordingRun_keepsRecordingWithStatsBesideTheMap() {
        AppSettings.setShowRouteMaps(ctx, true)
        window(PORTRAIT)
        QaHarness.seedLog(ctx, listOf(QaHarness.fiveKRunnerProfile()))
        QaHarness.seedRecordingRun()
        lastScenario = launchMain(openIntent(ctx, OpenDest.Track))
        rule.waitForNode(hasText(str(R.string.track_btn_stop)), 30_000)
        val points = RunSession.points.value.size
        assertEquals(0, rule.countOf(hasTestTag("tracker-side-panel")))

        rotate(LANDSCAPE)
        rule.waitForNode(hasText(str(R.string.track_btn_stop)), 30_000)
        rule.assertCountEventually(hasTestTag("tracker-side-panel"), 1)
        assertTrue("still recording", RunSession.tracking.value)
        assertEquals("no fix lost or doubled", points, RunSession.points.value.size)
        assertWhollyOnScreen("Stop", node(hasText(str(R.string.track_btn_stop)) and hasClickAction()))

        rotate(PORTRAIT)
        rule.waitForNode(hasText(str(R.string.track_btn_stop)), 30_000)
        rule.assertCountEventually(hasTestTag("tracker-side-panel"), 0)
        assertTrue(RunSession.tracking.value)
        assertEquals(points, RunSession.points.value.size)
    }

    /** Bug (e37 emulator, 914×411dp landscape, a run day with a plan, route maps
     *  on): the battery notice, "Follow today's plan", the plan card and the start
     *  options pushed the ✕ / Start row below the side panel; it did not scroll, so
     *  a run could not be started
     *  (`~/.cache/ux6/UxDevice3/live/shots/e37-land-tracker-pre-plan.png`). What
     *  is above the control row scrolls; ✕ and Start stay on screen. */
    @Test
    fun phoneLandscapeAfterBars_trackerWithPlan_closeAndStartOnScreen() {
        AppSettings.setShowRouteMaps(ctx, true)
        window(PHONE_LANDSCAPE_AFTER_BARS)
        QaHarness.seedLog(ctx, listOf(QaHarness.tenKRunnerProfile()))
        lastScenario = launchMain(openIntent(ctx, OpenDest.Track))
        val start = hasText(str(R.string.track_btn_start)) and hasClickAction()
        rule.waitForNode(start, 30_000)
        rule.waitForIdle()
        rule.assertCountEventually(hasTestTag("tracker-side-panel"), 1)
        val follow = hasText(str(R.string.track_plan_follow))
        assertEquals("today's plan is offered (a run day)", 1, rule.countOf(follow))
        assertEquals("the battery notice is shown", 1, rule.countOf(hasText(str(R.string.track_battery_caption))))
        assertWhollyOnScreen("Start", node(start))
        assertWhollyOnScreen("✕", node(hasClickAction() and hasContentDescription(str(R.string.track_cd_back))))
        // The notice and the plan scroll into view above the control row.
        rule.onNode(hasText(str(R.string.track_battery_caption))).performScrollTo()
        assertWhollyOnScreen("battery notice", node(hasText(str(R.string.track_battery_caption))))
        rule.onNode(follow).performScrollTo()
        assertWhollyOnScreen("Follow today's plan", node(follow))
        assertWhollyOnScreen("Start after a scroll", node(start))
    }

    /** Stop on a recording run with route maps on, in [qualifiers] at [font];
     *  returns once the Finish review is up. */
    private fun openFinishReview(qualifiers: String, font: Float = 1f) {
        AppSettings.setShowRouteMaps(ctx, true)
        window(qualifiers, font)
        QaHarness.seedLog(ctx, listOf(QaHarness.fiveKRunnerProfile()))
        QaHarness.seedRecordingRun()
        lastScenario = launchMain(openIntent(ctx, OpenDest.Track))
        rule.waitForNode(hasText(str(R.string.track_btn_stop)), 30_000)
        rule.onNode(hasText(str(R.string.track_btn_stop)) and hasClickAction()).performClick()
        rule.waitForNode(hasText(str(R.string.track_finish_title)), 15_000)
        rule.waitForIdle()
    }

    /** The review's figures (distance · moving time · pace), read as one line. */
    private val finishSummary get() = hasContentDescription(" moving · ", substring = true)

    /** Bug (e37 emulator, landscape): the Finish review stretched its effort and
     *  run-type chips and the Save row across the whole 914dp landscape screen.
     *  The route stands beside a column of at most the content width; the pair
     *  centres, and Save stays on screen. */
    @Test
    fun landscapeFinishReview_readableWidthAndSaveOnScreen() {
        openFinishReview(LANDSCAPE)
        val review = node(hasTestTag("tracker-finish-review")).boundsInRoot
        val route = node(hasTestTag("finish-route")).boundsInRoot
        // The review's column: every labelled node inside the sheet, the route aside.
        val parts = rule.onAllNodes(
            hasAnyAncestor(hasTestTag("tracker-finish-review")) and !hasAnyAncestor(hasTestTag("finish-route")) and
                (hasText("", substring = true) or hasClickAction()),
        ).fetchSemanticsNodes().map { it.boundsInRoot }.filter { it.width > 0f }
        val left = parts.minOf { it.left }
        val right = parts.maxOf { it.right }
        assertTrue("review column ${dp(right - left)}dp wide in a ${dp(review.width)}dp window", dp(right - left) <= ContentMaxWidth.value + 1f)
        assertTrue("route $route not beside the review column at ${dp(left)}dp", route.right <= left + 1f)
        val leftGap = route.left - review.left
        val rightGap = review.right - right
        assertTrue("route + review not centred: ${dp(leftGap)}dp | ${dp(rightGap)}dp", kotlin.math.abs(dp(leftGap) - dp(rightGap)) < 2f)
        assertWhollyOnScreen("Save run", node(hasText(str(R.string.track_btn_save_run)) and hasClickAction()))
    }

    /** Bug (e37 emulator, 914×411dp landscape after its system bars): the Finish
     *  review stacked a 160dp route thumbnail over the figures, so distance,
     *  moving time and pace were cut in half above the pinned Save row
     *  (`~/.cache/ux6/UxDevice3/before/e37-land-tracker-finish.png`). The route
     *  now stands beside them: figures, route and Save whole without a scroll. */
    @Test
    fun phoneLandscapeAfterBars_finishReviewShowsTheFiguresWhole() {
        openFinishReview(PHONE_LANDSCAPE_AFTER_BARS)
        assertWhollyOnScreen("the run's figures", node(finishSummary))
        assertWhollyOnScreen("the route", node(hasTestTag("finish-route")))
        assertWhollyOnScreen("Save run", node(hasText(str(R.string.track_btn_save_run)) and hasClickAction()))
    }

    /** The single text layout of the unmerged text node [n]. */
    private fun textLayout(n: SemanticsNode): TextLayoutResult {
        val layout = mutableListOf<TextLayoutResult>()
        n.config[SemanticsActions.GetTextLayoutResult].action?.invoke(layout)
        return layout.single()
    }

    /** The layout dropped text: a height clip, an ellipsis, or an unwrapped line
     *  wider than its box (as in `UxInvariantTest`). */
    private val TextLayoutResult.cutsOffText: Boolean
        get() = didOverflowHeight ||
            (0 until lineCount).any { isLineEllipsized(it) } ||
            (!layoutInput.softWrap && multiParagraph.intrinsics.maxIntrinsicWidth > size.width + 0.5f)

    /** Bug (SmallLand 640×360dp at 200 % font): the review beside the route got
     *  half the width, "Save run" wrapped to two lines cut by its pill and the
     *  figures' units were clipped ("0.37 m", "8:56 /m"). The review stands beside
     *  the route only when its column fits them; otherwise it takes the full width.
     *  A 891×411dp phone at 200 % keeps both whole too, whichever layout it gets. */
    private fun finishReviewWholeAt200(qualifiers: String) {
        openFinishReview(qualifiers, font = 2f)
        val figures = rule.onAllNodes(
            hasText("", substring = true) and hasAnyAncestor(finishSummary) and hasAnyAncestor(hasTestTag("tracker-finish-review")),
            useUnmergedTree = true,
        ).fetchSemanticsNodes().filter { n -> n.config[SemanticsProperties.Text].any { it.text.any(Char::isDigit) } }
        assertEquals("distance, moving time and pace", 3, figures.size)
        figures.forEach { n ->
            val text = n.config[SemanticsProperties.Text].joinToString { it.text }
            val layout = textLayout(n)
            assertTrue("figure \"$text\" is cut: ${layout.size}, ${layout.lineCount} line(s)", !layout.cutsOffText && layout.lineCount == 1)
            assertWhollyOnScreen("figure \"$text\"", n)
        }
        val saveLabel = str(R.string.track_btn_save_run)
        val pill = node(hasText(saveLabel) and hasClickAction())
        assertWhollyOnScreen("Save run", pill)
        val label = rule.onNode(hasText(saveLabel) and !hasClickAction(), useUnmergedTree = true).fetchSemanticsNode()
        val saveLayout = textLayout(label)
        assertEquals("\"$saveLabel\" lines", 1, saveLayout.lineCount)
        assertTrue("\"$saveLabel\" is cut in its pill: ${saveLayout.size}", !saveLayout.cutsOffText)
        val p = pill.boundsInRoot
        val l = label.boundsInRoot
        assertTrue("\"$saveLabel\" at $l runs past its pill $p", l.left >= p.left - 1f && l.right <= p.right + 1f && l.top >= p.top - 1f && l.bottom <= p.bottom + 1f)
    }

    @Test
    fun smallLandscape_font200_finishReviewSaveAndFiguresWhole() = finishReviewWholeAt200(SMALL_LANDSCAPE)

    @Test
    fun phoneLandscape_font200_finishReviewSaveAndFiguresWhole() = finishReviewWholeAt200(LANDSCAPE)

    /** Bug (e37 emulator, landscape, route maps on): Save run in the Finish review
     *  crashed the app (NullPointerException in osmdroid's `Polyline.setPoints`).
     *  Stop moves the live map out of the side-by-side row; osmdroid's default
     *  destroy mode tore its route overlays down when the view left the window,
     *  and the next track update (Save's reset, or a fix after Resume) wrote into
     *  them. The live map now outlives a move between layouts. */
    @Test
    fun landscapeFinishReview_resumeThenSave_keepsTheMapAlive() {
        openFinishReview(LANDSCAPE)
        rule.onNode(hasText(str(R.string.track_action_resume)) and hasClickAction()).performClick()
        rule.waitForNode(hasText(str(R.string.track_btn_stop)), 15_000)
        // A fix after Resume extends the route on the live map.
        val last = RunSession.points.value.last()
        RunSession.add(GpsPoint(last.lat + 0.00027, last.lon, last.observedAt + 10L, 5.0), 1_300L * 1_000_000_000L)
        rule.waitForIdle()
        rule.onNode(hasText(str(R.string.track_btn_stop)) and hasClickAction()).performClick()
        rule.waitForNode(hasText(str(R.string.track_finish_title)), 15_000)
        rule.onNode(hasText(str(R.string.track_btn_save_run)) and hasClickAction()).performClick()
        // The service's stop confirmation (no service runs under Robolectric).
        RunSession.markStopped()
        // The save publishes the run, then resets the session: the map's track
        // collector sees the emptied track.
        rule.waitForState { EventDispatcher.model.value?.runs?.size == 1 && !RunSession.saving.value && RunSession.points.value.isEmpty() }
        rule.waitForIdle()
    }

    /** Bug (e37 emulator, 914×411dp landscape after its system bars): on the
     *  "Your running right now" step the keypad band (a Next-field row over four
     *  3-key rows) was taller than the answers column: the fields had no height
     *  left and the last key row was squashed to a sliver
     *  (`~/.cache/ux6/UxDevice3/before/e37-land-setup-8.png`). Beside the
     *  question the keypad keeps four rows with Next in a fourth column. */
    @Test
    fun phoneLandscapeAfterBars_setupKeypadStep_fieldAndKeysWhole() {
        window(PHONE_LANDSCAPE_AFTER_BARS)
        rule.launchAppExpectingSetup()
        GuidedSetupWalk.walk(rule, stopAt = "run-now")
        rule.waitForNode(hasTestTag("numeric-keypad"))
        rule.waitForIdle()
        assertWhollyOnScreen("weekly running field", node(hasClickAction() and hasContentDescription(str(R.string.setup_runnow_weekly_label), substring = true)))
        val keys = rule.onAllNodes(hasClickAction() and hasAnyAncestor(hasTestTag("numeric-keypad"))).fetchSemanticsNodes()
        assertTrue("keypad keys: ${keys.size}", keys.size >= 12)
        keys.forEach { k ->
            assertWhollyOnScreen("key ${k.boundsInRoot}", k)
            assertTrue("key ${k.boundsInRoot} is ${dp(k.size.height.toFloat())}dp tall", dp(k.size.height.toFloat()) >= 47.5f)
        }
        assertWhollyOnScreen("Next", node(hasText(str(R.string.setup_btn_next)) and hasClickAction()))
        // Typing lands in the field; the keypad's Next moves on to the second one.
        key('2'); key('5')
        rule.onNode(hasContentDescription("${str(R.string.setup_runnow_weekly_label)}, 25", substring = true)).assertExists()
        rule.onNode(hasClickAction() and hasContentDescription(str(R.string.setup_keypad_next_cd, str(R.string.setup_runnow_longest_label)))).performClick()
        rule.waitForIdle()
        assertWhollyOnScreen("longest run field", node(hasClickAction() and hasContentDescription(str(R.string.setup_runnow_longest_label), substring = true)))
    }

    /** Bug (e37 emulator, landscape): the calendar date picker is taller than a
     *  phone's landscape window; its last week rows ran under Cancel/OK, cut
     *  (`~/.cache/ux6/UxDevice3/before/e37-land-datepicker.png`). A short window
     *  opens date pickers as the date field, without the calendar toggle. */
    @Test
    fun phoneLandscapeAfterBars_datePickerShowsNothingCut() {
        window(PHONE_LANDSCAPE_AFTER_BARS)
        rule.launchAppExpectingSetup()
        GuidedSetupWalk.walk(rule, stopAt = "race")
        rule.onNode(hasText(str(R.string.setup_date_pick)) and hasClickAction()).performScrollTo().performClick()
        rule.waitUntil(10_000) { rule.onAllNodes(isDialog()).fetchSemanticsNodes().isNotEmpty() }
        rule.waitForIdle()
        val inDialog = rule.onAllNodes(hasAnyAncestor(isDialog()) and (hasClickAction() or hasSetTextAction())).fetchSemanticsNodes()
        assertTrue("dialog controls: ${inDialog.size}", inDialog.isNotEmpty())
        inDialog.forEach { assertWhollyOnScreen("dialog control ${it.config}", it) }
        assertEquals("a date field to type into", 1, rule.countOf(hasAnyAncestor(isDialog()) and hasSetTextAction()))
    }

    /** UxDevice3 (e37 emulator, landscape): typing the race date opened the
     *  keyboard over the dialog; the platform panned the window, the field at the
     *  top and Cancel/OK under the keyboard
     *  (`~/.cache/ux6/UxDevice3/after/e37-land-datepicker-typing.png`). The dialog
     *  window resizes for the keyboard and its picker scrolls above the buttons
     *  (the layout in that resized window: `DatePickerKeyboardTest`). */
    @Test
    fun phoneLandscapeAfterBars_datePickerResizesForTheKeyboard() {
        window(PHONE_LANDSCAPE_AFTER_BARS)
        rule.launchAppExpectingSetup()
        GuidedSetupWalk.walk(rule, stopAt = "race")
        rule.onNode(hasText(str(R.string.setup_date_pick)) and hasClickAction()).performScrollTo().performClick()
        rule.waitUntil(10_000) { rule.onAllNodes(isDialog()).fetchSemanticsNodes().isNotEmpty() }
        rule.waitForIdle()
        val dialog = checkNotNull(ShadowDialog.getLatestDialog()) { "no dialog window" }
        val adjust = dialog.window!!.attributes.softInputMode and WindowManager.LayoutParams.SOFT_INPUT_MASK_ADJUST
        assertEquals("soft input adjust (RESIZE = ${WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE})", WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE, adjust)
    }

    /** Bug (e37 emulator, landscape with the camera on the right): the guided
     *  setup padded for the status and navigation bars only, so the answers'
     *  chevrons and chips ran under the camera cutout. It now keeps clear of it. */
    @Test
    fun landscapeCutout_setupKeepsClearOfIt() {
        window(LANDSCAPE)
        rule.launchAppExpectingSetup()
        rule.waitForIdle()
        val cutoutPx = with(rule.density) { 48.dp.roundToPx() }
        checkNotNull(lastScenario).onActivity { activity ->
            val insets = WindowInsetsCompat.Builder()
                .setInsets(WindowInsetsCompat.Type.displayCutout(), Insets.of(0, 0, cutoutPx, 0))
                .build()
            ViewCompat.dispatchApplyWindowInsets(activity.window.decorView, insets)
        }
        rule.waitForIdle()
        val right = rule.onAllNodes(hasClickAction()).fetchSemanticsNodes().maxOf { it.boundsInRoot.right }
        assertTrue("setup content reaches ${dp(right)}dp, under the ${dp(cutoutPx.toFloat())}dp cutout at ${dp(screen.right - cutoutPx)}dp", right <= screen.right - cutoutPx + 1f)
    }
}
