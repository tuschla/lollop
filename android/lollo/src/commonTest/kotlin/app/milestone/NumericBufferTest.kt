package app.milestone

import kotlin.test.assertEquals
import kotlin.test.Test

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
        val full = "1".repeat(NumericBufferMaxLength)
        assertEquals(full, editNumericBuffer(full.dropLast(1), '1', replaceAll = false))
        assertEquals(full, editNumericBuffer(full, '7', replaceAll = false))
        assertEquals(full, editNumericBuffer(full, '.', replaceAll = false))
    }

    @Test
    fun aDigitReplacesALoneZero() {
        assertEquals("5", editNumericBuffer("0", '5', replaceAll = false))
        assertEquals("0.", editNumericBuffer("0", '.', replaceAll = false))
    }

    @Test
    fun leadingDotBecomesZeroDot() {
        assertEquals("0.", editNumericBuffer("", '.', replaceAll = false))
    }
}
