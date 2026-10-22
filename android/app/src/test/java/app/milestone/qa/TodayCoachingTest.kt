package app.milestone.qa

import android.content.Context
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasScrollToNodeAction
import androidx.compose.ui.test.hasScrollToIndexAction
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeUp
import androidx.compose.ui.unit.dp
import androidx.test.core.app.ApplicationProvider
import app.milestone.AppClock
import app.milestone.Event
import app.milestone.EventDispatcher
import app.milestone.R
import app.milestone.ReadinessSignal
import app.milestone.planDateLabel
import java.util.Locale
import org.junit.After
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * Today coaching surfaces over the REAL core: the do-this numbers on a
 * plan item's face (plates per side, the effort target), the pain hold's
 * "I'm pain-free now" resolution, the session-done moment, moving a session to
 * another day, the calisthenics Easier/Harder rung choice, the minimal THIS WEEK
 * card and the hold card's call + one line + action. Each scenario is seeded
 * through the event log (or dispatched events) and asserted through the
 * semantics tree, then checked against [UxInvariants].
 */
@RunWith(RobolectricTestRunner::class)
class TodayCoachingTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    private val invariantKinds = listOf(
        "forbidden-text", "unlabeled-action", "small-touch-target", "duplicate-action-in-card",
        "duplicate-summary-on-screen",
    )

    @Before
    fun setUp() {
        Locale.setDefault(Locale.US)
        QaHarness.reset(ctx)
    }

    @After
    fun tearDown() = QaHarness.teardown()

    private fun assertNoViolations() {
        val hits = UxInvariants.check(rule, rule.density).filter { it.kind in invariantKinds }
        assertTrue("UX invariant(s) violated:\n" + hits.joinToString("\n"), hits.isEmpty())
    }

    /** Wait until at least one node matches, then return how many do. */
    private fun waitForAny(matcher: SemanticsMatcher, timeoutMs: Long = 10_000): Int {
        runCatching { rule.waitUntil(timeoutMs) { rule.onAllNodes(matcher).fetchSemanticsNodes().isNotEmpty() } }
        return rule.onAllNodes(matcher).fetchSemanticsNodes().size
    }

    private fun tagStartsWith(prefix: String) = SemanticsMatcher("testTag starts with $prefix") {
        it.config.getOrNull(SemanticsProperties.TestTag)?.startsWith(prefix) == true
    }

    // ── plates per side + effort target on a strength item's face ───────────
    @Test
    fun strengthPlan_itemFaceShowsPlatesPerSideAndEffortTarget() {
        QaHarness.seedLog(
            ctx,
            listOf(QaHarness.strengthProfile(), QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(2))),
        )
        rule.launchApp()
        val inSquat = hasAnyAncestor(hasTestTag("plan-item:Back Squat"))
        assertTrue(
            "the squat shows the plates to load on each side",
            waitForAny(inSquat and hasText(strPrefix(R.string.plan_face_per_side), substring = true)) >= 1,
        )
        // One effort statement on the face: the prescription line's "~2 reps left"
        // (worded by the core/pure formatter, not a resource), or for a newcomer
        // the guide's spelled-out line alone ("…stopping with about 2 reps left"),
        // whose subtitle then drops its own "~N reps left".
        val effort = inSquat and hasText("left", substring = true)
        assertTrue("the squat states its effort target once, in plain words", waitForAny(effort) == 1)
        assertTrue(
            "no separate Effort line restates it",
            rule.onAllNodes(inSquat and hasText(strPrefix(R.string.plan_face_effort), substring = true)).fetchSemanticsNodes().isEmpty(),
        )
        assertNoViolations()
    }

    // ── a pain hold resolves from the banner ─────────────────────────────────
    @Test
    fun painHold_painFreeNowLiftsTheHold() {
        val morning = app.milestone.todayEpochDay() * 86_400L + 8 * 3_600L
        QaHarness.seedLog(
            ctx,
            listOf(QaHarness.strengthProfile(), Event.SubmitReadiness(ReadinessSignal.Pain, 1.0, morning)),
        )
        rule.launchApp()
        // The resolve action's label is the core's own copy.
        rule.assertTextCountEventually("I'm pain-free now", 1)
        assertTrue(EventDispatcher.model.value?.train_blocked == true)
        rule.onNodeWithText("I'm pain-free now").performClick()
        rule.waitForIdle()
        // The confirm dialog carries the core's own copy; confirming resolves it.
        rule.onNodeWithText(str(R.string.main_resolve_hold_confirm)).performClick()
        rule.assertTextCountEventually("I'm pain-free now", 0)
        assertTrue(
            "ResolveHold lifted the training hold",
            EventDispatcher.model.value?.train_blocked == false,
        )
    }

    // ── one "I'm better" ends a reported illness AND the profile's answer ────
    @Test
    fun illnessInProfileAndReport_oneImBetterEndsBoth_thenPlanMyTrainingBuildsThePlan() {
        val today = app.milestone.todayEpochDay()
        val ill = (QaHarness.strengthProfile() as Event.SetProfile)
            .copy(health = app.milestone.HealthScreen(acute_illness = true), effectiveFromEpochDay = today)
        QaHarness.seedLog(
            ctx,
            listOf(ill, Event.SubmitReadiness(ReadinessSignal.Illness, 2.0, today * 86_400L + 8 * 3_600L)),
        )
        rule.launchApp()
        // One illness action covers both, not "I've recovered" plus "I'm better".
        rule.assertTextCountEventually("I'm better", 1)
        rule.assertTextCountEventually("I've recovered", 0)
        rule.onNodeWithText("I'm better").performClick()
        rule.waitForIdle()
        rule.onNodeWithText(str(R.string.main_resolve_hold_confirm)).performClick()
        rule.assertTextCountEventually("I'm better", 0)
        val m = EventDispatcher.model.value!!
        assertTrue("no hold left to switch off in Profile", !m.train_blocked)
        assertTrue("the profile answer is off", m.profile?.health?.acute_illness == false)
        assertTrue("the plan resumes", m.next_session != null)

        // Remove plan, then "Plan my training" builds it again.
        EventDispatcher.send(Event.ClearPlan)
        rule.assertTextCountEventually(str(R.string.main_plan_prompt_cta), 1)
        rule.onNodeWithText(str(R.string.main_plan_prompt_cta)).performClick()
        rule.assertTextCountEventually(str(R.string.main_plan_prompt_cta), 0)
        assertTrue("a tap builds the plan", EventDispatcher.model.value?.next_session != null)
    }

    // ── every planned set logged → the session-done card ─────────────────────
    @Test
    fun allSetsLogged_showsSessionDoneCard() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        val session = EventDispatcher.model.value?.next_session
        assertTrue("today's session is planned", session != null && session.items.isNotEmpty())
        // Log every prescribed set as the user would from the loop (same weight per
        // exercise, the top of the rep band), through the real core.
        val now = AppClock.nowSec()
        var id = now * 1_000
        session!!.items.filter { it.exercise.isNotEmpty() }.forEachIndexed { i, item ->
            val reps = maxOf(item.reps_low, item.reps_high, 1)
            repeat(maxOf(item.sets, 1)) { k ->
                EventDispatcher.send(
                    Event.LogSet(item.exercise, 60.0, reps, 8.0, now - 3_000 + i * 300 + k * 60, ++id, holdSec = null),
                )
            }
        }
        assertTrue("the session-done card appears", waitForAny(hasTestTag("session-done")) == 1)
        // The next session below is a preview: no log targets, no empty set slots.
        val next = EventDispatcher.model.value?.next_session
        if (next != null && next.epoch_day != EventDispatcher.model.value?.today_epoch_day) {
            assertTrue(
                "a future session offers nothing to log",
                rule.onAllNodes(hasClickAction() and hasContentDescription(strPrefix(R.string.plan_log_set_n_of_n), substring = true))
                    .fetchSemanticsNodes().isEmpty(),
            )
            assertTrue(
                "a future session shows no empty set slots",
                rule.onAllNodes(tagStartsWith("set-slots:")).fetchSemanticsNodes().isEmpty(),
            )
        }
        assertNoViolations()
    }

    // ── today's session: the current item carries the controls, one "?" ──────
    @Test
    fun todaysSession_onlyTheCurrentItemCarriesControls_andTheCardHasOneQuestionMark() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        rule.assertTextCountEventually(str(R.string.main_todays_session).uppercase(Locale.US), 1)
        val session = EventDispatcher.model.value!!.next_session!!
        val open = session.items.filter { it.exercise.isNotEmpty() && !it.done }
        assertTrue("scenario: several open lift items", open.size >= 2)
        // One filled "do this next" action, one Skip, one slot row: the current item's.
        assertTrue(waitForAny(hasTestTag("plan-action-filled")) == 1)
        rule.onAllNodesWithText(str(R.string.plan_skip_exercise)).assertCountEquals(1)
        assertTrue(rule.onAllNodes(tagStartsWith("set-slots:")).fetchSemanticsNodes().size == 1)
        // Every other open item is still one tap from its loop.
        open.drop(1).forEach { item ->
            assertTrue(
                "${item.exercise} is one tap from its first set",
                rule.onAllNodes(
                    hasClickAction() and hasContentDescription(strPrefix(R.string.plan_log_set_n_of_n, 1), substring = true) and
                        hasAnyAncestor(hasTestTag("plan-item:${item.exercise}")),
                ).fetchSemanticsNodes().size == 1,
            )
        }
        // No per-item "?": the card's single disclosure holds every item's evidence.
        open.forEach { item ->
            assertTrue(
                "no per-item ? on ${item.exercise}",
                rule.onAllNodes(hasContentDescription(str(R.string.main_details_subject, item.exercise))).fetchSemanticsNodes().isEmpty(),
            )
        }
        val details = hasClickAction() and hasContentDescription(str(R.string.main_details_subject, session.title))
        assertTrue("the session card has one ?", waitForAny(details) == 1)
        assertNoViolations()
        rule.onNode(details).performClick()
        rule.waitForIdle()
        // Every item is named inside the one panel: items resting on the same
        // evidence share one block whose header lists their names together.
        open.forEach { item ->
            assertTrue(
                "the panel lists ${item.exercise}'s evidence",
                rule.onAllNodes(hasText(item.exercise, substring = true)).fetchSemanticsNodes().size >= 2,
            )
        }
    }

    // ── "Move session" moves today's session to another day ──────────────────
    @Test
    fun moveSession_movesTodaysSessionOffToday() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        rule.assertTextCountEventually(str(R.string.main_todays_session).uppercase(Locale.US), 1)
        rule.onNode(hasClickAction() and hasContentDescription(str(R.string.main_move_session), substring = true)).performClick()
        rule.waitForIdle()
        val targets = rule.onAllNodes(tagStartsWith("move-target:"))
        assertTrue("the picker offers at least one day", targets.fetchSemanticsNodes().isNotEmpty())
        val target = targets[0].fetchSemanticsNode().config[SemanticsProperties.TestTag].removePrefix("move-target:").toLong()
        targets[0].performClick()
        // Today is now a rest day, folded into the card's overline; no set is due today.
        rule.assertTextCountEventually(str(R.string.main_rest_day_next_overline).uppercase(Locale.US), 1)
        rule.assertTextCountEventually(str(R.string.main_todays_session).uppercase(Locale.US), 0)
        assertTrue(
            "today's plan day reads rest after the move",
            EventDispatcher.model.value?.week_plan?.any {
                it.epoch_day == EventDispatcher.model.value?.today_epoch_day && it.status == "rest"
            } == true,
        )
        // The move confirms with its day, and Undo puts the session back on today.
        rule.assertTextCountEventually(str(R.string.main_moved_to, planDateLabel(target)), 1)
        rule.onNodeWithText(str(R.string.log_undo)).performClick()
        rule.assertTextCountEventually(str(R.string.main_todays_session).uppercase(Locale.US), 1)
    }

    // ── Skip exercise confirms with an Undo that reopens it ──────────────────
    @Test
    fun skipExercise_confirmsWithUndo() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        rule.assertTextCountEventually(str(R.string.main_todays_session).uppercase(Locale.US), 1)
        val first = EventDispatcher.model.value!!.next_session!!.items.first { it.exercise.isNotEmpty() && !it.done }
        rule.onAllNodesWithText(str(R.string.plan_skip_exercise))[0].performClick()
        rule.assertTextCountEventually(str(R.string.plan_skipped), 1)
        rule.assertTextCountEventually(str(R.string.plan_skipped_msg, first.exercise), 1)
        rule.onNodeWithText(str(R.string.log_undo)).performClick()
        rule.assertTextCountEventually(str(R.string.plan_skipped), 0)
        assertTrue(
            "Undo reopened ${first.exercise}",
            EventDispatcher.model.value?.next_session?.items?.first { it.exercise == first.exercise }?.ended_early == false,
        )
    }

    // ── the undo message sits above the Log FAB, never on it ────────────────
    @Test
    fun skipExercise_undoMessageSitsAboveTheLogFab() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        rule.assertTextCountEventually(str(R.string.main_todays_session).uppercase(Locale.US), 1)
        val first = EventDispatcher.model.value!!.next_session!!.items.first { it.exercise.isNotEmpty() && !it.done }
        rule.onAllNodesWithText(str(R.string.plan_skip_exercise))[0].performClick()
        rule.assertTextCountEventually(str(R.string.plan_skipped_msg, first.exercise), 1)
        val shown = rule.onNode(hasText(str(R.string.plan_skipped_msg, first.exercise))).getBoundsInRoot()
        val fab = rule.onNode(logFab).getBoundsInRoot()
        assertTrue("the message ($shown) sits above the Log FAB ($fab)", shown.bottom <= fab.top)
    }

    /** The floating Log button (Today only). */
    private val logFab get() = hasClickAction() and hasContentDescription(str(R.string.main_log))

    // ── the Log FAB lives on Today only ─────────────────────────────────────
    @Test
    fun logFab_isOnTodayOnly() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(7))))
        rule.launchApp()
        rule.assertTextCountEventually(str(R.string.main_todays_session).uppercase(Locale.US), 1)
        assertTrue("one Log FAB on Today", waitForAny(logFab) == 1)
        for (tab in listOf(R.string.main_dest_history, R.string.main_dest_profile)) {
            rule.selectTab(str(tab))
            rule.waitForIdle()
            assertTrue("no Log entry on ${str(tab)}", rule.onAllNodes(logFab).fetchSemanticsNodes().isEmpty())
        }
    }

    // ── Today's last row scrolls fully clear of the floating Log button ────
    @Test
    fun todayLastRow_scrollsClearOfTheLogFab() {
        QaHarness.seedLog(
            ctx,
            listOf(QaHarness.strengthProfile()) + listOf(6, 4, 2).map { QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(it)) },
        )
        rule.launchApp()
        rule.assertTextCountEventually(str(R.string.main_todays_session).uppercase(Locale.US), 1)
        val list = rule.onAllNodes(hasScrollToIndexAction())[0]
        repeat(12) { list.performTouchInput { swipeUp() } }
        rule.waitForIdle()
        val lastBottom = list.fetchSemanticsNode().children.maxOf { it.boundsInRoot.bottom }
        val fabTop = with(rule.density) { rule.onNode(logFab).getBoundsInRoot().top.toPx() }
        assertTrue("the last row ($lastBottom px) ends above the Log FAB ($fabTop px)", lastBottom <= fabTop + 1f)
    }

    // ── the hero's do-this action runs the prescription card's full width ───
    @Test
    fun heroAction_spansThePrescriptionCard() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        rule.assertTextCountEventually(str(R.string.main_todays_session).uppercase(Locale.US), 1)
        val first = EventDispatcher.model.value!!.next_session!!.items.first { it.exercise.isNotEmpty() && !it.done }
        val card = rule.onNode(hasTestTag("plan-item:${first.exercise}")).getBoundsInRoot()
        val action = rule.onNode(hasTestTag("plan-action-filled")).getBoundsInRoot()
        val inset = 14.dp
        assertTrue("start inset ${action.left - card.left}", (action.left - card.left - inset).value in -1f..1f)
        assertTrue("end inset ${card.right - action.right}", (card.right - action.right - inset).value in -1f..1f)
    }

    // ── calisthenics items offer Easier / Harder and Harder changes the rung ─
    @Test
    fun calisthenicsItem_offersEasierHarderAndHarderSwapsTheExercise() {
        QaHarness.seedLog(ctx, listOf(QaHarness.calisthenicsProfile()))
        rule.launchApp()
        // The chip speaks "Harder: <rung>, <target>".
        val harderLead = strPrefix(R.string.plan_calis_chip_cd, str(R.string.plan_calis_harder))
        val harder = hasContentDescription(harderLead, substring = true)
        assertTrue("a calisthenics item offers a harder rung", waitForAny(harder) >= 1)
        assertNoViolations()
        val chip = rule.onAllNodes(harder)[0]
        val desc = chip.fetchSemanticsNode().config.getOrNull(SemanticsProperties.ContentDescription)
            .orEmpty().joinToString()
        val harderName = desc.substringAfter(harderLead).trimStart().substringBefore(", ")
        assertTrue("the chip names the harder rung: $desc", harderName.isNotBlank())
        chip.performClick()
        // SetCalisthenicsRung: the item now prescribes the harder rung.
        assertTrue(
            "the plan item became $harderName",
            waitForAny(hasTestTag("plan-item:$harderName")) >= 1,
        )
    }

    // ── THIS WEEK: one count line, at most one note, no unlabeled bars ───────
    @Test
    fun thisWeek_showsCountLineAndAtMostOneNote_noUnlabeledBars() {
        // A plan started on the Monday two weeks back with nothing logged: last
        // week is finished at 0 done, so the core repeats the plan week (and adds
        // its re-entry hint); missed days this week add the move/carry-on note.
        val today = app.milestone.todayEpochDay()
        val monday = today - (java.time.LocalDate.ofEpochDay(today).dayOfWeek.value - 1)
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), Event.GeneratePlan(monday - 14)))
        rule.launchApp()
        val inWeek = hasAnyAncestor(hasTestTag("this-week"))
        rule.waitUntil(10_000) {
            rule.onAllNodes(hasScrollToNodeAction()).fetchSemanticsNodes().isNotEmpty() &&
                EventDispatcher.model.value?.consistency != null
        }
        rule.onNode(hasScrollToNodeAction()).performScrollToNode(hasTestTag("this-week"))
        rule.waitForIdle()
        assertTrue("the THIS WEEK card renders", waitForAny(hasTestTag("this-week")) == 1)
        val c = EventDispatcher.model.value?.consistency
        assertTrue("planned vs done is shown for this week", c != null && c.this_week_planned > 0)
        assertTrue("scenario: the core has more than one note to choose from", c!!.notes.size >= 3)
        val count = ctx.resources.getQuantityString(
            R.plurals.main_week_done_count,
            c.this_week_planned,
            c.this_week_done,
            c.this_week_planned,
        )
        assertTrue("one plain count line: $count", waitForAny(inWeek and hasText(count)) == 1)
        val notesOnFace = c.notes.filter { n ->
            rule.onAllNodes(inWeek and hasText(n.summary)).fetchSemanticsNodes().isNotEmpty()
        }
        assertTrue(
            "at most one coaching note on the face, got: ${notesOnFace.map { it.summary }}",
            notesOnFace.size <= 1,
        )
        assertTrue(
            "the core's count summary (with the multi-week share) stays behind the ?",
            notesOnFace.none { it === c.notes.first() },
        )
        if (c.repeating_week) {
            assertTrue(
                "the repeating-week explanation stays behind the ?",
                rule.onAllNodes(inWeek and hasText(c.notes[1].summary)).fetchSemanticsNodes().isEmpty(),
            )
        }
        // The old mini bars were one unlabeled drawn row spoken as "Last N plan weeks…"
        // (retired copy, so no resource to resolve).
        assertTrue(
            "no unlabeled multi-week bars",
            rule.onAllNodes(inWeek and hasContentDescription("plan weeks", substring = true))
                .fetchSemanticsNodes().isEmpty(),
        )
        assertNoViolations()
    }

    // ── a non-blocking hold: the call + one line + the action, no engine meta ─
    @Test
    fun headColdHold_cardShowsCallOneLineAndAction_noProvisionalLine() {
        val morning = app.milestone.todayEpochDay() * 86_400L + 8 * 3_600L
        QaHarness.seedLog(
            ctx,
            listOf(QaHarness.strengthProfile(), Event.SubmitReadiness(ReadinessSignal.Illness, 1.0, morning)),
        )
        rule.launchApp()
        // The resolve action's label is the core's own copy.
        rule.assertTextCountEventually("I've recovered", 1)
        val m = EventDispatcher.model.value!!
        val meta = listOfNotNull(m.provisional, m.autoreg_source)
        assertTrue("scenario: the core emits an engine-meta line for this day", meta.isNotEmpty())
        meta.forEach { line ->
            assertTrue(
                "no engine-meta line contradicts the hold: ${line.summary}",
                rule.onAllNodes(hasText(line.summary)).fetchSemanticsNodes().isEmpty(),
            )
        }
        // A core-emitted engine-meta phrase that must not appear while held.
        assertTrue(
            "no \"stay at plan\" line while the hold is active",
            rule.onAllNodes(hasText("stay at plan", substring = true)).fetchSemanticsNodes().isEmpty(),
        )
        // The hold's follow-up safety rows sit behind the "?", never stacked on the face.
        val call = m.today_headline?.summary
        m.adjustments.filter { it.safety_critical && it.summary != call }.forEach { a ->
            assertTrue(
                "follow-up row is behind the ?: ${a.summary}",
                rule.onAllNodes(hasText(a.summary)).fetchSemanticsNodes().isEmpty(),
            )
        }
        assertNoViolations()
    }
}
