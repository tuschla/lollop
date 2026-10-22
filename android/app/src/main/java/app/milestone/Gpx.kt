package app.milestone

import android.util.Xml
import kotlin.math.abs
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.contentOrNull
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
import java.time.LocalDateTime
import java.time.ZoneOffset

// Minimal GPX 1.0/1.1 track reader for two shell jobs: rendering a saved run's
// route (parsing the core-produced GPX from [Core.exportGpx] back into points) and
// importing a run recorded elsewhere (Garmin/Strava/Komoot exports). Reads only
// `<trk>/<trkseg>/<trkpt>`, lat/lon attributes, the optional `<time>`, and an
// optional heart-rate sample from the common Garmin TrackPointExtension
// (`<gpxtpx:hr>`, matched by local name so any namespace prefix works).
// Everything else (waypoints, routes, metadata) is ignored.

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
                "time" -> if (inTrkpt) timeSec = parseImportTimeSec(parser.nextText())
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

/** Physiologically plausible heart-rate samples (bpm) an import keeps. */
private val PLAUSIBLE_HR_BPM = 20..250

/**
 * A heart-rate sample from an imported file, kept only when finite and
 * physiologically plausible ([PLAUSIBLE_HR_BPM]). Rejects NaN / `Infinity` (a literal
 * `Infinity` token would otherwise reach the JSON wire as a bare token → the
 * whole event is serde-rejected and the import silently logs nothing) and
 * absurd magnitudes (e.g. 1e9 bpm poisoning TRIMP/CTL/ATL). Shared by the GPX
 * and TCX readers so both guard identically.
 */
internal fun parseHrBpm(raw: String): Int? {
    val v = raw.trim().toDoubleOrNull() ?: return null
    if (!v.isFinite() || v < PLAUSIBLE_HR_BPM.first || v > PLAUSIBLE_HR_BPM.last) return null
    return v.toInt()
}

/**
 * ISO-8601 `<time>`/`<Time>` → unix seconds; 0 when missing/unparseable (a
 * route-only GPX with no timestamps). Shared by the GPX and TCX readers so both
 * decode timestamps identically.
 *
 * GPX 1.1 and TCX both REQUIRE the timestamp in UTC (xsd:dateTime, conventionally
 * `…Z`), so [Instant]/[OffsetDateTime] cover the spec-conformant forms. But some
 * exporters emit a zone-NAIVE stamp (`YYYY-MM-DDThh:mm:ss`, no `Z` and no offset),
 * which parses under neither; discarding it would make a genuinely recorded run
 * look time-less and be misdiagnosed downstream as "a planned route, not a recorded
 * run". So a naive stamp is read as UTC, per the spec.
 */
internal fun parseImportTimeSec(raw: String): Long {
    val s = raw.trim()
    if (s.isEmpty()) return 0L
    return runCatching { Instant.parse(s).epochSecond }
        .recoverCatching { OffsetDateTime.parse(s).toEpochSecond() }
        .recoverCatching { LocalDateTime.parse(s).toInstant(ZoneOffset.UTC).epochSecond }
        .getOrDefault(0L)
}

/**
 * Accuracy (metres) stamped on an imported fix whose source file reported NO
 * accuracy at all (TCX always; GPX without `<hdop>`). It is a SENTINEL, not a
 * measurement: its one and only contract is "sits at the core's 30 m QC drop
 * gate (`running::MAX_GPS_ACCURACY_M` = 30.0) so an imported track is never rejected
 * for unknown accuracy, while being pinned to the WORST value that still passes
 * so any future accuracy-weighting feature treats it as the least-trusted
 * acceptable fix, never mistakes it for a good measured fix." If the core ever
 * tightens that gate below this value,
 * this must move with it (a `null`-accuracy import must always survive QC).
 */
internal const val IMPORT_UNKNOWN_ACCURACY_M = 30.0

/** Why one imported file (or one activity inside it) produced no run. The shell
 *  maps each to plain-language copy beside the file's name. */
internal enum class ImportFailure {
    ReadFailed, TooLarge, WrongType, NoTrack, TooShort, NoTimestamps, Damaged, NotARun, ImplausiblePace, Rejected, Unknown,
}

/** An import that produced no event, with its [reason]. [detail] carries the one
 *  variable part a reason needs (the activity's sport for [ImportFailure.NotARun]). */
internal class GpxImportException(val reason: ImportFailure, val detail: String? = null) :
    Exception(detail?.let { "$reason: $it" } ?: reason.name)

/** FIT sniff: the 12/14-byte FIT header carries ".FIT" at offset 8. */
fun isFitFile(bytes: ByteArray): Boolean =
    bytes.size >= 12 &&
        bytes[8] == '.'.code.toByte() && bytes[9] == 'F'.code.toByte() &&
        bytes[10] == 'I'.code.toByte() && bytes[11] == 'T'.code.toByte()

/**
 * Decode the core's [Core.parseFit] JSON into the same segment shape
 * [parseGpx]/[parseTcxActivities] produce, so all three formats share [importedRunEvent].
 *
 * Error envelopes: the object form `{"error":{"kind":…,"context":…}}` names why
 * (`not_a_run` with the lowercase sport as context: a cycling file is refused, never
 * logged as running volume; `no_track` with context `too_short`: a fragment under the
 * keep threshold, [MIN_RUN_KM] or [MIN_RUN_SEC]); the legacy plain-string form and the
 * panic firewall's object read as a damaged file. `json` is nullable: the JNI returns
 * a null jstring
 * on a double result-string allocation failure, read like a damaged file.
 */
internal fun fitSegments(json: String?): List<List<GpxFix>> {
    val root = Json
        .parseToJsonElement(json ?: throw GpxImportException(ImportFailure.Damaged))
        .jsonObject
    root["error"]?.let { err ->
        val obj = err as? JsonObject
        val kind = obj?.get("kind")?.jsonPrimitive?.contentOrNull
        val context = obj?.get("context")?.jsonPrimitive?.contentOrNull?.takeIf { it.isNotBlank() }
        throw when (kind) {
            "not_a_run" -> GpxImportException(ImportFailure.NotARun, context)
            "no_track" -> GpxImportException(if (context == "too_short") ImportFailure.TooShort else ImportFailure.NoTrack)
            "not_fit" -> GpxImportException(ImportFailure.WrongType)
            else -> GpxImportException(ImportFailure.Damaged)
        }
    }
    val segments = root["segments"] ?: throw GpxImportException(ImportFailure.Damaged)
    return segments.jsonArray.map { seg ->
        seg.jsonArray.map { p ->
            val o = p.jsonObject
            GpxFix(
                lat = o.getValue("lat").jsonPrimitive.double,
                lon = o.getValue("lon").jsonPrimitive.double,
                timeSec = o.getValue("time_sec").jsonPrimitive.long,
                hrBpm = o["hr_bpm"]?.jsonPrimitive?.intOrNull?.takeIf { it in PLAUSIBLE_HR_BPM },
                // Present only when the FIT recorded `record.gps_accuracy` (a
                // real device measurement, metres). Absent → null = unknown,
                // NOT a fabricated 5 m; importedRunEvent supplies the sentinel.
                accuracyM = o["accuracy_m"]?.jsonPrimitive?.doubleOrNull,
            )
        }
    }
}

/** Fastest believable average moving pace for an imported run, seconds per km
 *  (2:50 /km): anything faster is a bike ride, a car trip or a
 *  broken clock, never running volume. */
private const val FASTEST_PLAUSIBLE_SEC_PER_KM = 170.0

/** A leg slower than this counts as standing still: File 07's auto-pause rule, the
 *  same gate the core's moving time uses (`load::is_stopped`). Shared with the trim
 *  preview's moving time ([timedTrackFromSegments]). */
internal const val STOPPED_SPEED_MPS = 0.5

/**
 * Whether a track's average MOVING pace (distance over the time spent moving, legs
 * within each segment only, mirroring the core's moving time) is faster than
 * [FASTEST_PLAUSIBLE_SEC_PER_KM]. [segmentStarts] are indices that begin a new
 * segment: the pause-bridge leg into them is never counted.
 */
internal fun isImplausiblyFast(points: List<GpsPoint>, segmentStarts: Collection<Int>): Boolean {
    val starts = segmentStarts.toHashSet()
    var km = 0.0
    var movingSec = 0.0
    for (i in 1 until points.size) {
        if (i in starts) continue
        val dt = (points[i].observedAt - points[i - 1].observedAt).toDouble()
        if (dt <= 0.0) continue
        val legKm = segmentKm(points[i - 1], points[i])
        km += legKm
        if (legKm * 1000.0 / dt >= STOPPED_SPEED_MPS) movingSec += dt
    }
    return km > 0.0 && movingSec > 0.0 && movingSec / km < FASTEST_PLAUSIBLE_SEC_PER_KM
}

/**
 * Whether a track is a fragment under the keep threshold ([isShortRun]: [MIN_RUN_KM]
 * or [MIN_RUN_SEC]), measured as the core's FIT gate does: GPS distance and recorded
 * time summed over the legs within each segment ([segmentStarts] begin a new one, so a
 * pause bridge counts neither). One gate for GPX, TCX and FIT alike.
 */
internal fun isTooShortForARun(points: List<GpsPoint>, segmentStarts: Collection<Int>): Boolean {
    val starts = segmentStarts.toHashSet()
    var km = 0.0
    var sec = 0L
    for (i in 1 until points.size) {
        if (i in starts) continue
        val dt = points[i].observedAt - points[i - 1].observedAt
        if (dt <= 0L) continue
        km += segmentKm(points[i - 1], points[i])
        sec += dt
    }
    return isShortRun(km, sec)
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
 * - Refused ([ImportFailure.ImplausiblePace]) when its moving pace is faster than
 *   any run ([isImplausiblyFast]), and ([ImportFailure.TooShort]) when it is a
 *   fragment under the keep threshold ([isTooShortForARun]).
 */
fun importedRunEvent(segments: List<List<GpxFix>>, measuredHrMax: Double?): Event.LogRunTrack {
    if (segments.sumOf { it.size } < 2) {
        throw GpxImportException(ImportFailure.NoTrack)
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
                it.timeSec > 0 && abs(it.lat) <= 90.0 && abs(it.lon) <= 180.0
            }
        }
        .filter { it.isNotEmpty() }
    val all = timed.flatten()
    if (all.size < 2) {
        throw GpxImportException(ImportFailure.NoTimestamps)
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
    if (isImplausiblyFast(points, segmentStarts)) throw GpxImportException(ImportFailure.ImplausiblePace)
    if (isTooShortForARun(points, segmentStarts)) throw GpxImportException(ImportFailure.TooShort)

    // Time-weighted %HRmax: weight each kept HR sample by the seconds until the
    // NEXT kept fix so a reading held for minutes dominates a brief spike a plain
    // mean would over-count equally; the final fix (no following fix to bound it)
    // is weighted by the median inter-fix gap. Evenly spaced fixes give equal
    // weights, so the result matches a plain mean. A stale/low
    // measured_hr_max paired with a hard-effort file can push the average above
    // 100 % (physiologically impossible); clamp at the source so a 104.7 % never
    // rides the wire. The core independently re-clamps (it cannot trust any one
    // shell's import path), but clamping here keeps the persisted event honest too.
    val hrPctMax = if (all.any { it.hrBpm != null } && measuredHrMax != null && measuredHrMax > 0.0) {
        val fixGaps = all.zipWithNext { a, b -> (b.timeSec - a.timeSec).toDouble().coerceAtLeast(0.0) }
        val medianGap = if (fixGaps.isEmpty()) {
            1.0
        } else {
            val sorted = fixGaps.sorted()
            val m = sorted.size
            if (m % 2 == 1) sorted[m / 2] else (sorted[m / 2 - 1] + sorted[m / 2]) / 2.0
        }
        var num = 0.0
        var den = 0.0
        for (i in all.indices) {
            val hr = all[i].hrBpm ?: continue
            val w = if (i < all.lastIndex) fixGaps[i] else medianGap
            num += hr * w
            den += w
        }
        val avg = if (den > 0.0) num / den else all.mapNotNull { it.hrBpm }.average()
        (avg / measuredHrMax * 100.0).coerceAtMost(100.0)
    } else {
        0.0
    }

    return Event.LogRunTrack(
        points = points,
        hrPctMax = hrPctMax,
        longestRecentKm = 0.0,
        // Explicit unique id so a multi-activity TCX/GPX import (several runs built
        // in a tight loop) never collides on same-millisecond default ids.
        entryId = EntryIds.next(),
        // Logged-at = when the run happened (last fix), not when it was
        // imported: history sorts and displays by this.
        observedAt = all.last().timeSec,
        workoutType = null,
        segmentStarts = segmentStarts,
    )
}
