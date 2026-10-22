package app.milestone

import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * Multi-file / Share-to run import ([planRunImport], [runImportSummary],
 * [parseRunFileSegments], [fitSegments], [RunImport.urisFrom]): every skipped file is
 * named with its reason, one bad file never blocks the rest, a non-running FIT or an
 * implausibly fast track never becomes running volume, duplicates are skipped and
 * counted, and every system entry point (VIEW, SEND, SEND_MULTIPLE) yields its files.
 * Robolectric for the real Intent and string resources.
 */
@RunWith(RobolectricTestRunner::class)
class RunImportTest {

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    /** Metres → degrees of latitude (1° ≈ 111.2 km). */
    private fun deg(metres: Double) = metres / 111_195.0

    /** A 10-minute track ending at [endSec]: [metresPerFix] apart, one fix per 10 s,
     *  heading north from the equator. */
    private fun track(endSec: Long, metresPerFix: Double, fixes: Int = 61): List<List<GpxFix>> {
        val start = endSec - (fixes - 1) * 10L
        return listOf(List(fixes) { i -> GpxFix(lat = i * deg(metresPerFix), lon = 0.0, timeSec = start + i * 10L, hrBpm = null) })
    }

    /** An easy 6:00 /km run: 27.8 m every 10 s. */
    private fun easyRun(endSec: Long) = track(endSec, metresPerFix = 27.8)

    private fun ok(vararg runs: List<List<GpxFix>>) = Result.success(runs.toList())

    private fun failed(reason: ImportFailure, detail: String? = null) =
        Result.failure<List<List<List<GpxFix>>>>(GpxImportException(reason, detail))

    private fun plan(files: List<ParsedRunFile>, existing: Set<Long> = emptySet()) =
        planRunImport(files, existing) { segments -> importedRunEvent(segments, measuredHrMax = null) }

    @Test
    fun eachSkippedFileIsNamedWithItsReasonAndNeverBlocksTheOthers() {
        val p = plan(
            listOf(
                ParsedRunFile("morning.gpx", ok(easyRun(10_000))),
                ParsedRunFile("broken.gpx", ok(emptyList())),
                ParsedRunFile("bike.fit", failed(ImportFailure.NotARun, "cycling")),
                ParsedRunFile("evening.tcx", ok(easyRun(20_000), easyRun(30_000))),
            ),
        )
        assertEquals(listOf(10_000L, 20_000L, 30_000L), p.runs.map { it.event.observedAt })
        assertEquals(listOf("morning.gpx", "evening.tcx", "evening.tcx"), p.runs.map { it.fileName })
        assertEquals(
            listOf(
                RunImportFailure("broken.gpx", ImportFailure.NoTrack),
                RunImportFailure("bike.fit", ImportFailure.NotARun, "cycling"),
            ),
            p.failures,
        )
        assertEquals(0, p.duplicates)
    }

    @Test
    fun anImplausiblyFastTrackIsSkippedNotLoggedAsARun() {
        // 12 m/s ≈ 1:23 /km, far faster than the 2:50 /km plausibility gate: a ride
        // or a drive, never running volume.
        val p = plan(
            listOf(
                ParsedRunFile("ride.gpx", ok(track(10_000, metresPerFix = 120.0))),
                ParsedRunFile("run.gpx", ok(easyRun(20_000))),
            ),
        )
        assertEquals(listOf("run.gpx"), p.runs.map { it.fileName })
        assertEquals(listOf(RunImportFailure("ride.gpx", ImportFailure.ImplausiblePace)), p.failures)
    }

    @Test
    fun aFragmentUnderTheKeepThresholdIsSkippedAsTooShort() {
        // A 13 s, 3 m blip; 0.6 km in 2:30; and two 80 s pieces split by a long pause
        // (the bridge counts neither distance nor time, so 2:40 in all).
        val split = listOf(
            List(9) { i -> GpxFix(lat = i * deg(30.0), lon = 0.0, timeSec = 40_000L + i * 10L, hrBpm = null) },
            List(9) { i -> GpxFix(lat = (100 + i) * deg(30.0), lon = 0.0, timeSec = 41_000L + i * 10L, hrBpm = null) },
        )
        val p = plan(
            listOf(
                ParsedRunFile("blip.gpx", ok(listOf(listOf(GpxFix(0.0, 0.0, 9_987L, null), GpxFix(deg(3.0), 0.0, 10_000L, null))))),
                ParsedRunFile("quick.tcx", ok(track(20_000, metresPerFix = 40.0, fixes = 16))),
                ParsedRunFile("paused.gpx", ok(split)),
                ParsedRunFile("run.gpx", ok(easyRun(30_000))),
            ),
        )
        assertEquals(listOf("run.gpx"), p.runs.map { it.fileName })
        assertEquals(
            listOf("blip.gpx", "quick.tcx", "paused.gpx").map { RunImportFailure(it, ImportFailure.TooShort) },
            p.failures,
        )
        // Just over both thresholds: kept.
        assertEquals(listOf("long-enough.gpx"), plan(listOf(ParsedRunFile("long-enough.gpx", ok(track(50_000, metresPerFix = 30.0, fixes = 20))))).runs.map { it.fileName })
    }

    @Test
    fun theGateJudgesMovingPaceSoStopsCannotHideASpeed() {
        // 3:20 /km (5 m/s) is a fast but real run: it passes.
        val brisk = track(10_000, metresPerFix = 50.0).flatten().map { GpsPoint(it.lat, it.lon, it.timeSec, 5.0) }
        assertFalse(isImplausiblyFast(brisk, emptyList()))
        // 2:30 /km while moving, standing still every other 10 s: 5:00 /km elapsed, but
        // the moving pace is what counts, so it is refused.
        val stopStart = List(41) { i -> GpsPoint(lat = ((i + 1) / 2) * deg(66.7), lon = 0.0, observedAt = 1_000L + i * 10L, accuracyM = 5.0) }
        assertTrue(isImplausiblyFast(stopStart, emptyList()))
    }

    @Test
    fun runsAlreadyInHistoryOrRepeatedInTheBatchAreSkippedAndCounted() {
        // 10 000 is already logged; 20 000 appears in two shared files (same run exported
        // twice). Each must count once toward weekly volume, never twice.
        val p = plan(
            listOf(
                ParsedRunFile("a.gpx", ok(easyRun(10_000), easyRun(20_000))),
                ParsedRunFile("b.gpx", ok(easyRun(20_000), easyRun(40_000))),
            ),
            existing = setOf(10_000L),
        )
        assertEquals(listOf(20_000L, 40_000L), p.runs.map { it.event.observedAt })
        assertEquals(2, p.duplicates)
        assertTrue(p.failures.isEmpty())
    }

    @Test
    fun aFitFileForAnotherSportIsRefusedWithItsSport() {
        val json = """{"error":{"kind":"not_a_run","context":"cycling","message":"This FIT file records cycling, not a run"}}"""
        try {
            fitSegments(json)
            fail("a cycling FIT must not import")
        } catch (e: GpxImportException) {
            assertEquals(ImportFailure.NotARun, e.reason)
            assertEquals("cycling", e.detail)
        }
        // The legacy plain-string error and a null JNI result read as a damaged file.
        for (bad in listOf("""{"error":"Couldn't read this FIT file"}""", null)) {
            try {
                fitSegments(bad)
                fail("an error envelope must not import")
            } catch (e: GpxImportException) {
                assertEquals(ImportFailure.Damaged, e.reason)
            }
        }
    }

    @Test
    fun aFitFragmentUnderTheKeepThresholdIsSkippedAsTooShort() {
        val json = """{"error":{"kind":"no_track","context":"too_short","message":"Too short to import as a run"}}"""
        try {
            fitSegments(json)
            fail("a 13 s FIT fragment must not import")
        } catch (e: GpxImportException) {
            assertEquals(ImportFailure.TooShort, e.reason)
        }
        // Any other no_track still reads as a file without GPS points.
        try {
            fitSegments("""{"error":{"kind":"no_track","context":"","message":"No GPS points"}}""")
            fail("a FIT without points must not import")
        } catch (e: GpxImportException) {
            assertEquals(ImportFailure.NoTrack, e.reason)
        }
        AppSettings.distanceUnitOverride.value = DistanceUnitOverride.Mi
        try {
            assertEquals(
                "too short for a run, under 0.3 mi or 3 minutes",
                importFailureReason(ctx.resources, RunImportFailure("Activity.fit", ImportFailure.TooShort)),
            )
        } finally {
            AppSettings.distanceUnitOverride.value = DistanceUnitOverride.System
        }
    }

    @Test
    fun aFileThatIsNeitherFitNorGpxNorTcxIsRefusedFromItsHeader() {
        try {
            parseRunFileSegments("name,distance\nmonday,5".encodeToByteArray()) { fail("not a FIT file"); null }
            fail("a CSV must not import")
        } catch (e: GpxImportException) {
            assertEquals(ImportFailure.WrongType, e.reason)
        }
    }

    @Test
    fun summaryNamesEverySkippedFileWithItsReason() {
        val res = ctx.resources
        val skipped = listOf(
            RunImportFailure("broken.gpx", ImportFailure.NoTrack),
            RunImportFailure("bike.fit", ImportFailure.NotARun, "cycling"),
        )
        val s = runImportSummary(res, imported = 7, newestImportedAt = 1_700_000_000L, duplicates = 2, failures = skipped)
        val head = res.getQuantityString(R.plurals.import_result_many, 7, 7) + " · " +
            res.getQuantityString(R.plurals.main_import_summary_duplicates, 2, 2)
        val lines = res.getString(
            R.string.import_skip_item, "broken.gpx", res.getString(R.string.import_reason_no_track),
        ) + ", " + res.getString(
            R.string.import_skip_item, "bike.fit", res.getString(R.string.import_reason_not_a_run, "cycling"),
        )
        assertEquals(res.getString(R.string.import_result_with_skipped, head, lines), s.text)
        assertTrue("two skips are named inline, no details list", s.details.isEmpty())
        assertTrue(s.text.contains("broken.gpx") && s.text.contains("bike.fit") && s.text.contains("cycling"))
    }

    @Test
    fun summaryMovesManySkippedFilesBehindDetails() {
        val res = ctx.resources
        val skipped = (1..3).map { RunImportFailure("f$it.gpx", ImportFailure.ImplausiblePace) }
        val s = runImportSummary(res, imported = 0, newestImportedAt = null, duplicates = 0, failures = skipped)
        assertEquals(
            res.getString(
                R.string.import_result_with_skipped_count,
                res.getString(R.string.import_result_none),
                res.getQuantityString(R.plurals.import_result_skipped_count, 3, 3),
            ),
            s.text,
        )
        assertEquals(skipped.map { importFailureLine(res, it) }, s.details)
        assertTrue(s.details.all { it.contains(res.getString(R.string.import_reason_implausible_pace)) })
    }

    @Test
    fun aSingleImportedRunNamesItsDateAndOnlyDuplicatesKeepTheDuplicateMessage() {
        val res = ctx.resources
        assertEquals(
            res.getString(R.string.import_result_one, importDateLabel(1_700_000_000L)),
            runImportSummary(res, imported = 1, newestImportedAt = 1_700_000_000L, duplicates = 0, failures = emptyList()).text,
        )
        assertEquals(
            res.getString(R.string.main_import_duplicate),
            runImportSummary(res, imported = 0, newestImportedAt = null, duplicates = 3, failures = emptyList()).text,
        )
    }

    @Test
    fun everySystemEntryPointYieldsItsFiles() {
        val a = Uri.parse("content://files/a.gpx")
        val b = Uri.parse("content://files/b.fit")
        assertEquals(listOf(a), RunImport.urisFrom(Intent(Intent.ACTION_VIEW).setData(a)))
        assertEquals(listOf(a), RunImport.urisFrom(Intent(Intent.ACTION_SEND).putExtra(Intent.EXTRA_STREAM, a)))
        assertEquals(
            listOf(a, b),
            RunImport.urisFrom(
                Intent(Intent.ACTION_SEND_MULTIPLE).putParcelableArrayListExtra(Intent.EXTRA_STREAM, arrayListOf(a, b)),
            ),
        )
        // The launcher / notification intents carry no import.
        assertTrue(RunImport.urisFrom(Intent(Intent.ACTION_MAIN)).isEmpty())
        assertTrue(RunImport.urisFrom(null).isEmpty())
    }
}
