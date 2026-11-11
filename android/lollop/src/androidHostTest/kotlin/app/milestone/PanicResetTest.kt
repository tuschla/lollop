package app.milestone

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The shell must distinguish the `panic_reset` envelope (the core was replaced
 * with a fresh one → the persisted log must be replayed into it) from a plain
 * `panic` envelope (core intact → keep the last good view) and from an ordinary
 * view. These classifiers gate that recovery. They are top-level (not [Core]
 * members), so they run on the JVM without the native library.
 */
class PanicResetTest {
    private fun envelope(kind: ErrorKind, context: String, message: String) =
        coreJson.encodeToString(ErrorEnvelope.serializer(), ErrorEnvelope(CoreError(kind, context, message)))

    private val panicReset = envelope(ErrorKind.PanicReset, "update", "Model RwLock was poisoned.")

    // A panic that did NOT reset the core (the `view` path's "panic" kind).
    private val plainPanic = envelope(ErrorKind.Panic, "view", "boom")

    @Test
    fun panicResetEnvelopeIsDetectedAsBothErrorAndReset() {
        assertTrue(isCoreErrorEnvelope(panicReset))
        assertTrue(isCorePanicReset(panicReset))
    }

    @Test
    fun plainPanicIsAnErrorButNotAReset() {
        // The bridge was NOT replaced, so the shell keeps the last good view rather
        // than replaying the log — isCorePanicReset must be false here.
        assertTrue(isCoreErrorEnvelope(plainPanic))
        assertFalse(isCorePanicReset(plainPanic))
    }

    @Test
    fun anOrdinaryViewIsNeitherErrorNorReset() {
        val view = coreJson.encodeToString(ViewModel.serializer(), ViewModel())
        assertFalse(isCoreErrorEnvelope(view))
        assertFalse(isCorePanicReset(view))
        // A crux effect array (the accepted-event response) is not an error either.
        assertFalse(isCoreErrorEnvelope("""[{"Render":[]}]"""))
    }

    @Test
    fun leadingWhitespaceStillClassifiesTheEnvelope() {
        assertTrue(isCorePanicReset("  \n$panicReset"))
    }

    @Test
    fun rawResponsesClassifyByTheirLeadingByte() {
        assertTrue(isPanicResetResponse(" \n$panicReset".toByteArray()))
        assertFalse(isPanicResetResponse(plainPanic.toByteArray()))
        assertFalse(isPanicResetResponse("""[{"Render":[]}]""".toByteArray()))
        assertFalse(isPanicResetResponse(ByteArray(0)))
        assertFalse(isPanicResetResponse(null))
    }

    /** A stand-in for the native core: applies lines into [state]; a line in
     *  [poison] panics it, which swaps in an EMPTY core, as the bridge does. */
    private inner class FakeCore(private val poison: Set<String>) {
        val state = mutableListOf<String>()
        var applies = 0
        fun apply(line: String): ByteArray {
            applies++
            if (line in poison) {
                state.clear()
                return envelope(ErrorKind.PanicReset, "update", "boom").toByteArray()
            }
            state += line
            return """[{"Render":[]}]""".toByteArray()
        }
    }

    @Test
    fun aLineThatResetsTheCoreIsSkippedAndEverythingElseIsReplayed() {
        // Without recovery the reset would wipe "hold" (a DO-NOT-TRAIN hold logged
        // before the bad line) and only "after" would survive.
        val core = FakeCore(poison = setOf("bad"))
        val outcome = replayLog(listOf("hold", "bad", "after"), maxResets = 16, apply = core::apply)

        assertEquals(setOf(1), outcome.skipped)
        assertTrue(outcome.complete)
        assertEquals(listOf("hold", "after"), core.state)
    }

    @Test
    fun everyResettingLineIsSkipped() {
        val core = FakeCore(poison = setOf("bad1", "bad2"))
        val outcome = replayLog(listOf("a", "bad1", "b", "bad2", "c"), maxResets = 16, apply = core::apply)

        assertEquals(setOf(1, 3), outcome.skipped)
        assertTrue(outcome.complete)
        assertEquals(listOf("a", "b", "c"), core.state)
    }

    @Test
    fun aCleanLogReplaysOnceWithNothingSkipped() {
        val core = FakeCore(poison = emptySet())
        val outcome = replayLog(listOf("a", "b"), maxResets = 16, apply = core::apply)

        assertTrue(outcome.skipped.isEmpty())
        assertTrue(outcome.complete)
        assertEquals(2, core.applies)
    }

    @Test
    fun theReplayGivesUpOnceTheResetBudgetIsSpent() {
        val core = FakeCore(poison = setOf("bad1", "bad2"))
        val outcome = replayLog(listOf("a", "bad1", "bad2"), maxResets = 1, apply = core::apply)

        assertFalse(outcome.complete)
        assertEquals(setOf(1, 2), outcome.skipped)
    }
}
