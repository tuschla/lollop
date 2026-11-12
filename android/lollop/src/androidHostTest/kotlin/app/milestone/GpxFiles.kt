package app.milestone

import java.time.Instant

/** One `<trk>` of a [gpxDocument]: a `<trkseg>` per segment, typed [type] when given. */
internal class GpxTrack(val segments: List<List<GpsPoint>>, val type: String? = null)

/**
 * A GPX 1.1 document the way other apps write one, for the import tests: a `<trk>`
 * per track, each fix's `<time>` unless not [timed], and a strap's heart rate from
 * [hr] (called once per fix, in order) when given.
 */
internal fun gpxDocument(vararg tracks: GpxTrack, timed: Boolean = true, hr: ((GpsPoint) -> Int)? = null): String = buildString {
    append("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n")
    append("<gpx version=\"1.1\" creator=\"test\" xmlns=\"http://www.topografix.com/GPX/1/1\">\n")
    for (track in tracks) {
        append("<trk>")
        if (track.type != null) append("<type>${track.type}</type>")
        for (seg in track.segments) {
            append("<trkseg>")
            for (p in seg) {
                append("<trkpt lat=\"${p.lat}\" lon=\"${p.lon}\">")
                if (timed) append("<time>").append(Instant.ofEpochSecond(p.observedAt)).append("</time>")
                if (hr != null) append("<extensions><hr>${hr(p)}</hr></extensions>")
                append("</trkpt>\n")
            }
            append("</trkseg>")
        }
        append("</trk>")
    }
    append("</gpx>\n")
}
