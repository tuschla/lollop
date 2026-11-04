package app.milestone

import android.icu.text.DisplayContext
import android.icu.text.RelativeDateTimeFormatter
import android.icu.text.RelativeDateTimeFormatter.AbsoluteUnit
import android.icu.text.RelativeDateTimeFormatter.Direction
import android.icu.text.RelativeDateTimeFormatter.RelativeUnit
import android.icu.util.ULocale
import androidx.compose.runtime.Composable
import androidx.compose.ui.platform.LocalContext
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.time.format.TextStyle
import java.util.Locale
import kotlin.math.abs
import kotlinx.datetime.DayOfWeek
import kotlinx.datetime.LocalDate
import kotlinx.datetime.LocalDateTime
import kotlinx.datetime.toJavaDayOfWeek
import kotlinx.datetime.toJavaLocalDate
import kotlinx.datetime.toJavaLocalDateTime

actual typealias PlatformLocale = Locale

internal actual fun deviceLocale(): Locale = Locale.getDefault()

internal actual val Locale.region: String get() = country

/** ICU's best pattern for the skeleton (android.text.format.DateFormat), run by java.time. */
internal actual class DayFormatter(private val formatter: DateTimeFormatter) {
    actual fun format(date: LocalDate): String = date.toJavaLocalDate().format(formatter)

    actual fun format(dateTime: LocalDateTime): String = dateTime.toJavaLocalDateTime().format(formatter)
}

internal actual fun localizedDayFormatter(skeleton: String, locale: Locale): DayFormatter =
    DayFormatter(DateTimeFormatter.ofPattern(android.text.format.DateFormat.getBestDateTimePattern(locale, skeleton), locale))

internal actual fun weekdayName(day: DayOfWeek, width: WeekdayWidth, locale: Locale): String =
    day.toJavaDayOfWeek().getDisplayName(
        when (width) {
            WeekdayWidth.Full -> TextStyle.FULL
            WeekdayWidth.Short -> TextStyle.SHORT
            WeekdayWidth.ShortStandalone -> TextStyle.SHORT_STANDALONE
            WeekdayWidth.Narrow -> TextStyle.NARROW
        },
        locale,
    )

internal actual fun formatMediumDate(date: LocalDate, locale: Locale): String =
    date.toJavaLocalDate().format(DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM).withLocale(locale))

// ICU's relative-date wording, the wording DateUtils uses: "Today", "Yesterday", "3 days ago".
internal actual fun relativeDayLabel(dayDelta: Long, locale: Locale): String {
    val fmt = RelativeDateTimeFormatter.getInstance(
        ULocale.forLocale(locale),
        null,
        RelativeDateTimeFormatter.Style.SHORT,
        DisplayContext.CAPITALIZATION_FOR_BEGINNING_OF_SENTENCE,
    )
    val days = abs(dayDelta)
    val direction = if (dayDelta <= 0) Direction.LAST else Direction.NEXT
    return when (days) {
        0L -> fmt.format(Direction.THIS, AbsoluteUnit.DAY)
        1L -> fmt.format(direction, AbsoluteUnit.DAY)
        else -> fmt.format(days.toDouble(), direction, RelativeUnit.DAYS)
    }
}

@Composable
internal actual fun is24HourClock(): Boolean = android.text.format.DateFormat.is24HourFormat(LocalContext.current)
