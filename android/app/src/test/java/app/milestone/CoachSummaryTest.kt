package app.milestone

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import java.time.LocalDate
import java.time.ZoneOffset
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/** The plain-text coach summary: the week window, per-day grouping, set folding
 *  and the user's units. Pure builder over a hand-made view. */
@RunWith(RobolectricTestRunner::class)
class CoachSummaryTest {

    private val res get() = ApplicationProvider.getApplicationContext<Context>().resources

    @Before
    fun setUp() = Locale.setDefault(Locale.US)

    private fun day(y: Int, m: Int, d: Int) = LocalDate.of(y, m, d).toEpochDay()
    private fun at(day: Long, minute: Int = 0) = day * 86_400L + 43_200L + minute * 60L

    /** Wednesday 2026-09-23. */
    private val today = day(2026, 9, 23)
    private val monday = day(2026, 9, 21)
    private val lastMonday = day(2026, 9, 14)

    private fun squat(kg: Double, reps: Int, minute: Int) =
        LiftResultView(exercise = "Back Squat", weight_kg = kg, reps = reps, observed_at = at(monday, minute))

    private val model = ViewModel(
        lifts = listOf(
            squat(100.0, 5, 0), squat(100.0, 5, 3), squat(100.0, 5, 6), squat(110.0, 3, 9),
            LiftResultView(exercise = "Front Lever", reps = 1, hold_sec = 30, observed_at = at(monday, 12)),
            LiftResultView(exercise = "Front Lever", reps = 1, hold_sec = 30, observed_at = at(monday, 14)),
            LiftResultView(exercise = "Push-up", reps = 12, observed_at = at(monday, 16)),
            // Undated: no day, never in the summary.
            LiftResultView(exercise = "Deadlift", weight_kg = 140.0, reps = 3, observed_at = 0),
        ),
        runs = listOf(
            // Sunday before the window: left out.
            RunResultView(distance_km = 12.0, duration_min = 60.0, observed_at = at(day(2026, 9, 13))),
            RunResultView(distance_km = 8.04672, duration_min = 40.0, observed_at = at(day(2026, 9, 22)), session_rpe = 6),
        ),
        activities = listOf(
            ActivityResultView(
                entry_id = 1, observed_at = at(day(2026, 9, 15)), kind = ActivityKind.Sport,
                minutes = 45.0, vigorous = true, strengthening = true,
            ),
        ),
        profile = ProfileView(weight_lb = true),
    )

    private fun summary(weeks: Int = 2) =
        buildCoachSummary(res, model, today, ZoneOffset.UTC, WeightUnit.Lb, DistanceUnit.Mi, weeks)

    @Test
    fun headerStatesTheWindowAndEveryWeekIsListed() {
        val text = summary()
        assertTrue(text, text.startsWith(res.getString(R.string.coach_export_header, epochDayLabel(lastMonday), epochDayLabel(today))))
        assertTrue(text, text.contains(res.getString(R.string.coach_export_week, planDateLabel(lastMonday))))
        assertTrue(text, text.contains(res.getString(R.string.coach_export_week, planDateLabel(monday))))
        assertFalse("nothing before the window", text.contains("7.46 mi"))
        assertFalse("undated rows have no day", text.contains("Deadlift"))
    }

    @Test
    fun liftsFoldIdenticalSetsAndUseTheWeightUnit() {
        val text = summary()
        assertTrue(text, text.contains("- Back Squat: 3 × 5 @ 220.5 lb, 1 × 3 @ 242.5 lb"))
        assertTrue(text, text.contains("- Front Lever: 2 × 30 s hold"))
        assertTrue(text, text.contains("- Push-up: 1 × 12"))
    }

    @Test
    fun runsAndActivitiesReadInTheUsersUnits() {
        val text = summary()
        assertTrue(text, text.contains("- Run: 5.00 mi · 40:00 · ${runPaceLabel(8.04672, 40.0, DistanceUnit.Mi)} · effort 6/10"))
        assertTrue(text, text.contains("- Sport or PE: 45 min · vigorous · ${res.getString(R.string.coach_export_strengthening)}"))
    }

    @Test
    fun weekTotalsCountSessionsDistanceAndActiveMinutesNamingOnlyTheirSources() {
        val text = summary()
        assertTrue(text, text.contains("Totals: 1 lifting session · 1 run, 5.00 mi · 40 active minutes from runs\n"))
        assertTrue(text, text.contains("Totals: 45 active minutes from other activity\n"))
        assertFalse(text, text.contains("from runs and other activity"))
        val both = buildCoachSummary(
            res,
            model.copy(
                activities = model.activities + ActivityResultView(
                    entry_id = 2, observed_at = at(monday, 30), kind = ActivityKind.Walking, minutes = 20.0,
                ),
            ),
            today, ZoneOffset.UTC, WeightUnit.Lb, DistanceUnit.Mi, weeks = 1,
        )
        assertTrue(both, both.contains("60 active minutes from runs and other activity"))
    }

    @Test
    fun theWindowFollowsTheWeekCount() {
        val one = summary(weeks = 1)
        assertFalse(one, one.contains("Sport or PE"))
        assertTrue(one, one.contains("Back Squat"))
        val four = summary(weeks = 4)
        assertTrue(four, four.contains("7.46 mi"))
        assertTrue(four, four.contains(res.getString(R.string.coach_export_week_empty, planDateLabel(day(2026, 8, 31)))))
    }

    @Test
    fun consecutiveEmptyWeeksFoldIntoOneLine() {
        val text = summary(weeks = 6)
        val folded = res.getString(R.string.coach_export_weeks_empty, planDateLabel(day(2026, 8, 17)), planDateLabel(day(2026, 8, 31)))
        assertTrue(text, text.contains(folded))
        assertFalse(text, text.contains(res.getString(R.string.coach_export_week, planDateLabel(day(2026, 8, 24)))))
        assertEquals("one line for the three empty weeks", 1, text.lines().count { it.contains("nothing logged") })
        // The weeks with entries still follow, each under its own heading.
        assertTrue(text, text.contains(res.getString(R.string.coach_export_week, planDateLabel(day(2026, 9, 7)))))
    }
}
