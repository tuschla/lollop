package app.milestone

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.time.Instant
import kotlinx.datetime.TimeZone

/**
 * The session reminder's next fire time and the local-midnight tick, on both
 * platforms. Every expected instant is worked out by hand in UTC from the zone's
 * published offsets (tz database 2026 rules).
 */
class ReminderClockTest {
    private fun at(utc: String) = Instant.parse(utc).toEpochMilliseconds()

    private val berlin = TimeZone.of("Europe/Berlin")

    @Test
    fun nextFireIsTheNextLocalTimeInTheZone() {
        val m = 7 * 60 + 30
        // Berlin is CET (+01:00) on 2026-10-29: 06:00 local is 05:00Z, 07:30 local is 06:30Z.
        assertEquals(at("2026-10-29T06:30:00Z"), SessionReminder.nextFireMillis(at("2026-10-29T05:00:00Z"), m, berlin))
        // At or after the time: tomorrow.
        assertEquals(at("2026-10-30T06:30:00Z"), SessionReminder.nextFireMillis(at("2026-10-29T06:30:00Z"), m, berlin))
        assertEquals(at("2026-10-30T06:30:00Z"), SessionReminder.nextFireMillis(at("2026-10-29T20:00:00Z"), m, berlin))
        // The same instant in another zone: that zone's 07:30 (New York is still EDT, -04:00).
        assertEquals(at("2026-10-29T11:30:00Z"), SessionReminder.nextFireMillis(at("2026-10-29T05:00:00Z"), m, TimeZone.of("America/New_York")))
        // A time the spring-forward jump skips moves forward by the gap: 02:30 on
        // 2026-03-29 does not exist in Berlin (02:00 CET -> 03:00 CEST), so 03:30 CEST.
        assertEquals(at("2026-03-29T01:30:00Z"), SessionReminder.nextFireMillis(at("2026-03-29T00:00:00Z"), 2 * 60 + 30, berlin))
    }

    /** A local time a daylight-saving change repeats or skips: the reminder fires
     *  once, at the first pass; a skipped time moves forward by the gap, also when
     *  the gap is half an hour. */
    @Test
    fun nextFireAcrossDaylightSavingChanges() {
        val m = 2 * 60 + 30
        // Berlin falls back on 2026-10-25 at 03:00 CEST -> 02:00 CET: 02:30 comes at
        // 00:30Z (CEST) and again at 01:30Z (CET).
        assertEquals(at("2026-10-25T00:30:00Z"), SessionReminder.nextFireMillis(at("2026-10-25T00:00:00Z"), m, berlin))
        // Past the first pass, the repeat is not a second reminder: the next is 02:30 CET on the 26th.
        assertEquals(at("2026-10-26T01:30:00Z"), SessionReminder.nextFireMillis(at("2026-10-25T00:45:00Z"), m, berlin))
        // Later on the day Berlin springs forward (2026-03-29), 07:30 is CEST
        // (+02:00): 05:30Z, not an hour late.
        assertEquals(at("2026-03-29T05:30:00Z"), SessionReminder.nextFireMillis(at("2026-03-29T00:00:00Z"), 7 * 60 + 30, berlin))
        // Lord Howe springs forward 02:00 -> 02:30 (+10:30 -> +11:00) on 2026-10-04:
        // 02:15 is skipped, so 02:45 +11:00. Now is 00:30 local (14:00Z on the 3rd).
        val lordHowe = TimeZone.of("Australia/Lord_Howe")
        assertEquals(at("2026-10-03T15:45:00Z"), SessionReminder.nextFireMillis(at("2026-10-03T14:00:00Z"), 2 * 60 + 15, lordHowe))
        // Havana springs forward at midnight (00:00 -05:00 -> 01:00 -04:00) on
        // 2026-03-08: a 00:30 reminder fires at 01:30 -04:00.
        assertEquals(at("2026-03-08T05:30:00Z"), SessionReminder.nextFireMillis(at("2026-03-07T17:00:00Z"), 30, TimeZone.of("America/Havana")))
    }

    /** The midnight tick is the first instant of the next local day: an hour
     *  later on a 25-hour day, the end of the gap when midnight is skipped, and
     *  midnight itself (not the repeated hour before it) when the clock falls back
     *  at midnight. */
    @Test
    fun nextMidnightAcrossDaylightSavingChanges() {
        // Berlin's 2026-10-25 has 25 hours: its end is 00:00 CET on the 26th.
        assertEquals(at("2026-10-25T23:00:00Z"), nextLocalMidnightMillis(at("2026-10-25T12:00:00Z"), berlin))
        // Half a minute before Berlin's midnight that starts 2026-10-25 (CEST, +02:00):
        // that midnight. Exactly at it: the following one, never now.
        assertEquals(at("2026-10-24T22:00:00Z"), nextLocalMidnightMillis(at("2026-10-24T21:59:30Z"), berlin))
        assertEquals(at("2026-10-25T23:00:00Z"), nextLocalMidnightMillis(at("2026-10-24T22:00:00Z"), berlin))
        // Havana skips 2026-03-08 00:00: the day starts at 01:00 -04:00.
        assertEquals(at("2026-03-08T05:00:00Z"), nextLocalMidnightMillis(at("2026-03-07T17:00:00Z"), TimeZone.of("America/Havana")))
        // Santiago falls back at 24:00 -03:00 on 2026-04-04 to 23:00 -04:00: at
        // 23:45 -03:00 (02:45Z) the day still has its repeated hour to run.
        assertEquals(at("2026-04-05T04:00:00Z"), nextLocalMidnightMillis(at("2026-04-05T02:45:00Z"), TimeZone.of("America/Santiago")))
    }
}
