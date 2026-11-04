package app.milestone

import java.util.Locale
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

@RunWith(RobolectricTestRunner::class)
actual abstract class PlatformTest actual constructor()

actual fun testLocale(languageTag: String): PlatformLocale = Locale.forLanguageTag(languageTag).withLatinDigits()
