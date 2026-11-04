package app.milestone

import java.math.BigDecimal
import java.util.Locale
import kotlin.random.Random
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * The Kotlin ports iOS formats numbers with ([halfUpFixed], [groupThousands],
 * [zeroPad], [fmtLosslessPrefill]) against the JVM calls Android uses, over
 * figures at every scale the app shows (paces, kg and lb loads, distances, HR)
 * plus ties and random doubles. FormattingTest's table runs on both platforms.
 */
class FormattingJvmParityTest {

    private fun samples(): List<Double> {
        val rng = Random(20261104)
        val out = ArrayList<Double>()
        // Decimal grids, where ties live: every 0.001 up to 300 (loads, paces, km).
        for (i in 0..300_000) out += i / 1000.0
        for (i in 0..20_000) out += -i / 1000.0
        // Values the app derives by arithmetic (lb ↔ kg, mi ↔ km, paces).
        for (i in 0..5_000) {
            out += i * LB_PER_KG
            out += i / LB_PER_KG
            out += kmToMi(i / 10.0)
            out += miToKm(i / 10.0)
            out += i / 60.0
        }
        // Random doubles across magnitudes.
        repeat(200_000) {
            val mag = rng.nextInt(-6, 13)
            out += (rng.nextDouble() * 10.0 - 5.0) * Math.pow(10.0, mag.toDouble())
        }
        out += listOf(0.0, -0.0, Double.MIN_VALUE, Double.MAX_VALUE, -Double.MAX_VALUE, 1e-300, 123456789.0)
        return out
    }

    @Test
    fun halfUpFixed_matchesStringFormat() {
        for (v in samples()) {
            for (d in 0..3) {
                assertEquals("%.${d}f of $v", String.format(Locale.US, "%.${d}f", v), halfUpFixed(v, d))
            }
        }
    }

    @Test
    fun fmtLosslessPrefill_matchesBigDecimal() {
        for (v in samples()) {
            assertEquals("$v", BigDecimal.valueOf(v).stripTrailingZeros().toPlainString(), fmtLosslessPrefill(v))
        }
    }

    @Test
    fun groupThousandsAndZeroPad_matchStringFormat() {
        val rng = Random(7)
        val values = (-2_000L..2_000L) + List(50_000) { rng.nextLong() } + listOf(Long.MIN_VALUE, Long.MAX_VALUE)
        for (n in values) {
            assertEquals(String.format(Locale.US, "%,d", n), groupThousands(n))
            assertEquals(String.format(Locale.US, "%02d", n), zeroPad(n))
            assertEquals(String.format(Locale.US, "%03d", n), zeroPad(n, 3))
        }
    }
}
