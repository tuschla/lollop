package app.milestone

import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** JVM unit tests for the pure units math in Units.kt (no Android context). */
class UnitsTest {

    @Test
    fun localeResolvesImperialCountriesToMiles() {
        assertEquals(DistanceUnit.Mi, localeDistanceUnit(Locale("en", "US")))
        assertEquals(DistanceUnit.Mi, localeDistanceUnit(Locale("en", "GB")))
        assertEquals(DistanceUnit.Km, localeDistanceUnit(Locale("de", "DE")))
        assertEquals(DistanceUnit.Km, localeDistanceUnit(Locale("fr", "FR")))
    }

    @Test
    fun overridePrecedenceWinsOverLocale() {
        // Explicit override beats the locale; System falls back to the locale.
        assertEquals(DistanceUnit.Km, resolveDistanceUnit(DistanceUnitOverride.Km, Locale("en", "US")))
        assertEquals(DistanceUnit.Mi, resolveDistanceUnit(DistanceUnitOverride.Mi, Locale("de", "DE")))
        assertEquals(DistanceUnit.Mi, resolveDistanceUnit(DistanceUnitOverride.System, Locale("en", "US")))
        assertEquals(DistanceUnit.Km, resolveDistanceUnit(DistanceUnitOverride.System, Locale("de", "DE")))
    }

    @Test
    fun metersToDisplayConvertsPerUnit() {
        assertEquals(1.0, metersToDisplay(1000.0, DistanceUnit.Km), 1e-9)
        assertEquals(1.0, metersToDisplay(METERS_PER_MILE, DistanceUnit.Mi), 1e-9)
        assertEquals(1609.344, METERS_PER_MILE, 1e-9)
    }

    @Test
    fun paceConvertsKmToMile() {
        // A mile is longer, so min/mi > min/km for the same speed.
        val minPerKm = 5.0
        val minPerMi = paceInUnit(minPerKm, DistanceUnit.Mi)
        assertEquals(minPerKm * (METERS_PER_MILE / 1000.0), minPerMi, 1e-9)
        assertTrue("min/mi must exceed min/km", minPerMi > minPerKm)
        assertEquals(minPerKm, paceInUnit(minPerKm, DistanceUnit.Km), 1e-9)
        assertEquals(minPerKm, paceMiToKm(paceKmToMi(minPerKm)), 1e-9)
    }

    @Test
    fun formatPaceMinutesRendersMSSAndGuardsGarbage() {
        assertEquals("5:00", formatPaceMinutes(5.0))
        assertEquals("5:30", formatPaceMinutes(5.5))
        // 5.999 min → 6:00 (60s rollover), not 5:60.
        assertEquals("6:00", formatPaceMinutes(5.999))
        assertEquals("-:-", formatPaceMinutes(Double.POSITIVE_INFINITY))
        assertEquals("-:-", formatPaceMinutes(Double.NaN))
        assertEquals("-:-", formatPaceMinutes(-1.0))
    }
}
