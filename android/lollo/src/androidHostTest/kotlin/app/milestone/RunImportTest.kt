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
 * [parseRunFile] over the REAL core, [RunImport.urisFrom]): every skipped file is
 * named with its reason, one bad file never blocks the rest, an implausibly fast or
 * too short track never becomes running volume, duplicates are skipped and counted,
 * and every system entry point (VIEW, SEND, SEND_MULTIPLE) yields its files.
 * Robolectric for the real Intent and string resources. The core's gates themselves
 * are core tests (`shared/src/import/tests.rs`).
 */
@RunWith(RobolectricTestRunner::class)
class RunImportTest {

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    /** A GPX with one `<trkseg>` per segment of (lat, unix second) fixes on the meridian,
     *  each `<trk>` typed [type] when given. */
    private fun gpx(vararg tracks: List<List<Pair<Double, Long>>>, type: String? = null): ByteArray =
        gpxDocument(*tracks.map { meridianTrack(it, type) }.toTypedArray()).toByteArray()

    private fun meridianTrack(segments: List<List<Pair<Double, Long>>>, type: String? = null) =
        GpxTrack(segments.map { seg -> seg.map { (lat, t) -> GpsPoint(lat, 0.0, t, 0.0) } }, type)

    private fun track(endSec: Long, metresPerFix: Double, fixes: Int = 61): List<List<Pair<Double, Long>>> {
        val start = endSec - (fixes - 1) * 10L
        return listOf(List(fixes) { i -> i * metresPerFix / M_PER_DEG to start + i * 10L })
    }

    /** An easy 6:00 /km run. */
    private fun easyRun(endSec: Long) = track(endSec, metresPerFix = 27.8)

    private fun read(bytes: ByteArray) = runCatching { parseRunFile(bytes, Core::parseRunFile) }

    private fun plan(files: List<ParsedRunFile>, existing: Set<Long> = emptySet()) =
        planRunImport(files, existing, ::importedRunEvent)

    @Test
    fun eachSkippedFileIsNamedWithItsReasonAndNeverBlocksTheOthers() {
        val p = plan(
            listOf(
                ParsedRunFile("morning.gpx", read(gpx(easyRun(10_000)))),
                ParsedRunFile("broken.gpx", read(gpx(listOf(emptyList())))),
                ParsedRunFile("bike.gpx", read(gpx(easyRun(15_000), type = "cycling"))),
                ParsedRunFile("unreadable.fit", Result.failure(ImportException(ImportFailure.ReadFailed))),
                ParsedRunFile("evening.gpx", read(gpx(easyRun(20_000), easyRun(30_000)))),
            ),
        )
        assertEquals(listOf(10_000L, 20_000L, 30_000L), p.runs.map { it.event.observedAt })
        assertEquals(listOf("morning.gpx", "evening.gpx", "evening.gpx"), p.runs.map { it.fileName })
        assertEquals(
            listOf(
                RunImportFailure("broken.gpx", ImportFailure.NoTrack),
                RunImportFailure("bike.gpx", ImportFailure.NotARun, "cycling"),
                RunImportFailure("unreadable.fit", ImportFailure.ReadFailed),
            ),
            p.failures,
        )
        assertEquals(0, p.duplicates)
    }

    @Test
    fun anImplausiblyFastTrackOrAFragmentIsSkippedNotLoggedAsARun() {
        // 12 m/s ≈ 1:23 /km: a ride or a drive. A 13 s, 3 m blip; 0.6 km in 2:30.
        val p = plan(
            listOf(
                ParsedRunFile("ride.gpx", read(gpx(track(10_000, metresPerFix = 120.0)))),
                ParsedRunFile("blip.gpx", read(gpx(listOf(listOf(0.0 to 9_987L, 3.0 / M_PER_DEG to 10_000L))))),
                ParsedRunFile("quick.gpx", read(gpx(track(20_000, metresPerFix = 40.0, fixes = 16)))),
                ParsedRunFile("run.gpx", read(gpx(easyRun(30_000)))),
            ),
        )
        assertEquals(listOf("run.gpx"), p.runs.map { it.fileName })
        assertEquals(
            listOf(
                RunImportFailure("ride.gpx", ImportFailure.ImplausiblePace),
                RunImportFailure("blip.gpx", ImportFailure.TooShort),
                RunImportFailure("quick.gpx", ImportFailure.TooShort),
            ),
            p.failures,
        )
    }

    @Test
    fun eachGpxTrackIsItsOwnRunWithItsOwnSport() {
        // A run and a ride in one GPX: the run imports, the ride is refused.
        val doc = gpxDocument(meridianTrack(easyRun(10_000), "running"), meridianTrack(easyRun(20_000), "cycling"))
        val p = plan(listOf(ParsedRunFile("day.gpx", read(doc.toByteArray()))))
        assertEquals(listOf(10_000L), p.runs.map { it.event.observedAt })
        assertTrue(p.failures.isEmpty())
    }

    @Test
    fun runsAlreadyInHistoryOrRepeatedInTheBatchAreSkippedAndCounted() {
        // 10 000 is already logged; 20 000 appears in two shared files (same run exported
        // twice). Each must count once toward weekly volume, never twice.
        val p = plan(
            listOf(
                ParsedRunFile("a.gpx", read(gpx(easyRun(10_000), easyRun(20_000)))),
                ParsedRunFile("b.gpx", read(gpx(easyRun(20_000), easyRun(40_000)))),
            ),
            existing = setOf(10_000L),
        )
        assertEquals(listOf(20_000L, 40_000L), p.runs.map { it.event.observedAt })
        assertEquals(2, p.duplicates)
        assertTrue(p.failures.isEmpty())
    }

    @Test
    fun theTooShortReasonReadsInTheDisplayUnit() {
        EventDispatcher.model.value = ViewModel(profile = Profile(distance_unit = DistanceUnit.Mi))
        try {
            assertEquals(
                "too short for a run, under 0.3 mi or 3 minutes",
                importFailureReason(ctx.resources, RunImportFailure("Activity.fit", ImportFailure.TooShort)),
            )
        } finally {
            EventDispatcher.model.value = null
        }
    }

    @Test
    fun aFileThatIsNeitherFitNorGpxNorTcxIsRefusedFromItsHeader() {
        val e = read("name,distance\nmonday,5".encodeToByteArray()).exceptionOrNull() as ImportException
        assertEquals(ImportFailure.WrongType, e.reason)
    }

    @Test
    fun summaryNamesEverySkippedFileWithItsReason() {
        val skipped = listOf(
            RunImportFailure("broken.gpx", ImportFailure.NoTrack),
            RunImportFailure("bike.fit", ImportFailure.NotARun, "cycling"),
        )
        val s = runImportSummary(ctx.resources, imported = 7, newestImportedAt = 1_700_000_000L, duplicates = 2, failures = skipped)
        assertEquals(
            "Imported 7 runs · 2 already in history. Skipped: broken.gpx (no GPS points), bike.fit (a cycling activity, not a run)",
            s.text,
        )
        assertTrue("two skips are named inline, no details list", s.details.isEmpty())
    }

    @Test
    fun summaryMovesManySkippedFilesBehindDetails() {
        val skipped = (1..3).map { RunImportFailure("f$it.gpx", ImportFailure.ImplausiblePace) }
        val s = runImportSummary(ctx.resources, imported = 0, newestImportedAt = null, duplicates = 0, failures = skipped)
        assertEquals("No runs imported. Skipped 3 files", s.text)
        assertEquals((1..3).map { "f$it.gpx (pace too fast for a run)" }, s.details)
    }

    @Test
    fun aSingleImportedRunNamesItsDateAndOnlyDuplicatesKeepTheDuplicateMessage() {
        val res = ctx.resources
        assertEquals(
            "Imported a run from ${importDateLabel(1_700_000_000L)}",
            runImportSummary(res, imported = 1, newestImportedAt = 1_700_000_000L, duplicates = 0, failures = emptyList()).text,
        )
        assertEquals(
            "Already imported: a logged run ends at the same time",
            runImportSummary(res, imported = 0, newestImportedAt = null, duplicates = 3, failures = emptyList()).text,
        )
    }

    @Test
    fun everySystemEntryPointYieldsItsFiles() {
        val a = Uri.parse("content://files/a.gpx")
        val b = Uri.parse("content://files/b.fit")
        assertEquals(listOf(a), RunImport.urisFrom(ctx, Intent(Intent.ACTION_VIEW).setData(a)))
        assertEquals(listOf(a), RunImport.urisFrom(ctx, Intent(Intent.ACTION_SEND).putExtra(Intent.EXTRA_STREAM, a)))
        assertEquals(
            listOf(a, b),
            RunImport.urisFrom(
                ctx,
                Intent(Intent.ACTION_SEND_MULTIPLE).putParcelableArrayListExtra(Intent.EXTRA_STREAM, arrayListOf(a, b)),
            ),
        )
        // The launcher / notification intents carry no import.
        assertTrue(RunImport.urisFrom(ctx, Intent(Intent.ACTION_MAIN)).isEmpty())
        assertTrue(RunImport.urisFrom(ctx, null).isEmpty())
    }

    @Test
    fun onlyOtherAppsContentUrisAreImported() {
        val private = Uri.parse("file:///data/data/app.milestone/files/events.ndjson")
        val own = Uri.parse("content://${ctx.packageName}.fileprovider/shared/run.gpx")
        val other = Uri.parse("content://files/a.gpx")
        assertTrue(RunImport.urisFrom(ctx, Intent(Intent.ACTION_SEND).putExtra(Intent.EXTRA_STREAM, private)).isEmpty())
        assertTrue(RunImport.urisFrom(ctx, Intent(Intent.ACTION_VIEW).setData(own)).isEmpty())
        assertEquals(
            listOf(other),
            RunImport.urisFrom(
                ctx,
                Intent(Intent.ACTION_SEND_MULTIPLE).putParcelableArrayListExtra(Intent.EXTRA_STREAM, arrayListOf(private, own, other)),
            ),
        )
        val clipped = Intent(Intent.ACTION_SEND).apply {
            clipData = android.content.ClipData.newRawUri(null, private).apply { addItem(android.content.ClipData.Item(other)) }
        }
        assertEquals(listOf(other), RunImport.urisFrom(ctx, clipped))
    }
}
