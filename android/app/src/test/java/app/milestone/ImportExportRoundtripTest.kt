package app.milestone

import android.content.Context
import android.net.Uri
import android.os.Looper
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.test.core.app.ApplicationProvider
import app.milestone.qa.ProcessDeath
import app.milestone.qa.QaHarness
import app.milestone.qa.assertCountEventually
import app.milestone.qa.launchApp
import app.milestone.qa.launchAppExpectingSetup
import app.milestone.qa.plural
import app.milestone.qa.str
import java.io.File
import java.io.InputStream
import java.util.Locale
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.cos
import kotlin.math.sin
import kotlin.random.Random
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.async
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Before
import org.junit.Ignore
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf

/**
 * Export → import round trips, hunting for data loss and duplicates.
 *
 *  - The full data backup (Profile › Your data): the compacted event log written by
 *    [exportDataTo], validated by [readImportCandidate], swapped in by
 *    [replaceEventLog] and replayed by a fresh process, must rebuild the SAME
 *    History, plan and profile, and importing it twice must not double anything.
 *  - A run's GPX export ([Core.exportGpx]) re-imported through the real importer
 *    ([parseRunFileSegments] → [importedRunEvent]) must give back the same distance,
 *    moving time and pauses, and re-importing it next to the original must be
 *    recognised as the same run (no double-counted weekly volume).
 *  - Odd files (empty, one point, no timestamps, huge, another sport, the same file
 *    twice) each end in one clear, named outcome and never in a duplicate run.
 *
 * Everything runs the REAL core over JNI; the run-import batch rules are the
 * production [planRunImport] with the production [importedRunEvent] builder.
 */
@Ignore("PARKED: written but never run (owner parked the work before the first gradle run); verify, then BUG-ignore real failures")
@RunWith(RobolectricTestRunner::class)
class ImportExportRoundtripTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @Before
    fun setUp() {
        Locale.setDefault(Locale.US)
        QaHarness.reset(ctx)
        AppSettings.setDistanceUnitOverride(ctx, DistanceUnitOverride.Km)
    }

    @After
    fun tearDown() {
        QaHarness.teardown()
        RunSession.reset()
        AppSettings.distanceUnitOverride.value = DistanceUnitOverride.System
    }

    // ── Driving the process-scoped coroutines from the (main-looper) test thread ──

    /** Run [block] on the app's process scope and pump the paused main looper until
     *  it completes: the serial event consumer and every `Dispatchers.Main` hop
     *  resume there, so a plain runBlocking on this thread would deadlock. */
    @OptIn(ExperimentalCoroutinesApi::class)
    private fun <T> onAppScope(timeoutMs: Long = 30_000, block: suspend () -> T): T {
        val job = EventDispatcher.scope.async { block() }
        val end = System.currentTimeMillis() + timeoutMs
        while (!job.isCompleted) {
            shadowOf(Looper.getMainLooper()).idle()
            if (System.currentTimeMillis() > end) fail("app-scope work did not finish in ${timeoutMs}ms")
            Thread.sleep(2)
        }
        return job.getCompleted()
    }

    /** Pump the main looper until [cond] holds (the core's reply lands there). */
    private fun waitFor(timeoutMs: Long = 15_000, what: String = "condition", cond: () -> Boolean) {
        val end = System.currentTimeMillis() + timeoutMs
        while (!cond()) {
            shadowOf(Looper.getMainLooper()).idle()
            if (System.currentTimeMillis() > end) fail("timed out waiting for $what")
            Thread.sleep(2)
        }
    }

    private fun logLines(): List<String> =
        File(ctx.filesDir, "event-log.ndjson").takeIf { it.exists() }?.readLines()?.filter { it.isNotBlank() }.orEmpty()

    // ── The core without the UI (GPX round trips, import planning) ──────────────

    /** Seed [seed] as the durable log, replay it into the real core and date it to
     *  the pinned today, as boot does. Later [Core.send]s append to that log. */
    private fun bootCore(vararg seed: Event): ViewModel {
        QaHarness.seedLog(ctx, seed.toList())
        Core.restore(ctx)
        return Core.send(Event.SetToday(todayEpochDay(), utcOffsetSec())).view
    }

    /** A brand-new install (empty log, empty core) holding only [seed]. */
    private fun freshInstallCore(vararg seed: Event): ViewModel {
        QaHarness.reset(ctx)
        return bootCore(*seed)
    }

    private fun send(e: Event): ViewModel {
        val r = Core.send(e)
        assertTrue("core refused ${e::class.simpleName}", r.accepted)
        return r.view
    }

    // ── Synthetic, seeded tracks ────────────────────────────────────────────────

    private val metresPerDegLat = 111_195.0

    /**
     * A seeded wandering run: [n] fixes every [dtSec] s at about [mps] (±10 % per
     * leg, heading drifting), accuracy 4–8 m. A fix index in [pauseAt] opens a new
     * recording segment after a [pauseSec] stop during which the runner walked 60 m
     * (a pause bridge the core must not count). Returns the points and the
     * segment-start indices, as the tracker hands them to the core.
     */
    private fun synthRun(
        seed: Long,
        startSec: Long,
        n: Int,
        dtSec: Long = 3,
        mps: Double = 3.2,
        pauseAt: Set<Int> = emptySet(),
        pauseSec: Long = 150,
    ): Pair<List<GpsPoint>, List<Int>> {
        val rnd = Random(seed)
        var lat = 47.37
        var lon = 8.54
        var t = startSec
        var heading = rnd.nextDouble(0.0, 2 * PI)
        val pts = ArrayList<GpsPoint>(n)
        val starts = ArrayList<Int>()
        for (i in 0 until n) {
            if (i > 0) {
                val d = if (i in pauseAt) 60.0 else mps * dtSec * (0.9 + 0.2 * rnd.nextDouble())
                t += if (i in pauseAt) pauseSec else dtSec
                heading += (rnd.nextDouble() - 0.5) * 0.3
                lat += d * cos(heading) / metresPerDegLat
                lon += d * sin(heading) / (metresPerDegLat * cos(Math.toRadians(lat)))
                if (i in pauseAt) starts += i
            }
            pts += GpsPoint(lat, lon, t, 4.0 + rnd.nextDouble() * 4.0)
        }
        return pts to starts
    }

    private fun trackEvent(points: List<GpsPoint>, starts: List<Int> = emptyList(), entryId: Long = points.last().observedAt * 1000) =
        Event.LogRunTrack(
            points = points,
            hrPctMax = 0.0,
            longestRecentKm = 0.0,
            // What the tracker stamps: the run's last fix (RunTracking's save).
            observedAt = points.last().observedAt,
            entryId = entryId,
            segmentStarts = starts,
        )

    /** A GPX 1.1 document with one `<trkseg>` per segment (optionally untimed, or
     *  carrying a `<type>`), the way other apps write them. */
    private fun gpxDoc(segments: List<List<GpxFix>>, type: String? = null, timed: Boolean = true): String = buildString {
        append("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n")
        append("<gpx version=\"1.1\" creator=\"test\" xmlns=\"http://www.topografix.com/GPX/1/1\">\n<trk><name>t</name>")
        if (type != null) append("<type>$type</type>")
        for (seg in segments) {
            append("<trkseg>")
            for (f in seg) {
                append("<trkpt lat=\"%.7f\" lon=\"%.7f\">".format(Locale.US, f.lat, f.lon))
                if (timed) append("<time>").append(java.time.Instant.ofEpochSecond(f.timeSec)).append("</time>")
                append("</trkpt>\n")
            }
            append("</trkseg>")
        }
        append("</trk></gpx>\n")
    }

    /** The same fixes as a single-activity TCX with [sport]. */
    private fun tcxDoc(fixes: List<GpxFix>, sport: String): String = buildString {
        append("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n")
        append("<TrainingCenterDatabase xmlns=\"http://www.garmin.com/xmlschemas/TrainingCenterDatabase/v2\">")
        append("<Activities><Activity Sport=\"$sport\"><Id>").append(java.time.Instant.ofEpochSecond(fixes.first().timeSec)).append("</Id>")
        append("<Lap StartTime=\"").append(java.time.Instant.ofEpochSecond(fixes.first().timeSec)).append("\"><Track>")
        for (f in fixes) {
            append("<Trackpoint><Time>").append(java.time.Instant.ofEpochSecond(f.timeSec)).append("</Time>")
            append("<Position><LatitudeDegrees>%.7f</LatitudeDegrees><LongitudeDegrees>%.7f</LongitudeDegrees></Position>".format(Locale.US, f.lat, f.lon))
            append("</Trackpoint>")
        }
        append("</Track></Lap></Activity></Activities></TrainingCenterDatabase>\n")
    }

    private fun List<GpsPoint>.asFixes() = map { GpxFix(it.lat, it.lon, it.observedAt, hrBpm = null) }

    /** The production batch rules over real parsers: what [RunImport.importRuns]
     *  would send for [files] next to a history whose runs end at [existing]. */
    private fun planImport(existing: Set<Long>, vararg files: Pair<String, ByteArray>): RunImportPlan =
        planRunImport(
            files = files.map { (name, bytes) -> ParsedRunFile(name, runCatching { parseRunFileSegments(bytes, Core::parseFit) }) },
            existingObservedAt = existing,
            build = { importedRunEvent(it, null) },
        )

    private fun reasonsByFile(plan: RunImportPlan) = plan.failures.associate { it.fileName to it.reason }

    private fun fixture(name: String): ByteArray =
        checkNotNull(javaClass.classLoader!!.getResourceAsStream(name)) { "fixture $name not on the test classpath" }.use { it.readBytes() }

    private fun gpxOf(run: RunResultView): String =
        checkNotNull(Core.exportGpx(run.entry_id, run.observed_at)?.takeIf { it.isNotBlank() }) { "no GPX for ${run.entry_id}" }

    private fun segmentCount(gpx: String) = parseGpx(gpx).size

    private fun assertSameRun(what: String, a: RunResultView, b: RunResultView) {
        val kmTol = 0.005 + a.distance_km * 0.002
        assertTrue("$what distance ${a.distance_km} vs ${b.distance_km}", abs(a.distance_km - b.distance_km) <= kmTol)
        assertTrue("$what moving time ${a.duration_min} vs ${b.duration_min} min", abs(a.duration_min - b.duration_min) <= 0.05)
        assertEquals("$what logged-at stamp", a.observed_at, b.observed_at)
    }

    // ═══ GPX: export → import ═══════════════════════════════════════════════════

    @Test
    fun aRunsGpxImportedIntoAFreshInstallKeepsItsDistanceMovingTimeAndPauses() {
        // ~5.6 km over two pauses (each a 150 s stop with a 60 m walk).
        val (points, starts) = synthRun(seed = 11, startSec = QaHarness.daysAgoAtMidday(2), n = 1800, pauseAt = setOf(600, 1200))
        val original = bootCore(QaHarness.strengthProfile()).let { send(trackEvent(points, starts)) }.runs.single()
        val gpx = gpxOf(original)
        assertEquals("one <trkseg> per recording segment", 3, segmentCount(gpx))

        val parsed = parseRunFileSegments(gpx.toByteArray(), Core::parseFit).single()
        val reimported = importedRunEvent(parsed, null)
        assertEquals("the imported run is stamped at the same last fix", original.observed_at, reimported.observedAt)

        val restored = freshInstallCore(QaHarness.strengthProfile()).let { send(reimported) }.runs.single()
        assertSameRun("GPX round trip", original, restored)
        assertTrue(restored.has_gpx)
        assertEquals("pauses survive the round trip", 3, segmentCount(gpxOf(restored)))
        // A second export → import cycle is a fixed point: nothing drifts.
        val again = freshInstallCore(QaHarness.strengthProfile())
            .let { send(importedRunEvent(parseRunFileSegments(gpxOf(restored).toByteArray(), Core::parseFit).single(), null)) }
            .runs.single()
        assertSameRun("second round trip", restored, again)
    }

    @Test
    fun aRunsOwnGpxImportedNextToItIsTheSameRunNotASecond() {
        val (points, _) = synthRun(seed = 12, startSec = QaHarness.daysAgoAtMidday(1), n = 900)
        val vm = bootCore(QaHarness.strengthProfile()).let { send(trackEvent(points)) }
        val gpx = gpxOf(vm.runs.single())
        val plan = planImport(vm.runs.mapTo(HashSet()) { it.observed_at }, "run.gpx" to gpx.toByteArray())
        assertEquals("nothing new to log", 0, plan.runs.size)
        assertEquals(1, plan.duplicates)
        assertTrue(plan.failures.isEmpty())
    }

    /**
     * A tracked run that ends standing still (the runner waits at the door before
     * pressing Stop) is stamped at its LAST raw fix, but the core's GPS QC drops the
     * sub-2.5 m wander, so the exported GPX ends earlier. Re-importing that GPX must
     * still be recognised as the same run, not logged as a second one.
     */
    @Test
    fun aRunEndingStandingStillIsStillRecognisedWhenItsGpxIsReimported() {
        val (moving, _) = synthRun(seed = 13, startSec = QaHarness.daysAgoAtMidday(1), n = 900)
        val last = moving.last()
        // 40 s at the door: fixes every 2 s within ~0.5 m of the finish.
        val rnd = Random(13)
        val standing = (1..20).map { k ->
            GpsPoint(last.lat + (rnd.nextDouble() - 0.5) * 1e-5, last.lon + (rnd.nextDouble() - 0.5) * 1e-5, last.observedAt + 2L * k, 5.0)
        }
        val vm = bootCore(QaHarness.strengthProfile()).let { send(trackEvent(moving + standing)) }
        val run = vm.runs.single()
        val plan = planImport(vm.runs.mapTo(HashSet()) { it.observed_at }, "run.gpx" to gpxOf(run).toByteArray())
        assertEquals("re-importing the run's own GPX adds no run", 0, plan.runs.size)
        assertEquals(1, plan.duplicates)
    }

    /** A trimmed run keeps its identity (entry id + logged-at stamp) while its
     *  exported GPX ends at the trimmed finish; re-importing it is still that run. */
    @Test
    fun aTrimmedRunsGpxReimportedIsStillTheSameRun() {
        val (points, _) = synthRun(seed = 14, startSec = QaHarness.daysAgoAtMidday(1), n = 900)
        var vm = bootCore(QaHarness.strengthProfile()).let { send(trackEvent(points)) }
        val run = vm.runs.single()
        vm = send(Event.TrimRunTrack(run.entry_id, run.observed_at, points.first().observedAt + 60, points.last().observedAt - 300))
        val trimmed = vm.runs.single()
        assertTrue("the trim shortened the run", trimmed.distance_km < run.distance_km - 0.1)
        val plan = planImport(vm.runs.mapTo(HashSet()) { it.observed_at }, "trimmed.gpx" to gpxOf(trimmed).toByteArray())
        assertEquals("re-importing the trimmed run's GPX adds no run", 0, plan.runs.size)
    }

    @Test
    fun theSameRunAsGpxAndAsTcxInOneBatchLogsOnce() {
        val (points, _) = synthRun(seed = 15, startSec = QaHarness.daysAgoAtMidday(3), n = 700)
        val fixes = points.asFixes()
        val plan = planImport(
            emptySet(),
            "garmin.gpx" to gpxDoc(listOf(fixes)).toByteArray(),
            "garmin.tcx" to tcxDoc(fixes, "Running").toByteArray(),
            "copy-of-garmin.gpx" to gpxDoc(listOf(fixes)).toByteArray(),
        )
        assertEquals(1, plan.runs.size)
        assertEquals(2, plan.duplicates)
        assertTrue(plan.failures.isEmpty())
        // And once it is in history, the same three files add nothing.
        val vm = bootCore(QaHarness.strengthProfile()).let { send(plan.runs.single().event) }
        val again = planImport(
            vm.runs.mapTo(HashSet()) { it.observed_at },
            "garmin.gpx" to gpxDoc(listOf(fixes)).toByteArray(),
            "garmin.tcx" to tcxDoc(fixes, "Running").toByteArray(),
        )
        assertEquals(0, again.runs.size)
        assertEquals(2, again.duplicates)
    }

    // ═══ Odd files ══════════════════════════════════════════════════════════════

    @Test
    fun oddFilesEachEndInTheirOwnNamedReasonAndTheGoodOneStillImports() {
        val t0 = QaHarness.daysAgoAtMidday(4)
        val (good, _) = synthRun(seed = 16, startSec = t0, n = 600)
        val (fast, _) = synthRun(seed = 17, startSec = t0 + 86_400, n = 600, mps = 9.0)
        val one = good.take(1).asFixes()
        val standingStill = (0 until 600).map { GpxFix(47.0, 8.0, t0 + 10L * it, null) }
        val routeOnly = "<?xml version=\"1.0\"?><gpx version=\"1.1\"><rte><rtept lat=\"47.0\" lon=\"8.0\"/><rtept lat=\"47.1\" lon=\"8.1\"/></rte></gpx>"
        val plan = planImport(
            emptySet(),
            "empty.gpx" to ByteArray(0),
            "notes.txt" to "just some text\n".toByteArray(),
            "route.gpx" to routeOnly.toByteArray(),
            "one-point.gpx" to gpxDoc(listOf(one)).toByteArray(),
            "untimed.gpx" to gpxDoc(listOf(good.asFixes()), timed = false).toByteArray(),
            "standing.gpx" to gpxDoc(listOf(standingStill)).toByteArray(),
            "car.gpx" to gpxDoc(listOf(fast.asFixes())).toByteArray(),
            "bike.fit" to fixture("garmin-fenix-5-bike.fit"),
            "fragment.fit" to fixture("Activity.fit"),
            "good.gpx" to gpxDoc(listOf(good.asFixes())).toByteArray(),
        )
        assertEquals("only the real run imports", listOf("good.gpx"), plan.runs.map { it.fileName })
        assertEquals(0, plan.duplicates)
        val reasons = reasonsByFile(plan)
        assertEquals("every other file is named exactly once", 9, plan.failures.size)
        assertEquals(ImportFailure.WrongType, reasons["empty.gpx"])
        assertEquals(ImportFailure.WrongType, reasons["notes.txt"])
        assertEquals(ImportFailure.NoTrack, reasons["route.gpx"])
        assertEquals(ImportFailure.NoTrack, reasons["one-point.gpx"])
        assertEquals(ImportFailure.NoTimestamps, reasons["untimed.gpx"])
        assertEquals(ImportFailure.TooShort, reasons["standing.gpx"])
        assertEquals(ImportFailure.ImplausiblePace, reasons["car.gpx"])
        assertEquals(ImportFailure.NotARun, reasons["bike.fit"])
        assertEquals(ImportFailure.TooShort, reasons["fragment.fit"])
        // Each reason reads as a plain sentence naming the file (no blank detail).
        for (f in plan.failures) {
            val line = importFailureLine(ctx.resources, f)
            assertTrue(line, line.startsWith(f.fileName) && line.length > f.fileName.length + 5)
        }
        assertTrue(importFailureReason(ctx.resources, plan.failures.single { it.fileName == "bike.fit" }).contains("cycling"))
        // The core accepts what the importer built, with sane numbers.
        val run = bootCore(QaHarness.strengthProfile()).let { send(plan.runs.single().event) }.runs.single()
        assertTrue("distance ${run.distance_km}", run.distance_km in 5.0..6.5)
        assertTrue("moving time ${run.duration_min}", run.duration_min in 25.0..35.0)
    }

    /** A cut-off download: the file starts like a GPX but the XML stops mid-point. */
    @Test
    fun aTruncatedGpxIsReportedAsDamaged() {
        val (good, _) = synthRun(seed = 18, startSec = QaHarness.daysAgoAtMidday(2), n = 300)
        val whole = gpxDoc(listOf(good.asFixes()))
        val plan = planImport(emptySet(), "cut.gpx" to whole.substring(0, whole.length / 2).toByteArray())
        assertEquals(0, plan.runs.size)
        assertEquals(ImportFailure.Damaged, reasonsByFile(plan)["cut.gpx"])
    }

    /**
     * A ride exported as TCX says so: `<Activity Sport="Biking">`. At an easy
     * 20 km/h it is slower than the importer's fastest-believable-run gate, so the
     * sport is the only thing that tells it apart. The FIT path refuses another
     * sport by name ([aFitFileForAnotherSportIsRefusedWithItsSport] in
     * RunImportTest); a TCX ride must not become running volume either.
     */
    @Test
    fun aTcxBikeRideIsRefusedAsNotARun() {
        val (ride, _) = synthRun(seed = 19, startSec = QaHarness.daysAgoAtMidday(2), n = 1200, mps = 5.6)
        val plan = planImport(emptySet(), "ride.tcx" to tcxDoc(ride.asFixes(), "Biking").toByteArray())
        assertEquals("a ride is never logged as a run", 0, plan.runs.size)
        assertEquals(ImportFailure.NotARun, reasonsByFile(plan)["ride.tcx"])
    }

    /** Strava's GPX export names the activity type in `<trk><type>`. */
    @Test
    fun aGpxTypedAsCyclingIsRefusedAsNotARun() {
        val (ride, _) = synthRun(seed = 20, startSec = QaHarness.daysAgoAtMidday(2), n = 1200, mps = 5.6)
        val plan = planImport(emptySet(), "ride.gpx" to gpxDoc(listOf(ride.asFixes()), type = "cycling").toByteArray())
        assertEquals(0, plan.runs.size)
        assertEquals(ImportFailure.NotARun, reasonsByFile(plan)["ride.gpx"])
    }

    @Test
    fun aHugeGpxImportsDecimatedWithTheSameDistanceAndAnOversizedFileIsRefusedUnread() {
        // 25 000 fixes at 1 Hz (~7 h, ~80 km): an ultra recorded at full rate.
        val (pts, _) = synthRun(seed = 21, startSec = QaHarness.daysAgoAtMidday(5), n = 25_000, dtSec = 1)
        val trueKm = pts.zipWithNext { a, b -> segmentKm(a, b) }.sum()
        val bytes = gpxDoc(listOf(pts.asFixes())).toByteArray()
        assertTrue("a multi-MB file", bytes.size > 2_000_000)
        val ev = importedRunEvent(parseRunFileSegments(bytes, Core::parseFit).single(), null)
        assertTrue("decimated to the save cap: ${ev.points.size}", ev.points.size <= TRACK_DECIMATION_CAP + 2)
        assertEquals("endpoints kept", pts.first().observedAt, ev.points.first().observedAt)
        assertEquals(pts.last().observedAt, ev.observedAt)
        val run = bootCore(QaHarness.strengthProfile()).let { send(ev) }.runs.single()
        assertTrue("distance ${run.distance_km} vs true $trueKm", abs(run.distance_km - trueKm) / trueKm < 0.02)
        assertTrue("moving time ${run.duration_min}", abs(run.duration_min - (pts.last().observedAt - pts.first().observedAt) / 60.0) < 2.0)

        // Past the 32 MB cap the read stops with "too large", never buffering it all.
        var served = 0L
        val endless = object : InputStream() {
            override fun read(): Int = if (served++ < 40L * 1024 * 1024) 'x'.code else -1
            override fun read(b: ByteArray, off: Int, len: Int): Int {
                if (served >= 40L * 1024 * 1024) return -1
                b.fill('x'.code.toByte(), off, off + len)
                served += len
                return len
            }
        }
        val e = runCatching { readCappedBytes(endless) }.exceptionOrNull()
        assertTrue("got $e", e is GpxImportException && e.reason == ImportFailure.TooLarge)
        assertTrue("stopped near the cap, read $served bytes", served <= 33L * 1024 * 1024)
    }

    // ═══ Run import through the real share/pick pipeline ═════════════════════════

    private fun writeCacheFile(name: String, bytes: ByteArray): Uri {
        val f = File(ctx.cacheDir, "import-test/$name")
        f.parentFile!!.mkdirs()
        f.writeBytes(bytes)
        return Uri.fromFile(f)
    }

    @Test
    fun importingTheSameFileTwiceLogsOneRunAndSaysItWasAlreadyImported() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        val (points, _) = synthRun(seed = 22, startSec = QaHarness.daysAgoAtMidday(2), n = 800)
        val uri = writeCacheFile("morning.gpx", gpxDoc(listOf(points.asFixes())).toByteArray())

        // Shared twice in one go: one run, the copy counted as already there.
        RunImport.importRuns(ctx, listOf(uri, uri))
        val dup = plural(R.plurals.main_import_summary_duplicates, 1, 1)
        rule.assertCountEventually(hasText(str(R.string.import_result_one, importDateLabel(points.last().observedAt)), substring = true), 1, 20_000)
        rule.assertCountEventually(hasText(dup, substring = true), 1)
        assertEquals(1, EventDispatcher.model.value!!.runs.size)

        // The same file again: nothing new, and it says why.
        RunImport.importRuns(ctx, listOf(uri))
        rule.assertCountEventually(hasText(str(R.string.main_import_duplicate)), 1, 20_000)
        assertEquals("still exactly one run", 1, EventDispatcher.model.value!!.runs.size)
        assertEquals("one LogRunTrack line in the log", 1, logLines().count { EventLog.variantOf(it) == "LogRunTrack" })

        // A cold start replays exactly that one run.
        ProcessDeath.kill(ctx)
        rule.launchApp()
        assertEquals(1, EventDispatcher.model.value!!.runs.size)
        assertEquals(points.last().observedAt, EventDispatcher.model.value!!.runs.single().observed_at)
    }

    // ═══ Full data backup: export → fresh install → import ═══════════════════════

    /** Profile, an anchored plan, three weeks of lifting and running (hand-logged
     *  and GPS), check-ins and a calculator query, with an amend, a delete and a
     *  trim so compaction has real work to do. Oldest first. */
    private fun richHistory(): List<Event> {
        val profile = Event.SetProfile(
            progressionCadence = ProgressionCadence.WeekToWeek,
            liftGoal = LiftGoal.Hypertrophy,
            goalDistance = GoalDistance.TenK,
            concurrentGoal = ConcurrentGoal.Hypertrophy,
            weeklySets = 12,
            runningDaysPerWeek = 3,
            runningKmPerWeek = 25.0,
            advanced = false,
            bodyweightKg = 78.5,
            ageYears = 34.0,
            restingHrBpm = 52.0,
        )
        val out = ArrayList<Event>()
        out += profile
        out += Event.GeneratePlan(todayEpochDay() - 20)
        val rnd = Random(23)
        var gpsRun: Event.LogRunTrack? = null
        var amendTarget: Event.LogSet? = null
        var deleteTarget: Event.LogSet? = null
        for (day in 20 downTo 1) {
            val noon = QaHarness.daysAgoAtMidday(day)
            out += Event.SubmitCheckin(noon - 4 * 3600, sleepQuality = 2 + rnd.nextInt(4), soreness = 1 + rnd.nextInt(3), mood = 3 + rnd.nextInt(3))
            when (day % 3) {
                0 -> repeat(4) { k ->
                    val at = noon + 180L * k
                    val set = Event.LogSet("Back Squat", 80.0 + 2.5 * (20 - day) / 3, 8, 7.5 + 0.5 * (k % 2), at, at * 1000)
                    out += set
                    if (day == 9 && k == 1) amendTarget = set
                    if (day == 12 && k == 3) deleteTarget = set
                }
                1 -> out += Event.LogRun(5.0 + rnd.nextInt(3), 30.0 + rnd.nextInt(10), 0.0, 0.0, noon, noon * 1000)
                else -> if (gpsRun == null) {
                    val (pts, starts) = synthRun(seed = 24, startSec = noon - 3600, n = 1200, pauseAt = setOf(700))
                    gpsRun = trackEvent(pts, starts).also { out += it }
                }
            }
        }
        val a = amendTarget!!
        out += Event.AmendSet(a.entryId, a.exercise, a.weightKg + 5.0, a.reps - 1, a.rpe, a.observedAt, a.observedAt)
        out += Event.DeleteEntry(Event.EntryKind.Set, deleteTarget!!.entryId, deleteTarget!!.observedAt)
        val g = gpsRun!!
        out += Event.TrimRunTrack(g.entryId, g.observedAt, g.points.first().observedAt + 90, g.points.last().observedAt - 120)
        out += Event.ComputeProtein(78.5, masters = false, deficit = false)
        return out
    }

    /** Every ViewModel property that differs between [a] and [b] (by name), except
     *  the replayed-event counter, which compaction legitimately lowers. */
    private fun diffViews(a: ViewModel, b: ViewModel): List<String> =
        ViewModel::class.java.declaredFields
            .filterNot { java.lang.reflect.Modifier.isStatic(it.modifiers) || it.name == "input_count" }
            .onEach { it.isAccessible = true }
            .filter { it.get(a) != it.get(b) }
            .map { "${it.name}:\n  before ${it.get(a)}\n  after  ${it.get(b)}" }

    /** Import [file] as the Profile › Your data flow does, then restart the process
     *  the way [restartForImport] does (the next boot replays the swapped log). */
    private fun importBackupAndRestart(file: File): ImportSummary {
        val check = onAppScope { readImportCandidate(ctx, Uri.fromFile(file)) }
        assertTrue("the backup validates", check is ImportCheck.Valid)
        val lines = (check as ImportCheck.Valid).lines
        val summary = summarizeImport(lines, java.time.ZoneId.systemDefault())
        assertEquals(DataImportResult.Replaced, onAppScope { replaceEventLog(ctx, lines) })
        ctx.setupPrefs().edit().putInt(KEY_RESTORED_ENTRIES, summary.entries).commit()
        ProcessDeath.kill(ctx)
        rule.launchApp()
        return summary
    }

    @Test
    fun aFullBackupImportedIntoAFreshInstallRebuildsTheSameHistoryPlanAndProfile() {
        QaHarness.seedLog(ctx, richHistory())
        rule.launchApp()
        // One entry logged live this session, so the backup holds a post-boot append.
        val before = EventDispatcher.model.value!!.lifts.size
        val liveAt = AppClock.nowSec() - 600
        EventDispatcher.send(Event.LogSet("Bench Press", 60.0, 8, 8.0, liveAt, liveAt * 1000))
        waitFor(what = "the live set") { EventDispatcher.model.value!!.lifts.size == before + 1 }
        val original = EventDispatcher.model.value!!
        assertNotNull(original.program)
        assertTrue(original.runs.any { it.has_gpx })

        val backup = File(ctx.cacheDir, "backup/${dataExportFilename()}").apply { parentFile!!.mkdirs() }
        assertTrue("export reports success", onAppScope { exportDataTo(ctx, Uri.fromFile(backup)) })
        assertTrue(backup.length() > 0)

        // A brand-new install on another phone: guided setup up, nothing logged.
        QaHarness.teardown()
        QaHarness.reset(ctx)
        AppSettings.setDistanceUnitOverride(ctx, DistanceUnitOverride.Km)
        rule.launchAppExpectingSetup()

        val summary = importBackupAndRestart(backup)
        val restored = EventDispatcher.model.value!!
        val diff = diffViews(original, restored)
        assertTrue("the restored install differs:\n" + diff.joinToString("\n"), diff.isEmpty())
        assertTrue("entries counted ${summary.entries}", summary.entries >= original.lifts.size + original.runs.size)
        rule.assertCountEventually(hasText(plural(R.plurals.main_restored_entries, summary.entries, summary.entries)), 1)

        // The very same backup imported again: still the same, nothing doubled.
        importBackupAndRestart(backup)
        val twice = EventDispatcher.model.value!!
        val diff2 = diffViews(original, twice)
        assertTrue("a second import changed:\n" + diff2.joinToString("\n"), diff2.isEmpty())
        assertEquals(original.lifts.size, twice.lifts.size)
        assertEquals(original.runs.size, twice.runs.size)
    }

    /** A user who removed their plan and restores their backup onto a new phone
     *  gets their data back as it was: without a plan, not a freshly generated one. */
    @Test
    fun aBackupOfARemovedPlanRestoresWithoutAPlan() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), Event.GeneratePlan(todayEpochDay() - 7), QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(2)), Event.ClearPlan))
        ctx.setupPrefs().edit().putBoolean(KEY_PLAN_CLEARED, true).commit()
        rule.launchApp()
        assertNull("the removed plan stays removed on relaunch", EventDispatcher.model.value!!.program)
        val backup = File(ctx.cacheDir, "backup/noplan.ndjson").apply { parentFile!!.mkdirs() }
        assertTrue(onAppScope { exportDataTo(ctx, Uri.fromFile(backup)) })

        QaHarness.teardown()
        QaHarness.reset(ctx)
        rule.launchAppExpectingSetup()
        importBackupAndRestart(backup)
        assertEquals(1, EventDispatcher.model.value!!.lifts.size)
        assertNull("restoring a plan-less backup must not invent a plan", EventDispatcher.model.value!!.program)
    }

    @Test
    fun anImportIsRefusedWhileARunRecordsAndTheLiveLogIsUntouched() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(1))))
        rule.launchApp()
        val logBefore = logLines()
        RunSession.tracking.value = true
        try {
            val lines = listOf(QaHarness.calisthenicsProfile().toJson().toString())
            assertEquals(DataImportResult.RunInProgress, onAppScope { replaceEventLog(ctx, lines) })
        } finally {
            RunSession.tracking.value = false
        }
        assertEquals(logBefore, logLines())
        // And the core still persists: a later entry lands in the same log.
        val at = AppClock.nowSec() - 60
        EventDispatcher.send(Event.LogSet("Deadlift", 140.0, 3, 8.0, at, at * 1000))
        waitFor(what = "the set to persist") { logLines().size == logBefore.size + 1 }
    }

    @Test
    fun aRunFileOrAHalfWrittenBackupIsNeverAcceptedAsADataBackup() {
        QaHarness.seedLog(ctx, richHistory())
        rule.launchApp()
        val backup = File(ctx.cacheDir, "backup/full.ndjson").apply { parentFile!!.mkdirs() }
        assertTrue(onAppScope { exportDataTo(ctx, Uri.fromFile(backup)) })
        val (pts, _) = synthRun(seed = 25, startSec = QaHarness.daysAgoAtMidday(2), n = 200)
        val gpx = File(ctx.cacheDir, "backup/run.gpx").apply { writeText(gpxDoc(listOf(pts.asFixes()))) }
        // A copy cut off mid-line (an interrupted transfer).
        val text = backup.readText()
        val cut = File(ctx.cacheDir, "backup/cut.ndjson").apply { writeText(text.substring(0, text.length - 40)) }
        val empty = File(ctx.cacheDir, "backup/empty.ndjson").apply { writeText("") }
        for (f in listOf(gpx, cut, empty)) {
            assertEquals("${f.name} is refused", ImportCheck.Invalid, onAppScope { readImportCandidate(ctx, Uri.fromFile(f)) })
        }
    }
}
