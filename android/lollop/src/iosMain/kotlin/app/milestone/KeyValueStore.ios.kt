package app.milestone

import kotlinx.cinterop.ExperimentalForeignApi
import platform.Foundation.NSApplicationSupportDirectory
import platform.Foundation.NSFileManager
import platform.Foundation.NSUserDefaults
import platform.Foundation.NSUserDomainMask

actual abstract class PlatformContext private constructor() {
    /** The app process: iOS has no per-component context. */
    companion object INSTANCE : PlatformContext()
}

internal actual fun PlatformContext.keyValueStore(name: String): KeyValueStore =
    UserDefaultsStore(NSUserDefaults(suiteName = name))

@OptIn(ExperimentalForeignApi::class)
internal actual fun PlatformContext.hasEventLog(): Boolean {
    val dir = NSFileManager.defaultManager.URLForDirectory(
        NSApplicationSupportDirectory,
        NSUserDomainMask,
        appropriateForURL = null,
        create = false,
        error = null,
    ) ?: return false
    val path = dir.URLByAppendingPathComponent(EVENT_LOG_NAME)?.path ?: return false
    return NSFileManager.defaultManager.fileExistsAtPath(path)
}

/** [KeyValueStore] over one NSUserDefaults suite. Writes land in memory at once;
 *  [edit] with commit flushes them to disk before returning. */
internal class UserDefaultsStore(private val defaults: NSUserDefaults) : KeyValueStore {
    override fun contains(key: String): Boolean = defaults.objectForKey(key) != null

    override fun getBoolean(key: String, default: Boolean): Boolean =
        if (key in this) defaults.boolForKey(key) else default

    override fun getInt(key: String, default: Int): Int =
        if (key in this) defaults.integerForKey(key).toInt() else default

    override fun getLong(key: String, default: Long): Long =
        if (key in this) defaults.integerForKey(key) else default

    override fun getString(key: String, default: String?): String? =
        if (key in this) defaults.stringForKey(key) else default

    override fun edit(commit: Boolean, block: KeyValueStore.Editor.() -> Unit) {
        object : KeyValueStore.Editor {
            override fun putBoolean(key: String, value: Boolean) = defaults.setBool(value, key)

            override fun putInt(key: String, value: Int) = defaults.setInteger(value.toLong(), key)

            override fun putLong(key: String, value: Long) = defaults.setInteger(value, key)

            override fun putString(key: String, value: String?) =
                if (value == null) defaults.removeObjectForKey(key) else defaults.setObject(value, key)

            override fun remove(key: String) = defaults.removeObjectForKey(key)
        }.block()
        if (commit) defaults.synchronize()
    }
}
