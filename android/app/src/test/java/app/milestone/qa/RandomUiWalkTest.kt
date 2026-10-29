package app.milestone.qa

import android.content.Context
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.SemanticsNodeInteraction
import androidx.compose.ui.test.click
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasAnyDescendant
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.hasScrollToNodeAction
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performTextReplacement
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeDown
import androidx.compose.ui.test.swipeLeft
import androidx.compose.ui.test.swipeRight
import androidx.compose.ui.test.swipeUp
import androidx.compose.ui.text.TextLayoutResult
import androidx.lifecycle.Lifecycle
import androidx.test.core.app.ApplicationProvider
import androidx.test.espresso.Espresso
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
import app.milestone.HistoryFocus
import app.milestone.LiftGoal
import app.milestone.ProgressionCadence
import app.milestone.R
import app.milestone.ReadinessSignal
import app.milestone.RecentRace
import app.milestone.UiMessages
import app.milestone.ViewModel
import app.milestone.groupLiftSessions
import app.milestone.showLifting
import app.milestone.showRunning
import org.junit.After
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import java.time.ZoneId
import java.util.Locale

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
 * The bugs the walk found are pinned by the regression tests
 * below; the walk tolerates none of them, so it guards them too.
 *
 * Replay: every failure prints the start state, seed, step and the action trace.
 * Environment (read by the test JVM):
 *  - `UIWALK_STATE=<Start>` runs only that start's walk;
 *  - `UIWALK_SEED=<n>` walks exactly that seed (as printed);
 *  - `UIWALK_STEPS=<n>` steps per walk (default [DEFAULT_STEPS]);
 *  - long mode: `UIWALK_SEEDS=<n>` walks n seeds per start from the
 *    `UIWALK_FIRST`-th, `UIWALK_KEEPGOING=1` reports every failing seed;
 *  - `UIWALK_DEBUG=1` prints each screen after its step.
 * Gradle caches a passed test task: add `--rerun` when only the environment changed.
 */
@RunWith(RobolectricTestRunner::class)
class RandomUiWalkTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()
    private lateinit var originalQualifiers: String

    @Before
    fun setUp() {
        Locale.setDefault(Locale.US)
        originalQualifiers = RuntimeEnvironment.getQualifiers()
        QaHarness.reset(ctx)
        resetSettings()
    }

    @After
    fun tearDown() {
        QaHarness.teardown()
        RuntimeEnvironment.setQualifiers(originalQualifiers)
        RuntimeEnvironment.setFontScale(1f)
        resetSettings()
    }

    private fun resetSettings() {
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
    private fun clearAppliedCueLoads() {
        (app.milestone.AppliedCueLoads.javaClass.getDeclaredField("byItem").apply { isAccessible = true }
            .get(app.milestone.AppliedCueLoads) as MutableMap<*, *>).clear()
    }

    // ── Starting states ─────────────────────────────────────────────────────

    enum class Start { FreshInstall, Runner, Lifter, Hybrid, Calisthenics, Under18, PainHold, Illness, Pregnancy, LongHistory, MilesLb }

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

    // ── Bugs the walk found, pinned as regression tests ──────────────────

    /**
     * In landscape the Log run sheet opens with its keypad up
     * (RunEditor `keypadOpen = true`) under Duration, below the sheet's fold: the
     * user saw no keypad and the first Back only folded it, so the screen did not
     * change. The editor now scrolls the open keypad (and its field) into view.
     */
    @Test
    fun landscapeLogRun_keypadOpensInView() {
        start = Start.Hybrid
        RuntimeEnvironment.setQualifiers("w891dp-h411dp-land-xxhdpi")
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

    /**
     * The Log set editor's exercise search (a Widgets.kt `SearchWell`) keeps an
     * accessible name once it holds text: it names itself with the hint, not only
     * through the placeholder shown while it is empty.
     */
    @Test
    fun exerciseSearch_keepsItsAccessibleName() {
        start = Start.Lifter
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

    /**
     * A kg value the app itself derived from pounds (the core's
     * lb loads are exact divisions, e.g. `230.0 / LB_PER_KG` in completion.rs, and
     * an untouched lb Save re-emitted that exact kg) was shown in the kg weight
     * field unrounded (`fmtLosslessPrefill`, every digit): editing a 225-lb set read
     * "102.05828325225797" and overflowed the well. The field now shows the kg to
     * two decimals (`loadFigure`), and an untouched Save stores the figure shown.
     */
    @Test
    fun kgWeightField_showsAnLbDerivedLoadAtTwoDecimals() {
        start = Start.Lifter
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
        // An untouched Save stores the figure the field shows, not the raw division.
        rule.onNode(hasTestTag("editor-primary")).performClick()
        settle()
        val saved = Core.currentView().lifts.single().weight_kg
        assertTrue("an untouched Save stores the shown $figure kg, got $saved", saved == figure.toDouble())
    }

    /**
     * A phone in landscape (411dp tall) at 200 % font with a pain
     * report on top of an illness hold: the pinned DO-NOT-TRAIN banner (two
     * resolve actions, shown whole by design) leaves the destination no room, and
     * the "lollop" wordmark row was laid out over the bottom navigation, its text
     * on top of the Today / History / Profile labels. The top chrome now leaves the
     * wordmark out when the banner leaves it no room above the bottom bar.
     */
    @Test
    fun landscapeFont200_holdBanner_wordmarkClearsTheNav() {
        start = Start.Illness
        RuntimeEnvironment.setQualifiers("w891dp-h411dp-land-xxhdpi")
        RuntimeEnvironment.setFontScale(2f)
        QaHarness.seedLog(ctx, seedEvents(Start.Illness)!! + Event.SubmitReadiness(ReadinessSignal.Pain, 1.0, morning + 600))
        rule.launchApp()
        settle()
        val overlaps = textProblems().filter { it.startsWith("overlap:") }
        assertTrue("no text overlaps another:\n" + overlaps.joinToString("\n") + "\n" + screenDump(), overlaps.isEmpty())
    }

    /**
     * The pinned DO-NOT-TRAIN banner is never overlapped (INVARIANT 3): in
     * landscape at 100/150/200 % font on a 360dp- and a 411dp-tall phone, with a
     * two-action hold (illness + pain), no banner node (the card, any tap target's
     * touch bounds, the "?") intersects the bottom nav or the Log button, and Log
     * stays reachable.
     */
    @Test
    fun landscapeHoldBanner_isNeverOverlappedByNavOrLog() {
        start = Start.Illness
        val bad = ArrayList<String>()
        for ((w, h) in listOf(800 to 360, 891 to 411)) for (scale in listOf(1f, 1.5f, 2f)) {
            QaHarness.teardown()
            QaHarness.reset(ctx)
            resetSettings()
            RuntimeEnvironment.setQualifiers("w${w}dp-h${h}dp-land-xxhdpi")
            RuntimeEnvironment.setFontScale(scale)
            QaHarness.seedLog(ctx, seedEvents(Start.Illness)!! + Event.SubmitReadiness(ReadinessSignal.Pain, 1.0, morning + 600))
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

    /**
     * Walk `UIWALK_STATE=Runner UIWALK_SEED=1594695423
     * UIWALK_STEPS=49`: the week strip's done ✓ (the non-colour done cue) sat in a
     * fixed 16dp slot: at 130 % font its 13sp line is taller than 16dp and was cut
     * off. The slot now grows to the ✓'s measured line.
     */
    @Test
    fun weekStripDoneMark_font130_isNotCutOff() {
        start = Start.Lifter
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

    /**
     * Walk `UIWALK_STATE=PainHold UIWALK_SEED=1595153934
     * UIWALK_STEPS=65`: the set loop's "Last: 100 kg × 3 · 3 reps left" line beside
     * "Edit" had `maxLines = 1` and clipped at 200 % font. It now wraps.
     */
    @Test
    fun setLoopLastSetLine_font200_isNotCutOff() {
        start = Start.Lifter
        RuntimeEnvironment.setFontScale(2f)
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        settle()
        // One rated work set of today's first lift at its load: the loop then shows
        // "Last: <load> × <reps> · 3 reps left".
        val first = EventDispatcher.model.value!!.next_session!!.items.first { it.exercise.isNotEmpty() }
        val at = QaHarness.daysAgoAtMidday(0)
        EventDispatcher.send(Event.LogSet(first.exercise, first.load_kg ?: 100.0, maxOf(first.reps_low, 1), 7.0, at, at * 1000))
        awaitCoreLifts(1)
        rule.onNode(
            hasClickAction() and hasContentDescription(str(R.string.plan_log_set_n_of_n, 2, first.sets), substring = true) and
                hasAnyAncestor(hasTestTag("plan-item:${first.exercise}")),
        ).performScrollTo().performClick()
        settle()
        val prefix = str(R.string.log_last_set, "")
        val line = allUnmergedRoots().flatMap { flatten(it) }.flatMap { n ->
            n.config.getOrNull(SemanticsProperties.Text).orEmpty().map { it.text }
        }.firstOrNull { it.startsWith(prefix) }
        assertTrue("precondition: the loop shows the last set's line:\n${screenDump()}", line != null)
        val cut = textProblems().filter { it.startsWith("cut off: \"$prefix") }
        assertTrue("the last set's line \"$line\" is not cut off:\n" + cut.joinToString("\n"), cut.isEmpty())
    }

    /**
     * Walk `UIWALK_STATE=Lifter UIWALK_SEED=1593027678
     * UIWALK_STEPS=157`: a keypad field's value figure was one Display-size line
     * (`maxLines = 1`): in the loop's Edit set dialog at 200 % font a typed
     * 6-character "127.52" did not fit the well and was cut off. The figure now
     * steps down to the Title size, then wraps (Keypad.kt `fittedValueStyle`), in
     * every keypad editor.
     */
    @Test
    fun keypadValueField_font200_showsASixCharacterWeightWhole() {
        start = Start.Lifter
        RuntimeEnvironment.setFontScale(2f)
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        settle()
        val first = EventDispatcher.model.value!!.next_session!!.items.first { it.exercise.isNotEmpty() }
        val at = QaHarness.daysAgoAtMidday(0)
        EventDispatcher.send(Event.LogSet(first.exercise, first.load_kg ?: 125.0, maxOf(first.reps_low, 1), 7.0, at, at * 1000))
        awaitCoreLifts(1)
        rule.onNode(
            hasClickAction() and hasContentDescription(str(R.string.plan_log_set_n_of_n, 2, first.sets), substring = true) and
                hasAnyAncestor(hasTestTag("plan-item:${first.exercise}")),
        ).performScrollTo().performClick()
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
        val shown = allUnmergedRoots().flatMap { flatten(it) }.any { n -> n.config.getOrNull(SemanticsProperties.Text).orEmpty().any { it.text == "127.52" } }
        assertTrue("precondition: the weight field shows the typed 127.52:\n${screenDump()}", shown)
        val cut = textProblems().filter { it.startsWith("cut off: \"127.52\"") }
        assertTrue("the weight field shows 127.52 whole:\n" + cut.joinToString("\n"), cut.isEmpty())
    }

    /** Wait until the core holds [n] logged sets (an [EventDispatcher] send reaches
     *  it on its consumer), then [settle] the UI onto that view. */
    private fun awaitCoreLifts(n: Int) {
        val t0 = System.currentTimeMillis()
        while (Core.currentView().lifts.size != n) {
            if (System.currentTimeMillis() - t0 > 10_000) fail("the core holds ${Core.currentView().lifts.size} sets, expected $n")
            rule.waitForIdle()
            Thread.sleep(20)
        }
        settle()
    }

    private val morning get() = app.milestone.todayEpochDay() * 86_400L + 8 * 3_600L

    private fun profile(
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

    private fun liftDays(vararg daysAgo: Int): List<Event> = daysAgo.flatMap { d ->
        val t = QaHarness.daysAgoAtMidday(d)
        listOf("Back Squat" to 100.0, "Bench Press" to 70.0, "Deadlift" to 120.0).flatMapIndexed { ei, (ex, kg) ->
            (0 until 3).map { s ->
                val at = t + ei * 600L + s * 150L
                Event.LogSet(ex, kg, 5, 8.0, at, at * 1000)
            }
        }
    }

    private fun runs(vararg daysAgo: Int): List<Event> = daysAgo.mapIndexed { i, d ->
        val at = QaHarness.daysAgoAtMidday(d) + 3_600L
        Event.LogRun(5.0 + i, 28.0 + 5 * i, 0.0, at, at * 1000)
    }

    private fun seedEvents(start: Start): List<Event>? = when (start) {
        Start.FreshInstall -> null
        Start.Runner -> listOf(
            Event.SetProfile(
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
            ),
        ) + runs(1, 3, 6, 9, 40)
        Start.Lifter -> listOf(QaHarness.strengthProfile()) + liftDays(1, 3, 5, 8, 35)
        Start.Hybrid -> listOf(profile(runDays = 3, runKm = 20.0, goal = GoalDistance.FiveK)) + liftDays(2, 4) + runs(1, 3, 5)
        Start.Calisthenics -> listOf(QaHarness.calisthenicsProfile(), QaHarness.loggedHold(QaHarness.daysAgoAtMidday(1)), QaHarness.loggedHold(QaHarness.daysAgoAtMidday(3)))
        Start.Under18 -> listOf(profile(age = 15.0))
        Start.PainHold -> listOf(QaHarness.strengthProfile()) + liftDays(2) + Event.SubmitReadiness(ReadinessSignal.Pain, 1.0, morning)
        Start.Illness -> listOf(profile(runDays = 3, runKm = 20.0, goal = GoalDistance.FiveK)) + runs(2) + Event.SubmitReadiness(ReadinessSignal.Illness, 2.0, morning)
        Start.Pregnancy -> listOf(profile(age = 33.0, health = HealthScreen(pregnant = true)))
        Start.LongHistory -> LongHistorySeed.build(days = 150, fixEverySec = 60L).events
        Start.MilesLb -> listOf(QaHarness.strengthProfileLb()) + liftDays(1, 4) + runs(2, 6)
    }

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
            if (k > 0) {
                QaHarness.teardown()
                RuntimeEnvironment.setQualifiers(originalQualifiers)
                RuntimeEnvironment.setFontScale(1f)
                QaHarness.reset(ctx)
                resetSettings()
            }
            try {
                walkOnce(s, fixed ?: (BASE_SEED + s.ordinal * 7_919L + (first + k) * 104_729L), steps)
            } catch (e: AssertionError) {
                if (!keepGoing) throw e
                println("UIWALK-FAIL " + e.message)
                failures += e.message.orEmpty().lineSequence().first()
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
        add(4, "tap at (${px.toInt()},${py.toInt()})px") { rootNode(top).performTouchInput { click(Offset(px - tb.left, py - tb.top)) } }
        add(1, "swipe sideways") { rootNode(top).performTouchInput { if (rng.nextBoolean()) swipeLeft() else swipeRight() } }
        if (overlay) add(6, "swipe sheet down") { rootNode(top).performTouchInput { swipeDown() } }
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
        return desc == "BACK" && !relaunchIfLeft()
    }

    /** Run [block]; an exception thrown from app code is a crash (fail with the
     *  trace), one from the test framework (node gone, touch outside the window)
     *  only skips this action. */
    private fun guard(what: String, block: () -> Unit) {
        try {
            block()
        } catch (e: Throwable) {
            if (isAppCrash(e)) fail("crash during $what: $e\n${e.stackTraceToString().lines().take(30).joinToString("\n")}")
            trace += "   (skipped: ${e.javaClass.simpleName}: ${e.message?.lineSequence()?.firstOrNull()?.take(160)})"
        }
    }

    private fun isAppCrash(e: Throwable): Boolean {
        if (e is AssertionError && e.message?.startsWith("UIWALK") == true) throw e
        var t: Throwable? = e
        while (t != null) {
            if (t.stackTrace.any { f -> f.className.startsWith("app.milestone.") && !f.className.startsWith("app.milestone.qa.") }) return true
            t = t.cause
        }
        return false
    }

    // ── Lifecycle actions ───────────────────────────────────────────────────

    private fun recreate() {
        checkNotNull(lastScenario).recreate()
        rule.waitUntil(15_000) { roots().isNotEmpty() }
    }

    /** Back on the root screen leaves the app; the user opens it again (warm start). */
    private fun relaunchIfLeft(): Boolean {
        val sc = lastScenario ?: return false
        if (sc.state == Lifecycle.State.DESTROYED || activityFinishing()) {
            trace += "   (app left; relaunch)"
            runCatching { sc.close() }
            guard("relaunch") { lastScenario = launchMain() }
            return true
        }
        return false
    }

    private fun activityFinishing(): Boolean {
        var finishing = false
        runCatching { lastScenario?.onActivity { finishing = it.isFinishing } }
        return finishing
    }

    /** Kill the process (every singleton back to a fresh process, the durable log
     *  and prefs kept), relaunch, and require the same entries back. */
    private fun processDeath() {
        val beforeModel = EventDispatcher.model.value
        QaHarness.teardown()
        Core.resetForTest()
        for ((f, v) in listOf("restored" to false, "log" to null, "lastGoodView" to null, "appendTornGuard" to false, "lastSetToday" to null, "viewStale" to false, "logDegraded" to false)) {
            Core.javaClass.getDeclaredField(f).apply { isAccessible = true }.set(Core, v)
        }
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

    /** Idle the UI, then wait until the published model agrees with the core (a
     *  tap's event reaches the core on its JNI thread and the view comes back on a
     *  later main-looper turn). Diverging for 15 s is a failure. */
    private fun settle() {
        val t0 = System.currentTimeMillis()
        while (true) {
            guard("idle") { rule.waitForIdle() }
            val core = Core.currentView()
            guard("idle") { roots() }
            val ui = EventDispatcher.model.value
            if (ui != null && entrySignature(ui) == entrySignature(core)) return
            if (ui == null && System.currentTimeMillis() - t0 > 30_000) fail("stuck splash: no model published after 30 s")
            if (ui != null && System.currentTimeMillis() - t0 > 15_000) {
                fail("UI model diverged from the core:\n ui   ${entrySignature(ui)}\n core ${entrySignature(core)}")
            }
            Thread.sleep(20)
        }
    }

    private fun entrySignature(m: ViewModel) =
        "lifts=${m.lifts.size} runs=${m.runs.size} activities=${m.activities.size} blocked=${m.train_blocked} hold=${m.hold_reason} " +
            "liftIds=${m.lifts.map { it.entry_id }.hashCode()} runIds=${m.runs.map { it.entry_id }.hashCode()}"

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
            .map { it.toString() } + smallTargets()
        if (ux.isNotEmpty()) fail("UX invariants:\n" + ux.take(12).joinToString("\n"))
        val text = textProblems()
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

    /** The History LazyColumn's item count, read from its scroll-to-index bound. */
    private fun historyItemCount(): Int? {
        val lists = flatten(roots().first()).filter { it.config.getOrNull(SemanticsActions.ScrollToIndex) != null && it.isVisible() }
        val list = lists.maxByOrNull { it.boundsInRoot.height } ?: return null
        return try {
            node(list).performSemanticsAction(SemanticsActions.ScrollToIndex) { it(Int.MAX_VALUE) }
            null
        } catch (e: IllegalArgumentException) {
            Regex("""out of bounds \[0, (\d+)\)""").find(e.message.orEmpty())?.groupValues?.get(1)?.toInt()
        }
    }

    private fun endOfWalkChecks() {
        trace += "end: process death + relaunch"
        guard("end-of-walk process death") { processDeath() }
        settle()
        checkAll(afterBack = null)
    }

    // ── Screen helpers ──────────────────────────────────────────────────────

    private fun roots(): List<SemanticsNode> = rule.onAllNodes(isRoot()).fetchSemanticsNodes()

    private fun flatten(n: SemanticsNode): List<SemanticsNode> = listOf(n) + n.children.flatMap { flatten(it) }

    private fun allUnmerged(root: SemanticsNode): List<SemanticsNode> {
        val unmerged = rule.onAllNodes(isRoot(), useUnmergedTree = true).fetchSemanticsNodes()
            .firstOrNull { it.id == root.id } ?: return emptyList()
        return flatten(unmerged)
    }

    private fun allUnmergedRoots(): List<SemanticsNode> = rule.onAllNodes(isRoot(), useUnmergedTree = true).fetchSemanticsNodes()

    private fun SemanticsNode.isVisible() = boundsInRoot.width > 0f && boundsInRoot.height > 0f

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

    /** Fully visible actions whose touch target (own size or touch expansion) is
     *  under 48dp; a partly scrolled-out action is judged once fully in view. */
    private fun smallTargets(): List<String> {
        val d = rule.density.density
        return roots().flatMap { flatten(it) }.filter { n ->
            n.config.getOrNull(SemanticsActions.OnClick) != null && n.isVisible() &&
                n.boundsInRoot.width + 1f >= n.size.width && n.boundsInRoot.height + 1f >= n.size.height
        }.mapNotNull { n ->
            val w = maxOf(n.size.width.toFloat(), n.touchBoundsInRoot.width) / d
            val h = maxOf(n.size.height.toFloat(), n.touchBoundsInRoot.height) / d
            if (w + 0.5f < 48f || h + 0.5f < 48f) "[small-touch-target] \"${labelOf(n)}\" is ${w.toInt()}x${h.toInt()}dp (<48dp)" else null
        }
    }

    private fun labelOf(n: SemanticsNode): String =
        (n.config.getOrNull(SemanticsProperties.Text).orEmpty().map { it.text } +
            n.config.getOrNull(SemanticsProperties.ContentDescription).orEmpty() +
            listOfNotNull(n.config.getOrNull(SemanticsProperties.EditableText)?.text))
            .firstOrNull { it.isNotBlank() }?.take(60) ?: (n.config.getOrNull(SemanticsProperties.TestTag) ?: "")

    private fun node(n: SemanticsNode): SemanticsNodeInteraction {
        val id = n.id
        val b = n.boundsInRoot
        val all = rule.onAllNodes(SemanticsMatcher("node #$id") { it.id == id && it.boundsInRoot == b })
        val count = all.fetchSemanticsNodes().size
        return all[(count - 1).coerceAtLeast(0)]
    }

    private fun rootNode(r: SemanticsNode): SemanticsNodeInteraction = node(r)

    /** The selected bottom-nav destination, or null off the main scaffold. */
    private fun selectedTab(): String? {
        val tabs = setOf(str(R.string.main_dest_today), str(R.string.main_dest_history), str(R.string.main_dest_profile))
        return flatten(roots().first()).firstOrNull { n ->
            n.config.getOrNull(SemanticsProperties.Selected) == true && labelOf(n) in tabs
        }?.let { labelOf(it) }
    }

    /** The top window holds an open numeric keypad the user cannot see. */
    private fun offscreenKeypad(): Boolean = keypadNodes().let { k -> k.isNotEmpty() && k.none { it.isVisible() } }

    private fun keypadComposed(): Boolean = keypadNodes().isNotEmpty()

    private fun keypadNodes(): List<SemanticsNode> = roots().lastOrNull()?.let { top ->
        allUnmerged(top).filter { it.config.getOrNull(SemanticsProperties.TestTag) == "numeric-keypad" }
    }.orEmpty()

    /** What a user perceives: the windows, the selected tab, and the top window's texts. */
    private fun fingerprint(): String {
        val roots = roots()
        val top = roots.lastOrNull() ?: return "<none>"
        val texts = flatten(top).filter { it.isVisible() }.map { labelOf(it) + "@" + it.boundsInRoot.top.toInt() }
        return "${roots.size}|${selectedTab()}|${texts.hashCode()}"
    }
    private fun screenDump(): String = roots().mapIndexed { i, r ->
        "window $i: " + flatten(r).filter { it.isVisible() }.map { labelOf(it) }.filter { it.isNotBlank() }.distinct().take(40).joinToString(" | ")
    }.joinToString("\n")

    /** Visible text the layout cut off (ellipsis, lines past maxLines, an unwrapped
     *  line wider than its box) or that overlaps another visible text in the same
     *  window by more than 2dp each way (same rules as LongHistoryUiTest). */
    private fun textProblems(): List<String> {
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
    private fun chromeOverBanner(): List<String> {
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

    private fun quote(s: String) = "\"" + (if (s.length > 24) s.take(24) + "…(${s.length})" else s).replace("\n", "\\n") + "\""

    private fun fail(msg: String): Nothing {
        val replay = "UIWALK_STATE=${start.name} UIWALK_SEED=$seed UIWALK_STEPS=$step"
        throw AssertionError(
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

        /** A History lift-session card's count line ("1 exercise · 1 set"). */
        private val LIFT_COUNT = Regex("""^\d+ exercises? · \d+ sets?$""")

        /** Plausible and hostile text-field input: empty, blank, numbers with
         *  commas / dots / signs, huge, unicode, emoji, RTL, control characters. */
        private val TEXTS = listOf(
            "", " ", "0", "5", "12,5", "1,234.5", "1.234,5", "-3", "99999999999", "1e9", "NaN", "0.0001",
            "Back Squat", "squat", "Bench", "knee", "Front Lever", "Müller-Übung", "深蹲", "قرفصاء", "💪🏽🔥",
            "a".repeat(400), "Line\nbreak", "\t", "' OR 1=1 --", "<b>bold</b>", "%s %d", "\u200B",
        )
    }
}
