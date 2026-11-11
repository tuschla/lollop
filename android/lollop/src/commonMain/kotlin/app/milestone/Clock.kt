package app.milestone

import kotlin.math.abs
import kotlin.time.Instant
import kotlinx.datetime.LocalDate
import kotlinx.datetime.TimeZone
import kotlinx.datetime.atStartOfDayIn
import kotlinx.datetime.atTime
import kotlinx.datetime.offsetAt
import kotlinx.datetime.toInstant
import kotlinx.datetime.toLocalDateTime

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

/** The start of the local day after the one containing [nowMillis] in [zone], in
 *  epoch millis (the first moment of that day when a DST change skips midnight). */
internal fun nextLocalMidnightMillis(nowMillis: Long, zone: TimeZone = TimeZone.currentSystemDefault()): Long =
    LocalDate.fromEpochDays(localEpochDayAtMillis(nowMillis, zone) + 1).atStartOfDayIn(zone).toEpochMilliseconds()

/** Local noon of [epochDay] in [zone] as a unix second: the seed stamp for logging
 *  a PAST prescribed session from the week strip. Noon avoids DST/midnight edge
 *  cases and lands the entry squarely on that calendar day. */
internal fun planDayObservedAt(epochDay: Long, zone: TimeZone = TimeZone.currentSystemDefault()): Long =
    LocalDate.fromEpochDays(epochDay).atTime(12, 0).toInstant(zone).epochSeconds

/** The device's current UTC offset in SECONDS east of UTC, so the core can
 *  bucket a logged session's UTC `observed_at` into the correct local day. Uses
 *  the offset at "now" (DST-correct for the current instant). */
internal fun utcOffsetSec(): Long =
    TimeZone.currentSystemDefault().offsetAt(Instant.fromEpochMilliseconds(AppClock.nowMillis())).totalSeconds.toLong()

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

/** Epoch day of the Monday that starts [epochDay]'s week (epoch day 0 was a Thursday). */
internal fun mondayOf(epochDay: Long): Long = epochDay - (epochDay + 3).mod(7L)
