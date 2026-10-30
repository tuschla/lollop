package app.milestone.qa

import android.content.Context
import android.os.Looper
import app.milestone.AppClock
import app.milestone.ConcurrentGoal
import app.milestone.Core
import app.milestone.Event
import app.milestone.EventDispatcher
import app.milestone.GoalDistance
import app.milestone.GpsPoint
import app.milestone.HistoryFocus
import app.milestone.LiftGoal
import app.milestone.PrescriptionView
import app.milestone.ProgressionCadence
import app.milestone.ResistanceModality
import app.milestone.RunSession
import app.milestone.ViewModel
import app.milestone.isReturnCheckItem
import app.milestone.isRunItem
import app.milestone.setupPrefs
import app.milestone.UiMessages
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.async
import org.junit.Assert.fail
import org.robolectric.Shadows.shadowOf
import java.io.File

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

    private fun logFile(ctx: Context) = File(ctx.filesDir, "event-log.ndjson")

    /**
     * Reset every process-scoped bit of state the app singletons hold so the next
     * launched Activity boots as if the process were fresh: the native core's
     * bridge, Core's replay guard + cached view, the EventDispatcher queue,
     * consumer, boot gate and model flow, the setup SharedPreferences, and the
     * on-disk event log.
     *
     * Robolectric shares one sandbox classloader across the whole suite, so these
     * statics survive between tests; without this reset a prior scenario's core
     * state and "already booted" flags would leak into the next.
     */
    fun reset(ctx: Context) {
        // A prior test may have bumped the font scale (the 1.3 clipping check);
        // it is process-global under Robolectric, so restore the default here or
        // it leaks into every later scenario's layout.
        org.robolectric.RuntimeEnvironment.setFontScale(1.0f)

        // Snackbar queue and History focus are process-scoped: a message posted by
        // a prior scenario must not surface in this one's screenshots.
        UiMessages.clear()
        HistoryFocus.consume()

        // Pin the zone and the app clock to a fixed Monday morning so every
        // scenario sees the same week: Monday is a lift day in every seeded plan,
        // so "today's session" exists no matter which weekday the suite runs on,
        // and screenshots carry stable dates.
        java.util.TimeZone.setDefault(java.util.TimeZone.getTimeZone("UTC"))
        AppClock.offsetMillis = PINNED_NOW_MILLIS - System.currentTimeMillis()

        // Native core: drop the global bridge and rebuild it empty.
        Core.resetForTest()

        // Core's private replay/caching state (no public setters — this is the
        // one place that legitimately reaches in, for test isolation only).
        setPrivate(Core, "restored", false)
        setPrivate(Core, "log", null)
        setPrivate(Core, "lastGoodView", null)
        setPrivate(Core, "appendTornGuard", false)
        setPrivate(Core, "lastSetToday", null)
        setPrivate(Core, "viewStale", false)
        setPrivate(Core, "logDegraded", false)

        // Process-scoped session singletons (internal, same module → direct). The
        // dispatcher gets a fresh queue and consumer: a prior test's unit still in
        // flight when Robolectric recycled the main looper would otherwise have left
        // a consumer that never resumes, so this boot would wait on it forever (the
        // splash cancelling every frame, ActivityScenario.launch spinning).
        EventDispatcher.resetForTest()
        injectCoreLatency()
        // The run tracker's process singleton (live run, plan guide, save latch): a
        // scenario that left a run recording must not reopen the tracker in the next.
        app.milestone.RunSession.reset()
        // The set loop's running rest drives its readout; a prior scenario's rest
        // (or its exact-alarm notice) must not show in the next loop.
        app.milestone.RestAlarm.cancel(ctx)
        app.milestone.RestAlarm.resetProcessState()
        // The widget snapshot sync started by an earlier test's Activity is bound
        // to that test's Application (and files dir); the next launch starts it
        // again for this one.
        app.milestone.widget.WidgetSync.resetForTest()

        // Onboarding / plan-cleared markers live here; a stale onboarding_done
        // would suppress the fresh-install guided setup.
        ctx.setupPrefs().edit().clear().commit()
        ctx.getSharedPreferences("theme", Context.MODE_PRIVATE).edit().clear().commit()

        logFile(ctx).delete()
        File(ctx.filesDir, "event-log.ndjson.bak").delete()
        File(ctx.filesDir, "event-log.ndjson.tmp").delete()
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
        val send = EventDispatcher.sender
        val sendAll = EventDispatcher.batchSender
        EventDispatcher.sender = { e -> send(e).also { lag(ms) } }
        EventDispatcher.batchSender = { es -> sendAll(es).also { lag(ms) } }
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

    /** Point [app.milestone.ActiveRunStore] at [ctx]'s files dir. The store caches
     *  its file from the first init in this JVM, which may be another test's (now
     *  gone) files dir. */
    fun resetActiveRunStore(ctx: Context) {
        setPrivate(app.milestone.ActiveRunStore, "file", null)
        app.milestone.ActiveRunStore.init(ctx)
    }

    /** Run [block] on the app's process scope and pump the paused main looper until
     *  it completes: the serial event consumer and every `Dispatchers.Main` hop
     *  resume there, so a plain runBlocking on this thread would deadlock. */
    @OptIn(ExperimentalCoroutinesApi::class)
    fun <T> onAppScope(timeoutMs: Long = 30_000, block: suspend () -> T): T {
        val job = EventDispatcher.scope.async { block() }
        val end = System.currentTimeMillis() + timeoutMs
        while (!job.isCompleted) {
            shadowOf(Looper.getMainLooper()).idle()
            if (System.currentTimeMillis() > end) fail("app-scope work did not finish in ${timeoutMs}ms")
            Thread.sleep(2)
        }
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
        sessionType: String,
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
     * SetToday derives from [app.milestone.todayEpochDay]), at midday, so seeded
     * history lands inside the History screen's rolling windows.
     */
    fun daysAgoAtMidday(days: Int): Long =
        (app.milestone.todayEpochDay() - days) * 86_400L + 43_200L
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
        RunSession.tracking.value = true
    }

    private fun setPrivate(target: Any, field: String, value: Any?) {
        val f = target.javaClass.getDeclaredField(field)
        f.isAccessible = true
        f.set(target, value)
    }
}
