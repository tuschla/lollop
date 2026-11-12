package app.milestone

import android.content.Context
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.test.core.app.ApplicationProvider
import app.milestone.qa.LongHistorySeed
import app.milestone.qa.QaHarness
import app.milestone.qa.QaHarness.daysAgoAtMidday as at
import app.milestone.qa.QaHarness.onAppScope
import app.milestone.qa.launchApp
import app.milestone.qa.str
import app.milestone.core.CoreBridge
import app.milestone.core.JniCore
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.IOException
import java.io.InputStream
import java.util.concurrent.CountDownLatch
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
 * log (`DurableCore.apply`'s append, [EventLog.load]'s compaction rewrite), the data
 * backup swap ([swapLogFile] / [streamValidateImport]), the setup/theme prefs and the
 * run sidecar ([ActiveRunStore]).
 *
 * Invariants checked throughout: a committed event is never lost and never
 * duplicated, a crash at any byte leaves a log whose replay equals the pre-crash
 * committed state, the compaction rewrite is atomic, and the app always boots.
 *
 * The event log tests drive the REAL core over JNI (host `libshared.so`); a "crash"
 * is the on-disk bytes a killed process leaves behind plus [QaHarness.killProcess]
 * (every process-scoped singleton dropped, disk kept), and a "boot" is
 * [Core.restore] + the boot `SetToday`, exactly as the boot effect runs them.
 */
@RunWith(RobolectricTestRunner::class)
class PersistenceTortureTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()
    private val logFile get() = QaHarness.logFile(ctx)
    private val sidecar get() = File(ctx.filesDir, AppFileNames.ACTIVE_RUN)
    private val json = Json { ignoreUnknownKeys = true }

    @Before
    fun setUp() {
        QaHarness.reset(ctx)
        File(ctx.filesDir, AppFileNames.EVENT_LOG + AppFileNames.IMPORT_SUFFIX).delete()
        ActiveRunStore.start(ctx)
        sidecar.delete()
    }

    @After
    fun tearDown() {
        ctx.filesDir.setWritable(true, true)
        logFile.setWritable(true, true)
        File(ctx.filesDir, AppFileNames.EVENT_LOG + AppFileNames.TMP_SUFFIX).let { if (it.isDirectory) it.deleteRecursively() }
        ActiveRunStore.flush()
        sidecar.delete()
        QaHarness.teardown()
        RunSession.reset()
    }

    private fun line(e: Event) = e.toJson().toString()

    private fun track(startSec: Long, entryId: Long): Event.LogRunTrack {
        val pts = (0 until 40).map { i ->
            GpsPoint(52.52 + i * 0.00012, 13.405 + (i % 7) * 0.00003, startSec + i * 5L, 5.0)
        }
        return Event.LogRunTrack(pts, 0.0, observedAt = pts.last().observedAt, entryId = entryId)
    }

    private fun committed(): List<Event> = listOf(
        QaHarness.strengthProfile(),
        Event.GeneratePlan(todayEpochDay() - 7),
        Event.LogSet("Kniebeuge – Über", 100.0, 5, 8.0, observedAt = at(3), entryId = at(3) * 1000),
        Event.LogSet("Bench Press", 80.0, 5, 8.0, observedAt = at(3, 300), entryId = at(3, 300) * 1000),
        Event.LogRun(5.0, 27.5, 0.0, observedAt = at(2), entryId = at(2) * 1000),
        Event.SubmitCheckin(observedAt = at(1), sleepQuality = 4, soreness = 2, mood = 4, fatigue = 2, stress = 2),
        Event.DeleteEntry(EntryKind.Set, at(3, 300) * 1000),
        track(at(1, 3600), at(1, 3600) * 1000),
    )

    /** The event a crash tears: multibyte name, so the cut lands inside UTF-8 too. */
    private val torn = Event.LogSet("深蹲 🏋 Squat", 102.5, 3, 9.0, observedAt = at(0, -7200), entryId = at(0, -7200) * 1000)

    private val next = Event.LogSet("Deadlift", 140.0, 3, 8.5, observedAt = at(0, -3600), entryId = at(0, -3600) * 1000)

    private fun writeLog(events: List<Event>) = QaHarness.seedLog(ctx, events)

    private fun viewOf(events: List<Event>): ViewModel {
        writeLog(events)
        return QaHarness.reboot(ctx).view
    }

    private fun assertAllLinesParse(f: File = logFile) {
        f.readLines().filter { it.isNotBlank() }.forEachIndexed { i, l ->
            assertNotNull("line $i must parse: ${l.take(80)}", runCatching { json.parseToJsonElement(l) }.getOrNull())
        }
    }

    private fun countLines(predicate: (String) -> Boolean) =
        logFile.readLines().count { it.isNotBlank() && predicate(it) }

    private fun entryIdCount(id: Long) = countLines { "\"entry_id\":$id" in it }

    /**
     * A kill mid-append leaves the committed log plus a prefix of the new line.
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
            val booted = QaHarness.reboot(ctx)
            assertEquals("cut at byte $k/${wire.size}: replay = committed state", if (complete) withTorn else baseView, booted.view)
            val r = Core.send(next)
            assertTrue("cut at $k: the next append is persisted", r.accepted && r.persisted)
            assertAllLinesParse()
            assertEquals("cut at $k: the next append is on disk once", 1, entryIdCount(next.entryId))
            assertEquals("cut at $k: a committed set is never duplicated", 1, entryIdCount(at(3) * 1000))
            val after = QaHarness.reboot(ctx).view
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
        val committedOnly = EventLog.compact(base, Core)
        val withTorn = EventLog.compact(base + line(torn), Core)
        for (k in 0..wire.size) {
            val bytes = prefix + wire.copyOf(k)
            logFile.writeBytes(bytes)
            val read = EventLog.read(logFile, Core)
            assertEquals("cut at $k", if (k == wire.size) withTorn else committedOnly, read.lines)
            assertTrue("cut at $k: read() must not rewrite", bytes.contentEquals(logFile.readBytes()))
            val loaded = EventLog.load(logFile, Core)
            assertEquals("cut at $k: load == read", read.lines, loaded.lines)
            assertTrue("cut at $k: the healed log ends in a newline", AppFiles.endsWithNewline(logFile))
            assertEquals("cut at $k: a second load is a fixed point", loaded.lines, EventLog.load(logFile, Core).lines)
        }
    }

    /** A log big enough in superseded lines that [EventLog.load] rewrites it: every
     *  zone request but the last is superseded. */
    private fun compactableLog(): List<String> {
        val base = committed().map(::line)
        val zones = (0..EventLog.REWRITE_MIN_DROPPED_LINES).map { line(Event.ComputeHrZones(30.0 + it)) }
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
        val compacted = EventLog.compact(original, Core)
        assertTrue("fixture: the load must rewrite", EventLog.compactionWorthRewriting(original.size - compacted.size, original.size))
        val compactedBytes = compacted.joinToString("\n", postfix = "\n").toByteArray()
        val tmp = File(ctx.filesDir, AppFileNames.EVENT_LOG + AppFileNames.TMP_SUFFIX)
        val imp = File(ctx.filesDir, AppFileNames.EVENT_LOG + AppFileNames.IMPORT_SUFFIX)
        val bak = File(ctx.filesDir, AppFileNames.EVENT_LOG + AppFileNames.BAK_SUFFIX)
        val imported = listOf(line(QaHarness.strengthProfile()), line(next))
        val importedBytes = imported.joinToString("\n", postfix = "\n").toByteArray()

        fun check(state: String, expected: List<String>) {
            val got = EventLog.load(logFile, Core)
            assertEquals("$state: lines", expected, got.lines)
            assertFalse("$state: not a fresh install", got.freshInstall)
            for (f in listOf(tmp, imp, bak)) assertFalse("$state: ${f.name} left behind", f.exists())
            assertAllLinesParse()
            assertEquals("$state: load is a fixed point", expected, EventLog.load(logFile, Core).lines)
        }

        val stride = maxOf(1, compactedBytes.size / 400)
        for (k in (0..compactedBytes.size step stride) + compactedBytes.size) {
            logFile.writeBytes(originalBytes)
            tmp.writeBytes(compactedBytes.copyOf(k))
            check("rewrite tmp torn at $k", compacted)
            assertTrue("rewrite tmp torn at $k: original replaced by its compaction", compactedBytes.contentEquals(logFile.readBytes()))
        }
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
        check("swap: new in place, .bak still there", EventLog.compact(imported, Core))
    }

    /** A rewrite that cannot even open its temporary (disk full / a stray directory
     *  in the way) leaves the original log untouched, still returns the committed
     *  state, and the core's first append after it cannot glue onto the torn tail. */
    @Test
    fun aRewriteThatFailsMidCompactionKeepsTheOriginalAndTheNextAppendStillLands() {
        val base = committed()
        val baseNext = viewOf(base + next)
        val baseView = viewOf(base)
        val tmp = File(ctx.filesDir, AppFileNames.EVENT_LOG + AppFileNames.TMP_SUFFIX)
        val tornBytes = base.joinToString("") { line(it) + "\n" }.toByteArray() + line(torn).toByteArray().copyOf(23)
        logFile.writeBytes(tornBytes)
        tmp.mkdirs()
        File(tmp, "blocker").writeText("x")
        val booted = QaHarness.reboot(ctx)
        assertEquals("replay = committed state", baseView, booted.view)
        assertTrue("the failed rewrite left the original bytes", tornBytes.contentEquals(logFile.readBytes()))
        val r = Core.send(next)
        assertTrue(r.accepted && r.persisted)
        tmp.deleteRecursively()
        assertEquals("next append replays after the reboot", baseNext, QaHarness.reboot(ctx).view)
        assertEquals(1, entryIdCount(next.entryId))
    }

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

        val loaded = EventLog.load(logFile, Core)
        val baseLines = base.map(::line)
        assertEquals("every intact committed line once, in order", EventLog.compact(baseLines, Core), loaded.lines.filter { it in baseLines })
        for (l in loaded.lines) assertTrue("a survivor is a committed line or parseable junk", l in baseLines || EventLog.variantOf(l) == null)
        assertAllLinesParse()
        assertEquals("boot replays every intact event", baseView, QaHarness.reboot(ctx).view)
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
        val booted = QaHarness.reboot(ctx)
        for (f in future) assertEquals("kept verbatim: ${f.take(40)}", 1, countLines { it == f })
        assertEquals("the known variant with an extra field is applied", expected.lifts, booted.view.lifts)
        assertEquals(expected.profile, booted.view.profile)
        val r = Core.send(next)
        assertTrue(r.accepted && r.persisted)
    }

    /**
     * Disk full mid-append (`DurableCore.apply`'s [AppFiles.append] throws after writing part of
     * the line): the event is reported not persisted, the following append must not
     * glue onto the partial bytes, and the next boot has every persisted event once
     * and the failed one not at all.
     */
    @Test
    fun aDiskFullAppendIsReportedAndNeverCorruptsTheEventsAfterIt() {
        val base = committed()
        val expected = viewOf(base + next)
        writeLog(base)
        QaHarness.reboot(ctx)
        // The log moved where no append can land (its directory is missing), as a
        // full disk refuses it.
        val live = checkNotNull(Core.detachLog())
        Core.reattachLog(File(ctx.filesDir, "missing/dir/${AppFileNames.EVENT_LOG}"))
        val r = Core.send(torn)
        assertTrue("the core accepted it", r.accepted)
        assertFalse("but it is reported NOT persisted", r.persisted)
        // What the short write left behind on the real disk before ENOSPC.
        live.appendBytes(line(torn).toByteArray().copyOf(41))
        Core.reattachLog(live)
        val r2 = Core.send(next)
        assertTrue(r2.accepted && r2.persisted)
        assertEquals(1, entryIdCount(next.entryId))
        assertEquals("replay = what was reported saved", expected, QaHarness.reboot(ctx).view)
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
        val booted = QaHarness.reboot(ctx)
        assertEquals(baseView, booted.view)
        val r = Core.send(next)
        assertTrue(r.accepted)
        assertFalse("an unwritable log must report the entry as not saved", r.persisted)
        assertTrue("nothing on disk changed", before.contentEquals(logFile.readBytes()))
        AppSettings.load(ctx)
        ActiveRunStore.start(ctx)
        ActiveRunStore.begin()
        ActiveRunStore.append(GpsPoint(52.5, 13.4, at(0), 5.0), segmentStart = false)
        ActiveRunStore.flush()
        assertEquals(0, ActiveRunStore.recover().size)
        ctx.filesDir.setWritable(true, true)
        logFile.setWritable(true, true)
        assertEquals("after storage returns, boot heals and replays", baseView, QaHarness.reboot(ctx).view)
        assertTrue(Core.send(next).persisted)
        assertEquals(1, entryIdCount(next.entryId))
    }

    /** A reader ([EventLog.read]: export, panic-reset replay) racing a writer only
     *  ever sees whole committed events, in order, never fewer than before. */
    @Test
    fun aReadRacingAppendsSeesOnlyWholeEventsAndNeverGoesBackwards() {
        writeLog(listOf(QaHarness.strengthProfile()))
        QaHarness.reboot(ctx)
        val done = CountDownLatch(1)
        val failure = AtomicReference<Throwable?>(null)
        val reader = thread(name = "reader") {
            runCatching {
                var last = 0
                while (done.count > 0) {
                    val lines = EventLog.read(logFile, Core).lines
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
     *  send share the engine's lock) is appended AFTER the rewrite, never into the
     *  file the rename replaces. */
    @Test
    fun anAppendRacingBootsCompactionRewriteIsNotLost() {
        val original = compactableLog()
        logFile.writeText(original.joinToString("") { "$it\n" } + "{\"Lo")
        // The real core; boot's restore asks it for the compaction under the engine's lock.
        val restoring = CountDownLatch(1)
        val engine = DurableCore(
            object : CoreBridge by JniCore {
                override fun compactLog(lines: Array<String>): ByteArray? {
                    restoring.countDown()
                    return JniCore.compactLog(lines)
                }
            },
        )
        QaHarness.freshProcess(engine)
        val booter = thread(name = "boot") { engine.restore(ctx) }
        // Send the moment restore is compacting: the send must wait for the rewrite,
        // then append to the new file.
        assertTrue("restore never started", restoring.await(10, TimeUnit.SECONDS))
        val r = engine.send(next)
        booter.join(30_000)
        assertTrue(r.persisted)
        assertEquals(1, entryIdCount(next.entryId))
        assertAllLinesParse()
        QaHarness.reboot(ctx)
        assertEquals(1, entryIdCount(next.entryId))
    }

    /** The setup prefs wiped (or corrupted to empty) while the log is intact: the app
     *  must boot to the main screen with the user's data, not re-run first-run setup. */
    @Test
    fun wipedSetupPrefsWithAnIntactLogBootToTheMainScreenWithTheData() {
        val base = committed()
        writeLog(base)
        ctx.setupPrefs().edit().clear().commit()
        ctx.getSharedPreferences(AppFileNames.SETTINGS_STORE, Context.MODE_PRIVATE).edit().clear().commit()
        rule.launchApp()
        val vm = EventDispatcher.model.value
        assertNotNull(vm)
        assertNotNull("profile restored", vm!!.profile)
        assertEquals("guided setup must not reopen", 0,
            rule.onAllNodes(androidx.compose.ui.test.hasText(str(R.string.setup_step_train_q))).fetchSemanticsNodes().size)
    }

    /**
     * The user removed their plan (a committed ClearPlan in the log), then the setup
     * prefs were lost (cleared / corrupted to empty) while the log is intact. The
     * removal is part of the committed log, which compaction keeps, so the next boot
     * reads it from the core's view (`plan_removed`) and generates no plan.
     */
    @Test
    fun aRemovedPlanStaysRemovedWhenOnlyTheLogSurvives() {
        writeLog(listOf(QaHarness.strengthProfile(), Event.GeneratePlan(todayEpochDay() - 7), Event.ClearPlan))
        ctx.setupPrefs().edit().clear().commit()
        rule.launchApp()
        val vm = EventDispatcher.model.value!!
        assertNull("the removed plan came back", vm.program)
        assertTrue(vm.plan_removed)
        assertEquals("no plan generated at boot", "ClearPlan",
            QaHarness.logLines(ctx).map(EventLog::variantOf).last { it == "GeneratePlan" || it == "ClearPlan" })
    }

    /** The log gone while the prefs still say onboarding is done and an older
     *  build's plan was removed (vice versa): the app boots to a usable, empty main
     *  screen instead of crashing or hanging, and the legacy pref is dropped. */
    @Test
    fun aMissingLogWithStalePrefsStillBootsUsable() {
        ctx.setupPrefs().edit().putBoolean(KEY_ONBOARDING_DONE, true).putBoolean(KEY_PLAN_CLEARED, true).commit()
        rule.launchApp()
        val vm = EventDispatcher.model.value
        assertNotNull(vm)
        assertNull(vm!!.profile)
        assertFalse(ctx.setupPrefs().contains(KEY_PLAN_CLEARED))
    }

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
     * of up to [CoreConstants.TRACK_DECIMATION_CAP] fixes (~90 bytes each), so one year of a hybrid
     * athlete's history recorded at the tracker's screen-off cadence
     * (RunTrackingService FUSED_BG_INTERVAL_MS) is ~11.7 MiB; the
     * import cap ([MAX_IMPORT_BYTES]) once refused it at 8 MiB.
     */
    @Test
    fun aOneYearBackupWithGpsRunsIsImportable() {
        val history = LongHistorySeed.build(days = 365, fixEverySec = 4L)
        val exported = EventLog.compact(history.events.map(::line), Core)
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
     *  first well before twice the line cap is read. */
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
        assertTrue("read ${endless.served} bytes: past the line cap", endless.served < 2 * MAX_IMPORT_LINE_BYTES)
    }

    /** A data import that cannot write its temporary (disk full / read-only) fails
     *  as a whole: the live log is byte-identical, the core's log is re-attached so
     *  the next entry is still persisted, and nothing half-written replaces it. */
    @Test
    fun anImportThatCannotWriteFailsCleanlyAndTheLiveLogKeepsRecording() {
        assumeTrue("root ignores permissions", System.getProperty("user.name") != "root")
        val base = committed()
        writeLog(base)
        QaHarness.reboot(ctx)
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
            QaHarness.reboot(ctx).view
        })
    }

    /** A data import that runs out of heap mid-write (a backup too large for the
     *  device; [replaceEventLog] treats an OutOfMemoryError as a failed write)
     *  fails as a whole: the import reports [DataImportResult.Failed], the live log
     *  is byte-identical, no `.import` temporary is left, and the core's log is
     *  re-attached. */
    @Test
    fun anImportThatRunsOutOfMemoryFailsCleanlyAndTheLiveLogKeepsRecording() {
        val base = committed()
        writeLog(base)
        QaHarness.reboot(ctx)
        val before = logFile.readBytes()
        val profile = line(QaHarness.strengthProfile())
        val lines = object : AbstractList<String>() {
            override val size = 3
            override fun get(index: Int): String =
                if (index < 2) profile else throw OutOfMemoryError("injected: the backup outgrew the heap")
        }
        assertEquals(DataImportResult.Failed, onAppScope { replaceEventLog(ctx, lines) })
        assertTrue("live log untouched", before.contentEquals(logFile.readBytes()))
        assertFalse("no .import temporary left", File(ctx.filesDir, AppFileNames.EVENT_LOG + AppFileNames.IMPORT_SUFFIX).exists())
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
        QaHarness.reboot(ctx)
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
            QaHarness.reboot(ctx)
        } finally {
            logFile.setReadable(true, true)
            logFile.setWritable(true, true)
        }
        assertTrue("the boot result tells the user the data could not be read", booted.stale)
        assertTrue("bytes untouched", before.contentEquals(logFile.readBytes()))
        assertEquals(viewOf(committed()), QaHarness.reboot(ctx).view)
    }

    private fun fiveYearLog(fixEverySec: Long): Pair<File, Int> {
        val history = LongHistorySeed.build(days = 5 * 365, fixEverySec = fixEverySec)
        val big = File(ctx.cacheDir, "five-year.ndjson")
        big.bufferedWriter().use { w -> history.events.forEach { w.write(line(it)); w.write("\n") } }
        return big to history.events.size
    }

    /**
     * Boot's [EventLog.load] must fit in the per-app heap of an ordinary phone.
     * Android CDD §3.7 [C-0-2] guarantees a normal-screen xxhdpi (480 dpi) phone only
     * 128 MB of application memory for the WHOLE app. The load runs in a child JVM
     * capped at that; an OOM there is a boot crash loop on such a device, and the log
     * only grows, so the user cannot even reach Export. Always at the tracker's
     * screen-off cadence (~49 MiB of log).
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
        backup.bufferedWriter().use { w -> EventLog.compact(history.events.map(::line), Core).forEach { w.write(it); w.write("\n") } }
        val mb = backup.length() / (1024 * 1024)
        val dir = File(ctx.cacheDir, "import-target").apply { deleteRecursively(); mkdirs() }
        File(dir, AppFileNames.EVENT_LOG).writeText(line(QaHarness.strengthProfile()) + "\n")
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
            assertTrue("the log is the backup, byte for byte", backup.readBytes().contentEquals(File(dir, AppFileNames.EVENT_LOG).readBytes()))
            assertFalse("no .import temporary left", File(dir, AppFileNames.EVENT_LOG + AppFileNames.IMPORT_SUFFIX).exists())
        } finally {
            backup.delete()
            dir.deleteRecursively()
        }
    }

    /** The child gets this JVM's library path so it loads the same host core. */
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

    private fun fix(i: Int) = GpsPoint(52.52 + i * 0.0001, 13.405 + i * 0.00005, at(0, -1800L + i * 2), 4.5)

    /** The sidecar [ActiveRunStore] writes for a run of [fixes], those at [segmentStarts]
     *  opening a segment. */
    private fun recorded(fixes: List<GpsPoint>, segmentStarts: Set<Int> = emptySet()): ByteArray {
        ActiveRunStore.begin()
        fixes.forEachIndexed { i, p -> ActiveRunStore.append(p, segmentStart = i in segmentStarts) }
        ActiveRunStore.flush()
        return sidecar.readBytes()
    }

    /** Where the last line of [bytes] (which end in a newline) starts. */
    private fun lastLineStart(bytes: ByteArray) = bytes.copyOf(bytes.size - 1).lastIndexOf('\n'.code.toByte()) + 1

    /** A sidecar torn at every byte of its last fix recovers exactly the whole fixes
     *  (and their segment marks); a torn marker is a run with no fixes. */
    @Test
    fun aSidecarTornAtEveryByteRecoversExactlyTheWholeFixes() {
        ActiveRunStore.start(ctx)
        val fixes = (0 until 7).map(::fix)
        val whole = recorded(fixes, segmentStarts = setOf(3))
        for (k in lastLineStart(whole)..whole.size) {
            sidecar.writeBytes(whole.copyOf(k))
            val got = ActiveRunStore.recover()
            val want = if (k >= whole.size - 1) fixes else fixes.dropLast(1)
            assertEquals("cut at $k", want, got.toList())
            assertEquals("cut at $k: segment mark kept", listOf(3), got.segmentStarts)
        }
        val marker = recorded(emptyList())
        for (k in 0..marker.size) {
            sidecar.writeBytes(marker.copyOf(k))
            assertTrue(ActiveRunStore.hasActiveRun())
            assertEquals(0, ActiveRunStore.recover().size)
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
        ActiveRunStore.start(ctx)
        val fixes = (0 until 5).map(::fix)
        val whole = recorded(fixes + fix(5))
        sidecar.writeBytes(whole.copyOf(lastLineStart(whole) + 30))
        val recovered = ActiveRunStore.recover()
        assertEquals(fixes, recovered.toList())
        RunSession.restore(recovered)
        val resumed = fix(40)
        assertTrue(RunSession.add(resumed, (android.os.SystemClock.elapsedRealtime() + 2_000) * 1_000_000))
        ActiveRunStore.append(resumed, RunSession.isSegmentStart(resumed))
        ActiveRunStore.flush()
        val again = ActiveRunStore.recover()
        assertEquals("the post-resume fix survives a second crash", fixes + resumed, again.toList())
        assertEquals("…and still opens a segment", listOf(5), again.segmentStarts)
    }

    @Test
    fun sidecarGarbageLinesCostOnlyThemselves() {
        ActiveRunStore.start(ctx)
        val (marker, f0, f1, f2) = recorded(listOf(fix(0), fix(1), fix(2))).decodeToString().lines()
        val out = ByteArrayOutputStream()
        fun ln(b: ByteArray) { out.write(b); out.write('\n'.code) }
        ln(marker.toByteArray())
        ln(f0.toByteArray())
        ln(byteArrayOf(0xC3.toByte(), 0x28, 0xFF.toByte()))
        ln(f1.toByteArray())
        ln(ByteArray(64))
        ln("{\"lat\":\"x\"}".toByteArray())
        ln(f2.toByteArray())
        sidecar.writeBytes(out.toByteArray())
        assertEquals(listOf(fix(0), fix(1), fix(2)), ActiveRunStore.recover().toList())
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
            val r = EventLog.load(f, Core)
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
            val summary = summarizeImport(lines, kotlinx.datetime.TimeZone.UTC)
            swapLogFile(eventLogFile(File(args[1])), lines)
            println("lines=${lines.size} entries=${summary.entries} ms=${(System.nanoTime() - t0) / 1_000_000}")
        } catch (e: OutOfMemoryError) {
            println("OOM after ${(System.nanoTime() - t0) / 1_000_000} ms: $e\n${e.stackTrace.take(8).joinToString("\n") { "  at $it" }}")
            exitProcess(3)
        }
    }
}
