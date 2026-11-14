package app.milestone

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/**
 * The shell must distinguish the `panic_reset` envelope (the core was replaced
 * with a fresh one → the persisted log must be replayed into it) from a plain
 * `panic` envelope (core intact → keep the last good view) and from an ordinary
 * view. These classifiers gate that recovery, and [replayLog] runs it: a line that
 * resets the core is skipped and the replay starts over, within a reset budget.
 */
class PanicResetTest {
    private val panicReset = coreErrorJson(ErrorKind.PanicReset, "update", "Model RwLock was poisoned.")

    // A panic that did NOT reset the core (the `view` path's "panic" kind).
    private val plainPanic = coreErrorJson(ErrorKind.Panic, "view", "boom")

    /** An accepted event's response: the core's effect-request array. */
    private val effects = """[{"Render":[]}]"""

    @Test
    fun panicResetEnvelopeIsDetectedAsBothErrorAndReset() {
        assertTrue(isCoreErrorEnvelope(panicReset))
        assertTrue(isCorePanicReset(panicReset))
    }

    @Test
    fun plainPanicIsAnErrorButNotAReset() {
        // The bridge was NOT replaced, so the shell keeps the last good view and
        // does not replay the log: isCorePanicReset must be false here.
        assertTrue(isCoreErrorEnvelope(plainPanic))
        assertFalse(isCorePanicReset(plainPanic))
    }

    @Test
    fun anOrdinaryViewIsNeitherErrorNorReset() {
        val view = coreJson.encodeToString(ViewModel.serializer(), ViewModel())
        assertFalse(isCoreErrorEnvelope(view))
        assertFalse(isCorePanicReset(view))
        // A crux effect array (the accepted-event response) is not an error either.
        assertFalse(isCoreErrorEnvelope(effects))
    }

    @Test
    fun leadingWhitespaceStillClassifiesTheEnvelope() {
        assertTrue(isCorePanicReset("  \n$panicReset"))
    }

    @Test
    fun rawResponsesClassifyByTheirLeadingByte() {
        assertTrue(isPanicResetResponse(" \n$panicReset".encodeToByteArray()))
        assertFalse(isPanicResetResponse(plainPanic.encodeToByteArray()))
        assertFalse(isPanicResetResponse(effects.encodeToByteArray()))
        assertFalse(isPanicResetResponse(ByteArray(0)))
        assertFalse(isPanicResetResponse(null))
    }

    private fun line(id: Long) = Event.LogSet("Back Squat", 100.0, 5, 8.0, observedAt = 1_725_904_800, entryId = id).toJson().toString()

    /** [lines] replayed into [core], which empties itself on each line that resets it. */
    private fun replay(core: FakeCore, lines: List<String>, maxResets: Int = MAX_REPLAY_RESETS) =
        replayLog(lines, maxResets) { core.update(it.encodeToByteArray()) }

    @Test
    fun everyResettingLineIsSkipped() {
        // Without recovery a reset would wipe the lines before it (a DO-NOT-TRAIN
        // hold among them) and only the ones after it would survive.
        val lines = (1L..5L).map(::line)
        val core = FakeCore().apply { poison = setOf(lines[1], lines[3]) }
        val outcome = replay(core, lines)

        assertEquals(setOf(1, 3), outcome.skipped)
        assertTrue(outcome.complete)
        assertEquals(listOf(lines[0], lines[2], lines[4]), core.applied)
    }

    @Test
    fun aCleanLogReplaysOnceWithNothingSkipped() {
        val lines = (1L..2L).map(::line)
        val core = FakeCore()
        val outcome = replay(core, lines)

        assertTrue(outcome.skipped.isEmpty())
        assertTrue(outcome.complete)
        assertEquals(lines, core.applied, "each line applied once")
    }

    @Test
    fun theReplayGivesUpOnceTheResetBudgetIsSpent() {
        val lines = (1L..3L).map(::line)
        val core = FakeCore().apply { poison = setOf(lines[1], lines[2]) }
        val outcome = replay(core, lines, maxResets = 1)

        assertFalse(outcome.complete)
        assertEquals(setOf(1, 2), outcome.skipped)
    }
}
