package app.milestone.qa

import android.Manifest
import android.app.Application
import android.content.Context
import android.location.LocationManager
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.hasScrollToNodeAction
import androidx.test.core.app.ApplicationProvider
import app.milestone.AppRuntime
import app.milestone.AppSettings
import app.milestone.ConcurrentGoal
import app.milestone.DarkMode
import app.milestone.Event
import app.milestone.EventDispatcher
import app.milestone.GoalDistance
import app.milestone.LiftGoal
import app.milestone.OpenDest
import app.milestone.ProgressionCadence
import app.milestone.R
import app.milestone.ReadinessSignal
import app.milestone.RunSession
import app.milestone.openIntent
import java.util.Locale
import org.junit.After
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.ParameterizedRobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf

/**
 * Screenshot baselines for the wide and short windows: a phone in landscape
 * (and a small one), a tablet in portrait and in landscape, in light, dark and
 * at 200 % font. Each scene is one launch of the REAL app over the real core
 * (see [ScreenshotTest]), captured as `adaptive-<window>-<variant>-<scene>.png`.
 *
 * The portrait phone keeps its baselines in the other Screenshot* classes.
 */
@RunWith(ParameterizedRobolectricTestRunner::class)
class ScreenshotAdaptiveTest(private val window: Window, private val variant: Variant) {

    enum class Window(val qualifiers: String) {
        PhoneLand("w891dp-h411dp-land-xxhdpi"),
        SmallLand("w640dp-h360dp-land-xhdpi"),
        TabletPort("w800dp-h1280dp-port-xhdpi"),
        TabletLand("w1280dp-h800dp-land-xhdpi"),
    }

    enum class Variant(val mode: DarkMode, val fontScale: Float) {
        Light(DarkMode.Light, 1f),
        Dark(DarkMode.Dark, 1f),
        Font200(DarkMode.Light, 2f),
    }

    companion object {
        @JvmStatic
        @ParameterizedRobolectricTestRunner.Parameters(name = "{0}-{1}")
        fun configs(): List<Array<Any>> = listOf(
            Window.PhoneLand to Variant.Light,
            Window.PhoneLand to Variant.Dark,
            Window.PhoneLand to Variant.Font200,
            Window.SmallLand to Variant.Light,
            Window.SmallLand to Variant.Font200,
            Window.TabletPort to Variant.Light,
            Window.TabletLand to Variant.Light,
            Window.TabletLand to Variant.Dark,
        ).map { (w, v) -> arrayOf(w, v) }
    }

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @Before
    fun setUp() {
        QaHarness.reset(ctx)
        RuntimeEnvironment.setQualifiers(window.qualifiers)
        RuntimeEnvironment.setFontScale(variant.fontScale)
        useDarkMode(variant.mode)
        shadowOf(ctx as Application).grantPermissions(
            Manifest.permission.ACCESS_FINE_LOCATION,
            Manifest.permission.ACCESS_COARSE_LOCATION,
            Manifest.permission.POST_NOTIFICATIONS,
        )
        shadowOf(ctx.getSystemService(LocationManager::class.java)).setLocationEnabled(true)
        // No map tiles under Robolectric (and none wanted in a baseline).
        AppSettings.setShowRouteMaps(ctx, false)
    }

    @After
    fun tearDown() {
        QaHarness.teardown()
        RunSession.reset()
        QaHarness.freshProcess()
    }

    private fun shot(scene: String) =
        rule.shot("adaptive-${window.name.lowercase()}-${variant.name.lowercase()}-$scene")

    private fun waitForText(text: String, substring: Boolean = false) {
        rule.waitForNode(hasText(text, substring = substring))
        rule.waitForIdle()
    }

    private val logButton get() = hasClickAction() and hasContentDescription(str(R.string.main_log))

    private fun openChooser() {
        rule.onNode(logButton).performClick()
        waitForText(str(R.string.main_track_run_title))
    }

    private val squat = "Back squat"

    private fun seedStrengthWithHistory() {
        val prior = QaHarness.daysAgoAtMidday(2)
        QaHarness.seedLog(
            ctx,
            listOf(
                QaHarness.strengthProfile(),
                Event.LogSet(squat, 120.0, 3, 8.0, prior - 1_800, (prior - 1_800) * 1000),
                Event.LogSet("Row", 60.0, 8, 7.0, prior, prior * 1000),
                Event.LogSet("Bench press", 60.0, 8, 7.0, prior + 600, prior * 1000 + 1),
                Event.LogSet("Romanian deadlift", 80.0, 8, 7.0, prior + 1200, prior * 1000 + 2),
            ),
        )
    }

    @Test
    fun today() {
        seedStrengthWithHistory()
        rule.launchApp()
        waitForText(strPrefix(R.string.plan_face_per_side), substring = true)
        shot("today")
        rule.onNode(hasClickAction() and hasContentDescription(str(R.string.main_move_session), substring = true))
            .performScrollTo().performClick()
        rule.waitForNode(SemanticsMatcher("testTag starts with move-target:") {
            it.config.getOrNull(SemanticsProperties.TestTag)?.startsWith("move-target:") == true
        })
        shot("today-move-dialog")
    }

    @Test
    fun todayPainHold() {
        val thisMorning = QaHarness.daysAgoAtMidday(0) - 4 * 3_600L
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), Event.SubmitReadiness(ReadinessSignal.Pain, 1.0, thisMorning)))
        rule.launchApp()
        waitForText("I'm pain-free now")
        shot("today-painhold")
    }

    @Test
    fun setLoop() {
        QaHarness.seedLog(
            ctx,
            listOf(QaHarness.strengthProfile(), Event.LogSet(squat, 120.0, 3, 8.0, QaHarness.daysAgoAtMidday(1), QaHarness.daysAgoAtMidday(1) * 1000)),
        )
        rule.launchApp()
        rule.openLoopFor(squat)
        rule.waitForIdle()
        shot("setloop")
        // The weight keypad up (a semantics tap: beside the keypad pane the well can
        // sit at the scroll edge, where a centre tap lands outside its viewport).
        rule.onAllNodes(hasClickAction() and hasContentDescription(str(R.string.log_field_weight), substring = true))[0]
            .performScrollTo().performSemanticsAction(SemanticsActions.OnClick)
        rule.waitForNode(hasTestTag("numeric-keypad"))
        rule.waitForIdle()
        shot("setloop-keypad")
        rule.onNode(hasClickAction() and hasText(str(R.string.log_keypad_done))).performScrollTo().performClick()
        rule.waitForIdle()
        rule.onNodeWithTag("editor-primary").performClick()
        waitForText(strPrefix(R.string.log_set_header_n_of_n, 2), substring = true)
        rule.waitForNode(hasText(str(R.string.log_undo)))
        shot("setloop-rest")
    }

    @Test
    fun logChooserAndRunEditor() {
        QaHarness.seedLog(ctx, listOf(QaHarness.fiveKRunnerProfile()))
        rule.launchApp()
        openChooser()
        shot("log-chooser")
        rule.onNodeWithText(str(R.string.main_log_run_title)).performScrollTo().performClick()
        rule.waitForNode(hasContentDescription(str(R.string.log_field_duration)))
        rule.waitForIdle()
        shot("log-run-editor")
    }

    private fun seedRunsHistory() {
        val manual = buildList<Pair<Long, Event>> {
            for (w in 12 downTo 1) {
                for (off in intArrayOf(6, 4, 2)) {
                    val km = when (off) {
                        2 -> 6.0 + w % 3
                        4 -> 4.5 + w % 2
                        else -> 11.0 + w % 4
                    }
                    val at = QaHarness.daysAgoAtMidday(7 * (w - 1) + off)
                    add(at to Event.LogRun(km, km * 5.8, 0.0, observedAt = at, entryId = at * 1000))
                }
            }
            var t = QaHarness.daysAgoAtMidday(3)
            repeat(3) {
                t += 180
                add(t to Event.LogSet("Back Squat", 100.0, 5, 8.0, t, t * 1000))
            }
        }.sortedBy { it.first }.map { it.second }
        val thisMorning = QaHarness.daysAgoAtMidday(0) - 5 * 3600
        QaHarness.seedLog(ctx, listOf(QaHarness.fiveKRunnerProfile()) + manual + QaHarness.gpsTenK(thisMorning, sessionRpe = 7))
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_history))
        waitForText(str(R.string.main_overline_last_7_days).uppercase(Locale.US))
    }

    @Test
    fun historyRuns() {
        seedRunsHistory()
        shot("history-runs")
        rule.onAllNodes(hasScrollToNodeAction()).onFirst().performScrollToNode(hasText("Fastest 10K"))
        rule.onAllNodesWithText("Fastest 10K").onFirst().performClick()
        rule.waitForNode(hasTestTag("run-trim"))
        rule.waitForIdle()
        shot("history-run-detail")
        rule.onNodeWithTag("run-trim").performScrollTo().performClick()
        rule.waitUntil(10_000) {
            rule.onAllNodes(SemanticsMatcher.keyIsDefined(SemanticsActions.SetProgress)).fetchSemanticsNodes().isNotEmpty()
        }
        rule.waitForIdle()
        shot("history-run-trim")
    }

    @Test
    fun historyLifts() {
        fun sets(daysAgo: Int, vararg spec: Triple<String, Double, Int>): List<Event> {
            var t = QaHarness.daysAgoAtMidday(daysAgo)
            return spec.flatMap { (ex, kg, reps) ->
                (0 until 3).map {
                    t += 180
                    Event.LogSet(ex, kg, reps, 8.0, t, t * 1000)
                }
            }
        }
        QaHarness.seedLog(
            ctx,
            listOf(QaHarness.strengthProfile()) +
                sets(15, Triple("Back Squat", 100.0, 5), Triple("Bench Press", 67.5, 8)) +
                sets(8, Triple("Back Squat", 105.0, 5), Triple("Bench Press", 70.0, 8)) +
                sets(1, Triple("Back Squat", 110.0, 5), Triple("Bench Press", 72.5, 8)),
        )
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_history))
        rule.waitForIdle()
        shot("history-lifts")
        val header = plural(R.plurals.history_exercise_count, 2, 2) + " · " + plural(R.plurals.history_set_count, 6, 6)
        // The list is the first scroller (two panes: the list pane).
        rule.onAllNodes(hasScrollToNodeAction()).onFirst().performScrollToNode(hasText(header, substring = true))
        rule.waitForIdle()
        rule.onAllNodesWithText(header).onFirst().performSemanticsAction(SemanticsActions.OnClick)
        waitForText(str(R.string.history_delete_session))
        shot("history-lift-session")
    }

    private fun hybridProfile(): Event = Event.SetProfile(
        progressionCadence = ProgressionCadence.WeekToWeek,
        liftGoal = LiftGoal.Hypertrophy,
        goalDistance = GoalDistance.TenK,
        concurrentGoal = ConcurrentGoal.Hypertrophy,
        weeklySets = 12,
        runningDaysPerWeek = 3,
        runningKmPerWeek = 30.0,
        advanced = false,
        bodyweightKg = 81.7,
        ageYears = 30.0,
    )

    @Test
    fun profileAndCalculators() {
        QaHarness.seedLog(ctx, listOf(hybridProfile(), Event.ComputeHrZones(30.0, null)))
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_profile))
        shot("profile")
        val row = hasText(str(R.string.main_tool_hr_title)) and hasClickAction()
        rule.onAllNodes(hasScrollToNodeAction()).onFirst().performScrollToNode(row)
        rule.onAllNodes(row).onFirst().performClick()
        rule.waitUntil(10_000) { rule.onAllNodes(hasTestTag("calc-result")).fetchSemanticsNodes().isNotEmpty() }
        rule.waitForIdle()
        shot("calc-hr-zones")
        rule.onNode(hasClickAction() and hasContentDescription(str(R.string.log_field_age), substring = true))
            .performScrollTo().performClick()
        rule.waitForNode(hasTestTag("numeric-keypad"))
        rule.waitForIdle()
        shot("calc-hr-zones-keypad")
    }

    @Test
    fun evidenceReferences() {
        QaHarness.seedLog(ctx, listOf(hybridProfile()))
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_profile))
        val row = hasText(str(R.string.main_evidence_references_title)) and hasClickAction()
        rule.onAllNodes(hasScrollToNodeAction()).onFirst().performScrollToNode(row)
        rule.onAllNodes(row).onFirst().performClick()
        waitForText(str(R.string.main_evidence_references_body))
        shot("evidence-references")
    }

    @Test
    fun setup() {
        rule.launchAppExpectingSetup()
        shot("setup-train")
        GuidedSetupWalk.walk(rule, stopAt = "days")
        shot("setup-days")
        GuidedSetupWalk.bothPath.dropWhile { it.name != "days" }.takeWhile { it.name != "review" }.forEach { screen ->
            screen.answer(rule)
            rule.waitForIdle()
            rule.onNode(hasText(str(R.string.setup_btn_next)) and hasClickAction()).performClick()
            rule.waitForIdle()
        }
        shot("setup-review")
    }

    @Test
    fun trackerPrestart() {
        QaHarness.seedLog(ctx, listOf(QaHarness.fiveKRunnerProfile()))
        rule.launchApp()
        openChooser()
        rule.onNodeWithText(str(R.string.main_track_run_title)).performScrollTo().performClick()
        waitForText(str(R.string.track_plan_follow))
        shot("track-prestart")
    }

    private fun openRecording() {
        QaHarness.seedLog(ctx, listOf(QaHarness.fiveKRunnerProfile()))
        QaHarness.seedRecordingRun()
        lastScenario = launchMain(openIntent(ctx, OpenDest.Track))
        rule.waitUntil(30_000) {
            rule.onAllNodesWithText(str(R.string.track_btn_stop)).fetchSemanticsNodes().isNotEmpty() && AppRuntime.bootHandled
        }
        rule.waitForIdle()
    }

    @Test
    fun trackerRecordingAndFinish() {
        openRecording()
        shot("track-recording")
        rule.onNodeWithText(str(R.string.track_btn_stop)).performClick()
        waitForText(str(R.string.track_finish_title))
        rule.pickRating("effort-scale", 7)
        // The word slot reads the pick, on the track or its large-font fallback grid.
        rule.onNode(hasText(str(R.string.history_effort_selected, 7, str(R.string.history_effort_anchor_7)))).assertExists()
        shot("track-finish")
    }

    /** With route maps on: the map frame (no tiles load under Robolectric) and the
     *  stat sheet under it, or beside it on a landscape window. */
    @Test
    fun trackerRecordingWithMap() {
        AppSettings.setShowRouteMaps(ctx, true)
        openRecording()
        shot("track-recording-map")
    }
}
