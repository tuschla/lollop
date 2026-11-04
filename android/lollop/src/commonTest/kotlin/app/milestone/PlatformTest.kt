package app.milestone

/** Base class for a common test that reads the platform's locale and date data:
 *  Robolectric on the Android host (android.icu, DateFormat), a plain test on iOS. */
expect abstract class PlatformTest()

/** [languageTag] ("en-GB") as the app formats with it: Latin digits, like [appLocale]. */
expect fun testLocale(languageTag: String): PlatformLocale
