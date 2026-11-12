package app.milestone

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith

/**
 * The shell's number formatting reads the same on every platform. The table is
 * what `String.format(Locale.US, "%.Nf", v)` prints on the JVM (java.util.Formatter
 * rounds the shortest decimal digits half up): on Android [formatFixed] IS that
 * call, on iOS it is [halfUpFixed], so this pins both to Java's output. The port
 * itself is compared with `String.format` on the JVM by FormattingJvmParityTest.
 */
class FormattingTest {

    // value, then "%.0f" … "%.3f"
    private val fixedTable: List<Pair<Double, List<String>>> = listOf(
        0.0 to listOf("0", "0.0", "0.00", "0.000"),
        -0.0 to listOf("-0", "-0.0", "-0.00", "-0.000"),
        0.05 to listOf("0", "0.1", "0.05", "0.050"),
        0.15 to listOf("0", "0.2", "0.15", "0.150"),
        0.25 to listOf("0", "0.3", "0.25", "0.250"),
        0.35 to listOf("0", "0.4", "0.35", "0.350"),
        0.45 to listOf("0", "0.5", "0.45", "0.450"),
        1.005 to listOf("1", "1.0", "1.01", "1.005"),
        2.675 to listOf("3", "2.7", "2.68", "2.675"),
        1.45 to listOf("1", "1.5", "1.45", "1.450"),
        2.5 to listOf("3", "2.5", "2.50", "2.500"),
        -2.5 to listOf("-3", "-2.5", "-2.50", "-2.500"),
        0.5 to listOf("1", "0.5", "0.50", "0.500"),
        1.5 to listOf("2", "1.5", "1.50", "1.500"),
        9.95 to listOf("10", "10.0", "9.95", "9.950"),
        9.995 to listOf("10", "10.0", "10.00", "9.995"),
        99.95 to listOf("100", "100.0", "99.95", "99.950"),
        102.06 to listOf("102", "102.1", "102.06", "102.060"),
        4.35 to listOf("4", "4.4", "4.35", "4.350"),
        0.125 to listOf("0", "0.1", "0.13", "0.125"),
        0.375 to listOf("0", "0.4", "0.38", "0.375"),
        -0.04 to listOf("-0", "-0.0", "-0.04", "-0.040"),
        -1.25 to listOf("-1", "-1.3", "-1.25", "-1.250"),
        -0.5 to listOf("-1", "-0.5", "-0.50", "-0.500"),
        1e-5 to listOf("0", "0.0", "0.00", "0.000"),
        5e-5 to listOf("0", "0.0", "0.00", "0.000"),
        1.0E7 to listOf("10000000", "10000000.0", "10000000.00", "10000000.000"),
        1e20 to listOf(
            "100000000000000000000", "100000000000000000000.0",
            "100000000000000000000.00", "100000000000000000000.000",
        ),
        1234567.891 to listOf("1234568", "1234567.9", "1234567.89", "1234567.891"),
        12345.6789 to listOf("12346", "12345.7", "12345.68", "12345.679"),
        5.999 to listOf("6", "6.0", "6.00", "5.999"),
        2.2046226218 to listOf("2", "2.2", "2.20", "2.205"),
        1609.344 to listOf("1609", "1609.3", "1609.34", "1609.344"),
        0.95 to listOf("1", "1.0", "0.95", "0.950"),
        0.995 to listOf("1", "1.0", "1.00", "0.995"),
        Double.NaN to listOf("NaN", "NaN", "NaN", "NaN"),
        Double.POSITIVE_INFINITY to listOf("Infinity", "Infinity", "Infinity", "Infinity"),
        Double.NEGATIVE_INFINITY to listOf("-Infinity", "-Infinity", "-Infinity", "-Infinity"),
    )

    @Test
    fun formatFixed_printsWhatJavaPrints() {
        for ((v, expected) in fixedTable) {
            expected.forEachIndexed { decimals, want -> assertEquals(want, formatFixed(v, decimals), "formatFixed($v, $decimals)") }
        }
    }

    @Test
    fun groupThousands_splitsEveryThreeDigits() {
        assertEquals("0", groupThousands(0))
        assertEquals("999", groupThousands(999))
        assertEquals("1,000", groupThousands(1000))
        assertEquals("4,690", groupThousands(4690))
        assertEquals("-1,234,567", groupThousands(-1234567))
        assertEquals("-999", groupThousands(-999))
        assertEquals("9,223,372,036,854,775,807", groupThousands(Long.MAX_VALUE))
        assertEquals("-9,223,372,036,854,775,808", groupThousands(Long.MIN_VALUE))
        assertEquals("12\u202F345", groupThousands(12345, '\u202F'))
    }

    @Test
    fun zeroPad_padsLikePercentZeroD() {
        assertEquals("00", zeroPad(0))
        assertEquals("07", zeroPad(7))
        assertEquals("59", zeroPad(59))
        assertEquals("123", zeroPad(123))
        assertEquals("-7", zeroPad(-7))
        assertEquals("-07", zeroPad(-7, 3))
        assertEquals("0007", zeroPad(7L, 4))
    }

    @Test
    fun roundOrZero_isJavaMathRound() {
        assertEquals(3L, 2.5.roundOrZero())
        assertEquals(-2L, (-2.5).roundOrZero())
        assertEquals(0L, (-0.5).roundOrZero())
        assertEquals(1L, 0.5.roundOrZero())
        assertEquals(0L, Double.NaN.roundOrZero())
        assertEquals(Long.MAX_VALUE, Double.POSITIVE_INFINITY.roundOrZero())
        assertEquals(Long.MIN_VALUE, Double.NEGATIVE_INFINITY.roundOrZero())
        assertEquals(3, 2.5f.roundOrZero())
        assertEquals(0, Float.NaN.roundOrZero())
    }

    @Test
    fun fmtLosslessPrefill_isThePlainShortestDecimal() {
        assertEquals("100", fmtLosslessPrefill(100.0))
        assertEquals("42.195", fmtLosslessPrefill(42.195))
        assertEquals("12.5", fmtLosslessPrefill(12.50))
        assertEquals("0", fmtLosslessPrefill(0.0))
        assertEquals("0", fmtLosslessPrefill(-0.0))
        assertEquals("-12.5", fmtLosslessPrefill(-12.5))
        assertEquals("0.00001", fmtLosslessPrefill(1e-5))
        assertEquals("10000000", fmtLosslessPrefill(1.0E7))
        assertEquals("100000000000000000000", fmtLosslessPrefill(1e20))
        assertEquals("104.32624510230815", fmtLosslessPrefill(230.0 / CoreConstants.LB_PER_KG))
        assertFailsWith<NumberFormatException> { fmtLosslessPrefill(Double.NaN) }
        assertFailsWith<NumberFormatException> { fmtLosslessPrefill(Double.POSITIVE_INFINITY) }
    }
}
