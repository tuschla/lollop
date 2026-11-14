@file:Suppress("NOTHING_TO_INLINE")

package app.milestone

import kotlin.math.abs
import kotlin.math.roundToInt
import kotlin.math.roundToLong

/** [this] to the nearest whole number, ties toward positive infinity, as
 *  `java.lang.Math.round(double)`: NaN gives 0 where [roundToLong] throws, and
 *  the infinities clamp to [Long.MIN_VALUE] / [Long.MAX_VALUE]. */
internal inline fun Double.roundOrZero(): Long = if (isNaN()) 0L else roundToLong()

/** `java.lang.Math.round(float)`: [roundOrZero] for a Float, to an Int. */
internal inline fun Float.roundOrZero(): Int = if (isNaN()) 0 else roundToInt()

/**
 * [value] with exactly [decimals] fraction digits, ASCII digits and a '.' point,
 * no grouping: what `String.format(Locale.US, "%.Nf", value)` prints. Ties round
 * half up on the value's shortest decimal form (2.25 → "2.3", 0.15 → "0.2"), a
 * negative value keeps its sign when it rounds to zero ("-0.0"), and NaN and the
 * infinities print as "NaN", "Infinity", "-Infinity".
 */
internal expect fun formatFixed(value: Double, decimals: Int): String

/**
 * [formatFixed] in plain Kotlin: java.util.Formatter's `%.Nf`, which rounds the
 * shortest decimal digits of [value] (Double.toString's) half up. The iOS
 * formatter; the host tests hold it to the JVM's String.format.
 */
internal fun halfUpFixed(value: Double, decimals: Int): String {
    require(decimals >= 0) { "decimals must be >= 0: $decimals" }
    if (value.isNaN()) return "NaN"
    val negative = value < 0.0 || (value == 0.0 && 1.0 / value < 0.0)
    if (value.isInfinite()) return if (negative) "-Infinity" else "Infinity"
    val decimal = shortestDigits(if (negative) -value else value)
    val digits = decimal.digits
    var point = decimal.point
    // Digits that survive the rounding, counted from digits[0].
    val keep = point + decimals
    when {
        keep < 0 -> digits.setLength(0)
        keep < digits.length -> {
            val roundUp = digits[keep] >= '5'
            digits.setLength(keep)
            if (roundUp) {
                var i = keep - 1
                while (i >= 0 && digits[i] == '9') digits[i--] = '0'
                if (i >= 0) {
                    digits[i] = digits[i] + 1
                } else {
                    digits.insert(0, '1')
                    point++
                }
            }
        }
    }
    val out = StringBuilder()
    if (negative) out.append('-')
    val intDigits = StringBuilder()
    for (i in 0 until point) intDigits.append(if (i < digits.length) digits[i] else '0')
    val firstNonZero = intDigits.indexOfFirst { it != '0' }
    out.append(if (firstNonZero < 0) "0" else intDigits.substring(firstNonZero))
    if (decimals > 0) {
        out.append('.')
        for (i in point until point + decimals) out.append(if (i in digits.indices) digits[i] else '0')
    }
    return out.toString()
}

/**
 * Lossless prefill for the weight/distance keypad buffers: the exact stored
 * double without trailing zeros or an exponent (100.0 → "100", 42.195 →
 * "42.195", 12.50 → "12.5", 1.0E-5 → "0.00001"), never a fixed-decimal
 * rounding, so an untouched Save re-encodes the stored value. What
 * `BigDecimal.valueOf(v).stripTrailingZeros().toPlainString()` prints, from the
 * same shortest digits; throws [NumberFormatException] for NaN and the
 * infinities, as BigDecimal does.
 */
internal fun fmtLosslessPrefill(v: Double): String {
    if (!v.isFinite()) throw NumberFormatException("Infinite or NaN")
    val negative = v < 0.0
    // abs: -0.0 prints "-0.0", and BigDecimal has no negative zero.
    val (digits, point) = shortestDigits(abs(v))
    // Strip zeros on both ends; what is left reads digits × 10^(point - length).
    val first = digits.indexOfFirst { it != '0' }
    if (first < 0) return "0"
    val last = digits.indexOfLast { it != '0' }
    val significant = digits.substring(first, last + 1)
    val intLength = point - first
    val out = StringBuilder()
    if (negative) out.append('-')
    when {
        intLength <= 0 -> out.append("0.").append("0".repeat(-intLength)).append(significant)
        intLength >= significant.length -> out.append(significant).append("0".repeat(intLength - significant.length))
        else -> out.append(significant, 0, intLength).append('.').append(significant, intLength, significant.length)
    }
    return out.toString()
}

/** A non-negative finite double's shortest decimal digits (Double.toString's:
 *  "123.45", "1.0E-5", "1.2345E10"): the value is 0.<digits> × 10^point, i.e.
 *  [point] digits stand before the decimal point. */
private data class Digits(val digits: StringBuilder, val point: Int)

private fun shortestDigits(nonNegative: Double): Digits {
    val repr = nonNegative.toString()
    val e = repr.indexOf('E')
    val mantissa = if (e < 0) repr else repr.substring(0, e)
    val exponent = if (e < 0) 0 else repr.substring(e + 1).toInt()
    val dot = mantissa.indexOf('.')
    val digits = StringBuilder(if (dot < 0) mantissa else mantissa.removeRange(dot, dot + 1))
    return Digits(digits, (if (dot < 0) mantissa.length else dot) + exponent)
}

/** [n] with its thousands split by [separator] ("4,690", "-1,234,567"): what
 *  `String.format(Locale.US, "%,d", n)` prints for a ',' [separator]. */
internal fun groupThousands(n: Long, separator: Char = ','): String {
    val digits = n.toString()
    val start = if (digits[0] == '-') 1 else 0
    val count = digits.length - start
    if (count <= 3) return digits
    val out = StringBuilder(digits.length + count / 3)
    out.append(digits, 0, start)
    for (i in start until digits.length) {
        if (i > start && (digits.length - i) % 3 == 0) out.append(separator)
        out.append(digits[i])
    }
    return out.toString()
}

/** [n] left-padded with zeros to [width] digits ("%02d" → "07"); a negative [n]
 *  keeps its sign in front ("-7"), as java.util.Formatter's `%0Nd` does. */
internal fun zeroPad(n: Long, width: Int = 2): String {
    if (n < 0) return "-" + n.toString().substring(1).padStart(width - 1, '0')
    return n.toString().padStart(width, '0')
}

internal fun zeroPad(n: Int, width: Int = 2): String = zeroPad(n.toLong(), width)
