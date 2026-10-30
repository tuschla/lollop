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
// Locale.US. Counts and times go through string resources, java.text and
// DateUtils, which use the locale's digits: on a phone set to Arabic, Persian or
// Devanagari digits "3 × 5 @ 102.5 kg" would read "٣ × ٥ @ 102.5 kg". So the app
// formats in the device's locales with the Latin numbering system (`-u-nu-latn`):
// the same language, plurals and date order, digits 0–9.

/** [this] locale with Latin digits (the `nu-latn` Unicode extension). */
internal fun Locale.withLatinDigits(): Locale =
    if (getUnicodeLocaleType("nu") == "latn") this
    else Locale.Builder().setLocale(this).setUnicodeLocaleKeyword("nu", "latn").build()

private fun LocaleList.withLatinDigits(): LocaleList = LocaleList(*Array(size()) { get(it).withLatinDigits() })

/**
 * [this] context with its resources' locales in Latin digits. Every context that
 * formats user-visible text goes through it: MainActivity's and the services'
 * base contexts, and the application context where the notifications, the widget
 * and the run import result are built. It also sets the JVM default locale list
 * the same way, for formatters that read the default (DateUtils in
 * [formatLogDate], java.text).
 */
internal fun Context.withLatinDigits(): Context {
    val default = LocaleList.getDefault()
    val latinDefault = default.withLatinDigits()
    if (latinDefault != default) LocaleList.setDefault(latinDefault)
    val locales = resources.configuration.locales
    val latin = locales.withLatinDigits()
    if (latin == locales) return this
    // A delta that only sets the locales. fontScale 0 leaves the user's font
    // scale: before API 26 `Configuration()` starts from the defaults (1.0).
    val delta = Configuration().apply { fontScale = 0f; setLocales(latin) }
    return LatinDigitsContext(this, createConfigurationContext(delta).resources)
}

/** [base] with [latin] resources. Everything else (system services, files, the
 *  component's own context) stays the base's, as a ContextThemeWrapper's
 *  override configuration does. */
private class LatinDigitsContext(base: Context, private val latin: Resources) : ContextWrapper(base) {
    override fun getResources(): Resources = latin

    override fun getAssets(): AssetManager = latin.assets
}
