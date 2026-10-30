package app.milestone

import android.content.Context
import android.content.ContextWrapper
import android.content.res.AssetManager
import android.content.res.Configuration
import android.content.res.Resources
import android.os.LocaleList
import java.util.Locale

// The app writes every number in ASCII digits, so a screen or export never mixes
// digit scripts (RunCards.kt [displayFigure], DEVIATIONS #45). Figures pin
// Locale.US. Counts and times go through string resources, which use the
// locale's digits: on a phone set to Arabic, Persian or Devanagari digits
// "3 × 5 @ 102.5 kg" would read "٣ × ٥ @ 102.5 kg". So the app formats in the
// device's locales with the Latin numbering system (`-u-nu-latn`): the same
// language, plurals and date order, digits 0–9. Formatters that take a locale
// get one passed in ([appLocale]); the JVM default is never changed. The rest
// countdown's chronometer is drawn by System UI in the system's digits.

/** [this] locale with Latin digits (the `nu-latn` Unicode extension). */
internal fun Locale.withLatinDigits(): Locale =
    if (getUnicodeLocaleType("nu") == "latn") this
    else Locale.Builder().setLocale(this).setUnicodeLocaleKeyword("nu", "latn").build()

private fun LocaleList.withLatinDigits(): LocaleList = LocaleList(*Array(size()) { get(it).withLatinDigits() })

/**
 * The app's language with Latin digits, for every date, weekday and time the
 * shell formats outside the string resources ([formatLogDate],
 * [localizedDayFormatter] and the labels built on it). The app ships English only
 * (`res/values`), so this is the first English locale in [locales] ("15 Jul" for
 * en-GB), else English with the primary locale's region (en-DE on a German
 * phone). File names and exported data keep their fixed ISO patterns.
 */
internal fun appLocale(locales: LocaleList = LocaleList.getDefault()): Locale {
    appLocaleCache?.let { (key, locale) -> if (key == locales) return locale }
    val english = (0 until locales.size()).map(locales::get).firstOrNull { it.language == "en" }
        ?: Locale.Builder().setLanguage("en").setRegion(locales[0].country).build()
    return english.withLatinDigits().also { appLocaleCache = locales to it }
}

@Volatile private var appLocaleCache: Pair<LocaleList, Locale>? = null

@Volatile private var latinCache: Pair<Configuration, Resources>? = null

/**
 * [this] context's resources with Latin digits. Called where a text with a count
 * is built (a notification, the widget, the run import result, the coach
 * export), so a long-running service follows a language change. One copy is
 * kept per configuration, so a notification refreshed every second builds no new
 * context.
 */
internal fun Context.latinDigitResources(): Resources {
    val config = resources.configuration
    val latin = config.locales.withLatinDigits()
    if (latin == config.locales) return resources
    latinCache?.let { (key, res) -> if (key == config) return res }
    // A delta that only sets the locales. fontScale 0 leaves the user's font
    // scale: before API 26 `Configuration()` starts from the defaults (1.0).
    val delta = Configuration().apply { fontScale = 0f; setLocales(latin) }
    return createConfigurationContext(delta).resources.also { latinCache = Configuration(config) to it }
}

/** [this] context with [latinDigitResources], for MainActivity's base context: a
 *  language change recreates the Activity, so its base follows. */
internal fun Context.withLatinDigits(): Context {
    val latin = latinDigitResources()
    return if (latin === resources) this else LatinDigitsContext(this, latin)
}

/** [base] with [latin] resources. Everything else (system services, files, the
 *  component's own context) stays the base's, as a ContextThemeWrapper's
 *  override configuration does. */
private class LatinDigitsContext(base: Context, private val latin: Resources) : ContextWrapper(base) {
    override fun getResources(): Resources = latin

    override fun getAssets(): AssetManager = latin.assets
}
