package app.milestone

import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * The engine both platforms run ([DurableCore] + [EventLog] over [AppFiles]) on a
 * stand-in core ([FakeCore]) and a real file: replay, append, a torn tail,
 * compaction, a core panic reset, a degraded log and a stale view. Runs on the JVM (host tests) and
 * on iOS (simulator), where it also exercises the POSIX file seam.
 */
class DurableCoreTest : PlatformTest() {
    private val dir = newTempDir()
    private val log = fileIn(dir, AppFileNames.EVENT_LOG)

    private fun core(script: FakeCore = FakeCore()) = script to DurableCore(script)

    private fun set(id: Long) = Event.LogSet("Back Squat", 100.0, 5, 8.0, observedAt = 1_725_904_800, entryId = id)

    private fun wire(event: Event) = event.toJson().toString()

    private fun lines(): List<String> = buildList { AppFiles.forEachLine(log) { add(it) } }

    private fun write(lines: List<String>) = AppFiles.writeLinesSynced(log, lines)

    @AfterTest
    fun removeLog() {
        for (suffix in listOf("") + AppFileNames.LOG_SUFFIXES) AppFiles.delete(AppFiles.sibling(log, suffix))
        AppFiles.delete(dir)
    }

    @Test
    fun aFreshInstallAppendsAndAReturningOneReplays() {
        val (_, first) = core()
        assertTrue(first.restoreFrom(log), "no log yet: a fresh install")
        val sent = first.sendAll(listOf(set(1), set(2), Event.SetToday(20_000)))
        assertTrue(sent.accepted && sent.persisted && !sent.stale)
        assertEquals(3, sent.view.input_count, "the view reflects every applied event")
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
        AppFiles.append(log, wire(set(2)).take(17))
        assertFalse(AppFiles.endsWithNewline(log))

        val (script, engine) = core()
        engine.restoreFrom(log)
        assertEquals(listOf(wire(set(1))), script.applied, "the torn line is not replayed")
        assertEquals(listOf(wire(set(1))), lines(), "boot rewrote the log without it")
        assertTrue(AppFiles.endsWithNewline(log))

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
        engine.reattachLog(AppFiles.sibling(log, ".missing/dir/log"))
        val lost = engine.send(set(2))
        assertTrue(lost.accepted)
        assertFalse(lost.persisted)

        // The failed write may have left a partial line; the next one starts fresh.
        AppFiles.append(live, wire(set(2)).take(11))
        engine.reattachLog(live)
        assertTrue(engine.send(set(3)).persisted)
        assertEquals(listOf(wire(set(1)), wire(set(2)).take(11), wire(set(3))), lines())
    }

    @Test
    fun compactionDropsSupersededLinesAndRewritesALogWorthIt() {
        // Just enough dropped lines for a rewrite, in a log too long for their share to count.
        val dropped = EventLog.REWRITE_MIN_DROPPED_LINES
        val old = (1L..dropped).map { wire(set(it)) }
        val kept = listOf(wire(Event.ClearSets), wire(set(dropped + 1L))) +
            (1..dropped * EventLog.REWRITE_MIN_DROPPED_SHARE_INVERSE - 1).map { wire(Event.ComputeHrZones(30.0 + it)) }
        write(old + kept)
        val (script, engine) = core()
        engine.restoreFrom(log)
        assertEquals(kept, script.applied, "the compacted replay")
        assertEquals(kept, lines(), "$dropped dropped lines are worth a rewrite")
        assertEquals(dropped + 1L, EventLog.read(log, engine).maxEntryId, "boot raises the entry ids above this")
    }

    @Test
    fun aSmallCompactionLeavesTheDiskAlone() {
        // One dropped line in a log one line longer than the share that rewrites.
        val fill = (1..EventLog.REWRITE_MIN_DROPPED_SHARE_INVERSE - 1).map { wire(Event.ComputeHrZones(30.0 + it)) }
        val original = listOf(wire(set(1)), wire(Event.ClearSets)) + fill
        write(original)
        val (script, engine) = core()
        engine.restoreFrom(log)
        assertEquals(original.drop(1), script.applied)
        assertEquals(original, lines(), "dropping one line of ${original.size} is not worth an fsync")
    }

    @Test
    fun anInterruptedImportSwapIsRolledBack() {
        val bak = AppFiles.sibling(log, AppFileNames.BAK_SUFFIX)
        AppFiles.writeLinesSynced(bak, listOf(wire(set(1))))
        AppFiles.writeLinesSynced(AppFiles.sibling(log, AppFileNames.IMPORT_SUFFIX), listOf(wire(set(9))))
        val (script, engine) = core()
        assertFalse(engine.restoreFrom(log), "the set-aside log is put back: not a fresh install")
        assertEquals(listOf(wire(set(1))), script.applied)
        assertFalse(AppFiles.exists(bak))
        assertFalse(AppFiles.exists(AppFiles.sibling(log, AppFileNames.IMPORT_SUFFIX)))
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
        assertEquals(2, r.view.input_count, "a real view of the rebuilt core, not a blank")
        assertFalse(r.stale)
        assertEquals(listOf(wire(set(1))), lines(), "nothing appended")
    }

    @Test
    fun aLineThatResetsTheCoreOnReplayIsSkippedAndReportedStale() {
        write(listOf(wire(set(1)), wire(set(2)), wire(set(3))))
        val script = FakeCore().apply { poison = setOf(wire(set(2))) }
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
        assertEquals(1, engine.send(set(1)).view.input_count)
        script.viewFails = true
        val r = engine.send(set(2))
        assertTrue(r.accepted && r.persisted)
        assertTrue(r.stale)
        assertEquals(1, r.view.input_count, "the last good view, not a blank one")
        script.viewFails = false
        val fresh = engine.send(set(3))
        assertFalse(fresh.stale)
        assertEquals(3, fresh.view.input_count)
    }

    @Test
    fun anUnavailableCoreRefusesEventsAndNeverTouchesTheLog() {
        write(listOf(wire(set(1))))
        val script = FakeCore().apply { viewFails = true }
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
        AppFiles.append(log, "a\r\nb\rc\n\nd")
        assertEquals(listOf("a", "b", "c", "", "d"), lines())
        AppFiles.delete(log)
        AppFiles.append(log, "")
        assertEquals(emptyList(), lines())
        assertTrue(AppFiles.endsWithNewline(log), "an empty file counts as terminated")
    }
}
