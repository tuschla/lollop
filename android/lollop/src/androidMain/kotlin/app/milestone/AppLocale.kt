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

@Volatile private var latinCache: Pair<Configuration, Resources>? = null

/**
 * [this] context's resources with Latin digits. Called where a text is built
 * outside composition (a notification, the widget, the tile, the run import
 * result, the coach export), so a long-running service follows a language
 * change. One copy is kept per configuration, so a notification refreshed every
 * second builds no new context.
 */
internal actual fun Context.appResources(): Resources {
    val config = resources.configuration
    val latin = config.locales.withLatinDigits()
    if (latin == config.locales) return resources
    latinCache?.let { (key, res) -> if (key == config) return res }
    return createConfigurationContext(localesOnly(latin)).resources.also { latinCache = Configuration(config) to it }
}

/** A configuration delta that only sets [locales]. Its fontScale is 0 (leave the
 *  user's font scale): before API 26 `Configuration()` starts from the defaults (1.0). */
private fun localesOnly(locales: LocaleList): Configuration =
    Configuration().apply { fontScale = 0f; setLocales(locales) }

/** [this] context with [appResources], for MainActivity's base context: a
 *  language change recreates the Activity, so its base follows. */
internal fun Context.withLatinDigits(): Context {
    val latin = appResources()
    return if (latin === resources) this else LatinDigitsContext(this, latin)
}

/** [base] with [latin] resources. Everything else (system services, files, the
 *  component's own context) stays the base's, as a ContextThemeWrapper's
 *  override configuration does. */
private class LatinDigitsContext(base: Context, private val latin: Resources) : ContextWrapper(base) {
    override fun getResources(): Resources = latin

    override fun getAssets(): AssetManager = latin.assets
}
