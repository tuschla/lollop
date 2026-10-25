package app.milestone

import android.content.Context
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.test.core.app.ApplicationProvider
import java.io.File
import java.time.LocalDateTime
import java.time.ZoneOffset
import java.util.Locale
import java.util.TimeZone
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * Adversarial review proofs for the 2026-10-23 run fixes (import sport gate,
 * logged-at stamp, started_at captions, crash-sidecar boundaries). An
 * `@Ignore("REVIEW: …")` test FAILS on the current tree and proves a finding;
 * the others verify a fix claim.
 */
@RunWith(RobolectricTestRunner::class)
class ReviewRunsTest {

    @get:Rule
    val rule = createComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    private fun utc(y: Int, mo: Int, d: Int, h: Int, mi: Int): Long =
        LocalDateTime.of(y, mo, d, h, mi).toEpochSecond(ZoneOffset.UTC)

    // ── started_at vs observed_at on one label ────────────────────────────────

    @Test
    fun aRunAcrossMidnightIsNotTitledWithADifferentDayThanItsDate() {
        Locale.setDefault(Locale.US)
        TimeZone.setDefault(TimeZone.getTimeZone("UTC"))
        // Sun 2026-10-18 23:40 → Mon 2026-10-19 00:20 (a tracked run: started_at is
        // its first fix, observed_at its last). Viewed two weeks later, so the chip
        // is an absolute date.
        val run = RunResultView(
            distance_km = 7.0,
            duration_min = 40.0,
            observed_at = utc(2026, 10, 19, 0, 20),
            started_at = utc(2026, 10, 18, 23, 40),
            entry_id = 1,
            has_gpx = true,
        )
        val now = utc(2026, 11, 2, 12, 0) * 1000L
        var overline = ""
        var chip = ""
        rule.setContent {
            overline = runOverline(run, now)
            chip = runDateChip(run, now)
        }
        rule.waitForIdle()
        // One label, one day: the title names the START weekday (Sun Oct 18), so the
        // chip beside it must date that same day, never Monday Oct 19 (its end).
        assertEquals(formatLogDate(run.started_at, now), chip)
        assertTrue("chip \"$chip\" dates the start day", "18" in chip)
        assertTrue("overline \"$overline\" names the start weekday", overline.startsWith("Sunday"))
        assertTrue("overline \"$overline\" carries the chip", overline.endsWith(chip))
    }

    // ── nonRunSport: the recogniser misses Strava's own compound ride/ski types ──

    @Test
    fun stravaCompoundRideAndSkiTypesAreRecognised() {
        // Strava's ride types the reader's own comment cites ("Ride", "VirtualRide",
        // "EBikeRide") are recognised; these siblings of the same enum are not.
        val missed = listOf(
            "MountainBikeRide", "GravelRide", "EMountainBikeRide", "Handcycle", "Velomobile",
            "NordicSki", "AlpineSki", "BackcountrySki", "RollerSki", "mountainbiking",
        ).filter { nonRunSport(it) == null }
        assertEquals("recognised as not a run: none missed", emptyList<String>(), missed)
    }

    @Test
    fun aGravelRideFileIsRefused() {
        // 20 km/h for 30 min = 5.56 m/s, 180 s/km: slower than the 170 s/km
        // implausible-pace gate, so only the sport gate could refuse it.
        val mPerDegLon = 6_371_000.0 * Math.PI / 180.0
        val fixes = (0..1800 step 5).map { t -> GpxFix(0.0, 5.56 * t / mPerDegLon, 1_790_000_000L + t, null) }
        val outcome = runCatching { importedRunEvent(ImportedRun(listOf(fixes), sport = "GravelRide"), null) }
        assertTrue(
            "GravelRide imported as a run: ${outcome.getOrNull()?.let { "LogRunTrack with ${it.points.size} fixes" }}",
            (outcome.exceptionOrNull() as? GpxImportException)?.reason == ImportFailure.NotARun,
        )
    }

    @Test
    fun runningLabelsInOtherLanguagesAndMixedLabelsStayImportable() {
        // Verification of "unknown labels, numbers and Other still import, and any
        // word containing run or jog always counts as a run": no false positive on
        // running in other languages, Strava/Garmin run types or mixed labels.
        val runs = listOf(
            "Trail run", "TrailRun", "VirtualRun", "Laufen", "Traillauf", "Course à pied", "Trail",
            "Carrera", "Correr", "Corsa", "Hardlopen", "Löpning", "Løb", "Juoksu", "Bieganie", "Běh",
            "Futás", "Koşu", "Бег", "ランニング", "跑步", "Skriešana", "Running/Walking", "Walk/Run",
            "Run/Walk", "Hike & Run", "Swimrun", "9", "1", "running", "street_running", "track_running",
            "Skyrunning", "Fell running", "Orienteering", "Treadmill", "Other",
        )
        val refused = runs.filter { nonRunSport(it) != null }.map { "$it→${nonRunSport(it)}" }
        assertEquals(emptyList<String>(), refused)
    }

    // ── GPX reader: the new extension and the track's own <type> ────────────────

    @Test
    fun loggedAtExtensionAndTrackTypeAreReadOnlyWhereTheyBelong() {
        // A core-shaped export (namespace declared, extension before <trkseg>) plus a
        // Garmin-style <type> inside <extensions> and a <link><type>: the stamp is
        // read, the nested types are not the track's sport.
        val gpx = """
            <?xml version="1.0" encoding="UTF-8"?>
            <gpx version="1.1" creator="x" xmlns="http://www.topografix.com/GPX/1/1" xmlns:lollop="urn:lollop:gpx:1" xmlns:gx="urn:other">
              <trk>
                <name>n</name>
                <link href="http://x"><type>text/html</type></link>
                <extensions><gx:meta><type>cycling</type></gx:meta><lollop:logged_at>2026-09-21T14:20:18Z</lollop:logged_at></extensions>
                <trkseg>
                  <trkpt lat="0" lon="0"><time>2026-09-21T14:13:20Z</time></trkpt>
                  <trkpt lat="0" lon="0.001"><time>2026-09-21T14:13:50Z</time></trkpt>
                </trkseg>
              </trk>
            </gpx>
        """.trimIndent()
        val run = parseGpxRun(gpx)
        assertNull(run.sport)
        assertEquals(utc(2026, 9, 21, 14, 20) + 18, run.loggedAtSec)
        assertEquals(2, run.segments.single().size)
    }

    @Test
    fun aGpxCutOffAnywhereIsReportedDamaged() {
        // FixImport B6: "a truncated GPX is now Damaged". A cut-off download stops at
        // an arbitrary byte; try every cut past the root inside the track.
        val full = buildString {
            append("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<gpx version=\"1.1\" creator=\"x\" xmlns=\"http://www.topografix.com/GPX/1/1\">\n<trk><name>n</name><trkseg>\n")
            for (t in 0 until 40) append("<trkpt lat=\"0\" lon=\"${t * 3e-5}\"><time>2026-09-21T14:13:${(t % 60).toString().padStart(2, '0')}Z</time></trkpt>\n")
            append("</trkseg></trk>\n</gpx>\n")
        }
        val bytes = full.toByteArray()
        val start = full.indexOf("<trkseg>") + 8
        val outcomes = HashMap<String, Int>()
        val examples = ArrayList<String>()
        // Only cuts that lose data: before the closing </trkseg>.
        for (cut in start until full.lastIndexOf("</trkseg>")) {
            val file = ParsedRunFile("cut.gpx", runCatching { parseRunFileSegments(bytes.copyOf(cut)) { null } })
            val plan = planRunImport(listOf(file), emptySet()) { importedRunEvent(it, null) }
            val key = plan.failures.firstOrNull()?.reason?.name ?: "imported"
            if (key != "Damaged" && examples.size < 3) {
                examples += "$key after …${full.substring(maxOf(0, cut - 30), cut).replace("\n", "⏎")}" +
                    (plan.runs.firstOrNull()?.let { " (${it.event.points.size} of 40 fixes)" } ?: "")
            }
            outcomes[key] = (outcomes[key] ?: 0) + 1
            if (key != "Damaged") outcomes["lastNonDamagedOffset"] = cut - start
        }
        outcomes["fixLineBytes"] = full.indexOf("\n", start + 1) - start
        assertEquals("every data-losing truncation is Damaged: $outcomes, e.g. $examples", setOf("Damaged"), outcomes.keys - setOf("lastNonDamagedOffset", "fixLineBytes"))
    }

    @Test
    fun aGpxCutOffInItsSecondSegmentNeverImportsAPartialRun() {
        val fixLine = { t: Int -> "<trkpt lat=\"0\" lon=\"${t * 3e-5}\"><time>2026-09-21T14:${(13 + t / 60).toString().padStart(2, '0')}:${(t % 60).toString().padStart(2, '0')}Z</time></trkpt>\n" }
        val full = buildString {
            append("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<gpx version=\"1.1\" creator=\"x\" xmlns=\"http://www.topografix.com/GPX/1/1\">\n<trk><name>n</name>\n<trkseg>\n")
            for (t in 0 until 200) append(fixLine(t))
            append("</trkseg>\n<trkseg>\n")
            for (t in 300 until 500) append(fixLine(t))
            append("</trkseg></trk>\n</gpx>\n")
        }
        val bytes = full.toByteArray()
        val secondStart = full.lastIndexOf("<trkseg>") + 8
        val partial = ArrayList<String>()
        for (cut in secondStart until full.lastIndexOf("</trkseg>")) {
            val file = ParsedRunFile("cut.gpx", runCatching { parseRunFileSegments(bytes.copyOf(cut)) { null } })
            val plan = planRunImport(listOf(file), emptySet()) { importedRunEvent(it, null) }
            plan.runs.firstOrNull()?.let { if (partial.size < 3) partial += "cut at +${cut - secondStart}: ${it.event.points.size} of 400 fixes" }
        }
        assertEquals("a truncated file imported as a shorter run", emptyList<String>(), partial)
    }

    // ── crash sidecar: an old (pre-"seg") sidecar still recovers ────────────────

    @Test
    fun anOldSidecarWithoutSegmentMarkersStillRecoversByGapRule() {
        // Backward compatibility: a sidecar written before this change carries no
        // "seg" keys. It must still recover every fix, with boundaries re-derived
        // from fix-time gaps only (the pre-change behaviour).
        val f = File(ctx.filesDir, "active-run.ndjson")
        // The store caches its file from the first init in this JVM, which may be
        // another test's (now gone) files dir: point it at this one.
        ActiveRunStore.javaClass.getDeclaredField("file").apply { isAccessible = true }.set(ActiveRunStore, null)
        ActiveRunStore.init(ctx)
        val lines = buildList {
            add("{\"run\":\"active\"}")
            for (t in 0 until 20) add("{\"lat\":0.0,\"lon\":${t * 3e-5},\"observed_at\":${1_790_000_000 + t},\"accuracy_m\":5.0}")
            // 60 s without a fix (a gap over MAX_FIX_GAP_SEC), then 10 more.
            for (t in 80 until 90) add("{\"lat\":0.0,\"lon\":${t * 3e-5},\"observed_at\":${1_790_000_000 + t},\"accuracy_m\":5.0}")
            add("{\"lat\":0.0,\"lon\":0.1,\"obse") // torn final line
        }
        f.writeText(lines.joinToString("\n"))
        val recovered = ActiveRunStore.recover(ctx)
        assertEquals(30, recovered.size)
        assertEquals(emptyList<Int>(), recovered.segmentStarts)
        RunSession.reset()
        RunSession.restore(recovered)
        // The gap leg is a boundary; 28 one-second legs remain moving.
        assertEquals(28L, RunSession.elapsedSec.value)
        RunSession.reset()
        f.delete()
    }

    @Test
    fun theFirstLiveFixAfterARecoveryIsMarkedAsASegmentStart() {
        // After restore the splice to the first live fix is a boundary (pendingBreak),
        // and the sidecar must mark it, or a second crash would re-join the splice.
        RunSession.reset()
        val recovered = (0 until 10).map { GpsPoint(0.0, it * 3e-5, 1_790_000_000L + it, 5.0) }
        RunSession.restore(RecoveredTrack(recovered, emptyList()))
        val live = GpsPoint(0.001, 0.001, 1_790_000_500L, 5.0)
        val nanos = (android.os.SystemClock.elapsedRealtime() + 1_000L) * 1_000_000L
        assertTrue(RunSession.add(live, nanos))
        assertTrue(RunSession.isSegmentStart(live))
        RunSession.reset()
    }
}
