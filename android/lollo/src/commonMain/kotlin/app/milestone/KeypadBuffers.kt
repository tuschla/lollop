package app.milestone

internal const val NumericBufferMaxLength = 6

/**
 * Apply one keypad key to a numeric text buffer. `replaceAll` implements
 * calculator-style entry: the first key after (re)activating a field starts a
 * fresh number. Parsing and validation stay with the caller.
 */
fun editNumericBuffer(text: String, key: Char, replaceAll: Boolean): String {
    val base = if (replaceAll) "" else text
    if (base.length >= NumericBufferMaxLength) return base
    if (key == '.') {
        if (base.contains('.')) return base
        return if (base.isEmpty()) "0." else "$base."
    }
    if (base == "0") return "$key"
    return base + key
}

/** Plausible adult bodyweight span the keypad accepts, in canonical kg. */
const val BODYWEIGHT_MIN_KG: Double = 30.0
const val BODYWEIGHT_MAX_KG: Double = 250.0

/** Round to one decimal, the display precision of every bodyweight and distance field. */
private fun roundTenth(v: Double): Double = (v * 10.0).roundOrZero() / 10.0

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

/** Longest timer-style digit buffer: hhmmss. */
internal const val HMS_DIGITS_MAX = 6

/** A timer-style digit buffer filled right to left ("14530" = 1:45:30) as hours,
 *  minutes and seconds; null when empty, longer than [HMS_DIGITS_MAX] or not all
 *  digits. Each caller applies its own validity rule. */
internal fun hmsDigits(digits: String): Triple<Int, Int, Int>? {
    if (digits.isEmpty() || digits.length > HMS_DIGITS_MAX || !digits.all { it.isDigit() }) return null
    val p = digits.padStart(HMS_DIGITS_MAX, '0')
    return Triple(p.substring(0, 2).toInt(), p.substring(2, 4).toInt(), p.substring(4, 6).toInt())
}

/** A timer-style digit buffer as shown: "1:45:30", "45:30", "0:30"; "" when empty. */
internal fun hmsDisplay(digits: String): String {
    if (digits.isEmpty()) return ""
    val p = digits.padStart(HMS_DIGITS_MAX, '0').takeLast(HMS_DIGITS_MAX)
    val h = p.substring(0, 2).toInt()
    val m = p.substring(2, 4)
    val s = p.substring(4, 6)
    return if (h > 0) "$h:$m:$s" else "${m.toInt()}:$s"
}

/** Whole seconds as the timer-style digit buffer that displays them
 *  (6330 → "14530"); "" for zero. */
internal fun secondsToHmsDigits(sec: Int): String {
    val h = sec / 3600
    val m = sec % 3600 / 60
    val s = sec % 60
    val raw = if (h > 0) "$h${zeroPad(m)}${zeroPad(s)}" else "$m${zeroPad(s)}"
    return raw.trimStart('0')
}
