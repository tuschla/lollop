package app.milestone

import java.util.Locale

/**
 * Distance/pace units. The core (crux) is unit-agnostic: it stores and reasons in
 * SI (meters, min/km). Unit choice is pure shell chrome, resolved here and applied
 * only at the display edge. Everything in this file is a pure function of its inputs
 * (the locale defaults [localeDistanceUnit] and [localeWeightUnit] read the JVM
 * default locale) so the math is unit-testable without an Android context.
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
 * Reads [Locale.getDefault] by default. Kept separate from
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

/**
 * A canonical pace (minutes-per-km, the core's unit) → pace in the chosen unit's
 * denominator (min/km stays as-is; min/mi is the km pace scaled up). Pure.
 */
fun paceInUnit(minPerKm: Double, unit: DistanceUnit): Double = when (unit) {
    DistanceUnit.Km -> minPerKm
    DistanceUnit.Mi -> miToKm(minPerKm) // min/km × km/mi = min/mi
}

/** The run target's pace label in [unit], falling back to the other unit's label. */
internal fun RunTargetView.paceLabel(unit: DistanceUnit): String =
    if (unit == DistanceUnit.Mi) pace_label_mi.ifBlank { pace_label_km } else pace_label_km.ifBlank { pace_label_mi }

/** Kilometers → miles. Pure. */
fun kmToMi(km: Double): Double = km / (METERS_PER_MILE / 1000.0)

/** Miles → kilometers. Pure. */
fun miToKm(mi: Double): Double = mi * (METERS_PER_MILE / 1000.0)

/** Miles → kilometers for the WIRE, rounded to 2 decimals (0.01 km, below the 0.01 mi
 *  display step), so a mi-entered distance doesn't put a full binary double on the
 *  wire (6.2 mi → 9.977932800000001 km) that a later km display would render
 *  verbatim. Use only at the mi→km save boundary; km-native paths stay exact. */
internal fun miToKmWire(mi: Double): Double = round2(miToKm(mi))

private fun round2(x: Double): Double = Math.round(x * 100.0) / 100.0

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

/** Pounds → kilograms for the WIRE (225 lb → 102.06 kg, not 102.05828325225797);
 *  see [miToKmWire]. 102.06 kg still formats "225.0" lb. */
internal fun lbToKgWire(lb: Double): Double = round2(lbToKg(lb))

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

/** Round to one decimal, the display precision of every bodyweight and distance field. */
private fun roundTenth(v: Double): Double = Math.round(v * 10.0) / 10.0

/**
 * Bodyweight keypad conversion: the core stores canonical kg, the field shows the
 * display unit at 0.1 precision. The lb bounds are rounded to that step so a value
 * converted on a unit toggle stays inside them; lb goes back to kg via [lbToKgWire].
 */
fun bodyweightBounds(unit: WeightUnit): ClosedFloatingPointRange<Double> =
    if (unit == WeightUnit.Lb) {
        roundTenth(kgToLb(BODYWEIGHT_MIN_KG))..roundTenth(kgToLb(BODYWEIGHT_MAX_KG))
    } else {
        BODYWEIGHT_MIN_KG..BODYWEIGHT_MAX_KG
    }

fun bodyweightDisplay(kg: Double, unit: WeightUnit): Double =
    roundTenth(if (unit == WeightUnit.Lb) kgToLb(kg) else kg)

fun bodyweightBufferText(kg: Double, unit: WeightUnit): String = trimDecimal(bodyweightDisplay(kg, unit))

fun bodyweightDisplayToKg(value: Double, unit: WeightUnit): Double =
    if (unit == WeightUnit.Lb) lbToKgWire(value) else value

/** A km range shown in the user's distance unit. */
fun distanceBounds(km: ClosedFloatingPointRange<Double>, unit: DistanceUnit): ClosedFloatingPointRange<Double> =
    if (unit == DistanceUnit.Mi) roundTenth(kmToMi(km.start))..roundTenth(kmToMi(km.endInclusive)) else km

/** A canonical km value as the keypad buffer in the display unit. */
fun distanceBufferText(km: Double, unit: DistanceUnit): String =
    trimDecimal(if (unit == DistanceUnit.Mi) roundTenth(kmToMi(km)) else km)

/** A typed display-unit distance back to canonical km. */
fun displayToKm(value: Double, unit: DistanceUnit): Double =
    if (unit == DistanceUnit.Mi) miToKmWire(value) else value

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

/** A kg weight in [unit], trimmed like [trimDecimal] ("102.5", "226"). Pure. */
internal fun weightFigure(kg: Double, unit: WeightUnit): String =
    trimDecimal(if (unit == WeightUnit.Lb) kgToLb(kg) else kg)

/** Longest timer-style digit buffer: hhmmss. */
internal const val HMS_DIGITS_MAX = 6

/** A timer-style digit buffer filled right to left ("14530" = 1:45:30) as hours,
 *  minutes and seconds; null when empty, longer than [HMS_DIGITS_MAX] or not all
 *  digits. Each caller applies its own validity rule. Pure. */
internal fun hmsDigits(digits: String): Triple<Int, Int, Int>? {
    if (digits.isEmpty() || digits.length > HMS_DIGITS_MAX || !digits.all { it.isDigit() }) return null
    val p = digits.padStart(HMS_DIGITS_MAX, '0')
    return Triple(p.substring(0, 2).toInt(), p.substring(2, 4).toInt(), p.substring(4, 6).toInt())
}
