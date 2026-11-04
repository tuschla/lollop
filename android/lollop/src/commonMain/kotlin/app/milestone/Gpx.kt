package app.milestone

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

// Minimal GPX 1.0/1.1 track reader for two shell jobs: rendering a saved run's
// route (parsing the core-produced GPX from [CoreBridge.exportGpx] back into points) and
// importing a run recorded elsewhere (Garmin/Strava/Komoot exports). Reads only
// `<trk>/<trkseg>/<trkpt>`, lat/lon attributes, the optional `<time>`, an
// optional heart-rate sample from the common Garmin TrackPointExtension
// (`<gpxtpx:hr>`, matched by local name so any namespace prefix works), the
// track's `<type>` (Strava/Garmin name the sport there) and lollop's own
// `<trk><extensions><lollop:logged_at>` stamp. Each `<trk>` is one run. Everything
// else (waypoints, routes, metadata) is ignored.


/** Nominal 1-sigma GNSS range error (m) for `accuracy ≈ hdop × UERE`: a coarse
 *  estimate that only feeds the core's 30 m QC gate. */
private const val GPS_UERE_NOMINAL_M = 5.0

/** GPX `<hdop>` → estimated accuracy in metres ([GPS_UERE_NOMINAL_M]); null when
 *  missing, unparseable, non-finite or non-positive. */
internal fun hdopToAccuracyM(raw: String): Double? {
    val hdop = raw.trim().toDoubleOrNull() ?: return null
    if (!hdop.isFinite() || hdop <= 0.0) return null
    return hdop * GPS_UERE_NOMINAL_M
}

/**
 * One run read from an import file: one per GPX `<trk>`, per TCX `<Activity>`, or per FIT file.
 *
 * - [segments]: one list of fixes per recording segment; segments mark pauses.
 * - [sport]: the sport the file names for it, verbatim (TCX `<Activity Sport>`, GPX
 *   `<trk><type>`), or null when it names none. [importedRunEvent] refuses a
 *   recognised non-running sport ([nonRunSport]). A FIT file's sport is gated in
 *   the core, so FIT runs carry null.
 * - [loggedAtSec]: the run's logged-at stamp when the file is lollop's own GPX
 *   export (`<lollop:logged_at>`), else null. The stamp can sit after the last
 *   fix (the core's QC drops a standing-still finish; a trim cuts the end while
 *   the run keeps its stamp), and it is what identifies the run as already logged.
 */
internal data class ImportedRun(
    val segments: List<List<GpxFix>>,
    val sport: String? = null,
    val loggedAtSec: Long? = null,
)

/**
 * The non-running sport an import file names for an activity ([ImportedRun.sport]),
 * as plain lowercase words for [ImportFailure.NotARun]'s detail ("cycling", like the
 * core's FIT gate); null when it names a run or nothing this reader recognises.
 *
 * TCX's `Sport` is `Running`/`Biking`/`Other`, but a GPX `<type>` is free text in any
 * language ("running", "Ride", "trail_running", "Laufen", "1"). So, unlike the FIT
 * enum, only a recognised non-running sport is refused: an unknown label stays
 * importable (the implausible-pace gate still guards fast tracks), and a label that
 * names running in any word wins over everything else.
 */
internal fun nonRunSport(declared: String?): String? {
    val words = declared.orEmpty().lowercase().split(NON_WORD).filter { it.isNotEmpty() }
    if (words.any { "run" in it || "jog" in it }) return null
    fun named(test: (String) -> Boolean) = words.any(test)
    return when {
        named { "cycl" in it || it.startsWith("bik") || "bike" in it || "biking" in it || it in RIDE_WORDS } -> "cycling"
        named { it.startsWith("swim") } -> "swimming"
        named { it.startsWith("ski") || it.endsWith("ski") || "skiing" in it } -> "skiing"
        named { it.startsWith("hik") } -> "hiking"
        named { it.startsWith("walk") } -> "walking"
        else -> null
    }
}

private val NON_WORD = Regex("[^\\p{L}\\p{N}]+")

/** Strava's ride sport types no stem above catches, lowercased ("Ride",
 *  "VirtualRide", "GravelRide", "Velomobile"); the bike/cycle/ski compounds
 *  ("MountainBikeRide", "EBikeRide", "Handcycle", "NordicSki", "RollerSki")
 *  match the stems. */
private val RIDE_WORDS = setOf("ride", "virtualride", "gravelride", "velomobile")

/** One list of fixes per `<trkseg>` across all `<trk>`s; segments mark recording pauses. */
fun parseGpx(text: String): List<List<GpxFix>> = parseGpxRuns(text).flatMap { it.segments }

/** One [ImportedRun] per `<trk>` that holds fixes: its segments, its own `<type>` as
 *  [ImportedRun.sport] and lollop's `<lollop:logged_at>` stamp. [xml] opens the reader
 *  (the platform's; the host tests compare both). */
internal fun parseGpxRuns(text: String, xml: (String) -> ImportXmlReader = ::importXmlReader): List<ImportedRun> {
    val runs = mutableListOf<ImportedRun>()
    var segments = mutableListOf<List<GpxFix>>()
    var segment = mutableListOf<GpxFix>()
    // Trackpoint handling is gated on being INSIDE a `<trk>/<trkseg>` (mirrors
    // TCX's `inActivity` gate): a stray `<trkpt>` outside `<trk>` or after
    // `</trkseg>` would otherwise reuse the last finite lat/lon and append into
    // the already-committed segment; silently merging waypoints/route noise
    // into a recorded run.
    var inTrk = false
    var trkDepth = 0
    var inTrkseg = false
    var sport: String? = null
    var loggedAtSec: Long? = null
    var inTrkpt = false
    var lat = 0.0
    var lon = 0.0
    var timeSec = 0L
    var hr: Int? = null
    var accuracyM: Double? = null
    pullImportTags(text, "gpx", xml) { parser, event, name ->
        when (event) {
            ImportXmlEvent.StartTag -> when (name) {
                "trk" -> if (!inTrk) {
                    inTrk = true
                    trkDepth = parser.depth
                    segments = mutableListOf()
                    sport = null
                    loggedAtSec = null
                }
                // The track's own `<type>` only: a `<link>` inside `<trk>` has a
                // `<type>` (a MIME type) one level deeper, and so does a `<trkpt>`.
                "type" -> if (inTrk && parser.depth == trkDepth + 1) {
                    val declared = parser.nextText().trim()
                    if (sport == null && declared.isNotEmpty()) sport = declared
                }
                "logged_at" -> if (inTrk && !inTrkseg) {
                    loggedAtSec = parseImportTimeSec(parser.nextText()).takeIf { it > 0L }
                }
                "trkseg" -> if (inTrk) {
                    inTrkseg = true
                    segment = mutableListOf()
                }
                "trkpt" -> if (inTrkseg) {
                    inTrkpt = true
                    lat = parser.attribute("lat")?.toDoubleOrNull() ?: Double.NaN
                    lon = parser.attribute("lon")?.toDoubleOrNull() ?: Double.NaN
                    timeSec = 0L
                    hr = null
                    accuracyM = null
                }
                "time" -> if (inTrkpt) timeSec = parseImportTimeSec(parser.nextText())
                "hr" -> if (inTrkpt) hr = parseHrBpm(parser.nextText())
                // `<hdop>` is a direct child of `<trkpt>` in GPX 1.0/1.1.
                "hdop" -> if (inTrkpt) accuracyM = hdopToAccuracyM(parser.nextText())
            }
            ImportXmlEvent.EndTag -> when (name) {
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
                "trk" -> if (inTrk && parser.depth == trkDepth) {
                    inTrk = false
                    if (segments.isNotEmpty()) runs.add(ImportedRun(segments, sport, loggedAtSec))
                }
            }
            ImportXmlEvent.EndDocument -> Unit
        }
    }
    return runs
}

/** Deepest element nesting an import reads. A GPX or TCX track nests about ten
 *  levels deep (TCX `…/Trackpoint/Extensions/TPX/Speed`), but the platform pull
 *  parser's time grows super-linearly with depth (100 000 nested elements, a
 *  0.7 MB file, took 4.8 s; `ImportParserFuzzTest.deepNestingParsesInLinearTime`),
 *  so a deeper file is refused as damaged as soon as it gets there. */
internal const val MAX_IMPORT_XML_DEPTH = 64

/**
 * Pulls [text] through [onTag] with the reader [xml] opens: every start and end tag
 * with its local name (the prefix stripped, so `gpxtpx:hr` reads as `hr`). A file
 * nesting deeper than [MAX_IMPORT_XML_DEPTH] throws. A cut-off download often ends
 * right after a tag, and the pull parser then reaches the document's end without
 * throwing, so only a closed [root] proves the file is whole; anything else throws
 * too. Both throw the [ImportXmlException] the import maps to [ImportFailure.Damaged].
 */
internal inline fun pullImportTags(
    text: String,
    root: String,
    xml: (String) -> ImportXmlReader,
    onTag: (parser: ImportXmlReader, event: ImportXmlEvent, name: String) -> Unit,
) {
    val parser = xml(text)
    var rootClosed = false
    var event = parser.next()
    while (event != ImportXmlEvent.EndDocument) {
        if (event == ImportXmlEvent.StartTag && parser.depth > MAX_IMPORT_XML_DEPTH) {
            throw ImportXmlException("elements nest deeper than $MAX_IMPORT_XML_DEPTH levels")
        }
        val name = parser.name.substringAfterLast(':')
        if (event == ImportXmlEvent.EndTag && name == root && parser.depth == 1) rootClosed = true
        onTag(parser, event, name)
        event = parser.next()
    }
    if (!rootClosed) throw ImportXmlException("Import ends before </$root>: truncated")
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
 * ISO-8601 `<time>`/`<Time>` → unix seconds; 0 when missing or unparseable (a
 * route-only GPX). Shared by the GPX and TCX readers. Accepts `Z`, `+02:00`, `+02` and
 * `+0200` offsets. GPX and TCX require UTC, so a zone-naive stamp
 * (`YYYY-MM-DDThh:mm:ss`, written by some exporters) is read as UTC.
 *
 * Reads exactly what the JVM's java.time parsers this replaced accepted, tried in
 * turn (`ImportTimeParityTest` checks it against them): `Instant.parse`
 * (seconds required; `24:00:00` is the next midnight, `23:59:60` reads as `:59`),
 * `OffsetDateTime.parse` (strict; `Z`, `+HH`, `+HH:MM`, `+HH:MM:ss`),
 * `LocalDateTime.parse` (strict; no offset, read as UTC), and a colon-less offset
 * (`Z` or `+HHMM`) resolved smartly (a day past the month's end is its last day,
 * `24:00` the next midnight). Case-insensitive `T`/`Z`; fractions of up to nine
 * digits are dropped.
 */
internal fun parseImportTimeSec(raw: String): Long {
    val s = raw.trim()
    if (s.isEmpty()) return 0L
    val t = readIsoStamp(s) ?: return 0L
    return t.asInstant() ?: t.asOffsetDateTime() ?: t.asLocalDateTime() ?: t.asColonlessOffset() ?: 0L
}

/** How an [IsoStamp] names its offset. */
private enum class IsoOffset { None, Z, Hours, HoursMinutes, HoursMinutesSeconds, Colonless }

/** The fields of one ISO-8601 date-time as written, before any range check. */
private class IsoStamp(
    val year: Long,
    val month: Int,
    val day: Int,
    val hour: Int,
    val minute: Int,
    /** Null when the stamp has no seconds. */
    val second: Int?,
    /** No fraction, or one of only zeros. */
    val wholeSecond: Boolean,
    val offset: IsoOffset,
    val offsetSec: Int,
)

/** Unix seconds as `Instant.parse` reads [this] (ISO_INSTANT). */
private fun IsoStamp.asInstant(): Long? {
    if (second == null || offset == IsoOffset.None || offset == IsoOffset.Hours || offset == IsoOffset.Colonless) return null
    var h = hour
    var sec: Int = second
    var days = 0L
    if (h == 24 && minute == 0 && sec == 0 && wholeSecond) {
        h = 0
        days = 1L
    } else if (h == 23 && minute == 59 && sec == 60) {
        sec = 59
    }
    if (month !in 1..12 || day !in 1..monthLength(year, month) || h !in 0..23 || minute !in 0..59 || sec !in 0..59) return null
    val epoch = (isoEpochDay(year, month, day) + days) * 86_400L + h * 3600L + minute * 60L + sec - offsetSec
    return epoch.takeIf { it in INSTANT_MIN_SEC..INSTANT_MAX_SEC }
}

/** Unix seconds as `OffsetDateTime.parse` reads [this] (ISO_OFFSET_DATE_TIME, strict). */
private fun IsoStamp.asOffsetDateTime(): Long? {
    if (offset == IsoOffset.None || offset == IsoOffset.Colonless) return null
    return strictLocalSec()?.minus(offsetSec)
}

/** Unix seconds as `LocalDateTime.parse` reads [this] (strict), taken as UTC. */
private fun IsoStamp.asLocalDateTime(): Long? = if (offset == IsoOffset.None) strictLocalSec() else null

/** Unix seconds as an ISO local date-time with an `+HHMM` / `Z` offset reads [this],
 *  resolved smartly: a day past the month's end is its last day, `24:00` the next
 *  midnight. */
private fun IsoStamp.asColonlessOffset(): Long? {
    if (offset != IsoOffset.Z && offset != IsoOffset.Colonless) return null
    if (year !in ISO_YEARS || month !in 1..12 || day !in 1..31 || minute !in 0..59) return null
    val sec = second ?: 0
    var h = hour
    var days = 0L
    if (h == 24 && minute == 0 && sec == 0 && wholeSecond) {
        h = 0
        days = 1L
    } else if (h !in 0..23 || sec !in 0..59) {
        return null
    }
    val dom = minOf(day, monthLength(year, month))
    if (days == 1L && year == ISO_YEARS.last && month == 12 && dom == 31) return null
    return (isoEpochDay(year, month, dom) + days) * 86_400L + h * 3600L + minute * 60L + sec - offsetSec
}

/** [this] as strictly valid local date-time fields, in seconds since 1970-01-01T00:00. */
private fun IsoStamp.strictLocalSec(): Long? {
    val sec = second ?: 0
    if (year !in ISO_YEARS || month !in 1..12 || day !in 1..monthLength(year, month)) return null
    if (hour !in 0..23 || minute !in 0..59 || sec !in 0..59) return null
    return isoEpochDay(year, month, day) * 86_400L + hour * 3600L + minute * 60L + sec
}

/** java.time's year range (`Year.MIN_VALUE..Year.MAX_VALUE`). */
private val ISO_YEARS = -999_999_999L..999_999_999L

/** `Instant.MIN` / `Instant.MAX` in epoch seconds. */
private const val INSTANT_MIN_SEC = -31_557_014_167_219_200L
private const val INSTANT_MAX_SEC = 31_556_889_864_403_199L

private fun isLeapYear(year: Long): Boolean = year % 4 == 0L && (year % 100 != 0L || year % 400 == 0L)

private fun monthLength(year: Long, month: Int): Int = when (month) {
    2 -> if (isLeapYear(year)) 29 else 28
    4, 6, 9, 11 -> 30
    else -> 31
}

/** Days from 1970-01-01 to the proleptic Gregorian [year]-[month]-[day]. */
private fun isoEpochDay(year: Long, month: Int, day: Int): Long {
    val y = if (month <= 2) year - 1 else year
    val era = y.floorDiv(400L)
    val yearOfEra = y - era * 400
    val dayOfYear = (153 * ((month + 9) % 12) + 2) / 5 + day - 1
    val dayOfEra = yearOfEra * 365 + yearOfEra / 4 - yearOfEra / 100 + dayOfYear
    return era * 146_097 + dayOfEra - 719_468
}

/**
 * The fields of [s] when it has java.time's ISO date-time shape, else null: a year of
 * exactly four digits, or a sign and up to ten (`+` needs five or more; `-0000` is
 * refused), `-MM-dd`, `T` (any case), `HH:mm`, optional `:ss` with an optional
 * fraction of up to nine digits (a bare `.` included), then nothing, `Z` (any case),
 * `±HH`, `±HH:MM`, `±HH:MM:ss` or `±HHMM`. An offset field past its range (hours past
 * 23, minutes or seconds past 59) or beyond ±18:00 refuses the stamp.
 */
private fun readIsoStamp(s: String): IsoStamp? {
    var i = 0
    fun digits(n: Int): Int? {
        if (i + n > s.length) return null
        var v = 0
        for (k in i until i + n) {
            val c = s[k]
            if (c !in '0'..'9') return null
            v = v * 10 + (c - '0')
        }
        i += n
        return v
    }
    fun literal(c: Char, ignoreCase: Boolean = false): Boolean =
        (i < s.length && s[i].equals(c, ignoreCase)).also { if (it) i++ }

    val negativeYear = literal('-')
    val positiveYear = !negativeYear && literal('+')
    val yearStart = i
    while (i < s.length && i - yearStart < 10 && s[i] in '0'..'9') i++
    val yearDigits = i - yearStart
    if (yearDigits < 4 || (positiveYear && yearDigits == 4) || (!positiveYear && !negativeYear && yearDigits > 4)) return null
    val yearValue = s.substring(yearStart, i).toLong()
    if (negativeYear && yearValue == 0L) return null
    val year = if (negativeYear) -yearValue else yearValue
    if (!literal('-')) return null
    val month = digits(2) ?: return null
    if (!literal('-')) return null
    val day = digits(2) ?: return null
    if (!literal('T', ignoreCase = true)) return null
    val hour = digits(2) ?: return null
    if (!literal(':')) return null
    val minute = digits(2) ?: return null
    var second: Int? = null
    var wholeSecond = true
    if (literal(':')) {
        second = digits(2) ?: return null
        if (literal('.')) {
            val fractionStart = i
            while (i < s.length && i - fractionStart < 9 && s[i] in '0'..'9') i++
            wholeSecond = (fractionStart until i).all { s[it] == '0' }
        }
    }
    var offset = IsoOffset.None
    var offsetSec = 0
    if (literal('Z', ignoreCase = true)) {
        offset = IsoOffset.Z
    } else if (i < s.length && (s[i] == '+' || s[i] == '-')) {
        val sign = if (s[i] == '-') -1 else 1
        i++
        val h = digits(2)?.takeIf { it <= 23 } ?: return null
        var m = 0
        var sec = 0
        offset = when {
            literal(':') -> {
                m = digits(2)?.takeIf { it <= 59 } ?: return null
                if (literal(':')) {
                    sec = digits(2)?.takeIf { it <= 59 } ?: return null
                    IsoOffset.HoursMinutesSeconds
                } else {
                    IsoOffset.HoursMinutes
                }
            }
            i < s.length -> {
                m = digits(2)?.takeIf { it <= 59 } ?: return null
                IsoOffset.Colonless
            }
            else -> IsoOffset.Hours
        }
        offsetSec = sign * (h * 3600 + m * 60 + sec)
        if (offsetSec !in -18 * 3600..18 * 3600) return null
    }
    if (i != s.length) return null
    return IsoStamp(year, month, day, hour, minute, second, wholeSecond, offset, offsetSec)
}


/** Why one imported file (or one activity inside it) produced no run. The shell
 *  maps each to plain-language copy beside the file's name. */
internal enum class ImportFailure {
    ReadFailed, TooLarge, WrongType, NoTrack, TooShort, NoTimestamps, Damaged, NotARun, ImplausiblePace, Rejected, Unknown,
}

/** An import that produced no event, with its [reason]. [detail] carries the one
 *  variable part a reason needs (the activity's sport for [ImportFailure.NotARun]). */
internal class ImportException(val reason: ImportFailure, val detail: String? = null, cause: Throwable? = null) :
    Exception(detail?.let { "$reason: $it" } ?: reason.name, cause)

/** FIT sniff: the 12/14-byte FIT header carries ".FIT" at offset 8. */
fun isFitFile(bytes: ByteArray): Boolean =
    bytes.size >= 12 &&
        bytes[8] == '.'.code.toByte() && bytes[9] == 'F'.code.toByte() &&
        bytes[10] == 'I'.code.toByte() && bytes[11] == 'T'.code.toByte()

/**
 * Decode the core's [DurableCore.parseFitFile] JSON into the same segment shape
 * [parseGpx]/[parseTcxActivities] produce, so all three formats share [importedRunEvent].
 *
 * Error envelopes: the object form `{"error":{"kind":…,"context":…}}` names why
 * (`not_a_run` with the lowercase sport as context: a cycling file is refused, never
 * logged as running volume; `no_track` with context `too_short`: a fragment under the
 * keep threshold, [MIN_RUN_KM] or [MIN_RUN_SEC]); the legacy plain-string form and the
 * panic firewall's object read as a damaged file. A null [json] (the JNI returns null
 * when it cannot allocate the result string) reads as damaged too.
 */
internal fun fitSegments(json: String?): List<List<GpxFix>> {
    val root = Json
        .parseToJsonElement(json ?: throw ImportException(ImportFailure.Damaged))
        .jsonObject
    root["error"]?.let { err ->
        val obj = err as? JsonObject
        val kind = obj?.get("kind")?.jsonPrimitive?.contentOrNull
        val context = obj?.get("context")?.jsonPrimitive?.contentOrNull?.takeIf { it.isNotBlank() }
        throw when (kind) {
            "not_a_run" -> ImportException(ImportFailure.NotARun, context)
            "no_track" -> ImportException(if (context == "too_short") ImportFailure.TooShort else ImportFailure.NoTrack)
            "not_fit" -> ImportException(ImportFailure.WrongType)
            else -> ImportException(ImportFailure.Damaged)
        }
    }
    val segments = root["segments"] ?: throw ImportException(ImportFailure.Damaged)
    return segments.jsonArray.map { seg ->
        seg.jsonArray.map { p ->
            val o = p.jsonObject
            GpxFix(
                lat = o.getValue("lat").jsonPrimitive.double,
                lon = o.getValue("lon").jsonPrimitive.double,
                timeSec = o.getValue("time_sec").jsonPrimitive.long,
                hrBpm = o["hr_bpm"]?.jsonPrimitive?.intOrNull?.takeIf { it in PLAUSIBLE_HR_BPM },
                accuracyM = o["accuracy_m"]?.jsonPrimitive?.doubleOrNull,
            )
        }
    }
}

/** Fastest believable average moving pace for an imported run, seconds per km
 *  (2:50 /km): anything faster is a bike ride, a car trip or a
 *  broken clock, never running volume. */
private const val FASTEST_PLAUSIBLE_SEC_PER_KM = 170.0

/**
 * A track's legs within each segment ([legTotals]; a pause bridge counts none): GPS
 * distance, recorded time and moving time (legs at or above [MIN_MOVING_SPEED_MPS]),
 * as the core's FIT gate and moving time measure them.
 */
internal class LegTotals(val km: Double, val sec: Long, val movingSec: Long) {
    /** Moving pace faster than [FASTEST_PLAUSIBLE_SEC_PER_KM]: not running volume. */
    val implausiblyFast: Boolean
        get() = km > 0.0 && movingSec > 0L && movingSec / km < FASTEST_PLAUSIBLE_SEC_PER_KM

    /** A fragment under the keep threshold ([isShortRun]). One gate for GPX, TCX and FIT. */
    val tooShort: Boolean get() = isShortRun(km, sec)
}

/** [LegTotals] of [points]; [segmentStarts] are the indices that begin a new segment. */
internal fun legTotals(points: List<GpsPoint>, segmentStarts: Collection<Int>): LegTotals {
    val starts = segmentStarts.toHashSet()
    var km = 0.0
    var sec = 0L
    var movingSec = 0L
    for (i in 1 until points.size) {
        if (i in starts) continue
        val dt = points[i].observedAt - points[i - 1].observedAt
        if (dt <= 0L) continue
        val legKm = segmentKm(points[i - 1], points[i])
        km += legKm
        sec += dt
        if (movingLegKm(legKm, dt.toDouble()) != null) movingSec += dt
    }
    return LegTotals(km, sec, movingSec)
}

/** How far after an imported track's last fix its file-named logged-at stamp
 *  ([ImportedRun.loggedAtSec]) may sit and still be believed: a day, longer than
 *  any trimmed-off finish or standing-still tail of one run. */
private const val MAX_LOGGED_AT_AFTER_LAST_FIX_SEC = 24L * 3600

/** The most fixes in a row [dropGnssExcursions] drops as one dropout. */
private const val MAX_GNSS_EXCURSION_FIXES = 30

/** Implied ground speed (m/s) from [a] to [b], or null when [b] isn't stamped
 *  after [a] (no speed). */
private fun speedMps(a: GpxFix, b: GpxFix): Double? {
    val dt = b.timeSec - a.timeSec
    return if (dt > 0) haversineM(a.lat, a.lon, b.lat, b.lon) / dt else null
}

/**
 * [segment] without its GNSS dropouts: fixes the core's QC drops as jumps (an
 * implied speed over [MAX_PLAUSIBLE_SPEED_MPS] from the last kept fix,
 * `load::gps_speed_plausible`) when the track comes back, i.e. a later fix (at
 * most [MAX_GNSS_EXCURSION_FIXES] on) is plausible from that kept fix again. One
 * multipath or no-fix teleport then never makes the whole run "implausibly fast".
 * A jump the track never returns from (a car ride, a relocation) is kept, so the
 * pace gate ([LegTotals.implausiblyFast]) still judges it.
 */
internal fun dropGnssExcursions(segment: List<GpxFix>): List<GpxFix> {
    if (segment.size < 3) return segment
    val out = ArrayList<GpxFix>(segment.size)
    out.add(segment[0])
    var i = 1
    while (i < segment.size) {
        val kept = out.last()
        val jumps = speedMps(kept, segment[i])?.let { it > MAX_PLAUSIBLE_SPEED_MPS } == true
        val back = if (!jumps) {
            null
        } else {
            (i + 1 until minOf(segment.size, i + 1 + MAX_GNSS_EXCURSION_FIXES)).firstOrNull { k ->
                speedMps(kept, segment[k])?.let { it <= MAX_PLAUSIBLE_SPEED_MPS } == true
            }
        }
        if (back != null) {
            i = back
        } else {
            out.add(segment[i])
            i++
        }
    }
    return out
}

/**
 * Build the [Event.LogRunTrack] for an imported run. The core stays
 * the single source of record: it derives distance/duration/pace/splits/zone
 * from the points exactly as it does for a live-tracked run; the shell only
 * reshapes the file.
 *
 * - Refused ([ImportFailure.NotARun]) when the file names another sport
 *   ([nonRunSport]), checked first, as the core's FIT gate does.
 * - `hr_pct_max` is computed ONLY when the file carries HR samples AND the
 *   profile has a MEASURED HRmax, never estimated shell-side (the Tanaka
 *   estimate is core logic). Otherwise 0.0 = "no HR", same as the tracker.
 * - Per-fix accuracy: [GpxFix.accuracyM] when the file names one, else
 *   [IMPORT_UNKNOWN_ACCURACY_M].
 * - Decimated to [TRACK_DECIMATION_CAP] like a live save, keeping segment
 *   boundaries and endpoints, so one import can't bloat the append-only log.
 * - Refused ([ImportFailure.ImplausiblePace]) when its moving pace is faster than
 *   any run, and ([ImportFailure.TooShort]) when it is a fragment under the keep
 *   threshold ([LegTotals]).
 * - Stamped at the file's own logged-at stamp ([ImportedRun.loggedAtSec]) when it
 *   names a believable one, else at its last fix.
 */
internal fun importedRunEvent(run: ImportedRun, measuredHrMax: Double?): Event.LogRunTrack {
    nonRunSport(run.sport)?.let { throw ImportException(ImportFailure.NotARun, it) }
    val segments = run.segments
    if (segments.sumOf { it.size } < 2) {
        throw ImportException(ImportFailure.NoTrack)
    }
    // Drop untimed fixes (a leading observed_at=0 fix slips past the core's dt<=0
    // QC and its ~1.7e9 s leg adds distance without moving time), out-of-range
    // coordinates and GNSS dropouts ([dropGnssExcursions]), so the gates below
    // judge the legs the core keeps. HR averaging counts only the kept fixes too.
    val timed = segments
        .map { seg ->
            dropGnssExcursions(seg.filter { it.timeSec > 0 && abs(it.lat) <= 90.0 && abs(it.lon) <= 180.0 })
        }
        .filter { it.isNotEmpty() }
    val all = timed.flatten()
    if (all.size < 2) {
        throw ImportException(ImportFailure.NoTimestamps)
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

    val keep = decimationKeep(all.size, TRACK_DECIMATION_CAP, boundaryIdx)
    val points = keep.map { i ->
        val f = all[i]
        GpsPoint(
            lat = f.lat,
            lon = f.lon,
            observedAt = f.timeSec,
            accuracyM = f.accuracyM ?: IMPORT_UNKNOWN_ACCURACY_M,
        )
    }
    val segmentStarts = remapStarts(keep, boundaryIdx)
    val legs = legTotals(points, segmentStarts)
    if (legs.implausiblyFast) throw ImportException(ImportFailure.ImplausiblePace)
    if (legs.tooShort) throw ImportException(ImportFailure.TooShort)

    // Time-weighted %HRmax: each kept HR sample weighs the seconds until the next
    // kept fix (the last one the median gap), so a brief spike counts briefly.
    // Clamped at 100 %: a stale low measured HRmax can push the average above it.
    val hrPctMax = if (all.any { it.hrBpm != null } && measuredHrMax != null && measuredHrMax > 0.0) {
        val fixGaps = all.zipWithNext { a, b -> (b.timeSec - a.timeSec).toDouble().coerceAtLeast(0.0) }
        val medianGap = fixGaps.sorted().let { s ->
            val m = s.size
            if (m % 2 == 1) s[m / 2] else (s[m / 2 - 1] + s[m / 2]) / 2.0
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
        // Explicit unique id so a multi-activity TCX/GPX import (several runs built
        // in a tight loop) never collides on same-millisecond default ids.
        entryId = EntryIds.next(),
        // Logged-at = when the run happened, not when it was imported: history
        // sorts and displays by this, and the batch dedupe matches on it. A
        // lollop export names the run's own stamp, which may sit after the last
        // kept fix; anything else (or a stamp before the track ends, or more than
        // a day after it) is stamped at the last fix.
        observedAt = run.loggedAtSec
            ?.takeIf { it in all.last().timeSec..all.last().timeSec + MAX_LOGGED_AT_AFTER_LAST_FIX_SEC }
            ?: all.last().timeSec,
        workoutType = null,
        segmentStarts = segmentStarts,
    )
}
