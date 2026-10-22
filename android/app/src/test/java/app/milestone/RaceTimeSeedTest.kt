package app.milestone

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The race predictor form seeds its "MM"/"SS" keypad buffers from the core's
 * echoed `recent_time_sec`. Splitting the seconds naively (`round(t % 60)`) can
 * rehydrate an invalid "60"-second buffer for a fractional echo like 22:59.7;
 * [raceTimeSeed] must round to whole seconds FIRST and carry into the minute.
 */
class RaceTimeSeedTest {

    @Test
    fun wholeTimesSplitCleanly() {
        assertEquals("22" to "0", raceTimeSeed(1320.0)) // 22:00
        assertEquals("5" to "30", raceTimeSeed(330.0)) // 5:30
        assertEquals("0" to "0", raceTimeSeed(0.0))
    }

    @Test
    fun fractionNearAFullMinuteCarriesIntoMinutes() {
        // 22:59.7 must rehydrate as 23:00, never the invalid 22:60.
        assertEquals("23" to "0", raceTimeSeed(1379.7))
        // 1:59.6 → round 120 → 2:00.
        assertEquals("2" to "0", raceTimeSeed(119.6))
    }

    @Test
    fun subSecondFractionRoundsToNearest() {
        assertEquals("0" to "1", raceTimeSeed(0.5)) // half rounds up
        assertEquals("0" to "0", raceTimeSeed(0.4)) // rounds down
        assertEquals("0" to "45", raceTimeSeed(45.3))
    }

    @Test
    fun secondsBufferIsNeverSixty() {
        // The pre-fix bug: any t whose fractional remainder rounded to 60. Sweep a
        // range with a .7 fraction (the worst case for the old `round(t % 60)`) and
        // assert the seconds buffer stays in 0..59 for every reachable time.
        for (t in 0..7200) {
            val (_, sec) = raceTimeSeed(t + 0.7)
            assertTrue("seconds out of range for $t", sec.toInt() in 0..59)
        }
    }
}
