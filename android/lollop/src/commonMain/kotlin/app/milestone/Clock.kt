package app.milestone

import kotlin.concurrent.Volatile
import kotlin.math.abs
import kotlin.time.Instant
import kotlinx.datetime.LocalDate
import kotlinx.datetime.TimeZone
import kotlinx.datetime.atStartOfDayIn
import kotlinx.datetime.atTime
import kotlinx.datetime.offsetAt
import kotlinx.datetime.toInstant
import kotlinx.datetime.toLocalDateTime

/** The device's wall clock, milliseconds since the epoch (System.currentTimeMillis). */
internal expect fun wallClockMillis(): Long

/** Milliseconds on a clock that never jumps and keeps counting in deep sleep
 *  (Android `SystemClock.elapsedRealtime`): for durations, never for dates. */
internal expect fun monotonicMillis(): Long

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

    fun nowMillis(): Long = wallClockMillis() + offsetMillis

    fun nowSec(): Long = nowMillis() / 1000L
}

/** The local epoch-day (days since 1970-01-01) of the instant [epochMillis] in [zone]. */
internal fun localEpochDayAtMillis(epochMillis: Long, zone: TimeZone = TimeZone.currentSystemDefault()): Long =
    Instant.fromEpochMilliseconds(epochMillis).toLocalDateTime(zone).date.toEpochDays()

/** Today's local epoch-day (days since 1970-01-01), for GeneratePlan/SetToday. */
internal fun todayEpochDay(): Long = localEpochDayAtMillis(AppClock.nowMillis())

/** The local epoch-day of the unix second [epochSec] in [zone]. */
internal fun localEpochDay(epochSec: Long, zone: TimeZone = TimeZone.currentSystemDefault()): Long =
    Instant.fromEpochSeconds(epochSec).toLocalDateTime(zone).date.toEpochDays()

/** Unix second of the local midnight that starts the day containing [epochSec]
 *  (the first moment of the day when a DST change skips midnight). */
internal fun localDayStartSec(epochSec: Long, zone: TimeZone = TimeZone.currentSystemDefault()): Long =
    Instant.fromEpochSeconds(epochSec).toLocalDateTime(zone).date.atStartOfDayIn(zone).epochSeconds

/** Local noon of [epochDay] in [zone] as a unix second: the seed stamp for logging
 *  a PAST prescribed session from the week strip. Noon avoids DST/midnight edge
 *  cases and lands the entry squarely on that calendar day. */
internal fun planDayObservedAt(epochDay: Long, zone: TimeZone = TimeZone.currentSystemDefault()): Long =
    LocalDate.fromEpochDays(epochDay).atTime(12, 0).toInstant(zone).epochSeconds

/** Today's epoch-day for every plan gate: the core's clock echoed on the view
 *  (SetToday) rather than the wall clock, so a session that crosses midnight
 *  while foregrounded rolls over in lockstep with the core instead of stranding
 *  its Log buttons. Falls back to the device clock on an old core. */
internal fun ViewModel.today(): Long = today_epoch_day ?: todayEpochDay()

/** Today's session still to train: the hero dated today, with items, not blocked. */
internal fun ViewModel.todaysPlan(): SessionPlanView? =
    next_session?.takeIf { it.epoch_day == today() && it.items.isNotEmpty() && it.actionable }

/** Today's lift session once something of it is logged. After Finish the core moves
 *  next_session off today, so this is how the day is resumed. */
internal fun ViewModel.resumableLiftDay(): SessionPlanView? = week_plan.firstOrNull {
    it.epoch_day == today() && it.items.isNotEmpty() && sessionDiscipline(it.session_type) == "Lift" &&
        it.items.any { i -> i.sets_done > 0 || i.logged_sets.isNotEmpty() }
}

/** The device's current UTC offset in SECONDS east of UTC, so the core can
 *  bucket a logged session's UTC `observed_at` into the correct local day. Uses
 *  the offset at "now" (DST-correct for the current instant). */
internal fun utcOffsetSec(): Int =
    TimeZone.currentSystemDefault().offsetAt(Instant.fromEpochMilliseconds(AppClock.nowMillis())).totalSeconds

/**
 * Friendly log date for a history card from a unix-seconds stamp, in whole local
 * days within a week ("Today", "Yesterday", "3 days ago", the wording DateUtils
 * uses), a date ("Jul 15") from a week back. In the app's language with Latin
 * digits ([appLocale]), not the device's. Empty for an undated entry.
 */
internal fun formatLogDate(
    epochSec: Long,
    nowMillis: Long = AppClock.nowMillis(),
    locale: PlatformLocale = appLocale(),
    zone: TimeZone = TimeZone.currentSystemDefault(),
): String {
    if (epochSec <= 0L) return ""
    val day = localEpochDayAtMillis(epochSec * 1000L, zone)
    val today = localEpochDayAtMillis(nowMillis, zone)
    if (abs(nowMillis - epochSec * 1000L) >= WEEK_MILLIS) return epochDayShortLabel(day, today, locale)
    return relativeDayLabel(day - today, locale)
}

private const val WEEK_MILLIS = 7 * 24 * 3_600_000L
