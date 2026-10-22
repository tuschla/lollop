package app.milestone.qa

import android.content.Context
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.hasScrollToIndexAction
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performScrollToIndex
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeUp
import androidx.test.core.app.ApplicationProvider
import app.milestone.Core
import app.milestone.Event
import app.milestone.R
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import androidx.compose.ui.test.junit4.createEmptyComposeRule

/**
 * Completion-UI scenario tests: per-set, criteria-aware completion driven through
 * the REAL core over JNI, asserting what the user sees — slot fills, the
 * "Log set k of N" progression, done-collapse, End/Reopen, and the below-target
 * treatment of a warm-up.
 *
 * Qualification: the seeded strength plan is RIR-based (no prescribed
 * load), so a same-movement set qualifies when weight >= 0.9x the heaviest
 * same-movement set that session day. Tests exploit that: equal-weight sets all
 * qualify; a light warm-up next to a heavy set does not.
 */
@RunWith(RobolectricTestRunner::class)
class CompletionScenarioTest {

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

    /** A qualifying Back Squat set logged today. Distinct observed_at per id so the
     *  core attributes each as its own set (equal weight → all qualify). */
    private fun squatSet(weightKg: Double, reps: Int, id: Long) =
        Event.LogSet("Back Squat", weightKg, reps, 8.0, QaHarness.daysAgoAtMidday(0) + id, id)

    /** Every tap target that logs a plan item's [label] set: the current item's
     *  filled pill and each other open item's compact row (both speak
     *  "Log set 1 of 3, <exercise>"). */
    private fun logActions(label: String) = hasClickAction() and hasContentDescription(label, substring = true)

    /** The plan item's "Log set k of N" action label. */
    private fun logSetOf(k: Int, n: Int) = str(R.string.plan_log_set_n_of_n, k, n)

    /** The loop editor's "Set k of N" header. */
    private fun setHeader(k: Int, n: Int) = str(R.string.log_set_header_n_of_n, k, n)

    /** The set slots' spoken "k of N sets done". */
    private fun setsDone(k: Int, n: Int) = str(R.string.plan_sets_done_cd, k, n)

    // ── (a) the current item carries the controls, every other item one tap ──
    @Test
    fun freshPlan_heroShowsLogSetPerItem() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        // 4 open items, each one tap from its first set; only the CURRENT item (the
        // first) shows the filled "Log set 1 of 3", its slots and Skip exercise.
        // (Existence via count is viewport-independent.)
        rule.onAllNodesWithText(str(R.string.main_todays_session).uppercase(Locale.US)).assertCountEquals(1)
        rule.onAllNodes(logActions(logSetOf(1, 3))).assertCountEquals(4)
        rule.onAllNodesWithText(logSetOf(1, 3)).assertCountEquals(1)
        rule.onAllNodesWithText(str(R.string.plan_skip_exercise)).assertCountEquals(1)
        rule.onAllNodesWithContentDescription(setsDone(0, 3)).assertCountEquals(1)
    }

    // ── (b)+(editor loop) log one as-prescribed set through the UI ───────────
    @Test
    fun logOneSetThroughUI_fillsFirstSlotAndAdvances() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        // The squat is the first plan item; open its loop editor.
        rule.onAllNodesWithText(logSetOf(1, 3))[0].performClick()
        rule.waitForIdle()
        rule.onAllNodesWithText(setHeader(1, 3)).assertCountEquals(1)
        // Give the RIR set a real load on the keypad, then log it from the pinned
        // action; the editor STAYS open on the next set (gym-floor loop), and
        // "Done for now" closes it.
        rule.typeWeight("60")
        rule.onNodeWithTag("editor-primary").performClick()
        rule.assertTextCountEventually(setHeader(2, 3), 1)
        rule.onNodeWithText(str(R.string.log_set_done)).performClick()
        // Back on Today: the squat's first slot is filled and its button advanced.
        rule.assertCountEventually(hasContentDescription(setsDone(1, 3)), 1)
        rule.assertTextCountEventually(logSetOf(2, 3), 1)
    }

    // ── (c) all N qualifying sets → item collapses to done ───────────────────
    @Test
    fun allSetsQualifying_itemCollapsesToDone() {
        QaHarness.seedLog(
            ctx,
            listOf(
                QaHarness.strengthProfile(),
                squatSet(100.0, 5, 101),
                squatSet(100.0, 5, 102),
                squatSet(100.0, 5, 103),
            ),
        )
        rule.launchApp()
        // The done squat collapses to a ✓ "3/3" row with no Log button; its sets
        // still count, so there is nothing to reopen. The 3 open items stay one tap
        // from their first set. ("3/3" is a bare figure pair, not a string resource.)
        rule.onAllNodesWithText("3/3").assertCountEquals(1)
        rule.onAllNodesWithText(str(R.string.plan_reopen)).assertCountEquals(0)
        rule.onAllNodes(logActions(logSetOf(1, 3))).assertCountEquals(3)
    }

    // ── (d) a warm-up (bar weight) set does NOT fill a slot ──────────────────
    @Test
    fun warmupSet_belowTarget_doesNotFillSlot() {
        QaHarness.seedLog(
            ctx,
            listOf(
                QaHarness.strengthProfile(),
                squatSet(100.0, 5, 201), // the working set
                squatSet(20.0, 10, 202), // an empty-bar warm-up
            ),
        )
        rule.launchApp()
        // Exactly one slot is filled (the squat's, by the working set); the warm-up
        // is below target so no second slot is filled anywhere, and it is listed
        // under the item marked as not counted.
        rule.onAllNodesWithContentDescription(setsDone(1, 3)).assertCountEquals(1)
        rule.onAllNodesWithContentDescription(setsDone(2, 3)).assertCountEquals(0)
        rule.onAllNodesWithText(str(R.string.plan_set_not_counted), substring = true).assertCountEquals(1)
    }

    // ── the Log chooser's "Today's plan" tile opens the first OPEN exercise ──
    @Test
    fun todaysPlanTile_opensFirstUnfinishedExercise() {
        QaHarness.seedLog(
            ctx,
            listOf(
                QaHarness.strengthProfile(),
                squatSet(100.0, 5, 301),
                squatSet(100.0, 5, 302),
                squatSet(100.0, 5, 303),
            ),
        )
        rule.launchApp()
        rule.onNodeWithContentDescription(str(R.string.main_log)).performClick()
        rule.onNode(hasText(str(R.string.main_todays_plan, ""), substring = true)).performClick()
        // The squat is done, so the tile seeds the NEXT open item, not item 0: a
        // strength goal's hinge, the competition deadlift (STRENGTH-10).
        // The planned loop names the exercise (no search field): Deadlift, not the squat.
        rule.assertCountEventually(hasTestTag("loop-exercise") and hasText("Deadlift"), 1)
        rule.onAllNodes(hasTestTag("loop-exercise") and hasText("Back Squat", substring = true)).assertCountEquals(0)
        rule.onAllNodesWithText(setHeader(1, 3)).assertCountEquals(1)
    }

    // ── an lb profile sees loadable plate-grid loads on the hero ─────────────
    @Test
    fun poundsProfile_heroLoadsSitOnThePlateGrid() {
        QaHarness.seedLog(
            ctx,
            listOf(QaHarness.strengthProfileLb(), QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(2))),
        )
        rule.launchApp()
        val lbFigure = Regex("""(\d+(?:\.\d+)?) lb""")
        val loads = rule.onAllNodes(hasAnyAncestor(hasTestTag("plan-item:Back Squat")) and hasText(" lb", substring = true))
            .fetchSemanticsNodes()
            .flatMap { n -> n.config.getOrNull(SemanticsProperties.Text).orEmpty().map { it.text } }
            // "Last: …" echoes the logged history set (a kg log shown in lb), not a
            // prescribed load, and "Per side: …" lists the plates for that load
            // (a 2.5 lb plate is not a load), so neither is held to the grid.
            .filterNot { it.startsWith("Last") || it.startsWith("Per side") }
            .flatMap { t -> lbFigure.findAll(t).map { it.groupValues[1].toDouble() }.toList() }
        assertTrue("the anchored squat shows a pound load: $loads", loads.isNotEmpty())
        assertTrue("every shown load is a whole 5 lb step: $loads", loads.all { it % 5.0 == 0.0 })
    }

    // ── (e)+(f) Skip exercise → done "Skipped" (no ✓), Reopen → open again ───
    @Test
    fun endExercise_thenReopen() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        // Skip the first item (squat): nothing is logged yet, so ending it reads "Skip".
        rule.onAllNodesWithText(str(R.string.plan_skip_exercise))[0].performClick()
        // It collapses to a settled row marked skipped (0 sets: never "done"); the
        // other 3 stay open.
        rule.assertTextCountEventually(str(R.string.plan_skipped), 1)
        rule.onAllNodesWithText(str(R.string.plan_reopen)).assertCountEquals(1)
        rule.onAllNodes(logActions(logSetOf(1, 3))).assertCountEquals(3)
        // Reopen restores the open item with its first-set action. The done row sits
        // under the three open cards, below the fold, so scroll to the end of the list
        // (past the Log button inset) as a user would before tapping it.
        rule.onAllNodes(hasScrollToIndexAction())[0].performTouchInput { swipeUp() }
        rule.onNodeWithText(str(R.string.plan_reopen)).performClick()
        rule.assertTextCountEventually(str(R.string.plan_reopen), 0)
        // The reopened item returns at the top of the lazy Today list, which the
        // scroll above moved out of composition: scroll back up before counting.
        rule.onAllNodes(hasScrollToIndexAction())[0].performScrollToIndex(0)
        rule.assertCountEventually(logActions(logSetOf(1, 3)), 4)
    }

    // ── (h) a hold set in History shows seconds and no e1RM/@ RPE 0 ─────────
    @Test
    fun holdSetInHistory_showsSecondsNotFabricatedRpeOrE1rm() {
        QaHarness.seedLog(
            ctx,
            listOf(QaHarness.calisthenicsProfile(), QaHarness.loggedHold(QaHarness.daysAgoAtMidday(1))),
        )
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_history))
        // One 30 s hold set: its exercise line reads "1 × 30 s hold".
        val hold = str(R.string.history_sets_hold, 1, 30)
        rule.scrollToText(hold)
        rule.onAllNodesWithText(hold, substring = true).assertCountEquals(1)
        // No fabricated RPE or e1RM figure on a hold.
        rule.onAllNodesWithText("@ RPE 0", substring = true).assertCountEquals(0)
        rule.onAllNodesWithText("0 kg e1RM", substring = true).assertCountEquals(0)
    }
}
