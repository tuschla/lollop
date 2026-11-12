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
import kotlinx.cinterop.addressOf
import kotlinx.cinterop.convert
import kotlinx.cinterop.usePinned
import platform.Foundation.NSFileManager
import platform.Foundation.NSTemporaryDirectory
import platform.Foundation.NSUUID
import platform.posix.SEEK_END
import platform.posix.SEEK_SET
import platform.posix.fclose
import platform.posix.fopen
import platform.posix.fread
import platform.posix.fseek
import platform.posix.ftell
import platform.posix.fwrite

/**
 * The iOS engine as the app runs it: [DurableCore] over the Rust core's C ABI
 * ([NativeCore]) with the event log in a real file. Each [boot] is a new
 * process: the core emptied, a new engine, boot's restore.
 */
class CoreEngineTest {
    private val dir = NSTemporaryDirectory() + "lollop-engine-" + NSUUID().UUIDString
    private val path = "$dir/log.ndjson"

    init {
        check(NSFileManager.defaultManager.createDirectoryAtPath(dir, withIntermediateDirectories = true, attributes = null, error = null))
    }

    @AfterTest
    fun removeDir() {
        NSFileManager.defaultManager.removeItemAtPath(dir, error = null)
    }

    private fun boot(bridge: CoreBridge = NativeCore): DurableCore {
        NativeCore.reset()
        return DurableCore(bridge).also { it.restoreFrom(AppPath(path)) }
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

    private fun write(text: String) {
        val file = checkNotNull(fopen(path, "wb")) { "cannot open $path" }
        try {
            val bytes = text.encodeToByteArray()
            if (bytes.isNotEmpty()) bytes.usePinned { fwrite(it.addressOf(0), 1u, bytes.size.convert(), file) }
        } finally {
            fclose(file)
        }
    }

    private fun read(): String {
        val file = checkNotNull(fopen(path, "rb")) { "cannot open $path" }
        try {
            fseek(file, 0, SEEK_END)
            val size = ftell(file).toInt()
            fseek(file, 0, SEEK_SET)
            val bytes = ByteArray(size)
            if (size > 0) bytes.usePinned { fread(it.addressOf(0), 1u, size.convert(), file) }
            return bytes.decodeToString()
        } finally {
            fclose(file)
        }
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
        engine.restoreFrom(AppPath(path))
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
