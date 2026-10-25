package app.milestone.qa

import android.content.Context
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.test.core.app.ApplicationProvider
import app.milestone.DistanceUnit
import app.milestone.RunResultView
import app.milestone.HistoryDestination
import app.milestone.MilestoneTheme
import app.milestone.ViewModel
import app.milestone.WeightUnit
import app.milestone.buildCoachSummary
import app.milestone.buildMonthNav
import app.milestone.formatLogDate
import app.milestone.historyActivityDays
import app.milestone.historyRunStream
import app.milestone.latestRunOnDay
import app.milestone.planDateLabel
import app.milestone.recordDaySec
import app.milestone.recordRun
import app.milestone.runDateChip
import app.milestone.runDaySec
import app.milestone.runKmByDay
import app.milestone.runTitle
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.ZoneId
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import java.util.Locale
import java.util.TimeZone
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * One rule for a run's day: a run belongs to the local day it STARTED (the core's
 * `started_at`), the day its title and date chip name. History's month headers and
 * stream order, the heatmap (and its tap), the daily distance chart, the records'
 * dates and the coach summary all file a run there, never under the day it ended
 * (`observed_at`, a tracked run's last fix). Each assertion fails if that place
 * buckets by `observed_at`.
 */
@RunWith(RobolectricTestRunner::class)
class RunDayGroupingTest {

    @get:Rule
    val rule = createComposeRule()

    private val utc: ZoneId = ZoneOffset.UTC
    private val savedZone = TimeZone.getDefault()
    private val savedLocale = Locale.getDefault()

    @Before
    fun setUp() {
        // runTitle and the date chip read the device zone/locale; pin both.
        TimeZone.setDefault(TimeZone.getTimeZone("UTC"))
        Locale.setDefault(Locale.US)
    }

    @After
    fun tearDown() {
        TimeZone.setDefault(savedZone)
        Locale.setDefault(savedLocale)
    }

    private fun at(y: Int, mo: Int, d: Int, h: Int, mi: Int): Long =
        LocalDateTime.of(y, mo, d, h, mi).toEpochSecond(ZoneOffset.UTC)

    private fun day(y: Int, mo: Int, d: Int): Long = LocalDate.of(y, mo, d).toEpochDay()

    /** Sun 18 Oct 23:40 → Mon 19 Oct 00:20: across midnight. */
    private val acrossMidnight = RunResultView(
        distance_km = 7.0, duration_min = 40.0, entry_id = 1, has_gpx = true,
        started_at = at(2026, 10, 18, 23, 40), observed_at = at(2026, 10, 19, 0, 20),
    )

    /** Sat 31 Oct 23:40 → Sun 1 Nov 00:20: across a month boundary. */
    private val acrossMonth = RunResultView(
        distance_km = 8.0, duration_min = 40.0, entry_id = 2, has_gpx = true,
        started_at = at(2026, 10, 31, 23, 40), observed_at = at(2026, 11, 1, 0, 20),
    )

    /** The core lists runs by `observed_at`, oldest first. */
    private val runs = listOf(acrossMidnight, acrossMonth)

    /** Two weeks after the last run: the date chips are absolute dates. */
    private val now = at(2026, 11, 14, 12, 0) * 1000L

    @Test
    fun aRunAcrossMidnightOrAMonthEndIsFiledUnderTheDayAndMonthItStarted() {
        // Title and chip name the START day (the reference the grouping must match).
        var titles = emptyList<String>()
        rule.setContent { titles = runs.map { runTitle(it) } }
        rule.waitForIdle()
        assertEquals(listOf("Sunday run", "Saturday run"), titles)
        assertEquals(formatLogDate(acrossMidnight.started_at, now), runDateChip(acrossMidnight, now))
        assertEquals(formatLogDate(acrossMonth.started_at, now), runDateChip(acrossMonth, now))
        val chips = runs.map { runDateChip(it, now) }
        assertTrue("$chips", "18" in chips[0] && "19" !in chips[0] && "31" in chips[1] && "Oct" in chips[1])

        // History month headers: both runs started in October, so ONE "October 2026"
        // header heads the stream; by observed_at the Oct 31 run would sit under
        // "November 2026".
        val stream = historyRunStream(runs)
        assertEquals(listOf(acrossMonth, acrossMidnight), stream)
        val fmt = DateTimeFormatter.ofPattern("MMMM yyyy", Locale.US)
        val nav = buildMonthNav(stream.map(::runDaySec), base = 0, zone = utc, fmt = fmt)
        assertEquals(mapOf(0 to "October 2026"), nav.headerBefore)

        // Heatmap: the cells are the start days (Sun 18 Oct, Sat 31 Oct), and a tap on
        // one opens its run; the end days (Mon 19 Oct, Sun 1 Nov) are empty.
        val days = historyActivityDays(emptyList(), runs, utc)
        assertEquals(setOf(day(2026, 10, 18), day(2026, 10, 31)), days.keys)
        assertTrue(days.values.all { it.runs == 1 && !it.lifted })
        assertSame(acrossMidnight, latestRunOnDay(runs, day(2026, 10, 18), utc))
        assertSame(acrossMonth, latestRunOnDay(runs, day(2026, 10, 31), utc))
        assertNull(latestRunOnDay(runs, day(2026, 10, 19), utc))
        assertNull(latestRunOnDay(runs, day(2026, 11, 1), utc))

        // Daily distance chart: km on the start days.
        assertEquals(mapOf(day(2026, 10, 18) to 7.0, day(2026, 10, 31) to 8.0), runKmByDay(runs, utc))

        // Records (the core keys them by observed_at): the cell dates the run's start,
        // the same date its card's chip shows.
        for (r in runs) {
            val longest = recordRun(runs, null, r.observed_at)
            assertSame(r, longest)
            assertSame(r, recordRun(runs, r.entry_id, r.observed_at))
            assertEquals(runDateChip(r, now), formatLogDate(recordDaySec(longest, r.observed_at), now))
        }

        // Coach summary: each run is listed under its start day's line.
        val res = ApplicationProvider.getApplicationContext<Context>().resources
        val lines = buildCoachSummary(
            res, ViewModel(runs = runs), today = day(2026, 11, 2), zone = utc,
            weightUnit = WeightUnit.Kg, distanceUnit = DistanceUnit.Km, weeks = 4,
        ).lines()
        for (start in listOf(day(2026, 10, 18), day(2026, 10, 31))) {
            val i = lines.indexOf(planDateLabel(start))
            assertTrue("a day line for ${planDateLabel(start)}:\n${lines.joinToString("\n")}", i >= 0)
            assertTrue(lines[i + 1], lines[i + 1].startsWith("- "))
        }
        for (end in listOf(day(2026, 10, 19), day(2026, 11, 1))) {
            assertTrue("no day line for ${planDateLabel(end)}", planDateLabel(end) !in lines)
        }
    }

    /** The History screen itself: the Oct 31 → Nov 1 run sits under the one
     *  "October 2026" header with the Oct 18 run (an overline, set in capitals);
     *  no "November 2026" header exists. */
    @Test
    @Config(qualifiers = "w411dp-h2400dp")
    fun historyScreenHeadsBothRunsWithTheMonthTheyStarted() {
        rule.setContent { MilestoneTheme { HistoryDestination(model = ViewModel(runs = runs)) } }
        rule.waitForIdle()
        assertEquals(1, rule.onAllNodesWithText("OCTOBER 2026").fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodesWithText("NOVEMBER 2026").fetchSemanticsNodes().size)
        assertEquals(1, rule.onAllNodesWithText("Saturday run").fetchSemanticsNodes().size)
    }

    /** Overlapping runs (a long run with a short one logged inside it) end in the
     *  other order than they start: History's stream is in START order, so a month
     *  is one contiguous block with one header (a repeated header would repeat its
     *  LazyColumn key). */
    @Test
    fun overlappingRunsKeepOneHeaderPerMonth() {
        val long = RunResultView(
            distance_km = 30.0, duration_min = 180.0, entry_id = 3,
            started_at = at(2026, 10, 31, 23, 0), observed_at = at(2026, 11, 1, 2, 0),
        )
        val inside = RunResultView(
            distance_km = 5.0, duration_min = 30.0, entry_id = 4,
            started_at = at(2026, 11, 1, 0, 30), observed_at = at(2026, 11, 1, 1, 0),
        )
        val later = RunResultView(
            distance_km = 5.0, duration_min = 30.0, entry_id = 5,
            started_at = at(2026, 11, 3, 7, 0), observed_at = at(2026, 11, 3, 7, 30),
        )
        val stream = historyRunStream(listOf(inside, long, later))
        assertEquals(listOf(later, inside, long), stream)
        val fmt = DateTimeFormatter.ofPattern("MMMM yyyy", Locale.US)
        val nav = buildMonthNav(stream.map(::runDaySec), base = 0, zone = utc, fmt = fmt)
        assertEquals(mapOf(0 to "November 2026", 2 to "October 2026"), nav.headerBefore)
    }
}
