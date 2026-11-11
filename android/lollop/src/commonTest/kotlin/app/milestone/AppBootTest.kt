package app.milestone

import app.milestone.core.CoreBridge
import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlinx.coroutines.test.runTest

/**
 * A stand-in core for the boot sequence: every known event is accepted; the view
 * carries a profile when [profile] is set and a program once a GeneratePlan landed
 * (or from the start, [programAtStart]) until a ClearPlan, which sets
 * `plan_removed` until the next GeneratePlan; [refuseToday] drops SetToday and
 * [viewFails] answers every view with an error envelope.
 */
private class BootCore(
    var profile: Boolean = false,
    programAtStart: Boolean = false,
    val refuseToday: Boolean = false,
    val viewFails: Boolean = false,
) : CoreBridge {
    val applied = mutableListOf<String>()
    private var program = programAtStart
    private var planRemoved = false

    fun variants() = applied.map { EventLog.variantOf(it) }

    override fun update(event: ByteArray): ByteArray {
        val line = event.decodeToString()
        val variant = EventLog.variantOf(line) ?: return ByteArray(0)
        if (refuseToday && variant == "SetToday") return ByteArray(0)
        applied += line
        if (variant == "GeneratePlan") {
            program = true
            planRemoved = false
        }
        if (variant == "ClearPlan") {
            program = false
            planRemoved = true
        }
        return "[]".encodeToByteArray()
    }

    override fun view(): ByteArray = when {
        viewFails -> """{"error":{"kind":"panic","context":"view"}}"""
        else -> buildString {
            append("{\"input_count\":${applied.size}")
            if (profile) append(",\"profile\":{}")
            if (program) append(",\"program\":{\"name\":\"plan\"}")
            if (planRemoved) append(",\"plan_removed\":true")
            append('}')
        }
    }.encodeToByteArray()

    override fun parseRunFile(file: ByteArray): ByteArray? = null

    override fun exportGpx(run: ByteArray): ByteArray = ByteArray(0)

    override fun runTrack(run: ByteArray): ByteArray? = null

    override fun trackFigures(event: ByteArray): ByteArray? = null

    override fun exerciseKeys(names: Array<String>): ByteArray? = null

    override fun warmUpRamp(query: ByteArray): ByteArray? = null

    override fun draftFacts(draft: ByteArray): ByteArray? = null

    override fun thinTrack(track: ByteArray): ByteArray? = null

    override fun importedHrPct(hr: ByteArray): ByteArray? = null

    override fun compactLog(lines: Array<String>): ByteArray =
        """{"dropped":[],"torn":[],"max_entry_id":0}""".encodeToByteArray()

    override fun reset(): ByteArray {
        applied.clear()
        return ByteArray(0)
    }
}

/** [KeyValueStore] in memory. */
private class MemoryStore : KeyValueStore {
    val values = mutableMapOf<String, Any?>()

    override fun contains(key: String) = key in values

    override fun getBoolean(key: String, default: Boolean) = values[key] as? Boolean ?: default

    override fun getInt(key: String, default: Int) = values[key] as? Int ?: default

    override fun getLong(key: String, default: Long) = values[key] as? Long ?: default

    override fun getString(key: String, default: String?) = values[key] as? String ?: default

    override fun edit(commit: Boolean, block: KeyValueStore.Editor.() -> Unit) {
        object : KeyValueStore.Editor {
            override fun putBoolean(key: String, value: Boolean) { values[key] = value }

            override fun putInt(key: String, value: Int) { values[key] = value }

            override fun putLong(key: String, value: Long) { values[key] = value }

            override fun putString(key: String, value: String?) { values[key] = value }

            override fun remove(key: String) { values.remove(key) }
        }.block()
    }
}

/** The event queue's boot facts, as [EventDispatcher] keeps them. */
private class Ledger : BootLedger {
    override var bootHandled = false
    override var lastToday: Pair<Long, Long>? = null
    override var published: ViewModel? = null
    val reports = mutableListOf<SendResult>()

    override fun report(r: SendResult) {
        reports += r
    }
}

/**
 * The boot sequence both platforms run ([bootTurn] and the once-per-process
 * decisions) on a stand-in core and a real event log: a fresh install, a returning
 * user, the auto plan, a removed plan and the legacy plan-removed pref, the
 * rotation re-run, an unusable core, the import confirmation and the
 * interrupted-run offer.
 */
class AppBootTest {
    private val dir = newTempDir()
    private val log = AppFiles.sibling(dir, "/${AppFileNames.EVENT_LOG}")
    private val store = MemoryStore()
    private val settings = MemoryStore()
    private val ledger = Ledger()
    private val day = 20_000L to 3_600L

    @AfterTest
    fun removeLog() {
        for (suffix in listOf("") + AppFileNames.LOG_SUFFIXES) AppFiles.delete(AppFiles.sibling(log, suffix))
        AppFiles.delete(dir)
    }

    private suspend fun boot(core: BootCore, engine: DurableCore = DurableCore(core), today: Pair<Long, Long> = day): BootTurn? =
        bootTurn(engine, store, settings, ledger, restore = { engine.restoreFrom(log) }, today = today, planDay = { 19_999L })

    private fun logVariants(): List<String?> {
        val lines = mutableListOf<String?>()
        AppFiles.forEachLine(log) { if (it.isNotBlank()) lines += EventLog.variantOf(it) }
        return lines
    }

    @Test
    fun aFreshInstallIsDatedAndOffersTheSetupOnce() = runTest {
        val core = BootCore()
        val turn = assertNotNull(boot(core))
        assertTrue(turn.fresh, "no log: a first run")
        assertEquals(listOf("SetToday"), core.variants(), "dated, no plan without a profile")
        assertEquals(day, ledger.lastToday)
        assertEquals(1, ledger.reports.size, "the SetToday result is reported")
        assertTrue(showSetupOnBoot(turn.fresh, turn.view, store))
        store.edit { putBoolean(KEY_ONBOARDING_DONE, true) }
        assertFalse(showSetupOnBoot(turn.fresh, turn.view, store), "completed or skipped: never again")
    }

    @Test
    fun aReturningUserReplaysTheLogAndSkipsTheSetup() = runTest {
        AppFiles.writeLinesSynced(log, listOf(Event.SetToday(19_000).toJson().toString()))
        val core = BootCore()
        val turn = assertNotNull(boot(core))
        assertFalse(turn.fresh)
        assertEquals(listOf("SetToday", "SetToday"), core.variants(), "the log replayed, then today's clock")
        assertFalse(showSetupOnBoot(turn.fresh, turn.view, store))
    }

    @Test
    fun aProfileWithoutAPlanGetsOneFromToday() = runTest {
        val core = BootCore(profile = true)
        val turn = assertNotNull(boot(core))
        assertEquals(listOf("SetToday", "GeneratePlan"), core.variants())
        assertTrue(core.applied.last().contains("19999"), "the plan starts on the plan day")
        assertNotNull(turn.view.program)
        assertEquals(2, ledger.reports.size)
    }

    @Test
    fun anOlderBuildsDistanceUnitMovesIntoTheProfileOnce() = runTest {
        settings.edit { putString(LEGACY_DISTANCE_UNIT_KEY, "Mi") }
        val core = BootCore(profile = true)
        assertNotNull(boot(core))
        assertEquals(listOf("SetToday", "SetProfile", "GeneratePlan"), core.variants())
        val sent = core.applied.single { EventLog.variantOf(it) == "SetProfile" }
        assertTrue(sent.contains("\"distance_unit\":\"Mi\""), sent)
        assertFalse(settings.contains(LEGACY_DISTANCE_UNIT_KEY), "the setting is spent")
    }

    @Test
    fun withoutAnOlderBuildsDistanceUnitNothingIsSent() = runTest {
        val core = BootCore(profile = true)
        assertNotNull(boot(core))
        assertEquals(listOf("SetToday", "GeneratePlan"), core.variants())
    }

    @Test
    fun anOlderBuildsDistanceUnitWaitsForAProfile() = runTest {
        settings.edit { putString(LEGACY_DISTANCE_UNIT_KEY, "Mi") }
        val core = BootCore()
        assertNotNull(boot(core))
        assertEquals(listOf("SetToday"), core.variants())
        assertEquals("Mi", settings.getString(LEGACY_DISTANCE_UNIT_KEY, null), "kept for the guided setup")
    }

    @Test
    fun aRemovedPlanStaysRemovedAcrossRelaunches() = runTest {
        AppFiles.writeLinesSynced(log, listOf(Event.ClearPlan.toJson().toString()))
        repeat(2) { launch ->
            val core = BootCore(profile = true)
            val view = assertNotNull(boot(core)).view
            assertNull(view.program, "launch $launch: no plan generated over the removal")
            assertTrue(view.plan_removed)
            assertEquals(listOf("ClearPlan", "SetToday"), core.variants(), "launch $launch: the log replayed, then the clock")
        }
    }

    @Test
    fun aLegacyMarkerWhoseLogLostTheClearPlanLogsTheRemoval() = runTest {
        AppFiles.writeLinesSynced(log, listOf(Event.SetToday(19_000).toJson().toString()))
        store.edit { putBoolean(KEY_PLAN_CLEARED, true) }
        val core = BootCore(profile = true)
        val view = assertNotNull(boot(core)).view
        assertNull(view.program, "no plan generated over the removal")
        assertTrue(view.plan_removed)
        assertEquals(listOf("SetToday", "SetToday", "ClearPlan"), core.variants())
        assertEquals("ClearPlan", logVariants().last(), "the log carries the removal")
        assertFalse(KEY_PLAN_CLEARED in store, "the pref is read once")

        // The next launch has only the log.
        val relaunch = BootCore(profile = true)
        assertNull(assertNotNull(boot(relaunch)).view.program)
        assertEquals(listOf("SetToday", "ClearPlan", "SetToday"), relaunch.variants())
    }

    @Test
    fun aLegacyMarkerOverALoggedRemovalIsDropped() = runTest {
        AppFiles.writeLinesSynced(log, listOf(Event.ClearPlan.toJson().toString()))
        store.edit { putBoolean(KEY_PLAN_CLEARED, true) }
        val core = BootCore(profile = true)
        assertNull(assertNotNull(boot(core)).view.program)
        assertEquals(listOf("ClearPlan", "SetToday"), core.variants(), "no second ClearPlan")
        assertFalse(KEY_PLAN_CLEARED in store)
    }

    @Test
    fun aStaleLegacyMarkerIsDropped() = runTest {
        store.edit { putBoolean(KEY_PLAN_CLEARED, true) }
        val core = BootCore(profile = true, programAtStart = true)
        assertNotNull(assertNotNull(boot(core)).view.program, "the surviving program stays")
        assertEquals(listOf("SetToday"), core.variants(), "a ClearPlan never landed: none is sent now")
        assertFalse(KEY_PLAN_CLEARED in store)

        store.edit { putBoolean(KEY_PLAN_CLEARED, false) }
        val unmarked = BootCore(profile = true)
        assertNotNull(assertNotNull(boot(unmarked)).view.program, "a false marker removes nothing")
        assertEquals(listOf("SetToday", "GeneratePlan"), unmarked.variants())
        assertFalse(KEY_PLAN_CLEARED in store)
    }

    @Test
    fun aReRunOnTheSameDaySendsNothingAndANewDayResendsTheClock() = runTest {
        val core = BootCore()
        val engine = DurableCore(core)
        val first = assertNotNull(boot(core, engine))
        ledger.bootHandled = true
        ledger.published = first.view
        assertEquals(first.view, assertNotNull(boot(core, engine)).view, "the published model as it is")
        assertEquals(1, core.variants().count { it == "SetToday" }, "no second SetToday for the same day")

        val tomorrow = day.first + 1 to day.second
        assertNotNull(boot(core, engine, today = tomorrow))
        assertEquals(2, core.variants().count { it == "SetToday" })
        assertEquals(tomorrow, ledger.lastToday)
    }

    @Test
    fun aCoreThatCannotAnswerBootsNothing() = runTest {
        val core = BootCore(viewFails = true)
        assertNull(boot(core))
        assertTrue(core.applied.isEmpty(), "nothing restored into it")
        assertNull(ledger.lastToday)
    }

    @Test
    fun aCoreThatRefusesTheClockIsMarkedUnavailable() = runTest {
        val core = BootCore(refuseToday = true)
        val engine = DurableCore(core)
        assertNull(boot(core, engine))
        assertTrue(engine.unavailable)
        assertNull(ledger.lastToday)
    }

    @Test
    fun theImportConfirmationIsReadOnce() {
        assertNull(takeRestoredEntries(store))
        assertFalse(restoringImport(store))
        store.edit { putInt(KEY_RESTORED_ENTRIES, 12) }
        assertTrue(restoringImport(store))
        assertEquals(12, takeRestoredEntries(store))
        assertNull(takeRestoredEntries(store), "shown on the first boot after the import only")
        assertFalse(restoringImport(store))
    }

    @Test
    fun onlyARecentTrackOfTwoFixesIsOfferedForResume() {
        fun track(vararg at: Long) = RecoveredTrack(at.map { GpsPoint(52.5, 13.4, it, 5.0) }, emptyList())
        val now = 1_750_000_000L
        assertFalse(resumable(track(now), now), "one fix: nothing to resume")
        assertTrue(resumable(track(now - 60, now - 30), now))
        assertTrue(resumable(track(now - RESUME_STALE_SEC - 60, now - RESUME_STALE_SEC), now))
        assertFalse(resumable(track(now - RESUME_STALE_SEC - 2, now - RESUME_STALE_SEC - 1), now), "a dead run")
    }
}
