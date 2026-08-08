package app.milestone

import android.util.Xml
import kotlinx.serialization.json.double
import kotlinx.serialization.json.doubleOrNull
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
/**
 * [accuracyM] is the fix's horizontal accuracy in metres when the source file
 * carried a real figure, a FIT `record.gps_accuracy` reading, or a GPX
 * `<hdop>`-derived estimate ([hdopToAccuracyM]). `null` means the file said
 * nothing about accuracy (TCX always, GPX without `<hdop>`): honestly unknown,
 * NOT zero and NOT a good fix. [importedRunEvent] is the single place that
 * turns a `null` into a QC-passing sentinel, see [IMPORT_UNKNOWN_ACCURACY_M].
 */
data class GpxFix(
    val lat: Double,
    val lon: Double,
    val timeSec: Long,
    val hrBpm: Int?,
    val accuracyM: Double? = null,
)

/**
 * Nominal 1-sigma User Equivalent Range Error (metres) used to turn a
 * dimensionless GPX `<hdop>` into an accuracy estimate: `accuracy ≈ hdop × UERE`
 * (HDOP is unitless and multiplies the range error). 5 m is the conventional
 * figure for a modern consumer GNSS receiver with Selective Availability off.
 * This is an ESTIMATE, not a device-reported measurement: it is deliberately
 * coarse and only ever feeds the core's 30 m QC drop gate, never a precision
 * claim shown to the user.
 */
private const val GPS_UERE_NOMINAL_M = 5.0

/**
 * GPX `<hdop>` (dimensionless dilution of precision) → estimated horizontal
 * accuracy in metres, or `null` when the value is missing/unparseable/non-finite
 * or non-positive (a hand-edited `hdop=0`/negative is not a real fix quality).
 * Honest estimate only, see [GPS_UERE_NOMINAL_M].
 */
internal fun hdopToAccuracyM(raw: String): Double? {
    val hdop = raw.trim().toDoubleOrNull() ?: return null
    if (!hdop.isFinite() || hdop <= 0.0) return null
    return hdop * GPS_UERE_NOMINAL_M
}

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
    // the already-committed segment; silently merging waypoints/route noise
    // into a recorded run.
    var inTrk = false
    var inTrkseg = false
    var inTrkpt = false
    var lat = 0.0
    var lon = 0.0
    var timeSec = 0L
    var hr: Int? = null
    var accuracyM: Double? = null
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
                    accuracyM = null
                }
                "time" -> if (inTrkpt) timeSec = parseGpxTime(parser.nextText())
                "hr" -> if (inTrkpt) hr = parseHrBpm(parser.nextText())
                // `<hdop>` is a direct child of `<trkpt>` in GPX 1.0/1.1; a real
                // (estimated) accuracy, so surface it. Absent → stays null =
                // unknown, handled by importedRunEvent's sentinel.
                "hdop" -> if (inTrkpt) accuracyM = hdopToAccuracyM(parser.nextText())
            }
            XmlPullParser.END_TAG -> when (name) {
                "trkpt" -> if (inTrkpt) {
                    inTrkpt = false
                    if (lat.isFinite() && lon.isFinite()) {
                        segment.add(GpxFix(lat, lon, timeSec, hr, accuracyM))
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

/**
 * Accuracy (metres) stamped on an imported fix whose source file reported NO
 * accuracy at all (TCX always; GPX without `<hdop>`). It is a SENTINEL, not a
 * measurement: its one and only contract is "sits at the core's 30 m QC drop
 * gate (`running::MAX_GPS_ACCURACY_M` = 30.0) so an imported track is never rejected
 * for unknown accuracy, while being pinned to the WORST value that still passes
 * so any future accuracy-weighting feature treats it as the least-trusted
 * acceptable fix, never mistakes it for a good measured fix the way the old
 * fabricated 5.0 did." If the core ever tightens that gate below this value,
 * this must move with it (a `null`-accuracy import must always survive QC).
 */
private const val IMPORT_UNKNOWN_ACCURACY_M = 30.0

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
    // but the ffi panic firewall emits an OBJECT ({"kind":"panic",…}); e.g. a
    // truncated file that passes the magic sniff then panics inside fitparser.
    // Only the string form is user-facing copy; the panic object gets a
    // generic message.
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
                // Present only when the FIT recorded `record.gps_accuracy` (a
                // real device measurement, metres). Absent → null = unknown,
                // NOT a fabricated 5 m; importedRunEvent supplies the sentinel.
                accuracyM = o["accuracy_m"]?.jsonPrimitive?.doubleOrNull,
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
 * - Per-fix accuracy: a real figure ([GpxFix.accuracyM] vs FIT `gps_accuracy`
 *   or GPX `<hdop>`-derived) is passed through as-is; a fix with no accuracy
 *   info (TCX, GPX without `<hdop>`) gets [IMPORT_UNKNOWN_ACCURACY_M], a
 *   sentinel whose only contract is "passes the core's 30 m QC gate; provenance
 *   unknown", never treat it as a measurement.
 * - Decimated to [TRACK_DECIMATION_CAP] like a live save, keeping segment
 *   boundaries and endpoints, so one import can't bloat the append-only log.
 */
fun importedRunEvent(segments: List<List<GpxFix>>, measuredHrMax: Double?): Event.LogRunTrack {
    if (segments.sumOf { it.size } < 2) {
        throw GpxImportException("No GPS track found in this file")
    }
    // Drop untimestamped fixes at the SOURCE: a LEADING
    // observed_at=0 fix slips past the core's dt<=0 QC gate (the first point is
    // accepted unconditionally) and its ~1.7e9-second "leg" counts distance but
    // not moving time; silently fast pace, and a 1970 stamp on re-export. FIT
    // already drops these in fit.rs; mirror it for GPX/TCX here. Same choke
    // point drops out-of-range coordinates (a hand-edited lat="91.5" would
    // otherwise render as a real point). Deliberate side effect: HR averaging
    // below also counts only KEPT fixes; a fix untrusted for position/time
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
        // Real recorded accuracy passes through untouched; only a genuinely
        // unknown fix falls back to the sentinel (NOT a fabricated 5 m).
        GpsPoint(
            lat = f.lat,
            lon = f.lon,
            observedAt = f.timeSec,
            accuracyM = f.accuracyM ?: IMPORT_UNKNOWN_ACCURACY_M,
        )
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
