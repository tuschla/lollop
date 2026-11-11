package app.milestone

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import app.milestone.resources.Res
import app.milestone.resources.activity_minutes_invalid
import app.milestone.resources.log_decoupling_hint
import app.milestone.resources.log_protein_deficit_only
import app.milestone.resources.log_switch_masters
import app.milestone.resources.log_weekly_signs_1to2
import app.milestone.resources.log_weekly_signs_gt2
import app.milestone.resources.main_calc_protein_masters
import kotlinx.coroutines.runBlocking
import org.jetbrains.compose.resources.getString
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * Copy that states a core threshold takes the figure from the core
 * ([CoreConstants]), not from the string; filled, it reads exactly as the
 * literal English text did: Latin digits, a real minus sign, no trailing ".0".
 * (The short-run note and import reason are pinned in [TrackingCopyTest] and
 * [RunImportTest].)
 */
@RunWith(RobolectricTestRunner::class)
class CoreThresholdCopyTest {

    private val expected = mapOf(
        ReadinessSignal.Rpe to
            "Effort vs plan: +1 means 1 rep fewer left than planned · 1 off adjusts the weight, 2 off adjusts it more",
        ReadinessSignal.EstimatedOneRm to
            "Today's estimated max ÷ your normal · under 0.95 holds the weight, over 1.05 adds to it",
        ReadinessSignal.BarVelocity to "Average bar speed while lifting (m/s)",
        ReadinessSignal.VelocityLoss to "Bar-speed drop within a set (%) · stop at 10–40% depending on goal",
        ReadinessSignal.WellnessZ to "Wellness vs your normal (z-score) · −1.5 or lower eases today's session",
        ReadinessSignal.HrvLnRmssd to "HRV vs your normal (z-score) · below −0.5 eases today's session",
        ReadinessSignal.HrvCv to "HRV coefficient of variation (%)",
        ReadinessSignal.AerobicDecoupling to "Heart-rate drift (%) · over 10 keeps the run easy",
        ReadinessSignal.RestingHr to
            "Resting heart rate above your normal (bpm) · +5 for 2+ days eases training, +10 stops it",
        ReadinessSignal.Soreness to "Soreness, 1–7 · 6 or more eases intensity",
    )

    @Test
    fun everyValueHintReadsAsTheLiteralEnglishText() = runBlocking {
        val withScale = ReadinessSignal.entries.filter { it.valueHint != null }.toSet()
        assertEquals(expected.keys, withScale)
        for ((signal, text) in expected) {
            val (hint, args) = signal.valueHint!!
            assertEquals(signal.name, text, getString(hint, *args.toTypedArray()))
        }
    }

    @Test
    fun decouplingDurationHintReadsAsTheLiteralEnglishText() = runBlocking {
        assertEquals(
            "Heart-rate drift only counts on a steady run longer than 20 minutes.",
            getString(Res.string.log_decoupling_hint, decouplingMinEffortLabel()),
        )
    }

    @Test
    fun mastersAgeCopyReadsAsTheLiteralEnglishText() = runBlocking {
        assertEquals("Age 65+", getString(Res.string.log_switch_masters, mastersAgeLabel()))
        assertEquals("age 65+", getString(Res.string.main_calc_protein_masters, mastersAgeLabel()))
        assertEquals(
            "Under 65, the evidence supports a protein target only while you eat less than you burn.",
            getString(Res.string.log_protein_deficit_only, mastersAgeLabel()),
        )
    }

    @Test
    fun activityAndImportLimitsReadAsTheLiteralEnglishText() = runBlocking {
        assertEquals(
            "Enter 1 to 600 minutes.",
            getString(Res.string.activity_minutes_invalid, trimDecimal(CoreConstants.ACTIVITY_MAX_MINUTES)),
        )
        val ctx = ApplicationProvider.getApplicationContext<Context>()
        assertEquals(
            "too large, 32 MB max",
            importFailureReason(ctx.resources, RunImportFailure("big.gpx", ImportFailure.TooLarge)),
        )
    }

    @Test
    fun overtrainingWeekAnswersKeepTheirValuesAndText() = runBlocking {
        // The values the weekly check-in has always sent, one inside each band.
        assertEquals(listOf(0.5, 1.5, 3.0), OvertrainingWeeks)
        val (minWeeks, nforWeeks) = overtrainingWeekEdgeLabels()
        assertEquals("1–2 weeks", getString(Res.string.log_weekly_signs_1to2, minWeeks, nforWeeks))
        assertEquals("More than 2 weeks", getString(Res.string.log_weekly_signs_gt2, nforWeeks))
    }
}
