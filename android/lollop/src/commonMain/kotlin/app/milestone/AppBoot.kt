package app.milestone

import kotlinx.coroutines.withContext

// The app's boot sequence, the same on both platforms: probe the core, restore the
// event log (replay + compaction), date the core with the shell's clock, carry over
// a legacy plan-removed pref and a legacy distance unit, then the auto plan.
// [rememberCoachSession] runs it as one
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
 * GeneratePlan starts from; a plan the user removed ([ViewModel.plan_removed])
 * is not generated again. [store] is the setup store ([setupStore]), read for the
 * legacy [KEY_PLAN_CLEARED] upgrade; [settings] the settings store, read for the
 * legacy [LEGACY_DISTANCE_UNIT_KEY] upgrade.
 */
internal suspend fun bootTurn(
    engine: DurableCore,
    store: KeyValueStore,
    settings: KeyValueStore,
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
    // Upgrade from builds that also kept the removal in [KEY_PLAN_CLEARED]: an older
    // compaction could drop the log's final ClearPlan, so the log gets it back once.
    if (KEY_PLAN_CLEARED in store) {
        if (store.getBoolean(KEY_PLAN_CLEARED, false) && v.program == null && !v.plan_removed) {
            val cleared = withContext(CoreDispatcher) { engine.send(Event.ClearPlan) }
            ledger.report(cleared)
            v = cleared.view
        }
        store.edit { remove(KEY_PLAN_CLEARED) }
    }
    // Upgrade from builds that kept the distance unit in the settings store: the
    // profile takes it once, as a dated edit like the Profile screen's. Without a
    // profile it waits for the guided setup, which takes it as its first answer.
    val legacyUnit = settings.legacyDistanceUnit()
    val profile = v.profile
    if (legacyUnit != null && profile != null) {
        val applied = profile.distance_unit == legacyUnit || withContext(CoreDispatcher) {
            engine.send(Event.SetProfile.from(profile).effectiveFrom(v.today()).copy(distanceUnit = legacyUnit))
        }.let { sent ->
            ledger.report(sent)
            v = sent.view
            sent.accepted
        }
        if (applied) settings.dropLegacyDistanceUnit()
    }
    // A set-up user without a plan gets one unless they removed it; a re-run then
    // sees the program and cannot fire twice.
    if (v.profile != null && v.program == null && !v.plan_removed) {
        val plan = withContext(CoreDispatcher) { engine.send(Event.GeneratePlan(planDay())) }
        ledger.report(plan)
        v = plan.view
    }
    return BootTurn(v, fresh)
}

/** Legacy setup store key: older builds recorded "the user removed the plan" here
 *  as well as in the log. Read only by [bootTurn]'s one-time upgrade, which removes it. */
internal const val KEY_PLAN_CLEARED = "plan_cleared"

/** The setup store key of the onboarding flag: the guided setup was completed or
 *  skipped, so it never prompts again. */
internal const val KEY_ONBOARDING_DONE = "onboarding_done"

/** Settings-store key of the distance unit older builds kept there ("System",
 *  "Km" or "Mi"); the unit is the profile's now ([Profile.distance_unit]).
 *  [AppSettings.migrate] pins an in-use install's unset one to its locale's;
 *  [bootTurn] moves it into an existing profile, the guided setup into a new one. */
internal const val LEGACY_DISTANCE_UNIT_KEY = "distance_unit_override"

/** The legacy distance unit ([LEGACY_DISTANCE_UNIT_KEY]): "System" reads as the
 *  locale's; null when none is stored. */
internal fun KeyValueStore.legacyDistanceUnit(): DistanceUnit? = when (val stored = getString(LEGACY_DISTANCE_UNIT_KEY, null)) {
    null -> null
    else -> DistanceUnit.entries.firstOrNull { it.name == stored } ?: localeDistanceUnit()
}

/** Forget the legacy distance unit once a profile carries one. */
internal fun KeyValueStore.dropLegacyDistanceUnit() = edit { remove(LEGACY_DISTANCE_UNIT_KEY) }

/** The guided setup shows on a first run ([fresh] log, no [view] profile) not yet
 *  completed or skipped. */
internal fun showSetupOnBoot(fresh: Boolean, view: ViewModel, store: KeyValueStore): Boolean =
    fresh && !store.getBoolean(KEY_ONBOARDING_DONE, false) && view.profile == null

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
