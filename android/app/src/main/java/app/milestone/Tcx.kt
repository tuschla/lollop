package app.milestone

import org.xmlpull.v1.XmlPullParser

// Minimal TCX (Garmin Training Center XML) track reader for run import
// (Garmin/Strava/etc. exports that ship TCX instead of GPX). Reads only
// `<Activities>/<Activity>/<Lap>/<Track>/<Trackpoint>`, each Activity's `Sport`
// attribute, each Trackpoint's `<Time>`, `<Position>` (`LatitudeDegrees`/
// `LongitudeDegrees`), and an optional `<HeartRateBpm><Value>` sample. Tag names
// are matched by local name (namespace prefix stripped) so any TCX namespace
// declaration works. Everything else (Lap summary stats, Extensions, Author,
// Creator, Course) is ignored. Produces [GpxFix]es, the same shape as the GPX reader.

/**
 * One [ImportedRun] per `<Activity>` (a multi-sport day is several runs, not pauses
 * inside one), with its `Sport` attribute; its `<Track>`s are merged into recording
 * segments by [mergeTcxTracks]. Trackpoints without a `<Position>` (TCX writes these
 * during pauses) are skipped.
 */
internal fun parseTcxActivities(text: String): List<ImportedRun> {
    val activities = mutableListOf<ImportedRun>()
    var sport: String? = null
    // One raw fix list per `<Track>` in the current `<Activity>`, merged at Activity end.
    var tracks = mutableListOf<List<GpxFix>>()
    var segment = mutableListOf<GpxFix>()
    // Track/Trackpoint handling is gated on being INSIDE an <Activity>: a TCX
    // can legally carry a <Courses><Course><Track> block after </Activities>,
    // and without the gate its trackpoints would append into the last
    // already-committed activity's (live) list; silently merging a planned
    // course into a recorded run.
    var inActivity = false
    var inTrackpoint = false
    var inPosition = false
    var inHeartRateBpm = false
    var lat = Double.NaN
    var lon = Double.NaN
    var timeSec = 0L
    var hr: Int? = null
    pullImportTags(text, "TrainingCenterDatabase") { parser, event, name ->
        when (event) {
            XmlPullParser.START_TAG -> when (name) {
                "Activity" -> {
                    inActivity = true
                    tracks = mutableListOf()
                    sport = parser.getAttributeValue(null, "Sport")?.trim()?.takeIf { it.isNotEmpty() }
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
                "Time" -> if (inTrackpoint) timeSec = parseImportTimeSec(parser.nextText())
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
                        // TCX names no accuracy: null, see [IMPORT_UNKNOWN_ACCURACY_M].
                        segment.add(GpxFix(lat, lon, timeSec, hr))
                    }
                }
                "Track" -> if (inActivity && segment.isNotEmpty()) tracks.add(segment)
                "Activity" -> {
                    inActivity = false
                    val merged = mergeTcxTracks(tracks)
                    if (merged.isNotEmpty()) activities.add(ImportedRun(merged, sport))
                }
            }
        }
    }
    return activities
}

/** Longest gap (seconds) between one `<Track>`'s end and the next one's start that
 *  still counts as continuous running, the live pipeline's fix-gap threshold
 *  (RunSession's `MAX_FIX_GAP_SEC`). A 1-2 s trackpoint cadence never trips it. */
private const val MAX_TRACK_MERGE_GAP_SEC = 30L

/**
 * Fold consecutive per-`<Track>` fix lists into recording segments. Garmin/Strava
 * TCX writes one `<Track>` per `<Lap>`, autolaps on continuous running included,
 * and the core drops the entering leg of every segment from distance and moving
 * time (~−130 m / −40 s on an autolap-per-km marathon). So adjacent Tracks with a
 * gap of 0 to [MAX_TRACK_MERGE_GAP_SEC] merge; a larger gap or a backwards clock
 * step is a pause and opens a new segment. Empty tracks are skipped.
 */
internal fun mergeTcxTracks(tracks: List<List<GpxFix>>): List<List<GpxFix>> {
    val segments = mutableListOf<MutableList<GpxFix>>()
    for (track in tracks) {
        if (track.isEmpty()) continue
        val prev = segments.lastOrNull()
        if (prev != null && track.first().timeSec - prev.last().timeSec in 0..MAX_TRACK_MERGE_GAP_SEC) {
            prev.addAll(track)
        } else {
            segments.add(track.toMutableList())
        }
    }
    return segments
}

