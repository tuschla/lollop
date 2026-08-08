package app.milestone

import java.util.Collections
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * M1: [runSaveCriticalSection] must persist the run and tear down the session as
 * ONE unit that survives the caller's scope being cancelled mid-save, a rotation
 * cancels `rememberCoroutineScope` while the non-cancellable JNI `Core.send` is in
 * flight. The old bug: rotation cancels the job AFTER `Core.send` already appended
 * the run, `withContext` rethrows `CancellationException`, clear/reset are skipped,
 * and the `catch (Exception)` failure path re-shows Save/Discard for an
 * already-logged run → duplicate on Save-retry / phantom row on Discard.
 *
 * These tests drive a REAL coroutine cancellation against the extracted,
 * effect-injected function, the thing the on-device UI harness can't reproduce,
 * which is why M1 shipped without a guard until this refactor-for-testability.
 */
class RunSaveCriticalSectionTest {

    /**
     * (a) The M1 scenario: the calling scope is cancelled while [send] (the JNI
     * persist) is still suspended. The persist must still complete, and the sidecar
     * clear + session reset, sequenced inside the same `NonCancellable` block, must
     * BOTH run so a rotation can't strand an already-persisted run. Crucially the
     * cancellation must NOT be routed to the failure path (that is the double-log
     * bug); it re-throws instead. The `finish` navigation is deliberately best-effort
     * (its `NonCancellable` block may be skipped once the caller is cancelled) -
     * harmless, because `resetSession` already emptied the session.
     */
    @Test
    fun cancellationMidSendCompletesPersistAndTeardownAndNeverTakesFailurePath() = runTest {
        val order = Collections.synchronizedList(mutableListOf<String>())
        val sendStarted = CompletableDeferred<Unit>()
        val releaseSend = CompletableDeferred<Unit>()
        var failureCalled = false
        var propagated: Throwable? = null

        // A scope INDEPENDENT of the test scheduler so cancelling it models the
        // rotation tearing down `rememberCoroutineScope`, not the test's own job.
        val scope = CoroutineScope(Job() + Dispatchers.Default)
        val job = scope.launch {
            try {
                runSaveCriticalSection<String>(
                    send = {
                        sendStarted.complete(Unit) // signal: persist is in flight
                        releaseSend.await()        // ...and suspended, like a slow JNI call
                        order.add("send")
                        "vm"
                    },
                    clearSidecar = { order.add("clear") },
                    resetSession = { order.add("reset") },
                    finish = { vm -> order.add("finish:$vm") },
                    onFailure = { failureCalled = true },
                )
            } catch (t: Throwable) {
                propagated = t
                throw t
            }
        }

        sendStarted.await() // wait until the persist is genuinely suspended
        job.cancel()        // rotation cancels the calling scope mid-save
        releaseSend.complete(Unit) // the non-cancellable JNI send returns
        job.join()

        // Persist ran, THEN the sidecar clear, THEN the session reset: the whole
        // teardown completed despite the cancellation, and in order.
        assertEquals("persist + teardown all ran, in order", listOf("send", "clear", "reset"), order)
        // The M1 regression itself: a cancellation must NEVER be treated as a failed
        // save (that re-shows Save/Discard for a logged run → duplicate / phantom).
        assertFalse("cancellation is not a save failure", failureCalled)
        // It propagates as cancellation, is not swallowed.
        assertTrue(
            "cancellation re-thrown, not swallowed: ${propagated?.javaClass?.simpleName}",
            propagated is kotlinx.coroutines.CancellationException,
        )
    }

    /**
     * (b) A genuine persist failure (not a cancellation): [send] throws before the
     * clear/reset are reached (they are sequenced after it in the same block). The
     * failure path must fire so the captured run + its crash sidecar are KEPT for a
     * retry (C5), and clear/reset/finish must NOT run, so nothing tears down a run
     * that was never persisted.
     */
    @Test
    fun sendFailureTakesFailurePathAndDoesNotTearDownTheSession() = runTest {
        val order = Collections.synchronizedList(mutableListOf<String>())
        var failureCalled = false

        runSaveCriticalSection<String>(
            send = {
                order.add("send")
                throw RuntimeException("JNI boom")
            },
            clearSidecar = { order.add("clear") },
            resetSession = { order.add("reset") },
            finish = { vm -> order.add("finish:$vm") },
            onFailure = { failureCalled = true },
        )

        assertTrue("persist failure routes to the failure path", failureCalled)
        // The sidecar + session are untouched, so the captured run is still there to
        // retry: clear/reset/finish never ran.
        assertEquals("no teardown on a failed persist", listOf("send"), order)
    }

    /**
     * (c) Happy path (no cancellation): persist, clear, reset, then navigate, in
     * that exact order, and the failure path is never taken.
     */
    @Test
    fun happyPathRunsSendThenClearThenResetThenFinish() = runTest {
        val order = Collections.synchronizedList(mutableListOf<String>())
        var failureCalled = false

        runSaveCriticalSection<String>(
            send = {
                order.add("send")
                "vm"
            },
            clearSidecar = { order.add("clear") },
            resetSession = { order.add("reset") },
            finish = { vm -> order.add("finish:$vm") },
            onFailure = { failureCalled = true },
        )

        assertEquals(listOf("send", "clear", "reset", "finish:vm"), order)
        assertFalse("happy path never touches the failure path", failureCalled)
    }
}
