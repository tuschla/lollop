package de.tuschla.fitnessanlage

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
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

    // The Kotlin Event type has no variants for the dormant calculators yet, so
    // these lines are raw serde wire shapes (struct variants = single-key object,
    // unit variants = bare string) exactly as app.rs serializes them.
    private fun cooper(distanceM: Double) = """{"ComputeCooper":{"distance_m_12min":$distanceM}}"""
    private fun criticalSpeed(distanceM: Double) =
        """{"ComputeCriticalSpeed":{"efforts":[{"distance_m":$distanceM,"time_sec":720.0}]}}"""
    private fun apre(reps: Int) =
        """{"ComputeApre":{"scheme":"Apre6","reps":$reps,"current_load_lb":225.0}}"""
    private val clearCooper = "\"ClearCooper\""
    private val clearCriticalSpeed = "\"ClearCriticalSpeed\""
    private val clearApre = "\"ClearApre\""

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

    // ── families 9–11: Cooper / CriticalSpeed / Apre (lockstep with log.rs) ──

    @Test
    fun onlyLastCooperSurvives() {
        val out = EventLog.compact(listOf(cooper(2400.0), set("a"), cooper(2600.0)))
        assertEquals(listOf(set("a"), cooper(2600.0)), out)
    }

    @Test
    fun clearWipesTheCooperFamily() {
        val out = EventLog.compact(listOf(cooper(2400.0), cooper(2600.0), clearCooper))
        assertTrue(out.isEmpty())
    }

    @Test
    fun onlyLastCriticalSpeedSurvives() {
        val out = EventLog.compact(listOf(criticalSpeed(3000.0), set("a"), criticalSpeed(3200.0)))
        assertEquals(listOf(set("a"), criticalSpeed(3200.0)), out)
    }

    @Test
    fun clearWipesTheCriticalSpeedFamilyButKeepsLaterFit() {
        val out = EventLog.compact(
            listOf(criticalSpeed(3000.0), clearCriticalSpeed, criticalSpeed(3200.0))
        )
        assertEquals(listOf(criticalSpeed(3200.0)), out)
    }

    @Test
    fun onlyLastApreSurvives() {
        val out = EventLog.compact(listOf(apre(4), set("a"), apre(8)))
        assertEquals(listOf(set("a"), apre(8)), out)
    }

    @Test
    fun clearWipesTheApreFamily() {
        val out = EventLog.compact(listOf(apre(4), apre(8), clearApre))
        assertTrue(out.isEmpty())
    }

    @Test
    fun calculatorClearsOnlyTouchTheirOwnFamily() {
        // A ClearCooper must not disturb the CS fit or APRE adjustment (nor any
        // other family): each calculator clears independently.
        val out = EventLog.compact(
            listOf(cooper(2400.0), criticalSpeed(3000.0), apre(6), clearCooper)
        )
        assertEquals(listOf(criticalSpeed(3000.0), apre(6)), out)
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

    // ── load(): fresh-install vs compacted-empty (Core.restore's seed decision) ──

    private fun tempLog(): File =
        File.createTempFile("event-log", ".ndjson").apply { deleteOnExit() }

    @Test
    fun loadMissingFileIsFreshInstall() {
        val file = tempLog().also { it.delete() }
        val restored = EventLog.load(file)
        assertTrue(restored.freshInstall)
        assertTrue(restored.lines.isEmpty())
    }

    @Test
    fun loadCompactedEmptyLogIsReturningUserNotFreshInstall() {
        // A user who logged sets then cleared them: every line compacts away, but
        // the log file exists: this must NOT read as a fresh install, or the
        // caller would re-seed the SEED profile + onboarding over a deliberate
        // empty state.
        val file = tempLog()
        file.writeText(
            listOf(set("a"), set("b"), line(Event.ClearSets))
                .joinToString("\n", postfix = "\n")
        )
        val restored = EventLog.load(file)
        assertFalse(restored.freshInstall)
        assertTrue(restored.lines.isEmpty())
    }

    @Test
    fun loadBlankButExistingLogIsNotFreshInstall() {
        // A previous compaction that emptied the log leaves "\n" behind; the file
        // still exists, so this too is a returning user.
        val file = tempLog()
        file.writeText("\n")
        val restored = EventLog.load(file)
        assertFalse(restored.freshInstall)
        assertTrue(restored.lines.isEmpty())
    }

    @Test
    fun loadKeepsSurvivorsAndRewritesFileCompacted() {
        val file = tempLog()
        file.writeText(
            listOf(set("a"), line(Event.ClearSets), run(5.0))
                .joinToString("\n", postfix = "\n")
        )
        val restored = EventLog.load(file)
        assertFalse(restored.freshInstall)
        assertEquals(listOf(run(5.0)), restored.lines)
        // The durable log was compacted in place (atomic rewrite).
        assertEquals(listOf(run(5.0)), file.readLines().filter { it.isNotBlank() })
    }
}
