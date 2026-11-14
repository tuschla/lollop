package app.milestone

import android.content.Context
import android.content.SharedPreferences

/** The [setupStore] as SharedPreferences, for the tests to seed and inspect. */
internal fun Context.setupPrefs(): SharedPreferences = getSharedPreferences(AppFileNames.SETUP_STORE, Context.MODE_PRIVATE)
