package app.milestone

import android.text.format.DateUtils

/**
 * The shell's wall clock: every "now" the app stamps or shows (today's epoch-day,
 * new-entry timestamps, relative dates, rest readouts) reads it, so shifting it
 * moves all of them together. [offsetMillis] is 0 in production; the offline UI
 * suite pins the clock to a fixed training day so its scenarios are independent
 * of the weekday they run on. Sensor time (GPS fixes) and entry-id uniqueness
 * keep reading the system clock.
 */
internal object AppClock {
    @Volatile var offsetMillis: Long = 0L

    fun nowMillis(): Long = System.currentTimeMillis() + offsetMillis

    fun nowSec(): Long = nowMillis() / 1000L
}

/** Today's local epoch-day (days since 1970-01-01), for GeneratePlan/SetToday. */
internal fun todayEpochDay(): Long =
    java.time.Instant.ofEpochMilli(AppClock.nowMillis())
        .atZone(java.time.ZoneId.systemDefault())
        .toLocalDate()
        .toEpochDay()

/** Today's epoch-day for every plan gate: the core's clock echoed on the view
 *  (SetToday) rather than the wall clock, so a session that crosses midnight
 *  while foregrounded rolls over in lockstep with the core instead of stranding
 *  its Log buttons. Falls back to the device clock on an old core. */
internal fun ViewModel.today(): Long = today_epoch_day ?: todayEpochDay()

/** The device's current UTC offset in SECONDS east of UTC, so the core can
 *  bucket a logged session's UTC `observed_at` into the correct local day. Uses
 *  the offset at "now" (DST-correct for the current instant). */
internal fun utcOffsetSec(): Int = java.util.TimeZone.getDefault().getOffset(AppClock.nowMillis()) / 1000

/**
 * Friendly log date for a history card from a unix-seconds stamp: "2 hours ago",
 * "Yesterday", "Jul 15". Empty for an undated entry.
 */
internal fun formatLogDate(epochSec: Long, nowMillis: Long = AppClock.nowMillis()): String =
    if (epochSec <= 0L) {
        ""
    } else {
        DateUtils.getRelativeTimeSpanString(
            epochSec * 1000L,
            nowMillis,
            DateUtils.DAY_IN_MILLIS,
            DateUtils.FORMAT_ABBREV_RELATIVE,
        ).toString()
    }
