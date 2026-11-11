package app.milestone.qa

import android.content.Context
import android.os.Looper
import app.milestone.AppClock
import app.milestone.AppFileNames
import app.milestone.AppRuntime
import app.milestone.ConcurrentGoal
import app.milestone.Core
import app.milestone.DurableCore
import app.milestone.Event
import app.milestone.EventDispatcher
import app.milestone.GoalDistance
import app.milestone.GpsPoint
import app.milestone.LiftGoal
import app.milestone.PrescriptionView
import app.milestone.ProgressionCadence
import app.milestone.ResistanceModality
import app.milestone.RunSession
import app.milestone.RecentRace
import app.milestone.SendResult
import app.milestone.SessionType
import app.milestone.TestClock
import app.milestone.TestSink
import app.milestone.ViewModel
import app.milestone.isReturnCheckItem
import app.milestone.isRunItem
import app.milestone.setupPrefs
import app.milestone.todayEpochDay
import app.milestone.toJson
import app.milestone.utcOffsetSec
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.junit4.ComposeTestRule
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.async
import org.junit.Assert.fail
import org.robolectric.Shadows.shadowOf
import java.io.File
import java.util.Locale

/**
 * Offline QA harness: seeds the durable event log the REAL core replays on boot
 * and resets all process-scoped state between scenarios so each Robolectric test
 * starts from a clean slate.
 *
 * Seeding writes the exact ndjson wire form [app.milestone.Core] itself persists
 * (`event.toJson().toString()` per line — see Core.apply), so a scenario is
 * indistinguishable from a real user's history: [Core.restore] replays it through
 * JNI on launch, no shell-side shortcut.
 */
object QaHarness {

    internal fun logFile(ctx: Context) = File(ctx.filesDir, AppFileNames.EVENT_LOG)

    /** The durable log's non-blank lines. */
    fun logLines(ctx: Context): List<String> = logFile(ctx).readLines().filter { it.isNotBlank() }

    /** Process death, then the boot sequence: restore (compaction + replay) and the
     *  boot SetToday. */
    fun reboot(ctx: Context): SendResult {
        killProcess()
        AppRuntime.engine.restore(ctx)
        return AppRuntime.engine.send(Event.SetToday(todayEpochDay(), utcOffsetSec()))
    }

    /**
     * A fresh install in a fresh process, as each scenario starts: the app's process
     * ([freshProcess]) on the [TestClock] pinned to [PINNED_NOW_MILLIS] in UTC, the
     * JVM locale US, the default font scale, and no setup or theme preferences, no
     * event log and no rest left from an earlier scenario. Robolectric runs the
     * whole suite in one JVM, so without this a prior scenario's state would leak.
     */
    fun reset(ctx: Context) {
        Locale.setDefault(Locale.US)
        // A prior test may have bumped the font scale (the 1.3 clipping check);
        // it is process-global under Robolectric.
        org.robolectric.RuntimeEnvironment.setFontScale(1.0f)

        // A fixed Monday morning: Monday is a lift day in every seeded plan, so
        // "today's session" exists whichever weekday the suite runs on, and
        // screenshots carry stable dates.
        java.util.TimeZone.setDefault(java.util.TimeZone.getTimeZone("UTC"))
        TestClock.offsetMillis = PINNED_NOW_MILLIS - System.currentTimeMillis()
        freshProcess(engine = null)
        injectCoreLatency()

        // The rest's alarm, notifications and saved rest outlive a process.
        app.milestone.RestAlarm.cancel(ctx)
        ctx.setupPrefs().edit().clear().commit()
        ctx.getSharedPreferences(AppFileNames.SETTINGS_STORE, Context.MODE_PRIVATE).edit().clear().commit()

        logFile(ctx).delete()
        for (suffix in AppFileNames.LOG_SUFFIXES) File(ctx.filesDir, AppFileNames.EVENT_LOG + suffix).delete()
    }

    /** Process death: the launched Activity closed, the run sidecar's queued
     *  appends written (the dying process's last instants), then [freshProcess];
     *  every file and preference stays. */
    fun killProcess() {
        teardown()
        app.milestone.ActiveRunStore.flush()
        freshProcess()
    }

    // The engine a test runs the app on instead of the platform's; a process death
    // keeps it, [reset] drops it.
    private var engine: DurableCore? = null

    /** A new process on the same disk ([AppRuntime.resetForTest]), on the test
     *  clock and the real send path ([TestSink]), and on [engine] when a test runs
     *  its own. */
    fun freshProcess(engine: DurableCore? = this.engine) {
        this.engine = engine
        TestSink.reset()
        AppRuntime.resetForTest(engine, TestSink, TestClock)
        // Scenarios drive the platform engine directly too, outside the process.
        Core.forgetProcess()
    }

    /**
     * Race finder, off by default: with `QA_CORE_LATENCY_MS=<n>` in the test JVM's
     * environment, every queued core send (single and batch) holds its result back
     * n ms AFTER the core applied it, off the main looper. The event is then in the
     * durable log while the UI still shows the previous model, which is what a
     * loaded JVM does on a slow view build. A test that syncs on the log (or on
     * nothing) and then expects the new model on screen fails under it; sync on
     * the published [EventDispatcher.model] instead. Run with `--rerun`: Gradle
     * does not see an environment change.
     */
    private fun injectCoreLatency() {
        val ms = System.getenv("QA_CORE_LATENCY_MS")?.toLongOrNull()?.takeIf { it > 0 } ?: return
        val send = TestSink.sender
        val sendAll = TestSink.batchSender
        TestSink.sender = { e -> send(e).also { lag(ms) } }
        TestSink.batchSender = { es -> sendAll(es).also { lag(ms) } }
    }

    private suspend fun lag(ms: Long) = kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) { Thread.sleep(ms) }

    /**
     * Close the launched Activity at the END of a test (valid lifecycle point,
     * unlike closing it during the next test's setup). Prevents boot effects and
     * the process-scoped event consumer of finished tests from accumulating and
     * starving the single-thread dispatcher when the whole suite runs together.
     */
    fun teardown() {
        runCatching { lastScenario?.close() }
        lastScenario = null
    }

    /** Write [events] as the durable log the boot restore replays. */
    fun seedLog(ctx: Context, events: List<Event>) {
        val f = logFile(ctx)
        f.parentFile?.mkdirs()
        f.writeText(events.joinToString("") { it.toJson().toString() + "\n" })
    }

    /** Point the run sidecar store at [ctx]'s files dir and empty it. */
    fun clearSidecar(ctx: Context) {
        app.milestone.ActiveRunStore.start(ctx)
        app.milestone.ActiveRunStore.clearSync()
        File(ctx.filesDir, AppFileNames.ACTIVE_RUN).delete()
    }

    /** Idle the paused main looper until [done] holds (the core's replies and every
     *  `Dispatchers.Main` hop land there); fails after [timeoutMs]. */
    fun pumpMainUntil(what: String, timeoutMs: Long = 15_000, done: () -> Boolean) {
        val end = System.currentTimeMillis() + timeoutMs
        while (!done()) {
            shadowOf(Looper.getMainLooper()).idle()
            if (System.currentTimeMillis() > end) fail("timed out after ${timeoutMs}ms waiting for $what")
            Thread.sleep(2)
        }
    }

    /** Run [block] on the app's process scope and pump the paused main looper until
     *  it completes: the serial event consumer and every `Dispatchers.Main` hop
     *  resume there, so a plain runBlocking on this thread would deadlock. */
    @OptIn(ExperimentalCoroutinesApi::class)
    fun <T> onAppScope(timeoutMs: Long = 30_000, block: suspend () -> T): T {
        val job = AppRuntime.scope.async { block() }
        pumpMainUntil("app-scope work", timeoutMs) { job.isCompleted }
        return job.getCompleted()
    }

    /** The bytes of test resource [name] (a path under `src/test/resources` or a
     *  `shared/tests/fixtures` source root). */
    fun fixture(name: String): ByteArray =
        checkNotNull(QaHarness::class.java.classLoader!!.getResourceAsStream(name)) { "fixture $name not on the test classpath" }
            .use { it.readBytes() }

    /**
     * Log [items] of a [sessionType] plan session to the real core, dated midday on
     * [day], with entry ids after [firstId]: a `LogRun` per run item and each lift's
     * sets at its top reps (and hold), skipping a return check. [onModel] sees the
     * model after each send. Null when nothing was logged.
     */
    fun logPlanItems(
        day: Long,
        sessionType: SessionType,
        items: List<PrescriptionView>,
        firstId: Long,
        onModel: (ViewModel, PrescriptionView) -> Unit = { _, _ -> },
    ): ViewModel? {
        var v: ViewModel? = null
        var id = firstId
        val at = day * 86_400L + 43_200L
        for (item in items) {
            if (isRunItem(item, sessionType)) {
                v = Core.send(Event.LogRun(distanceKm = item.run_head_km ?: 6.0, durationMin = 40.0, hrPctMax = 0.7, observedAt = at, entryId = ++id))
                    .view.also { onModel(it, item) }
            } else if (!isReturnCheckItem(item, sessionType)) {
                repeat(item.sets.coerceAtLeast(1)) {
                    v = Core.send(
                        Event.LogSet(
                            exercise = item.exercise, weightKg = item.load_kg ?: 0.0, reps = item.reps_high.coerceAtLeast(1), rpe = 7.0,
                            observedAt = at, entryId = ++id, holdSec = item.hold_high_sec.takeIf { s -> item.timed_hold && s > 0 },
                        ),
                    ).view.also { onModel(it, item) }
                }
            }
        }
        return v
    }

    // ── Scenario event builders ────────────────────────────────────────────

    /** Monday 2026-09-21 10:00 UTC: the instant [reset] pins [AppClock] to. */
    val PINNED_NOW_MILLIS: Long = java.time.Instant.parse("2026-09-21T10:00:00Z").toEpochMilli()

    /**
     * A unix-seconds instant `days` before the pinned "today" (which the boot
     * SetToday derives from [app.milestone.todayEpochDay]), at midday plus
     * [plusSec], so seeded history lands inside the History screen's rolling windows.
     */
    fun daysAgoAtMidday(days: Int, plusSec: Long = 0): Long =
        (app.milestone.todayEpochDay() - days) * 86_400L + 43_200L + plusSec

    /**
     * The exact RunCard distance label for [km] under the current locale's default
     * unit (miles for US/GB/…, km elsewhere), computed with the app's own
     * formatter so the run-history assertion is unit-locale-independent.
     */
    fun expectedRunDistanceLabel(km: Double): String =
        app.milestone.runDistLabel(
            km,
            app.milestone.resolveDistanceUnit(app.milestone.DistanceUnitOverride.System),
        )

    /**
     * A set-up strength lifter with no logged history. Boot sees a profile but no
     * program and auto-generates the plan, so Today renders the prescription hero.
     */
    fun strengthProfile(): Event.SetProfile = Event.SetProfile(
        progressionCadence = ProgressionCadence.WeekToWeek,
        liftGoal = LiftGoal.MaxStrength,
        goalDistance = GoalDistance.General,
        concurrentGoal = ConcurrentGoal.Strength,
        weeklySets = 12,
        runningDaysPerWeek = 0,
        runningKmPerWeek = 0.0,
        advanced = false,
        bodyweightKg = 85.0,
        ageYears = 30.0,
    )

    /** [strengthProfile] with loads displayed in pounds. */
    fun strengthProfileLb(): Event.SetProfile = strengthProfile().copy(weightLb = true)

    /** A calisthenics athlete profile, for a bodyweight/hold-set history. */
    fun calisthenicsProfile(): Event.SetProfile =
        strengthProfile().copy(bodyweightKg = 72.0, resistanceModality = ResistanceModality.Calisthenics)

    /** [strengthProfile] training for a 10K on 3 running days, 25 km a week. */
    fun hybridTenKProfile(): Event.SetProfile =
        strengthProfile().copy(goalDistance = GoalDistance.TenK, runningDaysPerWeek = 3, runningKmPerWeek = 25.0)

    /** [strengthProfile] at 80 kg also training for a 5K on 3 running days, 20 km a
     *  week: lifts and runs, so every chooser row and every calculator is offered. */
    fun hybridFiveKProfile(): Event.SetProfile = strengthProfile().copy(
        goalDistance = GoalDistance.FiveK, runningDaysPerWeek = 3, runningKmPerWeek = 20.0, bodyweightKg = 80.0,
    )

    /** An advanced 10K runner (4 days, 35 km a week, no lifting) with a resting HR and
     *  a 25:00 5K two weeks ago, so run items carry pace, heart-rate and effort targets. */
    fun tenKRunnerProfile(): Event.SetProfile = strengthProfile().copy(
        goalDistance = GoalDistance.TenK, concurrentGoal = ConcurrentGoal.EndurancePriority, weeklySets = 0,
        runningDaysPerWeek = 4, runningKmPerWeek = 35.0, advanced = true, bodyweightKg = 70.0, restingHrBpm = 55.0,
        recentRace = RecentRace(distance_km = 5.0, time_sec = 25 * 60, epoch_day = app.milestone.todayEpochDay() - 14),
    )

    /** A 5K runner (4 days, 30 km a week, no lifting) with a 25:00 5K a week ago. */
    fun fiveKRunnerProfile(): Event.SetProfile = strengthProfile().copy(
        goalDistance = GoalDistance.FiveK, concurrentGoal = ConcurrentGoal.EndurancePriority, weeklySets = 0,
        runningDaysPerWeek = 4, runningKmPerWeek = 30.0, bodyweightKg = 70.0,
        recentRace = RecentRace(distance_km = 5.0, time_sec = 1500, epoch_day = app.milestone.todayEpochDay() - 7),
    )

    /** A 15-year-old 60 kg lifter (age only, no birthday). */
    fun youthProfile(): Event.SetProfile = strengthProfile().copy(bodyweightKg = 60.0, ageYears = 15.0)

    /** Seed [strengthProfile] with a Back Squat set 6, 4 and 2 days ago. */
    fun seedStrengthHistory(ctx: Context) = seedLog(
        ctx,
        listOf(strengthProfile(), loggedSquat(daysAgoAtMidday(6)), loggedSquat(daysAgoAtMidday(4)), loggedSquat(daysAgoAtMidday(2))),
    )

    /** A logged reps×load lift set (observed_at anchored to a fixed instant). */
    fun loggedSquat(observedAt: Long): Event = Event.LogSet(
        exercise = "Back Squat",
        weightKg = 120.0,
        reps = 3,
        rpe = 8.0,
        observedAt = observedAt,
        entryId = observedAt * 1000,
    )

    /** A logged timed-hold set (calisthenics), for the History hold-set render. */
    fun loggedHold(observedAt: Long): Event = Event.LogSet(
        exercise = "Front Lever",
        weightKg = 0.0,
        reps = 1,
        rpe = 0.0,
        observedAt = observedAt,
        entryId = observedAt * 1000,
        holdSec = 30,
    )

    /** A logged run, for the run-history render. */
    fun loggedRun(observedAt: Long): Event = Event.LogRun(
        distanceKm = 5.0,
        durationMin = 27.5,
        hrPctMax = 0.0,
        observedAt = observedAt,
        entryId = observedAt * 1000,
    )

    /** A 10.1 km GPS run at an even 5:00 /km, heading north: 1011 clean fixes, 10 m
     *  apart every 3 s, the first at [observedAt]. */
    fun gpsTenK(observedAt: Long, sessionRpe: Int? = null): Event.LogRunTrack {
        val points = (0 until 1011).map { i ->
            GpsPoint(lat = 47.0 + i * 10.0 / 111_195.0, lon = 8.0, observedAt = observedAt + i * 3L, accuracyM = 5.0)
        }
        return Event.LogRunTrack(
            points = points,
            hrPctMax = 0.0,
            observedAt = observedAt,
            entryId = observedAt * 1000,
            sessionRpe = sessionRpe,
        )
    }

    /** A live recording as the service leaves it: 21 fixes, ~30 m every 10 s (600 m,
     *  200 s moving, past the short-run gate), tracking on. The fix stamps sit far
     *  ahead of Robolectric's elapsed clock, so the GPS pill reads a steady
     *  "Recording". */
    fun seedRecordingRun() {
        val t0 = AppClock.nowSec() - 400
        for (i in 0..20) {
            RunSession.add(GpsPoint(52.5200 + i * 0.00027, 13.4050, t0 + i * 10L, 5.0), (1_000L + i * 10L) * 1_000_000_000L)
        }
        RunSession.recordingStarted()
    }
}

/** Wait until at least one node matches [matcher]. */
internal fun ComposeTestRule.waitForNode(matcher: SemanticsMatcher, timeoutMs: Long = 15_000) =
    waitUntil(timeoutMs) { onAllNodes(matcher).fetchSemanticsNodes().isNotEmpty() }

/** How many nodes match [matcher] now. */
internal fun ComposeTestRule.countOf(matcher: SemanticsMatcher): Int = onAllNodes(matcher).fetchSemanticsNodes().size

/** Wait until the launched app has booted (the boot effect ran and a model is published). */
internal fun ComposeTestRule.waitForBoot() {
    waitUntil(30_000) { AppRuntime.bootHandled && EventDispatcher.model.value != null }
    waitForIdle()
}
