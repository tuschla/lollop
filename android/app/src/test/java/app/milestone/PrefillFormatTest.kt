package app.milestone

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * The weight/distance keypad editors submit exactly what the prefill buffer
 * displays (the display-committed invariant), so a lossy prefill silently
 * rewrites an untouched Save. [fmtLosslessPrefill] must render the exact stored
 * value with trailing zeros trimmed, never rounded.
 */
class PrefillFormatTest {

    @Test
    fun wholeValuesDropTheDecimal() {
        assertEquals("100", fmtLosslessPrefill(100.0))
        assertEquals("10", fmtLosslessPrefill(10.0))
        assertEquals("0", fmtLosslessPrefill(0.0))
    }

    @Test
    fun fractionsArePreservedNotRounded() {
        // The old `%.1f` weight prefill rounded 100.25 → "100.3"; the old `%.2f`
        // distance prefill rounded 42.195 → "42.20". Both must now survive intact.
        assertEquals("100.25", fmtLosslessPrefill(100.25))
        assertEquals("42.195", fmtLosslessPrefill(42.195))
    }

    @Test
    fun trailingZerosAreTrimmed() {
        assertEquals("12.5", fmtLosslessPrefill(12.50))
        assertEquals("5.5", fmtLosslessPrefill(5.5))
    }

    @Test
    fun outputRoundTripsThroughTheBufferParser() {
        // The buffer is parsed back with `.toDoubleOrNull()` at submit; the prefill
        // must reparse to the exact stored double so an untouched Save is a no-op.
        for (v in listOf(100.0, 42.195, 12.5, 0.0, 7.25)) {
            assertEquals(v, fmtLosslessPrefill(v).toDouble(), 0.0)
        }
    }
}
