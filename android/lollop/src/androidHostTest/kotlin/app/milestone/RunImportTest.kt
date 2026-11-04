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

    private fun ok(vararg runs: List<List<GpxFix>>) = Result.success(runs.map { ImportedRun(it) })

    private fun failed(reason: ImportFailure, detail: String? = null) =
        Result.failure<List<ImportedRun>>(ImportException(reason, detail))

    private fun plan(files: List<ParsedRunFile>, existing: Set<Long> = emptySet()) =
        planRunImport(files, existing) { run -> importedRunEvent(run, measuredHrMax = null) }

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
        assertFalse(legTotals(brisk, emptyList()).implausiblyFast)
        // 2:30 /km while moving, standing still every other 10 s: 5:00 /km elapsed, but
        // the moving pace is what counts, so it is refused.
        val stopStart = List(41) { i -> GpsPoint(lat = ((i + 1) / 2) * deg(66.7), lon = 0.0, observedAt = 1_000L + i * 10L, accuracyM = 5.0) }
        assertTrue(legTotals(stopStart, emptyList()).implausiblyFast)
    }

    @Test
    fun eachGpxTrackIsItsOwnRunWithItsOwnSport() {
        // A 10 km run and a 5 km ride in one GPX: the run imports, the ride is refused.
        fun trkpts(n: Int, lat0: Double, t0: Long, mps: Double) = buildString {
            for (i in 0 until n) {
                append("<trkpt lat=\"${lat0 + i * deg(mps)}\" lon=\"8.54\"><time>${java.time.Instant.ofEpochSecond(t0 + i)}</time></trkpt>")
            }
        }
        val t0 = 1_750_000_000L
        val rideStart = t0 + 3000 + 60
        val doc = "<?xml version=\"1.0\"?><gpx version=\"1.1\">" +
            "<trk><type>running</type><trkseg>${trkpts(3000, 47.37, t0, 3.33)}</trkseg></trk>" +
            "<trk><type>cycling</type><trkseg>${trkpts(720, 47.37 + deg(3000 * 3.33), rideStart, 7.0)}</trkseg></trk></gpx>"
        val runs = parseRunFileSegments(doc.toByteArray()) { fail("not a FIT file"); null }
        assertEquals(listOf("running", "cycling"), runs.map { it.sport })
        val p = plan(listOf(ParsedRunFile("day.gpx", Result.success(runs))))
        assertEquals(listOf(t0 + 2999), p.runs.map { it.event.observedAt })
        assertTrue(p.failures.isEmpty())
    }

    @Test
    fun aFitOverItsCapIsRefusedBeforeTheCoreParsesIt() {
        fun fitHeader(size: Int) = ByteArray(size).also { ".FIT".toByteArray().copyInto(it, destinationOffset = 8) }
        try {
            parseRunFileSegments(fitHeader(MAX_FIT_IMPORT_BYTES + 1)) { fail("parsed an oversized FIT"); null }
            fail("an oversized FIT must not import")
        } catch (e: ImportException) {
            assertEquals(ImportFailure.TooLarge, e.reason)
        }
        var parsed = false
        runCatching { parseRunFileSegments(fitHeader(MAX_FIT_IMPORT_BYTES)) { parsed = true; null } }
        assertTrue("a FIT at the cap reaches the core", parsed)
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
    fun aFitErrorEnvelopeIsRefusedWithItsNamedReason() {
        val cases = listOf(
            """{"error":{"kind":"not_a_run","context":"cycling","message":"This FIT file records cycling, not a run"}}""" to ImportFailure.NotARun,
            // A 13 s fragment under the keep threshold.
            """{"error":{"kind":"no_track","context":"too_short","message":"Too short to import as a run"}}""" to ImportFailure.TooShort,
            """{"error":{"kind":"no_track","context":"","message":"No GPS points"}}""" to ImportFailure.NoTrack,
            // The legacy plain-string error and a null JNI result read as a damaged file.
            """{"error":"Couldn't read this FIT file"}""" to ImportFailure.Damaged,
            null to ImportFailure.Damaged,
        )
        for ((json, reason) in cases) {
            val e = runCatching { fitSegments(json) }.exceptionOrNull()
            assertEquals(json, reason, (e as? ImportException)?.reason)
        }
        val sport = runCatching { fitSegments(cases[0].first) }.exceptionOrNull() as ImportException
        assertEquals("cycling", sport.detail)
    }

    @Test
    fun theTooShortReasonReadsInTheDisplayUnit() {
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
        } catch (e: ImportException) {
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

    @Test
    fun onlyOtherAppsContentUrisAreImported() {
        val private = Uri.parse("file:///data/data/app.milestone/files/events.ndjson")
        val own = Uri.parse("content://app.milestone.fileprovider/shared/run.gpx")
        val other = Uri.parse("content://files/a.gpx")
        assertTrue(RunImport.urisFrom(Intent(Intent.ACTION_SEND).putExtra(Intent.EXTRA_STREAM, private)).isEmpty())
        assertTrue(RunImport.urisFrom(Intent(Intent.ACTION_VIEW).setData(own)).isEmpty())
        assertEquals(
            listOf(other),
            RunImport.urisFrom(
                Intent(Intent.ACTION_SEND_MULTIPLE).putParcelableArrayListExtra(Intent.EXTRA_STREAM, arrayListOf(private, own, other)),
            ),
        )
        val clipped = Intent(Intent.ACTION_SEND).apply {
            clipData = android.content.ClipData.newRawUri(null, private).apply { addItem(android.content.ClipData.Item(other)) }
        }
        assertEquals(listOf(other), RunImport.urisFrom(clipped))
    }
}
