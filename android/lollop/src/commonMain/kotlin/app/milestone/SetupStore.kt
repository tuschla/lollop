package app.milestone

/** The shell's setup store ([AppFileNames.SETUP_STORE]). Excluded from Auto Backup
 *  (it describes the event log, which is excluded too). */
internal fun PlatformContext.setupStore(): KeyValueStore = keyValueStore(AppFileNames.SETUP_STORE)

/** [setupStore] key: how many entries a data import restored, written just
 *  before the reboot into the imported log ([AppRuntime.requestReboot]) so that boot can
 *  confirm it, then removed. */
internal const val KEY_RESTORED_ENTRIES = "restored_entries"
