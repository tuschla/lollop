package app.milestone

import kotlin.math.PI

/** Metres per degree of latitude (and of longitude on the equator) on the core's sphere. */
internal const val M_PER_DEG: Double = PI * CoreConstants.EARTH_RADIUS_M / 180.0

/** A fix [northM] metres due north of 52.52°N 13.405°E, observed at [t]. */
internal fun northFix(northM: Double, t: Long, accuracyM: Double = 5.0) =
    GpsPoint(52.52 + northM / M_PER_DEG, 13.405, t, accuracyM)

/** [n] fixes due north from ([lat], [lon]), [stepM] metres and [stepSec] seconds
 *  apart, the first observed at [firstAt]. */
internal fun straightTrack(
    n: Int,
    firstAt: Long,
    stepM: Double = 10.0,
    stepSec: Long = 3L,
    lat: Double = 47.0,
    lon: Double = 8.0,
    accuracyM: Double = 5.0,
): List<GpsPoint> = List(n) { GpsPoint(lat + it * stepM / M_PER_DEG, lon, firstAt + it * stepSec, accuracyM) }
