package app.milestone.qa

import android.content.Context
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.test.core.app.ApplicationProvider
import app.milestone.Event
import app.milestone.EventDispatcher
import kotlinx.coroutines.awaitCancellation
import org.junit.After
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * The process-scoped [EventDispatcher] consumer outlives every test, but
 * Robolectric recycles the main looper's pending messages between tests. A test
 * that ends with a unit in flight (its resumption queued on main) leaves a
 * consumer that never resumes: the next test's boot would wait on it forever, the
 * splash cancelling every frame, `ActivityScenario.launch` spinning.
 *
 * One self-contained test, so it fails on its own whatever ran before it: a send
 * that never resumes stands in for the recycled resumption, then the per-test
 * reset ([QaHarness.reset]) runs as the next test's setup would, and that boot
 * must still come up.
 */
@RunWith(RobolectricTestRunner::class)
class DispatcherIsolationTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @Before
    fun setUp() {
        QaHarness.reset(ctx)
    }

    @After
    fun tearDown() = QaHarness.teardown()

    @Test
    fun aSendLeftInFlightDoesNotStallTheNextBoot() {
        var inFlight = false
        EventDispatcher.sender = { _ ->
            inFlight = true
            awaitCancellation()
        }
        EventDispatcher.send(Event.SetToday(app.milestone.todayEpochDay(), 0))
        assertTrue("the consumer took the send at once (main, immediate)", inFlight)

        // The next test's setup, then its launch.
        QaHarness.reset(ctx)
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
    }
}
