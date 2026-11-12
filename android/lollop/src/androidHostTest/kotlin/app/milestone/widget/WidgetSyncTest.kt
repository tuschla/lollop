package app.milestone.widget

import android.content.Context
import android.os.Looper
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.test.core.app.ApplicationProvider
import app.milestone.AppFileNames
import app.milestone.EventDispatcher
import app.milestone.JvmDefaultsRule
import app.milestone.qa.QaHarness
import app.milestone.qa.lastScenario
import app.milestone.qa.launchMain
import app.milestone.qa.waitForBoot
import java.io.File
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

/** [WidgetSurfaces] with the real app running: what replaces a stale snapshot. */
@RunWith(RobolectricTestRunner::class)
class WidgetSyncTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    @get:Rule
    val jvmDefaults = JvmDefaultsRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @Before
    fun setUp() {
        QaHarness.reset(ctx)
        File(ctx.filesDir, AppFileNames.WIDGET_SNAPSHOT).delete()
    }

    @After
    fun tearDown() {
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
        rule.waitForBoot()
        rule.waitUntil(10_000) { WidgetSnapshotStore.read(ctx)?.zone == "UTC" }
        val before = checkNotNull(WidgetSnapshotStore.read(ctx))
        val model = EventDispatcher.model.value

        TimeZone.setDefault(TimeZone.getTimeZone("Europe/London"))
        assertTrue(WidgetSnapshotStore.markStale(ctx, force = true, today = before.epochDay))
        QaHarness.pumpMainUntil("a fresh snapshot", 10_000) { WidgetSnapshotStore.read(ctx)?.stale == false }

        assertSame("no new model was published", model, EventDispatcher.model.value)
        assertEquals(before.copy(zone = "Europe/London"), WidgetSnapshotStore.read(ctx))
        assertEquals(before.copy(zone = "Europe/London"), WidgetSnapshotStore.today(ctx, before.epochDay))
    }

    /** The midnight tick marks yesterday's snapshot stale; the model the process
     *  holds is still yesterday's, so nothing rewrites it until the app sets the
     *  new day: the widget keeps "Open lollop to update". The mark gives
     *  [WidgetSurfaces] no [WidgetSnapshotStore.staleMarks] tick, its only cue to
     *  derive the snapshot again without a new model. */
    @Test
    fun theMidnightMarkLeavesYesterdaysSnapshotStale() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        lastScenario = launchMain()
        rule.waitForBoot()
        rule.waitUntil(10_000) { WidgetSnapshotStore.read(ctx) != null }
        val before = checkNotNull(WidgetSnapshotStore.read(ctx))
        val revision = WidgetSnapshotStore.revision.value
        val marks = WidgetSnapshotStore.staleMarks.value
        val model = EventDispatcher.model.value

        assertTrue(WidgetSnapshotStore.markStale(ctx, force = false, today = before.epochDay + 1))
        shadowOf(Looper.getMainLooper()).idle()

        assertEquals("no cue to derive it again", marks, WidgetSnapshotStore.staleMarks.value)
        assertSame("no new model was published", model, EventDispatcher.model.value)
        assertEquals(before.copy(stale = true), WidgetSnapshotStore.read(ctx))
        assertEquals("one write: the stale mark", revision + 1, WidgetSnapshotStore.revision.value)
    }
}
