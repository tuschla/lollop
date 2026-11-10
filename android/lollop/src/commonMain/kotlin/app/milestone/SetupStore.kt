package app.milestone

/** The shell's setup store ([AppFileNames.SETUP_STORE]). Excluded from Auto Backup
 *  (it describes the event log, which is excluded too). */
internal fun PlatformContext.setupStore(): KeyValueStore = keyValueStore(AppFileNames.SETUP_STORE)

// Persisted flag (in the [setupStore]) recording that the user removed
// their plan. It gates the boot auto-GeneratePlan so a "Remove plan" survives both
// rotation and relaunch; generating any plan clears it. See the boot sequence.
// A data backup carries it as a trailing ClearPlan line ([readDataExport]) and a
// restore sets it from the imported log ([replaceEventLog]).
internal const val KEY_PLAN_CLEARED = "plan_cleared"

/** [setupStore] key: how many entries a data import restored, written just
 *  before the reboot into the imported log ([restartForImport]) so that boot can
 *  confirm it, then removed. */
internal const val KEY_RESTORED_ENTRIES = "restored_entries"
