package app.milestone.qa

import android.content.Context
import androidx.annotation.StringRes
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasScrollToIndexAction
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isDialog
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performScrollToIndex
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.performTextClearance
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeUp
import androidx.test.core.app.ApplicationProvider
import app.milestone.AppClock
import app.milestone.ConcurrentGoal
import app.milestone.Event
import app.milestone.EventDispatcher
import app.milestone.GoalDistance
import app.milestone.LiftGoal
import app.milestone.ProgressionCadence
import app.milestone.R
import app.milestone.ReadinessSignal
import app.milestone.RecentRace
import app.milestone.setupPrefs
import java.io.File
import java.util.Locale
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * Interaction tests that try to break the surfaces of the 2026-10-18 UI sweep
 * (`planning/design/ui-sweep-2026-10-18.md`, rulings in `user-decisions.md`
 * 2026-10-18) over the REAL core: the Today-only "+ Log" FAB from every Today
 * state, the StatusCard / banner actions, the run detail sheet's rate / trim /
 * delete / export rows, every RatingScale editor, the calculators, the glossary,
 * the Log set editor and the weekly check-in's Save gate. Every assertion is on
 * what the user sees (the semantics tree) or what the core will replay (the
 * durable event log / the published model).
 */
@RunWith(RobolectricTestRunner::class)
class RedesignFlowsTest {

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

    // ── Shared drivers ──────────────────────────────────────────────────────

    /** The floating "+ Log" button (Today only). */
    private val logFab get() = hasClickAction() and hasContentDescription(str(R.string.main_log))

    private fun row(@StringRes title: Int) = hasText(str(title)) and hasClickAction()

    private val thisMorning get() = app.milestone.todayEpochDay() * 86_400L + 8 * 3_600L

    private fun countOf(matcher: SemanticsMatcher) = rule.onAllNodes(matcher).fetchSemanticsNodes().size

    private fun waitForAny(matcher: SemanticsMatcher, timeoutMs: Long = 10_000) {
        runCatching { rule.waitUntil(timeoutMs) { countOf(matcher) > 0 } }
        assertTrue("expected on screen: $matcher\nOn screen: ${screenTexts()}", countOf(matcher) > 0)
    }

    private fun screenTexts(): List<String> =
        rule.onAllNodes(SemanticsMatcher.keyIsDefined(SemanticsProperties.Text)).fetchSemanticsNodes()
            .flatMap { n -> n.config[SemanticsProperties.Text].map { it.text } }

    /** The event log's variant names, oldest first. */
    private fun loggedVariants(): List<String> = journeyLog().map { it.variant }

    /** Wait for model/log state; on timeout name what the screen shows. */
    private fun waitForState(timeoutMs: Long = 10_000, cond: () -> Boolean) {
        try {
            rule.waitForState(timeoutMs, cond)
        } catch (e: androidx.compose.ui.test.ComposeTimeoutException) {
            val vm = EventDispatcher.model.value
            throw AssertionError(
                "state not reached in ${timeoutMs}ms; log=${loggedVariants()}; blocked=${vm?.train_blocked} " +
                    "hold=${vm?.hold_reason}; on screen: ${screenTexts()}",
                e,
            )
        }
    }

    /** Tap [label] in the newest window: a confirm dialog stacks over the sheet
     *  (itself a dialog window) that may carry a same-worded action. */
    private fun tapDialogButton(@StringRes label: Int) {
        val m = hasText(str(label)) and hasClickAction()
        waitForAny(m)
        val all = rule.onAllNodes(m)
        all[all.fetchSemanticsNodes().size - 1].performClick()
    }

    /** Relaunch the app over the SAME durable log with the clock moved to [epochDay]. */
    private fun relaunchOn(epochDay: Long) {
        val log = File(ctx.filesDir, "event-log.ndjson").readText()
        val today = app.milestone.todayEpochDay()
        QaHarness.teardown()
        QaHarness.reset(ctx)
        File(ctx.filesDir, "event-log.ndjson").writeText(log)
        AppClock.offsetMillis += (epochDay - today) * 86_400_000L
        rule.launchApp()
    }

    /** Scroll Today's list to its true end: the last item, then fling past it. */
    private fun scrollTodayToEnd() {
        val list = rule.onAllNodes(hasScrollToIndexAction())[0]
        val info = list.fetchSemanticsNode().config.getOrNull(SemanticsProperties.CollectionInfo)
        if (info != null && info.rowCount > 0) list.performScrollToIndex(info.rowCount - 1)
        repeat(3) { list.performTouchInput { swipeUp() } }
        rule.waitForIdle()
    }

    /**
     * After scrolling Today to its end, the floating Log button covers nothing:
     * every list row ends above the button's top edge, and no tappable control
     * of the list sits under it.
     */
    private fun assertLastRowClearOfFab(state: String) {
        waitForAny(logFab)
        scrollTodayToEnd()
        val listNode = rule.onAllNodes(hasScrollToIndexAction())[0].fetchSemanticsNode()
        val fab = rule.onNode(logFab).fetchSemanticsNode().boundsInRoot
        val rows = listNode.children.map { it.boundsInRoot }.filter { it.height > 0f }
        assertTrue("[$state] Today has rows", rows.isNotEmpty())
        val lastBottom = rows.maxOf { it.bottom }
        assertTrue(
            "[$state] the last row (bottom $lastBottom px) scrolls clear of the Log FAB (top ${fab.top} px)",
            lastBottom <= fab.top + 1f,
        )
        val covered = rule.onAllNodes(hasClickAction() and hasAnyAncestor(hasScrollToIndexAction()))
            .fetchSemanticsNodes()
            .filter { it.boundsInRoot.height > 0f && it.boundsInRoot.overlapsStrictly(fab) }
            .map { n -> n.config.getOrNull(SemanticsProperties.ContentDescription) ?: n.config.getOrNull(SemanticsProperties.Text) }
        assertTrue("[$state] controls under the Log FAB at the end of Today: $covered", covered.isEmpty())
    }

    private fun Rect.overlapsStrictly(o: Rect): Boolean =
        left < o.right - 1f && o.left < right - 1f && top < o.bottom - 1f && o.top < bottom - 1f

    /** Tap the FAB and wait for the chooser; the always-offered rows are there. */
    private fun openChooserFromFab(state: String) {
        rule.onNode(logFab).performClick()
        waitForAny(row(R.string.main_report_pain))
        for (title in listOf(R.string.main_morning_checkin_title, R.string.main_weekly_checkin_title)) {
            assertEquals("[$state] chooser row ${str(title)}", 1, countOf(row(title)))
        }
        // The chooser, never a stale editor.
        assertEquals("[$state] no editor header", 0, countOf(hasText(strPrefix(R.string.log_set_header_n_of_n, 1), substring = true)))
    }

    private val todaysPlanRow get() = hasText(strPrefix(R.string.main_todays_plan), substring = true) and hasClickAction()

    // ── Profiles ────────────────────────────────────────────────────────────

    private fun runnerProfile(): Event = Event.SetProfile(
        progressionCadence = ProgressionCadence.WeekToWeek,
        liftGoal = LiftGoal.MaxStrength,
        goalDistance = GoalDistance.TenK,
        concurrentGoal = ConcurrentGoal.EndurancePriority,
        weeklySets = 0,
        runningDaysPerWeek = 4,
        runningKmPerWeek = 35.0,
        advanced = true,
        bodyweightKg = 70.0,
        ageYears = 30.0,
        restingHrBpm = 55.0,
        recentRace = RecentRace(distance_km = 5.0, time_sec = 25 * 60, epoch_day = app.milestone.todayEpochDay() - 14),
    )

    /** Lifts AND runs, so every chooser row and every calculator is offered. */
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

    private fun youthProfile(): Event = Event.SetProfile(
        progressionCadence = ProgressionCadence.WeekToWeek,
        liftGoal = LiftGoal.MaxStrength,
        goalDistance = GoalDistance.General,
        concurrentGoal = ConcurrentGoal.Strength,
        weeklySets = 12,
        runningDaysPerWeek = 0,
        runningKmPerWeek = 0.0,
        advanced = false,
        bodyweightKg = 60.0,
        ageYears = 15.0,
    )

    // ═══════════════ Today: the "+ Log" FAB from every state ═══════════════

    @Test
    fun fab_liftDay_opensChooserWithTodaysPlan_andLastRowClears() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(2))))
        rule.launchApp()
        assertLastRowClearOfFab("lift day")
        openChooserFromFab("lift day")
        assertEquals("today's plan fast-path", 1, countOf(todaysPlanRow))
        assertEquals(1, countOf(row(R.string.main_log_set_title)))
    }

    @Test
    fun fab_restDay_opensChooserWithoutAPlanRow() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        val vm = EventDispatcher.model.value!!
        val today = app.milestone.todayEpochDay()
        val rest = vm.week_plan.firstOrNull { it.epoch_day > today && it.items.isEmpty() }
        assertTrue("scenario: a rest day this week: ${vm.week_plan.map { it.epoch_day to it.items.size }}", rest != null)
        relaunchOn(rest!!.epoch_day)
        waitForAny(hasText(str(R.string.main_rest_day_next_overline).uppercase(Locale.US)))
        assertLastRowClearOfFab("rest day")
        openChooserFromFab("rest day")
        assertEquals("no session today, no plan row", 0, countOf(todaysPlanRow))
        assertEquals(1, countOf(row(R.string.main_log_set_title)))
    }

    @Test
    fun fab_runDay_opensChooserWithTrackRun_andLastRowClears() {
        QaHarness.seedLog(ctx, listOf(runnerProfile()))
        rule.launchApp()
        val vm = EventDispatcher.model.value!!
        val today = app.milestone.todayEpochDay()
        val runDay = vm.week_plan.first { d -> d.epoch_day >= today && d.items.any { it.run_target != null } }
        if (runDay.epoch_day != today) relaunchOn(runDay.epoch_day)
        waitForAny(hasTestTag("plan-item:run"))
        assertLastRowClearOfFab("run day")
        openChooserFromFab("run day")
        assertEquals(1, countOf(row(R.string.main_track_run_title)))
        assertEquals("today's run fast-path", 1, countOf(todaysPlanRow))
    }

    @Test
    fun fab_midLoop_reopensOnTheChooser_andThePlanRowResumesAtTheNextSet() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(2))))
        rule.launchApp()
        val first = EventDispatcher.model.value!!.next_session!!.items.first { it.exercise.isNotEmpty() }
        rule.onNode(
            hasClickAction() and hasContentDescription(strPrefix(R.string.plan_log_set_n_of_n, 1), substring = true) and
                hasAnyAncestor(hasTestTag("plan-item:${first.exercise}")),
        ).performScrollTo().performClick()
        waitForAny(hasText(str(R.string.log_set_header_n_of_n, 1, first.sets)))
        rule.onNode(hasTestTag("editor-primary")).performClick()
        waitForAny(hasText(str(R.string.log_set_header_n_of_n, 2, first.sets)))
        // Leave the loop with its X: the session stays in progress.
        rule.onNode(hasContentDescription(str(R.string.main_close)) and hasClickAction()).performClick()
        rule.waitForIdle()
        rule.waitUntil(10_000) { countOf(hasText(str(R.string.log_set_header_n_of_n, 2, first.sets))) == 0 }
        assertLastRowClearOfFab("mid loop")
        openChooserFromFab("mid loop")
        rule.onNode(todaysPlanRow).performScrollTo().performClick()
        waitForAny(hasText(str(R.string.log_set_header_n_of_n, 2, first.sets)))
        assertEquals("the loop continues on ${first.exercise}", 1, countOf(hasTestTag("loop-exercise") and hasText(first.exercise)))
    }

    @Test
    fun fab_painHold_opensChooserButOffersNoTrainingFastPath() {
        QaHarness.seedLog(
            ctx,
            listOf(QaHarness.strengthProfile(), Event.SubmitReadiness(ReadinessSignal.Pain, 1.0, thisMorning)),
        )
        rule.launchApp()
        assertTrue(EventDispatcher.model.value!!.train_blocked)
        waitForAny(hasText(str(R.string.main_do_not_train)))
        assertLastRowClearOfFab("pain hold")
        openChooserFromFab("pain hold")
        assertEquals("no plan fast-path while training is held", 0, countOf(todaysPlanRow))
        assertEquals(0, countOf(hasText(strPrefix(R.string.log_chooser_resume), substring = true)))
    }

    @Test
    fun fab_illnessCard_opensChooser_andLastRowClears() {
        QaHarness.seedLog(
            ctx,
            listOf(QaHarness.strengthProfile(), Event.SubmitReadiness(ReadinessSignal.Illness, 1.0, thisMorning)),
        )
        rule.launchApp()
        waitForAny(hasText("I've recovered"))
        assertLastRowClearOfFab("illness")
        openChooserFromFab("illness")
    }

    @Test
    fun fab_newUserWithoutProfile_opensTheFullChooser() {
        ctx.setupPrefs().edit().putBoolean("onboarding_done", true).commit()
        rule.launchApp()
        waitForAny(row(R.string.main_setup_prompt_cta))
        assertLastRowClearOfFab("new user")
        openChooserFromFab("new user")
        // No profile: every modality is offered, no plan row.
        for (title in listOf(R.string.main_track_run_title, R.string.main_log_set_title, R.string.main_log_run_title)) {
            assertEquals("new user chooser row ${str(title)}", 1, countOf(row(title)))
        }
        assertEquals(0, countOf(todaysPlanRow))
    }

    @Test
    fun fab_underEighteen_opensChooserWithActivity_andNoPlanRow() {
        QaHarness.seedLog(ctx, listOf(youthProfile()))
        rule.launchApp()
        waitForAny(hasText(str(R.string.main_hold_youth_title).uppercase(Locale.US)))
        assertEquals("no DO NOT TRAIN banner under 18", 0, countOf(hasText(str(R.string.main_do_not_train))))
        assertLastRowClearOfFab("under 18")
        openChooserFromFab("under 18")
        // The chooser's own row (Today's activity card has a same-titled action under the sheet).
        assertEquals(1, countOf(hasText(str(R.string.activity_chooser_sub), substring = true) and hasClickAction()))
        assertEquals("no plan fast-path while the plan is paused", 0, countOf(todaysPlanRow))
    }

    // ═══════════════ StatusCard / banner actions ═══════════════════════════

    private fun painReports(): List<kotlinx.serialization.json.JsonObject> =
        journeyLog().filter { it.variant == "SubmitReadiness" && it.payload["signal"].toString().contains("Pain") }.map { it.payload }

    /** A tolerable tendon ache reported through the real triage sheet; its
     *  "Delete report" asks first, Cancel keeps it, Delete removes it, and the
     *  snackbar's Undo puts the same report (area, kind, severity) back. */
    @Test
    fun painReport_deleteAsksFirst_cancelKeepsIt_undoRestoresTheSameReport() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        openChooserFromFab("pain report")
        rule.onNode(row(R.string.main_report_pain)).performClick()
        waitForAny(hasTestTag("pain-severity"))
        rule.onAllNodes(hasText(str(R.string.log_pain_area_left_knee)) and hasClickAction())[0].performScrollTo().performClick()
        // "What does it feel like?" is a tap-row that unfolds its options.
        rule.onNode(hasText(str(R.string.log_pain_feel)) and hasClickAction()).performScrollTo().performClick()
        rule.onAllNodes(hasText(str(R.string.log_pain_char_tendon)))[0].performScrollTo().performClick()
        rule.pickRating("pain-severity", 3)
        rule.onAllNodes(hasText(str(R.string.log_report_pain)) and hasClickAction())[0].performClick()
        waitForState { painReports().size == 1 }
        val sent = painReports().single()["pain"]!!.toString()
        assertTrue(sent, sent.contains("Left knee") && sent.contains("TendonLoadRelated") && sent.contains("\"severity\":3"))
        val vm = EventDispatcher.model.value!!
        assertFalse("a 3/10 tendon ache lets training go on: ${vm.hold_reason}", vm.train_blocked)

        // The card's quiet "Delete report", confirm-gated.
        val delete = hasText(str(R.string.main_remove_report)) and hasClickAction()
        rule.onAllNodes(hasScrollToIndexAction())[0].performScrollToNode(delete)
        rule.onNode(delete).performClick()
        waitForAny(hasText(str(R.string.main_remove_pain_title)))
        tapDialogButton(R.string.main_cancel)
        rule.waitForIdle()
        assertFalse("Cancel deletes nothing", loggedVariants().contains("RemoveReadiness"))
        assertTrue(EventDispatcher.model.value!!.readiness_summary.any { it.signal == "Pain" })

        rule.onNode(delete).performScrollTo().performClick()
        tapDialogButton(R.string.main_delete)
        waitForState { EventDispatcher.model.value!!.readiness_summary.none { it.signal == "Pain" } }
        waitForAny(hasText(str(R.string.main_report_removed)))
        rule.onNode(hasText(str(R.string.log_undo)) and hasClickAction()).performClick()
        waitForState { EventDispatcher.model.value!!.readiness_summary.any { it.signal == "Pain" } }
        val restored = painReports().last()["pain"]!!.toString()
        assertTrue("Undo restores the area, kind and severity: $restored",
            restored.contains("Left knee") && restored.contains("TendonLoadRelated") && restored.contains("\"severity\":3"))
        assertFalse(EventDispatcher.model.value!!.train_blocked)
    }

    /** A bare pain report holds training: the banner's delete asks first, Cancel
     *  keeps the hold, Delete lifts it and leaves the banner gone. */
    @Test
    fun painHoldBanner_deleteReport_cancelKeepsHold_confirmLiftsIt() {
        QaHarness.seedLog(
            ctx,
            listOf(QaHarness.strengthProfile(), Event.SubmitReadiness(ReadinessSignal.Pain, 1.0, thisMorning)),
        )
        rule.launchApp()
        val action = hasText(str(R.string.main_remove_report_action)) and hasClickAction()
        waitForAny(action)
        rule.onNode(action).performClick()
        waitForAny(hasText(str(R.string.main_remove_report_message_hold)))
        tapDialogButton(R.string.main_cancel)
        rule.waitForIdle()
        assertTrue("Cancel keeps the hold", EventDispatcher.model.value!!.train_blocked)
        assertFalse(loggedVariants().contains("RemoveReadiness"))
        assertEquals(1, countOf(hasText(str(R.string.main_do_not_train))))

        rule.onNode(action).performClick()
        tapDialogButton(R.string.main_delete)
        waitForState { !EventDispatcher.model.value!!.train_blocked }
        rule.assertTextCountEventually(str(R.string.main_do_not_train), 0)
        waitForAny(hasText(str(R.string.main_report_removed)))
        // The training fast-path is back on Today's FAB.
        openChooserFromFab("after delete")
        assertEquals(1, countOf(todaysPlanRow))
    }

    /** "I'm pain-free now": Cancel keeps the hold; confirming lifts it with a
     *  DATED ResolveHold that keeps the report in the history it shaped. */
    @Test
    fun painHoldBanner_resolve_cancelKeepsHold_confirmKeepsTheReportInHistory() {
        QaHarness.seedLog(
            ctx,
            listOf(QaHarness.strengthProfile(), Event.SubmitReadiness(ReadinessSignal.Pain, 1.0, thisMorning)),
        )
        rule.launchApp()
        val label = resolveLabels().single()
        rule.onNode(hasText(label) and hasClickAction()).performClick()
        waitForAny(hasText(str(R.string.main_resolve_hold_confirm)))
        tapDialogButton(R.string.main_cancel)
        rule.waitForIdle()
        assertTrue(EventDispatcher.model.value!!.train_blocked)
        assertFalse(loggedVariants().contains("ResolveHold"))

        rule.tapResolveAndConfirm(label)
        waitForState { !EventDispatcher.model.value!!.train_blocked }
        val resolve = journeyLog().single { it.variant == "ResolveHold" }.payload
        assertTrue("the resolve is dated: $resolve", resolve["observed_at"].toString().toLong() > 0L)
        assertEquals("the report stays in the log", 1, painReports().size)
        assertTrue(EventDispatcher.model.value!!.hold_resolutions.isEmpty())
        rule.assertTextCountEventually(str(R.string.main_do_not_train), 0)
    }

    /** A head cold lets training go on: the card's "I've recovered" ends it. */
    @Test
    fun illnessCard_recover_endsTheIllnessAndItsCard() {
        QaHarness.seedLog(
            ctx,
            listOf(QaHarness.strengthProfile(), Event.SubmitReadiness(ReadinessSignal.Illness, 1.0, thisMorning)),
        )
        rule.launchApp()
        val label = resolveLabels().single()
        assertFalse(EventDispatcher.model.value!!.train_blocked)
        rule.onAllNodes(hasScrollToIndexAction())[0].performScrollToNode(hasText(label) and hasClickAction())
        rule.tapResolveAndConfirm(label)
        waitForState { EventDispatcher.model.value!!.hold_resolutions.isEmpty() }
        rule.assertTextCountEventually(label, 0)
        assertTrue(loggedVariants().contains("ResolveHold"))
    }

    // ═══════════════ Run detail sheet ══════════════════════════════════════

    /** A GPS run recorded the way the tracker saves it: [fixes] fixes 3 s and
     *  10 m apart heading north, stamped (observed_at) at the LAST fix. */
    private fun trackedRun(endAt: Long, fixes: Int = 400): Event.LogRunTrack {
        val start = endAt - (fixes - 1) * 3L
        val points = (0 until fixes).map { i ->
            app.milestone.GpsPoint(lat = 47.0 + i * 10.0 / 111_195.0, lon = 8.0, observedAt = start + i * 3L, accuracyM = 5.0)
        }
        return Event.LogRunTrack(points = points, hrPctMax = 0.0, longestRecentKm = 0.0, observedAt = endAt, entryId = endAt * 1000)
    }

    private val yesterdaysRunTitle get() = str(R.string.history_run_weekday, "Sunday")

    private fun openYesterdaysRun(gps: Boolean, title: String = yesterdaysRunTitle) {
        rule.selectTab(str(R.string.main_dest_history))
        rule.scrollToText(title)
        rule.onAllNodes(hasText(title))[0].performClick()
        waitForAny(hasTestTag("run-delete"))
        // The route loads off the main thread; Trim appears once it has.
        if (gps) waitForAny(hasTestTag("run-trim"))
    }

    private fun amendMetas() = journeyLog().filter { it.variant == "AmendRunMeta" }.map { it.payload }

    private fun ratedRun() = EventDispatcher.model.value!!.runs.single()

    @Test
    fun rateSheet_cancelSavesNothing_saveRates_reTapClearsEffortButKeepsType() {
        val at = QaHarness.daysAgoAtMidday(1)
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), QaHarness.loggedRun(at)))
        rule.launchApp()
        openYesterdaysRun(gps = false)

        // Pick 7 + Tempo, then leave with Back: nothing is sent, nothing sticks.
        rule.onNodeWithTagScrolled("run-log-effort").performClick()
        waitForAny(hasTestTag("run-rate-save"))
        rule.pickRating("effort-scale", 7)
        rule.onNode(hasTestTag("run-type-Tempo")).performScrollTo().performClick()
        androidx.test.espresso.Espresso.pressBack()
        rule.waitUntil(10_000) { countOf(hasTestTag("run-rate-save")) == 0 }
        assertTrue("a cancelled rating sends nothing", amendMetas().isEmpty())
        rule.onNodeWithTagScrolled("run-log-effort").assert(hasText(str(R.string.run_rate_title), substring = true))
        rule.onNodeWithTagScrolled("run-log-effort").performClick()
        waitForAny(hasTestTag("run-rate-save"))
        rule.onNode(hasTestTag("effort-scale")).assert(
            SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, str(R.string.rating_unrated)),
        )

        // Save 7 + Tempo: one AmendRunMeta, both fields.
        rule.pickRating("effort-scale", 7)
        rule.onNode(hasTestTag("run-type-Tempo")).performScrollTo().performClick()
        rule.onNode(hasTestTag("run-rate-save")).performScrollTo().performClick()
        waitForState { ratedRun().session_rpe == 7 }
        assertEquals("Tempo", ratedRun().workout_type)
        assertEquals(1, amendMetas().size)
        rule.onNodeWithTagScrolled("run-log-effort")
            .assert(hasText(str(R.string.run_effort_value, 7, str(R.string.history_effort_anchor_7)), substring = true))

        // Reopen: 7 is picked; a re-tap clears it; Save sends a null effort, the type stays.
        rule.onNodeWithTagScrolled("run-log-effort").performClick()
        waitForAny(hasTestTag("run-rate-save"))
        rule.onNode(hasTestTag("effort-scale")).assert(
            SemanticsMatcher.expectValue(
                SemanticsProperties.StateDescription,
                str(R.string.rating_effort_spoken, 7, str(R.string.history_effort_anchor_7)),
            ),
        )
        rule.pickRating("effort-scale", 7)
        rule.onNode(hasTestTag("effort-scale")).assert(
            SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, str(R.string.rating_unrated)),
        )
        rule.onNode(hasTestTag("run-rate-save")).performScrollTo().performClick()
        waitForState { ratedRun().session_rpe == null }
        assertEquals("clearing the effort keeps the type", "Tempo", ratedRun().workout_type)
        rule.onNodeWithTagScrolled("run-log-effort").assert(hasText(str(R.string.run_rate_title), substring = true))
        val last = amendMetas().last()
        assertTrue("the clear is sent as null: $last", last["session_rpe"].toString() == "null")
    }

    @Test
    fun trimSheet_disabledUntilMoved_cancelAndDialogCancelTrimNothing_confirmTrims() {
        val end = QaHarness.daysAgoAtMidday(1)
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), trackedRun(end)))
        rule.launchApp()
        val before = ratedRun().distance_km
        openYesterdaysRun(gps = true)
        // The sheet's Trim; once the confirm is up, its own "Trim" is the newer window's.
        val trimButton = hasText(str(R.string.main_trim_confirm)) and hasClickAction()

        // Open, untouched: Trim is off and says how; Cancel goes back to the detail.
        rule.onNodeWithTagScrolled("run-trim").performClick()
        waitForAny(hasText(str(R.string.trim_hint)))
        rule.onAllNodes(trimButton)[0].performScrollTo().assertIsNotEnabled()
        rule.onNode(hasText(str(R.string.main_cancel)) and hasClickAction()).performScrollTo().performClick()
        waitForAny(hasTestTag("run-trim"))
        assertFalse(loggedVariants().contains("TrimRunTrack"))

        // Move the start thumb (its adjustable action) a fifth in: Trim turns on.
        rule.onNodeWithTagScrolled("run-trim").performClick()
        waitForAny(hasText(str(R.string.trim_hint)))
        val thumbs = rule.onAllNodes(SemanticsMatcher.keyIsDefined(androidx.compose.ui.semantics.SemanticsActions.SetProgress))
        val start = thumbs[0]
        val span = start.fetchSemanticsNode().config[SemanticsProperties.ProgressBarRangeInfo].range.endInclusive
        start.performSemanticsAction(androidx.compose.ui.semantics.SemanticsActions.SetProgress) { it(span * 0.2f) }
        rule.waitForIdle()
        assertEquals("the hint gives way to the removed distances", 0, countOf(hasText(str(R.string.trim_hint))))
        // Squeezing the window shut (start dragged onto the end) keeps < 2 fixes: no Trim.
        start.performSemanticsAction(androidx.compose.ui.semantics.SemanticsActions.SetProgress) { it(span * 2f) }
        rule.waitForIdle()
        rule.onAllNodes(trimButton)[0].performScrollTo().assertIsNotEnabled()
        start.performSemanticsAction(androidx.compose.ui.semantics.SemanticsActions.SetProgress) { it(span * 0.2f) }
        rule.waitForIdle()
        rule.onAllNodes(trimButton)[0].performScrollTo().assertIsEnabled().performClick()
        waitForAny(hasText(strPrefix(R.string.main_trim_confirm_title), substring = true))
        tapDialogButton(R.string.main_cancel)
        rule.waitForIdle()
        assertFalse("the confirm's Cancel trims nothing", loggedVariants().contains("TrimRunTrack"))

        rule.onAllNodes(trimButton)[0].performScrollTo().performClick()
        waitForAny(hasText(strPrefix(R.string.main_trim_confirm_title), substring = true))
        tapDialogButton(R.string.main_trim_confirm)
        waitForState { loggedVariants().contains("TrimRunTrack") }
        waitForState { ratedRun().distance_km < before }
        val after = ratedRun().distance_km
        assertTrue("a fifth off the start keeps ~80% ($before → $after km)", after in before * 0.7..before * 0.9)
        // Back on the (refreshed) detail.
        waitForAny(hasTestTag("run-delete"))
    }

    @Test
    fun deleteRun_cancelKeepsIt_confirmThenUndoRestoresItsRatingAndType() {
        val at = QaHarness.daysAgoAtMidday(1)
        QaHarness.seedLog(
            ctx,
            listOf(
                QaHarness.strengthProfile(),
                Event.LogRun(5.0, 27.5, 0.0, 0.0, observedAt = at, entryId = at * 1000),
                Event.AmendRunMeta(at * 1000, at, 6, app.milestone.WorkoutType.LongRun),
            ),
        )
        rule.launchApp()
        openYesterdaysRun(gps = false, title = str(R.string.history_run_long))
        rule.onNodeWithTagScrolled("run-delete").performClick()
        waitForAny(hasText(str(R.string.run_delete_message)))
        tapDialogButton(R.string.main_cancel)
        rule.waitForIdle()
        assertFalse("Cancel deletes nothing", loggedVariants().contains("DeleteEntry"))
        waitForAny(hasTestTag("run-delete"))

        rule.onNodeWithTagScrolled("run-delete").performClick()
        tapDialogButton(R.string.main_delete)
        waitForState { EventDispatcher.model.value!!.runs.isEmpty() }
        waitForAny(hasText(str(R.string.history_run_deleted)))
        rule.onNode(hasText(str(R.string.log_undo)) and hasClickAction()).performClick()
        waitForState { EventDispatcher.model.value!!.runs.isNotEmpty() }
        val back = ratedRun()
        assertEquals(at * 1000, back.entry_id)
        assertEquals("Undo keeps the rating", 6, back.session_rpe)
        assertEquals("Undo keeps the type", "LongRun", back.workout_type)
        assertEquals(5.0, back.distance_km, 1e-6)
    }

    /** Export GPX names the file by the run's START (spec §2.7), not by its
     *  logged-at stamp, which the tracker sets to the last fix. */
    @Test
    @org.junit.Ignore("BUG: Export GPX names the file by observed_at (the run's LAST fix), not its start (ui-sweep §2.7)")
    fun exportGpx_sharesAFileNamedByTheRunsStart() {
        val end = QaHarness.daysAgoAtMidday(1)
        val run = trackedRun(end, fixes = 1200)
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), run))
        rule.launchApp()
        openYesterdaysRun(gps = true)
        val application = ctx as android.app.Application
        org.robolectric.Shadows.shadowOf(application).clearNextStartedActivities()
        rule.onNodeWithTagScrolled("run-export").performClick()
        var chooser: android.content.Intent? = null
        waitForState { org.robolectric.Shadows.shadowOf(application).peekNextStartedActivity()?.also { chooser = it } != null }
        @Suppress("DEPRECATION")
        val send = chooser!!.getParcelableExtra<android.content.Intent>(android.content.Intent.EXTRA_INTENT)!!
        @Suppress("DEPRECATION")
        val uri = send.getParcelableExtra<android.net.Uri>(android.content.Intent.EXTRA_STREAM)!!
        val startSec = run.points.first().observedAt
        assertEquals(
            "the shared file names the run's start (${run.points.size * 3 / 60} min before its end)",
            app.milestone.gpxFileName(startSec),
            uri.lastPathSegment,
        )
    }

    /** An untagged run is titled by "the local weekday it started"
     *  (HistoryRuns.runTitle): a tracked run from Sunday 23:40 to Monday 00:20
     *  is a Sunday run, though the tracker stamps it at its last fix. */
    @Test
    @org.junit.Ignore("BUG: an untagged tracked run is titled by the weekday of its LAST fix (observed_at), not the day it started")
    fun trackedRunAcrossMidnight_isTitledByTheDayItStarted() {
        val midnight = app.milestone.todayEpochDay() * 86_400L
        val run = trackedRun(midnight + 20 * 60, fixes = 801)
        assertTrue("scenario: starts before midnight", run.points.first().observedAt < midnight)
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), run))
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_history))
        val either = hasText(str(R.string.history_run_weekday, "Monday")) or hasText(yesterdaysRunTitle)
        rule.onAllNodes(androidx.compose.ui.test.hasScrollToNodeAction())[0].performScrollToNode(either)
        assertEquals("titled by its start day: ${screenTexts().filter { it.endsWith(" run") }}", 1, countOf(hasText(yesterdaysRunTitle)))
        assertEquals(0, countOf(hasText(str(R.string.history_run_weekday, "Monday"))))
    }

    /** The detail's date line is "the run's local start date and time"
     *  (RunDetailSheet.runStartCaption): a tracked 11:00–12:00 run reads 11:00. */
    @Test
    @org.junit.Ignore("BUG: run detail start caption shows the last-fix time (observed_at), not the run's start")
    fun trackedRunDetail_captionStatesItsStartTime() {
        val end = QaHarness.daysAgoAtMidday(1)
        val run = trackedRun(end, fixes = 1201)
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), run))
        rule.launchApp()
        openYesterdaysRun(gps = true)
        val caption = screenTexts().firstOrNull { it.contains(" · ") && (it.contains("AM") || it.contains("PM")) }
        assertTrue("the start caption is shown: ${screenTexts()}", caption != null)
        assertTrue("the caption states the start (11:00 AM), not the last fix (12:00 PM): $caption", caption!!.contains("11:00"))
    }

    private fun androidx.compose.ui.test.junit4.ComposeTestRule.onNodeWithTagScrolled(tag: String) =
        onNode(hasTestTag(tag)).performScrollTo()

    // ═══════════════ RatingScale: tap, re-tap, clear, set progress ═════════

    private val setProgressKey = androidx.compose.ui.semantics.SemanticsActions.SetProgress

    /** The spoken state of the RatingScale track tagged [tag]. */
    private fun trackState(tag: String): String? =
        rule.onNode(hasTestTag(tag)).fetchSemanticsNode().config.getOrNull(SemanticsProperties.StateDescription)

    /** TalkBack's swipe on the one adjustable track: set its progress to [index]. */
    private fun setTrackProgress(tag: String, index: Float) {
        rule.onNode(hasTestTag(tag)).performScrollTo().performSemanticsAction(setProgressKey) { it(index) }
        rule.waitForIdle()
    }

    private fun effortSpoken(v: Int) = str(
        R.string.rating_effort_spoken, v,
        str(
            listOf(
                R.string.history_effort_anchor_0, R.string.history_effort_anchor_1, R.string.history_effort_anchor_2,
                R.string.history_effort_anchor_3, R.string.history_effort_anchor_4, R.string.history_effort_anchor_5,
                R.string.history_effort_anchor_6, R.string.history_effort_anchor_7, R.string.history_effort_anchor_8,
                R.string.history_effort_anchor_9, R.string.history_effort_anchor_10,
            )[v],
        ),
    )

    /** Log run's effort: tap picks, re-tap clears, and the adjustable action
     *  moves it, clamping out-of-range targets to the ends (never a crash, never
     *  a value outside 0–10), while the range it reports follows the pick. */
    @Test
    fun logRunEffort_tapReTapClear_andSetProgressClampsToTheEnds() {
        QaHarness.seedLog(ctx, listOf(hybridProfile()))
        rule.launchApp()
        openChooserFromFab("log run")
        rule.onNode(row(R.string.main_log_run_title)).performScrollTo().performClick()
        waitForAny(hasTestTag("effort-scale"))
        assertEquals(str(R.string.rating_unrated), trackState("effort-scale"))
        rule.pickRating("effort-scale", 5)
        assertEquals(effortSpoken(5), trackState("effort-scale"))
        assertEquals(1, countOf(hasText(str(R.string.history_effort_selected, 5, str(R.string.history_effort_anchor_5)))))
        rule.pickRating("effort-scale", 5)
        assertEquals("a re-tap clears", str(R.string.rating_unrated), trackState("effort-scale"))
        assertEquals("and the word slot empties", 0, countOf(hasText(str(R.string.history_effort_selected, 5, str(R.string.history_effort_anchor_5)))))
        setTrackProgress("effort-scale", 8f)
        assertEquals(effortSpoken(8), trackState("effort-scale"))
        val info = rule.onNode(hasTestTag("effort-scale")).fetchSemanticsNode().config[SemanticsProperties.ProgressBarRangeInfo]
        assertEquals(8f, info.current, 0f)
        assertEquals(10f, info.range.endInclusive, 0f)
        setTrackProgress("effort-scale", 42f)
        assertEquals("above the range clamps to 10", effortSpoken(10), trackState("effort-scale"))
        setTrackProgress("effort-scale", -3f)
        assertEquals("below the range clamps to 0", effortSpoken(0), trackState("effort-scale"))
        // A tap after an accessibility pick still clears it.
        rule.pickRating("effort-scale", 0)
        assertEquals(str(R.string.rating_unrated), trackState("effort-scale"))
    }

    /** Morning check-in: the scales are NOT clearable (a re-tap keeps the
     *  answer), a reversed scale's leftmost step is its worst value, an
     *  accessibility pick alone enables Save, and only the answered rows are sent. */
    @Test
    fun morningCheckin_reTapKeeps_reversedScaleLeftIsWorst_onlyAnswersAreSent() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        openChooserFromFab("check-in")
        rule.onNode(row(R.string.main_morning_checkin_title)).performScrollTo().performClick()
        waitForAny(hasTestTag("checkin-sleep"))
        rule.onNode(hasTestTag("editor-primary")).assertIsNotEnabled()
        // Soreness runs 5→1: the accessibility step 0 is the leftmost value, 5.
        setTrackProgress("checkin-soreness", 0f)
        rule.onNode(hasTestTag("editor-primary")).assertIsEnabled()
        rule.pickRating("checkin-sleep", 3)
        rule.pickRating("checkin-sleep", 3)
        val sleepState = trackState("checkin-sleep")
        assertTrue("a re-tap keeps sleep 3: $sleepState", sleepState != null && sleepState != str(R.string.rating_unrated) && sleepState.startsWith("3"))
        rule.onNode(hasTestTag("editor-primary")).performClick()
        waitForState { loggedVariants().contains("SubmitCheckin") }
        val sent = journeyLog().single { it.variant == "SubmitCheckin" }.payload
        assertEquals("soreness 5 (worst)", "5", sent["soreness"].toString())
        assertEquals("3", sent["sleep_quality"].toString())
        for (unanswered in listOf("mood", "fatigue", "stress")) {
            val v = sent[unanswered]?.toString()
            assertTrue("unanswered $unanswered is not sent as a value: $v", v == null || v == "null")
        }
        assertFalse("no illness answer, no illness report", loggedVariants().contains("SubmitReadiness"))
    }

    /** Pain severity is optional: picked then re-tapped it goes back to
     *  unrated and the report carries no severity at all. */
    @Test
    fun painSeverity_reTapClears_andTheReportIsSentUnrated() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        openChooserFromFab("pain")
        rule.onNode(row(R.string.main_report_pain)).performClick()
        waitForAny(hasTestTag("pain-severity"))
        rule.pickRating("pain-severity", 6)
        assertEquals(str(R.string.rating_pain_spoken, 6), trackState("pain-severity"))
        rule.pickRating("pain-severity", 6)
        assertEquals(str(R.string.rating_unrated), trackState("pain-severity"))
        setTrackProgress("pain-severity", 2f)
        assertEquals(str(R.string.rating_pain_spoken, 2), trackState("pain-severity"))
        rule.pickRating("pain-severity", 2)
        assertEquals(str(R.string.rating_unrated), trackState("pain-severity"))
        rule.onAllNodes(hasText(str(R.string.log_report_pain)) and hasClickAction())[0].performClick()
        waitForState { painReports().isNotEmpty() }
        val pain = painReports().single()["pain"]!!.toString()
        assertFalse("no severity on the wire: $pain", pain.contains("severity"))
        // An undescribed report is the conservative stop.
        waitForState { EventDispatcher.model.value!!.train_blocked }
    }

    // ═══════════════ Weekly check-in: the Save gate ════════════════════════

    private fun openWeekly() {
        openChooserFromFab("weekly")
        rule.onNode(row(R.string.main_weekly_checkin_title)).performScrollTo().performClick()
        waitForAny(hasTestTag("weekly-failed"))
    }

    private val nothingToReport get() = hasText(str(R.string.log_weekly_nothing)) and hasClickAction()

    private fun weeklySubmits() = journeyLog().filter { it.variant == "SubmitWeeklyCheckin" }.map { it.toString() }

    @Test
    fun weeklyCheckin_untouchedCantSave_oneAccessibilityAnswerCan_andOnlyItIsSent() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        openWeekly()
        rule.onNode(hasTestTag("editor-primary")).assertIsNotEnabled()
        assertEquals(1, countOf(nothingToReport))
        setTrackProgress("weekly-failed", 2f)
        rule.onNode(hasTestTag("editor-primary")).assertIsEnabled()
        assertEquals("the all-clear goes once something is answered", 0, countOf(nothingToReport))
        rule.onNode(hasTestTag("editor-primary")).performClick()
        waitForState { weeklySubmits().isNotEmpty() }
        val sent = weeklySubmits().single()
        assertTrue(sent, sent.contains("\"failed_key_sessions\":2"))
        assertFalse("the unanswered count is not sent as 0: $sent", sent.contains("\"rpe_load_gap_sessions\":0"))
    }

    @Test
    fun weeklyCheckin_invalidBarSpeedBlocksSave_untilCleared() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        openWeekly()
        rule.onNode(hasText(str(R.string.log_weekly_more_details)) and hasClickAction()).performScrollTo().performClick()
        rule.onNode(hasClickAction() and hasContentDescription(str(R.string.log_weekly_velocity_drop), substring = true))
            .performScrollTo().performClick()
        keypadKey('5')
        rule.onNode(hasTestTag("editor-primary")).assertIsNotEnabled()
        assertEquals("the range is named", 1, countOf(hasText(str(R.string.ui_keypad_range_hint, "0.00", "0.30", str(R.string.log_unit_m_per_s)))))
        rule.onNode(hasClickAction() and hasContentDescription(str(R.string.ui_cd_delete_digit)) and hasAnyAncestor(hasTestTag("numeric-keypad")))
            .performScrollTo().performClick()
        rule.waitForIdle()
        rule.onNode(hasTestTag("editor-primary")).assertIsEnabled()
        rule.onNode(hasTestTag("editor-primary")).performClick()
        waitForState { weeklySubmits().isNotEmpty() }
        assertFalse("an emptied bar speed is not sent", weeklySubmits().single().contains("weekly_velocity_drop_ms\":0"))
    }

    @Test
    fun weeklyCheckin_nothingToReportDoubleTap_sendsOneCheckin() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        openWeekly()
        // Pinned under the form (not in its scroll).
        val n = rule.onNode(nothingToReport)
        n.performClick()
        runCatching { n.performClick() }
        waitForState { weeklySubmits().isNotEmpty() }
        rule.waitForIdle()
        assertEquals(1, weeklySubmits().size)
    }

    /** Ticking a sign and unticking it answers nothing: the form is back to its
     *  untouched state, so Save must stay off (an untouched Save would send a
     *  fabricated week over the stored one, BUGS.md 2026-09-11 #1). */
    @Test
    @org.junit.Ignore("BUG: weekly check-in Save stays enabled after a sign is ticked then unticked (nothing answered)")
    fun weeklyCheckin_tickThenUntick_isStillUnanswered() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        openWeekly()
        val sleep = hasText(str(R.string.log_weekly_sign_sleep)) and hasClickAction()
        rule.onNode(sleep).performScrollTo().performClick()
        rule.onNode(hasTestTag("editor-primary")).assertIsEnabled()
        rule.onNode(sleep).performScrollTo().performClick()
        rule.waitForIdle()
        rule.onNode(hasTestTag("editor-primary")).assertIsNotEnabled()
        assertEquals("the explicit all-clear is offered again", 1, countOf(nothingToReport))
    }

    private fun keypadKey(k: Char) {
        rule.onNode(
            hasClickAction() and hasText("$k") and hasAnyAncestor(hasTestTag("numeric-keypad")),
        ).performScrollTo().performClick()
        rule.waitForIdle()
    }

    // ═══════════════ Calculators + glossary ════════════════════════════════

    private val calcResult = hasTestTag("calc-result")

    private fun openHrZones(seedQuery: Boolean = true) {
        QaHarness.seedLog(
            ctx,
            listOf(hybridProfile()) + if (seedQuery) listOf(Event.ComputeHrZones(30.0, null)) else emptyList(),
        )
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_profile))
        val calcRow = hasText(str(R.string.main_tool_hr_title)) and hasClickAction()
        rule.onAllNodes(androidx.compose.ui.test.hasScrollToNodeAction())[0].performScrollToNode(calcRow)
        rule.onAllNodes(calcRow)[0].performClick()
        waitForAny(hasText(str(R.string.log_btn_compute_hr)) and hasClickAction())
    }

    private fun headline(): String? =
        rule.onAllNodes(hasTestTag("calc-headline"), useUnmergedTree = true).fetchSemanticsNodes()
            .singleOrNull()?.config?.getOrNull(SemanticsProperties.Text)?.joinToString { it.text }

    private fun typeAge(digits: String) {
        rule.onNode(hasClickAction() and hasContentDescription(str(R.string.log_field_age), substring = true))
            .performScrollTo().performClick()
        waitForAny(hasTestTag("numeric-keypad"))
        digits.forEach { keypadKey(it) }
    }

    private val computeHr get() = hasText(str(R.string.log_btn_compute_hr)) and hasClickAction()

    @Test
    fun hrZones_clearResult_reRun_andANewAgeReplacesTheOneCard() {
        openHrZones()
        waitForAny(calcResult)
        assertEquals("187", headline())
        rule.onNode(hasClickAction() and hasContentDescription(str(R.string.main_calc_clear_subject, str(R.string.main_calc_hr_overline))))
            .performScrollTo().performClick()
        rule.assertCountEventually(calcResult, 0)
        assertTrue(loggedVariants().contains("ClearHrZones"))
        // The form stays, seeded: computing again brings the same answer back.
        rule.onNode(computeHr).performScrollTo().performClick()
        waitForAny(calcResult)
        assertEquals("187", headline())
        // A new age replaces the result in place: one card, Tanaka 208 − 0.7 × 40.
        typeAge("40")
        rule.onNode(computeHr).performScrollTo().performClick()
        waitForState { headline() == "180" }
        assertEquals(1, countOf(calcResult))
        assertEquals(0, countOf(hasTestTag("numeric-keypad")))
    }

    @Test
    fun hrZones_outOfRangeOrEmptyAge_cantCompute_andSaysTheRange() {
        openHrZones(seedQuery = false)
        assertEquals(0, countOf(calcResult))
        val range = hasText(str(R.string.ui_keypad_range_hint, "5", "100", str(R.string.log_unit_years)))
        for (bad in listOf("4", "101")) {
            typeAge(bad)
            rule.onNode(computeHr).performScrollTo().assertIsNotEnabled()
            assertEquals("age $bad names the range", 1, countOf(range))
        }
        // Emptied: no "0", no compute.
        repeat(4) {
            rule.onNode(hasClickAction() and hasContentDescription(str(R.string.ui_cd_delete_digit)) and hasAnyAncestor(hasTestTag("numeric-keypad")))
                .performScrollTo().performClick()
        }
        rule.waitForIdle()
        rule.onNode(computeHr).performScrollTo().assertIsNotEnabled()
        typeAge("45")
        rule.onNode(computeHr).performScrollTo().assertIsEnabled().performClick()
        waitForState { headline() == "177" }
        assertFalse("no invalid age was ever sent", journeyLog().any { it.variant == "ComputeHrZones" && it.payload["age_years"].toString().let { a -> a == "4.0" || a == "101.0" || a == "0.0" } })
    }

    @Test
    fun glossary_noMatchThenCleared_andBackClosesOnlyTheGlossary() {
        openHrZones()
        waitForAny(calcResult)
        rule.onNode(hasContentDescription(str(R.string.main_details_subject, str(R.string.main_calc_hr_overline)))).performScrollTo().performClick()
        val hrmax = str(R.string.main_glossary_hrmax_term)
        rule.onNode(hasContentDescription(str(R.string.main_explain, hrmax)) and hasClickAction()).performScrollTo().performClick()
        waitForAny(hasTestTag("glossary-search"))
        val search = rule.onNode(hasTestTag("glossary-search"))
        search.performTextInput("zzqx")
        rule.waitForIdle()
        assertEquals(1, countOf(hasText(str(R.string.main_glossary_no_match, "zzqx"))))
        assertEquals("no term rows under a no-match", 0, countOf(hasText(hrmax) and hasAnyAncestor(hasTestTag("glossary-list"))))
        search.performTextClearance()
        rule.waitForIdle()
        assertEquals(0, countOf(hasText(str(R.string.main_glossary_no_match, "zzqx"))))
        rule.onNode(hasTestTag("glossary-list")).performScrollToNode(hasText(hrmax))
        // Back closes the glossary and leaves the calculator (and its result) open.
        androidx.test.espresso.Espresso.pressBack()
        rule.waitUntil(10_000) { countOf(hasTestTag("glossary-search")) == 0 }
        assertEquals(1, countOf(calcResult))
        androidx.test.espresso.Espresso.pressBack()
        rule.waitUntil(10_000) { countOf(calcResult) == 0 }
        assertEquals("the Profile tab is still there", 1, countOf(hasText(str(R.string.main_dest_profile))))
    }

    // ═══════════════ Log set editor: free set vs plan item ═════════════════

    /** Every text in the window (dialog/sheet root) that holds [tag]'s node. */
    private fun textsInWindowOf(tag: String): List<String> {
        var root = rule.onNode(hasTestTag(tag)).fetchSemanticsNode()
        while (root.parent != null) root = root.parent!!
        val out = mutableListOf<String>()
        fun walk(n: androidx.compose.ui.semantics.SemanticsNode) {
            n.config.getOrNull(SemanticsProperties.Text)?.forEach { out += it.text }
            n.config.getOrNull(SemanticsProperties.ContentDescription)?.forEach { out += it }
            n.children.forEach(::walk)
        }
        walk(root)
        return out
    }

    @Test
    fun planItemSet_headerSpeaksPounds_freeSetHasNoHeader_andSavesThePickedExerciseInKg() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfileLb(), QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(2))))
        rule.launchApp()
        val item = EventDispatcher.model.value!!.next_session!!.items.first { it.exercise.isNotEmpty() }
        rule.onNode(
            hasClickAction() and hasContentDescription(strPrefix(R.string.plan_log_set_n_of_n, 1), substring = true) and
                hasAnyAncestor(hasTestTag("plan-item:${item.exercise}")),
        ).performScrollTo().performClick()
        waitForAny(hasText(str(R.string.log_set_header_n_of_n, 1, item.sets)))
        val planTexts = textsInWindowOf("editor-primary")
        assertTrue("the plan item's sheet states weights in lb: $planTexts", planTexts.any { Regex("""\d lb\b""").containsMatchIn(it) })
        assertTrue("and never in kg: $planTexts", planTexts.none { Regex("""\d(\.\d+)? ?kg\b""").containsMatchIn(it) })
        rule.onNode(hasContentDescription(str(R.string.main_close)) and hasClickAction()).performClick()
        rule.waitUntil(10_000) { countOf(hasTestTag("editor-primary")) == 0 }

        // A free set from the chooser: no header, the exercise first.
        openChooserFromFab("free set")
        rule.onNode(row(R.string.main_log_set_title)).performScrollTo().performClick()
        waitForAny(hasTestTag("exercise-search"))
        assertEquals(0, countOf(hasText(strPrefix(R.string.log_set_header_n_of_n, 1), substring = true)))
        val weight = hasClickAction() and hasContentDescription(str(R.string.log_field_weight), substring = true)
        assertEquals("weight waits for an exercise", 0, countOf(weight))
        rule.onNode(hasText("Back Squat") and hasClickAction()).performScrollTo().performClick()
        waitForAny(weight)
        rule.typeWeight("135")
        if (!runCatching { rule.onNode(hasTestTag("editor-primary")).assertIsEnabled() }.isSuccess) {
            rule.onNode(hasText("5") and hasClickAction() and hasAnyAncestor(hasTestTag("reps-scale"))).performScrollTo().performClick()
        }
        val before = EventDispatcher.model.value!!.lifts.size
        rule.onNode(hasTestTag("editor-primary")).performClick()
        waitForState { EventDispatcher.model.value!!.lifts.size == before + 1 }
        val set = journeyLog().last { it.variant == "LogSet" }.payload
        assertEquals("\"Back Squat\"", set["exercise"].toString())
        assertEquals("135 lb in kg", 61.23, set["weight_kg"].toString().toDouble(), 0.01)
    }
}
