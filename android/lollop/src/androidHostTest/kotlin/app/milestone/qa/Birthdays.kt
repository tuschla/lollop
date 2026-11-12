package app.milestone.qa

import app.milestone.R
import app.milestone.epochDayLabel
import java.time.LocalDate

/** 35 on the pinned Monday ([QaHarness.PINNED_NOW_MILLIS]). */
internal val adultBirthday: Long = LocalDate.of(1991, 3, 12).toEpochDay()

/** [adultBirthday] typed into the birthday picker's date field (US order MMDDYYYY). */
internal const val ADULT_BIRTHDAY_DIGITS = "03121991"

/** 15 on the pinned Monday. */
internal val youthBirthday: Long = LocalDate.of(2011, 3, 15).toEpochDay()

/** [youthBirthday] typed into the birthday picker's date field. */
internal const val YOUTH_BIRTHDAY_DIGITS = "03152011"

/** The birthday pill's / Age row's text for [day]: "Born Mar 12, 1991 · 35". */
internal fun born(day: Long, age: Int) = str(R.string.setup_birthday_value, epochDayLabel(day), age)
