package app.milestone

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
 * (or from the start, [programAtStart]) until a ClearPlan; [refuseToday] drops
 * SetToday, [viewFails] answers every view with an error envelope, and the
 * compaction reports [planRemoved].
 */
private class BootCore(
    var profile: Boolean = false,
    programAtStart: Boolean = false,
    val refuseToday: Boolean = false,
    val viewFails: Boolean = false,
    val planRemoved: Boolean = false,
) : CoreBridge {
    val applied = mutableListOf<String>()
    private var program = programAtStart

    fun variants() = applied.map { EventLog.variantOf(it) }

    override fun update(event: ByteArray): ByteArray {
        val line = event.decodeToString()
        val variant = EventLog.variantOf(line) ?: return ByteArray(0)
        if (refuseToday && variant == "SetToday") return ByteArray(0)
        applied += line
        if (variant == "GeneratePlan") program = true
        if (variant == "ClearPlan") program = false
        return "[]".encodeToByteArray()
    }

    override fun view(): ByteArray = when {
        viewFails -> """{"error":{"kind":"panic","context":"view"}}"""
        else -> buildString {
            append("{\"safety_tier\":\"${applied.size}\"")
            if (profile) append(",\"profile\":{}")
            if (program) append(",\"program\":{\"name\":\"plan\"}")
            append('}')
        }
    }.encodeToByteArray()

    override fun parseFit(bytes: ByteArray): String? = null

    override fun exportGpx(entryId: Long, observedAt: Long): String = ""

    override fun trackFigures(event: ByteArray): String? = null

    override fun compactLog(lines: Array<String>): String =
        """{"dropped":[],"torn":[],"max_entry_id":0,"plan_removed":$planRemoved}"""
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
    override var lastToday: Pair<Long, Int>? = null
    override var published: ViewModel? = null
    val reports = mutableListOf<SendResult>()

    override fun report(r: SendResult) {
        reports += r
    }
}

/**
 * The boot sequence both platforms run ([bootTurn] and the once-per-process
 * decisions) on a stand-in core and a real event log: a fresh install, a returning
 * user, the auto plan and the plan-removed marker, the rotation re-run, an unusable
 * core, the import confirmation and the interrupted-run offer.
 */
class AppBootTest {
    private val dir = newTempDir()
    private val log = LogFiles.sibling(dir, "/$EVENT_LOG_NAME")
    private val store = MemoryStore()
    private val ledger = Ledger()
    private val day = 20_000L to 3_600

    @AfterTest
    fun removeLog() {
        for (suffix in listOf("", ".tmp", ".bak", ".import")) LogFiles.delete(LogFiles.sibling(log, suffix))
        LogFiles.delete(dir)
    }

    private suspend fun boot(core: BootCore, engine: DurableCore = BridgeCore(core), today: Pair<Long, Int> = day): BootTurn? =
        bootTurn(engine, store, ledger, restore = { engine.restoreFrom(log) }, today = today, planDay = { 19_999L })

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
        LogFiles.writeLinesSynced(log, listOf(Event.SetToday(19_000).toJson().toString()))
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
    fun aRemovedPlanIsNotGeneratedAgain() = runTest {
        store.edit { putBoolean(KEY_PLAN_CLEARED, true) }
        val core = BootCore(profile = true)
        assertNull(assertNotNull(boot(core)).view.program)
        assertEquals(listOf("SetToday"), core.variants())
    }

    @Test
    fun aSurvivingProgramDropsAStaleMarker() = runTest {
        store.edit { putBoolean(KEY_PLAN_CLEARED, true) }
        boot(BootCore(profile = true, programAtStart = true))
        assertFalse(store.getBoolean(KEY_PLAN_CLEARED, true), "a ClearPlan never landed")
    }

    @Test
    fun aLogEndingWithThePlanRemovedReArmsALostMarker() = runTest {
        LogFiles.writeLinesSynced(log, listOf(Event.ClearPlan.toJson().toString()))
        val core = BootCore(profile = true, planRemoved = true)
        assertNull(assertNotNull(boot(core)).view.program, "no plan generated over the removal")
        assertTrue(store.getBoolean(KEY_PLAN_CLEARED, false))
    }

    @Test
    fun aReRunOnTheSameDaySendsNothingAndANewDayResendsTheClock() = runTest {
        val core = BootCore()
        val engine = BridgeCore(core)
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
        val engine = BridgeCore(core)
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
