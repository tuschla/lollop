package app.milestone

import java.util.Locale

/**
 * Distance/pace units. The core (crux) is unit-agnostic: it stores and reasons in
 * SI (meters, min/km). Unit choice is pure shell chrome, resolved here and applied
 * only at the display edge. Everything in this file is a pure function of its inputs
 * (the one exception, [localeDistanceUnit], reads the JVM default locale) so the math
 * is unit-testable without an Android context.
 */
enum class DistanceUnit(
    /** Short distance label, e.g. "5.2 km" / "3.2 mi". */
    val distanceLabel: String,
    /** Pace suffix, e.g. "5:30 /km" / "8:51 /mi". */
    val paceSuffix: String,
) {
    Km("km", "/km"),
    Mi("mi", "/mi"),
}

/**
 * User override for the distance unit. [System] defers to the device locale
 * ([localeDistanceUnit]); [Km]/[Mi] force a fixed unit regardless of locale. Stored
 * in [ThemeSettings] (shell chrome, not coaching state), consumed by [resolveDistanceUnit].
 */
enum class DistanceUnitOverride { System, Km, Mi }

/** Exact international mile in meters (NIST). 1 mi = 1609.344 m. */
const val METERS_PER_MILE: Double = 1609.344

/** Countries that conventionally use miles for road/running distance. */
private val MILE_COUNTRIES = setOf("US", "GB", "LR", "MM")

/**
 * Locale-derived default unit: miles for US/GB/LR/MM, kilometers everywhere else.
 * Reads [Locale.getDefault], the only non-pure call in this file. Kept separate from
 * the pure math so tests can drive [resolveDistanceUnit] with an explicit locale.
 */
fun localeDistanceUnit(locale: Locale = Locale.getDefault()): DistanceUnit =
    if (locale.country.uppercase(Locale.ROOT) in MILE_COUNTRIES) DistanceUnit.Mi else DistanceUnit.Km

/**
 * Resolve the effective unit: an explicit [DistanceUnitOverride] wins; [System] falls
 * back to the locale. Pure given [locale] (default = JVM default).
 */
fun resolveDistanceUnit(
    override: DistanceUnitOverride,
    locale: Locale = Locale.getDefault(),
): DistanceUnit = when (override) {
    DistanceUnitOverride.Km -> DistanceUnit.Km
    DistanceUnitOverride.Mi -> DistanceUnit.Mi
    DistanceUnitOverride.System -> localeDistanceUnit(locale)
}

/** Meters → display distance in the chosen unit (km or mi). Pure. */
fun metersToDisplay(meters: Double, unit: DistanceUnit): Double = when (unit) {
    DistanceUnit.Km -> meters / 1000.0
    DistanceUnit.Mi -> meters / METERS_PER_MILE
}

/** A pace expressed as minutes-per-km → minutes-per-mile. Pure. */
fun paceKmToMi(minPerKm: Double): Double = minPerKm * (METERS_PER_MILE / 1000.0)

/** A pace expressed as minutes-per-mile → minutes-per-km. Pure. */
fun paceMiToKm(minPerMi: Double): Double = minPerMi / (METERS_PER_MILE / 1000.0)

/**
 * A canonical pace (minutes-per-km, the core's unit) → pace in the chosen unit's
 * denominator (min/km stays as-is; min/mi is the km pace scaled up). Pure.
 */
fun paceInUnit(minPerKm: Double, unit: DistanceUnit): Double = when (unit) {
    DistanceUnit.Km -> minPerKm
    DistanceUnit.Mi -> paceKmToMi(minPerKm)
}

/**
 * Format a pace (in minutes, fractional) as "m:ss". Rounds to the nearest second and
 * carries a 60s rollover (e.g. 5.999 → "6:00"). Non-finite or negative input → "-:-".
 */
fun formatPaceMinutes(minutes: Double): String {
    if (!minutes.isFinite() || minutes < 0.0) return "-:-"
    val totalSeconds = Math.round(minutes * 60.0).toInt()
    val mins = totalSeconds / 60
    val secs = totalSeconds % 60
    return "$mins:${secs.toString().padStart(2, '0')}"
}
