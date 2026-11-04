package app.milestone

/** Name of the shell's setup store (SharedPreferences file / NSUserDefaults suite):
 *  the onboarding flag, [KEY_PLAN_CLEARED], [KEY_RESTORED_ENTRIES] and the tracker's
 *  start gates. Excluded from Auto Backup (it describes the event log, which is
 *  excluded too). */
internal const val SETUP_PREFS = "milestone_setup"

/** The [SETUP_PREFS] store. */
internal fun PlatformContext.setupStore(): KeyValueStore = keyValueStore(SETUP_PREFS)

// Persisted flag (in the [SETUP_PREFS] store) recording that the user removed
// their plan. It gates the boot auto-GeneratePlan so a "Remove plan" survives both
// rotation and relaunch; generating any plan clears it. See the boot sequence.
// A data backup carries it as a trailing ClearPlan line ([readDataExport]) and a
// restore sets it from the imported log ([replaceEventLog]).
internal const val KEY_PLAN_CLEARED = "plan_cleared"

/** [SETUP_PREFS] key: how many entries a data import restored, written just
 *  before the reboot into the imported log ([restartForImport]) so that boot can
 *  confirm it, then removed. */
internal const val KEY_RESTORED_ENTRIES = "restored_entries"
