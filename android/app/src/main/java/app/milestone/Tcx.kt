package app.milestone

import android.util.Xml
import org.xmlpull.v1.XmlPullParser
import java.io.StringReader
import java.time.Instant
import java.time.OffsetDateTime

/**
 * Minimal TCX (Garmin Training Center XML) track reader for run import
 * (Garmin/Strava/etc. exports that ship TCX instead of GPX). Reads only
 * `<Activities>/<Activity>/<Lap>/<Track>/<Trackpoint>`, each Trackpoint's
 * `<Time>`, `<Position>` (`LatitudeDegrees`/`LongitudeDegrees`), and an
 * optional `<HeartRateBpm><Value>` sample. Tag names are matched by local
 * name (namespace prefix stripped) so any TCX namespace declaration works.
 * Everything else (Lap summary stats, Extensions, Author, Creator, Course)
 * is ignored. Reuses [GpxFix], same shape as the GPX reader, only the
 * source markup differs.
 */

/**
 * One list of fixes per recording SEGMENT, grouped by the enclosing
 * `<Activity>`. A standard Garmin/Strava TCX opens a new `<Track>` per `<Lap>`
 * - including AUTOLAPS on continuous running, so a `<Track>` boundary is NOT
 * inherently a pause. Consecutive `<Track>`s whose inter-track time gap is
 * small (< [MAX_TRACK_MERGE_GAP_SEC], the same convention as the live
 * pipeline's fix-gap threshold) are therefore MERGED into one segment; only a
 * genuine pause (a larger gap, or a clock discontinuity) splits segments. This
 * fixes a systematic undercount: the core drops the entering leg of every
 * segment boundary from BOTH distance and moving time (~−130 m / −40 s on an
 * autolap-per-km marathon) if each lap became its own segment. A TCX export can
 * also bundle several `<Activity>` blocks (e.g. a multi-sport day), each is a
 * separate run, not a pause inside one, so callers that need one import per run
 * should use [parseTcxActivities] instead of flattening this. Trackpoints
 * without a `<Position>` (TCX emits these during pauses) are skipped rather
 * than producing a fix with missing coordinates.
 */
fun parseTcx(text: String): List<List<GpxFix>> = parseTcxActivities(text).flatten()

/**
 * Same trackpoint reader as [parseTcx], but keeping the `<Activity>`
 * grouping: outer list per `<Activity>`, inner list per `<Track>` within it.
 * Import call sites use this to turn each Activity into its own run event
 * instead of flattening a multi-activity export into one.
 */
fun parseTcxActivities(text: String): List<List<List<GpxFix>>> {
    val parser = Xml.newPullParser().apply {
        setFeature(XmlPullParser.FEATURE_PROCESS_NAMESPACES, false)
        setInput(StringReader(text))
    }
    val activities = mutableListOf<List<List<GpxFix>>>()
    // One raw fix list per `<Track>` in the current `<Activity>`; merged into
    // recording segments by [mergeTcxTracks] at Activity end (autolap boundaries
    // collapse, genuine pauses split).
    var tracks = mutableListOf<List<GpxFix>>()
    var segment = mutableListOf<GpxFix>()
    // Track/Trackpoint handling is gated on being INSIDE an <Activity>: a TCX
    // can legally carry a <Courses><Course><Track> block after </Activities>,
    // and without the gate its trackpoints would append into the last
    // already-committed activity's (live) list, silently merging a planned
    // course into a recorded run (review 2026-08-04, HIGH).
    var inActivity = false
    var inTrackpoint = false
    var inPosition = false
    var inHeartRateBpm = false
    var lat = Double.NaN
    var lon = Double.NaN
    var timeSec = 0L
    var hr: Int? = null
    var event = parser.eventType
    while (event != XmlPullParser.END_DOCUMENT) {
        val name = parser.name?.substringAfterLast(':')
        when (event) {
            XmlPullParser.START_TAG -> when (name) {
                "Activity" -> {
                    inActivity = true
                    tracks = mutableListOf()
                }
                "Track" -> if (inActivity) segment = mutableListOf()
                "Trackpoint" -> if (inActivity) {
                    inTrackpoint = true
                    lat = Double.NaN
                    lon = Double.NaN
                    timeSec = 0L
                    hr = null
                }
                "Position" -> if (inTrackpoint) inPosition = true
                "LatitudeDegrees" -> if (inTrackpoint && inPosition) {
                    lat = parser.nextText().trim().toDoubleOrNull() ?: Double.NaN
                }
                "LongitudeDegrees" -> if (inTrackpoint && inPosition) {
                    lon = parser.nextText().trim().toDoubleOrNull() ?: Double.NaN
                }
                "Time" -> if (inTrackpoint) timeSec = parseTcxTime(parser.nextText())
                "HeartRateBpm" -> if (inTrackpoint) inHeartRateBpm = true
                "Value" -> if (inTrackpoint && inHeartRateBpm) {
                    hr = parseHrBpm(parser.nextText())
                }
            }
            XmlPullParser.END_TAG -> when (name) {
                "Position" -> inPosition = false
                "HeartRateBpm" -> inHeartRateBpm = false
                "Trackpoint" -> if (inActivity) {
                    inTrackpoint = false
                    if (lat.isFinite() && lon.isFinite()) {
                        segment.add(GpxFix(lat, lon, timeSec, hr))
                    }
                }
                "Track" -> if (inActivity && segment.isNotEmpty()) tracks.add(segment)
                "Activity" -> {
                    inActivity = false
                    val merged = mergeTcxTracks(tracks)
                    if (merged.isNotEmpty()) activities.add(merged)
                }
            }
        }
        event = parser.next()
    }
    return activities
}

/** A time gap (seconds) between the end of one `<Track>` and the start of the
 *  next at or below this is treated as CONTINUOUS running (an autolap/lap
 *  boundary) and the two Tracks are merged into one segment; a larger gap is a
 *  genuine recording pause and splits them. Matches the live pipeline's
 *  fix-gap convention (RunSession's `MAX_FIX_GAP_SEC`, 30 s), a normal ~1–2 s
 *  trackpoint cadence never trips it, so only real pauses break a segment. */
private const val MAX_TRACK_MERGE_GAP_SEC = 30L

/**
 * Fold consecutive per-`<Track>` fix lists into recording segments. Standard
 * Garmin/Strava TCX writes one `<Track>` per `<Lap>` (incl. autolaps on
 * continuous running), so a Track boundary is a LAP boundary, not inherently a
 * pause. Adjacent Tracks separated by a small gap (≤ [MAX_TRACK_MERGE_GAP_SEC])
 * are merged into one segment so the core doesn't drop each lap's entering leg
 * from distance and moving time; a larger gap, or a non-positive/backwards
 * clock step, is a genuine pause and opens a new segment. Empty tracks are
 * skipped. Pure (no XML), so unit-testable directly. Package-visible for tests.
 */
internal fun mergeTcxTracks(tracks: List<List<GpxFix>>): List<List<GpxFix>> {
    val segments = mutableListOf<MutableList<GpxFix>>()
    for (track in tracks) {
        if (track.isEmpty()) continue
        val prev = segments.lastOrNull()
        val gap = if (prev != null && prev.isNotEmpty()) {
            track.first().timeSec - prev.last().timeSec
        } else {
            Long.MAX_VALUE
        }
        if (prev != null && gap in 0..MAX_TRACK_MERGE_GAP_SEC) {
            prev.addAll(track)
        } else {
            segments.add(track.toMutableList())
        }
    }
    return segments
}

/** ISO-8601 `<Time>` → unix seconds; 0 when missing/unparseable. */
private fun parseTcxTime(raw: String): Long {
    val s = raw.trim()
    if (s.isEmpty()) return 0L
    return runCatching { Instant.parse(s).epochSecond }
        .recoverCatching { OffsetDateTime.parse(s).toEpochSecond() }
        .getOrDefault(0L)
}
