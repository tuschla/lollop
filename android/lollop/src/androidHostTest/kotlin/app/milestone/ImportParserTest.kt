package app.milestone

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.fail
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * The shell's side of the run import ([parseRunFile], [importedRunEvent]): the core's
 * verdict maps to the shell's reasons, and an imported run becomes the event the
 * shell sends. The readers, the run gates and the heart-rate share are core tests
 * (`shared/src/import/tests.rs`).
 *
 * Robolectric, not plain JUnit: the heart-rate share is the core's
 * (`ImportedHrPct`), and the JVM loads `libshared.so` into one classloader only
 * (see [EventLogTest]).
 */
@RunWith(RobolectricTestRunner::class)
class ImportParserTest {

    private val run = ImportedRun(
        points = listOf(GpsPoint(0.0, 0.0, 1_000L, 30.0), GpsPoint(0.0, 0.0054, 1_200L, 12.5)),
        observed_at = 1_500L,
    )

    private fun refusal(verdict: ImportVerdict?): Pair<ImportFailure, String?> = try {
        parseRunFile(ByteArray(0)) { verdict }
        fail("refused: $verdict")
        throw AssertionError()
    } catch (e: ImportException) {
        e.reason to e.detail
    }

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
        val ev = importedRunEvent(run.copy(segment_starts = listOf(1)), measuredHrMax = null)
        assertEquals(run.points, ev.points)
        assertEquals(listOf(1), ev.segmentStarts)
        assertEquals(1_500L, ev.observedAt)
        assertEquals(null, ev.workoutType)
        assertEquals(0.0, ev.hrPctMax, 0.0)
    }

    @Test
    fun theHeartRateIsTheCoresShareOfAMeasuredHrMax() {
        // The core's `ImportedHrPct` op answers it (cases: `shared/src/import/tests.rs`).
        val hr = run.copy(avg_hr_bpm = 152.0)
        assertEquals(80.0, importedRunEvent(hr, measuredHrMax = 190.0).hrPctMax, 1e-9)
        assertEquals(100.0, importedRunEvent(hr, measuredHrMax = 140.0).hrPctMax, 0.0)
        assertEquals(0.0, importedRunEvent(hr, measuredHrMax = null).hrPctMax, 0.0)
    }

    @Test
    fun twoImportedRunsInOneMsGetDistinctIds() {
        // A multi-activity file builds several runs back-to-back; a millisecond
        // default id would collide, making a later DeleteEntry/AmendRun target the
        // wrong run. EntryIds.next() is unique per built run even in a tight loop.
        val a = importedRunEvent(run, measuredHrMax = null)
        val b = importedRunEvent(run, measuredHrMax = null)
        assertNotEquals(a.entryId, b.entryId)
    }
}
