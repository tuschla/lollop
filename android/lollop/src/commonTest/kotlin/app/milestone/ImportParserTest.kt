package app.milestone

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNotEquals

/**
 * The shell's side of the run import ([parseRunFile], [importedRunEvent]): the core's
 * verdict maps to the shell's reasons, and an imported run becomes the event the
 * shell sends. The readers, the run gates and the heart-rate conversion are core
 * tests (`shared/src/import/tests.rs`, the run-log tests).
 */
class ImportParserTest {

    private val run = ImportedRun(
        points = listOf(GpsPoint(0.0, 0.0, 1_000L, 30.0), GpsPoint(0.0, 0.0054, 1_200L, 12.5)),
        observed_at = 1_500L,
    )

    private fun refusal(verdict: ImportVerdict?): Pair<ImportFailure, String?> =
        assertFailsWith<ImportException>("refused: $verdict") { parseRunFile(ByteArray(0)) { verdict } }
            .let { it.reason to it.detail }

    @Test
    fun aRideKeepsItsSportAndACoreWithoutAnAnswerReadsAsDamaged() {
        assertEquals(ImportFailure.NotARun to "cycling", refusal(ImportVerdict(refusal = ImportRefusal.NotARun, sport = "cycling")))
        assertEquals(ImportFailure.NotARun to null, refusal(ImportVerdict(refusal = ImportRefusal.NotARun, sport = " ")))
        assertEquals(ImportFailure.Damaged to null, refusal(null))
        assertEquals(ImportFailure.NoTrack to null, refusal(ImportVerdict()))
        assertEquals(listOf(run), parseRunFile(ByteArray(0)) { ImportVerdict(runs = listOf(run)) })
    }

    @Test
    fun anImportedRunIsSentAsTheCoreReadIt() {
        val ev = importedRunEvent(run.copy(segment_starts = listOf(1)))
        assertEquals(run.points, ev.points)
        assertEquals(listOf(1), ev.segmentStarts)
        assertEquals(1_500L, ev.observedAt)
        assertEquals(null, ev.workoutType)
        assertEquals(0.0, ev.hrPctMax)
        assertEquals(null, ev.avgHrBpm)
    }

    @Test
    fun theHeartRateGoesAsBpmForTheCoreToConvert() {
        val ev = importedRunEvent(run.copy(avg_hr_bpm = 152.0))
        assertEquals(152.0, ev.avgHrBpm)
        assertEquals(0.0, ev.hrPctMax, "the core derives %HRmax from the bpm")
    }

    @Test
    fun twoImportedRunsInOneMsGetDistinctIds() {
        // A multi-activity file builds several runs back-to-back; a millisecond
        // default id would collide, making a later DeleteEntry/AmendRun target the
        // wrong run. EntryIds.next() is unique per built run even in a tight loop.
        assertNotEquals(importedRunEvent(run).entryId, importedRunEvent(run).entryId)
    }
}
