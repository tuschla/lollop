package app.milestone

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * The keypad editors submit exactly what the buffer displays (the
 * display-committed invariant), so the buffer-editing rules are load-bearing:
 * a stray second decimal point or unbounded growth would corrupt what gets
 * parsed and logged.
 */
class NumericBufferTest {

    @Test
    fun digitsAppend() {
        assertEquals("12", editNumericBuffer("1", '2', replaceAll = false))
        assertEquals("100.5", editNumericBuffer("100.", '5', replaceAll = false))
    }

    @Test
    fun firstKeyAfterActivationReplacesThePrefill() {
        // Calculator-style entry: opening a field pre-filled "100.0" and typing
        // "8" starts a fresh "8", not "100.08".
        assertEquals("8", editNumericBuffer("100.0", '8', replaceAll = true))
        // A leading '.' on a fresh buffer becomes "0." (parseable prefix).
        assertEquals("0.", editNumericBuffer("100.0", '.', replaceAll = true))
    }

    @Test
    fun secondDecimalPointIsIgnored() {
        assertEquals("10.5", editNumericBuffer("10.5", '.', replaceAll = false))
        assertEquals("0.", editNumericBuffer("0.", '.', replaceAll = false))
    }

    @Test
    fun lengthIsCapped() {
        assertEquals("123456", editNumericBuffer("123456", '7', replaceAll = false))
    }

    @Test
    fun leadingDotBecomesZeroDot() {
        assertEquals("0.", editNumericBuffer("", '.', replaceAll = false))
    }
}
