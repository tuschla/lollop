package app.milestone

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.runTest

/**
 * [runSaveCriticalSection] persists the run and tears down the session as ONE
 * unit that survives the caller's scope being cancelled mid-save (a rotation
 * cancels `rememberCoroutineScope` while the non-cancellable JNI `Core.send` is in
 * flight): clear and reset still run, and the cancellation never reaches the
 * failure path that re-shows Save/Discard for an already-logged run. The tests
 * drive a real coroutine cancellation against the effect-injected function.
 * [PlatformTest]: the failure path logs through [PlatformLog].
 */
class RunSaveCriticalSectionTest : PlatformTest() {

    /**
     * (a) The cancellation scenario: the calling scope is cancelled while [send] (the JNI
     * persist) is still suspended. The persist must still complete, and the sidecar
     * clear + session reset, sequenced inside the same `NonCancellable` block, must
     * BOTH run so a rotation can't strand an already-persisted run. The
     * cancellation must NOT be routed to the failure path (that is the double-log
     * bug); it re-throws instead. The `finish` navigation is best-effort (its
     * `NonCancellable` block may be skipped once the caller is cancelled), which is
     * harmless because `resetSession` already emptied the session.
     */
    @Test
    fun cancellationMidSendCompletesPersistAndTeardownAndNeverTakesFailurePath() = runTest {
        val order = mutableListOf<String>()
        val sendStarted = CompletableDeferred<Unit>()
        val releaseSend = CompletableDeferred<Unit>()
        var failureCalled = false
        var propagated: Throwable? = null

        // A scope INDEPENDENT of the test scheduler so cancelling it models the
        // rotation tearing down `rememberCoroutineScope`, not the test's own job.
        val scope = CoroutineScope(Job() + Dispatchers.Default)
        val job = scope.launch {
            try {
                runSaveCriticalSection(
                    send = {
                        sendStarted.complete(Unit)
                        releaseSend.await() // suspended, like a slow JNI call
                        order.add("send")
                    },
                    clearSidecar = { order.add("clear") },
                    resetSession = { order.add("reset") },
                    finish = { order.add("finish") },
                    onFailure = { failureCalled = true },
                )
            } catch (t: Throwable) {
                propagated = t
                throw t
            }
        }

        sendStarted.await()
        job.cancel() // rotation cancels the calling scope mid-save
        releaseSend.complete(Unit) // the non-cancellable JNI send returns
        job.join()

        assertEquals(listOf("send", "clear", "reset"), order, "persist + teardown all ran, in order")
        // The regression itself: a cancellation must NEVER be treated as a failed
        // save (that re-shows Save/Discard for a logged run → duplicate / phantom).
        assertFalse(failureCalled, "cancellation is not a save failure")
        assertTrue(propagated is CancellationException, "cancellation re-thrown, not swallowed: $propagated")
    }

    /**
     * (b) A genuine persist failure (not a cancellation): [send] throws before the
     * clear/reset are reached (they are sequenced after it in the same block). The
     * failure path must fire so the captured run + its crash sidecar are KEPT for a
     * retry, and clear/reset/finish must NOT run, so nothing tears down a run
     * that was never persisted.
     */
    @Test
    fun sendFailureTakesFailurePathAndDoesNotTearDownTheSession() = runTest {
        val order = mutableListOf<String>()
        var failureCalled = false

        runSaveCriticalSection(
            send = {
                order.add("send")
                throw RuntimeException("JNI boom")
            },
            clearSidecar = { order.add("clear") },
            resetSession = { order.add("reset") },
            finish = { order.add("finish") },
            onFailure = { failureCalled = true },
        )

        assertTrue(failureCalled, "persist failure routes to the failure path")
        assertEquals(listOf("send"), order, "no teardown on a failed persist")
    }

    @Test
    fun happyPathRunsSendThenClearThenResetThenFinish() = runTest {
        val order = mutableListOf<String>()
        var failureCalled = false

        runSaveCriticalSection(
            send = { order.add("send") },
            clearSidecar = { order.add("clear") },
            resetSession = { order.add("reset") },
            finish = { order.add("finish") },
            onFailure = { failureCalled = true },
        )

        assertEquals(listOf("send", "clear", "reset", "finish"), order)
        assertFalse(failureCalled, "happy path never touches the failure path")
    }

    /**
     * (c) The save entered from a scope cancelled BEFORE it started: the callers
     * launch it with [CoroutineStart.ATOMIC] right after claiming the save, so a
     * rotation in that window still runs the body, on an already-cancelled job. The
     * persist and the teardown (whose reset releases the claim) must run anyway, and
     * the cancellation is no save failure; `finish` is best-effort-skipped.
     */
    @Test
    fun aSaveEnteredFromAnAlreadyCancelledScopeStillPersistsAndTearsDown() = runTest {
        val order = mutableListOf<String>()
        val scope = CoroutineScope(Job() + Dispatchers.Default)
        scope.cancel()
        val job = scope.launch(start = CoroutineStart.ATOMIC) {
            runSaveCriticalSection(
                send = { order.add("send") },
                clearSidecar = { order.add("clear") },
                resetSession = { order.add("reset") },
                finish = { order.add("finish") },
                onFailure = { order.add("failure") },
            )
        }
        job.join()
        assertEquals(listOf("send", "clear", "reset"), order, "persist + teardown ran, in order")
    }
}
