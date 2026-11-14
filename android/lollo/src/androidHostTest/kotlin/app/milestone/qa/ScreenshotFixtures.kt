package app.milestone.qa

import android.content.Context
import android.os.Looper
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.junit4.ComposeTestRule
import app.milestone.Event
import app.milestone.R
import app.milestone.TestClock
import java.time.Duration
import java.util.Locale
import org.robolectric.Shadows.shadowOf

/** The squat as an athlete types it, sentence case (the core keeps user-entered
 *  names verbatim, so captures read "Back squat"). */
internal const val TYPED_SQUAT = "Back squat"

/** A strength lifter two days after an anchored squat plus accessory sets, so
 *  Today's item faces carry plates, an effort line and "Last:" lines. */
internal fun seedStrengthWithHistory(ctx: Context) {
    val prior = QaHarness.daysAgoAtMidday(2)
    QaHarness.seedLog(
        ctx,
        listOf(
            QaHarness.strengthProfile(),
            Event.LogSet(TYPED_SQUAT, 120.0, 3, 8.0, prior - 1_800, (prior - 1_800) * 1000),
            Event.LogSet("Row", 60.0, 8, 7.0, prior, prior * 1000),
            Event.LogSet("Bench press", 60.0, 8, 7.0, prior + 600, prior * 1000 + 1),
            Event.LogSet("Romanian deadlift", 80.0, 8, 7.0, prior + 1200, prior * 1000 + 2),
        ),
    )
}

/**
 * A 5K runner's History, open and settled: twelve weeks of three hand-logged runs
 * (easy, short, long), with [halfMarathon] a 21.2 km run five weeks back, one squat
 * session three days ago (so the Lifts | Runs segment shows) and this morning's
 * rated GPS 10K (Effort 7, the Fastest 10K).
 */
internal fun ComposeTestRule.openRunsHistory(ctx: Context, halfMarathon: Boolean) {
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
                add(at to Event.LogRun(km, km * 5.8, 0.0, observedAt = at, entryId = at * 1000))
            }
        }
        if (halfMarathon) {
            val half = QaHarness.daysAgoAtMidday(33) + 600
            add(half to Event.LogRun(21.2, 115.0, 0.0, observedAt = half, entryId = half * 1000))
        }
        var t = QaHarness.daysAgoAtMidday(3)
        repeat(3) {
            t += 180
            add(t to Event.LogSet("Back Squat", 100.0, 5, 8.0, t, t * 1000))
        }
    }.sortedBy { it.first }.map { it.second }
    // This morning, before the harness's pinned "now".
    val thisMorning = QaHarness.daysAgoAtMidday(0) - 5 * 3600
    QaHarness.seedLog(ctx, listOf(QaHarness.fiveKRunnerProfile()) + manual + QaHarness.gpsTenK(thisMorning, sessionRpe = 7))
    launchApp()
    selectTab(str(R.string.main_dest_history))
    waitForText(str(R.string.main_overline_last_7_days).uppercase(Locale.US))
}

/** Wait until a node matches [matcher], then until the UI is idle. */
internal fun ComposeTestRule.waitForNodeSettled(matcher: SemanticsMatcher) {
    waitForNode(matcher)
    waitForIdle()
}

/**
 * Stop the app clock half a second past [secondsIn] whole seconds into the rest that
 * started at [restStartMillis], then let every 1 s ticker read it (one tick on the
 * compose clock and on the main looper). The ticking readouts then hold still for a
 * capture whatever wall time the steps before it took: the loop's countdown (rounded
 * up) reads the rest less [secondsIn], Today's rest pill (rounded down) one second
 * less. [QaHarness.reset] lets the clock run again; a test's `@After` must too
 * ([releaseClock]), for classes that don't reset it.
 */
internal fun ComposeTestRule.holdClockInRest(restStartMillis: Long, secondsIn: Long) {
    mainClock.autoAdvance = false
    try {
        TestClock.frozenAtMillis = restStartMillis + secondsIn * 1_000L + 500L
        mainClock.advanceTimeBy(1_000L)
        shadowOf(Looper.getMainLooper()).idleFor(Duration.ofSeconds(1))
    } finally {
        mainClock.autoAdvance = true
    }
    waitForIdle()
}

/** Let the app clock run on after [holdClockInRest]. */
internal fun releaseClock() {
    TestClock.frozenAtMillis = null
}
