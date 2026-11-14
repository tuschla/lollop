package app.milestone

import platform.Foundation.NSLocale

actual abstract class PlatformTest actual constructor()

actual fun testLocale(languageTag: String): PlatformLocale =
    NSLocale(localeIdentifier = languageTag.replace('-', '_') + "@numbers=latn")
