package app.milestone

import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/**
 * The crash-safe active-run file ([ActiveRunStore]) over the platform file seam
 * ([AppFiles]): java.io on the JVM, POSIX on iOS. Recovery skips the marker and a
 * torn last line, keeps the segment marks, an append after a torn tail starts its
 * own line, and the post-save clear runs after every queued append. The engine is a
 * stand-in ([FakeCore]), so recovery keeps every fix (the core's thinning is
 * `RunTrackingUnitTest`'s and `RunFixGuardsTest`'s).
 */
class ActiveRunStoreTest : PlatformTest() {
    private val dir = newTempDir()
    private val file = fileIn(dir, AppFileNames.ACTIVE_RUN)

    @BeforeTest
    fun setUp() {
        AppRuntime.resetForTest(engine = DurableCore(FakeCore()))
        RunSession.reset()
        ActiveRunStore.open(file)
    }

    @AfterTest
    fun tearDown() {
        ActiveRunStore.flush()
        AppFiles.delete(file)
        AppFiles.delete(dir)
        RunSession.reset()
    }

    private fun fix(i: Int) = GpsPoint(lat = 52.5, lon = 13.4 + i * 3e-5, observedAt = 1_790_000_000L + i, accuracyM = 4.5)

    private fun lines(): List<String> = buildList { AppFiles.forEachLine(file) { add(it) } }

    /** The sidecar's lines once a run recorded [fixes], those at [segmentStarts] opening a segment. */
    private fun recorded(fixes: List<GpsPoint>, segmentStarts: Set<Int> = emptySet()): List<String> {
        ActiveRunStore.begin()
        fixes.forEachIndexed { i, p -> ActiveRunStore.append(p, segmentStart = i in segmentStarts) }
        ActiveRunStore.flush()
        return lines()
    }

    /** [lines] as a process killed while it appended the last one leaves them. */
    private fun writeTornAt(lines: List<String>, keep: Int) =
        AppFiles.writeText(file, lines.dropLast(1).joinToString("") { "$it\n" } + lines.last().take(keep))

    @Test
    fun aRunWritesTheMarkerThenOneLinePerFix() {
        ActiveRunStore.begin()
        ActiveRunStore.flush()
        assertTrue(ActiveRunStore.hasActiveRun(), "the marker alone marks a run in progress")
        assertEquals(0, ActiveRunStore.recover().size)
        val fixes = (0 until 3).map(::fix)
        assertEquals(1 + fixes.size, recorded(fixes).size, "the marker, then one line per fix")
        assertTrue(ActiveRunStore.hasActiveRun())
        assertEquals(fixes, ActiveRunStore.recover().toList())
    }

    @Test
    fun recoveryKeepsSegmentMarksAndSkipsATornLastLine() {
        val fixes = (0 until 5).map(::fix)
        writeTornAt(recorded(fixes + fix(5), segmentStarts = setOf(3)), keep = 25)
        val back = ActiveRunStore.recover()
        assertEquals(fixes, back.toList())
        assertEquals(listOf(3), back.segmentStarts)
    }

    @Test
    fun anAppendAfterATornTailStartsItsOwnLine() {
        val fixes = (0 until 3).map(::fix)
        writeTornAt(recorded(fixes + fix(3)), keep = 30)
        ActiveRunStore.append(fix(9), segmentStart = false)
        ActiveRunStore.flush()
        assertEquals(fixes + fix(9), ActiveRunStore.recover().toList())
        assertTrue(AppFiles.endsWithNewline(file))
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
        RunSession.points.value.forEach { ActiveRunStore.append(it, RunSession.isSegmentStart(it)) }
        ActiveRunStore.flush()
        assertEquals(listOf(2), ActiveRunStore.recover().segmentStarts)
    }

    @Test
    fun theTornTailCutKeepsWholeLinesOnly() {
        AppFiles.writeText(file, "a\nb\n")
        AppFiles.dropTornTail(file)
        assertEquals(listOf("a", "b"), lines())
        AppFiles.writeText(file, "a\nb")
        AppFiles.dropTornTail(file)
        assertEquals(listOf("a"), lines())
        AppFiles.writeText(file, "x".repeat(10_000))
        AppFiles.dropTornTail(file)
        assertEquals(emptyList<String>(), lines(), "no newline at all: nothing whole")
        AppFiles.delete(file)
        AppFiles.dropTornTail(file)
        assertTrue(AppFiles.exists(file), "a missing file is created empty")
    }

    @Test
    fun theSaveClearRunsAfterEveryQueuedAppend() {
        ActiveRunStore.begin()
        repeat(200) { ActiveRunStore.append(fix(it), segmentStart = false) }
        ActiveRunStore.clearSync()
        assertFalse(ActiveRunStore.hasActiveRun(), "no late append recreates it")
    }
}
