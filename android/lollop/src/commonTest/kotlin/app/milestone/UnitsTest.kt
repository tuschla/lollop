package app.milestone

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** The units math and the locale's default units. */
class UnitsTest {

    @Test
    fun localeResolvesImperialCountriesToMiles() {
        assertEquals(DistanceUnit.Mi, localeDistanceUnit(testLocale("en-US")))
        assertEquals(DistanceUnit.Mi, localeDistanceUnit(testLocale("en-GB")))
        assertEquals(DistanceUnit.Km, localeDistanceUnit(testLocale("de-DE")))
        assertEquals(DistanceUnit.Km, localeDistanceUnit(testLocale("fr-FR")))
    }

    @Test
    fun theProfileUnitWinsAndTheLocaleCoversThePreSetupApp() {
        assertEquals(DistanceUnit.Km, ViewModel(profile = Profile(distance_unit = DistanceUnit.Km)).distanceUnit())
        assertEquals(DistanceUnit.Mi, ViewModel(profile = Profile(distance_unit = DistanceUnit.Mi)).distanceUnit())
        assertEquals(localeDistanceUnit(), ViewModel().distanceUnit())
        assertEquals(localeDistanceUnit(), (null as ViewModel?).distanceUnit())
    }

    @Test
    fun lbToKgWireRoundsToTwoDecimalsAndRoundTrips() {
        // The lb→kg SAVE boundary rounds to 2 decimals so a fresh lb entry doesn't
        // put a full binary double on the wire (225 lb → 102.05828… unrounded).
        assertEquals(102.06, lbToKgWire(225.0), 1e-9)
        assertEquals(74.84, lbToKgWire(165.0), 1e-9)
        // Round-trip stays stable at the app-wide one-decimal lb display precision.
        assertEquals("225.0", formatFixed(kgToLb(lbToKgWire(225.0)), 1))
        assertEquals("165.0", formatFixed(kgToLb(lbToKgWire(165.0)), 1))
    }

    @Test
    fun metersToDisplayConvertsPerUnit() {
        assertEquals(1.0, metersToDisplay(1000.0, DistanceUnit.Km), 1e-9)
        assertEquals(1.0, metersToDisplay(1609.344, DistanceUnit.Mi), 1e-9)
    }

    @Test
    fun paceConvertsKmToMile() {
        // 5:00 /km is 8:02.8 /mi (5 × 1.609344 min).
        assertEquals(8.04672, paceInUnit(5.0, DistanceUnit.Mi), 1e-9)
        assertEquals(5.0, paceInUnit(5.0, DistanceUnit.Km), 1e-9)
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

    @Test
    fun trimDecimalDropsWholeAndKeepsFractional() {
        assertEquals("5", trimDecimal(5.0))
        assertEquals("5.5", trimDecimal(5.5))
        // Non-finite input returns the sibling formatters' dash sentinel, never
        // "NaN"/"Infinity".
        assertEquals("-", trimDecimal(Double.NaN))
        assertEquals("-", trimDecimal(Double.POSITIVE_INFINITY))
        assertEquals("-", trimDecimal(Double.NEGATIVE_INFINITY))
        // One decimal place, HALF_UP: the double 2.35 is 2.35000…0089 > 2.35.
        assertEquals("2.4", trimDecimal(2.35))
        // Rounded before the whole-number check: a value that rounds up to a whole
        // number drops the decimal too.
        assertEquals("10", trimDecimal(9.97))
        assertEquals("0", trimDecimal(-0.04))
    }

    @Test
    fun kmMiRoundTripAndKnownValues() {
        // One mile is exactly 1.609344 km (NIST), so the pair is each other's inverse.
        assertEquals(1.0, kmToMi(1.609344), 1e-9)
        assertEquals(1.609344, miToKm(1.0), 1e-9)
        assertEquals(10.0, miToKm(kmToMi(10.0)), 1e-9)
        assertEquals(6.21371, kmToMi(10.0), 1e-5)
    }

    @Test
    fun miToKmWireRoundsToTwoDecimalsAndRoundTrips() {
        // The mi→km SAVE boundary rounds to 2 decimals so a fresh mi entry doesn't put
        // a full binary double on the wire (6.2 mi → 9.9779328 km unrounded).
        assertEquals(9.98, miToKmWire(6.2), 1e-9)
        assertEquals(40.23, miToKmWire(25.0), 1e-9)
        // Round-trip stays stable at the distance field's two-decimal display precision.
        assertEquals("6.20", formatFixed(kmToMi(miToKmWire(6.2)), 2))
        assertEquals("25.00", formatFixed(kmToMi(miToKmWire(25.0)), 2))
    }

    @Test
    fun theMileDistanceCeilingSitsOnTheFieldGridAndSavesWithinTheKmLimit() {
        val max = runDistanceMax(DistanceUnit.Mi)
        // A ± chip coerced to the ceiling prints a value the field accepts.
        val chipped = formatFixed((62.0 + 0.5).coerceIn(0.0, max), 2)
        assertTrue(chipped.toDouble() <= max, "chip result $chipped must be <= $max")
        assertEquals(100.0, miToKmWire(max), 1e-9)
        assertEquals(100.0, runDistanceMax(DistanceUnit.Km), 0.0)
    }

    @Test
    fun bodyweightKeypadHonorsUnitAndRoundTrips() {
        // kg entries are exact; display rounds to the 0.1 field precision.
        assertEquals("80", bodyweightBufferText(80.0, WeightUnit.Kg))
        assertEquals("62.5", bodyweightBufferText(62.5, WeightUnit.Kg))
        assertEquals(182.0, bodyweightDisplayToKg(182.0, WeightUnit.Kg), 0.0)
        // lb: 80 kg ≈ 176.4 lb; a typed lb value rounds back to canonical kg via lbToKgWire.
        assertEquals("176.4", bodyweightBufferText(80.0, WeightUnit.Lb))
        assertEquals(80.01, bodyweightDisplayToKg(176.4, WeightUnit.Lb), 1e-9)
        // The display is stable across a typed lb value's canonical round-trip.
        assertEquals(176.4, bodyweightDisplay(bodyweightDisplayToKg(176.4, WeightUnit.Lb), WeightUnit.Lb), 1e-9)
    }

    @Test
    fun bodyweightBoundsCoverHeavyAndLightAdults() {
        // A 182 kg lifter (old scale capped at 150) can enter their weight.
        val kg = bodyweightBounds(WeightUnit.Kg)
        assertEquals(30.0, kg.start, 0.0)
        assertEquals(250.0, kg.endInclusive, 0.0)
        assertTrue(182.0 in kg)
        // lb: the converted bounds at the 0.1 lb display step.
        val lb = bodyweightBounds(WeightUnit.Lb)
        assertEquals(66.1, lb.start, 1e-9)
        assertEquals(551.2, lb.endInclusive, 1e-9)
    }

    @Test
    fun localeSeedsWeightUnit() {
        assertEquals(WeightUnit.Lb, localeWeightUnit(testLocale("en-US")))
        assertEquals(WeightUnit.Kg, localeWeightUnit(testLocale("de-DE")))
        // The UK runs in miles but weighs people in kg/stones, never lb by default.
        assertEquals(WeightUnit.Kg, localeWeightUnit(testLocale("en-GB")))
    }

    @Test
    fun weeklyVolumeScaleFollowsDistanceUnit() {
        assertEquals(40.23, weeklyVolumeScaleToKm(25, DistanceUnit.Mi), 1e-9)
        assertEquals(25, weeklyVolumeScaleValue(weeklyVolumeScaleToKm(25, DistanceUnit.Mi), DistanceUnit.Mi))
        assertEquals(93, weeklyVolumeScaleOptions(DistanceUnit.Mi).last())
        assertEquals(24.0, weeklyVolumeScaleToKm(24, DistanceUnit.Km), 0.0)
        assertEquals(24, weeklyVolumeScaleValue(24.0, DistanceUnit.Km))
        assertEquals(150, weeklyVolumeScaleOptions(DistanceUnit.Km).last())
    }
}
