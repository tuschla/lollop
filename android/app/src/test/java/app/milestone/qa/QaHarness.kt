package app.milestone.qa

import android.content.Context
import app.milestone.AppClock
import app.milestone.ConcurrentGoal
import app.milestone.Core
import app.milestone.Event
import app.milestone.EventDispatcher
import app.milestone.GoalDistance
import app.milestone.HistoryFocus
import app.milestone.LiftGoal
import app.milestone.ProgressionCadence
import app.milestone.ResistanceModality
import app.milestone.setupPrefs
import app.milestone.UiMessages
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
     * bridge, Core's replay guard + cached view, the EventDispatcher boot gate and
     * model flow, the setup SharedPreferences, and the on-disk event log.
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

        // Process-scoped session singletons (internal, same module → direct).
        EventDispatcher.model.value = null
        EventDispatcher.bootHandled = false
        EventDispatcher.recoveredSidecar = null
        // The run tracker's process singleton (live run, plan guide, save latch): a
        // scenario that left a run recording must not reopen the tracker in the next.
        app.milestone.RunSession.reset()
        // The set loop's running rest drives its readout; a prior scenario's rest
        // must not show in the next loop.
        app.milestone.RestAlarm.cancel(ctx)

        // Onboarding / plan-cleared markers live here; a stale onboarding_done
        // would suppress the fresh-install guided setup.
        ctx.setupPrefs().edit().clear().commit()
        ctx.getSharedPreferences("theme", Context.MODE_PRIVATE).edit().clear().commit()

        logFile(ctx).delete()
        File(ctx.filesDir, "event-log.ndjson.bak").delete()
        File(ctx.filesDir, "event-log.ndjson.tmp").delete()
    }

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
    fun strengthProfile(): Event = Event.SetProfile(
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
    fun strengthProfileLb(): Event = Event.SetProfile(
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
        weightLb = true,
    )

    /** A calisthenics athlete profile, for a bodyweight/hold-set history. */
    fun calisthenicsProfile(): Event = Event.SetProfile(
        progressionCadence = ProgressionCadence.WeekToWeek,
        liftGoal = LiftGoal.MaxStrength,
        goalDistance = GoalDistance.General,
        concurrentGoal = ConcurrentGoal.Strength,
        weeklySets = 12,
        runningDaysPerWeek = 0,
        runningKmPerWeek = 0.0,
        advanced = false,
        bodyweightKg = 72.0,
        ageYears = 30.0,
        resistanceModality = ResistanceModality.Calisthenics,
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
        longestRecentKm = 0.0,
        observedAt = observedAt,
        entryId = observedAt * 1000,
    )

    private fun setPrivate(target: Any, field: String, value: Any?) {
        val f = target.javaClass.getDeclaredField(field)
        f.isAccessible = true
        f.set(target, value)
    }
}
