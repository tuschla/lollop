@file:OptIn(ExperimentalForeignApi::class)

package app.milestone

import app.milestone.core.CoreBridge
import app.milestone.core.NativeCore
import app.milestone.core.cinterop.lollop_panic_for_test
import app.milestone.core.take
import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue
import kotlinx.cinterop.ExperimentalForeignApi

/**
 * The iOS engine as the app runs it: [DurableCore] over the Rust core's C ABI
 * ([NativeCore]) with the event log in a real file. Each [boot] is a new
 * process: the core emptied, a new engine, boot's restore.
 */
class CoreEngineTest {
    private val dir = newTempDir()
    private val log = fileIn(dir, AppFileNames.EVENT_LOG)

    @AfterTest
    fun removeLog() {
        for (suffix in listOf("") + AppFileNames.LOG_SUFFIXES) AppFiles.delete(AppFiles.sibling(log, suffix))
        AppFiles.delete(dir)
    }

    private fun boot(bridge: CoreBridge = NativeCore): DurableCore {
        NativeCore.reset()
        return DurableCore(bridge).also { it.restore(log) }
    }

    private fun set(id: Long) = Event.LogSet("Back Squat", 100.0, 5, 8.0, observedAt = 1_725_904_800, entryId = id)

    private fun wire(event: Event) = event.toJson().toString()

    private val profile = wire(
        Event.SetProfile(
            progressionCadence = ProgressionCadence.EverySession,
            liftGoal = LiftGoal.MaxStrength,
            goalDistance = GoalDistance.FiveK,
            concurrentGoal = ConcurrentGoal.Strength,
            weeklySets = 12,
            runningDaysPerWeek = 3,
            runningKmPerWeek = 30.0,
            advanced = false,
        ),
    )

    private val today = Event.SetToday(19_975)

    private fun liftIds(engine: DurableCore) = engine.currentView().lifts.map { it.entry_id }.toSet()

    private fun write(text: String) = AppFiles.writeText(log, text)

    private fun read(): String = AppFiles.openRead(log).use { f ->
        val size = f.length().toInt()
        val bytes = ByteArray(size)
        if (size > 0) f.readFully(0, bytes, size)
        bytes.decodeToString()
    }

    private fun lines() = read().lines().filter { it.isNotEmpty() }

    @Test
    fun aRestartReplaysTheLogIntoTheCore() {
        write("$profile\n")
        val first = boot()
        assertTrue(first.send(set(1)).let { it.accepted && it.persisted && !it.stale })
        assertTrue(first.sendAll(listOf(set(2), today)).let { it.accepted && it.persisted })
        assertEquals(setOf(1L, 2L), liftIds(first))
        assertEquals(listOf(profile, wire(set(1)), wire(set(2))), lines(), "SetToday is not persisted")

        assertEquals(setOf(1L, 2L), liftIds(boot()), "the restarted core has every saved set")
    }

    @Test
    fun aRebootReplaysTheReplacedLogInProcess() {
        write("$profile\n")
        val engine = boot()
        assertTrue(engine.send(set(1)).persisted)
        write("$profile\n${wire(set(5))}\n")
        engine.resetForReboot()
        engine.restore(log)
        assertEquals(setOf(5L), liftIds(engine), "only the replaced log's sets")
    }

    @Test
    fun bootHealsATornTail() {
        write("$profile\n${wire(set(1))}\n${wire(set(2)).take(20)}")
        val engine = boot()
        assertEquals(setOf(1L), liftIds(engine), "the torn line is not replayed")
        assertEquals("$profile\n${wire(set(1))}\n", read(), "boot rewrote the log without it")
        assertTrue(engine.send(set(3)).persisted)
        assertEquals(setOf(1L, 3L), liftIds(boot()))
    }

    @Test
    fun bootCompactsWithTheCoresRules() {
        val zones = (0 until 3).map { wire(Event.ComputeHrZones(30.0 + it)) }
        // A torn tail: boot rewrites the log with the core's compaction, however little it drops.
        write((listOf(profile) + zones + wire(set(1))).joinToString("") { "$it\n" } + wire(set(2)).take(20))
        val engine = boot()
        val kept = lines()
        assertFalse(zones.first() in kept, "a superseded zone request was dropped from the file: $kept")
        assertTrue(profile in kept && wire(set(1)) in kept)
        assertEquals(setOf(1L), liftIds(engine))
        assertFalse(engine.send(set(2)).stale)
    }

    @Test
    fun aPanicResetRebuildsTheCoreFromTheLog() {
        val poison = set(99)
        val bridge = object : CoreBridge by NativeCore {
            override fun update(event: ByteArray): ByteArray? =
                if (event.decodeToString() == wire(poison)) lollop_panic_for_test(true).take() else NativeCore.update(event)
        }
        write("$profile\n")
        val engine = boot(bridge)
        engine.send(set(1))
        engine.send(today)

        val r = engine.send(poison)
        assertFalse(r.accepted, "the event that reset the core is not saved")
        assertFalse(r.stale, "the rebuilt core has the whole log")
        assertEquals(setOf(1L), r.view.lifts.map { it.entry_id }.toSet())
        assertEquals(listOf(profile, wire(set(1))), lines())
    }
}
