package app.milestone.qa

import android.content.Context
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.test.core.app.ApplicationProvider
import app.milestone.AppClock
import app.milestone.Event
import app.milestone.EventDispatcher
import app.milestone.PrescriptionView
import app.milestone.R
import java.io.File
import java.util.Locale
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.double
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * Owner-reported regressions driven end to end over the real core: deleting a run
 * logged with a heart rate and undoing it keeps its bpm; the set loop's pinned
 * primary walks "Log set k of N" → "Next: <exercise>" → … → "Finish session"
 * through a whole session; and the loop's "End exercise" cue confirms with an Undo
 * that reopens the exercise.
 */
@RunWith(RobolectricTestRunner::class)
class OwnerRegressionJourneyTest {

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

    private fun eventLogLines(): List<String> =
        File(ctx.filesDir, "event-log.ndjson").takeIf { it.exists() }?.readLines().orEmpty()

    /** Wait for app state the screen doesn't show; the semantics fetch idles the looper. */
    private fun waitForState(timeoutMs: Long = 10_000, cond: () -> Boolean) =
        rule.waitUntil(timeoutMs) {
            rule.onAllNodes(isRoot()).fetchSemanticsNodes()
            cond()
        }

    private fun rest(sec: Long = 90) {
        AppClock.offsetMillis += sec * 1000L
    }

    private fun setHeader(k: Int, n: Int) = str(R.string.log_set_header_n_of_n, k, n)

    private fun logSet() = rule.onNodeWithTag("editor-primary").performClick()

    private fun primaryReads(label: String) {
        rule.assertCountEventually(hasTestTag("editor-primary") and hasText(label), 1)
    }

    private fun openLoopFor(exercise: String) {
        rule.onNode(
            hasClickAction() and hasContentDescription(strPrefix(R.string.plan_log_set_n_of_n, 1), substring = true) and
                hasAnyAncestor(hasTestTag("plan-item:$exercise")),
        ).performScrollTo().performClick()
        rule.waitUntil(5_000) {
            rule.onAllNodes(hasText(strPrefix(R.string.log_set_header_n_of_n, 1), substring = true)).fetchSemanticsNodes().isNotEmpty()
        }
    }

    private fun todaysItems(): List<PrescriptionView> =
        EventDispatcher.model.value!!.week_plan.first { it.epoch_day == app.milestone.todayEpochDay() }
            .items.filter { it.exercise.isNotBlank() }

    /** A clickable without a content description: tells an in-sheet action (or the
     *  snackbar's Undo) from Today's same-worded action named for TalkBack. */
    private val noDescription = SemanticsMatcher.keyNotDefined(SemanticsProperties.ContentDescription)

    // ── Delete a bpm run in History → Undo brings it back WITH its bpm ───────
    @Test
    fun deletingABpmRunThenUndo_restoresTheRunWithItsBpm() {
        val at = QaHarness.daysAgoAtMidday(1)
        val entryId = at * 1000
        QaHarness.seedLog(
            ctx,
            listOf(
                QaHarness.strengthProfile(),
                Event.LogRun(
                    distanceKm = 8.0, durationMin = 45.0, hrPctMax = 0.0, longestRecentKm = 0.0,
                    observedAt = at, entryId = entryId, avgHrBpm = 152.0,
                ),
            ),
        )
        rule.launchApp()
        assertEquals(152.0, EventDispatcher.model.value!!.runs.single { it.entry_id == entryId }.avg_hr_bpm!!, 0.0)
        rule.selectTab(str(R.string.main_dest_history))

        // Yesterday (Sunday) at midday, untagged → titled by its weekday.
        val title = str(R.string.history_run_weekday, "Sunday")
        rule.scrollToText(title)
        rule.onNodeWithText(title).performClick()
        rule.waitForIdle()
        // Delete run confirms first (spec §2.7), then lands with "Run deleted · Undo".
        rule.onNodeWithTag("run-delete").performScrollTo().performClick()
        rule.waitForIdle()
        rule.onNode(hasText(str(R.string.run_delete_message))).assertExists()
        rule.onNode(hasText(str(R.string.main_delete)) and hasClickAction()).performClick()
        waitForState { EventDispatcher.model.value!!.runs.none { it.entry_id == entryId } }
        rule.assertTextCountEventually(str(R.string.history_run_deleted), 1)

        rule.onNode(hasText(str(R.string.log_undo)) and hasClickAction()).performClick()
        waitForState { EventDispatcher.model.value!!.runs.any { it.entry_id == entryId } }
        val restored = EventDispatcher.model.value!!.runs.single { it.entry_id == entryId }
        assertEquals("the restored run keeps its bpm", 152.0, restored.avg_hr_bpm ?: 0.0, 0.0)

        // The durable restore carries the bpm (and leaves the % for the core to derive).
        val lines = eventLogLines()
        val deleteAt = lines.indexOfLast { it.contains("\"DeleteEntry\"") }
        assertTrue("the delete is logged: $lines", deleteAt >= 0)
        val restore = lines.drop(deleteAt + 1).last { it.contains("\"LogRun\"") }
        val payload = Json.parseToJsonElement(restore).jsonObject["LogRun"]!!.jsonObject
        assertEquals(entryId, payload["entry_id"]!!.jsonPrimitive.long)
        assertEquals(restore, 152.0, payload["avg_hr_bpm"]?.jsonPrimitive?.double ?: 0.0, 0.0)
        assertEquals(restore, 0.0, payload["hr_pct_max"]!!.jsonPrimitive.double, 0.0)

        // What the user sees: the restored run reopens in its editor on 152 bpm.
        rule.scrollToText(title)
        rule.onNodeWithText(title).performClick()
        rule.waitForIdle()
        rule.onNodeWithTag("run-log-details").performScrollTo().performClick()
        rule.waitForIdle()
        rule.assertCountEventually(
            hasContentDescription("${str(R.string.track_log_hr_avg)}, 152 ${str(R.string.track_log_hr_bpm)}"),
            1,
        )
    }

    // ── Delete a GPS run in History → Undo brings the run and its route back ──
    @Test
    fun deletingAGpsRunThenUndo_restoresTheRunAndItsRoute() {
        val at = QaHarness.daysAgoAtMidday(1)
        val entryId = at * 1000
        // A 2 km loop-free track heading north, one clean fix every 3 s, 10 m apart.
        val metresPerDegLat = 111_195.0
        val points = (0 until 201).map { i ->
            app.milestone.GpsPoint(lat = 47.0 + i * 10.0 / metresPerDegLat, lon = 8.0, observedAt = at + i * 3L, accuracyM = 5.0)
        }
        QaHarness.seedLog(
            ctx,
            listOf(
                QaHarness.strengthProfile(),
                Event.LogRunTrack(points = points, hrPctMax = 0.0, longestRecentKm = 0.0, observedAt = at, entryId = entryId),
            ),
        )
        rule.launchApp()
        val before = EventDispatcher.model.value!!.runs.single { it.entry_id == entryId }
        rule.selectTab(str(R.string.main_dest_history))
        val title = str(R.string.history_run_weekday, "Sunday")
        rule.scrollToText(title)
        rule.onNodeWithText(title).performClick()
        // Undo is built from the route, so wait until it has loaded (Trim appears then).
        rule.waitUntil(10_000) { rule.onAllNodes(hasTestTag("run-trim")).fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithTag("run-delete").performScrollTo().performClick()
        rule.waitForIdle()
        rule.onNode(hasText(str(R.string.main_delete)) and hasClickAction()).performClick()
        waitForState { EventDispatcher.model.value!!.runs.none { it.entry_id == entryId } }
        rule.assertTextCountEventually(str(R.string.history_run_deleted), 1)

        rule.onNode(hasText(str(R.string.log_undo)) and hasClickAction()).performClick()
        waitForState { EventDispatcher.model.value!!.runs.any { it.entry_id == entryId } }
        val restored = EventDispatcher.model.value!!.runs.single { it.entry_id == entryId }
        assertTrue("the restored run keeps its route", restored.has_gpx)
        assertEquals("same observed_at", before.observed_at, restored.observed_at)
        assertEquals("same distance", before.distance_km, restored.distance_km, 0.01)
    }

    // ── The loop's primary walks a whole session to "Finish session" ─────────
    @Test
    fun theLoopPrimaryWalksEverySetThenNextThenFinishSession() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        val items = todaysItems()
        assertTrue("scenario: several lifts today: $items", items.size >= 2 && items.none { it.done })
        openLoopFor(items.first().exercise)
        items.forEachIndexed { i, item ->
            rule.assertCountEventually(hasTestTag("loop-exercise") and hasText(item.exercise), 1)
            val n = app.milestone.plannedSetCount(item)
            rule.typeWeight("60")
            for (k in 1..n) {
                rule.assertTextCountEventually(setHeader(k, n), 1)
                primaryReads(str(R.string.log_primary_set_n_of_n, k, n))
                logSet()
                rest()
            }
            rule.assertTextCountEventually(str(R.string.plan_item_complete), 1)
            val next = items.getOrNull(i + 1)
            primaryReads(
                if (next != null) str(R.string.log_primary_next, next.exercise) else str(R.string.log_primary_finish_session),
            )
            assertEquals(n, todaysItems().first { it.exercise == item.exercise }.sets_done)
            logSet()
            // The lifter walks to the next bar (a tap within the 800 ms repeat guard
            // would be dropped as a double tap).
            rest()
        }
        // Finish session closes the loop onto Today's session-done card.
        rule.assertCountEventually(hasTestTag("editor-primary"), 0)
        rule.assertCountEventually(hasTestTag("session-done"), 1)
        assertTrue(todaysItems().all { it.done })
    }

    // ── "End exercise" from inside the loop → Undo reopens it ────────────────
    @Test
    fun endExerciseFromTheLoopCue_undoReopensTheExercise() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        val squat = todaysItems().first().exercise
        openLoopFor(squat)
        val n = app.milestone.plannedSetCount(todaysItems().first())
        assertTrue("scenario: sets remain after two", n > 2)
        // Set 1 as prescribed, then set 2 short of the planned reps (a missed set):
        // the coach cues "end the exercise here" with an End exercise action in the loop.
        rule.typeWeight("60")
        logSet()
        rule.assertTextCountEventually(setHeader(2, n), 1)
        rest()
        rule.onNode(hasText("1") and hasAnyAncestor(hasTestTag("reps-scale"))).performScrollTo().performClick()
        logSet()
        waitForState { todaysItems().first { it.exercise == squat }.logged_sets.size == 2 }
        val endLabel = str(R.string.log_cue_end_exercise)
        val inLoopEnd = hasText(endLabel) and hasClickAction() and noDescription
        rule.assertCountEventually(inLoopEnd, 1)
        val done = todaysItems().first { it.exercise == squat }.sets_done
        rule.onNode(inLoopEnd).performClick()

        val epochDay = app.milestone.todayEpochDay()
        waitForState { todaysItems().first { it.exercise == squat }.ended_early }
        rule.assertTextCountEventually(str(R.string.plan_ended_msg, squat, done, n), 1)
        rule.assertTextCountEventually(str(R.string.plan_item_complete), 1)

        // The snackbar's Undo (the loop's own "Logged … · Undo" row is spoken with
        // its set, so it carries a description).
        rule.onNode(hasText(str(R.string.log_undo)) and hasClickAction() and noDescription).performClick()
        waitForState { !todaysItems().first { it.exercise == squat }.ended_early }
        rule.assertTextCountEventually(str(R.string.plan_item_complete), 0)
        rule.assertTextCountEventually(setHeader(done + 1, n), 1)
        assertEquals(done, todaysItems().first { it.exercise == squat }.sets_done)

        // Durable: an ended=true marker, then the Undo's ended=false for the same day.
        val ends = eventLogLines().filter { it.contains("\"SetExerciseEnded\"") }
            .map { Json.parseToJsonElement(it).jsonObject["SetExerciseEnded"]!!.jsonObject }
            .filter { it["exercise"]!!.jsonPrimitive.content == squat && it["epoch_day"]!!.jsonPrimitive.long == epochDay }
        assertEquals("$ends", listOf(true, false), ends.map { it["ended"]!!.jsonPrimitive.boolean })
        // Still loggable from the loop's primary.
        rule.onNodeWithTag("editor-primary").assert(hasText(str(R.string.log_primary_set_n_of_n, done + 1, n)))
    }
}
