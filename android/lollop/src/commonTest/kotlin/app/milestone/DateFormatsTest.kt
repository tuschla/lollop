package app.milestone

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlinx.datetime.DayOfWeek
import kotlinx.datetime.LocalDate
import kotlinx.datetime.LocalDateTime
import kotlinx.datetime.TimeZone
import kotlinx.datetime.atTime
import kotlinx.datetime.toInstant

/**
 * Dates read the same on both platforms. The expected strings are Android's
 * (java.time over ICU's best patterns, ICU's relative dates), which the Android
 * actuals produce by construction; on iOS this holds NSDateFormatter and
 * NSRelativeDateTimeFormatter to them.
 */
class DateFormatsTest : PlatformTest() {
    private val us = testLocale("en-US")
    private val uk = testLocale("en-GB")
    private val oct15 = LocalDate(2026, 10, 15) // a Thursday

    @Test
    fun skeletonsFollowTheRegionsOrder() {
        assertEquals("Oct 15", localizedDayFormatter("MMMd", us).format(oct15))
        assertEquals("15 Oct", localizedDayFormatter("MMMd", uk).format(oct15))
        assertEquals("Thu, Oct 15", localizedDayFormatter("EEEMMMd", us).format(oct15))
        assertEquals("October 2026", localizedDayFormatter("yMMMM", us).format(oct15))
        assertEquals("Oct 15, 2026", localizedDayFormatter("yMMMd", us).format(oct15))
        assertEquals("Thu", localizedDayFormatter("EEE", us).format(oct15))
        assertEquals("14:05", localizedDayFormatter("Hmm", us).format(LocalDateTime(2026, 10, 15, 14, 5)))
        assertEquals("Oct 15", localizedDayFormatter("MMMd", us).formatEpochDay(oct15.toEpochDays()))
    }

    @Test
    fun weekdayNamesAtEachWidth() {
        assertEquals("Thursday", weekdayName(DayOfWeek.THURSDAY, WeekdayWidth.Full, us))
        assertEquals("Thu", weekdayName(DayOfWeek.THURSDAY, WeekdayWidth.Short, us))
        assertEquals("Thu", weekdayName(DayOfWeek.THURSDAY, WeekdayWidth.ShortStandalone, us))
        assertEquals("T", weekdayName(DayOfWeek.THURSDAY, WeekdayWidth.Narrow, us))
        assertEquals("Monday", weekdayName(DayOfWeek.MONDAY, WeekdayWidth.Full, us))
        assertEquals("Sun", weekdayName(DayOfWeek.SUNDAY, WeekdayWidth.Short, us))
    }

    @Test
    fun mediumDatesAndShortLabels() {
        assertEquals("Oct 15, 2026", formatMediumDate(oct15, us))
        assertEquals("15 Oct 2026", formatMediumDate(oct15, uk))
        val today = oct15.toEpochDays()
        assertEquals("Sep 11", epochDayShortLabel(LocalDate(2026, 9, 11).toEpochDays(), today, us))
        assertEquals("Dec 30, 2025", epochDayShortLabel(LocalDate(2025, 12, 30).toEpochDays(), today, us))
    }

    @Test
    fun relativeDaysAreNamedThenCounted() {
        assertEquals("Today", relativeDayLabel(0, us))
        assertEquals("Yesterday", relativeDayLabel(-1, us))
        assertEquals("Tomorrow", relativeDayLabel(1, us))
        assertEquals("2 days ago", relativeDayLabel(-2, us))
        assertEquals("3 days ago", relativeDayLabel(-3, us))
        assertEquals("In 3 days", relativeDayLabel(3, us))
    }

    @Test
    fun logDatesSwitchToADateAfterAWeek() {
        val zone = TimeZone.UTC
        val now = oct15.atTime(12, 0).toInstant(zone).toEpochMilliseconds()
        fun daysAgo(n: Long) = now / 1000L - n * 86_400L
        assertEquals("Today", formatLogDate(daysAgo(0), now, us, zone))
        assertEquals("Yesterday", formatLogDate(daysAgo(1), now, us, zone))
        assertEquals("6 days ago", formatLogDate(daysAgo(6), now, us, zone))
        assertEquals("Oct 1", formatLogDate(daysAgo(14), now, us, zone))
        assertEquals("1 Oct", formatLogDate(daysAgo(14), now, uk, zone))
        assertEquals("Oct 15, 2025", formatLogDate(daysAgo(365), now, us, zone))
        assertEquals("", formatLogDate(0L, now, us, zone))
    }
}
