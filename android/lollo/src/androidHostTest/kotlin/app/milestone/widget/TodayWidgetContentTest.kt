package app.milestone.widget

import android.content.Context
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.DpSize
import androidx.compose.ui.unit.dp
import androidx.glance.appwidget.testing.unit.hasStartActivityClickAction
import androidx.glance.appwidget.testing.unit.runGlanceAppWidgetUnitTest
import androidx.glance.color.DayNightColorProvider
import androidx.glance.testing.unit.hasContentDescriptionEqualTo
import androidx.glance.testing.unit.hasTextEqualTo
import androidx.glance.unit.ColorProvider
import androidx.test.core.app.ApplicationProvider
import app.milestone.AppTheme
import app.milestone.DarkMode
import app.milestone.HoldReason
import app.milestone.OpenDest
import app.milestone.Palette
import app.milestone.R
import app.milestone.Space
import app.milestone.Type
import app.milestone.accentTint
import app.milestone.openIntent
import app.milestone.withDynamicAccent
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import kotlin.math.pow

/**
 * The widget per size: which layout each launcher size gets (one row, the stacked
 * card, the week strip, one or two actions), that a larger font drops parts
 * rather than clipping them, what the Glance tree shows and where a tap goes,
 * and that the colours are the app's theme.
 */
@RunWith(RobolectricTestRunner::class)
class TodayWidgetContentTest {
    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()
    private val res get() = ctx.resources
    private val day = WidgetFixtures.DAY

    @After
    fun resetFont() = RuntimeEnvironment.setFontScale(1f)

    private val week = WidgetFixtures.week

    /** "Heavy day": four open lifts, Back Squat first. */
    private fun liftDay() = widgetFace(WidgetFixtures.states(res).getValue("lift").snapshot, day, res)

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

    // Launcher cells (Pixel launcher, measured; `WidgetSizes`): four columns on a
    // 1080 × 2400 phone (Android 17 emulator)…
    private val cells2x1 = DpSize(179.dp, 95.dp)
    private val cells3x1 = DpSize(276.dp, 95.dp)
    private val cells4x1 = DpSize(373.dp, 95.dp)
    private val cells2x2 = DpSize(179.dp, 202.dp)
    private val cells3x2 = DpSize(276.dp, 202.dp)
    private val cells4x2 = DpSize(373.dp, 202.dp)
    private val cells2x3 = DpSize(179.dp, 308.dp)
    private val cells4x3 = DpSize(373.dp, 308.dp)

    // …and five columns on a 1080 × 1920 @ 420 dpi phone (Android 14 emulator).
    private val five2x1 = DpSize(144.dp, 89.dp)
    private val five3x1 = DpSize(221.dp, 89.dp)
    private val five4x1 = DpSize(296.dp, 89.dp)
    private val five2x2 = DpSize(144.dp, 193.dp)
    private val five3x2 = DpSize(221.dp, 193.dp)
    private val five4x2 = DpSize(296.dp, 193.dp)
    private val five2x3 = DpSize(144.dp, 296.dp)
    private val five4x3 = DpSize(296.dp, 296.dp)

    private fun layoutAt(cells: DpSize, fontScale: Float = 1f, face: WidgetFace = liftDay()) =
        widgetLayout(hostStep(cells), fontScale, face)

    @Test
    fun stepsStayWithinThePlatformLimitAndTallWidgetsStack() {
        assertTrue("${WidgetSizes.size} steps", WidgetSizes.size <= 16)
        // Whatever the width, a widget two rows tall or more picks a stacked step,
        // never a one-row step closer in width (the host's squared-distance choice).
        for (w in 130..520 step 5) for (h in 140..520 step 5) {
            assertEquals("${w}x$h", WidgetShape.Stack, layoutAt(DpSize(w.dp, h.dp)).shape)
        }
        for (w in 130..520 step 5) for (h in 60..130 step 5) {
            assertEquals("${w}x$h", WidgetShape.Line, layoutAt(DpSize(w.dp, h.dp)).shape)
        }
    }

    /**
     * e34 (five columns, Android 14): the default 3×2 (221 × 193 dp) got the
     * 170 × 140 step, ~50 dp left empty, no date line, and below 240 dp one
     * button (no Log). Every cell of either grid gets a step at most 20 dp
     * shorter, so its layout fills it.
     */
    @Test
    fun everyLauncherCellGetsTheStepJustUnderIt() {
        val ones = listOf(cells2x1, cells3x1, cells4x1, five2x1, five3x1, five4x1)
        val tall = listOf(cells2x2, cells3x2, cells4x2, cells2x3, cells4x3, five2x2, five3x2, five4x2, five2x3, five4x3)
        // (A one-row step is 72 dp: the row centres its text in the launcher's row.)
        for (c in tall) assertTrue("$c gets ${hostStep(c)}", c.height - hostStep(c).height <= 20.dp)
        for (c in ones) assertEquals("$c", 72.dp, hostStep(c).height)
        // Two rows and more: the card with its date line.
        for (c in tall) layoutAt(c).let {
            assertEquals("$c", WidgetShape.Stack, it.shape)
            assertTrue("$c date line", it.overline)
        }
        // From three cells: "Today ·" in the overline and Log beside the primary.
        for (c in listOf(cells3x2, cells4x2, cells4x3, five3x2, five4x2, five4x3)) for (face in listOf(liftDay(), runDay())) {
            layoutAt(c, face = face).let {
                assertTrue("${face.title} $c", it.todayOverline)
                assertEquals("${face.title} $c", 2, it.maxActions)
            }
        }
        // One row from three cells on four columns, four on five: Log too, with the
        // title and its status line.
        for (c in listOf(cells3x1, cells4x1, five4x1)) for (face in listOf(liftDay(), runDay())) {
            layoutAt(c, face = face).let {
                assertEquals("${face.title} $c", 2, it.maxActions)
                assertTrue("${face.title} $c", it.status != null)
            }
        }
        // A label too long for the row beside Log ("Run · recording" in 200 dp) stands alone.
        assertEquals(1, layoutAt(five3x2, face = recording()).maxActions)
        assertEquals(2, layoutAt(cells3x2, face = recording()).maxActions)
        // Two cells on five columns (145 dp): the row keeps Space.Md at its sides, so a
        // title word fits beside the button ("Trai / ning p…" with Space.Screen, e34).
        assertEquals(Space.Md.dp, layoutAt(five2x1, face = hold()).edge)
        assertEquals(Space.Screen.dp, layoutAt(cells2x1, face = hold()).edge)
    }

    @Test
    fun eachLauncherSizeGetsItsOwnLayout() {
        // 2×1: one row, the title over the next set, one round action.
        layoutAt(cells2x1).let {
            assertEquals(WidgetShape.Line, it.shape)
            assertTrue(it.statusLines > 0)
            assertEquals(1, it.maxActions)
            assertFalse("no room for the label", it.labelled)
        }
        // 4×1: a status line that fits beside both actions keeps the primary labelled.
        layoutAt(cells4x1, face = runDay()).let {
            assertEquals(WidgetShape.Line, it.shape)
            assertTrue(it.labelled)
            assertEquals(2, it.maxActions)
        }
        // 2×2: the stacked card, date overline, one full-width action, no strip.
        layoutAt(cells2x2).let {
            assertEquals(WidgetShape.Stack, it.shape)
            assertTrue(it.overline)
            assertFalse(it.wide)
            assertEquals(1, it.maxActions)
            assertFalse(it.week)
        }
        for (c in listOf(cells3x2, cells4x2)) layoutAt(c).let {
            assertEquals(WidgetShape.Stack, it.shape)
            assertEquals(2, it.maxActions)
            assertFalse(it.week)
        }
        // Three rows: the week block (count over the day strip). Two cells wide, the
        // strip's labels do not fit their columns: the count alone.
        layoutAt(cells2x3).let { assertTrue(it.week); assertFalse(it.weekDays); assertEquals(1, it.maxActions) }
        layoutAt(cells4x3).let { assertTrue(it.week); assertTrue(it.weekDays); assertEquals(2, it.maxActions) }
        assertFalse(layoutAt(cells4x3, face = liftDay().copy(week = null, days = emptyList())).week)
        // Every size shows more as it grows: body lines never shrink with height.
        val lines = listOf(cells2x2, cells2x3).map { bodyLines(liftDay(), layoutAt(it), res).size }
        assertTrue("$lines", lines[1] >= lines[0] && lines[0] >= 2)
    }

    private val next = WidgetNext(day + 2, "Heavy volume day")
    private val ahead = listOf(
        WidgetAheadDay(
            day + 2,
            WidgetDayKind.Lift,
            "Heavy volume day",
            listOf(WidgetItem(WidgetItemKind.Lift, "Back squat", "4×5 @\u00A0112.5\u00A0kg"), WidgetItem(WidgetItemKind.Lift, "Deadlift", "4×5 @\u00A057.5\u00A0kg")),
        ),
    )
    private val runItem = WidgetItem(WidgetItemKind.Run, detail = "3.4\u00A0mi", pace = "9:00–9:40\u00A0/mi", figure = true)

    private fun runDay() = widgetFace(
        WidgetSnapshot(epochDay = day, kind = WidgetDayKind.Run, title = "Easy run", items = listOf(runItem), trackable = true, weekDone = 1, weekPlanned = 4, week = week),
        day,
        res,
    )

    private fun recording() = widgetFace(
        WidgetSnapshot(epochDay = day, kind = WidgetDayKind.Run, title = "Easy run", items = listOf(runItem), trackable = true, weekDone = 1, weekPlanned = 4, week = week),
        day,
        res,
        recording = true,
    )

    private fun restDay() = widgetFace(
        WidgetSnapshot(epochDay = day, kind = WidgetDayKind.Rest, restDay = true, next = next, ahead = ahead, weekDone = 1, weekPlanned = 4, week = week),
        day,
        res,
    )

    private fun doneDay() = widgetFace(
        WidgetSnapshot(
            epochDay = day, kind = WidgetDayKind.Run, title = "Easy run", items = listOf(runItem.copy(done = true)), sessionDone = true,
            next = next, ahead = ahead, weekDone = 2, weekPlanned = 4, week = week,
        ),
        day,
        res,
    )

    @Test
    fun tallSizesComposeTheirContentInsteadOfLeavingAGap() {
        val tall = listOf(cells2x2, cells3x2, cells4x2, cells2x3, cells4x3)
        val fiveTall = listOf(five2x2, five3x2, five4x2, five2x3, five4x3)
        // A run day: the distance is the hero figure, over its pace, at every stacked
        // size of four columns and three rows of five.
        assertEquals(WidgetHero("3.4\u00A0mi", "9:00–9:40\u00A0/mi", figure = true), runDay().hero)
        for (c in tall + five2x3 + five4x3) assertTrue("run $c", layoutAt(c, face = runDay()).hero)
        // Two rows of five (193 dp) hold the date line or the hero with its pace, not
        // both: the date line, then the run as its line.
        for (c in listOf(five2x2, five3x2, five4x2)) layoutAt(c, face = runDay()).let {
            assertFalse("run $c", it.hero)
            assertTrue("run $c", bodyLines(runDay(), it, res).first().text.startsWith("Run"))
        }
        // A run without a pace target whose detail is only a figure: the figure alone.
        val noPace = widgetFace(
            WidgetSnapshot(epochDay = day, kind = WidgetDayKind.Run, title = "Easy run", items = listOf(runItem.copy(pace = "")), trackable = true, weekDone = 1, weekPlanned = 4, week = week),
            day,
            res,
        )
        assertEquals(WidgetHero("3.4\u00A0mi", null, figure = true), noPace.hero)
        for (c in tall + fiveTall) assertTrue("run without pace $c", layoutAt(c, face = noPace).hero)
        // The 2×1 of a finished day says so ("Easy run" stood alone on the device).
        layoutAt(cells2x1, face = doneDay()).let { assertEquals(res.getString(R.string.widget_session_done), it.status) }
        // A rest day: the next session, its date and its items, from three cells wide.
        assertEquals("Heavy volume day", restDay().hero?.text)
        assertEquals(listOf("Back squat · 4×5 @\u00A0112.5\u00A0kg", "Deadlift · 4×5 @\u00A057.5\u00A0kg"), restDay().heroItems)
        for (c in listOf(cells3x2, cells4x2, cells2x3, cells4x3, five3x2, five4x2, five2x3, five4x3)) {
            val l = layoutAt(c, face = restDay())
            assertTrue("rest $c", l.hero)
            // Its items where they fit beside it (not in two rows of five, nor two narrow cells).
            if (c in listOf(cells3x2, cells4x2, cells2x3, cells4x3, five4x3)) {
                assertTrue("rest $c: an item of the next session", bodyLines(restDay(), l, res).isNotEmpty())
            }
        }
        layoutAt(cells4x3, face = doneDay()).let {
            assertTrue(it.hero)
            assertEquals(res.getString(R.string.widget_session_done), bodyLines(doneDay(), it, res).first().text)
        }
        // The week block wherever it fits beside the content: every tall size three rows high.
        for (face in listOf(runDay(), restDay(), doneDay(), liftDay())) for (c in listOf(cells2x3, cells4x3)) {
            assertTrue("${face.title} $c week", layoutAt(c, face = face).week)
        }
        // A hold on a wide card: the title set large on one line, its line under it.
        // Never two lines of it (a 2×3's "Training / paused" in large red read as an alarm).
        for (c in listOf(cells3x2, cells4x2, cells4x3, five4x2, five4x3)) assertTrue("hold $c", layoutAt(c, face = hold()).bigTitle)
        for (c in listOf(cells2x2, cells2x3, five2x2, five3x2, five2x3)) assertFalse("hold $c", layoutAt(c, face = hold()).bigTitle)
        // A wrapped item line keeps its prescription whole: never a line ending on "@"
        // nor one starting with it ("Back squat · 5×3" / "@ 117.5 kg" on a 2×2, e37).
        assertEquals("Back Squat · 5×3 @\u00A0117.5\u00A0kg", liftDay().items.first())
        assertFalse(layoutAt(cells4x3).bigTitle)
    }

    private fun hold() = widgetFace(WidgetSnapshot(epochDay = day, kind = WidgetDayKind.Paused, holdReason = HoldReason.Pain), day, res)

    @Test
    fun largerFontDropsPartsInsteadOfClipping() {
        // The 2×1: a title that wraps keeps both lines ("Training paused"), the status
        // takes what is left; at 2.0 only the title fits beside the action.
        assertEquals(2, layoutAt(cells2x1, face = hold()).titleLines)
        assertTrue(layoutAt(cells2x1).statusLines > 0)
        assertEquals(0, layoutAt(cells2x1, 2f).statusLines)
        // A 3×1 at 1.3: "Log set" labelled left "Heavy day" alone (e37); the primary
        // goes round so the title keeps its line.
        for (face in listOf(liftDay(), runDay(), doneDay())) {
            layoutAt(DpSize(276.dp, 95.dp), 1.3f, face).let { assertTrue("${face.title}: ${it.status}", it.status != null) }
        }
        // The 2×2 at 1.3: the wrapped title keeps its second line before the overline
        // ("Training pau…" was cut while the overline kept its row).
        layoutAt(cells2x2, 1.3f, hold()).let { assertEquals(2, it.titleLines); assertFalse(it.overline) }
        // …and its line stays whole ("Open lollo for …" was cut to one line).
        assertEquals(BodyLine(res.getString(R.string.widget_paused_body), 2), bodyLines(hold(), layoutAt(cells2x2, 1.3f, hold()), res).single())
        // A rest day's next-session line too long for two lines gives way to the short one.
        val nextDay = java.time.LocalDate.ofEpochDay(day + 2).dayOfWeek.getDisplayName(java.time.format.TextStyle.SHORT, java.util.Locale.US)
        assertEquals(listOf(res.getString(R.string.widget_next_day, nextDay)), bodyLines(restDay(), layoutAt(cells2x2, 1.3f, restDay()), res).map { it.text })
        // Two cells wide at 1.3: no strip of cut weekday labels, the count alone.
        assertFalse(layoutAt(cells2x3, 1.3f).weekDays)
        assertTrue(layoutAt(cells4x3, 1.3f).weekDays)
        val at1 = bodyLines(liftDay(), layoutAt(cells2x2, 1f), res)
        val at13 = bodyLines(liftDay(), layoutAt(cells2x2, 1.3f), res)
        assertTrue("$at1 / $at13", at13.sumOf { it.maxLines } <= at1.sumOf { it.maxLines })
        // One line left for a list shows its first item, never "4 more items" alone.
        assertTrue("$at13", at13.none { it.text == res.getQuantityString(R.plurals.widget_more_items, 4, 4) })
        // On every step and font scale the parts fit the height: the one row's title
        // and status within the row, body lines within the room the stacked card left
        // for them, and a stacked card always has room for a line. A text of n lines
        // is TextView's Roboto font padding once plus n lines of ascent to descent.
        fun h(sp: Float, scale: Float, lines: Int) = (sp * scale * (0.156f + 1.171f * lines)).dp
        for (face in listOf(liftDay(), hold(), runDay(), restDay(), doneDay())) for (step in WidgetSizes) for (scale in listOf(1f, 1.15f, 1.3f, 1.5f, 2f)) {
            val l = widgetLayout(step, scale, face)
            val at = "$step @$scale ${face.title}"
            if (l.shape == WidgetShape.Line) {
                val used = h(Type.BodyBold.fontSize.value, scale, l.titleLines) +
                    if (l.statusLines > 0) h(Type.Caption.fontSize.value, scale, l.statusLines) else 0.dp
                assertTrue("$at: $used > ${step.height}", used <= step.height - 8.dp)
            } else {
                val body = bodyLines(face, l, res)
                val used = body.fold(0.dp) { acc, b -> acc + h((if (b.large) Type.Body else Type.Caption).fontSize.value, scale, b.maxLines) }
                assertTrue("$at: $used > ${l.bodyHeight}", used <= l.bodyHeight)
                assertTrue("$at: no body line", l.hero || body.isNotEmpty())
            }
        }
    }

    @Test
    fun oneRowShowsTitleStatusAndTheTileAction() = runGlanceAppWidgetUnitTest {
        setContext(ctx)
        setAppWidgetSize(hostStep(cells2x1))
        provideComposable { TodayWidgetContent(liftDay(), colors) }
        onNode(hasTextEqualTo("Heavy day")).assertExists()
        onNode(hasTextEqualTo("Back Squat")).assertExists()
        // The tile's action as a round button, named for TalkBack; no Log at this size.
        onNode(hasContentDescriptionEqualTo(res.getString(R.string.tile_log_set))).assertExists()
        onNode(hasContentDescriptionEqualTo(res.getString(R.string.tile_log))).assertDoesNotExist()
        onNode(hasTextEqualTo("Deadlift · 3×5")).assertDoesNotExist()
        // The widget itself and the action open MainActivity through the OPEN contract.
        onAllNodes(hasStartActivityClickAction(openIntent(ctx, OpenDest.Today))).assertCountEquals(2)
    }

    @Test
    fun threeRowsShowItemsWeekStripAndBothActions() = runGlanceAppWidgetUnitTest {
        setContext(ctx)
        setAppWidgetSize(hostStep(cells4x3))
        provideComposable { TodayWidgetContent(liftDay(), colors) }
        onNode(hasTextEqualTo("Heavy day")).assertExists()
        onNode(hasTextEqualTo("Back Squat · 5×3 @\u00A0117.5\u00A0kg")).assertExists()
        onNode(hasTextEqualTo("Cable row · 3×5")).assertExists()
        onNode(hasTextEqualTo(res.getString(R.string.tile_log_set))).assertExists()
        onNode(hasContentDescriptionEqualTo(res.getString(R.string.tile_log))).assertExists()
        onNode(hasTextEqualTo("1 of 4 sessions done")).assertExists()
        // The strip, Monday to Sunday, spoken as the week count.
        onNode(hasContentDescriptionEqualTo("1 of 4 sessions done")).assertExists()
        onNode(hasTextEqualTo("\u2713")).assertExists()
        onAllNodes(hasTextEqualTo("\u25CF")).assertCountEquals(1)
        onAllNodes(hasTextEqualTo("\u25CB")).assertCountEquals(2)
        onAllNodes(hasStartActivityClickAction(openIntent(ctx, OpenDest.Today))).assertCountEquals(3)
    }

    @Test
    fun staleSnapshotRendersOpenToUpdateWithLogOnly() = runGlanceAppWidgetUnitTest {
        setContext(ctx)
        setAppWidgetSize(hostStep(cells3x2))
        val stale = WidgetSnapshot(epochDay = day, stale = true, kind = WidgetDayKind.Run, title = "Easy run", trackable = true, week = week)
        provideComposable { TodayWidgetContent(widgetFace(stale, day, res), colors) }
        onNode(hasTextEqualTo(res.getString(R.string.widget_open_to_update))).assertExists()
        onNode(hasTextEqualTo("Easy run")).assertDoesNotExist()
        onNode(hasTextEqualTo(res.getString(R.string.tile_start_run))).assertDoesNotExist()
        onNode(hasTextEqualTo(res.getString(R.string.tile_log))).assertExists()
        onNode(hasTextEqualTo("\u2713")).assertDoesNotExist()
    }

    private fun ColorProvider.at(night: Boolean): Color = (this as? DayNightColorProvider)?.getColor(night) ?: getColor(ctx)

    private fun assertPalette(what: String, p: Palette, c: WidgetColors, night: Boolean) {
        assertEquals("$what ground", p.bgTop, c.ground.at(night))
        assertEquals("$what body", p.onBgBody, c.body.at(night))
        assertEquals("$what muted", p.onBgMuted, c.muted.at(night))
        assertEquals("$what faint", p.onBgFaint, c.faint.at(night))
        assertEquals("$what accent", p.accent, c.accent.at(night))
        assertEquals("$what on accent", p.bgTop, c.onAccent.at(night))
        assertEquals("$what accent tint", p.accentTint(), c.accentTint.at(night))
        assertEquals("$what danger", p.dangerText, c[WidgetInk.Danger].at(night))
    }

    @Test
    fun coloursAreTheAppThemeInEveryModeAndPalette() {
        for (theme in AppTheme.entries) {
            // System: the day and night palettes, chosen by the host's mode.
            val system = widgetColors(ctx, theme, DarkMode.System, dynamic = false)
            assertPalette("$theme day", theme.light, system, night = false)
            assertPalette("$theme night", theme.dark, system, night = true)
            // A forced mode is that palette whatever the system says.
            assertPalette("$theme light", theme.light, widgetColors(ctx, theme, DarkMode.Light, dynamic = false), night = true)
            assertPalette("$theme dark", theme.dark, widgetColors(ctx, theme, DarkMode.Dark, dynamic = false), night = false)
        }
    }

    @Test
    fun systemAccentOnlyWhereTheAppUsesIt() {
        // Paper with the system accent (the default): the wallpaper accent through the
        // app's contrast guard, the Paper grounds and text.
        val paper = widgetColors(ctx, AppTheme.Paper, DarkMode.System, dynamic = true)
        assertPalette("paper day", AppTheme.Paper.light.withDynamicAccent(dynamicLightColorScheme(ctx).primary), paper, night = false)
        assertPalette("paper night", AppTheme.Paper.dark.withDynamicAccent(dynamicDarkColorScheme(ctx).primary), paper, night = true)
        assertEquals(AppTheme.Rose.light.accent, widgetColors(ctx, AppTheme.Rose, DarkMode.Light, dynamic = false).accent.at(false))
    }
}
