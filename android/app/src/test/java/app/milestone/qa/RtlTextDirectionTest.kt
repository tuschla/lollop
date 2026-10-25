package app.milestone.qa

import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Text
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.ResolvedTextDirection
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import app.milestone.MilestoneTheme
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
 * B2: in an RTL locale (Arabic) the app keeps its mirrored layout, but its
 * English-only copy must still read left to right. Before the fix every
 * paragraph took the RTL layout direction as its base direction, so neutral
 * characters at the edges were reordered: the plan target "5.5 km" rendered as
 * "km 5.5" and a coach note's final '.' jumped to the start of the line.
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
}
