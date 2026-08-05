package app.milestone

import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Instrumented parser tests that need the REAL `android.util.Xml` pull parser
 * (a non-functional stub in JVM unit tests), so they run on device/emulator and
 * complement the pure JVM [ImportParserTest].
 *
 * Covers the GPX `<trk>/<trkseg>` containment gate: a stray `<trkpt>` outside
 * `<trk>` or after `</trkseg>` must NOT append into a committed segment (it would
 * otherwise reuse the last finite lat/lon and pollute a recorded run, the fix
 * TCX already carried via its `inActivity` gate).
 */
@RunWith(AndroidJUnit4::class)
class GpxParserAndroidTest {

    @Test
    fun strayTrkptAfterTrksegCloseIsIgnored() {
        // A well-formed 2-point segment, then a rogue <trkpt> AFTER </trkseg>
        // (and a second one outside <trk> entirely). Neither may be counted.
        val gpx = """
            <gpx>
              <trk>
                <trkseg>
                  <trkpt lat="10.0" lon="20.0"><time>2020-01-01T00:00:00Z</time></trkpt>
                  <trkpt lat="10.1" lon="20.1"><time>2020-01-01T00:00:01Z</time></trkpt>
                </trkseg>
                <trkpt lat="99.0" lon="99.0"><time>2020-01-01T00:00:02Z</time></trkpt>
              </trk>
              <trkpt lat="88.0" lon="88.0"><time>2020-01-01T00:00:03Z</time></trkpt>
            </gpx>
        """.trimIndent()
        val segments = parseGpx(gpx)
        assertEquals("one committed segment", 1, segments.size)
        assertEquals("only the two in-segment fixes are kept", 2, segments[0].size)
    }

    @Test
    fun twoTrksegsRemainSeparateSegments() {
        // Sanity: genuine multi-segment GPX (a pause) still yields two segments.
        val gpx = """
            <gpx>
              <trk>
                <trkseg>
                  <trkpt lat="10.0" lon="20.0"><time>2020-01-01T00:00:00Z</time></trkpt>
                  <trkpt lat="10.1" lon="20.1"><time>2020-01-01T00:00:01Z</time></trkpt>
                </trkseg>
                <trkseg>
                  <trkpt lat="11.0" lon="21.0"><time>2020-01-01T00:05:00Z</time></trkpt>
                  <trkpt lat="11.1" lon="21.1"><time>2020-01-01T00:05:01Z</time></trkpt>
                </trkseg>
              </trk>
            </gpx>
        """.trimIndent()
        val segments = parseGpx(gpx)
        assertEquals(2, segments.size)
    }
}
