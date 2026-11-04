package app.milestone

import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.add
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray

/**
 * A stand-in for the Rust core with the parts of its contract the engine relies on:
 * an event naming a known variant ([EventLog.variantOf]) is accepted (`[]`), any
 * other line is dropped (no bytes); a [poison] line answers `panic_reset` and
 * empties the core, as the FFI firewall does; the view is `{"safety_tier":"<n>"}`
 * with n the events applied, so a test can tell a fresh view from a stand-in;
 * compaction drops every `LogSet` a later `ClearSets` supersedes and reports
 * non-JSON lines as torn.
 */
internal class ScriptedCore : CoreBridge {
    val applied = mutableListOf<String>()
    var poison: Set<String> = emptySet()
    var viewFails = false

    override fun update(event: ByteArray): ByteArray {
        val line = event.decodeToString()
        if (line in poison) {
            applied.clear()
            return """{"error":{"kind":"panic_reset","context":"update"}}""".encodeToByteArray()
        }
        if (EventLog.variantOf(line) == null) return ByteArray(0)
        applied += line
        return "[]".encodeToByteArray()
    }

    override fun view(): ByteArray =
        if (viewFails) {
            """{"error":{"kind":"panic","context":"view"}}""".encodeToByteArray()
        } else {
            """{"safety_tier":"${applied.size}"}""".encodeToByteArray()
        }

    override fun parseFit(bytes: ByteArray): String? = null

    override fun exportGpx(entryId: Long, observedAt: Long): String = ""

    override fun trackFigures(event: ByteArray): String? = null

    override fun compactLog(lines: Array<String>): String {
        val lastClear = lines.indexOfLast { EventLog.variantOf(it) == "ClearSets" }
        val entryId = Regex("\"entry_id\":(\\d+)")
        return buildJsonObject {
            putJsonArray("dropped") {
                lines.indices.filter { it < lastClear && EventLog.variantOf(lines[it]) == "LogSet" }.forEach { add(it) }
            }
            putJsonArray("torn") {
                lines.indices.filter { runCatching { Json.parseToJsonElement(lines[it]) }.isFailure }.forEach { add(it) }
            }
            put("max_entry_id", lines.maxOfOrNull { entryId.find(it)?.groupValues?.get(1)?.toLong() ?: 0L } ?: 0L)
            put("plan_removed", false)
        }.toString()
    }
}

/** A new, empty temporary directory. */
expect fun newTempDir(): LogPath

/**
 * The engine both platforms run ([DurableCore] + [EventLog] over [LogFiles]) on a
 * scripted core and a real file: replay, append, a torn tail, compaction, a core
 * panic reset, a degraded log and a stale view. Runs on the JVM (host tests) and
 * on iOS (simulator), where it also exercises the POSIX file seam.
 */
class DurableCoreTest {
    private val dir = newTempDir()
    private val log = LogFiles.sibling(dir, "/$EVENT_LOG_NAME")

    private fun core(script: ScriptedCore = ScriptedCore()) = script to BridgeCore(script)

    private fun set(id: Long) = Event.LogSet("Back Squat", 100.0, 5, 8.0, observedAt = 1_725_904_800, entryId = id)

    private fun wire(event: Event) = event.toJson().toString()

    private fun lines(): List<String> = buildList { LogFiles.forEachLine(log) { add(it) } }

    private fun write(lines: List<String>) = LogFiles.writeLinesSynced(log, lines)

    @AfterTest
    fun removeLog() {
        for (suffix in listOf("", ".tmp", ".bak", ".import")) LogFiles.delete(LogFiles.sibling(log, suffix))
        LogFiles.delete(dir)
    }

    @Test
    fun aFreshInstallAppendsAndAReturningOneReplays() {
        val (_, first) = core()
        assertTrue(first.restoreFrom(log), "no log yet: a fresh install")
        val sent = first.sendAll(listOf(set(1), set(2), Event.SetToday(20_000)))
        assertTrue(sent.accepted && sent.persisted && !sent.stale)
        assertEquals("3", sent.view.safety_tier, "the view reflects every applied event")
        assertEquals(listOf(wire(set(1)), wire(set(2))), lines(), "persisted events only, one per line")

        val (script, second) = core()
        assertFalse(second.restoreFrom(log), "a log exists: a returning user")
        assertEquals(lines(), script.applied, "boot replays the log in order")
        assertEquals(2L, EventLog.read(log, second).maxEntryId)
    }

    @Test
    fun aLogWithNoLinesLeftIsNotAFreshInstall() {
        write(listOf(""))
        assertFalse(core().second.restoreFrom(log))
    }

    @Test
    fun aSecondRestoreInTheSameProcessReplaysNothing() {
        write(listOf(wire(set(1))))
        val (script, engine) = core()
        engine.restoreFrom(log)
        assertFalse(engine.restoreFrom(log))
        assertEquals(1, script.applied.size, "replayed once")
    }

    @Test
    fun aTornTailIsHealedAndTheNextAppendGetsItsOwnLine() {
        write(listOf(wire(set(1))))
        LogFiles.append(log, wire(set(2)).take(17))
        assertFalse(LogFiles.endsWithNewline(log))

        val (script, engine) = core()
        engine.restoreFrom(log)
        assertEquals(listOf(wire(set(1))), script.applied, "the torn line is not replayed")
        assertEquals(listOf(wire(set(1))), lines(), "boot rewrote the log without it")
        assertTrue(LogFiles.endsWithNewline(log))

        assertTrue(engine.send(set(3)).persisted)
        assertEquals(listOf(wire(set(1)), wire(set(3))), lines())
    }

    @Test
    fun aFailedAppendIsReportedAndTheNextAppendLeadsWithANewline() {
        val (_, engine) = core()
        engine.restoreFrom(log)
        assertTrue(engine.send(set(1)).persisted)

        // A log path that can't be written (its directory is missing): the core
        // applied the event but it is reported not persisted.
        val live = checkNotNull(engine.detachLog())
        engine.reattachLog(LogFiles.sibling(log, ".missing/dir/log"))
        val lost = engine.send(set(2))
        assertTrue(lost.accepted)
        assertFalse(lost.persisted)

        // The failed write may have left a partial line; the next one starts fresh.
        LogFiles.append(live, wire(set(2)).take(11))
        engine.reattachLog(live)
        assertTrue(engine.send(set(3)).persisted)
        assertEquals(listOf(wire(set(1)), wire(set(2)).take(11), wire(set(3))), lines())
    }

    @Test
    fun compactionDropsSupersededLinesAndRewritesALogWorthIt() {
        val old = (1L..40L).map { wire(set(it)) }
        write(old + wire(Event.ClearSets) + wire(set(41)))
        val (script, engine) = core()
        engine.restoreFrom(log)
        val kept = listOf(wire(Event.ClearSets), wire(set(41)))
        assertEquals(kept, script.applied, "the compacted replay")
        assertEquals(kept, lines(), "40 dropped lines are worth a rewrite")
        assertEquals(41L, EventLog.read(log, engine).maxEntryId, "boot raises the entry ids above this")
    }

    @Test
    fun aSmallCompactionLeavesTheDiskAlone() {
        val fill = (2L..60L).map { wire(Event.ComputeHrZones(30.0 + it)) }
        val original = listOf(wire(set(1)), wire(Event.ClearSets)) + fill
        write(original)
        val (script, engine) = core()
        engine.restoreFrom(log)
        assertEquals(original.drop(1), script.applied)
        assertEquals(original, lines(), "dropping one line of 61 is not worth an fsync")
    }

    @Test
    fun anInterruptedImportSwapIsRolledBack() {
        val bak = LogFiles.sibling(log, ".bak")
        LogFiles.writeLinesSynced(bak, listOf(wire(set(1))))
        LogFiles.writeLinesSynced(LogFiles.sibling(log, ".import"), listOf(wire(set(9))))
        val (script, engine) = core()
        assertFalse(engine.restoreFrom(log), "the set-aside log is put back: not a fresh install")
        assertEquals(listOf(wire(set(1))), script.applied)
        assertFalse(LogFiles.exists(bak))
        assertFalse(LogFiles.exists(LogFiles.sibling(log, ".import")))
    }

    @Test
    fun aPanicResetRebuildsTheCoreFromTheLogAndKeepsTheClock() {
        val (script, engine) = core()
        engine.restoreFrom(log)
        engine.send(set(1))
        val today = Event.SetToday(20_000)
        engine.send(today)
        val poison = set(2)
        script.poison = setOf(wire(poison))

        val r = engine.send(poison)
        assertFalse(r.accepted, "the event that reset the core is not saved")
        assertEquals(listOf(wire(set(1)), wire(today)), script.applied, "log replayed, then the clock re-sent")
        assertEquals("2", r.view.safety_tier, "a real view of the rebuilt core, not a blank")
        assertFalse(r.stale)
        assertEquals(listOf(wire(set(1))), lines(), "nothing appended")
    }

    @Test
    fun aLineThatResetsTheCoreOnReplayIsSkippedAndReportedStale() {
        write(listOf(wire(set(1)), wire(set(2)), wire(set(3))))
        val script = ScriptedCore().apply { poison = setOf(wire(set(2))) }
        val (_, engine) = core(script)
        engine.restoreFrom(log)
        assertEquals(listOf(wire(set(1)), wire(set(3))), script.applied)
        assertTrue(engine.send(set(4)).stale, "the core misses a saved line until the next boot")
    }

    @Test
    fun anUnreadableLogStartsEmptyAndDegraded() {
        // A directory where the log should be: it exists but can't be read or appended to.
        val (script, engine) = core()
        assertFalse(engine.restoreFrom(dir))
        assertTrue(script.applied.isEmpty())
        val r = engine.send(set(1))
        assertTrue(r.accepted, "the core still works in memory")
        assertFalse(r.persisted)
        assertTrue(r.stale, "the core misses whatever the log held")
    }

    @Test
    fun aFailingViewKeepsTheLastGoodOneAndIsStale() {
        val (script, engine) = core()
        engine.restoreFrom(log)
        assertEquals("1", engine.send(set(1)).view.safety_tier)
        script.viewFails = true
        val r = engine.send(set(2))
        assertTrue(r.accepted && r.persisted)
        assertTrue(r.stale)
        assertEquals("1", r.view.safety_tier, "the last good view, not a blank one")
        script.viewFails = false
        val fresh = engine.send(set(3))
        assertFalse(fresh.stale)
        assertEquals("3", fresh.view.safety_tier)
    }

    @Test
    fun anUnavailableCoreRefusesEventsAndNeverTouchesTheLog() {
        write(listOf(wire(set(1))))
        val script = ScriptedCore().apply { viewFails = true }
        val (_, engine) = core(script)
        assertFalse(engine.probe())
        assertTrue(engine.unavailable)
        assertFalse(engine.restoreFrom(log))
        assertFalse(engine.send(set(2)).accepted)
        assertTrue(script.applied.isEmpty())
        assertEquals(listOf(wire(set(1))), lines())
        assertNull(engine.compaction(lines()), "no verdict from an unavailable core")
    }

    @Test
    fun linesSplitLikeJavaReadLine() {
        LogFiles.append(log, "a\r\nb\rc\n\nd")
        assertEquals(listOf("a", "b", "c", "", "d"), lines())
        LogFiles.delete(log)
        LogFiles.append(log, "")
        assertEquals(emptyList(), lines())
        assertTrue(LogFiles.endsWithNewline(log), "an empty file counts as terminated")
    }
}
