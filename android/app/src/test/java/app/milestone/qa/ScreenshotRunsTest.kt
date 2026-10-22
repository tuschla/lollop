package app.milestone.qa

import android.Manifest
import android.app.Application
import android.content.Context
import android.content.Intent
import android.location.LocationManager
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.hasAnyAncestor
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
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeUp
import androidx.test.core.app.ActivityScenario
import androidx.test.core.app.ApplicationProvider
import app.milestone.AppClock
import app.milestone.ConcurrentGoal
import app.milestone.DarkMode
import app.milestone.DistanceUnitOverride
import app.milestone.Event
import app.milestone.EventDispatcher
import app.milestone.GoalDistance
import app.milestone.GpsPoint
import app.milestone.LiftGoal
import app.milestone.MainActivity
import app.milestone.PrescriptionView
import app.milestone.ProgressionCadence
import app.milestone.R
import app.milestone.RecentRace
import app.milestone.RunSession
import app.milestone.RunStructureView
import app.milestone.RunTargetView
import app.milestone.AppSettings
import com.github.takahirom.roborazzi.captureScreenRoboImage
import java.util.Locale
import org.junit.After
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf

/**
 * Screenshot baselines for the run surfaces of the UX + coaching overhaul, light +
 * dark, captured from the REAL app over the real core (see [ScreenshotTest]):
 *
 *  - `runs-*`: History Runs (progress section: 16-week chart with the plan line,
 *    longest run, best efforts; a run card with its top record chip and effort),
 *    the run detail sheet (the EFFORT scale, run type, the rated state with its
 *    training load), History Lifts ("New best" session card + session sheet) and
 *    the manual run editor (h:mm:ss, bpm, effort).
 *  - `track-*`: the GPS tracker before Start (today's plan preview + "Follow
 *    today's plan"), recording a followed plan (step guide, Pace now / Avg), and
 *    the Finish review.
 *
 * Nothing here dispatches an event or saves a run: rated state is seeded, the run
 * editor and the Finish review are captured unsaved, so no write or save turn is
 * left on the serial consumer when the Activity closes.
 */
@RunWith(RobolectricTestRunner::class)
class ScreenshotRunsTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @Before
    fun setUp() {
        Locale.setDefault(Locale.US)
        QaHarness.reset(ctx)
        shadowOf(ctx as Application).grantPermissions(
            Manifest.permission.ACCESS_FINE_LOCATION,
            Manifest.permission.ACCESS_COARSE_LOCATION,
            Manifest.permission.POST_NOTIFICATIONS,
        )
        shadowOf(ctx.getSystemService(LocationManager::class.java)).setLocationEnabled(true)
        // No map tiles under Robolectric (and none wanted in a baseline): with route
        // maps off the tracker and the run detail render no map at all.
        AppSettings.setShowRouteMaps(ctx, false)
    }

    @After
    fun tearDown() {
        // Close the Activity first (its composition owns the tracker's scopes), then
        // drop every tracker singleton so no run state reaches the next test.
        QaHarness.teardown()
        RunSession.reset()
        AppSettings.distanceUnitOverride.value = DistanceUnitOverride.System
        AppSettings.showRouteMaps.value = true
    }

    private fun theme(mode: DarkMode) = AppSettings.setDarkMode(ctx, mode)

    private fun suffix(mode: DarkMode) = if (mode == DarkMode.Dark) "dark" else "light"

    @OptIn(com.github.takahirom.roborazzi.ExperimentalRoborazziApi::class)
    private fun shot(name: String) =
        rule.captureWithoutRipples { captureScreenRoboImage("src/test/screenshots/$name.png") }

    // ── Waits / drivers (every wait fetches semantics, which idles the looper) ──

    private fun waitFor(matcher: SemanticsMatcher, timeoutMs: Long = 10_000) {
        rule.waitUntil(timeoutMs) { rule.onAllNodes(matcher).fetchSemanticsNodes().isNotEmpty() }
        rule.waitForIdle()
    }

    private fun waitForText(text: String, substring: Boolean = false, timeoutMs: Long = 10_000) =
        waitFor(hasText(text, substring = substring), timeoutMs)

    /** A partially-expanded bottom sheet opened fully: an upward drag on its content
     *  is consumed by the sheet before its own scroll. */
    private fun expandSheetFrom(matcher: SemanticsMatcher) {
        rule.onAllNodes(matcher).onFirst().performTouchInput { swipeUp(startY = centerY, endY = centerY - 900f) }
        rule.waitForIdle()
    }

    private val logButton get() = hasClickAction() and hasContentDescription(str(R.string.main_log))

    private fun openChooser() {
        rule.onNode(logButton).performClick()
        waitForText(str(R.string.main_track_run_title))
    }

    /** The Records section's uppercase header. */
    private val records get() = str(R.string.history_records).uppercase(Locale.US)

    /** The session card header, e.g. "3 exercises · 8 sets". */
    private fun sessionHeader(exercises: Int, sets: Int) =
        plural(R.plurals.history_exercise_count, exercises, exercises) + " · " +
            plural(R.plurals.history_set_count, sets, sets)

    private fun key(k: String) =
        rule.onNode(hasText(k) and hasAnyAncestor(hasTestTag("numeric-keypad"))).performScrollTo().performClick()

    // ── Scenario seeds ─────────────────────────────────────────────────────────

    /** A 5K runner (4 days, 30 km a week planned) with a fresh 5K race as pace
     *  anchor and an age for heart-rate targets. Monday (the pinned today) is an
     *  easy run in the generated plan. */
    private fun runnerProfile(): Event = Event.SetProfile(
        progressionCadence = ProgressionCadence.WeekToWeek,
        liftGoal = LiftGoal.MaxStrength,
        goalDistance = GoalDistance.FiveK,
        concurrentGoal = ConcurrentGoal.EndurancePriority,
        weeklySets = 0,
        runningDaysPerWeek = 4,
        runningKmPerWeek = 30.0,
        advanced = false,
        bodyweightKg = 70.0,
        ageYears = 30.0,
        recentRace = RecentRace(distance_km = 5.0, time_sec = 1500, epoch_day = app.milestone.todayEpochDay() - 7),
    )

    /** A 10.1 km GPS run at an even 5:00 /km (one clean fix every 3 s, 10 m apart). */
    private fun gpsTenK(observedAt: Long, sessionRpe: Int?): Event {
        val metresPerDegLat = 111_195.0
        val points = (0 until 1011).map { i ->
            GpsPoint(lat = 47.0 + i * 10.0 / metresPerDegLat, lon = 8.0, observedAt = observedAt + i * 3L, accuracyM = 5.0)
        }
        return Event.LogRunTrack(
            points = points,
            hrPctMax = 0.0,
            longestRecentKm = 0.0,
            observedAt = observedAt,
            entryId = observedAt * 1000,
            sessionRpe = sessionRpe,
        )
    }

    /**
     * Twelve weeks of three hand-logged runs (easy, short, long) plus a half
     * marathon five weeks back, oldest first, one squat session three days ago (so
     * the Lifts | Runs segment shows), and this morning's rated GPS 10K. The core
     * then reports: a 16-week chart against the 30 km plan line, longest run
     * 21.2 km, best efforts 1K / 5K (inside the 10K) / 10K / Half marathon, and
     * Monday's card (untagged, so titled by its weekday) with "Effort 7/10" and the
     * Fastest 10K chip. Runs is the default segment (the run is newest).
     */
    private fun seedRunsHistory() {
        // (stamp, event) pairs so the mixed runs + sets replay oldest first.
        val manual = buildList<Pair<Long, Event>> {
            for (w in 12 downTo 1) {
                for (off in intArrayOf(6, 4, 2)) {
                    val km = when (off) {
                        2 -> 6.0 + w % 3
                        4 -> 4.5 + w % 2
                        else -> 11.0 + w % 4
                    }
                    val at = QaHarness.daysAgoAtMidday(7 * (w - 1) + off)
                    add(at to Event.LogRun(km, km * 5.8, 0.0, 0.0, at, at * 1000))
                }
            }
            val half = QaHarness.daysAgoAtMidday(33) + 600
            add(half to Event.LogRun(21.2, 115.0, 0.0, 0.0, half, half * 1000))
            var t = QaHarness.daysAgoAtMidday(3)
            repeat(3) {
                t += 180
                add(t to Event.LogSet("Back Squat", 100.0, 5, 8.0, t, t * 1000))
            }
        }.sortedBy { it.first }.map { it.second }
        // Today 07:00, before the pinned 10:00 "now".
        val thisMorning = QaHarness.daysAgoAtMidday(0) - 5 * 3600
        QaHarness.seedLog(ctx, listOf(runnerProfile()) + manual + gpsTenK(thisMorning, sessionRpe = 7))
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_history))
        waitForText(str(R.string.main_overline_last_7_days).uppercase(Locale.US))
    }

    /** Three squat/bench sessions with rising loads; yesterday's also adds rows, so
     *  its card reads "3 exercises · 8 sets" with "New best" on squat and bench. */
    private fun seedLiftHistory() {
        fun sets(daysAgo: Int, vararg spec: Triple<String, Double, Int>, count: (String) -> Int): List<Event> {
            var t = QaHarness.daysAgoAtMidday(daysAgo)
            return spec.flatMap { (ex, kg, reps) ->
                (0 until count(ex)).map {
                    t += 180
                    Event.LogSet(ex, kg, reps, 8.0, t, t * 1000)
                }
            }
        }
        val events = listOf(QaHarness.strengthProfile()) +
            sets(15, Triple("Back Squat", 100.0, 5), Triple("Bench Press", 67.5, 8)) { 3 } +
            sets(8, Triple("Back Squat", 105.0, 5), Triple("Bench Press", 70.0, 8)) { 3 } +
            sets(1, Triple("Back Squat", 110.0, 5), Triple("Bench Press", 72.5, 8), Triple("Barbell Row", 60.0, 10)) {
                if (it == "Barbell Row") 2 else 3
            }
        QaHarness.seedLog(ctx, events)
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_history))
        rule.scrollToText(sessionHeader(3, 8))
    }

    /** A live recording: 21 fixes, ~30 m every 10 s (600 m, 200 s moving). The fix
     *  stamps sit far ahead of Robolectric's elapsed clock, so the GPS pill reads a
     *  steady "Recording" (never a decaying lock or a signal loss). */
    private fun seedRecordingRun() {
        val t0 = AppClock.nowSec() - 400
        for (i in 0..20) {
            RunSession.add(
                GpsPoint(52.5200 + i * 0.00027, 13.4050, t0 + i * 10L, 5.0),
                (1_000L + i * 10L) * 1_000_000_000L,
            )
        }
        RunSession.tracking.value = true
    }

    /** Reopen the tracker onto a followed interval plan mid-run (rep 1 of 6). */
    private fun openRecordingWithPlan() {
        QaHarness.seedLog(ctx, listOf(runnerProfile()))
        RunSession.plannedRun.value = PrescriptionView(
            exercise = "Intervals",
            summary = "6 × 800 m · 400 m jog between",
            run_head = "6 × 800 m",
            face_lead = "6 × 800 m",
            run_target = RunTargetView(
                band = "interval",
                pace_fast_sec_per_km = 250,
                pace_slow_sec_per_km = 260,
                pace_label_km = "4:10–4:20 /km",
                pace_label_mi = "6:43–6:59 /mi",
                hr_label = "170–181 bpm",
                effort_cue = "effort 8–9 of 10",
                effort_detail = "8–9 of 10",
            ),
            run_structure = RunStructureView(reps = 6, rep_m = 800, recovery_m = 400),
        )
        seedRecordingRun()
        RunSession.startSteps(android.os.SystemClock.elapsedRealtime())
        lastScenario = ActivityScenario.launch(
            Intent(ctx, MainActivity::class.java).putExtra(MainActivity.EXTRA_OPEN_TRACKING, true),
        )
        rule.waitUntil(30_000) {
            rule.onAllNodesWithText(str(R.string.track_step_skip)).fetchSemanticsNodes().isNotEmpty() && EventDispatcher.bootHandled
        }
        rule.waitForIdle()
    }

    /** Stop → the Finish review, effort 7 picked (local state only; nothing saved). */
    private fun openFinishReview() {
        openRecordingWithPlan()
        rule.onNodeWithText(str(R.string.track_btn_stop)).performClick()
        waitForText(str(R.string.track_finish_title))
        rule.pickRating("effort-scale", 7)
    }

    // ── History · Runs: progress section + run card ────────────────────────────

    private fun runsProgress(mode: DarkMode) {
        theme(mode)
        seedRunsHistory()
        val s = suffix(mode)
        // Top of the Runs tab: the 7-day strip and the 12-week heatmap (all
        // activity), then the Lifts | Runs segment (Runs picked) and Progress.
        shot("runs-progress-top-$s")
        // The progress block switched to the 14-day daily chart.
        val daily = str(R.string.history_progress_days, 14)
        rule.scrollToText(daily)
        rule.onNodeWithText(daily).performClick()
        rule.waitForIdle()
        shot("runs-progress-daily-$s")
        // Records: the carded 2×2 grid, longest run + best efforts down to the half
        // marathon (the best-effort labels are the core's own copy).
        rule.scrollToText(records)
        rule.scrollToText("Half marathon".uppercase(Locale.US))
        shot("runs-progress-records-$s")
        // The lower Runs tab, scrolled down to today's card, so the first month header
        // (with its jump-to-month calendar button) and the newest run card share the
        // screen.
        rule.scrollToText("Fastest 10K")
        shot("runs-progress-bottom-$s")
        // The run list itself: today's card (weekday title, Effort 7/10, the Fastest
        // 10K chip) and the next ones.
        rule.scrollToText(str(R.string.history_run_weekday, "Saturday"))
        shot("runs-card-$s")
    }

    @Test fun runsProgressLight() = runsProgress(DarkMode.Light)

    @Test fun runsProgressDark() = runsProgress(DarkMode.Dark)

    // ── Run detail: rated GPS 10K (headline stats, route card, splits, YOUR LOG) ──

    private fun openRatedRunDetail() {
        seedRunsHistory()
        rule.scrollToText("Fastest 10K")
        rule.onAllNodesWithText("Fastest 10K").onFirst().performClick()
        // Rated: YOUR LOG's Effort row reads "7 of 10 · Very hard".
        waitForText(str(R.string.run_effort_value, 7, str(R.string.history_effort_anchor_7)))
        // The GPS route parses off the main thread (Trim appears once it has); wait
        // so the sheet's height is final before capturing.
        waitFor(hasTestTag("run-trim"))
    }

    private fun runsDetailRated(mode: DarkMode) {
        theme(mode)
        openRatedRunDetail()
        val s = suffix(mode)
        // First screen: title + record badge, start date, the 3-up stats and the whole
        // route card (maps off: the route, markers and labels on the plain card).
        shot("runs-detail-rated-top-$s")
        // Splits with the delta column, then YOUR LOG.
        rule.onNodeWithTag("run-log-type").performScrollTo()
        shot("runs-detail-rated-splits-$s")
        // The training-load "?" opened inside the YOUR LOG card.
        val trainingLoad = hasContentDescription(str(R.string.history_training_load_subject), substring = true) and hasClickAction()
        rule.onNode(trainingLoad).performScrollTo().performClick()
        rule.waitForIdle()
        shot("runs-detail-rated-why-$s")
        rule.onNode(trainingLoad).performClick()
        rule.waitForIdle()
        // Bottom: the action rows (Export GPX, Trim start or end, Delete run).
        rule.onNodeWithTag("run-delete").performScrollTo()
        shot("runs-detail-rated-$s")
        // Delete run confirms first.
        rule.onNodeWithTag("run-delete").performClick()
        rule.waitForIdle()
        shot("runs-detail-delete-confirm-$s")
        rule.onNode(hasText(str(R.string.main_cancel)) and hasClickAction()).performClick()
        rule.waitForIdle()
        // Rate run: the effort scale (7 picked) and the run type chips over the sheet.
        rule.onNodeWithTag("run-log-effort").performScrollTo().performClick()
        rule.waitForIdle()
        shot("runs-detail-rate-sheet-$s")
    }

    @Test fun runsDetailRatedLight() = runsDetailRated(DarkMode.Light)

    @Test fun runsDetailRatedDark() = runsDetailRated(DarkMode.Dark)

    @Test fun runsDetailRatedFontScale130() {
        RuntimeEnvironment.setFontScale(1.3f)
        theme(DarkMode.Light)
        openRatedRunDetail()
        shot("runs-detail-rated-top-font130-light")
        // The splits (delta column, the partial split's distance) readable at 130%.
        rule.onNodeWithTag("run-log-type").performScrollTo()
        shot("runs-detail-rated-font130-light")
    }

    // ── Full-screen route map and the trim sheet ───────────────────────────────

    private fun runsRouteFull(mode: DarkMode) {
        theme(mode)
        openRatedRunDetail()
        val s = suffix(mode)
        rule.onNode(hasContentDescription(str(R.string.runmap_open_full)) and hasClickAction()).performClick()
        waitFor(hasContentDescription(str(R.string.runmap_fit_route)))
        shot("runs-route-full-$s")
        rule.onNode(hasContentDescription(str(R.string.runmap_close)) and hasClickAction()).performClick()
        rule.waitForIdle()
        rule.onNodeWithTag("run-trim").performScrollTo().performClick()
        waitFor(hasText(str(R.string.trim_hint)))
        shot("runs-trim-$s")
    }

    @Test fun runsRouteFullLight() = runsRouteFull(DarkMode.Light)

    @Test fun runsRouteFullDark() = runsRouteFull(DarkMode.Dark)

    // ── Run detail: an unrated hand-logged run (no map, Rate this run) ────────

    private fun runsDetailRate(mode: DarkMode) {
        theme(mode)
        seedRunsHistory()
        // The newest hand-logged run (Saturday, unrated, untagged).
        val saturdayRun = str(R.string.history_run_weekday, "Saturday")
        rule.scrollToText(saturdayRun)
        rule.onAllNodesWithText(saturdayRun).onFirst().performClick()
        waitFor(hasTestTag("run-log-details"))
        shot("runs-detail-rate-${suffix(mode)}")
    }

    @Test fun runsDetailRateLight() = runsDetailRate(DarkMode.Light)

    @Test fun runsDetailRateDark() = runsDetailRate(DarkMode.Dark)

    // ── History · Lifts: "New best" session card + session sheet ───────────────

    private fun runsLifts(mode: DarkMode) {
        theme(mode)
        seedLiftHistory()
        val s = suffix(mode)
        shot("runs-lifts-card-$s")
        rule.onNodeWithText(sessionHeader(3, 8)).performClick()
        waitForText(str(R.string.history_delete_session))
        expandSheetFrom(hasTestTag("lift-set-row"))
        // Per-exercise set rows; "New best" once beside each record exercise's name.
        shot("runs-lifts-sheet-top-$s")
        // Scrolled to the sheet's end, so its bottom tail below Delete session shows.
        rule.onNodeWithText(str(R.string.history_delete_session)).performScrollTo()
        rule.onNodeWithText(str(R.string.history_delete_session)).performTouchInput { swipeUp() }
        rule.waitForIdle()
        shot("runs-lifts-sheet-bottom-$s")
        // A set opened from the table: the set as the sheet's largest figure, its
        // e1RM / load card, the cross-check and the Edit set / Delete set rows.
        rule.onAllNodes(hasTestTag("lift-set-row")).onFirst().performScrollTo().performClick()
        waitFor(hasTestTag("set-detail-delete"))
        shot("runs-lifts-set-detail-$s")
    }

    @Test fun runsLiftsLight() = runsLifts(DarkMode.Light)

    @Test fun runsLiftsDark() = runsLifts(DarkMode.Dark)

    // ── Manual run editor: h:mm:ss, bpm, effort ────────────────────────────────

    private fun runsEditor(mode: DarkMode) {
        theme(mode)
        QaHarness.seedLog(ctx, listOf(runnerProfile()))
        rule.launchApp()
        openChooser()
        rule.onNodeWithText(str(R.string.main_log_run_title)).performScrollTo().performClick()
        waitFor(hasContentDescription(str(R.string.log_field_duration)))
        val s = suffix(mode)
        // Distance 6.2 mi, duration typed timer-style: 1:05:30.
        listOf("6", ".", "2").forEach { key(it) }
        rule.onNode(hasContentDescription(str(R.string.log_field_duration))).performScrollTo().performClick()
        listOf("1", "0", "5", "3", "0").forEach { key(it) }
        shot("runs-editor-top-$s")
        // Average heart rate (bpm offered first, age known): 152 typed.
        rule.onNode(hasContentDescription(str(R.string.track_log_hr_avg))).performScrollTo().performClick()
        listOf("1", "5", "2").forEach { key(it) }
        shot("runs-editor-mid-$s")
        // Whole-run effort 6 picked, run-type chips below.
        rule.pickRating("effort-scale", 6)
        rule.onNodeWithText("Recovery").performScrollTo()
        shot("runs-editor-bottom-$s")
    }

    @Test fun runsEditorLight() = runsEditor(DarkMode.Light)

    @Test fun runsEditorDark() = runsEditor(DarkMode.Dark)

    // ── Tracker before Start: today's plan preview + Follow today's plan ───────

    private fun trackPrestart(mode: DarkMode) {
        theme(mode)
        QaHarness.seedLog(ctx, listOf(runnerProfile()))
        rule.launchApp()
        openChooser()
        rule.onNodeWithText(str(R.string.main_track_run_title)).performScrollTo().performClick()
        waitForText(str(R.string.track_plan_follow))
        waitForText(strPrefix(R.string.track_plan_title), substring = true)
        shot("track-prestart-${suffix(mode)}")
    }

    @Test fun trackPrestartLight() = trackPrestart(DarkMode.Light)

    @Test fun trackPrestartDark() = trackPrestart(DarkMode.Dark)

    // ── Tracker recording a followed plan: step guide + Pace now / Avg ─────────

    private fun trackRecording(mode: DarkMode) {
        theme(mode)
        openRecordingWithPlan()
        shot("track-recording-${suffix(mode)}")
    }

    @Test fun trackRecordingLight() = trackRecording(DarkMode.Light)

    @Test fun trackRecordingDark() = trackRecording(DarkMode.Dark)

    // ── Finish review: summary, effort cells, run type, Discard/Resume/Save ────

    private fun trackFinish(mode: DarkMode) {
        theme(mode)
        openFinishReview()
        val s = suffix(mode)
        shot("track-finish-$s")
        // The plan-preselected Interval changed to Recovery (local state only), with
        // the action row pinned to the panel's bottom edge.
        rule.onNodeWithText("Recovery").performScrollTo().performClick()
        rule.waitForIdle()
        shot("track-finish-bottom-$s")
    }

    @Test fun trackFinishLight() = trackFinish(DarkMode.Light)

    @Test fun trackFinishDark() = trackFinish(DarkMode.Dark)

    @Test fun trackFinishFontScale130() {
        RuntimeEnvironment.setFontScale(1.3f)
        theme(DarkMode.Light)
        openFinishReview()
        shot("track-finish-font130-light")
    }
}
