package app.milestone.qa

import app.milestone.ConcurrentGoal
import app.milestone.Event
import app.milestone.GoalDistance
import app.milestone.GpsPoint
import app.milestone.GpxFix
import app.milestone.LiftGoal
import app.milestone.PainDetail
import app.milestone.PainKind
import app.milestone.PainTrend
import app.milestone.ProgressionCadence
import app.milestone.ReadinessSignal
import app.milestone.WorkoutType
import app.milestone.importedRunEvent
import app.milestone.lbToKgWire
import kotlin.math.cos
import kotlin.math.roundToLong
import kotlin.math.sin

/**
 * Deterministic PRNG (the same 64-bit LCG as the core's `view_equivalence` test),
 * so a long history is identical on every run and machine.
 */
internal class Lcg(private var state: Long) {
    fun next(): Double {
        state = state * 6_364_136_223_846_793_005L + 1_442_695_040_888_963_407L
        return (state ushr 11).toDouble() / (1L shl 53).toDouble()
    }

    fun range(lo: Double, hi: Double): Double = lo + (hi - lo) * next()

    fun int(lo: Int, hiInclusive: Int): Int = (lo + (next() * (hiInclusive - lo + 1)).toInt()).coerceAtMost(hiInclusive)

    fun chance(p: Double): Boolean = next() < p
}

/** How a seeded run reached the log: the GPS tracker, the manual run editor, or a GPX import. */
internal enum class RunSource { Tracked, Manual, Imported }

/** One seeded run as the generator built it (the oracle the screen is checked against). */
internal data class SeededRun(
    val observedAt: Long,
    val entryId: Long,
    val source: RunSource,
    /** Geometric length of the generated route (tracked/imported) or the typed distance (manual). */
    val km: Double,
    val durationSec: Long,
    val workoutType: WorkoutType?,
    val sessionRpe: Int?,
)

/**
 * A 2–3 year training history of a hybrid lifter-runner, built from exactly the
 * wire events the app itself sends (LogSet, LogRunTrack from the tracker, LogRun
 * from the run editor, LogRunTrack from [importedRunEvent] for GPX imports,
 * SubmitCheckin, pain/illness reports and their ResolveHold, SetProfile edits,
 * GeneratePlan), in the order the app would have appended them.
 */
internal class LongHistory(
    val events: List<Event>,
    val sets: List<Event.LogSet>,
    val runs: List<SeededRun>,
    /** Local (UTC) epoch day of the pinned "today". */
    val today: Long,
    val longestRun: SeededRun,
    /** Local epoch day of the session with the most exercises (the "big" session). */
    val bigSessionDay: Long,
    val bigSessionExercises: Int,
    val bigSessionSets: Int,
    val lb: Boolean,
) {
    /** Distinct lifting days (one History session card each). */
    val liftDays: Set<Long> get() = sets.map { Math.floorDiv(it.observedAt, 86_400L) }.toSet()
}

internal object LongHistorySeed {

    private const val HOME_LAT = 52.52
    private const val HOME_LON = 13.405
    private const val M_PER_DEG_LAT = 111_195.0

    /** Longest run of the history: 34.2 km = 21.25 mi, so it folds its splits in either unit. */
    const val LONGEST_KM = 34.2

    /** Hybrid athlete: lifts 3×/week for strength, runs toward a half marathon. */
    fun profile(lb: Boolean, bodyweightKg: Double, effectiveFrom: Long?): Event = Event.SetProfile(
        progressionCadence = ProgressionCadence.WeekToWeek,
        liftGoal = LiftGoal.MaxStrength,
        goalDistance = GoalDistance.HalfMarathon,
        concurrentGoal = ConcurrentGoal.Strength,
        weeklySets = 12,
        runningDaysPerWeek = 3,
        runningKmPerWeek = 30.0,
        advanced = false,
        bodyweightKg = bodyweightKg,
        ageYears = 34.0,
        restingHrBpm = 52.0,
        measuredHrMax = 190.0,
        weightLb = lb,
        effectiveFromEpochDay = effectiveFrom,
    )

    private val dayA = listOf("Back Squat" to 100.0, "Bench Press" to 75.0, "Barbell Row" to 65.0, "Pull-up" to 0.0)
    private val dayB = listOf("Deadlift" to 130.0, "Overhead Press" to 47.5, "Romanian Deadlift" to 90.0, "Lat Pulldown" to 55.0)
    private val dayC = listOf("Front Squat" to 80.0, "Incline Bench Press" to 60.0, "Dumbbell Row" to 30.0, "Hip Thrust" to 90.0, "Plank" to 0.0)
    private val bigDay = listOf(
        "Back Squat" to 100.0, "Bench Press" to 75.0, "Deadlift" to 130.0, "Overhead Press" to 47.5,
        "Barbell Row" to 65.0, "Romanian Deadlift" to 90.0, "Lat Pulldown" to 55.0, "Biceps Curl" to 15.0,
    )

    /**
     * Build the history. [days] is the span of in-app logging ending yesterday;
     * [fixEverySec] the GPS cadence of tracked/imported runs (larger = lighter log).
     */
    fun build(days: Int = 900, lb: Boolean = false, fixEverySec: Long = 10L, seed: Long = 0x5EED_1A57L): LongHistory {
        val rng = Lcg(seed)
        val today = Math.floorDiv(QaHarness.PINNED_NOW_MILLIS, 86_400_000L)
        val first = today - days
        fun at(day: Long, hour: Int, minute: Int = 0): Long = day * 86_400L + hour * 3_600L + minute * 60L
        // Monday = 0 (epoch day 0 was a Thursday).
        fun dow(day: Long): Int = Math.floorMod(day + 3, 7L).toInt()

        val events = ArrayList<Event>()
        val sets = ArrayList<Event.LogSet>()
        val runs = ArrayList<SeededRun>()

        events += profile(lb, 84.0, first)
        events += Event.GeneratePlan(first)

        // Blocked spans: a sharp knee (pain hold) and two illnesses; no training inside.
        val painStart = today - 410
        val painEnd = painStart + 9
        val illStart = today - 205
        val illEnd = illStart + 6
        val coldStart = today - 62
        val coldEnd = coldStart + 4
        // A two-week holiday each summer.
        val holidays = listOf(today - 380 until today - 366, today - 15 - 365 * 2 until today - 365 * 2)
        fun blocked(day: Long) = day in painStart..painEnd || day in illStart..illEnd ||
            day in coldStart..coldEnd || holidays.any { day in it }

        // The marathon-prep long-run ramp of the last 16 weeks: the peak is LONGEST_KM.
        val longestDay = today - 36 + (6 - dow(today - 36)) // a Sunday ~5 weeks back
        val bigSessionDay = today - 3 // last Friday (today is a Monday)
        var longest: SeededRun? = null

        // Imports: older watch runs from before the app existed, imported a month in.
        val importAfterDay = first + 30
        val imported = ArrayList<Pair<Event, SeededRun>>()
        run {
            var d = first - 200
            while (d < first - 2) {
                val km = Math.round(rng.range(5.0, 14.0) * 100.0) / 100.0
                val pace = rng.range(320.0, 370.0)
                val (ev, sr) = importedRun(rng, d, km, pace, fixEverySec)
                imported += ev to sr
                d += rng.int(5, 11)
            }
        }

        var lastBodyweight = 84.0
        var dayIndex = 0
        for (day in first until today) {
            dayIndex++
            val progress = (day - first).toDouble() / days
            if (day == importAfterDay) {
                imported.forEach { (ev, sr) -> events += ev; runs += sr }
            }
            // A bodyweight update twice a year (a realistic profile edit).
            if ((day - first) % 182 == 90L) {
                lastBodyweight = Math.round((lastBodyweight + rng.range(-1.5, 1.0)) * 10.0) / 10.0
                events += profile(lb, lastBodyweight, day)
            }
            if (day == painStart) {
                events += Event.SubmitReadiness(
                    ReadinessSignal.Pain, 1.0, at(day, 8),
                    pain = PainDetail(PainKind.SharpJoint, severity = 6, trend = PainTrend.Stable, persists = true, location = "Left knee"),
                )
            }
            if (day == painEnd + 1) events += Event.ResolveHold(ReadinessSignal.Pain, at(day, 8))
            if (day == illStart) events += Event.SubmitReadiness(ReadinessSignal.Illness, 2.0, at(day, 7, 30))
            if (day == illEnd + 1) events += Event.ResolveHold(ReadinessSignal.Illness, at(day, 7, 30))
            if (day == coldStart) events += Event.SubmitReadiness(ReadinessSignal.Illness, 1.0, at(day, 7, 30))
            if (day == coldEnd + 1) events += Event.ResolveHold(ReadinessSignal.Illness, at(day, 7, 30))

            // Morning check-in on most days.
            if (rng.chance(0.7)) {
                events += Event.SubmitCheckin(
                    observedAt = at(day, 7, rng.int(0, 20)),
                    sleepQuality = rng.int(3, 5),
                    soreness = rng.int(1, 3),
                    mood = rng.int(3, 4),
                    fatigue = rng.int(2, 3),
                    stress = rng.int(2, 3),
                    restingHrBpm = (52 + rng.int(-2, 2)).toDouble(),
                    hrvRmssdMs = (62 + rng.int(-6, 6)).toDouble(),
                )
            }
            if (blocked(day)) continue

            val wd = dow(day)
            // ── Lifting: Mon / Wed / Fri, A-B-C rotation.
            if (wd == 0 || wd == 2 || wd == 4) {
                val big = day == bigSessionDay
                if (big || rng.chance(0.88)) {
                    val plan = when {
                        big -> bigDay
                        wd == 0 -> dayA
                        wd == 2 -> dayB
                        else -> dayC
                    }
                    var t = at(day, 17, 30)
                    for ((exercise, base) in plan) {
                        val n = if (big) 4 else rng.int(3, 5)
                        val reps = if (exercise == "Plank") 1 else rng.int(3, 8)
                        // ~45 % stronger over the span, with day-to-day noise.
                        val kg = if (base == 0.0) 0.0 else base * (1.0 + 0.45 * progress) * rng.range(0.96, 1.03)
                        val wire = if (base == 0.0) {
                            0.0
                        } else if (lb) {
                            lbToKgWire(Math.round(kg * 2.20462 / 5.0) * 5.0)
                        } else {
                            Math.round(kg / 2.5) * 2.5
                        }
                        for (s in 0 until n) {
                            t += rng.int(120, 240)
                            val rpe = if (exercise == "Plank" || rng.chance(0.15)) 0.0 else listOf(7.0, 7.5, 8.0, 8.5, 9.0)[rng.int(0, 4)]
                            val ev = Event.LogSet(
                                exercise = exercise,
                                weightKg = wire,
                                reps = reps,
                                rpe = rpe,
                                observedAt = t,
                                entryId = t * 1000,
                                holdSec = if (exercise == "Plank") 45 + 15 * rng.int(0, 3) else null,
                            )
                            sets += ev
                            events += ev
                        }
                    }
                }
            }
            // ── Running: Tue / Thu easy-ish, Sat tempo, Sun long.
            val runKind: WorkoutType? = when (wd) {
                1, 3 -> if (rng.chance(0.85)) WorkoutType.Easy else null
                5 -> if (rng.chance(0.7)) WorkoutType.Tempo else null
                6 -> if (day == longestDay || rng.chance(0.8)) WorkoutType.LongRun else null
                else -> null
            }
            if (runKind != null) {
                val inRamp = day > today - 112
                val km = when {
                    day == longestDay -> LONGEST_KM
                    runKind == WorkoutType.LongRun && inRamp ->
                        Math.min(LONGEST_KM - 1.0, 16.0 + (day - (today - 112)) * 0.22 + rng.range(-1.0, 1.0))
                    runKind == WorkoutType.LongRun -> rng.range(12.0, 18.0)
                    runKind == WorkoutType.Tempo -> rng.range(7.0, 10.0)
                    else -> rng.range(5.0, 9.0)
                }.let { Math.round(it * 100.0) / 100.0 }
                val pace = when (runKind) {
                    WorkoutType.Tempo -> rng.range(275.0, 300.0)
                    WorkoutType.LongRun -> rng.range(345.0, 365.0)
                    else -> rng.range(330.0, 375.0)
                } * (1.0 - 0.06 * progress)
                // Untagged ~15 %: the history card then titles it by weekday.
                val tag = if (rng.chance(0.15) && day != longestDay) null else runKind
                val rpe = if (rng.chance(0.4)) rng.int(3, 8) else null
                val manual = day != longestDay && rng.chance(0.2)
                val start = at(day, 7, rng.int(0, 40))
                val dur = Math.round(km * pace)
                val sr: SeededRun
                if (manual) {
                    val entryId = start * 1000 + 7
                    val obs = start + dur
                    events += Event.LogRun(
                        distanceKm = km, durationMin = dur / 60.0, hrPctMax = 0.0, longestRecentKm = 0.0,
                        observedAt = obs, entryId = entryId, workoutType = tag, sessionRpe = rpe,
                        avgHrBpm = if (rng.chance(0.5)) rng.int(138, 158).toDouble() else null,
                    )
                    sr = SeededRun(obs, entryId, RunSource.Manual, km, dur, tag, rpe)
                } else {
                    val pauseAt = if (km > 12.0 && rng.chance(0.3)) rng.range(0.3, 0.7) else null
                    val (points, starts, routeKm) = route(rng, start, km, pace, fixEverySec, pauseAt)
                    val entryId = start * 1000 + 3
                    val obs = points.last().observedAt
                    events += Event.LogRunTrack(
                        points = points, hrPctMax = 0.0, longestRecentKm = 0.0,
                        observedAt = obs, entryId = entryId, workoutType = tag, segmentStarts = starts, sessionRpe = rpe,
                    )
                    sr = SeededRun(obs, entryId, RunSource.Tracked, routeKm, obs - start, tag, rpe)
                }
                runs += sr
                if (day == longestDay) longest = sr
            }
        }
        return LongHistory(
            events = events,
            sets = sets,
            runs = runs.sortedBy { it.observedAt },
            today = today,
            longestRun = longest!!,
            bigSessionDay = bigSessionDay,
            bigSessionExercises = bigDay.size,
            bigSessionSets = sets.count { Math.floorDiv(it.observedAt, 86_400L) == bigSessionDay },
            lb = lb,
        )
    }

    /** One GPX import as [importedRunEvent] builds it (no recorded accuracy, HR from a strap). */
    private fun importedRun(rng: Lcg, day: Long, km: Double, paceSecPerKm: Double, fixEverySec: Long): Pair<Event, SeededRun> {
        val start = day * 86_400L + 18 * 3_600L
        val (points, _, routeKm) = route(rng, start, km, paceSecPerKm, fixEverySec, null)
        val fixes = points.map { GpxFix(it.lat, it.lon, it.observedAt, hrBpm = 140 + rng.int(0, 15)) }
        val ev = importedRunEvent(listOf(fixes), measuredHrMax = 190.0).copy(entryId = start * 1000 + 9)
        return ev to SeededRun(ev.observedAt, ev.entryId, RunSource.Imported, routeKm, points.last().observedAt - start, null, null)
    }

    private data class Route(val points: List<GpsPoint>, val segmentStarts: List<Int>, val km: Double)

    /**
     * A wandering route of [km] at about [paceSecPerKm] (±4 % per kilometre), one fix
     * every [everySec] seconds; [pauseAt] (a share of the distance) inserts a 4-minute
     * stop where recording resumes as a new segment (the tracker's pause boundary).
     */
    private fun route(rng: Lcg, start: Long, km: Double, paceSecPerKm: Double, everySec: Long, pauseAt: Double?): Route {
        val points = ArrayList<GpsPoint>()
        val starts = ArrayList<Int>()
        var lat = HOME_LAT + rng.range(-0.01, 0.01)
        var lon = HOME_LON + rng.range(-0.01, 0.01)
        var heading = rng.range(0.0, 2 * Math.PI)
        var t = start
        var done = 0.0
        val total = km * 1000.0
        var paused = false
        var kmFactor = rng.range(0.96, 1.04)
        var lastKm = 0
        points += GpsPoint(lat, lon, t, 4.0 + rng.int(0, 4))
        while (done < total) {
            if (done.toInt() / 1000 != lastKm) {
                lastKm = done.toInt() / 1000
                kmFactor = rng.range(0.96, 1.04)
            }
            if (pauseAt != null && !paused && done >= total * pauseAt) {
                paused = true
                t += 240
                starts += points.size
            }
            val speed = 1000.0 / (paceSecPerKm * kmFactor)
            val step = Math.min(speed * everySec, total - done)
            val dt = Math.max(1L, (step / speed).roundToLong())
            heading += rng.range(-0.25, 0.25)
            val mLon = M_PER_DEG_LAT * cos(Math.toRadians(lat))
            lat += step * cos(heading) / M_PER_DEG_LAT
            lon += step * sin(heading) / mLon
            t += dt
            done += step
            points += GpsPoint(lat, lon, t, 4.0 + rng.int(0, 4))
        }
        return Route(points, starts, total / 1000.0)
    }
}
