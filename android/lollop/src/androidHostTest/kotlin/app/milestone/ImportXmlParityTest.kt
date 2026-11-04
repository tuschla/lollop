package app.milestone

import java.time.Instant
import java.time.LocalDateTime
import java.time.OffsetDateTime
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import java.time.format.DateTimeFormatterBuilder
import kotlin.random.Random
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * The common import code against what it replaced or runs beside:
 *  - every GPX/TCX document the import tests read (plus XML edge cases: entities,
 *    CDATA, comments, self-closing tags, stray content, cuts at every offset) reads to
 *    the same runs, or the same failure, through the platform's xmlpull parser and
 *    through [XmlUtilImportReader], the iOS reader; the few malformed documents only
 *    one of them tolerates are pinned as such;
 *  - [parseImportTimeSec] answers exactly as the java.time parsers it replaced;
 *  - [decodeUtf16] (iOS) decodes exactly as the JDK's UTF-16 decoders.
 * The fuzz corpus is compared in `ImportParserFuzzTest`.
 */
@RunWith(RobolectricTestRunner::class)
class ImportXmlParityTest {

    private fun outcome(bytes: ByteArray, xml: (String) -> ImportXmlReader): Any = try {
        parseRunFileSegments(bytes, { null }, xml)
    } catch (e: ImportException) {
        e.reason
    } catch (e: Exception) {
        e::class.java.name
    }

    private fun platform(doc: String) = outcome(doc.toByteArray(), ::importXmlReader)
    private fun xmlutil(doc: String) = outcome(doc.toByteArray(), ::XmlUtilImportReader)

    private val gpxHead = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n" +
        "<gpx version=\"1.1\" creator=\"t\" xmlns=\"http://www.topografix.com/GPX/1/1\" " +
        "xmlns:gpxtpx=\"http://www.garmin.com/xmlschemas/TrackPointExtension/v1\">"

    private fun gpxDoc(body: String) = "$gpxHead<trk><type>running</type><trkseg>$body</trkseg></trk></gpx>\n"

    private val pt1 = "<trkpt lat=\"47.0\" lon=\"8.0\"><time>2026-03-04T05:06:00Z</time></trkpt>"
    private val pt2 = "<trkpt lat=\"47.001\" lon=\"8.001\"><time>2026-03-04T05:06:30Z</time></trkpt>"

    private fun tcxDoc(activities: String) =
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<TrainingCenterDatabase xmlns=\"http://www.garmin.com/xmlschemas/TrainingCenterDatabase/v2\">" +
            "<Activities>$activities</Activities></TrainingCenterDatabase>\n"

    private fun tcxPoint(t: String, lat: String, lon: String, hr: String = "142") =
        "<Trackpoint><Time>$t</Time><Position><LatitudeDegrees>$lat</LatitudeDegrees>" +
            "<LongitudeDegrees>$lon</LongitudeDegrees></Position><HeartRateBpm><Value>$hr</Value></HeartRateBpm></Trackpoint>"

    /** The documents the import tests read, verbatim or as their builders write them. */
    private val testDocuments: List<String> = listOf(
        // LocaleNumberTest
        """<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1" creator="t" xmlns="http://www.topografix.com/GPX/1/1" xmlns:gpxtpx="http://www.garmin.com/xmlschemas/TrackPointExtension/v1">
<trk><type>RUNNING</type><trkseg>
<trkpt lat="52.5200" lon="13.4050"><ele>34.5</ele><time>2026-03-04T05:06:00Z</time><hdop>1.5</hdop><extensions><gpxtpx:TrackPointExtension><gpxtpx:hr>142.0</gpxtpx:hr></gpxtpx:TrackPointExtension></extensions></trkpt>
<trkpt lat="52.5209" lon="13.4061"><ele>35.0</ele><time>2026-03-04T05:06:30+00:00</time><extensions><gpxtpx:TrackPointExtension><gpxtpx:hr>145</gpxtpx:hr></gpxtpx:TrackPointExtension></extensions></trkpt>
<trkpt lat="52.5218" lon="1.34072E1"><time>2026-03-04T07:07:00+0200</time></trkpt>
</trkseg></trk></gpx>""",
        """<?xml version="1.0" encoding="UTF-8"?>
<TrainingCenterDatabase xmlns="http://www.garmin.com/xmlschemas/TrainingCenterDatabase/v2"><Activities><Activity Sport="Running"><Lap><Track>
<Trackpoint><Time>2026-03-04T05:06:00.000Z</Time><Position><LatitudeDegrees>52.52</LatitudeDegrees><LongitudeDegrees>13.405</LongitudeDegrees></Position><HeartRateBpm><Value>142</Value></HeartRateBpm></Trackpoint>
<Trackpoint><Time>2026-03-04T05:06:30.000Z</Time><Position><LatitudeDegrees>52.5209</LatitudeDegrees><LongitudeDegrees>13.4061</LongitudeDegrees></Position><HeartRateBpm><Value>145</Value></HeartRateBpm></Trackpoint>
</Track></Lap></Activity></Activities></TrainingCenterDatabase>""",
        // ReviewRunsTest
        """
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
        """.trimIndent(),
        // ImportExportRoundtripTest
        """
            <gpx>
              <trk>
                <trkseg>
                  <trkpt lat="10.0" lon="20.0"><time>2020-01-01T00:00:00Z</time></trkpt>
                  <trkpt lat="10.1" lon="20.1"><time>2020-01-01T00:00:01Z</time></trkpt>
                </trkseg>
                <trkpt lat="10.2" lon="20.2"><time>2020-01-01T00:00:02Z</time></trkpt>
              </trk>
              <trkpt lat="10.3" lon="20.3"><time>2020-01-01T00:00:03Z</time></trkpt>
            </gpx>
        """.trimIndent(),
        "<?xml version=\"1.0\"?><gpx version=\"1.1\"><rte><rtept lat=\"47.0\" lon=\"8.0\"/><rtept lat=\"47.1\" lon=\"8.1\"/></rte></gpx>",
        // RunImportTest: two tracks, two sports
        "<?xml version=\"1.0\"?><gpx version=\"1.1\">" +
            "<trk><type>running</type><trkseg>$pt1$pt2</trkseg></trk>" +
            "<trk><type>cycling</type><trkseg>$pt1$pt2</trkseg></trk></gpx>",
        // The import tests' GPX / TCX builders (ImportExportRoundtripTest, ReviewRunsTest)
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<gpx version=\"1.1\" creator=\"test\" xmlns=\"http://www.topografix.com/GPX/1/1\">\n<trk><name>t</name>" +
            "<trkseg>\n$pt1\n$pt2\n</trkseg>\n<trkseg>\n$pt1\n</trkseg>\n</trk>\n</gpx>\n",
        tcxDoc(
            "<Activity Sport=\"Running\"><Id>2026-03-04T05:06:00Z</Id><Lap StartTime=\"2026-03-04T05:06:00Z\"><Track>" +
                tcxPoint("2026-03-04T05:06:00Z", "47.0", "8.0") + tcxPoint("2026-03-04T05:06:30Z", "47.001", "8.001") +
                "</Track></Lap><Lap><Track>" + tcxPoint("2026-03-04T05:06:31Z", "47.002", "8.002") +
                "<Trackpoint><Time>2026-03-04T05:06:40Z</Time></Trackpoint>" +
                "</Track></Lap></Activity>" +
                "<Activity Sport=\"Biking\"><Lap><Track>" + tcxPoint("2026-03-05T05:06:00Z", "47.0", "8.0") +
                tcxPoint("2026-03-05T05:07:00Z", "47.01", "8.01", hr = "NaN") + "</Track></Lap></Activity>",
        ) .replace("</Activities>", "</Activities><Courses><Course><Track>" + tcxPoint("2026-03-06T00:00:00Z", "1", "2") + "</Track></Course></Courses>"),
    )

    /** XML corners where two pull parsers could part ways. */
    private val edgeDocuments: List<String> = listOf(
        gpxDoc("$pt1$pt2"),
        gpxDoc("$pt1<trkpt lat=\"47.0005\" lon=\"8.0005\"/>$pt2"),
        gpxDoc("<trkpt lat=\"47.0\" lon=\"8.0\"><time>2026-03-04T05:06:00Z<!-- c --></time></trkpt>$pt2"),
        gpxDoc("<trkpt lat=\"47.0\" lon=\"8.0\"><time>2026-03-04<!-- c -->T05:06:00Z</time></trkpt>$pt2"),
        gpxDoc("<trkpt lat=\"47.0\" lon=\"8.0\"><time><![CDATA[2026-03-04T05:06:00Z]]></time></trkpt>$pt2"),
        gpxDoc("<trkpt lat=\"47.0\" lon=\"8.0\"><time>2026-03-04T05:06:<![CDATA[00]]>Z</time></trkpt>$pt2"),
        gpxDoc("<trkpt lat=\"47.0\" lon=\"8.0\"><time>2026-03-04T05:06:00&#x5A;</time></trkpt>$pt2"),
        gpxDoc("<trkpt lat=\"&#52;7.0\" lon=\"8.0\"><time>2026-03-04T05:06:00Z</time></trkpt>$pt2"),
        gpxDoc("<trkpt lat=\"47.0\" lon=\"8.0\"><time>2026-03-04T05:06:00Z<?pi x?></time></trkpt>$pt2"),
        gpxDoc("<trkpt lat=\"47.0\" lon=\"8.0\"><time>&amp;</time></trkpt>$pt2"),
        gpxDoc("<trkpt lat=\"47.0\" lon=\"8.0\"><time>&undefined;</time></trkpt>$pt2"),
        gpxDoc("<trkpt lat=\"47.0\" lon=\"8.0\"><time><b>x</b></time></trkpt>$pt2"),
        gpxDoc("<trkpt lat=\"47.0\" lon=\"8.0\"><time/></trkpt>$pt2"),
        gpxDoc("<trkpt lat=\"47.0\" lon=\"8.0\"><time>\r\n 2026-03-04T05:06:00Z \r\n</time></trkpt>$pt2"),
        gpxDoc("<trkpt lat=\"47.0\" lat=\"48.0\" lon=\"8.0\"><time>2026-03-04T05:06:00Z</time></trkpt>$pt2"),
        gpxDoc("<trkpt lat='47.0' lon='8.0'><time>2026-03-04T05:06:00Z</time></trkpt>$pt2"),
        gpxDoc("<trkpt lat=\" 47.0\n\" lon=\"8.0\"><time>2026-03-04T05:06:00Z</time></trkpt>$pt2"),
        gpxDoc("<trkpt gpxtpx:lat=\"1.0\" lat=\"47.0\" lon=\"8.0\"><time>2026-03-04T05:06:00Z</time></trkpt>$pt2"),
        gpxDoc("$pt1<trkpt lat=\"47.0\" lon=\"8.0\"><extensions><gpxtpx:TrackPointExtension><gpxtpx:hr>150</gpxtpx:hr></gpxtpx:TrackPointExtension></extensions><time>2026-03-04T05:06:10Z</time></trkpt>$pt2"),
        gpxDoc("$pt1 text between \u00e9 $pt2"),
        gpxDoc("$pt1$pt2").replace("<trk>", "<trk><trk>").replace("</trk>", "</trk></trk>"),
        gpxDoc("$pt1$pt2") + "trailing text",
        gpxDoc("$pt1$pt2") + "<!-- after -->\n<?pi after?>\n",
        gpxDoc("$pt1$pt2").replace("</gpx>", "</gpx><!DOCTYPE gpx>"),
        gpxDoc("$pt1$pt2").replace("<?xml version=\"1.0\" encoding=\"UTF-8\"?>", "<?xml version=\"1.0\" encoding=\"ISO-8859-1\" standalone=\"yes\"?>"),
        gpxDoc("$pt1$pt2").replace("<?xml version=\"1.0\" encoding=\"UTF-8\"?>", "<?xml encoding=\"UTF-8\"?>"),
        " " + gpxDoc("$pt1$pt2"),
        gpxDoc("$pt1$pt2").replace("<trkseg>", "<trkseg >").replace("</trkseg>", "</trkseg >"),
        gpxDoc("$pt1$pt2").replace("</trkseg>", "</trkseg2>"),
        gpxDoc("$pt1$pt2").replace("lat=\"47.0\"", "lat=47.0"),
        gpxDoc("$pt1$pt2").replace("8.0\">", "8.0\" >"),
        gpxDoc("<trkpt lat=\"47.0\" lon=\"8.0\"><time>2026-03-04T05:06:00Z</time></trkpt>\u0000$pt2"),
        gpxDoc("$pt1$pt2").replace("<trk>", "<trk>" + "<x>".repeat(70) + "</x>".repeat(70)),
        gpxDoc("$pt1$pt2").replace("<trk>", "<trk>" + "<x>".repeat(60) + "</x>".repeat(60)),
        gpxDoc("$pt1$pt2").replace("gpxtpx", "ns1"),
        gpxDoc("$pt1$pt2").replace("<type>running</type>", "<type> Trail Running </type><type>cycling</type>"),
        gpxDoc("$pt1$pt2").replace("<type>running</type>", "<extensions><lollop:logged_at xmlns:lollop=\"urn:lollop:gpx:1\">2026-03-04T05:07:00Z</lollop:logged_at></extensions>"),
        tcxDoc("<Activity Sport=\" Running \"><Lap><Track>" + tcxPoint("2026-03-04T05:06:00Z", "47.0", "8.0") + tcxPoint("2026-03-04T05:06:30Z", "47.001", "8.001") + "</Track></Lap></Activity>"),
        tcxDoc("<Activity><Lap><Track>" + tcxPoint("2026-03-04T05:06:00Z", " 47.0 ", "8.0") + tcxPoint("2026-03-04T05:06:30Z", "47.001", "8.001") + "</Track></Lap></Activity>"),
        tcxDoc("<ns:Activity xmlns:ns=\"urn:x\" Sport=\"Running\"><Lap><Track>" + tcxPoint("2026-03-04T05:06:00Z", "47.0", "8.0") + "</Track></Lap></ns:Activity>"),
    )

    /** Malformed documents only xmlpull (without namespace processing) reads; xmlutil
     *  calls them damaged: a prefix nobody declared, a second root element. No
     *  exporter is known to write either. */
    private val platformOnly = listOf(
        gpxDoc("$pt1$pt2").replace(" xmlns:gpxtpx=\"http://www.garmin.com/xmlschemas/TrackPointExtension/v1\"", "")
            .replace("<trkseg>", "<trkseg><gpxtpx:x/>"),
        gpxDoc("<trkpt lat=\"47.0\" lon=\"8.0\" undeclared:x=\"1\"><time>2026-03-04T05:06:00Z</time></trkpt>$pt2"),
        gpxDoc("$pt1$pt2") + "<gpx/>",
    )

    /** Malformed documents only xmlutil reads; xmlpull calls them damaged: a `<`
     *  inside an attribute value. */
    private val xmlutilOnly = listOf(
        gpxDoc("$pt1$pt2").replace("lat=\"47.0\"", "lat=\"47<0\""),
    )

    @Test
    fun bothReadersReadEveryTestAndEdgeDocumentAlike() {
        var compared = 0
        val diffs = mutableListOf<String>()
        fun compare(what: String, doc: String) {
            val p = platform(doc)
            val x = xmlutil(doc)
            compared++
            if (p != x) diffs += "$what: platform $p, xmlutil $x\n  ${doc.take(300)}"
        }
        for ((i, doc) in (testDocuments + edgeDocuments).withIndex()) {
            compare("doc $i", doc)
            // A download cut off at every offset.
            for (cut in 0 until doc.length) compare("doc $i cut at $cut", doc.substring(0, cut))
        }
        println("reader parity: $compared documents, ${diffs.size} differ")
        assertTrue("${diffs.size} of $compared differ:\n${diffs.take(5).joinToString("\n")}", diffs.isEmpty())
        // The test documents hold runs, so the comparison above compared runs, not only failures.
        assertTrue(testDocuments.count { platform(it) is List<*> && (platform(it) as List<*>).isNotEmpty() } >= 6)
    }

    @Test
    fun malformedXmlOnlyOneReaderToleratesIsDamagedForTheOther() {
        for (doc in platformOnly) {
            assertTrue(platform(doc).let { it is List<*> && it.isNotEmpty() })
            assertEquals(ImportFailure.Damaged, xmlutil(doc))
        }
        for (doc in xmlutilOnly) {
            assertEquals(ImportFailure.Damaged, platform(doc))
            assertTrue(xmlutil(doc).let { it is List<*> && it.isNotEmpty() })
        }
    }

    // ── Times ────────────────────────────────────────────────────────────────

    private val colonless: DateTimeFormatter = DateTimeFormatterBuilder()
        .append(DateTimeFormatter.ISO_LOCAL_DATE_TIME)
        .appendOffset("+HHMM", "Z")
        .toFormatter()

    /** The java.time reader [parseImportTimeSec] replaced. */
    private fun javaTimeSec(raw: String): Long {
        val s = raw.trim()
        if (s.isEmpty()) return 0L
        return runCatching { Instant.parse(s).epochSecond }
            .recoverCatching { OffsetDateTime.parse(s).toEpochSecond() }
            .recoverCatching { LocalDateTime.parse(s).toInstant(ZoneOffset.UTC).epochSecond }
            .recoverCatching { OffsetDateTime.parse(s, colonless).toEpochSecond() }
            .getOrDefault(0L)
    }

    private val years = listOf(
        "2026", "1970", "1969", "2000", "1900", "2024", "2023", "0000", "0001", "9999", "-0001", "-0000", "-2024",
        "+10000", "+00000", "+2026", "20260", "202", "+999999999", "999999999", "-999999999", "+1000000000",
        "-1000000000", "+9999999999", "+0999999999", "+12345678901",
    )
    private val months = listOf("01", "02", "03", "04", "06", "12", "00", "13", "1", "011")
    private val days = listOf("01", "28", "29", "30", "31", "32", "00", "1")
    private val seps = listOf("T", "t", " ", "", "TT")
    private val hours = listOf("00", "05", "23", "24", "25", "60", "9")
    private val minutes = listOf("00", "30", "59", "60", "5")
    private val seconds = listOf("", ":00", ":30", ":59", ":60", ":61", ":5", ":")
    private val fractions = listOf("", ".", ".0", ".000", ".000000000", ".123456789", ".1234567890", ".5", ",5", ".0000000000")
    private val offsets = listOf(
        "", "Z", "z", "+00:00", "-00:00", "+02:00", "-05:30", "+18:00", "-18:00", "+18:01", "+17:59:59", "+18:00:01",
        "+0200", "-0530", "+1800", "+1801", "+02", "-18", "+19", "+2", "+02:00:30", "+02:00:3", "+0200:30", "+24:00",
        "+23:59", "+020030", "+02:60", "+02:00:60", "+2:00", "Z+01:00", "UTC", "+02:", "ZZ", "+00",
    )

    @Test
    fun importTimesReadExactlyAsJavaTimeReadThem() {
        val fixed = listOf(
            "", " ", "garbage", "2026-03-04T05:06:00Z", " 2026-03-04T05:06:00Z\n", "2026-03-04T05:06:00", "2026-03-04T05:06",
            "2026-03-04T05:06Z", "2026-03-04", "2026-03-04T24:00:00Z", "2026-03-04T24:00Z", "2026-03-04T24:00:00.000Z",
            "2026-03-04T24:00:00.1Z", "2026-03-04T24:00:00+0100", "2026-03-04T24:00+0100", "2026-03-04T24:00:00",
            "2016-12-31T23:59:60Z", "2016-12-31T23:59:60+01:00", "2016-12-31T23:59:60+0100", "2016-12-31T23:59:60",
            "2026-02-30T10:00:00Z", "2026-02-30T10:00:00+0100", "2024-02-29T10:00:00Z", "2023-02-29T10:00:00Z",
            "2026-04-31T24:00Z", "999999999-12-31T24:00Z", "+999999999-12-31T23:59:59Z", "+1000000000-12-31T23:59:59Z",
            "+1000000000-12-31T23:59:59-01:00", "-1000000000-01-01T00:00:00Z", "-1000000000-01-01T00:00:00+01:00",
            "9999-12-31T23:59:59Z", "+10000-01-01T00:00:00Z", "1969-12-31T23:59:59Z", "2025-06-15T12:00:00+14:00",
            "2025-06-15T12:00:00.999999999Z", "2025-02-30T00:00:00Z", "-999999999-01-01T00:00:00Z",
            "2026-03-04T05:06:00.Z", "2026-03-04t05:06:00z", "\u0662\u0660\u0662\u0666-03-04T05:06:00Z",
        )
        val mismatches = mutableListOf<String>()
        fun check(s: String) {
            val expected = javaTimeSec(s)
            val actual = parseImportTimeSec(s)
            if (expected != actual) mismatches += "\"$s\": java.time $expected, common $actual"
        }
        fixed.forEach(::check)
        val rnd = Random(0x71AE)
        val n = 120_000
        repeat(n) {
            fun pick(l: List<String>, common: String) = if (rnd.nextInt(3) == 0) l.random(rnd) else common
            val s = pick(years, "2026") + "-" + pick(months, "03") + "-" + pick(days, "04") + pick(seps, "T") +
                pick(hours, "05") + ":" + pick(minutes, "06") + pick(seconds, ":00") + pick(fractions, "") + pick(offsets, "Z")
            check(s)
        }
        assertTrue("${mismatches.size} of ${fixed.size + n} differ:\n${mismatches.take(10).joinToString("\n")}", mismatches.isEmpty())
    }

    // ── UTF-16 ───────────────────────────────────────────────────────────────

    @Test
    fun utf16DecodesExactlyAsTheJdk() {
        val rnd = Random(0x16)
        val units = intArrayOf(0x0041, 0x00E9, 0xD83D, 0xDE00, 0xDBFF, 0xDC00, 0xFEFF, 0xFFFE, 0xFFFF, 0x003C, 0x0000)
        var cases = 0
        for (bigEndian in listOf(true, false)) {
            repeat(20_000) {
                val bytes = if (rnd.nextBoolean()) {
                    ByteArray(rnd.nextInt(12)) { rnd.nextInt(256).toByte() }
                } else {
                    val out = java.io.ByteArrayOutputStream()
                    repeat(rnd.nextInt(8)) {
                        val u = units[rnd.nextInt(units.size)]
                        if (bigEndian) { out.write(u shr 8); out.write(u and 0xFF) } else { out.write(u and 0xFF); out.write(u shr 8) }
                    }
                    if (rnd.nextInt(4) == 0) out.write(rnd.nextInt(256))
                    out.toByteArray()
                }
                val offset = if (bytes.isNotEmpty()) rnd.nextInt(minOf(3, bytes.size + 1)) else 0
                val length = bytes.size - offset
                val jdk = String(bytes, offset, length, if (bigEndian) Charsets.UTF_16BE else Charsets.UTF_16LE)
                assertEquals("${bytes.joinToString { "%02x".format(it) }} @$offset be=$bigEndian", jdk, decodeUtf16(bytes, offset, length, bigEndian))
                cases++
            }
        }
        assertEquals(40_000, cases)
    }
}
