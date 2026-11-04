package app.milestone

import androidx.compose.runtime.Composable
import kotlin.math.abs
import kotlin.math.floor
import kotlin.time.Instant
import kotlinx.datetime.DayOfWeek
import kotlinx.datetime.LocalDate
import kotlinx.datetime.LocalDateTime
import kotlinx.datetime.LocalTime
import kotlinx.datetime.TimeZone
import kotlinx.datetime.atTime
import kotlinx.datetime.isoDayNumber
import kotlinx.datetime.number
import kotlinx.datetime.toInstant
import kotlinx.datetime.toLocalDateTime
import platform.Foundation.NSCalendar
import platform.Foundation.NSCalendarIdentifierGregorian
import platform.Foundation.NSDate
import platform.Foundation.NSDateComponents
import platform.Foundation.NSDateFormatter
import platform.Foundation.NSDateFormatterMediumStyle
import platform.Foundation.NSDateFormatterNoStyle
import platform.Foundation.NSFormattingContextBeginningOfSentence
import platform.Foundation.NSLocale
import platform.Foundation.NSLocaleCountryCode
import platform.Foundation.NSLocaleLanguageCode
import platform.Foundation.NSRelativeDateTimeFormatter
import platform.Foundation.NSRelativeDateTimeFormatterStyleNamed
import platform.Foundation.NSRelativeDateTimeFormatterStyleNumeric
import platform.Foundation.NSRelativeDateTimeFormatterUnitsStyleShort
import platform.Foundation.NSTimeZone
import platform.Foundation.componentsFromLocaleIdentifier
import platform.Foundation.countryCode
import platform.Foundation.currentLocale
import platform.Foundation.dateWithTimeIntervalSince1970
import platform.Foundation.localTimeZone
import platform.Foundation.localeIdentifierFromComponents
import platform.Foundation.preferredLanguages
import platform.Foundation.timeIntervalSince1970
import platform.Foundation.timeZoneForSecondsFromGMT

actual typealias PlatformLocale = NSLocale

// The app locale (AppLocale.kt's rule): the first English language the user
// prefers, else English with the device's region, always with Latin digits.
// Rebuilt only when the preferred languages or the region change.
private var appLocaleCache: Pair<List<Any?>, NSLocale>? = null

internal actual fun appLocale(): NSLocale {
    val preferred = NSLocale.preferredLanguages
    val region = NSLocale.currentLocale.countryCode
    val key = preferred + region
    appLocaleCache?.let { (k, locale) -> if (k == key) return locale }
    val english = preferred.firstOrNull { id ->
        NSLocale.componentsFromLocaleIdentifier(id as String)[NSLocaleLanguageCode] == "en"
    } as String?
    val components: MutableMap<Any?, Any?> =
        if (english != null) NSLocale.componentsFromLocaleIdentifier(english).toMutableMap()
        else mutableMapOf<Any?, Any?>(NSLocaleLanguageCode to "en").apply {
            if (!region.isNullOrEmpty()) put(NSLocaleCountryCode, region)
        }
    // The ICU `numbers` keyword: java's `-u-nu-latn`.
    components["numbers"] = "latn"
    val locale = NSLocale(NSLocale.localeIdentifierFromComponents(components))
    appLocaleCache = key to locale
    return locale
}

internal actual fun deviceLocale(): NSLocale = NSLocale.currentLocale

internal actual val NSLocale.region: String get() = countryCode ?: ""

/** java.time formats in the ISO calendar, whatever the locale: so does every formatter here. */
private fun gregorianFormatter(locale: NSLocale) = NSDateFormatter().apply {
    this.locale = locale
    calendar = NSCalendar(calendarIdentifier = NSCalendarIdentifierGregorian)
    // The dates are wall-clock values: formatted as UTC instants of the same digits.
    timeZone = NSTimeZone.timeZoneForSecondsFromGMT(0)
}

private fun LocalDateTime.asUtcNSDate(): NSDate =
    NSDate.dateWithTimeIntervalSince1970(toInstant(TimeZone.UTC).epochSeconds.toDouble())

internal actual class DayFormatter(private val formatter: NSDateFormatter) {
    actual fun format(date: LocalDate): String = format(date.atTime(LocalTime(0, 0)))

    actual fun format(dateTime: LocalDateTime): String = formatter.stringFromDate(dateTime.asUtcNSDate())
}

internal actual fun localizedDayFormatter(skeleton: String, locale: NSLocale): DayFormatter {
    val f = gregorianFormatter(locale)
    // The CLDR best pattern for the skeleton, as Android's getBestDateTimePattern.
    f.dateFormat = NSDateFormatter.dateFormatFromTemplate(skeleton, 0u, locale) ?: skeleton
    return DayFormatter(f)
}

internal actual fun weekdayName(day: DayOfWeek, width: WeekdayWidth, locale: NSLocale): String {
    val f = gregorianFormatter(locale)
    val symbols = when (width) {
        WeekdayWidth.Full -> f.weekdaySymbols
        WeekdayWidth.Short -> f.shortWeekdaySymbols
        WeekdayWidth.ShortStandalone -> f.shortStandaloneWeekdaySymbols
        WeekdayWidth.Narrow -> f.veryShortWeekdaySymbols
    }
    // Foundation's weekday arrays start on Sunday; ISO numbers Monday 1 … Sunday 7.
    return symbols[day.isoDayNumber % 7] as String
}

internal actual fun formatMediumDate(date: LocalDate, locale: NSLocale): String {
    val f = gregorianFormatter(locale)
    f.dateStyle = NSDateFormatterMediumStyle
    f.timeStyle = NSDateFormatterNoStyle
    return f.stringFromDate(date.atTime(LocalTime(0, 0)).asUtcNSDate())
}

internal actual fun relativeDayLabel(dayDelta: Long, locale: NSLocale): String {
    val f = NSRelativeDateTimeFormatter()
    f.locale = locale
    f.calendar = NSCalendar(calendarIdentifier = NSCalendarIdentifierGregorian)
    f.unitsStyle = NSRelativeDateTimeFormatterUnitsStyleShort
    f.formattingContext = NSFormattingContextBeginningOfSentence
    // Named ("Today", "Yesterday") for 0 and ±1 only; ICU counts every other
    // distance ("2 days ago"), never "the day before yesterday".
    f.dateTimeStyle = if (abs(dayDelta) <= 1L) NSRelativeDateTimeFormatterStyleNamed else NSRelativeDateTimeFormatterStyleNumeric
    val components = NSDateComponents().apply { day = dayDelta }
    return f.localizedStringFromDateComponents(components)
}

// The 24-hour setting shows in the device locale's hour pattern ("j"): a
// 12-hour clock carries the am/pm marker ("a").
@Composable
internal actual fun is24HourClock(): Boolean =
    NSDateFormatter.dateFormatFromTemplate("j", 0u, NSLocale.currentLocale)?.contains('a') != true

/** NSCalendar resolves a time inside a DST gap forward by the gap, as
 *  java.util.Calendar does on Android. */
internal actual fun localInstantSeconds(utcDayMillis: Long, hour: Int, minute: Int): Long {
    val day = Instant.fromEpochMilliseconds(utcDayMillis).toLocalDateTime(TimeZone.UTC).date
    val components = NSDateComponents().apply {
        year = day.year.toLong()
        month = day.month.number.toLong()
        this.day = day.day.toLong()
        this.hour = hour.toLong()
        this.minute = minute.toLong()
        second = 0
    }
    val calendar = NSCalendar(calendarIdentifier = NSCalendarIdentifierGregorian)
    calendar.timeZone = NSTimeZone.localTimeZone
    val date = checkNotNull(calendar.dateFromComponents(components)) { "no date for $day $hour:$minute" }
    return floor(date.timeIntervalSince1970).toLong()
}

