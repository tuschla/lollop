package app.milestone

import android.content.Context
import android.content.res.Configuration
import android.os.LocaleList
import androidx.test.core.app.ApplicationProvider
import java.time.LocalDate
import java.time.ZoneOffset
import java.util.Locale
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertSame
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/** The app's locale for text formatted outside the resources, and the per-use
 *  Latin-digit resources (AppLocale.kt). */
@RunWith(RobolectricTestRunner::class)
class AppLocaleTest {

    private val savedLocale: Locale = Locale.getDefault()

    @After
    fun restoreLocale() = Locale.setDefault(savedLocale)

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()
    private val zone = ZoneOffset.UTC
    private val arabic = Locale.forLanguageTag("ar-EG")
    private val now = LocalDate.of(2026, 10, 30).atTime(12, 0).toInstant(zone).toEpochMilli()

    private fun daysAgo(n: Long) = now / 1000L - n * 86_400L

    @Test
    fun relativeDatesReadInEnglishWithLatinDigitsOnAnArabicPhone() {
        val locale = appLocale(LocaleList(arabic))
        assertEquals("en", locale.language)
        assertEquals("latn", locale.getUnicodeLocaleType("nu"))
        assertEquals("Today", formatLogDate(daysAgo(0), now, locale, zone))
        assertEquals("Yesterday", formatLogDate(daysAgo(1), now, locale, zone))
        assertEquals("3 days ago", formatLogDate(daysAgo(3), now, locale, zone))
        assertEquals("Oct 16", formatLogDate(daysAgo(14), now, locale, zone))
        assertEquals("Oct 30, 2025", formatLogDate(daysAgo(365), now, locale, zone))
        assertEquals("an undated entry has no date", "", formatLogDate(0L, now, locale, zone))
    }

    @Test
    fun anEnglishLocaleInTheListKeepsItsDateOrder() {
        val locale = appLocale(LocaleList(arabic, Locale.UK))
        assertEquals(Locale.UK.withLatinDigits(), locale)
        assertEquals("16 Oct", formatLogDate(daysAgo(14), now, locale, zone))
    }

    @Test
    fun latinDigitResourcesLeaveTheJvmDefaultAlone() {
        Locale.setDefault(arabic)
        val defaults = LocaleList.getDefault()
        val cfg = Configuration(ctx.resources.configuration).apply { setLocale(arabic) }
        val res = ctx.createConfigurationContext(cfg).latinDigitResources()
        assertEquals("latn", res.configuration.locales[0].getUnicodeLocaleType("nu"))
        assertEquals(arabic, Locale.getDefault())
        assertEquals(defaults, LocaleList.getDefault())
        assertSame("one copy per configuration", res, ctx.createConfigurationContext(cfg).latinDigitResources())
    }
}
