package app.milestone

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import app.milestone.core.JniCore
import app.milestone.qa.QaHarness
import app.milestone.qa.QaHarness.fixture
import java.time.Instant
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException
import kotlin.math.abs
import kotlin.math.sin
import kotlin.random.Random
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * Seeded mutation fuzzing of the run import across the JNI boundary ([parseRunFile]
 * over the REAL core's `ParseRunFile` op, then [importedRunEvent] and the core's own
 * figures) and of the core's other byte ops (`update`, `view`, `trackFigures`,
 * `exportGpx`, `runTrack`) with malformed input. The readers' and gates' own cases
 * (XML corners, entities, encodings, truncation, hostile values) are core tests
 * (`shared/src/import/tests.rs`).
 *
 * Seeds: generated GPX / TCX / FIT files, the core's own GPX export and the real
 * device FIT fixtures (`shared/tests/fixtures`). Mutations:
 * bit flips, byte insert/delete/duplicate, truncation, duplicated and nested tags,
 * entity tricks, BOMs, CRLF, giant attributes, weird coordinates and times.
 *
 * Invariants: every input finishes inside a time bound with runs or a named
 * [ImportFailure] (never another exception), every built [Event.LogRunTrack] carries
 * finite in-range fixes the core accepts with finite figures, and the same input
 * gives the same answer.
 *
 * `IMPORT_FUZZ_ITERS=<n>` sets the mutations per seed; `IMPORT_FUZZ_SEED=<n>` replays
 * one case.
 */
@RunWith(RobolectricTestRunner::class)
class ImportParserFuzzTest {

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @Before
    fun setUp() {
        QaHarness.reset(ctx)
    }

    @After
    fun tearDown() {
        QaHarness.teardown()
    }

    private val iters = System.getenv("IMPORT_FUZZ_ITERS")?.toIntOrNull() ?: 250
    private val onlyCase = System.getenv("IMPORT_FUZZ_SEED")?.toLongOrNull()

    private val worker = Executors.newSingleThreadExecutor { r -> Thread(r, "import-fuzz").apply { isDaemon = true } }

    /** Run [f] on a daemon worker; fail if it takes longer than [ms] (a hang). */
    private fun <T> bounded(what: String, ms: Long = 10_000, f: () -> T): T {
        val fut = worker.submit<T> { f() }
        return try {
            fut.get(ms, TimeUnit.MILLISECONDS)
        } catch (e: TimeoutException) {
            fut.cancel(true)
            fail("$what did not finish in $ms ms")
            throw e
        } catch (e: java.util.concurrent.ExecutionException) {
            throw e.cause ?: e
        }
    }

    private sealed interface Out {
        data class Runs(val runs: List<ImportedRun>) : Out
        data class Failed(val reason: ImportFailure, val detail: String?) : Out
    }

    private fun parse(bytes: ByteArray): Out = try {
        Out.Runs(parseRunFile(bytes, Core::parseRunFile))
    } catch (e: ImportException) {
        Out.Failed(e.reason, e.detail)
    }

    private fun assertSaneEvent(what: String, ev: Event.LogRunTrack) {
        val pts = ev.points
        assertTrue("$what: ≥ 2 points", pts.size >= 2)
        assertTrue("$what: decimated (${pts.size})", pts.size <= CoreConstants.TRACK_DECIMATION_CAP + ev.segmentStarts.size + 1)
        for (p in pts) {
            val ok = p.lat.isFinite() && p.lon.isFinite() && abs(p.lat) <= 90.0 && abs(p.lon) <= 180.0 &&
                p.observedAt > 0 && p.accuracyM.isFinite() && p.accuracyM >= 0.0
            assertTrue("$what: insane point $p", ok)
        }
        assertEquals("$what: starts sorted, unique, inside", ev.segmentStarts.sorted().distinct(), ev.segmentStarts)
        assertTrue("$what: starts in range ${ev.segmentStarts}", ev.segmentStarts.all { it in 1 until pts.size })
        assertTrue("$what: hr%", ev.hrPctMax.isFinite() && ev.hrPctMax in 0.0..100.0)
        assertTrue("$what: stamp", ev.observedAt > 0)
        Json.parseToJsonElement(ev.toJson().toString())
        val figures = Core.savedFigures(ev)
        assertNotNull("$what: the core previews the event", figures)
        assertTrue("$what: figures $figures", figures!!.distance_km.isFinite() && figures.duration_min.isFinite() &&
            figures.distance_km >= 0.0 && figures.duration_min >= 0.0)
    }

    private fun check(what: String, bytes: ByteArray): Out {
        val out = bounded(what) { runCatching { parse(bytes) } }
            .getOrElse { e -> throw AssertionError("$what: unexpected ${e::class.java.name}: ${e.message}", e) }
        if (out is Out.Runs) {
            for ((i, run) in out.runs.withIndex()) assertSaneEvent("$what run $i", importedRunEvent(run, measuredHrMax = 190.0))
        }
        assertEquals("$what: same input, same answer", out, bounded("$what (2nd)") { parse(bytes) })
        return out
    }

    private data class Fix(val lat: Double, val lon: Double, val t: Long, val hr: Int)

    private fun run(n: Int, t0: Long = 1_750_000_000L, seed: Int = 1): List<Fix> {
        val rnd = Random(seed)
        var lat = 47.37
        var lon = 8.54
        return List(n) { i ->
            lat += 3.0 * (0.9 + 0.2 * rnd.nextDouble()) / 111_195.0
            lon += sin(i * 0.05) * 1.5 / 75_000.0
            Fix(lat, lon, t0 + i, 120 + i % 40)
        }
    }

    private fun iso(t: Long) = Instant.ofEpochSecond(t).toString()

    private fun gpx(segments: List<List<Fix>>, type: String? = "running", hdop: Boolean = true): String = buildString {
        append("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n")
        append("<gpx version=\"1.1\" creator=\"fuzz\" xmlns=\"http://www.topografix.com/GPX/1/1\" ")
        append("xmlns:gpxtpx=\"http://www.garmin.com/xmlschemas/TrackPointExtension/v1\">\n")
        append("<metadata><name>m</name></metadata>\n<trk><name>t</name>")
        if (type != null) append("<type>$type</type>")
        append("\n")
        for (seg in segments) {
            append("<trkseg>\n")
            for (f in seg) {
                append("<trkpt lat=\"${f.lat}\" lon=\"${f.lon}\"><ele>400.0</ele><time>${iso(f.t)}</time>")
                if (hdop) append("<hdop>0.9</hdop>")
                append("<extensions><gpxtpx:TrackPointExtension><gpxtpx:hr>${f.hr}</gpxtpx:hr></gpxtpx:TrackPointExtension></extensions>")
                append("</trkpt>\n")
            }
            append("</trkseg>\n")
        }
        append("</trk>\n</gpx>\n")
    }

    private fun tcx(activities: List<Pair<String, List<List<Fix>>>>): String = buildString {
        append("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n")
        append("<TrainingCenterDatabase xmlns=\"http://www.garmin.com/xmlschemas/TrainingCenterDatabase/v2\">\n<Activities>\n")
        for ((sport, laps) in activities) {
            append("<Activity Sport=\"$sport\"><Id>${iso(laps.first().first().t)}</Id>\n")
            for (lap in laps) {
                append("<Lap StartTime=\"${iso(lap.first().t)}\"><TotalTimeSeconds>1</TotalTimeSeconds><Track>\n")
                for (f in lap) {
                    append("<Trackpoint><Time>${iso(f.t)}</Time><Position><LatitudeDegrees>${f.lat}</LatitudeDegrees>")
                    append("<LongitudeDegrees>${f.lon}</LongitudeDegrees></Position>")
                    append("<HeartRateBpm><Value>${f.hr}</Value></HeartRateBpm></Trackpoint>\n")
                }
                append("</Track></Lap>\n")
            }
            append("</Activity>\n")
        }
        append("</Activities>\n</TrainingCenterDatabase>\n")
    }

    /** FIT CRC-16, as the SDK computes it. */
    private fun fitCrc(data: ByteArray, len: Int = data.size): Int {
        val t = intArrayOf(0x0000, 0xCC01, 0xD801, 0x1400, 0xF001, 0x3C00, 0x2800, 0xE401, 0xA001, 0x6C00, 0x7800, 0xB401, 0x5000, 0x9C01, 0x8801, 0x4400)
        var crc = 0
        for (k in 0 until len) {
            val b = data[k].toInt() and 0xFF
            var tmp = t[crc and 0xF]
            crc = (crc shr 4) and 0x0FFF
            crc = crc xor tmp xor t[b and 0xF]
            tmp = t[crc and 0xF]
            crc = (crc shr 4) and 0x0FFF
            crc = crc xor tmp xor t[(b shr 4) and 0xF]
        }
        return crc
    }

    private fun java.io.ByteArrayOutputStream.le(v: Long, n: Int) { for (k in 0 until n) write(((v shr (8 * k)) and 0xFF).toInt()) }

    private fun semi(deg: Double): Long = Math.round(deg * 2_147_483_648.0 / 180.0)

    /** A FIT activity of [fixes]: record = timestamp, lat, lon, heart_rate; a session
     *  with [sport] (1 running, 2 cycling) at the end, as devices write it. */
    private fun fit(fixes: List<Fix>, sport: Int = 1): ByteArray {
        val d = java.io.ByteArrayOutputStream()
        d.write(0x40); d.write(0); d.write(0); d.le(20, 2); d.write(4)
        for (f in intArrayOf(253, 4, 0x86, 0, 4, 0x85, 1, 4, 0x85, 3, 1, 0x02)) d.write(f)
        for (f in fixes) {
            d.write(0)
            d.le(f.t - 631_065_600L, 4)
            d.le(semi(f.lat), 4)
            d.le(semi(f.lon), 4)
            d.write(f.hr)
        }
        d.write(0x41); d.write(0); d.write(0); d.le(18, 2); d.write(2)
        for (f in intArrayOf(5, 1, 0, 6, 1, 0)) d.write(f)
        d.write(1); d.write(sport); d.write(0)
        val body = d.toByteArray()
        val f = java.io.ByteArrayOutputStream()
        f.write(14); f.write(0x20); f.le(2132, 2); f.le(body.size.toLong(), 4); f.write(".FIT".toByteArray())
        f.le(fitCrc(f.toByteArray()).toLong(), 2)
        f.write(body)
        val all = f.toByteArray()
        f.le(fitCrc(all).toLong(), 2)
        return f.toByteArray()
    }

    /** The core's own GPX export of a logged run (what Share writes). */
    private fun coreGpx(points: List<GpsPoint>, starts: List<Int> = emptyList(), entryId: Long = 99L): String {
        val ev = Event.LogRunTrack(points, 0.0, observedAt = points.last().observedAt, entryId = entryId, segmentStarts = starts)
        assertTrue("core accepts the seed run", Core.send(ev).accepted)
        return Core.runGpx(entryId, points.last().observedAt)
    }

    private fun Fix.point() = GpsPoint(lat, lon, t, 5.0)

    private val interesting = byteArrayOf(0, 1, 0x0C, 0x0E, 0x20, 0x26, 0x3C, 0x3E, 0x40, 0x7F, 0x80.toByte(), 0xFF.toByte())

    private fun mutateBytes(rnd: Random, seed: ByteArray): ByteArray {
        val b = seed.toMutableList()
        repeat(1 + rnd.nextInt(4)) {
            val n = b.size.coerceAtLeast(1)
            when (rnd.nextInt(7)) {
                0 -> if (b.isNotEmpty()) { val i = rnd.nextInt(b.size); b[i] = (b[i].toInt() xor (1 shl rnd.nextInt(8))).toByte() }
                1 -> if (b.isNotEmpty()) b[rnd.nextInt(b.size)] = interesting[rnd.nextInt(interesting.size)]
                2 -> { val i = rnd.nextInt(b.size + 1); b.addAll(i, List(1 + rnd.nextInt(8)) { rnd.nextInt(256).toByte() }) }
                3 -> if (b.isNotEmpty()) { val i = rnd.nextInt(b.size); repeat(minOf(1 + rnd.nextInt(16), b.size - i)) { b.removeAt(i) } }
                4 -> if (b.isNotEmpty()) { val i = rnd.nextInt(b.size); val c = b.subList(i, minOf(b.size, i + 1 + rnd.nextInt(128))).toList(); b.addAll(rnd.nextInt(b.size + 1), c) }
                5 -> { val cut = rnd.nextInt(n + 1).coerceAtMost(b.size); while (b.size > cut) b.removeAt(b.size - 1) }
                else -> if (b.size > 1) { val i = rnd.nextInt(b.size); val j = rnd.nextInt(b.size); val x = b[i]; b[i] = b[j]; b[j] = x }
            }
        }
        return b.toByteArray()
    }

    private val weirdNumbers = listOf("NaN", "Infinity", "-Infinity", "1e308", "-1e308", "1e400", "91.5", "-180.0001", "0x1p3", "4,7", "", " 47.3 ", "-0", "4".repeat(5000))
    private val weirdTimes = listOf("", "garbage", "9999-12-31T23:59:59Z", "+10000-01-01T00:00:00Z", "1970-01-01T00:00:00Z", "1969-12-31T23:59:59Z",
        "2025-06-15T12:00:00+14:00", "2025-06-15T12:00:00.999999999Z", "2025-06-15T12:00:00", "2025-02-30T00:00:00Z", "-999999999-01-01T00:00:00Z")

    private fun mutateXml(rnd: Random, doc: String): String {
        var s = doc
        repeat(1 + rnd.nextInt(3)) {
            s = when (rnd.nextInt(12)) {
                0 -> Regex("lat=\"[^\"]*\"").replace(s, { m -> if (rnd.nextInt(8) == 0) "lat=\"${weirdNumbers.random(rnd)}\"" else m.value })
                1 -> Regex("lon=\"[^\"]*\"").replace(s, { m -> if (rnd.nextInt(8) == 0) "lon=\"${weirdNumbers.random(rnd)}\"" else m.value })
                2 -> Regex("<(time|Time)>[^<]*</").replace(s, { m -> if (rnd.nextInt(6) == 0) "<${m.groupValues[1]}>${weirdTimes.random(rnd)}</" else m.value })
                3 -> {
                    val tag = listOf("trkpt", "trkseg", "trk", "Trackpoint", "Track", "Lap", "Activity", "time", "Time").random(rnd)
                    val i = s.indexOf("<$tag", rnd.nextInt(s.length.coerceAtLeast(1)))
                    val j = if (i >= 0) s.indexOf("</$tag>", i) else -1
                    if (i >= 0 && j > i) s.substring(0, j) + "</$tag>" + s.substring(i, j + tag.length + 3) + s.substring(j + tag.length + 3) else s
                }
                4 -> {
                    val tag = listOf("trkseg", "trk", "gpx", "Track", "Activity", "Lap").random(rnd)
                    val i = s.indexOf("<$tag")
                    if (i >= 0) s.substring(0, i) + "<$tag>" + s.substring(i).replaceFirst("</$tag>", "</$tag></$tag>") else s
                }
                5 -> s.replace("\n", "\r\n")
                6 -> "\uFEFF" + s
                7 -> { val i = rnd.nextInt(s.length + 1); s.substring(0, i) + listOf("&amp;", "&#x41;", "&#0;", "&#xD800;", "&undefined;", "<![CDATA[<x>]]>", "<!-- c -->", "<?pi x?>", "\u0000", "&").random(rnd) + s.substring(i) }
                8 -> s.replaceFirst("<trkpt ", "<trkpt " + "x=\"" + "y".repeat(200_000) + "\" ").replaceFirst("<Trackpoint>", "<Trackpoint data=\"" + "z".repeat(200_000) + "\">")
                9 -> {
                    val lines = s.split("\n").toMutableList()
                    if (lines.size > 3) { val a = 1 + rnd.nextInt(lines.size - 2); val b = 1 + rnd.nextInt(lines.size - 2); val x = lines[a]; lines[a] = lines[b]; lines[b] = x }
                    lines.joinToString("\n")
                }
                10 -> s.replace(Regex("<(time|Time)>"), { m -> if (rnd.nextInt(20) == 0) m.value + "<b>x</b>" else m.value })
                else -> { val cut = rnd.nextInt(s.length + 1); s.substring(0, cut) }
            }
        }
        return s
    }

    private fun fuzz(name: String, seed: ByteArray, base: Long, xml: Boolean) {
        var runsOk = 0
        for (i in 0 until iters) {
            val case = base + i
            if (onlyCase != null && onlyCase != case) continue
            val rnd = Random(case)
            val bytes = if (xml && rnd.nextBoolean()) mutateXml(rnd, String(seed)).toByteArray() else mutateBytes(rnd, seed)
            val out = try {
                check("$name case $case (IMPORT_FUZZ_SEED=$case)", bytes)
            } catch (e: AssertionError) {
                throw AssertionError("${e.message}\n  input head: ${String(bytes.copyOf(minOf(bytes.size, 300)))}", e)
            }
            if (out is Out.Runs) runsOk++
        }
        if (onlyCase == null) assertTrue("$name: some mutants still import ($runsOk)", runsOk > 0)
    }

    @Test
    fun gpxMutantsAlwaysEndInARunOrANamedReason() {
        val r = run(400)
        fuzz("gpx", gpx(listOf(r.take(200), r.drop(200).map { it.copy(t = it.t + 120) })).toByteArray(), 0x6000_0000L, xml = true)
    }

    @Test
    fun coreExportedGpxMutantsAlwaysEndInARunOrANamedReason() {
        val doc = coreGpx(run(300).map { it.point() }, starts = listOf(150))
        assertTrue("the core exports the seed", doc.contains("<trkseg>"))
        fuzz("core gpx", doc.toByteArray(), 0x6100_0000L, xml = true)
    }

    @Test
    fun tcxMutantsAlwaysEndInARunOrANamedReason() {
        val a = run(300, seed = 2)
        val b = run(200, t0 = 1_750_100_000L, seed = 3)
        fuzz("tcx", tcx(listOf("Running" to listOf(a.take(150), a.drop(150)), "Running" to listOf(b))).toByteArray(), 0x6200_0000L, xml = true)
    }

    @Test
    fun fitMutantsAlwaysEndInARunOrANamedReason() {
        // The device fixtures are a 13 s fragment and a ride: only the generated run imports.
        fuzz("gen fit", fit(run(400)), 0x6300_0000L, xml = false)
        for ((si, name) in listOf("Activity.fit", "garmin-fenix-5-bike.fit").withIndex()) {
            val seed = fixture(name)
            for (i in 0 until iters) {
                val case = 0x6310_0000L + si * 0x10_0000L + i
                if (onlyCase != null && onlyCase != case) continue
                val rnd = Random(case)
                val bytes = mutateBytes(rnd, seed)
                // Keep the FIT magic half the time so the FIT reader, not the sniff, judges it.
                if (rnd.nextBoolean() && bytes.size >= 12) ".FIT".toByteArray().copyInto(bytes, 8)
                check("fit $name case $case (IMPORT_FUZZ_SEED=$case)", bytes)
            }
        }
    }

    @Test
    fun theSameRunAsFitGpxAndTcxGivesTheSameSavedFigures() {
        val pts = run(900, seed = 7)
        val events = mapOf(
            "fit" to fit(pts),
            "gpx" to gpx(listOf(pts), hdop = false).toByteArray(),
            "tcx" to tcx(listOf("Running" to listOf(pts))).toByteArray(),
        ).mapValues { (k, bytes) -> importedRunEvent((check(k, bytes) as Out.Runs).runs.single(), measuredHrMax = 190.0) }
        val figs = events.mapValues { (k, ev) -> checkNotNull(Core.savedFigures(ev)) { k } }
        val ref = figs.getValue("gpx")
        for ((k, f) in figs) {
            assertEquals("$k moving time", ref.duration_min, f.duration_min, 1e-9)
            assertEquals("$k distance", ref.distance_km, f.distance_km, ref.distance_km * 1e-4)
            assertEquals("$k hr%", events.getValue("gpx").hrPctMax, events.getValue(k).hrPctMax, 1e-9)
            assertEquals("$k stamp", events.getValue("gpx").observedAt, events.getValue(k).observedAt)
        }
    }

    @Test
    fun aFitRefusalReachesTheShellWithItsReason() {
        val pts = run(900, seed = 8)
        val cases = mapOf(
            fit(pts, sport = 2) to Out.Failed(ImportFailure.NotARun, "cycling"),
            fit(pts.take(100)) to Out.Failed(ImportFailure.TooShort, null),
            fit(pts.take(1)) to Out.Failed(ImportFailure.NoTrack, null),
            fit(pts).copyOf(30) to Out.Failed(ImportFailure.Damaged, null),
            byteArrayOf(14, 0x20, 0, 0, 0, 0, 0, 0, '.'.code.toByte(), 'F'.code.toByte(), 'I'.code.toByte(), 'T'.code.toByte(), 0, 0) to
                Out.Failed(ImportFailure.NoTrack, null),
        )
        for ((bytes, want) in cases) assertEquals(want, check("fit refusal", bytes))
    }

    @Test
    fun aTrackSentToTheCoreComesBackAsItsRouteBitForBit() {
        val pts = run(400, seed = 10).mapIndexed { i, f -> f.point().copy(accuracyM = 3.0 + (i % 7)) }
        val ev = Event.LogRunTrack(pts, 0.0, observedAt = pts.last().observedAt, entryId = 77L, segmentStarts = listOf(100, 250))
        assertTrue(Core.send(ev).accepted)
        val segs = Core.runTrack(77L, pts.last().observedAt)
        assertEquals(listOf(100, 150, 150), segs.map { it.size })
        assertEquals(pts, segs.flatten())
    }

    @Test
    fun aFarFutureRunTheImporterAcceptedReimportsFromItsOwnExport() {
        val pts = run(300).map { it.copy(t = it.t + 251_652_300_800L) } // year 10000
        val doc = gpx(listOf(pts))
        assertTrue("java.time names the year with its sign", doc.contains("<time>+10000-"))
        val ev = importedRunEvent((check("year 10000", doc.toByteArray()) as Out.Runs).runs.single(), null)
        assertTrue("the core accepts the run", Core.send(ev).accepted)
        val export = Core.runGpx(ev.entryId, ev.observedAt)
        assertTrue("exported", export.contains("<trkpt"))
        val back = importedRunEvent((check("its export", export.toByteArray()) as Out.Runs).runs.single(), null)
        assertEquals(ev.points.map { it.observedAt }, back.points.map { it.observedAt })
        assertEquals(ev.observedAt, back.observedAt)
    }

    private fun malformed(rnd: Random, seeds: List<String>): ByteArray = when (rnd.nextInt(6)) {
        0 -> ByteArray(rnd.nextInt(64)) { rnd.nextInt(256).toByte() }
        1 -> "[".repeat(50_000 + rnd.nextInt(10)).toByteArray()
        2 -> byteArrayOf(0xC3.toByte(), 0x28, 0xFF.toByte(), 0xFE.toByte())
        else -> mutateBytes(rnd, seeds[rnd.nextInt(seeds.size)].toByteArray())
    }

    private val eventSeeds by lazy {
        listOf(
            Event.SetToday(20_300L, 3600).toJson().toString(),
            Event.LogRunTrack(run(60).map { it.point() }, 0.0, observedAt = 1_750_000_060L, entryId = 5L).toJson().toString(),
            Event.ClearRuns.toJson().toString(),
            QaHarness.loggedRun(1_750_000_000L).toJson().toString(),
        )
    }

    @Test
    fun malformedUpdateBytesNeverCrashAndLeaveTheCoreUsable() {
        assertTrue(Core.send(QaHarness.loggedRun(1_749_000_000L)).accepted)
        var rejected = 0
        for (case in 0 until iters * 4) {
            val bytes = malformed(Random(0x6400_0000L + case), eventSeeds)
            val out = bounded("update $case") { JniCore.update(bytes) }
            assertNotNull("update $case returned null", out)
            val text = String(out!!, Charsets.UTF_8).trim()
            assertTrue("update $case: neither effects, empty nor error: ${text.take(200)}",
                text.isEmpty() || text.startsWith("[") || isCoreErrorEnvelope(text))
            assertTrue("update $case: a panic reset the core: $text", !isCorePanicReset(text))
            if (text.isEmpty()) rejected++
            val v = JniCore.view()
            assertNotNull("view after $case", v)
            assertTrue("view after $case is a view", Json.parseToJsonElement(String(v!!, Charsets.UTF_8)) is JsonObject)
        }
        assertTrue(rejected > 0)
        assertTrue(Core.send(Event.SetToday(20_300L, 0)).accepted)
    }

    @Test
    fun malformedTrackFiguresAndRunIdsAnswerWithEnvelopesOrEmpty() {
        for (case in 0 until iters * 2) {
            val bytes = malformed(Random(0x6500_0000L + case), eventSeeds)
            val text = bounded("trackFigures $case") { JniCore.trackFigures(bytes)?.decodeToString() }
            assertNotNull(text)
            if (!isCoreErrorEnvelope(text!!)) {
                val o = Json.parseToJsonElement(text).jsonObject
                val km = o["distance_km"]?.jsonPrimitive?.doubleOrNull
                val min = o["duration_min"]?.jsonPrimitive?.doubleOrNull
                assertTrue("trackFigures $case: $text", km != null && min != null && km.isFinite() && min.isFinite() && km >= 0 && min >= 0)
            }
        }
        assertTrue(Core.send(Event.LogRunTrack(run(300).map { it.point() }, 0.0, observedAt = 1_750_000_300L, entryId = 42L)).accepted)
        for (id in listOf(0L, -1L, Long.MIN_VALUE, Long.MAX_VALUE, 42L)) for (at in listOf(0L, -1L, Long.MIN_VALUE, Long.MAX_VALUE, 1_750_000_300L)) {
            val known = id == 42L && at == 1_750_000_300L
            val gpx = Core.runGpx(id, at)
            if (known) assertTrue(gpx.contains("<trkpt")) else assertEquals("exportGpx($id, $at)", "", gpx)
            assertEquals("runTrack($id, $at)", if (known) 300 else 0, Core.runTrack(id, at).sumOf { it.size })
        }
    }

    @Test
    fun parseRunFileOfArbitraryBytesIsAlwaysAVerdictTheShellMaps() {
        for (case in 0 until iters * 2) {
            val rnd = Random(0x6600_0000L + case)
            val bytes = ByteArray(12 + rnd.nextInt(600)) { rnd.nextInt(256).toByte() }
            ".FIT".toByteArray().copyInto(bytes, 8)
            bytes[0] = if (rnd.nextBoolean()) 14 else 12
            val verdict = bounded("parseRunFile $case") { Core.parseRunFile(bytes) }
            assertNotNull("parseRunFile $case: the core answers a verdict", verdict)
            assertTrue("parseRunFile $case: $verdict", verdict!!.runs.isEmpty() != (verdict.refusal == null))
            check("parseRunFile $case", bytes)
        }
    }
}
