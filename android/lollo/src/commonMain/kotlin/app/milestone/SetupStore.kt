package app.milestone

/** The shell's setup store ([AppFileNames.SETUP_STORE]). Excluded from Auto Backup
 *  (it describes the event log, which is excluded too). */
internal fun PlatformContext.setupStore(): KeyValueStore = keyValueStore(AppFileNames.SETUP_STORE)

/** [setupStore] key: how many entries a data import restored, written just
 *  before the reboot into the imported log ([AppRuntime.requestReboot]) so that boot can
 *  confirm it, then removed. */
internal const val KEY_RESTORED_ENTRIES = "restored_entries"

/** Record that a data import restored [entries] entries, before its reboot. Committed
 *  at once: the boot that confirms it may be a fresh process. */
internal fun markRestoredEntries(store: KeyValueStore, entries: Int) =
    store.edit(commit = true) { putInt(KEY_RESTORED_ENTRIES, entries) }

/** How many entries a data import just restored, read once: the key is removed, so
 *  the confirmation shows on the first boot after the import only. Null when none. */
internal fun takeRestoredEntries(store: KeyValueStore): Int? {
    val n = store.getInt(KEY_RESTORED_ENTRIES, -1)
    if (n < 0) return null
    store.edit { remove(KEY_RESTORED_ENTRIES) }
    return n
}

/** An import restart is still replaying the restored log (the key goes once boot ran). */
internal fun restoringImport(store: KeyValueStore): Boolean = store.getInt(KEY_RESTORED_ENTRIES, -1) >= 0
