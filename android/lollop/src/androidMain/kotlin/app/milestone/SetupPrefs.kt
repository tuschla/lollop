package app.milestone

import android.content.Context
import android.content.SharedPreferences

/** The [SETUP_PREFS] SharedPreferences (Android code and tests; common code uses
 *  [setupStore], the same file). */
internal fun Context.setupPrefs(): SharedPreferences = getSharedPreferences(SETUP_PREFS, Context.MODE_PRIVATE)
