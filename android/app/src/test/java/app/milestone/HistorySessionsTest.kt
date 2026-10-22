package app.milestone

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import java.time.LocalDateTime
import java.time.ZoneId
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/** History's shell-side session grouping, heatmap days, set summaries and Undo
 *  events (zone-explicit). Robolectric for the string resources. */
@RunWith(RobolectricTestRunner::class)
class HistorySessionsTest {

    private val berlin = ZoneId.of("Europe/Berlin")

    private fun at(local: String, zone: ZoneId = berlin): Long =
        LocalDateTime.parse(local).atZone(zone).toEpochSecond()

    private fun set(exercise: String, observedAt: Long, kg: Double = 100.0, reps: Int = 5) =
        LiftResultView(exercise = exercise, weight_kg = kg, reps = reps, observed_at = observedAt, entry_id = observedAt * 1000)

    @Test
    fun setsGroupIntoOneSessionPerLocalDay_newestFirst() {
        val days = listOf("2026-09-14", "2026-09-16", "2026-09-18")
        val lifts = days.flatMap { d ->
            (0 until 15).map { i -> set(if (i < 8) "Squat" else "Bench", at("${d}T18:00:00") + i * 120L) }
        }
        val sessions = groupLiftSessions(lifts, berlin)
        assertEquals(3, sessions.size)
        assertEquals(
            days.reversed().map { java.time.LocalDate.parse(it).toEpochDay() },
            sessions.map { it.epochDay },
        )
        sessions.forEach { s ->
            assertEquals(15, s.sets.size)
            assertEquals(listOf("Squat", "Bench"), s.exercises.map { it.first })
        }
    }

    @Test
    fun aSessionDayAcrossTheDstShiftStaysOneLocalDay() {
        // 2026-10-25 is the CEST→CET switch-back (a 25-hour day). 00:30 is still
        // CEST (UTC+2) and 23:30 is CET (UTC+1); bucketing with one fixed offset
        // would push the early set onto Oct 24.
        val lifts = listOf(
            set("Squat", at("2026-10-25T00:30:00")),
            set("Squat", at("2026-10-25T23:30:00")),
        )
        val sessions = groupLiftSessions(lifts, berlin)
        assertEquals(1, sessions.size)
        assertEquals(java.time.LocalDate.parse("2026-10-25").toEpochDay(), sessions.single().epochDay)
    }

    @Test
    fun undatedSetsFormOneTrailingSession() {
        val lifts = listOf(set("Squat", 0L), set("Squat", at("2026-09-14T18:00:00")))
        val sessions = groupLiftSessions(lifts, berlin)
        val day = java.time.LocalDate.parse("2026-09-14").toEpochDay()
        assertEquals(listOf(day.toString(), "undated"), sessions.map { it.key })
        assertEquals(null, sessions.last().epochDay)
    }

    @Test
    fun heatmapMarksALiftingDayOnceAndCountsEachRun() {
        val day = at("2026-09-14T18:00:00")
        val lifts = (0 until 20).map { set("Squat", day + it * 60L) }
        val runs = listOf(
            RunResultView(distance_km = 30.0, observed_at = at("2026-09-14T07:00:00")),
            RunResultView(distance_km = 5.0, observed_at = at("2026-09-15T07:00:00")),
        )
        val days = historyActivityDays(lifts, runs, berlin)
        val d14 = java.time.LocalDate.parse("2026-09-14").toEpochDay()
        assertEquals(DayActivity(lifted = true, runs = 1), days[d14]) // 20 sets = 1 session, + 1 run
        assertEquals(2, days[d14]!!.sessions)
        assertEquals(DayActivity(lifted = false, runs = 1), days[d14 + 1])
    }

    @Test
    fun identicalConsecutiveSetsFoldIntoOneSummaryLine() {
        val res = ApplicationProvider.getApplicationContext<Context>().resources
        val t = at("2026-09-14T18:00:00")
        val sets = listOf(
            set("Squat", t, 120.0, 5), set("Squat", t + 1, 120.0, 5), set("Squat", t + 2, 120.0, 5),
            set("Squat", t + 3, 125.0, 3),
        )
        val first = res.getString(R.string.history_sets_reps_load, 3, 5, res.getString(R.string.history_load, "120", "kg"))
        val second = res.getString(R.string.history_sets_reps_load, 1, 3, res.getString(R.string.history_load, "125", "kg"))
        assertEquals(first + res.getString(R.string.history_sets_separator) + second, exerciseSetsSummary(res, sets, WeightUnit.Kg))
    }

    @Test
    fun theNewestSessionIsFoundByEntryIdNotByInstance() {
        // A cached session holds the sets of an older decode; the latest model holds
        // equal-but-distinct instances. Identity (===) never matches them.
        val t = at("2026-09-14T18:00:00")
        val cached = groupLiftSessions(listOf(set("Squat", t), set("Squat", t + 60)), berlin).single()
        val freshDecode = listOf(set("Squat", t), set("Squat", t + 60))
        assertTrue(cached.holdsNewestSet(freshDecode))
        assertFalse(cached.holdsNewestSet(freshDecode + set("Bench", at("2026-09-15T18:00:00"))))
    }

    @Test
    fun undoOfADeletedSetRelogsItWithItsOwnIdAndStamp() {
        val s = set("Squat", at("2026-09-14T18:00:00"), 120.0, 5).copy(rpe = 8.0)
        val ev = restoreEventFor(HistoryEntry.Lift(s), emptyList()) as Event.LogSet
        assertEquals(s.entry_id, ev.entryId)
        assertEquals(s.observed_at, ev.observedAt)
        assertEquals(listOf(120.0, 5.0, 8.0), listOf(ev.weightKg, ev.reps.toDouble(), ev.rpe))
    }

    @Test
    fun undoOfADeletedGpsRunWaitsForItsRoute() {
        val run = RunResultView(distance_km = 5.0, observed_at = 2_000L, entry_id = 7L, has_gpx = true)
        assertEquals(null, restoreEventFor(HistoryEntry.Run(run), emptyList()))
        val route = listOf(
            listOf(GpxFix(0.0, 0.0, 1_000L, null), GpxFix(0.0, 0.001, 1_500L, null)),
            listOf(GpxFix(0.0, 0.002, 1_800L, null), GpxFix(0.0, 0.003, 2_000L, null)),
        )
        val ev = restoreEventFor(HistoryEntry.Run(run), route) as Event.LogRunTrack
        assertEquals(7L, ev.entryId)
        assertEquals(2_000L, ev.observedAt)
        assertEquals(4, ev.points.size)
        assertEquals(listOf(2), ev.segmentStarts)
    }

    @Test
    fun undoOfADeletedRunRelogsTheEnteredBpmNotTheDerivedPercentage() {
        val run = RunResultView(
            distance_km = 8.0, duration_min = 48.0, hr_pct_max = 82.5, avg_hr_bpm = 152.0,
            observed_at = 3_000L, entry_id = 9L, session_rpe = 5,
        )
        val ev = restoreEventFor(HistoryEntry.Run(run), emptyList()) as Event.LogRun
        assertEquals(152.0, ev.avgHrBpm)
        assertEquals(0.0, ev.hrPctMax, 0.0)
        val pctOnly = run.copy(avg_hr_bpm = null)
        val ev2 = restoreEventFor(HistoryEntry.Run(pctOnly), emptyList()) as Event.LogRun
        assertEquals(null, ev2.avgHrBpm)
        assertEquals(82.5, ev2.hrPctMax, 0.0)
    }
}
