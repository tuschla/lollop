package app.milestone

/** What platform services hang off: `android.content.Context` on Android; on iOS
 *  the one process-wide [PlatformContext.INSTANCE]. */
expect abstract class PlatformContext

/**
 * A small persistent key-value store for shell preferences (not coaching state,
 * which is the event log): SharedPreferences on Android, NSUserDefaults on iOS.
 * Reads return [default] for a key that was never written.
 */
internal interface KeyValueStore {
    operator fun contains(key: String): Boolean

    fun getBoolean(key: String, default: Boolean): Boolean

    fun getInt(key: String, default: Int): Int

    fun getLong(key: String, default: Long): Long

    fun getString(key: String, default: String?): String?

    /** Apply [block]'s writes together; [commit] writes them to disk before returning. */
    fun edit(commit: Boolean = false, block: Editor.() -> Unit)

    interface Editor {
        fun putBoolean(key: String, value: Boolean)

        fun putInt(key: String, value: Int)

        fun putLong(key: String, value: Long)

        fun putString(key: String, value: String?)

        fun remove(key: String)
    }
}

/** The store named [name] (one SharedPreferences file / NSUserDefaults suite). */
internal expect fun PlatformContext.keyValueStore(name: String): KeyValueStore

/** The event log's file name, in the app's private files (Android `filesDir`,
 *  iOS Application Support). */
internal const val EVENT_LOG_NAME = "event-log.ndjson"

/** Whether this install has an event log yet, i.e. it is not a fresh install. */
internal expect fun PlatformContext.hasEventLog(): Boolean
