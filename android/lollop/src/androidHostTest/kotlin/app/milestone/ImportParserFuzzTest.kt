package app.milestone

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import app.milestone.qa.QaHarness
import app.milestone.qa.QaHarness.fixture
import java.io.File
import java.time.Instant
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException
import kotlin.math.abs
import kotlin.math.sin
import kotlin.random.Random
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.contentOrNull
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
 * Seeded mutation fuzzing of the shell's run-import parsers ([parseRunFileSegments] →
 * [parseGpxRuns] / [parseTcxActivities] / [fitSegments] over the REAL core's
 * [Core.parseFit], then [importedRunEvent]) and of the JNI byte entry points
 * (`update`, `view`, `parseFit`, `trackFigures`, `exportGpx`) with malformed input.
 *
 * Seeds: generated GPX / TCX / FIT files, the core's own GPX export and the real
 * device FIT fixtures (`shared/tests/fixtures`, on the test classpath). Mutations:
 * bit flips, byte insert/delete/duplicate, truncation at sampled offsets, duplicated
 * and nested tags, entity tricks, BOMs, encodings, CRLF, giant attributes,
 * NaN/Infinity/1e308 coordinates, out-of-order/duplicated/future timestamps.
 *
 * Invariants: every input finishes inside a time bound with a run or a named
 * [ImportFailure] (never another exception), every built [Event.LogRunTrack] carries
 * finite in-range fixes the core accepts with finite figures, the same input gives the
 * same answer, external entities never resolve, and the Kotlin and Rust parsers agree
 * on the same track.
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

    // ── Time-bounded execution ─────────────────────────────────────────────────

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

    // ── Outcomes ───────────────────────────────────────────────────────────────

    private sealed interface Out {
        data class Runs(val runs: List<ImportedRun>) : Out
        data class Failed(val reason: ImportFailure, val detail: String?) : Out
    }

    /** What the import pipeline makes of one file; any exception that is not a named
     *  [ImportException] is a shell bug (it surfaces as [ImportFailure.Unknown]). */
    private fun parse(bytes: ByteArray): Out = try {
        Out.Runs(parseRunFileSegments(bytes, Core::parseFit))
    } catch (e: ImportException) {
        Out.Failed(e.reason, e.detail)
    }

    /** One run → the event, or the named reason it was refused. */
    private fun build(run: ImportedRun): Any = try {
        importedRunEvent(run, measuredHrMax = 190.0).copy(entryId = 1L)
    } catch (e: ImportException) {
        Out.Failed(e.reason, e.detail)
    }

    private fun assertSaneEvent(what: String, ev: Event.LogRunTrack) {
        val pts = ev.points
        assertTrue("$what: ≥ 2 points", pts.size >= 2)
        assertTrue("$what: decimated (${pts.size})", pts.size <= TRACK_DECIMATION_CAP + ev.segmentStarts.size + 1)
        for (p in pts) {
            val ok = p.lat.isFinite() && p.lon.isFinite() && abs(p.lat) <= 90.0 && abs(p.lon) <= 180.0 &&
                p.observedAt > 0 && p.accuracyM.isFinite() && p.accuracyM >= 0.0
            assertTrue("$what: insane point $p", ok)
        }
        assertEquals("$what: starts sorted, unique, inside", ev.segmentStarts.sorted().distinct(), ev.segmentStarts)
        assertTrue("$what: starts in range ${ev.segmentStarts}", ev.segmentStarts.all { it in 1 until pts.size })
        assertTrue("$what: hr%", ev.hrPctMax.isFinite() && ev.hrPctMax in 0.0..100.0)
        assertTrue("$what: stamp", ev.observedAt > 0)
        // The exact wire the shell sends must be JSON the core accepts.
        val wire = ev.toJson().toString()
        Json.parseToJsonElement(wire)
        val figures = Core.savedFigures(ev)
        assertNotNull("$what: the core previews the event", figures)
        assertTrue("$what: figures $figures", figures!!.distance_km.isFinite() && figures.duration_min.isFinite() &&
            figures.distance_km >= 0.0 && figures.duration_min >= 0.0)
    }

    /** Check one input end to end; returns its outcome for further assertions. */
    private fun check(what: String, bytes: ByteArray): Out {
        val first = bounded(what) { runCatching { parse(bytes) } }
        val out = first.getOrElse { e -> throw AssertionError("$what: unexpected ${e::class.java.name}: ${e.message}", e) }
        if (out is Out.Runs) {
            for ((i, run) in out.runs.withIndex()) {
                for (seg in run.segments) for (f in seg) {
                    assertTrue("$what: non-finite fix $f", f.lat.isFinite() && f.lon.isFinite())
                }
                when (val b = runCatching { build(run) }.getOrElse { e -> throw AssertionError("$what run $i: ${e::class.java.name}: ${e.message}", e) }) {
                    is Event.LogRunTrack -> assertSaneEvent("$what run $i", b)
                    is Out.Failed -> {}
                }
            }
        }
        val again = bounded("$what (2nd)") { runCatching { parse(bytes) } }.getOrThrow()
        assertEquals("$what: same input, same answer", out, again)
        return out
    }

    // ── Seed files ─────────────────────────────────────────────────────────────

    private data class Fix(val lat: Double, val lon: Double, val t: Long, val hr: Int)

    /** A seeded wandering 1 Hz run at ~3 m/s. */
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

    /** The core's own GPX export of a logged run (what Share / Undo read). */
    private fun coreGpx(points: List<GpsPoint>, starts: List<Int> = emptyList(), entryId: Long = 99L): String {
        val ev = Event.LogRunTrack(points, 0.0, observedAt = points.last().observedAt, entryId = entryId, segmentStarts = starts)
        assertTrue("core accepts the seed run", Core.send(ev).accepted)
        return Core.exportGpx(entryId, points.last().observedAt).orEmpty()
    }

    private fun Fix.point() = GpsPoint(lat, lon, t, 5.0)

    // ── Mutators ───────────────────────────────────────────────────────────────

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

    /** XML-structure mutations on the document text. */
    private fun mutateXml(rnd: Random, doc: String): String {
        var s = doc
        repeat(1 + rnd.nextInt(3)) {
            s = when (rnd.nextInt(12)) {
                0 -> Regex("lat=\"[^\"]*\"").replace(s, { m -> if (rnd.nextInt(8) == 0) "lat=\"${weirdNumbers.random(rnd)}\"" else m.value })
                1 -> Regex("lon=\"[^\"]*\"").replace(s, { m -> if (rnd.nextInt(8) == 0) "lon=\"${weirdNumbers.random(rnd)}\"" else m.value })
                2 -> Regex("<(time|Time)>[^<]*</").replace(s, { m -> if (rnd.nextInt(6) == 0) "<${m.groupValues[1]}>${weirdTimes.random(rnd)}</" else m.value })
                3 -> { // duplicate a random element span
                    val tags = listOf("trkpt", "trkseg", "trk", "Trackpoint", "Track", "Lap", "Activity", "time", "Time")
                    val tag = tags.random(rnd)
                    val i = s.indexOf("<$tag", rnd.nextInt(s.length.coerceAtLeast(1)))
                    val j = if (i >= 0) s.indexOf("</$tag>", i) else -1
                    if (i >= 0 && j > i) s.substring(0, j) + "</$tag>" + s.substring(i, j + tag.length + 3) + s.substring(j + tag.length + 3) else s
                }
                4 -> { // nest: open an extra element of the same kind, closed at the end
                    val tag = listOf("trkseg", "trk", "gpx", "Track", "Activity", "Lap").random(rnd)
                    val i = s.indexOf("<$tag")
                    if (i >= 0) s.substring(0, i) + "<$tag>" + s.substring(i).replaceFirst("</$tag>", "</$tag></$tag>") else s
                }
                5 -> s.replace("\n", "\r\n")
                6 -> "\uFEFF" + s
                7 -> { val i = rnd.nextInt(s.length + 1); s.substring(0, i) + listOf("&amp;", "&#x41;", "&#0;", "&#xD800;", "&undefined;", "<![CDATA[<x>]]>", "<!-- c -->", "<?pi x?>", "\u0000", "&").random(rnd) + s.substring(i) }
                8 -> s.replaceFirst("<trkpt ", "<trkpt " + "x=\"" + "y".repeat(200_000) + "\" ").replaceFirst("<Trackpoint>", "<Trackpoint data=\"" + "z".repeat(200_000) + "\">")
                9 -> { // swap two lines (out-of-order fixes)
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

    private fun hex(b: ByteArray) = b.take(40).joinToString("") { "%02x".format(it) }

    private fun die(msg: String): Nothing = throw AssertionError(msg)

    private fun fuzzXml(name: String, seed: String, base: Long) {
        var runsOk = 0
        for (i in 0 until iters) {
            val case = base + i
            if (onlyCase != null && onlyCase != case) continue
            val rnd = Random(case)
            val bytes = if (rnd.nextBoolean()) mutateXml(rnd, seed).toByteArray() else mutateBytes(rnd, seed.toByteArray())
            val out = try {
                check("$name case $case (IMPORT_FUZZ_SEED=$case)", bytes)
            } catch (e: AssertionError) {
                throw AssertionError("${e.message}\n  input head: ${String(bytes.copyOf(minOf(bytes.size, 300)))}", e)
            }
            if (out is Out.Runs && out.runs.isNotEmpty()) runsOk++
        }
        if (onlyCase == null) assertTrue("$name: some mutants still import ($runsOk)", runsOk > 0)
    }

    // ═══ XML fuzzing ═══════════════════════════════════════════════════════════

    @Test
    fun gpxMutantsAlwaysEndInARunOrANamedReason() {
        val r = run(400)
        fuzzXml("gpx", gpx(listOf(r.take(200), r.drop(200).map { it.copy(t = it.t + 120) })), 0x6000_0000L)
    }

    @Test
    fun coreExportedGpxMutantsAlwaysEndInARunOrANamedReason() {
        val doc = coreGpx(run(300).map { it.point() }, starts = listOf(150))
        assertTrue("the core exports the seed", doc.contains("<trkseg>"))
        fuzzXml("core gpx", doc, 0x6100_0000L)
    }

    @Test
    fun tcxMutantsAlwaysEndInARunOrANamedReason() {
        val a = run(300, seed = 2)
        val b = run(200, t0 = 1_750_100_000L, seed = 3)
        fuzzXml("tcx", tcx(listOf("Running" to listOf(a.take(150), a.drop(150)), "Running" to listOf(b))), 0x6200_0000L)
    }

    @Test
    fun fitMutantsThroughTheRealCoreAlwaysEndInARunOrANamedReason() {
        val seeds = listOf("Activity.fit" to fixture("Activity.fit"), "fenix bike" to fixture("garmin-fenix-5-bike.fit"), "gen" to fit(run(400)))
        var tracks = 0
        for ((si, pair) in seeds.withIndex()) {
            val (name, seed) = pair
            for (i in 0 until iters) {
                val case = 0x6300_0000L + si * 0x10_0000L + i
                if (onlyCase != null && onlyCase != case) continue
                val rnd = Random(case)
                val bytes = mutateBytes(rnd, seed)
                // Keep the FIT magic half the time so the core, not the sniff, judges it.
                if (rnd.nextBoolean() && bytes.size >= 12) ".FIT".toByteArray().copyInto(bytes, 8)
                val json = bounded("parseFit $name $case") { Core.parseFit(bytes) }
                assertNotNull("parseFit $name $case returned null", json)
                val root = Json.parseToJsonElement(json!!).jsonObject
                assertTrue("$name $case: $json", root.keys == setOf("segments") || root.keys == setOf("error"))
                val out = try {
                    check("fit $name case $case (IMPORT_FUZZ_SEED=$case) ${hex(bytes)}", bytes)
                } catch (e: AssertionError) {
                    throw AssertionError("${e.message}\n  parseFit: ${json.take(300)}", e)
                }
                if (out is Out.Runs) tracks++
            }
        }
        if (onlyCase == null) assertTrue("some FIT mutants still import ($tracks)", tracks > 0)
    }

    // ═══ Truncation ════════════════════════════════════════════════════════════

    @Test
    fun aGpxCutAtAnyOffsetIsDamagedOrTheWrongTypeNeverAShorterRun() {
        val doc = gpx(listOf(run(60), run(60, t0 = 1_750_000_200L))).toByteArray()
        for (cut in truncationCuts(doc)) {
            val out = check("gpx cut at $cut", doc.copyOf(cut))
            assertTrue("gpx cut at $cut: $out", out is Out.Failed && out.reason in setOf(ImportFailure.Damaged, ImportFailure.WrongType))
        }
    }

    @Test
    fun aTcxCutAtAnyOffsetIsDamagedOrTheWrongTypeNeverAShorterRun() {
        val doc = tcx(listOf("Running" to listOf(run(150, seed = 4)), "Running" to listOf(run(150, t0 = 1_750_100_000L, seed = 5)))).toByteArray()
        val afterFirst = String(doc).indexOf("</Activity>") + "</Activity>".length
        val bad = mutableListOf<String>()
        for (cut in (truncationCuts(doc) + afterFirst).distinct()) {
            val out = check("tcx cut at $cut", doc.copyOf(cut))
            val ok = out is Out.Failed && out.reason in setOf(ImportFailure.Damaged, ImportFailure.WrongType)
            if (!ok) bad += "cut $cut/${doc.size}: " + if (out is Out.Runs) "${out.runs.size} run(s)" else out.toString()
        }
        assertTrue("${bad.size} cuts were not reported damaged, e.g. ${bad.filter { "1 run" in it }.take(2) + bad.take(2)}", bad.isEmpty())
    }

    // ═══ Entities: XXE and billion laughs ══════════════════════════════════════

    private val secretLat = "47.123456"

    private fun Out.mentions(secret: String): Boolean = this is Out.Runs && runs.any { r ->
        r.sport?.contains(secret) == true || r.segments.flatten().any { it.lat == secret.toDouble() || it.lon == secret.toDouble() }
    }

    @Test
    fun externalEntitiesNeverResolve() {
        val secret = File(ctx.cacheDir, "xxe-secret.txt").apply { writeText(secretLat) }
        val dtd = File(ctx.cacheDir, "xxe.dtd").apply { writeText("<!ENTITY s \"$secretLat\">") }
        val pts = run(300)
        fun body(type: String, firstLat: String) = buildString {
            append("<gpx version=\"1.1\" xmlns=\"http://www.topografix.com/GPX/1/1\"><trk><type>$type</type><trkseg>")
            for ((i, f) in pts.withIndex()) {
                append("<trkpt lat=\"${if (i == 0) firstLat else f.lat.toString()}\" lon=\"${f.lon}\"><time>${iso(f.t)}</time></trkpt>")
            }
            append("</trkseg></trk></gpx>")
        }
        val uri = secret.toURI().toString()
        val docs = mapOf(
            "SYSTEM entity in text" to "<?xml version=\"1.0\"?><!DOCTYPE gpx [<!ENTITY x SYSTEM \"$uri\">]>" + body("&x;", "${pts[0].lat}"),
            "SYSTEM entity in attribute" to "<?xml version=\"1.0\"?><!DOCTYPE gpx [<!ENTITY x SYSTEM \"$uri\">]>" + body("running", "&x;"),
            "PUBLIC entity" to "<?xml version=\"1.0\"?><!DOCTYPE gpx [<!ENTITY x PUBLIC \"-//x\" \"$uri\">]>" + body("&x;", "${pts[0].lat}"),
            "external DTD" to "<?xml version=\"1.0\"?><!DOCTYPE gpx SYSTEM \"${dtd.toURI()}\">" + body("&s;", "${pts[0].lat}"),
            "parameter entity" to "<?xml version=\"1.0\"?><!DOCTYPE gpx [<!ENTITY % p SYSTEM \"${dtd.toURI()}\"> %p;]>" + body("&s;", "&s;"),
            "network entity" to "<?xml version=\"1.0\"?><!DOCTYPE gpx [<!ENTITY x SYSTEM \"http://127.0.0.1:9/x\">]>" + body("&x;", "${pts[0].lat}"),
            "TCX SYSTEM entity" to "<?xml version=\"1.0\"?><!DOCTYPE TrainingCenterDatabase [<!ENTITY x SYSTEM \"$uri\">]>" +
                tcx(listOf("Running" to listOf(pts))).substringAfter("?>").replaceFirst(Regex("<LatitudeDegrees>[^<]*<"), "<LatitudeDegrees>&x;<"),
        )
        for ((what, doc) in docs) {
            val out = check(what, doc.toByteArray())
            assertTrue("$what resolved the external entity: $out", !out.mentions(secretLat))
        }
    }

    /** `levels` nested internal entities, each 10 references to the one below:
     *  10^levels characters if expanded. */
    private fun laughs(levels: Int, where: String): String = buildString {
        append("<?xml version=\"1.0\"?>\n<!DOCTYPE gpx [\n<!ENTITY l0 \"lol\">\n")
        for (k in 1..levels) append("<!ENTITY l$k \"${"&l${k - 1};".repeat(10)}\">\n")
        append("]>\n")
        val pts = run(300)
        append("<gpx version=\"1.1\"><trk><type>${if (where == "type") "&l$levels;" else "running"}</type><trkseg>")
        for (f in pts) append("<trkpt lat=\"${f.lat}\" lon=\"${f.lon}\"><time>${iso(f.t)}</time></trkpt>")
        append("</trkseg></trk></gpx>")
    }

    @Test
    fun nestedEntityExpansionIsBounded() {
        // 6 levels = 3 MB of "lol": harmless to run, enough to see whether the parser
        // expands without a cap. A cap (or refusing DTDs) keeps amplification small.
        val doc = laughs(6, "type")
        val out = check("billion laughs ×6", doc.toByteArray())
        val sportLen = (out as? Out.Runs)?.runs?.firstOrNull()?.sport?.length ?: 0
        assertTrue("a ${doc.length}-byte file expanded to a $sportLen-char sport label", sportLen <= 100 * doc.length)
    }

    // ═══ Encodings, BOMs, line endings ═════════════════════════════════════════

    private fun importedEvent(bytes: ByteArray): Event.LogRunTrack {
        val out = check("import", bytes)
        val runs = (out as? Out.Runs)?.runs ?: die("not imported: $out")
        assertEquals(1, runs.size)
        return build(runs[0]) as? Event.LogRunTrack ?: die("refused: ${build(runs[0])}")
    }

    @Test
    fun bomCrlfAndLatin1DeclaredFilesImportTheSameRun() {
        val base = gpx(listOf(run(300)))
        val ref = importedEvent(base.toByteArray())
        val variants = mapOf(
            "UTF-8 BOM" to byteArrayOf(0xEF.toByte(), 0xBB.toByte(), 0xBF.toByte()) + base.toByteArray(),
            "CRLF" to base.replace("\n", "\r\n").toByteArray(),
            "ISO-8859-1 declared, Latin-1 name" to base.replace("encoding=\"UTF-8\"", "encoding=\"ISO-8859-1\"")
                .replace("<name>t</name>", "<name>Lauf am S\u00e9e</name>").toByteArray(Charsets.ISO_8859_1),
            "no XML declaration" to base.substringAfter("?>\n").toByteArray(),
        )
        for ((what, bytes) in variants) {
            assertEquals(what, ref, importedEvent(bytes))
        }
    }

    @Test
    fun aUtf16GpxImportsTheSameRun() {
        val base = gpx(listOf(run(300)))
        val ref = importedEvent(base.toByteArray())
        val utf16 = base.replace("encoding=\"UTF-8\"", "encoding=\"UTF-16\"")
        assertEquals(ref, importedEvent(byteArrayOf(0xFF.toByte(), 0xFE.toByte()) + utf16.toByteArray(Charsets.UTF_16LE)))
    }

    // ═══ Hostile values ════════════════════════════════════════════════════════

    @Test
    fun nonFiniteHugeAndOutOfRangeCoordinatesAreDroppedAtTheSource() {
        val pts = run(300)
        val invalid = listOf("NaN", "Infinity", "-Infinity", "1e308", "-1e308", "1e400", "-180.0001", "4,7", "", "4".repeat(5000))
        for (bad in invalid + "91.5") {
            // One bad latitude (fix 10) and, when it is no valid longitude either, one
            // bad longitude (fix 20): each dropped at the source, the run still imports.
            var doc = gpx(listOf(pts)).replaceFirst("lat=\"${pts[10].lat}\"", "lat=\"$bad\"")
            if (bad in invalid) doc = doc.replaceFirst("lon=\"${pts[20].lon}\"", "lon=\"$bad\"")
            val ev = importedEvent(doc.toByteArray())
            assertEquals(bad.take(20), if (bad in invalid) 298 else 299, ev.points.size)
            assertTrue("$bad never reaches the core", ev.points.none { abs(it.lat) > 90.0 || abs(it.lon) > 180.0 })
        }
    }

    @Test
    fun aSingleNullIslandFixDoesNotRefuseTheWholeRun() {
        val pts = run(1800, seed = 11) // 30 min, ~5.4 km
        val clean = importedEvent(gpx(listOf(pts)).toByteArray())
        val cleanKm = checkNotNull(Core.savedFigures(clean)).distance_km
        val glitch = gpx(listOf(pts)).replaceFirst("lat=\"${pts[900].lat}\" lon=\"${pts[900].lon}\"", "lat=\"0.0\" lon=\"0.0\"")
        // The core alone handles the raw track: its QC drops the 5 000 km jump.
        val raw = clean.copy(points = clean.points.mapIndexed { i, p -> if (i == 900) p.copy(lat = 0.0, lon = 0.0) else p })
        assertEquals("core figures with the glitch", cleanKm, checkNotNull(Core.savedFigures(raw)).distance_km, cleanKm * 0.01)
        val out = check("null island", glitch.toByteArray()) as Out.Runs
        val built = build(out.runs.single())
        assertTrue("the run imports: $built", built is Event.LogRunTrack)
    }

    @Test
    fun outOfOrderDuplicatedAndFutureTimestampsStillGiveSaneFigures() {
        val pts = run(400)
        val shuffled = pts.toMutableList().apply {
            this[50] = this[49]
            this[51] = this[51].copy(t = this[10].t)
            this[300] = this[300].copy(t = 4_102_444_800L) // 2100-01-01
            add(0, this[5])
        }
        for ((what, doc) in listOf(
            "shuffled" to gpx(listOf(shuffled)),
            "all in 2100" to gpx(listOf(pts.map { it.copy(t = it.t + 2_350_000_000L) })),
            "tcx shuffled" to tcx(listOf("Running" to listOf(shuffled))),
        )) {
            check(what, doc.toByteArray())
        }
    }

    @Test
    fun deeplyNestedAndGiantElementsFinishQuickly() {
        val pts = run(200)
        val deep = gpx(listOf(pts)).replaceFirst("<extensions>", "<extensions>" + "<x>".repeat(10_000) + "</x>".repeat(10_000))
        val giantText = gpx(listOf(pts)).replaceFirst("<name>t</name>", "<name>${"n".repeat(4_000_000)}</name>")
        val manySegs = gpx(pts.map { listOf(it) })
        for ((what, doc) in listOf("10k nested" to deep, "4 MB name" to giantText, "200 one-fix segments" to manySegs)) {
            check(what, doc.toByteArray())
        }
    }

    @Test
    fun deepNestingParsesInLinearTime() {
        val pts = run(200)
        fun nested(n: Int) = gpx(listOf(pts)).replaceFirst("<extensions>", "<extensions>" + "<x>".repeat(n) + "</x>".repeat(n)).toByteArray()
        val times = listOf(25_000, 50_000, 100_000).associateWith { n ->
            val doc = nested(n)
            val t0 = System.nanoTime()
            bounded("$n nested", 60_000) { runCatching { parse(doc) } }
            (System.nanoTime() - t0) / 1_000_000
        }
        // A 400-fix GPX parses in ~20 ms; 100k empty elements are 0.7 MB of input.
        assertTrue("parse ms by nesting depth: $times", times.getValue(100_000) < 2_000)
    }

    // ═══ Kotlin ↔ Rust agreement ═══════════════════════════════════════════════

    @Test
    fun theSameRunAsFitGpxAndTcxGivesTheSameSavedFigures() {
        val pts = run(900, seed = 7)
        val events = mapOf(
            "fit" to importedEvent(fit(pts)),
            "gpx" to importedEvent(gpx(listOf(pts), hdop = false).toByteArray()),
            "tcx" to importedEvent(tcx(listOf("Running" to listOf(pts))).toByteArray()),
        )
        val figs = events.mapValues { (k, ev) -> checkNotNull(Core.savedFigures(ev)) { k } }
        val ref = figs.getValue("gpx")
        for ((k, f) in figs) {
            assertEquals("$k moving time", ref.duration_min, f.duration_min, 1e-9)
            assertEquals("$k distance", ref.distance_km, f.distance_km, ref.distance_km * 1e-4)
            assertEquals("$k hr%", events.getValue("gpx").hrPctMax, events.getValue(k).hrPctMax, 1e-9)
            assertEquals("$k stamp", events.getValue("gpx").observedAt, events.getValue(k).observedAt)
        }
        // The FIT path's fixes are the semicircle-rounded source fixes (~1 cm).
        val fitPts = events.getValue("fit").points
        for ((a, b) in fitPts.zip(pts)) {
            assertEquals(b.t, a.observedAt)
            assertTrue("fit fix $a vs $b", abs(a.lat - b.lat) < 1e-7 && abs(a.lon - b.lon) < 1e-7)
        }
    }

    @Test
    fun aRustFitRefusalMapsToTheShellsReason() {
        val pts = run(900, seed = 8)
        val cases = mapOf(
            fit(pts, sport = 2) to (ImportFailure.NotARun to "cycling"),
            fit(pts.take(100)) to (ImportFailure.TooShort to null),
            fit(pts.take(1)) to (ImportFailure.NoTrack to null),
            fit(pts).copyOf(30) to (ImportFailure.Damaged to null),
            byteArrayOf(14, 0x20, 0, 0, 0, 0, 0, 0, '.'.code.toByte(), 'F'.code.toByte(), 'I'.code.toByte(), 'T'.code.toByte(), 0, 0) to (ImportFailure.NoTrack to null),
        )
        for ((bytes, want) in cases) {
            assertEquals("${hex(bytes)}", Out.Failed(want.first, want.second), check("fit refusal", bytes))
        }
    }

    @Test
    fun aTrackSentToTheCoreComesBackFromItsGpxBitForBit() {
        val pts = run(400, seed = 9).map { it.point() }
        val doc = coreGpx(pts)
        val back = parseGpx(doc).flatten()
        assertEquals(pts.size, back.size)
        val moved = pts.zip(back).flatMap { (a, b) -> listOf(a.lat to b.lat, a.lon to b.lon) }.filter { (a, b) -> a.toBits() != b.toBits() }
        assertTrue("${moved.size} of ${2 * pts.size} coordinates changed, e.g. ${moved.firstOrNull()}", moved.isEmpty())
    }

    @Test
    fun aCoreGpxExportReadsBackWithTheSameTimesSegmentsAndAccuracy() {
        val pts = run(400, seed = 10).mapIndexed { i, f -> f.point().copy(accuracyM = 3.0 + (i % 7)) }
        val doc = coreGpx(pts, starts = listOf(100, 250))
        val segs = parseGpx(doc)
        assertEquals(listOf(100, 150, 150), segs.map { it.size })
        val back = segs.flatten()
        for ((a, b) in pts.zip(back)) {
            assertEquals(a.observedAt, b.timeSec)
            assertEquals(a.accuracyM, b.accuracyM!!, 1e-9)
            assertEquals(a.lat, b.lat, 1e-12)
        }
    }

    @Test
    fun aFarFutureRunTheImporterAcceptedReimportsFromItsOwnExport() {
        val pts = run(300).map { it.copy(t = it.t + 251_652_300_800L) } // year 10000
        val doc = gpx(listOf(pts))
        assertTrue("java.time names the year with its sign", doc.contains("<time>+10000-"))
        val ev = importedEvent(doc.toByteArray())
        assertTrue("the core accepts the run", Core.send(ev).accepted)
        val export = Core.exportGpx(ev.entryId, ev.observedAt).orEmpty()
        assertTrue("exported", export.contains("<trkpt"))
        val back = parseGpx(export).flatten()
        assertEquals("times read back from ${Regex("<time>[^<]*</time>").find(export)?.value}",
            ev.points.map { it.observedAt }, back.map { it.timeSec })
    }

    // ═══ JNI byte entry points ═════════════════════════════════════════════════

    private val nativeUpdate = Core.javaClass.getDeclaredMethod("update", ByteArray::class.java).apply { isAccessible = true }
    private val nativeView = Core.javaClass.getDeclaredMethod("view").apply { isAccessible = true }
    private val nativeTrackFigures = Core.javaClass.getDeclaredMethod("trackFigures", ByteArray::class.java).apply { isAccessible = true }

    private fun update(bytes: ByteArray) = nativeUpdate.invoke(Core, bytes) as ByteArray?
    private fun view() = nativeView.invoke(Core) as ByteArray?

    private fun malformed(rnd: Random, seeds: List<String>): ByteArray = when (rnd.nextInt(6)) {
        0 -> ByteArray(rnd.nextInt(64)) { rnd.nextInt(256).toByte() }
        1 -> "[".repeat(50_000 + rnd.nextInt(10)).toByteArray()
        2 -> byteArrayOf(0xC3.toByte(), 0x28, 0xFF.toByte(), 0xFE.toByte())
        else -> mutateBytes(rnd, seeds[rnd.nextInt(seeds.size)].toByteArray())
    }

    private val eventSeeds by lazy {
        listOf(
            Event.SetToday(20_300L, 3600).toJson().toString(),
            Event.LogRunTrack(run(60).map { it.point() }, 0.0, 1_750_000_060L, 5L).toJson().toString(),
            Event.ClearRuns.toJson().toString(),
            QaHarness.loggedRun(1_750_000_000L).toJson().toString(),
        )
    }

    @Test
    fun malformedUpdateBytesNeverCrashAndLeaveTheCoreUsable() {
        assertTrue(Core.send(QaHarness.loggedRun(1_749_000_000L)).accepted)
        val before = view()!!
        var rejected = 0
        for (case in 0 until iters * 4) {
            val rnd = Random(0x6400_0000L + case)
            val bytes = malformed(rnd, eventSeeds)
            val out = bounded("update $case") { update(bytes) }
            assertNotNull("update $case returned null", out)
            val text = String(out!!, Charsets.UTF_8).trim()
            assertTrue("update $case: neither effects, empty nor error: ${text.take(200)}",
                text.isEmpty() || text.startsWith("[") || isCoreErrorEnvelope(text))
            assertTrue("update $case: a panic reset the core: $text", !isCorePanicReset(text))
            if (text.isEmpty()) rejected++
            val v = view()
            assertNotNull("view after $case", v)
            assertTrue("view after $case is a view", Json.parseToJsonElement(String(v!!, Charsets.UTF_8)) is JsonObject)
        }
        assertTrue(rejected > 0)
        // Rejected bytes changed nothing a seed event could not: the core still answers.
        assertTrue(Core.send(Event.SetToday(20_300L, 0)).accepted)
        assertTrue(before.isNotEmpty())
    }

    @Test
    fun malformedTrackFiguresAndExportIdsAnswerWithEnvelopesOrEmpty() {
        for (case in 0 until iters * 2) {
            val rnd = Random(0x6500_0000L + case)
            val bytes = malformed(rnd, eventSeeds)
            val text = bounded("trackFigures $case") { nativeTrackFigures.invoke(Core, bytes) as String? }
            assertNotNull(text)
            if (!isCoreErrorEnvelope(text!!)) {
                val o = Json.parseToJsonElement(text).jsonObject
                val km = o["distance_km"]?.jsonPrimitive?.doubleOrNull
                val min = o["duration_min"]?.jsonPrimitive?.doubleOrNull
                assertTrue("trackFigures $case: $text", km != null && min != null && km.isFinite() && min.isFinite() && km >= 0 && min >= 0)
            }
        }
        assertTrue(Core.send(Event.LogRunTrack(run(300).map { it.point() }, 0.0, 1_750_000_300L, 42L)).accepted)
        for (id in listOf(0L, -1L, Long.MIN_VALUE, Long.MAX_VALUE, 42L)) for (at in listOf(0L, -1L, Long.MIN_VALUE, Long.MAX_VALUE, 1_750_000_300L)) {
            val gpx = Core.exportGpx(id, at)
            assertNotNull(gpx)
            if (id == 42L && at == 1_750_000_300L) assertTrue(gpx!!.contains("<trkpt")) else assertEquals("exportGpx($id, $at)", "", gpx)
        }
    }

    @Test
    fun parseFitOfArbitraryBytesIsAlwaysAnEnvelopeTheShellMaps() {
        for (case in 0 until iters * 2) {
            val rnd = Random(0x6600_0000L + case)
            val bytes = ByteArray(12 + rnd.nextInt(600)) { rnd.nextInt(256).toByte() }
            ".FIT".toByteArray().copyInto(bytes, 8)
            bytes[0] = if (rnd.nextBoolean()) 14 else 12
            val json = bounded("parseFit $case") { Core.parseFit(bytes) }
            assertNotNull(json)
            val root = Json.parseToJsonElement(json!!).jsonObject
            val err = root["error"]
            if (err != null) {
                val kind = (err as? JsonObject)?.get("kind")?.jsonPrimitive?.contentOrNull
                assertTrue("parseFit $case: $json", kind in setOf("not_fit", "damaged", "no_track", "not_a_run") || err !is JsonObject)
            } else {
                assertTrue(root["segments"] is JsonArray)
            }
            // The shell's decode maps it: a run or a named reason, never another exception.
            try {
                fitSegments(json)
            } catch (e: ImportException) {
                // named
            }
        }
    }
}

/** Offsets in [from, until) to cut [doc] at, as a download cut off mid-file: every
 *  [step]th byte plus the end of every [tagStride]th tag (a cut right after a tag
 *  is the case the pull parser does not notice). The defaults sample about 150 of
 *  each across the whole document, never the whole document (its final newline
 *  excluded). */
internal fun truncationCuts(
    doc: ByteArray,
    from: Int = 0,
    until: Int = doc.size - 1,
    step: Int = maxOf(1, doc.size / 150),
    tagStride: Int = maxOf(1, doc.count { it == '>'.code.toByte() } / 150),
): List<Int> {
    val tagEnds = (maxOf(from, 1) until until).filter { doc[it - 1] == '>'.code.toByte() }
    return ((from until until step step) + tagEnds.filterIndexed { i, _ -> i % tagStride == 0 }).distinct().sorted()
}
