package app.milestone

import app.milestone.resources.Res
import app.milestone.resources.track_cd_distance_km
import app.milestone.resources.track_cd_distance_mi
import app.milestone.resources.track_cd_hours
import app.milestone.resources.track_cd_minutes
import app.milestone.resources.track_cd_not_available
import app.milestone.resources.track_cd_pace_km
import app.milestone.resources.track_cd_pace_mi
import app.milestone.resources.track_cd_seconds

/** A display-unit length to two decimals ("0.18"), the one figure format every
 *  run distance and split length uses, so a sheet never mixes decimal marks. */
internal fun displayFigure(displayValue: Double): String = formatFixed(displayValue, 2)

/** A distance figure without its unit ("3.21"), in the user's unit. */
internal fun distanceFigure(km: Double, unit: DistanceUnit): String = displayFigure(metersToDisplay(km * 1000.0, unit))

/** A pace figure without its unit ("5:30"), in the user's unit; the placeholder
 *  while there is no pace yet. */
internal fun paceFigure(minPerKm: Double, unit: DistanceUnit): String =
    if (minPerKm.isFinite()) formatPaceMinutes(paceInUnit(minPerKm, unit)) else EmptyValuePlaceholder

/** Distance in the display unit, e.g. "8.05 km" / "5.00 mi". */
internal fun runDistLabel(distanceKm: Double, unit: DistanceUnit): String = "${distanceFigure(distanceKm, unit)} ${unit.distanceLabel}"

/** Pace in the display unit, e.g. "5:34 /km" / "8:57 /mi". Recomputed from the
 *  run's distance+duration (not the core's always-/km `pace` string) so every run
 *  readout honours the chosen unit. [EmptyValuePlaceholder] for a degenerate run. */
internal fun runPaceLabel(distanceKm: Double, durationMin: Double, unit: DistanceUnit): String {
    if (distanceKm <= 0.0 || durationMin <= 0.0) return EmptyValuePlaceholder
    return formatPaceMinutes(paceInUnit(durationMin / distanceKm, unit)) + " " + unit.paceSuffix
}

/** A duration read aloud with words, e.g. "1 hour 2 minutes 3 seconds"; zero
 *  leading parts are dropped ("12 minutes 3 seconds"). */
internal fun spokenDuration(res: PlatformResources, totalSec: Long): String {
    val s = totalSec.coerceAtLeast(0L)
    val h = (s / 3600).toInt()
    val m = ((s % 3600) / 60).toInt()
    val sec = (s % 60).toInt()
    val parts = ArrayList<String>(3)
    if (h > 0) parts += res.getQuantityString(Res.plurals.track_cd_hours, h, h)
    if (h > 0 || m > 0) parts += res.getQuantityString(Res.plurals.track_cd_minutes, m, m)
    parts += res.getQuantityString(Res.plurals.track_cd_seconds, sec, sec)
    return parts.joinToString(" ")
}

/** A distance figure read aloud with its unit, e.g. "3.18 miles". */
internal fun spokenDistance(res: PlatformResources, figure: String, unit: DistanceUnit): String =
    res.getString(if (unit == DistanceUnit.Mi) Res.string.track_cd_distance_mi else Res.string.track_cd_distance_km, figure)

/** A run distance (km) read aloud in the user's unit, e.g. "3.18 miles". */
internal fun spokenRunDistance(res: PlatformResources, km: Double, unit: DistanceUnit): String =
    spokenDistance(res, distanceFigure(km, unit), unit)

/** A canonical pace (min/km) read aloud in the user's unit, e.g. "5 minutes 15
 *  seconds per kilometer", or "not available yet" when unknown. */
internal fun spokenPace(res: PlatformResources, minPerKm: Double, unit: DistanceUnit): String {
    if (!minPerKm.isFinite() || minPerKm <= 0.0) return res.getString(Res.string.track_cd_not_available)
    val secPerUnit = (paceInUnit(minPerKm, unit) * 60.0).roundOrZero()
    return res.getString(
        if (unit == DistanceUnit.Mi) Res.string.track_cd_pace_mi else Res.string.track_cd_pace_km,
        spokenDuration(res, secPerUnit),
    )
}


/** The face head of a run item in the display [unit]: the core's volume head
 *  ("6 × 800 m"), whose target and structure move into dedicated, unit-aware face
 *  lines. Null when the item carries no run target or its summary leads with no
 *  volume (then the full summary stays and no face lines are added, so nothing
 *  renders twice). */
internal fun runFaceHead(item: PrescriptionView, unit: DistanceUnit): String? =
    if (item.run_target != null && item.run_head.isNotBlank()) runHeadText(item, unit) else null

/** A run item's volume head in the display [unit], so the head and the pace line
 *  under it never mix km with /mi: a distance head ("8 km") is restated from its
 *  km figure; reps ("6 × 800 m"), minutes and bouts stay as the core wrote them. */
internal fun runHeadText(item: PrescriptionView, unit: DistanceUnit): String {
    val km = item.run_head_km
    return if (km == null || unit == DistanceUnit.Km) {
        item.run_head
    } else {
        // A figure and its unit never wrap apart, as in the core's own head.
        "${trimDecimal(metersToDisplay(km * 1000.0, unit))}\u00A0${unit.distanceLabel}"
    }
}


/**
 * Elapsed run time: `m:ss` under an hour, `h:mm:ss` at or beyond one, so a long
 * run (half/marathon, both goal distances the app supports) reads "1:12:05"
 * rather than a confusing "72:05". Negative spans clamp to zero.
 */
fun formatElapsed(seconds: Long): String {
    val s = seconds.coerceAtLeast(0L)
    return if (s >= 3600) {
        "${s / 3600}:${zeroPad((s % 3600) / 60)}:${zeroPad(s % 60)}"
    } else {
        "${s / 60}:${zeroPad(s % 60)}"
    }
}
