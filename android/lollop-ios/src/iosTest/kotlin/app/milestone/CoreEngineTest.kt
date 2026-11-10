@file:OptIn(ExperimentalForeignApi::class)

package app.milestone

import app.milestone.core.cinterop.LollopBytes
import app.milestone.core.cinterop.lollop_bytes_free
import app.milestone.core.cinterop.lollop_panic_for_test
import app.milestone.core.cinterop.lollop_reset
import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue
import kotlinx.cinterop.CValue
import kotlinx.cinterop.ExperimentalForeignApi
import kotlinx.cinterop.addressOf
import kotlinx.cinterop.convert
import kotlinx.cinterop.readBytes
import kotlinx.cinterop.useContents
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

private const val PROFILE =
    """{"SetProfile":{"progression_cadence":"EverySession","lift_goal":"MaxStrength",""" +
        """"goal_distance":"FiveK","concurrent_goal":"Strength","weekly_sets":12,""" +
        """"running_days_per_week":3,"running_km_per_week":30.0,"advanced":false}}"""

/** Copy a core-owned buffer into a [ByteArray] and free it. */
private fun CValue<LollopBytes>.take(): ByteArray {
    val out = useContents { data?.readBytes(len.convert()) ?: ByteArray(0) }
    lollop_bytes_free(this)
    return out
}

/**
 * The iOS engine as the app runs it: [BridgeCore] over the Rust core's C ABI
 * ([RustCoreBridge]) with the event log in a real file. Each [boot] is a new
 * process: the core emptied, a new engine, boot's restore.
 */
class CoreEngineTest {
    private val dir = NSTemporaryDirectory() + "lollop-engine-" + NSUUID().UUIDString
    private val path = "$dir/event-log.ndjson"

    init {
        check(NSFileManager.defaultManager.createDirectoryAtPath(dir, withIntermediateDirectories = true, attributes = null, error = null))
    }

    @AfterTest
    fun removeDir() {
        NSFileManager.defaultManager.removeItemAtPath(dir, error = null)
    }

    private fun boot(bridge: CoreBridge = RustCoreBridge): BridgeCore {
        lollop_reset()
        return BridgeCore(bridge).also { it.restoreFrom(AppPath(path)) }
    }

    private fun set(id: Long) = Event.LogSet("Back Squat", 100.0, 5, 8.0, observedAt = 1_725_904_800, entryId = id)

    private fun wire(event: Event) = event.toJson().toString()

    private val today = Event.SetToday(19_975)

    private fun liftIds(engine: BridgeCore) = engine.currentView().lifts.map { it.entry_id }.toSet()

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
        write("$PROFILE\n")
        val first = boot()
        assertTrue(first.send(set(1)).let { it.accepted && it.persisted && !it.stale })
        assertTrue(first.sendAll(listOf(set(2), today)).let { it.accepted && it.persisted })
        assertEquals(setOf(1L, 2L), liftIds(first))
        assertEquals(listOf(PROFILE, wire(set(1)), wire(set(2))), lines(), "SetToday is not persisted")

        assertEquals(setOf(1L, 2L), liftIds(boot()), "the restarted core has every saved set")
    }

    @Test
    fun bootHealsATornTail() {
        write("$PROFILE\n${wire(set(1))}\n${wire(set(2)).take(20)}")
        val engine = boot()
        assertEquals(setOf(1L), liftIds(engine), "the torn line is not replayed")
        assertEquals("$PROFILE\n${wire(set(1))}\n", read(), "boot rewrote the log without it")
        assertTrue(engine.send(set(3)).persisted)
        assertEquals(setOf(1L, 3L), liftIds(boot()))
    }

    @Test
    fun bootCompactsWithTheCoresRules() {
        val zones = (0 until 40).map { """{"ComputeHrZones":{"age_years":${30 + it}.0}}""" }
        write((listOf(PROFILE) + zones + wire(set(1))).joinToString("") { "$it\n" })
        val engine = boot()
        val kept = lines()
        assertTrue(kept.size < zones.size, "the superseded zone requests were dropped from the file: $kept")
        assertTrue(PROFILE in kept && wire(set(1)) in kept)
        assertEquals(setOf(1L), liftIds(engine))
        assertFalse(engine.send(set(2)).stale)
    }

    @Test
    fun aPanicResetRebuildsTheCoreFromTheLog() {
        val poison = set(99)
        val bridge = object : CoreBridge by RustCoreBridge {
            override fun update(event: ByteArray): ByteArray? =
                if (event.decodeToString() == wire(poison)) lollop_panic_for_test(true).take() else RustCoreBridge.update(event)
        }
        write("$PROFILE\n")
        val engine = boot(bridge)
        engine.send(set(1))
        engine.send(today)

        val r = engine.send(poison)
        assertFalse(r.accepted, "the event that reset the core is not saved")
        assertFalse(r.stale, "the rebuilt core has the whole log")
        assertEquals(setOf(1L), r.view.lifts.map { it.entry_id }.toSet())
        assertEquals(listOf(PROFILE, wire(set(1))), lines())
    }
}
