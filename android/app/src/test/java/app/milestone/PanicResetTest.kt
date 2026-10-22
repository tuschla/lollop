package app.milestone

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The shell must distinguish the FFI `panic_reset` envelope (the core bridge
 * was replaced with a fresh Engine → the persisted log must be replayed into it)
 * from a plain `panic` envelope (bridge intact → keep the last good view) and from
 * an ordinary view. These classifiers gate that recovery, so a drift in the FFI
 * envelope shape would either strand a bricked session or needlessly rebuild the
 * core. They are top-level (not [Core] members), so they run on the JVM without the
 * native library.
 */
class PanicResetTest {
    // Exact bytes ffi.rs emits after catch_panic replaces the bridge.
    private val panicReset =
        """{"error":{"kind":"panic_reset","context":"update","message":"Model RwLock was poisoned."}}"""

    // A panic that did NOT reset the bridge (the `view` path's "panic" kind).
    private val plainPanic =
        """{"error":{"kind":"panic","context":"view","message":"boom"}}"""

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
        val view = """{"train_blocked":false,"safety_tier":null,"kind":"day"}"""
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
     *  [poison] panics it, which swaps in an EMPTY core, like ffi.rs does. */
    private class FakeCore(private val poison: Set<String>) {
        val state = mutableListOf<String>()
        var applies = 0
        fun apply(line: String): ByteArray {
            applies++
            if (line in poison) {
                state.clear()
                return """{"error":{"kind":"panic_reset","context":"update","message":"boom"}}""".toByteArray()
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
