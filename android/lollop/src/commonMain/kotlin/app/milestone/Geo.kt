package app.milestone

import kotlin.math.PI
import kotlin.math.asin
import kotlin.math.cos
import kotlin.math.sin
import kotlin.math.sqrt

/** Degrees to radians, as the core's `f64::to_radians`: one multiply by π/180. */
internal fun toRadians(degrees: Double): Double = degrees * (PI / 180.0)

/** Great-circle metres between two fixes (haversine on the core's
 *  [CoreConstants.EARTH_RADIUS_M], so live, route and of-record distances agree): the
 *  one place the segment maths lives, shared by the route renderer, the trim preview
 *  ([timedTrackFromSegments]) and the live tracker ([segmentKm]). */
internal fun haversineM(lat1: Double, lon1: Double, lat2: Double, lon2: Double): Double {
    val dLat = toRadians(lat2 - lat1)
    val dLon = toRadians(lon2 - lon1)
    val a = sin(dLat / 2).let { it * it } +
        cos(toRadians(lat1)) * cos(toRadians(lat2)) * sin(dLon / 2).let { it * it }
    // Clamp asin's input to [0,1]: it can drift a hair past 1.0 on a near-antipodal
    // leg through FP rounding → asin(NaN).
    return 2 * CoreConstants.EARTH_RADIUS_M * asin(sqrt(a.coerceIn(0.0, 1.0)))
}

/** Great-circle distance between two consecutive fixes, km ([haversineM]), for the
 *  live accumulator ([RunSession.add]), recovery ([RunSession.restore]), the pace
 *  slices and the start-on-move gate. */
internal fun segmentKm(a: GpsPoint, b: GpsPoint): Double = haversineM(a.lat, a.lon, b.lat, b.lon) / 1000.0

/** The "Precise start" gate's accuracy bound (m): recording only begins on a fix at
 *  or under this. Typical post-lock consumer GPS is 5–15 m open sky, so this is a
 *  trustworthy start anchor without being unachievable under light tree/building
 *  cover. Consumed by [ArmTrigger]'s precise gate (RunSession.kt). */
internal const val GPS_PRECISE_ACCURACY_M = 20.0

/** The distance (km) a leg of [km] over [dtSec] adds to a run, or null when it is not
 *  a moving leg (no elapsed time, or slower than [CoreConstants.MIN_MOVING_SPEED_MPS]:
 *  standing still, so GPS wander at a red light accrues nothing). A moving leg keeps
 *  all its time; its distance is capped at [CoreConstants.MAX_PLAUSIBLE_SPEED_MPS]·dt
 *  (GPS jitter), so the live figures match the saved run's. Shared by the live
 *  figures, the pace slices, crash recovery and the trim preview, which test it
 *  against null for the moving rule. */
internal fun movingLegKm(km: Double, dtSec: Double): Double? {
    if (dtSec <= 0.0 || km * 1000.0 / dtSec < CoreConstants.MIN_MOVING_SPEED_MPS) return null
    return minOf(km, CoreConstants.MAX_PLAUSIBLE_SPEED_MPS * dtSec / 1000.0)
}

/**
 * One route fix reduced to what the trim preview needs: its absolute
 * epoch-second timestamp, the cumulative haversine distance (km) and the cumulative
 * MOVING seconds from the track start up to this point. PREVIEW figures ONLY — the
 * of-record distance/duration are always re-derived in the core after a trim event.
 */
data class TimedTrackPoint(val timeSec: Long, val cumKm: Double, val movingSec: Double)

/**
 * A route's [segments] reshaped for the trim preview: every fix flattened into one
 * time-ordered list carrying its epoch-second time, the cumulative distance in km
 * (the same running haversine sum [RouteTrack] colours by) and the cumulative moving
 * time. A segment gap (a GPS pause) adds neither; within a segment a leg adds its
 * seconds only when it clears the core's moving-leg rule ([movingLegKm]), so
 * standing still counts no time either.
 */
fun timedTrackFromSegments(segments: List<List<GpsPoint>>): List<TimedTrackPoint> {
    val out = ArrayList<TimedTrackPoint>()
    var runningM = 0.0
    var movingSec = 0.0
    for (seg in segments) {
        var prev: GpsPoint? = null
        for (fix in seg) {
            if (prev != null) {
                val legM = haversineM(prev.lat, prev.lon, fix.lat, fix.lon)
                runningM += legM
                val dt = (fix.observedAt - prev.observedAt).toDouble()
                if (movingLegKm(legM / 1000.0, dt) != null) movingSec += dt
            }
            out += TimedTrackPoint(fix.observedAt, runningM / 1000.0, movingSec)
            prev = fix
        }
    }
    return out
}
