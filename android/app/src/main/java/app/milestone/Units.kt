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
 * in [AppSettings] (shell chrome, not coaching state), consumed by [resolveDistanceUnit].
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

/**
 * A canonical pace (minutes-per-km, the core's unit) → pace in the chosen unit's
 * denominator (min/km stays as-is; min/mi is the km pace scaled up). Pure.
 */
fun paceInUnit(minPerKm: Double, unit: DistanceUnit): Double = when (unit) {
    DistanceUnit.Km -> minPerKm
    DistanceUnit.Mi -> paceKmToMi(minPerKm)
}

/** Kilometers → miles. Pure. */
fun kmToMi(km: Double): Double = km / (METERS_PER_MILE / 1000.0)

/** Miles → kilometers. Pure. */
fun miToKm(mi: Double): Double = mi * (METERS_PER_MILE / 1000.0)

/** Miles → kilometers for the WIRE: rounds the converted value to 2 decimals so a
 *  freshly mi-entered distance doesn't put a full binary double on the wire (6.2 mi
 *  → 9.977932800000001 km unrounded vs 9.98 rounded), which a later km-mode display
 *  (the exact/lossless km paths) would render verbatim. 0.01 km = 10 m is far below
 *  the 0.01 mi (≈16 m) display step, and the round-trip is stable at the fields'
 *  display precision. Use ONLY at the mi→km SAVE boundary (LogRunEditor distance,
 *  RaceFinishPredictor weekly volume) — the km-native untouched-prefill paths stay
 *  exact (they emit the original km verbatim), and the km→mi range bounds stay exact
 *  too; only the emitted wire value rounds. Mirrors [lbToKgWire]. */
internal fun miToKmWire(mi: Double): Double = Math.round(miToKm(mi) * 100.0) / 100.0

/**
 * Weight units. As with distance, the core (crux) reasons in SI (kilograms) and its
 * verbatim weight STRINGS are already rendered in the profile's chosen unit core-side
 * (see app::Profile::weight_lb). These helpers cover the few weights the SHELL formats
 * itself (never the core-rendered strings), and mirror the core's conversion factor so
 * kg↔lb can never drift between the two sides. Pure functions of their inputs.
 */
enum class WeightUnit { Kg, Lb }

/** Pounds per kilogram (mirrors app/mod.rs `LB_PER_KG`). 1 kg = 2.2046226218 lb. */
const val LB_PER_KG: Double = 2.2046226218

/** Kilograms → pounds. Pure. */
fun kgToLb(kg: Double): Double = kg * LB_PER_KG

/** Pounds → kilograms. Pure. */
fun lbToKg(lb: Double): Double = lb / LB_PER_KG

/** Pounds → kilograms for the WIRE: rounds the converted value to 2 decimals so a
 *  freshly lb-entered figure doesn't put a full binary double on the wire
 *  (225 lb → 102.05828325225797 kg unrounded vs 102.06 rounded), which a later
 *  kg-mode display (the exact/lossless kg paths) would render verbatim. 0.01 kg is
 *  far below the 0.1 lb display step, and the round-trip is stable: 102.06 kg →
 *  225.0004 lb still formats "225.0" at the app-wide %.1f lb precision. Use ONLY at
 *  the lb→kg SAVE boundary (LogSetEditor save, ProteinForm compute) — the kg-native
 *  untouched-prefill paths stay exact (they emit the original kg verbatim), and the
 *  kg→lb range bounds stay exact too; only the emitted wire value rounds. */
internal fun lbToKgWire(lb: Double): Double = Math.round(lbToKg(lb) * 100.0) / 100.0

/** The short unit label for a weight, e.g. "82.5 kg" / "182 lb". Pure. */
fun weightSuffix(u: WeightUnit): String = when (u) {
    WeightUnit.Kg -> "kg"
    WeightUnit.Lb -> "lb"
}

/** Countries that conventionally weigh people in pounds. The UK uses stones/kg
 *  for bodyweight, so it stays kg even though it runs in miles. */
private val POUND_COUNTRIES = setOf("US", "LR", "MM")

/**
 * Locale-derived default weight unit: pounds for US/LR/MM, kilograms elsewhere.
 * Seeds the guided setup's kg|lb choice on a first run (a re-run keeps the
 * profile's stored preference). Pure given [locale].
 */
fun localeWeightUnit(locale: Locale = Locale.getDefault()): WeightUnit =
    if (locale.country.uppercase(Locale.ROOT) in POUND_COUNTRIES) WeightUnit.Lb else WeightUnit.Kg

/** Plausible adult bodyweight span the keypad accepts, in canonical kg. */
const val BODYWEIGHT_MIN_KG: Double = 30.0
const val BODYWEIGHT_MAX_KG: Double = 250.0

/** Round to one decimal, the display precision of every bodyweight field. */
private fun bodyweightRound1(v: Double): Double = Math.round(v * 10.0) / 10.0

/**
 * Bodyweight KEYPAD conversion (ProfileEditor "You · Bodyweight", GuidedSetup
 * "About you"). The core stores canonical kg; the field shows and parses the
 * display unit. Pure.
 *
 * [bodyweightBounds] is the accepted range in the display unit: 30–250 kg, or the
 * converted bounds rounded to the 0.1 lb display step (66.1–551.2 lb) so a value
 * converted on a unit toggle never lands just outside its own range.
 * [bodyweightDisplay] is a canonical kg reading in the display unit at 0.1 precision;
 * [bodyweightBufferText] is that figure as keypad text ("80", "176.4").
 * [bodyweightDisplayToKg] maps a typed display value back to canonical kg for the
 * wire: kg stays exact, lb rounds via [lbToKgWire] (the SAVE boundary every lb
 * weight field uses).
 */
fun bodyweightBounds(unit: WeightUnit): ClosedFloatingPointRange<Double> =
    if (unit == WeightUnit.Lb) {
        bodyweightRound1(kgToLb(BODYWEIGHT_MIN_KG))..bodyweightRound1(kgToLb(BODYWEIGHT_MAX_KG))
    } else {
        BODYWEIGHT_MIN_KG..BODYWEIGHT_MAX_KG
    }

fun bodyweightDisplay(kg: Double, unit: WeightUnit): Double =
    bodyweightRound1(if (unit == WeightUnit.Lb) kgToLb(kg) else kg)

fun bodyweightBufferText(kg: Double, unit: WeightUnit): String = trimDecimal(bodyweightDisplay(kg, unit))

fun bodyweightDisplayToKg(value: Double, unit: WeightUnit): Double =
    if (unit == WeightUnit.Lb) lbToKgWire(value) else value

/**
 * Weekly running-volume SCALE in the user's distance unit (ProfileEditor
 * "Running km/wk" / "Running mi/wk"). The core stores canonical km; the scale's
 * options, highlight and row value are whole display units, and a pick converts
 * back to km for the wire (km exact, mi via [miToKmWire]). Range 1–150 km, i.e.
 * 1–93 mi. Pure.
 */
const val WEEKLY_VOLUME_MAX_KM: Double = 150.0

fun weeklyVolumeScaleOptions(unit: DistanceUnit): List<Int> =
    if (unit == DistanceUnit.Mi) {
        (1..Math.round(kmToMi(WEEKLY_VOLUME_MAX_KM)).toInt()).toList()
    } else {
        (1..WEEKLY_VOLUME_MAX_KM.toInt()).toList()
    }

fun weeklyVolumeScaleValue(km: Double, unit: DistanceUnit): Int =
    Math.round(if (unit == DistanceUnit.Mi) kmToMi(km) else km).toInt()

fun weeklyVolumeScaleToKm(value: Int, unit: DistanceUnit): Double =
    if (unit == DistanceUnit.Mi) miToKmWire(value.toDouble()) else value.toDouble()

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

/**
 * Trim a human-scale decimal to one decimal place: a whole value drops the
 * decimal ("5"), a fractional one keeps a single place ("5.5"). Rounds FIRST, so a
 * value that rounds to a whole number drops it too (9.97 → "10", never "10.0").
 * Shared by every distance/RPE/duration/magnitude label so the "no trailing .0"
 * rule lives once.
 */
internal fun trimDecimal(v: Double): String {
    if (!v.isFinite()) return "-"
    val oneDecimal = String.format(Locale.US, "%.1f", v)
    val trimmed = if (oneDecimal.endsWith(".0")) oneDecimal.dropLast(2) else oneDecimal
    return if (trimmed == "-0") "0" else trimmed
}
