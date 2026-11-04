package app.milestone

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import app.milestone.qa.ProcessDeath
import app.milestone.qa.QaHarness
import app.milestone.widget.WidgetSnapshotStore
import app.milestone.widget.widgetSnapshotOf
import app.milestone.qa.QaHarness.daysAgoAtMidday as at
import java.util.Random
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.CyclicBarrier
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference
import kotlin.concurrent.thread
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.double
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * The REAL core (host `libshared.so`) driven over JNI from many threads at once, the
 * way production can reach it: queued sends from the UI ([EventDispatcher] on the
 * `core-jni` executor), the tracker's run save, a multi-activity import
 * ([Core.sendAll]), GPX exports ([Core.exportGpx], from IO), the Finish-review
 * preview ([Core.savedFigures], main), boot compaction ([Core.compaction]),
 * widget/tile reads of the published view, a data export reading the log
 * ([EventLog.read]) while it grows, and rapid Undo/redo.
 *
 * Invariants: no crash, error envelope or panic reset; every send accepted,
 * persisted and fresh; no event lost or duplicated (each entry id is in the view and
 * the log exactly as the last write left it); every thread's view at a quiet point
 * equals a fresh thread's (the Rust view memos are thread-local, so each thread's
 * caches hold states the others moved past); the live view equals a cold reboot's
 * replay of the log; nothing waits past [STALL_SEC] (a deadlock fails, not hangs).
 * `CORE_STRESS_ROUNDS` scales the main stress (default 3).
 */
@RunWith(RobolectricTestRunner::class)
class CoreConcurrencyTest {

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()
    private val logFile get() = QaHarness.logFile(ctx)
    private val json = Json { ignoreUnknownKeys = true }

    @Before
    fun setUp() = QaHarness.reset(ctx)

    @After
    fun tearDown() {
        Core.faultForTest = null
        QaHarness.teardown()
    }

    // ── Harness ────────────────────────────────────────────────────────────────

    private fun boot(events: List<Event>): SendResult {
        QaHarness.seedLog(ctx, events)
        return QaHarness.reboot(ctx)
    }

    private fun sendOk(e: Event, who: String): SendResult {
        val r = Core.send(e)
        check(r.accepted && r.persisted && !r.stale) {
            "$who: ${e::class.simpleName} accepted=${r.accepted} persisted=${r.persisted} stale=${r.stale}"
        }
        return r
    }

    /** A ~0.7 km run east at ~3 m/s, one fix every 3 s, the last at [end]. */
    private fun track(rng: Random, end: Long, entryId: Long): Event.LogRunTrack {
        val n = 240
        var x = 0.0
        val pts = (0 until n).map { i ->
            x += 7.0 + rng.nextInt(40) / 10.0
            GpsPoint(47.0 + rng.nextInt(10) * 1e-6, 8.0 + x / 75_800.0, end - (n - 1 - i) * 3L, 5.0)
        }
        return Event.LogRunTrack(pts, 0.0, observedAt = end, entryId = entryId, utcOffsetSec = utcOffsetSec())
    }

    /** The view a brand-new thread reads: every Rust thread-local view memo is cold. */
    private fun coldView(): ViewModel {
        val out = AtomicReference<Any?>(null)
        thread(name = "cold-view") { out.set(runCatching { Core.currentView() }.getOrElse { it }) }
            .join(STALL_SEC * 1000)
        return when (val v = out.get()) {
            is ViewModel -> v
            is Throwable -> throw v
            else -> error("cold view gave nothing in $STALL_SEC s (deadlock?)")
        }
    }

    /** Run [n] workers; any throw or a worker still running after [STALL_SEC] fails. */
    private fun runWorkers(n: Int, body: (Int) -> Unit) {
        val failure = AtomicReference<Throwable?>(null)
        val threads = (0 until n).map { t ->
            thread(name = "jni-$t") { runCatching { body(t) }.onFailure { failure.compareAndSet(null, it) } }
        }
        threads.forEach { it.join(STALL_SEC * 1000) }
        failure.get()?.let { throw AssertionError("worker failed: ${it.message}", it) }
        threads.firstOrNull { it.isAlive }?.let { fail("${it.name} still running after $STALL_SEC s (deadlock?)") }
    }

    /** Whether wire [line] names entry [id] (the field may close the object). */
    private fun carriesId(line: String, id: Long) =
        "\"entry_id\":$id," in line || "\"entry_id\":$id}" in line

    private fun assertWholeLines(ls: List<String>, what: String) = ls.forEachIndexed { i, l ->
        assertNotNull("$what: line $i is not JSON: ${l.take(80)}", runCatching { json.parseToJsonElement(l) }.getOrNull())
    }

    /** Fields that identify a set row and its last written values. */
    private data class SetRow(val id: Long, val at: Long, val exercise: String, val kg: Double, val reps: Int)

    private fun SetRow.log() = Event.LogSet(exercise, kg, reps, 8.0, observedAt = at, entryId = id)
    private fun SetRow.amend() = Event.AmendSet(id, exercise, kg, reps, 8.0, observedAt = at, observedAtFallback = at)

    // ── 1. Everything at once ───────────────────────────────────────────────────

    @Test
    fun manyThreadsSendingExportingAndReadingLoseNothingAndReplayToTheLiveView() {
        val rounds = System.getenv("CORE_STRESS_ROUNDS")?.toIntOrNull() ?: 3
        val threads = 8
        val perRound = 7
        boot(listOf(QaHarness.hybridTenKProfile(), Event.GeneratePlan(todayEpochDay() - 7)))

        val sets = ConcurrentHashMap<Long, SetRow>()
        val gpsRuns = ConcurrentHashMap<Long, Event.LogRunTrack>()
        val manualRuns = ConcurrentHashMap.newKeySet<Long>()
        val batches = ConcurrentHashMap.newKeySet<List<Long>>()
        val issuedIds = ConcurrentHashMap.newKeySet<Long>()
        val exercises = listOf("Back squat", "Bench press", "Deadlift", "Barbell row")
        val checkpoints = LinkedBlockingQueue<Pair<Int, Result<ViewModel>>>()
        val quiet = CyclicBarrier(threads)
        val released = CyclicBarrier(threads + 1)

        fun id(): Long = EntryIds.next().also { check(issuedIds.add(it)) { "EntryIds.next() repeated $it across threads" } }

        val coordinatorFailure = AtomicReference<Throwable?>(null)
        val coordinator = thread(name = "coordinator") {
            runCatching {
                repeat(rounds) { round ->
                    val views = (0 until threads).map {
                        checkpoints.poll(STALL_SEC, TimeUnit.SECONDS) ?: error("round $round: no checkpoint in $STALL_SEC s (deadlock?)")
                    }
                    val cold = coldView()
                    views.forEach { (t, v) ->
                        val warm = v.getOrElse { throw AssertionError("round $round, thread $t: ${it.message}", it) }
                        assertTrue("round $round: thread $t's view differs from a fresh thread's", warm == cold)
                    }
                    released.await(STALL_SEC, TimeUnit.SECONDS)
                }
            }.onFailure {
                coordinatorFailure.set(it)
                // Wake the workers parked on the barrier instead of letting them time out.
                released.reset()
            }
        }

        try {
            runWorkers(threads) { t ->
                val rng = Random(0x5EEDL + t)
                val mine = ArrayList<Long>()
                val myGps = ArrayList<Long>()
                var failed: Throwable? = null
                fun step() {
                    val stamp = at(rng.nextInt(20), rng.nextInt(30_000).toLong())
                    when (rng.nextInt(12)) {
                        0, 1, 2 -> {
                            val row = SetRow(id(), stamp, exercises[rng.nextInt(4)], 40.0 + 2.5 * rng.nextInt(40), 1 + rng.nextInt(10))
                            sets[row.id] = row
                            mine += row.id
                            sendOk(row.log(), "t$t log set")
                        }
                        3 -> mine.randomOrNull(rng)?.let { sid ->
                            // Delete, then the snackbar's Undo re-logs the row with its own id and stamp.
                            val row = sets.getValue(sid)
                            val gone = sendOk(Event.DeleteEntry(Event.EntryKind.Set, sid, row.at), "t$t delete").view
                            assertTrue("t$t: deleted set $sid still shown", gone.lifts.none { it.entry_id == sid })
                            sendOk(row.log(), "t$t undo delete")
                        }
                        4 -> mine.randomOrNull(rng)?.let { sid ->
                            // Edit, then Undo of the edit (amend back), then maybe redo.
                            val row = sets.getValue(sid)
                            val edited = row.copy(kg = row.kg + 5.0, reps = row.reps + 1)
                            sendOk(edited.amend(), "t$t edit")
                            sendOk(row.amend(), "t$t undo edit")
                            val redo = rng.nextBoolean()
                            if (redo) sendOk(edited.amend(), "t$t redo edit")
                            sets[sid] = if (redo) edited else row
                        }
                        5 -> {
                            val ev = track(rng, stamp, id())
                            // The Finish review previews off the monitor (main thread), then Save sends.
                            val preview = Core.savedFigures(ev) ?: error("t$t: no preview")
                            val view = sendOk(ev, "t$t save run").view
                            val card = view.runs.singleOrNull { it.entry_id == ev.entryId } ?: error("t$t: saved run ${ev.entryId} not shown once")
                            assertEquals("t$t: Finish review km vs saved card", preview.distance_km, card.distance_km, 0.0)
                            assertTrue("t$t: saved GPS run has no route", card.has_gpx)
                            gpsRuns[ev.entryId] = ev
                            myGps += ev.entryId
                            val gpx = Core.exportGpx(ev.entryId, ev.observedAt)
                            assertTrue("t$t: export of run ${ev.entryId} right after its save is empty", !gpx.isNullOrBlank() && "<trkpt" in gpx)
                        }
                        6 -> myGps.randomOrNull(rng)?.let { rid ->
                            val ev = gpsRuns.getValue(rid)
                            val pts = ev.points
                            if (pts.size > 60 && rng.nextBoolean()) {
                                val keep = pts[20].observedAt..pts[pts.size - 20].observedAt
                                sendOk(Event.TrimRunTrack(rid, ev.observedAt, keep.first, keep.last), "t$t trim")
                                gpsRuns[rid] = ev.copy(points = pts.filter { it.observedAt in keep })
                            } else {
                                sendOk(Event.DeleteEntry(Event.EntryKind.Run, rid, ev.observedAt), "t$t delete run")
                                sendOk(ev, "t$t undo delete run")
                            }
                            val gpx = Core.exportGpx(rid, ev.observedAt)
                            assertTrue("t$t: export of live run $rid is empty", !gpx.isNullOrBlank())
                        }
                        7 -> {
                            val run = Event.LogRun(3.0 + rng.nextInt(80) / 10.0, 20.0 + rng.nextInt(30), 0.0, observedAt = stamp, entryId = id(), utcOffsetSec = utcOffsetSec())
                            manualRuns += run.entryId
                            sendOk(run, "t$t manual run")
                        }
                        8 -> {
                            // A multi-activity import: one batch, one monitor turn.
                            val batch = (0 until 3).map { track(rng, at(25 + rng.nextInt(5), it * 4_000L), id()) }
                            val r = Core.sendAll(batch)
                            check(r.accepted && r.persisted && !r.stale) { "t$t import: accepted=${r.accepted} persisted=${r.persisted} stale=${r.stale}" }
                            batch.forEach { gpsRuns[it.entryId] = it }
                            batches += batch.map { it.entryId }
                        }
                        9 -> {
                            // A data export / boot compaction reading the log while it grows.
                            val read = EventLog.read(logFile).lines
                            assertWholeLines(read, "t$t export read")
                            val dropped = checkNotNull(Core.compaction(read)) { "t$t: no compaction verdict" }.dropped
                            assertTrue("t$t: compaction drops lines it was not given: $dropped", dropped.all { it in read.indices })
                        }
                        else -> {
                            // Widget / tile / reminder reads of the current view.
                            val v = Core.currentView()
                            widgetSnapshotOf(v, DistanceUnit.Km, "UTC")
                            widgetSnapshotOf(v, DistanceUnit.Mi, "UTC")
                        }
                    }
                }
                repeat(rounds) {
                    repeat(perRound) { if (failed == null) runCatching { step() }.onFailure { failed = it } }
                    quiet.await(STALL_SEC, TimeUnit.SECONDS)
                    checkpoints.put(t to (failed?.let { Result.failure(it) } ?: runCatching { Core.currentView() }))
                    released.await(STALL_SEC, TimeUnit.SECONDS)
                }
                failed?.let { throw it }
            }
        } finally {
            coordinator.join(STALL_SEC * 1000)
            coordinatorFailure.get()?.let { throw it }
        }
        assertFalse("coordinator still running (deadlock?)", coordinator.isAlive)

        val live = Core.currentView()
        // Nothing lost, nothing doubled: each id once, with its last write.
        val liftIds = live.lifts.groupingBy { it.entry_id }.eachCount()
        for ((sid, row) in sets) {
            assertEquals("set $sid shown once", 1, liftIds[sid])
            val v = live.lifts.single { it.entry_id == sid }
            assertEquals("set $sid last write", row.kg to row.reps, v.weight_kg to v.reps)
        }
        assertEquals("only the sent sets", sets.keys, liftIds.keys - 0L)
        val runIds = live.runs.groupingBy { it.entry_id }.eachCount()
        assertEquals("every run once", (gpsRuns.keys + manualRuns).associateWith { 1 }, runIds)
        // An import batch's lines are contiguous in the log (one monitor turn).
        val logLines = QaHarness.logLines(ctx)
        assertWholeLines(logLines, "final log")
        for (batch in batches) {
            val idx = batch.map { id -> logLines.indexOfFirst { it.startsWith("{\"LogRunTrack\"") && carriesId(it, id) } }
            assertEquals("import batch $batch interleaved: lines $idx", idx.first() + 1, idx[1])
            assertEquals("import batch $batch interleaved: lines $idx", idx.first() + 2, idx[2])
        }
        val exports = gpsRuns.values.associate { it.entryId to Core.exportGpx(it.entryId, it.observedAt) }
        exports.forEach { (rid, g) -> assertTrue("run $rid: empty export", !g.isNullOrBlank()) }

        // A cold boot of the durable log renders the live view and the same routes.
        val replayed = QaHarness.reboot(ctx)
        assertTrue("replayed view is stale", !replayed.stale)
        assertTrue("reboot view differs from the live view", replayed.view == live)
        for ((rid, g) in exports) {
            assertEquals("run $rid: export after reboot", g, Core.exportGpx(rid, gpsRuns.getValue(rid).observedAt))
        }
    }

    // ── 2. Racing edits of ONE entry ─────────────────────────────────────────────

    /** Six threads edit, delete and re-log (Undo) the same set at once. Whatever the
     *  interleaving, the rows the core shows for that id are exactly what the log
     *  order (= apply order, one monitor) gives under the core's own matcher (a delete
     *  removes the newest row with the id, an amend replaces it at the tail and is a
     *  no-op on a deleted row, a log pushes a row): nothing lost or doubled, and a
     *  reboot of the compacted log agrees. */
    @Test
    fun racingEditsOfOneEntryKeepExactlyWhatTheLogOrderSays() {
        boot(listOf(QaHarness.strengthProfile()))
        val base = SetRow(EntryIds.next(), at(1), "Back squat", 100.0, 5)
        sendOk(base.log(), "seed")
        runWorkers(6) { t ->
            val rng = Random(0xED17L + t)
            repeat(40) { i ->
                val row = base.copy(kg = 100.0 + t * 10 + i % 5, reps = 1 + (i + t) % 9)
                when (rng.nextInt(3)) {
                    0 -> sendOk(row.amend(), "t$t amend")
                    1 -> sendOk(Event.DeleteEntry(Event.EntryKind.Set, base.id, base.at), "t$t delete")
                    else -> sendOk(row.log(), "t$t re-log")
                }
            }
        }
        val live = Core.currentView()
        val expected = ArrayList<Pair<Double, Int>>()
        for (l in QaHarness.logLines(ctx).filter { carriesId(it, base.id) }) {
            val o = json.parseToJsonElement(l).jsonObject
            val variant = o.keys.single()
            val f = o.getValue(variant).jsonObject
            val values = { f.getValue("weight_kg").jsonPrimitive.double to f.getValue("reps").jsonPrimitive.int }
            when (variant) {
                "LogSet" -> expected += values()
                "DeleteEntry" -> if (expected.isNotEmpty()) expected.removeAt(expected.lastIndex)
                "AmendSet" -> if (expected.isNotEmpty()) { expected.removeAt(expected.lastIndex); expected += values() }
                else -> fail("unexpected line $l")
            }
        }
        val shown = live.lifts.filter { it.entry_id == base.id }.map { it.weight_kg to it.reps }
        assertEquals("rows for set ${base.id} vs the log order", expected.sortedBy { it.toString() }, shown.sortedBy { it.toString() })
        assertTrue("reboot view differs from the live view", QaHarness.reboot(ctx).view == live)
    }

    // ── 3. A panic reset in the middle of the stress ─────────────────────────────

    /** One thread's event panics the core (the FFI swaps in an EMPTY bridge and
     *  answers `panic_reset`) while five others keep logging. [Core.apply] rebuilds
     *  the fresh core from the durable log under the same monitor, so no other
     *  thread's accepted entry is lost, the poisoned event is refused and never
     *  persisted, and every later view (on any thread) is the full model. */
    @Test
    fun aPanicResetMidStressRebuildsFromTheLogAndLosesNothingElse() {
        boot(listOf(QaHarness.strengthProfile(), Event.GeneratePlan(todayEpochDay() - 7)))
        val nativeUpdate = Core::class.java.getDeclaredMethod("update", ByteArray::class.java).apply { isAccessible = true }
        val nativeView = Core::class.java.getDeclaredMethod("view").apply { isAccessible = true }
        val resets = AtomicInteger()
        Core.faultForTest = object : CoreFault {
            override fun update(event: ByteArray): ByteArray? {
                if (POISON in String(event, Charsets.UTF_8)) {
                    // What ffi.rs catch_panic(reset = true) does: a fresh, empty bridge.
                    Core.resetForTest()
                    resets.incrementAndGet()
                    return """{"error":{"kind":"panic_reset","context":"update","message":"injected"}}""".toByteArray()
                }
                return nativeUpdate.invoke(Core, event) as ByteArray?
            }

            override fun view(): ByteArray? = nativeView.invoke(Core) as ByteArray?
        }
        val accepted = ConcurrentHashMap.newKeySet<Long>()
        runWorkers(6) { t ->
            repeat(25) { i ->
                val stamp = at(i % 10, t * 1_000L + i)
                if (t == 0 && i % 5 == 4) {
                    // Entries accepted before this send are in the durable log the reset replays.
                    val before = HashSet(accepted)
                    val r = Core.send(Event.LogSet(POISON, 50.0, 5, 8.0, observedAt = stamp))
                    assertFalse("t0: the poisoned event was reported saved", r.accepted)
                    val shown = r.view.lifts.map { it.entry_id }.toSet()
                    assertTrue("t0: the view after the reset lost ${before - shown}", shown.containsAll(before))
                } else {
                    val e = Event.LogSet("Lift $t", 50.0 + i, 5, 8.0, observedAt = stamp)
                    val r = sendOk(e, "t$t#$i")
                    accepted += e.entryId
                    assertTrue("t$t#$i: own set missing from its view", r.view.lifts.any { it.entry_id == e.entryId })
                }
            }
        }
        Core.faultForTest = null
        assertEquals("injected resets", 5, resets.get())
        val live = Core.currentView()
        val ids = live.lifts.groupingBy { it.entry_id }.eachCount()
        assertEquals("every accepted set once, nothing else", accepted.associateWith { 1 }, ids)
        assertTrue("the poisoned event reached the log", QaHarness.logLines(ctx).none { POISON in it })
        assertTrue("reboot view differs from the live view", QaHarness.reboot(ctx).view == live)
    }

    // ── 4. Boot compaction racing the first sends ───────────────────────────────

    /** Boot's restore compacts and rewrites a long log while four threads queue their
     *  first entries the moment it begins: every send waits for the rewrite, lands in
     *  the NEW file exactly once, and a second boot replays to the same view. */
    @Test
    fun sendsRacingBootCompactionAllLandInTheRewrittenLogOnce() {
        val profile = QaHarness.strengthProfile()
        val zones = (0 until 60).map { Event.ComputeHrZones(30.0 + it) }
        val old = (0 until 10).map { Event.LogSet("Back squat", 80.0 + it, 5, 8.0, observedAt = at(30 + it), entryId = at(30 + it) * 1000) }
        QaHarness.seedLog(ctx, listOf(profile) + zones + old)
        ProcessDeath.kill(ctx)
        val restoredFlag = Core::class.java.getDeclaredField("restored").apply { isAccessible = true }
        val sent = ConcurrentHashMap.newKeySet<Long>()
        val booter = thread(name = "boot") { Core.restore(ctx) }
        runWorkers(4) { t ->
            val deadline = System.currentTimeMillis() + STALL_SEC * 1000
            while (restoredFlag.get(Core) != true) {
                check(System.currentTimeMillis() < deadline) { "restore never started" }
                Thread.onSpinWait()
            }
            repeat(15) { i ->
                val e = Event.LogSet("Lift $t", 60.0 + i, 5, 8.0, observedAt = at(i % 7, t * 100L + i))
                sendOk(e, "t$t#$i")
                sent += e.entryId
            }
        }
        booter.join(STALL_SEC * 1000)
        assertFalse("boot still running (deadlock?)", booter.isAlive)
        val live = Core.send(Event.SetToday(todayEpochDay(), utcOffsetSec())).view
        val onDisk = QaHarness.logLines(ctx)
        assertTrue("the log was not compacted", onDisk.none { it.startsWith("{\"ComputeHrZones\"") && "\"age_years\":30.0" in it })
        for (id in sent) assertEquals("set $id on disk once", 1, onDisk.count { carriesId(it, id) })
        assertEquals("every set once", (sent + old.map { it.entryId }).associateWith { 1 }, live.lifts.groupingBy { it.entry_id }.eachCount())
        assertTrue("reboot view differs from the live view", QaHarness.reboot(ctx).view == live)
    }

    // ── 5. Widget, tile and reminder reads racing the snapshot writes ────────────

    /** [WidgetSync] writes the snapshot of each published view (app scope), the
     *  midnight / zone receiver marks it stale, and the tile and the session reminder
     *  read it from their own threads. Once a snapshot exists every read returns one
     *  that was written (never a torn or missing file), and nothing deadlocks. */
    @Test
    fun widgetTileAndReminderReadsRacingSnapshotWritesSeeOnlyWholeSnapshots() {
        val view = boot(listOf(QaHarness.strengthProfile(), Event.GeneratePlan(todayEpochDay() - 7))).view
        val base = widgetSnapshotOf(view, DistanceUnit.Km, "UTC")
        val snapshots = (0 until 50).map { base.copy(weekDone = it) }
        val written = snapshots.toHashSet() + snapshots.map { it.copy(stale = true) }
        WidgetSnapshotStore.write(ctx, snapshots[0])
        runWorkers(8) { t ->
            repeat(150) { i ->
                when (t % 4) {
                    0, 1 -> WidgetSnapshotStore.write(ctx, snapshots[(i * 7 + t) % snapshots.size])
                    2 -> WidgetSnapshotStore.markStale(ctx, force = i % 2 == 0)
                    else -> {
                        val r = WidgetSnapshotStore.read(ctx)
                        assertNotNull("t$t#$i: snapshot unreadable mid-write", r)
                        assertTrue("t$t#$i: read a snapshot nobody wrote: $r", r in written)
                        WidgetSnapshotStore.today(ctx)
                    }
                }
            }
        }
    }

    private fun <T> List<T>.randomOrNull(rng: Random): T? = if (isEmpty()) null else this[rng.nextInt(size)]

    private companion object {
        /** Seconds any single wait may take before the test calls it a deadlock. */
        const val STALL_SEC = 120L

        /** An exercise name the fault-injected core "panics" on. */
        const val POISON = "PANIC PROBE"
    }
}
