package app.milestone

import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/**
 * The crash-safe active-run file ([ActiveRunStore]) over the platform file seam
 * ([LogFiles]): java.io on the JVM, POSIX on iOS. Recovery skips the marker and a
 * torn last line, keeps the segment marks, an append after a torn tail starts its
 * own line, and the post-save clear runs after every queued append.
 */
class ActiveRunStoreTest {
    private val dir = newTempDir()
    private val file = LogFiles.sibling(dir, "/active-run.ndjson")

    @BeforeTest
    fun setUp() {
        RunSession.reset()
        ActiveRunStore.useFile(file)
    }

    @AfterTest
    fun tearDown() {
        ActiveRunStore.flush()
        ActiveRunStore.useFile(null)
        LogFiles.delete(file)
        LogFiles.delete(dir)
        RunSession.reset()
    }

    private fun fix(i: Int) = GpsPoint(lat = 52.5, lon = 13.4 + i * 3e-5, observedAt = 1_790_000_000L + i, accuracyM = 4.5)

    private fun line(p: GpsPoint, seg: Boolean = false) =
        "{\"lat\":${p.lat},\"lon\":${p.lon},\"observed_at\":${p.observedAt},\"accuracy_m\":${p.accuracyM}" +
            (if (seg) ",\"seg\":true" else "") + "}"

    private fun lines(): List<String> = buildList { LogFiles.forEachLine(file) { add(it) } }

    @Test
    fun aRunWritesTheMarkerThenOneLinePerFix() {
        ActiveRunStore.begin()
        val fixes = (0 until 3).map(::fix)
        fixes.forEach(ActiveRunStore::append)
        ActiveRunStore.flush()
        assertEquals(listOf("{\"run\":\"active\"}") + fixes.map { line(it) }, lines())
        assertTrue(ActiveRunStore.hasActiveRun())
        assertEquals(fixes, ActiveRunStore.recover().toList())
    }

    @Test
    fun recoveryKeepsSegmentMarksAndSkipsATornLastLine() {
        val fixes = (0 until 5).map(::fix)
        val text = "{\"run\":\"active\"}\n" + fixes.mapIndexed { i, p -> line(p, seg = i == 3) + "\n" }.joinToString("") +
            line(fix(5)).take(25)
        LogFiles.writeText(file, text)
        val back = ActiveRunStore.recover()
        assertEquals(fixes, back.toList())
        assertEquals(listOf(3), back.segmentStarts)
    }

    @Test
    fun anAppendAfterATornTailStartsItsOwnLine() {
        val fixes = (0 until 3).map(::fix)
        LogFiles.writeText(file, "{\"run\":\"active\"}\n" + fixes.joinToString("") { line(it) + "\n" } + line(fix(3)).take(30))
        ActiveRunStore.append(fix(9))
        ActiveRunStore.flush()
        assertEquals(fixes + fix(9), ActiveRunStore.recover().toList())
        assertTrue(LogFiles.endsWithNewline(file))
    }

    @Test
    fun aSegmentStartFixIsMarked() {
        // RunSession decides the boundary: a pause between the second and third fix.
        assertTrue(RunSession.add(fix(0), 1_000_000_000L))
        assertTrue(RunSession.add(fix(1), 2_000_000_000L))
        RunSession.setPaused(true)
        RunSession.setPaused(false)
        assertTrue(RunSession.add(fix(2), 3_000_000_000L))
        ActiveRunStore.begin()
        RunSession.points.value.forEach(ActiveRunStore::append)
        ActiveRunStore.flush()
        assertEquals(line(fix(2), seg = true), lines().last())
        assertEquals(listOf(2), ActiveRunStore.recover().segmentStarts)
    }

    @Test
    fun theTornTailCutKeepsWholeLinesOnly() {
        LogFiles.writeText(file, "a\nb\n")
        LogFiles.dropTornTail(file)
        assertEquals(listOf("a", "b"), lines())
        LogFiles.writeText(file, "a\nb")
        LogFiles.dropTornTail(file)
        assertEquals(listOf("a"), lines())
        LogFiles.writeText(file, "x".repeat(10_000))
        LogFiles.dropTornTail(file)
        assertEquals(emptyList<String>(), lines(), "no newline at all: nothing whole")
        LogFiles.delete(file)
        LogFiles.dropTornTail(file)
        assertTrue(LogFiles.exists(file), "a missing file is created empty")
    }

    @Test
    fun theSaveClearRunsAfterEveryQueuedAppend() {
        ActiveRunStore.begin()
        repeat(200) { ActiveRunStore.append(fix(it)) }
        ActiveRunStore.clearSync()
        assertFalse(ActiveRunStore.hasActiveRun(), "no late append recreates it")
    }
}
