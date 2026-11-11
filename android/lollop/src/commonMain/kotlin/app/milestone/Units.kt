package app.milestone

// The core reasons in SI (meters, min/km) and renders its own distance text in
// the profile's [DistanceUnit]; the helpers here cover the figures the SHELL
// formats itself, in that same unit.

val DistanceUnit.distanceLabel: String
    get() = when (this) {
        DistanceUnit.Km -> "km"
        DistanceUnit.Mi -> "mi"
    }

val DistanceUnit.paceSuffix: String get() = "/$distanceLabel"

/** The unit every distance reads in: the profile's; before a profile exists, the
 *  device locale's ([localeDistanceUnit]). */
internal fun ViewModel?.distanceUnit(): DistanceUnit = this?.profile?.distance_unit ?: localeDistanceUnit()

/** Countries that conventionally use miles for road/running distance. */
private val MILE_COUNTRIES = setOf("US", "GB", "LR", "MM")

fun localeDistanceUnit(locale: PlatformLocale = deviceLocale()): DistanceUnit =
    if (locale.region.uppercase() in MILE_COUNTRIES) DistanceUnit.Mi else DistanceUnit.Km

fun metersToDisplay(meters: Double, unit: DistanceUnit): Double = when (unit) {
    DistanceUnit.Km -> meters / 1000.0
    DistanceUnit.Mi -> meters / CoreConstants.MILE_M
}

fun paceInUnit(minPerKm: Double, unit: DistanceUnit): Double = when (unit) {
    DistanceUnit.Km -> minPerKm
    DistanceUnit.Mi -> miToKm(minPerKm) // min/km × km/mi = min/mi
}

fun kmToMi(km: Double): Double = km / (CoreConstants.MILE_M / 1000.0)

fun miToKm(mi: Double): Double = mi * (CoreConstants.MILE_M / 1000.0)

/** Miles → kilometers for the WIRE, rounded to 2 decimals (0.01 km, below the 0.01 mi
 *  display step), so a mi-entered distance doesn't put a full binary double on the
 *  wire (6.2 mi → 9.977932800000001 km) that a later km display would render
 *  verbatim. Use only at the mi→km save boundary; km-native paths stay exact. */
internal fun miToKmWire(mi: Double): Double = round2(miToKm(mi))

private fun round2(x: Double): Double = (x * 100.0).roundOrZero() / 100.0

/**
 * As with distance, the core reasons in SI (kilograms) and its verbatim weight
 * STRINGS are already rendered in the profile's chosen unit core-side (see
 * profile::Profile::weight_lb). These helpers cover the few weights the SHELL
 * formats itself (never the core-rendered strings), with the core's conversion
 * factor ([CoreConstants.LB_PER_KG]).
 */
enum class WeightUnit { Kg, Lb }

internal fun ViewModel.weightUnit(): WeightUnit = if (profile?.weight_lb == true) WeightUnit.Lb else WeightUnit.Kg

internal val Event.SetProfile.weightUnit: WeightUnit get() = if (weightLb) WeightUnit.Lb else WeightUnit.Kg

fun kgToLb(kg: Double): Double = kg * CoreConstants.LB_PER_KG

fun lbToKg(lb: Double): Double = lb / CoreConstants.LB_PER_KG

/** Pounds → kilograms for the WIRE (225 lb → 102.06 kg, not 102.05828325225797);
 *  see [miToKmWire]. 102.06 kg still formats "225.0" lb. */
internal fun lbToKgWire(lb: Double): Double = round2(lbToKg(lb))

fun weightSuffix(u: WeightUnit): String = when (u) {
    WeightUnit.Kg -> "kg"
    WeightUnit.Lb -> "lb"
}

/** Countries that conventionally weigh people in pounds. The UK uses stones/kg
 *  for bodyweight, so it stays kg even though it runs in miles. */
private val POUND_COUNTRIES = setOf("US", "LR", "MM")

/** Seeds the guided setup's kg|lb choice on a first run (a re-run keeps the
 *  profile's stored preference). */
fun localeWeightUnit(locale: PlatformLocale = deviceLocale()): WeightUnit =
    if (locale.region.uppercase() in POUND_COUNTRIES) WeightUnit.Lb else WeightUnit.Kg

/**
 * Format a pace (in minutes, fractional) as "m:ss". Rounds to the nearest second and
 * carries a 60s rollover (e.g. 5.999 → "6:00"). Non-finite or negative input → "-:-".
 */
fun formatPaceMinutes(minutes: Double): String {
    if (!minutes.isFinite() || minutes < 0.0) return "-:-"
    val totalSeconds = (minutes * 60.0).roundOrZero().toInt()
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
    val oneDecimal = formatFixed(v, 1)
    val trimmed = if (oneDecimal.endsWith(".0")) oneDecimal.dropLast(2) else oneDecimal
    return if (trimmed == "-0") "0" else trimmed
}

/** A kg weight in [unit], trimmed like [trimDecimal] ("102.5", "226"). */
internal fun weightFigure(kg: Double, unit: WeightUnit): String =
    trimDecimal(if (unit == WeightUnit.Lb) kgToLb(kg) else kg)

/** [n] with its thousands split by a narrow no-break space ("4 690"): neither ','
 *  nor '.', so no locale reads the mark as a decimal point. Every grouped figure
 *  goes through it. */
internal fun groupedFigure(n: Long): String = groupThousands(n, '\u202F')
