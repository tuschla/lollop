package app.milestone.qa

import android.content.Context
import androidx.annotation.StringRes
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.hasScrollToNodeAction
import androidx.compose.ui.test.isSelectable
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.junit4.ComposeTestRule
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.toSize
import androidx.test.core.app.ApplicationProvider
import app.milestone.Event
import app.milestone.EventDispatcher
import app.milestone.R
import app.milestone.ReadinessSignal
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

/**
 * Runs the reusable [UxInvariants] checker over rendered scenario screens: every
 * invariant is an active gate, so a UI change that leaks placeholder text, drops
 * an action label, shrinks a tap target or clips an action at large fonts fails
 * the unit-test run.
 */
@RunWith(RobolectricTestRunner::class)
class UxInvariantTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @Before
    fun setUp() {
        java.util.Locale.setDefault(java.util.Locale.US)
        QaHarness.reset(ctx)
    }

    @After
    fun tearDown() = QaHarness.teardown()

    /** Fail with the offending node paths if any violation of [kinds] is present. */
    private fun assertNoViolations(all: List<UxInvariants.Violation>, vararg kinds: String) {
        val hits = all.filter { it.kind in kinds }
        assertTrue(
            "UX invariant(s) violated:\n" + hits.joinToString("\n"),
            hits.isEmpty(),
        )
    }

    // ── Today ───────────────────────────────────────────────────────────────

    @Test
    fun todayPlan_noGarbageTextOrDuplicateActions() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        val v = UxInvariants.check(rule, rule.density)
        assertNoViolations(v, "forbidden-text", "duplicate-action-in-card")
    }

    @Test
    fun todayPlan_fontScale130_requiredActionsNotClipped() {
        RuntimeEnvironment.setFontScale(1.3f)
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        val v = UxInvariants.check(rule, rule.density, checkClipping = true)
        assertNoViolations(v, "clipped-action")
    }

    @Test
    fun runHistory_noGarbageTextOrDuplicateActions() {
        QaHarness.seedLog(
            ctx,
            listOf(QaHarness.strengthProfile(), QaHarness.loggedRun(QaHarness.daysAgoAtMidday(1))),
        )
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_history))
        rule.scrollToText(QaHarness.expectedRunDistanceLabel(5.0))
        val v = UxInvariants.check(rule, rule.density)
        assertNoViolations(v, "forbidden-text", "duplicate-action-in-card")
    }

    @Test
    fun historyHoldSet_hasNoForbiddenText() {
        // A timed hold renders its seconds; no fabricated RPE or e1RM figure.
        QaHarness.seedLog(
            ctx,
            listOf(QaHarness.calisthenicsProfile(), QaHarness.loggedHold(QaHarness.daysAgoAtMidday(1))),
        )
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_history))
        rule.scrollToText(str(R.string.history_sets_hold, 1, 30))
        val v = UxInvariants.check(rule, rule.density)
        assertNoViolations(v, "forbidden-text")
    }

    @Test
    fun todayPlan_allActionsAreLabeled() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        val v = UxInvariants.check(rule, rule.density)
        assertNoViolations(v, "unlabeled-action")
    }

    @Test
    fun planItemActions_meet48dpTouchTarget() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        val v = UxInvariants.check(rule, rule.density)
        assertNoViolations(v, "small-touch-target")
    }

    // ── Every other key screen: all rules at once ───────────────────────────

    private val allKinds = arrayOf(
        "forbidden-text", "unlabeled-action", "small-touch-target", "duplicate-action-in-card",
    )

    private fun seedAnchoredLifter() = QaHarness.seedLog(
        ctx,
        listOf(
            QaHarness.strengthProfile(),
            QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(1)),
            QaHarness.loggedRun(QaHarness.daysAgoAtMidday(2)),
        ),
    )

    @Test
    fun setLoopEditor_meetsEveryInvariant() {
        seedAnchoredLifter()
        rule.launchApp()
        rule.onAllNodesWithText(strPrefix(R.string.plan_log_set_n_of_n, 1), substring = true)[0].performClick()
        rule.assertCountEventually(hasText(strPrefix(R.string.log_set_header_n_of_n, 1), substring = true), 1)
        assertNoViolations(UxInvariants.check(rule, rule.density), *allKinds)
    }

    @Test
    fun setLoopEditor_primaryInThumbReachAndControlsExplained() {
        seedAnchoredLifter()
        rule.launchApp()
        rule.onAllNodesWithText(strPrefix(R.string.plan_log_set_n_of_n, 1), substring = true)[0].performClick()
        rule.assertCountEventually(hasText(strPrefix(R.string.log_set_header_n_of_n, 1), substring = true), 1)
        // The loop's one primary action sits in the bottom 40% of its window
        // (thumb reach), not top-right.
        val primary = rule.onNode(hasTestTag("editor-primary")).fetchSemanticsNode()
        var root: SemanticsNode = primary
        while (root.parent != null) root = root.parent!!
        val windowHeight = root.boundsInRoot.height
        assertTrue(
            "primary action top ${primary.boundsInRoot.top} of $windowHeight is out of thumb reach",
            primary.boundsInRoot.top >= windowHeight * 0.6f,
        )
        // The effort scale explains itself: a glossary action and plain "reps left" captions.
        val explainRpe = str(R.string.main_explain, str(R.string.main_glossary_rpe_term))
        assertTrue(rule.onAllNodes(hasContentDescription(explainRpe)).fetchSemanticsNodes().isNotEmpty())
        val twoLeft = str(R.string.log_effort_cd, "2")
        assertTrue(rule.onAllNodes(hasContentDescription(twoLeft) and isSelectable()).fetchSemanticsNodes().isNotEmpty())
        // The planned loop names the current exercise (no search field or chip row)
        // and offers one labelled "Change" action with a ≥48dp target.
        rule.onNode(hasTestTag("loop-exercise") and hasText("Back Squat")).assertIsDisplayed()
        val change = rule.onNode(hasClickAction() and hasContentDescription(str(R.string.log_change_exercise_cd, "Back Squat")))
            .fetchSemanticsNode()
        val minPx = with(rule.density) { 48.dp.toPx() }
        assertTrue(
            "Change target ${change.size} is under 48dp",
            change.size.width >= minPx - 0.5f && change.size.height >= minPx - 0.5f,
        )
        assertTrue(rule.onAllNodes(hasSetTextAction()).fetchSemanticsNodes().isEmpty())
    }

    @Test
    fun logChooser_meetsEveryInvariant() {
        seedAnchoredLifter()
        rule.launchApp()
        rule.onNode(hasContentDescription(str(R.string.main_log))).performClick()
        rule.assertCountEventually(hasText(str(R.string.main_log_set_title)), 1)
        assertNoViolations(UxInvariants.check(rule, rule.density), *allKinds)
    }

    /** Lifts AND runs, so every chooser row (both modalities) is offered. */
    private fun hybridProfile(): Event = Event.SetProfile(
        progressionCadence = app.milestone.ProgressionCadence.WeekToWeek,
        liftGoal = app.milestone.LiftGoal.MaxStrength,
        goalDistance = app.milestone.GoalDistance.FiveK,
        concurrentGoal = app.milestone.ConcurrentGoal.Strength,
        weeklySets = 12,
        runningDaysPerWeek = 3,
        runningKmPerWeek = 20.0,
        advanced = false,
        bodyweightKg = 80.0,
        ageYears = 30.0,
    )

    @Test
    fun logChooser_trackRunIsOnTopAndOnScreenAtOpen() {
        // Track run (GPS) is one of the most-used actions, so it is the first row
        // (second only to today's plan fast-path) and visible the moment the sheet
        // opens, with no scroll; Report pain stays offered.
        QaHarness.seedLog(ctx, listOf(hybridProfile()))
        rule.launchApp()
        rule.onNode(hasContentDescription(str(R.string.main_log))).performClick()
        val trackRunRow = hasText(str(R.string.main_track_run_title)) and hasClickAction()
        rule.assertCountEventually(trackRunRow, 1)
        rule.waitForIdle()
        val trackRun = rule.onNode(trackRunRow)
        trackRun.assertIsDisplayed()
        // Unclipped geometry: a row scrolled out of the sheet would still report
        // its real offset here (clipped bounds collapse to zero).
        val track = trackRun.fetchSemanticsNode()
        var root: SemanticsNode = track
        while (root.parent != null) root = root.parent!!
        val trackTop = track.positionInRoot.y
        val trackBottom = trackTop + track.size.height
        assertTrue(
            "Track run bottom $trackBottom is below the window (${root.size.height})",
            trackBottom <= root.size.height,
        )
        // Every other row sits below it; only the "Today's plan" row may precede it.
        // The rarer entries (session review, import, raw signal) fold behind "More".
        val below = listOf(
            R.string.main_log_set_title, R.string.main_log_run_title, R.string.main_morning_checkin_title,
            R.string.main_report_pain, R.string.main_weekly_checkin_title, R.string.log_chooser_more,
        ).map { str(it) }
        for (title in below) {
            val rowTop = rule.onNode(hasText(title) and hasClickAction()).fetchSemanticsNode().positionInRoot.y
            assertTrue(
                "\"$title\" (top $rowTop) sits above Track run (top $trackTop)",
                rowTop > trackTop,
            )
        }
        // The danger-styled safety path is on screen at open too.
        rule.onNode(hasText(str(R.string.main_report_pain)) and hasClickAction()).assertIsDisplayed()
        // The rarer entries stay out of the everyday list until "More" opens them.
        val rarer = listOf(
            R.string.main_session_review_title, R.string.main_import_run_title, R.string.main_advanced_signal_title,
        ).map { str(it) }
        for (title in rarer) {
            assertTrue("\"$title\" shows before More is opened", rule.onAllNodes(hasText(title) and hasClickAction()).fetchSemanticsNodes().isEmpty())
        }
        rule.onNode(hasText(str(R.string.log_chooser_more)) and hasClickAction()).performScrollTo().performClick()
        for (title in rarer) {
            rule.assertCountEventually(hasText(title) and hasClickAction(), 1)
        }
    }

    @Test
    fun liftHistory_meetsEveryInvariant() {
        seedAnchoredLifter()
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_history))
        rule.selectHistoryFilter(str(R.string.main_segment_lifts))
        rule.scrollToText("Back Squat")
        assertNoViolations(UxInvariants.check(rule, rule.density), *allKinds)
    }

    @Test
    fun profile_meetsEveryInvariant() {
        seedAnchoredLifter()
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_profile))
        assertNoViolations(UxInvariants.check(rule, rule.density), *allKinds)
        rule.openPlanAnswers()
        assertNoViolations(UxInvariants.check(rule, rule.density), *allKinds)
    }

    // ── Today hero: engine meta placement + one render per adjustment ───────

    /** An hour before the pinned "now", so a seeded readiness input is today's. */
    private val earlierToday get() = QaHarness.PINNED_NOW_MILLIS / 1000 - 3_600

    @Test
    fun todayHero_engineMetaStaysBehindTheSessionQuestionMark() {
        QaHarness.seedLog(
            ctx,
            listOf(
                QaHarness.strengthProfile(),
                QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(2)),
                QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(1)),
                Event.SubmitCheckin(observedAt = earlierToday, sleepQuality = 4, soreness = 2, mood = 4, fatigue = 2, stress = 2),
            ),
        )
        rule.launchApp()
        val vm = EventDispatcher.model.value!!
        val meta = listOfNotNull(vm.provisional, vm.autoreg_source)
        assertTrue("scenario carries an engine-meta disclosure", meta.isNotEmpty())
        rule.assertTextCountEventually(str(R.string.main_todays_session).uppercase(java.util.Locale.US), 1)
        // Engine meta is never on the face: it sits in the session card's "?" panel.
        meta.forEach { rule.onAllNodesWithText(it.summary).assertCountEquals(0) }
        val title = vm.next_session!!.title
        rule.onNode(hasClickAction() and hasContentDescription(str(R.string.main_details_subject, title))).performClick()
        rule.waitForIdle()
        meta.forEach { rule.assertCountEventually(hasText(it.summary), 1) }
    }

    @Test
    fun todayAdjusted_eachAdjustmentRendersOnce() {
        QaHarness.seedLog(
            ctx,
            listOf(
                QaHarness.strengthProfile(),
                QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(1)),
                // RPE 2 over target: the core downgrades today's session.
                Event.SubmitReadiness(ReadinessSignal.Rpe, 2.0, observedAt = earlierToday),
            ),
        )
        rule.launchApp()
        val ns = EventDispatcher.model.value!!.next_session!!
        assertEquals("adjusted", ns.status)
        rule.onAllNodesWithText(ns.adjustment!!.summary).assertCountEquals(1)
        assertNoViolations(UxInvariants.check(rule, rule.density), "duplicate-summary-on-screen", "forbidden-text")
    }

    // ── Guided setup (fresh install) at large font scales ───────────────────

    @Test
    fun guidedSetup_fontScale130_meetsEveryInvariant() = walkGuidedSetupChecking(1.3f)

    @Test
    fun guidedSetup_fontScale200_meetsEveryInvariant() = walkGuidedSetupChecking(2.0f)

    /**
     * Walk the whole Both path with calisthenics and a race goal (the longest:
     * lifting goal, placement, running goal, race details, which leads, days and
     * weekdays, both levels, running now, then a review) and on EVERY screen check all
     * invariants, that no action spills off the edge, that no option label is cut
     * off, and that a gated screen says what is missing while Next is disabled.
     */
    private fun walkGuidedSetupChecking(fontScale: Float) {
        RuntimeEnvironment.setFontScale(fontScale)
        rule.launchAppExpectingSetup()
        val visited = mutableListOf<String>()
        GuidedSetupWalk.walk(rule) { screen ->
            visited += screen.name
            assertNoViolations(UxInvariants.check(rule, rule.density, checkClipping = true), *allKinds, "clipped-action")
            assertNoClippedOptionText(screen.name)
            screen.gateCaption?.let { caption ->
                rule.onAllNodesWithText(str(caption)).assertCountEquals(1)
                rule.onNode(hasText(str(R.string.setup_btn_next)) and hasClickAction()).assertIsNotEnabled()
            }
        }
        assertEquals(GuidedSetupWalk.bothPath.map { it.name }, visited)
        // The review lists every answer in plain words before anything is saved.
        rule.onAllNodesWithText(str(R.string.setup_step_review_q)).assertCountEquals(1)
        // The days row reads like the Profile row (non-breaking spaces keep
        // "5 days" and "2 lift" whole when it wraps).
        val days = plural(R.plurals.setup_days_count, 5, 5) + " · " + str(R.string.setup_days_split_sub, 2, 3)
        rule.onAllNodes(hasText(days, substring = true)).assertCountEquals(1)
        // Equal lifting and running levels read once.
        rule.onAllNodes(hasText(strPrefix(R.string.setup_review_level_running), substring = true)).assertCountEquals(0)
        // Every new answer is reviewable before Finish.
        val recentRace = str(R.string.setup_recent_race_value, str(R.string.setup_goaldist_5k), "26:15")
        rule.onAllNodes(hasText(recentRace, substring = true)).assertCountEquals(1)
        val volume = str(R.string.setup_review_volume_both, 12, 25, "mi")
        rule.onAllNodes(hasText(volume, substring = true)).assertCountEquals(1)
        // Weekday short names come from java.time, not string resources.
        rule.onAllNodes(hasText("Mon, Tue, Thu, Sat, Sun", substring = true)).assertCountEquals(1)
        rule.onAllNodes(hasText(str(R.string.setup_place_push_full), substring = true)).assertCountEquals(1)
        rule.onAllNodes(hasText(str(R.string.setup_parq_value_clear), substring = true)).assertCountEquals(1)
    }

    @Test
    fun profile_fontScale200_noClippedOptionText() {
        RuntimeEnvironment.setFontScale(2.0f)
        seedAnchoredLifter()
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_profile))
        assertNoClippedOptionText("profile")
        rule.openPlanAnswers()
        assertNoClippedOptionText("profile-plan-answers")
        // Jargon the copy deck forbids bare: the high-load row is plain language now.
        rule.onAllNodes(hasText("parasympathetic", substring = true)).assertCountEquals(0)
    }

    /** An under-18 lifter: the core holds the plan; Today shows the calm youth card. */
    private fun youthProfile(): Event = Event.SetProfile(
        progressionCadence = app.milestone.ProgressionCadence.WeekToWeek,
        liftGoal = app.milestone.LiftGoal.MaxStrength,
        goalDistance = app.milestone.GoalDistance.General,
        concurrentGoal = app.milestone.ConcurrentGoal.Strength,
        weeklySets = 12,
        runningDaysPerWeek = 0,
        runningKmPerWeek = 0.0,
        advanced = false,
        bodyweightKg = 60.0,
        ageYears = 15.0,
    )

    @Test
    // The spot-check phone (w411dp-h660dp). The youth card's "Check age" opens
    // Profile, unfolds the plan answers and opens the Age row: its birthday
    // picker shows whole, above the navigation bar, and the legacy stated age
    // asks for the birthday.
    @Config(qualifiers = "w411dp-h660dp-xxhdpi")
    fun checkAge_opensTheFoldedPlanAnswersAtTheBirthdayPicker() {
        QaHarness.seedLog(ctx, listOf(youthProfile()))
        rule.launchApp()
        rule.onAllNodes(hasText(str(R.string.main_hold_check_age)) and hasClickAction())[0].performClick()
        rule.waitForIdle()
        val navTop = rule.onNodeWithText(str(R.string.main_dest_history)).getBoundsInRoot().top
        val pick = rule.onNode(hasText(str(R.string.setup_birthday_pick)) and hasClickAction())
        pick.assertIsDisplayed()
        assertEquals("the birthday picker is not cut off", pick.getUnclippedBoundsInRoot(), pick.getBoundsInRoot())
        assertTrue("the picker sits above the navigation bar", pick.getUnclippedBoundsInRoot().bottom <= navTop)
        rule.onAllNodes(hasText(str(R.string.setup_birthday_add_prompt))).assertCountEquals(1)
    }

    /** At a 1.3 font scale the under-18 card is Today's first list item: it
     *  scrolls with the page (never pinned over it), with both its actions
     *  reachable, and no DO NOT TRAIN banner anywhere. */
    @Test
    @Config(qualifiers = "w411dp-h660dp-xxhdpi")
    fun youthCard_scrollsWithToday_atLargeFont() {
        RuntimeEnvironment.setFontScale(1.3f)
        QaHarness.seedLog(ctx, listOf(youthProfile()))
        rule.launchApp()
        val title = hasText(str(R.string.main_hold_youth_title).uppercase(java.util.Locale.US))
        rule.waitUntilDisplayed(title)
        rule.onNode(title).assert(hasAnyAncestor(hasScrollToNodeAction()))
        rule.onAllNodes(hasText(str(R.string.main_do_not_train))).assertCountEquals(0)
        for (action in listOf(R.string.main_hold_check_age, R.string.coach_export_action)) {
            rule.onNode(hasText(str(action)) and hasClickAction()).performScrollTo().assertIsDisplayed()
        }
    }

    /**
     * No option label (anything inside a radio-button control: segmented buttons,
     * choice cards, scale cells) is cut off. A label is cut off when its own layout
     * dropped content (lines past maxLines, a height clip, an ellipsis, an unwrapped
     * line wider than its box) or when it spills outside its option's bounds (a
     * segmented button lays its label out beside the check icon, so a label the
     * segment can't hold runs past the border while the text layout itself is
     * fine). Segmented rows must fall back to a list rather than clip "Hypertrophy"
     * at 200 %.
     *
     * Not [TextLayoutResult.hasVisualOverflow]: the semantics layout result is
     * re-laid out at the parent's MAX width, so every content-sized label narrower
     * than its container (a card title, a segment label) reads as width overflow.
     */
    private fun assertNoClippedOptionText(screen: String) {
        val clipped = mutableListOf<String>()
        rule.onAllNodes(isRoot(), useUnmergedTree = true).fetchSemanticsNodes()
            .forEach { collectClippedOptionText(it, option = null, out = clipped) }
        assertTrue("[$screen] option text cut off: $clipped", clipped.isEmpty())
    }

    /** Walks the unmerged tree; [option] is the nearest radio-button ancestor (or self). */
    private fun collectClippedOptionText(node: SemanticsNode, option: SemanticsNode?, out: MutableList<String>) {
        val owner = if (node.config.getOrNull(SemanticsProperties.Role) == Role.RadioButton) node else option
        if (owner != null) {
            node.config.getOrNull(SemanticsActions.GetTextLayoutResult)?.action?.let { getLayout ->
                val layouts = mutableListOf<TextLayoutResult>()
                if (getLayout(layouts)) {
                    val spills = !owner.unclippedBounds.encloses(node.unclippedBounds)
                    layouts.filter { spills || it.cutsOffText }.forEach { out += it.layoutInput.text.text }
                }
            }
        }
        node.children.forEach { collectClippedOptionText(it, owner, out) }
    }

    /** Layout bounds before any ancestor clip (boundsInRoot is already clipped, which
     *  would hide exactly the spill being checked for). */
    private val SemanticsNode.unclippedBounds: Rect get() = Rect(positionInRoot, size.toSize())

    /** [inner] lies within this rect, allowing 1px of layout rounding. */
    private fun Rect.encloses(inner: Rect): Boolean =
        inner.left >= left - 1f && inner.top >= top - 1f && inner.right <= right + 1f && inner.bottom <= bottom + 1f

    /** The layout itself dropped text: lines beyond maxLines or below a height clip
     *  (both in [TextLayoutResult.didOverflowHeight]), an ellipsis, or an unwrapped
     *  line wider than the box it got. */
    private val TextLayoutResult.cutsOffText: Boolean
        get() = didOverflowHeight ||
            (0 until lineCount).any { isLineEllipsized(it) } ||
            (!layoutInput.softWrap && multiParagraph.intrinsics.maxIntrinsicWidth > size.width)
}

/**
 * Drives the guided setup along the Both path with real taps, screen by screen.
 * Shared by the invariant walk and the setup screenshots.
 */
internal object GuidedSetupWalk {
    /** One wizard screen: its caption while Next is disabled (null = never gated)
     *  and the taps that answer it. */
    class Screen(val name: String, @StringRes val gateCaption: Int?, val answer: (ComposeTestRule) -> Unit)

    /** The longest walk: Both + calisthenics (adds the placement screen) + a race
     *  goal (adds race details), so every wizard screen is visited. */
    val bothPath = listOf(
        Screen("train", R.string.setup_need_train) { choose(it, R.string.setup_modality_both_title) },
        Screen("lift-goal", R.string.setup_need_goal) {
            choose(it, R.string.setup_liftgoal_strength)
            choose(it, R.string.setup_modality_calis_title)
        },
        Screen("placement", null) {
            choose(it, R.string.setup_place_push_full)
            choose(it, R.string.setup_place_pull_many)
            choose(it, R.string.setup_equip_bar)
        },
        Screen("run-goal", R.string.setup_need_goal) { choose(it, R.string.setup_goaldist_10k) },
        // A recent 5K in 26:15, typed on the pinned keypad.
        Screen("race", null) { choose(it, R.string.setup_goaldist_5k); key(it, '2'); key(it, '6'); key(it, '1'); key(it, '5') },
        Screen("lead", R.string.setup_need_lead) { choose(it, R.string.setup_bothpriority_liftlean_title) },
        // Five days, then five ticked weekdays (fewer would block Next). The day
        // count is a figure and the weekday short names come from java.time.
        Screen("days", R.string.setup_days_pick_hint) {
            choose(it, "5")
            listOf("Mon", "Tue", "Thu", "Sat", "Sun").forEach { d -> choose(it, d) }
        },
        // Lifting and running levels are separate groups with the same titles.
        Screen("level", R.string.setup_need_level) {
            chooseNth(it, R.string.setup_level_intermediate_title, 0)
            chooseNth(it, R.string.setup_level_intermediate_title, 1)
        },
        Screen("run-now", null) { key(it, '2'); key(it, '5') },
        // The birthday needs an answer (here a date typed in the picker's date
        // field: 12 Mar 1991); bodyweight and sex stay unanswered.
        Screen("about", R.string.setup_about_pick_hint) { pickBirthday(it, "03121991") },
        Screen("health", null) {},
        Screen("review", null) {},
    )

    /** Walk from the first screen; [onArrive] runs on each screen before it is
     *  answered. Stops on arrival at [stopAt] (or at the review, before Finish). */
    fun walk(rule: ComposeTestRule, stopAt: String? = null, onArrive: (Screen) -> Unit = {}) {
        for (screen in bothPath) {
            rule.waitForIdle()
            onArrive(screen)
            if (screen.name == stopAt || screen === bothPath.last()) return
            screen.answer(rule)
            rule.waitForIdle()
            rule.onNode(hasText(str(R.string.setup_btn_next)) and hasClickAction()).performClick()
        }
    }

    private fun choose(rule: ComposeTestRule, text: String) {
        rule.onNode(hasText(text) and hasClickAction()).performScrollTo().performClick()
        rule.waitForIdle()
    }

    private fun choose(rule: ComposeTestRule, @StringRes label: Int) = choose(rule, str(label))

    /** The [n]th of several same-labelled options (e.g. the running level group). */
    private fun chooseNth(rule: ComposeTestRule, @StringRes label: Int, n: Int) {
        rule.onAllNodes(hasText(str(label)) and hasClickAction())[n].performScrollTo().performClick()
        rule.waitForIdle()
    }

    /** Open the birthday picker, type [digits] into its date field (the picker
     *  opens in typed-date mode, US order MMDDYYYY) and confirm. */
    private fun pickBirthday(rule: ComposeTestRule, digits: String) {
        choose(rule, R.string.setup_birthday_pick)
        rule.onAllNodes(hasSetTextAction())[0].performTextInput(digits)
        rule.waitForIdle()
        rule.onNode(hasText(str(R.string.log_ok)) and hasClickAction()).performClick()
        rule.waitForIdle()
    }

    /** A key on a pinned number keypad. */
    private fun key(rule: ComposeTestRule, k: Char) {
        rule.onNode(hasText(k.toString()) and hasClickAction()).performClick()
        rule.waitForIdle()
    }
}
