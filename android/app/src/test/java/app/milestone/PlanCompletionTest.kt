package app.milestone

import android.content.Context
import android.content.res.Resources
import androidx.compose.ui.platform.AccessibilityManager
import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * Pure per-set completion helpers: slot derivation, the
 * next-open-item selection the "Today's plan" fast-path and the loop use, the
 * "set k of N" computation, and the hold / RPE line formatting. Each asserts a
 * user-visible outcome that the pre-fix behaviour got wrong (a fabricated "@ RPE
 * 0", a "0 kg × 1" hold, a fully-logged item showing "6 of 5", the first-item
 * prefill trap). Robolectric for the string resources.
 */
@RunWith(RobolectricTestRunner::class)
class PlanCompletionTest {

    private val res: Resources get() = ApplicationProvider.getApplicationContext<Context>().resources

    private fun load(figure: String, unit: String) = res.getString(R.string.log_value_with_unit, figure, unit)

    private fun distance(figure: String, unit: DistanceUnit) =
        res.getString(R.string.plan_run_logged_distance, figure, unit.distanceLabel)

    private fun item(
        exercise: String = "Back Squat",
        sets: Int = 5,
        setsDone: Int = 0,
        done: Boolean = false,
    ) = PrescriptionView(exercise = exercise, sets = sets, sets_done = setsDone, done = done)

    @Test
    fun plannedSetCountFloorsAtOne() {
        assertEquals(5, plannedSetCount(item(sets = 5)))
        // A core that omitted `sets` must never render a 0-slot row.
        assertEquals(1, plannedSetCount(item(sets = 0)))
    }

    @Test
    fun nextSetNumberIsOneBasedAndClamped() {
        assertEquals(1, nextSetNumber(item(sets = 5, setsDone = 0)))
        assertEquals(3, nextSetNumber(item(sets = 5, setsDone = 2)))
        // A fully-logged item reads "5 of 5", never "6 of 5".
        assertEquals(5, nextSetNumber(item(sets = 5, setsDone = 5)))
        assertEquals(5, nextSetNumber(item(sets = 5, setsDone = 9)))
    }

    @Test
    fun setSlotMarksFillExactlyTheCountedSetsAndShowShortReps() {
        assertEquals(listOf("", "", null, null, null), setSlotMarks(item(sets = 5, setsDone = 2)))
        assertEquals(listOf<String?>(null), setSlotMarks(item(sets = 0, setsDone = 0)))
        // A counted set short of the rep target shows its reps instead of a ✓.
        val short = PrescriptionView(
            exercise = "Back Squat", sets = 3, sets_done = 2, reps_low = 5, reps_high = 5,
            logged_sets = listOf(
                ItemSetView(entry_id = 1, reps = 5, qualifies = true),
                ItemSetView(entry_id = 2, reps = 3, qualifies = false),
                ItemSetView(entry_id = 3, reps = 4, qualifies = true),
            ),
        )
        assertEquals(listOf("", "4", null), setSlotMarks(short))
    }

    @Test
    fun firstOpenItemSkipsDoneItems() {
        val squat = item("Back Squat", done = true)
        val bench = item("Bench", done = false)
        // The fast-path must prefill the first NOT-done item, not items.first().
        assertSame(bench, firstOpenItem(listOf(squat, bench)))
        assertNull(firstOpenItem(listOf(squat, item("Row", done = true))))
    }

    @Test
    fun nextOpenItemSkipsCurrentAndDone() {
        val squat = item("Back Squat", done = true)
        val bench = item("Bench", done = false)
        val row = item("Row", done = false)
        assertSame(bench, nextOpenItem(listOf(squat, bench, row), "Back Squat"))
        // Excludes the exercise just completed even if listed first among the open.
        assertSame(row, nextOpenItem(listOf(bench, row), "bench"))
    }

    @Test
    fun liftSetLineRendersHoldsWithoutRepsOrRpe() {
        assertEquals(res.getString(R.string.log_set_line_hold, 30), liftSetLine(res, 30, 0.0, 1, 0.0, false, WeightUnit.Kg))
        // A loaded hold shows the added load, still no reps/RPE.
        assertEquals(
            res.getString(R.string.log_set_line_hold_loaded, 30, load("10", "kg")),
            liftSetLine(res, 30, 10.0, 1, 8.0, true, WeightUnit.Kg),
        )
    }

    @Test
    fun liftSetLineOmitsRpeUnlessRecorded() {
        val base = res.getString(R.string.log_set_line_reps, load("100", "kg"), 5)
        assertEquals(base, liftSetLine(res, null, 100.0, 5, 0.0, false, WeightUnit.Kg))
        assertEquals(
            res.getString(R.string.log_set_line_effort, base, repsLeftText(res, 8.0)),
            liftSetLine(res, null, 100.0, 5, 8.0, true, WeightUnit.Kg),
        )
    }

    @Test
    fun liftSetLineShowsWholeOrHalfPoundsNeverATrailingZero() {
        // 61.23 kg is 134.99 lb: shown as 135, not "135.0".
        assertEquals(res.getString(R.string.log_set_line_reps, load("135", "lb"), 5), liftSetLine(res, null, 61.23, 5, 0.0, false, WeightUnit.Lb))
        assertEquals(res.getString(R.string.log_set_line_reps, load("101.25", "kg"), 3), liftSetLine(res, null, 101.25, 3, 0.0, false, WeightUnit.Kg))
    }

    @Test
    fun setTargetBandBuildsRepAndRpeBands() {
        assertEquals(
            res.getString(R.string.log_band_reps_range, 5, 8) + " \u00b7 " + res.getString(R.string.log_band_effort, repsLeftText(res, 8.0)),
            setTargetBand(res, PrescriptionView(reps_low = 5, reps_high = 8, target_rpe = 8.0)),
        )
        assertEquals(
            res.getQuantityString(R.plurals.log_band_reps, 5, 5),
            setTargetBand(res, PrescriptionView(reps_low = 5, reps_high = 5)),
        )
        assertEquals(
            res.getQuantityString(R.plurals.log_band_reps, 3, 3) + " \u00b7 85% e1RM",
            setTargetBand(res, PrescriptionView(reps_low = 3, reps_high = 3, intensity_label = "85% e1RM")),
        )
        // A single: the singular, never "1 reps".
        assertEquals(
            res.getQuantityString(R.plurals.log_band_reps, 1, 1),
            setTargetBand(res, PrescriptionView(reps_low = 1, reps_high = 1)),
        )
        assertNotEquals(res.getQuantityString(R.plurals.log_band_reps, 2, 1), setTargetBand(res, PrescriptionView(reps_low = 1, reps_high = 1)))
        assertEquals(
            res.getString(R.string.log_band_hold_range, 20, 45),
            setTargetBand(res, PrescriptionView(timed_hold = true, hold_low_sec = 20, hold_high_sec = 45)),
        )
        assertEquals(
            res.getString(R.string.log_band_hold, 30),
            setTargetBand(res, PrescriptionView(timed_hold = true, hold_low_sec = 30, hold_high_sec = 30)),
        )
    }

    @Test
    fun runProgressTextHonoursDistanceUnit() {
        val item = PrescriptionView(run_logged_km = 4.0, run_logged_min = 27.0)
        assertEquals(distance("4", DistanceUnit.Km) + " \u00b7 27:00", runProgressText(res, item, DistanceUnit.Km))
        // 4 km ≈ 2.49 mi (rounded by trimDecimal); the time is unit-free.
        assertEquals(distance("2.5", DistanceUnit.Mi) + " \u00b7 27:00", runProgressText(res, item, DistanceUnit.Mi))
        assertEquals("", runProgressText(res, PrescriptionView(), DistanceUnit.Km))
        // Under one unit two decimals stay, as on the run card: 0.225 km ≈ 0.14 mi.
        assertEquals(distance("0.14", DistanceUnit.Mi), runProgressText(res, PrescriptionView(run_logged_km = 0.225), DistanceUnit.Mi))
    }

    @Test
    fun runProgressTextShowsTheTimeAsAClockNotDecimalMinutes() {
        // 3.7 min is 3:42, never "3.7 min".
        assertEquals("3:42", runProgressText(res, PrescriptionView(run_logged_min = 3.7), DistanceUnit.Km))
        assertEquals("1:19:05", fmtRunTime(79 * 60 + 5))
    }

    @Test
    fun effortSaysHowManyRepsWereLeft() {
        assertEquals(res.getQuantityString(R.plurals.log_reps_left, 0, 0), repsLeftText(res, 10.0))
        assertEquals(res.getString(R.string.log_reps_left_range, 0, 1), repsLeftText(res, 9.5))
        assertEquals(res.getQuantityString(R.plurals.log_reps_left, 1, 1), repsLeftText(res, 9.0))
        assertEquals(res.getQuantityString(R.plurals.log_reps_left, 2, 2), repsLeftText(res, 8.0))
        assertEquals(res.getString(R.string.log_reps_left_range, 2, 3), repsLeftText(res, 7.5))
        assertEquals(res.getString(R.string.log_reps_left_4plus), repsLeftText(res, 6.0))
        assertEquals(res.getString(R.string.log_reps_left_4plus), repsLeftText(res, 5.0))
    }

    @Test
    fun resolveSaveWeightKgPrefersTheVerbatimLoadOnlyWhenUntouched() {
        // Untouched Save with a seeded/verbatim load emits it losslessly.
        assertEquals(100.0, resolveSaveWeightKg(false, true, 100.0, "100", WeightUnit.Kg)!!, 0.0)
        // The lifter types 102.5 (weightFresh cleared) → the parsed buffer wins.
        assertEquals(102.5, resolveSaveWeightKg(false, false, 100.0, "102.5", WeightUnit.Kg)!!, 0.0)
        // A bodyweight rung logs 0 regardless of the buffer.
        assertEquals(0.0, resolveSaveWeightKg(true, true, 100.0, "999", WeightUnit.Kg)!!, 0.0)
    }

    @Test
    fun resolveSaveWeightKgConvertsTypedPoundsToKg() {
        // A touched lb buffer converts lb→kg; 225 lb ≈ 102.06 kg.
        assertEquals(102.058, resolveSaveWeightKg(false, false, null, "225", WeightUnit.Lb)!!, 0.01)
    }

    @Test
    fun fmtClockPadsSeconds() {
        assertEquals("0:05", fmtClock(5))
        assertEquals("1:30", fmtClock(90))
        assertEquals("0:00", fmtClock(-3))
    }

    @Test
    fun actionMessagesStayUpLongEnoughAndHonourTheAccessibilityTimeout() {
        assertEquals(10_000L, actionMessageMillis(long = false, a11y = null))
        assertEquals(15_000L, actionMessageMillis(long = true, a11y = null))
        val slowReader = object : AccessibilityManager {
            override fun calculateRecommendedTimeoutMillis(
                originalTimeoutMillis: Long,
                containsIcons: Boolean,
                containsText: Boolean,
                containsControls: Boolean,
            ): Long = if (containsControls && containsText) 60_000L else originalTimeoutMillis
        }
        assertEquals(60_000L, actionMessageMillis(long = false, a11y = slowReader))
    }
}
