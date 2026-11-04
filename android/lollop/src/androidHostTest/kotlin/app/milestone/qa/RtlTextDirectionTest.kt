package app.milestone.qa

import androidx.activity.ComponentActivity
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performSemanticsAction
import app.milestone.Accent
import app.milestone.AppSettings
import app.milestone.GpxFix
import app.milestone.RunResultView
import app.milestone.RunRoute
import app.milestone.TrimRunSheetContent
import app.milestone.timedTrackFromSegments
import kotlin.math.abs
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Text
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.ResolvedTextDirection
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import app.milestone.DistanceUnit
import app.milestone.MilestoneTheme
import app.milestone.R
import app.milestone.RowChevron
import app.milestone.RunDistanceBars
import app.milestone.StatusCardFooter
import app.milestone.TabularFigures
import app.milestone.Type
import java.util.Locale
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * In an RTL locale (Arabic) the app keeps its mirrored layout, but its
 * English-only copy reads left to right: each paragraph's base direction is LTR,
 * so neutral characters at the edges stay in place ("5.5 km", a coach note's
 * final '.'). The mirrored layout itself is covered too: canvas-ordered chart
 * ticks, the chevron.
 */
@RunWith(RobolectricTestRunner::class)
@Config(qualifiers = "+ar")
class RtlTextDirectionTest {

    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    @Before
    fun arabic() = Locale.setDefault(Locale.forLanguageTag("ar"))

    @After
    fun restore() = Locale.setDefault(Locale.US)

    private val plan = "5.5 km"
    private val note = "Back Squat: today's top set beat your recent best. You can add about 4% next time."
    private val delta = "\u22122.5"

    /** Base direction LTR, every character in an LTR bidi run. */
    private fun assertLtr(what: String, layout: TextLayoutResult) {
        val text = layout.layoutInput.text.text
        assertEquals("$what '$text': paragraph direction", ResolvedTextDirection.Ltr, layout.getParagraphDirection(0))
        text.indices.forEach {
            assertEquals("$what '$text': bidi run at $it ('${text[it]}')", ResolvedTextDirection.Ltr, layout.getBidiRunDirection(it))
        }
    }

    /** Characters [a] then [b] of the logical text appear left to right on screen. */
    private fun assertLeftOf(layout: TextLayoutResult, a: Int, b: Int) {
        val text = layout.layoutInput.text.text
        val boxA = layout.getBoundingBox(a)
        val boxB = layout.getBoundingBox(b)
        assertTrue(
            "'$text': '${text[a]}'@$a $boxA must render left of '${text[b]}'@$b $boxB",
            boxA.right <= boxB.left + 0.5f,
        )
    }

    @Test
    fun englishCopy_readsLeftToRight_underArabicLayout() {
        var direction: LayoutDirection? = null
        val layouts = mutableMapOf<String, TextLayoutResult>()
        rule.setContent {
            MilestoneTheme {
                direction = LocalLayoutDirection.current
                Column(Modifier.width(360.dp)) {
                    // The plan target and the coach note, in the app's own type scale.
                    Text(plan, style = Type.Body, onTextLayout = { layouts["plan"] = it })
                    Text(note, style = Type.Body, onTextLayout = { layouts["note"] = it })
                    // A neutral-only figure (no letter to take a direction from).
                    Text(delta, style = Type.StatValue, onTextLayout = { layouts["delta"] = it })
                    // A tabular caption (the FieldLabel hint style).
                    Text(plan, style = Type.Caption.merge(TabularFigures), onTextLayout = { layouts["caption"] = it })
                    // No explicit style: Material's LocalTextStyle from the theme.
                    Text(plan, onTextLayout = { layouts["default"] = it })
                }
            }
        }
        rule.waitForIdle()
        assertEquals("the ar locale must drive an RTL layout", LayoutDirection.Rtl, direction)

        layouts.forEach { (what, layout) -> assertLtr(what, layout) }

        layouts.getValue("plan").let { assertLeftOf(it, 0, plan.indexOf('k')) }
        layouts.getValue("caption").let { assertLeftOf(it, 2, plan.indexOf('k')) }
        layouts.getValue("default").let { assertLeftOf(it, 0, plan.indexOf('k')) }
        layouts.getValue("delta").let { assertLeftOf(it, 0, 1) }
        layouts.getValue("note").let { l ->
            val last = note.lastIndex
            // The sentence's final '.' follows "time", on the same line.
            assertEquals(l.getLineForOffset(last - 1), l.getLineForOffset(last))
            assertLeftOf(l, last - 1, last)
            // The mid-sentence '.' follows "best".
            val stop = note.indexOf(". You")
            assertEquals(l.getLineForOffset(stop - 1), l.getLineForOffset(stop))
            assertLeftOf(l, stop - 1, stop)
        }
    }

    /** Canvas-drawn labels (charts, calculator bars) measure with the type scale
     *  directly, outside any Material Text; they must resolve LTR too. */
    @Test
    fun typeScale_measuredUnderRtl_resolvesLtr() {
        val styles: Map<String, TextStyle> = mapOf(
            "Display" to Type.Display, "Title" to Type.Title, "BannerTitle" to Type.BannerTitle,
            "Countdown" to Type.Countdown, "KeypadKey" to Type.KeypadKey, "StatValue" to Type.StatValue,
            "CardTitle" to Type.CardTitle, "Section" to Type.Section, "Body" to Type.Body,
            "Caption" to Type.Caption, "Chip" to Type.Chip, "Overline" to Type.Overline,
            "TabularFigures" to TabularFigures,
        )
        val layouts = mutableMapOf<String, TextLayoutResult>()
        rule.setContent {
            val measurer = rememberTextMeasurer()
            styles.forEach { (name, style) ->
                layouts[name] = measurer.measure(plan, style, layoutDirection = LayoutDirection.Rtl)
            }
        }
        rule.waitForIdle()
        assertEquals(styles.keys, layouts.keys)
        layouts.forEach { (name, layout) ->
            assertLtr("Type.$name", layout)
            assertLeftOf(layout, 0, plan.indexOf('k'))
        }
    }

    /** Text direction follows content, but the layout still mirrors: a Row's
     *  first child sits at the right edge, its last at the left. */
    @Test
    fun layout_stillMirrors_underArabic() {
        rule.setContent {
            MilestoneTheme {
                Row(Modifier.width(300.dp)) {
                    Text("Distance", style = Type.Body, modifier = Modifier.testTag("first"))
                    Spacer(Modifier.weight(1f))
                    Text(plan, style = Type.Body, modifier = Modifier.testTag("last"))
                }
            }
        }
        val first = rule.onNodeWithTag("first").fetchSemanticsNode().boundsInRoot
        val last = rule.onNodeWithTag("last").fetchSemanticsNode().boundsInRoot
        val right = with(rule.density) { 300.dp.toPx() }
        assertTrue("RTL: first child $first must end at the right edge (${right}px)", first.right >= right - 1f)
        assertTrue("RTL: last child $last must start at the left edge", last.left <= 1f)
    }

    /** A chart canvas draws oldest-left in every direction, so its date ticks stay
     *  in canvas order: "Today" under the newest (rightmost) bar. */
    @Test
    fun chartAxisTicks_followTheCanvas_underArabic() {
        rule.setContent {
            MilestoneTheme {
                Box(Modifier.width(300.dp)) {
                    RunDistanceBars(emptyMap(), todayLocalDay = 20_000L, unit = DistanceUnit.Km, days = 14, chartHeight = 96.dp)
                }
            }
        }
        val today = rule.onNodeWithText(rule.activity.getString(R.string.main_today_axis)).fetchSemanticsNode().boundsInRoot
        val right = with(rule.density) { 300.dp.toPx() }
        assertTrue("RTL: \"Today\" $today must end at the right edge (${right}px), under today's bar", today.right >= right - 1f)
    }

    /** The row chevron points to the end edge (left under RTL), and down when expanded. */
    @Test
    fun rowChevron_pointsToTheEndEdge_andDownWhenExpanded_underArabic() {
        rule.setContent {
            MilestoneTheme {
                Column {
                    for (expanded in listOf(false, true)) {
                        Box(Modifier.testTag("chevron-$expanded").background(Color.White)) {
                            RowChevron(expanded, tint = Color.Black)
                        }
                    }
                }
            }
        }
        fun ink(expanded: Boolean): Pair<Float, Float> {
            val px = rule.onNodeWithTag("chevron-$expanded").captureToImage().toPixelMap()
            // Mean ink x along the middle row, and mean ink y down the middle column,
            // as fractions of the glyph box: the tip is the only ink on each.
            val row = (0 until px.width).filter { px[it, px.height / 2].red < 0.5f }
            val col = (0 until px.height).filter { px[px.width / 2, it].red < 0.5f }
            return (row.average() / px.width).toFloat() to (col.average() / px.height).toFloat()
        }
        val (collapsedX, _) = ink(expanded = false)
        assertTrue("RTL: a collapsed chevron points left, tip at x=$collapsedX of the box", collapsedX < 0.5f)
        val (_, expandedY) = ink(expanded = true)
        assertTrue("RTL: an expanded chevron points down, tip at y=$expandedY of the box", expandedY > 0.5f)
    }

    private fun footer(direction: LayoutDirection): Pair<androidx.compose.ui.geometry.Rect, androidx.compose.ui.geometry.Rect> {
        rule.setContent {
            MilestoneTheme {
                CompositionLocalProvider(LocalLayoutDirection provides direction) {
                    Box(Modifier.width(360.dp)) {
                        StatusCardFooter(primaryLabel = "Resolve", onPrimary = {}, secondaryLabel = "Remove", onSecondary = {})
                    }
                }
            }
        }
        val primary = rule.onNodeWithText("Resolve").fetchSemanticsNode().boundsInRoot
        val secondary = rule.onNodeWithText("Remove").fetchSemanticsNode().boundsInRoot
        return secondary to primary
    }

    /** A status card footer puts its secondary action at the start edge and the
     *  main one at the end: right and left under RTL. */
    @Test
    fun statusCardFooter_secondaryAtTheStart_underArabic() {
        val (secondary, primary) = footer(LayoutDirection.Rtl)
        assertTrue("RTL: secondary $secondary must sit right of primary $primary", secondary.left >= primary.right)
    }

    @Test
    fun statusCardFooter_secondaryAtTheStart_leftToRight() {
        val (secondary, primary) = footer(LayoutDirection.Ltr)
        assertTrue("LTR: secondary $secondary must sit left of primary $primary", secondary.right <= primary.left)
    }

    /** The trim slider mirrors its thumbs under RTL; the track's kept window sits
     *  between them on screen in both directions. */
    @Test
    fun trimTrack_keptWindowSitsBetweenTheThumbs_inBothDirections() {
        AppSettings.showRouteMaps.value = false
        try {
            val fixes = (0..20).map { GpxFix(48.0, 11.0 + it * 0.0005, 1_700_000_000L + it * 10L, hrBpm = null) }
            val segments = listOf(fixes)
            val route = RunRoute(gpx = null, segments = segments, trimmable = true, track = timedTrackFromSegments(segments))
            val run = RunResultView(distance_km = 0.75, duration_min = 200 / 60.0, entry_id = 1L, observed_at = 1_700_000_200L, has_gpx = true)
            var dir by mutableStateOf(LayoutDirection.Ltr)
            var accent = Color.Unspecified
            rule.setContent {
                MilestoneTheme {
                    accent = Accent
                    CompositionLocalProvider(LocalLayoutDirection provides dir) {
                        TrimRunSheetContent(run, route, onCancel = {}, onConfirm = { _, _ -> })
                    }
                }
            }
            val thumbs = rule.onAllNodes(SemanticsMatcher.keyIsDefined(SemanticsActions.SetProgress))
            val span = thumbs[0].fetchSemanticsNode().config[SemanticsProperties.ProgressBarRangeInfo].range.endInclusive
            // Keep the second half: the start thumb moves to the middle.
            thumbs[0].performSemanticsAction(SemanticsActions.SetProgress) { it(span * 0.5f) }
            rule.waitForIdle()
            fun midwayIsAccent(): Boolean {
                val a = thumbs[0].fetchSemanticsNode().boundsInRoot
                val b = thumbs[1].fetchSemanticsNode().boundsInRoot
                val c = rule.onRoot().captureToImage().toPixelMap()[((a.center.x + b.center.x) / 2f).toInt(), a.center.y.toInt()]
                return abs(c.red - accent.red) < 0.08f && abs(c.green - accent.green) < 0.08f && abs(c.blue - accent.blue) < 0.08f
            }
            assertTrue("LTR: the kept window between the thumbs is Accent", midwayIsAccent())

            dir = LayoutDirection.Rtl
            rule.waitForIdle()
            val a = thumbs[0].fetchSemanticsNode().boundsInRoot
            val b = thumbs[1].fetchSemanticsNode().boundsInRoot
            assertTrue("RTL mirrors the thumbs: the end thumb sits left of the start thumb", b.center.x < a.center.x)
            assertTrue("RTL: the kept window between the thumbs is Accent", midwayIsAccent())
        } finally {
            AppSettings.showRouteMaps.value = true
        }
    }
}
