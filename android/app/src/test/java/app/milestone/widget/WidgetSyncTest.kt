package app.milestone.widget

import android.content.Context
import android.os.Looper
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.test.core.app.ApplicationProvider
import app.milestone.EventDispatcher
import app.milestone.qa.QaHarness
import app.milestone.qa.lastScenario
import app.milestone.qa.launchMain
import java.io.File
import java.util.Locale
import java.util.TimeZone
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf

/** [WidgetSync] with the real app running: what replaces a stale snapshot. */
@RunWith(RobolectricTestRunner::class)
class WidgetSyncTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @Before
    fun setUp() {
        Locale.setDefault(Locale.US)
        QaHarness.reset(ctx)
        File(ctx.filesDir, WidgetSnapshotStore.FILE_NAME).delete()
    }

    @After
    fun tearDown() {
        TimeZone.setDefault(TimeZone.getTimeZone("UTC"))
        QaHarness.teardown()
    }

    /**
     * A time zone change that keeps the local day publishes no new model (the
     * resume SetToday leaves the view as it was), so the stale mark itself must
     * bring a fresh snapshot in the new zone while the app process holds a model:
     * else the widget, the tile and the session reminder stay on the stale one
     * until a cold start.
     */
    @Test
    fun aSameDayTimeZoneChangeRewritesTheStaleSnapshotFromTheModel() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        lastScenario = launchMain()
        rule.waitUntil(30_000) { EventDispatcher.bootHandled && EventDispatcher.model.value != null }
        rule.waitForIdle()
        rule.waitUntil(10_000) { WidgetSnapshotStore.read(ctx)?.zone == "UTC" }
        val before = checkNotNull(WidgetSnapshotStore.read(ctx))
        val model = EventDispatcher.model.value

        TimeZone.setDefault(TimeZone.getTimeZone("Europe/London"))
        assertTrue(WidgetSnapshotStore.markStale(ctx, force = true, today = before.epochDay))
        waitMain("a fresh snapshot") { WidgetSnapshotStore.read(ctx)?.stale == false }

        assertSame("no new model was published", model, EventDispatcher.model.value)
        assertEquals(before.copy(zone = "Europe/London"), WidgetSnapshotStore.read(ctx))
        assertEquals(before.copy(zone = "Europe/London"), WidgetSnapshotStore.today(ctx, before.epochDay))
    }

    /** Run the main looper (where [WidgetSync] collects) until [done]. */
    private fun waitMain(what: String, timeoutMs: Long = 10_000, done: () -> Boolean) {
        val until = System.currentTimeMillis() + timeoutMs
        while (!done()) {
            check(System.currentTimeMillis() < until) { "timed out waiting for $what: ${WidgetSnapshotStore.read(ctx)}" }
            shadowOf(Looper.getMainLooper()).idle()
            Thread.sleep(20)
        }
    }
}
