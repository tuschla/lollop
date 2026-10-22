package app.milestone.qa

import android.Manifest
import android.app.Application
import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.os.Parcel
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasScrollToNodeAction
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isDialog
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.isSelectable
import androidx.compose.ui.test.junit4.ComposeTestRule
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performSemanticsAction
import androidx.test.core.app.ActivityScenario
import androidx.test.core.app.ApplicationProvider
import androidx.test.platform.app.InstrumentationRegistry
import app.milestone.ActiveRunStore
import app.milestone.AppClock
import app.milestone.AppSettings
import app.milestone.AppTheme
import app.milestone.ConcurrentGoal
import app.milestone.Core
import app.milestone.DarkMode
import app.milestone.DistanceUnit
import app.milestone.DistanceUnitOverride
import app.milestone.Event
import app.milestone.EventDispatcher
import app.milestone.EventLog
import app.milestone.GoalDistance
import app.milestone.GpsPoint
import app.milestone.HistoryFocus
import app.milestone.LiftGoal
import app.milestone.LiftResultView
import app.milestone.MainActivity
import app.milestone.ProgressionCadence
import app.milestone.R
import app.milestone.RecentRace
import app.milestone.RunSession
import app.milestone.TRACK_DECIMATION_CAP
import app.milestone.UiMessages
import app.milestone.runDistLabel
import app.milestone.segmentKm
import app.milestone.todayEpochDay
import app.milestone.utcOffsetSec
import java.io.File
import java.util.Locale
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.cos
import kotlin.math.sin
import kotlin.random.Random
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Ignore
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.android.controller.ActivityController

/**
 * Process death for the offline scenario suite. Android kills a backgrounded app's
 * process and later starts a NEW one on the same disk: every singleton (the native
 * core, the dispatcher's model, the run session, in-memory settings) is gone, while
 * the event log, the run sidecar and SharedPreferences remain, and, when the user
 * comes back from Recents, the Activity gets its saved-instance Bundle back.
 *
 * [kill] wipes exactly the process-scoped state (unlike [QaHarness.reset], it keeps
 * every file and preference). [saveState] captures the Bundle the OS would keep,
 * round-tripped through a Parcel as across a real process boundary;
 * [relaunchWithState] starts the new process's Activity from it.
 */
internal object ProcessDeath {
    private var restored: ActivityController<MainActivity>? = null

    /** Close any Activity and drop every process-scoped singleton, keeping disk. */
    fun kill(ctx: Context) {
        QaHarness.teardown()
        restored?.let { c -> runCatching { c.pause().stop().destroy() } }
        restored = null
        // The sidecar's queued appends belong to the dying process's last instants.
        ActiveRunStore.flush()
        Core.resetForTest()
        for ((field, value) in listOf(
            "restored" to false, "log" to null, "lastGoodView" to null, "appendTornGuard" to false,
            "lastSetToday" to null, "viewStale" to false, "logDegraded" to false,
        )) {
            Core.javaClass.getDeclaredField(field).apply { isAccessible = true }.set(Core, value)
        }
        EventDispatcher.model.value = null
        EventDispatcher.bootHandled = false
        EventDispatcher.recoveredSidecar = null
        RunSession.reset()
        UiMessages.clear()
        HistoryFocus.consume()
        // In-memory settings fall back to their initializers; onCreate reloads prefs.
        AppSettings.theme.value = AppTheme.Paper
        AppSettings.dynamicAccent.value = true
        AppSettings.darkMode.value = DarkMode.System
        AppSettings.distanceUnitOverride.value = DistanceUnitOverride.System
        AppSettings.showRouteMaps.value = true
    }

    /** The current Activity's saved-instance state as the OS keeps it for a killed
     *  task, after a Parcel round trip, and its parcelled size in bytes. */
    fun saveState(): Pair<Bundle, Int> {
        val out = Bundle()
        val act = checkNotNull(lastScenario) { "no launched Activity" }
        act.onActivity { InstrumentationRegistry.getInstrumentation().callActivityOnSaveInstanceState(it, out) }
        val p = Parcel.obtain()
        try {
            out.writeToParcel(p, 0)
            val size = p.dataSize()
            p.setDataPosition(0)
            val back = Bundle.CREATOR.createFromParcel(p)
            back.classLoader = MainActivity::class.java.classLoader
            return back to size
        } finally {
            p.recycle()
        }
    }

    /** Start the new process's Activity from [state] (back from Recents) and wait
     *  for boot hydration. */
    fun relaunchWithState(rule: ComposeTestRule, state: Bundle) {
        restored = Robolectric.buildActivity(MainActivity::class.java).setup(state)
        rule.waitUntil(30_000) {
            rule.onAllNodes(isRoot()).fetchSemanticsNodes().isNotEmpty() && EventDispatcher.bootHandled
        }
        rule.waitForIdle()
    }

    /** Close an Activity started by [relaunchWithState] at the end of a test. */
    fun teardown() {
        restored?.let { c -> runCatching { c.pause().stop().destroy() } }
        restored = null
    }
}

/**
 * Activity recreation (rotation, theme/locale change) and process death in the
 * middle of the app's stateful flows, against the design rule that a config change
 * mid-entry keeps the open sheet and its fields (AppScaffold: "a rotation mid-entry
 * keeps the sheet OPEN on the same editor (the editors' own fields are already
 * rememberSaveable)") and the event-log rule that only COMMITTED entries persist.
 *
 * Every scenario checks: nothing committed is lost, nothing is logged twice, the
 * app doesn't crash, the restored screen is the one the user left (or, after a
 * cold start without saved state, a sensible home), and a half-filled draft keeps
 * its values across recreation. The run-session part replays a recording run's
 * crash sidecar ([ActiveRunStore]) through [RunSession.restore] and checks the
 * resumed run neither loses fixes nor double-counts distance or moving time.
 */
@Ignore("PARKED: written but never run (owner parked the work before the first gradle run); verify, then BUG-ignore real failures")
@RunWith(RobolectricTestRunner::class)
class RestoreRoundtripTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @Before
    fun setUp() {
        Locale.setDefault(Locale.US)
        QaHarness.reset(ctx)
        clearSidecar()
        RunSession.reset()
        shadowOf(ctx as Application).grantPermissions(
            Manifest.permission.ACCESS_FINE_LOCATION,
            Manifest.permission.ACCESS_COARSE_LOCATION,
            Manifest.permission.POST_NOTIFICATIONS,
        )
        // No map tiles under Robolectric.
        AppSettings.setShowRouteMaps(ctx, false)
    }

    @After
    fun tearDown() {
        QaHarness.teardown()
        ProcessDeath.teardown()
        RunSession.reset()
        clearSidecar()
        AppSettings.distanceUnitOverride.value = DistanceUnitOverride.System
        AppSettings.showRouteMaps.value = true
        AppSettings.theme.value = AppTheme.Paper
        AppSettings.darkMode.value = DarkMode.System
    }

    private fun clearSidecar() {
        ActiveRunStore.init(ctx)
        ActiveRunStore.clearSync()
        File(ctx.filesDir, "active-run.ndjson").delete()
    }

    // ── Shared drivers ──────────────────────────────────────────────────────────

    private fun recreate() {
        checkNotNull(lastScenario).recreate()
        rule.waitUntil(15_000) { rule.onAllNodes(isRoot()).fetchSemanticsNodes().isNotEmpty() }
        rule.waitForIdle()
    }

    private fun waitForState(timeoutMs: Long = 15_000, cond: () -> Boolean) =
        rule.waitUntil(timeoutMs) {
            rule.onAllNodes(isRoot()).fetchSemanticsNodes()
            cond()
        }

    private fun waitForText(text: String, substring: Boolean = false, timeoutMs: Long = 15_000) =
        rule.waitUntil(timeoutMs) { rule.onAllNodes(hasText(text, substring = substring)).fetchSemanticsNodes().isNotEmpty() }

    private fun waitForTag(tag: String, timeoutMs: Long = 15_000) =
        rule.waitUntil(timeoutMs) { rule.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty() }

    private fun count(m: SemanticsMatcher) = rule.onAllNodes(m).fetchSemanticsNodes().size

    private fun logLines(): List<String> =
        File(ctx.filesDir, "event-log.ndjson").takeIf { it.exists() }?.readLines()?.filter { it.isNotBlank() }.orEmpty()

    private fun logCount(variant: String) = logLines().count { EventLog.variantOf(it) == variant }

    private fun payload(line: String, variant: String) = Json.parseToJsonElement(line).jsonObject[variant]!!.jsonObject

    private val model get() = checkNotNull(EventDispatcher.model.value)

    /** A [app.milestone.RatingScale]'s spoken state ("3 of 5 …" / "Not rated"). */
    private fun ratingState(tag: String): String? =
        rule.onAllNodesWithTag(tag).fetchSemanticsNodes()
            .firstNotNullOfOrNull { it.config.getOrNull(SemanticsProperties.StateDescription) }

    private fun assertSavedStateFitsABinderTransaction(size: Int) {
        // The whole saved state rides one Binder transaction (1 MB, shared by the
        // process); Android recommends keeping it to tens of KB.
        assertTrue("saved instance state is $size bytes", size < 200_000)
    }

    // ═══ 1. A half-filled set in the gym loop ═══════════════════════════════════

    private val todayStartSec get() = QaHarness.daysAgoAtMidday(0) - 43_200L

    private fun todaysSets(exercise: String): List<LiftResultView> =
        model.lifts.filter { it.exercise == exercise && it.observed_at >= todayStartSec }

    private fun openLoopFor(exercise: String) {
        rule.onNode(
            hasClickAction() and hasContentDescription(strPrefix(R.string.plan_log_set_n_of_n, 1), substring = true) and
                hasAnyAncestor(hasTestTag("plan-item:$exercise")),
        ).performScrollTo().performClick()
        waitForText(strPrefix(R.string.log_set_header_n_of_n, 1), substring = true)
    }

    private fun weightWell(shown: String) = hasContentDescription("${str(R.string.log_field_weight)}, $shown kg")

    @Test
    fun aHalfFilledSetSurvivesRotationAndProcessDeathAndIsLoggedExactlyOnce() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(1))))
        rule.launchApp()
        openLoopFor("Back Squat")
        rule.typeWeight("62.5")
        rule.onNode(weightWell("62.5")).assertExists()

        // Rotation with the typed weight not yet saved: the loop and the draft stay.
        recreate()
        rule.assertCountEventually(weightWell("62.5"), 1)
        rule.assertTextCountEventually(str(R.string.log_set_header_n_of_n, 1, 4), 1)
        assertEquals("a draft is never logged by a rotation", 0, todaysSets("Back Squat").size)

        // Save, and rotate before the core has even answered.
        rule.onNodeWithTag("editor-primary").performClick()
        recreate()
        waitForState { todaysSets("Back Squat").size == 1 }
        rule.assertTextCountEventually(str(R.string.log_set_header_n_of_n, 2, 4), 1)
        assertEquals(listOf(62.5), todaysSets("Back Squat").map { it.weight_kg })
        assertEquals("seeded + one", 2, logCount("LogSet"))

        // Backgrounded, killed, restored from Recents: the loop comes back on set 2.
        val (state, size) = ProcessDeath.saveState()
        assertSavedStateFitsABinderTransaction(size)
        ProcessDeath.kill(ctx)
        ProcessDeath.relaunchWithState(rule, state)
        waitForState { todaysSets("Back Squat").size == 1 }
        assertEquals(0, count(hasText(str(R.string.log_set_header_n_of_n, 1, 4))))
        assertEquals(listOf(62.5), todaysSets("Back Squat").map { it.weight_kg })

        // A cold start (no saved state): Today, the set replayed once, no editor.
        ProcessDeath.kill(ctx)
        rule.launchApp()
        assertEquals(listOf(62.5), todaysSets("Back Squat").map { it.weight_kg })
        assertEquals(0, count(hasText(strPrefix(R.string.log_set_header_n_of_n, 1), substring = true)))
        assertEquals(2, logCount("LogSet"))
    }

    // ═══ 2. The morning check-in ════════════════════════════════════════════════

    private fun openMorningCheckin() {
        rule.onNodeWithContentDescription(str(R.string.main_log)).performClick()
        rule.onNodeWithText(str(R.string.main_morning_checkin_title)).performScrollTo().performClick()
        waitForTag("checkin-sleep")
    }

    @Test
    fun aHalfAnsweredMorningCheckinKeepsItsAnswersAcrossRotationAndSubmitsOnce() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        openMorningCheckin()
        rule.pickRating("checkin-sleep", 2)
        rule.pickRating("checkin-soreness", 4)
        val sleep = ratingState("checkin-sleep")
        val soreness = ratingState("checkin-soreness")
        assertNotNull(sleep)

        recreate()
        waitForTag("checkin-sleep")
        assertEquals("sleep answer kept", sleep, ratingState("checkin-sleep"))
        assertEquals("soreness answer kept", soreness, ratingState("checkin-soreness"))
        assertEquals("nothing submitted by a rotation", 0, logCount("SubmitCheckin"))

        rule.pickRating("checkin-mood", 3)
        rule.onNodeWithTag("editor-primary").performClick()
        waitForState { logCount("SubmitCheckin") == 1 && model.checked_in_today }
        recreate()
        waitForState { EventDispatcher.model.value != null }
        assertEquals("one check-in after a rotation", 1, logCount("SubmitCheckin"))

        ProcessDeath.kill(ctx)
        rule.launchApp()
        assertTrue(model.checked_in_today)
        val echo = checkNotNull(model.checkin_today)
        assertEquals(2, echo.sleep_quality)
        assertEquals(4, echo.soreness)
        assertEquals(3, echo.mood)
        assertEquals(1, logCount("SubmitCheckin"))
    }

    // ═══ 3. The run Finish review ═══════════════════════════════════════════════

    /** 21 fixes, ~30 m every 10 s (600 m, 200 s), recording, as the service leaves them. */
    private fun seedRecordingRun() {
        val t0 = AppClock.nowSec() - 400
        for (i in 0..20) {
            RunSession.add(GpsPoint(52.5200 + i * 0.00027, 13.4050, t0 + i * 10L, 5.0), (1_000L + i * 10L) * 1_000_000_000L)
        }
        RunSession.tracking.value = true
    }

    private fun launchOnTracker() {
        lastScenario = ActivityScenario.launch(
            Intent(ctx, MainActivity::class.java).putExtra(MainActivity.EXTRA_OPEN_TRACKING, true),
        )
        waitForText(str(R.string.track_btn_stop), timeoutMs = 30_000)
    }

    private fun stopToFinishReview() {
        rule.onNodeWithText(str(R.string.track_btn_stop)).performClick()
        waitForText(str(R.string.track_finish_title))
    }

    /** Save the run as the tracker's Save does, standing in for the service's stop
     *  confirmation (no service runs under Robolectric), and wait for the whole save. */
    private fun saveRunAndAwait() {
        rule.mainClock.autoAdvance = false
        try {
            rule.onNodeWithText(str(R.string.track_btn_save_run)).performClick()
            RunSession.tracking.value = false
        } finally {
            rule.mainClock.autoAdvance = true
        }
        waitForState { logCount("LogRunTrack") >= 1 && !RunSession.saving.value && RunSession.points.value.isEmpty() }
    }

    @Test
    fun theFinishReviewSurvivesRotationWithItsRatingAndTheRunSavesOnce() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        seedRecordingRun()
        launchOnTracker()
        stopToFinishReview()
        rule.pickRating("effort-scale", 7)
        rule.onNodeWithText("Interval").performScrollTo().performClick()
        val effort = ratingState("effort-scale")

        recreate()
        waitForText(str(R.string.track_finish_title))
        assertEquals("the effort answer is kept", effort, ratingState("effort-scale"))
        rule.onNodeWithText("Interval").performScrollTo().assertIsSelected()
        assertEquals("the captured track is untouched", 21, RunSession.points.value.size)
        assertEquals(0, logCount("LogRunTrack"))

        saveRunAndAwait()
        recreate()
        waitForState { model.runs.size == 1 }
        assertEquals("one run after a rotation", 1, logCount("LogRunTrack"))
        val saved = payload(logLines().single { EventLog.variantOf(it) == "LogRunTrack" }, "LogRunTrack")
        assertEquals(7, saved["session_rpe"]!!.jsonPrimitive.int)
        assertEquals("Interval", saved["workout_type"]!!.jsonPrimitive.content)
        assertEquals(0, count(hasText(str(R.string.track_finish_title))))

        ProcessDeath.kill(ctx)
        rule.launchApp()
        assertEquals(1, model.runs.size)
        assertEquals(7, model.runs.single().session_rpe)
        assertEquals("no recovery offered for a saved run", 0, count(hasText(str(R.string.main_resume_run_title))))
    }

    // ═══ 4 + 6. An open run detail sheet, its rate sheet and the trim sheet ═════

    /** 10.1 km at an even 5:00 /km (1011 fixes, 10 m every 3 s) ending at 07:00. */
    private fun gpsTenK(): Event.LogRunTrack {
        val end = QaHarness.daysAgoAtMidday(0) - 5 * 3600
        val start = end - 1010 * 3L
        val points = (0 until 1011).map { i ->
            GpsPoint(lat = 47.0 + i * 10.0 / 111_195.0, lon = 8.0, observedAt = start + i * 3L, accuracyM = 5.0)
        }
        return Event.LogRunTrack(points = points, hrPctMax = 0.0, longestRecentKm = 0.0, observedAt = end, entryId = end * 1000)
    }

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
        recentRace = RecentRace(distance_km = 5.0, time_sec = 1500, epoch_day = todayEpochDay() - 7),
    )

    /** Seed a runner with [run] (plus an older hand-logged run) and open its sheet
     *  the way the app's own "View" action does. */
    private fun openRunDetail(run: Event.LogRunTrack) {
        val older = QaHarness.daysAgoAtMidday(3)
        QaHarness.seedLog(ctx, listOf(runnerProfile(), Event.LogRun(6.0, 33.0, 0.0, 0.0, older, older * 1000), run))
        rule.launchApp()
        HistoryFocus.request(run.entryId, run.observedAt)
        waitForTag("run-trim")
    }

    @Test
    fun anOpenRunDetailAndItsRateSheetComeBackAfterRotationAndProcessDeath() {
        val run = gpsTenK()
        openRunDetail(run)
        recreate()
        waitForTag("run-trim")
        assertEquals("no run doubled", 2, model.runs.size)

        // Rate it: pick 6, rotate with the rate sheet open, then save.
        rule.onNodeWithTag("run-log-effort").performScrollTo().performClick()
        waitForTag("run-rate-save")
        rule.pickRating("effort-scale", 6)
        val picked = ratingState("effort-scale")
        recreate()
        waitForTag("run-rate-save")
        assertEquals("the unsaved rating is kept", picked, ratingState("effort-scale"))
        assertEquals(0, logCount("AmendRunMeta"))
        rule.onNodeWithTag("run-rate-save").performClick()
        waitForState { model.runs.firstOrNull { it.entry_id == run.entryId }?.session_rpe == 6 }
        assertEquals(1, logCount("AmendRunMeta"))

        // Killed in the background with the detail open; back from Recents.
        val (state, size) = ProcessDeath.saveState()
        assertSavedStateFitsABinderTransaction(size)
        ProcessDeath.kill(ctx)
        ProcessDeath.relaunchWithState(rule, state)
        waitForTag("run-trim")
        assertEquals(2, model.runs.size)
        assertEquals(6, model.runs.single { it.entry_id == run.entryId }.session_rpe)
        assertEquals(1, logLines().count { it.contains("\"LogRunTrack\"") })
    }

    /** The two thumbs of the trim [androidx.compose.material3.RangeSlider], start first. */
    private fun trimThumbs() = rule.onAllNodes(SemanticsMatcher.keyIsDefined(SemanticsActions.SetProgress))

    private fun setThumb(index: Int, seconds: Float) {
        trimThumbs()[index].performSemanticsAction(SemanticsActions.SetProgress) { it(seconds) }
        rule.waitForIdle()
    }

    /** The "Keeps 8.2 km · 41:00" summary line under the slider. */
    private fun keepsLine(): String {
        val prefix = strPrefix(R.string.main_trim_keeps)
        return rule.onAllNodes(hasText(prefix, substring = true)).fetchSemanticsNodes()
            .firstNotNullOf { n -> n.config.getOrNull(SemanticsProperties.Text)?.joinToString("")?.takeIf { it.startsWith(prefix) } }
    }

    @Test
    fun aTrimWindowChosenBeforeRotationIsTheOneAppliedAndTheTrimLandsOnce() {
        val run = gpsTenK()
        val first = run.points.first().observedAt
        val last = run.points.last().observedAt
        openRunDetail(run)
        val beforeKm = model.runs.single { it.entry_id == run.entryId }.distance_km
        rule.onNodeWithTag("run-trim").performScrollTo().performClick()
        rule.waitUntil(15_000) { trimThumbs().fetchSemanticsNodes().size == 2 }

        // Cut 5 min off the start and 10 min off the end.
        setThumb(0, 300f)
        setThumb(1, (last - first - 600).toFloat())
        val keeps = keepsLine()
        assertEquals(0, count(hasText(str(R.string.trim_hint))))

        recreate()
        rule.waitUntil(15_000) { trimThumbs().fetchSemanticsNodes().size == 2 }
        assertEquals("the chosen window survives rotation", keeps, keepsLine())

        // Trim → confirm dialog; rotate with the dialog up, then confirm.
        rule.onNode(hasText(str(R.string.main_trim_confirm)) and hasClickAction()).performScrollTo().performClick()
        rule.waitUntil(5_000) { count(isDialog()) > 0 }
        recreate()
        rule.waitUntil(15_000) { count(hasText(str(R.string.main_trim_confirm)) and hasAnyAncestor(isDialog())) == 1 }
        assertEquals(0, logCount("TrimRunTrack"))
        rule.onNode(hasText(str(R.string.main_trim_confirm)) and hasClickAction() and hasAnyAncestor(isDialog())).performClick()
        waitForState { logCount("TrimRunTrack") == 1 && model.runs.single { it.entry_id == run.entryId }.distance_km < beforeKm - 1.0 }

        val trim = payload(logLines().single { EventLog.variantOf(it) == "TrimRunTrack" }, "TrimRunTrack")
        assertEquals("start kept from the chosen second", (first + 300).toDouble(), trim["keep_from_sec"]!!.jsonPrimitive.long.toDouble(), 1.0)
        assertEquals("end kept to the chosen second", (last - 600).toDouble(), trim["keep_until_sec"]!!.jsonPrimitive.long.toDouble(), 1.0)
        val trimmedKm = model.runs.single { it.entry_id == run.entryId }.distance_km
        // 15 of 50.5 minutes at an even pace go: ~7.1 km remain.
        assertEquals(10.1 * (50.5 - 15.0) / 50.5, trimmedKm, 0.15)
        assertEquals(2, model.runs.size)

        ProcessDeath.kill(ctx)
        rule.launchApp()
        assertEquals(2, model.runs.size)
        assertEquals(trimmedKm, model.runs.single { it.entry_id == run.entryId }.distance_km, 1e-9)
        assertEquals(1, logCount("TrimRunTrack"))
    }

    // ═══ 5. Guided setup, mid-way ══════════════════════════════════════════════

    private fun onSetupScreen(name: String): Boolean {
        val marker = when (name) {
            "level" -> str(R.string.setup_level_intermediate_title)
            "run-now" -> null
            "train" -> str(R.string.setup_step_train_q)
            else -> null
        }
        return marker != null && count(hasText(marker)) > 0
    }

    private fun next() {
        rule.onNode(hasText(str(R.string.setup_btn_next)) and hasClickAction()).performClick()
        rule.waitForIdle()
    }

    @Test
    fun guidedSetupResumesOnTheSameScreenWithEveryAnswerAfterRotationAndProcessDeath() {
        rule.launchAppExpectingSetup()
        GuidedSetupWalk.walk(rule, stopAt = "level")
        assertTrue(onSetupScreen("level"))

        recreate()
        assertTrue("rotation keeps the wizard on its screen", onSetupScreen("level"))
        assertFalse(onSetupScreen("train"))
        val level = GuidedSetupWalk.bothPath.single { it.name == "level" }
        level.answer(rule)

        // Killed in the background mid-setup, restored from Recents.
        val (state, size) = ProcessDeath.saveState()
        assertSavedStateFitsABinderTransaction(size)
        ProcessDeath.kill(ctx)
        ProcessDeath.relaunchWithState(rule, state)
        rule.waitUntil(15_000) { onSetupScreen("level") }
        assertFalse("no profile was written mid-setup", File(ctx.filesDir, "event-log.ndjson").exists())
        next()
        for (screen in GuidedSetupWalk.bothPath.dropWhile { it.name != "run-now" }) {
            if (screen.name == "review") break
            screen.answer(rule)
            rule.waitForIdle()
            next()
        }
        rule.onNode(hasText(str(R.string.setup_btn_finish)) and hasClickAction()).performScrollTo().performClick()
        waitForState(30_000) { EventDispatcher.model.value?.profile != null && EventDispatcher.model.value?.program != null }

        // Every answer from before the rotation and the process death landed, once.
        assertEquals(1, logCount("SetProfile"))
        val p = checkNotNull(model.profile)
        assertEquals("the 5K in 26:15 typed before the process death", 26 * 60 + 15, p.recent_race?.time_sec)
        assertEquals(5.0, p.recent_race!!.distance_km, 1e-9)
        assertEquals(GoalDistance.TenK.name, p.goal_distance)
        assertEquals("five ticked weekdays", 5, Integer.bitCount(checkNotNull(p.training_weekdays)))
        assertEquals(java.time.LocalDate.of(1991, 3, 12).toEpochDay(), p.birth_epoch_day)

        ProcessDeath.kill(ctx)
        rule.launchApp()
        assertEquals("no second setup after a cold start", 0, count(hasText(str(R.string.setup_step_train_q))))
        assertEquals(p, model.profile)
    }

    @Test
    fun aColdStartMidSetupStartsTheSetupOverWithNothingSaved() {
        rule.launchAppExpectingSetup()
        GuidedSetupWalk.walk(rule, stopAt = "days")
        ProcessDeath.kill(ctx)
        rule.launchAppExpectingSetup()
        assertTrue("back on the first question", onSetupScreen("train"))
        assertFalse(File(ctx.filesDir, "event-log.ndjson").exists())
        assertEquals(null, EventDispatcher.model.value?.profile)
    }

    // ═══ 7. A calculator with a result ═════════════════════════════════════════

    @Test
    fun aRecomputedCalculatorResultSurvivesRotationAndProcessDeath() {
        QaHarness.seedLog(
            ctx,
            listOf(
                Event.SetProfile(
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
                ),
                Event.ComputeProtein(81.7, masters = false, deficit = true),
            ),
        )
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_profile))
        val row = hasText(str(R.string.main_tool_protein_title)) and hasClickAction()
        rule.onAllNodes(hasScrollToNodeAction()).onFirst().performScrollToNode(row)
        rule.onAllNodes(row).onFirst().performClick()
        waitForTag("calc-result")
        val first = model.protein_figures.single()

        // A new bodyweight, typed on the keypad, then Compute.
        rule.onNode(hasClickAction() and hasContentDescription(str(R.string.log_field_bodyweight), substring = true))
            .performScrollTo().performClick()
        for (k in "95") {
            rule.onNode(hasText("$k") and hasClickAction() and hasAnyAncestor(hasTestTag("numeric-keypad"))).performScrollTo().performClick()
        }
        rule.onNode(hasText(str(R.string.log_btn_compute_protein)) and hasClickAction()).performScrollTo().performClick()
        waitForState { model.protein_input?.bodyweight_kg == 95.0 }
        val second = model.protein_figures.single()
        assertTrue("the answer moved with the weight", second.high_g_per_day > first.high_g_per_day)
        val headline = "${Math.round(second.low_g_per_day)}–${Math.round(second.high_g_per_day)}"

        recreate()
        waitForTag("calc-result")
        assertHeadline(headline)

        val (state, size) = ProcessDeath.saveState()
        assertSavedStateFitsABinderTransaction(size)
        ProcessDeath.kill(ctx)
        ProcessDeath.relaunchWithState(rule, state)
        waitForTag("calc-result")
        assertHeadline(headline)
        assertEquals(95.0, model.protein_input?.bodyweight_kg)
    }

    /** The calculator's answer (tagged text, read in the unmerged tree). */
    private fun assertHeadline(text: String) {
        val m = hasTestTag("calc-headline") and hasText(text)
        rule.waitUntil(10_000) { rule.onAllNodes(m, useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty() }
    }

    // ═══ 8. Theme and unit switches ═══════════════════════════════════════════

    private fun segment(label: String) = hasText(label) and isSelectable()

    private fun tapProfileOption(m: SemanticsMatcher) {
        rule.onAllNodes(hasScrollToNodeAction())[0].performScrollToNode(m)
        rule.onAllNodes(m).onFirst().performClick()
        rule.waitForIdle()
    }

    @Test
    fun themeAndUnitSwitchesSurviveRotationAndProcessDeathAndAWeightUnitRoundTripChangesNothingElse() {
        val profile = Event.SetProfile(
            progressionCadence = ProgressionCadence.WeekToWeek,
            liftGoal = LiftGoal.MaxStrength,
            goalDistance = GoalDistance.TenK,
            concurrentGoal = ConcurrentGoal.Strength,
            weeklySets = 12,
            runningDaysPerWeek = 2,
            runningKmPerWeek = 15.0,
            advanced = false,
            female = true,
            bodyweightKg = 63.4,
            restingHrBpm = 54.0,
            measuredHrMax = 188.0,
            recentRace = RecentRace(distance_km = 10.0, time_sec = 52 * 60, epoch_day = todayEpochDay() - 20),
            liftDaysPerWeek = 3,
            birthEpochDay = java.time.LocalDate.of(1990, 6, 1).toEpochDay(),
        )
        val runAt = QaHarness.daysAgoAtMidday(2)
        QaHarness.seedLog(
            ctx,
            listOf(
                profile,
                Event.GeneratePlan(todayEpochDay() - 10),
                QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(3)),
                Event.LogRun(8.0, 44.0, 0.0, 0.0, runAt, runAt * 1000),
            ),
        )
        rule.launchApp()
        val before = model
        val beforeProfile = checkNotNull(before.profile)

        rule.selectTab(str(R.string.main_dest_profile))
        tapProfileOption(hasText(AppTheme.Meadow.label) and isSelectable())
        tapProfileOption(segment(str(R.string.main_unit_mi)))
        tapProfileOption(segment(app.milestone.weightSuffix(app.milestone.WeightUnit.Lb)))
        waitForState { model.profile?.weight_lb == true }
        assertEquals(AppTheme.Meadow, AppSettings.theme.value)
        assertEquals(DistanceUnitOverride.Mi, AppSettings.distanceUnitOverride.value)

        recreate()
        assertEquals(AppTheme.Meadow, AppSettings.theme.value)
        rule.onAllNodes(hasScrollToNodeAction())[0].performScrollToNode(hasText(AppTheme.Meadow.label) and isSelectable())
        rule.onNode(hasText(AppTheme.Meadow.label) and isSelectable()).assertIsSelected()

        // Cold start: only what was saved can bring them back.
        ProcessDeath.kill(ctx)
        rule.launchApp()
        assertEquals(AppTheme.Meadow, AppSettings.theme.value)
        assertEquals(DistanceUnitOverride.Mi, AppSettings.distanceUnitOverride.value)
        assertTrue(model.profile!!.weight_lb)
        rule.selectTab(str(R.string.main_dest_history))
        rule.selectHistoryFilter(str(R.string.main_segment_runs))
        rule.scrollToText(runDistLabel(8.0, DistanceUnit.Mi))

        // Back to kg: the profile is the one the user had, field for field; the plan
        // and every logged number are untouched by the two unit flips.
        rule.selectTab(str(R.string.main_dest_profile))
        tapProfileOption(segment(app.milestone.weightSuffix(app.milestone.WeightUnit.Kg)))
        waitForState { model.profile?.weight_lb == false }
        assertEquals(beforeProfile, model.profile)
        assertEquals(before.program, model.program)
        assertEquals(before.week_plan, model.week_plan)
        assertEquals(before.lifts, model.lifts)
        assertEquals(before.runs.map { it.distance_km to it.duration_min }, model.runs.map { it.distance_km to it.duration_min })
    }

    // ═══ 9. The persisted run session (crash sidecar → RunSession.restore) ══════

    private val metresPerDegLat = 111_195.0

    /**
     * A seeded wandering run of [n] fixes every [dtSec] s at about [mps] (±15 %),
     * with standing-still stretches (a traffic light: 8 fixes within a metre) every
     * 150 fixes, so the moving-time gate is exercised.
     */
    private fun synthRun(seed: Long, startSec: Long, n: Int, dtSec: Long = 3, mps: Double = 3.0): List<GpsPoint> {
        val rnd = Random(seed)
        var lat = 52.52
        var lon = 13.40
        var heading = rnd.nextDouble(0.0, 2 * PI)
        return (0 until n).map { i ->
            if (i > 0) {
                val standing = i % 150 in 1..8
                val d = if (standing) rnd.nextDouble() * 0.6 else mps * dtSec * (0.85 + 0.3 * rnd.nextDouble())
                heading += (rnd.nextDouble() - 0.5) * 0.4
                lat += d * cos(heading) / metresPerDegLat
                lon += d * sin(heading) / (metresPerDegLat * cos(Math.toRadians(lat)))
            }
            GpsPoint(lat, lon, startSec + i * dtSec, 3.0 + rnd.nextDouble() * 6.0)
        }
    }

    /** Feed [points] the way the service's onFix does: the fix's own monotonic stamp
     *  (here tracking its wall-clock offset from [t0] above [baseElapsedMs]) into
     *  [RunSession.add], and every accepted fix into the sidecar. [pauseBefore]
     *  marks a Pause/Resume boundary before those indices. */
    private fun feedLive(points: List<GpsPoint>, t0: Long, baseElapsedMs: Long, pauseBefore: Set<Int> = emptySet()) {
        points.forEachIndexed { i, p ->
            if (i in pauseBefore) RunSession.markResumeBoundary()
            val nanos = (baseElapsedMs + (p.observedAt - t0) * 1000L) * 1_000_000L
            if (RunSession.add(p, nanos)) ActiveRunStore.append(p)
        }
    }

    private data class Live(val km: Double, val sec: Long, val points: List<GpsPoint>, val starts: List<Int>)

    private fun liveSnapshot() = Live(
        RunSession.distanceKm.value,
        RunSession.elapsedSec.value,
        RunSession.points.value.toList(),
        RunSession.decimatedSegmentStarts(Int.MAX_VALUE),
    )

    /** The process dies: RunSession is gone, the sidecar is read back and resumed. */
    private fun dieAndRecover(): List<GpsPoint> {
        ActiveRunStore.flush()
        RunSession.reset()
        val recovered = ActiveRunStore.recover(ctx)
        RunSession.restore(recovered)
        return recovered
    }

    @Test
    fun aRecoveredRunKeepsEveryFixItsDistanceAndItsMovingTime() {
        val t0 = AppClock.nowSec() - 3600
        val pts = synthRun(seed = 31, startSec = t0, n = 1200)
        ActiveRunStore.begin(ctx)
        feedLive(pts, t0, baseElapsedMs = 5_000)
        val live = liveSnapshot()
        val livePace = RunSession.paceBucketsFlow.value

        val recovered = dieAndRecover()
        assertEquals("every accepted fix is in the sidecar, once", live.points, recovered)
        assertEquals(live.points, RunSession.points.value.toList())
        assertEquals("no double-counted distance", live.km, RunSession.distanceKm.value, 1e-9)
        assertEquals("no double-counted moving time", live.sec.toDouble(), RunSession.elapsedSec.value.toDouble(), 1.0)
        assertEquals(live.starts, RunSession.decimatedSegmentStarts(Int.MAX_VALUE))
        assertEquals("the pace slices resume where they were", livePace.size, RunSession.paceBucketsFlow.value.size)
        assertTrue(RunSession.tracking.value)
        assertFalse(RunSession.armed.value)
    }

    @Test
    fun aResumedRunCountsOnlyLegsRunAfterTheResumeAndASecondCrashRecoversItAllOnce() {
        val t0 = AppClock.nowSec() - 7200
        val before = synthRun(seed = 32, startSec = t0, n = 600)
        ActiveRunStore.begin(ctx)
        feedLive(before, t0, baseElapsedMs = 5_000)
        val atCrash = liveSnapshot()
        dieAndRecover()

        // Two minutes later and 80 m away the GPS is back; the next 400 fixes run on.
        val resumeAt = before.last().observedAt + 120
        val after = synthRun(seed = 33, startSec = resumeAt, n = 400).map {
            it.copy(lat = it.lat - 52.52 + before.last().lat + 80.0 / metresPerDegLat, lon = it.lon - 13.40 + before.last().lon)
        }
        val elapsedBase = android.os.SystemClock.elapsedRealtime() + 1_000
        feedLive(after, resumeAt, baseElapsedMs = elapsedBase)
        val afterKm = after.zipWithNext { a, b -> a to b }
            .filter { (a, b) -> segmentKm(a, b) * 1000.0 / (b.observedAt - a.observedAt) >= 0.5 }
            .sumOf { (a, b) -> segmentKm(a, b) }
        assertEquals(
            "distance = before the crash + the legs after it, never the 80 m splice",
            atCrash.km + afterKm, RunSession.distanceKm.value, 1e-6,
        )
        assertEquals(listOf(before.size), RunSession.decimatedSegmentStarts(Int.MAX_VALUE))
        val resumed = liveSnapshot()

        // A second process death: the sidecar now holds both parts, each once.
        val second = dieAndRecover()
        assertEquals(before.size + after.size, second.size)
        assertEquals(resumed.points, second)
        assertEquals(resumed.km, RunSession.distanceKm.value, 1e-6)
        assertEquals(resumed.sec.toDouble(), RunSession.elapsedSec.value.toDouble(), 2.0)
    }

    /**
     * A runner pauses at a crossing for 20 s, walks 30 m to the other side and
     * resumes. Live, the Resume opens a new segment, so the walk is neither distance
     * nor moving time. After a crash the sidecar has no pause marker and the 20 s gap
     * is under the fix-gap rule, so the recovered run must still not count the walk:
     * a recovered run never shows more than it had before the crash.
     */
    @Test
    fun aShortPauseWithAWalkIsNotCountedAfterRecovery() {
        val t0 = AppClock.nowSec() - 3600
        val legs = synthRun(seed = 34, startSec = t0, n = 400)
        // Index 200 resumes 20 s after 199 and 30 m further on.
        val pts = legs.mapIndexed { i, p ->
            if (i < 200) p else p.copy(lat = p.lat + 30.0 / metresPerDegLat, observedAt = p.observedAt + 17)
        }
        ActiveRunStore.begin(ctx)
        feedLive(pts, t0, baseElapsedMs = 5_000, pauseBefore = setOf(200))
        val live = liveSnapshot()
        assertEquals(listOf(200), live.starts)

        dieAndRecover()
        assertTrue(
            "recovered ${RunSession.distanceKm.value} km > live ${live.km} km",
            RunSession.distanceKm.value <= live.km + 1e-9,
        )
        assertTrue("recovered ${RunSession.elapsedSec.value} s > live ${live.sec} s", RunSession.elapsedSec.value <= live.sec + 1)
    }

    @Test
    fun aVeryLongRecoveredRunIsThinnedWithoutLosingItsShape() {
        // 2 h 30 at 1 Hz: 9000 fixes, past the recovery cap.
        val t0 = AppClock.nowSec() - 10_000
        val pts = synthRun(seed = 35, startSec = t0, n = 9000, dtSec = 1)
        ActiveRunStore.begin(ctx)
        feedLive(pts, t0, baseElapsedMs = 5_000)
        val live = liveSnapshot()
        val recovered = dieAndRecover()
        assertTrue("thinned to the cap: ${recovered.size}", recovered.size <= TRACK_DECIMATION_CAP + 1)
        assertEquals("start kept", pts.first(), recovered.first())
        assertEquals("finish kept", pts.last(), recovered.last())
        assertTrue("no fabricated pause: ${RunSession.decimatedSegmentStarts(Int.MAX_VALUE)}", RunSession.decimatedSegmentStarts(Int.MAX_VALUE).isEmpty())
        assertTrue("distance ${RunSession.distanceKm.value} vs live ${live.km}", abs(RunSession.distanceKm.value - live.km) / live.km < 0.02)
        assertTrue("never more than live", RunSession.distanceKm.value <= live.km + 1e-9)
        assertTrue("moving time ${RunSession.elapsedSec.value} vs ${live.sec}", abs(RunSession.elapsedSec.value - live.sec) <= live.sec * 0.03)
    }

    @Test
    fun aTornLastSidecarLineLosesOnlyThatFix() {
        val t0 = AppClock.nowSec() - 3600
        val pts = synthRun(seed = 36, startSec = t0, n = 300)
        ActiveRunStore.begin(ctx)
        feedLive(pts, t0, baseElapsedMs = 5_000)
        ActiveRunStore.flush()
        val f = File(ctx.filesDir, "active-run.ndjson")
        f.appendText("{\"lat\":52.5,\"lon\":13.4,\"observed_at\":")
        val recovered = dieAndRecover()
        assertEquals(pts, recovered)
        assertTrue(RunSession.tracking.value)
    }

    // ── The resume prompt, end to end ──────────────────────────────────────────

    @Test
    fun aRunInterruptedByProcessDeathIsOfferedResumedWithEveryFixAndSavedOnce() {
        // The resume gate ages the sidecar against the wall clock, so run on it.
        AppClock.offsetMillis = 0
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        val now = System.currentTimeMillis() / 1000
        val pts = synthRun(seed = 37, startSec = now - 900, n = 290)
        ActiveRunStore.begin(ctx)
        feedLive(pts, pts.first().observedAt, baseElapsedMs = 5_000)
        RunSession.tracking.value = true
        val live = liveSnapshot()

        ProcessDeath.kill(ctx)
        rule.launchApp()
        rule.assertTextCountEventually(str(R.string.main_resume_run_title), 1)
        rule.onNode(hasText(str(R.string.main_resume)) and hasClickAction() and hasAnyAncestor(isDialog())).performClick()
        waitForState { RunSession.tracking.value && RunSession.points.value.size == pts.size }
        assertEquals(live.km, RunSession.distanceKm.value, 1e-9)
        assertEquals(live.sec.toDouble(), RunSession.elapsedSec.value.toDouble(), 1.0)
        waitForText(str(R.string.track_btn_stop))

        // A rotation on the resumed tracker: no second offer, no doubled fixes.
        recreate()
        waitForText(str(R.string.track_btn_stop))
        assertEquals(0, count(hasText(str(R.string.main_resume_run_title))))
        assertEquals(pts.size, RunSession.points.value.size)

        stopToFinishReview()
        saveRunAndAwait()
        assertEquals(1, logCount("LogRunTrack"))
        waitForState { model.runs.size == 1 }
        val saved = model.runs.single()
        assertTrue("saved ${saved.distance_km} km vs live ${live.km} km", abs(saved.distance_km - live.km) <= 0.02 * live.km + 0.01)
        assertEquals(pts.last().observedAt, saved.observed_at)
        ActiveRunStore.flush()
        assertFalse("the sidecar of a saved run is gone", File(ctx.filesDir, "active-run.ndjson").exists())

        ProcessDeath.kill(ctx)
        rule.launchApp()
        assertEquals(0, count(hasText(str(R.string.main_resume_run_title))))
        assertEquals(1, model.runs.size)
    }

    @Test
    fun aRotationMidRunNeverOffersARecoveryOrDuplicatesFixes() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        val t0 = AppClock.nowSec() - 600
        val pts = synthRun(seed = 38, startSec = t0, n = 150)
        ActiveRunStore.begin(ctx)
        feedLive(pts, t0, baseElapsedMs = 5_000)
        RunSession.tracking.value = true
        val live = liveSnapshot()
        launchOnTracker()
        repeat(2) {
            recreate()
            waitForText(str(R.string.track_btn_stop))
        }
        assertEquals(0, count(hasText(str(R.string.main_resume_run_title))))
        assertEquals(live.points, RunSession.points.value.toList())
        assertEquals(live.km, RunSession.distanceKm.value, 1e-12)
        assertEquals(0, logCount("LogRunTrack"))
    }

    @Test
    fun aStaleOrOneFixSidecarIsDroppedWithoutAPromptOrARun() {
        AppClock.offsetMillis = 0
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        val now = System.currentTimeMillis() / 1000
        // Four hours old: past the 3 h resume window.
        val stale = synthRun(seed = 39, startSec = now - 4 * 3600 - 900, n = 200)
        ActiveRunStore.begin(ctx)
        stale.forEach { ActiveRunStore.append(it) }
        ActiveRunStore.flush()
        rule.launchApp()
        ActiveRunStore.flush()
        assertEquals(0, count(hasText(str(R.string.main_resume_run_title))))
        assertFalse(File(ctx.filesDir, "active-run.ndjson").exists())
        assertEquals(0, model.runs.size)

        // A crash before the second fix: nothing to resume.
        ProcessDeath.kill(ctx)
        ActiveRunStore.begin(ctx)
        ActiveRunStore.append(GpsPoint(52.5, 13.4, now - 5, 5.0))
        ActiveRunStore.flush()
        rule.launchApp()
        ActiveRunStore.flush()
        assertEquals(0, count(hasText(str(R.string.main_resume_run_title))))
        assertFalse(File(ctx.filesDir, "active-run.ndjson").exists())
    }

    /** The save path after a recovery sends the same run the core would have got
     *  without the crash (same fixes, same pauses), so the logged distance agrees. */
    @Test
    fun aRunSavedAfterRecoveryLogsTheSameDistanceAsWithoutTheCrash() {
        val t0 = AppClock.nowSec() - 3600
        val pts = synthRun(seed = 40, startSec = t0, n = 800)
        ActiveRunStore.begin(ctx)
        feedLive(pts, t0, baseElapsedMs = 5_000)
        val uninterrupted = Event.LogRunTrack(
            points = RunSession.decimatedTrackForCore(), hrPctMax = 0.0, longestRecentKm = 0.0,
            observedAt = pts.last().observedAt, entryId = 1_000_001, segmentStarts = RunSession.decimatedSegmentStarts(),
        )
        dieAndRecover()
        val recovered = uninterrupted.copy(
            points = RunSession.decimatedTrackForCore(), entryId = 1_000_002, segmentStarts = RunSession.decimatedSegmentStarts(),
        )
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        Core.restore(ctx)
        Core.send(Event.SetToday(todayEpochDay(), utcOffsetSec()))
        Core.send(uninterrupted)
        val runs = Core.send(recovered).view.runs
        val a = runs.single { it.entry_id == 1_000_001L }
        val b = runs.single { it.entry_id == 1_000_002L }
        assertEquals("distance", a.distance_km, b.distance_km, 1e-6)
        assertEquals("moving time", a.duration_min, b.duration_min, 1e-6)
    }
}
