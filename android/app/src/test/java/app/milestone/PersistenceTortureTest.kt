package app.milestone

import android.content.Context
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.test.core.app.ApplicationProvider
import app.milestone.qa.LongHistorySeed
import app.milestone.qa.ProcessDeath
import app.milestone.qa.QaHarness
import app.milestone.qa.QaHarness.onAppScope
import app.milestone.qa.launchApp
import app.milestone.qa.str
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.IOException
import java.io.InputStream
import java.util.Locale
import java.util.concurrent.CountDownLatch
import java.util.concurrent.CyclicBarrier
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference
import kotlin.concurrent.thread
import kotlin.system.exitProcess
import kotlinx.serialization.json.Json
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * Fault-injection / torture tests of the shell's persistence: the append-only event
 * log ([Core.apply]'s append, [EventLog.load]'s compaction rewrite), the data
 * backup swap ([swapLogFile] / [streamValidateImport]), the setup/theme prefs and the
 * run sidecar ([ActiveRunStore]).
 *
 * Invariants checked throughout: a committed event is never lost and never
 * duplicated, a crash at any byte leaves a log whose replay equals the pre-crash
 * committed state, the compaction rewrite is atomic, and the app always boots.
 *
 * The event log tests drive the REAL core over JNI (host `libshared.so`); a "crash"
 * is the on-disk bytes a killed process leaves behind plus [ProcessDeath.kill]
 * (every process-scoped singleton dropped, disk kept), and a "boot" is
 * [Core.restore] + the boot `SetToday`, exactly as the boot effect runs them.
 */
@RunWith(RobolectricTestRunner::class)
class PersistenceTortureTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()
    private val logFile get() = File(ctx.filesDir, "event-log.ndjson")
    private val sidecar get() = File(ctx.filesDir, "active-run.ndjson")
    private val json = Json { ignoreUnknownKeys = true }

    @Before
    fun setUp() {
        Locale.setDefault(Locale.US)
        QaHarness.reset(ctx)
        File(ctx.filesDir, "event-log.ndjson.import").delete()
        QaHarness.resetActiveRunStore(ctx)
        sidecar.delete()
    }

    @After
    fun tearDown() {
        ctx.filesDir.setWritable(true, true)
        logFile.setWritable(true, true)
        File(ctx.filesDir, "event-log.ndjson.tmp").let { if (it.isDirectory) it.deleteRecursively() }
        ActiveRunStore.flush()
        sidecar.delete()
        QaHarness.teardown()
        RunSession.reset()
    }

    // ── Fixtures ────────────────────────────────────────────────────────────────

    private fun at(daysAgo: Int, plusSec: Long = 0) = QaHarness.daysAgoAtMidday(daysAgo) + plusSec

    private fun line(e: Event) = e.toJson().toString()

    /** A small track (a 600 m loop, one fix / 5 s). */
    private fun track(startSec: Long, entryId: Long): Event.LogRunTrack {
        val pts = (0 until 40).map { i ->
            GpsPoint(52.52 + i * 0.00012, 13.405 + (i % 7) * 0.00003, startSec + i * 5L, 5.0)
        }
        return Event.LogRunTrack(pts, 0.0, observedAt = pts.last().observedAt, entryId = entryId)
    }

    /** The committed history every crash test starts from: a profile, a plan, sets
     *  (one deleted), a manual run, a track, a check-in and a pain report. Multibyte
     *  exercise names make a torn line also a torn UTF-8 sequence. */
    private fun committed(): List<Event> = listOf(
        QaHarness.strengthProfile(),
        Event.GeneratePlan(todayEpochDay() - 7),
        Event.LogSet("Kniebeuge – Über", 100.0, 5, 8.0, observedAt = at(3), entryId = at(3) * 1000),
        Event.LogSet("Bench Press", 80.0, 5, 8.0, observedAt = at(3, 300), entryId = at(3, 300) * 1000),
        Event.LogRun(5.0, 27.5, 0.0, observedAt = at(2), entryId = at(2) * 1000),
        Event.SubmitCheckin(observedAt = at(1), sleepQuality = 4, soreness = 2, mood = 4, fatigue = 2, stress = 2),
        Event.DeleteEntry(Event.EntryKind.Set, at(3, 300) * 1000),
        track(at(1, 3600), at(1, 3600) * 1000),
    )

    /** The event a crash tears: multibyte name, so the cut lands inside UTF-8 too. */
    private val torn = Event.LogSet("深蹲 🏋 Squat", 102.5, 3, 9.0, observedAt = at(0, -7200), entryId = at(0, -7200) * 1000)

    /** The first event the next process appends after the crash. */
    private val next = Event.LogSet("Deadlift", 140.0, 3, 8.5, observedAt = at(0, -3600), entryId = at(0, -3600) * 1000)

    private fun writeLog(events: List<Event>) = QaHarness.seedLog(ctx, events)

    /** Process death, then a cold boot exactly as the boot effect runs it. */
    private fun reboot(): SendResult {
        ProcessDeath.kill(ctx)
        Core.restore(ctx)
        return Core.send(Event.SetToday(todayEpochDay(), utcOffsetSec()))
    }

    /** The view a cold boot of exactly [events] produces (a clean, uncrashed log). */
    private fun viewOf(events: List<Event>): ViewModel {
        writeLog(events)
        return reboot().view
    }

    /** Every non-blank line on disk parses as JSON (no torn line survived). */
    private fun assertAllLinesParse(f: File = logFile) {
        f.readLines().filter { it.isNotBlank() }.forEachIndexed { i, l ->
            assertNotNull("line $i must parse: ${l.take(80)}", runCatching { json.parseToJsonElement(l) }.getOrNull())
        }
    }

    private fun countLines(predicate: (String) -> Boolean) =
        logFile.readLines().count { it.isNotBlank() && predicate(it) }

    private fun entryIdCount(id: Long) = countLines { "\"entry_id\":$id" in it }

    private fun setField(name: String, value: Any?) {
        Core.javaClass.getDeclaredField(name).apply { isAccessible = true }.set(Core, value)
    }

    // ── 1. Crash at every byte of an append ─────────────────────────────────────

    /**
     * A kill mid-`appendText` leaves the committed log plus a prefix of the new line.
     * For EVERY prefix length: the next boot replays exactly the committed state (the
     * torn event counts only once its whole JSON is on disk), the tail is healed, and
     * the next process's first append lands as its own line: after another reboot the
     * model is committed + that append, each event exactly once.
     */
    @Test
    fun aCrashAtEveryByteOfAnAppendReplaysTheCommittedStateAndTheNextAppendSurvives() {
        val base = committed()
        val baseView = viewOf(base)
        val withTorn = viewOf(base + torn)
        val baseNext = viewOf(base + next)
        val tornNext = viewOf(base + torn + next)
        assertFalse("fixture: the torn event must change the model", baseView == withTorn)

        val prefix = base.joinToString("") { line(it) + "\n" }.toByteArray()
        val wire = line(torn).toByteArray()
        for (k in 0..wire.size) {
            logFile.writeBytes(prefix + wire.copyOf(k))
            val complete = k == wire.size
            val booted = reboot()
            assertEquals("cut at byte $k/${wire.size}: replay = committed state", if (complete) withTorn else baseView, booted.view)
            val r = Core.send(next)
            assertTrue("cut at $k: the next append is persisted", r.accepted && r.persisted)
            assertAllLinesParse()
            assertEquals("cut at $k: the next append is on disk once", 1, entryIdCount(next.entryId))
            assertEquals("cut at $k: a committed set is never duplicated", 1, entryIdCount(at(3) * 1000))
            val after = reboot().view
            assertEquals("cut at $k: replay after the second boot", if (complete) tornNext else baseNext, after)
        }
    }

    /** [EventLog.read] (export, panic-reset replay) of a log torn at every byte is
     *  exactly the committed lines and never touches the file. */
    @Test
    fun aReadOfALogTornAtEveryByteReturnsOnlyCommittedLinesAndLeavesTheBytes() {
        val base = committed().map(::line)
        val prefix = base.joinToString("") { "$it\n" }.toByteArray()
        val wire = line(torn).toByteArray()
        val committedOnly = EventLog.compact(base)
        val withTorn = EventLog.compact(base + line(torn))
        for (k in 0..wire.size) {
            val bytes = prefix + wire.copyOf(k)
            logFile.writeBytes(bytes)
            val read = EventLog.read(logFile)
            assertEquals("cut at $k", if (k == wire.size) withTorn else committedOnly, read.lines)
            assertTrue("cut at $k: read() must not rewrite", bytes.contentEquals(logFile.readBytes()))
            val loaded = EventLog.load(logFile)
            assertEquals("cut at $k: load == read", read.lines, loaded.lines)
            assertTrue("cut at $k: the healed log ends in a newline", EventLog.endsWithNewline(logFile))
            assertEquals("cut at $k: a second load is a fixed point", loaded.lines, EventLog.load(logFile).lines)
        }
    }

    // ── 2. Crash at every byte of the compaction rewrite / import swap ──────────

    /** A log big enough in superseded lines that [EventLog.load] rewrites it. */
    private fun compactableLog(): List<String> {
        val base = committed().map(::line)
        val zones = (0 until 40).map { """{"ComputeHrZones":{"age_years":${30 + it}.0}}""" }
        return base.take(3) + zones + base.drop(3)
    }

    /**
     * Every on-disk state a kill can leave during [EventLog.load]'s rewrite (tmp torn
     * at every byte, tmp whole but not renamed, renamed) and during [swapLogFile] (an
     * `.import` torn at every byte, the live log set aside as `.bak` with or without
     * the new one in place): the next load returns exactly one consistent log (the
     * old one's compaction, or the new one's), and leaves no temporary behind.
     */
    @Test
    fun everyCrashStateOfTheRewriteAndTheImportSwapLoadsOneWholeLog() {
        val original = compactableLog()
        val originalBytes = original.joinToString("") { "$it\n" }.toByteArray()
        val compacted = EventLog.compact(original)
        assertTrue("fixture: the load must rewrite", EventLog.compactionWorthRewriting(original.size - compacted.size, original.size))
        val compactedBytes = compacted.joinToString("\n", postfix = "\n").toByteArray()
        val tmp = File(ctx.filesDir, "event-log.ndjson.tmp")
        val imp = File(ctx.filesDir, "event-log.ndjson.import")
        val bak = File(ctx.filesDir, "event-log.ndjson.bak")
        val imported = listOf(line(QaHarness.strengthProfile()), line(next))
        val importedBytes = imported.joinToString("\n", postfix = "\n").toByteArray()

        fun check(state: String, expected: List<String>) {
            val got = EventLog.load(logFile)
            assertEquals("$state: lines", expected, got.lines)
            assertFalse("$state: not a fresh install", got.freshInstall)
            for (f in listOf(tmp, imp, bak)) assertFalse("$state: ${f.name} left behind", f.exists())
            assertAllLinesParse()
            assertEquals("$state: load is a fixed point", expected, EventLog.load(logFile).lines)
        }

        val stride = maxOf(1, compactedBytes.size / 400)
        for (k in (0..compactedBytes.size step stride) + compactedBytes.size) {
            logFile.writeBytes(originalBytes)
            tmp.writeBytes(compactedBytes.copyOf(k))
            check("rewrite tmp torn at $k", compacted)
            assertTrue("rewrite tmp torn at $k: original replaced by its compaction", compactedBytes.contentEquals(logFile.readBytes()))
        }
        // Renamed: the compacted log is in place; nothing more to drop.
        logFile.writeBytes(compactedBytes)
        check("rewrite renamed", compacted)
        assertTrue(compactedBytes.contentEquals(logFile.readBytes()))

        for (k in 0..importedBytes.size) {
            logFile.writeBytes(originalBytes)
            imp.writeBytes(importedBytes.copyOf(k))
            check("import tmp torn at $k", compacted)
        }
        // swapLogFile's fallback path: live log moved to .bak, new one not yet in place.
        logFile.delete()
        bak.writeBytes(originalBytes)
        imp.writeBytes(importedBytes)
        check("swap: live set aside, new not renamed", compacted)
        // ...new one renamed in, .bak not yet deleted: the import committed.
        logFile.writeBytes(importedBytes)
        bak.writeBytes(originalBytes)
        check("swap: new in place, .bak still there", EventLog.compact(imported))
    }

    /** A rewrite that cannot even open its temporary (disk full / a stray directory
     *  in the way) leaves the original log untouched, still returns the committed
     *  state, and the core's first append after it cannot glue onto the torn tail. */
    @Test
    fun aRewriteThatFailsMidCompactionKeepsTheOriginalAndTheNextAppendStillLands() {
        val base = committed()
        val baseNext = viewOf(base + next)
        val baseView = viewOf(base)
        val tmp = File(ctx.filesDir, "event-log.ndjson.tmp")
        val tornBytes = base.joinToString("") { line(it) + "\n" }.toByteArray() + line(torn).toByteArray().copyOf(23)
        logFile.writeBytes(tornBytes)
        tmp.mkdirs()
        File(tmp, "blocker").writeText("x")
        val booted = reboot()
        assertEquals("replay = committed state", baseView, booted.view)
        assertTrue("the failed rewrite left the original bytes", tornBytes.contentEquals(logFile.readBytes()))
        val r = Core.send(next)
        assertTrue(r.accepted && r.persisted)
        tmp.deleteRecursively()
        assertEquals("next append replays after the reboot", baseNext, reboot().view)
        assertEquals(1, entryIdCount(next.entryId))
    }

    // ── 3. Garbage, partial UTF-8 and lines from a newer app ────────────────────

    @Test
    fun garbageAndPartialUtf8LinesNeverCostAnIntactEvent() {
        val base = committed()
        val baseView = viewOf(base)
        val out = ByteArrayOutputStream()
        fun ln(b: ByteArray) { out.write(b); out.write('\n'.code) }
        val lines = base.map { line(it).toByteArray() }
        ln(lines[0])
        ln(byteArrayOf(0xC3.toByte(), 0x28, 0x7B, 0xFF.toByte(), 0xFE.toByte()))   // invalid UTF-8
        ln(lines[1])
        ln(byteArrayOf(0, 0, 0, 0))                                                   // NULs (a zero-filled block)
        ln(lines[2])
        ln(line(torn).toByteArray().let { it.copyOf(it.indexOf(0xE6.toByte()) + 2) }) // cut inside 深
        ln(lines[3])
        ln("\u0000{\"LogSet\":{}}".toByteArray())
        ln(lines[4])
        ln(ByteArray(300) { (it * 37).toByte() })                                    // binary junk
        for (l in lines.drop(5)) ln(l)
        out.write("\r\n".toByteArray())
        logFile.writeBytes(out.toByteArray())

        val loaded = EventLog.load(logFile)
        val baseLines = base.map(::line)
        assertEquals("every intact committed line once, in order", EventLog.compact(baseLines), loaded.lines.filter { it in baseLines })
        for (l in loaded.lines) assertTrue("a survivor is a committed line or parseable junk", l in baseLines || EventLog.variantOf(l) == null)
        assertAllLinesParse()
        assertEquals("boot replays every intact event", baseView, reboot().view)
        assertTrue(Core.send(next).persisted)
        assertEquals(1, entryIdCount(next.entryId))
    }

    /**
     * Lines a newer app version wrote: an unknown struct variant, an unknown unit
     * variant, a known variant with an unknown field, a known variant with an unknown
     * enum value. Boot must not crash, must keep every such line byte-for-byte through
     * a forced rewrite (so an upgrade gets them back), and must still apply the known
     * variant whose only novelty is an extra field.
     */
    @Test
    fun linesFromANewerVersionSurviveARewriteAndDoNotBlockBoot() {
        val base = committed()
        val extraSet = Event.LogSet("Row", 60.0, 8, 7.0, observedAt = at(1, 600), entryId = at(1, 600) * 1000)
        val expected = viewOf(base + extraSet)
        val extraSetFuture = line(extraSet).replaceFirst("{\"exercise\"", "{\"tempo\":\"3-1-1\",\"exercise\"")
        val future = listOf(
            """{"LogMeal":{"kcal":540,"observed_at":${at(1, 900)}}}""",
            "\"ClearMeals\"",
            extraSetFuture,
            line(Event.LogRun(6.0, 33.0, 0.0, observedAt = at(1, 1200), entryId = at(1, 1200) * 1000))
                .replaceFirst("\"observed_at\"", "\"workout_type\":\"Fartlek\",\"observed_at\""),
        )
        // A torn tail forces load() to rewrite the file.
        logFile.writeText(base.joinToString("") { line(it) + "\n" } + future.joinToString("") { "$it\n" } + "{\"LogSe")
        val booted = reboot()
        for (f in future) assertEquals("kept verbatim: ${f.take(40)}", 1, countLines { it == f })
        assertEquals("the known variant with an extra field is applied", expected.lifts, booted.view.lifts)
        assertEquals(expected.profile, booted.view.profile)
        val r = Core.send(next)
        assertTrue(r.accepted && r.persisted)
    }

    // ── 4. Disk full / read-only storage ────────────────────────────────────────

    /**
     * Disk full mid-append ([Core.apply]'s `appendText` throws after writing part of
     * the line): the event is reported not persisted, the following append must not
     * glue onto the partial bytes, and the next boot has every persisted event once
     * and the failed one not at all.
     */
    @Test
    fun aDiskFullAppendIsReportedAndNeverCorruptsTheEventsAfterIt() {
        val devFull = File("/dev/full")
        assumeTrue("needs /dev/full", devFull.exists() && devFull.canWrite())
        val base = committed()
        val expected = viewOf(base + next)
        writeLog(base)
        reboot()
        val live = logFile
        setField("log", devFull)
        val r = Core.send(torn)
        assertTrue("the core accepted it", r.accepted)
        assertFalse("but it is reported NOT persisted", r.persisted)
        // What the short write left behind on the real disk before ENOSPC.
        live.appendBytes(line(torn).toByteArray().copyOf(41))
        setField("log", live)
        val r2 = Core.send(next)
        assertTrue(r2.accepted && r2.persisted)
        assertEquals(1, entryIdCount(next.entryId))
        assertEquals("replay = what was reported saved", expected, reboot().view)
        assertAllLinesParse()
        assertEquals(1, entryIdCount(next.entryId))
        assertEquals("the unsaved event stays unsaved", 0, entryIdCount(torn.entryId))
    }

    /** Read-only storage: boot still restores, a torn tail it can't heal arms the
     *  newline guard, an append is reported not persisted (never a crash), and the
     *  run sidecar and settings degrade silently. */
    @Test
    fun readOnlyStorageBootsAndReportsEveryUnsavedEntry() {
        assumeTrue("root ignores permissions", System.getProperty("user.name") != "root")
        val base = committed()
        val baseView = viewOf(base)
        logFile.writeBytes(base.joinToString("") { line(it) + "\n" }.toByteArray() + "{\"Log".toByteArray())
        val before = logFile.readBytes()
        logFile.setWritable(false, false)
        ctx.filesDir.setWritable(false, false)
        val booted = reboot()
        assertEquals(baseView, booted.view)
        val r = Core.send(next)
        assertTrue(r.accepted)
        assertFalse("an unwritable log must report the entry as not saved", r.persisted)
        assertTrue("nothing on disk changed", before.contentEquals(logFile.readBytes()))
        AppSettings.load(ctx)
        ActiveRunStore.init(ctx)
        ActiveRunStore.begin(ctx)
        ActiveRunStore.append(GpsPoint(52.5, 13.4, at(0), 5.0))
        ActiveRunStore.flush()
        assertEquals(0, ActiveRunStore.recover(ctx).size)
        ctx.filesDir.setWritable(true, true)
        logFile.setWritable(true, true)
        assertEquals("after storage returns, boot heals and replays", baseView, reboot().view)
        assertTrue(Core.send(next).persisted)
        assertEquals(1, entryIdCount(next.entryId))
    }

    // ── 5. Concurrency ──────────────────────────────────────────────────────────

    /** Two threads appending at once: every event lands exactly once as a whole
     *  line, and the replay equals the live model. */
    @Test
    fun concurrentAppendsFromTwoThreadsLoseAndDuplicateNothing() {
        writeLog(listOf(QaHarness.strengthProfile()))
        reboot()
        val perThread = 120
        val barrier = CyclicBarrier(2)
        val failure = AtomicReference<Throwable?>(null)
        val threads = (0 until 2).map { t ->
            thread(name = "append-$t") {
                runCatching {
                    barrier.await()
                    repeat(perThread) { i ->
                        val obs = at(5, (t * 10_000 + i * 60).toLong())
                        val r = Core.send(Event.LogSet("Lift $t", 50.0 + i, 5, 8.0, observedAt = obs, entryId = obs * 1000 + t))
                        check(r.accepted && r.persisted) { "thread $t event $i not saved" }
                    }
                }.onFailure { failure.compareAndSet(null, it) }
            }
        }
        threads.forEach { it.join(60_000) }
        failure.get()?.let { throw it }
        val live = Core.currentView()
        assertAllLinesParse()
        assertEquals(2 * perThread, countLines { it.startsWith("{\"LogSet\"") })
        for (t in 0 until 2) for (i in 0 until perThread) {
            val obs = at(5, (t * 10_000 + i * 60).toLong())
            assertEquals("t$t#$i once", 1, entryIdCount(obs * 1000 + t))
        }
        val replayed = reboot().view
        assertEquals(live.lifts, replayed.lifts)
        assertEquals(live.input_count, replayed.input_count)
    }

    /** A reader ([EventLog.read]: export, panic-reset replay) racing a writer only
     *  ever sees whole committed events, in order, never fewer than before. */
    @Test
    fun aReadRacingAppendsSeesOnlyWholeEventsAndNeverGoesBackwards() {
        writeLog(listOf(QaHarness.strengthProfile()))
        reboot()
        val done = CountDownLatch(1)
        val failure = AtomicReference<Throwable?>(null)
        val reader = thread(name = "reader") {
            runCatching {
                var last = 0
                while (done.count > 0) {
                    val lines = EventLog.read(logFile).lines
                    for (l in lines) check(EventLog.variantOf(l) != null) { "partial line read: ${l.take(60)}" }
                    val sets = lines.count { it.startsWith("{\"LogSet\"") }
                    check(sets >= last) { "read went backwards: $sets < $last" }
                    last = sets
                }
            }.onFailure { failure.compareAndSet(null, it) }
        }
        repeat(300) { i ->
            val obs = at(4, i * 30L)
            assertTrue(Core.send(Event.LogSet("Squat", 60.0, 5, 8.0, observedAt = obs, entryId = obs * 1000)).persisted)
        }
        done.countDown()
        reader.join(30_000)
        failure.get()?.let { throw it }
    }

    /** An event sent while boot's restore compacts + rewrites the log (restore and
     *  send share the Core monitor) is appended AFTER the rewrite, never into the
     *  file the rename replaces. */
    @Test
    fun anAppendRacingBootsCompactionRewriteIsNotLost() {
        val original = compactableLog()
        logFile.writeText(original.joinToString("") { "$it\n" } + "{\"Lo")
        ProcessDeath.kill(ctx)
        val restoredFlag = Core.javaClass.getDeclaredField("restored").apply { isAccessible = true }
        val booter = thread(name = "boot") { Core.restore(ctx) }
        // Send the moment restore has begun (it marks `restored` before reading the
        // log): the send must wait for the rewrite, then append to the new file.
        val deadline = System.currentTimeMillis() + 10_000
        while (restoredFlag.get(Core) != true) {
            check(System.currentTimeMillis() < deadline) { "restore never started" }
            Thread.onSpinWait()
        }
        val r = Core.send(next)
        booter.join(30_000)
        assertTrue(r.persisted)
        assertEquals(1, entryIdCount(next.entryId))
        assertAllLinesParse()
        reboot()
        assertEquals(1, entryIdCount(next.entryId))
    }

    // ── 6. Prefs vs log ─────────────────────────────────────────────────────────

    /** The setup prefs wiped (or corrupted to empty) while the log is intact: the app
     *  must boot to the main screen with the user's data, not re-run first-run setup. */
    @Test
    fun wipedSetupPrefsWithAnIntactLogBootToTheMainScreenWithTheData() {
        val base = committed()
        writeLog(base)
        ctx.setupPrefs().edit().clear().commit()
        ctx.getSharedPreferences("theme", Context.MODE_PRIVATE).edit().clear().commit()
        rule.launchApp()
        val vm = EventDispatcher.model.value
        assertNotNull(vm)
        assertNotNull("profile restored", vm!!.profile)
        assertEquals("guided setup must not reopen", 0,
            rule.onAllNodes(androidx.compose.ui.test.hasText(str(R.string.setup_step_train_q))).fetchSemanticsNodes().size)
    }

    /**
     * The user removed their plan (a committed ClearPlan in the log), then the setup
     * prefs were lost (cleared / corrupted to empty, or the async `apply()` of
     * KEY_PLAN_CLEARED not yet on disk when the process died) while the log is intact.
     * The removal is part of the committed log, so the next boot must keep the plan
     * removed: compaction keeps a final ClearPlan ([RestoredLog.planRemoved]) and the
     * boot re-arms the marker from it before the auto-GeneratePlan gate.
     */
    @Test
    fun aRemovedPlanStaysRemovedWhenOnlyTheLogSurvives() {
        writeLog(listOf(QaHarness.strengthProfile(), Event.GeneratePlan(todayEpochDay() - 7), Event.ClearPlan))
        ctx.setupPrefs().edit().clear().commit()
        rule.launchApp()
        assertNull("the removed plan came back", EventDispatcher.model.value!!.program)
    }

    /** The log gone while the prefs still say onboarding is done (vice versa): the
     *  app boots to a usable, empty main screen instead of crashing or hanging. */
    @Test
    fun aMissingLogWithStalePrefsStillBootsUsable() {
        ctx.setupPrefs().edit().putBoolean("onboarding_done", true).putBoolean(KEY_PLAN_CLEARED, true).commit()
        rule.launchApp()
        val vm = EventDispatcher.model.value
        assertNotNull(vm)
        assertNull(vm!!.profile)
    }

    // ── 7. Backup import interrupted ────────────────────────────────────────────

    /** A backup cut at every byte (an interrupted copy/transfer): validation either
     *  refuses it or returns only whole, known events, never a partial line. */
    @Test
    fun aBackupCutAtEveryByteNeverValidatesToAPartialLine() {
        val lines = committed().map(::line)
        val bytes = lines.joinToString("") { "$it\n" }.toByteArray()
        for (k in 0..bytes.size) {
            when (val c = streamValidateImport(ByteArrayInputStream(bytes.copyOf(k)), MAX_IMPORT_BYTES)) {
                is ImportCheck.Valid -> {
                    assertEquals("cut at $k", lines.subList(0, c.lines.size), c.lines)
                }
                ImportCheck.Invalid -> {}
            }
        }
    }

    /** An IO error half-way through reading a backup surfaces as a failure, and the
     *  live log is untouched. */
    @Test
    fun anIoErrorHalfWayThroughAnImportLeavesTheLiveLogUntouched() {
        writeLog(committed())
        val before = logFile.readBytes()
        val backup = committed().joinToString("") { line(it) + "\n" }.toByteArray()
        val failing = object : InputStream() {
            var served = 0
            override fun read(): Int {
                if (served >= backup.size / 2) throw IOException("transfer interrupted")
                return backup[served++].toInt() and 0xFF
            }
        }
        val outcome = runCatching { streamValidateImport(failing, MAX_IMPORT_BYTES) }
        assertTrue("an interrupted read is a failure, not a half backup", outcome.isFailure || outcome.getOrNull() == ImportCheck.Invalid)
        assertTrue(before.contentEquals(logFile.readBytes()))
    }

    /**
     * The app's own export must be importable. Every GPS run is one LogRunTrack line
     * of up to [TRACK_DECIMATION_CAP] fixes (~90 bytes each), so one year of a hybrid
     * athlete's history recorded at the tracker's screen-OFF cadence (one fix / 4 s,
     * RunTrackingService FUSED_BG_INTERVAL_MS; screen-on is 2 s) is ~11.7 MiB; the
     * import cap ([MAX_IMPORT_BYTES]) once refused it at 8 MiB.
     */
    @Test
    fun aOneYearBackupWithGpsRunsIsImportable() {
        val history = LongHistorySeed.build(days = 365, fixEverySec = 4L)
        val exported = EventLog.compact(history.events.map(::line))
        val bytes = exported.joinToString("") { "$it\n" }.toByteArray()
        val tracks = exported.count { it.startsWith("{\"LogRunTrack\"") }
        val check = streamValidateImport(ByteArrayInputStream(bytes), MAX_IMPORT_BYTES)
        assertTrue(
            "export of 1 year (${bytes.size / 1024} KiB, $tracks GPS runs) must import; cap ${MAX_IMPORT_BYTES / 1024} KiB",
            check is ImportCheck.Valid,
        )
    }

    /** The import's bounds still stop absurd picks: a line past the line cap (a
     *  newline-less binary file) and a stream past the total cap are refused, the
     *  first after reading at most one chunk past the line cap. */
    @Test
    fun anImportPastTheLineOrTotalCapIsRefused() {
        val ok = (line(QaHarness.strengthProfile()) + "\n").toByteArray()
        val content = ok.size - 1L
        assertTrue(streamValidateImport(ByteArrayInputStream(ok), max = ok.size.toLong(), maxLine = content) is ImportCheck.Valid)
        assertEquals(ImportCheck.Invalid, streamValidateImport(ByteArrayInputStream(ok), max = ok.size - 1L))
        assertEquals(ImportCheck.Invalid, streamValidateImport(ByteArrayInputStream(ok), MAX_IMPORT_BYTES, maxLine = content - 1))
        val endless = object : InputStream() {
            var served = 0L
            override fun read(): Int = 'x'.code.also { served++ }
            override fun read(b: ByteArray, off: Int, len: Int): Int {
                b.fill('x'.code.toByte(), off, off + len)
                served += len
                return len
            }
        }
        assertEquals(ImportCheck.Invalid, streamValidateImport(endless, MAX_IMPORT_BYTES))
        assertTrue("read ${endless.served} bytes", endless.served <= MAX_IMPORT_LINE_BYTES + 64 * 1024)
    }

    /** A data import that cannot write its temporary (disk full / read-only) fails
     *  as a whole: the live log is byte-identical, the core's log is re-attached so
     *  the next entry is still persisted, and nothing half-written replaces it. */
    @Test
    fun anImportThatCannotWriteFailsCleanlyAndTheLiveLogKeepsRecording() {
        assumeTrue("root ignores permissions", System.getProperty("user.name") != "root")
        val base = committed()
        writeLog(base)
        reboot()
        val before = logFile.readBytes()
        ctx.filesDir.setWritable(false, false)
        val res = try {
            onAppScope { replaceEventLog(ctx, listOf(line(QaHarness.strengthProfile()))) }
        } finally {
            ctx.filesDir.setWritable(true, true)
        }
        assertEquals(DataImportResult.Failed, res)
        assertTrue("live log untouched", before.contentEquals(logFile.readBytes()))
        val r = Core.send(next)
        assertTrue("the log was re-attached", r.accepted && r.persisted)
        assertEquals(1, entryIdCount(next.entryId))
        assertEquals(viewOf(base + next), run {
            logFile.writeBytes(before + (line(next) + "\n").toByteArray())
            reboot().view
        })
    }

    /** A data import that runs out of heap mid-write (a backup too large for the
     *  device; [replaceEventLog] once let the OutOfMemoryError escape its
     *  `catch (e: Exception)` and kill the app) fails as a whole: the import reports
     *  [DataImportResult.Failed], the live log is byte-identical, no `.import`
     *  temporary is left, and the core's log is re-attached. */
    @Test
    fun anImportThatRunsOutOfMemoryFailsCleanlyAndTheLiveLogKeepsRecording() {
        val base = committed()
        writeLog(base)
        reboot()
        val before = logFile.readBytes()
        val profile = line(QaHarness.strengthProfile())
        // The OOM strikes after two lines reached the temporary.
        val lines = object : AbstractList<String>() {
            override val size = 3
            override fun get(index: Int): String =
                if (index < 2) profile else throw OutOfMemoryError("injected: the backup outgrew the heap")
        }
        assertEquals(DataImportResult.Failed, onAppScope { replaceEventLog(ctx, lines) })
        assertTrue("live log untouched", before.contentEquals(logFile.readBytes()))
        assertFalse("no .import temporary left", File(ctx.filesDir, "event-log.ndjson.import").exists())
        val r = Core.send(next)
        assertTrue("the log was re-attached", r.accepted && r.persisted)
        assertEquals(1, entryIdCount(next.entryId))
    }

    /** An Activity recreation between the import's log swap and its restart re-runs
     *  boot's [Core.restore]: it must not re-attach the detached log, or a queued
     *  event would append to the imported file. */
    @Test
    fun aRestoreAfterTheImportDetachedTheLogDoesNotReattachIt() {
        writeLog(committed())
        reboot()
        val before = logFile.readBytes()
        Core.detachLog()
        Core.restore(ctx)
        assertTrue(Core.send(next).accepted)
        assertTrue("nothing appended to the detached log", before.contentEquals(logFile.readBytes()))
    }

    /** A log the app cannot read at all boots to a usable (empty) model flagged
     *  stale, so the user is told, never a crash, and its bytes are never touched. */
    @Test
    fun anUnreadableLogBootsUsableWithANoticeAndIsNeverOverwritten() {
        assumeTrue("root ignores permissions", System.getProperty("user.name") != "root")
        writeLog(committed())
        val before = logFile.readBytes()
        logFile.setReadable(false, false)
        logFile.setWritable(false, false)
        val booted = try {
            reboot()
        } finally {
            logFile.setReadable(true, true)
            logFile.setWritable(true, true)
        }
        assertTrue("the boot result tells the user the data could not be read", booted.stale)
        assertTrue("bytes untouched", before.contentEquals(logFile.readBytes()))
        assertEquals(viewOf(committed()), reboot().view)
    }

    // ── 8. Very large log (5 years) ─────────────────────────────────────────────

    /** Five years of a hybrid athlete's history as the app writes it, one GPS fix
     *  every [fixEverySec]. */
    private fun fiveYearLog(fixEverySec: Long = 10L): Pair<File, Int> {
        val history = LongHistorySeed.build(days = 5 * 365, fixEverySec = fixEverySec)
        val big = File(ctx.cacheDir, "five-year.ndjson")
        big.bufferedWriter().use { w -> history.events.forEach { w.write(line(it)); w.write("\n") } }
        return big to history.events.size
    }

    /** Boot's [EventLog.load] of a 5-year log finishes in bounded time (host JVM). */
    @Test
    fun aFiveYearLogLoadsInBoundedTime() {
        val big = fiveYearLog().first
        val t0 = System.nanoTime()
        EventLog.load(big)
        val ms = (System.nanoTime() - t0) / 1_000_000
        big.delete()
        assertTrue("5-year load took $ms ms", ms < 20_000)
    }

    /**
     * Boot's [EventLog.load] must fit in the per-app heap of an ordinary phone.
     * Android CDD §3.7 [C-0-2] guarantees a normal-screen xxhdpi (480 dpi) phone only
     * 128 MB of application memory for the WHOLE app. The load runs in a child JVM
     * capped at that; an OOM there is a boot crash loop on such a device, and the log
     * only grows, so the user cannot even reach Export. Always at the tracker's
     * screen-off cadence (one fix / 4 s, ~49 MiB of log).
     */
    @Test
    fun aFiveYearLogLoadsInTheMinimumPhoneHeap() {
        val (big, n) = fiveYearLog(fixEverySec = 4L)
        val mb = big.length() / (1024 * 1024)
        val probe = runProbe(LoadProbe::class.java, heapMb = 128, big.path)
        val needed = if (probe.first == 0) 128 else listOf(192, 256, 384, 512, 768, 1024).firstOrNull { runProbe(LoadProbe::class.java, it, big.path).first == 0 }
        big.delete()
        assertEquals(
            "5-year log ($mb MiB, $n lines) must load in a 128 MB heap; it needs ${needed ?: ">1024"} MB:\n${probe.second}",
            0, probe.first,
        )
    }

    /**
     * The app's own 5-year backup (its compacted log, ~49 MiB at the tracker's
     * screen-off cadence) must import in the same 128 MB heap the boot fits in: the
     * import holds the validated lines as boot holds the log, and [swapLogFile]
     * streams them to the temporary instead of joining them into one string (which
     * OOM-killed the app at 128 and 192 MB on the emulator). Runs the shell's import
     * pipeline off the core ([ImportProbe]) in a child JVM capped at 128 MB.
     */
    @Test
    fun aFiveYearBackupImportsInTheMinimumPhoneHeap() {
        val history = LongHistorySeed.build(days = 5 * 365, fixEverySec = 4L)
        val backup = File(ctx.cacheDir, "five-year-backup.ndjson")
        backup.bufferedWriter().use { w -> EventLog.compact(history.events.map(::line)).forEach { w.write(it); w.write("\n") } }
        val mb = backup.length() / (1024 * 1024)
        val dir = File(ctx.cacheDir, "import-target").apply { deleteRecursively(); mkdirs() }
        File(dir, "event-log.ndjson").writeText(line(QaHarness.strengthProfile()) + "\n")
        val probe = runProbe(ImportProbe::class.java, heapMb = 128, backup.path, dir.path)
        val needed = if (probe.first == 0) {
            128
        } else {
            listOf(192, 256, 384, 512, 768, 1024).firstOrNull { runProbe(ImportProbe::class.java, it, backup.path, dir.path).first == 0 }
        }
        try {
            assertEquals(
                "5-year backup ($mb MiB) must import in a 128 MB heap; it needs ${needed ?: ">1024"} MB:\n${probe.second}",
                0, probe.first,
            )
            assertTrue("the log is the backup, byte for byte", backup.readBytes().contentEquals(File(dir, "event-log.ndjson").readBytes()))
            assertFalse("no .import temporary left", File(dir, "event-log.ndjson.import").exists())
        } finally {
            backup.delete()
            dir.deleteRecursively()
        }
    }

    /** Runs [main]'s `main` in a child JVM with an `-Xmx[heapMb]m` cap and this JVM's
     *  library path, so the child loads the same host core. */
    private fun runProbe(main: Class<*>, heapMb: Int, vararg args: String): Pair<Int, String> {
        val java = File(System.getProperty("java.home"), "bin/java").path
        val cp = System.getProperty("java.class.path")
        val libs = "-Djava.library.path=" + System.getProperty("java.library.path")
        val p = ProcessBuilder(java, "-Xmx${heapMb}m", libs, "-cp", cp, main.name, *args)
            .redirectErrorStream(true).start()
        val out = p.inputStream.bufferedReader().readText()
        assertTrue("probe finished", p.waitFor(120, TimeUnit.SECONDS))
        return p.exitValue() to out.takeLast(2000)
    }

    // ── 9. Run sidecar torn writes ──────────────────────────────────────────────

    private fun fix(i: Int) = GpsPoint(52.52 + i * 0.0001, 13.405 + i * 0.00005, at(0, -1800L + i * 2), 4.5)

    private fun fixLine(p: GpsPoint, seg: Boolean = false) =
        "{\"lat\":${p.lat},\"lon\":${p.lon},\"observed_at\":${p.observedAt},\"accuracy_m\":${p.accuracyM}" +
            (if (seg) ",\"seg\":true" else "") + "}"

    /** A sidecar torn at every byte of its last fix recovers exactly the whole fixes
     *  (and their segment marks); a torn header is a run with no fixes. */
    @Test
    fun aSidecarTornAtEveryByteRecoversExactlyTheWholeFixes() {
        ActiveRunStore.init(ctx)
        val fixes = (0 until 6).map(::fix)
        val head = "{\"run\":\"active\"}\n" + fixes.mapIndexed { i, p -> fixLine(p, seg = i == 3) + "\n" }.joinToString("")
        val last = fixLine(fix(6)).toByteArray()
        for (k in 0..last.size) {
            sidecar.writeBytes(head.toByteArray() + last.copyOf(k))
            val got = ActiveRunStore.recover(ctx)
            val want = if (k == last.size) fixes + fix(6) else fixes
            assertEquals("cut at $k", want, got.toList())
            assertEquals("cut at $k: segment mark kept", listOf(3), got.segmentStarts)
        }
        for (k in 0..17) {
            sidecar.writeBytes("{\"run\":\"active\"}\n".toByteArray().copyOf(k))
            assertTrue(ActiveRunStore.hasActiveRun(ctx))
            assertEquals(0, ActiveRunStore.recover(ctx).size)
        }
    }

    /**
     * A resumed run keeps appending to the SAME sidecar (Resume restarts the service
     * with ACTION_START, which never calls [ActiveRunStore.begin]). If the crash left
     * the last fix torn, the first post-resume fix must not glue onto the partial
     * bytes: [ActiveRunStore.append] cuts the torn tail first, so a second crash keeps
     * that fix and its `"seg":true` resume-splice mark.
     */
    @Test
    fun aFixAppendedAfterResumingATornSidecarSurvivesTheNextCrash() {
        ActiveRunStore.init(ctx)
        val fixes = (0 until 5).map(::fix)
        sidecar.writeText("{\"run\":\"active\"}\n" + fixes.joinToString("") { fixLine(it) + "\n" } + fixLine(fix(5)).take(30))
        val recovered = ActiveRunStore.recover(ctx)
        assertEquals(fixes, recovered.toList())
        RunSession.restore(recovered)
        val resumed = fix(40)
        assertTrue(RunSession.add(resumed, (android.os.SystemClock.elapsedRealtime() + 2_000) * 1_000_000))
        ActiveRunStore.append(resumed)
        ActiveRunStore.flush()
        val again = ActiveRunStore.recover(ctx)
        assertEquals("the post-resume fix survives a second crash", fixes + resumed, again.toList())
        assertEquals("…and still opens a segment", listOf(5), again.segmentStarts)
    }

    /** Garbage and invalid UTF-8 lines inside the sidecar cost only themselves. */
    @Test
    fun sidecarGarbageLinesCostOnlyThemselves() {
        ActiveRunStore.init(ctx)
        val out = ByteArrayOutputStream()
        out.write("{\"run\":\"active\"}\n".toByteArray())
        out.write((fixLine(fix(0)) + "\n").toByteArray())
        out.write(byteArrayOf(0xC3.toByte(), 0x28, 0xFF.toByte(), '\n'.code.toByte()))
        out.write((fixLine(fix(1)) + "\n").toByteArray())
        out.write(ByteArray(64) + "\n".toByteArray())
        out.write("{\"lat\":\"x\"}\n".toByteArray())
        out.write((fixLine(fix(2)) + "\n").toByteArray())
        sidecar.writeBytes(out.toByteArray())
        assertEquals(listOf(fix(0), fix(1), fix(2)), ActiveRunStore.recover(ctx).toList())
    }
}

/** Child-JVM probe for [PersistenceTortureTest.aFiveYearLogLoadsInTheMinimumPhoneHeap]:
 *  boot's [EventLog.load] (the core's compaction included) of the file in `args[0]`;
 *  exit 0 + `ms=` on success, 3 on OOM, 4 when the core library did not load. */
object LoadProbe {
    @JvmStatic
    fun main(args: Array<String>) {
        if (Core.compaction(emptyList()) == null) {
            println("core library not loaded (java.library.path=${System.getProperty("java.library.path")})")
            System.exit(4)
        }
        val f = File(args[0])
        val t0 = System.nanoTime()
        try {
            val r = EventLog.load(f)
            println("lines=${r.lines.size} ms=${(System.nanoTime() - t0) / 1_000_000}")
        } catch (e: OutOfMemoryError) {
            println("OOM after ${(System.nanoTime() - t0) / 1_000_000} ms: $e")
            System.exit(3)
        }
    }
}

/** Child-JVM probe for [PersistenceTortureTest.aFiveYearBackupImportsInTheMinimumPhoneHeap]:
 *  the data import of the backup in `args[0]` into the directory `args[1]` as the
 *  shell runs it around the core's detach: [streamValidateImport] (pick), [summarizeImport]
 *  (the confirmation) and [swapLogFile] (the replace), with the validated lines held
 *  throughout as the confirmation dialog holds them. Exit 0 on success, 3 on OOM, 4
 *  when the backup does not validate. */
object ImportProbe {
    @JvmStatic
    fun main(args: Array<String>) {
        val t0 = System.nanoTime()
        try {
            val check = File(args[0]).inputStream().use { streamValidateImport(it, MAX_IMPORT_BYTES) }
            val lines = (check as? ImportCheck.Valid)?.lines ?: run {
                println("the backup does not validate")
                exitProcess(4)
            }
            val summary = summarizeImport(lines, java.time.ZoneOffset.UTC)
            swapLogFile(File(args[1]), lines)
            println("lines=${lines.size} entries=${summary.entries} ms=${(System.nanoTime() - t0) / 1_000_000}")
        } catch (e: OutOfMemoryError) {
            println("OOM after ${(System.nanoTime() - t0) / 1_000_000} ms: $e\n${e.stackTrace.take(8).joinToString("\n") { "  at $it" }}")
            exitProcess(3)
        }
    }
}
