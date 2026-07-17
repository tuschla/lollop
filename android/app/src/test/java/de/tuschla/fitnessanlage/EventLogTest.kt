package de.tuschla.fitnessanlage

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Unit tests for [EventLog.compact], the Kotlin half of the compaction lockstep
 * with Rust `compact_event_log` (shared/src/log.rs). Lines are produced by the
 * real [Event.toJson] so a wire-shape drift would surface here too.
 */
class EventLogTest {

    private fun line(e: Event) = e.toJson().toString()

    private fun set(exercise: String) = line(Event.LogSet(exercise, 100.0, 5, 8.0))
    private fun run(km: Double) = line(Event.LogRun(km, km * 5.0, 75.0, 0.0))
    private fun profile(weeklySets: Int) = line(
        Event.SetProfile(
            ProgressionCadence.WeekToWeek, LiftGoal.MaxStrength, GoalDistance.TenK,
            ConcurrentGoal.Strength, weeklySets, 4, 45.0, false, 75.0,
        )
    )
    private fun protein(bw: Double) = line(Event.ComputeProtein(bw, masters = false, deficit = true))

    @Test
    fun clearDropsFamilyPrefixButKeepsLaterEntries() {
        val out = EventLog.compact(listOf(set("a"), set("b"), line(Event.ClearSets), set("c")))
        assertEquals(listOf(set("c")), out)
    }

    @Test
    fun trailingClearCollapsesToEmpty() {
        val out = EventLog.compact(listOf(run(5.0), run(6.0), line(Event.ClearRuns)))
        assertTrue(out.isEmpty())
    }

    @Test
    fun clearOnlyTouchesItsOwnFamily() {
        val out = EventLog.compact(listOf(set("a"), run(5.0), line(Event.ClearSets)))
        assertEquals(listOf(run(5.0)), out)
    }

    @Test
    fun clearRunsWipesBothLogRunAndLogRunTrack() {
        val track = line(Event.LogRunTrack(listOf(GpsPoint(0.0, 0.0, 0, 5.0)), 0.0, 0.0))
        val out = EventLog.compact(listOf(run(5.0), track, line(Event.ClearRuns), run(8.0)))
        assertEquals(listOf(run(8.0)), out)
    }

    @Test
    fun onlyLastProfileSurvives() {
        val out = EventLog.compact(listOf(profile(10), set("a"), profile(20)))
        assertEquals(listOf(set("a"), profile(20)), out)
    }

    @Test
    fun onlyLastProteinSurvives() {
        val out = EventLog.compact(listOf(protein(70.0), set("a"), protein(90.0)))
        assertEquals(listOf(set("a"), protein(90.0)), out)
    }

    @Test
    fun emptyLogStaysEmpty() {
        assertTrue(EventLog.compact(emptyList()).isEmpty())
    }

    @Test
    fun unparseableLineIsNeverRemoved() {
        val out = EventLog.compact(listOf("{ hand edited garbage", set("a"), line(Event.ClearSets)))
        assertEquals(listOf("{ hand edited garbage"), out)
    }

    @Test
    fun variantOfReadsUnitAndStructTags() {
        assertEquals("ClearSets", EventLog.variantOf(line(Event.ClearSets)))
        assertEquals("LogSet", EventLog.variantOf(set("a")))
        assertEquals(null, EventLog.variantOf("not json"))
    }
}
