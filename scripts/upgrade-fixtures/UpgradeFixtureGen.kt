package app.milestone

import java.io.File
import kotlinx.serialization.json.buildJsonObject // only:4728546
import kotlinx.serialization.json.put // only:4728546
import org.junit.Test
import org.junit.runner.RunWith // only:901d239
import org.robolectric.RobolectricTestRunner // only:901d239

/**
 * Upgrade fixture generator for 4728546 and 901d239 (generate.sh): runs INSIDE
 * an old worktree, so `Event.toJson()` and `EventLog` are that version's shell
 * code. generate.sh drops each line tagged `// only:<sha>` for another build.
 * Writes to $UPG_GEN_DIR/<sha>/:
 *  - raw.ndjson         every persisted line as the old shell appended it
 *  - log.ndjson         the same file after the old boot's EventLog.load (its rewrite)
 *  - backup.ndjson      what the old Profile › Your data export wrote (EventLog.read lines)
 *  - active-run.ndjson  4728546 only: an interrupted run's sidecar in the old
 *                       ActiveRunStore format
 */
@RunWith(RobolectricTestRunner::class) // only:901d239
class UpgradeFixtureGen {
    private val today = 20725L // 2026-09-29, a Tuesday
    private val off = 7200 // Berlin summer

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

    private fun scenario(): List<Any> {
        val ev = ArrayList<Any>()
        val d0 = today - 56
        ev += Event.SetProfile(
            progressionCadence = ProgressionCadence.WeekToWeek,
            liftGoal = LiftGoal.MaxStrength,
            goalDistance = GoalDistance.TenK,
            concurrentGoal = ConcurrentGoal.Strength,
            weeklySets = 12,
            runningDaysPerWeek = 3,
            runningKmPerWeek = 24.0,
            advanced = false,
            female = false,
            bodyweightKg = 78.0,
            ageYears = 36.0,
            restingHrBpm = 54.0,
            liftDaysPerWeek = 3,
            birthEpochDay = 7425,
            editedAtSec = at(d0, 9),
        )
        ev += Event.ComputeHrZones(ageYears = 36.0, restingHrBpm = 54.0)
        ev += Event.ComputeProtein(bodyweightKg = 78.0, masters = false, deficit = false)
        ev += Event.GeneratePlan(d0)
        // Retired calculators (Cooper / critical speed / APRE): raw old-core wire lines. // only:4728546
        ev += """{"ComputeCooper":{"distance_m_12min":2650.0}}""" // only:4728546
        ev += """{"ComputeCriticalSpeed":{"efforts":[{"distance_m":1200.0,"time_sec":270.0},{"distance_m":3600.0,"time_sec":900.0}]}}""" // only:4728546
        ev += "\"ClearCooper\"" // only:4728546
        ev += """{"ComputeCooper":{"distance_m_12min":2710.0}}""" // only:4728546
        var id = 1_790_000_000_000L
        val setIds = ArrayList<Pair<Long, Long>>()
        val runIds = ArrayList<Pair<Long, Long>>()
        // A legacy set: no entry id (logged before ids existed).
        ev += """{"LogSet":{"exercise":"Squat","weight_kg":90.0,"reps":5,"rpe":7.0,"observed_at":${at(d0 - 3, 18)}}}"""
        for (w in 0 until 8) {
            val mon = d0 + w * 7 - 1 // Mondays
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
                if (k == 1) {
                    id += 1
                    val t = at(day, 18, 40)
                    ev += Event.LogSet("Plank", 0.0, 1, 7.0, observedAt = t, entryId = id, holdSec = 60)
                    setIds += id to t
                }
            }
            for ((k, dayOff) in listOf(1, 3, 5).withIndex()) {
                val day = mon + dayOff
                if (w == 6 && k == 2) continue // the GPS run below takes this slot
                id += 1
                val t = at(day, 7, 30)
                val km = if (k == 2) 10.0 + w * 0.5 else 6.0 + k
                ev += Event.LogRun(
                    distanceKm = km,
                    durationMin = km * 5.6,
                    hrPctMax = 0.0,
                    longestRecentKm = if (w == 0) 0.0 else 10.0 + (w - 1) * 0.5, // only:4728546
                    observedAt = t,
                    entryId = id,
                    workoutType = if (k == 2) WorkoutType.LongRun else WorkoutType.Easy,
                    sessionRpe = 4 + k,
                    avgHrBpm = 140.0 + k * 5,
                    utcOffsetSec = off,
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
                    fatigue = 2,
                    stress = 2,
                    restingHrBpm = 54.0 + (d % 3),
                    hrvRmssdMs = 62.0 - (d % 3),
                )
            }
            ev += Event.SubmitReview(overtrainingSignalCount = 0, observedAt = at(mon + 6, 20))
        }
        // GPS run in week 7 with a pause (two recording segments).
        val gpsDay = d0 + 6 * 7 - 1 + 5
        val (pts, starts) = track(at(gpsDay, 8), 160, pauseAt = 90)
        id += 1
        val gpsId = id
        ev += Event.LogRunTrack(
            points = pts,
            hrPctMax = 0.0,
            longestRecentKm = 12.5, // only:4728546
            observedAt = pts.last().observedAt,
            entryId = gpsId,
            workoutType = WorkoutType.Steady,
            segmentStarts = starts,
            sessionRpe = 5,
            avgHrBpm = 151.0,
            utcOffsetSec = off,
        )
        // Calculator re-entries over the weeks (last write wins): enough superseded
        // lines that the old boot's compaction rewrites the log file.
        for (k in 0 until 18) ev += Event.PredictRace(5000.0, 1560.0 - k * 3, 10000.0, 24.0, weeksSinceRace = 1)
        for (k in 0 until 16) ev += Event.ComputeHrZones(ageYears = 36.0, restingHrBpm = 56.0 - k % 3)
        ev += Event.TrimRunTrack(gpsId, pts.last().observedAt, keepFromSec = pts[3].observedAt, keepUntilSec = pts[155].observedAt)
        ev += Event.AmendRunMeta(gpsId, pts.last().observedAt, sessionRpe = 6, workoutType = WorkoutType.Tempo)
        // Edits: amend a set twice (second supersedes), delete a set, amend + delete runs,
        // amend the legacy (id 0) set by its old timestamp.
        val (aId, aT) = setIds[10]
        ev += Event.AmendSet(aId, "Squat", 102.5, 4, 8.0, observedAt = aT)
        ev += Event.AmendSet(aId, "Squat", 102.5, 5, 8.5, observedAt = aT)
        val (dId, dT) = setIds[20]
        ev += Event.DeleteEntry(Event.EntryKind.Set, dId, dT)
        val legacyT = at(d0 - 3, 18)
        ev += Event.AmendSet(0, "Squat", 92.5, 5, 7.0, observedAt = legacyT + 3600, observedAtFallback = legacyT)
        val (rId, rT) = runIds[4]
        ev += Event.AmendRun(rId, 7.2, 41.0, 0.0, observedAt = rT, workoutType = WorkoutType.Tempo, sessionRpe = 6, avgHrBpm = 152.0)
        val (r2Id, r2T) = runIds[7]
        ev += Event.DeleteEntry(Event.EntryKind.Run, r2Id, r2T)
        ev += Event.SetExerciseEnded("Overhead press", today - 10, true, observedAt = at(today - 10, 19))
        ev += Event.SetExerciseEnded("Overhead press", today - 10, false, observedAt = at(today - 10, 19, 5))
        ev += Event.SetExerciseEnded("Deadlift", today - 3, true, observedAt = at(today - 3, 19))
        ev += Event.LogActivity(ActivityKind.Cycling, 45.0, vigorous = false, observedAt = at(today - 5, 17), entryId = ++id)
        ev += Event.PredictRace(recentDistanceM = 5000.0, recentTimeSec = 1500.0, goalDistanceM = 10000.0, weeklyKm = 24.0, weeksSinceRace = 2)
        ev += Event.PlanHypertrophyMeso("Chest", 4)
        ev += Event.ClearHypertrophyPlan
        ev += Event.PlanHypertrophyMeso("Quads", 5, notGrowing = true)
        ev += """{"ComputeApre":{"scheme":"Apre6","reps":8,"current_load_kg":80.0}}""" // only:4728546
        ev += Event.SubmitWeeklyCheckin(Event.SubmitReview(badDay = false, observedAt = at(today - 2, 20)))
        // Profile edit three weeks in, plan removed and re-generated.
        ev += Event.SetProfile(
            progressionCadence = ProgressionCadence.WeekToWeek,
            liftGoal = LiftGoal.MaxStrength,
            goalDistance = GoalDistance.TenK,
            concurrentGoal = ConcurrentGoal.Strength,
            weeklySets = 14,
            runningDaysPerWeek = 3,
            runningKmPerWeek = 28.0,
            advanced = false,
            female = false,
            bodyweightKg = 77.0,
            ageYears = 36.0,
            restingHrBpm = 53.0,
            liftDaysPerWeek = 3,
            birthEpochDay = 7425,
            effectiveFromEpochDay = today - 21,
            editedAtSec = at(today - 21, 9),
        )
        ev += Event.ClearPlan
        ev += Event.GeneratePlan(today - 14)
        ev += Event.MoveSession(today + 1, today + 2)
        ev += Event.RestartHrvBaseline(at(today - 4, 7, 5))
        // A mis-tapped sharp-pain report, undone, then the real (tendon) report.
        ev += Event.SubmitReadiness(ReadinessSignal.Pain, 1.0, at(today - 1, 18), pain = PainDetail(PainKind.SharpJoint, 7, PainTrend.Stable, location = "Left knee"))
        ev += Event.RemoveReadiness(ReadinessSignal.Pain, at(today - 1, 18))
        ev += Event.SubmitReadiness(ReadinessSignal.Pain, 1.0, at(today - 1, 18, 1), pain = PainDetail(PainKind.TendonLoadRelated, 3, PainTrend.Stable, location = "Left knee"))
        ev += Event.SubmitReadiness(ReadinessSignal.Rpe, 8.5, at(today - 1, 19), streak = 2) // only:4728546
        ev += Event.SubmitReadiness(ReadinessSignal.Rpe, 8.5, at(today - 1, 19)) // only:901d239
        ev += Event.SetToday(today, off) // not persisted by these builds
        return ev
    }

    @Test
    fun generate() {
        val tag = File(".").canonicalPath.substringAfter("upg-").substringBefore('/')
        val genDir = System.getenv("UPG_GEN_DIR") ?: error("UPG_GEN_DIR unset: run scripts/upgrade-fixtures/generate.sh")
        val out = File(genDir, tag).apply { mkdirs() }
        val lines = scenario().mapNotNull {
            when (it) {
                is String -> it
                is Event -> if (it.persist) it.toJson().toString() else null
                else -> error("bad item")
            }
        }
        File(out, "raw.ndjson").writeText(lines.joinToString("\n", postfix = "\n"))
        val work = File(out, "work").apply { deleteRecursively(); mkdirs() }
        val log = File(work, "event-log.ndjson")
        log.writeText(lines.joinToString("\n", postfix = "\n"))
        EventLog.load(log) // the old boot's repair + compaction rewrite
        log.copyTo(File(out, "log.ndjson"), overwrite = true)
        val backup = EventLog.read(log).lines
        File(out, "backup.ndjson").writeText(backup.joinToString("\n", postfix = "\n"))
        // Interrupted-run sidecar, written exactly as ActiveRunStore.begin/append do. // only:4728546
        val (pts, _) = track(at(today, 7), 40, pauseAt = 25) // only:4728546
        val side = StringBuilder("{\"run\":\"active\"}\n") // only:4728546
        pts.forEachIndexed { i, p -> // only:4728546
            side.append( // only:4728546
                buildJsonObject { // only:4728546
                    put("lat", p.lat) // only:4728546
                    put("lon", p.lon) // only:4728546
                    put("observed_at", p.observedAt) // only:4728546
                    put("accuracy_m", p.accuracyM) // only:4728546
                    if (i == 25) put("seg", true) // only:4728546
                }.toString(), // only:4728546
            ).append('\n') // only:4728546
        } // only:4728546
        side.append("{\"lat\":52.53,\"lon\":13.4") // torn tail (killed mid-append) // only:4728546
        File(out, "active-run.ndjson").writeText(side.toString()) // only:4728546
        work.deleteRecursively()
        println("UPG wrote ${lines.size} raw, ${backup.size} kept lines to $out")
    }
}
