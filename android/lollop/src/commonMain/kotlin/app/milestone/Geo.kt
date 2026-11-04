package app.milestone

import kotlin.math.asin
import kotlin.math.cos
import kotlin.math.sin
import kotlin.math.sqrt

/** Mean Earth radius (m), matching the core's EARTH_RADIUS_M = 6_371_000 (running.rs)
 *  so live, route and of-record distances agree. */
private const val EarthRadiusM = 6_371_000.0

/** Degrees to radians, as `java.lang.Math.toRadians` (whose rounding differs by
 *  JDK: older Android runtimes divide by 180 then multiply by π). */
internal expect fun toRadians(degrees: Double): Double

/** Great-circle metres between two fixes (haversine on the core's radius): the one
 *  place the segment maths lives, shared by the route renderer, the trim preview
 *  ([timedTrackFromSegments]) and the live tracker ([segmentKm]). */
internal fun haversineM(lat1: Double, lon1: Double, lat2: Double, lon2: Double): Double {
    val dLat = toRadians(lat2 - lat1)
    val dLon = toRadians(lon2 - lon1)
    val a = sin(dLat / 2).let { it * it } +
        cos(toRadians(lat1)) * cos(toRadians(lat2)) * sin(dLon / 2).let { it * it }
    // Clamp asin's input to [0,1]: it can drift a hair past 1.0 on a near-antipodal
    // leg through FP rounding → asin(NaN).
    return 2 * EarthRadiusM * asin(sqrt(a.coerceIn(0.0, 1.0)))
}

/** Great-circle distance between two consecutive fixes, km ([haversineM]), for the
 *  live accumulator ([RunSession.add]), recovery ([RunSession.restore]), the pace
 *  slices and the start-on-move gate. */
internal fun segmentKm(a: GpsPoint, b: GpsPoint): Double = haversineM(a.lat, a.lon, b.lat, b.lon) / 1000.0

/** The "Precise start" gate's accuracy bound (m): recording only begins on a fix at
 *  or under this. Typical post-lock consumer GPS is 5–15 m open sky, so 20 m is a
 *  trustworthy start anchor without being unachievable under light tree/building
 *  cover. Consumed by [ArmTrigger]'s precise gate (RunSession.kt). */
internal const val GPS_PRECISE_ACCURACY_M = 20.0
