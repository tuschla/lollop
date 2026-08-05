package app.milestone

import android.util.Xml
import kotlinx.serialization.json.double
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import org.xmlpull.v1.XmlPullParser
import java.io.StringReader
import java.time.Instant
import java.time.OffsetDateTime

/**
 * Minimal GPX 1.0/1.1 track reader for two shell jobs: rendering a saved run's
 * route (parsing the core-produced [RunResultView.gpx] back into points) and
 * importing a run recorded elsewhere (Garmin/Strava/Komoot exports). Reads only
 * `<trk>/<trkseg>/<trkpt>`, lat/lon attributes, the optional `<time>`, and an
 * optional heart-rate sample from the common Garmin TrackPointExtension
 * (`<gpxtpx:hr>`, matched by local name so any namespace prefix works).
 * Everything else (waypoints, routes, metadata) is ignored.
 */
data class GpxFix(val lat: Double, val lon: Double, val timeSec: Long, val hrBpm: Int?)

/** One list of fixes per `<trkseg>`; segments mark recording pauses. */
fun parseGpx(text: String): List<List<GpxFix>> {
    val parser = Xml.newPullParser().apply {
        setFeature(XmlPullParser.FEATURE_PROCESS_NAMESPACES, false)
        setInput(StringReader(text))
    }
    val segments = mutableListOf<List<GpxFix>>()
    var segment = mutableListOf<GpxFix>()
    // Trackpoint handling is gated on being INSIDE a `<trk>/<trkseg>` (mirrors
    // TCX's `inActivity` gate): a stray `<trkpt>` outside `<trk>` or after
    // `</trkseg>` would otherwise reuse the last finite lat/lon and append into
    // the already-committed segment, silently merging waypoints/route noise
    // into a recorded run (review 2026-08-04, LOW).
    var inTrk = false
    var inTrkseg = false
    var inTrkpt = false
    var lat = 0.0
    var lon = 0.0
    var timeSec = 0L
    var hr: Int? = null
    var event = parser.eventType
    while (event != XmlPullParser.END_DOCUMENT) {
        val name = parser.name?.substringAfterLast(':')
        when (event) {
            XmlPullParser.START_TAG -> when (name) {
                "trk" -> inTrk = true
                "trkseg" -> if (inTrk) {
                    inTrkseg = true
                    segment = mutableListOf()
                }
                "trkpt" -> if (inTrkseg) {
                    inTrkpt = true
                    lat = parser.getAttributeValue(null, "lat")?.toDoubleOrNull() ?: Double.NaN
                    lon = parser.getAttributeValue(null, "lon")?.toDoubleOrNull() ?: Double.NaN
                    timeSec = 0L
                    hr = null
                }
                "time" -> if (inTrkpt) timeSec = parseGpxTime(parser.nextText())
                "hr" -> if (inTrkpt) hr = parseHrBpm(parser.nextText())
            }
            XmlPullParser.END_TAG -> when (name) {
                "trkpt" -> if (inTrkpt) {
                    inTrkpt = false
                    if (lat.isFinite() && lon.isFinite()) {
                        segment.add(GpxFix(lat, lon, timeSec, hr))
                    }
                }
                "trkseg" -> if (inTrkseg) {
                    inTrkseg = false
                    if (segment.isNotEmpty()) segments.add(segment)
                }
                "trk" -> inTrk = false
            }
        }
        event = parser.next()
    }
    return segments
}

/**
 * A heart-rate sample from an imported file, kept only when finite and
 * physiologically plausible (20..=250 bpm). Rejects NaN / `Infinity` (a literal
 * `Infinity` token would otherwise reach the JSON wire as a bare token → the
 * whole event is serde-rejected and the import silently logs nothing) and
 * absurd magnitudes (e.g. 1e9 bpm poisoning TRIMP/CTL/ATL). Shared by the GPX
 * and TCX readers so both guard identically.
 */
internal fun parseHrBpm(raw: String): Int? {
    val v = raw.trim().toDoubleOrNull() ?: return null
    if (!v.isFinite() || v < 20.0 || v > 250.0) return null
    return v.toInt()
}

/** ISO-8601 `<time>` → unix seconds; 0 when missing/unparseable (route-only GPX). */
private fun parseGpxTime(raw: String): Long {
    val s = raw.trim()
    if (s.isEmpty()) return 0L
    return runCatching { Instant.parse(s).epochSecond }
        .recoverCatching { OffsetDateTime.parse(s).toEpochSecond() }
        .getOrDefault(0L)
}

/** Why an import produced no event, surfaced verbatim in the failure toast. */
class GpxImportException(message: String) : Exception(message)

/** FIT sniff: the 12/14-byte FIT header carries ".FIT" at offset 8. */
fun isFitFile(bytes: ByteArray): Boolean =
    bytes.size >= 12 &&
        bytes[8] == '.'.code.toByte() && bytes[9] == 'F'.code.toByte() &&
        bytes[10] == 'I'.code.toByte() && bytes[11] == 'T'.code.toByte()

/**
 * Decode the core's [Core.parseFit] JSON into the same segment shape
 * [parseGpx]/[parseTcx] produce, so all three formats share [importedRunEvent].
 * The core's `{"error":…}` envelope becomes the import-failure toast verbatim.
 *
 * `json` is nullable: the M5-hardened JNI returns a null jstring on a double
 * result-string allocation failure. Treat null like the error envelope, a
 * user-facing "couldn't read" failure, so the caller's runCatching toasts it.
 */
fun fitSegments(json: String?): List<List<GpxFix>> {
    val root = kotlinx.serialization.json.Json
        .parseToJsonElement(json ?: throw GpxImportException("Couldn't read this FIT file"))
        .jsonObject
    // Two error shapes share the key: parse_fit's own Err is a plain STRING,
    // but the ffi panic firewall emits an OBJECT ({"kind":"panic",…}), e.g. a
    // truncated file that passes the magic sniff then panics inside fitparser.
    // Only the string form is user-facing copy; the panic object gets a
    // generic message (review 2026-08-03).
    root["error"]?.let { err ->
        val msg = (err as? kotlinx.serialization.json.JsonPrimitive)?.content
            ?: "Couldn't read this FIT file"
        throw GpxImportException(msg)
    }
    val segments = root["segments"] ?: throw GpxImportException("Couldn't read this FIT file")
    return segments.jsonArray.map { seg ->
        seg.jsonArray.map { p ->
            val o = p.jsonObject
            GpxFix(
                lat = o.getValue("lat").jsonPrimitive.double,
                lon = o.getValue("lon").jsonPrimitive.double,
                timeSec = o.getValue("time_sec").jsonPrimitive.long,
                hrBpm = o["hr_bpm"]?.jsonPrimitive?.intOrNull,
            )
        }
    }
}

/**
 * Build the [Event.LogRunTrack] for an imported GPX document. The core stays
 * the single source of record: it derives distance/duration/pace/splits/zone
 * from the points exactly as it does for a live-tracked run; the shell only
 * reshapes the file.
 *
 * - `hr_pct_max` is computed ONLY when the file carries HR samples AND the
 *   profile has a MEASURED HRmax, never estimated shell-side (the Tanaka
 *   estimate is core logic). Otherwise 0.0 = "no HR", same as the tracker.
 * - GPX carries no fix accuracy; 5.0 m (a typical good phone/watch fix) is
 *   assumed so the core's accuracy QC doesn't reject a whole imported file.
 * - Decimated to [TRACK_DECIMATION_CAP] like a live save, keeping segment
 *   boundaries and endpoints, so one import can't bloat the append-only log.
 */
fun importedRunEvent(segments: List<List<GpxFix>>, measuredHrMax: Double?): Event.LogRunTrack {
    if (segments.sumOf { it.size } < 2) {
        throw GpxImportException("No GPS track found in this file")
    }
    // Drop untimestamped fixes at the SOURCE (review 2026-08-03): a LEADING
    // observed_at=0 fix slips past the core's dt<=0 QC gate (the first point is
    // accepted unconditionally) and its ~1.7e9-second "leg" counts distance but
    // not moving time, silently fast pace, and a 1970 stamp on re-export. FIT
    // already drops these in fit.rs; mirror it for GPX/TCX here. Same choke
    // point drops out-of-range coordinates (a hand-edited lat="91.5" would
    // otherwise render as a real point). Deliberate side effect: HR averaging
    // below also counts only KEPT fixes, a fix untrusted for position/time
    // isn't trusted for its HR sample either.
    val timed = segments
        .map { seg ->
            seg.filter {
                it.timeSec > 0 && kotlin.math.abs(it.lat) <= 90.0 && kotlin.math.abs(it.lon) <= 180.0
            }
        }
        .filter { it.isNotEmpty() }
    val all = timed.flatten()
    if (all.size < 2) {
        throw GpxImportException("This file has no timestamps (a planned route, not a recorded run)")
    }

    // Flatten segments and remember each segment's first index (the core's
    // segment_starts contract: indices that BEGIN a new segment; a single
    // unbroken track sends none).
    val boundaryIdx = mutableSetOf<Int>()
    var idx = 0
    for (seg in timed) {
        if (idx > 0) boundaryIdx.add(idx)
        idx += seg.size
    }

    // Stride-decimate over the cap, always keeping the first/last point and
    // every segment boundary (mirrors RunSession's decimation contract).
    val n = all.size
    val keep: List<Int> = if (n <= TRACK_DECIMATION_CAP) {
        (0 until n).toList()
    } else {
        val stride = (n + TRACK_DECIMATION_CAP - 1) / TRACK_DECIMATION_CAP
        (0 until n).filter { it % stride == 0 || it == n - 1 || it in boundaryIdx }
    }
    val indexMap = keep.withIndex().associate { (new, old) -> old to new }
    val points = keep.map { i ->
        val f = all[i]
        GpsPoint(lat = f.lat, lon = f.lon, observedAt = f.timeSec, accuracyM = 5.0)
    }
    val segmentStarts = boundaryIdx.sorted().mapNotNull { indexMap[it] }

    val hrSamples = all.mapNotNull { it.hrBpm }
    val hrPctMax = if (hrSamples.isNotEmpty() && measuredHrMax != null && measuredHrMax > 0.0) {
        hrSamples.average() / measuredHrMax * 100.0
    } else {
        0.0
    }

    return Event.LogRunTrack(
        points = points,
        hrPctMax = hrPctMax,
        longestRecentKm = 0.0,
        // Logged-at = when the run happened (last fix), not when it was
        // imported: history sorts and displays by this.
        observedAt = all.last().timeSec,
        workoutType = null,
        segmentStarts = segmentStarts,
    )
}
