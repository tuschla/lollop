package app.milestone

import java.io.File
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import org.junit.Test

/**
 * Upgrade fixture generator (generate.sh) for 203ab56 (2026-08-08): that build persisted
 * EVERY accepted event, SetToday included, and had no data export. Writes
 * raw.ndjson, log.ndjson (after the old EventLog.load rewrite) and active-run.ndjson.
 */
class UpgradeFixtureGen {
    private val today = 20725L
    private val off = 7200

    private fun at(day: Long, hour: Int, min: Int = 0): Long = day * 86_400 + hour * 3600 + min * 60 - off

    private fun track(startSec: Long, n: Int, pauseAt: Int): Pair<List<GpsPoint>, List<Int>> {
        val pts = ArrayList<GpsPoint>()
        var lat = 52.5200
        var lon = 13.4050
        var t = startSec
        for (i in 0 until n) {
            if (i > 0) {
                t += if (i == pauseAt) 120 else 5
                lat += if (i == pauseAt) 0.0003 else 0.00011 * (1 + (i % 7) / 10.0)
                lon += 0.00007 * (1 + (i % 5) / 10.0)
            }
            pts += GpsPoint(lat, lon, t, 4.0 + (i % 4))
        }
        return pts to listOf(pauseAt)
    }

    private fun profile(sets: Int, km: Double, bw: Double) = Event.SetProfile(
        progressionCadence = ProgressionCadence.WeekToWeek,
        liftGoal = LiftGoal.MaxStrength,
        goalDistance = GoalDistance.TenK,
        concurrentGoal = ConcurrentGoal.Strength,
        weeklySets = sets,
        runningDaysPerWeek = 3,
        runningKmPerWeek = km,
        advanced = false,
        enduranceIntensityPctVo2max = 70.0,
        female = false,
        bodyweightKg = bw,
        ageYears = 36.0,
        restingHrBpm = 54.0,
    )

    private fun scenario(): List<Any> {
        val ev = ArrayList<Any>()
        val d0 = today - 56
        ev += Event.SetToday(d0, off)
        ev += profile(12, 24.0, 78.0)
        ev += Event.ComputeHrZones(ageYears = 36.0, restingHrBpm = 54.0)
        ev += Event.ComputeProtein(bodyweightKg = 78.0, masters = false, deficit = false)
        ev += Event.GeneratePlan(d0)
        ev += """{"ComputeCooper":{"distance_m_12min":2650.0}}"""
        ev += """{"ComputeCriticalSpeed":{"efforts":[{"distance_m":1200.0,"time_sec":270.0},{"distance_m":3600.0,"time_sec":900.0}]}}"""
        ev += "\"ClearCooper\""
        ev += """{"ComputeCooper":{"distance_m_12min":2710.0}}"""
        var id = 1_790_000_000_000L
        val setIds = ArrayList<Pair<Long, Long>>()
        val runIds = ArrayList<Pair<Long, Long>>()
        ev += """{"LogSet":{"exercise":"Squat","weight_kg":90.0,"reps":5,"rpe":7.0,"observed_at":${at(d0 - 3, 18)}}}"""
        for (w in 0 until 8) {
            val mon = d0 + w * 7 - 1
            for (d in 0 until 7 step 2) ev += Event.SetToday(mon + d, off)
            for ((k, dayOff) in listOf(0, 2, 4).withIndex()) {
                val day = mon + dayOff
                val lifts = when (k) {
                    0 -> listOf("Squat" to 100.0, "Bench press" to 70.0)
                    1 -> listOf("Deadlift" to 130.0, "Overhead press" to 45.0)
                    else -> listOf("Squat" to 95.0, "Bench press" to 67.5)
                }
                var minute = 0
                for ((ex, base) in lifts) {
                    repeat(3) { s ->
                        id += 1
                        val t = at(day, 18, minute)
                        minute += 4
                        ev += Event.LogSet(ex, base + 2.5 * w, 5, 7.5 + 0.5 * s, observedAt = t, entryId = id)
                        setIds += id to t
                    }
                }
            }
            for ((k, dayOff) in listOf(1, 3, 5).withIndex()) {
                val day = mon + dayOff
                if (w == 6 && k == 2) continue
                id += 1
                val t = at(day, 7, 30)
                val km = if (k == 2) 10.0 + w * 0.5 else 6.0 + k
                ev += Event.LogRun(
                    distanceKm = km,
                    durationMin = km * 5.6,
                    hrPctMax = 0.0,
                    longestRecentKm = if (w == 0) 0.0 else 10.0 + (w - 1) * 0.5,
                    observedAt = t,
                    entryId = id,
                    workoutType = if (k == 2) WorkoutType.LongRun else WorkoutType.Recovery,
                )
                runIds += id to t
            }
            for (d in 0 until 7 step 2) {
                val day = mon + d
                ev += Event.SubmitCheckin(
                    observedAt = at(day, 7),
                    sleepQuality = 3 + (d % 2),
                    soreness = 2 + (w % 2),
                    mood = 4,
                    restingHrBpm = 54.0 + (d % 3),
                    hrvRmssdMs = 62.0 - (d % 3),
                )
            }
            ev += Event.SubmitReview(overtrainingSignalCount = 0, observedAt = at(mon + 6, 20))
        }
        val gpsDay = d0 + 6 * 7 - 1 + 5
        val (pts, starts) = track(at(gpsDay, 8), 160, pauseAt = 90)
        id += 1
        ev += Event.LogRunTrack(
            points = pts,
            hrPctMax = 0.0,
            longestRecentKm = 12.5,
            observedAt = pts.last().observedAt,
            entryId = id,
            workoutType = WorkoutType.Steady,
            segmentStarts = starts,
        )
        for (k in 0 until 18) ev += Event.PredictRace(5000.0, 1560.0 - k * 3, 10000.0, 24.0, weeksSinceRace = 1)
        for (k in 0 until 16) ev += Event.ComputeHrZones(ageYears = 36.0, restingHrBpm = 56.0 - k % 3)
        val (aId, aT) = setIds[10]
        ev += Event.AmendSet(aId, "Squat", 102.5, 4, 8.0, observedAt = aT)
        ev += Event.AmendSet(aId, "Squat", 102.5, 5, 8.5, observedAt = aT)
        val (dId, dT) = setIds[20]
        ev += Event.DeleteEntry(Event.EntryKind.Set, dId, dT)
        val legacyT = at(d0 - 3, 18)
        ev += Event.AmendSet(0, "Squat", 92.5, 5, 7.0, observedAt = legacyT + 3600, observedAtFallback = legacyT)
        val (rId, rT) = runIds[4]
        ev += Event.AmendRun(rId, 7.2, 41.0, 0.0, observedAt = rT, workoutType = WorkoutType.Tempo)
        val (r2Id, r2T) = runIds[7]
        ev += Event.DeleteEntry(Event.EntryKind.Run, r2Id, r2T)
        ev += Event.PredictRace(recentDistanceM = 5000.0, recentTimeSec = 1500.0, goalDistanceM = 10000.0, weeklyKm = 24.0, weeksSinceRace = 2)
        ev += Event.PlanHypertrophyMeso("Chest", 4)
        ev += Event.ClearHypertrophyPlan
        ev += Event.PlanHypertrophyMeso("Quads", 5)
        ev += """{"ComputeApre":{"scheme":"Apre6","reps":8,"current_load_lb":175.0}}"""
        ev += profile(14, 28.0, 77.0)
        ev += Event.ClearPlan
        ev += Event.GeneratePlan(today - 14)
        ev += Event.SubmitReadiness(ReadinessSignal.Pain, 1.0, at(today - 1, 18), pain = PainDetail(PainKind.SharpJoint, 7, PainTrend.Stable, location = "Left knee"))
        ev += Event.RemoveReadiness(ReadinessSignal.Pain)
        ev += Event.SubmitReadiness(ReadinessSignal.Pain, 1.0, at(today - 1, 18, 1), pain = PainDetail(PainKind.TendonLoadRelated, 3, PainTrend.Stable, location = "Left knee"))
        ev += Event.SubmitReadiness(ReadinessSignal.Rpe, 8.5, at(today - 1, 19), streak = 2)
        ev += Event.SetToday(today - 1, off)
        return ev
    }

    @Test
    fun generate() {
        val tag = File(".").canonicalPath.substringAfter("upg-").substringBefore('/')
        val out = File(System.getProperty("user.home"), ".cache/UpgradePath/gen/$tag").apply { mkdirs() }
        val lines = scenario().map {
            when (it) {
                is String -> it
                is Event -> it.toJson().toString()
                else -> error("bad item")
            }
        }
        File(out, "raw.ndjson").writeText(lines.joinToString("\n", postfix = "\n"))
        val work = File(out, "work").apply { deleteRecursively(); mkdirs() }
        val log = File(work, "event-log.ndjson")
        log.writeText(lines.joinToString("\n", postfix = "\n"))
        EventLog.load(log)
        log.copyTo(File(out, "log.ndjson"), overwrite = true)
        val (pts, _) = track(at(today, 7), 40, pauseAt = 25)
        val side = StringBuilder("{\"run\":\"active\"}\n")
        // 203ab56 ActiveRunStore.append: no "seg" mark yet.
        pts.forEach { p ->
            side.append(
                buildJsonObject {
                    put("lat", p.lat)
                    put("lon", p.lon)
                    put("observed_at", p.observedAt)
                    put("accuracy_m", p.accuracyM)
                }.toString(),
            ).append('\n')
        }
        side.append("{\"lat\":52.53,\"lon\":13.4")
        File(out, "active-run.ndjson").writeText(side.toString())
        work.deleteRecursively()
        // A user who removed the plan: this build's compaction drops the
        // GeneratePlan AND its ClearPlan, so only the plan_cleared pref says so.
        val removed = listOf(
            profile(12, 24.0, 78.0).toJson().toString(),
            Event.GeneratePlan(today - 28).toJson().toString(),
            Event.LogSet("Squat", 100.0, 5, 8.0, observedAt = at(today - 2, 18), entryId = 1_790_000_000_001L).toJson().toString(),
            Event.ClearPlan.toJson().toString(),
        )
        File(out, "plan-removed.ndjson").writeText(EventLog.compact(removed).joinToString("\n", postfix = "\n"))
        println("UPG wrote ${lines.size} raw lines to $out")
    }
}
