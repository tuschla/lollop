package app.milestone

import java.io.ByteArrayInputStream
import java.io.File
import java.io.InputStream
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.assertNotNull
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * [EventLog]: reading, repairing and rewriting the durable log around the core's
 * compaction (its rules are tested in `shared/src/log/tests.rs`), plus
 * [readBounded]'s guard on the other end of the same log: a restore reads a
 * user-chosen document before it may replace the log, and that read must stay
 * bounded.
 *
 * Robolectric, not plain JUnit: the JVM loads `libshared.so` into one classloader
 * only, and the Robolectric suite's shared sandbox loads it first, so a plain-JVM
 * [Core] in the same test JVM could not load it and would never compact.
 */
@RunWith(RobolectricTestRunner::class)
class EventLogTest {

    @Before
    fun coreAnswers() {
        Core.faultForTest = null
        Core.clearUnavailableForTest()
        assertNotNull(
            "the host core does not answer (library load error: ${Core.loadError}); these tests need it to compact",
            Core.compaction(emptyList()),
        )
    }

    private fun line(e: Event) = e.toJson().toString()

    // Pin observedAt + entryId so a helper called twice (input AND expected)
    // produces identical lines: the LogSet/LogRun defaults are the wall clock.
    private fun set(exercise: String) = line(Event.LogSet(exercise, 100.0, 5, 8.0, observedAt = 0, entryId = 0))
    private fun run(km: Double) = line(Event.LogRun(km, km * 5.0, 75.0, observedAt = 0, entryId = 0))
    private fun setToday(day: Long) = line(Event.SetToday(day))
    private fun checkin(observedAt: Long) =
        line(Event.SubmitCheckin(observedAt, sleepQuality = 3, soreness = 3, mood = 3))

    private val DAY = 86_400L
    private val torn = """{"LogSet":{"exercise":"Bench","weigh"""

    // ── compact(): the core's rules over wire lines ──

    @Test
    fun compactAppliesTheCoresRulesAndKeepsANonJsonLine() {
        val out = EventLog.compact(listOf(torn, set("a"), line(Event.ClearSets), run(5.0)))
        assertEquals(listOf(torn, run(5.0)), out)
    }

    @Test
    fun withoutTheCoreTheLogIsReadAsItIsMinusTornLines() {
        // The core-unavailable boot still exports the raw log.
        val file = tempLog()
        file.writeText(listOf(set("a"), line(Event.ClearSets), torn, run(5.0)).joinToString("\n", postfix = "\n"))
        Core.faultForTest = object : CoreFault {
            override fun update(event: ByteArray): ByteArray? = null
            override fun view(): ByteArray? = null
        }
        try {
            assertEquals(listOf(set("a"), line(Event.ClearSets), run(5.0)), EventLog.read(file).lines)
            assertEquals(listOf(set("a"), line(Event.ClearSets)), EventLog.compact(listOf(set("a"), line(Event.ClearSets))))
        } finally {
            Core.faultForTest = null
        }
    }

    @Test
    fun activityVariantsAreRecognized() {
        assertEquals("ClearActivities", EventLog.variantOf(line(Event.ClearActivities)))
        assertEquals(
            "LogActivity",
            EventLog.variantOf(line(Event.LogActivity(ActivityKind.Sport, 45.0, observedAt = 100, entryId = 1))),
        )
    }

    @Test
    fun variantOfReadsUnitAndStructTags() {
        assertEquals("ClearSets", EventLog.variantOf(line(Event.ClearSets)))
        assertEquals("LogSet", EventLog.variantOf(set("a")))
        assertEquals(null, EventLog.variantOf("not json"))
    }

    @Test
    fun variantOfRejectsShapeMismatchedNamedLines() {
        // A unit variant with a body, a struct variant with a non-object payload, or
        // a struct name as a bare string names NO event, so import rejects it.
        assertEquals(null, EventLog.variantOf("""{"ClearSets":{}}"""))
        assertEquals(null, EventLog.variantOf("""{"LogSet":5}"""))
        assertEquals(null, EventLog.variantOf("\"LogSet\""))
        assertEquals("ClearSets", EventLog.variantOf("\"ClearSets\""))
        assertEquals(
            "SetExerciseEnded",
            EventLog.variantOf(
                """{"SetExerciseEnded":{"exercise":"Bench","epoch_day":1,"ended":true,"observed_at":0}}""",
            ),
        )
    }

    // ── load(): fresh-install vs compacted-empty (Core.restore's seed decision) ──

    private fun tempLog(): File =
        File.createTempFile("event-log", ".ndjson").apply { deleteOnExit() }

    @Test
    fun loadMissingFileIsFreshInstall() {
        val file = tempLog().also { it.delete() }
        val restored = EventLog.load(file)
        assertTrue(restored.freshInstall)
        assertTrue(restored.lines.isEmpty())
    }

    @Test
    fun loadCompactedEmptyLogIsReturningUserNotFreshInstall() {
        // A user who logged sets then cleared them: every line compacts away, but
        // the log file exists: this must NOT read as a fresh install, or the
        // caller would re-seed the SEED profile + onboarding over a deliberate
        // empty state.
        val file = tempLog()
        file.writeText(
            listOf(set("a"), set("b"), line(Event.ClearSets))
                .joinToString("\n", postfix = "\n")
        )
        val restored = EventLog.load(file)
        assertFalse(restored.freshInstall)
        assertTrue(restored.lines.isEmpty())
    }

    @Test
    fun loadBlankButExistingLogIsNotFreshInstall() {
        // A previous compaction that emptied the log leaves "\n" behind; the file
        // still exists, so this too is a returning user.
        val file = tempLog()
        file.writeText("\n")
        val restored = EventLog.load(file)
        assertFalse(restored.freshInstall)
        assertTrue(restored.lines.isEmpty())
    }

    @Test
    fun loadKeepsSurvivorsAndRewritesFileCompacted() {
        val file = tempLog()
        file.writeText(
            listOf(set("a"), line(Event.ClearSets), run(5.0))
                .joinToString("\n", postfix = "\n")
        )
        val restored = EventLog.load(file)
        assertFalse(restored.freshInstall)
        assertEquals(listOf(run(5.0)), restored.lines)
        // The durable log was compacted in place (atomic rewrite).
        assertEquals(listOf(run(5.0)), file.readLines().filter { it.isNotBlank() })
    }

    // ── load(): drop crash-truncated lines, but never a valid-JSON future event
    //    this build doesn't recognize (forward-compat) ──

    @Test
    fun loadGarbageCollectsATornLineAndRewritesTheFile() {
        // A crash mid-appendText left a truncated, non-JSON record. Replay would
        // reject it on every launch, so load() drops it from the durable log too.
        val file = tempLog()
        file.writeText(
            (listOf(set("a"), torn, run(5.0)).joinToString("\n")) + "\n"
        )
        val restored = EventLog.load(file)
        assertFalse(restored.freshInstall)
        assertEquals(listOf(set("a"), run(5.0)), restored.lines)
        assertEquals(listOf(set("a"), run(5.0)), file.readLines().filter { it.isNotBlank() })
    }

    @Test
    fun loadPreservesUnknownButValidJsonVariants() {
        // FORWARD-COMPAT: a newer build wrote events this build doesn't know (an
        // unknown struct variant AND an unknown unit variant). Both are valid JSON,
        // so both survive load() untouched: an old build has to hand them back when
        // the user upgrades again.
        val file = tempLog()
        val unknownObject = """{"FutureStructEvent":{"weeks":6,"note":"hi"}}"""
        val unknownString = "\"FutureUnitEvent\""
        val original = listOf(set("a"), unknownObject, unknownString, run(5.0))
        file.writeText(original.joinToString("\n") + "\n")
        val restored = EventLog.load(file)
        assertFalse(restored.freshInstall)
        assertEquals(original, restored.lines)
        // Nothing dropped → the file is left exactly as-is (no rewrite needed).
        assertEquals(original, file.readLines().filter { it.isNotBlank() })
    }

    @Test
    fun loadLeavesANormalMixedLogUnchanged() {
        // No torn lines and nothing compaction can drop: load() returns the lines
        // verbatim and does not rewrite the file.
        val file = tempLog()
        val original = listOf(set("a"), run(5.0), checkin(0))
        val bytes = original.joinToString("\n") + "\n"
        file.writeText(bytes)
        val restored = EventLog.load(file)
        assertFalse(restored.freshInstall)
        assertEquals(original, restored.lines)
        assertEquals(bytes, file.readText())
    }

    @Test
    fun loadSkipsTheRewriteForATinyDropButStillReturnsCompactedLines() {
        // A routine launch: two superseded SetTodays are the only drops in a log
        // large enough that the bounded rewrite rule calls them not worth an
        // fsync. The file ON DISK stays untouched while replay still gets the
        // compacted survivors. Sized from the rule itself, so a retuned threshold
        // keeps this a below-threshold case.
        val drops = 2
        val filler = (1..drops * EventLog.REWRITE_MIN_DROPPED_SHARE_INVERSE).map { set("f$it") }
        val oldCheckin = checkin(DAY)
        // A recent reading: a windowing rule would anchor on it and drop the old one.
        val original = filler + listOf(setToday(1), setToday(2), oldCheckin, checkin(60 * DAY), setToday(60))
        assertFalse(EventLog.compactionWorthRewriting(drops, original.size))
        val file = tempLog()
        file.writeText(original.joinToString("\n") + "\n")

        val restored = EventLog.load(file)
        // Compaction still drops the superseded SetTodays (and keeps the check-in)...
        assertFalse(restored.lines.contains(setToday(1)))
        assertFalse(restored.lines.contains(setToday(2)))
        assertTrue(restored.lines.contains(setToday(60)))
        assertTrue(restored.lines.contains(oldCheckin))
        assertEquals(original.size - drops, restored.lines.size)
        // ...but the durable file is left exactly as written.
        assertEquals(original, file.readLines().filter { it.isNotBlank() })
    }

    @Test
    fun loadSelfHealsANewlinelessTornTailThenAcceptsAnAppend() {
        // A process kill mid-appendText left a newline-less partial last line.
        // load() drops it AND rewrites the durable log with a trailing newline, so
        // the next append starts a fresh line instead of gluing its JSON onto the
        // torn bytes (which would lose that event on the next boot).
        val file = tempLog()
        file.writeText(set("a") + "\n" + torn) // valid first line, torn tail, NO trailing newline
        val restored = EventLog.load(file)
        assertFalse(restored.freshInstall)
        assertEquals(listOf(set("a")), restored.lines)
        assertEquals(listOf(set("a")).joinToString("\n", postfix = "\n"), file.readText())
        // A subsequent append lands on its own line → the whole log reparses.
        file.appendText(run(5.0) + "\n")
        val reloaded = EventLog.load(file)
        assertEquals(listOf(set("a"), run(5.0)), reloaded.lines)
    }

    @Test
    fun loadRewritesAValidButNewlinelessLastLineWithATrailingNewline() {
        // Even when every line parses, a newline-less tail (an append cut off right
        // before its '\n') must be healed: the next append would otherwise glue
        // onto this line and corrupt both events.
        val file = tempLog()
        val original = listOf(set("a"), run(5.0))
        file.writeText(original.joinToString("\n")) // valid last line, NO trailing newline
        val restored = EventLog.load(file)
        assertFalse(restored.freshInstall)
        assertEquals(original, restored.lines)
        assertEquals(original.joinToString("\n", postfix = "\n"), file.readText())
    }

    // ── read(): the export's view of the log must never mutate it ──

    @Test
    fun readCompactsWithoutTouchingTheLogOrItsTemporaries() {
        // Everything load() would act on: a compactable prefix (≥5% dropped), a
        // torn newline-less tail, and leftover swap temporaries. read() runs while
        // the core may be appending, so it must change none of it.
        val file = tempLog()
        val bytes = listOf(set("a"), line(Event.ClearSets), run(5.0)).joinToString("\n", postfix = "\n") + torn
        file.writeText(bytes)
        val temps = listOf(".tmp", ".import", ".bak").map { File(file.parentFile, file.name + it) }
        try {
            temps.forEach { it.writeText("x") }

            val read = EventLog.read(file)

            assertFalse(read.freshInstall)
            assertEquals(listOf(run(5.0)), read.lines)
            assertEquals(bytes, file.readText())
            assertTrue(temps.all { it.exists() && it.readText() == "x" })
        } finally {
            temps.forEach { it.delete() }
        }
    }

    @Test
    fun readOfAMissingLogCreatesNothingAndRestoresNothing() {
        // load() would move a crash-left .bak back into place; read() must not.
        val file = tempLog().also { it.delete() }
        val bak = File(file.parentFile, file.name + ".bak").apply { writeText(set("a") + "\n") }
        try {
            val read = EventLog.read(file)
            assertTrue(read.freshInstall)
            assertTrue(read.lines.isEmpty())
            assertFalse(file.exists())
            assertTrue(bak.exists())
        } finally {
            bak.delete()
        }
    }

    @Test
    fun loadReportsTheLargestEntryIdEvenWhenItsLineCompactsAway() {
        // EntryIds must never reuse an id a restored entry carried, including one a
        // later clear removed (a stale amend/delete could still name it).
        val file = tempLog()
        file.writeText(
            listOf(
                line(Event.LogSet("a", 100.0, 5, 8.0, observedAt = 1, entryId = 5)),
                line(Event.LogSet("b", 100.0, 5, 8.0, observedAt = 2, entryId = 900)),
                line(Event.ClearSets),
                line(Event.LogRun(5.0, 25.0, 75.0, observedAt = 3, entryId = 12)),
            ).joinToString("\n", postfix = "\n")
        )
        assertEquals(900L, EventLog.load(file).maxEntryId)
    }

    @Test
    fun loadReportsARemovedPlan() {
        val file = tempLog()
        file.writeText(listOf(line(Event.GeneratePlan(1)), line(Event.ClearPlan)).joinToString("\n", postfix = "\n"))
        val restored = EventLog.load(file)
        assertEquals(listOf(line(Event.ClearPlan)), restored.lines)
        assertTrue(restored.planRemoved)
    }

    @Test
    fun entryIdsStayAboveARestoredIdAfterTheClockMovedBack() {
        // A restored id from a clock that ran ahead of today's wall clock.
        val restored = System.currentTimeMillis() + 10L * 365 * 86_400_000L
        EntryIds.raiseTo(restored)
        val a = EntryIds.next()
        val b = EntryIds.next()
        assertTrue(a > restored)
        assertTrue(b > a)
    }

    @Test
    fun swapLogFileReplacesTheLogAndLeavesNoTemporary() {
        val dir = java.nio.file.Files.createTempDirectory("swap").toFile()
        try {
            File(dir, "event-log.ndjson").writeText(set("old") + "\n")
            swapLogFile(dir, listOf(set("a"), run(5.0)))
            assertEquals(listOf(set("a"), run(5.0)).joinToString("\n", postfix = "\n"), File(dir, "event-log.ndjson").readText())
            assertFalse(File(dir, "event-log.ndjson.import").exists())
        } finally {
            dir.deleteRecursively()
        }
    }

    /** Emits [n] bytes and records how many the reader actually pulled, so a test
     *  can pin read amplification and not just the returned value. */
    private class CountingSource(private val n: Long) : InputStream() {
        var bytesRead = 0L
            private set
        private var pos = 0L

        override fun read(): Int {
            if (pos >= n) return -1
            pos++
            bytesRead++
            return 0
        }

        override fun read(b: ByteArray, off: Int, len: Int): Int {
            if (pos >= n) return -1
            val take = minOf(len.toLong(), n - pos).toInt()
            b.fill(0, off, off + take)
            pos += take
            bytesRead += take
            return take
        }
    }

    @Test
    fun streamValidateRejectsAnOversizedStreamWithoutDrainingIt() {
        // A content provider may report an unknown size, so the bound has to hold on
        // the stream itself: a stream past the cap is rejected...
        val src = CountingSource(MAX_IMPORT_BYTES + 1)
        assertEquals(ImportCheck.Invalid, streamValidateImport(src, MAX_IMPORT_BYTES))
        // ...and the reader stops there instead of buffering the whole file: at most
        // one 64 KiB chunk past the cap is ever pulled (bounded memory).
        assertTrue(
            "read ${src.bytesRead} bytes past the cap",
            src.bytesRead <= MAX_IMPORT_BYTES + 64 * 1024,
        )
    }

    @Test
    fun streamValidateAcceptsAWellFormedLogAndRejectsAJunkLine() {
        val log = (set("a") + "\n" + run(5.0) + "\n").toByteArray()
        assertEquals(
            ImportCheck.Valid(listOf(set("a"), run(5.0))),
            streamValidateImport(ByteArrayInputStream(log), MAX_IMPORT_BYTES),
        )
        // A single unknown/garbage line rejects the whole file (streamed, so a large
        // binary file trips this on its first line before it can fill the heap).
        val junk = (set("a") + "\n" + "not a wire line" + "\n").toByteArray()
        assertEquals(
            ImportCheck.Invalid,
            streamValidateImport(ByteArrayInputStream(junk), MAX_IMPORT_BYTES),
        )
    }

    @Test
    fun streamValidateRejectsAMalformedNamedClearTheCoreWouldReject() {
        // {"ClearSets":{}} is a unit variant given a body, a shape the core rejects
        // at serde. Import must not accept it.
        val evil = (set("a") + "\n" + """{"ClearSets":{}}""" + "\n").toByteArray()
        assertEquals(
            ImportCheck.Invalid,
            streamValidateImport(ByteArrayInputStream(evil), MAX_IMPORT_BYTES),
        )
    }
}
