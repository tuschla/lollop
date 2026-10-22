package app.milestone

import java.util.Collections
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.DelicateCoroutinesApi
import kotlinx.coroutines.cancel
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.launch
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * [runSaveCriticalSection] must persist the run and tear down the session as
 * ONE unit that survives the caller's scope being cancelled mid-save, a rotation
 * cancels `rememberCoroutineScope` while the non-cancellable JNI `Core.send` is in
 * flight. The old bug: rotation cancels the job AFTER `Core.send` already appended
 * the run, `withContext` rethrows `CancellationException`, clear/reset are skipped,
 * and the `catch (Exception)` failure path re-shows Save/Discard for an
 * already-logged run → duplicate on Save-retry / phantom row on Discard.
 *
 * These tests drive a REAL coroutine cancellation against the extracted,
 * effect-injected function, the thing the on-device UI harness can't reproduce,
 * which is why it shipped without a guard until this refactor-for-testability.
 */
@OptIn(DelicateCoroutinesApi::class)
class RunSaveCriticalSectionTest {

    /**
     * (a) The cancellation scenario: the calling scope is cancelled while [send] (the JNI
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
        // The regression itself: a cancellation must NEVER be treated as a failed
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
     * retry, and clear/reset/finish must NOT run, so nothing tears down a run
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

    /**
     * (d) The save/discard entry race. Two Save entries arrive before the
     * first send resolves (a double-tap, or a Save racing the auto-log-on-stop). The
     * atomic [claimSaving] CAS admits exactly ONE, so only one `Core.send` runs; the
     * loser bails before launching. Mirrors the real seam logCaptured/stopAndLog use:
     * claim the flag, and only the winner launches [runSaveCriticalSection].
     */
    @Test
    fun concurrentSaveEntriesRunExactlyOneSend() = runTest {
        val saving = MutableStateFlow(false)
        val sends = AtomicInteger(0)
        val sendStarted = CompletableDeferred<Unit>()
        val releaseSend = CompletableDeferred<Unit>()
        // A scope INDEPENDENT of the test scheduler: the winning send suspends on real
        // deferreds while the second entry is attempted, exactly the double-tap window.
        val scope = CoroutineScope(Job() + Dispatchers.Default)

        fun enter(): Boolean {
            if (!claimSaving(saving)) return false
            scope.launch {
                runSaveCriticalSection<Unit>(
                    send = {
                        sendStarted.complete(Unit)
                        releaseSend.await()
                        sends.incrementAndGet()
                    },
                    clearSidecar = {},
                    resetSession = { saving.value = false }, // reset() releases the claim
                    finish = {},
                    onFailure = { saving.value = false },
                )
            }
            return true
        }

        assertTrue("first entry wins the claim", enter())
        sendStarted.await()                                 // the winner's send is in flight
        assertFalse("second entry during the in-flight save is rejected", enter())
        releaseSend.complete(Unit)                          // let the persist finish
        scope.coroutineContext[Job]!!.children.forEach { it.join() }
        assertEquals("exactly one send despite two entries", 1, sends.get())
        assertFalse("flag released after the save resolved", saving.value)
    }

    /**
     * (e) The stopAndLog → logCaptured ownership handoff. stopAndLog CASes
     * the flag true, then calls logCaptured(ownsSaving = true); logCaptured must NOT
     * re-CAS (which would fail on the already-true flag and drop the save). A
     * non-owning entry against an already-claimed flag still loses.
     */
    @Test
    fun ownsSavingHandoffSkipsTheClaimAndNonOwnerStillLoses() {
        val saving = MutableStateFlow(false)
        assertTrue("stopAndLog claims the flag", claimSaving(saving))
        assertTrue(
            "logCaptured(ownsSaving = true) proceeds without re-CAS",
            claimSaving(saving, ownsSaving = true),
        )
        assertFalse("a non-owning entry against a held flag loses", claimSaving(saving, ownsSaving = false))
        saving.value = false // reset() releases
        assertTrue("released flag can be re-claimed", claimSaving(saving))
    }

    /**
     * (f) The claim→NonCancellable-send handoff must be ATOMIC. logCaptured CAS-claims
     * the process-scoped `saving` flag, then `scope.launch`es the critical section. If
     * a rotation cancels that scope in the window BETWEEN the claim and the coroutine
     * entering the NonCancellable send, a DEFAULT-start launch creates the coroutine
     * already-cancelled and NEVER runs its body → `saving` stays latched forever and
     * the recomposed Save/Discard row is stuck disabled (the run is dead-ended).
     * CoroutineStart.ATOMIC guarantees the body starts and reaches the NonCancellable
     * persist + teardown, whose resetSession releases the claim.
     */
    @Test
    fun atomicStartCompletesTheSaveAndReleasesTheClaimWhenCancelledBeforeSend() = runTest {
        val saving = MutableStateFlow(false)
        val order = Collections.synchronizedList(mutableListOf<String>())
        assertTrue("claim won", claimSaving(saving))
        // A scope already cancelled BEFORE the critical section is launched: the exact
        // rotation-in-the-handoff-window the ATOMIC start defends against.
        val scope = CoroutineScope(Job() + Dispatchers.Default)
        scope.cancel()
        val job = scope.launch(start = CoroutineStart.ATOMIC) {
            runSaveCriticalSection<Unit>(
                send = { order.add("send") },
                clearSidecar = { order.add("clear") },
                resetSession = {
                    saving.value = false // reset() releases the claim
                    order.add("reset")
                },
                finish = { order.add("finish") },
                onFailure = {
                    saving.value = false
                    order.add("failure")
                },
            )
        }
        job.join()
        // The persist + teardown ran despite the pre-cancelled scope; the failure path
        // was never taken (a cancellation is not a save failure), and `finish` (its own
        // NonCancellable block) is best-effort-skipped once the caller is cancelled.
        assertEquals("persist + teardown ran, in order", listOf("send", "clear", "reset"), order)
        assertFalse("the saving claim is released, not orphaned", saving.value)
    }
}
