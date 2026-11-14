package app.milestone

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import java.time.LocalDateTime
import java.time.ZoneId
import kotlinx.datetime.TimeZone
import kotlinx.datetime.toJavaZoneId
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/** History's shell-side session grouping, heatmap days and set summaries
 *  (zone-explicit). Robolectric for the string resources. */
@RunWith(RobolectricTestRunner::class)
class HistorySessionsTest {

    private val berlin = TimeZone.of("Europe/Berlin")

    private fun at(local: String, zone: ZoneId = berlin.toJavaZoneId()): Long =
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
    fun exerciseSpellingsMergeLikeTheCoreUnderTheFirstLoggedName() {
        val lifts = listOf(
            set("Squat", at("2026-10-01T09:00")).copy(e1rm_kg = 116.7),
            set("squat ", at("2026-10-03T09:00"), kg = 110.0).copy(e1rm_kg = 128.3, e1rm_pr = true),
            set(" SQUAT", at("2026-10-03T09:10"), kg = 105.0).copy(e1rm_kg = 122.5),
        )
        val sessions = groupLiftSessions(lifts, berlin)
        assertEquals(listOf("squat "), sessions.first().exercises.map { it.first })
        assertEquals(2, sessions.first().exercises.single().second.size)
        val series = bestE1rmByExerciseDay(sessions)
        assertEquals(setOf("squat"), series.keys)
        assertEquals(listOf(116.7, 128.3), series.getValue("squat").map { it.second })
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
    fun theLastSevenDaysCountARunByItsStartLikeTheHeatmap() {
        val now = at("2026-09-20T12:00:00")
        val lifts = (0 until 10).map { set("Squat", at("2026-09-18T18:00:00") + it * 60L).copy(tonnage_kg = 500.0) } +
            set("Squat", at("2026-09-10T18:00:00")).copy(tonnage_kg = 500.0)
        val runs = listOf(
            RunResultView(distance_km = 5.0, observed_at = at("2026-09-19T07:40:00"), started_at = at("2026-09-19T07:00:00")),
            // Logged inside the window, started before it: an earlier week's run.
            RunResultView(distance_km = 21.0, observed_at = at("2026-09-14T08:00:00"), started_at = at("2026-09-12T08:00:00")),
        )
        val week = lastSevenDays(lifts, runs, now, berlin)
        assertEquals(2, week.sessions) // one lifting day + one run
        assertEquals(5.0, week.runKm, 0.0)
        assertEquals(10 * 500.0, week.liftKg, 0.0)
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
}
