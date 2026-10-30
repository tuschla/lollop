package app.milestone.qa

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.SemanticsNodeInteraction
import androidx.compose.ui.test.click
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasAnyDescendant
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasScrollAction
import androidx.compose.ui.test.hasScrollToNodeAction
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isDialog
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performTextReplacement
import androidx.compose.ui.test.performTouchInput
import androidx.test.espresso.Espresso
import app.milestone.Core
import app.milestone.Event
import app.milestone.EventDispatcher
import app.milestone.GoalDistance
import app.milestone.PrescriptionView
import app.milestone.R
import app.milestone.ReadinessSignal
import java.time.LocalDate
import java.time.format.TextStyle
import java.util.Locale
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment

/**
 * Fixed layout and accessibility cases at large font scales and in landscape,
 * most of them first seen by [RandomUiWalkTest] (the `UIWALK_*` line in a KDoc
 * replays that walk), checked with the walk's own probes.
 */
@RunWith(RobolectricTestRunner::class)
class LayoutRegressionTest : UiProbeTest() {

    private fun landscape() = RuntimeEnvironment.setQualifiers("w891dp-h411dp-land-xxhdpi")

    /** An illness hold with a pain report on top: the hold banner with two resolve actions. */
    private fun illnessAndPain(): List<Event> =
        listOf(profile(runDays = 3, runKm = 20.0, goal = GoalDistance.FiveK)) + runs(2) +
            Event.SubmitReadiness(ReadinessSignal.Illness, 2.0, morning) + Event.SubmitReadiness(ReadinessSignal.Pain, 1.0, morning + 600)

    /** Every text on screen, merged or not. */
    private fun screenTexts(): List<String> = allUnmergedRoots().flatMap { flatten(it) }.flatMap { n ->
        n.config.getOrNull(SemanticsProperties.Text).orEmpty().map { it.text }
    }

    /** Log one rated work set of today's first lift (at [fallbackKg] without a plan
     *  load) and return that lift. */
    private fun logFirstWorkSet(fallbackKg: Double): PrescriptionView {
        val first = EventDispatcher.model.value!!.next_session!!.items.first { it.exercise.isNotEmpty() }
        val at = QaHarness.daysAgoAtMidday(0)
        EventDispatcher.send(Event.LogSet(first.exercise, first.load_kg ?: fallbackKg, maxOf(first.reps_low, 1), 7.0, at, at * 1000))
        awaitCoreLifts(1)
        return first
    }

    /** Tap "Move" on the session [title] by its semantics action: after
     *  performScrollTo's minimal scroll the floating Log button can sit over it in
     *  landscape. */
    private fun tapMove(title: String) {
        rule.onAllNodes(
            hasClickAction() and hasContentDescription(str(R.string.plan_secondary_action_cd, str(R.string.main_move_session), title)),
        )[0].performScrollTo().performSemanticsAction(SemanticsActions.OnClick)
        settle()
    }

    /** In landscape the Log run sheet opens with its keypad (under Duration, below
     *  the sheet's fold) scrolled into view, and Back changes what the user sees. */
    @Test
    fun landscapeLogRun_keypadOpensInView() {
        landscape()
        QaHarness.seedLog(ctx, listOf(profile(runDays = 3, runKm = 20.0, goal = GoalDistance.FiveK)))
        rule.launchApp()
        rule.onNode(hasContentDescription(str(R.string.main_log)) and hasClickAction()).performClick()
        settle()
        rule.onNode(hasText(str(R.string.main_log_run_title)) and hasClickAction()).performClick()
        settle()
        assertTrue("precondition: the Log run sheet opens with its keypad", keypadComposed())
        assertFalse("the open keypad is on screen (it lies below the sheet's fold):\n${screenDump()}", offscreenKeypad())
        val before = fingerprint()
        Espresso.pressBackUnconditionally()
        settle()
        assertNotEquals("Back changes what the user sees:\n${screenDump()}", before, fingerprint())
    }

    /** The Log set editor's exercise search (a Widgets.kt `SearchWell`) keeps an
     *  accessible name once it holds text: it names itself with the hint, not only
     *  through the placeholder shown while it is empty. */
    @Test
    fun exerciseSearch_keepsItsAccessibleName() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        rule.onNode(hasContentDescription(str(R.string.main_log)) and hasClickAction()).performClick()
        settle()
        rule.onNode(hasText(str(R.string.main_log_set_title)) and hasClickAction()).performClick()
        settle()
        val search = rule.onNode(hasTestTag("exercise-search"))
        search.performTextReplacement(" ")
        settle()
        val unlabeled = UxInvariants.check(rule, rule.density).filter { it.kind == "unlabeled-action" }
        assertTrue("a blank search leaves no unlabeled action:\n" + unlabeled.joinToString("\n"), unlabeled.isEmpty())
        search.performTextReplacement("squat")
        settle()
        val name = search.fetchSemanticsNode().config.getOrNull(SemanticsProperties.ContentDescription).orEmpty()
        assertTrue("the field keeps its name \"${str(R.string.log_exercise_search_hint)}\" while it holds text: $name", str(R.string.log_exercise_search_hint) in name)
    }

    /** A kg load the app derived from pounds (an exact division such as
     *  `225.0 / LB_PER_KG`) shows in the kg weight field at most to two decimals
     *  (`loadFigure`), and an untouched Save stores the figure shown. */
    @Test
    fun kgWeightField_showsAnLbDerivedLoadAtTwoDecimals() {
        val at = QaHarness.daysAgoAtMidday(1)
        val lbDerivedKg = 225.0 / 2.2046226218
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), Event.LogSet("Bench Press", lbDerivedKg, 5, 8.0, at, at * 1000)))
        rule.launchApp()
        rule.onNode(hasText(str(R.string.main_dest_history)) and hasClickAction()).performClick()
        settle()
        rule.onNode(hasClickAction() and SemanticsMatcher("a lift session card") { n ->
            n.config.getOrNull(SemanticsProperties.Text).orEmpty().any { LIFT_COUNT.matches(it.text) }
        }).performSemanticsAction(SemanticsActions.OnClick)
        settle()
        rule.onNode(hasTestTag("lift-set-row")).performClick()
        settle()
        rule.onNode(hasText(str(R.string.history_edit_set)) and hasClickAction()).performClick()
        settle()
        val well = "${str(R.string.log_field_weight)}, "
        val shown = roots().flatMap { flatten(it) }
            .flatMap { it.config.getOrNull(SemanticsProperties.ContentDescription).orEmpty() }
            .firstOrNull { it.startsWith(well) && it.endsWith(" kg") }
        assertTrue("the weight well is on screen:\n${screenDump()}", shown != null)
        val figure = shown!!.removePrefix(well).removeSuffix(" kg")
        assertTrue("the kg field shows at most two decimals, got \"$figure\"", Regex("""^\d+(\.\d{1,2})?$""").matches(figure))
        rule.onNode(hasTestTag("editor-primary")).performClick()
        settle()
        val saved = Core.currentView().lifts.single().weight_kg
        assertTrue("an untouched Save stores the shown $figure kg, got $saved", saved == figure.toDouble())
    }

    /** Landscape (411dp tall) at 200 % font under a two-action hold banner: the top
     *  chrome (the "lollop" wordmark) never lies over the bottom navigation, nor any
     *  text over another. */
    @Test
    fun landscapeFont200_holdBanner_wordmarkClearsTheNav() {
        landscape()
        RuntimeEnvironment.setFontScale(2f)
        QaHarness.seedLog(ctx, illnessAndPain())
        rule.launchApp()
        settle()
        val overlaps = textProblems().filter { it.startsWith("overlap:") }
        assertTrue("no text overlaps another:\n" + overlaps.joinToString("\n") + "\n" + screenDump(), overlaps.isEmpty())
    }

    /** The pinned DO-NOT-TRAIN banner is never overlapped (INVARIANT 3): in
     *  landscape at 100/150/200 % font on a 360dp- and a 411dp-tall phone, with a
     *  two-action hold (illness + pain), no banner node (the card, any tap target's
     *  touch bounds, the "?") intersects the bottom nav or the Log button, and Log
     *  stays reachable. */
    @Test
    fun landscapeHoldBanner_isNeverOverlappedByNavOrLog() {
        val bad = ArrayList<String>()
        for ((w, h) in listOf(800 to 360, 891 to 411)) for (scale in listOf(1f, 1.5f, 2f)) {
            QaHarness.teardown()
            QaHarness.reset(ctx)
            resetSettings()
            RuntimeEnvironment.setQualifiers("w${w}dp-h${h}dp-land-xxhdpi")
            RuntimeEnvironment.setFontScale(scale)
            QaHarness.seedLog(ctx, illnessAndPain())
            rule.launchApp()
            settle()
            val config = "${h}dp-tall, font ${(scale * 100).toInt()} %"
            val banner = allUnmerged(roots().first()).firstOrNull { it.config.getOrNull(SemanticsProperties.TestTag) == "safety-banner" }
            if (banner == null || !banner.isVisible()) {
                bad += "$config: the hold banner is not on screen"
                continue
            }
            bad += chromeOverBanner().map { "$config: $it" }
            val log = flatten(roots().first()).filter { n ->
                n.isVisible() && n.config.getOrNull(SemanticsActions.OnClick) != null &&
                    str(R.string.main_log) in n.config.getOrNull(SemanticsProperties.ContentDescription).orEmpty()
            }
            if (log.isEmpty()) bad += "$config: no Log action on screen"
        }
        assertTrue("the hold banner is never overlapped and Log stays reachable:\n" + bad.joinToString("\n"), bad.isEmpty())
    }

    /** `UIWALK_STATE=Runner UIWALK_SEED=1594695423 UIWALK_STEPS=49`: at 130 % font
     *  the week strip's done ✓ (the non-colour done cue) is whole. */
    @Test
    fun weekStripDoneMark_font130_isNotCutOff() {
        RuntimeEnvironment.setFontScale(1.3f)
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        settle()
        // Complete today's session: every set of every item, at its load.
        val items = EventDispatcher.model.value!!.week_plan.first { it.epoch_day == app.milestone.todayEpochDay() }.items
        var id = 0L
        EventDispatcher.sendAll(
            items.filter { it.exercise.isNotEmpty() }.flatMap { item ->
                (0 until maxOf(item.sets, 1)).map {
                    id++
                    Event.LogSet(item.exercise, item.load_kg ?: 60.0, maxOf(item.reps_high, item.reps_low, 1), 8.0, QaHarness.daysAgoAtMidday(0) + id, id)
                }
            },
        )
        awaitCoreLifts(id.toInt())
        // The week strip sits below the hero: bring it on screen.
        rule.onAllNodes(hasScrollToNodeAction())[0].performScrollToNode(hasTestTag("this-week"))
        settle()
        val strip = allUnmergedRoots().flatMap { flatten(it) }.first { it.config.getOrNull(SemanticsProperties.TestTag) == "this-week" }
        val marks = flatten(strip).filter { n ->
            n.isVisible() && n.config.getOrNull(SemanticsProperties.Text).orEmpty().any { it.text == "\u2713" }
        }
        assertTrue("precondition: today's week-strip day shows its done ✓:\n${screenDump()}", marks.isNotEmpty())
        val cut = textProblems().filter { it.startsWith("cut off: \"\u2713\"") }
        assertTrue("the done ✓ is not cut off:\n" + cut.joinToString("\n"), cut.isEmpty())
    }

    /** `UIWALK_STATE=PainHold UIWALK_SEED=1595153934 UIWALK_STEPS=65`: at 200 % font
     *  the set loop's "Last: 100 kg × 3 · 3 reps left" line beside "Edit" is whole. */
    @Test
    fun setLoopLastSetLine_font200_isNotCutOff() {
        RuntimeEnvironment.setFontScale(2f)
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        settle()
        val first = logFirstWorkSet(fallbackKg = 100.0)
        rule.openLoopFor(first.exercise, set = 2)
        settle()
        val prefix = str(R.string.log_last_set, "")
        val line = screenTexts().firstOrNull { it.startsWith(prefix) }
        assertTrue("precondition: the loop shows the last set's line:\n${screenDump()}", line != null)
        val cut = textProblems().filter { it.startsWith("cut off: \"$prefix") }
        assertTrue("the last set's line \"$line\" is not cut off:\n" + cut.joinToString("\n"), cut.isEmpty())
    }

    /** `UIWALK_STATE=Lifter UIWALK_SEED=1593027678 UIWALK_STEPS=157`: at 200 % font a
     *  typed 6-character weight ("127.52") shows whole in the loop's Edit set dialog
     *  (Keypad.kt `fittedValueStyle` steps the figure down, then wraps). */
    @Test
    fun keypadValueField_font200_showsASixCharacterWeightWhole() {
        RuntimeEnvironment.setFontScale(2f)
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        settle()
        val first = logFirstWorkSet(fallbackKg = 125.0)
        rule.openLoopFor(first.exercise, set = 2)
        settle()
        // The loop's "Last: … · Edit" row opens the Edit set dialog.
        rule.onNode(hasClickAction() and hasContentDescription(str(R.string.log_edit_set_cd, ""), substring = true)).performScrollTo().performClick()
        settle()
        // Type in the Edit set dialog, the top window (the loop under it has a
        // weight field and a keypad of its own).
        fun topmost(m: SemanticsMatcher): SemanticsNodeInteraction =
            rule.onAllNodes(m).let { all -> all[all.fetchSemanticsNodes().size - 1] }
        // Tap the figure at the well's start (its centre can be a ± chip at 200 %).
        topmost(hasClickAction() and hasContentDescription("${str(R.string.log_field_weight)}, ", substring = true))
            .performScrollTo().performTouchInput { click(centerLeft + Offset(48f * density, 0f)) }
        settle()
        for (k in "127.52") {
            val key = if (k == '.') hasContentDescription(str(R.string.ui_cd_decimal_point)) else hasText("$k")
            topmost(hasClickAction() and (key or hasAnyDescendant(key)) and hasAnyAncestor(hasTestTag("numeric-keypad")))
                .performScrollTo().performClick()
        }
        settle()
        assertTrue("precondition: the weight field shows the typed 127.52:\n${screenDump()}", "127.52" in screenTexts())
        val cut = textProblems().filter { it.startsWith("cut off: \"127.52\"") }
        assertTrue("the weight field shows 127.52 whole:\n" + cut.joinToString("\n"), cut.isEmpty())
    }

    /** `UIWALK_STATE=MilesLb UIWALK_SEED=1592776843 UIWALK_STEPS=214`: at 200 % font
     *  on set 1 of a barbell lift, every logged warm-up step's done mark ("✓ ×3") is
     *  whole. */
    @Test
    fun warmUpDoneMark_font200_isNotCutOff() {
        RuntimeEnvironment.setFontScale(2f)
        // Logged lifts give today's items a load, so they show plates and warm-ups.
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()) + liftDays(1, 3, 5, 8, 35))
        rule.launchApp()
        settle()
        // Today's first plate-loaded lift with its warm-ups under 80 % of the work
        // load logged as the loop's one-tap warm-up logs them (unrated). The ramp's
        // top step is left out: logged first, the core counts it as a work set
        // (completion.rs `the_warm_up_ramp_top_step_does_not_count_before_the_first_work_set`)
        // and the loop drops its warm-up row.
        val first = EventDispatcher.model.value!!.next_session!!.items.first { it.exercise.isNotEmpty() && it.plates_per_side.isNotEmpty() }
        assertTrue("precondition: ${first.exercise} has no set logged today", first.sets_done == 0)
        val workKg = first.load_kg ?: first.suggested_load_kg!!
        val steps = app.milestone.warmUpSteps(workKg, app.milestone.WeightUnit.Kg, first.exercise).filter { it.kg < 0.8 * workKg }
        assertTrue("precondition: ${first.exercise} at $workKg kg has a load step besides the bar: $steps", steps.any { !it.bar })
        val at = QaHarness.daysAgoAtMidday(0)
        val seeded = Core.currentView().lifts.size
        EventDispatcher.sendAll(steps.mapIndexed { i, s -> Event.LogSet(first.exercise, s.kg, s.reps, 0.0, at + i, (at + i) * 1000) })
        awaitCoreLifts(seeded + steps.size)
        rule.openLoopFor(first.exercise)
        settle()
        val marks = screenTexts().filter { it.startsWith("\u2713\u00A0") }
        assertTrue("precondition: the warm-up row shows a done step per warm-up:\n${screenDump()}", marks.size == steps.size)
        val cut = textProblems().filter { it.startsWith("cut off: \"\u2713") }
        assertTrue("the done warm-up steps $marks are not cut off:\n" + cut.joinToString("\n"), cut.isEmpty())
    }

    /** `UIWALK_STATE=LongHistory UIWALK_SEED=1593292569 UIWALK_STEPS=149`: in
     *  landscape at 200 % font, the dialog "Move" opens on a session the core cannot
     *  move shows the core's whole reason ("No other day this week works. On
     *  Thursday, …"). */
    @Test
    fun moveBlockedDialog_landscapeFont200_showsTheWholeReason() {
        landscape()
        RuntimeEnvironment.setFontScale(2f)
        QaHarness.seedLog(ctx, LongHistorySeed.build(days = 150, fixEverySec = 60L).events)
        rule.launchApp()
        settle()
        val vm = EventDispatcher.model.value!!
        val blocked = (listOfNotNull(vm.next_session) + vm.week_plan).firstOrNull { s ->
            s.move_targets.isEmpty() && !s.move_blocked_reason.isNullOrBlank()
        }
        assertTrue("precondition: a session the core cannot move, with its reason", blocked != null)
        val reason = blocked!!.move_blocked_reason!!
        tapMove(blocked.title)
        assertTrue("precondition: the dialog shows the core's reason \"$reason\":\n${screenDump()}", reason in screenTexts())
        val cut = textProblems().filter { it.startsWith("cut off: \"${reason.take(20)}") }
        assertTrue("the move-blocked reason is shown whole:\n" + cut.joinToString("\n"), cut.isEmpty())
    }

    /** The move day picker in landscape at 200 % font with the most target days a
     *  generated week offers (one run day: the long run may go to all four free
     *  days): every target row scrolls whole (≥ 48dp) into the body between the
     *  title and Cancel, and a tap on the last one moves the session there. */
    @Test
    fun moveDayPicker_landscapeFont200_reachesEveryTargetDay() {
        landscape()
        RuntimeEnvironment.setFontScale(2f)
        QaHarness.seedLog(ctx, listOf(profile(runDays = 1, runKm = 6.0)))
        rule.launchApp()
        settle()
        val vm = EventDispatcher.model.value!!
        val session = (listOfNotNull(vm.next_session) + vm.week_plan).maxBy { it.move_targets.size }
        val targets = session.move_targets
        assertTrue("precondition: a session with four target days, got ${session.title} $targets", targets.size >= 4)
        // A day other than the hero's shows its "Move" once picked in the week strip.
        if (session.epoch_day != vm.next_session?.epoch_day) {
            val weekday = LocalDate.ofEpochDay(session.epoch_day).dayOfWeek.getDisplayName(TextStyle.FULL, Locale.ENGLISH)
            rule.onAllNodes(hasScrollToNodeAction())[0].performScrollToNode(hasTestTag("this-week"))
            settle()
            rule.onNode(hasClickAction() and hasContentDescription("$weekday, ", substring = true)).performScrollTo().performSemanticsAction(SemanticsActions.OnClick)
            settle()
        }
        tapMove(session.title)
        val inDialog = hasAnyAncestor(isDialog())
        val title = rule.onNode(hasText(str(R.string.main_move_session_title, session.title)) and inDialog, useUnmergedTree = true)
            .fetchSemanticsNode().boundsInRoot
        val cancel = rule.onNode(hasText(str(R.string.main_cancel)) and inDialog, useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
        val d = rule.density.density
        val slack = 2f * d
        val unreachable = targets.mapNotNull { day ->
            val tag = hasTestTag("move-target:$day") and hasClickAction()
            // Scroll it into view when the body scrolls; a fixed body leaves it where it is.
            if (rule.onAllNodes(tag and hasAnyAncestor(hasScrollAction())).fetchSemanticsNodes().isNotEmpty()) {
                rule.onNode(tag).performScrollTo()
                settle()
            }
            val b = rule.onNode(tag).fetchSemanticsNode().boundsInRoot
            // boundsInRoot is clipped by the dialog: a clipped row shows only a sliver.
            if (b.top >= title.bottom - slack && b.bottom <= cancel.top + slack && b.height >= app.milestone.ActionMinHeight.value * d - slack) {
                null
            } else {
                "move-target:$day at ${(b.top / d).toInt()}-${(b.bottom / d).toInt()}dp (title ends ${(title.bottom / d).toInt()}dp, Cancel starts ${(cancel.top / d).toInt()}dp)"
            }
        }
        assertTrue("every target day is in view between the title and Cancel once scrolled to:\n" + unreachable.joinToString("\n"), unreachable.isEmpty())
        val last = targets.last()
        rule.onNode(hasTestTag("move-target:$last") and hasClickAction()).performClick()
        settle()
        val moved = Core.currentView().let { listOfNotNull(it.next_session) + it.week_plan }
            .any { it.epoch_day == last && it.moved_from_epoch_day == session.epoch_day }
        assertTrue("a tap on the last target day moves ${session.title} there", moved)
    }

    private companion object {
        /** A History lift-session card's count line ("1 exercise · 1 set"). */
        val LIFT_COUNT = Regex("""^\d+ exercises? · \d+ sets?$""")
    }
}
