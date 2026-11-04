package app.milestone

import android.content.Context
import android.content.SharedPreferences
import androidx.core.content.edit

actual typealias PlatformContext = Context

internal actual fun Context.keyValueStore(name: String): KeyValueStore =
    SharedPreferencesStore(getSharedPreferences(name, Context.MODE_PRIVATE))

internal actual fun Context.hasEventLog(): Boolean = eventLogFile(filesDir).exists()

/** [KeyValueStore] over [prefs]; [edit] is SharedPreferences' apply() (commit() when asked). */
internal class SharedPreferencesStore(private val prefs: SharedPreferences) : KeyValueStore {
    override fun contains(key: String): Boolean = prefs.contains(key)

    override fun getBoolean(key: String, default: Boolean): Boolean = prefs.getBoolean(key, default)

    override fun getInt(key: String, default: Int): Int = prefs.getInt(key, default)

    override fun getLong(key: String, default: Long): Long = prefs.getLong(key, default)

    override fun getString(key: String, default: String?): String? = prefs.getString(key, default)

    // androidx `edit`: one Editor, then commit() when asked, else apply().
    override fun edit(commit: Boolean, block: KeyValueStore.Editor.() -> Unit) = prefs.edit(commit) {
        val editor = this
        object : KeyValueStore.Editor {
            override fun putBoolean(key: String, value: Boolean) { editor.putBoolean(key, value) }

            override fun putInt(key: String, value: Int) { editor.putInt(key, value) }

            override fun putLong(key: String, value: Long) { editor.putLong(key, value) }

            override fun putString(key: String, value: String?) { editor.putString(key, value) }

            override fun remove(key: String) { editor.remove(key) }
        }.block()
    }
}
