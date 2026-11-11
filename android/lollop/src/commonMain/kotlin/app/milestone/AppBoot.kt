package app.milestone

import kotlinx.coroutines.withContext

// The app's boot sequence, the same on both platforms: probe the core, restore the
// event log (replay + compaction), date the core with the shell's clock, reconcile
// the plan-removed marker and the auto plan. [rememberCoachSession] runs it as one
// exclusive turn of the event queue, then makes the once-per-process decisions
// below (guided setup, the import confirmation, the interrupted-run offer).

/** The once-per-process boot facts ([AppRuntime] holds the app's; a test passes
 *  its own). */
internal interface BootLedger {
    /** The first boot turn of this process has run (a rotation's re-run skips the
     *  probe and the one-time decisions). */
    var bootHandled: Boolean

    /** The (epoch day, UTC offset) the core was last told by SetToday, or null. */
    var lastToday: Pair<Long, Long>?

    /** The model the queue last published, or null before the first. */
    val published: ViewModel?

    /** Surface a send's notices (an entry not saved, a log that couldn't be read). */
    fun report(r: SendResult)
}

/** What a boot turn hydrated: the [view], and whether the restore found no log
 *  ([fresh], a first run). */
internal data class BootTurn(val view: ViewModel, val fresh: Boolean)

/**
 * One boot turn on [engine]; the caller runs it inside [EventDispatcher.exclusive] so
 * no queued event interleaves with restore / SetToday / GeneratePlan. Returns null
 * when the core is unusable (it can't answer the probe, or it reads a view but
 * refuses the clock): the app shows its error state instead of an empty model over
 * a user who has data.
 *
 * The first turn of a process probes the core and [restore]s the log; a re-run
 * (rotation) skips the probe and resends SetToday only when the day or offset
 * ([today]) changed since the last send. [planDay] is the epoch day an automatic
 * GeneratePlan starts from. [store] is the setup store ([setupStore]): the
 * plan-removed marker ([KEY_PLAN_CLEARED]) is dropped when a program survived and
 * re-armed when the restored log ends with the plan removed.
 */
internal suspend fun bootTurn(
    engine: DurableCore,
    store: KeyValueStore,
    ledger: BootLedger,
    restore: () -> Boolean,
    today: Pair<Long, Long> = todayEpochDay() to utcOffsetSec(),
    planDay: () -> Long = ::todayEpochDay,
): BootTurn? {
    if (!ledger.bootHandled && !withContext(CoreDispatcher) { engine.probe() }) return null
    val fresh = withContext(CoreDispatcher) { restore() }
    // The shell's clock enters the core as event data (no clock in-core).
    val published = ledger.published
    var v = if (ledger.bootHandled && published != null && ledger.lastToday == today) {
        published
    } else {
        val sent = withContext(CoreDispatcher) { engine.send(Event.SetToday(today.first, today.second)) }
        // A core that reads a view but refuses the clock can't apply anything either.
        if (!sent.accepted) {
            engine.markUnavailable()
            return null
        }
        // An unreadable log or a replay that had to skip lines surfaces here.
        ledger.report(sent)
        ledger.lastToday = today
        sent.view
    }
    // A surviving program means a ClearPlan never landed: drop the stale marker.
    if (v.program != null && store.getBoolean(KEY_PLAN_CLEARED, false)) {
        store.edit { putBoolean(KEY_PLAN_CLEARED, false) }
    }
    // A log that ends with the plan removed re-arms a lost marker (first turn only).
    if (!ledger.bootHandled && v.program == null && engine.restoredPlanRemoved &&
        !store.getBoolean(KEY_PLAN_CLEARED, false)
    ) {
        store.edit { putBoolean(KEY_PLAN_CLEARED, true) }
    }
    // A set-up user without a plan gets one unless they removed it; a re-run then
    // sees the program and cannot fire twice.
    if (v.profile != null && v.program == null && !store.getBoolean(KEY_PLAN_CLEARED, false)) {
        val plan = withContext(CoreDispatcher) { engine.send(Event.GeneratePlan(planDay())) }
        ledger.report(plan)
        v = plan.view
    }
    return BootTurn(v, fresh)
}

/** The setup store key of the onboarding flag: the guided setup was completed or
 *  skipped, so it never prompts again. */
internal const val KEY_ONBOARDING_DONE = "onboarding_done"

/** The guided setup shows on a first run ([fresh] log, no [view] profile) not yet
 *  completed or skipped. */
internal fun showSetupOnBoot(fresh: Boolean, view: ViewModel, store: KeyValueStore): Boolean =
    fresh && !store.getBoolean(KEY_ONBOARDING_DONE, false) && view.profile == null

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

/** How old a crash-recovery sidecar's newest fix may be and still be offered for
 *  resume. Beyond this the app was gone long enough that the run is almost certainly
 *  dead (a stale resume would let one Stop&save write a garbage multi-hour run). */
internal const val RESUME_STALE_SEC = 3L * 60L * 60L

/** Whether an interrupted run's [recovered] fixes are worth offering at [nowSec]:
 *  at least two fixes, the newest no older than [RESUME_STALE_SEC]. */
internal fun resumable(recovered: RecoveredTrack, nowSec: Long): Boolean {
    if (recovered.size < 2) return false
    val newestSec = recovered.maxOfOrNull { it.observedAt } ?: 0L
    return nowSec - newestSec <= RESUME_STALE_SEC
}
