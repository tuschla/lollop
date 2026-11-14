package app.milestone

import kotlinx.datetime.DayOfWeek
import kotlinx.datetime.LocalDate
import kotlinx.datetime.LocalDateTime

// Every date, weekday and time the shell formats outside the string resources
// goes through these, in the app's language with Latin digits ([appLocale]).
// Android: java.time + ICU, exactly as before the multiplatform split; iOS:
// NSDateFormatter / NSRelativeDateTimeFormatter with the same CLDR data. File
// names and exported data keep their fixed ISO patterns and don't use these.

/** A locale: `java.util.Locale` on Android, `NSLocale` on iOS. */
expect class PlatformLocale

/** The app's language with Latin digits ([appLocaleOf]'s rule over the device's locales). */
internal expect fun appLocale(): PlatformLocale

/**
 * The app-locale rule, for both platforms: the app ships English only, so this is
 * the first English locale in the device's [preferred] list ("15 Jul" for en-GB),
 * else [english] (English with the device's region: en-DE on a German phone),
 * then [withLatinDigits] (the `latn` numbering system: "٣" never appears in a date).
 */
internal inline fun <L, P> appLocaleOf(
    preferred: List<L>,
    language: (L) -> String?,
    english: () -> L,
    withLatinDigits: (L) -> P,
): P = withLatinDigits(preferred.firstOrNull { language(it) == "en" } ?: english())

/** The device's primary locale (distance and weight unit defaults read its region). */
internal expect fun deviceLocale(): PlatformLocale

/** [this] locale's ISO 3166 region ("US", "GB"); empty when it has none. */
internal expect val PlatformLocale.region: String

/** A date/time formatter for one CLDR skeleton in one locale ([localizedDayFormatter]). */
internal expect class DayFormatter {
    fun format(date: LocalDate): String

    fun format(dateTime: LocalDateTime): String
}

/** A date formatter for [skeleton] in the app's language and the device region's
 *  order, the platform's best pattern for it ("MMMd" → "Sep 21" / "21 Sept"). */
internal expect fun localizedDayFormatter(skeleton: String, locale: PlatformLocale = appLocale()): DayFormatter

/** [localizedDayFormatter] for a local-day index (days since the epoch). */
internal fun DayFormatter.formatEpochDay(epochDay: Long): String = format(LocalDate.fromEpochDays(epochDay))

/** How long a weekday name is ([weekdayName]); the CLDR widths. */
internal enum class WeekdayWidth {
    /** "Monday" */
    Full,

    /** "Mon" */
    Short,

    /** "Mon" on its own (no surrounding date): the stand-alone form. */
    ShortStandalone,

    /** "M" */
    Narrow,
}

internal expect fun weekdayName(day: DayOfWeek, width: WeekdayWidth, locale: PlatformLocale = appLocale()): String

/** [day]'s short name ("Thu") in the app's language. */
internal fun weekdayShort(day: DayOfWeek, locale: PlatformLocale = appLocale()): String =
    weekdayName(day, WeekdayWidth.Short, locale)

/** [day]'s two-letter label ("Mo", "Tu") in the app's language, for every week
 *  strip (Today's and the widgets'): a single narrow initial collides (M/T/T, S/S). */
internal fun weekdayStripLabel(day: DayOfWeek, locale: PlatformLocale = appLocale()): String =
    weekdayName(day, WeekdayWidth.ShortStandalone, locale).take(2)

/** A local epoch day as a medium date in the app's language ("Oct 6, 2026"). */
internal fun epochDayLabel(day: Long, locale: PlatformLocale = appLocale()): String =
    formatMediumDate(LocalDate.fromEpochDays(day), locale)

/** [day] as a short localized date, dropping the year when it is [today]'s year
 *  ("Sep 11"; "Dec 30, 2025" otherwise), for values that must fit a row. */
internal fun epochDayShortLabel(day: Long, today: Long, locale: PlatformLocale = appLocale()): String {
    val date = LocalDate.fromEpochDays(day)
    if (date.year != LocalDate.fromEpochDays(today).year) return epochDayLabel(day, locale)
    return localizedDayFormatter("MMMd", locale).format(date)
}

/** [date] in the locale's medium date style ("Oct 6, 2026"). */
internal expect fun formatMediumDate(date: LocalDate, locale: PlatformLocale = appLocale()): String

/**
 * A day relative to today, capitalised for the start of a sentence: "Today",
 * "Yesterday", "Tomorrow", "3 days ago", "In 3 days". [dayDelta] is the day
 * minus today (negative = past).
 */
internal expect fun relativeDayLabel(dayDelta: Long, locale: PlatformLocale = appLocale()): String

internal expect fun is24HourClock(ctx: PlatformContext): Boolean

/**
 * Combine a DatePicker day (UTC-midnight millis: its Y/M/D are the *picked*
 * calendar date) with a local wall-clock time into local unix seconds, in the
 * device zone; a time inside a DST gap resolves as the platform calendar does
 * (forward by the gap). UNCLAMPED: the caller ([ObservedAtRow]) clamps to now
 * and surfaces when it had to, so a future stamp can never be logged silently.
 */
internal expect fun localInstantSeconds(utcDayMillis: Long, hour: Int, minute: Int): Long
