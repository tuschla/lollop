package app.milestone.widget

import android.content.Context
import androidx.compose.ui.unit.DpSize
import androidx.compose.ui.unit.dp
import androidx.glance.appwidget.testing.unit.runGlanceAppWidgetUnitTest
import androidx.glance.appwidget.testing.unit.hasStartActivityClickAction
import androidx.glance.testing.unit.hasTextEqualTo
import androidx.test.core.app.ApplicationProvider
import app.milestone.AppTheme
import app.milestone.DarkMode
import app.milestone.OpenDest
import app.milestone.R
import app.milestone.openIntent
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import kotlin.math.pow

/** The Glance widget tree per size: what each size shows and where a tap goes. */
@RunWith(RobolectricTestRunner::class)
class TodayWidgetContentTest {
    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()
    private val day = 20_389L

    private fun runDay() = widgetFace(
        WidgetSnapshot(
            epochDay = day,
            kind = WidgetDayKind.Run,
            title = "Easy run",
            items = listOf(WidgetItem(WidgetItemKind.Run, detail = "8 km easy")),
            trackable = true,
            weekDone = 2,
            weekPlanned = 4,
        ),
        day,
        ctx.resources,
    )

    private val colors get() = widgetColors(ctx, AppTheme.Paper, DarkMode.System, dynamic = false)

    /**
     * The step a host shows for a widget of [size]: of the steps that fit, the one
     * closest to [size]; the smallest when none fits. This is the platform's choice
     * (RemoteViews.findBestFitLayout, API 31+) and Glance's below 31 (findBestSize).
     */
    private fun hostStep(size: DpSize): DpSize =
        WidgetSizes.filter { it.width <= size.width && it.height <= size.height }
            .minByOrNull { (size.width - it.width).value.pow(2) + (size.height - it.height).value.pow(2) }
            ?: WidgetSizes.minBy { it.width.value * it.height.value }

    /** A default 3×2-cell widget on a 411 dp wide Android 14 phone (emulator, measured). */
    private val phoneDefault = DpSize(221.dp, 195.dp)
    private val smallest = DpSize(110.dp, 110.dp)

    @Test
    fun hostStepLaysOutLikeTheRealSize() {
        assertEquals(WidgetLayout(maxLines = 3, showWeek = true, maxActions = 2), widgetLayout(hostStep(phoneDefault)))
        assertEquals(WidgetLayout(maxLines = 1, showWeek = false, maxActions = 2), widgetLayout(hostStep(DpSize(250.dp, 110.dp))))
        assertEquals(WidgetLayout(maxLines = 1, showWeek = false, maxActions = 1), widgetLayout(hostStep(smallest)))
        assertEquals(WidgetLayout(maxLines = 6, showWeek = true, maxActions = 2), widgetLayout(hostStep(DpSize(400.dp, 420.dp))))
        // A host reporting less than the minimum still gets the smallest step.
        assertEquals(smallest, hostStep(DpSize(100.dp, 90.dp)))
        // Every size from the minimum up lays out as its real size would.
        for (width in 110..500 step 5) {
            for (height in 110..500 step 5) {
                val size = DpSize(width.dp, height.dp)
                assertEquals("$size", widgetLayout(size), widgetLayout(hostStep(size)))
            }
        }
    }

    @Test
    fun phoneDefaultRunDayShowsItemsWeekAndBothButtons() = runGlanceAppWidgetUnitTest {
        setContext(ctx)
        val step = hostStep(phoneDefault)
        setAppWidgetSize(step)
        provideComposable { TodayWidgetContent(runDay(), colors, widgetLayout(step)) }
        onNode(hasTextEqualTo("Easy run")).assertExists()
        onNode(hasTextEqualTo("Run · 8 km easy")).assertExists()
        onNode(hasTextEqualTo("2 of 4 sessions done")).assertExists()
        onNode(hasTextEqualTo("Track run")).assertExists()
        onNode(hasTextEqualTo("Log")).assertExists()
        // Every tap target opens MainActivity through the OPEN contract: the widget
        // itself and both buttons (the matcher compares intents without extras; the
        // destinations per button are pinned in WidgetSnapshotTest).
        onAllNodes(hasStartActivityClickAction(openIntent(ctx, OpenDest.Today))).assertCountEquals(3)
    }

    @Test
    fun smallRunDayKeepsTitleAndTrackRunOnly() = runGlanceAppWidgetUnitTest {
        setContext(ctx)
        setAppWidgetSize(smallest)
        provideComposable { TodayWidgetContent(runDay(), colors, widgetLayout(smallest)) }
        onNode(hasTextEqualTo("Easy run")).assertExists()
        onNode(hasTextEqualTo("Track run")).assertExists()
        onNode(hasTextEqualTo("Log")).assertDoesNotExist()
        onNode(hasTextEqualTo("2 of 4 sessions done")).assertDoesNotExist()
        onAllNodes(hasStartActivityClickAction(openIntent(ctx, OpenDest.Today))).assertCountEquals(2)
    }

    @Test
    fun staleSnapshotRendersOpenToUpdate() = runGlanceAppWidgetUnitTest {
        setContext(ctx)
        setAppWidgetSize(phoneDefault)
        val stale = WidgetSnapshot(epochDay = day, stale = true, kind = WidgetDayKind.Run, title = "Easy run", trackable = true)
        provideComposable { TodayWidgetContent(widgetFace(stale, day, ctx.resources), colors, widgetLayout(phoneDefault)) }
        onNode(hasTextEqualTo(ctx.getString(R.string.widget_open_to_update))).assertExists()
        onNode(hasTextEqualTo("Easy run")).assertDoesNotExist()
        onNode(hasTextEqualTo("Track run")).assertDoesNotExist()
        onNode(hasTextEqualTo("Log")).assertExists()
    }
}
