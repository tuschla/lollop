package app.milestone

/**
 * The name of every file and key-value store the app keeps whose name another
 * place repeats. Installed data is found by these names: a value never changes.
 * Files live in the app's private files ([appFile]: Android `filesDir`, iOS
 * Application Support); a store is one SharedPreferences file / NSUserDefaults
 * suite ([keyValueStore]). Android's Auto Backup rules (res/xml/backup_rules.xml,
 * data_extraction_rules.xml) can't reference Kotlin; AppIdentifiersTest checks them
 * against these names.
 */
internal object AppFileNames {
    /** The event log, the source of truth ([eventLogPath]). */
    const val EVENT_LOG = "event-log.ndjson"

    /** A write-then-rename temporary beside its file (the event log's rewrite, the widget snapshot). */
    const val TMP_SUFFIX = ".tmp"

    /** An imported log written beside the event log before it is swapped in ([swapLogFile]). */
    const val IMPORT_SUFFIX = ".import"

    /** The event log set aside while an import is swapped in. */
    const val BAK_SUFFIX = ".bak"

    /** Every temporary the event log can leave beside it ([EVENT_LOG] + suffix). */
    val LOG_SUFFIXES = listOf(TMP_SUFFIX, IMPORT_SUFFIX, BAK_SUFFIX)

    /** The crash-safe file of the run being recorded ([ActiveRunStore]). */
    const val ACTIVE_RUN = "active-run.ndjson"

    /** Android's widget snapshot (WidgetSnapshotStore). */
    const val WIDGET_SNAPSHOT = "widget-today.json"

    /** The setup store ([setupStore]): the onboarding flag, [KEY_PLAN_CLEARED],
     *  [KEY_RESTORED_ENTRIES] and the tracker's start gates. */
    const val SETUP_STORE = "milestone_setup"

    /** The settings store ([AppSettings]), named for its first setting. */
    const val SETTINGS_STORE = "theme"

    /** The session reminder's setting and the days it reminded on (SessionReminder, both platforms). */
    const val REMINDER_STORE = "session_reminder"
}
